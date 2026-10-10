//! The focus query: an outside process asks a running bateri, for the
//! one pane whose identity it holds, "is the user looking at this pane, and
//! how long since they last touched it".
//!
//! **Why:** a status strip of AI sessions (evlat) paints a session that
//! finished or asks something; when the user is looking at that session's
//! pane the signal is noise. From outside, without a permission, nobody can
//! tell which pane inside bateri has the focus — so bateri says it, **for the
//! asked pane only**: the pane's UUID (`bateri://tab/<UUID>`) is the
//! authority, no other pane, title, directory or list leaves by any path.
//! Pull only, asked at the moment of the event; `idle` in whole seconds,
//! because a millisecond series of key moments is a known side channel on
//! what was typed.
//!
//! **The halves here:** the wire (`focus 1 <UUID>\n` → one token line), a
//! bounded unix socket server in the instance directory
//! ([`crate::ssh_route::prepare_instance`]) whose answer comes from an
//! injected answerer (the platform shell's: the main thread's live state), the
//! client that finds the live instances and asks them under limits, the
//! clock that counts sleep, and `bateri focus`'s body ([`focus_main`]).
//! No answer in time, an unknown version or a broken line is `pane=unknown` —
//! never `pane=none`, which only means "no live instance knows this pane".

use std::io::{self, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use bt_core::PaneUuid;

use crate::ssh_route::{self, SUN_PATH};

/// `bateri focus`: the subcommand's word, the first argument.
pub const SUBCOMMAND: &str = "focus";

/// The listener's name in the instance directory. The sweep and ⌘Q remove it
/// with the directory (`ssh_route::remove_instance`).
pub const FOCUS_SOCKET: &str = "focus";

/// The wire's version: the request's second word.
pub const WIRE_VERSION: &str = "1";

/// The request's first word.
const REQUEST_WORD: &str = "focus";

/// The longest request line the server reads (`focus 1 <36>` is 44 bytes): a
/// longer one is a broken peer and is closed without an answer.
const REQUEST_LIMIT: usize = 64;

/// The longest answer line the client reads; a longer one is `unknown`.
const ANSWER_LIMIT: usize = 256;

/// How long the server waits for a connection's request and its write — a
/// **design constant**, hundreds of milliseconds: the client writes the whole
/// line at once.
pub const SERVER_READ_LIMIT: Duration = Duration::from_millis(200);

/// How long an answerer may take — a **design constant** for the platform
/// shell's answerer (the main queue's turn); past it the answerer gives `None`
/// and the reply is `pane=unknown`.
pub const ANSWER_WAIT: Duration = Duration::from_millis(200);

/// How long the client waits for one instance (write, then the answer) — a
/// **design constant**, above the server's read limit + the answer wait.
pub const CLIENT_INSTANCE_LIMIT: Duration = Duration::from_millis(500);

/// How long the client waits in all — a **design constant**: the outside
/// process asks at the moment of an event and must not hang.
pub const CLIENT_TOTAL_LIMIT: Duration = Duration::from_millis(900);

/// How many connections the server answers at once — a **design constant**;
/// one past it is accepted and closed (the client reads `unknown`), so no
/// client can exhaust the threads.
const MAX_IN_FLIGHT: usize = 8;

/// `bateri focus`'s exit code with an answer (`pane=live` or `pane=none`).
pub const EXIT_ANSWER: i32 = 0;

/// `bateri focus`'s exit code for a usage error (a diagnostic on stderr).
pub const EXIT_USAGE: i32 = 2;

/// `bateri focus`'s exit code for `pane=unknown` — kept apart from `none` so
/// the difference stays testable.
pub const EXIT_UNKNOWN: i32 = 3;

// ─── wire ────────────────────────────────────────────────────────────────

/// What an instance says about the asked pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// The pane is open: `focused` — bateri active, its window key and it the
    /// window's focused pane; `idle_secs` — whole seconds since its last input.
    Live { focused: bool, idle_secs: u64 },
    /// No open pane has this identity.
    None,
    /// No answer could be had (time out, unknown wire, broken answer).
    Unknown,
}

