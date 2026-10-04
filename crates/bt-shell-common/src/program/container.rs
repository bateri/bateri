//! A container's or a cluster's guide bar: where an interactive session of
//! `docker`, `podman` or `kubectl` runs — `container redis:alpine`,
//! `k8s prod-eu · payments  pod/api-7f9c` — read from the command line.
//!
//! **What reaches the bar** is a closed list: the operand a command names
//! (the image of `run`, the container of `exec`, a compose service, a
//! kubectl resource), and for kubectl the `--context` and `--namespace`
//! values. kubectl's context otherwise comes from its kubeconfig — the
//! `current-context:` line alone, read in the background job
//! ([`read_context`]); no other line of the file is kept, so its tokens and
//! certificates never reach anything.
//!
//! **Nothing past an option a table does not know is read**, the database
//! clients' rule: the Go command lines (spf13/pflag) reject an option they
//! do not know, so an unknown one here is only a gap in a table — and its
//! value could be taken for the operand. A container's bar then keeps its
//! title alone; kubectl's keeps what came before the option, and reads no
//! kubeconfig (a later `--context` would name another context).
//!
//! **No guess at kubectl's cluster**: a context name is shown (and marked)
//! only when it names where the session runs — not when `--cluster` or
//! `--server` sends it elsewhere, not from a relative kubeconfig path (it is
//! relative to kubectl's working directory, which is not known here), and
//! the namespace only when given (a context's default namespace is not
//! read).

use std::path::{Path, PathBuf};

use super::options::{Dialect, Opt, Takes, Walk, row, walk, walk_to_operand};
use crate::jobs::ProcArgs;

/// A Kubernetes session ([`kubernetes`]): what its command line says and
/// where its context is read from when it says none.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Kube {
    /// The context `--context` names.
    pub context: Option<String>,
    /// `--namespace`; a context's default namespace is not guessed.
    pub namespace: Option<String>,
    /// What the session runs in: `pod/api-7f9c`, `deploy/web`, `node/n1`.
    pub resource: Option<String>,
    /// The kubeconfig files whose `current-context:` names the context, in
    /// kubectl's order ([`read_context`]); empty when the command line names
    /// it or it cannot be known.
    pub files: Vec<PathBuf>,
}

/// What an option's value is to a container or cluster session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    /// kubectl's `--context`.
    Context,
    /// kubectl's `--namespace`.
    Namespace,
    /// kubectl's `--kubeconfig`.
    Kubeconfig,
    /// kubectl's `--cluster` and `--server`: the session goes to another
    /// cluster than the context's.
    Cluster,
    /// `kubectl debug --copy-to`: the session runs in the copy.
    CopyTo,
    /// podman's `exec --latest`: no container operand.
    Latest,
    /// Anything else — an environment, a user, a token.
    Other,
}

use Role::{Cluster, Context, CopyTo, Kubeconfig, Latest, Namespace, Other};
use Takes::{Attached, Nothing, Value};

/// Where an interactive container session runs: the image a `run` starts,
/// the container an `exec` enters or the compose service. `None` when the
/// command is no such session (`docker build`, `docker logs`); `Some(None)`
/// when it is one but its line does not plainly say where (an option a table
/// does not know, podman's `--latest`). `name` is the process's —
/// `docker-compose` reads compose's own line.
pub(super) fn container(name: &str, args: &[String]) -> Option<Option<String>> {
    if name == "docker-compose" {
        // As docker's plugin its first argument is `compose`.
        let args = match args.first() {
            Some(first) if first == "compose" => args.get(1..).unwrap_or_default(),
            _ => args,
        };
        return compose(args);
    }
    // Past an option the table does not know there is no operand: the
    // command is not known.
    let (_, rest) = walk_to_operand(args, Dialect::Pflag, global_option);
    let (command, rest) = rest?.split_first()?;
    match command.as_str() {
        "run" => Some(operand(rest, run_option)),
        "exec" => Some(operand(rest, exec_option)),
        // The management form: `docker container run|exec`.
        "container" => match rest.split_first() {
            Some((sub, rest)) if sub == "run" => Some(operand(rest, run_option)),
            Some((sub, rest)) if sub == "exec" => Some(operand(rest, exec_option)),
            _ => None,
        },
        "compose" => compose(rest),
        _ => None,
    }
}

/// Compose's line after `compose`: its own options, then `exec` or `run`
/// and the service.
fn compose(args: &[String]) -> Option<Option<String>> {
    let (_, rest) = walk_to_operand(args, Dialect::Pflag, compose_global_option);
    let (command, rest) = rest?.split_first()?;
    match command.as_str() {
        "exec" => Some(operand(rest, compose_exec_option)),
        "run" => Some(operand(rest, compose_run_option)),
        _ => None,
    }
}

/// A docker command's operand after its options — they end at it, the rest
/// is the command run inside. `None` past an option the table does not
/// know or a `--latest` that names no container.
fn operand(args: &[String], lookup: fn(&str) -> Option<(Takes, Role)>) -> Option<String> {
    let (walk, rest) = walk_to_operand(args, Dialect::Pflag, lookup);
    if walk.has(Latest) {
        return None;
    }
    rest?.first().and_then(|name| plain(name))
}

/// A kubectl session: `exec`, `run` or `debug`; `None` for any other
/// command. `record`'s `KUBECONFIG` and `HOME` name its kubeconfig files.
pub(super) fn kubernetes(args: &[String], record: &ProcArgs) -> Option<Kube> {
    let (global, rest) = walk_to_operand(args, Dialect::Pflag, kubectl_global_option);
    let (command, rest) = rest?.split_first()?;
    let lookup: fn(&str) -> Option<(Takes, Role)> = match command.as_str() {
        "exec" => kubectl_exec_option,
        "run" => kubectl_run_option,
        "debug" => kubectl_debug_option,
        _ => return None,
    };
    // Past an option the table does not know nothing is read, but what came
    // before it holds (`--context prod exec --new-flag …`): only the
    // kubeconfig is not read then — a `--context` after the unknown option
    // would name another.
    let walk = walk(rest, Dialect::Pflag, lookup);
    // pflag: the last of a repeated option wins; an empty value is unset.
    let last = |role: Role| -> Option<&str> {
        global
            .options
            .iter()
            .chain(&walk.options)
            .rev()
            .find(|(seen, _)| *seen == role)
            .and_then(|(_, value)| *value)
            .filter(|value| !value.is_empty())
    };
    let elsewhere = last(Cluster).is_some();
    let named = last(Context);
    let resource = match last(CopyTo) {
        Some(copy) => plain(copy).map(|copy| format!("pod/{copy}")),
        None => resource(&walk),
    };
    let files = if named.is_some() || elsewhere || walk.unknown {
        Vec::new()
    } else {
        kubeconfig_files(last(Kubeconfig), record)
    };
    Some(Kube {
        context: named.filter(|_| !elsewhere).and_then(plain),
        namespace: last(Namespace).and_then(plain),
        resource,
        files,
    })
}

