//! The browsers of a page in a pane: the room a page names for a browser is
//! where the engine's frames are drawn, and what the person does there goes to the engine.
//!
//! The engine runs in the helper ([`bt_shell_common::browser`]); this is the pane's side of it.
//!
//! - **What is drawn.** The helper's frames arrive on its thread, are copied into textures of the
//!   renderer's device the moment they arrive (the helper reuses the surface), and the page's list
//!   puts the last one in the browser's place ([`Pictures`]); a frame that comes wakes the pane
//!   for one frame of the page and nothing else. A pane that is not drawn costs the copy and no
//!   more, and the helper is never kept waiting for it.
//! - **What is done.** The pointer, the wheel and the keys over a room go to its engine (not to
//!   the page: a press on what the page drew over the room reaches the page, as
//!   [`pencere_drive::Page::reaches`] says), text goes as keys and, only while an input method
//!   composes, as its text. A key the engine and its web page had no use for comes back and is
//!   the page's, unless the page asked to have it first (its `keys`).
//! - **What is asked.** A menu, a dialog, a file chooser, a download: one at a time, each answered
//!   once whatever the person does ([`questions`]). Nothing of a download is written before they
//!   say where.
//! - **What is not here.** The first start: an engine that is not on this Mac is asked for
//!   with a sheet and downloaded with the progress told to the page ([`install`]).
//!
//! The rooms and the protocol are `bt_shell_common::browser::rooms`'s; this holds what is
//! AppKit's. Everything of a page is the main thread's: what the helper's threads say is queued
//! and drained here ([`PageBrowsers::drain`]), and what that makes the pane do is done after the
//! queue is let go of, since a menu runs the main queue while it is up.

mod arrow;
mod input;
mod install;
mod jobs;
mod questions;
mod route;
mod swipe;

use route::Probing;

pub(crate) use install::update_in_background;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use browser_ipc::message::{AgentOutput, Command, Cursor, EditAction, KeyData, KeyKind, MouseKind};
use browser_ipc::{Frame, Surface, import, wgpu};
use bt_gpu::{Renderer, UiBitmap, UiImage, UiList, UiQuad, UiRect};
use bt_shell_common::browser::helper::Manager;
use bt_shell_common::browser::rooms::{Effect, Frames, Rooms};
use bt_shell_common::native_view::{
    BrowserAsk, BrowserSlot, BrowserState, Button, GesturePhase, Id, Input, Mods, Outline, Phase,
    Point, Surfaces,
};
use objc2_app_kit::NSEvent;

use crate::pane::TerminalPane;

/// What the profiles of this application's browsers are kept under, with the machine's name.
const APPLICATION: &str = "bateri";

/// The count of a click, as the kit gives it (the engine takes it as it is).
type Clicks = u32;

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The pictures of the page's browsers, as textures of the renderer's device: the last frame of
/// each browser and of its popup. Written from the helper's frame thread, read when the page is
/// drawn.
pub(crate) struct Pictures {
    device: wgpu::Device,
    queue: wgpu::Queue,
    slots: Mutex<HashMap<Id, Shown>>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

#[derive(Default)]
struct Shown {
    view: Option<wgpu::Texture>,
    popup: Option<wgpu::Texture>,
    /// Where the popup goes in the view, device pixels.
    popup_at: Option<(i32, i32)>,
    /// The arrow of a swipe between pages, since when it has been so, and the pixels to the point
    /// it is drawn at.
    arrow: Option<(swipe::Arrow, Instant, f32)>,
}

impl Pictures {
    fn new(renderer: &Renderer, wake: Arc<dyn Fn() + Send + Sync>) -> Arc<Self> {
        Arc::new(Self {
            device: renderer.device().clone(),
            queue: renderer.queue().clone(),
            slots: Mutex::new(HashMap::new()),
            wake,
        })
    }

    /// The arrow of the room's swipe between pages is now `arrow`; `scale` is pixels to the point.
    /// One that moves by itself (it withdraws, or fades after the browser went) is given frames
    /// until it is over: the page draws only when asked to.
    fn set_arrow(&self, room: Id, arrow: swipe::Arrow, scale: f32) {
        {
            let mut slots = lock(&self.slots);
            let shown = slots.entry(room).or_default();
            shown.arrow = (arrow != swipe::Arrow::Hidden).then(|| (arrow, Instant::now(), scale));
        }
        (self.wake)();
        if arrow::moves_alone(arrow) {
            let wake = self.wake.clone();
            // The longest a motion of the arrow takes, and a frame's worth more.
            let frames =
                (arrow::TAKEN.max(arrow::WITHDRAW) + FRAME * 2).as_millis() / FRAME.as_millis();
            let ticker = std::thread::Builder::new()
                .name("swipe-arrow".into())
                .spawn(move || {
                    for _ in 0..frames {
                        std::thread::sleep(FRAME);
                        wake();
                    }
                });
            drop(ticker);
        }
    }
}

/// How often the arrow that moves by itself is given a frame.
const FRAME: Duration = Duration::from_millis(16);

/// The arrow's picture, for the way back and the way forward: made once.
fn glyph(way: swipe::Way) -> &'static [u8] {
    static BOTH: OnceLock<[Vec<u8>; 2]> = OnceLock::new();
    let both = BOTH.get_or_init(|| {
        [
            arrow::glyph_pixels(swipe::Way::Back),
            arrow::glyph_pixels(swipe::Way::Forward),
        ]
    });
    match way {
        swipe::Way::Back => &both[0],
        swipe::Way::Forward => &both[1],
    }
}

/// The renderer's name for the arrow's picture.
fn glyph_id(way: swipe::Way) -> u64 {
    0xA220_0000_0000_0000
        | match way {
            swipe::Way::Back => 1,
            swipe::Way::Forward => 2,
        }
}

