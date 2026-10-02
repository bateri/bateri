//! The helper ssh session of a remote pane (045 Karar 10): one long-lived
//! `ssh … sh` that answers "does this exist, what is it, how big" line by line,
//! so a ⌘-hover over a name in a remote `ls` costs one round trip on an open
//! connection, not a new ssh handshake.
//!
//! - **[`HelperSession`]** is the process: the route gate's argv ([`Dial`]) +
//!   [`crate::remote_files::helper_script`], a reader thread turning its
//!   standard output into lines and every wait bounded ([`OPEN_TIMEOUT`],
//!   [`STAT_TIMEOUT`], [`COUNT_TIMEOUT`]) — a hung ssh never blocks for good.
//!   `BatchMode=yes` (the upload's argv): no password can be asked, a server
//!   that wants one fails the open and the reason goes to the pane's label (R1.3).
//! - **[`RemoteHelper`]** is the pane's handle: a worker thread born at the
//!   first question (lazy), owning the session and the answer cache
//!   ([`RemoteCache`]). A question carries the remote session's generation
//!   (`Session::remote_target`'s command): another generation closes the old
//!   session and opens a new one, and so does [`RemoteHelper::close`] (the
//!   pane closing) and [`IDLE`] without a question. While the load indicator
//!   samples ([`Query::Load`], 046 Karar 1) a question comes every few seconds,
//!   so the session stays open; it closes [`IDLE`] after sampling stops.
//! - **Resolution** ([`remote_paths`]) is `links::resolve`'s, on the remote
//!   disk: `~` from the helper's greeting, a relative name from the remote
//!   OSC 7 directory and **no** relative name without it (Karar 2-A, R1.2).
//!
//! Platform-free like `upload`'s processes; the platform shell takes the answer
//! back to its main thread. The rationale is in
//! `.tasks/045-uzak-dosya-indirme/discussion.md` → Karar 1, 2, 10.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use crate::links::{self, Entry};
use crate::remote_files::{
    Ask, LoadSample, RemoteEntry, ends_reply, helper_script, load_request_line, parse_greeting,
    parse_load, parse_reply, request_line,
};
use crate::upload::{collect_stderr, is_safe, last_line, remote_command};

/// How long the helper may take to greet — an ssh handshake, a jump host. A
/// **design constant**, not a measurement: generous, because a server that
/// answers slowly once answers fast afterwards on the open connection.
pub const OPEN_TIMEOUT: Duration = Duration::from_secs(15);

