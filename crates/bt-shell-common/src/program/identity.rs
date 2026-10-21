//! Which program a pane runs, as a tab's icon needs to know it.
//!
//! The question is not the guide bar's ([`super::find`]: does the program read
//! the keyboard itself, and what does its bar say). A coding agent is a program
//! the bar deliberately does not know — `node …/claude` is no REPL — and the
//! icon exists to name exactly that, together with the editor, the monitor, the
//! build and the other things a pane spends its time in. One table of names
//! ([`ROWS`]) and one decision over three sources, none of which ever reads a
//! title (a program sets its own):
//!
//! 1. **The program's own name** ([`from_app`]): the `app=` of `OSC 7501`, which
//!    the ledger keeps for this lookup alone. Case does not matter.
//! 2. **The foreground group** ([`find`], deciding in [`from_group`]): the
//!    processes' names, and for an interpreter the script it runs — `node
//!    …/bin/claude`, npm's `…/node_modules/@openai/codex/bin/codex.js`, `python3
//!    …/bin/kimi` — so the same program is known where the kernel names the
//!    process after the interpreter (macOS) and where it names it after the
//!    script (Linux). A language REPL, a database client, a container session
//!    and a root or nested shell are the guide bar's own: [`super::classify`]
//!    says which, nothing is copied.
//! 3. **The command line** ([`from_line`]): the shell's own words for the
//!    command, which is all a remote session offers (the process table there
//!    shows `ssh`) and all there is in the instant a command starts, before the
//!    process has executed. Wrappers are looked through (`VAR=…`, `env`, `sudo`,
//!    `npx`, `bunx`, `uvx`, `exec`, `time`, `nohup`); the rest is read as an argv,
//!    by the same decision as a process's.
//!
//! [`decide`] orders them: the program's own word, then the process table, then
//! the command line, else the shell — or the server, in a remote session.
//!
//! **Only a name is taken from an argv**: the script's an interpreter runs, or
//! the program's own when the kernel names its file instead (a versioned
//! install). The command line of a process carries passwords
//! ([`crate::jobs::ProcArgs`]); here it is compared against the table and
//! dropped — nothing else of it is kept, shown or printed, and no environment
//! variable is asked for.

use super::{
    Family, Member, PythonRuns, ancestors_in, base, classify, command_below, first_positional,
    node_script, python_line, shell,
};
use crate::jobs::{self, ProcArgs, ProcessTable, ShellParent};

/// A coding agent the icon draws by its own mark. [`Self::Other`] is every agent
/// without one (aider, goose, amp) and shares a generic mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Agent {
    ClaudeCode,
    Codex,
    OpenCode,
    Antigravity,
    GeminiCli,
    Pi,
    Herdr,
    CursorCli,
    CopilotCli,
    QwenCode,
    KimiCli,
    Other,
}

impl Agent {
    /// What a sentence for a person calls the agent ("Claude opened 3 terminals"); `None` for
    /// the agents that share the generic mark, which have no name here to say.
    pub fn title(self) -> Option<&'static str> {
        match self {
            Agent::ClaudeCode => Some("Claude"),
            Agent::Codex => Some("Codex"),
            Agent::OpenCode => Some("OpenCode"),
            Agent::Antigravity => Some("Antigravity"),
            Agent::GeminiCli => Some("Gemini"),
            Agent::Pi => Some("Pi"),
            Agent::Herdr => Some("Herdr"),
            Agent::CursorCli => Some("Cursor"),
            Agent::CopilotCli => Some("Copilot"),
            Agent::QwenCode => Some("Qwen"),
            Agent::KimiCli => Some("Kimi"),
            Agent::Other => None,
        }
    }

    /// Whether the agent reads an image off the system clipboard itself when it hears ⌃V. A
    /// terminal's paste is text, so ⌘V on a clipboard that holds only an image sends such an
    /// agent ⌃V in the paste's place ([`crate::keys::image_paste_key`]). Each was read
    /// in its own source: Claude Code's and OpenCode's `ctrl+v` binding, Gemini CLI's
    /// `input.paste` and Codex's fixed `paste_image`. An agent not read hears nothing: to a
    /// program that does not bind it this way ⌃V is a key of its own.
    ///
    /// In a remote session the agent reads the server's clipboard, finds no image and says so —
    /// still more than a paste that does nothing.
    ///
    /// No wildcard: a new agent is a decision.
    pub fn pastes_clipboard_images(self) -> bool {
        match self {
            Agent::ClaudeCode | Agent::Codex | Agent::GeminiCli | Agent::OpenCode => true,
            Agent::Antigravity
            | Agent::Pi
            | Agent::Herdr
            | Agent::CursorCli
            | Agent::CopilotCli
            | Agent::QwenCode
            | Agent::KimiCli
            | Agent::Other => false,
        }
    }
}

/// What a pane runs, as far as an icon tells.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// The pane's shell, a nested one, or nothing the table knows.
    Shell,
    /// `ssh` or `mosh`, or a remote session that runs nothing known: the
    /// server.
    Remote,
    Agent(Agent),
    Editor,
    /// A language's REPL (a row of `INTERPRETERS`).
    Repl,
    /// A database client's session.
    Database,
    /// A container's or a cluster's session.
    Container,
    /// A system monitor.
    Monitor,
    /// A git interface.
    Git,
    /// A build, a test run or a package manager's task.
    Task,
    /// A document or pager.
    Pager,
    /// A file manager.
    Files,
    /// A shell running as root (`sudo -i`, `su`).
    Root,
}

impl Kind {
    /// The kind's word in a handover's pane state (`crate::handover::PaneState`): written by
    /// one bateri, read by the next, so a word never changes once released — a new kind is a
    /// new word. No wildcard: a new kind is a decision.
    pub fn word(self) -> &'static str {
        match self {
            Kind::Shell => "shell",
            Kind::Remote => "remote",
            Kind::Agent(agent) => match agent {
                Agent::ClaudeCode => "claude-code",
                Agent::Codex => "codex",
                Agent::OpenCode => "opencode",
                Agent::Antigravity => "antigravity",
                Agent::GeminiCli => "gemini-cli",
                Agent::Pi => "pi",
                Agent::Herdr => "herdr",
                Agent::CursorCli => "cursor-cli",
                Agent::CopilotCli => "copilot-cli",
                Agent::QwenCode => "qwen-code",
                Agent::KimiCli => "kimi-cli",
                Agent::Other => "agent",
            },
            Kind::Editor => "editor",
            Kind::Repl => "repl",
            Kind::Database => "database",
            Kind::Container => "container",
            Kind::Monitor => "monitor",
            Kind::Git => "git",
            Kind::Task => "task",
            Kind::Pager => "pager",
            Kind::Files => "files",
            Kind::Root => "root",
        }
    }

    /// [`Self::word`]'s inverse; `None` for a word this build does not know (a newer
    /// bateri's kind), which is no kind rather than an error.
    pub fn from_word(word: &str) -> Option<Kind> {
        Some(match word {
            "shell" => Kind::Shell,
            "remote" => Kind::Remote,
            "claude-code" => Kind::Agent(Agent::ClaudeCode),
            "codex" => Kind::Agent(Agent::Codex),
            "opencode" => Kind::Agent(Agent::OpenCode),
            "antigravity" => Kind::Agent(Agent::Antigravity),
            "gemini-cli" => Kind::Agent(Agent::GeminiCli),
            "pi" => Kind::Agent(Agent::Pi),
            "herdr" => Kind::Agent(Agent::Herdr),
            "cursor-cli" => Kind::Agent(Agent::CursorCli),
            "copilot-cli" => Kind::Agent(Agent::CopilotCli),
            "qwen-code" => Kind::Agent(Agent::QwenCode),
            "kimi-cli" => Kind::Agent(Agent::KimiCli),
            "agent" => Kind::Agent(Agent::Other),
            "editor" => Kind::Editor,
            "repl" => Kind::Repl,
            "database" => Kind::Database,
            "container" => Kind::Container,
            "monitor" => Kind::Monitor,
            "git" => Kind::Git,
            "task" => Kind::Task,
            "pager" => Kind::Pager,
            "files" => Kind::Files,
            "root" => Kind::Root,
            _ => return None,
        })
    }
}

