//! The helper's life (Karar 1, 5): started with the first browser, ended some time after the last
//! one is closed, and what the browsers are told when it dies.
//!
//! [`Manager`] is one for the application. A browser is [`Manager::open`]ed (a [`Browser`] is its
//! handle: it closes it when dropped); the first one starts the helper, the last one's closing
//! ends it after `linger` — a page that closes a browser and opens another a moment later keeps
//! the engine. A helper that dies (the socket's end is how it is known) tells every open browser's
//! [`Sink`] `lost`; the next `open` starts another one.
//!
//! The start is the contract of `browser_ipc::Launch`: the proxy is open first (its port is on the
//! command line, for the helper's whole life), then the socket in a folder of this user's
//! alone, then the mach name the frames come to, then the process, and `accept` hears the hello
//! — the single-use key, the protocol, and the pid of the process that was started. The folder of
//! the app is checked just before it is run ([`Config::trust`]: the signature, the platform
//! shell's). Starting a process is given too ([`Spawn`]), so the whole life is tried with a helper
//! that is a thread of the test.
//!
//! **Not on the main thread.** `open` starts the helper when there is none, and that waits for
//! its hello (seconds, in a cold start).
//!
//! The helper hears of profiles through `Command::Open`. One context, one folder and one
//! credential per profile for the helper's life (the helper answers an `Error` to a context it
//! knows under another folder): a profile with a name is the same for every browser of the same
//! key; one without a name is its browser's own.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use browser_ipc::message::{Command, Event, Profile};
use browser_ipc::{Frame, Launch, Receiver, Surface, Writer, channel};

use super::layout::{self, Hold, Layout};
use super::profile::ProfileKey;
use super::proxy::Proxy;

/// How long the helper has to say hello.
pub const START_TIMEOUT: Duration = Duration::from_secs(20);
/// How long it keeps running after its last browser is closed.
pub const LINGER: Duration = Duration::from_secs(30);
/// How long a helper has to end on its own after `Quit` before it is killed.
const QUIT_WAIT: Duration = Duration::from_secs(8);

/// Checks the app that is about to run (its signature): an `Err` stops the start.
pub type Trust = Arc<dyn Fn(&Path) -> Result<(), String> + Send + Sync>;

/// A started helper process, or what stands for one.
pub trait Process: Send {
    /// The id the helper's hello must carry.
    fn id(&self) -> u32;
    /// What is known of its end, once it has ended.
    fn exited(&mut self) -> io::Result<Option<String>>;
    fn kill(&mut self);
}

/// Starts the helper's executable with its command line.
pub trait Spawn: Send + Sync {
    fn spawn(&self, executable: &Path, args: &[String]) -> io::Result<Box<dyn Process>>;
}

/// What a browser is told. Called from the helper's own threads: keep it short and hand the work
/// on (the main queue, a flag and a wake).
pub trait Sink: Send + Sync {
    /// Something happened to the browser (`Event::State`, `Popup`, `Menu`, `Download`, ...).
    fn event(&self, event: Event);
    /// A frame of the page or of a popup.
    fn frame(&self, frame: Frame, surface: Surface);
    /// The helper is gone: this browser with it. Said once.
    fn lost(&self, why: String);
}

/// Where the helper comes from.
#[derive(Clone, Debug)]
pub enum Source {
    /// The installed versions: the newest is run, and held while it runs.
    Installed(Layout),
    /// An app run where it is, and nothing is installed or pruned: a development copy's
    /// (`make dev`), which is not a release.
    Fixed(PathBuf),
}

#[derive(Clone)]
pub struct Config {
    pub source: Source,
    /// The helper's `--root`: where profiles are kept.
    pub profiles: PathBuf,
    /// A folder the socket is made in (`0700`).
    pub run: PathBuf,
    /// Cookie encryption off the user's keychain: tests and development copies.
    pub mock_keychain: bool,
    pub linger: Duration,
    pub start_timeout: Duration,
    pub trust: Trust,
}

#[derive(Debug)]
pub enum StartError {
    /// No helper is installed: the person is asked before one is downloaded.
    NotInstalled,
    /// The app is not the one bateri may run.
    Untrusted(String),
    Io(io::Error),
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartError::NotInstalled => f.write_str("the browser engine is not installed"),
            StartError::Untrusted(why) => write!(f, "the browser engine is not trusted: {why}"),
            StartError::Io(e) => write!(f, "the browser engine did not start: {e}"),
        }
    }
}