/// The resource a kubectl session runs in: its first operand, a bare name a
/// pod's (`api` → `pod/api`).
fn resource(walk: &Walk<'_, Role>) -> Option<String> {
    let name = plain(walk.operands().first()?)?;
    Some(if name.contains('/') {
        name
    } else {
        format!("pod/{name}")
    })
}

/// The files kubectl reads its kubeconfig from: `--kubeconfig`, else
/// `KUBECONFIG`'s list (`:`-separated, empty entries skipped; an empty
/// variable is unset), else `.kube/config` under **the process's own**
/// `HOME` — not the user's home: `HOME=/x kubectl` reads `/x`'s. A relative
/// path stays relative — [`read_context`] gives up at it, as it names a file
/// in kubectl's working directory (so does a kubectl without `HOME`).
fn kubeconfig_files(flag: Option<&str>, record: &ProcArgs) -> Vec<PathBuf> {
    if let Some(path) = flag {
        let path = PathBuf::from(path);
        return if path.is_absolute() {
            vec![path]
        } else {
            Vec::new()
        };
    }
    if let Some(list) = record.var("KUBECONFIG").filter(|list| !list.is_empty()) {
        return list
            .split(':')
            .filter(|entry| !entry.is_empty())
            .map(PathBuf::from)
            .collect();
    }
    let home = record.var("HOME").unwrap_or_default();
    vec![Path::new(home).join(".kube/config")]
}

/// A name as shown: one carrying a control character is no name — it would
/// be drawn as a box, and no one typed it on purpose.
fn plain(value: &str) -> Option<String> {
    (!value.is_empty() && !value.chars().any(char::is_control)).then(|| value.to_owned())
}

/// What a kubeconfig says of its current context ([`current_context`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Current {
    /// `current-context: prod-eu`.
    Set(String),
    /// No such line, or an empty one (`""`, `~`, `null`): the next file
    /// decides.
    Unset,
    /// A value this line reading does not take apart (a block scalar, an
    /// anchor, an escape): the context is not known.
    Unreadable,
}

/// The key of the one line read: at the line's start, the top level.
const CURRENT_CONTEXT: &str = "current-context:";

/// The most of one kubeconfig that is read looking for its line: past it,
/// the context is not known. A kubeconfig is kilobytes, with its certificates
/// inline tens of them.
const KUBECONFIG_MAX: u64 = 4 * 1024 * 1024;

/// `line`'s value if it is the top-level `current-context:` line; `None`
/// for any other line. Anything YAML could read otherwise than as a plain or
/// quoted scalar on this one line is [`Current::Unreadable`]: a wrong name
/// would mark another cluster.
fn context_line(line: &str) -> Option<Current> {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let rest = line.strip_prefix(CURRENT_CONTEXT)?;
    if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
        // `current-context:x` is a plain scalar of its own, not this key.
        return None;
    }
    let value = rest.trim_matches([' ', '\t']);
    let parsed = match value.chars().next() {
        None | Some('#') => return Some(Current::Unset),
        Some('"') => double_quoted(&value[1..]),
        Some('\'') => single_quoted(&value[1..]),
        Some('&' | '*' | '!' | '|' | '>' | '[' | '{' | '%' | '@' | '`' | ']' | '}' | ',') => None,
        // A sequence's or a mapping's indicator, not a scalar.
        Some('-' | '?' | ':') if value[1..].is_empty() || value[1..].starts_with([' ', '\t']) => {
            None
        }
        Some(_) => {
            let plain = value.split(" #").next().unwrap_or(value);
            let plain = plain.split("\t#").next().unwrap_or(plain).trim_end();
            match plain {
                "~" | "null" | "Null" | "NULL" => return Some(Current::Unset),
                plain => Some(plain.to_owned()),
            }
        }
    };
    Some(match parsed {
        Some(name) if name.is_empty() => Current::Unset,
        Some(name) if !name.chars().any(char::is_control) => Current::Set(name),
        _ => Current::Unreadable,
    })
}

/// A double-quoted scalar's text after its opening quote, closed on this
/// line and followed by nothing but a comment; an escape is not decoded
/// (`None`).
fn double_quoted(rest: &str) -> Option<String> {
    let (name, after) = rest.split_once('"')?;
    (!name.contains('\\') && only_comment(after)).then(|| name.to_owned())
}

/// A single-quoted scalar's text after its opening quote (`''` is a quote),
/// closed on this line and followed by nothing but a comment.
fn single_quoted(rest: &str) -> Option<String> {
    let mut name = String::new();
    let mut chars = rest.char_indices();
    while let Some((at, ch)) = chars.next() {
        if ch != '\'' {
            name.push(ch);
            continue;
        }
        if rest[at + 1..].starts_with('\'') {
            name.push('\'');
            chars.next();
            continue;
        }
        return only_comment(&rest[at + 1..]).then_some(name);
    }
    None
}

/// After a closed quote: nothing but blanks, or a comment — a `#` after a
/// blank (YAML's `"a"#b` is no comment).
fn only_comment(after: &str) -> bool {
    let trimmed = after.trim_start_matches([' ', '\t']);
    trimmed.is_empty() || (trimmed.starts_with('#') && trimmed.len() < after.len())
}

/// The context the first of `files` that sets one names — kubectl's merge:
/// a missing file is skipped, an empty value leaves the choice to the next
/// file. `None` when none sets it or one cannot be read (a relative path,
/// a file that is not regular, an unreadable value). A background job's
/// call: it opens files.
pub(super) fn read_context(files: &[PathBuf]) -> Option<String> {
    for path in files {
        if !path.is_absolute() {
            return None;
        }
        match std::fs::metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            // A FIFO would block the open; a directory is no file.
            Ok(meta) if meta.is_file() => {}
            _ => return None,
        }
        match scan(path)? {
            Current::Set(name) => return Some(name),
            Current::Unset => {}
            Current::Unreadable => return None,
        }
    }
    None
}

/// One kubeconfig's current context ([`current_context`]), at most
/// [`KUBECONFIG_MAX`] bytes read. `None` when the file cannot be read or is
/// longer than the bound before its line.
fn scan(path: &Path) -> Option<Current> {
    use std::io::Read as _;
    let file = std::fs::File::open(path).ok()?;
    let mut reader = std::io::BufReader::new(file.take(KUBECONFIG_MAX));
    let current = current_context(&mut reader)?;
    let rest = reader.into_inner();
    // At the bound the rest was not read — unless the file ends right there:
    // no line yet is not "unset".
    if current == Current::Unset
        && rest.limit() == 0
        && rest.into_inner().read(&mut [0_u8]).ok()? > 0
    {
        return None;
    }
    Some(current)
}

