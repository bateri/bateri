//! Uploading a Finder drop to the remote directory (037 Karar 7 → Kullanıcı
//! kararı): a file or folder dropped in a remote session goes, after
//! confirmation, to the remote shell's directory through a `tar c | ssh … tar x`
//! stream. Nothing is pasted on its own (037 phase-7); the result line says
//! where it went.
//!
//! Three halves:
//!
//! - **Pure:** translating the ssh argv ([`ssh_argv`]), the remote scripts and
//!   quoting, the probe's reply ([`parse_probe`]), the tar stream's header
//!   reader ([`TarWatcher`]), the text of the line and the sheet. The tested half.
//! - **Process:** local measurement ([`measure`]), the probe ([`probe`]) and the
//!   stream ([`transfer`]) — all on a background thread, results to the main queue.
//! - **Queue** ([`Uploads`]): the main thread's state — order, progress, result
//!   line. AppKit-free; `window` sets up the sheet and the dispatch.
//!
//! **No password can be asked.** An ssh born from the GUI has no controlling
//! terminal; with `BatchMode=yes` and no key/agent it fails at once with a clear
//! error instead of hanging. If the user has a `ControlMaster`, it rides on it,
//! so even on a password host an open master connection is enough.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use bt_core::{
    HostMark, RemoteKind, RemoteTarget, Transfer, TransferAction, TransferControls, TransferTone,
};

use crate::jobs::SSH_VALUED;

/// The shortest interval between progress reports — a **design constant**. Every
/// report is a content frame (the line and the bar changed); reporting at display
/// rate would burn frames on a counter the eye cannot read. A fifth of a second
/// keeps the bar fluid and the numbers readable.
pub(crate) const TICK: Duration = Duration::from_millis(200);

/// The window over which speed is measured — a **design constant**: instantaneous
/// speed jumps packet by packet, while a long average reports a slowdown late.
const SPEED_WINDOW: Duration = Duration::from_secs(3);

/// How long the result line (`✓ 3 files uploaded`, `Cancelled — …`) stays in the
/// dock — a **design constant**. This is the stop condition: when it expires the
/// line goes away and no further frame is requested.
pub(crate) const LINGER: Duration = Duration::from_secs(4);

// ─── ssh and remote scripts ──────────────────────────────────────────────

/// ssh flags **kept** without a value: address family, agent/X11 forwarding,
/// compression, GSSAPI, quiet. The ones that change the connection itself; the
/// rest (`-t -n -N -W -s -O -v -f -M` …) break the stream or produce noise and
/// are dropped.
const KEPT_FLAGS: &str = "46AaCgKkqXxYy";

/// ssh options **kept** with their value: bind address, cipher, config, pkcs11,
/// identity, jump, user, MAC, `-o`, tag, port, control socket. Which ones take a
/// value is told by `jobs`'s table ([`SSH_VALUED`]); there is no second table.
const KEPT_VALUED: &str = "BbcFIiJlmoPpS";

/// The upload's ssh argv from the remote session's target (the script is appended).
///
/// **Our options come first**, because ssh takes the **first** value of a config
/// key: the user's `-o RequestTTY=force` must stay behind. `-T` (no tty: the
/// stream is binary), `BatchMode=yes` (no password can be asked, fail at once),
/// `ControlMaster=no` (**use** an open master connection but do not become one: a
/// master connection going to the background would hold the stream's pipe end).
///
/// With ssh the target's argv is filtered option by option ([`KEPT_FLAGS`],
/// [`KEPT_VALUED`]); options after the destination are read too (OpenSSH parses
/// them again) and the remote command (`-t prod tmux`) is dropped. With mosh
/// there is no ssh argv: only the host, with default ssh settings.
pub(crate) fn ssh_argv(target: &RemoteTarget) -> Vec<String> {
    let mut argv = vec![
        "-T".to_owned(),
        "-o".to_owned(),
        "BatchMode=yes".to_owned(),
        "-o".to_owned(),
        "ControlMaster=no".to_owned(),
    ];
    let program = match target.kind {
        RemoteKind::Mosh => {
            argv.push(target.host.clone());
            "ssh".to_owned()
        }
        RemoteKind::Ssh => {
            let program = target.argv.first().cloned().unwrap_or_else(|| "ssh".into());
            let args = target.argv.get(1..).unwrap_or_default();
            let mut destination = None;
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
                        // The first non-option after the destination: the remote command.
                        break;
                    }
                    destination = Some(arg.clone());
                    continue;
                };
                for (at, flag) in cluster.char_indices() {
                    if SSH_VALUED.contains(flag) {
                        let attached = &cluster[at + flag.len_utf8()..];
                        let value = if attached.is_empty() {
                            index += 1;
                            args.get(index - 1).cloned()
                        } else {
                            Some(attached.to_owned())
                        };
                        if KEPT_VALUED.contains(flag)
                            && let Some(value) = value
                        {
                            argv.push(format!("-{flag}"));
                            argv.push(value);
                        }
                        break;
                    }
                    if KEPT_FLAGS.contains(flag) {
                        argv.push(format!("-{flag}"));
                    }
                }
            }
            argv.extend(destination.or_else(|| Some(target.host.clone())));
            program
        }
    };
    argv.insert(0, program);
    argv
}

/// POSIX single quoting; an inner `'` as `'"'"'` — **produces no backslash**.
///
/// The remote command is first read by the user's **login shell** (it may be fish
/// or csh too), and fish treats `\'` and `\\` as escapes inside single quotes; the
/// `'\''` form would carry the backslash to the outer layer in nested quoting and
/// break the command in fish. `"'"` reads the same in every shell.
fn sq(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('\'');
    for c in text.chars() {
        if c == '\'' {
            quoted.push_str("'\"'\"'");
        } else {
            quoted.push(c);
        }
    }
    quoted.push('\'');
    quoted
}

/// The remote command: the POSIX script wrapped in `sh -c`. The login shell only
/// reads `sh -c '…'`, so the script's syntax is independent of the shell.
fn remote_command(script: &str) -> String {
    format!("sh -c {}", sq(script))
}

/// Whether this path can safely enter the remote script: **it must carry no
/// backslash and no control character.** Both break the two-layer quoting (login
/// shell + `sh -c`) in fish or csh; such a name is rejected openly on the sheet (a
/// known limit) rather than silently written to the wrong place.
fn is_safe(text: &str) -> bool {
    !text.chars().any(|c| c == '\\' || c.is_control())
}

/// The mark saying the probe's reply starts here: the login shell's rc file (an
/// `echo` in `.bashrc`) can mix into the output, and everything before the mark is
/// skipped.
const PROBE_MARK: &str = "BT-UPLOAD";

/// The script's exit code when it cannot change into the target directory.
const NO_DIRECTORY: i32 = 3;

/// What is asked remotely before the sheet opens, **in a single connection**: the
/// directory's full path (the home directory if there is no `dir`), `df -Pk`'s line,
/// whether `tar` exists, and whether each name already exists at the target (and is a
/// folder). **Indices**, not names, are printed: a name carrying a newline (rejected,
/// but still) must not be able to break the parsing.
pub(crate) fn probe_script(dir: Option<&str>, names: &[String]) -> String {
    let mut script = String::new();
    if let Some(dir) = dir {
        let _ = write!(script, "cd {} || exit {NO_DIRECTORY}; ", sq(dir));
    }
    let _ = write!(
        script,
        "echo {PROBE_MARK}; pwd; df -Pk . | tail -n 1 | sed 's/^/BT-DF /'; \
         command -v tar >/dev/null 2>&1 && echo BT-TAR; "
    );
    for (index, name) in names.iter().enumerate() {
        let name = sq(name);
        let _ = write!(
            script,
            "{{ [ -e {name} ] || [ -L {name} ]; }} && echo BT-E{index}; \
             [ -d {name} ] && echo BT-D{index}; "
        );
    }
    script.push_str("exit 0");
    script
}

/// The probe's reply.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ProbeReply {
    /// The target directory's full remote path (`pwd`).
    pub(crate) dir: String,
    /// Free space, bytes; `None` if `df` could not be read (no check can be made,
    /// and nothing is blocked either).
    pub(crate) free: Option<u64>,
    /// Whether `tar` exists remotely.
    pub(crate) tar: bool,
    /// Names already present at the target: index and whether it is a folder.
    pub(crate) existing: Vec<(usize, bool)>,
}

/// The probe's standard output → the reply; `None` if there is no mark (the script
/// never ran).
pub(crate) fn parse_probe(out: &str) -> Option<ProbeReply> {
    let mut lines = out.lines().skip_while(|line| line.trim() != PROBE_MARK);
    lines.next()?;
    let dir = lines.next()?.to_owned();
    let mut reply = ProbeReply {
        dir,
        ..ProbeReply::default()
    };
    let mut existing: Vec<(usize, bool)> = Vec::new();
    // Every line carries its own mark: if `df` prints nothing (missing, or not
    // supported on the mount point) the next line must not be read in its
    // place.
    for line in lines {
        let line = line.trim();
        if let Some(df) = line.strip_prefix("BT-DF ") {
            reply.free = df_available(df);
        } else if line == "BT-TAR" {
            reply.tar = true;
        } else if let Some(index) = line.strip_prefix("BT-E").and_then(|n| n.parse().ok()) {
            existing.push((index, false));
        } else if let Some(index) = line
            .strip_prefix("BT-D")
            .and_then(|n| n.parse::<usize>().ok())
            && let Some(entry) = existing.iter_mut().find(|(at, _)| *at == index)
        {
            entry.1 = true;
        }
    }
    reply.existing = existing;
    Some(reply)
}

/// Free space in bytes from `df -Pk`'s data line. The device name or the mount point
/// may contain spaces: the column is found as the one **immediately left** of the
/// capacity percentage (`NN%`), not by counting from the start.
fn df_available(line: &str) -> Option<u64> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    let capacity = fields.iter().position(|field| {
        field.ends_with('%') && field[..field.len() - 1].parse::<u32>().is_ok()
    })?;
    let kib: u64 = fields.get(capacity.checked_sub(1)?)?.parse().ok()?;
    kib.checked_mul(1024)
}

/// The script that unpacks the stream remotely: change into the target directory
/// and extract from standard input. `-p` keeps permissions, `-o` does **not** keep
/// ownership — a file uploaded as root must not end up with the local user's uid.
fn extract_script(dir: &str) -> String {
    format!("cd {} && exec tar -x -p -o -f -", sq(dir))
}

/// The script that deletes the half-written file on cancel (or when the disk fills);
/// `rel` is its path in the stream (`./static/app.js`), relative to the directory.
fn cleanup_script(dir: &str, rel: &str) -> String {
    format!("cd {} && rm -f -- {}", sq(dir), sq(rel))
}

// ─── tar stream reader ───────────────────────────────────────────────────

/// Block size.
const BLOCK: usize = 512;

/// The largest pax header size accumulated — more than enough for a `path=`
/// record; anything larger (a long attribute list) is passed over unread.
const PAX_LIMIT: usize = 64 * 1024;

/// The record's type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Entry {
    /// Regular file (`0`, `\0`, `7`) or hard link (`1`): counted.
    File,
    /// pax extended header (`x`): the next record's path may be here.
    Pax,
    /// Everything else (folder, symbolic link, global pax).
    Other,
}

/// A state machine reading the **headers** of the tar bytes we stream: how many
/// files finished, how many content bytes passed and which file is being written now.
///
/// That is why progress is exact: we carry the bytes and see every file's boundary.
/// The half-written file ([`Self::current`]) is the one deleted on cancel.
///
/// bsdtar puts a pax header (`x`) before the record for long, non-ASCII or large
/// files, and the `path=` there overrides the next header's name; for large files
/// the size field can be base-256. Both are read.
#[derive(Debug)]
pub(crate) struct TarWatcher {
    header: Vec<u8>,
    /// The record's remaining data bytes (padding excluded).
    remaining: u64,
    /// The padding after the data.
    padding: u64,
    entry: Entry,
    pax: Vec<u8>,
    /// The next record's path, as pax said.
    next_path: Option<String>,
    /// A file whose header has passed but whose data has not finished.
    pub(crate) current: Option<String>,
    /// The number of files whose data has passed completely.
    pub(crate) files: u64,
    /// Content bytes passed (file data only).
    pub(crate) bytes: u64,
}

impl Default for TarWatcher {
    fn default() -> Self {
        Self {
            header: Vec::with_capacity(BLOCK),
            remaining: 0,
            padding: 0,
            entry: Entry::Other,
            pax: Vec::new(),
            next_path: None,
            current: None,
            files: 0,
            bytes: 0,
        }
    }
}

