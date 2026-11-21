//! The browsers of a page: one of the engine's for each room the page names, opened and sized as
//! the page's outline says, told what the page asks of it, and the page told what the engine says.
//!
//! The engine is the helper's ([`super::helper`]) and speaks from its own threads; everything of
//! the page is the main thread's. The two meet in a queue: what the engine says is put in it and
//! the shell is woken ([`Shared`]), and the shell drains it on the main thread ([`Rooms::drain`]),
//! where each thing is turned into what the page is told and what the shell does ([`Shell`]).
//! Every question the engine asks (a menu, a dialog, a file, a download) holds an [`Answer`] that
//! says "no" when it is dropped unanswered, so that a question is answered once whatever becomes
//! of the person's side of it.
//!
//! Nothing here draws or touches AppKit: the frames go to the shell as they arrive ([`Frames`]),
//! and the rest is the protocol's, so it runs in the gate against a helper that is a thread.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use browser_ipc::message::{
    AgentOutput, Command, Cursor, DialogKind, DownloadState, EditAction, Event, FileMode, KeyData,
    KeyKind, MenuItem, modifier,
};
use browser_ipc::{Frame, Surface};
use pencere_view::{
    BrowserAsk, BrowserError, BrowserFound, BrowserOptions, BrowserState, BrowserWindows, Id, Key,
    Mods, Rect, Shortcut,
};

use super::helper::{Browser, Handle, Manager, Sink, StartError};
use super::profile::ProfileKey;
use super::route::Dialer;

/// The most asks a room holds for its engine while the engine is being started.
const HELD_ASKS: usize = 64;

/// Where the engine's frames go as they come, on the helper's thread.
pub trait Frames: Send + Sync {
    /// A frame of the room's page (`popup`: of the list that goes over it).
    fn frame(&self, room: Id, frame: Frame, surface: Surface, popup: bool);
    /// The popup shows at `x`, `y` (device pixels of the view) or is gone.
    fn popup(&self, room: Id, shown: Option<(i32, i32)>);
    /// The room is gone: its pictures go.
    fn gone(&self, room: Id);
}

/// What the shell does with what the engine says, on the main thread.
pub trait Shell {
    /// The page is told what the browser is and does ([`pencere_drive::Page::browser`]).
    fn state(&mut self, room: Id, state: BrowserState);
    /// A link opens a window and the page said it decides ([`BrowserWindows::Page`]).
    fn window(&mut self, room: Id, url: &str, background: bool);
    /// A key the browser and its page had no use for: the page's. `down` is false for the
    /// release of a key whose press was the page's.
    fn unused_key(&mut self, room: Id, key: Key, mods: Mods, down: bool);
    /// The pointer's shape over the room.
    fn cursor(&mut self, room: Id, cursor: Cursor);
    /// A context menu is asked for.
    fn menu(&mut self, room: Id, menu: Menu);
    /// A dialog of the web page (`alert`, `confirm`, `prompt`, leaving the page) is asked for.
    fn dialog(&mut self, room: Id, dialog: Dialog);
    /// The page moved on while a dialog was up: the dialog numbered `req` ([`Dialog::req`]) is
    /// void, and the shell takes it down (it may be a sheet that is open, one parked behind another
    /// or one still in the line) without answering it ([`Dialog::void`]).
    fn dialog_void(&mut self, room: Id, req: u32);
    /// Files are asked for.
    fn files(&mut self, room: Id, files: Files);
    /// A download is offered.
    fn download(&mut self, room: Id, download: Download);
    /// A download that was accepted has come to its end: `path` is a complete file, or none of it
    /// is there (`done` false).
    fn downloaded(&mut self, room: Id, path: PathBuf, done: bool);
    /// The engine is not on this Mac: the shell asks the person, and [`Rooms::retry`]s.
    fn not_installed(&mut self, room: Id);
    /// The way to the program's machine is wanted: the shell finds out where its way ends
    /// ([`super::route::prove`]) and says so with [`Rooms::route`], once, whatever comes of it.
    fn route(&mut self);
    /// An agent's job in the room is done.
    fn agent(&mut self, _room: Id, _req: u32, _output: AgentOutput) {}
    /// The answer to the question numbered `req` of `Command::CanScroll` (asked with
    /// [`Rooms::send`]): `can` is that the page, or the box under the point, scrolls that way.
    fn can_scroll(&mut self, _room: Id, _req: u32, _can: bool) {}
    /// Something the page's writer may read where a program's diagnostics are read.
    fn diagnostic(&mut self, _room: Id, _words: String) {}
}

/// What a [`Shell`] is told, kept in the order it came: the shell does each thing when nothing
/// of the rooms is borrowed (a menu runs the main queue while it is up, and what is done with a
/// question may come back to the rooms).
pub enum Effect {
    State(Id, BrowserState),
    Window(Id, String, bool),
    UnusedKey(Id, Key, Mods, bool),
    Cursor(Id, Cursor),
    Menu(Id, Menu),
    Dialog(Id, Dialog),
    /// The dialog of this number is void ([`Shell::dialog_void`]).
    DialogVoid(Id, u32),
    Files(Id, Files),
    Download(Id, Download),
    Downloaded(Id, PathBuf, bool),
    NotInstalled(Id),
    Route,
    Agent(Id, u32, AgentOutput),
    /// The answer to a `Command::CanScroll` ([`Shell::can_scroll`]).
    CanScroll(Id, u32, bool),
    Diagnostic(Id, String),
}