/// A kubeconfig's top-level `current-context:` line ([`context_line`]),
/// read line by line: **no other line is kept** — each is looked at and
/// dropped, so a token or a certificate reaches nothing.
/// [`Current::Unset`] when the file never names the key; `None` when the
/// reading fails or the line is not UTF-8.
///
/// **A key named in a form this reading does not take is
/// [`Current::Unreadable`]**, not unset: a JSON kubeconfig (`"current-context":
/// "prod"`), a quoted key, a space before the colon. kubectl reads those, and
/// taking the file as silent would hand the choice to the next file in
/// `KUBECONFIG` — another cluster's name and mark. A comment is no mention.
fn current_context(reader: &mut impl std::io::BufRead) -> Option<Current> {
    let mut line = Vec::new();
    let mut first = true;
    let mut mentioned = false;
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line).ok()? == 0 {
            return Some(if mentioned {
                Current::Unreadable
            } else {
                Current::Unset
            });
        }
        let mut bytes = line.as_slice();
        if std::mem::take(&mut first) {
            bytes = bytes.strip_prefix("\u{feff}".as_bytes()).unwrap_or(bytes);
        }
        if !bytes.starts_with(CURRENT_CONTEXT.as_bytes()) {
            let comment = bytes.trim_ascii_start().starts_with(b"#");
            mentioned |= !comment && contains(bytes, KEY_NAME.as_bytes());
            continue;
        }
        let text = std::str::from_utf8(bytes).ok()?;
        match context_line(text.strip_suffix('\n').unwrap_or(text)) {
            Some(current) => return Some(current),
            None => mentioned = true,
        }
    }
}

/// The key's name, in whatever form a line spells it.
const KEY_NAME: &str = "current-context";

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

// --- tables ---------------------------------------------------------------

/// docker's and podman's own options, before the command (`docker
/// --help`, `podman --help`); a command's table falls back to it, since
/// podman reads them after the command too.
const GLOBAL: &[Opt<Role>] = &[
    ("--config", Value, Other),
    ("-c", Value, Other),
    ("--context", Value, Other),
    ("-D", Nothing, Other),
    ("--debug", Nothing, Other),
    ("-H", Value, Other),
    ("--host", Value, Other),
    ("-l", Value, Other),
    ("--log-level", Value, Other),
    ("--tls", Nothing, Other),
    ("--tlscacert", Value, Other),
    ("--tlscert", Value, Other),
    ("--tlskey", Value, Other),
    ("--tlsverify", Nothing, Other),
    ("--help", Nothing, Other),
    ("-v", Nothing, Other),
    ("--version", Nothing, Other),
    // podman's.
    ("--connection", Value, Other),
    ("--url", Value, Other),
    ("--identity", Value, Other),
    ("-r", Nothing, Other),
    ("--remote", Nothing, Other),
    ("--root", Value, Other),
    ("--runroot", Value, Other),
    ("--storage-driver", Value, Other),
    ("--storage-opt", Value, Other),
    ("--cgroup-manager", Value, Other),
    ("--events-backend", Value, Other),
    ("--network-cmd-path", Value, Other),
    ("--network-config-dir", Value, Other),
    ("--runtime", Value, Other),
    ("--runtime-flag", Value, Other),
    ("--syslog", Nothing, Other),
    ("--tmpdir", Value, Other),
    ("--module", Value, Other),
    ("--ssh", Value, Other),
    ("--out", Value, Other),
    ("--noout", Nothing, Other),
    ("--cdi-spec-dir", Value, Other),
    ("--hooks-dir", Value, Other),
    ("--imagestore", Value, Other),
    ("--registries-conf", Value, Other),
    ("--transient-store", Nothing, Other),
    ("--volumepath", Value, Other),
    ("--db-backend", Value, Other),
    ("--conmon", Value, Other),
];

fn global_option(name: &str) -> Option<(Takes, Role)> {
    row(GLOBAL, name)
}