impl Answer {
    /// The token line, without the newline: a machine contract — tokens are
    /// added, never removed.
    pub fn token_line(&self) -> String {
        match self {
            Answer::Live { focused, idle_secs } => {
                format!("pane=live focused={} idle={idle_secs}", u8::from(*focused))
            }
            Answer::None => "pane=none".to_owned(),
            Answer::Unknown => "pane=unknown".to_owned(),
        }
    }

    /// Reads an instance's token line. Unknown tokens are skipped (a newer
    /// instance says more); a `pane=live` without `focused=` and `idle=`, an
    /// unknown `pane=` or a line that is not printable ASCII is `Unknown`.
    pub fn parse(line: &str) -> Answer {
        if !line
            .bytes()
            .all(|byte| byte == b' ' || byte.is_ascii_graphic())
        {
            return Answer::Unknown;
        }
        let mut tokens = line.split(' ').filter(|token| !token.is_empty());
        match tokens.next() {
            Some("pane=none") => Answer::None,
            Some("pane=live") => {
                let (mut focused, mut idle_secs) = (None, None);
                for token in tokens {
                    if let Some(value) = token.strip_prefix("focused=") {
                        focused = match value {
                            "0" => Some(false),
                            "1" => Some(true),
                            _ => return Answer::Unknown,
                        };
                    } else if let Some(value) = token.strip_prefix("idle=") {
                        match value.parse() {
                            Ok(secs) => idle_secs = Some(secs),
                            Err(_) => return Answer::Unknown,
                        }
                    }
                }
                match (focused, idle_secs) {
                    (Some(focused), Some(idle_secs)) => Answer::Live { focused, idle_secs },
                    _ => Answer::Unknown,
                }
            }
            _ => Answer::Unknown,
        }
    }
}

/// The request line for `pane`, newline included.
pub fn request_line(pane: &PaneUuid) -> String {
    format!("{REQUEST_WORD} {WIRE_VERSION} {}\n", pane.as_str())
}

/// The asked pane from a request line (without its newline): `focus 1 <UUID>`
/// exactly. Another version, another word or a broken identity is `None` and
/// the connection closes without an answer.
pub fn parse_request(line: &[u8]) -> Option<PaneUuid> {
    let line = std::str::from_utf8(line).ok()?;
    let mut words = line.split(' ');
    let (word, version, id) = (words.next()?, words.next()?, words.next()?);
    if word != REQUEST_WORD || version != WIRE_VERSION || words.next().is_some() {
        return None;
    }
    PaneUuid::parse(id)
}

/// One line from `stream`, its newline dropped: at most `limit` bytes, by
/// `deadline`. `None` for a longer line, end of stream before the newline,
/// the deadline or an error.
fn read_line(stream: &mut UnixStream, limit: usize, deadline: Instant) -> Option<Vec<u8>> {
    let mut line = Vec::with_capacity(limit);
    let mut chunk = [0u8; 64];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return None;
        }
        stream.set_read_timeout(Some(left)).ok()?;
        let room = (limit + 1 - line.len()).min(chunk.len());
        let read = match stream.read(&mut chunk[..room]) {
            Ok(0) => return None,
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return None,
        };
        line.extend_from_slice(&chunk[..read]);
        if let Some(end) = line.iter().position(|&byte| byte == b'\n') {
            line.truncate(end);
            return Some(line);
        }
        if line.len() > limit {
            return None;
        }
    }
}

// ─── clock ───────────────────────────────────────────────────────────────

/// A moment on a monotonic clock that **counts sleep** (macOS
/// `CLOCK_MONOTONIC`, Linux `CLOCK_BOOTTIME`): Rust's `Instant` stops during
/// sleep on macOS, and after two hours with the lid closed `idle` would say
/// four seconds — the outside process would take it as "looking".
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Moment(Duration);