/// How long a [`Ask::Stat`] reply may take: a handful of `stat`s. Design constant.
pub const STAT_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a [`Ask::Count`] reply may take: it walks a whole folder tree (the
/// download sheet's file count). Design constant.
pub const COUNT_TIMEOUT: Duration = Duration::from_secs(120);

/// How long a load sample's reply may take (046): a few `cat`s, a `df` and,
/// with the popover open, a `ps`. Design constant — short, because the
/// indicator hides on a timeout and the next tick asks again.
pub const LOAD_TIMEOUT: Duration = Duration::from_secs(10);

/// A session with no question for this long closes (R1.3): an idle ssh
/// connection is not held open for the whole remote session. Design constant —
/// long enough that hovering over a listing, reading, then hovering again does
/// not pay a new handshake.
pub const IDLE: Duration = Duration::from_secs(120);

/// After a failed open, the same generation's questions get the same answer
/// for this long without a new ssh: hovering over every word of an unreachable
/// server's listing must not start one connection per word. Design constant.
pub const RETRY_AFTER: Duration = Duration::from_secs(10);

/// The pane label's text when a relative name cannot be resolved because the
/// remote shell has not reported its folder (Karar 2-A, R1.2).
pub const REMOTE_CWD_UNKNOWN: &str = "Remote folder unknown — enable OSC 7 on the server";

/// The open helper process.
pub struct HelperSession {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    stderr: Option<thread::JoinHandle<String>>,
    home: Option<String>,
    seq: u64,
}

impl HelperSession {
    /// Starts `ssh` (the argv without the remote command) running the helper
    /// script and waits up to `timeout` for its greeting. `Err` is the pane
    /// label's text: why there is no link (R1.3).
    pub fn open(ssh: &[String], host: &str, timeout: Duration) -> Result<Self, String> {
        let Some((program, args)) = ssh.split_first() else {
            return Err(format!("No ssh command for {host}"));
        };
        let mut child = Command::new(program)
            .args(args)
            .arg(remote_command(&helper_script()))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("ssh could not be started: {error}"))?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("ssh could not be started for {host}"));
        };
        let stderr = Some(collect_stderr(child.stderr.take(), None));
        let (tx, lines) = mpsc::channel();
        thread::Builder::new()
            .name("remote helper output".into())
            .spawn(move || {
                // Bytes, not `lines()`: a remote rc file's non-UTF-8 banner must
                // not end the session before the greeting (`/code-review`, 045).
                let mut reader = BufReader::new(stdout);
                let mut bytes = Vec::new();
                loop {
                    bytes.clear();
                    match reader.read_until(b'\n', &mut bytes) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                    if bytes.last() == Some(&b'\n') {
                        bytes.pop();
                        if bytes.last() == Some(&b'\r') {
                            bytes.pop();
                        }
                    }
                    let line = String::from_utf8_lossy(&bytes).into_owned();
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            })
            .map_err(|error| format!("ssh could not be started: {error}"))?;
        let mut session = HelperSession {
            child,
            stdin,
            lines,
            stderr,
            home: None,
            seq: 0,
        };
        let deadline = Instant::now() + timeout;
        let mut out = String::new();
        loop {
            match session.next_line(deadline) {
                Ok(line) => {
                    out.push_str(&line);
                    out.push('\n');
                    if let Some(home) = parse_greeting(&out) {
                        session.home = home;
                        return Ok(session);
                    }
                }
                Err(timed_out) => return Err(session.failure(host, timed_out)),
            }
        }
    }

    /// The remote home directory from the greeting (`~` resolves to it).
    pub fn home(&self) -> Option<&str> {
        self.home.as_deref()
    }

    /// One request: what each of `paths` is, by index (`None` — it does not
    /// exist). `Err` if a path cannot safely enter the script, the connection
    /// closed or the reply did not come in `timeout`; after an `Err` the
    /// session is not to be asked again (the caller drops it).
    pub fn ask(
        &mut self,
        ask: Ask,
        paths: &[String],
        timeout: Duration,
    ) -> Result<Vec<Option<RemoteEntry>>, String> {
        self.seq += 1;
        let seq = self.seq;
        let line = request_line(seq, ask, paths)
            .ok_or_else(|| "The name can't be sent to the server safely".to_owned())?;
        let out = self.exchange(&line, seq, timeout)?;
        parse_reply(&out, seq, paths.len())
            .map_err(|_| "The server's answer could not be read".to_owned())
    }

    /// One load sample (046 Karar 2): `Ok(None)` if the server has no Linux
    /// `/proc` (`BT-NOPROC`). `Err` as [`Self::ask`]'s — the session is dropped
    /// after it.
    pub fn load(&mut self, detail: bool, timeout: Duration) -> Result<Option<LoadSample>, String> {
        self.seq += 1;
        let seq = self.seq;
        let out = self.exchange(&load_request_line(seq, detail), seq, timeout)?;
        parse_load(&out, seq).map_err(|_| "The server's answer could not be read".to_owned())
    }

    /// Writes one request line and reads up to its reply's end line, within
    /// `timeout`.
    fn exchange(&mut self, line: &str, seq: u64, timeout: Duration) -> Result<String, String> {
        self.stdin
            .write_all(line.as_bytes())
            .and_then(|()| self.stdin.flush())
            .map_err(|_| "The connection to the server closed".to_owned())?;
        let deadline = Instant::now() + timeout;
        let mut out = String::new();
        loop {
            let line = self.next_line(deadline).map_err(|timed_out| {
                if timed_out {
                    "The server did not answer in time".to_owned()
                } else {
                    "The connection to the server closed".to_owned()
                }
            })?;
            let end = ends_reply(&line, seq);
            out.push_str(&line);
            out.push('\n');
            if end {
                return Ok(out);
            }
        }
    }

    /// The next output line before `deadline`; `Err(true)` timed out,
    /// `Err(false)` the output ended.
    fn next_line(&self, deadline: Instant) -> Result<String, bool> {
        let left = deadline.saturating_duration_since(Instant::now());
        match self.lines.recv_timeout(left) {
            Ok(line) => Ok(line),
            Err(RecvTimeoutError::Timeout) => Err(true),
            Err(RecvTimeoutError::Disconnected) => Err(false),
        }
    }

    /// The open's failure text: the process is ended first, so its error
    /// output is complete (ssh's own last line says why).
    fn failure(mut self, host: &str, timed_out: bool) -> String {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let stderr = self
            .stderr
            .take()
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default();
        open_failure(host, timed_out, &stderr)
    }
}