/// `run`'s options (`docker run --help`, and podman's that docker lacks);
/// the options end at the image.
const RUN: &[Opt<Role>] = &[
    ("--add-host", Value, Other),
    ("--annotation", Value, Other),
    ("-a", Value, Other),
    ("--attach", Value, Other),
    ("--blkio-weight", Value, Other),
    ("--blkio-weight-device", Value, Other),
    ("--cap-add", Value, Other),
    ("--cap-drop", Value, Other),
    ("--cgroup-parent", Value, Other),
    ("--cgroupns", Value, Other),
    ("--cidfile", Value, Other),
    ("--cpu-period", Value, Other),
    ("--cpu-quota", Value, Other),
    ("--cpu-rt-period", Value, Other),
    ("--cpu-rt-runtime", Value, Other),
    ("-c", Value, Other),
    ("--cpu-shares", Value, Other),
    ("--cpus", Value, Other),
    ("--cpuset-cpus", Value, Other),
    ("--cpuset-mems", Value, Other),
    ("-d", Nothing, Other),
    ("--detach", Nothing, Other),
    ("--detach-keys", Value, Other),
    ("--device", Value, Other),
    ("--device-cgroup-rule", Value, Other),
    ("--device-read-bps", Value, Other),
    ("--device-read-iops", Value, Other),
    ("--device-write-bps", Value, Other),
    ("--device-write-iops", Value, Other),
    ("--disable-content-trust", Nothing, Other),
    ("--dns", Value, Other),
    ("--dns-option", Value, Other),
    ("--dns-opt", Value, Other),
    ("--dns-search", Value, Other),
    ("--domainname", Value, Other),
    ("--entrypoint", Value, Other),
    ("-e", Value, Other),
    ("--env", Value, Other),
    ("--env-file", Value, Other),
    ("--expose", Value, Other),
    ("--gpus", Value, Other),
    ("--group-add", Value, Other),
    ("--health-cmd", Value, Other),
    ("--health-interval", Value, Other),
    ("--health-retries", Value, Other),
    ("--health-start-interval", Value, Other),
    ("--health-start-period", Value, Other),
    ("--health-timeout", Value, Other),
    ("--help", Nothing, Other),
    ("-h", Value, Other),
    ("--hostname", Value, Other),
    ("--init", Nothing, Other),
    ("-i", Nothing, Other),
    ("--interactive", Nothing, Other),
    ("--ip", Value, Other),
    ("--ip6", Value, Other),
    ("--ipc", Value, Other),
    ("--isolation", Value, Other),
    ("--kernel-memory", Value, Other),
    ("-l", Value, Other),
    ("--label", Value, Other),
    ("--label-file", Value, Other),
    ("--link", Value, Other),
    ("--link-local-ip", Value, Other),
    ("--log-driver", Value, Other),
    ("--log-opt", Value, Other),
    ("--mac-address", Value, Other),
    ("-m", Value, Other),
    ("--memory", Value, Other),
    ("--memory-reservation", Value, Other),
    ("--memory-swap", Value, Other),
    ("--memory-swappiness", Value, Other),
    ("--mount", Value, Other),
    ("--name", Value, Other),
    ("--network", Value, Other),
    ("--net", Value, Other),
    ("--network-alias", Value, Other),
    ("--net-alias", Value, Other),
    ("--no-healthcheck", Nothing, Other),
    ("--oom-kill-disable", Nothing, Other),
    ("--oom-score-adj", Value, Other),
    ("--pid", Value, Other),
    ("--pids-limit", Value, Other),
    ("--platform", Value, Other),
    ("--privileged", Nothing, Other),
    ("-p", Value, Other),
    ("--publish", Value, Other),
    ("-P", Nothing, Other),
    ("--publish-all", Nothing, Other),
    ("--pull", Value, Other),
    ("-q", Nothing, Other),
    ("--quiet", Nothing, Other),
    ("--read-only", Nothing, Other),
    ("--restart", Value, Other),
    ("--rm", Nothing, Other),
    ("--runtime", Value, Other),
    ("--security-opt", Value, Other),
    ("--shm-size", Value, Other),
    ("--sig-proxy", Nothing, Other),
    ("--stop-signal", Value, Other),
    ("--stop-timeout", Value, Other),
    ("--storage-opt", Value, Other),
    ("--sysctl", Value, Other),
    ("--tmpfs", Value, Other),
    ("-t", Nothing, Other),
    ("--tty", Nothing, Other),
    ("--ulimit", Value, Other),
    ("--use-api-socket", Nothing, Other),
    ("-u", Value, Other),
    ("--user", Value, Other),
    ("--userns", Value, Other),
    ("--uts", Value, Other),
    ("-v", Value, Other),
    ("--volume", Value, Other),
    ("--volume-driver", Value, Other),
    ("--volumes-from", Value, Other),
    ("-w", Value, Other),
    ("--workdir", Value, Other),
    // podman's.
    ("--arch", Value, Other),
    ("--os", Value, Other),
    ("--variant", Value, Other),
    ("--pod", Value, Other),
    ("--pod-id-file", Value, Other),
    ("--secret", Value, Other),
    ("--hostuser", Value, Other),
    ("--chrootdirs", Value, Other),
    ("--cgroup-conf", Value, Other),
    ("--cgroups", Value, Other),
    ("--conmon-pidfile", Value, Other),
    ("--creds", Value, Other),
    ("--decryption-key", Value, Other),
    ("--env-merge", Value, Other),
    ("--env-host", Nothing, Other),
    ("--gidmap", Value, Other),
    ("--uidmap", Value, Other),
    ("--subgidname", Value, Other),
    ("--subuidname", Value, Other),
    ("--group-entry", Value, Other),
    ("--passwd-entry", Value, Other),
    ("--http-proxy", Nothing, Other),
    ("--image-volume", Value, Other),
    ("--init-path", Value, Other),
    ("--no-hosts", Nothing, Other),
    ("--passwd", Nothing, Other),
    ("--personality", Value, Other),
    ("--pidfile", Value, Other),
    ("--preserve-fd", Value, Other),
    ("--preserve-fds", Value, Other),
    ("--rdt-class", Value, Other),
    ("--read-only-tmpfs", Nothing, Other),
    ("--replace", Nothing, Other),
    ("--requires", Value, Other),
    ("--retry", Value, Other),
    ("--retry-delay", Value, Other),
    ("--rootfs", Nothing, Other),
    ("--sdnotify", Value, Other),
    ("--seccomp-policy", Value, Other),
    ("--shm-size-systemd", Value, Other),
    ("--systemd", Value, Other),
    ("--timeout", Value, Other),
    ("--tls-verify", Nothing, Other),
    ("--tz", Value, Other),
    ("--umask", Value, Other),
    ("--unsetenv", Value, Other),
    ("--unsetenv-all", Nothing, Other),
];

fn run_option(name: &str) -> Option<(Takes, Role)> {
    row(RUN, name).or_else(|| global_option(name))
}

/// `exec`'s options (`docker exec --help`, and podman's).
const EXEC: &[Opt<Role>] = &[
    ("-d", Nothing, Other),
    ("--detach", Nothing, Other),
    ("--detach-keys", Value, Other),
    ("-e", Value, Other),
    ("--env", Value, Other),
    ("--env-file", Value, Other),
    ("--help", Nothing, Other),
    ("-i", Nothing, Other),
    ("--interactive", Nothing, Other),
    ("--privileged", Nothing, Other),
    ("-t", Nothing, Other),
    ("--tty", Nothing, Other),
    ("-u", Value, Other),
    ("--user", Value, Other),
    ("-w", Value, Other),
    ("--workdir", Value, Other),
    // podman's.
    ("-l", Nothing, Latest),
    ("--latest", Nothing, Latest),
    ("--preserve-fd", Value, Other),
    ("--preserve-fds", Value, Other),
];

fn exec_option(name: &str) -> Option<(Takes, Role)> {
    row(EXEC, name).or_else(|| global_option(name))
}

/// Compose's own options, before its command (`docker compose --help`).
const COMPOSE: &[Opt<Role>] = &[
    ("--all-resources", Nothing, Other),
    ("--ansi", Value, Other),
    ("--compatibility", Nothing, Other),
    ("--dry-run", Nothing, Other),
    ("--env-file", Value, Other),
    ("-f", Value, Other),
    ("--file", Value, Other),
    ("--parallel", Value, Other),
    ("--profile", Value, Other),
    ("--progress", Value, Other),
    ("--project-directory", Value, Other),
    ("--workdir", Value, Other),
    ("-p", Value, Other),
    ("--project-name", Value, Other),
    ("--verbose", Nothing, Other),
    ("--help", Nothing, Other),
];

fn compose_global_option(name: &str) -> Option<(Takes, Role)> {
    row(COMPOSE, name)
}

/// `compose exec`'s options (`docker compose exec --help`, and its hidden
/// `-i`/`-t`).
const COMPOSE_EXEC: &[Opt<Role>] = &[
    ("-d", Nothing, Other),
    ("--detach", Nothing, Other),
    ("--dry-run", Nothing, Other),
    ("-e", Value, Other),
    ("--env", Value, Other),
    ("--index", Value, Other),
    ("-T", Nothing, Other),
    ("--no-tty", Nothing, Other),
    ("--no-TTY", Nothing, Other),
    ("--privileged", Nothing, Other),
    ("-u", Value, Other),
    ("--user", Value, Other),
    ("-w", Value, Other),
    ("--workdir", Value, Other),
    ("-i", Nothing, Other),
    ("--interactive", Nothing, Other),
    ("-t", Nothing, Other),
    ("--tty", Nothing, Other),
    ("--help", Nothing, Other),
];