impl From<io::Error> for StartError {
    fn from(e: io::Error) -> Self {
        StartError::Io(e)
    }
}

/// What the helper is doing, for the person and for tests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub running: bool,
    /// The running helper's version and the Chromium in it.
    pub version: Option<String>,
    pub chromium: Option<String>,
    pub browsers: usize,
}

pub struct Manager {
    config: Config,
    spawn: Arc<dyn Spawn>,
    /// The helper that runs, if one does.
    current: Mutex<Option<Arc<Run>>>,
    /// One start at a time: a second `open` waits for the first one's helper.
    starting: Mutex<()>,
    next_browser: AtomicU32,
    next_run: AtomicU64,
}

/// One helper's life.
struct Run {
    id: u64,
    commands: Writer<Command>,
    proxy: Proxy,
    process: Mutex<Box<dyn Process>>,
    version: String,
    chromium: String,
    browsers: Mutex<HashMap<u32, Arc<dyn Sink>>>,
    /// Open browsers' profiles by context, so one is the same for every browser of its key.
    profiles: Mutex<HashMap<ProfileKey, Profile>>,
    /// Which browser an open download's progress is about.
    downloads: Mutex<HashMap<u32, u32>>,
    /// Counts every open and close: a stop that was scheduled for an idle helper does not happen
    /// when anything was opened or closed since.
    epoch: AtomicU64,
    /// Over: the helper is gone or was told to go.
    over: AtomicBool,
    _hold: Option<Hold>,
}

/// A browser of the helper. Closing it is dropping it.
pub struct Browser {
    id: u32,
    run: Arc<Run>,
    manager: Arc<Manager>,
}

impl Manager {
    pub fn new(config: Config, spawn: Arc<dyn Spawn>) -> Arc<Manager> {
        Arc::new(Manager {
            config,
            spawn,
            current: Mutex::new(None),
            starting: Mutex::new(()),
            next_browser: AtomicU32::new(1),
            next_run: AtomicU64::new(1),
        })
    }

    pub fn status(&self) -> Status {
        let current = lock(&self.current).clone();
        match current.filter(|run| !run.over.load(Ordering::Acquire)) {
            Some(run) => Status {
                running: true,
                version: Some(run.version.clone()),
                chromium: Some(run.chromium.clone()),
                browsers: lock(&run.browsers).len(),
            },
            None => Status {
                running: false,
                version: None,
                chromium: None,
                browsers: 0,
            },
        }
    }

    /// Whether a helper is installed to run (always, for a fixed one).
    pub fn installed(&self) -> bool {
        match &self.config.source {
            Source::Installed(layout) => layout.newest().is_some(),
            Source::Fixed(app) => app.is_dir(),
        }
    }