impl Shell for Vec<Effect> {
    fn state(&mut self, room: Id, state: BrowserState) {
        self.push(Effect::State(room, state));
    }
    fn window(&mut self, room: Id, url: &str, background: bool) {
        self.push(Effect::Window(room, url.to_owned(), background));
    }
    fn unused_key(&mut self, room: Id, key: Key, mods: Mods, down: bool) {
        self.push(Effect::UnusedKey(room, key, mods, down));
    }
    fn cursor(&mut self, room: Id, cursor: Cursor) {
        self.push(Effect::Cursor(room, cursor));
    }
    fn menu(&mut self, room: Id, menu: Menu) {
        self.push(Effect::Menu(room, menu));
    }
    fn dialog(&mut self, room: Id, dialog: Dialog) {
        self.push(Effect::Dialog(room, dialog));
    }
    fn dialog_void(&mut self, room: Id, req: u32) {
        self.push(Effect::DialogVoid(room, req));
    }
    fn files(&mut self, room: Id, files: Files) {
        self.push(Effect::Files(room, files));
    }
    fn download(&mut self, room: Id, download: Download) {
        self.push(Effect::Download(room, download));
    }
    fn downloaded(&mut self, room: Id, path: PathBuf, done: bool) {
        self.push(Effect::Downloaded(room, path, done));
    }
    fn not_installed(&mut self, room: Id) {
        self.push(Effect::NotInstalled(room));
    }
    fn route(&mut self) {
        self.push(Effect::Route);
    }
    fn agent(&mut self, room: Id, req: u32, output: AgentOutput) {
        self.push(Effect::Agent(room, req, output));
    }
    fn can_scroll(&mut self, room: Id, req: u32, can: bool) {
        self.push(Effect::CanScroll(room, req, can));
    }
    fn diagnostic(&mut self, room: Id, words: String) {
        self.push(Effect::Diagnostic(room, words));
    }
}

/// A question of the engine, answered once: with [`Answer::send`], or with the answer that
/// means "no" when it is dropped.
pub struct Answer {
    handle: Handle,
    otherwise: Option<Command>,
}

impl Answer {
    fn new(handle: Handle, otherwise: Command) -> Self {
        Self {
            handle,
            otherwise: Some(otherwise),
        }
    }

    /// The answer. A browser that has gone, or a helper that ended, has nobody to hear it.
    pub fn send(mut self, command: Command) {
        self.otherwise = None;
        let _ = self.handle.send(command);
    }
}

impl Drop for Answer {
    fn drop(&mut self) {
        if let Some(command) = self.otherwise.take() {
            let _ = self.handle.send(command);
        }
    }
}

/// A context menu: where it was asked for (points from the room's corner), its lines, and what
/// the page under the pointer is (a link, an image, selected text, an editor).
pub struct Menu {
    pub x: i32,
    pub y: i32,
    pub items: Vec<MenuItem>,
    pub link: Option<String>,
    pub source: Option<String>,
    pub selection: Option<String>,
    pub editable: bool,
    answer: Answer,
}

impl Menu {
    /// The line the person picked (its `command`), or none.
    pub fn pick(self, command: Option<i32>) {
        let req = self.answer.req();
        self.answer.send(Command::MenuResult { req, command });
    }
}

/// A dialog of the web page.
pub struct Dialog {
    pub kind: DialogKind,
    pub origin: String,
    pub message: String,
    pub default_text: String,
    req: u32,
    answer: Answer,
}

impl Dialog {
    /// The dialog's number, which [`Shell::dialog_void`] names it by.
    pub fn req(&self) -> u32 {
        self.req
    }

    /// The dialog is void (the page moved on): taken down with no answer. The engine has no one
    /// waiting for one and would ignore it.
    pub fn void(mut self) {
        self.answer.otherwise = None;
    }

    /// The person's answer: `ok` for OK (or Leave), `text` for a prompt's.
    pub fn answer(self, ok: bool, text: String) {
        let req = self.answer.req();
        self.answer.send(Command::DialogResult { req, ok, text });
    }
}

/// Files the web page asks for.
pub struct Files {
    pub mode: FileMode,
    pub title: String,
    pub default_path: String,
    pub accept: Vec<String>,
    answer: Answer,
}

impl Files {
    /// The paths the person chose; none is a refusal.
    pub fn choose(self, paths: Vec<PathBuf>) {
        let req = self.answer.req();
        let paths = paths
            .into_iter()
            .filter_map(|path| path.into_os_string().into_string().ok())
            .collect();
        self.answer.send(Command::FileResult { req, paths });
    }
}

