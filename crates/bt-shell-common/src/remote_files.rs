//! A remote session's files (045): the rules of previewing (⌘-click) and
//! downloading a file named in an ssh or mosh session's output — the pure
//! half, without I/O.
//!
//! - **The helper session's protocol** ([`helper_script`], [`request_line`],
//!   [`parse_greeting`], [`parse_reply`]): one long-lived `ssh … sh` per pane
//!   answers "does this exist, what is it, how big, how old" line by line
//!   (Karar 10). A request is one line of `sh` the loop `eval`s, every path
//!   quoted with `upload`'s [`sq`] and rejected by its [`is_safe`] (no
//!   backslash, no control character — so no newline can split a request);
//!   the reply names paths by **index** and carries the request's sequence
//!   number, so a late answer cannot be taken for a newer question.
//! - **The download script** ([`download_script`]): `upload`'s mirror, a
//!   `tar c` stream of one item out of its folder (Karar 11).
//! - **The scp path** ([`scp_path`]): "Copy as scp Path" from the session's
//!   ssh argv (R3).
//! - **The download sheet** ([`download_sheet`]): whether a download asks
//!   first (a folder, a clash, no space) and with which buttons (R4).
//! - **The open policy** ([`preview_open`]): a file previews, in its default
//!   application if it is a known document, as plain text otherwise; a folder
//!   does not preview (Karar 3, 4).
//! - **The preview path** ([`preview_path`]): `{dir}/{host}/{remote absolute
//!   path}`, every escape refused.
//! - **The cleanup planner** ([`plan_sweep`]): what the launch, the daily and
//!   the Clear Now sweeps delete and what they move to the download folder
//!   instead (Karar 9).
//!
//! The stream and the two-way queue are `download` and `upload::Transfers`
//! (045 phase-2); the helper session, the preview and the drag come with
//! phase-3…5. The rationale is in `.tasks/045-uzak-dosya-indirme/discussion.md`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use bt_core::{DownloadConflict, PreviewKeep, RemoteKind, RemoteTarget};

use crate::download::Conflict;
use crate::jobs::SSH_VALUED;
use crate::links::Content;
use crate::upload::{NO_DIRECTORY, format_bytes, is_safe, sq};

// ─── helper session protocol ─────────────────────────────────────────────

/// The mark the helper prints first: the login shell's rc file (an `echo` in
/// `.bashrc`) can print before it, and everything before the mark is skipped
/// (`upload`'s `PROBE_MARK` precedent).
const HELPER_MARK: &str = "BT-HELPER";

/// The helper's `sh` script — the remote command of a long-lived `ssh` (wrapped
/// with `upload`'s `remote_command`). It greets with [`HELPER_MARK`] and the
/// remote home directory, then `eval`s one request line at a time
/// ([`request_line`]) until its standard input closes.
///
/// Two requests: `bt_stat` (what each path is: `BT-F` file with size, mtime
/// and `x` bit, `BT-D` folder, `BT-N` nothing) and `bt_count` (`bt_stat`, but a
/// folder answers `BT-C` with its file count and total bytes). The size and the
/// mtime come from GNU `stat -c` or BSD `stat -f`, whichever the server has;
/// `-` if neither answers. Symlinks are followed (`-L`, `[ -d ]`): a link to a
/// folder is a folder. A folder's bytes are its regular files' sizes as `ls -ln`
/// prints them — what `tar` streams, not what `du` allocates.
pub fn helper_script() -> String {
    format!(
        "bt_sm() {{ stat -L -c '%s %Y' -- \"$1\" 2>/dev/null \
         || stat -L -f '%z %m' -- \"$1\" 2>/dev/null || echo '- -'; }}; \
         bt_one() {{ if [ -d \"$2\" ]; then \
         if [ \"$3\" = c ]; then echo \"BT-C $1 $(find \"$2\" -type f -exec ls -ln {{}} + \
         2>/dev/null | awk '{{n++; s+=$5}} END {{printf \"%d %.0f\", n, s}}')\"; \
         else echo \"BT-D $1\"; fi; \
         elif [ -e \"$2\" ]; then x=0; [ -x \"$2\" ] && x=1; \
         echo \"BT-F $1 $(bt_sm \"$2\") $x\"; \
         else echo \"BT-N $1\"; fi; }}; \
         bt_run() {{ m=$1; s=$2; shift 2; i=0; echo \"BT-R $s\"; \
         for p in \"$@\"; do bt_one \"$i\" \"$p\" \"$m\"; i=$((i+1)); done; \
         echo \"BT-END $s\"; }}; \
         bt_stat() {{ bt_run s \"$@\"; }}; bt_count() {{ bt_run c \"$@\"; }}; \
         echo {HELPER_MARK}; printf 'BT-HOME %s\\n' \"$HOME\"; \
         while IFS= read -r bt_line; do eval \"$bt_line\"; done"
    )
}

/// What a request asks of each path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    /// What it is — the ⌘-hover's verification.
    Stat,
    /// What it is and, for a folder, its file count and bytes — the download
    /// sheet's question; a walk of the whole tree, so never on hover.
    Count,
}

/// One request line for the helper (newline included): `bt_stat 7 '/a' '/b'`.
/// `None` if a path cannot safely enter the script ([`is_safe`]: a backslash or
/// a control character) — such a name is not a link, never a mangled one.
pub fn request_line(seq: u64, ask: Ask, paths: &[String]) -> Option<String> {
    let mut line = match ask {
        Ask::Stat => format!("bt_stat {seq}"),
        Ask::Count => format!("bt_count {seq}"),
    };
    for path in paths {
        if !is_safe(path) {
            return None;
        }
        let _ = write!(line, " {}", sq(path));
    }
    line.push('\n');
    Some(line)
}