impl TarWatcher {
    /// The next chunk of the stream.
    pub(crate) fn feed(&mut self, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            if self.remaining > 0 {
                let take =
                    usize::try_from(self.remaining).map_or(bytes.len(), |r| r.min(bytes.len()));
                let (data, rest) = bytes.split_at(take);
                match self.entry {
                    Entry::File => self.bytes += data.len() as u64,
                    Entry::Pax if self.pax.len() + data.len() <= PAX_LIMIT => {
                        self.pax.extend_from_slice(data);
                    }
                    Entry::Pax | Entry::Other => {}
                }
                self.remaining -= take as u64;
                bytes = rest;
                if self.remaining == 0 {
                    self.entry_done();
                }
                continue;
            }
            if self.padding > 0 {
                let take =
                    usize::try_from(self.padding).map_or(bytes.len(), |p| p.min(bytes.len()));
                self.padding -= take as u64;
                bytes = &bytes[take..];
                continue;
            }
            let take = (BLOCK - self.header.len()).min(bytes.len());
            self.header.extend_from_slice(&bytes[..take]);
            bytes = &bytes[take..];
            if self.header.len() == BLOCK {
                let header = std::mem::take(&mut self.header);
                self.begin(&header);
                self.header = header;
                self.header.clear();
            }
        }
    }

    /// A header block is complete.
    fn begin(&mut self, header: &[u8]) {
        // A zero block is the end of the archive (or its padding).
        if header.iter().all(|&byte| byte == 0) {
            return;
        }
        let size = tar_size(&header[124..136]);
        let kind = header[156];
        self.entry = match kind {
            b'0' | 0 | b'7' | b'1' => Entry::File,
            b'x' => Entry::Pax,
            _ => Entry::Other,
        };
        let name = self.next_path.take();
        if self.entry == Entry::File {
            self.current = Some(name.unwrap_or_else(|| tar_name(header)));
        } else if self.entry == Entry::Pax {
            self.pax.clear();
        }
        self.remaining = size;
        self.padding = (BLOCK as u64 - size % BLOCK as u64) % BLOCK as u64;
        if size == 0 {
            self.entry_done();
        }
    }

    /// The record's data is finished.
    fn entry_done(&mut self) {
        match self.entry {
            Entry::File => {
                self.files += 1;
                self.current = None;
            }
            Entry::Pax => self.next_path = pax_path(&self.pax),
            Entry::Other => {}
        }
        self.entry = Entry::Other;
    }
}

/// The header's size field: octal ASCII or (if the high bit is set) base-256
/// binary.
fn tar_size(field: &[u8]) -> u64 {
    if field.first().is_some_and(|&byte| byte & 0x80 != 0) {
        return field[1..]
            .iter()
            .fold(u64::from(field[0] & 0x7f), |acc, &byte| {
                (acc << 8) | u64::from(byte)
            });
    }
    field
        .iter()
        .skip_while(|&&byte| byte == b' ')
        .take_while(|&&byte| (b'0'..=b'7').contains(&byte))
        .fold(0, |acc, &byte| acc * 8 + u64::from(byte - b'0'))
}

/// The ustar header's name: `prefix/name` (just the name if prefix is empty).
fn tar_name(header: &[u8]) -> String {
    let field = |range: std::ops::Range<usize>| {
        let raw = &header[range];
        let end = raw.iter().position(|&byte| byte == 0).unwrap_or(raw.len());
        String::from_utf8_lossy(&raw[..end]).into_owned()
    };
    let name = field(0..100);
    let prefix = if &header[257..262] == b"ustar" {
        field(345..500)
    } else {
        String::new()
    };
    if prefix.is_empty() {
        name
    } else {
        format!("{prefix}/{name}")
    }
}

/// `path` from the pax records (`"{length} {key}={value}\n"`).
fn pax_path(data: &[u8]) -> Option<String> {
    let mut rest = data;
    let mut path = None;
    while !rest.is_empty() {
        let space = rest.iter().position(|&byte| byte == b' ')?;
        let length: usize = std::str::from_utf8(&rest[..space]).ok()?.parse().ok()?;
        if length <= space || length > rest.len() {
            return path;
        }
        let record = &rest[space + 1..length];
        let record = record.strip_suffix(b"\n").unwrap_or(record);
        if let Some(value) = record.strip_prefix(b"path=") {
            path = Some(String::from_utf8_lossy(value).into_owned());
        }
        rest = &rest[length..];
    }
    path
}

// ─── text ────────────────────────────────────────────────────────────────

/// A byte count in decimal units (Finder's units): `512 B`, `18.2 MB`.
pub(crate) fn format_bytes(bytes: u64) -> String {
    let (value, unit) = scaled(bytes, bytes);
    if unit == "B" {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {unit}")
    }
}

/// `done / total`, both in the **total's** unit: `18.2 / 44.6 MB`.
fn format_pair(done: u64, total: u64) -> String {
    let (value, unit) = scaled(done, total);
    let (whole, _) = scaled(total, total);
    if unit == "B" {
        format!("{done} / {total} B")
    } else {
        format!("{value:.1} / {whole:.1} {unit}")
    }
}

/// Converts `bytes` into `reference`'s unit.
fn scaled(bytes: u64, reference: u64) -> (f64, &'static str) {
    const UNITS: [(u64, &str); 3] = [(1_000_000_000, "GB"), (1_000_000, "MB"), (1_000, "KB")];
    for (size, unit) in UNITS {
        if reference >= size {
            return (bytes as f64 / size as f64, unit);
        }
    }
    (bytes as f64, "B")
}

/// Bytes per second: `1.2 MB/s`.
fn format_rate(per_second: f64) -> String {
    // audit: rounding only to pick the unit; the value is printed from the f64.
    let whole = per_second.max(0.0) as u64;
    let (value, unit) = scaled(whole, whole);
    if unit == "B" {
        format!("{whole} B/s")
    } else {
        let exact = per_second / (whole as f64 / value);
        format!("{exact:.1} {unit}/s")
    }
}

/// Remaining time: `22s`, `1m 05s`, `1h 02m`.
fn format_duration(seconds: u64) -> String {
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m {:02}s", seconds / 60, seconds % 60),
        _ => format!("{}h {:02}m", seconds / 3600, seconds % 3600 / 60),
    }
}

/// `n file(s)`.
fn files_word(count: u64) -> &'static str {
    if count == 1 { "file" } else { "files" }
}

/// `done of total`, both in the **total's** unit: `48.0 of 96.0 MB`
/// (the stop question).
fn format_of(done: u64, total: u64) -> String {
    format_pair(done, total).replacen(" / ", " of ", 1)
}

/// How the queue ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum End {
    /// Everything uploaded.
    Done,
    /// ⌘., the line's `Cancel` or the popover's cancel.
    Cancelled,
    /// The remote disk filled up.
    DiskFull,
    /// The remote session (ssh) closed; pending items cancelled.
    Closed,
    /// The stream failed; the last line from ssh or tar.
    Failed(String),
}

/// The item's name in the list and in the result line: a trailing `/` for a folder
/// (`static/`), the name itself for a file.
fn label(local: &Local) -> String {
    if local.dir {
        format!("{}/", local.name)
    } else {
        local.name.clone()
    }
}

/// A summary of the items when the queue ends: how many items, how many uploaded,
/// their common destination (if any) and the single item's name.
struct Tally<'a> {
    items: usize,
    done: usize,
    /// That directory, if all items went to the same directory.
    dest: Option<&'a str>,
    /// Its name if there is a single item ([`label`]).
    single: Option<String>,
}

impl<'a> Tally<'a> {
    fn of(entries: &'a [Item]) -> Self {
        let dest = entries.first().map(|entry| entry.job.dir.as_str());
        let same = entries
            .iter()
            .all(|entry| Some(entry.job.dir.as_str()) == dest);
        Self {
            items: entries.len(),
            done: entries
                .iter()
                .filter(|entry| entry.state == EntryState::Done)
                .count(),
            dest: dest.filter(|_| same),
            single: match entries {
                [entry] => Some(label(&entry.job.local)),
                _ => None,
            },
        }
    }

    /// `backup.tar.gz`, `static/` or `3 files`.
    fn what(&self) -> String {
        self.single.clone().unwrap_or_else(|| {
            // audit: the item count is per drop; it fits in a `u64`.
            format!("{} {}", self.items, files_word(self.items as u64))
        })
    }
}

/// The failure's reason, in the line's lowercase form (`disk full on prod`).
fn failure_reason(end: &End, host: &str) -> Option<String> {
    match end {
        End::Done | End::Cancelled => None,
        End::DiskFull => Some(format!("disk full on {host}")),
        End::Closed => Some(format!("connection to {host} lost")),
        End::Failed(reason) => Some(reason.clone()),
    }
}

/// The result line's body, its tone and the number of leading characters drawn in
/// that tone (037 phase-7): success green, cancel dim, the failure text red and the
/// count after it dim.
fn end_line(end: &End, host: &str, tally: &Tally<'_>) -> (String, TransferTone, usize) {
    if let Some(reason) = failure_reason(end, host) {
        let head = format!("Failed — {reason}");
        let lead = head.chars().count();
        let body = format!("{head} · {} of {} uploaded", tally.done, tally.items);
        return (body, TransferTone::Error, lead);
    }
    let body = match (end, tally.dest) {
        (End::Done, Some(dest)) => format!("✓ {} → {dest}", tally.what()),
        (End::Done, None) => format!(
            "✓ {} {} uploaded",
            tally.items,
            // audit: the item count is per drop; it fits in a `u64`.
            files_word(tally.items as u64)
        ),
        _ if tally.items > 1 => format!(
            "Cancelled — {} of {} uploaded, partial file removed",
            tally.done, tally.items
        ),
        _ => "Cancelled — partial file removed".to_owned(),
    };
    let tone = if *end == End::Done {
        TransferTone::Success
    } else {
        TransferTone::Quiet
    };
    let lead = body.chars().count();
    (body, tone, lead)
}

/// The notification shown while bateri is in the background (037 phase-7): title
/// and body. Cancelling is the user's own action — no notification.
fn end_notice(end: &End, host: &str, tally: &Tally<'_>) -> Option<(String, String)> {
    if let Some(reason) = failure_reason(end, host) {
        let mut chars = reason.trim_end_matches('.').chars();
        let reason = chars
            .next()
            .map(|first| first.to_uppercase().chain(chars).collect::<String>())
            .unwrap_or_default();
        return Some((
            "Upload failed".to_owned(),
            format!("{reason}. {} of {} uploaded.", tally.done, tally.items),
        ));
    }
    if *end != End::Done {
        return None;
    }
    let body = match tally.dest {
        Some(dest) => format!("to {host}:{dest}"),
        None => format!("to {host}"),
    };
    Some((format!("{} uploaded", tally.what()), body))
}

// ─── local measurement ───────────────────────────────────────────────────

/// A dropped item: its path, name, whether it is a folder, how many files and how
/// many bytes — both **locally, before the upload starts** (the sheet states them,
/// and progress is measured against them).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Local {
    pub(crate) path: PathBuf,
    pub(crate) name: String,
    pub(crate) dir: bool,
    pub(crate) files: u64,
    pub(crate) bytes: u64,
}

/// Measures a path. Symbolic links are **not followed** (tar does not follow them
/// either, it carries the link as a link); an unreadable subfolder is skipped — tar
/// will fail on it anyway and the line will say so.
pub(crate) fn measure(path: &Path) -> std::io::Result<Local> {
    let meta = std::fs::symlink_metadata(path)?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut local = Local {
        path: path.to_owned(),
        name,
        dir: meta.is_dir(),
        files: 0,
        bytes: 0,
    };
    walk(path, &meta, &mut local);
    Ok(local)
}

fn walk(path: &Path, meta: &std::fs::Metadata, into: &mut Local) {
    if meta.is_file() {
        into.files += 1;
        into.bytes += meta.len();
    } else if meta.is_dir()
        && let Ok(entries) = std::fs::read_dir(path)
    {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.path().symlink_metadata() {
                walk(&entry.path(), &meta, into);
            }
        }
    }
}

// ─── sheet ───────────────────────────────────────────────────────────────

/// The confirmation sheet's text and button.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Sheet {
    pub(crate) message: String,
    pub(crate) informative: String,
    /// The confirm button's title: `Upload`, `Replace` if a same-named file exists,
    /// `Merge` if a same-named folder exists.
    pub(crate) button: &'static str,
    /// Whether the confirm button is enabled: disabled if there is not enough space,
    /// if there is no remote tar, or if a name cannot be sent safely.
    pub(crate) enabled: bool,
}