impl Drop for HelperSession {
    fn drop(&mut self) {
        // Closing stdin ends the loop; the kill makes sure a stuck ssh goes too.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Why the helper did not open, as the pane's label says it (R1.3): one line,
/// with ssh's own last line if it gave one.
pub fn open_failure(host: &str, timed_out: bool, stderr: &str) -> String {
    let last = last_line(stderr);
    if timed_out {
        format!("No link: {host} did not answer in time")
    } else if last.is_empty() {
        format!("No link: can't reach {host} without a password (ssh-agent or ControlMaster)")
    } else {
        format!("No link: can't reach {host} without a password — {last}")
    }
}

// ─── resolution ──────────────────────────────────────────────────────────

/// Whether a hit's candidates are all relative names while the remote folder
/// is unknown (`cwd` empty): the pane's label then says why there is no link
/// ([`REMOTE_CWD_UNKNOWN`]). An absolute or `~` candidate can still resolve.
pub fn cwd_unknown(candidates: &[String], cwd: &str) -> bool {
    cwd.is_empty()
        && !candidates.is_empty()
        && candidates
            .iter()
            .all(|candidate| !candidate.starts_with('/') && !candidate.starts_with('~'))
}

/// Each candidate's remote absolute path, in order — `links::resolve`'s rules on
/// the remote disk: `~`/`~/…` under `home`, an absolute path as is, a relative
/// one under `cwd` — and **not at all** if `cwd` is empty (R1.2), nor `~user`.
/// `None` also for a path that cannot safely enter the helper's script
/// (`upload::is_safe`): such a name is not a link, never a mangled one.
pub fn remote_paths(candidates: &[String], cwd: &str, home: Option<&str>) -> Vec<Option<String>> {
    let home = home.map(Path::new);
    // The title's directory may be `~`-rooted (`Session::remote_link_directory`);
    // without the remote home it does not resolve and relative names stay unlinked.
    let cwd: Option<PathBuf> = match cwd {
        "" => None,
        "~" => home.map(Path::to_path_buf),
        _ => match cwd.strip_prefix("~/") {
            Some(rest) => home.map(|home| home.join(rest)),
            None => Some(PathBuf::from(cwd)),
        },
    };
    let cwd = cwd.as_deref();
    candidates
        .iter()
        .map(|candidate| {
            links::resolve(Path::new(candidate), cwd, home, |_| Some(Entry::Dir))
                .and_then(|resolved| resolved.path.to_str().map(str::to_owned))
                .filter(|path| is_safe(path))
        })
        .collect()
}

/// The helper's answers that exist, per remote generation (R1.1): a name asked
/// once is not asked again while the same ssh session runs. A missing name is
/// **not** cached — a file created a moment later must be found at the next ⌘
/// (the view forgets its own "missing" when ⌘ goes up).
#[derive(Debug, Default)]
pub struct RemoteCache {
    command: Option<u64>,
    entries: HashMap<String, RemoteEntry>,
}

impl RemoteCache {
    /// The cached answer for `path` under `command`; another generation empties
    /// the cache first.
    pub fn get(&mut self, command: u64, path: &str) -> Option<RemoteEntry> {
        self.sync(command);
        self.entries.get(path).copied()
    }

    /// Remembers what `path` is under `command`.
    pub fn insert(&mut self, command: u64, path: String, entry: RemoteEntry) {
        self.sync(command);
        self.entries.insert(path, entry);
    }

    fn sync(&mut self, command: u64) {
        if self.command != Some(command) {
            self.entries.clear();
            self.command = Some(command);
        }
    }
}

// ─── worker ──────────────────────────────────────────────────────────────

/// What a question asks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Query {
    /// The ⌘-hover's verification: the first candidate that exists, resolved
    /// against `cwd` (the remote OSC 7 folder, empty if unknown).
    Verify {
        candidates: Vec<String>,
        cwd: String,
    },
    /// The download's question about one absolute path: what it is, a folder's
    /// file count and bytes included — always fresh, never from the cache.
    Count(String),
    /// A load sample for the ssh status bar's indicator (046 Karar 1, 2);
    /// `detail` while the popover is open (OS, cores, top processes).
    Load { detail: bool },
}

/// The answer to a [`Query`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    /// The winning candidate's index, its remote absolute path and what it is;
    /// `None` if no candidate exists.
    Verified(Option<(usize, String, RemoteEntry)>),
    /// What the path is; `None` if it does not exist.
    Counted(Option<RemoteEntry>),
    /// The load sample, or why there is none.
    Load(LoadReply),
}

/// The answer to a [`Query::Load`]. Its failures are split by what the
/// sampler does next (046 Karar 1): a failed **open** comes back here as
/// [`LoadReply::Unreachable`] and ends sampling for the generation (no ssh
/// attempt every few seconds against a server that wants a password); a
/// failure on an **open** session stays the reply's `Err` — the indicator
/// hides and the next tick tries once more.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadReply {
    /// The raw readings.
    Sample(LoadSample),
    /// The server has no Linux `/proc` (`BT-NOPROC`): no indicator.
    NoProc,
    /// The helper session could not be opened; the pane label's text.
    Unreachable(String),
}

/// The reply callback — runs on the worker thread; the platform shell posts it
/// to its main thread. The second argument is the ssh argv the answer came
/// over (empty when no session opened): a download's or a preview's stream
/// rides the **same** route as its question (047 R5).
pub type Reply = Box<dyn FnOnce(Result<Answer, String>, &[String]) + Send>;

/// How the worker gets the session's ssh argv: the route gate
/// ([`crate::ssh_route::dial`]) runs **on the worker thread**, only when a
/// session has to be opened — it spawns ssh processes and may wait at a sheet.
pub struct Dial {
    /// A job the user started (a sheet may ask for the password): it neither
    /// reads nor writes the failed-open hold ([`RETRY_AFTER`]) — a hover's
    /// failure must not refuse a ⌘-click for ten seconds, and a cancelled sheet
    /// is not a hover's label.
    pub user: bool,
    /// The argv without the remote command (`Err`: the open's failure text).
    pub argv: Box<dyn FnOnce() -> Result<Vec<String>, String> + Send>,
}

impl Dial {
    /// A fixed argv, no gate (the tests' local shell).
    pub fn fixed(argv: Vec<String>) -> Self {
        Self {
            user: false,
            argv: Box::new(move || Ok(argv)),
        }
    }