/// The helper's greeting → the remote home directory (`None` if `$HOME` is
/// empty); `None` as a whole if the mark never came (the script did not run).
pub fn parse_greeting(out: &str) -> Option<Option<String>> {
    let mut lines = out.lines().skip_while(|line| line.trim() != HELPER_MARK);
    lines.next()?;
    let home = lines.next()?.strip_prefix("BT-HOME")?;
    let home = home.strip_prefix(' ').unwrap_or(home);
    Some((!home.is_empty()).then(|| home.to_owned()))
}

/// What a remote path is — one answer of the helper.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteEntry {
    /// A file (anything that exists and is not a folder). The size and the
    /// mtime (Unix seconds) are `None` when the server's `stat` gave neither.
    File {
        size: Option<u64>,
        mtime: Option<u64>,
        executable: bool,
    },
    /// A folder; its contents only for an [`Ask::Count`] request.
    Dir(Option<FolderSize>),
}

/// A folder's regular files: how many and how many bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FolderSize {
    pub files: u64,
    pub bytes: u64,
}

/// Why a reply could not be read. Never a panic: the bytes come from a remote
/// shell and its rc files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplyError {
    /// No `BT-R {seq}` line: the answer to this request has not started.
    NotStarted,
    /// The `BT-END {seq}` line has not come.
    Unterminated,
    /// A line inside the reply that is not one of the protocol's.
    Malformed(String),
    /// A path the reply did not answer.
    Unanswered(usize),
}

/// Whether `line` ends the reply to `seq` — the caller reads lines until it
/// sees this, then hands them to [`parse_reply`].
pub fn ends_reply(line: &str, seq: u64) -> bool {
    line.trim()
        .strip_prefix("BT-END ")
        .and_then(|n| n.parse::<u64>().ok())
        == Some(seq)
}

/// The helper's output → the answer to request `seq` that asked about `asked`
/// paths, by index: `None` for a path that does not exist.
///
/// Lines before `BT-R {seq}` are skipped (rc noise, a late reply to an older
/// request); inside, every line must be the protocol's, every index below
/// `asked` and every path answered.
pub fn parse_reply(
    out: &str,
    seq: u64,
    asked: usize,
) -> Result<Vec<Option<RemoteEntry>>, ReplyError> {
    let begin = format!("BT-R {seq}");
    let mut lines = out.lines().map(str::trim).skip_while(|line| *line != begin);
    if lines.next().is_none() {
        return Err(ReplyError::NotStarted);
    }
    let mut answers: Vec<Option<Option<RemoteEntry>>> = vec![None; asked];
    for line in lines {
        if ends_reply(line, seq) {
            return answers
                .into_iter()
                .enumerate()
                .map(|(index, answer)| answer.ok_or(ReplyError::Unanswered(index)))
                .collect();
        }
        let malformed = || ReplyError::Malformed(line.to_owned());
        let (index, answer) = reply_line(line).ok_or_else(malformed)?;
        let slot = answers.get_mut(index).ok_or_else(malformed)?;
        *slot = Some(answer);
    }
    Err(ReplyError::Unterminated)
}

/// One answer line → its index and what the path is.
fn reply_line(line: &str) -> Option<(usize, Option<RemoteEntry>)> {
    let mut fields = line.split(' ');
    let tag = fields.next()?;
    let index = fields.next()?.parse().ok()?;
    let rest: Vec<&str> = fields.collect();
    let number = |field: &str| -> Option<Option<u64>> {
        if field == "-" {
            Some(None)
        } else {
            field.parse().ok().map(Some)
        }
    };
    let answer = match (tag, rest.as_slice()) {
        ("BT-N", []) => None,
        ("BT-D", []) => Some(RemoteEntry::Dir(None)),
        ("BT-C", [files, bytes]) => Some(RemoteEntry::Dir(Some(FolderSize {
            files: files.parse().ok()?,
            bytes: bytes.parse().ok()?,
        }))),
        ("BT-F", [size, mtime, x]) => Some(RemoteEntry::File {
            size: number(size)?,
            mtime: number(mtime)?,
            executable: match *x {
                "0" => false,
                "1" => true,
                _ => return None,
            },
        }),
        _ => return None,
    };
    Some((index, answer))
}

// ─── download ────────────────────────────────────────────────────────────

/// A remote absolute path → its folder and its name (`/var/log/x` →
/// `/var/log`, `x`; `/x` → `/`, `x`). `None` for a relative path, the root and
/// a name that is `.` or `..` — there is no single item to stream.
pub fn split_remote(path: &str) -> Option<(&str, &str)> {
    if !path.starts_with('/') {
        return None;
    }
    let trimmed = path.trim_end_matches('/');
    let at = trimmed.rfind('/')?;
    let (dir, name) = (&trimmed[..at], &trimmed[at + 1..]);
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    Some((if dir.is_empty() { "/" } else { dir }, name))
}

/// The remote script that streams one item as a tar archive to standard
/// output: change into its folder and `tar c` it as `./name` (a leading `-`
/// cannot read as an option). Exits [`NO_DIRECTORY`] if the folder cannot be
/// entered — `upload`'s code, so one failure text serves both directions. `None`
/// for a path [`split_remote`] refuses or that is not [`is_safe`].
pub fn download_script(path: &str) -> Option<String> {
    let (dir, name) = split_remote(path)?;
    if !is_safe(path) {
        return None;
    }
    Some(format!(
        "cd {} || exit {NO_DIRECTORY}; exec tar -c -f - {}",
        sq(dir),
        sq(&format!("./{name}"))
    ))
}