impl Frames for Pictures {
    fn frame(&self, room: Id, _frame: Frame, surface: Surface, popup: bool) {
        // Copied now, on the helper's thread: the helper reuses the surface, and a pane that is
        // not drawn must not keep it waiting.
        let Ok(texture) = import::copy(&self.device, &self.queue, &surface) else {
            return;
        };
        {
            let mut slots = lock(&self.slots);
            let shown = slots.entry(room).or_default();
            if popup {
                shown.popup = Some(texture);
            } else {
                shown.view = Some(texture);
            }
        }
        (self.wake)();
    }

    fn popup(&self, room: Id, at: Option<(i32, i32)>) {
        {
            let mut slots = lock(&self.slots);
            let shown = slots.entry(room).or_default();
            shown.popup_at = at;
            if at.is_none() {
                shown.popup = None;
            }
        }
        (self.wake)();
    }

    fn gone(&self, room: Id) {
        lock(&self.slots).remove(&room);
    }
}

/// The renderer's name for a browser's picture: a number no page's figure is likely to make.
fn image_id(room: Id, popup: bool) -> u64 {
    ((room.number() << 1) | u64::from(popup)) ^ 0xB0B0_B0B0_B0B0_B0B0
}

impl Surfaces for Pictures {
    fn paint(
        &self,
        renderer: &Renderer,
        list: &mut UiList,
        slot: &BrowserSlot,
        origin: (f32, f32),
    ) {
        let mut slots = lock(&self.slots);
        let Some(shown) = slots.get_mut(&slot.id) else {
            return;
        };
        let room = UiRect::new(
            slot.rect.x + origin.0,
            slot.rect.y + origin.1,
            slot.rect.w,
            slot.rect.h,
        );
        let visible = UiRect::new(
            slot.clip.x + origin.0,
            slot.clip.y + origin.1,
            slot.clip.w,
            slot.clip.h,
        );
        if let Some(texture) = &shown.view {
            // The frame at its own size, never stretched: while a resize waits for the frame of
            // the new size the old one stays sharp, cut to the room, with the room's fill
            // showing where it is short.
            let size = texture.size();
            // A texture's side is a few thousand pixels at most: f32 holds it exactly.
            #[allow(clippy::cast_precision_loss)]
            let rect = UiRect::new(room.x, room.y, size.width as f32, size.height as f32);
            let clip = intersection(room, visible);
            renderer.ui_image_texture(
                list,
                image_id(slot.id, false),
                texture,
                UiImage::new(rect).clip(clip).opacity(slot.opacity),
            );
        }
        if let (Some(texture), Some((x, y))) = (&shown.popup, shown.popup_at) {
            let size = texture.size();
            // The popup's place and size are a few thousand pixels at most: f32 holds them exactly.
            #[allow(clippy::cast_precision_loss)]
            let rect = UiRect::new(
                room.x + x as f32,
                room.y + y as f32,
                size.width as f32,
                size.height as f32,
            );
            renderer.ui_image_texture(
                list,
                image_id(slot.id, true),
                texture,
                UiImage::new(rect).clip(visible).opacity(slot.opacity),
            );
        }
        if let Some((arrow, since, scale)) = shown.arrow {
            match arrow::look(arrow, since.elapsed()) {
                Some(mut look) => {
                    look.opacity *= f64::from(slot.opacity);
                    paint_arrow(renderer, list, look, visible, scale);
                }
                // The motion is over.
                None => shown.arrow = None,
            }
        }
    }
}

/// The arrow of a swipe between pages, over the room: a dark disc, a lighter one growing inside it
/// as the pull fills, and the arrow. `scale` is pixels to the point.
fn paint_arrow(
    renderer: &Renderer,
    list: &mut UiList,
    look: arrow::Look,
    visible: UiRect,
    scale: f32,
) {
    let place = arrow::place(look, [visible.x, visible.y, visible.w, visible.h], scale);
    let rect = |r: [f32; 4]| UiRect::new(r[0], r[1], r[2], r[3]);
    let alpha = look.opacity as f32;
    let travel = look.fill.clamp(0.0, 1.0) as f32;
    let armed = look.fill >= 1.0;
    list.quad(
        UiQuad::new(rect(place.disc))
            .fill([0.0, 0.0, 0.0, 0.62 * alpha])
            .radius(place.radius)
            .border(scale.max(1.0), [1.0, 1.0, 1.0, 0.22 * alpha])
            .shadow(
                0.0,
                2.0 * scale,
                10.0 * scale,
                [0.0, 0.0, 0.0, 0.35 * alpha],
            )
            .clip(visible),
    );
    if place.filling[2] >= 1.0 {
        list.quad(
            UiQuad::new(rect(place.filling))
                .fill([1.0, 1.0, 1.0, if armed { 0.34 } else { 0.16 } * alpha])
                .radius(place.filling[2] / 2.0)
                .clip(visible),
        );
    }
    let bitmap = UiBitmap {
        id: glyph_id(look.way),
        width: arrow::GLYPH,
        height: arrow::GLYPH,
        pixels: glyph(look.way),
    };
    let image = UiImage::new(rect(place.glyph))
        .clip(visible)
        .opacity((0.55 + 0.45 * travel) * alpha);
    renderer.ui_image(list, &bitmap, image);
}

fn intersection(a: UiRect, b: UiRect) -> UiRect {
    let (x0, y0) = (a.x.max(b.x), a.y.max(b.y));
    let (x1, y1) = ((a.x + a.w).min(b.x + b.w), (a.y + a.h).min(b.y + b.h));
    UiRect::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
}