    /// The gate's dial: `masters` `None` (the timed run) → today's argv.
    pub fn gated(
        masters: Option<std::sync::Arc<crate::ssh_route::Masters>>,
        target: bt_core::RemoteTarget,
        ask: crate::ssh_route::Ask,
    ) -> Self {
        Self {
            user: matches!(ask, crate::ssh_route::Ask::Sheet(_)),
            argv: Box::new(move || {
                crate::ssh_route::dial(masters.as_deref(), &target, ask)
                    .map_err(|denied| denied.text())
            }),
        }
    }
}

/// One question to the helper.
pub struct Request {
    /// The remote session's generation: another one closes the open session.
    pub command: u64,
    /// The session's argv, through the route gate.
    pub dial: Dial,
    pub host: String,
    pub query: Query,
    pub reply: Reply,
}

enum Message {
    Ask(Request),
    Close,
}

/// The pane's handle on its helper session. The worker thread is born at the
/// first [`RemoteHelper::ask`] and ends when the handle drops (the session's
/// ssh with it).
#[derive(Default)]
pub struct RemoteHelper {
    tx: Option<Sender<Message>>,
}

impl RemoteHelper {
    /// Sends a question; the reply comes on the worker thread. If the worker
    /// cannot be started the reply is called here with the reason.
    pub fn ask(&mut self, request: Request) {
        // A worker that ended (it cannot today, but a panic would) is replaced.
        let request = match &self.tx {
            Some(tx) => match tx.send(Message::Ask(request)) {
                Ok(()) => return,
                Err(mpsc::SendError(Message::Ask(request))) => request,
                Err(mpsc::SendError(Message::Close)) => return,
            },
            None => request,
        };
        let (tx, rx) = mpsc::channel();
        let spawned = thread::Builder::new()
            .name("remote helper".into())
            .spawn(move || run(&rx));
        match spawned {
            Ok(_) => {
                if let Err(mpsc::SendError(Message::Ask(request))) = tx.send(Message::Ask(request))
                {
                    (request.reply)(Err("The remote helper stopped".to_owned()), &[]);
                }
                self.tx = Some(tx);
            }
            Err(error) => (request.reply)(
                Err(format!("The remote helper could not start: {error}")),
                &[],
            ),
        }
    }

    /// Closes the open session (the pane closes, the remote session ended); the
    /// next question opens a new one.
    pub fn close(&mut self) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Message::Close);
        }
    }
}