impl Moment {
    /// Now. A failing clock (no such thing in practice) reads zero: `idle`
    /// then saturates at zero, never a panic.
    pub fn now() -> Moment {
        #[cfg(target_os = "linux")]
        let clock = libc::CLOCK_BOOTTIME;
        #[cfg(not(target_os = "linux"))]
        let clock = libc::CLOCK_MONOTONIC;
        let mut spec = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: `spec` is a valid, writable `timespec`; the clock id is a
        // constant this platform defines.
        if unsafe { libc::clock_gettime(clock, &mut spec) } != 0 {
            return Moment(Duration::ZERO);
        }
        let secs = u64::try_from(spec.tv_sec).unwrap_or(0);
        let nanos = u32::try_from(spec.tv_nsec).unwrap_or(0);
        Moment(Duration::new(secs, nanos))
    }

    #[cfg(test)]
    fn at(since_origin: Duration) -> Moment {
        Moment(since_origin)
    }
}

/// `idle`: whole seconds from `since` to `now`, rounded **down**; a `since`
/// after `now` is zero.
pub fn idle_secs(since: Moment, now: Moment) -> u64 {
    now.0.saturating_sub(since.0).as_secs()
}

// ─── server ──────────────────────────────────────────────────────────────

/// The server's answer source: the asked pane's answer, or `None` when it
/// could not be had in time ([`ANSWER_WAIT`]) — the reply is `pane=unknown`.
pub type Answerer = Arc<dyn Fn(&PaneUuid) -> Option<Answer> + Send + Sync>;

/// Listens on `<dir>/`[`FOCUS_SOCKET`]: a stale file there is removed first,
/// the socket is bound **before** this returns, the accept loop runs on its
/// own thread for the process' lifetime. Each connection gets a short-lived
/// thread (at most [`MAX_IN_FLIGHT`]) with [`SERVER_READ_LIMIT`]: a client
/// that connects and never writes holds only its own thread. The path is
/// checked against `sun_path` only (`ssh_route::fits`' space and `%` refusal
/// is ssh's concern, not this socket's).
pub fn serve(dir: &Path, answerer: Answerer) -> io::Result<()> {
    let path = dir.join(FOCUS_SOCKET);
    if path.as_os_str().len() >= SUN_PATH {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "focus socket path too long",
        ));
    }
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let listener = UnixListener::bind(&path)?;
    thread::Builder::new()
        .name("focus listener".into())
        .spawn(move || accept_loop(&listener, &answerer))?;
    Ok(())
}

fn accept_loop(listener: &UnixListener, answerer: &Answerer) {
    let in_flight = Arc::new(AtomicUsize::new(0));
    loop {
        let stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::ConnectionAborted => continue,
            // Out of descriptors or buffers (a GUI app's soft limit is low):
            // passing, so back off and listen on — returning would leave a
            // bound socket nobody reads until the process ends.
            Err(error) if is_transient(&error) => {
                thread::sleep(ACCEPT_BACKOFF);
                continue;
            }
            // A dead listener: every client reads `unknown` — the safe way.
            Err(_) => return,
        };
        if in_flight.fetch_add(1, Ordering::SeqCst) >= MAX_IN_FLIGHT {
            in_flight.fetch_sub(1, Ordering::SeqCst);
            continue;
        }
        let (answerer, count) = (Arc::clone(answerer), Arc::clone(&in_flight));
        let spawned = thread::Builder::new()
            .name("focus answer".into())
            .spawn(move || {
                answer(stream, &answerer);
                count.fetch_sub(1, Ordering::SeqCst);
            });
        if spawned.is_err() {
            in_flight.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

/// How long the accept loop waits after a passing failure ([`is_transient`])
/// — a **design constant**, well below the client's per-instance limit.
const ACCEPT_BACKOFF: Duration = Duration::from_millis(50);

/// An accept failure that passes: the process' or the system's descriptor
/// table is full, or the kernel is out of buffers.
fn is_transient(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(libc::EMFILE | libc::ENFILE | libc::ENOBUFS | libc::ENOMEM)
    )
}

/// One connection: the request within the limit, then the answer line. A
/// broken request closes without a byte.
fn answer(mut stream: UnixStream, answerer: &Answerer) {
    let deadline = Instant::now() + SERVER_READ_LIMIT;
    let Some(line) = read_line(&mut stream, REQUEST_LIMIT, deadline) else {
        return;
    };
    let Some(pane) = parse_request(&line) else {
        return;
    };
    let answer = answerer(&pane).unwrap_or(Answer::Unknown);
    let _ = stream.set_write_timeout(Some(SERVER_READ_LIMIT));
    let _ = stream.write_all(format!("{}\n", answer.token_line()).as_bytes());
}

// ─── client ──────────────────────────────────────────────────────────────

/// The client's result: the answer and the line to print — for `pane=live`
/// the instance's own line, as it sent it (a new token reaches the
/// outside process without a new client); otherwise the canonical line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    pub answer: Answer,
    pub line: String,
}