/// The download's confirmation sheet (R4): the text and the confirm buttons, each
/// with the conflict rule it starts the download under; the caller adds
/// "Cancel" last.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadSheet {
    pub message: String,
    pub informative: String,
    /// `Download`, or `Keep Both` + `Replace` when the sheet asks about a clash.
    pub buttons: Vec<(&'static str, Conflict)>,
    /// `false` when this Mac has not enough free space: every confirm button is
    /// disabled and the text says why.
    pub enabled: bool,
}

/// Whether a download asks first (Karar 5, R4) and how: `Ok(conflict)` starts
/// it without a sheet, `Err(sheet)` asks. A sheet only when it is needed — a
/// **folder** (its file count and size), a **clash** at the destination while
/// `download_conflict` is `ask` (Keep Both / Replace), or **not enough space** on
/// this Mac (disabled, with the reason). A single file with none of those goes
/// at once: downloading to this Mac is not as risky as writing to a server.
///
/// `dest` is the destination folder as the sheet names it; `free` its free
/// bytes (`None` — unknown, never a reason to refuse); `clash` whether the
/// remote name is already taken there.
pub fn download_sheet(
    host: &str,
    name: &str,
    entry: &RemoteEntry,
    dest: &str,
    free: Option<u64>,
    clash: bool,
    setting: DownloadConflict,
) -> Result<Conflict, DownloadSheet> {
    let (folder, bytes) = match entry {
        RemoteEntry::File { size, .. } => (None, *size),
        RemoteEntry::Dir(size) => (Some(size), size.map(|size| size.bytes)),
    };
    let short = free.zip(bytes).filter(|(free, bytes)| bytes > free);
    let ask = clash && setting == DownloadConflict::Ask;
    let conflict = match setting {
        DownloadConflict::Replace => Conflict::Replace,
        DownloadConflict::Ask | DownloadConflict::KeepBoth => Conflict::KeepBoth,
    };
    if folder.is_none() && !ask && short.is_none() {
        return Ok(conflict);
    }
    let quoted = format!("“{name}”");
    let (message, summary) = match folder {
        Some(size) => (
            format!("Download folder {quoted} from {host}?"),
            match size {
                Some(size) => format!(
                    "{} {}, {} → {dest}",
                    size.files,
                    if size.files == 1 { "file" } else { "files" },
                    format_bytes(size.bytes)
                ),
                None => format!("→ {dest}"),
            },
        ),
        None => (
            format!("Download {quoted} from {host}?"),
            match bytes {
                Some(bytes) => format!("{} → {dest}", format_bytes(bytes)),
                None => format!("→ {dest}"),
            },
        ),
    };
    let mut lines = vec![summary];
    if clash {
        lines.push(match setting {
            DownloadConflict::Ask => {
                format!("An item named {quoted} already exists there. Keep both, or replace it?")
            }
            DownloadConflict::KeepBoth => {
                format!("An item named {quoted} already exists there: the new one gets a number.")
            }
            DownloadConflict::Replace => {
                format!("An item named {quoted} already exists there: it will be replaced.")
            }
        });
    }
    if let Some((free, bytes)) = short {
        lines.push(format!(
            "This Mac has {} free, {quoted} needs {}.",
            format_bytes(free),
            format_bytes(bytes)
        ));
    }
    let buttons = if ask {
        vec![
            ("Keep Both", Conflict::KeepBoth),
            ("Replace", Conflict::Replace),
        ]
    } else {
        vec![("Download", conflict)]
    };
    Err(DownloadSheet {
        message,
        informative: lines.join("\n"),
        buttons,
        enabled: short.is_none(),
    })
}

// ─── scp path ────────────────────────────────────────────────────────────

/// "Copy as scp Path" (R3): the remote item as `scp` names it —
/// `-P 2222 deploy@prod:/var/log/x` — from the session's argv.
///
/// The port (`-p N`, `-o Port=N`, `ssh://host:N`) becomes scp's `-P N` and the
/// user (`-l u`, `-o User=u`, `u@host`) the `u@` prefix. **Any other option
/// that takes a value** (`-J`, `-i`, `-F`, another `-o` …) cannot be carried in
/// one word, so the result falls back to the bare `host:/path` — the user's own
/// `~/.ssh/config` is the place for those. Value-less flags are ignored. With
/// mosh there is no ssh argv: the host as typed. The word is single-quoted
/// when it holds a character the shell would read (`upload`'s [`sq`]).
pub fn scp_path(target: &RemoteTarget, path: &str) -> String {
    let bare = |destination: &str| quoted(&format!("{}:{path}", bracketed(destination)));
    if target.kind == RemoteKind::Mosh {
        return bare(&target.host);
    }
    let Some(parsed) = parse_ssh(target) else {
        return bare(&target.host);
    };
    let destination = match &parsed.user {
        Some(user) if !parsed.host.contains('@') => format!("{user}@{}", parsed.host),
        _ => parsed.host.clone(),
    };
    match parsed.port {
        Some(port) if parsed.translatable => format!("-P {port} {}", bare(&destination)),
        _ => bare(&destination),
    }
}

