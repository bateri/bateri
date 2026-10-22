//! A pane's desk for the view protocol: where a program that writes pencere
//! frames into its pane is heard and answered, and where the shell takes what
//! the conversation asks of the window.
//!
//! The rules of the protocol are the `pencere` crate's and are written once
//! there: who may open a view, which `open` is refused and why, which files of
//! a bundle are missing, what the view is told, what ends it. The crate does
//! no I/O and has no clock; this module is what a terminal supplies to it for
//! one pane — the stream, the source of unpredictable bytes, the store of
//! verified files, what can be seen of the pane, the theme — and where the
//! crate's answers go: bytes into the pane's input, everything else to the
//! shell, which owns the web view, the question to the person and the timers.
//!
//! **Why it is in the stream.** The program's `hello` is answered before the
//! reply to the `CSI c` its SDK writes right behind it (§5.2): that order is
//! only there if the `welcome` is written while the parser is still between the
//! two, which is the parser's thread and no other. A shell that read the
//! output later would reach the `CSI c` reply first. So a frame is taken in the
//! same place a kitty graphics command is: the parser hands the body of an
//! `ESC _ … ESC \` over, [`crate::graphics::Commands`] puts the frame together
//! (a message is several frames), and the message that ends is **parked** here
//! while the parser halts behind it.
//!
//! **Why it is outside the lock.** Under the `Term` lock only frames are put
//! together, each at most 4 KiB. What a message makes the crate do is not
//! cheap: a bundle's file is up to 8 MiB of base64 whose SHA-256 is checked,
//! and a manifest's JSON is read. The reader lets go of the lock and the lease
//! a frame waits for ([`Desk::settle`], from `settle_halt`), the crate takes
//! the message, and the bytes it writes go to the pane's input in the order
//! the crate made them; then the parser goes on. The lock order is `term` →
//! the desk's slots, a **leaf** that holds a push or a take and nothing else, so
//! the reverse order exists nowhere (what is observed of the pane — the shell's
//! log, the master's modes — is read before the conversation is locked, and the
//! wake is called after both are released).
//!
//! **What does not come in.** A stream with no desk (the journal's replay, the
//! handover's VT, a session opened without [`ViewOptions`]) skips the frames:
//! the bytes are still the parser's and the screen is the same, but nothing is
//! heard or written — a replay must not answer twice. A frame that was cut by a
//! handover is not carried: the new process offers the program a restart
//! instead (§10.7), which is what the stream the old process had cannot give.
//!
//! **A known limit.** The scanner (OSC 133) has read the whole of a read by the
//! time the parser reaches a frame in it, so a command's end and a frame written
//! just before it can be seen in the opposite order. What the crate checks when it
//! hears a `hello` (the prompt is not being edited, the shell is not in the
//! foreground of the tty, the tty is raw) is read when the frame is heard, which
//! is what keeps a stale `hello` from being answered into the shell's input.

use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::fs::File;
use std::mem;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use ::pencere::frame::Message;
use ::pencere::session::{Config, Observed, Session as Conversation};
use ::pencere::theme::{Rgb, Theme as ViewTheme};
use ::pencere::token::Entropy;

pub use ::pencere::bundle::{Digest, Store, Verified};
pub use ::pencere::icon;
pub use ::pencere::message::HostInfo;
pub use ::pencere::sandbox::{
    Consent, Declared, Directive, Domain, Grant, Locality, Permission, UiMeta,
};
pub use ::pencere::session::{Action, Size, State, ViewSource, ViewStart};

use crate::color::{Theme, srgb_bytes};
use crate::shell::ShellLog;
use crate::wake::Wake;

/// The monospaced font list a view is given when the person named no family
/// (`[font] family`): the system's, as a CSS `font-family`.
pub const DEFAULT_FONT_MONO: &str = "\"SF Mono\", Menlo, monospace";

