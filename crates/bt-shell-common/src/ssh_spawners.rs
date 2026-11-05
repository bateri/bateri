//! The one connection: what bateri does on a server goes over the connection kablo keeps to it
//! ([`crate::kablo::served::Served`], [`crate::kablo::service::Backend::command`]), not over an
//! `ssh` of its own. A job that starts `ssh` makes a second connection, a second login and a
//! second place for the person to be asked a password, and it is the one kind of mistake no type
//! prevents: the call is an ordinary `Command::new`. So this test counts where the source starts
//! `ssh` and names the places that may. (What `SshRunner::run` runs is `ssh -G` for the
//! configuration of a target: `ssh_wrap`'s `config` and `key_of`; the runner's own spawn is in
//! [`crate::ssh_route`], a file that may.)
//!
//! What it reads is the source of every crate (without the items under `#[cfg(test)]`): a call
//! `Command::new(..)` whose program is named `ssh` or is not a literal (it may be the person's
//! `ssh`), and a call of [`crate::ssh_route::command`], which is how every other `ssh` of bateri
//! is made.

use std::path::{Path, PathBuf};

/// Where `ssh` may be started, by file and function, and why.
const ALLOWED: [(&str, &str, &str); 9] = [
    (
        "bt-shell-common/src/kablo/ssh.rs",
        "carry",
        "the bridge to a server: `ssh <host> kablo attach`, which the one connection is made of",
    ),
    (
        "bt-shell-common/src/kablo/install.rs",
        "run_command_in",
        "putting kablo and the files of bateri on a server, and the counter, before a connection exists",
    ),
    (
        "bt-shell-common/src/hold.rs",
        "hold_main",
        "the pane that typed `ssh` itself, when kablo cannot be kept on that server: the person's own ssh",
    ),
    (
        "bt-shell-common/src/hold.rs",
        "execute",
        "the same pane takes the process over with the person's own `ssh` (`Action::Exec`)",
    ),
    (
        "bt-shell-common/src/hold.rs",
        "run_program",
        "the program bateri asks the pane's terminal to run: the person's command, whatever it is",
    ),
    (
        "bt-shell-common/src/child.rs",
        "query_bash_version",
        "asks this computer's bash for its version",
    ),
    (
        "bt-shell-common/src/child.rs",
        "query_login_path",
        "asks this computer's login shell for its `PATH`",
    ),
    (
        "bt-shell-common/src/program.rs",
        "run_version",
        "`--version` of a program on this computer",
    ),
    (
        "bt-shell-common/src/kablo/local.rs",
        "bridge_command",
        "kablo's `attach` on this computer: the bridge to the local account, no ssh in it",
    ),
];

/// Files that are the connection's own management: the masters (`ssh -M`, `-O check`, `-O exit`)
/// and what `ssh -G` says of a target.
const ALLOWED_FILES: [&str; 1] = ["bt-shell-common/src/ssh_route.rs"];

/// One place that starts `ssh`: where, in which function, how.
#[derive(Debug, PartialEq, Eq)]
struct Spawner {
    line: usize,
    function: String,
    call: String,
}

/// The names `std::process::Command` goes by in `source`: `Command`, and what it is renamed to.
fn command_names(source: &str) -> Vec<String> {
    let mut names = vec!["Command".to_owned()];
    let mut from = 0;
    while let Some(at) = source[from..].find("Command as ") {
        from += at + "Command as ".len();
        let alias: String = source[from..]
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !alias.is_empty() {
            names.push(alias);
        }
    }
    names
}

/// Whether `program`, the argument of a `Command::new`, may be `ssh`: it says so, or it is not a
/// literal (a path, an argument, a setting: what is in it is not read here).
fn may_be_ssh(program: &str) -> bool {
    let program = program.trim();
    program.contains("ssh") || !program.starts_with('"')
}