/// One kind's names: what a process, a script or a command is called, and what
/// a program reports as its `app`. The single table — a new program is a name
/// in a row.
struct Row {
    kind: Kind,
    /// Process, script, package and command names, lowercase and with `-` for
    /// `_` ([`normalized`]).
    names: &'static [&'static str],
    /// `OSC 7501`'s `app` names, the same way. Only agents report.
    apps: &'static [&'static str],
}

/// Shells are not here, nor are the REPLs, the clients and the container
/// sessions: the guide bar's own tables say which names those are
/// ([`shell::is_shell`], [`classify`]) and the icon reads them. `cursor` is an
/// `app` but no name — the command of that name opens the editor, not the
/// agent.
const ROWS: [Row; 19] = [
    Row {
        kind: Kind::Agent(Agent::ClaudeCode),
        names: &["claude", "claude-code"],
        apps: &["claude", "claude-code"],
    },
    Row {
        kind: Kind::Agent(Agent::Codex),
        names: &["codex"],
        apps: &["codex"],
    },
    Row {
        kind: Kind::Agent(Agent::OpenCode),
        names: &["opencode", "opencode-ai"],
        apps: &["opencode"],
    },
    Row {
        kind: Kind::Agent(Agent::Antigravity),
        // `agy` is the command its installer puts on the path.
        names: &["antigravity", "agy"],
        apps: &["antigravity", "agy"],
    },
    Row {
        kind: Kind::Agent(Agent::GeminiCli),
        names: &["gemini", "gemini-cli"],
        apps: &["gemini", "gemini-cli"],
    },
    Row {
        kind: Kind::Agent(Agent::Pi),
        // Its npm package, the script's when the process keeps node's argv.
        names: &["pi", "pi-coding-agent"],
        apps: &["pi"],
    },
    Row {
        kind: Kind::Agent(Agent::Herdr),
        names: &["herdr"],
        apps: &["herdr"],
    },
    Row {
        kind: Kind::Agent(Agent::CursorCli),
        names: &["cursor-agent"],
        apps: &["cursor", "cursor-agent"],
    },
    Row {
        kind: Kind::Agent(Agent::CopilotCli),
        names: &["copilot"],
        apps: &["copilot", "github-copilot"],
    },
    Row {
        kind: Kind::Agent(Agent::QwenCode),
        names: &["qwen", "qwen-code"],
        apps: &["qwen", "qwen-code"],
    },
    Row {
        kind: Kind::Agent(Agent::KimiCli),
        names: &["kimi", "kimi-cli"],
        apps: &["kimi", "kimi-cli"],
    },
    Row {
        kind: Kind::Agent(Agent::Other),
        names: &["aider", "goose", "amp"],
        apps: &[],
    },
    Row {
        kind: Kind::Remote,
        names: &["ssh", "mosh", "mosh-client"],
        apps: &[],
    },
    Row {
        kind: Kind::Editor,
        names: &[
            "vim", "nvim", "vi", "view", "helix", "hx", "nano", "micro", "emacs", "kak",
        ],
        apps: &[],
    },
    Row {
        kind: Kind::Monitor,
        names: &[
            "htop", "btop", "top", "glances", "bpytop", "btm", "nvtop", "atop",
        ],
        apps: &[],
    },
    Row {
        kind: Kind::Git,
        names: &["lazygit", "tig", "gitui"],
        apps: &[],
    },
    Row {
        kind: Kind::Task,
        names: &[
            "cargo", "make", "gmake", "npm", "pnpm", "yarn", "pytest", "go", "just", "ninja",
            "cmake",
        ],
        apps: &[],
    },
    Row {
        kind: Kind::Pager,
        names: &["man", "less", "bat", "more", "most"],
        apps: &[],
    },
    Row {
        kind: Kind::Files,
        names: &["yazi", "ranger", "nnn", "lf", "mc", "vifm"],
        apps: &[],
    },
];

/// File extensions a script's name loses before the lookup (`codex.js`).
const SCRIPT_EXTENSIONS: [&str; 5] = ["js", "mjs", "cjs", "ts", "py"];

/// A name as the table spells it: ASCII lowercase, `-` for `_` (a Python module
/// `kimi_cli` is the package `kimi-cli`).
fn normalized(name: &str) -> String {
    name.chars()
        .map(|ch| {
            if ch == '_' {
                '-'
            } else {
                ch.to_ascii_lowercase()
            }
        })
        .collect()
}

/// The kind a process, script, package or command name has; a shell by the
/// guide bar's own list.
fn by_name(name: &str) -> Option<Kind> {
    let name = normalized(name);
    if shell::is_shell(&name) {
        return Some(Kind::Shell);
    }
    ROWS.iter()
        .find(|row| row.names.contains(&name.as_str()))
        .map(|row| row.kind)
}

/// The program that reported itself as `app` (`OSC 7501`); `None` for a name
/// the table does not know — the report is then no help, not a reason to guess.
pub fn from_app(app: &str) -> Option<Kind> {
    let app = normalized(app);
    ROWS.iter()
        .find(|row| row.apps.contains(&app.as_str()))
        .map(|row| row.kind)
}

/// The npm package a path is inside (`…/node_modules/@openai/codex/bin/x.js` →
/// `codex`, `…/node_modules/npm/bin/npm-cli.js` → `npm`): a script is often
/// named for its entry (`cli.js`), its package is what the program is called.
fn package_of(path: &str) -> Option<&str> {
    let parts: Vec<&str> = path.split('/').collect();
    let at = parts.iter().rposition(|part| *part == "node_modules")?;
    let first = parts.get(at + 1)?;
    if first.starts_with('@') {
        parts.get(at + 2).copied()
    } else {
        Some(first)
    }
}

/// The stems a bundled program's entry point has, which name nothing: the program is the folder
/// it sits in (Cursor CLI runs `…/cursor-agent/versions/2025.09.18-7ae6800/index.js`).
const ENTRY_STEMS: [&str; 3] = ["index", "cli", "main"];

