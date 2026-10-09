//! A remote session's files: the rules of previewing (⌘-click) and
//! downloading a file named in an ssh or mosh session's output — the pure
//! half, without I/O.
//!
//! - **The helper session's protocol** ([`helper_script`], [`request_line`],
//!   [`parse_greeting`], [`parse_reply`]): one long-lived `ssh … sh` per pane
//!   answers "does this exist, what is it, how big, how old" line by
//!   line. A request is one line of `sh` the loop `eval`s, every path
//!   quoted with `upload`'s [`sq`] and rejected by its [`is_safe`] (no
//!   backslash, no control character — so no newline can split a request);
//!   the reply names paths by **index** and carries the request's sequence
//!   number, so a late answer cannot be taken for a newer question.
//! - **The load sample** ([`load_request_line`], [`parse_load`]): the
//!   `bt_load` request on the same session — raw `/proc` readings; what they
//!   mean is `remote_stats`'s.
//! - **The download script** ([`download_script`]): `upload`'s mirror, a
//!   `tar c` stream of one item out of its folder.
//! - **The scp path** ([`scp_path`]): "Copy as scp Path" from the session's
//!   ssh argv.
//! - **The download sheet** ([`download_sheet`]): whether a download asks
//!   first (a folder, a clash, no space) and with which buttons.
//! - **The open policy** ([`preview_open`]): a file previews, in its default
//!   application if it is a known document, as plain text otherwise; a folder
//!   does not preview.
//! - **The preview path** ([`preview_path`]): `{dir}/{host}/{remote absolute
//!   path}`, every escape refused.
//! - **The cleanup planner** ([`plan_sweep`]): what the launch, the daily and
//!   the Clear Now sweeps delete and what they move to the download folder
//!   instead.
//! - **The preview index's text** ([`PreviewIndex`]): what bateri wrote and
//!   when it last opened each copy, `{preview_dir}/.index`.
//!
//! The stream and the two-way queue are `download` and `upload::Transfers`,
//! the helper session `remote_helper`, the cache's disk half `preview_cache`.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use bt_core::{DownloadConflict, PreviewKeep, RemoteKind, RemoteTarget};

use crate::download::Conflict;
use crate::jobs::SSH_VALUED;
use crate::links::Content;
use crate::ports::{Bound, bound_of, parse_proc_address};
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
///
/// A third request, `bt_load {seq} [p]`, samples the host's load
/// for the ssh status bar: every line of its reply carries its own tag
/// (`BT-L cpu …`, `BT-L mem {key} {kB}`, `BT-L load …`, `BT-L up …`,
/// `BT-L disk {n}%`; with `p` also `BT-L os …`, `BT-L cores …`, the script's
/// own PID `BT-L self {pid}` and one `BT-L proc …` line per live process
/// ([`PROC_AWK`])), or `BT-NOPROC` if `/proc/stat` cannot be read — a server
/// without Linux's `/proc` has no indicator. Every source but `/proc/stat`
/// fails silently: a missing line is a missing value ([`parse_load`]).
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
         bt_load() {{ bt_s=$1; echo \"BT-R $bt_s\"; \
         if [ -r /proc/stat ]; then \
         awk '/^cpu /{{$1 = \"\"; print \"BT-L cpu\" $0; exit}}' /proc/stat; \
         awk '$1 ~ /^(MemTotal|MemAvailable|MemFree|Buffers|Cached|SwapTotal|SwapFree):$/ \
         {{sub(/:$/, \"\", $1); print \"BT-L mem \" $1 \" \" $2}}' /proc/meminfo 2>/dev/null; \
         read bt_a bt_b bt_c bt_r < /proc/loadavg 2>/dev/null && echo \"BT-L load $bt_a $bt_b $bt_c\"; \
         read bt_a bt_r < /proc/uptime 2>/dev/null && echo \"BT-L up $bt_a\"; \
         df -P / 2>/dev/null | awk 'NR > 1 {{d = $5}} END {{if (d != \"\") print \"BT-L disk \" d}}'; \
         if [ \"$2\" = p ]; then \
         sed -n 's/^PRETTY_NAME=/BT-L os /p' /etc/os-release 2>/dev/null; \
         echo \"BT-L cores $(grep -c '^cpu[0-9]' /proc/stat 2>/dev/null)\"; \
         echo \"BT-L self $$\"; awk '{PROC_AWK}' /proc/[0-9]*/stat 2>/dev/null; \
         fi; \
         else echo BT-NOPROC; fi; echo \"BT-END $bt_s\"; }}; \
         bt_ports() {{ bt_s=$1; echo \"BT-R $bt_s\"; \
         if [ -r /proc/net/tcp ]; then \
         awk -v r=\"$2\" '{PORTS_TREE_AWK}' /proc/[0-9]*/stat 2>/dev/null | \
         while read bt_p bt_c; do echo \"BT-N $bt_p $bt_c\"; \
         ls -l /proc/$bt_p/fd 2>/dev/null | awk -v p=\"$bt_p\" '{PORTS_SOCKET_AWK}'; done; \
         awk '$4 == \"0A\" {{print \"BT-T \" $2 \" \" $10}}' /proc/net/tcp /proc/net/tcp6 2>/dev/null; \
         else echo BT-NOPROC; fi; echo \"BT-END $bt_s\"; }}; \
         echo {HELPER_MARK}; printf 'BT-HOME %s\\n' \"$HOME\"; \
         while IFS= read -r bt_line; do eval \"$bt_line\"; done"
    )
}

