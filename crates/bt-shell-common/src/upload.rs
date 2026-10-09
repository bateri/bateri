//! Uploading a Finder drop to the remote directory: a file or folder dropped in
//! a remote session goes, after confirmation, to the remote shell's directory
//! through a `tar c | ssh … tar x` stream. Nothing is pasted on its own; the
//! result line says where it went.
//!
//! Three halves:
//!
//! - **Pure:** translating the ssh argv ([`ssh_argv`]), the remote scripts and
//!   quoting, the probe's reply ([`parse_probe`]), the tar stream's header
//!   reader ([`TarWatcher`]), the text of the line and the sheet. The tested half.
//! - **Process:** local measurement ([`measure`]), the probe ([`probe`]) and the
//!   stream ([`transfer`]) — all on a background thread, results to the main queue.
//! - **Queue** ([`Transfers`]): the main thread's state — order, progress, result
//!   line — for **both directions**: an item carries its way
//!   ([`Way`]) and lane ([`Lane`]), the stream of a download is
//!   [`crate::download`]'s. AppKit-free; `uploader` sets up the sheet and the
//!   dispatch.
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

use bt_core::{HostMark, RemoteKind, RemoteTarget, Transfer, TransferControls, TransferTone};

use crate::download::Conflict;
use crate::jobs::SSH_VALUED;
use crate::ssh_route::Route;

/// The shortest interval between progress reports — a **design constant**. Every
/// report is a content frame (the line and the bar changed); reporting at display
/// rate would burn frames on a counter the eye cannot read. A fifth of a second
/// keeps the bar fluid and the numbers readable.
pub const TICK: Duration = Duration::from_millis(200);

/// The window over which speed is measured — a **design constant**: instantaneous
/// speed jumps packet by packet, while a long average reports a slowdown late.
const SPEED_WINDOW: Duration = Duration::from_secs(3);

/// How long the result line (`✓ 3 files uploaded`, `Cancelled — …`) stays in the
/// dock — a **design constant**. This is the stop condition: when it expires the
/// line goes away and no further frame is requested.
pub const LINGER: Duration = Duration::from_secs(4);

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
pub fn ssh_argv(target: &RemoteTarget) -> Vec<String> {
    ssh_argv_for(target, &Route::Direct)
}

/// [`ssh_argv`] on a route ([`crate::ssh_route`]): on [`Route::Direct`]
/// byte for byte today's argv; on [`Route::Ours`] `-o ControlPath=<socket>` goes
/// **behind** the user's options, right before the destination — ssh takes a
/// key's first value, so a `-S`/`-o ControlPath` the user typed is never
/// overridden, while the config file's value (read after the command line) is.
pub fn ssh_argv_for(target: &RemoteTarget, route: &Route) -> Vec<String> {
    let connection = connection(target);
    let mut argv = vec![
        connection.program,
        "-T".to_owned(),
        "-o".to_owned(),
        "BatchMode=yes".to_owned(),
        "-o".to_owned(),
        "ControlMaster=no".to_owned(),
    ];
    argv.extend(connection.options);
    if let Route::Ours(socket) = route {
        argv.push("-o".to_owned());
        argv.push(format!("ControlPath={}", socket.display()));
    }
    argv.push(connection.destination);
    argv
}

/// The target's connection, split into its three parts: the ssh program, the
/// options kept from the user's argv ([`KEPT_FLAGS`], [`KEPT_VALUED`]) and the
/// destination. The **one** parser of the target's argv: the stream's argv
/// ([`ssh_argv_for`]) and the master connection's ([`crate::ssh_route`]) are
/// both assembled from it.
pub(crate) struct Connection {
    pub(crate) program: String,
    pub(crate) options: Vec<String>,
    pub(crate) destination: String,
}

pub(crate) fn connection(target: &RemoteTarget) -> Connection {
    let mut options = Vec::new();
    match target.kind {
        RemoteKind::Mosh => Connection {
            program: "ssh".to_owned(),
            options,
            destination: target.host.clone(),
        },
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
                            options.push(format!("-{flag}"));
                            options.push(value);
                        }
                        break;
                    }
                    if KEPT_FLAGS.contains(flag) {
                        options.push(format!("-{flag}"));
                    }
                }
            }
            Connection {
                program,
                options,
                destination: destination.unwrap_or_else(|| target.host.clone()),
            }
        }
    }
}