    /// Opens a browser in the profile `key` names, `width` × `height` points on a display of
    /// `scale`; its news go to `sink`. Starts the helper when none runs.
    pub fn open(
        self: &Arc<Self>,
        key: &ProfileKey,
        (width, height): (u32, u32),
        scale: f32,
        sink: Arc<dyn Sink>,
    ) -> Result<Browser, StartError> {
        let run = self.running()?;
        let id = self.next_browser.fetch_add(1, Ordering::Relaxed);
        let profile = run.profile(key, id);
        lock(&run.browsers).insert(id, sink);
        run.epoch.fetch_add(1, Ordering::AcqRel);
        let sent = run.commands.send(&Command::Open {
            browser: id,
            profile,
            width,
            height,
            scale,
        });
        if let Err(e) = sent {
            lock(&run.browsers).remove(&id);
            return Err(StartError::Io(e));
        }
        // The helper may have died between the lookup and the insert: nobody would say so.
        if run.over.load(Ordering::Acquire) {
            lock(&run.browsers).remove(&id);
            return Err(StartError::Io(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "the browser engine ended as the browser opened",
            )));
        }
        Ok(Browser {
            id,
            run,
            manager: self.clone(),
        })
    }

    /// The running helper, started first if there is none.
    fn running(self: &Arc<Self>) -> Result<Arc<Run>, StartError> {
        let _one_start = lock(&self.starting);
        if let Some(run) = lock(&self.current).clone()
            && !run.over.load(Ordering::Acquire)
        {
            return Ok(run);
        }
        let run = self.start()?;
        *lock(&self.current) = Some(run.clone());
        Ok(run)
    }

    fn start(self: &Arc<Self>) -> Result<Arc<Run>, StartError> {
        let (app, executable, hold) = match &self.config.source {
            Source::Installed(layout) => {
                let version = layout.newest().ok_or(StartError::NotInstalled)?;
                let hold = layout.hold(&version)?;
                (
                    layout.app(&version),
                    layout.executable(&version),
                    Some(hold),
                )
            }
            Source::Fixed(app) => {
                if !app.is_dir() {
                    return Err(StartError::NotInstalled);
                }
                (app.clone(), app.join(layout::EXECUTABLE), None)
            }
        };
        (self.config.trust)(&app).map_err(StartError::Untrusted)?;
        layout::make_private(&self.config.profiles)?;

        let proxy = Proxy::bind()?;
        let launch = Launch::prepare(&self.config.run)?;
        let receiver = Receiver::register(launch.mach_name())?;
        let args = launch.args(
            &self.config.profiles,
            &proxy.url(),
            self.config.mock_keychain,
        );
        let mut process = self.spawn.spawn(&executable, &args)?;
        let connection = match launch.accept(process.id(), self.config.start_timeout) {
            Ok(connection) => connection,
            Err(e) => {
                process.kill();
                return Err(StartError::Io(e));
            }
        };

        let run = Arc::new(Run {
            id: self.next_run.fetch_add(1, Ordering::Relaxed),
            commands: connection.commands,
            proxy,
            process: Mutex::new(process),
            version: connection.version,
            chromium: connection.chromium,
            browsers: Mutex::new(HashMap::new()),
            profiles: Mutex::new(HashMap::new()),
            downloads: Mutex::new(HashMap::new()),
            epoch: AtomicU64::new(0),
            over: AtomicBool::new(false),
            _hold: hold,
        });
        {
            let (manager, run, events) = (self.clone(), run.clone(), connection.events);
            std::thread::Builder::new()
                .name("browser-events".into())
                .spawn(move || manager.pump_events(&run, events))?;
        }
        {
            let run = run.clone();
            std::thread::Builder::new()
                .name("browser-frames".into())
                .spawn(move || pump_frames(&run, &receiver))?;
        }
        if let Source::Installed(layout) = &self.config.source {
            // What is older and no helper runs from is of no use any more.
            layout.prune();
        }
        Ok(run)
    }

    /// Reads the helper's news until its end, then tells what is open that it is gone.
    fn pump_events(&self, run: &Arc<Run>, mut events: channel::Reader<Event>) {
        while let Ok(Some(event)) = events.recv() {
            run.dispatch(event);
        }
        let stopped = run.over.swap(true, Ordering::AcqRel);
        {
            let mut current = lock(&self.current);
            if current.as_ref().is_some_and(|c| c.id == run.id) {
                *current = None;
            }
        }
        let why = {
            let mut process = lock(&run.process);
            let ended = process.exited().ok().flatten();
            if ended.is_none() {
                process.kill();
            }
            ended.unwrap_or_else(|| "its connection ended".to_owned())
        };
        let lost: Vec<Arc<dyn Sink>> = lock(&run.browsers).drain().map(|(_, sink)| sink).collect();
        if !stopped {
            for sink in lost {
                sink.lost(why.clone());
            }
        }
    }

    /// Ends the helper that is idle, if nothing was opened or closed since `epoch` and it is
    /// still the one.
    fn stop_if_idle(&self, run: &Arc<Run>, epoch: u64) {
        // A start waits for this one's end: two helpers on one folder of profiles would fight for it.
        let _no_start = lock(&self.starting);
        if run.epoch.load(Ordering::Acquire) != epoch
            || !lock(&run.browsers).is_empty()
            || run.over.swap(true, Ordering::AcqRel)
        {
            return;
        }
        {
            let mut current = lock(&self.current);
            if current.as_ref().is_some_and(|c| c.id == run.id) {
                *current = None;
            }
        }
        let _ = run.commands.send(&Command::Quit);
        let deadline = Instant::now() + QUIT_WAIT;
        loop {
            let ended = lock(&run.process).exited().ok().flatten().is_some();
            if ended {
                break;
            }
            if Instant::now() >= deadline {
                lock(&run.process).kill();
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        run.commands.shutdown();
    }
}

impl Run {
    /// The profile the helper is told of for `key`, for the browser `id`.
    fn profile(&self, key: &ProfileKey, id: u32) -> Profile {
        let mut profiles = lock(&self.profiles);
        if key.persistent()
            && let Some(profile) = profiles.get(key)
        {
            return profile.clone();
        }
        let dir = key.dir();
        let context = dir.clone().unwrap_or_else(|| format!("memory-{id}"));
        let profile = Profile {
            context: context.clone(),
            dir,
            user: context,
            pass: channel::random_token(),
        };
        self.proxy.allow(&profile.user, &profile.pass, key);
        if key.persistent() {
            profiles.insert(key.clone(), profile.clone());
        }
        profile
    }

    fn sink(&self, browser: u32) -> Option<Arc<dyn Sink>> {
        lock(&self.browsers).get(&browser).cloned()
    }

    fn dispatch(&self, event: Event) {
        let browser = match &event {
            Event::Download { browser, req, .. } => {
                lock(&self.downloads).insert(*req, *browser);
                Some(*browser)
            }
            Event::DownloadProgress { req, state, .. } => {
                let mut downloads = lock(&self.downloads);
                if matches!(state, browser_ipc::message::DownloadState::Running) {
                    downloads.get(req).copied()
                } else {
                    downloads.remove(req)
                }
            }
            other => other.browser(),
        };
        if let Some(sink) = browser.and_then(|browser| self.sink(browser)) {
            sink.event(event);
        }
    }
}

fn pump_frames(run: &Run, receiver: &Receiver) {
    while !run.over.load(Ordering::Acquire) {
        match receiver.recv(100) {
            Ok(Some((frame, surface))) => {
                if let Some(sink) = run.sink(frame.browser) {
                    sink.frame(frame, surface);
                }
            }
            Ok(None) => {}
            Err(_) => return,
        }
    }
}

/// A way to tell a browser something and to answer its questions, which does not keep it open:
/// closing is [`Browser`]'s drop alone. A question of the browser holds one, so that it is
/// answered whoever is holding the browser by then.
#[derive(Clone)]
pub struct Handle {
    id: u32,
    run: Arc<Run>,
}

impl Handle {
    pub fn id(&self) -> u32 {
        self.id
    }

    /// As [`Browser::send`]; an answer to a browser that is gone, or of a helper that ended,
    /// is an error nobody needs to act on.
    pub fn send(&self, command: Command) -> io::Result<()> {
        send_to(&self.run, self.id, command)
    }
}

fn send_to(run: &Run, id: u32, command: Command) -> io::Result<()> {
    if command.browser().is_some_and(|browser| browser != id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a command for another browser",
        ));
    }
    run.commands.send(&command)
}