/// The folder an entry point belongs to: the nearest above it that is not a version (it starts
/// with a digit) or a build's output.
fn folder_of(script: &str) -> Option<&str> {
    script.rsplit('/').skip(1).find(|folder| {
        !folder.starts_with(|c: char| c.is_ascii_digit())
            && !matches!(
                *folder,
                "versions"
                    | "releases"
                    | "current"
                    | "dist"
                    | "bundle"
                    | "build"
                    | "out"
                    | "lib"
                    | "bin"
                    | "src"
            )
    })
}

/// The kind an interpreter's script is: its file name without the extension,
/// else its package, else — for an entry point that names nothing — its folder.
fn of_script(script: &str) -> Option<Kind> {
    let file = base(script);
    let stem = match file.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() && SCRIPT_EXTENSIONS.contains(&extension) => {
            stem
        }
        _ => file,
    };
    by_name(stem)
        .or_else(|| package_of(script).and_then(by_name))
        .or_else(|| {
            ENTRY_STEMS
                .contains(&stem)
                .then(|| folder_of(script))
                .flatten()
                .and_then(by_name)
        })
}

/// One process of the foreground group, as the decision reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Process {
    /// The process's name as the table gives it (the kernel's: the interpreter's
    /// on macOS, the script's on Linux), or the one it was started by when the
    /// kernel's names nothing ([`Process::read`]).
    pub name: String,
    /// The group's member the guide bar's [`classify`] reads: a record only for
    /// the names it can use (interpreters, clients, shells, tools).
    pub member: Member,
}

/// Whether a decision reads a process of this name by its record: an
/// interpreter or a client, a shell, a tool or an elevator.
fn wanted(name: &str) -> bool {
    Family::of_name(name).is_some() || shell::is_candidate(name)
}

impl Process {
    /// A process of the group; the record is kept only when the name is one a
    /// decision reads it for, and its user with it.
    pub fn new(
        pid: u32,
        parent: Option<u32>,
        name: String,
        uid: Option<u32>,
        record: Option<ProcArgs>,
    ) -> Self {
        let candidate = wanted(&name).then(|| super::Candidate {
            name: name.clone(),
            uid,
            record,
        });
        Self {
            name,
            member: Member {
                pid,
                parent,
                candidate,
            },
        }
    }

    /// `pid` as the table has it; `None` when its name is unreadable. The
    /// record is the **argv alone**, no variable asked for.
    ///
    /// A name nothing knows is asked again of the record ([`started_as`]): the
    /// kernel names a process after the file it executed, and a program
    /// installed as a versioned file behind a link (Claude Code's
    /// `~/.local/bin/claude` → `…/claude/versions/2.1.296`) is called by its
    /// version there. One more record read per unknown member and look.
    fn read(pid: u32, table: &impl ProcessTable) -> Option<Self> {
        let mut name = table.name(pid)?;
        if !wanted(&name)
            && by_name(&name).is_none()
            && let Some(started) = table
                .procargs(pid, &[])
                .and_then(|record| started_as(&record))
        {
            name = started;
        }
        let (uid, record) = if wanted(&name) {
            (table.uid(pid), table.procargs(pid, &[]))
        } else {
            (None, None)
        };
        Some(Self::new(pid, table.parent(pid), name, uid, record))
    }

    /// The effective user, when the record was read.
    fn uid(&self) -> Option<u32> {
        self.member.candidate.as_ref().and_then(|c| c.uid)
    }
}

/// The name a process was started by, when it is one the table knows: the
/// path it was executed by, else its argv[0]. Only that name is kept of the
/// record.
fn started_as(record: &ProcArgs) -> Option<String> {
    [
        Some(record.exec.as_str()),
        record.args.first().map(String::as_str),
    ]
    .into_iter()
    .flatten()
    .map(base)
    .find(|name| by_name(name).is_some())
    .map(str::to_owned)
}

/// What a process is by its name, else by the script an interpreter runs.
/// The guide bar's families (a REPL, a client, a container) are not decided
/// here ([`of_family`]).
fn of_process(process: &Process) -> Option<Kind> {
    if let Some(kind) = by_name(&process.name) {
        return Some(kind);
    }
    let record = process.member.candidate.as_ref()?.record.as_ref()?;
    // A program that renames itself (Node's `process.title`: Pi does) writes its name over the
    // interpreter's argv, so the script is gone and argv's first word is all that names it.
    if let Some(kind) = record.args.first().and_then(|first| by_name(base(first))) {
        return Some(kind);
    }
    let args = record.args.get(1..).unwrap_or_default();
    match Family::of_name(&process.name)? {
        Family::Node => node_script(args).and_then(|(script, _)| of_script(script)),
        Family::Python => match python_line(args).1 {
            PythonRuns::Script(script, _) => of_script(script),
            PythonRuns::Module(module, _) => by_name(module.split('.').next().unwrap_or(module)),
            PythonRuns::Nothing | PythonRuns::Code => None,
        },
        Family::Bun if matches!(first_positional(args, &[]), Some("run" | "test")) => {
            Some(Kind::Task)
        }
        _ => None,
    }
}

/// The kind a guide-bar family is. No wildcard: a new family is a decision.
fn of_family(family: Family) -> Kind {
    match family {
        Family::Python | Family::Node | Family::Bun | Family::Deno | Family::Ruby => Kind::Repl,
        Family::Database(_) => Kind::Database,
        Family::Container | Family::Kubernetes => Kind::Container,
        Family::Root => Kind::Root,
        Family::Nested(_) => Kind::Shell,
    }
}

/// The first of `hits` with no other hit above it in the group: a tool that
/// started another (`lazygit` its editor, `man` its pager) is the one named.
fn topmost(members: &[Member], hits: &[(u32, Kind)]) -> Option<Kind> {
    hits.iter()
        .find(|(pid, _)| {
            !ancestors_in(members, *pid)
                .iter()
                .any(|up| hits.iter().any(|(other, _)| other == up))
        })
        .map(|&(_, kind)| kind)
}

/// The foreground group's kind; `None` when nothing in it is known.
///
/// **An agent first**: the programs an agent runs share its group (`git`, a
/// test run, a `python3` it started), and none of them is what the pane is
/// doing. Then the guide bar's own answer ([`classify`] — a root shell, a REPL, a
/// client, a container session, a nested shell), then any other known name, the
/// outermost of them.
pub fn from_group(group: &[Process]) -> Option<Kind> {
    let members: Vec<Member> = group.iter().map(|p| p.member.clone()).collect();
    let named: Vec<(u32, Kind)> = group
        .iter()
        .filter_map(|p| Some((p.member.pid, of_process(p)?)))
        .collect();
    let agents: Vec<(u32, Kind)> = named
        .iter()
        .copied()
        .filter(|(_, kind)| matches!(kind, Kind::Agent(_)))
        .collect();
    if let Some(kind) = topmost(&members, &agents) {
        return Some(kind);
    }
    if let Some(program) = classify(&members, None) {
        return Some(of_family(program.family));
    }
    topmost(&members, &named)
}