/// POSIX single quoting; an inner `'` as `'"'"'` — **produces no backslash**.
///
/// The remote command is first read by the user's **login shell** (it may be fish
/// or csh too), and fish treats `\'` and `\\` as escapes inside single quotes; the
/// `'\''` form would carry the backslash to the outer layer in nested quoting and
/// break the command in fish. `"'"` reads the same in every shell.
pub(crate) fn sq(text: &str) -> String {
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
pub(crate) fn remote_command(script: &str) -> String {
    format!("sh -c {}", sq(script))
}

/// Whether this path can safely enter the remote script: **it must carry no
/// backslash and no control character.** Both break the two-layer quoting (login
/// shell + `sh -c`) in fish or csh; such a name is rejected openly on the sheet (a
/// known limit) rather than silently written to the wrong place.
pub(crate) fn is_safe(text: &str) -> bool {
    !text.chars().any(|c| c == '\\' || c.is_control())
}

/// The mark saying the probe's reply starts here: the login shell's rc file (an
/// `echo` in `.bashrc`) can mix into the output, and everything before the mark is
/// skipped.
const PROBE_MARK: &str = "BT-UPLOAD";

/// The script's exit code when it cannot change into the target directory.
pub(crate) const NO_DIRECTORY: i32 = 3;

/// What is asked remotely before the sheet opens, **in a single connection**: the
/// directory's full path (the home directory if there is no `dir`), `df -Pk`'s line,
/// whether `tar` exists, and whether each name already exists at the target (and is a
/// folder). **Indices**, not names, are printed: a name carrying a newline (rejected,
/// but still) must not be able to break the parsing.
pub(crate) fn probe_script(dir: Option<&str>, names: &[String]) -> String {
    let mut script = String::new();
    if let Some(dir) = dir {
        // A directory read from the title may be `~`-rooted
        // (`Session::remote_link_directory`); quoting would keep the tilde
        // literal, so the home part goes through `$HOME`.
        let target = match dir {
            "~" => "\"$HOME\"".to_owned(),
            _ => match dir.strip_prefix("~/") {
                Some(rest) => format!("\"$HOME\"/{}", sq(rest)),
                None => sq(dir),
            },
        };
        let _ = write!(script, "cd {target} || exit {NO_DIRECTORY}; ");
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
pub struct ProbeReply {
    /// The target directory's full remote path (`pwd`).
    pub dir: String,
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
    pub current: Option<String>,
    /// The number of files whose data has passed completely.
    pub files: u64,
    /// Content bytes passed (file data only).
    pub bytes: u64,
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
pub fn format_bytes(bytes: u64) -> String {
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
pub enum End {
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

/// [`label`] by the name the item **landed** under: "Keep both" may have renamed
/// a download (`report 2.txt`), and the result line must name what is there.
fn landed_label(entry: &Item) -> String {
    match entry.landed.as_ref().and_then(|landed| landed.file_name()) {
        Some(name) if entry.job.local.dir => format!("{}/", name.to_string_lossy()),
        Some(name) => name.to_string_lossy().into_owned(),
        None => label(&entry.job.local),
    }
}

/// Which way a set of items goes: every text that names the
/// direction reads it from here, so an upload-only queue keeps its words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ways {
    Up,
    Down,
    Both,
}

impl Ways {
    /// The directions of `ways`; an empty set reads as `Up` (the upload's words).
    fn of(ways: impl IntoIterator<Item = Direction>) -> Self {
        let (mut up, mut down) = (false, false);
        for way in ways {
            match way {
                Direction::Up => up = true,
                Direction::Down => down = true,
            }
        }
        match (up, down) {
            (true, true) => Self::Both,
            (false, true) => Self::Down,
            _ => Self::Up,
        }
    }

    /// The past participle: `uploaded`, `downloaded`, `transferred`.
    fn verb(self) -> &'static str {
        match self {
            Self::Up => "uploaded",
            Self::Down => "downloaded",
            Self::Both => "transferred",
        }
    }

    /// The arrow: `↑` to the server, `↓` to this Mac, both when mixed.
    fn arrow(self) -> &'static str {
        match self {
            Self::Up => "↑",
            Self::Down => "↓",
            Self::Both => "↑↓",
        }
    }
}

/// A summary of the items when the queue ends: how many items, how many arrived,
/// their common destination (if any), the single item's name and which way they went.
struct Tally {
    items: usize,
    done: usize,
    /// That directory, if all items went to the same directory.
    dest: Option<String>,
    /// Its name if there is a single item ([`label`]).
    single: Option<String>,
    ways: Ways,
}

impl Tally {
    fn of(entries: &[Item]) -> Self {
        let dest = entries.first().map(|entry| entry.job.dest());
        let same = entries.iter().all(|entry| Some(entry.job.dest()) == dest);
        Self {
            items: entries.len(),
            done: entries
                .iter()
                .filter(|entry| entry.state == EntryState::Done)
                .count(),
            dest: dest.filter(|_| same),
            single: match entries {
                [entry] => Some(landed_label(entry)),
                _ => None,
            },
            ways: Ways::of(entries.iter().map(|entry| entry.job.way.direction())),
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

/// A download's full disk is this Mac's, not the server's: the reason the
/// queue ends with ([`Transfers::finish_item`]).
const LOCAL_DISK_FULL: &str = "disk full on this Mac";

/// The result line's body, its tone and the number of leading characters drawn in
/// that tone: success green, cancel dim, the failure text red and the
/// count after it dim.
fn end_line(end: &End, host: &str, tally: &Tally) -> (String, TransferTone, usize) {
    let verb = tally.ways.verb();
    if let Some(reason) = failure_reason(end, host) {
        let head = format!("Failed — {reason}");
        let lead = head.chars().count();
        let body = format!("{head} · {} of {} {verb}", tally.done, tally.items);
        return (body, TransferTone::Error, lead);
    }
    let body = match (end, &tally.dest) {
        (End::Done, Some(dest)) => format!("✓ {} → {dest}", tally.what()),
        (End::Done, None) => format!(
            "✓ {} {} {verb}",
            tally.items,
            // audit: the item count is per drop; it fits in a `u64`.
            files_word(tally.items as u64)
        ),
        _ if tally.items > 1 => format!(
            "Cancelled — {} of {} {verb}, partial file removed",
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

/// The notification shown while bateri is in the background: title
/// and body. Cancelling is the user's own action — no notification.
fn end_notice(end: &End, host: &str, tally: &Tally) -> Option<(String, String)> {
    if let Some(reason) = failure_reason(end, host) {
        let mut chars = reason.trim_end_matches('.').chars();
        let reason = chars
            .next()
            .map(|first| first.to_uppercase().chain(chars).collect::<String>())
            .unwrap_or_default();
        let title = match tally.ways {
            Ways::Up => "Upload failed",
            Ways::Down => "Download failed",
            Ways::Both => "Transfer failed",
        };
        return Some((
            title.to_owned(),
            format!(
                "{reason}. {} of {} {}.",
                tally.done,
                tally.items,
                tally.ways.verb()
            ),
        ));
    }
    if *end != End::Done {
        return None;
    }
    let body = match (tally.ways, &tally.dest) {
        (Ways::Up, Some(dest)) => format!("to {host}:{dest}"),
        (Ways::Up, None) => format!("to {host}"),
        (Ways::Down, Some(dest)) => format!("from {host} to {dest}"),
        (Ways::Down, None) => format!("from {host}"),
        (Ways::Both, _) => format!("with {host}"),
    };
    Some((format!("{} {}", tally.what(), tally.ways.verb()), body))
}

// ─── local measurement ───────────────────────────────────────────────────

/// A dropped item: its path, name, whether it is a folder, how many files and how
/// many bytes — both **locally, before the upload starts** (the sheet states them,
/// and progress is measured against them).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Local {
    pub(crate) path: PathBuf,
    pub(crate) name: String,
    pub(crate) dir: bool,
    pub(crate) files: u64,
    pub(crate) bytes: u64,
}

/// Measures a path. Symbolic links are **not followed** (tar does not follow them
/// either, it carries the link as a link); an unreadable subfolder is skipped — tar
/// will fail on it anyway and the line will say so.
pub fn measure(path: &Path) -> std::io::Result<Local> {
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
pub struct Sheet {
    pub message: String,
    pub informative: String,
    /// The confirm button's title: `Upload`, `Replace` if a same-named file exists,
    /// `Merge` if a same-named folder exists.
    pub button: &'static str,
    /// Whether the confirm button is enabled: disabled if there is not enough space,
    /// if there is no remote tar, or if a name cannot be sent safely.
    pub enabled: bool,
}

/// The confirmation sheet: the title says
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
pub fn sheet(host: &str, reported: bool, items: &[Local], reply: &ProbeReply, busy: bool) -> Sheet {
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
pub(crate) fn last_line(text: &str) -> &str {
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
pub fn probe(
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
pub struct Shared {
    pub(crate) cancel: AtomicBool,
    pub(crate) disk_full: AtomicBool,
    pub(crate) bytes: AtomicU64,
    pub(crate) files: AtomicU64,
    /// The pids of the two processes in the stream (local tar, ssh) — cancelling
    /// **kills** them: on a slow connection the stream thread stays blocked writing
    /// to ssh's input and never looks at the flag; once the process dies the write
    /// returns with `EPIPE`.
    pids: Mutex<Vec<u32>>,
    /// Whether a progress report is waiting on the main queue (at most one).
    pub tick_pending: AtomicBool,
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
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
        self.kill();
    }

    /// Content bytes passed and the number of finished files.
    pub fn progress(&self) -> (u64, u64) {
        (
            self.bytes.load(Ordering::Acquire),
            self.files.load(Ordering::Acquire),
        )
    }

    pub(crate) fn track(&self, children: &[&Child]) {
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
pub(crate) fn wait_untracked(
    child: &mut Child,
    shared: &Shared,
) -> std::io::Result<std::process::ExitStatus> {
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
pub enum Outcome {
    Done,
    Cancelled,
    DiskFull,
    Failed(String),
}

/// The line saying the remote disk is full (GNU tar, bsdtar and busybox print the
/// same `strerror`).
pub(crate) const DISK_FULL: &str = "No space left on device";

/// The local `tar c`'s flags up to `-C` (the directory and `./name` follow).
///
/// macOS's `/usr/bin/tar` is bsdtar: it must not produce `._name` AppleDouble
/// records and attribute paxes — they would become junk files remotely and
/// warnings in GNU tar (`COPYFILE_DISABLE` at the call site says the same to
/// older releases). Its default format is pax-restricted, which [`TarWatcher`]
/// reads.
#[cfg(target_os = "macos")]
const LOCAL_TAR_FLAGS: [&str; 7] = [
    "-c",
    "-f",
    "-",
    "--no-mac-metadata",
    "--no-xattrs",
    "--no-acls",
    "-C",
];

/// The local `tar c`'s flags up to `-C` on Linux, where `/usr/bin/tar` is GNU
/// tar: it has no `--no-mac-metadata` (there is no Mac metadata to drop) and its
/// default `gnu` format writes `@LongLink` records, so the format is pinned to
/// pax — the family [`TarWatcher`] reads. Found by `make linux`.
#[cfg(not(target_os = "macos"))]
const LOCAL_TAR_FLAGS: [&str; 7] = [
    "-c",
    "-f",
    "-",
    "--format=pax",
    "--no-xattrs",
    "--no-acls",
    "-C",
];

/// Uploads one item: `tar c` locally, `ssh … tar x` remotely, the bytes in between
/// pass through us ([`TarWatcher`]). **On a background thread**; `tick` posts the
/// progress report to the main queue (at most once per [`TICK`]).
///
/// Cancel or disk full: both processes are killed and the file **being written** is
/// deleted remotely — for a single file the file itself, for a folder only the one
/// being written at that moment; finished ones stay.
pub fn transfer(
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
    // `./name`: a name starting with `-` must not be taken for an option.
    let local_tar = Command::new("/usr/bin/tar")
        .args(LOCAL_TAR_FLAGS)
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
pub(crate) fn collect_stderr(
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

/// The window and tab title: while an upload streams, `↑ N% · ` in
/// front of the current title — on the alternate screen (vim) there is no dock and
/// this is the only place showing progress; otherwise the title as is.
pub fn titled(percent: Option<u8>, title: &str) -> String {
    titled_as(percent.map(|percent| ("↑", percent)), title)
}

/// [`titled`] with the direction's arrow: `↓ N% · ` while only
/// downloads stream, `↑↓ N% · ` while both do ([`Transfers::title_prefix`]).
pub fn titled_as(prefix: Option<(&str, u8)>, title: &str) -> String {
    match prefix {
        Some((arrow, percent)) => format!("{arrow} {percent}% · {title}"),
        None => title.to_owned(),
    }
}

// ─── queue ───────────────────────────────────────────────────────────────

/// Stopping the streaming item asks first if it has been running longer than
/// this — a **design constant, not a measurement**. Stopping a short upload
/// is cheap (dropping again takes seconds); losing an upload past half a minute must
/// not happen with a single wrong click.
pub const STOP_ASK_AFTER: Duration = Duration::from_secs(30);

/// Which way an item goes: `Up` to the server, `Down` to this Mac.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    #[default]
    Up,
    Down,
}

/// Where an item waits: the **queue** runs one item at a time
/// (uploads and right-click downloads); a **preview** and a **Finder** drop start
/// at once — the user is looking at the screen, or Finder holds a placeholder.
/// Every lane is counted alike by the line, the totals, the list, the stop question
/// and ⌘..
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lane {
    #[default]
    Queue,
    Preview,
    Finder,
}

/// How an item travels: an upload always waits in the queue; a download carries
/// its lane and what happens when its landing name is taken.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Way {
    #[default]
    Up,
    Down {
        lane: Lane,
        conflict: Conflict,
    },
}

impl Way {
    pub fn direction(self) -> Direction {
        match self {
            Self::Up => Direction::Up,
            Self::Down { .. } => Direction::Down,
        }
    }

    pub fn lane(self) -> Lane {
        match self {
            Self::Up => Lane::Queue,
            Self::Down { lane, .. } => lane,
        }
    }
}

/// An item in the queue: the local item, the remote folder and the way.
///
/// One shape serves both directions. Upload: `local` is the item on this Mac and
/// `dir` the remote destination. Download: `dir` is the remote **source** folder,
/// `local.name` the remote name, `local.path` where it lands (the conflict rule may
/// still pick another name, [`crate::download::transfer`]) and `local.files`/`bytes`
/// the remote count.
#[derive(Clone, Debug)]
pub struct Job {
    pub local: Local,
    pub dir: String,
    pub way: Way,
}

impl Job {
    /// A download of the remote absolute path `remote` to `landing` (the local
    /// path it should take), `files`/`bytes` from the remote count. `None` for a
    /// path that names no single item ([`crate::remote_files::split_remote`]).
    pub fn download(
        remote: &str,
        landing: PathBuf,
        dir: bool,
        files: u64,
        bytes: u64,
        lane: Lane,
        conflict: Conflict,
    ) -> Option<Self> {
        let (folder, name) = crate::remote_files::split_remote(remote)?;
        Some(Self {
            local: Local {
                path: landing,
                name: name.to_owned(),
                dir,
                files,
                bytes,
            },
            dir: folder.to_owned(),
            way: Way::Down { lane, conflict },
        })
    }

    /// The remote item's absolute path (a download's source).
    pub fn remote_path(&self) -> String {
        if self.dir.ends_with('/') {
            format!("{}{}", self.dir, self.local.name)
        } else {
            format!("{}/{}", self.dir, self.local.name)
        }
    }

    /// Where a download should land (its conflict rule may still pick another
    /// name).
    pub fn landing(&self) -> &Path {
        &self.local.path
    }

    /// Where the item goes, as the result line and the list name it: the remote
    /// folder of an upload, the local folder of a download.
    fn dest(&self) -> String {
        match self.way {
            Way::Up => self.dir.clone(),
            Way::Down { .. } => self
                .local
                .path
                .parent()
                .map(|parent| parent.display().to_string())
                .unwrap_or_default(),
        }
    }
}

/// An item's state in the queue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EntryState {
    Waiting,
    Running,
    Done,
}

/// An item of the queue. A finished item **stays** on the list: the
/// popover shows it as `✓ Uploaded` and the result line counts it.
#[derive(Debug)]
struct Item {
    /// Unique across queues: the popover's button finds the item by this — the
    /// position would shift as items finish and are removed.
    id: u64,
    job: Job,
    state: EntryState,
    /// Where a finished download landed (the conflict rule may have renamed it):
    /// the target of the popover's "Show in Finder" and "Open".
    landed: Option<PathBuf>,
}

/// An item that is streaming.
#[derive(Debug)]
struct Current {
    id: u64,
    shared: Arc<Shared>,
    /// When the stream started: the criterion of the stop question ([`STOP_ASK_AFTER`]).
    started: Instant,
    lane: Lane,
    /// The item was stopped on its own: the `Cancelled` result does not end the
    /// queue, the item leaves the list and the next one starts.
    skip: bool,
}

/// An item that started ([`Transfers::start`]): the stream thread's inputs.
#[derive(Debug)]
pub struct Started {
    pub id: u64,
    pub ssh: Vec<String>,
    pub job: Job,
    pub shared: Arc<Shared>,
}

/// A tab's transfer queue — **bound to that tab's ssh connection**:
/// its generation is the remote session's command generation; if the generation
/// changes (ssh closed) the pending items are cancelled.
#[derive(Debug)]
struct Queue {
    command: u64,
    ssh: Vec<String>,
    host: String,
    mark: HostMark,
    entries: Vec<Item>,
    /// The streaming items: at most one from the queue lane, any number from the
    /// preview and Finder lanes.
    running: Vec<Current>,
    /// The bar's denominator and the bytes of finished items: the unsent part of an
    /// individually stopped item is subtracted from the denominator and the sent part
    /// counts as done — so the bar never goes backwards.
    bytes_total: u64,
    bytes_done: u64,
    /// Speed samples: (instant, the queue's bytes passed).
    samples: VecDeque<(Instant, u64)>,
    /// The last measured speed, bytes/s (the popover's row shows it too).
    rate: Option<f64>,
    /// No further item will start: the queue ends with this end once nothing streams.
    ending: Option<End>,
    /// A preview or Finder lane item failed: those lanes are independent of the
    /// queue lane, so the failure does not stop the waiting items — it is the
    /// queue's end once everything else finished, unless that end is already
    /// something else.
    side_failure: Option<End>,
}

impl Queue {
    fn current(&self, id: u64) -> Option<&Current> {
        self.running.iter().find(|current| current.id == id)
    }

    /// The queue lane's streaming item.
    fn queued(&self) -> Option<&Current> {
        self.running
            .iter()
            .find(|current| current.lane == Lane::Queue)
    }

    /// The item the line and a stop without an id speak of: the queue lane's,
    /// otherwise the longest-streaming one.
    fn subject(&self) -> Option<&Current> {
        self.queued().or_else(|| self.longest())
    }

    /// The streaming item that started first.
    fn longest(&self) -> Option<&Current> {
        self.running.iter().min_by_key(|current| current.started)
    }

    fn entry(&self, id: u64) -> Option<(usize, &Item)> {
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

    /// Which way the entries in `state` go (every entry with `None`).
    fn ways(&self, state: Option<EntryState>) -> Ways {
        Ways::of(
            self.entries
                .iter()
                .filter(|entry| state.is_none_or(|state| entry.state == state))
                .map(|entry| entry.job.way.direction()),
        )
    }

    /// The streaming items' bytes passed.
    fn running_bytes(&self) -> u64 {
        self.running
            .iter()
            .map(|current| current.shared.progress().0)
            .sum()
    }
}

/// A tab's transfer state (main thread): whether a sheet is in progress, the queue
/// and the result line's generation.
#[derive(Debug, Default)]
pub struct Transfers {
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
    /// Whether the list is open: the line is reborn on every refresh and this
    /// is stamped on it at each birth — otherwise the 200 ms refresh would
    /// overwrite it. (The button under the mouse is the session's one hover
    /// slot, `Session::set_footer_hover`, which the refresh does not write.)
    list_open: bool,
    /// The arrow and percentage last written to the title ([`Self::title_percent_changed`]).
    titled: Option<(&'static str, u8)>,
    /// An update waits for the application's transfers to end: how many are left across every
    /// pane — the line leads with it ([`Self::status`]). `None`: no update waits.
    update_waits: Option<usize>,
}

/// The queue ended: the result line, its generation and the notification (title,
/// body) to show if bateri is in the background.
#[derive(Debug, PartialEq, Eq)]
pub struct Ended {
    pub line: Transfer,
    pub serial: u64,
    pub notice: Option<(String, String)>,
}

/// The answer to a stop request ([`Transfers::stop_request`]).
#[derive(Debug, PartialEq, Eq)]
pub enum Stop {
    /// Stop without asking.
    Now { id: Option<u64>, all: bool },
    /// Ask first: the item with `id` has been streaming for over thirty seconds.
    Ask(StopQuestion),
}

/// The stop question: the sheet's title and text, which item it was
/// asked for (if it finishes meanwhile the sheet closes on its own) and whether it is
/// the whole queue.
#[derive(Debug, PartialEq, Eq)]
pub struct StopQuestion {
    pub id: u64,
    pub all: bool,
    pub title: String,
    pub text: String,
}

/// The state of a popover row and the left column's second line.
#[derive(Clone, Debug, PartialEq)]
pub enum RowStatus {
    /// Streaming: the bar's fraction (`0..=1`) and `18.2 / 96.0 MB · 1.2 MB/s`.
    Running { fraction: f64, detail: String },
    /// Queued: `Waiting · 48.5 MB`.
    Waiting(String),
    /// Finished: `✓ Uploaded · 96.0 MB`.
    Done(String),
}

/// The button on the right of a popover row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowAction {
    /// Stop the streaming item (asks first if past 30 s).
    Cancel,
    /// Remove the waiting item from the queue (does not ask).
    Remove,
    /// A finished download: reveal it in Finder.
    ShowInFinder,
    /// A finished preview: open it.
    Open,
}

impl RowStatus {
    /// The row's button by its state alone: `Cancel` when streaming, `Remove` when
    /// waiting, none when done — a finished download's button is [`ListRow::action`]'s.
    pub fn action(&self) -> Option<RowAction> {
        match self {
            Self::Running { .. } => Some(RowAction::Cancel),
            Self::Waiting(_) => Some(RowAction::Remove),
            Self::Done(_) => None,
        }
    }
}

/// A popover row: the name (`static/ · 124 files` for a folder), the state, the
/// dim destination line (`→ /var/www/app`), the direction (the row's arrow) and the
/// button.
#[derive(Clone, Debug, PartialEq)]
pub struct ListRow {
    pub id: u64,
    pub name: String,
    pub status: RowStatus,
    pub dest: String,
    pub direction: Direction,
    pub action: Option<RowAction>,
}

/// The "Show transfers (N)" popover: title and rows.
#[derive(Clone, Debug, PartialEq)]
pub struct TransferList {
    pub title: String,
    pub rows: Vec<ListRow>,
}

impl Transfers {
    /// Whether a new drop can be accepted.
    ///
    /// Not for a queue that was cancelled but whose streaming item has not finished
    /// yet (its half-written file is being deleted) either: a confirmed drop could not
    /// be added to it and would be silently dropped.
    pub fn can_accept(&self) -> bool {
        !self.asking
            && self
                .queue
                .as_ref()
                .is_none_or(|queue| queue.ending.is_none())
    }

    /// The probe or the sheet started / ended.
    pub fn set_asking(&mut self, asking: bool) {
        self.asking = asking;
    }

    /// Whether a drop's probe or confirmation sheet is in progress: the stop question
    /// does not open meanwhile (two sheets cannot be stacked).
    pub fn asking(&self) -> bool {
        self.asking
    }

    /// Whether a queue is running (⌘.'s gate).
    pub fn active(&self) -> bool {
        self.queue.is_some()
    }

    /// The items not finished yet — streaming or waiting: what an
    /// update waits for. A cancelled queue still deleting its half-written
    /// file counts its streaming item.
    pub fn unfinished(&self) -> usize {
        self.queue.as_ref().map_or(0, |queue| {
            queue
                .entries
                .iter()
                .filter(|entry| entry.state != EntryState::Done)
                .count()
        })
    }

    /// An update waits for `left` transfers of the application (`None`: no
    /// update waits); `true` if that changed — the caller redraws the line.
    pub fn set_update_waits(&mut self, left: Option<usize>) -> bool {
        let changed = self.update_waits != left;
        self.update_waits = left;
        changed
    }

    /// Whether the queue is streaming: the gate of the sheet's "added to the queue" line.
    pub fn busy(&self) -> bool {
        self.queue
            .as_ref()
            .is_some_and(|queue| queue.ending.is_none())
    }

    /// The remote session's generation (to tell that ssh closed).
    pub fn command(&self) -> Option<u64> {
        self.queue.as_ref().map(|queue| queue.command)
    }

    /// Appends the confirmed items to the end of the queue; creates the queue if there
    /// is none. If the queue belongs to **another** remote session (reconnected, the
    /// old one still finishing) the old queue counts as closed and the new one cannot
    /// be set up behind it or in its place — then `false` and the drop is dropped.
    pub fn enqueue(
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
            running: Vec::new(),
            bytes_total: 0,
            bytes_done: 0,
            samples: VecDeque::new(),
            rate: None,
            ending: None,
            side_failure: None,
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
                landed: None,
            });
        }
        true
    }

    /// Starts at `now` every item that may start: each waiting preview and Finder
    /// item, and the queue lane's next item if none of its own streams.
    /// Empty if the queue is ending.
    pub fn start(&mut self, now: Instant) -> Vec<Started> {
        let Some(queue) = self.queue.as_mut() else {
            return Vec::new();
        };
        if queue.ending.is_some() {
            return Vec::new();
        }
        let mut queue_busy = queue
            .running
            .iter()
            .any(|current| current.lane == Lane::Queue);
        let mut started = Vec::new();
        for entry in &mut queue.entries {
            if entry.state != EntryState::Waiting {
                continue;
            }
            let lane = entry.job.way.lane();
            if lane == Lane::Queue {
                if queue_busy {
                    continue;
                }
                queue_busy = true;
            }
            entry.state = EntryState::Running;
            let shared = Arc::new(Shared::default());
            queue.running.push(Current {
                id: entry.id,
                shared: Arc::clone(&shared),
                started: now,
                lane,
                skip: false,
            });
            started.push(Started {
                id: entry.id,
                ssh: queue.ssh.clone(),
                job: entry.job.clone(),
                shared,
            });
        }
        started
    }

    /// Starts the queue lane's next item at `now`: the stream thread's inputs. `None`
    /// if a queue lane item is already streaming or the queue is ending.
    pub fn start_next(&mut self, now: Instant) -> Option<(Vec<String>, Job, Arc<Shared>)> {
        let queue = self.queue.as_mut()?;
        if queue.queued().is_some() || queue.ending.is_some() {
            return None;
        }
        let entry = queue.entries.iter_mut().find(|entry| {
            entry.state == EntryState::Waiting && entry.job.way.lane() == Lane::Queue
        })?;
        entry.state = EntryState::Running;
        let shared = Arc::new(Shared::default());
        queue.running.push(Current {
            id: entry.id,
            shared: Arc::clone(&shared),
            started: now,
            lane: Lane::Queue,
            skip: false,
        });
        Some((queue.ssh.clone(), entry.job.clone(), shared))
    }

    /// The queue lane's streaming item's id.
    pub fn running_id(&self) -> Option<u64> {
        self.queue.as_ref()?.queued().map(|current| current.id)
    }

    /// Whether the item with `id` is streaming (the stop sheet's closing question).
    pub fn is_running(&self, id: u64) -> bool {
        self.queue
            .as_ref()
            .is_some_and(|queue| queue.current(id).is_some())
    }

    /// A stop request at `now`: `all` is the whole queue (⌘., the line's
    /// `Cancel`/`Cancel all`, the popover's `Cancel all`), otherwise the queue lane's
    /// streaming item. [`Self::stop_request_item`] with no id.
    pub fn stop_request(&self, all: bool, now: Instant) -> Option<Stop> {
        self.stop_request_item(None, all, now)
    }

    /// A stop request for the streaming item `id` (the popover row's `Cancel`), or
    /// without an id for the queue lane's item — with `all`, the longest-streaming
    /// one, so the question comes if any item has streamed past [`STOP_ASK_AFTER`].
    /// In a single-item queue `all` and the item are the same. A question if the item
    /// has been running longer than the threshold, otherwise at once; `None` if there
    /// is no queue.
    pub fn stop_request_item(&self, id: Option<u64>, all: bool, now: Instant) -> Option<Stop> {
        let queue = self.queue.as_ref()?;
        let all = all || queue.entries.len() <= 1;
        let subject = match id {
            // The item asked about no longer streams: nothing to stop — a late row
            // `Cancel` must not land on whatever streams now.
            Some(id) => Some(queue.current(id)?),
            None if all => queue.longest(),
            None => queue.subject(),
        };
        let Some(current) = subject else {
            return Some(Stop::Now { id: None, all });
        };
        if now.saturating_duration_since(current.started) <= STOP_ASK_AFTER {
            return Some(Stop::Now {
                id: Some(current.id),
                all,
            });
        }
        let Some((_, entry)) = queue.entry(current.id) else {
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
                    " {waiting} waiting {} won't be {}.",
                    files_word(waiting as u64),
                    queue.ways(Some(EntryState::Waiting)).verb()
                );
            }
            let others = queue.running.len() - 1;
            if others > 0 {
                let (noun, verb) = if others == 1 {
                    ("transfer", "stops")
                } else {
                    ("transfers", "stop")
                };
                let _ = write!(text, " {others} other {noun} {verb}.");
            }
            let done = queue.done();
            if done > 0 {
                let (noun, verb) = if done == 1 {
                    ("file", "stays")
                } else {
                    ("files", "stay")
                };
                let _ = match queue.ways(Some(EntryState::Done)) {
                    Ways::Up => write!(text, " {done} finished {noun} {verb} on {}.", queue.host),
                    Ways::Down => write!(text, " {done} finished {noun} {verb} on this Mac."),
                    Ways::Both => write!(text, " {done} finished {noun} {verb}."),
                };
            }
        }
        let ways = if all && queue.entries.len() > 1 {
            queue.ways(None)
        } else {
            Ways::of([entry.job.way.direction()])
        };
        let title = match (all && queue.entries.len() > 1, ways) {
            (true, Ways::Up) => "Stop all uploads?",
            (true, Ways::Down) => "Stop all downloads?",
            (true, Ways::Both) => "Stop all transfers?",
            (false, Ways::Down) => "Stop downloading?",
            (false, _) => "Stop uploading?",
        };
        Some(Stop::Ask(StopQuestion {
            id: current.id,
            all,
            title: title.to_owned(),
            text,
        }))
    }

    /// The stop question's default button, in the question's direction:
    /// `Keep uploading`, `Keep downloading` or `Keep transferring`.
    pub fn keep_label(&self, question: &StopQuestion) -> &'static str {
        let Some(queue) = &self.queue else {
            return "Keep uploading";
        };
        let ways = if question.all && queue.entries.len() > 1 {
            queue.ways(None)
        } else {
            Ways::of(
                queue
                    .entry(question.id)
                    .map(|(_, entry)| entry.job.way.direction()),
            )
        };
        match ways {
            Ways::Up => "Keep uploading",
            Ways::Down => "Keep downloading",
            Ways::Both => "Keep transferring",
        }
    }

    /// Applies the stop. `id` is the question's (or the request's) item: if that item
    /// finished meanwhile, nothing is done — the question spoke of losing that item.
    /// Without `all` only that item stops (without an id, the queue lane's) and the
    /// queue goes on; its result comes when the item finishes ([`Self::finish_item`]).
    /// With nothing else waiting or streaming, stopping the item cancels the queue.
    pub fn stop(&mut self, id: Option<u64>, all: bool) -> Option<Ended> {
        let queue = self.queue.as_mut()?;
        if let Some(id) = id
            && queue.current(id).is_none()
        {
            return None;
        }
        // With nothing else waiting or streaming, stopping the streaming item cancels
        // the queue: the result must say `Cancelled — 1 of 2 uploaded, …`, not the
        // finished items' `✓`.
        let alone = queue.waiting() + queue.running.len().saturating_sub(1) == 0;
        if all || queue.entries.len() <= 1 || alone {
            return self.cancel();
        }
        let target = id.or_else(|| queue.subject().map(|current| current.id));
        if let Some(current) = queue
            .running
            .iter_mut()
            .find(|current| Some(current.id) == target)
        {
            current.skip = true;
            current.shared.cancel();
        }
        None
    }

    /// Cancels the whole queue: the waiting items will not start, the streaming items
    /// are killed and their half-written files deleted; the result comes when the last
    /// of them finishes. With no streaming item (the stream thread could not be born)
    /// the result is immediate.
    fn cancel(&mut self) -> Option<Ended> {
        let queue = self.queue.as_mut()?;
        if queue.ending.is_none() {
            queue.ending = Some(End::Cancelled);
        }
        if queue.running.is_empty() {
            return self.end();
        }
        for current in &queue.running {
            current.shared.cancel();
        }
        None
    }

    /// Cancels and abandons the queue (the tab is closing): the streaming items are
    /// killed and their half-written files deleted on the stream threads; the result
    /// is shown to no one.
    pub fn abandon(&mut self) {
        if let Some(queue) = self.queue.take() {
            for current in &queue.running {
                current.shared.cancel();
            }
        }
    }

    /// The remote session closed: waiting items are cancelled; the streaming items
    /// finish over their own connections. With no streaming item the result is
    /// immediate.
    pub fn close(&mut self) -> Option<Ended> {
        let queue = self.queue.as_mut()?;
        if queue.ending.is_none() {
            queue.ending = Some(End::Closed);
        }
        if queue.running.is_empty() {
            return self.end();
        }
        None
    }

    /// The queue lane's streaming item finished ([`Self::finish_item`]).
    pub fn finish(&mut self, outcome: Outcome) -> Option<Ended> {
        let id = self.queue.as_ref()?.queued()?.id;
        self.finish_item(id, outcome, None)
    }

    /// The streaming item `id` finished (`landed`: where a download landed); the
    /// result if the queue finished too — once **nothing** streams any more, so a
    /// preview's report is never lost behind a failed upload. Nothing is pasted on its
    /// own: an upload can take minutes and the path would be typed into
    /// whatever vim or mysql is open at that moment — the result line says where it went.
    pub fn finish_item(
        &mut self,
        id: u64,
        outcome: Outcome,
        landed: Option<PathBuf>,
    ) -> Option<Ended> {
        let queue = self.queue.as_mut()?;
        let at = queue.running.iter().position(|current| current.id == id)?;
        let current = queue.running.remove(at);
        let (bytes, _) = current.shared.progress();
        let index = queue.entries.iter().position(|entry| entry.id == id)?;
        let direction = queue.entries[index].job.way.direction();
        // Queue lane items still to finish: a side lane's failure must not stop them.
        let queue_pending = queue.entries.iter().any(|entry| {
            entry.id != id && entry.job.way.lane() == Lane::Queue && entry.state != EntryState::Done
        });
        match outcome {
            Outcome::Done => {
                let entry = &mut queue.entries[index];
                entry.state = EntryState::Done;
                entry.landed = landed;
                queue.bytes_done += entry.job.local.bytes;
            }
            // An individually stopped item leaves the list; its sent part counts as
            // done, its unsent part leaves the denominator (the bar never goes back).
            Outcome::Cancelled if current.skip && queue.ending.is_none() => {
                let entry = queue.entries.remove(index);
                let sent = bytes.min(entry.job.local.bytes);
                queue.bytes_done += sent;
                queue.bytes_total -= entry.job.local.bytes - sent;
                // If the waiting items were removed meanwhile, the queue ends as a cancel.
                if queue.waiting() == 0 && queue.running.is_empty() {
                    queue.ending = Some(End::Cancelled);
                }
            }
            Outcome::Cancelled => {
                queue.entries[index].state = EntryState::Waiting;
                queue.bytes_done += bytes;
                queue.ending.get_or_insert(End::Cancelled);
            }
            Outcome::DiskFull | Outcome::Failed(_)
                if current.lane != Lane::Queue && queue_pending =>
            {
                let end = match outcome {
                    Outcome::Failed(reason) => End::Failed(reason),
                    _ => End::Failed(LOCAL_DISK_FULL.to_owned()),
                };
                // The item leaves the list the way an individually stopped one does.
                let entry = queue.entries.remove(index);
                let sent = bytes.min(entry.job.local.bytes);
                queue.bytes_done += sent;
                queue.bytes_total -= entry.job.local.bytes - sent;
                queue.side_failure.get_or_insert(end);
            }
            Outcome::DiskFull => {
                queue.entries[index].state = EntryState::Waiting;
                queue.bytes_done += bytes;
                queue.ending = Some(match direction {
                    Direction::Up => End::DiskFull,
                    Direction::Down => End::Failed(LOCAL_DISK_FULL.to_owned()),
                });
            }
            Outcome::Failed(reason) => {
                queue.entries[index].state = EntryState::Waiting;
                queue.bytes_done += bytes;
                queue.ending = Some(End::Failed(reason));
            }
        }
        if !queue.running.is_empty() {
            return None;
        }
        if queue.waiting() == 0 || queue.ending.is_some() {
            self.end()
        } else {
            None
        }
    }

    /// Whether a preview to `landing` is already on its way: a second ⌘-click on
    /// the same file waits for it instead of streaming the copy twice
    /// (the second stream's cache question could see the
    /// first one's renamed but not yet recorded copy as edited).
    pub fn previewing(&self, landing: &Path) -> bool {
        self.queue.as_ref().is_some_and(|queue| {
            queue.entries.iter().any(|entry| {
                entry.state != EntryState::Done
                    && entry.job.way.lane() == Lane::Preview
                    && entry.job.landing() == landing
            })
        })
    }

    /// Where the finished download with `id` landed (the popover's "Show in Finder"
    /// and "Open").
    pub fn landed(&self, id: u64) -> Option<PathBuf> {
        let (_, entry) = self.queue.as_ref()?.entry(id)?;
        entry.landed.clone()
    }

    /// Closes the queue and returns the result line.
    fn end(&mut self) -> Option<Ended> {
        let queue = self.queue.take()?;
        self.serial += 1;
        // ssh closed but the streaming item finished over its own connection and
        // nothing was waiting: everything arrived, the result is a success.
        let all_done = queue
            .entries
            .iter()
            .all(|entry| entry.state == EntryState::Done);
        let end = match queue.ending {
            Some(End::Closed) if all_done && queue.side_failure.is_none() => End::Done,
            ending => ending.or(queue.side_failure).unwrap_or(End::Done),
        };
        let tally = Tally::of(&queue.entries);
        let (body, tone, lead) = end_line(&end, &queue.host, &tally);
        // A queue of previews only that succeeded opens its files: the opened
        // window is the news, a notification would repeat it. The
        // result line stays — it says where the copy is. A failure still notifies.
        let previews_only = queue
            .entries
            .iter()
            .all(|entry| entry.job.way.lane() == Lane::Preview);
        let notice = if previews_only && end == End::Done {
            None
        } else {
            end_notice(&end, &queue.host, &tally)
        };
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

    /// The status line last written to the dock.
    pub fn shown(&self) -> Option<&Transfer> {
        self.shown.as_ref()
    }

    /// Writes [`Self::shown`]. A line without buttons (a result, or none) closes
    /// the list too: there is no button left to hold it open.
    pub fn set_shown(&mut self, transfer: Option<Transfer>) {
        if transfer.as_ref().is_none_or(|t| t.controls.items == 0) {
            self.list_open = false;
        }
        self.shown = transfer;
    }

    /// The list opened/closed; if it changed, the stamped line.
    pub fn set_list_open(&mut self, open: bool) -> Option<Transfer> {
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
        shown.controls.list_open = self.list_open;
        Some(shown)
    }

    /// The queue's passed and total bytes (the Dock icon's bar); `None` if there is no
    /// queue.
    pub fn totals(&self) -> Option<(u64, u64)> {
        let queue = self.queue.as_ref()?;
        Some((queue.bytes_done + queue.running_bytes(), queue.bytes_total))
    }

    /// The percentage for the title prefix: while an item streams, based
    /// on the whole queue's bytes, rounded down; `None` if nothing streams or it is
    /// being cancelled. If ssh closed, the streaming items finish over their own
    /// connections and the prefix stays with them.
    pub fn percent(&self) -> Option<u8> {
        let queue = self.queue.as_ref()?;
        if queue.ending == Some(End::Cancelled) || queue.running.is_empty() {
            return None;
        }
        let (sent, total) = self.totals()?;
        if total == 0 {
            return Some(0);
        }
        // audit: clamped to `sent ≤ total`; the ratio is 0..=100, fits in a `u8`.
        Some((sent.min(total) as f64 / total as f64 * 100.0).floor() as u8)
    }

    /// The title prefix now: the streaming items' arrow and [`Self::percent`].
    fn prefix(&self) -> Option<(&'static str, u8)> {
        let percent = self.percent()?;
        let queue = self.queue.as_ref()?;
        let ways = Ways::of(queue.running.iter().filter_map(|current| {
            queue
                .entry(current.id)
                .map(|(_, entry)| entry.job.way.direction())
        }));
        Some((ways.arrow(), percent))
    }

    /// Whether the title's prefix differs from the last one written; if so, stores
    /// the new one — the title is written at most once per percent (and once per
    /// change of direction: the arrow can flip at the same percent).
    pub fn title_percent_changed(&mut self) -> bool {
        let prefix = self.prefix();
        if self.titled == prefix {
            return false;
        }
        self.titled = prefix;
        true
    }

    /// The percentage last written to the title.
    pub fn title_percent(&self) -> Option<u8> {
        self.titled.map(|(_, percent)| percent)
    }

    /// The arrow and percentage last written to the title ([`titled_as`]'s input).
    pub fn title_prefix(&self) -> Option<(&'static str, u8)> {
        self.titled
    }

    /// The local name of the item the status line speaks of (the queue lane's,
    /// else the longest-streaming one); `None` while nothing streams — the tab
    /// bar's summary card names it beside the percentage.
    pub fn flowing_name(&self) -> Option<&str> {
        let queue = self.queue.as_ref()?;
        let current = queue.subject()?;
        queue
            .entry(current.id)
            .map(|(_, item)| item.job.local.name.as_str())
    }

    /// The wait is over: the line goes away if the result line is still this generation
    /// and no new queue has started.
    pub fn linger_over(&self, serial: u64) -> bool {
        self.queue.is_none() && self.serial == serial
    }

    /// The popover's content: all items — finished, streaming and
    /// waiting — in order. `None` if there is no queue or it is ending: the popover
    /// must close.
    pub fn list(&self) -> Option<TransferList> {
        let queue = self.queue.as_ref()?;
        if queue.ending.is_some() {
            return None;
        }
        let rows = queue
            .entries
            .iter()
            .map(|entry| {
                let local = &entry.job.local;
                let direction = entry.job.way.direction();
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
                        let verb = match direction {
                            Direction::Up => "Uploaded",
                            Direction::Down => "Downloaded",
                        };
                        RowStatus::Done(format!("✓ {verb} · {}", format_bytes(local.bytes)))
                    }
                    EntryState::Waiting => {
                        RowStatus::Waiting(format!("Waiting · {}", format_bytes(local.bytes)))
                    }
                    EntryState::Running => {
                        let sent = queue
                            .current(entry.id)
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
                let action = match (&status, entry.job.way, &entry.landed) {
                    (RowStatus::Done(_), Way::Down { lane, .. }, Some(_)) => {
                        Some(if lane == Lane::Preview {
                            RowAction::Open
                        } else {
                            RowAction::ShowInFinder
                        })
                    }
                    _ => status.action(),
                };
                let dest = match &entry.landed {
                    Some(landed) => landed
                        .parent()
                        .map(|parent| parent.display().to_string())
                        .unwrap_or_default(),
                    None => entry.job.dest(),
                };
                ListRow {
                    id: entry.id,
                    name,
                    status,
                    dest: format!("→ {dest}"),
                    direction,
                    action,
                }
            })
            .collect();
        let title = match queue.ways(None) {
            Ways::Up => format!("Uploading to {}", queue.host),
            Ways::Down => format!("Downloading from {}", queue.host),
            Ways::Both => format!("Transfers with {}", queue.host),
        };
        Some(TransferList { title, rows })
    }

    /// Removes the waiting item with `id` from the queue (the popover's `Remove`); a
    /// no-op for a streaming or finished item.
    pub fn remove(&mut self, id: u64) {
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

    /// The dock's status line at `now`; `None` if nothing streams. The line names the
    /// queue lane's item (otherwise the longest-streaming one); a queue going one way
    /// leads with its arrow and `k of n`, a mixed one with the summary `↑1 ↓2`.
    pub fn status(&mut self, now: Instant) -> Option<Transfer> {
        let queue = self.queue.as_mut()?;
        let current = queue.subject()?;
        let (id, (_, files)) = (current.id, current.shared.progress());
        let sent = queue.bytes_done + queue.running_bytes();
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
        let (index, entry) = queue.entry(id)?;
        let local = &entry.job.local;
        let items = queue.entries.len();
        let mut body = String::new();
        // The update's wait leads: ⌘. is the way past it.
        if let Some(left) = self.update_waits.filter(|&left| left > 0) {
            let noun = if left == 1 { "transfer" } else { "transfers" };
            let _ = write!(body, "Update waits for {left} {noun} · ");
        }
        match queue.ways(None) {
            Ways::Both => {
                let down = queue
                    .entries
                    .iter()
                    .filter(|entry| entry.job.way.direction() == Direction::Down)
                    .count();
                let _ = write!(body, "↑{} ↓{down} · ", items - down);
            }
            ways => {
                let _ = write!(body, "{} ", ways.arrow());
                if items > 1 {
                    let _ = write!(body, "{} of {} · ", index + 1, items);
                }
            }
        }
        body.push_str(&local.name);
        if local.dir {
            body.push('/');
        }
        // The folder's file count next to the name: the item's own information, while
        // the bytes are the queue's (the demo the user approved).
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
    fn a_tilde_directory_from_the_title_resolves_under_the_remote_home() {
        let home = scratch("tilde");
        std::fs::create_dir_all(home.join("my foo")).unwrap();
        for (dir, tail) in [("~", None), ("~/my foo", Some("my foo"))] {
            let output = Command::new("/bin/sh")
                .arg("-c")
                .arg(remote_command(&probe_script(Some(dir), &[])))
                .env("HOME", &home)
                .output()
                .expect("shell did not run");
            let out = String::from_utf8_lossy(&output.stdout);
            let reply = parse_probe(&out).unwrap_or_else(|| panic!("{dir}: {out}"));
            let want = home.file_name().unwrap().to_str().unwrap();
            assert!(
                reply.dir.ends_with(tail.unwrap_or(want)),
                "{dir}: {}",
                reply.dir
            );
        }
        let _ = std::fs::remove_dir_all(&home);
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
            .args(LOCAL_TAR_FLAGS)
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
                    way: Way::Up,
                },
                state,
                landed: None,
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
            way: Way::Up,
        }
    }

    fn queue_of(jobs: Vec<Job>) -> Transfers {
        let mut uploads = Transfers::default();
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

    /// The unfinished items are what an update waits for, and a
    /// waiting update leads the line — only while it waits.
    #[test]
    fn a_waiting_update_leads_the_line_with_the_unfinished_count() {
        let mut uploads = queue_of(vec![job("a", false, 1, 10), job("b", false, 1, 10)]);
        assert_eq!(Transfers::default().unfinished(), 0);
        assert_eq!(uploads.unfinished(), 2);
        let start = Instant::now();
        let _ = uploads.start_next(start).unwrap();
        assert_eq!(uploads.finish(Outcome::Done), None);
        let _ = uploads.start_next(start).unwrap();
        assert_eq!(
            uploads.unfinished(),
            1,
            "the finished item is not waited for"
        );
        let plain = uploads.status(start).expect("line").body;
        assert!(uploads.set_update_waits(Some(3)));
        assert!(!uploads.set_update_waits(Some(3)), "no change, no redraw");
        let waiting = uploads.status(start).expect("line").body;
        assert_eq!(waiting, format!("Update waits for 3 transfers · {plain}"));
        uploads.set_update_waits(Some(1));
        assert!(
            uploads
                .status(start)
                .unwrap()
                .body
                .starts_with("Update waits for 1 transfer · ")
        );
        uploads.set_update_waits(None);
        assert_eq!(uploads.status(start).unwrap().body, plain);
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
    fn the_open_list_survives_the_refresh_and_changes_only_on_edges() {
        let mut uploads = queue_of(vec![job("a", false, 1, 10), job("b", false, 1, 10)]);
        let now = Instant::now();
        let (_, _, _shared) = uploads.start_next(now).expect("first item");
        let status = uploads.status(now).expect("line");
        uploads.set_shown(Some(status));
        let open = uploads.set_list_open(true).expect("changed");
        assert!(open.controls.list_open);
        uploads.set_shown(Some(open));
        assert_eq!(
            uploads.set_list_open(true),
            None,
            "the same state requests no frame"
        );
        // The 200 ms refresh does not overwrite it.
        let status = uploads.status(now).expect("line");
        assert!(status.controls.list_open);
        // A line without buttons (a result) closes it.
        uploads.set_shown(Some(Transfer::default()));
        assert_eq!(uploads.set_list_open(false), None, "already closed");
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

    fn download(name: &str, lane: Lane, bytes: u64) -> Job {
        Job::download(
            &format!("/var/log/{name}"),
            PathBuf::from("/Users/me/Downloads").join(name),
            false,
            1,
            bytes,
            lane,
            Conflict::KeepBoth,
        )
        .expect("an absolute path")
    }

    fn started_names(started: &[Started]) -> Vec<&str> {
        started
            .iter()
            .map(|started| started.job.local.name.as_str())
            .collect()
    }

    #[test]
    fn previews_and_finder_drops_start_at_once_while_the_queue_runs_one_by_one() {
        let mut transfers = queue_of(vec![
            job("a", false, 1, 100),
            download("b", Lane::Queue, 100),
            download("c", Lane::Preview, 100),
            download("d", Lane::Finder, 100),
        ]);
        let now = Instant::now();
        let started = transfers.start(now);
        assert_eq!(started_names(&started), ["a", "c", "d"]);
        assert_eq!(started[1].job.remote_path(), "/var/log/c");
        assert!(transfers.start(now).is_empty(), "the queue waits for `a`");
        let id = |name: &str| {
            started
                .iter()
                .find(|s| s.job.local.name == name)
                .unwrap()
                .id
        };

        // The line names the queue's item and leads with the summary.
        let status = transfers.status(now).expect("line");
        assert!(status.body.starts_with("↑1 ↓3 · a  "), "{}", status.body);
        assert_eq!(status.controls.items, 4);

        // A preview finishing does not end the queue; it lands and offers `Open`.
        let preview = PathBuf::from("/Users/me/Previews/prod/var/log/c");
        assert_eq!(
            transfers.finish_item(id("c"), Outcome::Done, Some(preview.clone())),
            None
        );
        assert_eq!(transfers.landed(id("c")), Some(preview));
        assert_eq!(transfers.finish_item(id("a"), Outcome::Done, None), None);
        let next = transfers.start(now);
        assert_eq!(started_names(&next), ["b"], "the queue's next item");
        assert!(
            transfers
                .status(now)
                .unwrap()
                .body
                .starts_with("↑1 ↓3 · b  ")
        );

        let list = transfers.list().expect("list");
        assert_eq!(list.title, "Transfers with prod");
        let rows: Vec<_> = list
            .rows
            .iter()
            .map(|row| (row.name.as_str(), row.direction, row.action))
            .collect();
        assert_eq!(
            rows,
            [
                ("a", Direction::Up, None),
                ("b", Direction::Down, Some(RowAction::Cancel)),
                ("c", Direction::Down, Some(RowAction::Open)),
                ("d", Direction::Down, Some(RowAction::Cancel)),
            ]
        );
        assert_eq!(list.rows[2].dest, "→ /Users/me/Previews/prod/var/log");
        assert_eq!(
            list.rows[2].status,
            RowStatus::Done("✓ Downloaded · 100 B".into())
        );

        let landed = PathBuf::from("/Users/me/Downloads/d 2");
        assert_eq!(
            transfers.finish_item(id("d"), Outcome::Done, Some(landed)),
            None
        );
        assert_eq!(
            transfers.list().unwrap().rows[3].action,
            Some(RowAction::ShowInFinder)
        );
        let ended = transfers
            .finish_item(next[0].id, Outcome::Done, None)
            .unwrap();
        assert_eq!(ended.line.body, "✓ 4 files transferred");
        assert_eq!(
            ended.notice,
            Some(("4 files transferred".into(), "with prod".into()))
        );
    }

    #[test]
    fn a_download_queue_reads_down_in_the_line_the_title_and_the_result() {
        let mut transfers = queue_of(vec![
            download("syslog", Lane::Queue, 100),
            download("auth.log", Lane::Queue, 100),
        ]);
        let now = Instant::now();
        let started = transfers.start(now);
        started[0].shared.bytes.store(50, Ordering::Release);
        let status = transfers.status(now).unwrap();
        assert_eq!(status.body, "↓ 1 of 2 · syslog  50 / 200 B");
        assert!(transfers.title_percent_changed());
        assert_eq!(transfers.title_prefix(), Some(("↓", 25)));
        assert_eq!(
            titled_as(transfers.title_prefix(), "⇄ prod"),
            "↓ 25% · ⇄ prod"
        );
        assert_eq!(transfers.list().unwrap().title, "Downloading from prod");

        assert_eq!(transfers.finish(Outcome::Done), None);
        let second = transfers.start(now);
        let ended = transfers
            .finish_item(second[0].id, Outcome::Done, None)
            .unwrap();
        assert_eq!(ended.line.body, "✓ 2 files → /Users/me/Downloads");
        assert_eq!(
            ended.notice,
            Some((
                "2 files downloaded".into(),
                "from prod to /Users/me/Downloads".into()
            ))
        );

        // The arrow follows what streams: an upload joining a preview flips it to both.
        let mut transfers = queue_of(vec![
            job("a", false, 1, 100),
            download("p", Lane::Preview, 100),
        ]);
        let _ = transfers.start(now);
        assert!(transfers.title_percent_changed());
        assert_eq!(transfers.title_prefix(), Some(("↑↓", 0)));
    }

    #[test]
    fn a_finished_preview_does_not_notify_but_a_failed_one_does() {
        let now = Instant::now();
        let mut transfers = queue_of(vec![download("app.log", Lane::Preview, 10)]);
        let started = transfers.start(now);
        let landing = PathBuf::from("/Users/me/Downloads/app.log");
        assert!(transfers.previewing(&landing), "on its way");
        assert!(!transfers.previewing(Path::new("/Users/me/Downloads/b.log")));
        let ended = transfers
            .finish_item(started[0].id, Outcome::Done, None)
            .expect("ended");
        assert!(!transfers.previewing(&landing), "landed");
        assert_eq!(ended.line.body, "✓ app.log → /Users/me/Downloads");
        assert_eq!(ended.notice, None, "the opened file is the news");
        let mut transfers = queue_of(vec![download("app.log", Lane::Preview, 10)]);
        let started = transfers.start(now);
        let ended = transfers
            .finish_item(started[0].id, Outcome::Failed("gone".into()), None)
            .expect("ended");
        assert!(ended.notice.is_some());
        // A preview beside a download keeps the download's notification.
        let mut transfers = queue_of(vec![
            download("app.log", Lane::Preview, 10),
            download("db.sql", Lane::Queue, 10),
        ]);
        let started = transfers.start(now);
        assert!(
            transfers
                .finish_item(started[0].id, Outcome::Done, None)
                .is_none()
        );
        let ended = transfers
            .finish_item(started[1].id, Outcome::Done, None)
            .expect("ended");
        assert!(ended.notice.is_some());
    }

    #[test]
    fn a_failed_preview_does_not_stop_the_waiting_queue() {
        // The lanes are independent.
        let now = Instant::now();
        let mut transfers = queue_of(vec![
            download("a.log", Lane::Queue, 10),
            download("b.log", Lane::Queue, 10),
            download("app.log", Lane::Preview, 10),
        ]);
        let started = transfers.start(now);
        let preview = started
            .iter()
            .find(|started| started.job.local.name == "app.log")
            .expect("the preview starts at once")
            .id;
        let first = started
            .iter()
            .find(|started| started.job.local.name == "a.log")
            .expect("the queue starts")
            .id;
        assert!(
            transfers
                .finish_item(preview, Outcome::Failed("denied".into()), None)
                .is_none()
        );
        assert!(transfers.finish_item(first, Outcome::Done, None).is_none());
        let next = transfers.start(now);
        assert_eq!(next.len(), 1, "the waiting item still starts");
        let ended = transfers
            .finish_item(next[0].id, Outcome::Done, None)
            .expect("ended");
        assert_eq!(
            ended.line.tone,
            TransferTone::Error,
            "the failure is still told"
        );
        assert!(ended.line.body.contains("denied"), "{}", ended.line.body);
    }

    #[test]
    fn a_download_during_the_result_line_starts_at_once() {
        // The right-click download while the last result lingers.
        let now = Instant::now();
        let mut transfers = queue_of(vec![download("a.log", Lane::Queue, 10)]);
        let started = transfers.start(now);
        let first = transfers
            .finish_item(started[0].id, Outcome::Done, None)
            .expect("ended")
            .serial;
        assert!(transfers.can_accept(), "the lingering line refuses nothing");
        assert!(transfers.enqueue(
            7,
            words(&["ssh", "prod"]),
            "prod".into(),
            HostMark::Production,
            vec![download("b.log", Lane::Queue, 10)],
        ));
        assert_eq!(started_names(&transfers.start(now)), ["b.log"]);
        assert!(
            !transfers.linger_over(first),
            "the new line replaces the result"
        );
        // So does a preview.
        let mut transfers = queue_of(vec![download("a.log", Lane::Queue, 10)]);
        let started = transfers.start(now);
        let _ = transfers.finish_item(started[0].id, Outcome::Done, None);
        assert!(transfers.enqueue(
            7,
            words(&["ssh", "prod"]),
            "prod".into(),
            HostMark::Production,
            vec![download("p.log", Lane::Preview, 10)],
        ));
        assert_eq!(started_names(&transfers.start(now)), ["p.log"]);
    }

    #[test]
    fn the_stop_question_speaks_of_the_direction() {
        let start = Instant::now();
        let long = start + STOP_ASK_AFTER + Duration::from_secs(1);
        let mut transfers = queue_of(vec![download("syslog", Lane::Queue, 96_000_000)]);
        let started = transfers.start(start);
        started[0].shared.bytes.store(48_000_000, Ordering::Release);
        let Some(Stop::Ask(question)) = transfers.stop_request(true, long) else {
            panic!("question");
        };
        assert_eq!(question.title, "Stop downloading?");
        assert_eq!(question.text, "syslog: 48.0 of 96.0 MB will be lost.");
        assert_eq!(transfers.keep_label(&question), "Keep downloading");

        // Both ways: the whole queue; the other streaming item is counted.
        let mut transfers = queue_of(vec![
            job("done.txt", false, 1, 10),
            job("backup.tar.gz", false, 1, 96_000_000),
            download("p", Lane::Preview, 100),
            download("w", Lane::Queue, 100),
        ]);
        let first = transfers.start(start);
        assert_eq!(started_names(&first), ["done.txt", "p"]);
        assert_eq!(transfers.finish(Outcome::Done), None);
        let second = transfers.start(start);
        second[0].shared.bytes.store(48_000_000, Ordering::Release);
        let Some(Stop::Ask(question)) = transfers.stop_request(true, long) else {
            panic!("question");
        };
        assert_eq!(question.title, "Stop all transfers?");
        assert_eq!(question.id, first[1].id, "the longest-streaming item");
        assert_eq!(
            question.text,
            "p: 0 of 100 B will be lost. 1 waiting file won't be downloaded. 1 other \
             transfer stops. 1 finished file stays on prod."
        );
        assert_eq!(transfers.keep_label(&question), "Keep transferring");
        // A row's `Cancel` names its own item.
        let Some(Stop::Ask(one)) = transfers.stop_request_item(Some(second[0].id), false, long)
        else {
            panic!("question");
        };
        assert_eq!(one.title, "Stop uploading?");
        assert_eq!(one.id, second[0].id);
        // Stopping the preview alone leaves the upload streaming.
        assert_eq!(transfers.stop(Some(first[1].id), false), None);
        assert!(first[1].shared.cancel.load(Ordering::Acquire));
        assert!(!second[0].shared.cancel.load(Ordering::Acquire));
        assert_eq!(
            transfers.finish_item(first[1].id, Outcome::Cancelled, None),
            None
        );
        assert!(transfers.is_running(second[0].id));
        assert!(!transfers.is_running(first[1].id));
        // A late `Cancel` on the finished row stops nothing else.
        assert_eq!(
            transfers.stop_request_item(Some(first[1].id), false, long),
            None
        );
        assert!(!second[0].shared.cancel.load(Ordering::Acquire));
    }

    #[test]
    fn the_queue_ends_only_when_nothing_streams() {
        let now = Instant::now();
        let mut transfers = queue_of(vec![
            job("a", false, 1, 10),
            download("p", Lane::Preview, 10),
        ]);
        let started = transfers.start(now);
        assert_eq!(
            transfers.finish_item(started[0].id, Outcome::Failed("tar: denied".into()), None),
            None,
            "the preview still streams"
        );
        assert!(transfers.start(now).is_empty(), "the queue is ending");
        // The preview alone still has a line and a title percent.
        assert!(
            transfers
                .status(now)
                .unwrap()
                .body
                .starts_with("↑1 ↓1 · p  ")
        );
        assert_eq!(transfers.percent(), Some(0));
        let ended = transfers
            .finish_item(started[1].id, Outcome::Done, Some("/x/p".into()))
            .unwrap();
        assert_eq!(ended.line.body, "Failed — tar: denied · 1 of 2 transferred");
        assert_eq!(
            ended.notice.map(|(title, _)| title).as_deref(),
            Some("Transfer failed")
        );

        // A single download kept beside its namesake is named as it landed.
        let mut transfers = queue_of(vec![download("report.txt", Lane::Queue, 10)]);
        let started = transfers.start(now);
        let ended = transfers
            .finish_item(
                started[0].id,
                Outcome::Done,
                Some("/Users/me/Downloads/report 2.txt".into()),
            )
            .unwrap();
        assert_eq!(ended.line.body, "✓ report 2.txt → /Users/me/Downloads");
        assert_eq!(
            ended.notice.map(|(title, _)| title).as_deref(),
            Some("report 2.txt downloaded")
        );

        // A download's full disk is this Mac's.
        let mut transfers = queue_of(vec![download("big", Lane::Queue, 10)]);
        let _ = transfers.start(now);
        let ended = transfers.finish(Outcome::DiskFull).unwrap();
        assert_eq!(
            ended.line.body,
            "Failed — disk full on this Mac · 0 of 1 downloaded"
        );
        // Cancelling the whole queue kills every streaming item.
        let mut transfers = queue_of(vec![
            job("a", false, 1, 10),
            download("f", Lane::Finder, 10),
        ]);
        let started = transfers.start(now);
        assert_eq!(transfers.stop(None, true), None);
        assert!(
            started
                .iter()
                .all(|s| s.shared.cancel.load(Ordering::Acquire))
        );
        assert_eq!(
            transfers.finish_item(started[1].id, Outcome::Cancelled, None),
            None
        );
        let ended = transfers
            .finish_item(started[0].id, Outcome::Cancelled, None)
            .unwrap();
        assert_eq!(
            ended.line.body,
            "Cancelled — 0 of 2 transferred, partial file removed"
        );
    }

    #[test]
    fn every_glyph_of_a_download_row_is_one_the_atlas_checks() {
        // `every_glyph_of_the_row_is_one_the_atlas_checks`' vocabulary, both ways.
        let mut transfers = queue_of(vec![
            job("a", false, 1, 100),
            download("b", Lane::Queue, 100),
            download("c", Lane::Preview, 100),
        ]);
        let now = Instant::now();
        let _ = transfers.start(now);
        let mut texts = vec![transfers.status(now).unwrap().body];
        let mut down = queue_of(vec![download("b", Lane::Queue, 100)]);
        let _ = down.start(now);
        texts.push(down.status(now).unwrap().body);
        texts.push(down.finish(Outcome::DiskFull).unwrap().line.body);
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