/// What a key that is down was sent as, so that its release is the same key.
#[derive(Default)]
struct Typing {
    /// What the input method composes, which `hasMarkedText` says.
    marked: String,
    /// The text a key event committed, while the event is being interpreted.
    inserted: Option<String>,
    /// The input method set marked text while the event was interpreted.
    composed: bool,
    /// The input method named a command for the event (the key is no text).
    commanded: bool,
    /// The commands it named, as the engine is sent them ([`input::edit_command`]).
    commands: Vec<String>,
}

/// What the system's text handling made of a key event.
struct Interpreted {
    inserted: Option<String>,
    composed: bool,
    commanded: bool,
    commands: Vec<String>,
}

/// The browsers of one page, and the pane's side of them.
pub(crate) struct PageBrowsers {
    rooms: RefCell<Rooms>,
    pictures: Arc<Pictures>,
    /// What the outline's apply produced: done on the main queue, not inside the frame.
    effects: RefCell<Vec<Effect>>,
    /// The room the person has the keyboard in.
    focused: Cell<Option<Id>>,
    /// The room that had it last: where the host's own keys go when the keyboard is on the page.
    last_focused: Cell<Option<Id>>,
    /// The room a pointer button went down in: the pointer is its until every button is up.
    held: Cell<Option<Id>>,
    /// The room a pinch began in: the fingers are its until they leave the trackpad.
    pinching: Cell<Option<Id>>,
    /// The room a swipe between pages is followed in ([`swipe`]), and the swipe.
    swiping: Cell<Option<Id>>,
    swipe: RefCell<swipe::Swipe>,
    /// The room a side button of the mouse went down in: its release is that room's.
    side: Cell<Option<Id>>,
    /// Device pixels to the point, as the outline last said.
    scale: Cell<f32>,
    /// The room the pointer is over.
    over: Cell<Option<Id>>,
    /// The shape the engine asked for over `over`.
    shape: Cell<Cursor>,
    /// The keys that are down, by the Mac's key code.
    down: RefCell<HashMap<u16, KeyData>>,
    typing: RefCell<Typing>,
    /// A key event is being interpreted by the system's text handling: its text is the key's.
    interpreting: Cell<bool>,
    /// Where the person last pressed in a room, for the input method's window (the protocol
    /// says where the caret is no more than that).
    pressed_at: Cell<Point>,
    questions: RefCell<VecDeque<questions::Question>>,
    asking: Cell<bool>,
    /// The dialogs of web pages that are up as sheets or parked behind another, by their number:
    /// a dialog the page left behind is taken down by its sheet ([`questions`]).
    dialogs: RefCell<Vec<(u32, objc2::rc::Retained<objc2_app_kit::NSAlert>)>>,
    /// The question at the head of the queue waits for a sheet that is up to end.
    seat_wait: Cell<bool>,
    installing: Cell<bool>,
    /// The probe that is out, and the way it is to show ends at the program's machine.
    probing: RefCell<Option<Probing>>,
    /// The jobs an agent gave the helper that have not come back, by their number, and the last
    /// number used ([`jobs`]).
    jobs: RefCell<HashMap<u32, Sender<AgentOutput>>>,
    next_job: Cell<u32>,
    /// Which page's browsers these are: what a thread says of a way is for the page that asked,
    /// and not for one that took its place in the pane (a program behind another `ssh`).
    serial: u64,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl PageBrowsers {
    /// The browsers of `pane`'s page. `None` where there is no engine to be had (no home
    /// directory).
    pub(crate) fn new(
        manager: Arc<Manager>,
        renderer: &Renderer,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Rc<Self> {
        let pictures = Pictures::new(renderer, wake.clone());
        let rooms = Rooms::new(manager, APPLICATION, wake.clone(), pictures.clone());
        Rc::new(Self {
            rooms: RefCell::new(rooms),
            pictures,
            effects: RefCell::new(Vec::new()),
            focused: Cell::new(None),
            last_focused: Cell::new(None),
            held: Cell::new(None),
            pinching: Cell::new(None),
            swiping: Cell::new(None),
            swipe: RefCell::new(swipe::Swipe::default()),
            side: Cell::new(None),
            scale: Cell::new(1.0),
            over: Cell::new(None),
            shape: Cell::new(Cursor::Arrow),
            down: RefCell::new(HashMap::new()),
            typing: RefCell::new(Typing::default()),
            interpreting: Cell::new(false),
            pressed_at: Cell::new(Point::new(0.0, 0.0)),
            questions: RefCell::new(VecDeque::new()),
            asking: Cell::new(false),
            dialogs: RefCell::new(Vec::new()),
            seat_wait: Cell::new(false),
            installing: Cell::new(false),
            probing: RefCell::new(None),
            jobs: RefCell::new(HashMap::new()),
            next_job: Cell::new(0),
            serial: {
                static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            },
            wake,
        })
    }

    /// Which page's browsers these are ([`PageBrowsers::serial`]'s field).
    pub(crate) fn serial(&self) -> u64 {
        self.serial
    }

    /// The pictures, for the page's list.
    pub(crate) fn pictures(&self) -> Arc<Pictures> {
        self.pictures.clone()
    }

    /// The page's outline changed: browsers are made and moved as it says. Called inside a frame,
    /// so what it makes the page or the pane do waits for the main queue.
    pub(crate) fn outline(&self, outline: &Outline, scale: f32) {
        let mut told = Vec::new();
        self.scale.set(scale);
        self.rooms
            .borrow_mut()
            .apply(&outline.browsers, scale, &mut told);
        let kept = |id: Id| outline.browsers.iter().any(|b| b.id == id);
        if self.swiping.get().is_some_and(|id| !kept(id)) {
            self.swiping.set(None);
            self.swipe.borrow_mut().reset();
        }
        if self.side.get().is_some_and(|id| !kept(id)) {
            self.side.set(None);
        }
        if self.focused.get().is_some_and(|id| !kept(id)) {
            self.focused.set(None);
        }
        if self.last_focused.get().is_some_and(|id| !kept(id)) {
            self.last_focused.set(None);
        }
        if self.over.get().is_some_and(|id| !kept(id)) {
            self.over.set(None);
        }
        if self.held.get().is_some_and(|id| !kept(id)) {
            self.held.set(None);
        }
        if !told.is_empty() {
            self.effects.borrow_mut().extend(told);
            (self.wake)();
        }
    }

    /// What the helper's threads said, and what the outline made, turned into what the page is
    /// told and what the pane does.
    pub(crate) fn drain(&self, pane: &TerminalPane) {
        let mut effects = std::mem::take(&mut *self.effects.borrow_mut());
        self.rooms.borrow_mut().drain(&mut effects);
        let mut page_changed = false;
        for effect in effects {
            page_changed |= self.apply(pane, effect);
        }
        if page_changed {
            pane.drain_page();
        }
        // A frame, or a state, may be all there was.
        if let Some(link) = pane.link() {
            link.poke_page();
        }
    }

    /// One thing the engines said, done. `true`: the page was told something it may answer.
    fn apply(&self, pane: &TerminalPane, effect: Effect) -> bool {
        let Some(handle) = pane.page_handle() else {
            return false;
        };
        match effect {
            Effect::State(id, state) => {
                handle.page().browser(id, state);
                true
            }
            Effect::Window(id, url, background) => {
                handle.page().browser_window(id, &url, background);
                true
            }
            Effect::UnusedKey(_, key, mods, down) => {
                if down {
                    handle.input(Input::Key { key, mods }, mods);
                } else {
                    handle.key_up(key, mods);
                }
                true
            }
            Effect::Cursor(id, cursor) => {
                self.shape.set(cursor);
                if self.over.get() == Some(id) {
                    input::cursor_of(cursor).set();
                }
                false
            }
            Effect::Menu(id, menu) => {
                self.ask(pane, questions::Question::Menu(id, menu));
                false
            }
            Effect::Dialog(_, dialog) => {
                self.ask(pane, questions::Question::Dialog(dialog));
                false
            }
            Effect::DialogVoid(_, req) => {
                self.dialog_void(pane, req);
                false
            }
            Effect::Files(_, files) => {
                self.ask(pane, questions::Question::Files(files));
                false
            }
            Effect::Download(_, download) => {
                self.ask(pane, questions::Question::Download(download));
                false
            }
            Effect::Downloaded(_, path, done) => {
                questions::downloaded(pane, &path, done);
                false
            }
            Effect::NotInstalled(id) => {
                self.not_installed(pane, id);
                true
            }
            Effect::Route => {
                self.find_route(pane);
                false
            }
            Effect::Agent(_, req, output) => {
                self.job_done(req, output);
                false
            }
            Effect::CanScroll(id, req, can) => {
                self.scrolled(pane, id, req, can);
                false
            }
            Effect::Diagnostic(_, words) => {
                eprintln!("bateri: the browser: {words}");
                false
            }
        }
    }

    /// What the page asks of a browser ([`pencere_drive::Host::browser`]).
    pub(crate) fn ask_of(&self, id: Id, ask: BrowserAsk) {
        let mut told = Vec::new();
        self.rooms.borrow_mut().ask(id, ask, &mut told);
        if !told.is_empty() {
            // Not done inline: this is called while the page drains its replies.
            self.effects.borrow_mut().extend(told);
            (self.wake)();
        }
    }

    /// The keyboard goes to the room named `id` (the page asked, or the person clicked in it) or
    /// back to the page (`None`).
    pub(crate) fn give_keyboard(&self, id: Option<Id>) {
        let before = self.focused.replace(id);
        if id.is_some() {
            self.last_focused.set(id);
        }
        if before != id {
            let mut rooms = self.rooms.borrow_mut();
            if let Some(before) = before {
                rooms.focus(before, false);
            }
            if let Some(id) = id {
                rooms.focus(id, true);
            }
        }
    }

    /// Whether `id` names a room of this page's browsers.
    pub(crate) fn is_room(&self, id: Id) -> bool {
        self.rooms.borrow().ids().any(|room| room == id)
    }

    /// Whether the keyboard is in a room.
    pub(crate) fn has_keyboard(&self) -> bool {
        self.focused.get().is_some()
    }

    /// The window has the keyboard, or has not: the engine's focus follows it.
    pub(crate) fn set_active(&self, active: bool) {
        if let Some(id) = self.focused.get() {
            self.rooms.borrow_mut().focus(id, active);
        }
    }

    /// The room under `at` (the page's points) that the page does not cover, and where `at` is in
    /// it, points from its corner.
    fn room_at(&self, pane: &TerminalPane, at: Point) -> Option<(Id, (i32, i32))> {
        let handle = pane.page_handle()?;
        let page = handle.page();
        let room = page.browsers().iter().rev().find(|room| {
            !room.hidden
                && at.x >= room.rect.x
                && at.y >= room.rect.y
                && at.x < room.rect.x + room.rect.w
                && at.y < room.rect.y + room.rect.h
                && page.reaches(room.id, at)
        })?;
        let local = (
            input::rounded(at.x - room.rect.x),
            input::rounded(at.y - room.rect.y),
        );
        Some((room.id, local))
    }

    /// Where `at` is in the room `id`, even outside it (a drag that began in it goes on).
    fn local_in(&self, pane: &TerminalPane, id: Id, at: Point) -> Option<(i32, i32)> {
        let handle = pane.page_handle()?;
        let page = handle.page();
        let room = page.browsers().iter().find(|room| room.id == id)?;
        Some((
            input::rounded(at.x - room.rect.x),
            input::rounded(at.y - room.rect.y),
        ))
    }

    fn send(&self, id: Id, command: impl FnOnce(u32) -> Command) {
        self.rooms.borrow().send(id, command);
    }

    /// The pointer: `true` where a room took it (the page is not told).
    pub(crate) fn pointer(&self, pane: &TerminalPane, input: &Input) -> bool {
        match *input {
            Input::Move { at, mods } => {
                let mods = crate::native_browser::mods_bits(mods);
                if let Some(held) = self.held.get() {
                    if let Some((x, y)) = self.local_in(pane, held, at) {
                        self.send(held, |browser| {
                            mouse(
                                browser,
                                MouseKind::Move,
                                x,
                                y,
                                Button::Left,
                                0,
                                mods | browser_ipc::message::modifier::LEFT_BUTTON,
                            )
                        });
                    }
                    return true;
                }
                match self.room_at(pane, at) {
                    Some((id, (x, y))) => {
                        if self.over.replace(Some(id)) != Some(id) {
                            input::cursor_of(self.shape.get()).set();
                        }
                        self.send(id, |browser| {
                            mouse(browser, MouseKind::Move, x, y, Button::Left, 0, mods)
                        });
                        true
                    }
                    None => {
                        self.left(pane);
                        false
                    }
                }
            }
            Input::Leave => {
                self.left(pane);
                false
            }
            Input::Press {
                at,
                button,
                mods,
                count,
            } => {
                let Some((id, (x, y))) = self.room_at(pane, at) else {
                    // A press on the page's own is the person's leaving the browser.
                    if self.focused.get().is_some() {
                        self.give_keyboard(None);
                    }
                    return false;
                };
                self.held.set(Some(id));
                self.pressed_at.set(at);
                self.take_keyboard(pane, id);
                let mods = mods_bits(mods) | input::held_bit(button);
                self.send(id, |browser| {
                    mouse(browser, MouseKind::Down, x, y, button, count, mods)
                });
                true
            }
            Input::Release {
                at, button, mods, ..
            } => {
                let Some(held) = self.held.take() else {
                    return false;
                };
                if let Some((x, y)) = self.local_in(pane, held, at) {
                    let mods = mods_bits(mods);
                    self.send(held, |browser| {
                        mouse(browser, MouseKind::Up, x, y, button, 1, mods)
                    });
                }
                true
            }
            _ => false,
        }
    }

    /// The pointer left the room it was over.
    fn left(&self, pane: &TerminalPane) {
        let _ = pane;
        if let Some(id) = self.over.take() {
            self.send(id, |browser| {
                mouse(browser, MouseKind::Leave, 0, 0, Button::Left, 0, 0)
            });
        }
    }

    /// The person pressed in the room `id`: it has the keyboard, and the page is told they did
    /// ([`pencere_drive::Page::focus`]).
    fn take_keyboard(&self, pane: &TerminalPane, id: Id) {
        self.give_keyboard(Some(id));
        if let Some(handle) = pane.page_handle() {
            handle.page().focus(Some(id));
        }
        pane.drain_page();
    }

    /// The wheel over a room: `true` where a room took it. `dx` and `dy` are the page's reading
    /// (down and right positive). A trackpad's sideways gesture may be a swipe between pages
    /// ([`swipe`]): then the engine is not given what the fingers do.
    pub(crate) fn wheel(&self, pane: &TerminalPane, at: Point, wheel: Wheel, mods: Mods) -> bool {
        let Wheel {
            dx,
            dy,
            phase,
            precise,
        } = wheel;
        // A swipe the arrow follows is its room's, over whatever the fingers pass.
        let followed = self
            .swiping
            .get()
            .filter(|_| self.swipe.borrow().is_pulling());
        let found = match followed {
            Some(id) => self.local_in(pane, id, at).map(|local| (id, local)),
            None => self.room_at(pane, at),
        };
        let Some((id, (x, y))) = found else {
            if let Some(id) = followed {
                // The room went while the fingers were on it.
                self.end_swipe(pane, id);
                return true;
            }
            return false;
        };
        if let Some(other) = self.swiping.get().filter(|room| *room != id) {
            self.end_swipe(pane, other);
        }
        let scroll = swipe::Scroll {
            // The engine's sign is the opposite of the page's.
            dx: -f64::from(dx),
            dy: -f64::from(dy),
            phase: swipe_phase(phase),
            momentum: matches!(phase, Phase::Coast | Phase::Stop),
            precise,
            at: (x, y),
        };
        let (back, forward) = self.rooms.borrow().history(id);
        let step = self.swipe.borrow_mut().feed(
            scroll,
            NSEvent::isSwipeTrackingFromScrollEventsEnabled(),
            swipe::History { back, forward },
        );
        self.swiping.set(Some(id));
        self.swipe_step(pane, id, step);
        if !step.forward {
            return true;
        }
        let (dx, dy) = input::wheel_of(dx, dy, precise);
        if dx != 0 || dy != 0 {
            let mods = mods_bits(mods);
            self.send(id, |browser| Command::Wheel {
                browser,
                x,
                y,
                dx,
                dy,
                mods,
            });
        }
        true
    }

    /// What a step of the swipe asks for: the helper's answer to whether the page scrolls, the
    /// arrow, and the browser going.
    fn swipe_step(&self, pane: &TerminalPane, id: Id, step: swipe::Step) {
        if let Some(ask) = step.ask {
            self.send(id, |browser| Command::CanScroll {
                browser,
                req: ask.req,
                x: ask.at.0,
                y: ask.at.1,
                dx: ask.dx,
            });
        }
        if let Some(arrow) = step.arrow {
            self.show_arrow(pane, id, arrow);
        }
        if let Some(way) = step.go {
            self.send(id, |browser| match way {
                swipe::Way::Back => Command::Back { browser },
                swipe::Way::Forward => Command::Forward { browser },
            });
        }
    }

    /// The arrow is `arrow` in the room. With Reduce Motion nothing moves by itself: what would
    /// withdraw or fade is gone at once.
    fn show_arrow(&self, pane: &TerminalPane, id: Id, arrow: swipe::Arrow) {
        let arrow = if arrow::moves_alone(arrow) && pane.reduces_motion() {
            swipe::Arrow::Hidden
        } else {
            arrow
        };
        self.pictures.set_arrow(id, arrow, self.scale.get());
    }

    /// Whatever of a swipe is left in the room is nobody's: the arrow goes.
    fn end_swipe(&self, pane: &TerminalPane, id: Id) {
        self.swiping.set(None);
        if self.swipe.borrow_mut().reset().is_some() {
            self.show_arrow(pane, id, swipe::Arrow::Hidden);
        }
    }

    /// The helper's answer to whether the page scrolls where the swipe began.
    fn scrolled(&self, pane: &TerminalPane, id: Id, req: u32, can: bool) {
        if self.swiping.get() != Some(id) {
            return;
        }
        let (back, forward) = self.rooms.borrow().history(id);
        let step = self
            .swipe
            .borrow_mut()
            .answer(req, can, swipe::History { back, forward });
        self.swipe_step(pane, id, step);
    }

    /// A side button of the mouse (`number` 3 goes back, 4 forward) over a room: `true` where a
    /// room took it. The page has it first (`mousedown`, `mouseup` with `button` 3 or 4) and the
    /// engine goes when the button is let go, unless the page took the `mouseup`, as Chrome does
    /// (the helper's `Command::Mouse`). The release goes where the press did.
    pub(crate) fn side_button(
        &self,
        pane: &TerminalPane,
        at: Point,
        number: isize,
        down: bool,
        mods: Mods,
    ) -> bool {
        let Some(button) = input::side_button_of(number) else {
            return false;
        };
        let (id, (x, y)) = if down {
            let Some((id, local)) = self.room_at(pane, at) else {
                return false;
            };
            self.side.set(Some(id));
            (id, local)
        } else {
            let Some(id) = self.side.take() else {
                return false;
            };
            let Some(local) = self.local_in(pane, id, at) else {
                // The room went while the button was down.
                return true;
            };
            (id, local)
        };
        let kind = if down { MouseKind::Down } else { MouseKind::Up };
        let mods = mods_bits(mods);
        self.send(id, |browser| Command::Mouse {
            browser,
            kind,
            x,
            y,
            button,
            clicks: 1,
            mods,
        });
        true
    }

    /// A pinch over a room: `true` where a room took it (the page is not told). It is the room's
    /// from the fingers' coming to their leaving, wherever they go meanwhile; a pinch that began
    /// on what the page drew is the page's, and one that began in a room is never the page's,
    /// so a box of the page's that zooms does not zoom behind a browser.
    pub(crate) fn magnify(
        &self,
        pane: &TerminalPane,
        at: Point,
        by: f32,
        phase: GesturePhase,
    ) -> bool {
        let (id, (x, y)) = match phase {
            GesturePhase::Begin => {
                let Some((id, local)) = self.room_at(pane, at) else {
                    return false;
                };
                self.pinching.set(Some(id));
                (id, local)
            }
            _ => {
                let Some(id) = self.pinching.get() else {
                    return false;
                };
                let Some(local) = self.local_in(pane, id, at) else {
                    // The room went while the fingers were on it: what is left of the pinch is
                    // nobody's.
                    self.pinching.set(None);
                    return true;
                };
                (id, local)
            }
        };
        for (kind, scale) in input::pinch_of(phase, by) {
            self.send(id, |browser| Command::Pinch {
                browser,
                kind,
                x,
                y,
                scale,
            });
        }
        if phase == GesturePhase::End {
            self.pinching.set(None);
        }
        true
    }

    // --- The keyboard ------------------------------------------------------------------------

    /// A key down with the keyboard in a room. `true`: the room took it. A key the page asked for
    /// (its `keys`) is the page's and is not taken.
    pub(crate) fn key_down(&self, pane: &TerminalPane, event: &NSEvent) -> bool {
        // ⌥⌘I opens the engine's tools: the host's own key, as in a browser. With the keyboard on
        // the page (after a load error's card, say) it is the browser that had the keyboard last.
        let flags = event.modifierFlags();
        if event.keyCode() == 34
            && flags.contains(objc2_app_kit::NSEventModifierFlags::Command)
            && flags.contains(objc2_app_kit::NSEventModifierFlags::Option)
            && !flags.contains(objc2_app_kit::NSEventModifierFlags::Control)
            && let Some(room) = self.tools_for(pane)
        {
            self.send(room, |browser| Command::DevTools { browser });
            return true;
        }
        let Some(room) = self.focused.get() else {
            return false;
        };
        if let Some((key, mods)) = crate::native_page::event_key_of(event)
            && self.rooms.borrow().page_key(room, key, mods)
            && self.typing.borrow().marked.is_empty()
        {
            return false;
        }
        // ⇧⌘Z (Edit's Redo, which has no row in the host's menu) goes below as any ⌘ key does: to
        // the engine as a key, and back as the menu's command when the page did not take it.
        let shortcut = flags.contains(objc2_app_kit::NSEventModifierFlags::Command)
            || flags.contains(objc2_app_kit::NSEventModifierFlags::Control);
        let characters = event
            .characters()
            .map(|text| text.to_string())
            .unwrap_or_default();
        let composing = !self.typing.borrow().marked.is_empty();
        if (shortcut || input::is_function_key(&characters)) && !composing {
            // The system's text handling has a command for many of these (⌘← is the line's start,
            // ⌃A the paragraph's): it rides with the key. What else it would make of the key (no
            // text: a shortcut's key is no text) is let go, as before.
            let commands = self.interpret(pane, event).commands;
            self.raw(room, event, &characters, commands);
            return true;
        }
        // Text, or what an input method makes of the key: the system's handling of text says
        // which.
        let Interpreted {
            inserted,
            composed,
            commanded,
            commands,
        } = self.interpret(pane, event);
        let was = composing;
        match inserted {
            Some(text) => {
                let one = text.encode_utf16().count() == 1;
                if one && !was {
                    self.typed(room, event, &text);
                } else {
                    self.send(room, |browser| Command::ImeCommit { browser, text });
                }
            }
            None if composed => {}
            None if commanded || !composing => self.raw(room, event, &characters, commands),
            // The input method kept the key (a candidate chosen with an arrow).
            None => {}
        }
        true
    }

    /// Hands the key to the system's text handling (`interpretKeyEvents:`) and says what it made
    /// of it: text, a composition, commands.
    fn interpret(&self, pane: &TerminalPane, event: &NSEvent) -> Interpreted {
        {
            let mut typing = self.typing.borrow_mut();
            typing.inserted = None;
            typing.composed = false;
            typing.commanded = false;
            typing.commands.clear();
        }
        self.interpreting.set(true);
        pane.view()
            .interpretKeyEvents(&objc2_foundation::NSArray::from_slice(&[event]));
        self.interpreting.set(false);
        let mut typing = self.typing.borrow_mut();
        Interpreted {
            inserted: typing.inserted.take(),
            composed: std::mem::take(&mut typing.composed),
            commanded: std::mem::take(&mut typing.commanded),
            commands: std::mem::take(&mut typing.commands),
        }
    }

    /// A key that is no text: down (with the commands the system named for it), its character
    /// where it makes one, and (when it comes) up.
    fn raw(&self, room: Id, event: &NSEvent, characters: &str, commands: Vec<String>) {
        let key = key_data(event, characters, event.keyCode());
        self.down.borrow_mut().insert(event.keyCode(), key);
        self.send(room, |browser| Command::Key {
            browser,
            kind: KeyKind::Down,
            key,
            commands,
        });
        if input::has_character(key.vk) {
            self.send(room, |browser| Command::Key {
                browser,
                kind: KeyKind::Char,
                key,
                commands: Vec::new(),
            });
        }
    }

    /// A key that made one character: down, the character, and (when it comes) up.
    fn typed(&self, room: Id, event: &NSEvent, text: &str) {
        let mut key = key_data(event, text, event.keyCode());
        key.character = input::unit_of(text);
        self.down.borrow_mut().insert(event.keyCode(), key);
        self.send(room, |browser| Command::Key {
            browser,
            kind: KeyKind::Down,
            key,
            commands: Vec::new(),
        });
        self.send(room, |browser| Command::Key {
            browser,
            kind: KeyKind::Char,
            key,
            commands: Vec::new(),
        });
    }

    /// A key came up. `true` where its press was a room's.
    pub(crate) fn key_up(&self, event: &NSEvent) -> bool {
        let Some(key) = self.down.borrow_mut().remove(&event.keyCode()) else {
            return false;
        };
        if let Some(room) = self.focused.get() {
            self.send(room, |browser| Command::Key {
                browser,
                kind: KeyKind::Up,
                key,
                commands: Vec::new(),
            });
        }
        true
    }

    /// The system's text handling inserts `text` (a key's, an input method's commit, the character
    /// viewer's, dictation): while a key event is being interpreted it is kept for [`Self::key_down`]
    /// to say how, and otherwise it is the text of the person's.
    pub(crate) fn insert(&self, text: String) {
        let Some(room) = self.focused.get() else {
            return;
        };
        // A commit ends what was composed.
        self.typing.borrow_mut().marked.clear();
        if self.interpreting.get() {
            self.typing.borrow_mut().inserted = Some(text);
            return;
        }
        self.send(room, |browser| Command::ImeCommit { browser, text });
    }

    /// The input method composes `text`, with `selected` of it selected.
    pub(crate) fn compose(&self, text: String, selected: (usize, usize)) {
        let Some(room) = self.focused.get() else {
            return;
        };
        {
            let mut typing = self.typing.borrow_mut();
            typing.composed = true;
            typing.marked.clone_from(&text);
        }
        if text.is_empty() {
            self.send(room, |browser| Command::ImeCancel { browser });
            return;
        }
        let at = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        let (from, to) = (at(selected.0), at(selected.0 + selected.1));
        self.send(room, |browser| Command::ImeCompose {
            browser,
            text,
            from,
            to,
        });
    }

    /// The composition ends with what it has typed (`unmarkText`).
    pub(crate) fn unmark(&self) {
        let Some(room) = self.focused.get() else {
            return;
        };
        let marked = std::mem::take(&mut self.typing.borrow_mut().marked);
        if !marked.is_empty() {
            self.send(room, |browser| Command::ImeCommit {
                browser,
                text: marked,
            });
        }
    }

    /// The system's text handling names a command (`moveToBeginningOfLine:` for ⌘←, ⌫'s
    /// `deleteBackward:`) for the key being interpreted: the key is no text, and goes as a key with
    /// the command beside it ([`input::edit_command`] says which names go).
    pub(crate) fn commanded(&self, selector: &str) {
        let mut typing = self.typing.borrow_mut();
        typing.commanded = true;
        if let Some(name) = input::edit_command(selector)
            && typing.commands.len() < input::COMMANDS_MAX
            && !typing.commands.contains(&name)
        {
            typing.commands.push(name);
        }
    }

    /// What the input method composes now.
    pub(crate) fn marked(&self) -> String {
        self.typing.borrow().marked.clone()
    }

    /// An item of the Edit menu (copy, cut, paste, undo, redo, select all) with the keyboard in a
    /// room: the engine runs it on the field or the page that has the focus, with the system's
    /// pasteboard (a paste is the engine's own, with its `paste` event). `false` where no room has
    /// the keyboard.
    fn edit(&self, action: EditAction) -> bool {
        let Some(room) = self.focused.get() else {
            return false;
        };
        self.send(room, |browser| Command::Edit { browser, action });
        true
    }

    /// The item of the Edit menu that has the key `key` as its shortcut (⌘C, ⌘X, ⌘V, ⌘A, ⌘Z).
    ///
    /// Run by its key, the item is the page's second: the key goes to the engine as a key and the
    /// engine gives it back if the page did not take it (a page with its own undo, an editor),
    /// and then it is the command ([`bt_shell_common::browser::rooms::edit_of_key`]), as in
    /// Chrome. Run by a click on its row there is no key, and the command is run at once.
    pub(crate) fn edit_menu(&self, pane: &TerminalPane, key: char) -> bool {
        let Some(room) = self.focused.get() else {
            return false;
        };
        let app = objc2_app_kit::NSApplication::sharedApplication(objc2::MainThreadOnly::mtm(pane));
        if let Some(event) = app.currentEvent()
            && event.r#type() == objc2_app_kit::NSEventType::KeyDown
            && input::is_key_of(
                key,
                event
                    .charactersIgnoringModifiers()
                    .map(|text| text.to_string())
                    .as_deref(),
                event.modifierFlags(),
            )
        {
            let characters = event
                .characters()
                .map(|text| text.to_string())
                .unwrap_or_default();
            let data = key_data(&event, &characters, event.keyCode());
            // Down and up together: the up of a key a menu took is not waited for.
            for kind in [KeyKind::Down, KeyKind::Up] {
                self.send(room, |browser| Command::Key {
                    browser,
                    kind,
                    key: data,
                    commands: Vec::new(),
                });
            }
            return true;
        }
        self.edit(input::edit_action(key))
    }

    /// The browser the host's tools key means: the one the keyboard is in, or with it on the page
    /// the one that had it last if it is shown, or the page's only shown one.
    fn tools_for(&self, pane: &TerminalPane) -> Option<Id> {
        if let Some(room) = self.focused.get() {
            return Some(room);
        }
        let handle = pane.page_handle()?;
        let page = handle.page();
        let shown: Vec<Id> = page
            .browsers()
            .iter()
            .filter(|room| !room.hidden)
            .map(|room| room.id)
            .collect();
        self.last_focused
            .get()
            .filter(|id| shown.contains(id))
            .or(match shown.as_slice() {
                [only] => Some(*only),
                _ => None,
            })
    }

    /// Where the input method's window goes: about where the person last pressed in a room, the
    /// protocol saying no more of the caret.
    pub(crate) fn caret(&self) -> Point {
        self.pressed_at.get()
    }
}

fn mouse(
    browser: u32,
    kind: MouseKind,
    x: i32,
    y: i32,
    button: Button,
    clicks: Clicks,
    mods: u32,
) -> Command {
    Command::Mouse {
        browser,
        kind,
        x,
        y,
        button: input::button_of(button),
        clicks: clicks.max(1),
        mods,
    }
}

/// A wheel event as the page reads it: `dx` and `dy` are down and right positive, `phase` is where
/// it is in its gesture, and `precise` that the deltas are a trackpad's points and not a wheel's
/// lines.
#[derive(Clone, Copy)]
pub(crate) struct Wheel {
    pub dx: f32,
    pub dy: f32,
    pub phase: Phase,
    pub precise: bool,
}

/// What part of a gesture a wheel event is, as the swipe reads it.
fn swipe_phase(phase: Phase) -> swipe::Phase {
    match phase {
        Phase::Begin => swipe::Phase::Began,
        Phase::Move => swipe::Phase::Changed,
        Phase::Lift => swipe::Phase::Ended,
        _ => swipe::Phase::None,
    }
}

/// The key event's data for the engine.
fn key_data(event: &NSEvent, characters: &str, code: u16) -> KeyData {
    let ignoring = event
        .charactersIgnoringModifiers()
        .map(|text| text.to_string())
        .unwrap_or_default();
    KeyData {
        vk: input::vk_of(code, &ignoring),
        native: i32::from(code),
        character: input::unit_of(characters),
        unmodified: input::unit_of(&ignoring),
        mods: input::mods_of(event.modifierFlags()),
        system: false,
    }
}

/// The modifiers of the page's reading, as the protocol's bits.
pub(crate) fn mods_bits(mods: Mods) -> u32 {
    use browser_ipc::message::modifier;
    let mut bits = 0;
    for (held, bit) in [
        (mods.shift, modifier::SHIFT),
        (mods.control, modifier::CONTROL),
        (mods.alt, modifier::ALT),
        (mods.command, modifier::COMMAND),
    ] {
        if held {
            bits |= bit;
        }
    }
    bits
}

/// A state with only a status and what to say of it, as the page is told it.
pub(crate) fn status(status: &str, words: &str) -> BrowserState {
    BrowserState::with_status(status, Some(words))
}

/// Runs `job` on the main queue with the pane `id`, if it is still open: what a thread of the
/// browsers says goes this way.
pub(crate) fn on_main(id: u64, job: impl FnOnce(&TerminalPane) + Send + 'static) {
    dispatch2::DispatchQueue::main().exec_async(move || {
        // audit: a block running on the main queue is on the main thread by definition.
        let mtm = objc2::MainThreadMarker::new().expect("the main queue is the main thread");
        if let Some(pane) = crate::embed::pane(mtm, id) {
            job(&pane);
        }
    });
}