/// A download the engine offers: nothing is written until the person says where.
pub struct Download {
    pub url: String,
    /// The name the web page gave it, as it gave it ([`super::downloads::safe_name`] makes it
    /// one that is safe to write).
    pub name: String,
    pub mime: String,
    pub size: Option<u64>,
    req: u32,
    answer: Answer,
    saved: Arc<Mutex<HashMap<u32, PathBuf>>>,
}

impl Download {
    /// Save it at `path`: the engine writes there, and the shell hears of its end.
    pub fn save(self, path: PathBuf) {
        let Some(text) = path.to_str().map(str::to_owned) else {
            return;
        };
        lock(&self.saved).insert(self.req, path);
        let req = self.req;
        self.answer.send(Command::DownloadResult {
            req,
            path: Some(text),
        });
    }
}

impl Answer {
    /// The request number of the command that says "no" (every question has one).
    fn req(&self) -> u32 {
        match self.otherwise.as_ref() {
            Some(
                Command::MenuResult { req, .. }
                | Command::DialogResult { req, .. }
                | Command::FileResult { req, .. }
                | Command::DownloadResult { req, .. },
            ) => *req,
            _ => 0,
        }
    }
}

/// What a browser's thread puts in the queue.
enum Told {
    Opened {
        room: Id,
        generation: u64,
        result: Result<Browser, StartError>,
    },
    Event {
        room: Id,
        generation: u64,
        event: Event,
    },
    Lost {
        room: Id,
        generation: u64,
        why: String,
    },
}

/// A browser of the page, as an agent's `ls` has it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seen {
    pub id: Id,
    /// What the browser is ([`BrowserState::status`]).
    pub status: String,
    /// The address it is at, empty before the first load.
    pub url: String,
    pub title: String,
}

/// What the helper's threads and the main thread share.
struct Shared {
    told: Mutex<VecDeque<Told>>,
    /// Called after something is put in the queue, from any thread: the shell hops to the main
    /// thread and drains.
    wake: Arc<dyn Fn() + Send + Sync>,
    frames: Arc<dyn Frames>,
    /// Where downloads that were accepted are going, by request.
    saved: Arc<Mutex<HashMap<u32, PathBuf>>>,
}