/// An IPv6 literal (`::1`) needs brackets before scp's `:`; `u@::1` keeps its
/// user outside them.
fn bracketed(destination: &str) -> String {
    let (user, host) = match destination.rsplit_once('@') {
        Some((user, host)) => (Some(user), host),
        None => (None, destination),
    };
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    match user {
        Some(user) => format!("{user}@{host}"),
        None => host,
    }
}

/// `word` as is when every character reads literally in a POSIX shell,
/// single-quoted otherwise.
fn quoted(word: &str) -> String {
    let plain = word.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || matches!(c, '/' | '.' | '_' | '-' | ':' | '@' | '+' | ',' | '[' | ']')
    });
    if plain { word.to_owned() } else { sq(word) }
}

/// What [`scp_path`] reads from an ssh argv.
struct SshDestination {
    /// `[user@]host`, scheme and port dropped.
    host: String,
    user: Option<String>,
    port: Option<String>,
    /// No option outside the port and the user took a value.
    translatable: bool,
}

/// Walks the ssh argv the way `upload::ssh_argv` does (`jobs`'s
/// [`SSH_VALUED`] says which flag takes a value; options after the destination
/// count too, the remote command ends the walk).
fn parse_ssh(target: &RemoteTarget) -> Option<SshDestination> {
    let args = target.argv.get(1..)?;
    let mut destination: Option<String> = None;
    let mut user = None;
    let mut port = None;
    let mut translatable = true;
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        index += 1;
        if arg == "--" {
            if destination.is_none() {
                destination = args.get(index).cloned();
            }
            break;
        }
        let Some(cluster) = arg.strip_prefix('-').filter(|rest| !rest.is_empty()) else {
            if destination.is_some() {
                break;
            }
            destination = Some(arg.clone());
            continue;
        };
        for (at, flag) in cluster.char_indices() {
            if !SSH_VALUED.contains(flag) {
                continue;
            }
            let attached = &cluster[at + flag.len_utf8()..];
            let value = if attached.is_empty() {
                index += 1;
                args.get(index - 1).cloned()
            } else {
                Some(attached.to_owned())
            };
            match (flag, value) {
                ('p', Some(value)) => port = Some(value),
                ('l', Some(value)) => user = Some(value),
                ('o', Some(value)) => match ssh_option(&value) {
                    Some(("port", value)) => port = Some(value.to_owned()),
                    Some(("user", value)) => user = Some(value.to_owned()),
                    _ => translatable = false,
                },
                _ => translatable = false,
            }
            break;
        }
    }
    let destination = destination?;
    let Some(uri) = destination.strip_prefix("ssh://") else {
        return Some(SshDestination {
            host: destination,
            user,
            port,
            translatable,
        });
    };
    // `ssh://[user@]host[:port][/]`: the URI's own user and port win.
    let uri = uri.trim_end_matches('/');
    let (uri_user, rest) = match uri.rsplit_once('@') {
        Some((user, rest)) => (Some(user.to_owned()), rest),
        None => (None, uri),
    };
    let (host, uri_port) = match rest.strip_prefix('[') {
        Some(v6) => {
            let (host, after) = v6.split_once(']')?;
            (host, after.strip_prefix(':'))
        }
        None => match rest.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (rest, None),
        },
    };
    Some(SshDestination {
        host: host.to_owned(),
        user: uri_user.or(user),
        port: uri_port.map(str::to_owned).or(port),
        translatable,
    })
}

/// `-o Key=value` / `-o "Key value"` → the lowercased key and the value.
fn ssh_option(option: &str) -> Option<(&'static str, &str)> {
    let (key, value) = option
        .split_once('=')
        .or_else(|| option.split_once(char::is_whitespace))?;
    let value = value.trim();
    match key.trim().to_ascii_lowercase().as_str() {
        "port" => Some(("port", value)),
        "user" => Some(("user", value)),
        _ => None,
    }
}

// ─── open policy ─────────────────────────────────────────────────────────

/// How a remote file's preview opens (R5.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewOpen {
    /// In the default application of its content type — a known document.
    Default,
    /// In the default plain-text application: a script, a program, an `x`-bit
    /// file and every unknown type — the user wants to **read** it, and a
    /// preview is never run (Karar 4).
    PlainText,
}

/// The remote open policy: what a ⌘-click on `entry` does. `content` is the
/// platform's answer for the file's name (044's `DOCUMENT_TYPES`, UTType on
/// macOS) — asked only for a file. A folder does not preview (`None`, Karar 3).
///
/// | entry | action |
/// |---|---|
/// | folder | `None` |
/// | file with `x` | [`PreviewOpen::PlainText`] |
/// | file, known document | [`PreviewOpen::Default`] |
/// | any other file | [`PreviewOpen::PlainText`] |
pub fn preview_open(entry: &RemoteEntry, content: impl FnOnce() -> Content) -> Option<PreviewOpen> {
    match entry {
        RemoteEntry::Dir(_) => None,
        RemoteEntry::File {
            executable: true, ..
        } => Some(PreviewOpen::PlainText),
        RemoteEntry::File { .. } => Some(match content() {
            Content::Document => PreviewOpen::Default,
            Content::Package | Content::Other => PreviewOpen::PlainText,
        }),
    }
}

// ─── preview path ────────────────────────────────────────────────────────