/// The bytes the in-memory shelf keeps before the oldest go. A view's bundle is
/// at most 64 MiB (the protocol's limit), so one bundle always fits; the number is a
/// policy, not a measurement, for a pane opened with no store of its own.
const MEMORY_LIMIT: usize = 128 * 1024 * 1024;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The font list a view gets for the family the person set: the family first
/// (without the characters that would end its quotes), then the system's.
/// `None`, or a family with nothing left, is just the system's.
pub fn font_stack(family: Option<&str>) -> String {
    let family: String = family
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '"' | '\'' | '\\' | ','))
        .collect();
    let family = family.trim();
    if family.is_empty() {
        DEFAULT_FONT_MONO.to_owned()
    } else {
        format!("\"{family}\", {DEFAULT_FONT_MONO}")
    }
}

/// What a pane is opened with to host views ([`crate::SessionOptions::views`]).
/// A pane opened without it hears no frame and answers none: its program falls
/// back to what it does when nothing answers.
#[derive(Clone, Debug)]
pub struct ViewOptions {
    /// The application's name and version: in the `welcome`, as the MCP client
    /// of the program and as the host of the view.
    pub host: HostInfo,
    /// Where verified bundle files are kept, by digest.
    pub store: Shelf,
    /// The font list the view is given ([`font_stack`]).
    pub font_mono: String,
    /// The person's language and region (BCP 47), when known.
    pub locale: Option<String>,
    /// The person's time zone (IANA), when known.
    pub time_zone: Option<String>,
    /// The pane's content area as the application knows it, in CSS pixels, which are
    /// points — the unit a view is told its container in. This crate has no scale to
    /// turn the grid's device pixels into them, so the application says; a pane it does
    /// not know the area of yet is `0 × 0`, and the view is told the real one
    /// ([`crate::Session::set_view_size`]) before it starts.
    pub size: Size,
    /// How to find the group the pane's shell leads ([`ShellGroup`]); `None` for a pane
    /// whose child is the shell itself.
    pub shell_group: Option<ShellGroup>,
}

impl ViewOptions {
    /// Options for an application named `name`, with an in-memory shelf and the
    /// system's font; the application sets the rest.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            host: HostInfo {
                name: name.into(),
                version: version.into(),
            },
            store: Shelf::in_memory(),
            font_mono: font_stack(None),
            locale: None,
            time_zone: None,
            size: Size {
                width: 0,
                height: 0,
            },
            shell_group: None,
        }
    }
}

/// How a pane finds the process group its shell leads, from the pid of the pane's child.
///
/// The child is the shell where the platform starts it directly. On macOS it is `login(1)`,
/// which forks the shell, so the group the terminal's foreground is compared with is a
/// child's, and finding it takes the process table, which this crate does not read: the
/// application hands the question down. `None` is "not known (yet)": `login` has not forked,
/// or the table could not be read.
#[derive(Clone)]
pub struct ShellGroup(Arc<dyn Fn(u32) -> Option<u32> + Send + Sync>);

impl ShellGroup {
    /// A finder: `find(child)` is the group the shell of the pane whose child is `child` leads.
    pub fn new(find: impl Fn(u32) -> Option<u32> + Send + Sync + 'static) -> Self {
        Self(Arc::new(find))
    }
}

impl fmt::Debug for ShellGroup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ShellGroup(..)")
    }
}

/// The group a pane's shell leads, found when first asked and kept: the shell is forked once
/// and keeps its group, and the process table is not asked on every frame.
struct Shell {
    find: Option<ShellGroup>,
    child: u32,
    /// `0` for not found yet: no process group has that id.
    found: AtomicU32,
}

impl Shell {
    fn new(find: Option<ShellGroup>, child: u32) -> Self {
        Self {
            find,
            child,
            found: AtomicU32::new(0),
        }
    }