impl Shared {
    fn put(&self, told: Told) {
        lock(&self.told).push_back(told);
        (self.wake)();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One room's end of the engine's conversation.
struct RoomSink {
    room: Id,
    generation: u64,
    shared: Arc<Shared>,
}

impl Sink for RoomSink {
    fn event(&self, event: Event) {
        // A popup's place goes with the frames, which are not the main thread's.
        if let Event::Popup { show, x, y, .. } = &event {
            self.shared
                .frames
                .popup(self.room, show.then_some((*x, *y)));
        }
        self.shared.put(Told::Event {
            room: self.room,
            generation: self.generation,
            event,
        });
    }

    fn frame(&self, frame: Frame, surface: Surface) {
        let popup = frame.kind == browser_ipc::KIND_POPUP;
        self.shared.frames.frame(self.room, frame, surface, popup);
    }

    fn lost(&self, why: String) {
        self.shared.put(Told::Lost {
            room: self.room,
            generation: self.generation,
            why,
        });
    }
}

/// What the helper is told of [`BrowserWindows`]: it words the rows of its menu that open a link or an
/// image by it ("Open Link in New Tab", or "Open Link" where the browser goes there itself).
fn new_windows_of(windows: BrowserWindows) -> browser_ipc::message::NewWindows {
    match windows {
        BrowserWindows::Here => browser_ipc::message::NewWindows::Here,
        BrowserWindows::Page => browser_ipc::message::NewWindows::Page,
    }
}

enum Life {
    /// The way to the program's machine is being found; the engine starts when it is.
    Routing,
    /// The page's machine has no way here ([`Route::Refused`]): a reload looks again.
    NoRoute,
    /// The engine is being started (it takes a while the first time).
    Opening,
    /// The engine is not on this Mac; the shell is asking.
    NotInstalled,
    Open(Browser),
    /// The engine ended: a reload starts it again.
    Over,
}

struct Room {
    generation: u64,
    options: BrowserOptions,
    /// The room's size, points: what the engine draws at.
    size: (u32, u32),
    scale: f32,
    life: Life,
    /// What the page asked before the engine was open.
    asks: Vec<BrowserAsk>,
    /// Whether the person has the keyboard in it.
    focus: bool,
    /// The search the page last asked for, for its next and previous.
    find: Option<(String, bool)>,
    /// The keys whose press went to the page, which their release follows.
    pressed: HashSet<i32>,
    /// The last of what the page was told, which a change of part of it is made on.
    state: BrowserState,
    /// Whether the engine has said it is open.
    opened: bool,
}

/// Where the way of a page's browsers to its program's machine stands. One for the page: the
/// program is one, and so is the machine it runs on.
enum Route {
    /// Nobody has asked yet.
    Unknown,
    /// The shell was asked and has not said.
    Asked,
    /// Shown to end at the program's machine: the machine's name (what its profiles are kept under)
    /// and the way is in the manager's routes.
    Proven(String),
    /// Not shown to: why, as a line for the person.
    Refused(String),
}

/// The browsers of one page.
pub struct Rooms {
    manager: Arc<Manager>,
    route: Route,
    application: String,
    shared: Arc<Shared>,
    rooms: HashMap<Id, Room>,
    generation: u64,
}

impl Rooms {
    /// `application` is what the profiles of this page's browsers are kept under, with the
    /// machine the way shows ([`ProfileKey`]); `wake` is called from the engine's threads after
    /// they put something to be drained; `frames` is where the engine's pictures go.
    pub fn new(
        manager: Arc<Manager>,
        application: &str,
        wake: Arc<dyn Fn() + Send + Sync>,
        frames: Arc<dyn Frames>,
    ) -> Self {
        Self {
            manager,
            route: Route::Unknown,
            application: application.to_owned(),
            shared: Arc::new(Shared {
                told: Mutex::new(VecDeque::new()),
                wake,
                frames,
                saved: Arc::new(Mutex::new(HashMap::new())),
            }),
            rooms: HashMap::new(),
            generation: 0,
        }
    }

    /// Makes the browsers the outline names, and no others: a room that is new gets one (and the
    /// page is told it is starting), one that is gone loses its, another profile under the same
    /// name is another browser, and a room of another size or on another display is told so.
    pub fn apply(
        &mut self,
        browsers: &[pencere_drive::Browser],
        scale: f32,
        shell: &mut dyn Shell,
    ) {
        let gone: Vec<Id> = self
            .rooms
            .keys()
            .filter(|id| !browsers.iter().any(|b| b.id == **id))
            .copied()
            .collect();
        for id in gone {
            self.rooms.remove(&id);
            self.shared.frames.gone(id);
        }
        for browser in browsers {
            let size = size_of(browser.rect);
            match self.rooms.get_mut(&browser.id) {
                Some(room) if room.options.profile == browser.options.profile => {
                    room.options.keys.clone_from(&browser.options.keys);
                    let windows = new_windows_of(browser.options.windows);
                    let changed = windows != new_windows_of(room.options.windows);
                    room.options.windows = browser.options.windows;
                    if changed && let Life::Open(engine) = &room.life {
                        let _ = engine.send(Command::Windows {
                            browser: engine.id(),
                            mode: windows,
                        });
                    }
                    if (room.size, room.scale) != (size, scale) {
                        room.size = size;
                        room.scale = scale;
                        if let Life::Open(engine) = &room.life {
                            let _ = engine.send(Command::Resize {
                                browser: engine.id(),
                                width: size.0,
                                height: size.1,
                                scale,
                            });
                        }
                    }
                }
                _ => {
                    // Another profile under the name is another browser: the old one's pictures go.
                    if self.rooms.remove(&browser.id).is_some() {
                        self.shared.frames.gone(browser.id);
                    }
                    self.make(browser.id, browser.options.clone(), size, scale, shell);
                }
            }
        }
    }

    fn make(
        &mut self,
        id: Id,
        options: BrowserOptions,
        size: (u32, u32),
        scale: f32,
        shell: &mut dyn Shell,
    ) {
        self.generation += 1;
        let generation = self.generation;
        // An engine starts only once the way to the program's machine is shown: the machine is
        // part of what its profile is kept under, and its first request goes by that way.
        let (life, state) = match &self.route {
            Route::Proven(_) => (
                Life::Opening,
                BrowserState::with_status(BrowserState::STARTING, Some(STARTING)),
            ),
            Route::Refused(why) => (
                Life::NoRoute,
                BrowserState::with_status(BrowserState::NO_ROUTE, Some(why)),
            ),
            Route::Unknown | Route::Asked => (
                Life::Routing,
                BrowserState::with_status(BrowserState::STARTING, Some(FINDING)),
            ),
        };
        shell.state(id, state.clone());
        self.rooms.insert(
            id,
            Room {
                generation,
                options: options.clone(),
                size,
                scale,
                life,
                asks: Vec::new(),
                focus: false,
                find: None,
                pressed: HashSet::new(),
                state,
                opened: false,
            },
        );
        match self.route {
            Route::Proven(_) => {
                self.start(id, generation, options.profile.as_deref(), size, scale);
            }
            Route::Unknown => {
                self.route = Route::Asked;
                shell.route();
            }
            Route::Asked | Route::Refused(_) => {}
        }
    }

    /// The shell found where the way to the program's machine ends, or that it ends nowhere it
    /// could show: `Ok` is the machine's name and the way to it (it is set for the proxy here), the
    /// `Err` a line for the person. The rooms that waited start, or say there is no route.
    pub fn route(
        &mut self,
        proof: Result<(String, Arc<dyn Dialer>), String>,
        shell: &mut dyn Shell,
    ) {
        match proof {
            Ok((machine, way)) => {
                self.manager.routes().set(&machine, way);
                self.route = Route::Proven(machine);
                let waiting: Vec<Id> = self
                    .rooms
                    .iter()
                    .filter(|(_, room)| matches!(room.life, Life::Routing | Life::NoRoute))
                    .map(|(id, _)| *id)
                    .collect();
                for id in waiting {
                    self.retry(id, shell);
                }
            }
            Err(why) => {
                self.route = Route::Refused(why.clone());
                for (id, room) in &mut self.rooms {
                    if matches!(room.life, Life::Routing) {
                        room.life = Life::NoRoute;
                        room.state = BrowserState::with_status(BrowserState::NO_ROUTE, Some(&why));
                        shell.state(*id, room.state.clone());
                    }
                }
            }
        }
    }

    /// Opens the room's engine on a thread of its own: the first open starts the helper and waits
    /// for it.
    fn start(&self, id: Id, generation: u64, profile: Option<&str>, size: (u32, u32), scale: f32) {
        let manager = self.manager.clone();
        let shared = self.shared.clone();
        let Route::Proven(machine) = &self.route else {
            return;
        };
        let key = ProfileKey::new(machine, &self.application, profile);
        let _ = std::thread::Builder::new()
            .name("browser-open".into())
            .spawn(move || {
                let result = match key {
                    Ok(key) => {
                        let sink = Arc::new(RoomSink {
                            room: id,
                            generation,
                            shared: shared.clone(),
                        });
                        manager.open(&key, size, scale, sink)
                    }
                    Err(e) => Err(StartError::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        e.to_string(),
                    ))),
                };
                shared.put(Told::Opened {
                    room: id,
                    generation,
                    result,
                });
            });
    }