impl Reply {
    fn canonical(answer: Answer) -> Reply {
        Reply {
            answer,
            line: answer.token_line(),
        }
    }
}

/// Asks the live instances under `roots` ([`ssh_route::live_instances`])
/// about `pane`: with `pid` only the instance that pid owns, otherwise all of
/// them in order — the first `pane=live` wins. None knows it → `pane=none`
/// (no live instance at all included); one could not answer and none said
/// live → `pane=unknown`. Each instance gets [`CLIENT_INSTANCE_LIMIT`], all of
/// them [`CLIENT_TOTAL_LIMIT`].
pub fn ask(roots: &[PathBuf], pid: Option<u32>, pane: &PaneUuid) -> Reply {
    let deadline = Instant::now() + CLIENT_TOTAL_LIMIT;
    let mut unknown = false;
    for instance in ssh_route::live_instances(roots) {
        if pid.is_some_and(|pid| pid != instance.pid) {
            continue;
        }
        let reply = ask_instance(&instance.dirs, pane, deadline);
        match reply.answer {
            Answer::Live { .. } => return reply,
            Answer::None => {}
            Answer::Unknown => unknown = true,
        }
    }
    Reply::canonical(if unknown {
        Answer::Unknown
    } else {
        Answer::None
    })
}

/// One instance: the first of its directories whose socket accepts. None
/// accepts (an older bateri, the listener not up yet, a stale file — refused
/// or absent) → `unknown`: a live instance that cannot answer may still hold
/// the pane.
fn ask_instance(dirs: &[PathBuf], pane: &PaneUuid, deadline: Instant) -> Reply {
    let deadline = deadline.min(Instant::now() + CLIENT_INSTANCE_LIMIT);
    for dir in dirs {
        // No connect timeout in std: a local connect only blocks on a full
        // backlog (Linux), and the server accepts on a thread of its own.
        let Ok(mut stream) = UnixStream::connect(dir.join(FOCUS_SOCKET)) else {
            continue;
        };
        return exchange(&mut stream, pane, deadline);
    }
    Reply::canonical(Answer::Unknown)
}

fn exchange(stream: &mut UnixStream, pane: &PaneUuid, deadline: Instant) -> Reply {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero()
        || stream.set_write_timeout(Some(left)).is_err()
        || stream.write_all(request_line(pane).as_bytes()).is_err()
    {
        return Reply::canonical(Answer::Unknown);
    }
    let Some(line) = read_line(stream, ANSWER_LIMIT, deadline) else {
        return Reply::canonical(Answer::Unknown);
    };
    let Ok(line) = String::from_utf8(line) else {
        return Reply::canonical(Answer::Unknown);
    };
    match Answer::parse(&line) {
        answer @ Answer::Live { .. } => Reply { answer, line },
        answer => Reply::canonical(answer),
    }
}

// ─── `bateri focus` ──────────────────────────────────────────────────────

/// `bateri focus`'s usage line (stderr, on a usage error).
pub const USAGE: &str = "usage: bateri focus [--pid PID] bateri://tab/<UUID>";