fn compose_exec_option(name: &str) -> Option<(Takes, Role)> {
    row(COMPOSE_EXEC, name).or_else(|| compose_global_option(name))
}

/// `compose run`'s options (`docker compose run --help`).
const COMPOSE_RUN: &[Opt<Role>] = &[
    ("--build", Nothing, Other),
    ("--cap-add", Value, Other),
    ("--cap-drop", Value, Other),
    ("-d", Nothing, Other),
    ("--detach", Nothing, Other),
    ("--dry-run", Nothing, Other),
    ("--entrypoint", Value, Other),
    ("-e", Value, Other),
    ("--env", Value, Other),
    ("--env-from-file", Value, Other),
    ("-i", Nothing, Other),
    ("--interactive", Nothing, Other),
    ("-l", Value, Other),
    ("--label", Value, Other),
    ("--name", Value, Other),
    ("-T", Nothing, Other),
    ("--no-TTY", Nothing, Other),
    ("--no-deps", Nothing, Other),
    ("-p", Value, Other),
    ("--publish", Value, Other),
    ("--pull", Value, Other),
    ("-q", Nothing, Other),
    ("--quiet", Nothing, Other),
    ("--quiet-build", Nothing, Other),
    ("--quiet-pull", Nothing, Other),
    ("--remove-orphans", Nothing, Other),
    ("--rm", Nothing, Other),
    ("-P", Nothing, Other),
    ("--service-ports", Nothing, Other),
    ("--use-aliases", Nothing, Other),
    ("-t", Nothing, Other),
    ("--tty", Nothing, Other),
    ("-u", Value, Other),
    ("--user", Value, Other),
    ("-v", Value, Other),
    ("--volume", Value, Other),
    ("-w", Value, Other),
    ("--workdir", Value, Other),
    ("--help", Nothing, Other),
];

fn compose_run_option(name: &str) -> Option<(Takes, Role)> {
    row(COMPOSE_RUN, name).or_else(|| compose_global_option(name))
}

/// kubectl's own options (`kubectl options`), anywhere on its line; the
/// secrets (`--token`, `--password`, the client key) take a value like any
/// other, so none is ever an operand.
const KUBECTL: &[Opt<Role>] = &[
    ("--as", Value, Other),
    ("--as-group", Value, Other),
    ("--as-uid", Value, Other),
    ("--cache-dir", Value, Other),
    ("--certificate-authority", Value, Other),
    ("--client-certificate", Value, Other),
    ("--client-key", Value, Other),
    ("--cluster", Value, Cluster),
    ("--context", Value, Context),
    ("--disable-compression", Nothing, Other),
    ("--insecure-skip-tls-verify", Nothing, Other),
    ("--kubeconfig", Value, Kubeconfig),
    ("--kuberc", Value, Other),
    ("--log-flush-frequency", Value, Other),
    ("--match-server-version", Nothing, Other),
    ("-n", Value, Namespace),
    ("--namespace", Value, Namespace),
    ("--password", Value, Other),
    ("--profile", Value, Other),
    ("--profile-output", Value, Other),
    ("--request-timeout", Value, Other),
    ("-s", Value, Cluster),
    ("--server", Value, Cluster),
    ("--tls-server-name", Value, Other),
    ("--token", Value, Other),
    ("--user", Value, Other),
    ("--username", Value, Other),
    ("-v", Value, Other),
    ("--v", Value, Other),
    ("--vmodule", Value, Other),
    ("--warnings-as-errors", Nothing, Other),
    ("-h", Nothing, Other),
    ("--help", Nothing, Other),
    // klog's, in older releases.
    ("--add_dir_header", Nothing, Other),
    ("--alsologtostderr", Nothing, Other),
    ("--log_backtrace_at", Value, Other),
    ("--log_dir", Value, Other),
    ("--log_file", Value, Other),
    ("--log_file_max_size", Value, Other),
    ("--logtostderr", Nothing, Other),
    ("--one_output", Nothing, Other),
    ("--skip_headers", Nothing, Other),
    ("--skip_log_headers", Nothing, Other),
    ("--stderrthreshold", Value, Other),
];

fn kubectl_global_option(name: &str) -> Option<(Takes, Role)> {
    row(KUBECTL, name)
}

/// `kubectl exec`'s options (`kubectl exec --help`).
const KUBECTL_EXEC: &[Opt<Role>] = &[
    ("-c", Value, Other),
    ("--container", Value, Other),
    ("-f", Value, Other),
    ("--filename", Value, Other),
    ("--pod-running-timeout", Value, Other),
    ("-q", Nothing, Other),
    ("--quiet", Nothing, Other),
    ("-i", Nothing, Other),
    ("--stdin", Nothing, Other),
    ("-t", Nothing, Other),
    ("--tty", Nothing, Other),
];

fn kubectl_exec_option(name: &str) -> Option<(Takes, Role)> {
    row(KUBECTL_EXEC, name).or_else(|| kubectl_global_option(name))
}

/// `kubectl run`'s options (`kubectl run --help`); `--cascade` and
/// `--dry-run` take a value only attached.
const KUBECTL_RUN: &[Opt<Role>] = &[
    ("--allow-missing-template-keys", Nothing, Other),
    ("--annotations", Value, Other),
    ("--attach", Nothing, Other),
    ("--cascade", Attached, Other),
    ("--command", Nothing, Other),
    ("--dry-run", Attached, Other),
    ("--env", Value, Other),
    ("--expose", Nothing, Other),
    ("--field-manager", Value, Other),
    ("-f", Value, Other),
    ("--filename", Value, Other),
    ("--force", Nothing, Other),
    ("--grace-period", Value, Other),
    ("--image", Value, Other),
    ("--image-pull-policy", Value, Other),
    ("-k", Value, Other),
    ("--kustomize", Value, Other),
    ("-l", Value, Other),
    ("--labels", Value, Other),
    ("--leave-stdin-open", Nothing, Other),
    ("-o", Value, Other),
    ("--output", Value, Other),
    ("--override-type", Value, Other),
    ("--overrides", Value, Other),
    ("--pod-running-timeout", Value, Other),
    ("--port", Value, Other),
    ("--privileged", Nothing, Other),
    ("-q", Nothing, Other),
    ("--quiet", Nothing, Other),
    ("-R", Nothing, Other),
    ("--recursive", Nothing, Other),
    ("--restart", Value, Other),
    ("--rm", Nothing, Other),
    ("--save-config", Nothing, Other),
    ("--show-managed-fields", Nothing, Other),
    ("-i", Nothing, Other),
    ("--stdin", Nothing, Other),
    ("--template", Value, Other),
    ("--timeout", Value, Other),
    ("-t", Nothing, Other),
    ("--tty", Nothing, Other),
    ("--wait", Nothing, Other),
];