/// What the process table says about a pane's foreground ([`find`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seen {
    /// No answer yet — ask again on the next output: the shell itself holds the
    /// terminal (the command has not executed), the group cannot be read, or
    /// sudo runs nothing yet.
    Waiting,
    /// The group is read and nothing in it is known.
    Nothing,
    Kind(Kind),
}

/// The pane's foreground group, read from the process table on the main
/// thread (system calls only, like [`super::find`]) and decided by
/// [`from_group`]. While sudo or `su` holds the terminal the answer is the
/// command it runs, as the guide bar's is: the elevator's own processes are
/// nothing a pane does.
pub fn find(parent: ShellParent, child: u32, table: &impl ProcessTable) -> Seen {
    let Some(shell) = jobs::shell_pid(parent, child, table) else {
        return Seen::Waiting;
    };
    let Some(groups) = table
        .groups(shell)
        .filter(|groups| groups.foreground != 0 && groups.foreground != groups.own)
    else {
        return Seen::Waiting;
    };
    let mut pids = table.members(groups.foreground);
    pids.sort_unstable();
    let mut group: Vec<Process> = pids
        .into_iter()
        .filter_map(|pid| Process::read(pid, table))
        .collect();
    let elevators: Vec<u32> = group
        .iter()
        .filter(|p| shell::ELEVATORS.contains(&p.name.as_str()) && p.uid() == Some(0))
        .map(|p| p.member.pid)
        .collect();
    if !elevators.is_empty() {
        let Some(command) = command_below(&elevators, table) else {
            return Seen::Waiting;
        };
        group = Process::read(command, table).into_iter().collect();
    }
    if group.is_empty() {
        return Seen::Waiting;
    }
    from_group(&group).map_or(Seen::Nothing, Seen::Kind)
}

/// The name of the program in a pane's foreground that a person's answer about it is kept by
/// (`view_ask::key`): a name the program cannot pick for itself, so that another program is
/// not taken for it. The first leaf of the foreground group (the one a close question names
/// first), by the file it executed — `containers`, `claude` —, and for `node` and `python` the
/// **script** they run, base name only (`containers.js`): an interpreter's own name is every
/// script's, and an answer kept by it would be given to the next script the same interpreter
/// runs.
///
/// `None` when no name is known: no job in front, a group that cannot be read, a leaf that is
/// an interpreter running no script (a REPL, `-e`), another interpreter, or a shell — what they
/// run is not read, and an answer about a program that cannot be told from another is not kept.
/// **Only a name is taken from an argv** (the module's rule); nothing else of the record is
/// kept and no variable is asked for.
pub fn kept_name(parent: ShellParent, child: u32, table: &impl ProcessTable) -> Option<String> {
    let shell = jobs::shell_pid(parent, child, table)?;
    let groups = table
        .groups(shell)
        .filter(|groups| groups.foreground != 0 && groups.foreground != groups.own)?;
    let mut pids = table.members(groups.foreground);
    pids.sort_unstable();
    let parents: Vec<Option<u32>> = pids.iter().map(|&pid| table.parent(pid)).collect();
    let leaf = pids
        .iter()
        .copied()
        .find(|pid| !parents.contains(&Some(*pid)))?;
    kept_name_of(&Process::read(leaf, table)?)
}

/// [`kept_name`] of one process.
fn kept_name_of(process: &Process) -> Option<String> {
    fn named(name: &str) -> Option<&str> {
        Some(name).filter(|name| !name.is_empty())
    }
    let record = || process.member.candidate.as_ref()?.record.as_ref();
    let name = match Family::of_name(&process.name) {
        Some(Family::Node) => {
            let args = record()?.args.get(1..).unwrap_or_default();
            node_script(args).and_then(|(path, _)| named(base(path)))
        }
        Some(Family::Python) => {
            let args = record()?.args.get(1..).unwrap_or_default();
            match python_line(args).1 {
                PythonRuns::Script(path, _) => named(base(path)),
                PythonRuns::Module(module, _) => named(module),
                PythonRuns::Nothing | PythonRuns::Code => None,
            }
        }
        // Another interpreter, a client: what it runs is not read here.
        Some(Family::Bun | Family::Deno | Family::Ruby) => None,
        Some(Family::Database(_) | Family::Container | Family::Kubernetes) => named(&process.name),
        Some(Family::Root | Family::Nested(_)) => None,
        None if shell::is_shell(&process.name) => None,
        None => named(&process.name),
    };
    name.map(str::to_owned)
}

/// What a command line is, once its wrappers are looked through.
enum Command {
    /// The command and its arguments.
    Argv(Vec<String>),
    /// A root shell: `sudo -i`, `sudo -s`, `su`.
    Elevated,
}

/// The first command of a line as words: split on blanks, quotes and a
/// backslash honoured, ended at the first unquoted control operator (`|`, `;`,
/// `&`, `<`, `>`, a parenthesis). Not a shell — what a lookup needs of one.
fn words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    let mut chars = line.chars();
    while let Some(ch) = chars.next() {
        match (quote, ch) {
            (Some(open), _) if ch == open => quote = None,
            (Some('"'), '\\') => {
                if let Some(next) = chars.next() {
                    word.push(next);
                }
            }
            (Some(_), _) => word.push(ch),
            (None, '\'' | '"') => {
                quote = Some(ch);
                started = true;
            }
            (None, '\\') => {
                if let Some(next) = chars.next() {
                    word.push(next);
                    started = true;
                }
            }
            (None, _) if ch.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            (None, '|' | ';' | '&' | '<' | '>' | '(' | ')') => break,
            (None, _) => {
                word.push(ch);
                started = true;
            }
        }
    }
    if started {
        words.push(word);
    }
    words
}

/// `NAME=value`: a variable set for the command.
fn is_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

/// The index after the options at `at`; the ones in `valued` take the next
/// word as their value unless it is attached with `=`, `--` ends them.
fn after_options(words: &[String], mut at: usize, valued: &[&str]) -> usize {
    while let Some(word) = words.get(at) {
        if word == "--" {
            return at + 1;
        }
        if !word.starts_with('-') || word == "-" {
            break;
        }
        let name = word.split_once('=').map_or(word.as_str(), |(name, _)| name);
        at += 1 + usize::from(valued.contains(&name) && !word.contains('='));
    }
    at.min(words.len())
}

const ENV_VALUED: [&str; 7] = [
    "-u",
    "--unset",
    "-C",
    "--chdir",
    "-S",
    "--split-string",
    "-P",
];
const TIME_VALUED: [&str; 4] = ["-f", "--format", "-o", "--output"];
/// sudo's options that take a value.
const SUDO_VALUED: [&str; 19] = [
    "-u",
    "--user",
    "-g",
    "--group",
    "-h",
    "--host",
    "-p",
    "--prompt",
    "-C",
    "--close-from",
    "-D",
    "--chdir",
    "-R",
    "--chroot",
    "-r",
    "--role",
    "-t",
    "--type",
    "-T",
];
/// The package runners' options that take a value (`npx -p pkg`, `uvx --from
/// pkg`).
const RUNNER_VALUED: [&str; 7] = [
    "-p",
    "--package",
    "-c",
    "--call",
    "--from",
    "--with",
    "--python",
];