/// The confirmation sheet (Kullanıcı kararı 1; its text 037 phase-7): the title says
/// what goes where, the first line states the size and destination
/// (`96.0 MB → /var/www/app`, with the file count for a folder), a list of names for
/// multiple items; same-named item, free space and tar as today. `reported`: whether
/// the destination is the remote shell's OSC 7 directory (if not, it is the home
/// directory and the sheet says so). `busy`: an upload is running — the sheet says it
/// does not stop.
///
/// **Free space is compared only with this drop**: the not-yet-sent bytes of items
/// waiting in the queue are not subtracted (known limit) — a "not enough" summing
/// the two could not say which item does not fit.
pub(crate) fn sheet(
    host: &str,
    reported: bool,
    items: &[Local],
    reply: &ProbeReply,
    busy: bool,
) -> Sheet {
    let one = items.len() == 1;
    let quoted = |name: &str| format!("“{name}”");
    let bytes: u64 = items.iter().map(|item| item.bytes).sum();
    let dest = &reply.dir;
    let (message, summary) = match items {
        [item] if item.dir => (
            format!("Upload folder {} to {host}?", quoted(&item.name)),
            format!(
                "{} {}, {} → {dest}",
                item.files,
                files_word(item.files),
                format_bytes(item.bytes)
            ),
        ),
        [item] => (
            format!("Upload {} to {host}?", quoted(&item.name)),
            format!("{} → {dest}", format_bytes(item.bytes)),
        ),
        _ => (
            format!("Upload {} items to {host}?", items.len()),
            format!("{} → {dest}", format_bytes(bytes)),
        ),
    };
    let mut lines = vec![summary];
    if !reported {
        lines.push(
            "That is the home folder, because the remote shell has not reported its folder."
                .to_owned(),
        );
    }
    if !one {
        lines.extend(items.iter().map(|item| item.name.clone()));
    }
    let clashes: Vec<(usize, bool)> = reply
        .existing
        .iter()
        .copied()
        .filter(|(index, _)| *index < items.len())
        .collect();
    let merges = clashes
        .iter()
        .any(|&(index, remote_dir)| remote_dir && items[index].dir);
    match clashes[..] {
        [] => {}
        [(index, remote_dir)] => {
            let kind = if remote_dir { "folder" } else { "file" };
            let what = if items[index].dir && remote_dir {
                "same-named files are replaced, others are kept."
            } else {
                "it will be replaced."
            };
            lines.push(format!(
                "A {kind} named {} already exists there: {what}",
                quoted(&items[index].name)
            ));
        }
        _ => lines.push(format!(
            "{} items already exist there: same-named files are replaced, others are kept.",
            clashes.len()
        )),
    }
    let mut enabled = true;
    if let Some(free) = reply.free
        && free < bytes
    {
        enabled = false;
        let needs = if one {
            format!("{} needs", quoted(&items[0].name))
        } else {
            "these items need".to_owned()
        };
        lines.push(format!(
            "{host} has {} free, {needs} {}.",
            format_bytes(free),
            format_bytes(bytes)
        ));
    }
    if !reply.tar {
        enabled = false;
        lines.push(format!(
            "tar is not installed on {host}, so nothing can be sent."
        ));
    }
    if let Some(item) = items.iter().find(|item| !is_safe(&item.name)) {
        enabled = false;
        lines.push(format!(
            "{} has a backslash or a control character in its name and can't be sent safely.",
            quoted(&item.name)
        ));
    }
    if busy {
        lines.push("Added to the queue; the current upload keeps going.".to_owned());
    }
    let button = if clashes.is_empty() {
        "Upload"
    } else if merges {
        "Merge"
    } else {
        "Replace"
    };
    Sheet {
        message,
        informative: lines.join("\n"),
        button,
        enabled,
    }
}

// ─── process ─────────────────────────────────────────────────────────────

/// The probe's failure: the text of the error sheet opened in place of the sheet.
pub(crate) fn probe_failure(host: &str, code: Option<i32>, stderr: &str) -> String {
    let last = last_line(stderr);
    match code {
        Some(NO_DIRECTORY) => format!("The remote folder on {host} can no longer be opened."),
        _ if last.is_empty() => format!(
            "ssh could not connect to {host} without asking for a password. Uploads need \
             key-based login (ssh-agent) or an open ControlMaster connection."
        ),
        _ => format!(
            "ssh could not connect to {host} without asking for a password. Uploads need \
             key-based login (ssh-agent) or an open ControlMaster connection.\n\n{last}"
        ),
    }
}

/// The text's last non-empty line.
fn last_line(text: &str) -> &str {
    text.lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
}

/// Runs the remote script over ssh (without input): exit code, stdout, stderr.
fn run_ssh(ssh: &[String], script: &str) -> std::io::Result<(Option<i32>, String, String)> {
    let output = Command::new(&ssh[0])
        .args(&ssh[1..])
        .arg(remote_command(script))
        .stdin(Stdio::null())
        .output()?;
    Ok((
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

/// The work before the sheet, on a background thread: local measurement and the
/// remote probe. `Err` → the error sheet's text.
pub(crate) fn probe(
    ssh: &[String],
    host: &str,
    dir: Option<&str>,
    paths: &[String],
) -> Result<(Vec<Local>, ProbeReply), String> {
    let items: Vec<Local> = paths
        .iter()
        .filter_map(|path| measure(Path::new(path)).ok())
        .collect();
    if items.is_empty() {
        return Err("The dropped items could not be read.".to_owned());
    }
    if let Some(dir) = dir.filter(|dir| !is_safe(dir)) {
        return Err(format!(
            "The remote folder {dir} has a backslash or a control character in its path and \
             can't be used safely."
        ));
    }
    let names: Vec<String> = items.iter().map(|item| item.name.clone()).collect();
    let (code, out, err) = run_ssh(ssh, &probe_script(dir, &names))
        .map_err(|error| format!("ssh could not be started: {error}"))?;
    match parse_probe(&out) {
        Some(reply) if code == Some(0) => Ok((items, reply)),
        _ => Err(probe_failure(host, code, &err)),
    }
}

/// The shared state of one item's stream: the main thread cancels and reads the
/// progress, the stream thread writes it.
#[derive(Debug, Default)]
pub(crate) struct Shared {
    cancel: AtomicBool,
    disk_full: AtomicBool,
    bytes: AtomicU64,
    files: AtomicU64,
    /// The pids of the two processes in the stream (local tar, ssh) — cancelling
    /// **kills** them: on a slow connection the stream thread stays blocked writing
    /// to ssh's input and never looks at the flag; once the process dies the write
    /// returns with `EPIPE`.
    pids: Mutex<Vec<u32>>,
    /// Whether a progress report is waiting on the main queue (at most one).
    pub(crate) tick_pending: AtomicBool,
}

impl Shared {
    /// Kills all processes (cancel and disk full).
    fn kill(&self) {
        for &pid in self
            .pids
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
        {
            // SAFETY: `kill(2)` takes only a pid and a signal; the pid is this
            // stream's own child and leaves the list under this lock **before it
            // is reaped** ([`wait_untracked`]) — no signal can reach a pid that
            // was reaped and handed to another process.
            // audit: pid from `u32` to `pid_t`; on macOS pids are positive `i32`.
            unsafe {
                libc::kill(pid as libc::pid_t, libc::SIGTERM);
            }
        }
    }

    /// Requests cancellation and kills the processes.
    pub(crate) fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
        self.kill();
    }

    /// Content bytes passed and the number of finished files.
    pub(crate) fn progress(&self) -> (u64, u64) {
        (
            self.bytes.load(Ordering::Acquire),
            self.files.load(Ordering::Acquire),
        )
    }

    fn track(&self, children: &[&Child]) {
        let mut pids = self.pids.lock().unwrap_or_else(PoisonError::into_inner);
        pids.clear();
        pids.extend(children.iter().map(|child| child.id()));
        drop(pids);
        // If cancelled before starting (⌘. when its turn came), kill at once.
        if self.cancel.load(Ordering::Acquire) {
            self.kill();
        }
    }

    fn untrack(&self, pid: u32) {
        self.pids
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|&tracked| tracked != pid);
    }
}

/// Waits for the child to exit, removes its pid from [`Shared`]'s list and **only
/// then** reaps it (`wait`). The order is required: a reaped pid can be handed to
/// another process by the kernel right away, and had it stayed on the list a cancel
/// arriving in between would kill an unrelated process. `waitid(…, WNOWAIT)` reports
/// the exit without reaping.
fn wait_untracked(child: &mut Child, shared: &Shared) -> std::io::Result<std::process::ExitStatus> {
    let pid = child.id();
    loop {
        // SAFETY: `siginfo_t` is a plain C struct, zero is a valid initial value;
        // `waitid` only writes into it. The pid is this thread's own child.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // SAFETY: as above; `WNOWAIT` does not reap the child, `wait` is below.
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                libc::id_t::from(pid),
                &raw mut info,
                libc::WEXITED | libc::WNOWAIT,
            )
        };
        if result == 0 || std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
        {
            break;
        }
    }
    shared.untrack(pid);
    child.wait()
}

/// The result of one item's stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Done,
    Cancelled,
    DiskFull,
    Failed(String),
}

/// The line saying the remote disk is full (GNU tar, bsdtar and busybox print the
/// same `strerror`).
const DISK_FULL: &str = "No space left on device";

/// Uploads one item: `tar c` locally, `ssh … tar x` remotely, the bytes in between
/// pass through us ([`TarWatcher`]). **On a background thread**; `tick` posts the
/// progress report to the main queue (at most once per [`TICK`]).
///
/// Cancel or disk full: both processes are killed and the file **being written** is
/// deleted remotely — for a single file the file itself, for a folder only the one
/// being written at that moment; finished ones stay (Kullanıcı kararı 5, 6).
pub(crate) fn transfer(
    ssh: &[String],
    local: &Local,
    dir: &str,
    shared: &Arc<Shared>,
    tick: impl Fn(),
) -> Outcome {
    let parent = local.path.parent().unwrap_or(Path::new("/"));
    let remote = Command::new(&ssh[0])
        .args(&ssh[1..])
        .arg(remote_command(&extract_script(dir)))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn();
    let mut remote = match remote {
        Ok(child) => child,
        Err(error) => return Outcome::Failed(format!("ssh could not be started: {error}")),
    };
    // `./name`: a name starting with `-` must not be taken for an option. macOS's
    // tar must not produce `._name` AppleDouble records and attribute paxes — they
    // would become junk files remotely and warnings in GNU tar.
    let local_tar = Command::new("/usr/bin/tar")
        .args([
            "-c",
            "-f",
            "-",
            "--no-mac-metadata",
            "--no-xattrs",
            "--no-acls",
            "-C",
        ])
        .arg(parent)
        .arg(format!("./{}", local.name))
        .env("COPYFILE_DISABLE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut local_tar = match local_tar {
        Ok(child) => child,
        Err(error) => {
            let _ = remote.kill();
            let _ = remote.wait();
            return Outcome::Failed(format!("tar could not be started: {error}"));
        }
    };
    shared.track(&[&local_tar, &remote]);

    let remote_err = collect_stderr(remote.stderr.take(), Some(Arc::clone(shared)));
    let local_err = collect_stderr(local_tar.stderr.take(), None);
    let mut watcher = TarWatcher::default();
    if let (Some(mut source), Some(mut sink)) = (local_tar.stdout.take(), remote.stdin.take()) {
        let mut buffer = vec![0; 64 * 1024];
        let mut last = Instant::now();
        loop {
            let read = match source.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => read,
            };
            watcher.feed(&buffer[..read]);
            if sink.write_all(&buffer[..read]).is_err() {
                break;
            }
            shared.bytes.store(watcher.bytes, Ordering::Release);
            shared.files.store(watcher.files, Ordering::Release);
            if last.elapsed() >= TICK {
                last = Instant::now();
                tick();
            }
        }
        // The input closes here: the remote tar must see the end of the stream.
    }
    let remote_status = wait_untracked(&mut remote, shared);
    let local_status = wait_untracked(&mut local_tar, shared);
    let remote_err = remote_err.join().unwrap_or_default();
    let local_err = local_err.join().unwrap_or_default();

    let interrupted = if shared.disk_full.load(Ordering::Acquire) {
        Some(Outcome::DiskFull)
    } else if shared.cancel.load(Ordering::Acquire) {
        Some(Outcome::Cancelled)
    } else {
        None
    };
    if let Some(outcome) = interrupted {
        if let Some(partial) = watcher.current.take() {
            let _ = run_ssh(ssh, &cleanup_script(dir, &partial));
        }
        return outcome;
    }
    if !local_status.is_ok_and(|status| status.success()) {
        let line = last_line(&local_err);
        return Outcome::Failed(if line.is_empty() {
            "the local tar failed".to_owned()
        } else {
            line.to_owned()
        });
    }
    match remote_status {
        Ok(status) if status.success() => Outcome::Done,
        Ok(status) => {
            let line = last_line(&remote_err);
            Outcome::Failed(if line.is_empty() {
                format!("ssh exited with {}", status.code().unwrap_or(-1))
            } else {
                line.to_owned()
            })
        }
        Err(error) => Outcome::Failed(error.to_string()),
    }
}