/// The `awk` program of `bt_ports` that finds the remote shell's tree: it
/// reads every `/proc/[pid]/stat` the glob names ([`PROC_AWK`]'s way — one
/// file at a time in `BEGIN`, `comm` split at the last `)`, zombies and dead
/// tasks skipped) and prints `{pid} {comm}` for the shell `r` and each of its
/// descendants. Nothing when `r` is not alive (the session ended under the
/// request).
pub const PORTS_TREE_AWK: &str = "BEGIN { for (i = 1; i < ARGC; i++) { f = ARGV[i]; \
     if ((getline l < f) > 0 && match(l, /[)] [^)]*$/)) { h = substr(l, 1, RSTART - 1); \
     n = split(substr(l, RSTART + 2), s, \" \"); p = index(h, \" (\"); \
     if (n >= 2 && p > 1 && s[1] != \"Z\" && s[1] != \"X\") { k = substr(h, 1, p - 1); \
     up[k] = s[2]; nm[k] = substr(h, p + 2) } } \
     close(f) } \
     if (!(r in up)) exit; d[r] = 1; c = 1; \
     while (c) { c = 0; for (k in up) if (!(k in d) && (up[k] in d)) { d[k] = 1; c = 1 } } \
     for (k in d) print k \" \" nm[k] }";

/// The `awk` program of `bt_ports` over one process's `ls -l /proc/[p]/fd`:
/// `BT-S {p} {inode}` for each `socket:[inode]` link. An unreadable folder
/// (another user's process) lists nothing.
pub const PORTS_SOCKET_AWK: &str = "{ n = $NF; if (substr(n, 1, 8) == \"socket:[\") \
     print \"BT-S \" p \" \" substr(n, 9, length(n) - 9) }";

/// The `awk` program that reads every `/proc/[pid]/stat` the glob names and
/// prints `BT-L proc {pid} {ppid} {starttime} {utime} {stime} {comm}` for each
/// live process — the raw counters of `top`'s method; the difference is
/// `remote_stats::Sampler`'s (the server keeps no state).
///
/// - **`comm` is split at the last `)`** (`proc(5)`: it is in parentheses and
///   may itself carry spaces and `)`; nothing after it does). `[)]`, not `\)`:
///   the script carries no backslash it does not need (fish reads the outer
///   quoting).
/// - **Zombies (`Z`) and dead tasks (`X`) are skipped on the server**: they
///   have no CPU to show, and a host with thousands of them (the user's had
///   7 079) would otherwise send them all every sample.
/// - **One file at a time with `getline` in `BEGIN`**: a process that exits
///   between the glob and the read gives `-1`, not the fatal "cannot open" a
///   file operand gives in some `awk`s; `close` keeps the descriptors few.
/// - `utime` and `stime` are printed as read, not summed: `awk`'s numbers are
///   doubles and `mawk` prints a large one in exponent form.
pub const PROC_AWK: &str = "BEGIN { for (i = 1; i < ARGC; i++) { f = ARGV[i]; \
     if ((getline l < f) > 0 && match(l, /[)] [^)]*$/)) { h = substr(l, 1, RSTART - 1); \
     n = split(substr(l, RSTART + 2), s, \" \"); p = index(h, \" (\"); \
     if (n >= 20 && p > 1 && s[1] != \"Z\" && s[1] != \"X\") \
     print \"BT-L proc \" substr(h, 1, p - 1) \" \" s[2] \" \" s[20] \" \" s[12] \" \" s[13] \" \" substr(h, p + 2) } \
     close(f) } }";

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

// ─── listening ports ─────────────────────────────────────────────────────

/// The request line of a scan of the server's listening ports under our
/// remote shell `shell` (newline included): `bt_ports 9 4242`.
pub fn ports_request_line(seq: u64, shell: u32) -> String {
    format!("bt_ports {seq} {shell}\n")
}

/// A TCP port a process under our remote shell listens on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteListener {
    pub port: u16,
    /// The address it is bound to, as the server's `/proc` says it — where a
    /// forward points (`127.0.0.1`, `::1`, or the address itself).
    pub address: std::net::IpAddr,
    pub bound: Bound,
    pub pid: u32,
    /// `/proc/[pid]/stat`'s `comm`, control characters dropped.
    pub name: String,
}