    /// The engine is installed now (or the person asked again): the room's start is tried again.
    pub fn retry(&mut self, id: Id, shell: &mut dyn Shell) {
        let Some(room) = self.rooms.get_mut(&id) else {
            return;
        };
        if matches!(room.life, Life::Open(_) | Life::Opening) {
            return;
        }
        // No route is looked for again: the program's machine may have changed since.
        if matches!(room.life, Life::NoRoute) && !matches!(self.route, Route::Proven(_)) {
            room.life = Life::Routing;
            room.state = BrowserState::with_status(BrowserState::STARTING, Some(FINDING));
            shell.state(id, room.state.clone());
            if !matches!(self.route, Route::Asked) {
                self.route = Route::Asked;
                shell.route();
            }
            return;
        }
        if matches!(room.life, Life::Routing) && !matches!(self.route, Route::Proven(_)) {
            return;
        }
        self.generation += 1;
        room.generation = self.generation;
        room.life = Life::Opening;
        room.opened = false;
        room.state = BrowserState::with_status(BrowserState::STARTING, Some(STARTING));
        shell.state(id, room.state.clone());
        let (generation, profile, size, scale) = (
            room.generation,
            room.options.profile.clone(),
            room.size,
            room.scale,
        );
        self.start(id, generation, profile.as_deref(), size, scale);
    }

    /// What the page asks of the browser of `id` ([`pencere_drive::Host::browser`]).
    pub fn ask(&mut self, id: Id, ask: BrowserAsk, shell: &mut dyn Shell) {
        let Some(room) = self.rooms.get_mut(&id) else {
            return;
        };
        match &room.life {
            Life::Open(engine) => {
                let command = command_of(engine.id(), &ask, &mut room.find);
                if let Some(command) = command {
                    let _ = engine.send(command);
                }
            }
            // The way out of an engine that ended is to ask it to load again.
            Life::Over if matches!(ask, BrowserAsk::Reload { .. } | BrowserAsk::Go(_)) => {
                if room.asks.len() < HELD_ASKS {
                    room.asks.push(ask);
                }
                self.retry(id, shell);
            }
            Life::Over => {}
            // The person is asked again: a reload is how they say "now" to an engine they turned
            // away.
            Life::NotInstalled if matches!(ask, BrowserAsk::Reload { .. }) => {
                shell.not_installed(id);
            }
            // The person says "again" to a way that was not shown: it is looked for again.
            Life::NoRoute if matches!(ask, BrowserAsk::Reload { .. } | BrowserAsk::Go(_)) => {
                if room.asks.len() < HELD_ASKS {
                    room.asks.push(ask);
                }
                if matches!(self.route, Route::Refused(_)) {
                    self.route = Route::Unknown;
                }
                self.retry(id, shell);
            }
            Life::NoRoute => {}
            Life::Opening | Life::NotInstalled | Life::Routing => {
                if room.asks.len() < HELD_ASKS {
                    room.asks.push(ask);
                }
            }
        }
    }

    /// The person has the keyboard in the room, or has not.
    pub fn focus(&mut self, id: Id, on: bool) {
        if let Some(room) = self.rooms.get_mut(&id) {
            room.focus = on;
            if let Life::Open(engine) = &room.life {
                let _ = engine.send(Command::Focus {
                    browser: engine.id(),
                    on,
                });
            }
        }
    }

    /// Whether there is a page to go back to and one to go forward to in the room's browser (what
    /// its last state said; neither where it has not said).
    pub fn history(&self, id: Id) -> (bool, bool) {
        self.rooms
            .get(&id)
            .map_or((false, false), |room| (room.state.back, room.state.forward))
    }

    /// Whether the room's engine has the keyboard (the person gave it, and it is open).
    pub fn has_focus(&self, id: Id) -> bool {
        self.rooms.get(&id).is_some_and(|room| room.focus)
    }