/// Reads a process's error output to the end on a separate thread (so the pipe does
/// not fill up and block the process); if `disk` is given, stops the stream at once
/// on a "disk full" line — GNU tar keeps swallowing the stream after the error and
/// the remaining gigabytes would be wasted.
fn collect_stderr(
    stream: Option<std::process::ChildStderr>,
    disk: Option<Arc<Shared>>,
) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let Some(stream) = stream else {
            return String::new();
        };
        let mut text = String::new();
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else { break };
            if let Some(shared) = &disk
                && line.contains(DISK_FULL)
            {
                shared.disk_full.store(true, Ordering::Release);
                shared.kill();
            }
            text.push_str(&line);
            text.push('\n');
        }
        text
    })
}

/// The window and tab title (037 phase-7): while an upload streams, `↑ N% · ` in
/// front of the current title — on the alternate screen (vim) there is no dock and
/// this is the only place showing progress; otherwise the title as is.
pub(crate) fn titled(percent: Option<u8>, title: &str) -> String {
    match percent {
        Some(percent) => format!("↑ {percent}% · {title}"),
        None => title.to_owned(),
    }
}

// ─── queue ───────────────────────────────────────────────────────────────

/// Stopping the streaming item asks first if it has been running longer than this
/// (037 phase-7) — a **design constant, not a measurement**. Stopping a short upload
/// is cheap (dropping again takes seconds); losing an upload past half a minute must
/// not happen with a single wrong click.
pub(crate) const STOP_ASK_AFTER: Duration = Duration::from_secs(30);

/// An item in the queue: the local item and the remote destination directory.
#[derive(Clone, Debug)]
pub(crate) struct Job {
    pub(crate) local: Local,
    pub(crate) dir: String,
}

/// An item's state in the queue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EntryState {
    Waiting,
    Running,
    Done,
}

/// An item of the queue. A finished item **stays** on the list (037 phase-7): the
/// popover shows it as `✓ Uploaded` and the result line counts it.
#[derive(Debug)]
struct Item {
    /// Unique across queues: the popover's button finds the item by this — the
    /// position would shift as items finish and are removed.
    id: u64,
    job: Job,
    state: EntryState,
}

/// The item currently streaming.
#[derive(Debug)]
struct Current {
    id: u64,
    shared: Arc<Shared>,
    /// When the stream started: the criterion of the stop question ([`STOP_ASK_AFTER`]).
    started: Instant,
}

/// A tab's upload queue — **bound to that tab's ssh connection** (Kullanıcı kararı 7):
/// its generation is the remote session's command generation; if the generation
/// changes (ssh closed) the pending items are cancelled.
#[derive(Debug)]
struct Queue {
    command: u64,
    ssh: Vec<String>,
    host: String,
    mark: HostMark,
    entries: Vec<Item>,
    current: Option<Current>,
    /// The bar's denominator and the bytes of finished items: the unsent part of an
    /// individually stopped item is subtracted from the denominator and the sent part
    /// counts as done — so the bar never goes backwards.
    bytes_total: u64,
    bytes_done: u64,
    /// Speed samples: (instant, the queue's bytes passed).
    samples: VecDeque<(Instant, u64)>,
    /// The last measured speed, bytes/s (the popover's row shows it too).
    rate: Option<f64>,
    /// The next item will not start: the queue ends with this end.
    ending: Option<End>,
    /// The streaming item was stopped on its own: the `Cancelled` result does not end
    /// the queue, the item leaves the list and the next one starts.
    skip: bool,
}

impl Queue {
    fn running_entry(&self) -> Option<(usize, &Item)> {
        let id = self.current.as_ref()?.id;
        self.entries
            .iter()
            .enumerate()
            .find(|(_, entry)| entry.id == id)
    }

    fn waiting(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.state == EntryState::Waiting)
            .count()
    }

    fn done(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.state == EntryState::Done)
            .count()
    }
}

/// A tab's upload state (main thread): whether a sheet is in progress, the queue and
/// the result line's generation.
#[derive(Debug, Default)]
pub(crate) struct Uploads {
    /// A probe or confirmation sheet is in progress: a new drop is rejected (two
    /// sheets cannot be stacked).
    asking: bool,
    queue: Option<Queue>,
    /// The items' id counter ([`Item::id`]).
    next_id: u64,
    /// The result line's generation: when the wait ends the line goes away only if the
    /// same result is still shown.
    serial: u64,
    /// The line last written to the dock ([`Self::shown`]).
    shown: Option<Transfer>,
    /// The button under the mouse and whether the list is open (037 phase-6): the line
    /// is reborn on every refresh and these two are stamped on it at each birth —
    /// otherwise the 200 ms refresh would overwrite the mouse state.
    hover: Option<TransferAction>,
    list_open: bool,
    /// The percentage last written to the title ([`Self::title_percent_changed`]).
    titled: Option<u8>,
}

/// The queue ended: the result line, its generation and the notification (title,
/// body) to show if bateri is in the background.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Ended {
    pub(crate) line: Transfer,
    pub(crate) serial: u64,
    pub(crate) notice: Option<(String, String)>,
}

/// The answer to a stop request ([`Uploads::stop_request`]).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Stop {
    /// Stop without asking.
    Now { id: Option<u64>, all: bool },
    /// Ask first: the item with `id` has been streaming for over thirty seconds.
    Ask(StopQuestion),
}

/// The stop question (037 phase-7): the sheet's title and text, which item it was
/// asked for (if it finishes meanwhile the sheet closes on its own) and whether it is
/// the whole queue.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct StopQuestion {
    pub(crate) id: u64,
    pub(crate) all: bool,
    pub(crate) title: String,
    pub(crate) text: String,
}

/// The state of a popover row and the left column's second line.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RowStatus {
    /// Streaming: the bar's fraction (`0..=1`) and `18.2 / 96.0 MB · 1.2 MB/s`.
    Running { fraction: f64, detail: String },
    /// Queued: `Waiting · 48.5 MB`.
    Waiting(String),
    /// Finished: `✓ Uploaded · 96.0 MB`.
    Done(String),
}

/// The button on the right of a popover row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RowAction {
    /// Stop the streaming item (asks first if past 30 s).
    Cancel,
    /// Remove the waiting item from the queue (does not ask).
    Remove,
}

impl RowStatus {
    /// The row's button: `Cancel` when streaming, `Remove` when waiting, none when done.
    pub(crate) fn action(&self) -> Option<RowAction> {
        match self {
            Self::Running { .. } => Some(RowAction::Cancel),
            Self::Waiting(_) => Some(RowAction::Remove),
            Self::Done(_) => None,
        }
    }
}

/// A popover row: the name (`static/ · 124 files` for a folder), the state and the
/// dim destination line (`→ /var/www/app`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ListRow {
    pub(crate) id: u64,
    pub(crate) name: String,
    pub(crate) status: RowStatus,
    pub(crate) dest: String,
}

/// The "Show files (N)" popover (037 phase-7): title and rows.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct UploadList {
    pub(crate) title: String,
    pub(crate) rows: Vec<ListRow>,
}

impl Uploads {
    /// Whether a new drop can be accepted.
    ///
    /// Not for a queue that was cancelled but whose streaming item has not finished
    /// yet (its half-written file is being deleted) either: a confirmed drop could not
    /// be added to it and would be silently dropped.
    pub(crate) fn can_accept(&self) -> bool {
        !self.asking
            && self
                .queue
                .as_ref()
                .is_none_or(|queue| queue.ending.is_none())
    }

    /// The probe or the sheet started / ended.
    pub(crate) fn set_asking(&mut self, asking: bool) {
        self.asking = asking;
    }

    /// Whether a drop's probe or confirmation sheet is in progress: the stop question
    /// does not open meanwhile (two sheets cannot be stacked).
    pub(crate) fn asking(&self) -> bool {
        self.asking
    }

    /// Whether a queue is running (⌘.'s gate).
    pub(crate) fn active(&self) -> bool {
        self.queue.is_some()
    }

    /// Whether the queue is streaming: the gate of the sheet's "added to the queue" line.
    pub(crate) fn busy(&self) -> bool {
        self.queue
            .as_ref()
            .is_some_and(|queue| queue.ending.is_none())
    }

    /// The remote session's generation (to tell that ssh closed).
    pub(crate) fn command(&self) -> Option<u64> {
        self.queue.as_ref().map(|queue| queue.command)
    }

    /// Appends the confirmed items to the end of the queue; creates the queue if there
    /// is none. If the queue belongs to **another** remote session (reconnected, the
    /// old one still finishing) the old queue counts as closed and the new one cannot
    /// be set up behind it or in its place — then `false` and the drop is dropped.
    pub(crate) fn enqueue(
        &mut self,
        command: u64,
        ssh: Vec<String>,
        host: String,
        mark: HostMark,
        jobs: Vec<Job>,
    ) -> bool {
        let queue = self.queue.get_or_insert_with(|| Queue {
            command,
            ssh,
            host,
            mark,
            entries: Vec::new(),
            current: None,
            bytes_total: 0,
            bytes_done: 0,
            samples: VecDeque::new(),
            rate: None,
            ending: None,
            skip: false,
        });
        if queue.command != command || queue.ending.is_some() {
            return false;
        }
        for job in jobs {
            self.next_id += 1;
            queue.bytes_total += job.local.bytes;
            queue.entries.push(Item {
                id: self.next_id,
                job,
                state: EntryState::Waiting,
            });
        }
        true
    }

    /// Starts the next item at `now`: the stream thread's inputs. `None` if an item is
    /// already streaming or the queue is ending.
    pub(crate) fn start_next(&mut self, now: Instant) -> Option<(Vec<String>, Job, Arc<Shared>)> {
        let queue = self.queue.as_mut()?;
        if queue.current.is_some() || queue.ending.is_some() {
            return None;
        }
        let entry = queue
            .entries
            .iter_mut()
            .find(|entry| entry.state == EntryState::Waiting)?;
        entry.state = EntryState::Running;
        let shared = Arc::new(Shared::default());
        queue.current = Some(Current {
            id: entry.id,
            shared: Arc::clone(&shared),
            started: now,
        });
        Some((queue.ssh.clone(), entry.job.clone(), shared))
    }

    /// The streaming item's id (the stop sheet's closing question).
    pub(crate) fn running_id(&self) -> Option<u64> {
        self.queue
            .as_ref()?
            .current
            .as_ref()
            .map(|current| current.id)
    }

    /// A stop request (037 phase-7) at `now`: `all` is the whole queue (⌘., the line's
    /// `Cancel`/`Cancel all`, the popover's `Cancel all`), otherwise the streaming item
    /// (the popover row's `Cancel`). In a single-item queue the two are the same. A
    /// question if the streaming item has been running longer than
    /// [`STOP_ASK_AFTER`], otherwise at once; `None` if there is no queue.
    pub(crate) fn stop_request(&self, all: bool, now: Instant) -> Option<Stop> {
        let queue = self.queue.as_ref()?;
        let all = all || queue.entries.len() <= 1;
        let Some(current) = &queue.current else {
            return Some(Stop::Now { id: None, all });
        };
        if now.saturating_duration_since(current.started) <= STOP_ASK_AFTER {
            return Some(Stop::Now {
                id: Some(current.id),
                all,
            });
        }
        let Some((_, entry)) = queue.running_entry() else {
            return Some(Stop::Now { id: None, all });
        };
        let local = &entry.job.local;
        let (sent, _) = current.shared.progress();
        let mut text = format!(
            "{}: {} will be lost.",
            label(local),
            format_of(sent.min(local.bytes), local.bytes)
        );
        if all {
            let waiting = queue.waiting();
            if waiting > 0 {
                // audit: the item count is per drop; it fits in a `u64`.
                let _ = write!(
                    text,
                    " {waiting} waiting {} won't be uploaded.",
                    files_word(waiting as u64)
                );
            }
            let done = queue.done();
            if done > 0 {
                let (noun, verb) = if done == 1 {
                    ("file", "stays")
                } else {
                    ("files", "stay")
                };
                let _ = write!(text, " {done} finished {noun} {verb} on {}.", queue.host);
            }
        }
        let title = if all && queue.entries.len() > 1 {
            "Stop all uploads?"
        } else {
            "Stop uploading?"
        };
        Some(Stop::Ask(StopQuestion {
            id: current.id,
            all,
            title: title.to_owned(),
            text,
        }))
    }