/// The `ssh` starts in `source` (all of it: the caller cuts the tests off).
fn spawners(source: &str) -> Vec<Spawner> {
    let mut found = Vec::new();
    // A file that imports kablo's `stream::Command` (the words of a program the server runs) means
    // that one by `Command`: the process is then only spelled `process::Command`.
    let streams = source.contains("stream::Command");
    let mut needles: Vec<(String, bool)> = command_names(source)
        .into_iter()
        .map(|name| {
            if streams && name == "Command" {
                (format!("process::{name}::new("), true)
            } else {
                (format!("{name}::new("), true)
            }
        })
        .collect();
    needles.push(("ssh_route::command(".to_owned(), false));
    for (needle, names_ssh) in needles {
        let needle = needle.as_str();
        let mut from = 0;
        while let Some(at) = source[from..].find(needle) {
            let start = from + at;
            let after = start + needle.len();
            from = after;
            let program = argument(&source[after..]);
            if names_ssh && !may_be_ssh(program) {
                continue;
            }
            found.push(Spawner {
                line: source[..start].matches('\n').count() + 1,
                function: function_of(&source[..start]),
                call: format!("{needle}{program})"),
            });
        }
    }
    found.sort_by_key(|spawner| spawner.line);
    found
}

/// The text up to the parenthesis that closes the one just opened.
fn argument(rest: &str) -> &str {
    let mut depth = 1usize;
    for (at, c) in rest.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return &rest[..at];
                }
            }
            _ => {}
        }
    }
    rest
}

/// The function a point of the source is in: the last one that was begun before it.
fn function_of(before: &str) -> String {
    let mut name = String::new();
    let mut from = 0;
    while let Some(at) = before[from..].find("fn ") {
        let start = from + at;
        from = start + 3;
        let is_word_start = before[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
        if !is_word_start {
            continue;
        }
        let ident: String = before[start + 3..]
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !ident.is_empty() {
            name = ident;
        }
    }
    name
}

/// `source` without the items under `#[cfg(test)]` (a module, a function, an `impl`, a `use`),
/// their lines left empty so that the others keep their numbers. The source is rustfmt's: an item
/// that opens a block ends at the `}` that stands where the attribute does.
fn without_tests(source: &str) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let mut kept = Vec::with_capacity(lines.len());
    let mut at = 0;
    while at < lines.len() {
        let line = lines[at];
        if line.trim() != "#[cfg(test)]" {
            kept.push(line);
            at += 1;
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let mut item = at + 1;
        while item < lines.len() && lines[item].trim_start().starts_with("#[") {
            item += 1;
        }
        let end = item_end(&lines, item, indent);
        let emptied = end.min(lines.len() - 1) + 1 - at;
        kept.extend(std::iter::repeat_n("", emptied));
        at = end + 1;
    }
    kept.join("\n")
}

/// The last line of the item that begins at `lines[item]`, the attribute of which stands at
/// `indent`: where a block it opens is closed (`closes`), or where a statement, a field or a
/// constant ends (`;` or `,` with every bracket it opened closed).
fn item_end(lines: &[&str], item: usize, indent: usize) -> usize {
    let mut depth = 0i32;
    // A line that opens a block: from there on the block ends where the indentation says (the
    // brackets of the code inside it are not counted: a string may hold one).
    let mut block = false;
    for (at, line) in lines.iter().enumerate().skip(item) {
        let brackets = |open: &[char], close: &[char]| {
            line.chars().filter(|c| open.contains(c)).count() as i32
                - line.chars().filter(|c| close.contains(c)).count() as i32
        };
        let trimmed = line.trim_end();
        if !block {
            depth += brackets(&['{', '[', '('], &['}', ']', ')']);
        }
        block = block || trimmed.ends_with('{');
        let closed_here = at > item && closes(line, indent);
        let one_line = !block && depth <= 0 && trimmed.ends_with(['}', ';', ',']);
        if closed_here || one_line {
            return at;
        }
    }
    lines.len().saturating_sub(1)
}

/// Whether `line` closes the block that opened at `indent`.
fn closes(line: &str, indent: usize) -> bool {
    let trimmed = line.trim_start();
    line.len() - trimmed.len() == indent
        && trimmed.starts_with(['}', ']', ')'])
        && trimmed
            .trim_start_matches(['}', ']', ')'])
            .trim_matches([';', ','])
            .is_empty()
}

/// The sources of every crate: each file without its tests, and its path from `crates`.
fn sources() -> Vec<(String, String)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&crates).expect("the crates").flatten() {
        walk(&entry.path().join("src"), &mut files);
    }
    let mut sources = Vec::new();
    for file in files {
        let name = file
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if name == "tests.rs"
            || name == "ssh_spawners.rs"
            || file.components().any(|part| part.as_os_str() == "tests")
        {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let text = without_tests(&text);
        let path = file
            .strip_prefix(&crates)
            .unwrap_or(&file)
            .to_string_lossy()
            .into_owned();
        sources.push((path, text));
    }
    sources
}