impl Browser {
    pub fn id(&self) -> u32 {
        self.id
    }

    /// A handle on this browser for what answers its questions ([`Handle`]).
    pub fn handle(&self) -> Handle {
        Handle {
            id: self.id,
            run: self.run.clone(),
        }
    }

    /// Tells the helper something about this browser (or answers one of its questions: a menu,
    /// a dialog, a download). A command that names another browser is refused.
    pub fn send(&self, command: Command) -> io::Result<()> {
        send_to(&self.run, self.id, command)
    }

    /// Whether the helper this browser was opened in still runs.
    pub fn alive(&self) -> bool {
        !self.run.over.load(Ordering::Acquire)
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        let run = &self.run;
        if lock(&run.browsers).remove(&self.id).is_none() {
            return;
        }
        let _ = run.commands.send(&Command::Close { browser: self.id });
        let epoch = run.epoch.fetch_add(1, Ordering::AcqRel) + 1;
        if !lock(&run.browsers).is_empty() {
            return;
        }
        let (manager, run) = (self.manager.clone(), self.run.clone());
        let linger = manager.config.linger;
        let _ = std::thread::Builder::new()
            .name("browser-linger".into())
            .spawn(move || {
                std::thread::sleep(linger);
                manager.stop_if_idle(&run, epoch);
            });
    }
}

/// A lock whose holder panicked is still the data: the helper's life goes on.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
pub(crate) mod tests;