/// A runner's package spec as the command it installs: `@scope/name@1.2` →
/// `name`.
fn package_command(spec: &str) -> &str {
    let unscoped = spec.rsplit('/').next().unwrap_or(spec);
    unscoped.split('@').next().unwrap_or(unscoped)
}

/// Looks through the wrappers in front of a command (`VAR=…`, `env`, `sudo`,
/// `exec`, `time`, `nohup`, and the package runners `npx`, `bunx`, `uvx`).
/// `None` when no command is left (a bare `sudo`, an assignment alone).
fn command_of(words: &[String]) -> Option<Command> {
    let mut at = 0;
    loop {
        let word = words.get(at)?;
        if is_assignment(word) {
            at += 1;
            continue;
        }
        let start = at + 1;
        match base(word) {
            "env" => at = after_options(words, start, &ENV_VALUED),
            "exec" => at = after_options(words, start, &["-a"]),
            "time" => at = after_options(words, start, &TIME_VALUED),
            "nohup" => at = start,
            "sudo" => {
                at = after_options(words, start, &SUDO_VALUED);
                if at >= words.len() {
                    // No command: a root shell if one was asked for.
                    let shell_asked = words[start..at].iter().any(|option| {
                        option == "--login"
                            || option == "--shell"
                            || (option.starts_with('-')
                                && !option.starts_with("--")
                                && option.contains(['i', 's']))
                    });
                    return shell_asked.then_some(Command::Elevated);
                }
            }
            "su" => return Some(Command::Elevated),
            "npx" | "bunx" | "uvx" => {
                let package = after_options(words, start, &RUNNER_VALUED);
                let spec = words.get(package)?;
                let mut argv = vec![package_command(spec).to_owned()];
                argv.extend(words[package + 1..].iter().cloned());
                return Some(Command::Argv(argv));
            }
            _ => return Some(Command::Argv(words[at..].to_vec())),
        }
    }
}

/// The kind the first command of a shell line is; `None` when it is none the
/// table knows (or the line holds no command). The line is read as an argv and
/// decided as a process is ([`from_group`]): `python3` is a REPL, `python3
/// build.py` is not, `node …/claude` is the agent.
pub fn from_line(line: &str) -> Option<Kind> {
    match command_of(&words(line))? {
        Command::Elevated => Some(Kind::Root),
        Command::Argv(argv) => {
            let name = base(argv.first()?).to_owned();
            let record = ProcArgs {
                exec: String::new(),
                args: argv,
                env: Vec::new(),
            };
            from_group(&[Process::new(0, None, name, None, Some(record))])
        }
    }
}

/// The kind the program in front of a terminal on a server is, from the name
/// the server gave it (what kablo says of the terminal, controls removed): the
/// table of a local name and nothing else, so `claude` and `npm` are what they
/// are on this Mac. The name is all there is, no argv: an interpreter (`node`,
/// `python3`) is no one's, and a versioned file name (`2.1.296`, what a server
/// whose kablo is older reports for Claude Code) names nothing — the pane keeps
/// the server's icon. The server's shell is the server, not a shell: `None`
/// too.
pub fn from_server(program: &str) -> Option<Kind> {
    if super::is_shell_name(program) {
        return None;
    }
    by_name(program)
}