    /// The group, or `None` when it cannot be told yet — which makes the check of the
    /// foreground unknown ([`observed`]), not failed.
    fn group(&self) -> Option<u32> {
        let Some(find) = &self.find else {
            // The child is the shell, and a shell is the leader of its own group.
            return Some(self.child);
        };
        match self.found.load(Ordering::Acquire) {
            0 => {
                let group = (find.0)(self.child).filter(|group| *group > 0)?;
                self.found.store(group, Ordering::Release);
                Some(group)
            }
            group => Some(group),
        }
    }
}

/// A store that locks for itself: panes call it at once, and what takes a while in it — a
/// file written, read and checked — does not make a call for another file wait.
pub trait Shared: Send + Sync {
    /// Whether the store holds the file with this digest.
    fn contains(&self, digest: &Digest) -> bool;
    /// Keeps a verified file.
    fn insert(&self, file: Verified);
    /// The file with this digest.
    fn get(&self, digest: &Digest) -> Option<Arc<[u8]>>;
}

/// A [`Store`] that needs `&mut` made [`Shared`] by one lock around each call: for the
/// stores that are memory only and fast.
struct Locked<S>(Mutex<S>);

impl<S: Store + Send> Shared for Locked<S> {
    fn contains(&self, digest: &Digest) -> bool {
        lock(&self.0).contains(digest)
    }

    fn insert(&self, file: Verified) {
        lock(&self.0).insert(file);
    }

    fn get(&self, digest: &Digest) -> Option<Arc<[u8]>> {
        lock(&self.0).get(digest)
    }
}

/// A store of bundle files that panes share: the one the application opened
/// (on disk, with its own policy for how much it keeps) or an in-memory one.
/// Clones are the same store.
///
/// **No lock of its own** for a store that locks for itself ([`Shelf::shared`]): a
/// pane's `get`, which the web view waits for, is not held up by another's
/// `insert`, which writes up to 8 MiB. A store that needs `&mut` ([`Shelf::new`])
/// gets one lock for all the panes, held for a call. Every call runs off the main
/// thread (the reader's, or the file queue's).
#[derive(Clone)]
pub struct Shelf(Arc<dyn Shared>);

impl Shelf {
    /// A shelf over `store`, one lock around every call.
    pub fn new(store: impl Store + Send + 'static) -> Self {
        Self(Arc::new(Locked(Mutex::new(store))))
    }

    /// A shelf over a store that does its own locking.
    pub fn shared(store: impl Shared + 'static) -> Self {
        Self(Arc::new(store))
    }

    /// A shelf in memory, bounded by [`MEMORY_LIMIT`]: the oldest file goes
    /// first. For a pane opened with no store of its own, and for tests.
    pub fn in_memory() -> Self {
        Self::new(Bounded::new(MEMORY_LIMIT))
    }
}

impl Store for Shelf {
    fn contains(&self, digest: &Digest) -> bool {
        self.0.contains(digest)
    }

    fn insert(&mut self, file: Verified) {
        self.0.insert(file);
    }

    fn get(&self, digest: &Digest) -> Option<Arc<[u8]>> {
        self.0.get(digest)
    }
}

impl fmt::Debug for Shelf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Shelf(..)")
    }
}

/// The in-memory store: files by digest, the oldest dropped when `limit` bytes
/// are passed (a store may drop an entry at any time, §8.5).
struct Bounded {
    files: HashMap<Digest, Arc<[u8]>>,
    order: VecDeque<Digest>,
    total: usize,
    limit: usize,
}

impl Bounded {
    fn new(limit: usize) -> Self {
        Self {
            files: HashMap::new(),
            order: VecDeque::new(),
            total: 0,
            limit,
        }
    }
}

impl Store for Bounded {
    fn contains(&self, digest: &Digest) -> bool {
        self.files.contains_key(digest)
    }