fn kubectl_run_option(name: &str) -> Option<(Takes, Role)> {
    row(KUBECTL_RUN, name).or_else(|| kubectl_global_option(name))
}

/// `kubectl debug`'s options (`kubectl debug --help`).
const KUBECTL_DEBUG: &[Opt<Role>] = &[
    ("--arguments-only", Nothing, Other),
    ("--attach", Nothing, Other),
    ("-c", Value, Other),
    ("--container", Value, Other),
    ("--copy-to", Value, CopyTo),
    ("--custom", Value, Other),
    ("--env", Value, Other),
    ("-f", Value, Other),
    ("--filename", Value, Other),
    ("--image", Value, Other),
    ("--image-pull-policy", Value, Other),
    ("--keep-annotations", Nothing, Other),
    ("--keep-init-containers", Nothing, Other),
    ("--keep-labels", Nothing, Other),
    ("--keep-liveness", Nothing, Other),
    ("--keep-readiness", Nothing, Other),
    ("--keep-startup", Nothing, Other),
    ("--profile", Value, Other),
    ("-q", Nothing, Other),
    ("--quiet", Nothing, Other),
    ("--replace", Nothing, Other),
    ("--same-node", Nothing, Other),
    ("--set-image", Value, Other),
    ("--share-processes", Nothing, Other),
    ("-i", Nothing, Other),
    ("--stdin", Nothing, Other),
    ("--target", Value, Other),
    ("-t", Nothing, Other),
    ("--tty", Nothing, Other),
];