/// The pane's kind from what is known, in the order of trust: the program's
/// own word, the server's word of the terminal the pane shows
/// ([`from_server`]), the process table, the command line; else the shell. In a
/// remote session the process table shows only the local `ssh` and the line is
/// the `ssh` itself, so the table is not asked (`group` is ignored) and a
/// command nothing knows leaves the server — which is why the server's word, when
/// there is one, comes before the line.
pub fn decide(
    app: Option<Kind>,
    server: Option<Kind>,
    group: Option<Kind>,
    line: Option<Kind>,
    remote: bool,
) -> Kind {
    let group = if remote { None } else { group };
    app.or(server)
        .or(group)
        .or(line)
        .unwrap_or(if remote { Kind::Remote } else { Kind::Shell })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::tests::{Table, login_shell};

    const AGENTS: [(&str, Agent); 13] = [
        ("claude", Agent::ClaudeCode),
        ("codex", Agent::Codex),
        ("opencode", Agent::OpenCode),
        ("antigravity", Agent::Antigravity),
        ("agy", Agent::Antigravity),
        ("gemini", Agent::GeminiCli),
        ("pi", Agent::Pi),
        ("herdr", Agent::Herdr),
        ("cursor-agent", Agent::CursorCli),
        ("copilot", Agent::CopilotCli),
        ("qwen", Agent::QwenCode),
        ("kimi", Agent::KimiCli),
        ("aider", Agent::Other),
    ];

    fn seen(table: &Table) -> Seen {
        find(ShellParent::Login, 100, table)
    }

    fn kept(table: &Table) -> Option<String> {
        kept_name(ShellParent::Login, 100, table)
    }

    #[test]
    fn two_scripts_of_one_interpreter_are_two_programs_to_keep_an_answer_by() {
        let node = |script: &'static str| {
            login_shell(200).run(200, 101, 200, &["node", script, "--token", "secret"])
        };
        assert_eq!(
            kept(&node("/home/me/tools/containers.js")).as_deref(),
            Some("containers.js")
        );
        assert_eq!(
            kept(&node("/tmp/evil/containers-two.js")).as_deref(),
            Some("containers-two.js")
        );
        // Only a name: not the path, not the arguments.
        let name = kept(&node("/home/me/tools/containers.js")).expect("a name");
        assert!(!name.contains('/') && !name.contains("secret"), "{name}");
        let python = |argv: &[&'static str]| login_shell(200).run(200, 101, 200, argv);
        assert_eq!(
            kept(&python(&["python3", "-u", "tools/db.py", "--dsn", "x"])).as_deref(),
            Some("db.py")
        );
        assert_eq!(
            kept(&python(&["python3", "-m", "tools.db"])).as_deref(),
            Some("tools.db")
        );
        // A program that is not an interpreter is its own name.
        assert_eq!(
            kept(&login_shell(200).run(200, 101, 200, &["/usr/local/bin/containers"])).as_deref(),
            Some("containers")
        );
    }

    #[test]
    fn a_program_that_cannot_be_told_from_another_is_not_named() {
        // An interpreter with no script: a REPL, inline code, or an argv it rewrote.
        for argv in [
            &["node"][..],
            &["node", "-e", "serve()"],
            &["python3"],
            &["python3", "-c", "serve()"],
            &["bun", "run", "dev"],
            &["ruby", "tool.rb"],
        ] {
            assert_eq!(
                kept(&login_shell(200).run(200, 101, 200, argv)),
                None,
                "{argv:?}"
            );
        }
        // A shell, however it was started, runs anything.
        assert_eq!(
            kept(&login_shell(200).run(200, 101, 200, &["bash", "tool.sh"])),
            None
        );
        // Nothing in front, or the group cannot be read.
        assert_eq!(kept(&login_shell(101)), None);
        let unreadable = Table::new(None)
            .with(100, 1, 100, "login")
            .with(101, 100, 101, "zsh");
        assert_eq!(kept(&unreadable), None);
        // The leaf of a wrapper is what runs: `sh` → `node` is the node script's.
        let wrapped = login_shell(200)
            .run(200, 101, 200, &["sh", "-c", "node tool.js"])
            .run(201, 200, 200, &["node", "tool.js"]);
        assert_eq!(kept(&wrapped).as_deref(), Some("tool.js"));
    }

    fn kind(kind: Kind) -> Seen {
        Seen::Kind(kind)
    }

    fn agent(agent: Agent) -> Kind {
        Kind::Agent(agent)
    }

    #[test]
    fn every_kind_has_a_word_of_its_own_that_reads_back() {
        let mut words = std::collections::HashSet::new();
        for kind in crate::icon::KINDS {
            assert_eq!(Kind::from_word(kind.word()), Some(kind), "{kind:?}");
            assert!(words.insert(kind.word()), "{kind:?} shares its word");
        }
        // The words are a handover's format: the released ones stay as they are.
        assert_eq!(Kind::Agent(Agent::ClaudeCode).word(), "claude-code");
        assert_eq!(Kind::Editor.word(), "editor");
        assert_eq!(Kind::from_word("a-kind-from-a-newer-bateri"), None);
        assert_eq!(Kind::from_word(""), None);
    }

    #[test]
    fn every_agent_is_known_by_its_own_name() {
        for (name, which) in AGENTS {
            assert_eq!(by_name(name), Some(agent(which)), "{name}");
            let table = login_shell(200).run(200, 101, 200, &[name]);
            assert_eq!(seen(&table), kind(agent(which)), "{name}");
            let installed: &'static str =
                Box::leak(format!("/Users/me/.local/bin/{name}").into_boxed_str());
            let table = login_shell(200).run(200, 101, 200, &[installed]);
            assert_eq!(seen(&table), kind(agent(which)), "{installed}");
            assert_eq!(from_line(name), Some(agent(which)), "{name} typed");
        }
    }

    #[test]
    fn a_program_the_kernel_names_by_its_versioned_file_is_known_by_the_name_it_was_started_by() {
        // Claude Code's installer links `~/.local/bin/claude` to the versioned file; macOS
        // names the process after the file (measured: `2.1.296`), the path keeps the link.
        let linked = login_shell(200).exec(
            200,
            101,
            200,
            ("2.1.296", "/Users/me/.local/bin/claude"),
            &["claude", "--resume"],
            &[],
        );
        assert_eq!(seen(&linked), kind(agent(Agent::ClaudeCode)));
        // A relative start leaves the resolved file as the path: argv[0] names it.
        let resolved = login_shell(200).exec(
            200,
            101,
            200,
            ("2.1.296", "/Users/me/.local/share/claude/versions/2.1.296"),
            &["claude"],
            &[],
        );
        assert_eq!(seen(&resolved), kind(agent(Agent::ClaudeCode)));
        // Its helpers share the group and name nothing; the agent stays the answer.
        let helpers = linked
            .run(201, 200, 200, &["npm", "exec", "some-server"])
            .run(202, 200, 200, &["caffeinate", "-i"]);
        assert_eq!(seen(&helpers), kind(agent(Agent::ClaudeCode)));
        // A versioned file nothing knows by any name is still nothing.
        let unknown = login_shell(200).exec(
            200,
            101,
            200,
            ("1.0.0", "/opt/tool/versions/1.0.0"),
            &["tool"],
            &[],
        );
        assert_eq!(seen(&unknown), Seen::Nothing);
    }

    #[test]
    fn an_interpreters_script_names_the_agent_where_the_kernel_names_the_interpreter() {
        // macOS: the process is `node`, the script is the second word.
        let node = |argv: &[&'static str]| {
            login_shell(200).exec(200, 101, 200, ("node", "/opt/homebrew/bin/node"), argv, &[])
        };
        assert_eq!(
            seen(&node(&["node", "/opt/homebrew/bin/claude"])),
            kind(agent(Agent::ClaudeCode))
        );
        assert_eq!(
            seen(&node(&[
                "node",
                "/Users/me/.nvm/versions/node/v22.0.0/lib/node_modules/@openai/codex/bin/codex.js",
                "--yolo",
            ])),
            kind(agent(Agent::Codex)),
            "npm's wrapper: the script without its extension"
        );
        assert_eq!(
            seen(&node(&[
                "node",
                "--max-old-space-size=4096",
                "/usr/local/lib/node_modules/@anthropic-ai/claude-code/cli.js",
            ])),
            kind(agent(Agent::ClaudeCode)),
            "a generic entry file: the package names it, node's own options are skipped"
        );
        assert_eq!(
            seen(&node(&[
                "node",
                "/usr/local/lib/node_modules/@google/gemini-cli/dist/index.js"
            ])),
            kind(agent(Agent::GeminiCli))
        );
        // Python agents: a script, and a module.
        let python = |argv: &[&'static str]| {
            login_shell(200).exec(200, 101, 200, ("python3", "/usr/bin/python3"), argv, &[])
        };
        assert_eq!(
            seen(&python(&["python3", "/Users/me/.local/bin/kimi", "--yolo"])),
            kind(agent(Agent::KimiCli))
        );
        assert_eq!(
            seen(&python(&["python3", "-m", "aider", "--model", "x"])),
            kind(agent(Agent::Other))
        );
        assert_eq!(
            seen(&python(&["python3", "-m", "kimi_cli"])),
            kind(agent(Agent::KimiCli)),
            "a module's underscore is the package's dash"
        );
        // Linux: the process is named after the script and the argv is the
        // interpreter's.
        let linux = login_shell(200).exec(
            200,
            101,
            200,
            ("claude", "/usr/bin/node"),
            &["node", "/usr/bin/claude"],
            &[],
        );
        assert_eq!(seen(&linux), kind(agent(Agent::ClaudeCode)));
        // A program that renamed itself: the interpreter's argv is its name alone.
        let retitled = login_shell(200).exec(
            200,
            101,
            200,
            ("node", "/opt/homebrew/bin/node"),
            &["pi", ""],
            &[],
        );
        assert_eq!(seen(&retitled), kind(agent(Agent::Pi)));
        // A bundled entry point names nothing; the folder it is installed in does.
        let bundled = login_shell(200).exec(
            200,
            101,
            200,
            (
                "node",
                "/Users/u/.local/share/cursor-agent/versions/2025.09.18-7ae6800/node",
            ),
            &[
                "node",
                "--use-system-ca",
                "/Users/u/.local/share/cursor-agent/versions/2025.09.18-7ae6800/index.js",
            ],
            &[],
        );
        assert_eq!(seen(&bundled), kind(agent(Agent::CursorCli)));
        let pi = "/Users/u/.pi/agent/install/releases/1.0.4/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js";
        let packaged = login_shell(200).exec(
            200,
            101,
            200,
            ("node", "/usr/local/bin/node"),
            &["node", pi],
            &[],
        );
        assert_eq!(seen(&packaged), kind(agent(Agent::Pi)));
        // A project's own entry point is not taken for a program by its folder's name.
        let project = login_shell(200).exec(
            200,
            101,
            200,
            ("node", "/usr/local/bin/node"),
            &["node", "/Users/u/Projects/shop/dist/index.js"],
            &[],
        );
        assert_eq!(seen(&project), Seen::Nothing);
    }

    #[test]
    fn a_program_that_runs_a_script_of_its_own_is_not_an_agent() {
        let table = login_shell(200).exec(
            200,
            101,
            200,
            ("node", "/opt/homebrew/bin/node"),
            &["node", "server.js"],
            &[],
        );
        assert_eq!(seen(&table), Seen::Nothing);
        let table = login_shell(200).exec(
            200,
            101,
            200,
            ("python3", "/usr/bin/python3"),
            &["python3", "manage.py", "runserver"],
            &[],
        );
        assert_eq!(seen(&table), Seen::Nothing, "no REPL either");
    }

    #[test]
    fn the_tools_are_known_by_name_and_the_outermost_one_wins() {
        let named = [
            ("vim", Kind::Editor),
            ("nvim", Kind::Editor),
            ("htop", Kind::Monitor),
            ("lazygit", Kind::Git),
            ("make", Kind::Task),
            ("cargo", Kind::Task),
            ("man", Kind::Pager),
            ("yazi", Kind::Files),
            ("ssh", Kind::Remote),
            ("mosh-client", Kind::Remote),
        ];
        for (name, expected) in named {
            let table = login_shell(200).run(200, 101, 200, &[name]);
            assert_eq!(seen(&table), kind(expected), "{name}");
        }
        // Python scripts that are tools: macOS names the process `python3`.
        let ranger = login_shell(200).exec(
            200,
            101,
            200,
            ("python3", "/usr/bin/python3"),
            &["python3", "/opt/homebrew/bin/ranger"],
            &[],
        );
        assert_eq!(seen(&ranger), kind(Kind::Files));
        // The tool that opened another is the one named.
        let table =
            login_shell(200)
                .run(200, 101, 200, &["lazygit"])
                .run(201, 200, 200, &["nvim", "file"]);
        assert_eq!(seen(&table), kind(Kind::Git));
        // Whatever the pid order.
        let table =
            login_shell(300)
                .run(300, 101, 300, &["lazygit"])
                .run(250, 300, 300, &["nvim", "file"]);
        assert_eq!(seen(&table), kind(Kind::Git));
        // A pager under a command nothing names.
        let table =
            login_shell(200)
                .run(200, 101, 200, &["git", "log"])
                .run(201, 200, 200, &["less"]);
        assert_eq!(seen(&table), kind(Kind::Pager));
    }

    #[test]
    fn an_agent_is_what_the_pane_does_whatever_it_runs() {
        let table = login_shell(200)
            .exec(
                200,
                101,
                200,
                ("node", "/opt/homebrew/bin/node"),
                &["node", "/opt/homebrew/bin/claude"],
                &[],
            )
            .run(201, 200, 200, &["python3"])
            .run(202, 200, 200, &["make", "test"])
            .run(203, 200, 200, &["bash"]);
        assert_eq!(seen(&table), kind(agent(Agent::ClaudeCode)));
    }

    #[test]
    fn the_guide_bars_families_are_read_not_copied() {
        let repl = |name: &'static str, exec: &'static str, argv: &[&'static str]| {
            seen(&login_shell(200).exec(200, 101, 200, (name, exec), argv, &[]))
        };
        assert_eq!(
            repl("python3", "/usr/bin/python3", &["python3"]),
            kind(Kind::Repl)
        );
        assert_eq!(repl("node", "/usr/bin/node", &["node"]), kind(Kind::Repl));
        assert_eq!(
            repl(
                "psql",
                "/usr/bin/psql",
                &["psql", "-h", "db.prod", "-U", "app", "main"]
            ),
            kind(Kind::Database)
        );
        assert_eq!(
            repl(
                "docker",
                "/usr/bin/docker",
                &["docker", "exec", "-it", "web", "sh"]
            ),
            kind(Kind::Container)
        );
        // A shell started from the prompt.
        assert_eq!(
            seen(&login_shell(200).run(200, 101, 200, &["bash"])),
            kind(Kind::Shell)
        );
    }

    #[test]
    fn sudo_is_looked_through_and_a_root_shell_is_a_kind() {
        // `sudo htop`: sudo holds the terminal, htop is below it.
        let table = login_shell(200)
            .run(200, 101, 200, &["sudo", "htop"])
            .owned_by(200, 0)
            .run(201, 200, 201, &["htop"])
            .owned_by(201, 0);
        assert_eq!(seen(&table), kind(Kind::Monitor));
        // `sudo -i`: its root shell.
        let table = login_shell(200)
            .run(200, 101, 200, &["sudo", "-i"])
            .owned_by(200, 0)
            .with(201, 200, 201, "zsh")
            .owned_by(201, 0);
        assert_eq!(seen(&table), kind(Kind::Root));
        // Still at its password prompt: nothing yet.
        let table = login_shell(200)
            .run(200, 101, 200, &["sudo", "htop"])
            .owned_by(200, 0);
        assert_eq!(seen(&table), Seen::Waiting);
    }

    #[test]
    fn nothing_known_and_not_yet_are_told_apart() {
        // The shell itself holds the terminal: the command has not executed.
        assert_eq!(seen(&login_shell(101)), Seen::Waiting);
        // No shell yet.
        let table = Table::new(Some(200)).with(100, 1, 100, "login");
        assert_eq!(seen(&table), Seen::Waiting);
        let table = login_shell(200).run(200, 101, 200, &["frobnicate", "--now"]);
        assert_eq!(seen(&table), Seen::Nothing);
    }

    #[test]
    fn an_app_name_is_looked_up_without_regard_to_case() {
        for app in ["claude", "claude-code", "Claude-Code", "CLAUDE"] {
            assert_eq!(from_app(app), Some(agent(Agent::ClaudeCode)), "{app}");
        }
        assert_eq!(from_app("codex"), Some(agent(Agent::Codex)));
        assert_eq!(from_app("Cursor"), Some(agent(Agent::CursorCli)));
        assert_eq!(from_app("github-copilot"), Some(agent(Agent::CopilotCli)));
        assert_eq!(from_app("gemini-cli"), Some(agent(Agent::GeminiCli)));
        assert_eq!(from_app("kimi_cli"), Some(agent(Agent::KimiCli)));
        assert_eq!(from_app("cargo"), None, "a tool is no reporting agent");
        assert_eq!(from_app("vim"), None);
        assert_eq!(from_app(""), None);
        assert_eq!(from_app("unheard-of"), None);
    }

    #[test]
    fn wrappers_in_front_of_a_command_are_looked_through() {
        let cases = [
            ("env FOO=1 npx opencode", Some(agent(Agent::OpenCode))),
            ("FOO=1 BAR=2 codex --yolo", Some(agent(Agent::Codex))),
            ("sudo htop", Some(Kind::Monitor)),
            ("sudo -u bob -E vim notes.txt", Some(Kind::Editor)),
            ("sudo -i", Some(Kind::Root)),
            ("sudo -s", Some(Kind::Root)),
            ("su -", Some(Kind::Root)),
            ("time nohup exec cargo build --release", Some(Kind::Task)),
            ("exec -a login zsh", Some(Kind::Shell)),
            ("env -i -u HOME PATH=/bin make", Some(Kind::Task)),
            ("sudo -u", None),
            (
                "npx -y @anthropic-ai/claude-code@latest",
                Some(agent(Agent::ClaudeCode)),
            ),
            ("bunx opencode-ai@latest", Some(agent(Agent::OpenCode))),
            ("uvx --from kimi-cli kimi", Some(agent(Agent::KimiCli))),
            (
                "npx --package=@openai/codex codex",
                Some(agent(Agent::Codex)),
            ),
            ("sudo", None),
            ("sudo -u bob", None),
            ("FOO=1", None),
            ("env", None),
            ("", None),
            ("   ", None),
        ];
        for (line, expected) in cases {
            assert_eq!(from_line(line), expected, "{line:?}");
        }
    }

    #[test]
    fn a_line_is_read_as_the_first_command_with_its_quotes() {
        let cases = [
            ("\"vim\" 'my notes.txt'", Some(Kind::Editor)),
            ("vim a\\ b.txt | cat", Some(Kind::Editor)),
            ("/usr/local/bin/nvim", Some(Kind::Editor)),
            ("./claude", Some(agent(Agent::ClaudeCode))),
            ("htop; claude", Some(Kind::Monitor)),
            ("ls -l | claude", None),
            ("echo 'a | vim'", None),
            ("cd src && vim", None),
            ("git log", None),
            ("ssh -p 22 host", Some(Kind::Remote)),
        ];
        for (line, expected) in cases {
            assert_eq!(from_line(line), expected, "{line:?}");
        }
    }

    #[test]
    fn a_line_is_decided_as_a_process_is() {
        let cases = [
            ("python3", Some(Kind::Repl)),
            ("python3 -i", Some(Kind::Repl)),
            ("python3 build.py", None),
            ("node", Some(Kind::Repl)),
            ("node server.js", None),
            (
                "node /opt/homebrew/bin/claude",
                Some(agent(Agent::ClaudeCode)),
            ),
            (
                "node ~/.nvm/versions/node/v22/lib/node_modules/@openai/codex/bin/codex.js",
                Some(agent(Agent::Codex)),
            ),
            ("python3 -m aider", Some(agent(Agent::Other))),
            ("python3 -m pytest -x", Some(Kind::Task)),
            ("bun run dev", Some(Kind::Task)),
            ("psql -h db.prod -U app main", Some(Kind::Database)),
            ("docker exec -it web sh", Some(Kind::Container)),
            ("docker ps", None),
            ("kubectl exec -it api-7f9c -- sh", Some(Kind::Container)),
            ("bash", Some(Kind::Shell)),
        ];
        for (line, expected) in cases {
            assert_eq!(from_line(line), expected, "{line:?}");
        }
    }

    #[test]
    fn the_sources_are_trusted_in_order_and_a_remote_session_skips_the_table() {
        let (app, group, line) = (
            Some(agent(Agent::Codex)),
            Some(Kind::Editor),
            Some(Kind::Task),
        );
        assert_eq!(decide(app, None, group, line, false), agent(Agent::Codex));
        assert_eq!(decide(None, None, group, line, false), Kind::Editor);
        assert_eq!(decide(None, None, None, line, false), Kind::Task);
        assert_eq!(decide(None, None, None, None, false), Kind::Shell);
        // Remote: the process table says `ssh` and nothing of the far side.
        assert_eq!(
            decide(None, None, Some(Kind::Remote), line, true),
            Kind::Task
        );
        assert_eq!(
            decide(None, None, Some(Kind::Remote), None, true),
            Kind::Remote
        );
        assert_eq!(decide(None, None, None, None, true), Kind::Remote);
        assert_eq!(decide(app, None, None, line, true), agent(Agent::Codex));
    }

    #[test]
    fn the_server_is_heard_over_the_line_that_is_only_the_ssh_and_under_the_programs_own_word() {
        let claude = agent(Agent::ClaudeCode);
        // The line of a pane on a server is the `ssh` itself, which names the server.
        let ssh = from_line("ssh rasp");
        assert_eq!(ssh, Some(Kind::Remote));
        assert_eq!(decide(None, Some(claude), None, ssh, true), claude);
        // A server that says nothing the table knows leaves the server's own icon.
        assert_eq!(decide(None, None, None, ssh, true), Kind::Remote);
        // What the program says of itself is still first.
        assert_eq!(
            decide(Some(agent(Agent::Codex)), Some(claude), None, ssh, true),
            agent(Agent::Codex)
        );
        // And the server is first over a table that is not asked on a server.
        assert_eq!(
            decide(None, Some(Kind::Task), Some(Kind::Editor), None, true),
            Kind::Task
        );
    }

    #[test]
    fn a_program_a_server_names_is_the_icon_the_same_name_has_here() {
        for (name, kind) in [
            ("npm", Some(Kind::Task)),
            ("claude", Some(agent(Agent::ClaudeCode))),
            ("codex", Some(agent(Agent::Codex))),
            ("vim", Some(Kind::Editor)),
            ("htop", Some(Kind::Monitor)),
            ("ssh", Some(Kind::Remote)),
            // The table spells names in lowercase.
            ("Claude", Some(agent(Agent::ClaudeCode))),
        ] {
            assert_eq!(from_server(name), kind, "{name}");
        }
    }

    #[test]
    fn a_name_nothing_here_knows_is_no_icon_and_the_servers_shell_is_the_server() {
        // A version number is what a server whose kablo is older says for Claude Code, and the
        // interpreter of a script is no one's without the argv that names the script.
        for name in [
            "2.1.296",
            "node",
            "python3",
            "bun",
            "sudo",
            "gcc",
            "a.out",
            "",
            "claude\u{1b}x",
        ] {
            assert_eq!(from_server(name), None, "{name:?}");
        }
        // The shell in front of a terminal on a server is a terminal waiting for a command.
        for name in ["zsh", "-zsh", "bash", "-bash", "fish", "-fish", "sh"] {
            assert_eq!(from_server(name), None, "{name}");
        }
    }

    #[test]
    fn the_table_is_consistent() {
        let mut seen_names: Vec<&str> = Vec::new();
        for row in &ROWS {
            for name in row.names.iter().chain(row.apps) {
                assert_eq!(*name, normalized(name), "{name} is spelled as looked up");
            }
            for name in row.names {
                assert!(!shell::is_shell(name), "{name} is a shell, not a row");
                assert!(!seen_names.contains(name), "{name} is in two rows");
                seen_names.push(name);
            }
            if !matches!(row.kind, Kind::Agent(_)) {
                assert!(row.apps.is_empty(), "only agents report an app");
            }
        }
        // Every agent has a row and every row is found by each of its names.
        for (_, which) in AGENTS {
            assert!(ROWS.iter().any(|row| row.kind == agent(which)));
        }
        for row in &ROWS {
            for name in row.names {
                assert_eq!(by_name(name), Some(row.kind), "{name}");
            }
            for app in row.apps {
                assert_eq!(from_app(app), Some(row.kind), "{app}");
            }
        }
    }
}
