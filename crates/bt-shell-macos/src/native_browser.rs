//! The browsers of a page in a pane (plan 034, Phase 5): the room a page names for a browser is
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

mod input;
mod install;
mod questions;

pub(crate) use install::update_in_background;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::{Arc, Mutex, PoisonError};

use browser_ipc::message::{Command, Cursor, KeyData, KeyKind, MouseKind};
use browser_ipc::{Frame, Surface, import, wgpu};
use bt_gpu::{Renderer, UiImage, UiList, UiRect};
use bt_shell_common::browser::helper::Manager;
use bt_shell_common::browser::rooms::{Effect, Frames, Rooms};
use bt_shell_common::native_view::{
    BrowserAsk, BrowserSlot, BrowserState, Button, Id, Input, Mods, Outline, Point, Surfaces,
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
        let slots = lock(&self.slots);
        let Some(shown) = slots.get(&slot.id) else {
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
    }
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
}

/// The browsers of one page, and the pane's side of them.
pub(crate) struct PageBrowsers {
    rooms: RefCell<Rooms>,
    pictures: Arc<Pictures>,
    /// What the outline's apply produced: done on the main queue, not inside the frame.
    effects: RefCell<Vec<Effect>>,
    /// The room the person has the keyboard in.
    focused: Cell<Option<Id>>,
    /// The room a pointer button went down in: the pointer is its until every button is up.
    held: Cell<Option<Id>>,
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
    installing: Cell<bool>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl PageBrowsers {
    /// The browsers of `pane`'s page. `None` where there is no engine to be had (no home
    /// directory).
    pub(crate) fn new(
        pane: &TerminalPane,
        manager: Arc<Manager>,
        renderer: &Renderer,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Rc<Self> {
        let pictures = Pictures::new(renderer, wake.clone());
        // What the profile of a program's browsers is kept under: the machine the program runs
        // on, which a program of this Mac's is "mac". A server's name is Phase 6's to give.
        let machine = pane.browser_machine();
        let rooms = Rooms::new(
            manager,
            &machine,
            APPLICATION,
            wake.clone(),
            pictures.clone(),
        );
        Rc::new(Self {
            rooms: RefCell::new(rooms),
            pictures,
            effects: RefCell::new(Vec::new()),
            focused: Cell::new(None),
            held: Cell::new(None),
            over: Cell::new(None),
            shape: Cell::new(Cursor::Arrow),
            down: RefCell::new(HashMap::new()),
            typing: RefCell::new(Typing::default()),
            interpreting: Cell::new(false),
            pressed_at: Cell::new(Point::new(0.0, 0.0)),
            questions: RefCell::new(VecDeque::new()),
            asking: Cell::new(false),
            installing: Cell::new(false),
            wake,
        })
    }

    /// The pictures, for the page's list.
    pub(crate) fn pictures(&self) -> Arc<Pictures> {
        self.pictures.clone()
    }

    /// The page's outline changed: browsers are made and moved as it says. Called inside a frame,
    /// so what it makes the page or the pane do waits for the main queue.
    pub(crate) fn outline(&self, outline: &Outline, scale: f32) {
        let mut told = Vec::new();
        self.rooms
            .borrow_mut()
            .apply(&outline.browsers, scale, &mut told);
        let kept = |id: Id| outline.browsers.iter().any(|b| b.id == id);
        if self.focused.get().is_some_and(|id| !kept(id)) {
            self.focused.set(None);
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
            Effect::Agent(..) => false,
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
        #[allow(clippy::cast_possible_truncation)]
        let local = (
            (at.x - room.rect.x).round() as i32,
            (at.y - room.rect.y).round() as i32,
        );
        Some((room.id, local))
    }

    /// Where `at` is in the room `id`, even outside it (a drag that began in it goes on).
    fn local_in(&self, pane: &TerminalPane, id: Id, at: Point) -> Option<(i32, i32)> {
        let handle = pane.page_handle()?;
        let page = handle.page();
        let room = page.browsers().iter().find(|room| room.id == id)?;
        #[allow(clippy::cast_possible_truncation)]
        Some((
            (at.x - room.rect.x).round() as i32,
            (at.y - room.rect.y).round() as i32,
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
    /// (down and right positive).
    pub(crate) fn wheel(
        &self,
        pane: &TerminalPane,
        at: Point,
        dx: f32,
        dy: f32,
        precise: bool,
        mods: Mods,
    ) -> bool {
        let Some((id, (x, y))) = self.room_at(pane, at) else {
            return false;
        };
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

    // --- The keyboard ------------------------------------------------------------------------

    /// A key down with the keyboard in a room. `true`: the room took it. A key the page asked for
    /// (its `keys`) is the page's and is not taken.
    pub(crate) fn key_down(&self, pane: &TerminalPane, event: &NSEvent) -> bool {
        let Some(room) = self.focused.get() else {
            return false;
        };
        // ⌥⌘I opens the engine's tools for the room: the host's own key, as in a browser.
        let flags = event.modifierFlags();
        if event.keyCode() == 34
            && flags.contains(objc2_app_kit::NSEventModifierFlags::Command)
            && flags.contains(objc2_app_kit::NSEventModifierFlags::Option)
            && !flags.contains(objc2_app_kit::NSEventModifierFlags::Control)
        {
            self.send(room, |browser| Command::DevTools { browser });
            return true;
        }
        if let Some((key, mods)) = crate::native_page::event_key_of(event)
            && self.rooms.borrow().page_key(room, key, mods)
            && self.typing.borrow().marked.is_empty()
        {
            return false;
        }
        let flags = event.modifierFlags();
        let shortcut = flags.contains(objc2_app_kit::NSEventModifierFlags::Command)
            || flags.contains(objc2_app_kit::NSEventModifierFlags::Control);
        let characters = event
            .characters()
            .map(|text| text.to_string())
            .unwrap_or_default();
        let composing = !self.typing.borrow().marked.is_empty();
        if (shortcut || input::is_function_key(&characters)) && !composing {
            self.raw(room, event, &characters);
            return true;
        }
        // Text, or what an input method makes of the key: the system's handling of text says
        // which.
        {
            let mut typing = self.typing.borrow_mut();
            typing.inserted = None;
            typing.composed = false;
            typing.commanded = false;
        }
        self.interpreting.set(true);
        pane.view()
            .interpretKeyEvents(&objc2_foundation::NSArray::from_slice(&[event]));
        self.interpreting.set(false);
        let (inserted, composed, commanded, was) = {
            let mut typing = self.typing.borrow_mut();
            (
                typing.inserted.take(),
                std::mem::take(&mut typing.composed),
                std::mem::take(&mut typing.commanded),
                composing,
            )
        };
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
            None if commanded || !composing => self.raw(room, event, &characters),
            // The input method kept the key (a candidate chosen with an arrow).
            None => {}
        }
        true
    }

    /// A key that is no text: down, its character where it makes one, and (when it comes) up.
    fn raw(&self, room: Id, event: &NSEvent, characters: &str) {
        let key = key_data(event, characters, event.keyCode());
        self.down.borrow_mut().insert(event.keyCode(), key);
        self.send(room, |browser| Command::Key {
            browser,
            kind: KeyKind::Down,
            key,
        });
        if input::has_character(key.vk) {
            self.send(room, |browser| Command::Key {
                browser,
                kind: KeyKind::Char,
                key,
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
        });
        self.send(room, |browser| Command::Key {
            browser,
            kind: KeyKind::Char,
            key,
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
        #[allow(clippy::cast_possible_truncation)]
        let (from, to) = (selected.0 as u32, (selected.0 + selected.1) as u32);
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

    /// The input method names a command (⌫, an arrow, ⏎) for the key being interpreted: the key
    /// is no text, and is sent as a key.
    pub(crate) fn commanded(&self) {
        self.typing.borrow_mut().commanded = true;
    }

    /// What the input method composes now.
    pub(crate) fn marked(&self) -> String {
        self.typing.borrow().marked.clone()
    }

    /// Pasted text (⌘V): inserted as the engine's own paste is, as text.
    pub(crate) fn paste(&self, text: String) {
        if let Some(room) = self.focused.get() {
            self.send(room, |browser| Command::ImeCommit { browser, text });
        }
    }

    /// A shortcut of the Edit menu with the keyboard in a room (⌘A, ⌘C, ⌘X, ⌘Z): as the key, which
    /// a web page that binds it hears.
    pub(crate) fn shortcut(&self, character: char) {
        let Some(room) = self.focused.get() else {
            return;
        };
        let vk = i32::from(character.to_ascii_uppercase() as u8);
        let key = KeyData {
            vk,
            native: 0,
            character: u16::from(character as u8),
            unmodified: u16::from(character as u8),
            mods: browser_ipc::message::modifier::COMMAND,
            system: false,
        };
        for kind in [KeyKind::Down, KeyKind::Up] {
            self.send(room, |browser| Command::Key { browser, kind, key });
        }
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