/// Where a remote file's preview lives: `{dir}/{host}/{remote absolute path}`
/// (R5.1) — the same file always lands on the same copy, which is what lets an
/// unchanged file open from the cache (R5.4).
///
/// Refused (`None`), because the copy must never land outside `{dir}/{host}`:
/// a remote path that is not absolute, has a `.` or `..` segment or names no
/// file (`/`); a host that is empty, `.`/`..`, starts with `.` (the folder's own
/// hidden files, such as its index, live there) or holds a `/`; NUL anywhere.
/// Repeated slashes collapse (`//a` is `/a`).
pub fn preview_path(dir: &Path, host: &str, remote: &str) -> Option<PathBuf> {
    if host.is_empty() || host.starts_with('.') || host.contains(['/', '\0']) {
        return None;
    }
    let rest = remote.strip_prefix('/')?;
    if remote.contains('\0') {
        return None;
    }
    let mut path = dir.join(host);
    let mut named = false;
    for segment in rest.split('/').filter(|segment| !segment.is_empty()) {
        if segment == "." || segment == ".." {
            return None;
        }
        path.push(segment);
        named = true;
    }
    named.then_some(path)
}

// ─── cleanup ─────────────────────────────────────────────────────────────

/// What runs a cleanup (Karar 9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sweep {
    /// At launch: the expired previews, then the oldest until the folder is
    /// within its size limit.
    Launch,
    /// Once a day while bateri runs: the expired previews only — nothing for
    /// size, a preview open in an application must not vanish under it.
    Daily,
    /// The settings window's Clear Now: every preview.
    ClearNow,
}

/// One preview copy as the sweep sees it. Times are Unix seconds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CachedPreview {
    pub path: PathBuf,
    /// Its size and mtime on disk now.
    pub size: u64,
    pub mtime: u64,
    /// When bateri last opened it.
    pub last_open: u64,
    /// The size and mtime bateri wrote; `None` if the index has no record of
    /// it.
    pub written: Option<(u64, u64)>,
}

impl CachedPreview {
    /// Changed since bateri wrote it — the user unlocked and edited it.
    fn diverged(&self) -> bool {
        self.written != Some((self.size, self.mtime))
    }
}

/// The sweep's answer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SweepPlan {
    /// Copies to delete.
    pub delete: Vec<PathBuf>,
    /// Copies the user changed: never deleted, moved to the download folder and
    /// reported instead.
    pub rescue: Vec<PathBuf>,
}