/// `bateri focus [--pid P] <url>`'s body (`args` after `focus`): the token
/// line and its newline to `out`, the exit code back — [`EXIT_ANSWER`],
/// [`EXIT_UNKNOWN`] or [`EXIT_USAGE`] (a diagnostic on stderr, nothing on
/// `out`). No GUI, no AppKit.
pub fn focus_main(args: &[String], roots: &[PathBuf], out: &mut impl Write) -> i32 {
    let Some((pid, pane)) = parse_args(args) else {
        eprintln!("{USAGE}");
        return EXIT_USAGE;
    };
    let reply = ask(roots, pid, &pane);
    let written = out
        .write_all(reply.line.as_bytes())
        .and_then(|()| out.write_all(b"\n"))
        .and_then(|()| out.flush());
    if written.is_err() {
        return EXIT_UNKNOWN;
    }
    match reply.answer {
        Answer::Unknown => EXIT_UNKNOWN,
        Answer::Live { .. } | Answer::None => EXIT_ANSWER,
    }
}

/// `[--pid P] <url>`: the pid a positive number, the URL `bateri://tab/<UUID>`.
fn parse_args(args: &[String]) -> Option<(Option<u32>, PaneUuid)> {
    match args {
        [url] => Some((None, PaneUuid::from_url(url)?)),
        [flag, pid, url] if flag == "--pid" => {
            let pid = pid.parse::<u32>().ok().filter(|&pid| pid > 0)?;
            Some((Some(pid), PaneUuid::from_url(url)?))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::os::unix::fs::DirBuilderExt;

    use crate::ssh_route::prepare_instance;

    const ID: &str = "0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0";
    const OTHER: &str = "11111111-2222-3333-4444-555555555555";

    fn pane(id: &str) -> PaneUuid {
        PaneUuid::parse(id).unwrap()
    }

    fn scratch(name: &str) -> PathBuf {
        // Short on purpose: sockets are bound under it.
        let dir = PathBuf::from(format!("/tmp/bt-fo-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // A root of instance directories is private (`prepare_dir`).
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&dir)
            .expect("temp directory");
        dir
    }

    /// An answerer that knows `ID` only.
    fn knows_id(focused: bool) -> Answerer {
        Arc::new(move |asked: &PaneUuid| {
            Some(if asked.as_str() == ID {
                Answer::Live {
                    focused,
                    idle_secs: 4,
                }
            } else {
                Answer::None
            })
        })
    }

    /// An instance directory under `root` owned by `pid` (this process when
    /// `None`).
    fn instance(root: &Path, name: &str, pid: Option<u32>) -> PathBuf {
        let dir = prepare_instance(root, name).unwrap();
        if let Some(pid) = pid {
            std::fs::write(dir.join("pid"), pid.to_string()).unwrap();
        }
        dir
    }

    #[test]
    fn running_out_of_descriptors_does_not_end_the_listener() {
        for code in [libc::EMFILE, libc::ENFILE, libc::ENOBUFS, libc::ENOMEM] {
            assert!(is_transient(&io::Error::from_raw_os_error(code)), "{code}");
        }
        assert!(!is_transient(&io::Error::from_raw_os_error(libc::EBADF)));
        assert!(!is_transient(&io::Error::other("no os code")));
    }

    #[test]
    fn the_wire_round_trips() {
        let line = request_line(&pane(ID));
        assert_eq!(line, format!("focus 1 {ID}\n"));
        assert_eq!(
            parse_request(line.trim_end().as_bytes()),
            Some(pane(ID)),
            "round trip"
        );
        for answer in [
            Answer::Live {
                focused: true,
                idle_secs: 4,
            },
            Answer::Live {
                focused: false,
                idle_secs: 0,
            },
            Answer::None,
            Answer::Unknown,
        ] {
            assert_eq!(Answer::parse(&answer.token_line()), answer);
        }
        assert_eq!(
            Answer::Live {
                focused: true,
                idle_secs: 4
            }
            .token_line(),
            "pane=live focused=1 idle=4"
        );
    }

    #[test]
    fn broken_requests_are_refused() {
        let long = format!("focus 1 {ID}{}", "x".repeat(REQUEST_LIMIT));
        for line in [
            format!("focus 2 {ID}"),
            format!("focus  1 {ID}"),
            format!("focus 1 {ID} extra"),
            format!("blur 1 {ID}"),
            "focus 1 not-a-uuid".to_owned(),
            "focus 1".to_owned(),
            String::new(),
            long,
        ] {
            assert_eq!(parse_request(line.as_bytes()), None, "{line:?}");
        }
        assert_eq!(parse_request(&[0xff, 0xfe]), None);
    }

    #[test]
    fn answers_skip_unknown_tokens_and_refuse_broken_ones() {
        assert_eq!(
            Answer::parse("pane=live idle=7 future=x focused=0"),
            Answer::Live {
                focused: false,
                idle_secs: 7
            }
        );
        for line in [
            "pane=live focused=1",
            "pane=live idle=3",
            "pane=live focused=2 idle=3",
            "pane=live focused=1 idle=-1",
            "pane=maybe",
            "",
            "pane=live focused=1 idle=3\u{1b}[2J",
        ] {
            assert_eq!(Answer::parse(line), Answer::Unknown, "{line:?}");
        }
    }

    #[test]
    fn idle_rounds_down_to_whole_seconds() {
        let at = |millis| Moment::at(Duration::from_millis(millis));
        assert_eq!(idle_secs(at(1_000), at(1_000)), 0);
        assert_eq!(idle_secs(at(1_000), at(1_999)), 0);
        assert_eq!(idle_secs(at(1_000), at(2_000)), 1);
        assert_eq!(idle_secs(at(1_000), at(5_999)), 4);
        assert_eq!(idle_secs(at(5_000), at(1_000)), 0, "a later stamp");
        let now = Moment::now();
        assert!(Moment::now() >= now, "monotonic");
    }

    #[test]
    fn a_live_instance_answers_over_a_real_socket() {
        let root = scratch("live");
        let dir = instance(&root, "aaaaaaaa", None);
        serve(&dir, knows_id(true)).unwrap();
        let roots = [root.clone()];
        let reply = ask(&roots, None, &pane(ID));
        assert_eq!(reply.line, "pane=live focused=1 idle=4");
        assert_eq!(ask(&roots, None, &pane(OTHER)).answer, Answer::None);
        let mut out = Vec::new();
        assert_eq!(
            focus_main(
                &[
                    "--pid".into(),
                    std::process::id().to_string(),
                    pane(ID).url()
                ],
                &roots,
                &mut out
            ),
            EXIT_ANSWER
        );
        assert_eq!(out, b"pane=live focused=1 idle=4\n");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn no_instance_is_none_and_usage_errors_print_nothing() {
        let root = scratch("none");
        let roots = [root.clone()];
        let mut out = Vec::new();
        assert_eq!(focus_main(&[pane(ID).url()], &roots, &mut out), EXIT_ANSWER);
        assert_eq!(out, b"pane=none\n");
        for args in [
            vec![],
            vec!["bateri://tab/nope".to_owned()],
            vec!["--pid".to_owned(), pane(ID).url()],
            vec!["--pid".to_owned(), "0".to_owned(), pane(ID).url()],
            vec!["--pid".to_owned(), "x".to_owned(), pane(ID).url()],
            vec!["--now".to_owned(), "1".to_owned(), pane(ID).url()],
            vec![pane(ID).url(), pane(ID).url()],
        ] {
            let mut out = Vec::new();
            assert_eq!(focus_main(&args, &roots, &mut out), EXIT_USAGE, "{args:?}");
            assert!(out.is_empty());
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_silent_listener_is_unknown_within_the_limit() {
        let root = scratch("silent");
        let dir = instance(&root, "aaaaaaaa", None);
        // Bound and listening, never accepted: the connect succeeds.
        let _listener = UnixListener::bind(dir.join(FOCUS_SOCKET)).unwrap();
        let started = Instant::now();
        let mut out = Vec::new();
        assert_eq!(
            focus_main(&[pane(ID).url()], &[root.clone()], &mut out),
            EXIT_UNKNOWN
        );
        assert_eq!(out, b"pane=unknown\n");
        assert!(
            started.elapsed() < CLIENT_TOTAL_LIMIT + Duration::from_millis(300),
            "{:?}",
            started.elapsed()
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_stale_socket_is_skipped() {
        let root = scratch("stale");
        // A living instance (the parent stands in) whose listener is gone:
        // refused, so it cannot vouch — and the next instance answers.
        let stale = instance(&root, "aaaaaaaa", Some(std::os::unix::process::parent_id()));
        drop(UnixListener::bind(stale.join(FOCUS_SOCKET)).unwrap());
        let roots = [root.clone()];
        assert_eq!(ask(&roots, None, &pane(ID)).answer, Answer::Unknown);
        let live = instance(&root, "bbbbbbbb", None);
        serve(&live, knows_id(false)).unwrap();
        assert_eq!(
            ask(&roots, None, &pane(ID)).answer,
            Answer::Live {
                focused: false,
                idle_secs: 4
            }
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn pid_asks_only_its_instance() {
        let root = scratch("pid");
        let parent = std::os::unix::process::parent_id();
        let theirs = instance(&root, "aaaaaaaa", Some(parent));
        serve(&theirs, knows_id(true)).unwrap();
        let ours = instance(&root, "bbbbbbbb", None);
        serve(&ours, Arc::new(|_: &PaneUuid| Some(Answer::None))).unwrap();
        let roots = [root.clone()];
        assert_eq!(
            ask(&roots, Some(std::process::id()), &pane(ID)).answer,
            Answer::None
        );
        assert_eq!(
            ask(&roots, Some(parent), &pane(ID)).answer,
            Answer::Live {
                focused: true,
                idle_secs: 4
            }
        );
        assert_eq!(ask(&roots, Some(u32::MAX), &pane(ID)).answer, Answer::None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_client_that_never_writes_does_not_hold_the_next() {
        let root = scratch("hold");
        let dir = instance(&root, "aaaaaaaa", None);
        serve(&dir, knows_id(true)).unwrap();
        let mut idle = UnixStream::connect(dir.join(FOCUS_SOCKET)).unwrap();
        idle.write_all(b"focus 1 ").unwrap();
        let started = Instant::now();
        let reply = ask(&[root.clone()], None, &pane(ID));
        assert_eq!(reply.line, "pane=live focused=1 idle=4");
        assert!(
            started.elapsed() < SERVER_READ_LIMIT,
            "the second client waited for the first: {:?}",
            started.elapsed()
        );
        drop(idle);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_answerer_out_of_time_is_unknown_and_a_broken_request_gets_nothing() {
        let root = scratch("late");
        let dir = instance(&root, "aaaaaaaa", None);
        serve(&dir, Arc::new(|_: &PaneUuid| None)).unwrap();
        assert_eq!(
            ask(&[root.clone()], None, &pane(ID)).answer,
            Answer::Unknown
        );
        let mut stream = UnixStream::connect(dir.join(FOCUS_SOCKET)).unwrap();
        stream
            .write_all(format!("focus 9 {ID}\n").as_bytes())
            .unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut rest = Vec::new();
        stream.read_to_end(&mut rest).unwrap();
        assert!(rest.is_empty(), "an unknown version was answered");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn serving_replaces_a_stale_file() {
        let root = scratch("replace");
        let dir = instance(&root, "aaaaaaaa", None);
        std::fs::write(dir.join(FOCUS_SOCKET), "").unwrap();
        serve(&dir, knows_id(true)).unwrap();
        assert!(matches!(
            ask(&[root.clone()], None, &pane(ID)).answer,
            Answer::Live { .. }
        ));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