    fn insert(&mut self, file: Verified) {
        let (digest, bytes) = file.into_parts();
        if self.files.contains_key(&digest) {
            return;
        }
        self.total += bytes.len();
        self.files.insert(digest, bytes.into());
        self.order.push_back(digest);
        // The file just kept is the last to go: a store that dropped it would
        // make the open it is for ask again, forever.
        while self.total > self.limit && self.order.len() > 1 {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(gone) = self.files.remove(&oldest) {
                self.total -= gone.len();
            }
        }
    }

    fn get(&self, digest: &Digest) -> Option<Arc<[u8]>> {
        self.files.get(digest).cloned()
    }
}

/// The source of the token's bytes: the operating system's, `getentropy`. The
/// token is the only thing between a stream anyone can write to and a view that
/// runs code on the person's machine, so there is no weaker fallback: if the
/// call fails the desk **opens no view** (`failed` is read after every message,
/// and opens are refused with `policy`), rather than issue a token that can
/// be guessed.
pub(crate) struct SystemEntropy {
    failed: Arc<AtomicBool>,
}

impl Entropy for SystemEntropy {
    fn token_bytes(&mut self) -> [u8; 16] {
        system_random().unwrap_or_else(|| {
            self.failed.store(true, Ordering::Release);
            [0u8; 16]
        })
    }
}

/// Sixteen unpredictable bytes from the operating system (`getentropy`); `None` if the call
/// fails. **The one place the application asks for them**: the token of an `open` and the
/// random name of a view's origin both come from here, so a source that cannot be trusted
/// is one fix and there is no second `unsafe` call to keep in step with it.
pub fn system_random() -> Option<[u8; 16]> {
    let mut bytes = [0u8; 16];
    // SAFETY: the pointer is valid for `bytes.len()` writes, and `getentropy`
    // takes up to 256 bytes at a time.
    let done = unsafe { libc::getentropy(bytes.as_mut_ptr().cast(), bytes.len()) } == 0;
    done.then_some(bytes)
}

/// The crate's conversation, as a pane has it.
pub(crate) type Conversing = Conversation<Shelf, SystemEntropy>;

/// What the pane is seen as, asked when a frame is heard
/// ([`pane_observer`]).
pub(crate) type Observer = Box<dyn Fn() -> Observed + Send + Sync>;

/// The tty's two readings the hardening checks of the protocol use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Tty {
    /// The tty's foreground process group, if the call gave one.
    foreground: Option<u32>,
    /// `ICANON` or `ECHO` is on: the state a program must have left (§4.9).
    canonical_or_echo: bool,
}

/// Reads the pane's tty from the master's copy: on macOS and Linux the master's
/// `tcgetattr` answers with the slave's flags — the program's — and
/// `tcgetpgrp` with the foreground group. `None` if the modes cannot be read.
fn read_tty(master: BorrowedFd<'_>) -> Option<Tty> {
    // SAFETY: an all-zero `termios` is a valid value of a plain C struct; the
    // call only writes it.
    let mut modes: libc::termios = unsafe { mem::zeroed() };
    // SAFETY: `master` is open for the call's duration and `modes` is a valid
    // out pointer.
    if unsafe { libc::tcgetattr(master.as_raw_fd(), &mut modes) } != 0 {
        return None;
    }
    // SAFETY: as above; the result is checked.
    let group = unsafe { libc::tcgetpgrp(master.as_raw_fd()) };
    Some(Tty {
        // `-1`: the tty has no foreground group (yet, or any more).
        foreground: u32::try_from(group).ok().filter(|group| *group > 0),
        canonical_or_echo: modes.c_lflag & (libc::ICANON | libc::ECHO) != 0,
    })
}