/// The cleanup planner (Karar 9): which of `previews` the `sweep` removes,
/// `keep` and `limit` (bytes) from the settings, `now` in Unix seconds.
///
/// - The age is counted from the **last opening**, so a preview in use stays.
/// - [`Sweep::Launch`] removes the expired ones (every one under
///   [`PreviewKeep::UntilLaunch`]), then — while the folder is above `limit` —
///   the oldest opened first. [`Sweep::Daily`] only the expired ones, and
///   nothing under `UntilLaunch` (those are this session's). [`Sweep::ClearNow`]
///   every one.
/// - A copy whose size or mtime differs from what bateri wrote is never
///   deleted: it goes to [`SweepPlan::rescue`] on every trigger.
/// - A copy the index has no record of is **left alone** on every trigger: it
///   is not known to be bateri's (a damaged index must delete nothing), but it
///   still counts toward the size.
///
/// The order is the oldest opened first (path on a tie): deterministic.
pub fn plan_sweep(
    previews: &[CachedPreview],
    sweep: Sweep,
    keep: PreviewKeep,
    limit: u64,
    now: u64,
) -> SweepPlan {
    let mut known: Vec<&CachedPreview> = previews
        .iter()
        .filter(|preview| preview.written.is_some())
        .collect();
    known.sort_by(|a, b| (a.last_open, &a.path).cmp(&(b.last_open, &b.path)));
    let expired = |preview: &CachedPreview| match keep.max_age() {
        None => sweep == Sweep::Launch,
        Some(age) => now.saturating_sub(preview.last_open) > age.as_secs(),
    };
    let mut due: Vec<&CachedPreview> = match sweep {
        Sweep::ClearNow => known.clone(),
        Sweep::Launch | Sweep::Daily => known
            .iter()
            .copied()
            .filter(|preview| expired(preview))
            .collect(),
    };
    if sweep == Sweep::Launch {
        let mut total: u64 = previews
            .iter()
            .filter(|preview| !due.iter().any(|d| d.path == preview.path))
            .map(|preview| preview.size)
            .sum();
        for preview in &known {
            if total <= limit {
                break;
            }
            if !due.iter().any(|d| d.path == preview.path) {
                total = total.saturating_sub(preview.size);
                due.push(preview);
            }
        }
    }
    due.sort_by(|a, b| (a.last_open, &a.path).cmp(&(b.last_open, &b.path)));
    let mut plan = SweepPlan::default();
    for preview in due {
        if preview.diverged() {
            plan.rescue.push(preview.path.clone());
        } else {
            plan.delete.push(preview.path.clone());
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn words(argv: &[&str]) -> Vec<String> {
        argv.iter().map(|&arg| arg.to_owned()).collect()
    }

    fn ssh(argv: &[&str]) -> RemoteTarget {
        RemoteTarget {
            host: "prod".to_owned(),
            kind: RemoteKind::Ssh,
            argv: words(argv),
            line: String::new(),
        }
    }

    #[test]
    fn a_request_is_one_quoted_line_and_refuses_unsafe_names() {
        assert_eq!(
            request_line(7, Ask::Stat, &words(&["/var/www", "My Drive", "it's"])),
            Some("bt_stat 7 '/var/www' 'My Drive' 'it'\"'\"'s'\n".to_owned())
        );
        assert_eq!(
            request_line(8, Ask::Count, &words(&["/a"])),
            Some("bt_count 8 '/a'\n".to_owned())
        );
        assert_eq!(request_line(1, Ask::Stat, &words(&["a\nb"])), None);
        assert_eq!(request_line(1, Ask::Stat, &words(&["a\\b"])), None);
    }

    #[test]
    fn the_greeting_gives_the_remote_home() {
        assert_eq!(
            parse_greeting("Welcome!\nBT-HELPER\nBT-HOME /home/deploy\n"),
            Some(Some("/home/deploy".to_owned()))
        );
        assert_eq!(parse_greeting("BT-HELPER\nBT-HOME \n"), Some(None));
        assert_eq!(parse_greeting("bash: sh: not found\n"), None);
        assert_eq!(parse_greeting("BT-HELPER\n"), None);
    }

    #[test]
    fn a_reply_is_read_by_index_and_sequence() {
        let out = "noise\nBT-R 6\nBT-N 0\nBT-END 6\n\
                   BT-R 7\nBT-F 0 1234 1700000000 0\nBT-D 1\nBT-N 2\n\
                   BT-F 3 - - 1\nBT-C 4 12 3400000\nBT-END 7\n";
        assert_eq!(
            parse_reply(out, 7, 5),
            Ok(vec![
                Some(RemoteEntry::File {
                    size: Some(1234),
                    mtime: Some(1_700_000_000),
                    executable: false
                }),
                Some(RemoteEntry::Dir(None)),
                None,
                Some(RemoteEntry::File {
                    size: None,
                    mtime: None,
                    executable: true
                }),
                Some(RemoteEntry::Dir(Some(FolderSize {
                    files: 12,
                    bytes: 3_400_000
                }))),
            ])
        );
        assert_eq!(parse_reply(out, 6, 1), Ok(vec![None]), "an older reply");
        assert!(ends_reply("BT-END 7", 7));
        assert!(!ends_reply("BT-END 6", 7));
    }

    #[test]
    fn a_broken_reply_is_an_error_not_a_panic() {
        assert_eq!(parse_reply("", 1, 1), Err(ReplyError::NotStarted));
        assert_eq!(
            parse_reply("BT-R 1\nBT-N 0\n", 1, 1),
            Err(ReplyError::Unterminated)
        );
        assert_eq!(
            parse_reply("BT-R 1\nBT-END 1\n", 1, 1),
            Err(ReplyError::Unanswered(0))
        );
        for line in [
            "BT-F 0 12",
            "BT-F 0 x 1 0",
            "BT-F 0 1 1 2",
            "BT-N 5",
            "BT-N",
            "BT-N x",
            "BT-C 0 1",
            "BT-D 0 extra",
            "hello",
            "BT-F 0 99999999999999999999999 1 0",
        ] {
            let out = format!("BT-R 1\n{line}\nBT-END 1\n");
            assert_eq!(
                parse_reply(&out, 1, 1),
                Err(ReplyError::Malformed(line.to_owned())),
                "{line}"
            );
        }
    }

    #[test]
    fn the_helper_answers_through_a_local_sh() {
        // The real loop, a local `sh` in place of ssh: the script and the
        // parser must speak the same lines on this machine's `stat`.
        let dir = std::env::temp_dir().join(format!("bt-remote-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("folder/sub")).expect("temp folder");
        std::fs::write(dir.join("folder/a"), b"hello").expect("file");
        std::fs::write(dir.join("folder/sub/b"), b"abc").expect("file");
        std::fs::write(dir.join("it's here"), b"x").expect("file");
        let path = |name: &str| dir.join(name).to_string_lossy().into_owned();
        let asked = words(&[&path("it's here"), &path("folder"), &path("missing")]);
        let mut input = request_line(1, Ask::Stat, &asked).expect("safe");
        input.push_str(&request_line(2, Ask::Count, &asked).expect("safe"));
        let out = Command::new("sh")
            .arg("-c")
            .arg(helper_script())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write as _;
                child
                    .stdin
                    .take()
                    .expect("piped stdin")
                    .write_all(input.as_bytes())?;
                child.wait_with_output()
            })
            .expect("sh runs");
        let out = String::from_utf8_lossy(&out.stdout);
        assert!(parse_greeting(&out).is_some(), "{out}");
        let stat = parse_reply(&out, 1, 3).expect(&out);
        assert!(
            matches!(
                stat[0],
                Some(RemoteEntry::File {
                    size: Some(1),
                    mtime: Some(_),
                    executable: false
                })
            ),
            "{out}"
        );
        assert_eq!(stat[1], Some(RemoteEntry::Dir(None)));
        assert_eq!(stat[2], None);
        let count = parse_reply(&out, 2, 3).expect(&out);
        assert_eq!(
            count[1],
            Some(RemoteEntry::Dir(Some(FolderSize { files: 2, bytes: 8 }))),
            "{out}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_download_script_streams_one_item_out_of_its_folder() {
        assert_eq!(
            download_script("/var/log/app.log"),
            Some(format!(
                "cd '/var/log' || exit {NO_DIRECTORY}; exec tar -c -f - './app.log'"
            ))
        );
        assert_eq!(
            download_script("/-rf"),
            Some(format!(
                "cd '/' || exit {NO_DIRECTORY}; exec tar -c -f - './-rf'"
            ))
        );
        assert_eq!(split_remote("/srv/www/"), Some(("/srv", "www")));
        for refused in ["relative", "/", "/a/..", "/a/.", "/a\\b", "/a\nb"] {
            assert_eq!(download_script(refused), None, "{refused:?}");
        }
    }

    #[test]
    fn the_scp_path_carries_the_port_and_the_user() {
        assert_eq!(scp_path(&ssh(&["ssh", "prod"]), "/var/x"), "prod:/var/x");
        assert_eq!(
            scp_path(&ssh(&["ssh", "-p", "2222", "prod"]), "/var/x"),
            "-P 2222 prod:/var/x"
        );
        assert_eq!(
            scp_path(&ssh(&["ssh", "-p2222", "-l", "deploy", "-C", "prod"]), "/x"),
            "-P 2222 deploy@prod:/x"
        );
        assert_eq!(
            scp_path(&ssh(&["ssh", "-o", "Port=2200", "prod"]), "/x"),
            "-P 2200 prod:/x"
        );
        assert_eq!(
            scp_path(&ssh(&["ssh", "ssh://deploy@h:2222"]), "/x"),
            "-P 2222 deploy@h:/x"
        );
        assert_eq!(
            scp_path(&ssh(&["ssh", "prod", "-p", "22"]), "/x"),
            "-P 22 prod:/x",
            "an option after the destination counts too"
        );
        assert_eq!(
            scp_path(&ssh(&["ssh", "prod", "tmux", "-p", "9"]), "/x"),
            "prod:/x",
            "the remote command's options are not ssh's"
        );
    }

    #[test]
    fn an_untranslatable_option_leaves_only_host_and_path() {
        for argv in [
            &["ssh", "-J", "jump", "prod"][..],
            &["ssh", "-J", "jump", "-p", "2222", "prod"],
            &["ssh", "-i", "key", "-p", "2222", "prod"],
            &["ssh", "-o", "ProxyJump=jump", "-p", "2222", "prod"],
        ] {
            assert_eq!(scp_path(&ssh(argv), "/x"), "prod:/x", "{argv:?}");
        }
        let mosh = RemoteTarget {
            host: "deploy@prod".to_owned(),
            kind: RemoteKind::Mosh,
            argv: words(&["mosh", "--ssh=ssh -p 2222", "deploy@prod"]),
            line: String::new(),
        };
        assert_eq!(scp_path(&mosh, "/x"), "deploy@prod:/x");
    }

    #[test]
    fn the_scp_word_is_quoted_and_an_ipv6_host_bracketed() {
        assert_eq!(
            scp_path(&ssh(&["ssh", "prod"]), "/srv/My Drive/it's"),
            "'prod:/srv/My Drive/it'\"'\"'s'"
        );
        assert_eq!(scp_path(&ssh(&["ssh", "::1"]), "/x"), "[::1]:/x");
        assert_eq!(
            scp_path(&ssh(&["ssh", "ssh://u@[::1]:2222"]), "/x"),
            "-P 2222 u@[::1]:/x"
        );
    }

    #[test]
    fn a_folder_does_not_preview_and_a_program_reads_as_text() {
        let file = |executable| RemoteEntry::File {
            size: Some(1),
            mtime: Some(1),
            executable,
        };
        assert_eq!(
            preview_open(&file(false), || Content::Document),
            Some(PreviewOpen::Default)
        );
        assert_eq!(
            preview_open(&file(false), || Content::Other),
            Some(PreviewOpen::PlainText)
        );
        assert_eq!(
            preview_open(&file(true), || Content::Document),
            Some(PreviewOpen::PlainText),
            "an x-bit file is read, never run"
        );
        assert_eq!(
            preview_open(&RemoteEntry::Dir(None), || Content::Document),
            None
        );
    }

    #[test]
    fn the_preview_path_mirrors_the_remote_path_and_refuses_escapes() {
        let dir = Path::new("/cache/Previews");
        assert_eq!(
            preview_path(dir, "deploy@prod", "/var/log/app.log"),
            Some(PathBuf::from("/cache/Previews/deploy@prod/var/log/app.log"))
        );
        assert_eq!(
            preview_path(dir, "prod", "//etc//hosts"),
            Some(PathBuf::from("/cache/Previews/prod/etc/hosts"))
        );
        for (host, remote) in [
            ("prod", "relative/x"),
            ("prod", "/a/../../etc/passwd"),
            ("prod", "/a/./b"),
            ("prod", "/"),
            ("prod", "/a\0b"),
            ("", "/x"),
            ("..", "/x"),
            (".index", "/x"),
            ("a/b", "/x"),
        ] {
            assert_eq!(preview_path(dir, host, remote), None, "{host:?} {remote:?}");
        }
    }

    const DAY: u64 = 86_400;
    const NOW: u64 = 100 * DAY;

    fn preview(name: &str, size: u64, opened_days_ago: u64) -> CachedPreview {
        CachedPreview {
            path: PathBuf::from(name),
            size,
            mtime: 1_000,
            last_open: NOW - opened_days_ago * DAY,
            written: Some((size, 1_000)),
        }
    }

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn the_launch_sweep_takes_the_expired_then_the_oldest_over_the_limit() {
        let previews = [
            preview("old", 10, 9),
            preview("mid", 50, 3),
            preview("new", 50, 1),
            preview("today", 30, 0),
        ];
        // `old` expired (7 days); 130 bytes remain over a 100-byte limit, so
        // `mid` (the oldest opened) goes too.
        let plan = plan_sweep(&previews, Sweep::Launch, PreviewKeep::Week, 100, NOW);
        assert_eq!(plan.delete, paths(&["old", "mid"]));
        assert!(plan.rescue.is_empty());
        let roomy = plan_sweep(&previews, Sweep::Launch, PreviewKeep::Week, 1_000, NOW);
        assert_eq!(roomy.delete, paths(&["old"]));
        let all = plan_sweep(
            &previews,
            Sweep::Launch,
            PreviewKeep::UntilLaunch,
            1_000,
            NOW,
        );
        assert_eq!(all.delete, paths(&["old", "mid", "new", "today"]));
    }

    #[test]
    fn the_daily_sweep_never_removes_for_size() {
        let previews = [preview("old", 10, 2), preview("big", 5_000, 0)];
        let plan = plan_sweep(&previews, Sweep::Daily, PreviewKeep::Day, 100, NOW);
        assert_eq!(plan.delete, paths(&["old"]));
        let session = plan_sweep(&previews, Sweep::Daily, PreviewKeep::UntilLaunch, 0, NOW);
        assert_eq!(
            session,
            SweepPlan::default(),
            "this session's previews stay"
        );
    }

    #[test]
    fn clear_now_takes_every_known_preview() {
        let previews = [preview("a", 1, 0), preview("b", 1, 40)];
        let plan = plan_sweep(
            &previews,
            Sweep::ClearNow,
            PreviewKeep::Month,
            u64::MAX,
            NOW,
        );
        assert_eq!(plan.delete, paths(&["b", "a"]));
    }

    #[test]
    fn a_changed_copy_is_rescued_and_an_unknown_one_left_alone() {
        let mut edited = preview("edited", 12, 9);
        edited.mtime = 2_000;
        let mut resized = preview("resized", 12, 0);
        resized.size = 13;
        let mut unknown = preview("unknown", 500, 50);
        unknown.written = None;
        let previews = [edited, resized, unknown, preview("plain", 1, 9)];
        for sweep in [Sweep::Launch, Sweep::Daily, Sweep::ClearNow] {
            let plan = plan_sweep(&previews, sweep, PreviewKeep::Week, 1_000, NOW);
            assert!(
                !plan.delete.iter().any(|path| path.ends_with("edited")
                    || path.ends_with("resized")
                    || path.ends_with("unknown")),
                "{sweep:?}: {plan:?}"
            );
            assert!(plan.rescue.contains(&PathBuf::from("edited")), "{sweep:?}");
            assert!(
                !plan.rescue.contains(&PathBuf::from("unknown")),
                "{sweep:?}"
            );
            assert!(plan.delete.contains(&PathBuf::from("plain")), "{sweep:?}");
        }
        // The unknown copy still counts toward the size: over a 400-byte limit
        // the launch takes every known copy it may, and leaves it.
        let plan = plan_sweep(&previews, Sweep::Launch, PreviewKeep::Month, 400, NOW);
        assert_eq!(plan.delete, paths(&["plain"]));
        assert_eq!(plan.rescue, paths(&["edited", "resized"]));
    }

    fn file(size: u64) -> RemoteEntry {
        RemoteEntry::File {
            size: Some(size),
            mtime: None,
            executable: false,
        }
    }

    #[test]
    fn a_single_file_downloads_without_asking_unless_it_clashes_or_does_not_fit() {
        let sheet = |entry: &RemoteEntry, free, clash, setting| {
            download_sheet(
                "prod",
                "report.pdf",
                entry,
                "~/Downloads",
                free,
                clash,
                setting,
            )
        };
        // Nothing to ask: the setting's conflict rule rides along.
        assert_eq!(
            sheet(&file(10), Some(100), false, DownloadConflict::Ask),
            Ok(Conflict::KeepBoth)
        );
        assert_eq!(
            sheet(&file(10), None, true, DownloadConflict::Replace),
            Ok(Conflict::Replace)
        );
        assert_eq!(
            sheet(&file(10), None, true, DownloadConflict::KeepBoth),
            Ok(Conflict::KeepBoth)
        );
        // A clash under `ask`: Keep Both / Replace.
        let asked =
            sheet(&file(2_000_000), Some(1 << 40), true, DownloadConflict::Ask).expect_err("asks");
        assert_eq!(asked.message, "Download “report.pdf” from prod?");
        assert_eq!(
            asked.buttons,
            vec![
                ("Keep Both", Conflict::KeepBoth),
                ("Replace", Conflict::Replace)
            ]
        );
        assert!(asked.enabled);
        assert!(
            asked.informative.starts_with("2.0 MB → ~/Downloads\n"),
            "{}",
            asked.informative
        );
        assert!(asked.informative.contains("Keep both, or replace it?"));
        // Not enough space: disabled, with the reason.
        let full = sheet(&file(500), Some(100), false, DownloadConflict::Ask).expect_err("asks");
        assert!(!full.enabled);
        assert_eq!(full.buttons, vec![("Download", Conflict::KeepBoth)]);
        assert!(
            full.informative
                .contains("This Mac has 100 B free, “report.pdf” needs 500 B."),
            "{}",
            full.informative
        );
    }

    #[test]
    fn a_folder_always_asks_with_its_count() {
        let folder = RemoteEntry::Dir(Some(FolderSize {
            files: 3,
            bytes: 1_500_000,
        }));
        let sheet = download_sheet(
            "prod",
            "logs",
            &folder,
            "/Users/me/Downloads",
            Some(1 << 40),
            true,
            DownloadConflict::Replace,
        )
        .expect_err("a folder asks");
        assert_eq!(sheet.message, "Download folder “logs” from prod?");
        assert_eq!(
            sheet.informative,
            "3 files, 1.5 MB → /Users/me/Downloads\n\
             An item named “logs” already exists there: it will be replaced."
        );
        assert_eq!(sheet.buttons, vec![("Download", Conflict::Replace)]);
        assert!(sheet.enabled);
    }
}