#[test]
fn ssh_is_started_only_by_the_connection_and_its_setup() {
    let mut wrong = Vec::new();
    let mut allowed_seen = [false; ALLOWED.len()];
    for (path, text) in sources() {
        for spawner in spawners(&text) {
            let place = ALLOWED
                .iter()
                .position(|(file, function, _)| *file == path && *function == spawner.function);
            if let Some(place) = place {
                allowed_seen[place] = true;
            } else if !ALLOWED_FILES.contains(&path.as_str()) {
                wrong.push(format!(
                    "{path}:{} in fn {}: {}",
                    spawner.line, spawner.function, spawner.call
                ));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "`ssh` is started only for the connection kablo keeps (`carry`), for putting kablo on a \
         server, for the masters and `ssh -G` (`ssh_route`) and for the pane that typed `ssh` \
         itself: any other job runs over that connection (`Served`, `Backend::command`) and must \
         not start an `ssh` of its own. Started here:\n{}",
        wrong.join("\n")
    );
    // A place on the list that no longer starts `ssh` is a place that is not allowed to.
    for (seen, (file, function, why)) in allowed_seen.iter().zip(ALLOWED) {
        assert!(
            seen,
            "{file} fn {function} ({why}) starts no `ssh` any more: take it off the list"
        );
    }
}

#[test]
fn the_scan_finds_a_spawner_in_the_function_it_is_in() {
    let source = "\
fn alone() {}
fn upload(host: &str) {
    let mut c = std::process::Command::new(\"ssh\");
    c.arg(host);
}
fn tar() { Command::new(\"/usr/bin/tar\"); }
pub(crate) fn bridge(program: &str) {
    let c = crate::ssh_route::command(program);
    let d = Command::new(&settings.ssh);
}
";
    let found = spawners(source);
    let seen: Vec<(usize, &str, &str)> = found
        .iter()
        .map(|spawner| {
            (
                spawner.line,
                spawner.function.as_str(),
                spawner.call.as_str(),
            )
        })
        .collect();
    assert_eq!(
        seen,
        [
            (3, "upload", "Command::new(\"ssh\")"),
            (8, "bridge", "ssh_route::command(program)"),
            (9, "bridge", "Command::new(&settings.ssh)"),
        ]
    );
}

#[test]
fn the_scan_reads_what_follows_an_item_under_cfg_test() {
    let source = "\
fn a() {}

#[cfg(test)]
use std::io;

#[cfg(test)]
mod fake {
    fn x() { Command::new(\"ssh\"); }
}

fn b() {
    Command::new(\"ssh\");
}

struct Held {
    #[cfg(test)]
    spy: Option<Box<dyn Fn() + Send>>,
    kept: u8,
}

#[cfg(test)]
const MODES: u8 = 1
    | 2;

fn c() {
    #[cfg(test)]
    if let Some(hook) = spy() {
        hook();
    }
    Command::new(\"ssh\");
}

#[cfg(test)]
mod tests {
    fn y() { Command::new(\"ssh\"); }
}
";
    let found = spawners(&without_tests(source));
    let seen: Vec<(usize, &str)> = found
        .iter()
        .map(|spawner| (spawner.line, spawner.function.as_str()))
        .collect();
    assert_eq!(seen, [(12, "b"), (30, "c")]);
}

#[test]
fn a_renamed_command_and_a_program_that_is_not_a_literal_are_seen() {
    let source = "\
use std::process::Command as Process;
fn up(program: &str) {
    Process::new(\"ssh\");
    Process::new(program);
    Process::new(\"/usr/bin/tar\");
}
";
    let found = spawners(source);
    let seen: Vec<(usize, &str)> = found
        .iter()
        .map(|spawner| (spawner.line, spawner.function.as_str()))
        .collect();
    assert_eq!(seen, [(3, "up"), (4, "up")]);
}