    /// Applies the stop. `id` is the question's (or the request's) item: if that item
    /// finished meanwhile and another is streaming, nothing is done — the question
    /// spoke of losing that item. Without `all` only that item stops and the queue
    /// goes on; its result comes when the item finishes ([`Self::finish`]).
    pub(crate) fn stop(&mut self, id: Option<u64>, all: bool) -> Option<Ended> {
        let queue = self.queue.as_mut()?;
        let running = queue.current.as_ref().map(|current| current.id);
        if id.is_some() && id != running {
            return None;
        }
        // With nothing waiting, stopping the streaming item cancels the queue: the
        // result must say `Cancelled — 1 of 2 uploaded, …`, not the finished items'
        // `✓` (`/code-review`).
        if all || queue.entries.len() <= 1 || queue.waiting() == 0 {
            return self.cancel();
        }
        if let Some(current) = &queue.current {
            queue.skip = true;
            current.shared.cancel();
        }
        None
    }

    /// Cancels the whole queue: the waiting items will not start, the streaming item is
    /// killed and its half-written file deleted; the result comes when the item
    /// finishes. With no streaming item (the stream thread could not be born) the
    /// result is immediate.
    fn cancel(&mut self) -> Option<Ended> {
        let queue = self.queue.as_mut()?;
        if queue.ending.is_none() {
            queue.ending = Some(End::Cancelled);
        }
        match &queue.current {
            Some(current) => {
                current.shared.cancel();
                None
            }
            None => self.end(),
        }
    }

    /// Cancels and abandons the queue (the tab is closing): the streaming item is
    /// killed and its half-written file deleted on the stream thread; the result is
    /// shown to no one.
    pub(crate) fn abandon(&mut self) {
        if let Some(queue) = self.queue.take()
            && let Some(current) = &queue.current
        {
            current.shared.cancel();
        }
    }

    /// The remote session closed: waiting items are cancelled; the streaming item
    /// finishes over its own connection. With no streaming item the result is immediate.
    pub(crate) fn close(&mut self) -> Option<Ended> {
        let queue = self.queue.as_mut()?;
        if queue.ending.is_none() {
            queue.ending = Some(End::Closed);
        }
        if queue.current.is_none() {
            return self.end();
        }
        None
    }

    /// The streaming item finished; the result if the queue finished too. Nothing is
    /// pasted on its own (037 phase-7): an upload can take minutes and the path would be
    /// typed into whatever vim or mysql is open at that moment — the result line says
    /// where it went.
    pub(crate) fn finish(&mut self, outcome: Outcome) -> Option<Ended> {
        let queue = self.queue.as_mut()?;
        let current = queue.current.take()?;
        let (bytes, _) = current.shared.progress();
        let index = queue
            .entries
            .iter()
            .position(|entry| entry.id == current.id)?;
        match outcome {
            Outcome::Done => {
                let entry = &mut queue.entries[index];
                entry.state = EntryState::Done;
                queue.bytes_done += entry.job.local.bytes;
            }
            // An individually stopped item leaves the list; its sent part counts as
            // done, its unsent part leaves the denominator (the bar never goes back).
            Outcome::Cancelled if std::mem::take(&mut queue.skip) && queue.ending.is_none() => {
                let entry = queue.entries.remove(index);
                let sent = bytes.min(entry.job.local.bytes);
                queue.bytes_done += sent;
                queue.bytes_total -= entry.job.local.bytes - sent;
                // If the waiting items were removed meanwhile, the queue ends as a cancel.
                if queue.waiting() == 0 {
                    queue.ending = Some(End::Cancelled);
                }
            }
            Outcome::Cancelled => {
                queue.entries[index].state = EntryState::Waiting;
                queue.bytes_done += bytes;
                queue.ending.get_or_insert(End::Cancelled);
            }
            Outcome::DiskFull => {
                queue.entries[index].state = EntryState::Waiting;
                queue.bytes_done += bytes;
                queue.ending = Some(End::DiskFull);
            }
            Outcome::Failed(reason) => {
                queue.entries[index].state = EntryState::Waiting;
                queue.bytes_done += bytes;
                queue.ending = Some(End::Failed(reason));
            }
        }
        if queue.waiting() == 0 || queue.ending.is_some() {
            self.end()
        } else {
            None
        }
    }

    /// Closes the queue and returns the result line.
    fn end(&mut self) -> Option<Ended> {
        let queue = self.queue.take()?;
        self.serial += 1;
        // ssh closed but the streaming item finished over its own connection and
        // nothing was waiting: everything arrived, the result is a success (`/code-review`).
        let all_done = queue
            .entries
            .iter()
            .all(|entry| entry.state == EntryState::Done);
        let end = match queue.ending {
            Some(End::Closed) if all_done => End::Done,
            ending => ending.unwrap_or(End::Done),
        };
        let tally = Tally::of(&queue.entries);
        let (body, tone, lead) = end_line(&end, &queue.host, &tally);
        let notice = end_notice(&end, &queue.host, &tally);
        Some(Ended {
            line: Transfer {
                host: queue.host,
                mark: queue.mark,
                body,
                tone,
                lead,
                controls: TransferControls::default(),
                progress: None,
            },
            serial: self.serial,
            notice,
        })
    }

    /// The status line last written to the dock — the input of the mouse's button
    /// question (`bt_core::transfer_button_at` reads the same layout as drawing).
    pub(crate) fn shown(&self) -> Option<&Transfer> {
        self.shown.as_ref()
    }

    /// Writes [`Self::shown`]. A line without buttons (a result, or none) drops the
    /// mouse state too: there is no button left under it.
    pub(crate) fn set_shown(&mut self, transfer: Option<Transfer>) {
        if transfer.as_ref().is_none_or(|t| t.controls.items == 0) {
            self.hover = None;
            self.list_open = false;
        }
        self.shown = transfer;
    }

    /// Whether the button under the mouse changed; if so, the stamped line — the one to
    /// write (the caller hands it to the session). Movement staying on the same button
    /// is `None`: no frame is requested.
    pub(crate) fn set_hover(&mut self, hover: Option<TransferAction>) -> Option<Transfer> {
        // On a line without buttons the mouse is over no button.
        let buttons = self.shown.as_ref().is_some_and(|t| t.controls.items > 0);
        let hover = hover.filter(|_| buttons);
        if self.hover == hover {
            return None;
        }
        self.hover = hover;
        self.restamp()
    }

    /// The button under the mouse. Only tests ask: AppKit's cursor rect sets the
    /// pointer, hover is only the fill's tone.
    #[cfg(test)]
    pub(crate) fn hover(&self) -> Option<TransferAction> {
        self.hover
    }

    /// The list opened/closed; if it changed, the stamped line.
    pub(crate) fn set_list_open(&mut self, open: bool) -> Option<Transfer> {
        if self.list_open == open {
            return None;
        }
        self.list_open = open;
        self.restamp()
    }

    /// Refreshes the shown line's button state; `None` on a line without buttons.
    fn restamp(&self) -> Option<Transfer> {
        let mut shown = self.shown.clone()?;
        if shown.controls.items == 0 {
            return None;
        }
        shown.controls.hover = self.hover;
        shown.controls.list_open = self.list_open;
        Some(shown)
    }

    /// The queue's passed and total bytes (the Dock icon's bar); `None` if there is no
    /// queue.
    pub(crate) fn totals(&self) -> Option<(u64, u64)> {
        let queue = self.queue.as_ref()?;
        let running = queue
            .current
            .as_ref()
            .map_or(0, |current| current.shared.progress().0);
        Some((queue.bytes_done + running, queue.bytes_total))
    }

    /// The percentage for the title prefix (037 phase-7): while an item streams, based
    /// on the whole queue's bytes, rounded down; `None` if nothing streams or it is
    /// being cancelled. If ssh closed, the streaming item finishes over its own
    /// connection and the prefix stays with it.
    pub(crate) fn percent(&self) -> Option<u8> {
        let queue = self.queue.as_ref()?;
        if queue.ending == Some(End::Cancelled) || queue.current.is_none() {
            return None;
        }
        let (sent, total) = self.totals()?;
        if total == 0 {
            return Some(0);
        }
        // audit: clamped to `sent ≤ total`; the ratio is 0..=100, fits in a `u8`.
        Some((sent.min(total) as f64 / total as f64 * 100.0).floor() as u8)
    }

    /// Whether the title's percentage differs from the last one written; if so, stores
    /// the new one — the title is written at most once per percent.
    pub(crate) fn title_percent_changed(&mut self) -> bool {
        let percent = self.percent();
        if self.titled == percent {
            return false;
        }
        self.titled = percent;
        true
    }

    /// The percentage last written to the title ([`titled`]'s input).
    pub(crate) fn title_percent(&self) -> Option<u8> {
        self.titled
    }

    /// The wait is over: the line goes away if the result line is still this generation
    /// and no new queue has started.
    pub(crate) fn linger_over(&self, serial: u64) -> bool {
        self.queue.is_none() && self.serial == serial
    }

    /// The popover's content (037 phase-7): all items — finished, streaming and
    /// waiting — in order. `None` if there is no queue or it is ending: the popover
    /// must close.
    pub(crate) fn list(&self) -> Option<UploadList> {
        let queue = self.queue.as_ref()?;
        if queue.ending.is_some() {
            return None;
        }
        let rows = queue
            .entries
            .iter()
            .map(|entry| {
                let local = &entry.job.local;
                let name = if local.dir {
                    format!(
                        "{}/ · {} {}",
                        local.name,
                        local.files,
                        files_word(local.files)
                    )
                } else {
                    local.name.clone()
                };
                let status = match entry.state {
                    EntryState::Done => {
                        RowStatus::Done(format!("✓ Uploaded · {}", format_bytes(local.bytes)))
                    }
                    EntryState::Waiting => {
                        RowStatus::Waiting(format!("Waiting · {}", format_bytes(local.bytes)))
                    }
                    EntryState::Running => {
                        let sent = queue
                            .current
                            .as_ref()
                            .map_or(0, |current| current.shared.progress().0)
                            .min(local.bytes);
                        let mut detail = format_pair(sent, local.bytes);
                        if let Some(rate) = queue.rate {
                            detail.push_str(" · ");
                            detail.push_str(&format_rate(rate));
                        }
                        let fraction = if local.bytes == 0 {
                            0.0
                        } else {
                            sent as f64 / local.bytes as f64
                        };
                        RowStatus::Running { fraction, detail }
                    }
                };
                ListRow {
                    id: entry.id,
                    name,
                    status,
                    dest: format!("→ {}", entry.job.dir),
                }
            })
            .collect();
        Some(UploadList {
            title: format!("Uploading to {}", queue.host),
            rows,
        })
    }

    /// Removes the waiting item with `id` from the queue (the popover's `Remove`); a
    /// no-op for a streaming or finished item.
    pub(crate) fn remove(&mut self, id: u64) {
        let Some(queue) = &mut self.queue else {
            return;
        };
        if let Some(index) = queue
            .entries
            .iter()
            .position(|entry| entry.id == id && entry.state == EntryState::Waiting)
        {
            let entry = queue.entries.remove(index);
            queue.bytes_total -= entry.job.local.bytes;
        }
    }