/// The helper's output → the `bt_ports` reply to request `seq`: `Ok(None)`
/// for `BT-NOPROC` (no Linux `/proc`). One listener per port and process,
/// ascending, the widest binding kept (`0.0.0.0` beside `::1`). Lines it does
/// not know are skipped (a newer script); never a panic — the bytes are the
/// server's.
pub fn parse_ports(out: &str, seq: u64) -> Result<Option<Vec<RemoteListener>>, ReplyError> {
    let begin = format!("BT-R {seq}");
    let mut lines = out.lines().map(str::trim).skip_while(|line| *line != begin);
    if lines.next().is_none() {
        return Err(ReplyError::NotStarted);
    }
    let mut names: BTreeMap<u32, String> = BTreeMap::new();
    let mut sockets: BTreeMap<u64, u32> = BTreeMap::new();
    let mut rows: Vec<(std::net::IpAddr, u16, u64)> = Vec::new();
    let mut no_proc = false;
    for line in lines {
        if ends_reply(line, seq) {
            if no_proc {
                return Ok(None);
            }
            let mut found: Vec<RemoteListener> = rows
                .into_iter()
                .filter_map(|(address, port, inode)| {
                    let pid = *sockets.get(&inode)?;
                    Some(RemoteListener {
                        port,
                        address,
                        bound: bound_of(address),
                        pid,
                        name: names.get(&pid).cloned().unwrap_or_default(),
                    })
                })
                .filter(|listener| listener.port != 0)
                .collect();
            found.sort_by_key(|listener| (listener.port, listener.pid, listener.bound));
            found.dedup_by(|later, kept| later.port == kept.port && later.pid == kept.pid);
            return Ok(Some(found));
        }
        if line == "BT-NOPROC" {
            no_proc = true;
            continue;
        }
        let mut fields = line.splitn(3, ' ');
        match (fields.next(), fields.next(), fields.next()) {
            (Some("BT-N"), Some(pid), name) => {
                if let Ok(pid) = pid.parse() {
                    names.insert(pid, clean(name.unwrap_or_default()));
                }
            }
            (Some("BT-S"), Some(pid), Some(inode)) => {
                if let (Ok(pid), Ok(inode)) = (pid.parse(), inode.trim().parse()) {
                    sockets.insert(inode, pid);
                }
            }
            (Some("BT-T"), Some(local), Some(inode)) => {
                if let (Some((address, port)), Ok(inode)) =
                    (parse_proc_address(local), inode.trim().parse())
                {
                    rows.push((address, port, inode));
                }
            }
            _ => {}
        }
    }
    Err(ReplyError::Unterminated)
}

// ─── load sample ─────────────────────────────────────────────────────────

/// The request line of a load sample (newline included): `bt_load 9` or, with
/// the popover's details, `bt_load 9 p`. It shares the session's sequence
/// numbers with [`request_line`], so no reply is taken for another's.
pub fn load_request_line(seq: u64, detail: bool) -> String {
    if detail {
        format!("bt_load {seq} p\n")
    } else {
        format!("bt_load {seq}\n")
    }
}

/// `/proc/stat`'s aggregate `cpu` line, reduced to what a percentage needs.
/// The percentage is the difference of two readings (`remote_stats::Sampler`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CpuCounters {
    /// All time, in jiffies: `user` through `steal` (the `guest` columns are
    /// already inside `user` and `nice`).
    pub total: u64,
    /// Idle time: `idle` + `iowait`.
    pub idle: u64,
}

/// One process of the popover's top three (`remote_stats::Sampler` derives it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Process {
    /// `/proc/[pid]/stat`'s `comm`, control characters dropped (the name is
    /// the server's).
    pub name: String,
    /// CPU between two samples in **tenths** of a percent, one core = 100 %
    /// (`top`'s Irix mode; an integer: the answer is `Eq`).
    pub cpu: u32,
}

/// One live process's raw counters from `/proc/[pid]/stat` ([`PROC_AWK`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
    pub pid: u32,
    pub ppid: u32,
    /// `starttime` (clock ticks after boot): with the PID, the process's
    /// identity — a PID reused between two samples is another process.
    pub start: u64,
    /// `utime + stime`, clock ticks.
    pub ticks: u64,
    /// `comm`, control characters dropped.
    pub name: String,
}

/// The process scan of a `p` sample.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProcessScan {
    /// The helper's `sh` (`$$`): it and its children are our own measuring,
    /// not the server's load.
    pub self_pid: Option<u32>,
    pub tasks: Vec<Task>,
}

/// One `bt_load` reply ([`parse_load`]): raw readings, nothing derived yet.
/// Sizes are in bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LoadSample {
    pub cpu: CpuCounters,
    pub mem_total: u64,
    /// `MemAvailable`, or `MemFree + Buffers + Cached` on a kernel before 3.14.
    pub mem_available: u64,
    pub swap_total: u64,
    pub swap_free: u64,
    /// The 1, 5 and 15 minute load averages, in **hundredths**.
    pub load: Option<[u32; 3]>,
    /// Seconds since boot.
    pub uptime: Option<u64>,
    /// The root file system's use, % (`df -P /`'s own rounding).
    pub disk: Option<u8>,
    /// `PRETTY_NAME` of `/etc/os-release` — only with the `p` flag.
    pub os: Option<String>,
    /// The number of `cpuN` lines — only with the `p` flag.
    pub cores: Option<u32>,
    /// Every live process's counters — only with the `p` flag.
    pub scan: Option<ProcessScan>,
}