/// The worker loop: one session at a time, closed on [`IDLE`], on
/// [`Message::Close`] and on another generation.
fn run(rx: &Receiver<Message>) {
    let mut open: Option<Open> = None;
    let mut failed: Option<Failure> = None;
    let mut cache = RemoteCache::default();
    loop {
        let message = if open.is_some() {
            match rx.recv_timeout(IDLE) {
                Ok(message) => message,
                Err(RecvTimeoutError::Timeout) => {
                    open = None;
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match rx.recv() {
                Ok(message) => message,
                Err(_) => break,
            }
        };
        match message {
            Message::Close => open = None,
            Message::Ask(request) => {
                let Request {
                    command,
                    dial,
                    host,
                    query,
                    reply,
                } = request;
                let (answer, ssh) = serve(
                    &mut open,
                    &mut failed,
                    &mut cache,
                    (command, dial, &host),
                    query,
                );
                reply(answer, &ssh);
            }
        }
    }
}

/// The last failed open: its generation, when and why ([`RETRY_AFTER`]).
struct Failure {
    command: u64,
    at: Instant,
    text: String,
}

impl Failure {
    fn holds(&self, command: u64) -> bool {
        self.command == command && self.at.elapsed() < RETRY_AFTER
    }
}

/// The open session: its generation and the argv it was opened with (the
/// replies hand it on to the streams).
struct Open {
    command: u64,
    ssh: Vec<String>,
    session: HelperSession,
}

/// One question against the open session (opened or reopened as needed); a
/// failed request drops the session, so the next question starts clean. The
/// argv the answer came over goes back with it.
fn serve(
    open: &mut Option<Open>,
    failed: &mut Option<Failure>,
    cache: &mut RemoteCache,
    (command, dial, host): (u64, Dial, &str),
    query: Query,
) -> (Result<Answer, String>, Vec<String>) {
    // A name the script cannot carry is refused before it can cost the session.
    if let Query::Count(path) = &query
        && !is_safe(path)
    {
        return (
            Err(format!("{path} can't be downloaded safely")),
            Vec::new(),
        );
    }
    if open.as_ref().is_some_and(|open| open.command != command) {
        *open = None;
    }
    // A load sample's failed open is an answer of its own (`LoadReply`).
    let opening_failed = |text: String| match query {
        Query::Load { .. } => Ok(Answer::Load(LoadReply::Unreachable(text))),
        _ => Err(text),
    };
    if open.is_none() {
        if let Some(failure) = failed
            .as_ref()
            .filter(|failure| !dial.user && failure.holds(command))
        {
            return (opening_failed(failure.text.clone()), Vec::new());
        }
        let user = dial.user;
        let opened = (dial.argv)().and_then(|ssh| {
            HelperSession::open(&ssh, host, OPEN_TIMEOUT).map(|session| (ssh, session))
        });
        match opened {
            Ok((ssh, session)) => {
                *failed = None;
                *open = Some(Open {
                    command,
                    ssh,
                    session,
                });
            }
            Err(text) => {
                if !user {
                    *failed = Some(Failure {
                        command,
                        at: Instant::now(),
                        text: text.clone(),
                    });
                }
                return (opening_failed(text), Vec::new());
            }
        }
    }
    let Some(current) = open.as_mut() else {
        return (Err(format!("No link: can't reach {host}")), Vec::new());
    };
    let ssh = current.ssh.clone();
    let result = answer(&mut current.session, cache, command, query);
    if result.is_err() {
        *open = None;
    }
    (result, ssh)
}

/// [`serve`]'s work once a session is open.
fn answer(
    session: &mut HelperSession,
    cache: &mut RemoteCache,
    command: u64,
    query: Query,
) -> Result<Answer, String> {
    match query {
        Query::Load { detail } => Ok(Answer::Load(match session.load(detail, LOAD_TIMEOUT)? {
            Some(sample) => LoadReply::Sample(sample),
            None => LoadReply::NoProc,
        })),
        Query::Count(path) => {
            let mut entries =
                session.ask(Ask::Count, std::slice::from_ref(&path), COUNT_TIMEOUT)?;
            Ok(Answer::Counted(entries.pop().flatten()))
        }
        Query::Verify { candidates, cwd } => {
            let paths = remote_paths(&candidates, &cwd, session.home());
            let mut asked: Vec<String> = Vec::new();
            for path in paths.iter().flatten() {
                if cache.get(command, path).is_none() && !asked.contains(path) {
                    asked.push(path.clone());
                }
            }
            if !asked.is_empty() {
                let entries = session.ask(Ask::Stat, &asked, STAT_TIMEOUT)?;
                for (path, entry) in asked.into_iter().zip(entries) {
                    if let Some(entry) = entry {
                        cache.insert(command, path, entry);
                    }
                }
            }
            Ok(Answer::Verified(paths.into_iter().enumerate().find_map(
                |(index, path)| {
                    let path = path?;
                    let entry = cache.get(command, &path)?;
                    Some((index, path, entry))
                },
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::time::UNIX_EPOCH;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bt-helper-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp directory");
        dir
    }

    /// A local shell in place of ssh: `sh -c "<remote command>"` — the very
    /// script that would run remotely, without a connection (`download`'s
    /// precedent).
    fn local_ssh() -> Vec<String> {
        vec!["/bin/sh".to_owned(), "-c".to_owned()]
    }

    fn text(path: &Path) -> String {
        path.to_str().expect("utf-8 temp path").to_owned()
    }

    #[test]
    fn the_session_answers_requests_in_turn_over_a_local_shell() {
        let root = scratch("session");
        let file = root.join("it's a file.txt");
        fs::write(&file, vec![b'x'; 1234]).unwrap();
        let script = root.join("run.sh");
        fs::write(&script, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        let folder = root.join("logs");
        fs::create_dir_all(folder.join("deep")).unwrap();
        fs::write(folder.join("a"), b"12345").unwrap();
        fs::write(folder.join("deep/b"), b"123").unwrap();

        let mut session =
            HelperSession::open(&local_ssh(), "local", OPEN_TIMEOUT).expect("the helper greets");
        assert_eq!(
            session.home().map(str::to_owned),
            std::env::var("HOME").ok().filter(|home| !home.is_empty())
        );
        let mtime = fs::metadata(&file)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let asked = [
            text(&file),
            text(&script),
            text(&folder),
            text(&root.join("nothing here")),
        ];
        let first = session
            .ask(Ask::Stat, &asked, STAT_TIMEOUT)
            .expect("first reply");
        assert_eq!(
            first[0],
            Some(RemoteEntry::File {
                size: Some(1234),
                mtime: Some(mtime),
                executable: false,
            })
        );
        assert!(matches!(
            first[1],
            Some(RemoteEntry::File {
                executable: true,
                ..
            })
        ));
        assert_eq!(first[2], Some(RemoteEntry::Dir(None)));
        assert_eq!(first[3], None);
        // A second request on the same connection: the next sequence number,
        // and a folder counted this time.
        let second = session
            .ask(Ask::Count, &[text(&folder)], COUNT_TIMEOUT)
            .expect("second reply");
        assert_eq!(
            second,
            vec![Some(RemoteEntry::Dir(Some(
                crate::remote_files::FolderSize { files: 2, bytes: 8 }
            )))]
        );
        // A name that cannot enter the script is refused, not mangled.
        assert!(
            session
                .ask(Ask::Stat, &["/tmp/a\\b".to_owned()], STAT_TIMEOUT)
                .is_err()
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// `bt_load` end to end on this machine (046 R4.1, R4.2): macOS has no
    /// `/proc` and answers `BT-NOPROC`, `make linux` reads a real one. The
    /// branch is the file, not the target: what decides is what the script sees.
    #[test]
    fn a_load_sample_answers_through_a_local_shell() {
        use crate::remote_stats::Sampler;
        use bt_core::StatsForm;

        let mut session =
            HelperSession::open(&local_ssh(), "local", OPEN_TIMEOUT).expect("the helper greets");
        let first = session.load(false, LOAD_TIMEOUT).expect("a reply");
        if Path::new("/proc/stat").exists() {
            let plain = first.expect("a Linux /proc gives a sample");
            assert!(
                plain.cpu.total > 0 && plain.cpu.idle <= plain.cpu.total,
                "{plain:?}"
            );
            assert!(plain.mem_total > 0, "{plain:?}");
            assert!(plain.mem_available <= plain.mem_total, "{plain:?}");
            assert!(plain.load.is_some() && plain.uptime.is_some(), "{plain:?}");
            assert!(plain.disk.is_some_and(|disk| disk <= 100), "{plain:?}");
            // The details only with `p`.
            assert!(plain.os.is_none() && plain.cores.is_none() && plain.scan.is_none());
            let detailed = session
                .load(true, LOAD_TIMEOUT)
                .expect("a reply")
                .expect("a sample");
            assert!(
                detailed.cores.is_some_and(|cores| cores > 0),
                "{detailed:?}"
            );
            // The scan: our own `sh` is named and read like every live
            // process; every task is well formed (a real `/proc`).
            let scan = detailed.scan.as_ref().expect("a `p` sample scans");
            let own = scan.self_pid.expect("the helper names its PID");
            assert!(
                scan.tasks.iter().any(|task| task.pid == own),
                "the helper's own stat line: {scan:?}"
            );
            assert!(
                scan.tasks
                    .iter()
                    .all(|task| task.pid > 0 && !task.name.is_empty()),
                "{scan:?}"
            );
            let mut sampler = Sampler::default();
            sampler.take(&plain, StatsForm::Sparkline);
            let reading = sampler.take(&detailed, StatsForm::Sparkline);
            assert!(
                reading.stats.cpu.is_none_or(|cpu| cpu <= 100),
                "{reading:?}"
            );
            assert!(reading.stats.mem <= 100, "{reading:?}");
            assert_eq!(reading.detail.processes, None, "one scan: measuring");
            // The second scan has a difference; our own measuring is not in it.
            let again = session
                .load(true, LOAD_TIMEOUT)
                .expect("a reply")
                .expect("a sample");
            let measured = sampler.take(&again, StatsForm::Sparkline);
            let top = measured.detail.processes.expect("two scans: measured");
            assert!(top.len() <= crate::remote_stats::TOP_PROCESSES, "{top:?}");
            let cores = again.cores.unwrap_or(1);
            assert!(
                top.iter()
                    .all(|process| process.cpu > 0 && process.cpu <= 1000 * cores),
                "{top:?}"
            );
        } else {
            assert_eq!(first, None, "no /proc here: BT-NOPROC");
            assert_eq!(session.load(true, LOAD_TIMEOUT), Ok(None));
        }
        // The session goes on: a `bt_stat` after it reads its own reply.
        assert_eq!(
            session.ask(Ask::Stat, &["/".to_owned()], STAT_TIMEOUT),
            Ok(vec![Some(RemoteEntry::Dir(None))])
        );
    }

    #[test]
    fn a_helper_that_never_greets_fails_with_the_reason() {
        // `false` in place of ssh: exits at once, says nothing.
        let silent = vec!["/bin/sh".to_owned(), "-c".to_owned(), "exit 255".to_owned()];
        let error = HelperSession::open(&silent, "prod", OPEN_TIMEOUT)
            .err()
            .expect("no greeting");
        assert!(error.contains("prod"), "{error}");
        // ssh's own last line is carried.
        let noisy = vec![
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            "echo 'Permission denied (publickey).' >&2; exit 255".to_owned(),
        ];
        let error = HelperSession::open(&noisy, "prod", OPEN_TIMEOUT)
            .err()
            .expect("no greeting");
        assert!(error.ends_with("Permission denied (publickey)."), "{error}");
        assert!(open_failure("prod", true, "").contains("did not answer"));
    }

    #[test]
    fn an_empty_remote_folder_rejects_relative_names_only() {
        let candidates = vec![
            "backups".to_owned(),
            "/var/log/syslog".to_owned(),
            "~/notes.txt".to_owned(),
            "~".to_owned(),
            "~root/x".to_owned(),
        ];
        assert_eq!(
            remote_paths(&candidates, "", Some("/home/deploy")),
            vec![
                None,
                Some("/var/log/syslog".to_owned()),
                Some("/home/deploy/notes.txt".to_owned()),
                Some("/home/deploy".to_owned()),
                None,
            ]
        );
        // With OSC 7's folder the relative name resolves under it, `./` dropped.
        assert_eq!(
            remote_paths(&["./backups".to_owned()], "/var/www", None),
            vec![Some("/var/www/backups".to_owned())]
        );
        // No home from the greeting: `~` is no link.
        assert_eq!(remote_paths(&["~/x".to_owned()], "/srv", None), vec![None]);
        // A name the script cannot carry.
        assert_eq!(remote_paths(&["a\\b".to_owned()], "/srv", None), vec![None]);
        // The title's `~`-rooted directory expands under the remote home.
        assert_eq!(
            remote_paths(&["backups".to_owned()], "~", Some("/root")),
            vec![Some("/root/backups".to_owned())]
        );
        assert_eq!(
            remote_paths(&["x".to_owned()], "~/app", Some("/home/d")),
            vec![Some("/home/d/app/x".to_owned())]
        );
        assert_eq!(remote_paths(&["x".to_owned()], "~", None), vec![None]);
        // The label's question: only relative names and no folder.
        assert!(cwd_unknown(
            &["backups".to_owned(), "My Drive".to_owned()],
            ""
        ));
        assert!(!cwd_unknown(&["backups".to_owned()], "/srv"));
        assert!(!cwd_unknown(&["backups".to_owned(), "/etc".to_owned()], ""));
        assert!(!cwd_unknown(&["~/x".to_owned()], ""));
    }

    #[test]
    fn the_cache_empties_when_the_generation_changes() {
        let mut cache = RemoteCache::default();
        let file = RemoteEntry::File {
            size: Some(1),
            mtime: None,
            executable: false,
        };
        cache.insert(7, "/a".to_owned(), file);
        assert_eq!(cache.get(7, "/a"), Some(file));
        assert_eq!(cache.get(8, "/a"), None);
        // The old generation's answer does not come back either.
        assert_eq!(cache.get(7, "/a"), None);
    }

    /// The worker end to end: the first candidate that exists wins, a relative
    /// name resolves under the folder, a second question with the same names is
    /// answered from the cache, and a new generation opens a new session.
    #[test]
    fn the_worker_verifies_candidates_and_answers_counts() {
        let root = scratch("worker");
        fs::create_dir_all(root.join("My Drive")).unwrap();
        fs::write(root.join("notes.txt"), b"hello").unwrap();
        let mut helper = RemoteHelper::default();
        let ask = |helper: &mut RemoteHelper, command: u64, query: Query| {
            let (tx, rx) = mpsc::channel();
            helper.ask(Request {
                command,
                dial: Dial::fixed(local_ssh()),
                host: "local".to_owned(),
                query,
                reply: Box::new(move |answer, _| {
                    let _ = tx.send(answer);
                }),
            });
            rx.recv_timeout(Duration::from_secs(30))
                .expect("the worker replies")
        };
        let verify = |candidates: &[&str], cwd: &Path| Query::Verify {
            candidates: candidates.iter().map(|c| (*c).to_owned()).collect(),
            cwd: text(cwd),
        };
        // `Drive` does not exist, `My Drive` does (iTerm2's search order).
        let answer = ask(&mut helper, 1, verify(&["Drive", "My Drive"], &root));
        assert_eq!(
            answer,
            Ok(Answer::Verified(Some((
                1,
                text(&root.join("My Drive")),
                RemoteEntry::Dir(None)
            ))))
        );
        // Without the folder a relative name is no link.
        let answer = ask(&mut helper, 1, verify(&["notes.txt"], Path::new("")));
        assert_eq!(answer, Ok(Answer::Verified(None)));
        let answer = ask(&mut helper, 1, verify(&["notes.txt"], &root));
        assert!(matches!(
            answer,
            Ok(Answer::Verified(Some((
                0,
                _,
                RemoteEntry::File { size: Some(5), .. }
            ))))
        ));
        // From the cache: the file is gone remotely, the same generation still
        // answers what it saw.
        fs::remove_file(root.join("notes.txt")).unwrap();
        let answer = ask(&mut helper, 1, verify(&["notes.txt"], &root));
        assert!(matches!(answer, Ok(Answer::Verified(Some(_)))));
        // A new generation asks again.
        let answer = ask(&mut helper, 2, verify(&["notes.txt"], &root));
        assert_eq!(answer, Ok(Answer::Verified(None)));
        // A count is always fresh.
        let answer = ask(&mut helper, 2, Query::Count(text(&root.join("My Drive"))));
        assert_eq!(
            answer,
            Ok(Answer::Counted(Some(RemoteEntry::Dir(Some(
                crate::remote_files::FolderSize { files: 0, bytes: 0 }
            )))))
        );
        helper.close();
        let answer = ask(&mut helper, 2, Query::Count(text(&root.join("nothing"))));
        assert_eq!(answer, Ok(Answer::Counted(None)));
        let _ = fs::remove_dir_all(&root);
    }

    /// An unreachable server is not dialled once per hovered word: within
    /// [`RETRY_AFTER`] the same generation gets the first failure's answer.
    #[test]
    fn a_failed_open_is_not_retried_at_once() {
        let root = scratch("retry");
        let counter = root.join("dials");
        let dial = format!(
            "echo dial >> '{}'; echo 'Permission denied (publickey).' >&2; exit 255",
            text(&counter)
        );
        let mut helper = RemoteHelper::default();
        let mut ask = |command: u64| {
            let (tx, rx) = mpsc::channel();
            helper.ask(Request {
                command,
                dial: Dial::fixed(vec!["/bin/sh".to_owned(), "-c".to_owned(), dial.clone()]),
                host: "prod".to_owned(),
                query: Query::Count("/etc".to_owned()),
                reply: Box::new(move |answer, _| {
                    let _ = tx.send(answer);
                }),
            });
            rx.recv_timeout(Duration::from_secs(30))
                .expect("the worker replies")
        };
        let first = ask(1).expect_err("no greeting");
        assert!(first.contains("Permission denied"), "{first}");
        assert_eq!(ask(1), Err(first));
        assert_eq!(fs::read_to_string(&counter).unwrap().lines().count(), 1);
        // Another generation dials again.
        assert!(ask(2).is_err());
        assert_eq!(fs::read_to_string(&counter).unwrap().lines().count(), 2);
        let _ = fs::remove_dir_all(&root);
    }

    /// A job the user started (047) is not refused by a hover's held
    /// failure, does not leave one behind, and its reply carries the argv the
    /// session was opened with — the stream rides the same route.
    #[test]
    fn a_user_dial_ignores_the_held_failure_and_hands_on_its_argv() {
        let root = scratch("user-dial");
        let counter = root.join("dials");
        let dial = format!(
            "echo dial >> '{}'; echo 'Permission denied (password).' >&2; exit 255",
            text(&counter)
        );
        let failing = vec!["/bin/sh".to_owned(), "-c".to_owned(), dial];
        let counted = text(&root);
        let mut helper = RemoteHelper::default();
        let mut ask = |dial: Dial| {
            let (tx, rx) = mpsc::channel();
            helper.ask(Request {
                command: 1,
                dial,
                host: "prod".to_owned(),
                query: Query::Count(counted.clone()),
                reply: Box::new(move |answer, ssh: &[String]| {
                    let _ = tx.send((answer, ssh.to_vec()));
                }),
            });
            rx.recv_timeout(Duration::from_secs(30))
                .expect("the worker replies")
        };
        // A background failure is held...
        assert!(ask(Dial::fixed(failing.clone())).0.is_err());
        let user = |argv: Vec<String>| Dial {
            user: true,
            argv: Box::new(move || Ok(argv)),
        };
        // ...a user's dial tries anyway, and its own failure is not held.
        let (answer, ssh) = ask(user(failing.clone()));
        assert!(answer.is_err());
        assert!(ssh.is_empty());
        assert_eq!(fs::read_to_string(&counter).unwrap().lines().count(), 2);
        // The gate's own refusal reaches the reply as the error text.
        let refused = Dial {
            user: true,
            argv: Box::new(|| Err(crate::ssh_route::CANCELLED.to_owned())),
        };
        assert_eq!(ask(refused).0.unwrap_err(), crate::ssh_route::CANCELLED);
        let (answer, ssh) = ask(user(local_ssh()));
        assert!(matches!(answer, Ok(Answer::Counted(Some(_)))), "{answer:?}");
        assert_eq!(ssh, local_ssh());
        // Later questions of the open session hand the same argv on.
        let (_, ssh) = ask(Dial::fixed(failing));
        assert_eq!(ssh, local_ssh());
        let _ = fs::remove_dir_all(&root);
    }

    /// A load sample's failed open is an answer, not an error (046 Karar 1):
    /// the sampler ends the generation on it, while an `Err` — a failure on
    /// an open session — earns one retry. Held failures answer the same way.
    #[test]
    fn a_load_samples_failed_open_is_unreachable() {
        let root = scratch("load-open");
        let counter = root.join("dials");
        let dial = format!(
            "echo dial >> '{}'; echo 'Permission denied (password).' >&2; exit 255",
            text(&counter)
        );
        let mut helper = RemoteHelper::default();
        let mut ask = |ssh: Vec<String>, query: Query| {
            let (tx, rx) = mpsc::channel();
            helper.ask(Request {
                command: 1,
                dial: Dial::fixed(ssh),
                host: "prod".to_owned(),
                query,
                reply: Box::new(move |answer, _| {
                    let _ = tx.send(answer);
                }),
            });
            rx.recv_timeout(Duration::from_secs(30))
                .expect("the worker replies")
        };
        let failing = || vec!["/bin/sh".to_owned(), "-c".to_owned(), dial.clone()];
        for _ in 0..2 {
            match ask(failing(), Query::Load { detail: false }) {
                Ok(Answer::Load(LoadReply::Unreachable(text))) => {
                    assert!(text.contains("Permission denied"), "{text}");
                }
                other => panic!("expected Unreachable, got {other:?}"),
            }
        }
        assert_eq!(fs::read_to_string(&counter).unwrap().lines().count(), 1);
        // Other questions keep their `Err`.
        assert!(ask(failing(), Query::Count("/etc".to_owned())).is_err());
        let _ = fs::remove_dir_all(&root);
        // An open session answers through the worker.
        let mut helper = RemoteHelper::default();
        let (tx, rx) = mpsc::channel();
        helper.ask(Request {
            command: 1,
            dial: Dial::fixed(local_ssh()),
            host: "local".to_owned(),
            query: Query::Load { detail: true },
            reply: Box::new(move |answer, _| {
                let _ = tx.send(answer);
            }),
        });
        let answer = rx
            .recv_timeout(Duration::from_secs(30))
            .expect("the worker replies");
        assert!(
            matches!(
                answer,
                Ok(Answer::Load(LoadReply::Sample(_) | LoadReply::NoProc))
            ),
            "{answer:?}"
        );
    }
}
