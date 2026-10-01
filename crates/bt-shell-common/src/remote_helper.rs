//! The helper ssh session of a remote pane (045 Karar 10): one long-lived
//! `ssh … sh` that answers "does this exist, what is it, how big" line by line,
//! so a ⌘-hover over a name in a remote `ls` costs one round trip on an open
//! connection, not a new ssh handshake.
//!
//! - **[`HelperSession`]** is the process: [`crate::upload::ssh_argv`] +
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
//!   pane closing) and [`IDLE`] without a question.
//! - **Resolution** ([`remote_paths`]) is `links::resolve`'s, on the remote
//!   disk: `~` from the helper's greeting, a relative name from the remote
//!   OSC 7 directory and **no** relative name without it (Karar 2-A, R1.2).
//!
//! Platform-free like `upload`'s processes; the platform shell takes the answer
//! back to its main thread. The rationale is in
//! `.tasks/045-uzak-dosya-indirme/discussion.md` → Karar 1, 2, 10.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use crate::links::{self, Entry};
use crate::remote_files::{
    Ask, RemoteEntry, ends_reply, helper_script, parse_greeting, parse_reply, request_line,
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
                return parse_reply(&out, seq, paths.len())
                    .map_err(|_| "The server's answer could not be read".to_owned());
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
    let cwd = (!cwd.is_empty()).then(|| Path::new(cwd));
    let home = home.map(Path::new);
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
}

/// The answer to a [`Query`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    /// The winning candidate's index, its remote absolute path and what it is;
    /// `None` if no candidate exists.
    Verified(Option<(usize, String, RemoteEntry)>),
    /// What the path is; `None` if it does not exist.
    Counted(Option<RemoteEntry>),
}

/// The reply callback — runs on the worker thread; the platform shell posts it
/// to its main thread.
pub type Reply = Box<dyn FnOnce(Result<Answer, String>) + Send>;

/// One question to the helper.
pub struct Request {
    /// The remote session's generation: another one closes the open session.
    pub command: u64,
    /// The ssh argv without the remote command ([`crate::upload::ssh_argv`]).
    pub ssh: Vec<String>,
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
                    (request.reply)(Err("The remote helper stopped".to_owned()));
                }
                self.tx = Some(tx);
            }
            Err(error) => {
                (request.reply)(Err(format!("The remote helper could not start: {error}")))
            }
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
    let mut open: Option<(u64, HelperSession)> = None;
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
                    ssh,
                    host,
                    query,
                    reply,
                } = request;
                let answer = serve(
                    &mut open,
                    &mut failed,
                    &mut cache,
                    (command, &ssh, &host),
                    query,
                );
                reply(answer);
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

/// One question against the open session (opened or reopened as needed); a
/// failed request drops the session, so the next question starts clean.
fn serve(
    open: &mut Option<(u64, HelperSession)>,
    failed: &mut Option<Failure>,
    cache: &mut RemoteCache,
    (command, ssh, host): (u64, &[String], &str),
    query: Query,
) -> Result<Answer, String> {
    // A name the script cannot carry is refused before it can cost the session.
    if let Query::Count(path) = &query
        && !is_safe(path)
    {
        return Err(format!("{path} can't be downloaded safely"));
    }
    if open.as_ref().is_some_and(|(at, _)| *at != command) {
        *open = None;
    }
    if open.is_none() {
        if let Some(failure) = failed.as_ref().filter(|failure| failure.holds(command)) {
            return Err(failure.text.clone());
        }
        match HelperSession::open(ssh, host, OPEN_TIMEOUT) {
            Ok(session) => {
                *failed = None;
                *open = Some((command, session));
            }
            Err(text) => {
                *failed = Some(Failure {
                    command,
                    at: Instant::now(),
                    text: text.clone(),
                });
                return Err(text);
            }
        }
    }
    let Some((_, session)) = open.as_mut() else {
        return Err(format!("No link: can't reach {host}"));
    };
    let result = answer(session, cache, command, query);
    if result.is_err() {
        *open = None;
    }
    result
}

/// [`serve`]'s work once a session is open.
fn answer(
    session: &mut HelperSession,
    cache: &mut RemoteCache,
    command: u64,
    query: Query,
) -> Result<Answer, String> {
    match query {
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
                ssh: local_ssh(),
                host: "local".to_owned(),
                query,
                reply: Box::new(move |answer| {
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
                ssh: vec!["/bin/sh".to_owned(), "-c".to_owned(), dial.clone()],
                host: "prod".to_owned(),
                query: Query::Count("/etc".to_owned()),
                reply: Box::new(move |answer| {
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
}