/// The helper's output → the `bt_load` reply to request `seq`: `Ok(None)` for
/// `BT-NOPROC` (no Linux `/proc`, no indicator).
///
/// Only the `cpu` line and `MemTotal` are required; every other value is
/// optional and a missing line leaves it empty. An unknown `BT-L` tag is
/// skipped (a newer script), any other line inside the reply — and a known
/// required line that does not parse — is [`ReplyError::Malformed`]. Never a
/// panic: the bytes are the server's.
pub fn parse_load(out: &str, seq: u64) -> Result<Option<LoadSample>, ReplyError> {
    let begin = format!("BT-R {seq}");
    let mut lines = out.lines().map(str::trim).skip_while(|line| *line != begin);
    if lines.next().is_none() {
        return Err(ReplyError::NotStarted);
    }
    let mut sample = LoadSample::default();
    let mut cpu = false;
    let mut mem = MemFields::default();
    let mut no_proc = false;
    for line in lines {
        if ends_reply(line, seq) {
            if no_proc {
                return Ok(None);
            }
            let (Some(total), true) = (mem.total, cpu) else {
                return Err(ReplyError::Malformed("no cpu or MemTotal line".to_owned()));
            };
            sample.mem_total = total;
            sample.mem_available = mem.available.unwrap_or_else(|| {
                mem.free
                    .unwrap_or(0)
                    .saturating_add(mem.buffers.unwrap_or(0))
                    .saturating_add(mem.cached.unwrap_or(0))
            });
            sample.swap_total = mem.swap_total.unwrap_or(0);
            sample.swap_free = mem.swap_free.unwrap_or(0);
            return Ok(Some(sample));
        }
        let malformed = || ReplyError::Malformed(line.to_owned());
        if line == "BT-NOPROC" {
            no_proc = true;
            continue;
        }
        let rest = line.strip_prefix("BT-L ").ok_or_else(malformed)?;
        let (tag, value) = rest.split_once(' ').unwrap_or((rest, ""));
        let value = value.trim();
        match tag {
            "cpu" => {
                sample.cpu = cpu_counters(value).ok_or_else(malformed)?;
                cpu = true;
            }
            "mem" => mem.read(value).ok_or_else(malformed)?,
            "load" => sample.load = load_averages(value),
            "up" => sample.uptime = whole_seconds(value),
            "disk" => {
                sample.disk = value
                    .strip_suffix('%')
                    .and_then(|n| n.parse::<u8>().ok())
                    .map(|n| n.min(100));
            }
            "os" => {
                let name = clean(value.trim_matches(|c| c == '"' || c == '\''));
                sample.os = (!name.is_empty()).then_some(name);
            }
            "cores" => sample.cores = value.parse().ok().filter(|&n| n > 0),
            "self" => {
                sample
                    .scan
                    .get_or_insert_with(ProcessScan::default)
                    .self_pid = value.parse().ok();
            }
            "proc" => {
                let scan = sample.scan.get_or_insert_with(ProcessScan::default);
                if let Some(task) = task(value) {
                    scan.tasks.push(task);
                }
            }
            _ => {}
        }
    }
    Err(ReplyError::Unterminated)
}

/// `/proc/meminfo`'s fields as they arrive, in kB.
#[derive(Default)]
struct MemFields {
    total: Option<u64>,
    available: Option<u64>,
    free: Option<u64>,
    buffers: Option<u64>,
    cached: Option<u64>,
    swap_total: Option<u64>,
    swap_free: Option<u64>,
}

impl MemFields {
    /// One `{key} {kB}` line; the value is kept in bytes. `None` if the number
    /// does not parse; an unknown key is skipped.
    fn read(&mut self, value: &str) -> Option<()> {
        let (key, kb) = value.split_once(' ')?;
        let bytes = kb.trim().parse::<u64>().ok()?.saturating_mul(1024);
        let slot = match key {
            "MemTotal" => &mut self.total,
            "MemAvailable" => &mut self.available,
            "MemFree" => &mut self.free,
            "Buffers" => &mut self.buffers,
            "Cached" => &mut self.cached,
            "SwapTotal" => &mut self.swap_total,
            "SwapFree" => &mut self.swap_free,
            _ => return Some(()),
        };
        *slot = Some(bytes);
        Some(())
    }
}

/// `user nice system idle [iowait irq softirq steal guest guest_nice]` → the
/// counters; at least the first four (the oldest kernels' line).
fn cpu_counters(value: &str) -> Option<CpuCounters> {
    let mut fields = [0u64; 8];
    let mut count = 0;
    for (slot, field) in fields.iter_mut().zip(value.split_whitespace()) {
        *slot = field.parse().ok()?;
        count += 1;
    }
    if count < 4 {
        return None;
    }
    let total = fields.iter().fold(0u64, |sum, &n| sum.saturating_add(n));
    Some(CpuCounters {
        total,
        idle: fields[3].saturating_add(fields[4]),
    })
}

/// A non-negative decimal → integer units of `1 / scale` (hundredths of a load
/// average, tenths of a percent); `None` if it is not one.
fn scaled(text: &str, scale: f64) -> Option<u32> {
    let value: f64 = text.parse().ok()?;
    let scaled = (value * scale).round();
    (scaled.is_finite() && (0.0..=f64::from(u32::MAX)).contains(&scaled)).then_some(scaled as u32)
}

fn load_averages(value: &str) -> Option<[u32; 3]> {
    let mut fields = value.split_whitespace().map(|field| scaled(field, 100.0));
    Some([fields.next()??, fields.next()??, fields.next()??])
}

/// `/proc/uptime`'s `12345.67` → whole seconds.
fn whole_seconds(value: &str) -> Option<u64> {
    value.split('.').next()?.parse().ok()
}

/// `{pid} {ppid} {starttime} {utime} {stime} {comm}` → a task; `None` if a
/// number does not parse. `comm` is the rest of the line, spaces and `)`
/// included.
fn task(value: &str) -> Option<Task> {
    let mut rest = value;
    let mut number = || {
        let (field, tail) = rest.split_once(' ').unwrap_or((rest, ""));
        rest = tail;
        field.parse::<u64>().ok()
    };
    let pid = u32::try_from(number()?).ok()?;
    let ppid = u32::try_from(number()?).ok()?;
    let start = number()?;
    let ticks = number()?.checked_add(number()?)?;
    Some(Task {
        pid,
        ppid,
        start,
        ticks,
        name: clean(rest),
    })
}