/// What the protocol needs to know of a pane (§6.2, §6.5), from what was read
/// of it. On a pane in a remote session the tty is `ssh`'s, not the program's:
/// the two checks of the tty are skipped there (`None`), and the shell's own
/// marks stand. `shell` is the group the pane's shell leads, if it can be told:
/// when it cannot, whether the shell is in front is **unknown** (`None`), never
/// "no" — a check that said "no" for a shell that is in front would answer a
/// `hello` into its line editor.
fn observed(editing: bool, remote: bool, tty: Option<Tty>, shell: Option<u32>) -> Observed {
    let local = if remote { None } else { tty };
    Observed {
        prompt_editing: editing,
        // The moment of the frame, which the parser alone knows ([`Desk::park`]).
        alternate_screen: None,
        shell_in_foreground: local
            .and_then(|tty| tty.foreground)
            .zip(shell)
            .map(|(group, shell)| group == shell),
        tty_canonical_or_echo: local.map(|tty| tty.canonical_or_echo),
    }
}

/// The observer of a pane: the shell's log for the prompt and the remote
/// session, the master's copy for the tty. Each is read with its own leaf lock
/// and none is held while another is taken. `child_pid` is the pid of the
/// pane's child, which `group` turns into the shell's group where the child is
/// not the shell (`login(1)` on macOS).
pub(crate) fn pane_observer(
    shell: Arc<Mutex<ShellLog>>,
    master: Arc<Mutex<Option<File>>>,
    child_pid: u32,
    group: Option<ShellGroup>,
) -> Observer {
    let leads = Shell::new(group, child_pid);
    Box::new(move || {
        let (editing, remote) = {
            let log = lock(&shell);
            (log.prompt_editing(), log.in_remote_session())
        };
        let tty = if remote {
            None
        } else {
            lock(&master)
                .as_ref()
                .and_then(|file| read_tty(file.as_fd()))
        };
        observed(editing, remote, tty, leads.group())
    })
}

/// The terminal's look as the view is given it: the roles come from the
/// theme's own values and its own rules — the dim role, the accent, the
/// separator's step toward the background — and none is computed again here.
/// `font_mono` is a CSS font list ([`font_stack`]).
pub(crate) fn view_theme(theme: &Theme, font_mono: &str) -> ViewTheme {
    let role = |hex: u32| {
        let [r, g, b] = srgb_bytes(hex);
        Rgb(r, g, b)
    };
    let [r, g, b] = theme.background_srgb();
    let mut view = ViewTheme::new(Rgb(r, g, b), role(theme.foreground), font_mono);
    let [r, g, b] = theme.separator_srgb();
    view.dim = Some(role(theme.dim));
    view.accent = Some(role(theme.accent));
    view.border = Some(Rgb(r, g, b));
    view.success = Some(role(theme.success));
    view.error = Some(role(theme.error));
    view.warning = Some(role(theme.warning));
    view.info = Some(role(theme.info));
    view
}

/// Whether the conversation is somewhere the shell must put a deadline on: an
/// `open` that waits for its files, the MCP start-up or the person, and a view
/// that was told to go and has not answered. The crate has no clock
/// ([`crate::Session::view_wait_over`] is what the shell calls when the wait it
/// chose runs out), so it is the shell that has to know a wait has begun.
fn waits(state: State) -> bool {
    matches!(state, State::Opening | State::Closing)
}

/// The slot a pane's conversation is kept in. See the module's documentation.
///
/// Two leaf locks, so that the heavy work is never in the way of the cheap:
/// `inner` holds the conversation while a message is handled (a file's digest
/// is checked there, and a main-thread call such as a message from the view
/// waits for it), `slots` holds only what is handed from one thread to the
/// other — the parked messages and the shell's actions — and is never held
/// while anything is computed. The order is `inner` → `slots`, and nothing
/// takes `inner` while holding `slots`.
pub(crate) struct Desk {
    inner: Mutex<Inner>,
    slots: Mutex<Slots>,
    /// Told when the shell has something to take ([`Wake::view_changed`]).
    wake: Arc<dyn Wake>,
    observer: Observer,
    /// The token's source failed ([`SystemEntropy`]): no view opens.
    entropy_failed: Arc<AtomicBool>,
}

struct Inner {
    conversation: Conversing,
    /// The font list the view is given.
    font_mono: String,
    /// The theme the view was last given.
    theme: Theme,
}