fn kubectl_debug_option(name: &str) -> Option<(Takes, Role)> {
    row(KUBECTL_DEBUG, name).or_else(|| kubectl_global_option(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &[&str]) -> Vec<String> {
        line.iter().map(|&arg| arg.to_owned()).collect()
    }

    fn record(env: &[(&str, &str)]) -> ProcArgs {
        ProcArgs {
            exec: String::new(),
            args: Vec::new(),
            env: env
                .iter()
                .map(|&(key, value)| (key.to_owned(), value.to_owned()))
                .collect(),
        }
    }

    fn docker(line: &[&str]) -> Option<Option<String>> {
        container("docker", &args(line))
    }

    fn at(target: &str) -> Option<Option<String>> {
        Some(Some(target.to_owned()))
    }

    const HOME: &str = "/Users/me";

    fn kube(line: &[&str]) -> Option<Kube> {
        kubernetes(&args(line), &record(&[("HOME", HOME)]))
    }

    #[test]
    fn docker_run_names_its_image() {
        // Measured: `docker run -it --rm redis:alpine sh`, argv as typed.
        assert_eq!(
            docker(&["run", "-it", "--rm", "redis:alpine", "sh"]),
            at("redis:alpine")
        );
        // Options taking a value: their value is never the image.
        assert_eq!(
            docker(&[
                "run",
                "-e",
                "KEY=v",
                "-u",
                "root",
                "-w",
                "/srv",
                "--env-file",
                ".env",
                "--name",
                "web",
                "-v",
                "/x:/y",
                "-p",
                "8080:80",
                "-it",
                "nginx:1.27",
                "bash",
            ]),
            at("nginx:1.27")
        );
        // Attached, pflag's `=` forms and a flag's written value.
        assert_eq!(
            docker(&[
                "run",
                "-eKEY=v",
                "--name=web",
                "-p=80:80",
                "--rm=false",
                "-it",
                "alpine"
            ]),
            at("alpine")
        );
        // The image's own arguments are the command's: not read.
        assert_eq!(
            docker(&["run", "-it", "alpine", "sh", "-c", "--weird"]),
            at("alpine")
        );
        // `--` ends the options, and the management form.
        assert_eq!(docker(&["run", "-it", "--", "alpine"]), at("alpine"));
        assert_eq!(
            docker(&["container", "run", "-it", "--rm", "debian:12"]),
            at("debian:12")
        );
        // docker's own options before the command.
        assert_eq!(
            docker(&["--context", "remote", "-l", "debug", "run", "-it", "alpine"]),
            at("alpine")
        );
        // podman reads the same line.
        assert_eq!(
            container("podman", &args(&["run", "-it", "--pod", "dev", "fedora"])),
            at("fedora")
        );
    }

    #[test]
    fn docker_exec_names_its_container() {
        // Measured: `docker exec -it -u redis bt-probe sh` and the
        // management form `docker container exec -it bt-probe sh`.
        assert_eq!(
            docker(&["exec", "-it", "-u", "redis", "bt-probe", "sh"]),
            at("bt-probe")
        );
        assert_eq!(
            docker(&["container", "exec", "-it", "bt-probe", "sh"]),
            at("bt-probe")
        );
        assert_eq!(
            docker(&["exec", "-e", "A=1", "-w", "/app", "-it", "web", "bash"]),
            at("web")
        );
        // podman's `--latest` names no container: the operand is the command.
        assert_eq!(
            container("podman", &args(&["exec", "-it", "-l", "sh"])),
            Some(None)
        );
    }

    #[test]
    fn compose_names_its_service() {
        // Measured: `docker compose -p btprobe exec cache sh` is docker
        // running its plugin, `docker-compose compose -p btprobe exec cache
        // sh`; the plugin as a program of its own reads the same line.
        let line = ["compose", "-p", "btprobe", "exec", "cache", "sh"];
        assert_eq!(docker(&line), at("cache"));
        assert_eq!(container("docker-compose", &args(&line)), at("cache"));
        assert_eq!(
            container("docker-compose", &args(&line[1..])),
            at("cache"),
            "standalone"
        );
        assert_eq!(
            docker(&[
                "compose", "-f", "dev.yaml", "run", "--rm", "-p", "80:80", "app", "bash"
            ]),
            at("app")
        );
        assert_eq!(
            docker(&[
                "compose", "exec", "-u", "root", "--index", "2", "db", "psql"
            ]),
            at("db")
        );
    }

    #[test]
    fn other_commands_are_no_session() {
        for line in [
            &["build", "."][..],
            &["logs", "-f", "web"],
            &["login"],
            &["compose", "up"],
            &["container", "ls"],
            &[],
            &["--context"],
        ] {
            assert_eq!(docker(line), None, "{line:?}");
        }
        assert_eq!(container("podman", &args(&["machine", "ssh"])), None);
    }

    #[test]
    fn an_unknown_option_leaves_the_target_unknown() {
        // A table's gap: its value could read as the image.
        assert_eq!(
            docker(&["run", "--some-new-option", "value", "-it", "alpine"]),
            Some(None)
        );
        assert_eq!(docker(&["exec", "--frobnicate", "-it", "web"]), Some(None));
        // Before the command it is not even known to be a session.
        assert_eq!(docker(&["--frobnicate", "run", "-it", "alpine"]), None);
        // A run without an image is no session docker starts.
        assert_eq!(docker(&["run", "-it"]), Some(None));
    }

    #[test]
    fn kubectl_exec_names_its_context_namespace_and_pod() {
        let found = kube(&[
            "--context",
            "prod-eu",
            "exec",
            "-it",
            "-n",
            "payments",
            "api-7f9c",
            "-c",
            "app",
            "--",
            "sh",
        ])
        .expect("kubectl exec");
        assert_eq!(found.context.as_deref(), Some("prod-eu"));
        assert_eq!(found.namespace.as_deref(), Some("payments"));
        assert_eq!(found.resource.as_deref(), Some("pod/api-7f9c"));
        assert!(found.files.is_empty(), "named: nothing to read");
        // pflag forms, options anywhere, a resource with its kind.
        let found = kube(&[
            "exec",
            "deploy/web",
            "-it",
            "--namespace=shop",
            "--context=stage",
        ])
        .expect("kubectl exec");
        assert_eq!(found.resource.as_deref(), Some("deploy/web"));
        assert_eq!(found.namespace.as_deref(), Some("shop"));
        assert_eq!(found.context.as_deref(), Some("stage"));
        assert_eq!(
            kube(&["-n=ops", "exec", "-it", "x"]).and_then(|kube| kube.namespace),
            Some("ops".to_owned())
        );
        // The command after `--` is not the pod; `-f` names it in a file.
        let from_file = kube(&["exec", "-it", "-f", "pod.yaml", "--", "sh"]).expect("exec");
        assert_eq!(from_file.resource, None);
        // The value of an option is never the pod.
        let valued = kube(&[
            "exec", "--token", "abc", "-c", "side", "-it", "api", "--", "sh",
        ])
        .expect("exec");
        assert_eq!(valued.resource.as_deref(), Some("pod/api"));
    }

    #[test]
    fn kubectl_run_and_debug_name_their_pod() {
        let run = kube(&[
            "run",
            "tmp",
            "-it",
            "--rm",
            "--image=busybox",
            "--restart=Never",
            "--",
            "sh",
        ])
        .expect("kubectl run");
        assert_eq!(run.resource.as_deref(), Some("pod/tmp"));
        let debug = kube(&[
            "debug", "-it", "api-7f9c", "--image", "busybox", "--target", "app",
        ])
        .expect("kubectl debug");
        assert_eq!(debug.resource.as_deref(), Some("pod/api-7f9c"));
        let node = kube(&["debug", "node/n1", "-it", "--image=ubuntu"]).expect("debug");
        assert_eq!(node.resource.as_deref(), Some("node/n1"));
        // A copy is where the session runs.
        let copy = kube(&[
            "debug",
            "api",
            "-it",
            "--copy-to=api-debug",
            "--image=busybox",
        ])
        .expect("debug");
        assert_eq!(copy.resource.as_deref(), Some("pod/api-debug"));
        // Other commands are no session.
        for line in [
            &["get", "pods"][..],
            &["logs", "-f", "api"],
            &["edit", "deploy/web"],
            &[],
        ] {
            assert_eq!(kube(line), None, "{line:?}");
        }
    }

    #[test]
    fn kubectls_context_is_read_from_the_files_it_reads() {
        let files = |line: &[&str], env: &[(&str, &str)]| {
            let env: Vec<_> = env.iter().copied().chain([("HOME", HOME)]).collect();
            kubernetes(&args(line), &record(&env))
                .expect("kubectl exec")
                .files
        };
        let line = ["exec", "-it", "api"];
        // The default: `~/.kube/config`.
        assert_eq!(files(&line, &[]), [PathBuf::from("/Users/me/.kube/config")]);
        // `KUBECONFIG`'s list in order, its empty entries skipped.
        assert_eq!(
            files(&line, &[("KUBECONFIG", "/a/one::/b/two")]),
            [PathBuf::from("/a/one"), PathBuf::from("/b/two")]
        );
        // An empty variable is unset.
        assert_eq!(
            files(&line, &[("KUBECONFIG", "")]),
            [PathBuf::from("/Users/me/.kube/config")]
        );
        // `--kubeconfig` beats the variable.
        assert_eq!(
            files(
                &["--kubeconfig", "/c/three", "exec", "-it", "api"],
                &[("KUBECONFIG", "/a/one")]
            ),
            [PathBuf::from("/c/three")]
        );
        // A relative one is kubectl's working directory's: unknowable.
        assert!(files(&["--kubeconfig=k.yaml", "exec", "-it", "api"], &[]).is_empty());
        // The process's own home, not the user's.
        assert_eq!(
            files(&line, &[("HOME", "/root")]),
            [PathBuf::from("/root/.kube/config")]
        );
        // Without one kubectl reads a relative path: unknowable.
        let homeless = kubernetes(&args(&line), &record(&[])).expect("exec");
        assert_eq!(homeless.files, [PathBuf::from(".kube/config")]);
        assert_eq!(read_context(&homeless.files), None);
    }

    #[test]
    fn an_overridden_cluster_names_no_context() {
        // `--cluster` and `--server` send the session elsewhere than the
        // context's cluster: the context's name would be another cluster's.
        for line in [
            &[
                "--context",
                "prod",
                "--cluster",
                "stage",
                "exec",
                "-it",
                "api",
            ][..],
            &["exec", "-it", "api", "--server=https://10.0.0.1:6443"],
            &["-s", "https://10.0.0.1:6443", "exec", "-it", "api"],
        ] {
            let found = kube(line).expect("exec");
            assert_eq!(found.context, None, "{line:?}");
            assert!(found.files.is_empty(), "{line:?}");
            assert_eq!(found.resource.as_deref(), Some("pod/api"), "{line:?}");
        }
    }

    #[test]
    fn an_unknown_kubectl_option_ends_what_is_read() {
        // What came before it holds; the kubeconfig is not read.
        let found = kube(&[
            "--context",
            "prod",
            "-n",
            "ops",
            "exec",
            "-it",
            "api",
            "--frobnicate",
            "x",
            "--",
            "sh",
        ])
        .expect("exec");
        assert_eq!(found.context.as_deref(), Some("prod"));
        assert_eq!(found.namespace.as_deref(), Some("ops"));
        assert_eq!(found.resource.as_deref(), Some("pod/api"));
        // After it nothing: its value could read as the pod, a `--context`
        // past it is not seen — so no kubeconfig is read either.
        let found = kube(&[
            "exec",
            "--frobnicate",
            "x",
            "-it",
            "api",
            "--context",
            "prod",
        ])
        .expect("exec");
        assert_eq!(found, Kube::default());
        // Before the command it is not even known to be a session.
        assert_eq!(kube(&["--frobnicate", "exec", "-it", "api"]), None);
    }

    #[test]
    fn a_control_character_is_no_name() {
        assert_eq!(docker(&["exec", "-it", "a\u{1b}b"]), Some(None));
        let found = kube(&["--context", "p\nq", "exec", "-it", "api"]).expect("exec");
        assert_eq!(found.context, None);
        assert!(found.files.is_empty(), "named, if unshowable");
    }

    #[test]
    fn the_current_context_line_is_read_as_yaml_reads_it() {
        let cases = [
            ("current-context: prod-eu\n", Current::Set("prod-eu".into())),
            (
                "apiVersion: v1\nclusters: []\ncurrent-context: kubernetes-admin@kubernetes\nkind: Config\n",
                Current::Set("kubernetes-admin@kubernetes".into()),
            ),
            (
                "current-context: prod # the live one\n",
                Current::Set("prod".into()),
            ),
            (
                "current-context: \"prod eu\"\n",
                Current::Set("prod eu".into()),
            ),
            (
                "current-context: 'it''s'  # quoted\n",
                Current::Set("it's".into()),
            ),
            (
                "current-context: arn:aws:eks:eu-west-1:123:cluster/prod\r\n",
                Current::Set("arn:aws:eks:eu-west-1:123:cluster/prod".into()),
            ),
            ("\u{feff}current-context: bom\n", Current::Set("bom".into())),
            ("current-context: a#b\n", Current::Set("a#b".into())),
            // Not set: missing, empty, null.
            ("apiVersion: v1\n", Current::Unset),
            ("current-context:\n", Current::Unset),
            ("current-context: \"\"\n", Current::Unset),
            ("current-context: ~\n", Current::Unset),
            ("current-context: null\n", Current::Unset),
            ("current-context: # none\n", Current::Unset),
            // A comment is no mention.
            ("# current-context: commented\n", Current::Unset),
            ("  # current-context: indented comment\n", Current::Unset),
            // Named in a form this reading does not take: not known, so
            // the next file does not decide (JSON, an indented or quoted
            // key, a space before the colon).
            (
                "{\"kind\": \"Config\", \"current-context\": \"prod\"}\n",
                Current::Unreadable,
            ),
            (
                "contexts:\n- context:\n    current-context: nested\n",
                Current::Unreadable,
            ),
            ("\"current-context\": prod\n", Current::Unreadable),
            ("current-context : prod\n", Current::Unreadable),
            ("current-context:x\n", Current::Unreadable),
            // The first line wins.
            (
                "current-context: one\ncurrent-context: two\n",
                Current::Set("one".into()),
            ),
            // Another key that begins the same is passed over.
            (
                "current-context:x\ncurrent-context: real\n",
                Current::Set("real".into()),
            ),
            // What this reading does not take apart.
            ("current-context: |\n  prod\n", Current::Unreadable),
            ("current-context: &a prod\n", Current::Unreadable),
            ("current-context: *a\n", Current::Unreadable),
            ("current-context: \"pr\\u006fd\"\n", Current::Unreadable),
            ("current-context: \"open\n", Current::Unreadable),
            ("current-context: [a]\n", Current::Unreadable),
            ("current-context: 'a' b\n", Current::Unreadable),
            ("current-context: a\u{7}b\n", Current::Unreadable),
            ("current-context: \"a\"#b\n", Current::Unreadable),
            ("current-context: - a\n", Current::Unreadable),
            ("current-context: -a\n", Current::Set("-a".into())),
            (
                "current-context: \"a\" # quoted\n",
                Current::Set("a".into()),
            ),
        ];
        for (text, expected) in cases {
            assert_eq!(
                current_context(&mut text.as_bytes()),
                Some(expected),
                "{text:?}"
            );
        }
        // Not UTF-8 on the line itself: not known.
        assert_eq!(current_context(&mut &b"current-context: \xff\n"[..]), None);
    }

    #[test]
    fn the_first_file_that_sets_a_context_wins() {
        let dir = std::env::temp_dir().join(format!("bt-kube-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let write = |name: &str, text: &str| {
            let path = dir.join(name);
            std::fs::write(&path, text).expect("kubeconfig");
            path
        };
        // A real kubeconfig's shape: the secrets around the line.
        let first = write(
            "first",
            "apiVersion: v1\nclusters:\n- cluster:\n    certificate-authority-data: Q0VSVA==\n    \
             server: https://prod:6443\n  name: prod\nusers:\n- name: admin\n  user:\n    \
             token: hunter2-token\ncurrent-context: \"\"\n",
        );
        let second = write("second", "users: []\ncurrent-context: prod-eu\n");
        let third = write("third", "current-context: other\n");
        let missing = dir.join("missing");
        assert_eq!(
            read_context(&[
                missing.clone(),
                first.clone(),
                second.clone(),
                third.clone()
            ])
            .as_deref(),
            Some("prod-eu"),
            "missing skipped, empty unset, first set wins"
        );
        assert_eq!(
            read_context(&[third.clone(), second.clone()]).as_deref(),
            Some("other")
        );
        assert_eq!(read_context(&[first.clone()]), None);
        assert_eq!(read_context(&[]), None);
        // Unknowable before the answer: a relative path, an unreadable value,
        // a directory.
        assert_eq!(
            read_context(&[PathBuf::from("rel/config"), second.clone()]),
            None
        );
        let block = write("block", "current-context: |\n  x\n");
        assert_eq!(read_context(&[block, second.clone()]), None);
        assert_eq!(read_context(&[dir.clone(), second.clone()]), None);
        // A JSON kubeconfig names its context in a form not read here: the
        // next file does not decide.
        let json = write("json", "{\"current-context\": \"prod\"}\n");
        assert_eq!(read_context(&[json, second.clone()]), None);
        // A file ending right at the bound was read whole; one past it was
        // not.
        let at_bound = dir.join("at-bound");
        std::fs::write(&at_bound, vec![b'#'; KUBECONFIG_MAX as usize]).expect("big");
        assert_eq!(
            read_context(&[at_bound, second.clone()]).as_deref(),
            Some("prod-eu")
        );
        let past = dir.join("past");
        std::fs::write(&past, vec![b'#'; KUBECONFIG_MAX as usize + 1]).expect("big");
        assert_eq!(read_context(&[past, second.clone()]), None);
        // A FIFO is not opened.
        let fifo = dir.join("fifo");
        let path = std::ffi::CString::new(fifo.to_string_lossy().as_bytes()).expect("path");
        // SAFETY: a NUL-terminated path from this frame.
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        assert_eq!(read_context(&[fifo, second]), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