    /// The dock's status line at `now`; `None` if there is no queue.
    pub(crate) fn status(&mut self, now: Instant) -> Option<Transfer> {
        let queue = self.queue.as_mut()?;
        let current = queue.current.as_ref()?;
        let (bytes, files) = current.shared.progress();
        let sent = queue.bytes_done + bytes;
        queue.samples.push_back((now, sent));
        while queue
            .samples
            .front()
            .is_some_and(|(at, _)| now.duration_since(*at) > SPEED_WINDOW)
        {
            queue.samples.pop_front();
        }
        let rate = match (queue.samples.front(), queue.samples.back()) {
            (Some(&(first, from)), Some(&(last, to)))
                if last.duration_since(first) >= Duration::from_millis(500) && to > from =>
            {
                Some((to - from) as f64 / last.duration_since(first).as_secs_f64())
            }
            _ => None,
        };
        queue.rate = rate;
        let (index, entry) = queue.running_entry()?;
        let local = &entry.job.local;
        let items = queue.entries.len();
        let mut body = String::from("↑ ");
        if items > 1 {
            let _ = write!(body, "{} of {} · ", index + 1, items);
        }
        body.push_str(&local.name);
        if local.dir {
            body.push('/');
        }
        // The folder's file count next to the name: the item's own information, while
        // the bytes are the queue's (the demo the user approved, after 037 phase-7).
        if local.dir {
            let _ = write!(
                body,
                " · {} of {} {}",
                files.min(local.files),
                local.files,
                files_word(local.files)
            );
        }
        // Bytes, speed and remaining time are **the whole queue's**: the bar fills by
        // the queue too, and if the text stated another ratio the two would contradict.
        let total = queue.bytes_total;
        let shown = sent.min(total);
        body.push_str("  ");
        body.push_str(&format_pair(shown, total));
        if let Some(rate) = rate {
            body.push_str(" · ");
            body.push_str(&format_rate(rate));
            let left = total.saturating_sub(shown) as f64 / rate;
            // audit: remaining time rounds to seconds; no infinity/NaN (`rate > 0`).
            body.push_str(" · ");
            body.push_str(&format_duration(left.ceil() as u64));
        }
        let progress = if queue.bytes_total == 0 {
            0
        } else {
            // audit: clamped to `sent ≤ bytes_total`; ratio in ten-thousandths, fits u16.
            (sent.min(queue.bytes_total) as f64 / queue.bytes_total as f64 * 10_000.0) as u16
        };
        Some(Transfer {
            host: queue.host.clone(),
            mark: queue.mark,
            body,
            controls: TransferControls {
                // The number the popover shows ([`Self::list`]): finished, streaming and
                // waiting items.
                // audit: queue items come per drop; clamped to `u16`.
                items: u16::try_from(items).unwrap_or(u16::MAX),
                list_open: self.list_open,
                hover: self.hover,
            },
            progress: Some(progress),
            ..Transfer::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(argv: &[&str]) -> Vec<String> {
        argv.iter().map(|&arg| arg.to_owned()).collect()
    }

    fn ssh_target(argv: &[&str]) -> RemoteTarget {
        RemoteTarget {
            host: "prod".to_owned(),
            kind: RemoteKind::Ssh,
            argv: words(argv),
            line: String::new(),
        }
    }

    const OURS: [&str; 6] = ["ssh", "-T", "-o", "BatchMode=yes", "-o", "ControlMaster=no"];

    fn with_ours(rest: &[&str]) -> Vec<String> {
        words(&OURS).into_iter().chain(words(rest)).collect()
    }

    #[test]
    fn the_upload_keeps_the_connection_options_and_drops_the_rest() {
        let target = ssh_target(&[
            "ssh", "-p", "2222", "-l", "deploy", "-J", "jump", "-i", "k", "prod",
        ]);
        assert_eq!(
            ssh_argv(&target),
            with_ours(&[
                "-p", "2222", "-l", "deploy", "-J", "jump", "-i", "k", "prod"
            ])
        );
        // tty, noise and the remote command are dropped; ours come first, because ssh
        // takes a key's first value.
        let target = ssh_target(&[
            "ssh",
            "-tv",
            "-o",
            "RequestTTY=force",
            "prod",
            "tmux",
            "attach",
        ]);
        assert_eq!(
            ssh_argv(&target),
            with_ours(&["-o", "RequestTTY=force", "prod"])
        );
        // An attached value and a kept flag in a cluster; an option after the destination.
        let target = ssh_target(&["/opt/ssh", "-Ap2222", "prod", "-i", "k"]);
        let mut expected = with_ours(&["-A", "-p", "2222", "-i", "k", "prod"]);
        expected[0] = "/opt/ssh".to_owned();
        assert_eq!(ssh_argv(&target), expected);
        // What follows `--` is the destination.
        let target = ssh_target(&["ssh", "-C", "--", "deploy@prod"]);
        assert_eq!(ssh_argv(&target), with_ours(&["-C", "deploy@prod"]));
    }

    #[test]
    fn mosh_uploads_through_plain_ssh_to_the_host() {
        let target = RemoteTarget {
            host: "deploy@prod".to_owned(),
            kind: RemoteKind::Mosh,
            argv: words(&["mosh", "--ssh=ssh -p 2222", "deploy@prod"]),
            line: String::new(),
        };
        assert_eq!(ssh_argv(&target), with_ours(&["deploy@prod"]));
    }

    #[test]
    fn single_quotes_never_produce_a_backslash() {
        assert_eq!(sq("plain"), "'plain'");
        assert_eq!(sq("it's"), "'it'\"'\"'s'");
        assert!(!remote_command(&probe_script(Some("/a'b"), &["c'd".into()])).contains('\\'));
    }

    /// Runs the script **in two layers**: the login shell (`shell -c`) and the `sh -c`
    /// it opens — the local equivalent of what ssh does remotely.
    fn run_remote(shell: &str, script: &str, cwd: &Path) -> (Option<i32>, String) {
        let output = Command::new(shell)
            .arg("-c")
            .arg(remote_command(script))
            .current_dir(cwd)
            .output()
            .expect("shell did not run");
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
        )
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bt-upload-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp directory");
        dir
    }

    #[test]
    fn the_probe_runs_under_every_login_shell_and_reports_what_is_there() {
        let dir = scratch("probe");
        let target = dir.join("it's \"odd\" $HOME");
        std::fs::create_dir_all(target.join("static")).unwrap();
        std::fs::write(target.join("a b.txt"), "x").unwrap();
        let names: Vec<String> = ["a b.txt", "static", "new", "-dash"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let script = probe_script(Some(target.to_str().unwrap()), &names);
        for shell in ["/bin/sh", "/bin/bash", "/bin/zsh"] {
            let (code, out) = run_remote(shell, &format!("echo noise; {script}"), &dir);
            assert_eq!(code, Some(0), "{shell}: {out}");
            let reply = parse_probe(&out).unwrap_or_else(|| panic!("{shell}: {out}"));
            // `pwd` may not resolve the symbolic link (`/var` → `/private/var`).
            assert!(
                reply.dir.ends_with("it's \"odd\" $HOME"),
                "{shell}: {}",
                reply.dir
            );
            assert!(reply.tar, "{shell}");
            assert!(reply.free.is_some_and(|free| free > 0), "{shell}");
            assert_eq!(reply.existing, [(0, false), (1, true)], "{shell}");
        }
        // If the directory is missing the script exits with a separate code.
        let script = probe_script(Some(dir.join("gone").to_str().unwrap()), &names);
        let (code, _) = run_remote("/bin/sh", &script, &dir);
        assert_eq!(code, Some(NO_DIRECTORY));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_probe_reply_skips_noise_and_reads_df_from_the_right() {
        let out = "Welcome!\nBT-UPLOAD\n/srv/app\nBT-DF /dev/disk 1 k 100 12000 50% /Volumes/My Disk\nBT-TAR\nBT-E1\nBT-D1\nBT-E3\n";
        assert_eq!(
            parse_probe(out),
            Some(ProbeReply {
                dir: "/srv/app".into(),
                free: Some(12_000 * 1024),
                tar: true,
                existing: vec![(1, true), (3, false)],
            })
        );
        assert_eq!(parse_probe("no mark\n"), None);
        // If `df` printed nothing, the next mark is read in its own place.
        let reply = parse_probe("BT-UPLOAD\n/srv\nBT-TAR\nBT-E0\n").unwrap();
        assert_eq!((reply.free, reply.tar), (None, true));
        assert_eq!(reply.existing, [(0, false)]);
        assert_eq!(df_available("garbage"), None);
    }

    #[test]
    fn names_with_a_backslash_or_control_character_are_refused() {
        assert!(is_safe("it's \"fine\" $x"));
        assert!(!is_safe("a\\b"));
        assert!(!is_safe("a\nb"));
    }

    fn tar_of(dir: &Path, name: &str) -> Vec<u8> {
        let output = Command::new("/usr/bin/tar")
            .args([
                "-c",
                "-f",
                "-",
                "--no-mac-metadata",
                "--no-xattrs",
                "--no-acls",
                "-C",
            ])
            .arg(dir)
            .arg(format!("./{name}"))
            .env("COPYFILE_DISABLE", "1")
            .output()
            .expect("tar did not run");
        assert!(output.status.success());
        output.stdout
    }

    #[test]
    fn the_watcher_counts_files_and_content_bytes_through_pax_headers() {
        let dir = scratch("watch");
        let tree = dir.join("static");
        std::fs::create_dir_all(tree.join("css")).unwrap();
        std::fs::write(tree.join("app.js"), vec![b'a'; 1000]).unwrap();
        std::fs::write(tree.join("empty"), b"").unwrap();
        // A long, non-ASCII name: bsdtar puts a pax header in front of it.
        let long = format!("ç{}.txt", "x".repeat(150));
        std::fs::write(tree.join("css").join(&long), vec![b'b'; 700]).unwrap();
        std::os::unix::fs::symlink("app.js", tree.join("link")).unwrap();
        let stream = tar_of(&dir, "static");
        let local = measure(&tree).unwrap();
        assert_eq!((local.files, local.bytes, local.dir), (3, 1700, true));

        // The same result in one gulp and byte by byte.
        for chunk in [stream.len(), 1, 511, 513] {
            let mut watcher = TarWatcher::default();
            for part in stream.chunks(chunk) {
                watcher.feed(part);
            }
            assert_eq!((watcher.files, watcher.bytes), (3, 1700), "chunk {chunk}");
            assert_eq!(watcher.current, None);
        }
        // A stream cut in the middle of the long-named file: the half-written file
        // carries pax's path.
        let at = stream
            .windows(700)
            .position(|window| window.iter().all(|&b| b == b'b'))
            .unwrap();
        let mut watcher = TarWatcher::default();
        watcher.feed(&stream[..at + 10]);
        assert_eq!(
            watcher.current.as_deref(),
            Some(format!("./static/css/{long}").as_str())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tar_sizes_read_octal_and_base_256() {
        assert_eq!(tar_size(b"00000001750\0"), 1000);
        assert_eq!(tar_size(b"     1750 \0\0"), 1000);
        let mut big = [0u8; 12];
        big[0] = 0x80;
        big[11] = 0x01;
        big[10] = 0x02;
        assert_eq!(tar_size(&big), 0x0201);
        assert_eq!(
            pax_path(b"12 path=a/b\n20 mtime=1234567.5\n"),
            Some("a/b".into())
        );
        assert_eq!(pax_path(b"garbage"), None);
    }

    #[test]
    fn sizes_rates_and_durations_read_like_finder() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(18_200_000), "18.2 MB");
        assert_eq!(format_bytes(3_400_000_000), "3.4 GB");
        assert_eq!(format_pair(18_200_000, 44_600_000), "18.2 / 44.6 MB");
        assert_eq!(format_pair(500_000, 44_600_000), "0.5 / 44.6 MB");
        assert_eq!(format_pair(10, 900), "10 / 900 B");
        assert_eq!(format_rate(1_200_000.0), "1.2 MB/s");
        assert_eq!(format_rate(800.0), "800 B/s");
        assert_eq!(format_duration(22), "22s");
        assert_eq!(format_duration(65), "1m 05s");
        assert_eq!(format_duration(3720), "1h 02m");
    }

    fn entries(items: &[(&str, bool, &str, EntryState)]) -> Vec<Item> {
        items
            .iter()
            .enumerate()
            .map(|(id, &(name, dir, at, state))| Item {
                id: id as u64,
                job: Job {
                    local: local(name, dir, 1, 1),
                    dir: at.into(),
                },
                state,
            })
            .collect()
    }

    #[test]
    fn the_end_line_says_what_went_where() {
        use EntryState::{Done, Waiting};
        let line = |end: &End, items: &[(&str, bool, &str, EntryState)]| {
            end_line(end, "prod-web-1", &Tally::of(&entries(items)))
        };
        let one = [("backup.tar.gz", false, "/var/www/app", Done)];
        assert_eq!(
            line(&End::Done, &one),
            (
                "✓ backup.tar.gz → /var/www/app".into(),
                TransferTone::Success,
                30
            )
        );
        let folder = [("static", true, "/var/www/app", Done)];
        assert_eq!(line(&End::Done, &folder).0, "✓ static/ → /var/www/app");
        let three = [
            ("a", false, "/var/www/app", Done),
            ("b", false, "/var/www/app", Done),
            ("c", true, "/var/www/app", Done),
        ];
        assert_eq!(line(&End::Done, &three).0, "✓ 3 files → /var/www/app");
        let apart = [
            ("a", false, "/var/www/app", Done),
            ("b", false, "/srv", Done),
        ];
        assert_eq!(line(&End::Done, &apart).0, "✓ 2 files uploaded");

        let (body, tone, _) = line(&End::Cancelled, &[("a", false, "/srv", Waiting)]);
        assert_eq!(
            (body.as_str(), tone),
            ("Cancelled — partial file removed", TransferTone::Quiet)
        );
        let two = [("a", false, "/srv", Done), ("b", false, "/srv", Waiting)];
        assert_eq!(
            line(&End::Cancelled, &two).0,
            "Cancelled — 1 of 2 uploaded, partial file removed"
        );

        let none = [
            ("a", false, "/srv", Waiting),
            ("b", false, "/srv", Waiting),
            ("c", false, "/srv", Waiting),
        ];
        let (body, tone, lead) = line(&End::DiskFull, &none);
        assert_eq!(body, "Failed — disk full on prod-web-1 · 0 of 3 uploaded");
        assert_eq!(tone, TransferTone::Error);
        assert_eq!(
            body.chars().take(lead).collect::<String>(),
            "Failed — disk full on prod-web-1",
            "only the error text is red"
        );
        assert_eq!(
            line(&End::Closed, &two).0,
            "Failed — connection to prod-web-1 lost · 1 of 2 uploaded"
        );
        assert_eq!(
            line(&End::Failed("tar: Permission denied".into()), &one).0,
            "Failed — tar: Permission denied · 1 of 1 uploaded"
        );
    }

    #[test]
    fn a_notice_is_sent_for_success_and_failure_but_not_for_a_cancel() {
        use EntryState::{Done, Waiting};
        let notice = |end: &End, items: &[(&str, bool, &str, EntryState)]| {
            end_notice(end, "prod-web-1", &Tally::of(&entries(items)))
        };
        let three = [
            ("a", false, "/var/www/app", Done),
            ("b", false, "/var/www/app", Done),
            ("c", false, "/var/www/app", Done),
        ];
        assert_eq!(
            notice(&End::Done, &three),
            Some((
                "3 files uploaded".into(),
                "to prod-web-1:/var/www/app".into()
            ))
        );
        let one = [("backup.tar.gz", false, "/srv", Done)];
        assert_eq!(
            notice(&End::Done, &one).map(|(title, _)| title).as_deref(),
            Some("backup.tar.gz uploaded")
        );
        let apart = [
            ("a", false, "/var/www/app", Done),
            ("b", false, "/srv", Done),
        ];
        assert_eq!(
            notice(&End::Done, &apart).map(|(_, body)| body).as_deref(),
            Some("to prod-web-1")
        );
        let none = [
            ("a", false, "/srv", Waiting),
            ("b", false, "/srv", Waiting),
            ("c", false, "/srv", Waiting),
        ];
        assert_eq!(
            notice(&End::DiskFull, &none),
            Some((
                "Upload failed".into(),
                "Disk full on prod-web-1. 0 of 3 uploaded.".into()
            ))
        );
        assert_eq!(
            notice(&End::Closed, &none).map(|(_, body)| body).as_deref(),
            Some("Connection to prod-web-1 lost. 0 of 3 uploaded.")
        );
        assert_eq!(notice(&End::Cancelled, &none), None, "cancel is the user's");
    }

    #[test]
    fn the_title_carries_the_percent_only_while_uploading() {
        assert_eq!(titled(Some(64), "⇄ prod-web-1"), "↑ 64% · ⇄ prod-web-1");
        assert_eq!(titled(None, "⇄ prod-web-1"), "⇄ prod-web-1");

        let mut uploads = queue_of(vec![job("a", false, 1, 100), job("b", false, 1, 100)]);
        assert_eq!(uploads.percent(), None, "not streaming yet");
        let (_, _, shared) = uploads.start_next(Instant::now()).unwrap();
        assert!(uploads.title_percent_changed(), "0% the first time");
        assert_eq!(uploads.title_percent(), Some(0));
        shared.bytes.store(1, Ordering::Release);
        assert!(!uploads.title_percent_changed(), "once per percent");
        shared.bytes.store(129, Ordering::Release);
        assert!(uploads.title_percent_changed());
        assert_eq!(uploads.title_percent(), Some(64));
        let _ = uploads.stop(None, true);
        assert!(
            uploads.title_percent_changed(),
            "cancelling: prefix removed"
        );
        assert_eq!(uploads.title_percent(), None);
    }

    fn local(name: &str, dir: bool, files: u64, bytes: u64) -> Local {
        Local {
            path: PathBuf::from("/Users/me").join(name),
            name: name.to_owned(),
            dir,
            files,
            bytes,
        }
    }

    fn reply(existing: Vec<(usize, bool)>, free: Option<u64>) -> ProbeReply {
        ProbeReply {
            dir: "/var/www/app".into(),
            free,
            tar: true,
            existing,
        }
    }

    #[test]
    fn the_sheet_names_the_target_and_the_button_follows_what_exists() {
        let file = [local("backup.tar.gz", false, 1, 96_000_000)];
        let fresh = sheet("prod-web-1", true, &file, &reply(vec![], None), false);
        assert_eq!(fresh.message, "Upload “backup.tar.gz” to prod-web-1?");
        assert_eq!(fresh.informative, "96.0 MB → /var/www/app");
        assert_eq!((fresh.button, fresh.enabled), ("Upload", true));

        // A drop arriving while an upload runs: the upload does not stop.
        let busy = sheet("prod", true, &file, &reply(vec![], None), true);
        assert_eq!(
            busy.informative,
            "96.0 MB → /var/www/app\nAdded to the queue; the current upload keeps going."
        );

        let replace = sheet("prod", true, &file, &reply(vec![(0, false)], None), false);
        assert_eq!(replace.button, "Replace");
        assert!(
            replace.informative.contains(
                "A file named “backup.tar.gz” already exists there: it will be replaced."
            )
        );

        let folder = [local("static", true, 124, 38_200_000)];
        let merge = sheet(
            "prod",
            false,
            &folder,
            &reply(vec![(0, true)], Some(12_000_000)),
            false,
        );
        assert_eq!(merge.message, "Upload folder “static” to prod?");
        assert_eq!(merge.button, "Merge");
        assert!(!merge.enabled, "not enough space");
        assert_eq!(
            merge.informative,
            "124 files, 38.2 MB → /var/www/app\n\
             That is the home folder, because the remote shell has not reported its folder.\n\
             A folder named “static” already exists there: same-named files are replaced, \
             others are kept.\n\
             prod has 12.0 MB free, “static” needs 38.2 MB."
        );

        // Several items in one drop on a single sheet: total, destination and names.
        let three = [
            local("dump.sql", false, 1, 32_300_000),
            local("logs.tar", false, 1, 20_100_000),
            local("README.md", false, 1, 100_000),
        ];
        let many = sheet("prod-web-1", true, &three, &reply(vec![], None), false);
        assert_eq!(many.message, "Upload 3 items to prod-web-1?");
        assert_eq!(
            many.informative,
            "52.5 MB → /var/www/app\ndump.sql\nlogs.tar\nREADME.md"
        );

        let two = [local("a", false, 1, 10), local("b", true, 2, 20)];
        let mut no_tar = reply(vec![(0, false), (1, true)], None);
        no_tar.tar = false;
        let refused = sheet("prod", true, &two, &no_tar, false);
        assert_eq!(refused.message, "Upload 2 items to prod?");
        assert!(!refused.enabled);
        assert!(refused.informative.contains("2 items already exist there"));
        assert!(refused.informative.contains("tar is not installed on prod"));

        let odd = [local("a\\b", false, 1, 1)];
        assert!(!sheet("prod", true, &odd, &reply(vec![], None), false).enabled);
    }

    fn job(name: &str, dir: bool, files: u64, bytes: u64) -> Job {
        Job {
            local: local(name, dir, files, bytes),
            dir: "/srv".into(),
        }
    }

    fn queue_of(jobs: Vec<Job>) -> Uploads {
        let mut uploads = Uploads::default();
        assert!(uploads.enqueue(
            7,
            words(&["ssh", "prod"]),
            "prod".into(),
            HostMark::Production,
            jobs
        ));
        uploads
    }

    #[test]
    fn the_queue_runs_in_order_and_pastes_nothing() {
        let mut uploads = queue_of(vec![
            job("backup.tar.gz", false, 1, 44_600_000),
            job("static", true, 124, 1_000_000),
        ]);
        let start = Instant::now();
        let (_, first, shared) = uploads.start_next(start).expect("first item");
        assert_eq!(first.local.name, "backup.tar.gz");
        assert!(uploads.start_next(start).is_none(), "sequential only");

        shared.bytes.store(18_200_000, Ordering::Release);
        let status = uploads.status(start).expect("line");
        assert_eq!(status.body, "↑ 1 of 2 · backup.tar.gz  18.2 / 45.6 MB");
        assert_eq!(
            status.controls,
            TransferControls {
                items: 2,
                ..TransferControls::default()
            },
        );
        assert_eq!(status.mark, HostMark::Production);
        // The bar is by the whole queue's bytes: 18.2 / (44.6 + 1.0).
        assert_eq!(status.progress, Some(3_991));
        shared.bytes.store(19_400_000, Ordering::Release);
        let status = uploads.status(start + Duration::from_secs(1)).unwrap();
        assert_eq!(
            status.body,
            "↑ 1 of 2 · backup.tar.gz  19.4 / 45.6 MB · 1.2 MB/s · 22s"
        );

        // A finished item does not end the queue and no path is pasted —
        // `finish`'s answer has nothing to paste, only the result.
        assert_eq!(uploads.finish(Outcome::Done), None);
        let (_, second, shared) = uploads.start_next(start).expect("second item");
        assert_eq!(second.local.name, "static");
        shared.files.store(57, Ordering::Release);
        let status = uploads.status(start + Duration::from_secs(2)).unwrap();
        assert!(
            status
                .body
                .starts_with("↑ 2 of 2 · static/ · 57 of 124 files  "),
            "{}",
            status.body
        );
        assert!(status.body.contains(" / 45.6 MB"), "{}", status.body);
        // A finished item stays in the count: "Show files (2)".
        assert_eq!(status.controls.items, 2);

        let ended = uploads.finish(Outcome::Done).expect("queue ended");
        assert_eq!(ended.line.body, "✓ 2 files → /srv");
        assert_eq!(ended.line.tone, TransferTone::Success);
        assert_eq!(
            (ended.line.progress, ended.line.controls),
            (None, TransferControls::default()),
            "result line has no buttons"
        );
        assert_eq!(
            ended.notice,
            Some(("2 files uploaded".into(), "to prod:/srv".into()))
        );
        assert!(!uploads.active());
        assert!(uploads.linger_over(ended.serial));
    }

    #[test]
    fn cancelling_keeps_the_count_and_removes_the_partial_file() {
        let mut uploads = queue_of(vec![job("a", false, 1, 1_000), job("b", false, 1, 1)]);
        let (_, _, shared) = uploads.start_next(Instant::now()).unwrap();
        assert_eq!(uploads.stop(None, true), None, "result when item ends");
        assert!(shared.cancel.load(Ordering::Acquire));
        let ended = uploads.finish(Outcome::Cancelled).expect("ended");
        assert_eq!(
            ended.line.body,
            "Cancelled — 0 of 2 uploaded, partial file removed"
        );
        assert_eq!(ended.notice, None, "no notice on cancel");
    }

    #[test]
    fn a_closed_connection_finishes_the_current_item_and_says_it_failed() {
        let mut uploads = queue_of(vec![job("a", false, 1, 1), job("b", false, 1, 1)]);
        let _ = uploads.start_next(Instant::now()).unwrap();
        assert_eq!(
            uploads.close(),
            None,
            "streaming item ends over its own link"
        );
        let ended = uploads.finish(Outcome::Done).expect("ended");
        assert_eq!(
            ended.line.body,
            "Failed — connection to prod lost · 1 of 2 uploaded"
        );
        assert_eq!(ended.line.tone, TransferTone::Error);
        // Nothing was waiting and the streaming item finished over its own connection: success.
        let mut uploads = queue_of(vec![job("a", false, 1, 1)]);
        let _ = uploads.start_next(Instant::now()).unwrap();
        assert_eq!(uploads.close(), None);
        let ended = uploads.finish(Outcome::Done).expect("ended");
        assert_eq!(ended.line.body, "✓ a → /srv");
        assert_eq!(ended.line.tone, TransferTone::Success);
        // With no streaming item the result is immediate.
        let mut uploads = queue_of(vec![job("a", false, 1, 1)]);
        assert!(uploads.close().is_some());
        // A new drop does not enter a closed queue.
        let mut uploads = queue_of(vec![job("a", false, 1, 1)]);
        assert!(!uploads.enqueue(
            8,
            vec![],
            "prod".into(),
            HostMark::None,
            vec![job("b", false, 1, 1)]
        ));
    }

    #[test]
    fn the_popover_lists_every_item_with_its_state_destination_and_button() {
        let mut uploads = queue_of(vec![
            job("backup.tar.gz", false, 1, 96_000_000),
            job("static", true, 124, 38_200_000),
            job("photos.zip", false, 1, 48_500_000),
        ]);
        let start = Instant::now();
        let _ = uploads.start_next(start).unwrap();
        assert_eq!(uploads.finish(Outcome::Done), None);
        let (_, _, shared) = uploads.start_next(start).unwrap();
        shared.bytes.store(18_200_000, Ordering::Release);
        let _ = uploads.status(start);
        shared.bytes.store(19_400_000, Ordering::Release);
        let _ = uploads.status(start + Duration::from_secs(1));

        let list = uploads.list().expect("list");
        assert_eq!(list.title, "Uploading to prod");
        let rows: Vec<(&str, &str)> = list
            .rows
            .iter()
            .map(|row| (row.name.as_str(), row.dest.as_str()))
            .collect();
        assert_eq!(
            rows,
            [
                ("backup.tar.gz", "→ /srv"),
                ("static/ · 124 files", "→ /srv"),
                ("photos.zip", "→ /srv")
            ]
        );
        assert_eq!(
            list.rows[0].status,
            RowStatus::Done("✓ Uploaded · 96.0 MB".into())
        );
        let RowStatus::Running { fraction, detail } = &list.rows[1].status else {
            panic!("streaming item");
        };
        assert_eq!(detail, "19.4 / 38.2 MB · 1.2 MB/s");
        assert!((fraction - 19.4 / 38.2).abs() < 1e-9);
        assert_eq!(
            list.rows[2].status,
            RowStatus::Waiting("Waiting · 48.5 MB".into())
        );
        let actions: Vec<_> = list.rows.iter().map(|row| row.status.action()).collect();
        assert_eq!(
            actions,
            [None, Some(RowAction::Cancel), Some(RowAction::Remove)],
            "no button when done"
        );

        // `Remove` takes the waiting item off without asking and the count drops.
        uploads.remove(list.rows[2].id);
        assert_eq!(uploads.list().unwrap().rows.len(), 2);
        assert_eq!(uploads.status(start).unwrap().controls.items, 2);
        // `remove` is a no-op on a streaming or finished item (`stop` stops the streaming one).
        uploads.remove(list.rows[0].id);
        uploads.remove(list.rows[1].id);
        assert_eq!(uploads.list().unwrap().rows.len(), 2);
    }

    #[test]
    fn stopping_one_item_takes_it_off_the_list_and_the_queue_goes_on() {
        let mut uploads = queue_of(vec![
            job("a", false, 1, 10),
            job("b", false, 1, 20),
            job("c", false, 1, 30),
        ]);
        let now = Instant::now();
        let _ = uploads.start_next(now).unwrap();
        let first = uploads.running_id();
        assert_eq!(uploads.stop(first, false), None);
        assert_eq!(uploads.finish(Outcome::Cancelled), None, "queue goes on");
        let (_, next, _) = uploads.start_next(now).expect("next");
        assert_eq!(next.local.name, "b");
        assert_eq!(
            uploads.list().unwrap().rows.len(),
            2,
            "the stopped one left the list"
        );
        assert_eq!(uploads.finish(Outcome::Done), None);
        let _ = uploads.start_next(now).unwrap();
        let ended = uploads.finish(Outcome::Done).unwrap();
        assert_eq!(ended.line.body, "✓ 2 files → /srv");

        // If the question was for an item that finished meanwhile, the stop touches no other.
        let mut uploads = queue_of(vec![job("a", false, 1, 10), job("b", false, 1, 20)]);
        let _ = uploads.start_next(now).unwrap();
        let asked = uploads.running_id();
        assert_eq!(uploads.finish(Outcome::Done), None);
        let (_, _, shared) = uploads.start_next(now).unwrap();
        assert_eq!(uploads.stop(asked, true), None);
        assert!(!shared.cancel.load(Ordering::Acquire), "new item goes on");

        // With nothing left waiting, stopping the streaming item cancels the queue: the
        // finished item does not say `✓`.
        let mut uploads = queue_of(vec![job("a", false, 1, 10), job("b", false, 1, 20)]);
        let _ = uploads.start_next(now).unwrap();
        assert_eq!(uploads.finish(Outcome::Done), None);
        let _ = uploads.start_next(now).unwrap();
        let last = uploads.running_id();
        assert_eq!(uploads.stop(last, false), None);
        let ended = uploads.finish(Outcome::Cancelled).unwrap();
        assert_eq!(
            ended.line.body,
            "Cancelled — 1 of 2 uploaded, partial file removed"
        );
        assert_eq!(ended.notice, None);

        // With a single item, the line's stop cancels the queue.
        let mut uploads = queue_of(vec![job("a", false, 1, 10)]);
        let _ = uploads.start_next(now).unwrap();
        let only = uploads.running_id();
        assert_eq!(uploads.stop(only, false), None);
        assert_eq!(
            uploads.finish(Outcome::Cancelled).unwrap().line.body,
            "Cancelled — partial file removed"
        );
    }

    #[test]
    fn a_long_upload_asks_before_it_stops() {
        let mut uploads = queue_of(vec![
            job("done.txt", false, 1, 10),
            job("backup.tar.gz", false, 1, 96_000_000),
            job("photos.zip", false, 1, 48_500_000),
        ]);
        let start = Instant::now();
        let _ = uploads.start_next(start).unwrap();
        assert_eq!(uploads.finish(Outcome::Done), None);
        let (_, _, shared) = uploads.start_next(start).unwrap();
        let id = uploads.running_id();
        shared.bytes.store(48_000_000, Ordering::Release);

        // The threshold is a design constant; below it there is no question.
        let short = start + STOP_ASK_AFTER;
        assert_eq!(
            uploads.stop_request(true, short),
            Some(Stop::Now { id, all: true })
        );
        let long = start + STOP_ASK_AFTER + Duration::from_secs(1);
        assert_eq!(
            uploads.stop_request(true, long),
            Some(Stop::Ask(StopQuestion {
                id: id.unwrap(),
                all: true,
                title: "Stop all uploads?".into(),
                text: "backup.tar.gz: 48.0 of 96.0 MB will be lost. 1 waiting file won't \
                       be uploaded. 1 finished file stays on prod."
                    .into(),
            }))
        );
        // The popover row's `Cancel` is only that item: waiting and finished items are
        // not counted.
        let Some(Stop::Ask(one)) = uploads.stop_request(false, long) else {
            panic!("question");
        };
        assert_eq!(one.title, "Stop uploading?");
        assert_eq!(one.text, "backup.tar.gz: 48.0 of 96.0 MB will be lost.");
        assert!(!one.all);
        // Removing a waiting item never asks (`remove`, not `stop_request`) —
        // `the_popover_lists_every_item…`.
    }

    #[test]
    fn a_stopping_queue_refuses_new_drops_and_a_closing_tab_drops_the_queue() {
        let mut uploads = queue_of(vec![job("a", false, 1, 10)]);
        let (_, _, shared) = uploads.start_next(Instant::now()).unwrap();
        assert!(uploads.can_accept());
        assert!(uploads.busy());
        assert_eq!(uploads.stop(None, true), None);
        assert!(!uploads.can_accept(), "deleting partial file");
        assert_eq!(uploads.list(), None, "popover closes");
        uploads.abandon();
        assert!(!uploads.active());
        assert!(shared.cancel.load(Ordering::Acquire));
        assert!(uploads.can_accept());
    }

    #[test]
    fn a_new_result_outlives_the_old_linger() {
        let mut uploads = queue_of(vec![job("a", false, 1, 1)]);
        let _ = uploads.start_next(Instant::now()).unwrap();
        let first = uploads.finish(Outcome::Done).unwrap().serial;
        assert!(uploads.enqueue(
            7,
            vec![],
            "prod".into(),
            HostMark::None,
            vec![job("b", false, 1, 1)]
        ));
        assert!(!uploads.linger_over(first), "new queue running");
        let _ = uploads.start_next(Instant::now()).unwrap();
        let second = uploads.finish(Outcome::Done).unwrap().serial;
        assert!(
            !uploads.linger_over(first),
            "old linger must not clear new result"
        );
        assert!(uploads.linger_over(second));
    }

    #[test]
    fn the_pointer_state_survives_the_refresh_and_changes_only_on_edges() {
        let mut uploads = queue_of(vec![job("a", false, 1, 10), job("b", false, 1, 10)]);
        let now = Instant::now();
        let (_, _, _shared) = uploads.start_next(now).expect("first item");
        let status = uploads.status(now).expect("line");
        uploads.set_shown(Some(status));
        let hovered = uploads
            .set_hover(Some(TransferAction::Cancel))
            .expect("changed");
        assert_eq!(hovered.controls.hover, Some(TransferAction::Cancel));
        uploads.set_shown(Some(hovered));
        assert_eq!(
            uploads.set_hover(Some(TransferAction::Cancel)),
            None,
            "moving on the same button requests no frame"
        );
        // The 200 ms refresh does not overwrite the mouse state.
        let status = uploads.status(now).expect("line");
        assert_eq!(status.controls.hover, Some(TransferAction::Cancel));
        let open = uploads.set_list_open(true).expect("changed");
        assert!(open.controls.list_open);
        // A line without buttons (a result) drops the mouse state.
        uploads.set_shown(Some(Transfer::default()));
        assert_eq!(uploads.hover(), None);
        assert_eq!(
            uploads.set_hover(Some(TransferAction::List)),
            None,
            "no button"
        );
        assert_eq!(uploads.hover(), None);
    }

    #[test]
    fn every_glyph_of_the_row_is_one_the_atlas_checks() {
        // `bt-atlas` checks these characters in the small class
        // (`the_upload_row_has_no_box_in_the_small_class`); every string of the line
        // must stay within that vocabulary.
        use EntryState::{Done, Waiting};
        let two = entries(&[("a", false, "/srv", Done), ("b", true, "/x", Waiting)]);
        let same = entries(&[("a", false, "/srv", Done)]);
        let mut texts: Vec<String> = [End::Done, End::Cancelled, End::DiskFull, End::Closed]
            .iter()
            .flat_map(|end| {
                [
                    end_line(end, "h", &Tally::of(&two)).0,
                    end_line(end, "h", &Tally::of(&same)).0,
                ]
            })
            .collect();
        texts.push("↑ 1 of 2 · a/ · 1 of 2 files  1.0 / 2.0 MB · 1.0 MB/s · 1s".to_owned());
        for text in texts {
            for ch in text.chars().filter(|ch| !ch.is_ascii()) {
                assert!(bt_core::UPLOAD_GLYPHS.contains(&ch), "'{ch}' in {text:?}");
            }
        }
    }

    #[test]
    fn a_folder_travels_through_the_stream_and_arrives_whole() {
        // A local shell in place of ssh: `sh -c "<remote command>"` — the very script
        // and stream that would run remotely, without a connection.
        let root = scratch("transfer");
        let source = root.join("src");
        let target = root.join("it's here");
        std::fs::create_dir_all(source.join("static/css")).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(source.join("static/app.js"), vec![b'a'; 5000]).unwrap();
        std::fs::write(source.join("static/css/site.css"), b"body{}").unwrap();
        let item = measure(&source.join("static")).unwrap();
        let shared = Arc::new(Shared::default());
        let ticks = std::cell::Cell::new(0);
        let outcome = transfer(
            &words(&["/bin/sh", "-c"]),
            &item,
            target.to_str().unwrap(),
            &shared,
            || ticks.set(ticks.get() + 1),
        );
        assert_eq!(outcome, Outcome::Done);
        assert_eq!(shared.progress(), (5006, 2));
        assert_eq!(
            std::fs::read(target.join("static/css/site.css")).unwrap(),
            b"body{}"
        );
        assert_eq!(
            std::fs::read(target.join("static/app.js")).unwrap().len(),
            5000
        );
        // If the remote tar fails, its line becomes the result.
        let outcome = transfer(
            &words(&["/bin/sh", "-c"]),
            &item,
            root.join("missing").to_str().unwrap(),
            &Arc::new(Shared::default()),
            || {},
        );
        assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