    /// Sends a command of the browser of `id` made from its number; `false` where it is not
    /// open (the person's input to an engine that is not there is let go).
    pub fn send(&self, id: Id, command: impl FnOnce(u32) -> Command) -> bool {
        match self.rooms.get(&id).map(|room| &room.life) {
            Some(Life::Open(engine)) => engine.send(command(engine.id())).is_ok(),
            _ => false,
        }
    }

    /// Whether `key` with `mods` is one the page asked to have itself (the room's
    /// [`BrowserOptions::keys`]) and not the browser.
    pub fn page_key(&self, id: Id, key: Key, mods: Mods) -> bool {
        self.rooms.get(&id).is_some_and(|room| {
            room.options
                .keys
                .iter()
                .any(|shortcut| shortcut_is(shortcut, key, mods))
        })
    }

    /// The rooms that hold a browser (for the shell's walk of them).
    pub fn ids(&self) -> impl Iterator<Item = Id> + '_ {
        self.rooms.keys().copied()
    }

    /// What `bateri browser ls` tells of the page's browsers, by id.
    pub fn seen(&self) -> Vec<Seen> {
        let mut seen: Vec<Seen> = self
            .rooms
            .iter()
            .map(|(id, room)| Seen {
                id: *id,
                status: if room.state.status.is_empty() {
                    BrowserState::READY.to_owned()
                } else {
                    room.state.status.clone()
                },
                url: room.state.url.clone(),
                title: room.state.title.clone(),
            })
            .collect();
        seen.sort_by_key(|browser| browser.id);
        seen
    }

    /// The handle on the browser of `id`, for what answers for it (the agent's jobs).
    pub fn handle(&self, id: Id) -> Option<Handle> {
        match self.rooms.get(&id).map(|room| &room.life) {
            Some(Life::Open(engine)) => Some(engine.handle()),
            _ => None,
        }
    }

    /// What the engines said since the last time, turned into what the page is told and what the
    /// shell does.
    pub fn drain(&mut self, shell: &mut dyn Shell) {
        loop {
            let Some(told) = lock(&self.shared.told).pop_front() else {
                return;
            };
            match told {
                Told::Opened {
                    room,
                    generation,
                    result,
                } => self.opened(room, generation, result, shell),
                Told::Event {
                    room,
                    generation,
                    event,
                } => {
                    if self.current(room, generation) {
                        self.event(room, event, shell);
                    }
                }
                Told::Lost {
                    room,
                    generation,
                    why,
                } => {
                    if self.current(room, generation) {
                        self.over(room, &why, shell);
                    }
                }
            }
        }
    }

    fn current(&self, id: Id, generation: u64) -> bool {
        self.rooms
            .get(&id)
            .is_some_and(|room| room.generation == generation)
    }

    fn opened(
        &mut self,
        id: Id,
        generation: u64,
        result: Result<Browser, StartError>,
        shell: &mut dyn Shell,
    ) {
        // A room that went, or began again, while its engine was being opened has no use for it:
        // dropping it closes it.
        if !self.current(id, generation) {
            return;
        }
        let Some(room) = self.rooms.get_mut(&id) else {
            return;
        };
        match result {
            Ok(engine) => {
                // Where a link the menu opens goes decides the words of its row.
                let _ = engine.send(Command::Windows {
                    browser: engine.id(),
                    mode: new_windows_of(room.options.windows),
                });
                // The room may have been sized and given the keyboard while the engine started.
                let _ = engine.send(Command::Resize {
                    browser: engine.id(),
                    width: room.size.0,
                    height: room.size.1,
                    scale: room.scale,
                });
                if room.focus {
                    let _ = engine.send(Command::Focus {
                        browser: engine.id(),
                        on: true,
                    });
                }
                for ask in std::mem::take(&mut room.asks) {
                    if let Some(command) = command_of(engine.id(), &ask, &mut room.find) {
                        let _ = engine.send(command);
                    }
                }
                room.life = Life::Open(engine);
            }
            Err(StartError::NotInstalled) => {
                room.life = Life::NotInstalled;
                shell.not_installed(id);
            }
            Err(e) => {
                let why = e.to_string();
                self.over(id, &why, shell);
            }
        }
    }

    /// The engine ended (or could not start): the page is told, and a reload makes it again.
    fn over(&mut self, id: Id, why: &str, shell: &mut dyn Shell) {
        let Some(room) = self.rooms.get_mut(&id) else {
            return;
        };
        room.life = Life::Over;
        room.pressed.clear();
        room.state = BrowserState::with_status(
            BrowserState::CRASHED,
            Some(&format!("The browser engine stopped: {why}")),
        );
        shell.state(id, room.state.clone());
    }

    fn event(&mut self, id: Id, event: Event, shell: &mut dyn Shell) {
        let Some(room) = self.rooms.get_mut(&id) else {
            return;
        };
        let handle = match &room.life {
            Life::Open(engine) => engine.handle(),
            _ => return,
        };
        match event {
            Event::Opened { .. } => {
                room.opened = true;
                room.state = BrowserState::default();
                shell.state(id, room.state.clone());
            }
            Event::State {
                url,
                title,
                loading,
                can_back,
                can_forward,
                zoom,
                error,
                found,
                ..
            } => {
                room.opened = true;
                room.state = BrowserState {
                    url,
                    title,
                    loading,
                    back: can_back,
                    forward: can_forward,
                    status: BrowserState::READY.to_owned(),
                    message: None,
                    error: error.map(|e| BrowserError {
                        url: e.url,
                        code: e.code,
                        message: e.message,
                    }),
                    found: found.map(|f| BrowserFound {
                        count: f.count,
                        current: (f.current > 0).then_some(f.current),
                    }),
                    // A zoom is a small number (a quarter to five): f32 loses nothing a person sees.
                    #[allow(clippy::cast_possible_truncation)]
                    zoom: zoom as f32,
                };
                shell.state(id, room.state.clone());
            }
            Event::Cursor { cursor, .. } => shell.cursor(id, cursor),
            Event::Crashed { reason, .. } => {
                room.state = BrowserState::with_status(
                    BrowserState::CRASHED,
                    Some(&format!("The page's process stopped: {reason}")),
                );
                shell.state(id, room.state.clone());
            }
            Event::Refused { url, reason, .. } => {
                let mut state = room.state.clone();
                state.error = Some(BrowserError {
                    url,
                    code: 0,
                    message: if reason == "address" {
                        "This address cannot be opened here.".to_owned()
                    } else {
                        "This kind of address is not opened here.".to_owned()
                    },
                });
                shell.state(id, state);
            }
            Event::Window {
                url, background, ..
            } => match room.options.windows {
                BrowserWindows::Here => {
                    let _ = handle.send(Command::Navigate {
                        browser: handle.id(),
                        url,
                    });
                }
                BrowserWindows::Page => shell.window(id, &url, background),
            },
            Event::UnhandledKey { kind, key, .. } => {
                // The Edit menu's own keys go to the engine as keys, the page's first (a page
                // with its own undo takes ⌘Z and the key is not given back); one that comes back
                // was nobody's, and is the menu's command, which the engine runs on what has the
                // focus. The page is not given these keys.
                if let Some(action) = edit_of_key(&key) {
                    if kind == KeyKind::Down {
                        let _ = handle.send(Command::Edit {
                            browser: handle.id(),
                            action,
                        });
                    }
                    return;
                }
                let Some(page_key) = key_of(&key) else {
                    return;
                };
                let mods = mods_of(key.mods);
                match kind {
                    KeyKind::Down | KeyKind::Char => {
                        if kind == KeyKind::Down {
                            room.pressed.insert(key.vk);
                            shell.unused_key(id, page_key, mods, true);
                        }
                    }
                    // Only the release of a press the page was given is the page's.
                    KeyKind::Up => {
                        if room.pressed.remove(&key.vk) {
                            shell.unused_key(id, page_key, mods, false);
                        }
                    }
                }
            }
            Event::Menu {
                req,
                x,
                y,
                items,
                link,
                source,
                selection,
                editable,
                ..
            } => shell.menu(
                id,
                Menu {
                    x,
                    y,
                    items,
                    link,
                    source,
                    selection,
                    editable,
                    answer: Answer::new(handle, Command::MenuResult { req, command: None }),
                },
            ),
            Event::Dialog {
                req,
                kind,
                origin,
                message,
                default_text,
                ..
            } => shell.dialog(
                id,
                Dialog {
                    kind,
                    origin,
                    message,
                    default_text,
                    req,
                    answer: Answer::new(
                        handle,
                        Command::DialogResult {
                            req,
                            ok: false,
                            text: String::new(),
                        },
                    ),
                },
            ),
            Event::DialogVoid { req, .. } => shell.dialog_void(id, req),
            Event::FileDialog {
                req,
                mode,
                title,
                default_path,
                accept,
                ..
            } => shell.files(
                id,
                Files {
                    mode,
                    title,
                    default_path,
                    accept,
                    answer: Answer::new(
                        handle,
                        Command::FileResult {
                            req,
                            paths: Vec::new(),
                        },
                    ),
                },
            ),
            Event::Download {
                req,
                url,
                name,
                mime,
                size,
                ..
            } => shell.download(
                id,
                Download {
                    url,
                    name,
                    mime,
                    size,
                    req,
                    answer: Answer::new(handle, Command::DownloadResult { req, path: None }),
                    saved: self.shared.saved.clone(),
                },
            ),
            Event::DownloadProgress { req, state, .. } => {
                if state != DownloadState::Running
                    && let Some(path) = lock(&self.shared.saved).remove(&req)
                {
                    shell.downloaded(id, path, state == DownloadState::Done);
                }
            }
            Event::AgentResult { req, output, .. } => shell.agent(id, req, output),
            Event::CanScroll { req, can, .. } => shell.can_scroll(id, req, can),
            Event::Error { message, .. } => shell.diagnostic(id, message),
            Event::Hello { .. } | Event::Closed { .. } | Event::Popup { .. } => {}
        }
    }
}