/// A string from the server, without control characters (a process name is
/// whatever the process chose).
fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
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
    // A symlink is followed (`-h`) only when it is the item itself: the helper
    // answered for its target (`stat -L`), and an archived link would land
    // dangling. Links **inside** a folder stay links.
    let item = sq(&format!("./{name}"));
    Some(format!(
        "cd {} || exit {NO_DIRECTORY}; if [ -L {item} ]; then exec tar -c -h -f - {item}; \
         else exec tar -c -f - {item}; fi",
        sq(dir),
    ))
}

/// The download's confirmation sheet: the text and the confirm buttons, each
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

/// Whether a download asks first and how: `Ok(conflict)` starts
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

/// "Copy as scp Path": the remote item as `scp` names it —
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

/// `word` as is when every character reads literally in a POSIX shell **and**
/// zsh, single-quoted otherwise — `[`/`]` are a glob in zsh (an IPv6 host's
/// `[::1]`, `app[1].log`).
fn quoted(word: &str) -> String {
    let plain = word.chars().all(|c| {
        c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | ':' | '@' | '+' | ',')
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

/// How a remote file's preview opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewOpen {
    /// In the default application of its content type — a known document.
    Default,
    /// In the default plain-text application: a script, a program, an `x`-bit
    /// file and every unknown type — the user wants to **read** it, and a
    /// preview is never run.
    PlainText,
}

/// The remote open policy: what a ⌘-click on `entry` does. `content` is the
/// platform's answer for the file's name (the link opener's `DOCUMENT_TYPES`,
/// UTType on macOS) — asked only for a file. A folder does not preview (`None`).
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
/// — the same file always lands on the same copy, which is what lets an
/// unchanged file open from the cache.
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

/// What runs a cleanup.
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

/// The cleanup planner: which of `previews` the `sweep` removes,
/// `keep` and `limit` (bytes) from the settings, `now` in Unix seconds.
///
/// - The age is counted from the **last opening**, so a preview in use stays.
/// - [`Sweep::Launch`] removes the expired ones (every one under
///   [`PreviewKeep::UntilLaunch`]), then — while the folder is above `limit` —
///   the oldest opened first. [`Sweep::Daily`] only the expired ones, and
///   nothing under `UntilLaunch` (those are this session's). [`Sweep::ClearNow`]
///   every one.
/// - A copy whose size or mtime differs from what bateri wrote is never
///   deleted: it goes to [`SweepPlan::rescue`] on every trigger that would take
///   it — and [`Sweep::Launch`] and [`Sweep::ClearNow`] rescue **every** changed
///   copy, due or not: the user's edits must not wait a week in a cache folder
///   (the daily sweep, while the copy may be open, only takes the due ones).
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
        Sweep::Launch => known
            .iter()
            .copied()
            .filter(|preview| expired(preview) || preview.diverged())
            .collect(),
        Sweep::Daily => known
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

// ─── preview index ───────────────────────────────────────────────────────

/// The preview index's first line: a file without it is not ours (or another
/// version's) and reads as damaged.
const INDEX_HEADER: &str = "bateri-previews 1";

/// One copy's record: what bateri wrote and when it last opened it. Times are
/// Unix seconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IndexRecord {
    /// The size and mtime the copy had when bateri finished writing it (tar
    /// keeps the remote mtime, so it is also the remote's).
    pub written: (u64, u64),
    pub last_open: u64,
}

/// The preview folder's index, `{preview_dir}/.index`: a record
/// per copy, by its path **relative** to the folder. It is what tells bateri's
/// copy from the user's edit and an unchanged remote file from a changed
/// one.
///
/// The text is a header line and one line per copy, `{last_open} {size}
/// {mtime} {path}` — the path last, so it may hold spaces. A path with a
/// newline is never written (the helper refuses such names, [`is_safe`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PreviewIndex {
    pub records: BTreeMap<String, IndexRecord>,
}

/// Why an index could not be read: the sweep then deletes nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexDamaged(pub String);

impl PreviewIndex {
    /// The index's text → its records. Any line that is not the header's or a
    /// record's damages the whole index: a half-read index would make the
    /// copies it lost look unknown, which is safe, but the copies it misread
    /// could be deleted.
    pub fn parse(text: &str) -> Result<Self, IndexDamaged> {
        let mut lines = text.lines();
        if lines.next() != Some(INDEX_HEADER) {
            return Err(IndexDamaged("no header".to_owned()));
        }
        let mut records = BTreeMap::new();
        for line in lines {
            let damaged = || IndexDamaged(line.to_owned());
            let mut fields = line.splitn(4, ' ');
            let mut number = || -> Result<u64, IndexDamaged> {
                fields
                    .next()
                    .and_then(|field| field.parse().ok())
                    .ok_or_else(damaged)
            };
            let (last_open, size, mtime) = (number()?, number()?, number()?);
            let path = fields
                .next()
                .filter(|path| !path.is_empty())
                .ok_or_else(damaged)?;
            records.insert(
                path.to_owned(),
                IndexRecord {
                    written: (size, mtime),
                    last_open,
                },
            );
        }
        Ok(Self { records })
    }