struct Slots {
    /// The messages the parser parked, with whether the pane was on the
    /// alternate screen where each one ended. The parser halts behind each, so
    /// there is one at a time.
    parked: VecDeque<(Message, bool)>,
    /// What the shell is to do, in order: everything the conversation asked
    /// that is not bytes for the program.
    views: Vec<Action>,
    /// The conversation is in a state the shell sets a deadline for
    /// ([`waits`]), as the shell was last told: each edge of it is news.
    waiting: bool,
}

impl Desk {
    /// The desk of a pane with `options` (its content area in CSS pixels
    /// among them) and the opening `theme`.
    pub(crate) fn new(
        options: ViewOptions,
        theme: Theme,
        wake: Arc<dyn Wake>,
        observer: Observer,
    ) -> Self {
        let failed = Arc::new(AtomicBool::new(false));
        let ViewOptions {
            host,
            store,
            font_mono,
            locale,
            time_zone,
            size,
            // The observer's: how it finds the shell is [`pane_observer`]'s.
            shell_group: _,
        } = options;
        let mut config = Config::new(host, view_theme(&theme, &font_mono), size);
        config.view.locale = locale;
        config.view.time_zone = time_zone;
        let entropy = SystemEntropy {
            failed: Arc::clone(&failed),
        };
        Self {
            inner: Mutex::new(Inner {
                conversation: Conversation::new(config, store, entropy),
                font_mono,
                theme,
            }),
            slots: Mutex::new(Slots {
                parked: VecDeque::new(),
                views: Vec::new(),
                waiting: false,
            }),
            wake,
            observer,
            entropy_failed: failed,
        }
    }

    /// Parks a message the parser has just ended, with whether the pane was on
    /// the alternate screen where it ended. Under the `Term` lock: a push
    /// and nothing else.
    pub(crate) fn park(&self, message: Message, alternate: bool) {
        lock(&self.slots).parked.push_back((message, alternate));
    }

    /// Hears what the parser parked, with the lock and the lease let go: the
    /// crate's answer to each, in order, goes to `write` (the pane's input) and
    /// what is for the shell is kept for it.
    pub(crate) fn settle(&self, write: impl Fn(Vec<u8>)) {
        let parked = mem::take(&mut lock(&self.slots).parked);
        if parked.is_empty() {
            return;
        }
        // Read with no slot held ([`pane_observer`]'s leaf locks).
        let seen = (self.observer)();
        let (writes, notify) = {
            let mut inner = lock(&self.inner);
            for (message, alternate) in parked {
                // Before each message, not only after: a `hello` that issued a
                // token from a failed source must not have its `open` heard
                // as if the token were sound, however the messages are batched.
                self.close_if_unsafe(&mut inner);
                inner.conversation.observe(Observed {
                    alternate_screen: Some(alternate),
                    ..seen
                });
                inner.conversation.message(message);
            }
            self.drain(&mut inner)
        };
        self.deliver(writes, notify, write);
    }

    /// Runs `act` on the conversation, then does what it asked: bytes to
    /// `write`, the rest for the shell.
    pub(crate) fn apply<R>(
        &self,
        write: impl Fn(Vec<u8>),
        act: impl FnOnce(&mut Conversing) -> R,
    ) -> R {
        let (result, writes, notify) = {
            let mut inner = lock(&self.inner);
            let result = act(&mut inner.conversation);
            let (writes, notify) = self.drain(&mut inner);
            (result, writes, notify)
        };
        self.deliver(writes, notify, write);
        result
    }