/// What the page is told while the engine is being got ready.
pub const STARTING: &str = "Starting the browser…";

/// What the page says of a browser while the way to the program's machine is looked for.
pub const FINDING: &str = "Finding the way to the program's machine…";

/// The size the engine draws a room at, points: the room's, a point at least.
fn size_of(rect: Rect) -> (u32, u32) {
    // `as` from a float saturates: a room is a point at least and never wraps.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let side = |length: f32| length.round().max(1.0) as u32;
    (side(rect.w), side(rect.h))
}

/// What a page's ask is to the engine of browser `browser`. `find` is the search the page last
/// made (its pattern and whether the case counts), which the next and the previous go on with.
/// None for an ask the engine has nothing to do for.
pub fn command_of(
    browser: u32,
    ask: &BrowserAsk,
    find: &mut Option<(String, bool)>,
) -> Option<Command> {
    Some(match ask {
        BrowserAsk::Go(url) => Command::Navigate {
            browser,
            url: url.clone(),
        },
        BrowserAsk::Back => Command::Back { browser },
        BrowserAsk::Forward => Command::Forward { browser },
        BrowserAsk::Reload { cache } => Command::Reload {
            browser,
            ignore_cache: !cache,
        },
        BrowserAsk::Stop => Command::Stop { browser },
        BrowserAsk::Find { pattern, case } => {
            if pattern.is_empty() {
                *find = None;
                return Some(Command::StopFinding {
                    browser,
                    keep_selection: false,
                });
            }
            *find = Some((pattern.clone(), *case));
            Command::Find {
                browser,
                pattern: pattern.clone(),
                case: *case,
                forward: true,
                next: false,
            }
        }
        BrowserAsk::FindNext | BrowserAsk::FindPrevious => {
            let (pattern, case) = find.clone()?;
            Command::Find {
                browser,
                pattern,
                case,
                forward: matches!(ask, BrowserAsk::FindNext),
                next: true,
            }
        }
        BrowserAsk::StopFinding => {
            *find = None;
            Command::StopFinding {
                browser,
                keep_selection: false,
            }
        }
        BrowserAsk::Zoom(ratio) => Command::Zoom {
            browser,
            ratio: f64::from(*ratio),
        },
        BrowserAsk::Devtools => Command::DevTools { browser },
        BrowserAsk::Unknown => return None,
    })
}