    /// The index's text ([`PreviewIndex::parse`]'s inverse); a path with a
    /// newline or a carriage return is left out.
    pub fn render(&self) -> String {
        let mut text = format!("{INDEX_HEADER}\n");
        for (path, record) in &self.records {
            if path.contains(['\n', '\r']) || path.is_empty() {
                continue;
            }
            let (size, mtime) = record.written;
            let _ = writeln!(text, "{} {size} {mtime} {path}", record.last_open);
        }
        text
    }
}

/// Whether a cached copy can open as it is: the remote file's size and
/// mtime are what bateri wrote, and the local copy is still what bateri wrote.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheState {
    /// Open the copy; nothing downloads.
    Fresh,
    /// No copy, or the remote file changed: download it.
    Stale,
    /// The local copy changed since bateri wrote it (the user unlocked and
    /// edited it) or the index has no record of it: it is moved to the download
    /// folder before a new copy takes its place — a re-download must not destroy
    /// what may be the user's edits (the unknown copy's side is the sweep's,
    /// which never deletes one).
    Diverged,
}

/// [`CacheState`] from the record, the local copy's size and mtime now (`None`:
/// no copy) and the remote's (`None` fields: the server's `stat` gave none —
/// then nothing can be compared and it downloads).
pub fn cache_state(
    record: Option<&IndexRecord>,
    local: Option<(u64, u64)>,
    remote: (Option<u64>, Option<u64>),
) -> CacheState {
    let Some(local) = local else {
        return CacheState::Stale;
    };
    let Some(record) = record else {
        return CacheState::Diverged;
    };
    if local != record.written {
        return CacheState::Diverged;
    }
    match remote {
        (Some(size), Some(mtime)) if (size, mtime) == record.written => CacheState::Fresh,
        _ => CacheState::Stale,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_port_reply_joins_sockets_to_their_processes() {
        let out = "noise from an rc file\n\
                   BT-R 4\n\
                   BT-N 900 node\n\
                   BT-S 900 777\n\
                   BT-S 900 778\n\
                   BT-N 901 python3 -m\n\
                   BT-S 901 779\n\
                   BT-T 0100007F:1435 777\n\
                   BT-T 00000000000000000000000001000000:1435 778\n\
                   BT-T 00000000:1F90 779\n\
                   BT-T 00000000:0016 12\n\
                   BT-Z something newer\n\
                   BT-END 4\n";
        let found = parse_ports(out, 4).expect("a reply").expect("a /proc");
        assert_eq!(
            found
                .iter()
                .map(|l| (l.port, l.pid, l.bound, l.name.as_str()))
                .collect::<Vec<_>>(),
            [
                (5173, 900, Bound::Loopback, "node"),
                (8080, 901, Bound::Any, "python3 -m"),
            ],
            "one per port and process; sshd's :22 (not ours) is not there"
        );
        assert_eq!(found[0].address, std::net::IpAddr::from([127, 0, 0, 1]));
        assert_eq!(parse_ports("BT-R 5\nBT-NOPROC\nBT-END 5\n", 5), Ok(None));
        assert_eq!(parse_ports("BT-R 5\n", 5), Err(ReplyError::Unterminated));
        assert_eq!(
            parse_ports("BT-R 6\nBT-END 6\n", 5),
            Err(ReplyError::NotStarted)
        );
        assert_eq!(ports_request_line(9, 4242), "bt_ports 9 4242\n");
    }
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
    fn a_load_reply_gives_the_raw_readings() {
        assert_eq!(load_request_line(9, false), "bt_load 9\n");
        assert_eq!(load_request_line(9, true), "bt_load 9 p\n");
        let out = "noise\nBT-R 9\n\
                   BT-L cpu 100 20 30 800 50 0 0 0 0 0\n\
                   BT-L mem MemTotal 2000\nBT-L mem MemFree 100\n\
                   BT-L mem MemAvailable 800\nBT-L mem SwapTotal 1000\n\
                   BT-L mem SwapFree 750\nBT-L load 0.52 1.00 12.25\n\
                   BT-L up 12345.67\nBT-L disk 54%\n\
                   BT-L os \"Ubuntu 22.04.3 LTS\"\nBT-L cores 4\nBT-L self 77\n\
                   BT-L proc 812 1 4000 1200 300 postgres\n\
                   BT-L proc 900 1 5000 7 3 tmux: server\n\
                   BT-L future 1 2 3\nBT-END 9\n";
        let sample = parse_load(out, 9).expect("readable").expect("a sample");
        assert_eq!(
            sample.cpu,
            CpuCounters {
                total: 1000,
                idle: 850
            }
        );
        assert_eq!(sample.mem_total, 2000 * 1024);
        assert_eq!(sample.mem_available, 800 * 1024);
        assert_eq!(
            (sample.swap_total, sample.swap_free),
            (1000 * 1024, 750 * 1024)
        );
        assert_eq!(sample.load, Some([52, 100, 1225]));
        assert_eq!(sample.uptime, Some(12345));
        assert_eq!(sample.disk, Some(54));
        assert_eq!(sample.os.as_deref(), Some("Ubuntu 22.04.3 LTS"));
        assert_eq!(sample.cores, Some(4));
        assert_eq!(
            sample.scan,
            Some(ProcessScan {
                self_pid: Some(77),
                tasks: vec![
                    Task {
                        pid: 812,
                        ppid: 1,
                        start: 4000,
                        ticks: 1500,
                        name: "postgres".to_owned(),
                    },
                    Task {
                        pid: 900,
                        ppid: 1,
                        start: 5000,
                        ticks: 10,
                        name: "tmux: server".to_owned(),
                    },
                ],
            })
        );
        // Another sequence number's reply is not this one.
        assert_eq!(parse_load(out, 8), Err(ReplyError::NotStarted));
        assert_eq!(parse_load("BT-R 1\nBT-NOPROC\nBT-END 1\n", 1), Ok(None));
    }

    #[test]
    fn a_load_reply_without_optional_lines_is_still_a_sample() {
        // A kernel before 3.14 (no MemAvailable), no swap, no df, no ps, the
        // oldest four-column cpu line.
        let out = "BT-R 2\nBT-L cpu 10 0 10 80\nBT-L mem MemTotal 1000\n\
                   BT-L mem MemFree 100\nBT-L mem Buffers 50\nBT-L mem Cached 250\n\
                   BT-L cores 0\nBT-L os \nBT-END 2\n";
        let sample = parse_load(out, 2).expect("readable").expect("a sample");
        assert_eq!(
            sample.cpu,
            CpuCounters {
                total: 100,
                idle: 80
            }
        );
        assert_eq!(sample.mem_available, 400 * 1024);
        assert_eq!(
            (sample.swap_total, sample.load, sample.uptime),
            (0, None, None)
        );
        assert_eq!((sample.disk, sample.cores, sample.os), (None, None, None));
        assert_eq!(sample.scan, None, "no `p`, no scan");
    }

    #[test]
    fn a_broken_load_reply_is_an_error_not_a_panic() {
        let reply = |body: &str| format!("BT-R 3\n{body}BT-END 3\n");
        let base = "BT-L cpu 1 2 3 4\nBT-L mem MemTotal 10\n";
        assert!(parse_load(&reply(base), 3).expect("readable").is_some());
        for broken in [
            "BT-L cpu 1 2 3\nBT-L mem MemTotal 10\n",
            "BT-L cpu 1 2 x 4\nBT-L mem MemTotal 10\n",
            "BT-L mem MemTotal 10\n",
            "BT-L cpu 1 2 3 4\n",
            "BT-L cpu 1 2 3 4\nBT-L mem MemTotal ten\n",
            "BT-L cpu 1 2 3 4\nBT-L mem MemTotal 10\nnot ours\n",
            "BT-L cpu 99999999999999999999 2 3 4\nBT-L mem MemTotal 10\n",
        ] {
            assert!(
                matches!(parse_load(&reply(broken), 3), Err(ReplyError::Malformed(_))),
                "{broken:?}"
            );
        }
        assert_eq!(
            parse_load("BT-R 3\nBT-L cpu 1 2 3 4\n", 3),
            Err(ReplyError::Unterminated)
        );
        // Optional values that do not parse are just missing; a process name
        // loses its control characters, a process whose numbers do not parse
        // is dropped.
        let odd = format!(
            "{base}BT-L load 1 x 2\nBT-L up soon\nBT-L disk 120%\nBT-L disk full\n\
             BT-L self x\nBT-L proc 5 1 9 10 5 evil\x1b[2Jname\x07\nBT-L proc 6 1 9 x 1 a\n\
             BT-L proc -1 1 9 1 1 b\nBT-L proc 7 1 9 1\n\
             BT-L proc 8 1 9 18446744073709551615 1 c\n"
        );
        let sample = parse_load(&reply(&odd), 3)
            .expect("readable")
            .expect("a sample");
        assert_eq!(
            (sample.load, sample.uptime, sample.disk),
            (None, None, None)
        );
        assert_eq!(
            sample.scan,
            Some(ProcessScan {
                self_pid: None,
                tasks: vec![Task {
                    pid: 5,
                    ppid: 1,
                    start: 9,
                    ticks: 15,
                    name: "evil[2Jname".to_owned(),
                }],
            })
        );
    }

    /// [`PROC_AWK`] in this machine's `awk` (BSD here, `mawk`/`gawk` under
    /// `make linux`) over hand-made `/proc/[pid]/stat` files: the fields are
    /// `proc(5)`'s (`ppid` 4, `utime` 14, `stime` 15, `starttime` 22), `comm`
    /// is cut at the **last** `)` with its spaces and parentheses, a zombie
    /// and a short line are skipped, a file that vanished does not stop the
    /// scan.
    #[test]
    fn the_proc_scan_reads_stat_files_in_awk() {
        let dir = std::env::temp_dir().join(format!("bt-proc-awk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch folder");
        // Fields 3.. numbered so a shifted index shows: state, ppid 4, pgrp 5, …
        let tail = |state: &str| {
            format!(
                "{state} 104 105 106 107 108 109 110 111 112 113 1400 1500 116 117 118 119 \
                 120 121 22000 123 124 125"
            )
        };
        let files = [
            ("1", format!("4242 (tmux: server) {}\n", tail("S"))),
            ("2", format!("4243 (a) (b) {}\n", tail("R"))),
            ("3", format!("4244 (gone) {}\n", tail("Z"))),
            ("4", "4245 (short) S 1 2 3\n".to_owned()),
        ];
        let mut paths: Vec<_> = files
            .iter()
            .map(|(name, body)| {
                let path = dir.join(name);
                std::fs::write(&path, body).expect("a stat file");
                path
            })
            .collect();
        paths.insert(1, dir.join("vanished"));
        let out = std::process::Command::new("awk")
            .arg(PROC_AWK)
            .args(&paths)
            .output()
            .expect("awk runs");
        let _ = std::fs::remove_dir_all(&dir);
        let text = String::from_utf8_lossy(&out.stdout);
        assert_eq!(
            text.lines().collect::<Vec<_>>(),
            [
                "BT-L proc 4242 104 22000 1400 1500 tmux: server",
                "BT-L proc 4243 104 22000 1400 1500 a) (b",
            ],
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let parsed = task(
            text.lines()
                .nth(1)
                .and_then(|l| l.strip_prefix("BT-L proc "))
                .unwrap_or(""),
        );
        assert_eq!(
            parsed.map(|task| (task.ticks, task.name)),
            Some((2900, "a) (b".to_owned()))
        );
    }

    #[test]
    fn the_download_script_streams_one_item_out_of_its_folder() {
        assert_eq!(
            download_script("/var/log/app.log"),
            Some(format!(
                "cd '/var/log' || exit {NO_DIRECTORY}; if [ -L './app.log' ]; then exec tar -c -h -f - './app.log'; else exec tar -c -f - './app.log'; fi"
            ))
        );
        assert_eq!(
            download_script("/-rf"),
            Some(format!(
                "cd '/' || exit {NO_DIRECTORY}; if [ -L './-rf' ]; then exec tar -c -h -f - './-rf'; else exec tar -c -f - './-rf'; fi"
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
        assert_eq!(scp_path(&ssh(&["ssh", "::1"]), "/x"), "'[::1]:/x'");
        assert_eq!(
            scp_path(&ssh(&["ssh", "ssh://u@[::1]:2222"]), "/x"),
            "-P 2222 'u@[::1]:/x'"
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

    #[test]
    fn the_index_round_trips_and_a_damaged_one_is_refused() {
        let mut index = PreviewIndex::default();
        index.records.insert(
            "prod/var/log/app.log".to_owned(),
            IndexRecord {
                written: (120, 1_700_000_000),
                last_open: 1_700_000_500,
            },
        );
        index.records.insert(
            "prod/home/me/My Notes.txt".to_owned(),
            IndexRecord {
                written: (0, 5),
                last_open: 9,
            },
        );
        let text = index.render();
        assert!(text.starts_with("bateri-previews 1\n"), "{text}");
        assert_eq!(PreviewIndex::parse(&text), Ok(index.clone()));
        assert_eq!(
            PreviewIndex::parse("bateri-previews 1\n"),
            Ok(PreviewIndex::default())
        );
        // A path that would split a line is never written.
        index.records.insert(
            "prod/a\nb".to_owned(),
            IndexRecord {
                written: (1, 1),
                last_open: 1,
            },
        );
        assert_eq!(
            PreviewIndex::parse(&index.render()).unwrap().records.len(),
            2
        );
        for damaged in [
            "",
            "garbage",
            "bateri-previews 2\n",
            "bateri-previews 1\n1 2 3\n",
            "bateri-previews 1\nx 2 3 a\n",
            "bateri-previews 1\n1 2 3 a\n1 2\n",
        ] {
            assert!(PreviewIndex::parse(damaged).is_err(), "{damaged:?}");
        }
    }

    #[test]
    fn an_unchanged_copy_opens_and_an_edited_one_is_never_overwritten() {
        let record = IndexRecord {
            written: (10, 500),
            last_open: 0,
        };
        let some = (Some(10), Some(500));
        assert_eq!(
            cache_state(Some(&record), Some((10, 500)), some),
            CacheState::Fresh
        );
        // The remote file changed, or its `stat` said nothing: download.
        for remote in [
            (Some(11), Some(500)),
            (Some(10), Some(501)),
            (None, Some(500)),
            (Some(10), None),
        ] {
            assert_eq!(
                cache_state(Some(&record), Some((10, 500)), remote),
                CacheState::Stale,
                "{remote:?}"
            );
        }
        assert_eq!(cache_state(Some(&record), None, some), CacheState::Stale);
        assert_eq!(cache_state(None, None, some), CacheState::Stale);
        // The user edited the copy, or nothing says it is bateri's: keep it.
        assert_eq!(
            cache_state(Some(&record), Some((10, 900)), some),
            CacheState::Diverged
        );
        assert_eq!(
            cache_state(None, Some((10, 500)), some),
            CacheState::Diverged
        );
    }

    #[test]
    fn the_launch_rescues_every_changed_copy_the_daily_only_the_due() {
        let mut edited = preview("edited", 12, 1);
        edited.mtime = 2_000;
        let previews = [edited, preview("fresh", 1, 1)];
        let launch = plan_sweep(&previews, Sweep::Launch, PreviewKeep::Week, 1_000, NOW);
        assert_eq!(launch.rescue, paths(&["edited"]));
        assert!(launch.delete.is_empty());
        let daily = plan_sweep(&previews, Sweep::Daily, PreviewKeep::Week, 1_000, NOW);
        assert_eq!(daily, SweepPlan::default());
    }
}