    /// Hands the view's message to the conversation; whether the pane wrote anything to
    /// the program for it. What is written for a message of the view is a request of the
    /// view passed on (or the call of the tool the view is for): it is out until the
    /// program answers, and the shell is the one with a clock to give up waiting
    /// ([`crate::Session::view_requests_overdue`]). The bytes are the crate's own word
    /// that a request went out, so nothing of the message is read here.
    pub(crate) fn view_message(&self, text: &str, write: impl Fn(Vec<u8>)) -> bool {
        let wrote = Cell::new(false);
        self.apply(
            |bytes| {
                wrote.set(true);
                write(bytes);
            },
            |conversation| conversation.view_message(text),
        );
        wrote.get()
    }

    /// The program ended (§10.3): the command's `D`, or the child's exit.
    pub(crate) fn program_ended(&self, write: impl Fn(Vec<u8>)) {
        self.apply(write, Conversing::program_ended);
    }

    /// Starts the view again, with what the pane is seen as now: the crate
    /// offers the program a restart only if the checks of §6.2 pass.
    pub(crate) fn restart(&self, write: impl Fn(Vec<u8>)) {
        let seen = (self.observer)();
        self.apply(write, |conversation| {
            conversation.observe(seen);
            conversation.restart();
        });
    }

    /// The terminal's look changed. The font list stays what it was.
    pub(crate) fn set_theme(&self, theme: Theme, write: impl Fn(Vec<u8>)) {
        let (writes, notify) = {
            let mut inner = lock(&self.inner);
            if inner.theme == theme {
                return;
            }
            inner.theme = theme;
            let view = view_theme(&inner.theme, &inner.font_mono);
            inner.conversation.set_theme(view);
            self.drain(&mut inner)
        };
        self.deliver(writes, notify, write);
    }

    /// The font list changed (`[font] family`): the view is told if it runs.
    pub(crate) fn set_font(&self, family: Option<&str>, write: impl Fn(Vec<u8>)) {
        let font_mono = font_stack(family);
        let (writes, notify) = {
            let mut inner = lock(&self.inner);
            if inner.font_mono == font_mono {
                return;
            }
            inner.font_mono = font_mono;
            let view = view_theme(&inner.theme, &inner.font_mono);
            inner.conversation.set_theme(view);
            self.drain(&mut inner)
        };
        self.deliver(writes, notify, write);
    }

    /// What the shell is to do, in order; empties the slot.
    pub(crate) fn take_views(&self) -> Vec<Action> {
        mem::take(&mut lock(&self.slots).views)
    }

    /// The token's source failed: nothing may open (see [`SystemEntropy`]).
    fn close_if_unsafe(&self, inner: &mut Inner) {
        if self.entropy_failed.load(Ordering::Acquire) {
            inner.conversation.allow_opens(false);
        }
    }

    /// Takes the conversation's actions out: the bytes for the program are
    /// returned, the rest is kept for the shell, and whether the shell has to be
    /// told — only when the slot was empty, so a burst is one news. A wait that
    /// begins or ends is news as well, though nothing is kept for it: an `open`
    /// is answered with bytes only, and the shell is the one with a clock to end
    /// the wait with ([`waits`]) — and to forget it when the conversation left
    /// the state on its own.
    fn drain(&self, inner: &mut Inner) -> (Vec<Vec<u8>>, bool) {
        self.close_if_unsafe(inner);
        let mut writes = Vec::new();
        let mut kept = Vec::new();
        for action in inner.conversation.take_actions() {
            match action {
                Action::Write(bytes) => writes.push(bytes),
                other => kept.push(other),
            }
        }
        let waiting = waits(inner.conversation.state());
        let mut slots = lock(&self.slots);
        let moved = waiting != slots.waiting;
        slots.waiting = waiting;
        let notify = slots.views.is_empty() && (!kept.is_empty() || moved);
        slots.views.extend(kept);
        (writes, notify)
    }

    /// Writes the bytes and tells the shell: with no slot held, and the bytes
    /// first, as the crate's order is.
    fn deliver(&self, writes: Vec<Vec<u8>>, notify: bool, write: impl Fn(Vec<u8>)) {
        for bytes in writes {
            write(bytes);
        }
        if notify {
            self.wake.view_changed();
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