/// The modifiers the engine reports, as the page reads them.
pub fn mods_of(mods: u32) -> Mods {
    Mods {
        shift: mods & modifier::SHIFT != 0,
        control: mods & modifier::CONTROL != 0,
        alt: mods & modifier::ALT != 0,
        command: mods & modifier::COMMAND != 0,
    }
}

/// The command of the Edit menu that a key the engine gave back stands for: ⌘C, ⌘X, ⌘V, ⌘A, ⌘Z
/// and ⇧⌘Z, with no other modifier (⌥⌘V is nobody's here).
pub fn edit_of_key(key: &KeyData) -> Option<EditAction> {
    let mods = mods_of(key.mods);
    if !mods.command || mods.control || mods.alt {
        return None;
    }
    match (key.vk, mods.shift) {
        (65, false) => Some(EditAction::SelectAll),
        (67, false) => Some(EditAction::Copy),
        (86, false) => Some(EditAction::Paste),
        (88, false) => Some(EditAction::Cut),
        (90, false) => Some(EditAction::Undo),
        (90, true) => Some(EditAction::Redo),
        _ => None,
    }
}

/// The key a key the engine gave back is for the page: the keys the page reads by their
/// virtual-key number, and the rest by the character they make without the modifiers (⌘K is `k`).
pub fn key_of(key: &KeyData) -> Option<Key> {
    Some(match key.vk {
        8 => Key::Backspace,
        9 => Key::Tab,
        13 => Key::Enter,
        27 => Key::Escape,
        33 => Key::PageUp,
        34 => Key::PageDown,
        35 => Key::End,
        36 => Key::Home,
        37 => Key::Left,
        38 => Key::Up,
        39 => Key::Right,
        40 => Key::Down,
        46 => Key::Delete,
        vk @ 112..=131 => Key::Function(u8::try_from(vk - 111).unwrap_or(1)),
        _ => {
            let character = char::from_u32(u32::from(key.unmodified))
                .filter(|c| !c.is_control() && !('\u{f700}'..='\u{f8ff}').contains(c))
                .or_else(|| {
                    u8::try_from(key.vk)
                        .ok()
                        .filter(u8::is_ascii_alphanumeric)
                        .map(|b| char::from(b.to_ascii_lowercase()))
                })?;
            Key::Character(character.to_lowercase().next().unwrap_or(character))
        }
    })
}

fn shortcut_is(shortcut: &Shortcut, key: Key, mods: Mods) -> bool {
    let same = |a: Key, b: Key| match (a, b) {
        (Key::Character(a), Key::Character(b)) => a.to_lowercase().eq(b.to_lowercase()),
        (a, b) => a == b,
    };
    same(shortcut.key, key) && shortcut.mods == mods
}

#[cfg(test)]
mod tests;
