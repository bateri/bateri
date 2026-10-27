//! A page of the kit that is a module in a sandbox ([`pencere_sandbox`]), in a pane.
//!
//! The module runs on a thread of its own and **the frame never waits for it**: a build takes the
//! newest page the module has handed out, lays it out, paints it, tells the module how it stands
//! (so the module can answer where the pointer is) and, if the page is for another size or is
//! moving, asks for the next one. What comes back wakes the application, which drains it
//! ([`PageDrive::drain`]) and draws a frame. A page that does not change costs nothing: nothing
//! is asked, nothing is woken, no frame is drawn.
//!
//! **The page's images come the same way.** A file of the bundle is decoded on a thread of its
//! own ([`Images`]); the layout asks for its size, the paint for its pixels, and what is not there
//! yet is a box with nothing in it. The decoder wakes the application through the page's own
//! `wake` — once for everything that arrived since it last looked — and the drain reads what
//! came ([`Images::poll`]): a size is a page laid out again, pixels a page drawn again, a refusal
//! a word for the page's author ([`Drained::diagnostics`]).
//!
//! Until the module has handed out its first page — it is compiled first, which takes a moment —
//! the pane shows a line saying so, laid out like any page but not told to the module. A module
//! that goes wrong is gone: the pane is told why ([`Drained::ended`]) and takes the page down.
//!
//! **A page's editors are the pane's** ([`super::editing`]): the frame makes them for the rooms
//! the page laid out, does what its replies asked of them and draws them, and the pane's keys,
//! input method, pasteboard and pointer reach them between frames ([`PageEditors`]), measured by
//! the renderer the frames are drawn with ([`PageDrive::set_renderer`]).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use bt_core::Flow;
use bt_gpu::Renderer;
use pencere_editor::Editors;
use pencere_sandbox::{
    Assets, ImageLimits, Images, Module, ModuleLimits, Program, Script, Unbounded,
};
use pencere_view::{
    Button, DrawList, Element, Event, Key, Laid, Measure, Mods, Motions, Phase, Place, Rect, Role,
    ScrollTo, Size, div, text,
};

use super::editing::{Editing, Route, Told, in_a_room};
use super::{
    Drained, Field, HostScrolls, Id, Input, OnOutline, Outline, PageDrive, Placement, Point, Reply,
    Scrolling, Shaped, Typeset, UiContext, UiList, UiPage, from_pane, into_page, to_list,
};

/// What the page and its handle share.
struct Shared {
    module: Module,
    /// The page's images, if it has a bundle to read them from.
    images: Option<Rc<Images>>,
    /// Refused images the pane has not been told of.
    diagnostics: Vec<String>,
    /// The page as the module last handed it out.
    root: Option<Element>,
    /// The page as it was last laid out: what a key is asked against, for the command it does,
    /// and what the wheel moves a box on.
    laid: Option<Arc<Laid>>,
    /// The viewport (points) and scale `laid` is for.
    laid_for: Option<(Size, f32)>,
    /// The page has to be laid out again (a new page, an image's size): until it is, a frame for
    /// what only the scrolling changed moves the boxes of the page last laid out.
    fresh: bool,
    /// How far the pane has the page's boxes scrolled.
    scrolls: HostScrolls,
    /// The scrolls the module asked for that the next frame does.
    asks: Vec<ScrollTo>,
    /// The page is moving: it is asked for again at every frame, until it says it is not.
    animating: bool,
    /// The time the page at rest wants to be asked for again, on the frames' clock.
    wake_at: Option<f64>,
    /// The viewport (points) and the scale the module was last asked for a page for.
    asked: Option<(Size, f32)>,
    /// Where the page stands in the pane.
    place: Placement,
    /// The fields and what a screen reader says, as last told, in the pane's points.
    outline: Outline,
    on_outline: Option<OnOutline>,
    /// A page arrived that the pane has not been told of.
    redraw: bool,
    /// What the module said that the pane has not taken.
    replies: Vec<(Reply, bool)>,
    /// Why the module is gone, until the pane has been told.
    ended: Option<String>,
    told_end: bool,
    /// What the module said to its program that the pane has not carried.
    sent: Vec<String>,
    /// The page's editors.
    editing: Editing,
    /// What measures the page's text between frames.
    measure_with: MeasureWith,
    /// The frame just drawn left something for the next (a scroll an editor asked, a layout): the
    /// link is asked for it.
    again: bool,
    /// When the caret of the editor with the keyboard next blinks, on the frames' clock.
    blink_at: Option<f64>,
}

/// What measures a page's text between frames ([`PageDrive::set_renderer`]).
enum MeasureWith {
    /// Nothing yet: the pane has not given the renderer, and nothing is done to the editors.
    Nothing,
    Renderer(Rc<Renderer>),
    /// The tests' stand-in ([`super::editing::Even`]).
    #[cfg(test)]
    Even,
}

impl Shared {
    /// Takes what the module has put out into what the page and its handle keep. `building`: a
    /// frame is being laid out and drawn from what is taken, so it is that frame that shows it —
    /// the pane is not told to draw another for it (the wake that came with it finds nothing new
    /// to say). The drain, which is not a frame, is the one that says a page is to be drawn.
    fn sync(&mut self, building: bool) {
        let update = self.module.poll();
        if let Some(frame) = update.frame {
            self.root = Some(frame.root);
            self.animating = frame.animating;
            self.wake_at = frame.wake_at;
            self.fresh = true;
            self.redraw |= !building;
        }
        self.asks.extend(update.asks);
        if let Some(images) = &self.images {
            let arrived = images.poll();
            // A size the layout was waiting for, pixels the paint was: either way a frame of the
            // page, the one that lays it out again and draws it.
            if (arrived.relayout || arrived.redraw) && !building {
                self.redraw = true;
            }
            self.fresh |= arrived.relayout || arrived.redraw;
            self.diagnostics
                .extend(arrived.diagnostics.iter().map(ToString::to_string));
        }
        let place = self.place;
        for mut answer in update.replies {
            // What it asks of the editors is done at the next frame, once they are made for the
            // page it came with: a frame is worth drawing.
            if self.editing.take_reply(&mut answer.reply) && !building {
                self.redraw = true;
            }
            self.replies
                .push((from_pane(answer.reply, place), answer.pointer));
        }
        // How much the module may say is not decided here: it was told *full* by the flow when
        // the program had no room, and what it said is what the program has room for.
        if self.ended.is_none() {
            self.sent.extend(update.sent);
        }
        if let Some(fault) = update.closed
            && self.ended.is_none()
        {
            self.ended = Some(fault.to_string());
        }
    }
}

impl Shared {
    /// Tells the module where the boxes went that moved since it was last told: at once, not at
    /// the next frame, so that the page starts on the rows that come into view while the frame
    /// that moves the boxes is drawn.
    fn tell_scrolled(&mut self) {
        let moved = self.scrolls.scrolls.take_moved();
        if !moved.is_empty() {
            self.module.event(Event::Scrolled(moved));
        }
    }

    /// What came of something done to the editors: told the module (the texts first, so that it
    /// reads the version the events tell of), the scrolls they ask for done by the next frame, the
    /// page laid out again there where they want it. `building`: a frame is being drawn, and a
    /// next one is asked of the link if there is anything for it; otherwise the pane is to draw
    /// one ([`Drained::redraw`]) where anything came of it, or `drawn` says the editors changed.
    fn tell(&mut self, told: Told, building: bool, drawn: bool) {
        let more = !told.scrolls.is_empty() || told.relayout;
        let anything = !told.is_empty();
        if !told.events.is_empty() {
            self.module.texts(told.texts);
            for event in told.events {
                self.module.event(event);
            }
        }
        self.asks.extend(told.scrolls);
        self.fresh |= told.relayout;
        if building {
            self.again |= more;
        } else if anything || drawn {
            self.redraw = true;
        }
    }
}

/// What a module is held to in what it says to the program of its pane: the program's `flow`
/// ([`Flow`]) says whether the program has the room, and is told what the module takes of what
/// the program says. A module whose page no program opened (the engine's own, in this process)
/// has none: whatever it says goes nowhere.
struct Held(Flow);

impl Program for Held {
    fn may_say(&self, bytes: usize) -> bool {
        self.0.may_say(bytes)
    }

    fn taken(&self, bytes: usize) {
        self.0.taken(bytes);
    }
}

/// The page of a module as a pane's [`UiPage`], and the handle the pane drives it by.
///
/// `bytes` is the component (binary or text) and `limits` what it is held to. `terminal` is the
/// pane's font family and `inset` the points at the top the application keeps for its strip.
/// `flow` is the flow of the program that opened the page, if one did. `assets` is where the
/// files of its bundle are read from, if it has one: a page without them draws no image.
/// `wake` is called from the module's thread — and from the images' — when something is put
/// out; it hops to the main thread and has the pane drain the handle ([`PageDrive::drain`]),
/// **and never lays out or draws itself**: the images' decoder may call it from inside a
/// build. Returns at once: the module is compiled and started on its thread.
pub fn module_page(
    bytes: Vec<u8>,
    limits: ModuleLimits,
    terminal: Option<String>,
    inset: f32,
    flow: Option<Flow>,
    assets: Option<Arc<dyn Assets>>,
    wake: impl Fn() + Send + Sync + 'static,
) -> (Box<dyn UiPage>, ModuleHandle) {
    page_around(
        |program, wake| Module::from_component(bytes, limits, program, wake),
        terminal,
        inset,
        flow,
        assets,
        wake,
    )
}

/// The page of a script as a pane's [`UiPage`]: the same as [`module_page`], with the
/// interpreter of the script's language as the module and `script` as what it runs: its text, its
/// path in its bundle (what its errors name and what a `require` of it is relative to) and the
/// bundle. The other scripts of the bundle it can load are the ones that say they are of the type
/// `text/x-lua`, and it can load no other file; the bundle is where its images come from too.
pub fn script_page(
    script: Script,
    limits: ModuleLimits,
    terminal: Option<String>,
    inset: f32,
    flow: Option<Flow>,
    wake: impl Fn() + Send + Sync + 'static,
) -> (Box<dyn UiPage>, ModuleHandle) {
    let assets = script.bundle.clone();
    page_around(
        |program, wake| Module::from_script(script, limits, program, wake),
        terminal,
        inset,
        flow,
        assets,
        wake,
    )
}

fn page_around(
    start: impl FnOnce(Arc<dyn Program>, Box<dyn Fn() + Send + Sync>) -> Module,
    terminal: Option<String>,
    inset: f32,
    flow: Option<Flow>,
    assets: Option<Arc<dyn Assets>>,
    wake: impl Fn() + Send + Sync + 'static,
) -> (Box<dyn UiPage>, ModuleHandle) {
    let (page, handle) = parts(start, terminal, inset, flow, assets, wake);
    (Box::new(page), handle)
}

/// The page and its handle, the page as what it is.
fn parts(
    start: impl FnOnce(Arc<dyn Program>, Box<dyn Fn() + Send + Sync>) -> Module,
    terminal: Option<String>,
    inset: f32,
    flow: Option<Flow>,
    assets: Option<Arc<dyn Assets>>,
    wake: impl Fn() + Send + Sync + 'static,
) -> (ModulePage, ModuleHandle) {
    let program: Arc<dyn Program> = match flow {
        Some(flow) => Arc::new(Held(flow)),
        None => Arc::new(Unbounded),
    };
    // One way to the main queue for the module and for the images.
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
    let images = assets.map(|assets| {
        let wake = Arc::clone(&wake);
        Rc::new(Images::new(assets, ImageLimits::default(), move || wake()))
    });
    let module_wake = Arc::clone(&wake);
    let shaped = Rc::new(RefCell::new(Shaped::default()));
    let shared = Rc::new(RefCell::new(Shared {
        module: start(program, Box::new(move || module_wake())),
        images: images.clone(),
        diagnostics: Vec::new(),
        root: None,
        laid: None,
        laid_for: None,
        fresh: true,
        scrolls: HostScrolls::new(),
        asks: Vec::new(),
        animating: false,
        wake_at: None,
        asked: None,
        place: Placement { inset, zoom: 1.0 },
        outline: Outline::default(),
        on_outline: None,
        redraw: false,
        replies: Vec::new(),
        ended: None,
        told_end: false,
        sent: Vec::new(),
        editing: Editing::new(),
        measure_with: MeasureWith::Nothing,
        again: false,
        blink_at: None,
    }));
    let page = ModulePage {
        terminal: terminal.clone(),
        inset,
        shaped: Rc::clone(&shaped),
        images,
        shared: Rc::clone(&shared),
        motions: Motions::default(),
    };
    let handle = ModuleHandle {
        shared,
        shaped,
        terminal,
    };
    (page, handle)
}

/// The drive of a module's page: the pane's input in, what the module says out. Cloned freely;
/// all of it on the main thread.
#[derive(Clone)]
pub struct ModuleHandle {
    shared: Rc<RefCell<Shared>>,
    /// The lines the page's frames shaped, which measuring between frames finds too.
    shaped: Rc<RefCell<Shaped>>,
    /// The pane's font family: what `Family::Terminal` is.
    terminal: Option<String>,
}

impl ModuleHandle {
    /// The page's fields, in the pane's points, as of the last frame.
    pub fn fields(&self) -> Vec<Field> {
        self.shared.borrow().outline.fields.clone()
    }

    /// Does `run` to the page's editors, on the page as it was last laid out, measuring as its
    /// frames do; and tells the page and the pane what came of it — a frame where anything did, or
    /// always where `drawn` (a key, the input method). `None` (and nothing done) before the page is
    /// laid out or anything measures it.
    fn edit<T>(
        &self,
        drawn: bool,
        run: impl FnOnce(&mut Editors, &Laid, &mut dyn Measure, f64) -> T,
    ) -> Option<T> {
        let mut shared = self.shared.borrow_mut();
        let laid = shared.laid.clone()?;
        let now = shared.editing.now();
        let out = {
            let Shared {
                editing,
                measure_with,
                ..
            } = &mut *shared;
            let editors = &mut editing.editors;
            match measure_with {
                MeasureWith::Nothing => return None,
                MeasureWith::Renderer(renderer) => {
                    let mut shaped = self.shaped.try_borrow_mut().ok()?;
                    let mut typeset = Typeset {
                        renderer,
                        terminal: &self.terminal,
                        shaped: &mut shaped,
                        images: None,
                    };
                    run(editors, &laid, &mut typeset, now)
                }
                #[cfg(test)]
                MeasureWith::Even => run(editors, &laid, &mut super::editing::Even, now),
            }
        };
        let told = shared.editing.told();
        shared.tell(told, false, drawn);
        Some(out)
    }

    /// A pointer input (the page's points) over the page's editors: a left press in one is the
    /// editor's (it puts the caret, and the editor has the keyboard), a move with it held drags,
    /// the release ends it; a left press outside every editor's room takes the keyboard from the
    /// one that had it, back to the page, as a press outside a field does. The place under the
    /// pointer, where it is over an editor, and whether the page is to be told the keyboard left.
    fn pointer_in_editors(&self, input: &Input) -> (Option<Place>, bool) {
        if !matches!(
            input,
            Input::Press { .. } | Input::Move(_) | Input::Release { .. }
        ) {
            return (None, false);
        }
        let done = self.edit(false, |editors, laid, measure, _| {
            let mut left = false;
            let at = match *input {
                Input::Press {
                    at,
                    button: Button::Left,
                    mods,
                    count,
                } => {
                    let took = editors.press(laid, at, mods, count, measure);
                    if !took && editors.focused().is_some() && !in_a_room(laid, at) {
                        editors.set_focus(None);
                        left = true;
                    }
                    at
                }
                Input::Press { at, .. } => at,
                Input::Move(at) => {
                    editors.drag_to(laid, at, measure);
                    at
                }
                Input::Release { at, button } => {
                    if button == Button::Left {
                        editors.release();
                    }
                    at
                }
                Input::Scroll { .. }
                | Input::Leave
                | Input::Key { .. }
                | Input::KeyUp { .. }
                | Input::Mods(_) => return (None, false),
            };
            let place = editors
                .place_at(laid, at, measure)
                .map(|(editor, at)| Place { editor, at });
            (place, left)
        });
        if matches!(input, Input::Press { .. }) {
            self.shared.borrow_mut().editing.person_moved_keyboard();
        }
        done.unwrap_or((None, false))
    }
}

impl PageDrive for ModuleHandle {
    /// A key that does one of the commands the page can be told to do now is that command, and the
    /// page is told the command and not the key (the kit's specification, Section 4).
    fn send_input(&self, input: Input) {
        let mut shared = self.shared.borrow_mut();
        let place = shared.place;
        let (took, redraw) = shared.scrolls.pointer(&into_page(input, place));
        shared.redraw |= redraw;
        if took {
            shared.tell_scrolled();
            return;
        }
        if let Input::Key { key, mods } = input
            && let Some(command) = shared
                .laid
                .as_ref()
                .and_then(|laid| laid.command_for(key, mods))
        {
            shared.module.event(Event::Command(command));
            return;
        }
        let input = into_page(input, shared.place);
        drop(shared);
        // Over an editor, the pointer is the editor's as well, and the page is told the place
        // under it; after what the editor told (the keyboard it took, the caret it put).
        let (place, left) = self.pointer_in_editors(&input);
        let shared = self.shared.borrow();
        if left {
            shared.module.event(Event::Focus(None));
        }
        shared.module.event(match place {
            Some(place) => Event::Pointer { input, place },
            None => Event::Input(input),
        });
    }

    fn send_wheel(&self, at: Point, dx: f32, dy: f32, phase: Phase) {
        let mut shared = self.shared.borrow_mut();
        let place = shared.place;
        let (page_at, zoom) = (place.to_page(at), place.zoom);
        let (dx, dy) = (dx / zoom, dy / zoom);
        let left = match shared.laid.clone() {
            Some(laid) => {
                let wheel = shared.scrolls.wheel(&laid, page_at, dx, dy, phase);
                shared.redraw |= wheel.took;
                shared.tell_scrolled();
                wheel.left
            }
            None => Point::new(dx, dy),
        };
        // What no box took is the page's.
        if left.x != 0.0 || left.y != 0.0 {
            shared.module.event(Event::Input(Input::Scroll {
                at: page_at,
                dx: left.x,
                dy: left.y,
            }));
        }
    }

    fn set_scrolling(&self, scrolling: Scrolling) {
        let mut shared = self.shared.borrow_mut();
        shared.scrolls.set(scrolling);
        shared.redraw = true;
    }

    fn send_command(&self, id: Id) {
        self.shared.borrow().module.event(Event::Command(id));
    }

    fn send_files(&self, at: Point, paths: Vec<String>) {
        let shared = self.shared.borrow();
        let at = shared.place.to_page(at);
        shared.module.event(Event::Files { at, paths });
    }

    fn send_text(&self, field: Id, text: &str) {
        self.shared.borrow().module.event(Event::Text {
            field,
            text: text.to_owned(),
        });
    }

    fn send_focus(&self, field: Option<Id>) {
        let mut shared = self.shared.borrow_mut();
        // The person clicked into a field: the keyboard is not an editor's any more.
        if field.is_some() {
            shared.editing.editors.set_focus(None);
            shared.editing.person_moved_keyboard();
            shared.redraw = true;
        }
        shared.module.event(Event::Focus(field));
    }

    fn send_pick(&self, index: usize) {
        self.shared.borrow().module.event(Event::MenuPick(index));
    }

    fn send_message(&self, text: &str) {
        self.shared.borrow().module.message(text.to_owned());
    }

    fn send_room(&self) {
        self.shared.borrow().module.room();
    }

    fn watch_outline(&self, told: Box<dyn Fn(&Outline)>) {
        let (outline, told) = {
            let mut shared = self.shared.borrow_mut();
            let told: Rc<dyn Fn(&Outline)> = Rc::from(told);
            shared.on_outline = Some(Rc::clone(&told));
            (shared.outline.clone(), told)
        };
        told(&outline);
    }

    fn set_zoom(&self, zoom: f32) {
        let mut shared = self.shared.borrow_mut();
        if shared.place.zoom != zoom {
            shared.place.zoom = zoom;
            // Laid out again at the size it now has.
            shared.redraw = true;
        }
    }

    fn set_renderer(&self, renderer: Rc<Renderer>) {
        self.shared.borrow_mut().measure_with = MeasureWith::Renderer(renderer);
    }

    fn set_active(&self, active: bool) {
        let mut shared = self.shared.borrow_mut();
        let was = shared.editing.blink;
        shared.editing.blink.set_active(active);
        if shared.editing.blink != was && shared.editing.editors.focused().is_some() {
            shared.redraw = true;
        }
    }

    fn editors(&self) -> Option<PageEditors> {
        Some(PageEditors {
            handle: self.clone(),
        })
    }

    fn drain(&self, reply: &mut dyn FnMut(Reply, bool)) -> Drained {
        let (replies, drained) = {
            let mut shared = self.shared.borrow_mut();
            shared.sync(false);
            let ended = if shared.told_end {
                None
            } else {
                shared.ended.clone().inspect(|_| shared.told_end = true)
            };
            let drained = Drained {
                redraw: std::mem::take(&mut shared.redraw),
                diagnostics: std::mem::take(&mut shared.diagnostics),
                ended,
                sent: std::mem::take(&mut shared.sent),
            };
            (std::mem::take(&mut shared.replies), drained)
        };
        // After the borrow is let go of: the pane may take the page down in the middle.
        for (said, pointer) in replies {
            reply(said, pointer);
        }
        drained
    }
}

/// The editors of a module's page, as the pane's keys, input method, pasteboard and screen reader
/// reach them ([`PageDrive::editors`]). What is done is done to the editor with the keyboard, on the
/// page as it was last laid out, and what comes of it is told the page at once; the pane drains the
/// page after it ([`PageDrive::drain`]) and draws the frame that shows it. The ranges an input
/// method speaks of are in units of UTF-16, as macOS counts them; the points are the pane's.
#[derive(Clone)]
pub struct PageEditors {
    handle: ModuleHandle,
}

impl PageEditors {
    /// The editor with the keyboard.
    pub fn focused(&self) -> Option<Id> {
        self.handle.shared.borrow().editing.editors.focused()
    }

    /// A key pressed while an editor has the keyboard, and where it went (`Route`): the page's
    /// command for it first, then the editor's own, then the text. A command, or a key that is the
    /// page's, is told the page here, and what the editor does is done. The rest is the pane's:
    /// `Type`, the key to its system's handling of text (an input method, which says what is
    /// typed: [`PageEditors::type_text`], [`PageEditors::compose`]); `Copy` and `Cut`, the text to
    /// its pasteboard; `Paste`, its pasteboard's text to [`PageEditors::paste`]. `NotEditor`: no
    /// editor has the keyboard, or the page is not laid out yet.
    pub fn key(&self, key: Key, mods: Mods) -> Route {
        let route = self
            .handle
            .edit(true, |editors, laid, measure, now| {
                editors.key(laid, key, mods, measure, now)
            })
            .unwrap_or(Route::NotEditor);
        let shared = self.handle.shared.borrow();
        match route {
            Route::Command(id) => shared.module.event(Event::Command(id)),
            Route::Page => shared.module.event(Event::Input(Input::Key { key, mods })),
            _ => {}
        }
        route
    }

    /// Text typed, from the system's handling of text: in place of what an input method composed,
    /// where it did.
    pub fn type_text(&self, text: &str) {
        self.handle.edit(true, |editors, laid, measure, now| {
            editors.type_text(laid, text, measure, now);
        });
    }

    /// The pasteboard's text, pasted.
    pub fn paste(&self, text: &str) {
        self.handle.edit(true, |editors, laid, measure, now| {
            editors.paste(laid, text, measure, now);
        });
    }

    /// An input method composes `text`: the part of it selected (where it starts, how long), and
    /// the range of the document it replaces (`None`: what it composed before, else the selection).
    pub fn compose(&self, text: &str, selected: (usize, usize), replace: Option<(usize, usize)>) {
        self.handle.edit(true, |editors, laid, measure, now| {
            editors.set_marked_utf16(laid, text, selected, replace, measure, now);
        });
    }

    /// The input method is done: what it composed stays, as typed.
    pub fn unmark(&self) {
        self.handle.edit(true, |editors, laid, measure, _| {
            editors.unmark(laid, measure);
        });
    }

    /// The main selection of the editor with the keyboard: where it starts and how long it is.
    pub fn selected(&self) -> Option<(usize, usize)> {
        self.handle.shared.borrow().editing.editors.selected_utf16()
    }

    /// What the input method composes there, likewise.
    pub fn marked(&self) -> Option<(usize, usize)> {
        self.handle.shared.borrow().editing.editors.marked_utf16()
    }

    /// The text of a range of it (cut to the document).
    pub fn text(&self, start: usize, len: usize) -> Option<String> {
        self.handle
            .shared
            .borrow()
            .editing
            .editors
            .text_utf16(start, len)
    }

    /// How long its text is.
    pub fn length(&self) -> Option<usize> {
        let shared = self.handle.shared.borrow();
        let editors = &shared.editing.editors;
        let buffer = editors.buffer(editors.focused()?)?;
        buffer.utf16_of(buffer.len_chars())
    }

    /// The box, in the pane's points, of the place `units` into its text: where an input method
    /// opens its window.
    pub fn rect(&self, units: usize) -> Option<Rect> {
        let rect = self
            .handle
            .edit(false, |editors, laid, measure, _| {
                editors.rect_utf16(laid, units, measure)
            })
            .flatten()?;
        Some(self.handle.shared.borrow().place.to_pane(rect))
    }

    /// The place of its text under `at` (the pane's points), where `at` is over it.
    pub fn index_at(&self, at: Point) -> Option<usize> {
        let at = self.handle.shared.borrow().place.to_page(at);
        let (id, place) = self
            .handle
            .edit(false, |editors, laid, measure, _| {
                editors.place_at(laid, at, measure)
            })
            .flatten()?;
        let shared = self.handle.shared.borrow();
        let editors = &shared.editing.editors;
        if editors.focused() != Some(id) {
            return None;
        }
        editors.buffer(id)?.utf16_of(place)
    }

    /// What a screen reader is told of the editor named `id`, whether it has the keyboard or not:
    /// its text, and its main selection (where it starts, how long).
    pub fn contents(&self, id: Id) -> Option<(String, (usize, usize))> {
        let shared = self.handle.shared.borrow();
        let buffer = shared.editing.editors.buffer(id)?;
        let main = buffer.main();
        let start = buffer.utf16_of(main.start())?;
        let end = buffer.utf16_of(main.end())?;
        Some((buffer.text(), (start, end - start)))
    }
}

/// What stands where the page will be until the module has handed out its first.
fn starting_line() -> Element {
    div()
        .size_full()
        .items_center()
        .justify_center()
        .child(text("Starting the page\u{2026}").color(Role::Dim))
}

/// A module's page as a [`UiPage`]: built from the newest page the module has handed out.
struct ModulePage {
    terminal: Option<String>,
    inset: f32,
    /// The lines shaped so far; the handle measures with them between frames.
    shaped: Rc<RefCell<Shaped>>,
    /// The page's images; the handle polls the same ones ([`Shared::sync`]).
    images: Option<Rc<Images>>,
    shared: Rc<RefCell<Shared>>,
    /// Where the elements of the page that move were: the host moves them between the module's
    /// frames, laying the same page out again at each frame while one moves.
    motions: Motions,
}

/// How long a resized pane waits, at most, for the page of its new size: two frames at 60 Hz.
/// The wait ends as soon as that page comes (a module answers in a millisecond or two), and the
/// frame it is drawn into is presented with the resize itself, so a longer wait costs a slow
/// module's drag some smoothness and never a picture of the wrong size. Bounded, so a module
/// that does not answer cannot hold the window.
const RESIZE_WAIT: Duration = Duration::from_millis(32);

/// How many times a frame lays the page out again for its editors (a caret an overlay stands by
/// moved, an editor's content grew): the overlay stands by the caret after one. What is left after
/// the last is the next frame's.
const RELAYOUTS: usize = 2;

/// What a frame is asked for: the surface (pixels), its pixels to a point, the frame's time on the
/// link's clock, and whether Reduce Motion is on.
#[derive(Clone, Copy, Debug)]
struct FrameAt {
    size_px: (u32, u32),
    scale: f64,
    now: f64,
    reduce_motion: bool,
}

impl ModulePage {
    /// The page for the frame `at`, laid out and painted measuring with `measure`, its editors in
    /// their places: the draw list (pixels, from the top of the page's area, below the strip), and
    /// the outline to tell the host where it changed.
    fn frame(
        &mut self,
        at: FrameAt,
        measure: &mut dyn Measure,
    ) -> (DrawList, Option<(OnOutline, Outline)>) {
        let zoom = self.shared.borrow().place.zoom;
        let inset = (self.inset * at.scale as f32).round();
        let scale = at.scale as f32 * zoom;
        let width = at.size_px.0 as f32;
        let height = (at.size_px.1 as f32 - inset).max(0.0);
        let size = Size::new(width / scale, height / scale);
        let viewport = Size::new(width, height);
        let (root, cached) = {
            let mut shared = self.shared.borrow_mut();
            shared.again = false;
            shared.sync(true);
            // A page for this size, the next step of one that is moving and the page whose time
            // has come; a page that is none of these is not asked for.
            let due = shared.wake_at.is_some_and(|wake| at.now >= wake);
            let resized = shared.asked.is_some_and(|asked| asked != (size, scale));
            if shared.asked != Some((size, scale)) || shared.animating || due {
                shared.asked = Some((size, scale));
                shared.wake_at = None;
                shared.module.render(size, at.now, at.reduce_motion);
            }
            // A resized pane waits a little for the page made for its size rather than draw,
            // for a frame, the one laid out for the size it was: that frame is what flickers.
            // A page that comes meanwhile for another size (one asked before) does not end it.
            if resized && shared.module.wait_for_frame(size, RESIZE_WAIT) {
                shared.sync(true);
            }
            shared.scrolls.frame(at.now);
            let asks = std::mem::take(&mut shared.asks);
            shared.scrolls.ask(asks);
            // Only the scrolling changed: the boxes move on the page as it was laid out.
            let still =
                !shared.fresh && shared.laid_for == Some((size, scale)) && !self.motions.moving();
            let cached = shared.laid.clone().filter(|_| still);
            (shared.root.clone(), cached)
        };
        let starting = root.is_none();
        let mut laid = match cached {
            Some(laid) if !starting => {
                let mut laid = (*laid).clone();
                self.shared
                    .borrow_mut()
                    .scrolls
                    .scrolls
                    .place(&mut laid, at.now, at.reduce_motion);
                laid
            }
            _ => self.lay(root.clone(), viewport, scale, at, measure),
        };
        let mut told = Told::default();
        let mut caret = false;
        if !starting {
            // The editors made for the rooms there, and what the page asked of them done; where a
            // caret an overlay stands by moved (an edit, the scrolling), the same tree is laid out
            // again, the page asked nothing.
            for pass in 0..=RELAYOUTS {
                let again = {
                    let mut shared = self.shared.borrow_mut();
                    shared.editing.settle(&laid, measure);
                    shared.editing.editors.wants_layout() || laid.wants_layout()
                };
                if !again {
                    break;
                }
                if pass == RELAYOUTS {
                    // What is left is the next frame's, which lays the page out again.
                    self.shared.borrow_mut().again = true;
                    break;
                }
                laid = self.lay(root.clone(), viewport, scale, at, measure);
            }
            let mut shared = self.shared.borrow_mut();
            told = shared.editing.told();
            let shown = shared.editing.editors.focused().is_some();
            let (lit, next) = shared.editing.blink.at(at.now, at.reduce_motion, shown);
            caret = lit;
            shared.blink_at = next;
        }
        let laid = Arc::new(laid);
        let draws = laid.paint(measure);
        let draws = self
            .shared
            .borrow_mut()
            .editing
            .editors
            .paint(draws, measure, caret);
        let mut shared = self.shared.borrow_mut();
        // The module may ask of this layout from now on, and brings what depends on it back
        // within it; a page that was already within it answers nothing. The line that stands in
        // for a page is not the module's.
        if !starting {
            shared.module.lay_out(Arc::clone(&laid));
            shared.laid = Some(Arc::clone(&laid));
            shared.laid_for = Some((size, scale));
            shared.fresh = false;
            // Where the boxes went (a spring, a glide, an ask, a list that got shorter): the
            // page builds what came into view.
            shared.tell_scrolled();
            // And what the editors have for it, read against this layout.
            shared.tell(told, true, false);
        }
        let outline = Outline::of(&laid, shared.place);
        let changed = (outline != shared.outline)
            .then(|| {
                shared
                    .on_outline
                    .clone()
                    .map(|told| (told, outline.clone()))
            })
            .flatten();
        shared.outline = outline;
        (draws, changed)
    }

    /// `root` (the line that stands in for a page, where there is none yet) laid out where the pane
    /// has its boxes scrolled, measuring with the page's editors, and the boxes placed.
    fn lay(
        &mut self,
        root: Option<Element>,
        viewport: Size,
        scale: f32,
        at: FrameAt,
        measure: &mut dyn Measure,
    ) -> Laid {
        let starting = root.is_none();
        let mut root = root.unwrap_or_else(starting_line);
        let mut laid = {
            let shared = self.shared.borrow();
            shared.scrolls.scrolls.apply(&mut root);
            let mut measured = shared.editing.editors.measure(measure);
            self.motions.layout(
                root,
                viewport,
                scale,
                &mut measured,
                at.now,
                at.reduce_motion,
            )
        };
        if !starting {
            self.shared
                .borrow_mut()
                .scrolls
                .scrolls
                .place(&mut laid, at.now, at.reduce_motion);
        }
        laid
    }
}

impl UiPage for ModulePage {
    fn build(&mut self, cx: &UiContext<'_>) -> UiList {
        let inset = (self.inset * cx.scale as f32).round();
        let shaped = Rc::clone(&self.shaped);
        let mut shaped = shaped.borrow_mut();
        if shaped.lines.len() > super::SHAPED_LIMIT {
            shaped.lines.clear();
        }
        let (terminal, images) = (self.terminal.clone(), self.images.clone());
        let mut typeset = Typeset {
            renderer: cx.renderer,
            terminal: &terminal,
            shaped: &mut shaped,
            images: images.as_deref(),
        };
        let at = FrameAt {
            size_px: cx.size_px,
            scale: cx.scale,
            now: cx.now,
            reduce_motion: cx.reduce_motion,
        };
        let (draws, told) = self.frame(at, &mut typeset);
        let list = to_list(
            cx.renderer,
            cx.theme,
            cx.minimum_contrast,
            &mut typeset,
            &draws,
            (0.0, inset),
        );
        drop(shaped);
        // After the page is let go of: the host moves its fields in this call.
        if let Some((told, outline)) = told {
            told(&outline);
        }
        list
    }

    fn animating(&self) -> bool {
        let shared = self.shared.borrow();
        shared.animating || self.motions.moving() || shared.scrolls.moving() || shared.again
    }

    fn wake_at(&self) -> Option<f64> {
        let shared = self.shared.borrow();
        [shared.wake_at, shared.scrolls.wake_at(), shared.blink_at]
            .into_iter()
            .flatten()
            .reduce(f64::min)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_view::guest_dir;
    use bt_core::{MINIMUM_CONTRAST, Theme};
    use bt_gpu::{Renderer, UiItem};
    use pencere_view::Point;
    use pencere_view::{Button, Key, Mods, Rect};
    use std::sync::mpsc::{Receiver, channel};
    use std::time::{Duration, Instant};

    /// A file `make guest` leaves in [`guest_dir`] (the page modules of pencere's checkout, and the
    /// script of the container list). **A test that needs one fails where it is not there**: it
    /// is a heavy test, `make test-heavy` builds them first, and a test that went green by
    /// returning early would be a guard that guards nothing.
    fn guest_file(name: &str) -> Vec<u8> {
        let path = guest_dir().join(name);
        std::fs::read(&path).unwrap_or_else(|why| {
            panic!(
                "{} is not there ({why}): `make guest` builds the page modules from the pencere \
                 checkout the build is pinned to",
                path.display()
            )
        })
    }

    /// The container list's module, and a GPU device to draw on.
    fn rig() -> Option<(Renderer, Vec<u8>)> {
        let bytes = guest_file("containers.wasm");
        let Ok(renderer) = Renderer::system_default() else {
            eprintln!("skipped: no GPU device");
            return None;
        };
        Some((renderer, bytes))
    }

    fn build(page: &mut Box<dyn UiPage>, renderer: &Renderer, theme: &Theme, now: f64) -> UiList {
        page.build(&UiContext {
            renderer,
            size_px: (2000, 1200),
            scale: 2.0,
            theme,
            now,
            reduce_motion: false,
            minimum_contrast: MINIMUM_CONTRAST,
        })
    }

    fn wait(woken: &Receiver<()>) -> bool {
        woken.recv_timeout(Duration::from_secs(30)).is_ok()
    }

    fn boxes(list: &UiList) -> usize {
        list.items()
            .iter()
            .filter(|item| matches!(item, UiItem::Quad(_)))
            .count()
    }

    /// The module's page through the pane's page: empty until the module has handed one out,
    /// then the container list; the field's place is told; and a page at rest asks nothing of the
    /// module and wakes nobody.
    #[test]
    #[ignore = "heavy: needs the page modules of the pencere checkout; make test-heavy (make guest)"]
    fn a_modules_page_is_empty_until_it_arrives_then_drawn_and_then_at_rest() {
        let Some((renderer, bytes)) = rig() else {
            return;
        };
        let theme = Theme::BATERI;
        let (told, woken) = channel();
        let told = std::sync::Mutex::new(told);
        let (mut page, handle) = module_page(
            bytes,
            ModuleLimits::default(),
            None,
            32.0,
            None,
            None,
            move || {
                let _ = told.lock().unwrap().send(());
            },
        );
        // Each outline told: where its fields are, and what a screen reader says of it.
        type Told = Vec<(Vec<Field>, Vec<String>)>;
        let told: Rc<RefCell<Told>> = Rc::default();
        {
            let told = Rc::clone(&told);
            handle.watch_outline(Box::new(move |outline| {
                let words = outline.accessible.iter().map(|a| a.words.clone()).collect();
                told.borrow_mut().push((outline.fields.clone(), words));
            }));
        }

        // A line instead of a page yet, and the first frame does not wait for the module.
        let began = Instant::now();
        assert_eq!(boxes(&build(&mut page, &renderer, &theme, 0.0)), 0);
        assert!(began.elapsed() < Duration::from_secs(1));

        // The module's page comes: the application is woken, drains it, draws it.
        assert!(wait(&woken), "the module handed out no page");
        let drained = handle.drain(&mut |_, _| {});
        assert!(drained.redraw, "{drained:?}");
        assert_eq!(drained.ended, None);
        let list = build(&mut page, &renderer, &theme, 0.1);
        assert!(boxes(&list) > 100, "the rows and dots of the list");
        let field = handle
            .fields()
            .first()
            .expect("room for the filter's field")
            .rect;
        assert!(field.y >= 32.0, "below the strip: {field:?}");
        // The host is told where the fields are as they change: none before the page, the filter's
        // once it comes. In between, the line that stands for the page is read out, and has none.
        let fields_told = || {
            let mut places: Vec<Vec<Rect>> = told
                .borrow()
                .iter()
                .map(|(fields, _)| fields.iter().map(|f| f.rect).collect())
                .collect();
            places.dedup();
            places
        };
        assert_eq!(fields_told(), [Vec::new(), vec![field]]);
        assert!(
            told.borrow().iter().any(|(fields, words)| {
                fields.is_empty() && words.iter().any(|w| w == "Starting the page…")
            }),
            "the line that stands for the page is told to a screen reader: {:?}",
            told.borrow()
        );

        // The page that was laid out is settled, and nothing wakes the application for it.
        while woken.recv_timeout(Duration::from_millis(300)).is_ok() {
            let _ = handle.drain(&mut |_, _| {});
            build(&mut page, &renderer, &theme, 0.2);
        }
        assert!(!page.animating());
        for step in 0..3 {
            build(&mut page, &renderer, &theme, 1.0 + f64::from(step));
        }
        assert!(
            woken.recv_timeout(Duration::from_millis(300)).is_err(),
            "a page at rest woke the application"
        );
        assert_eq!(
            fields_told(),
            [Vec::new(), vec![field]],
            "the field's place is told once"
        );

        // The pointer over a row: the module answers, the page changes, the pane is told.
        handle.send_input(Input::Move(Point::new(300.0, 32.0 + 150.0)));
        assert!(wait(&woken));
        let mut replies = Vec::new();
        let drained = handle.drain(&mut |reply, pointer| replies.push((reply, pointer)));
        assert!(drained.redraw);
        assert_eq!(replies.len(), 1);
        assert!(replies[0].1, "a pointer move is the pointer's");
        assert!(replies[0].0.redraw);
    }

    /// The container list as a script, through the pane's page: the same as the module's —
    /// empty until the interpreter has made the page, then the list; a page at rest asks nothing
    /// of the script and wakes nobody; the pointer over a row changes it.
    #[test]
    #[ignore = "heavy: needs the page modules of the pencere checkout; make test-heavy (make guest)"]
    fn a_scripts_container_list_is_drawn_then_at_rest_and_answers_the_pointer() {
        let Ok(renderer) = Renderer::system_default() else {
            eprintln!("skipped: no GPU device");
            return;
        };
        let theme = Theme::BATERI;
        let (told, woken) = channel();
        let told = std::sync::Mutex::new(told);
        // The example script of pencere's checkout, copied beside the modules by `make guest`.
        let script = guest_file("containers.lua");
        let (mut page, handle) = script_page(
            Script {
                path: "page.lua".to_owned(),
                source: script,
                bundle: None,
            },
            ModuleLimits::default(),
            None,
            32.0,
            None,
            move || {
                let _ = told.lock().unwrap().send(());
            },
        );
        assert_eq!(boxes(&build(&mut page, &renderer, &theme, 0.0)), 0);
        assert!(wait(&woken), "the script handed out no page");
        let drained = handle.drain(&mut |_, _| {});
        assert!(drained.redraw, "{drained:?}");
        assert_eq!(drained.ended, None);
        let list = build(&mut page, &renderer, &theme, 0.1);
        assert!(boxes(&list) > 100, "the rows and dots of the list");
        let field = handle
            .fields()
            .first()
            .expect("room for the filter's field")
            .rect;
        assert!(field.y >= 32.0, "below the strip: {field:?}");

        // Laid out and settled, the page is at rest: nothing wakes the application for it.
        while woken.recv_timeout(Duration::from_millis(300)).is_ok() {
            let _ = handle.drain(&mut |_, _| {});
            build(&mut page, &renderer, &theme, 0.2);
        }
        assert!(!page.animating());
        for step in 0..3 {
            build(&mut page, &renderer, &theme, 1.0 + f64::from(step));
        }
        assert!(
            woken.recv_timeout(Duration::from_millis(300)).is_err(),
            "a page at rest woke the application"
        );

        handle.send_input(Input::Move(Point::new(300.0, 32.0 + 150.0)));
        assert!(wait(&woken));
        let mut replies = Vec::new();
        let drained = handle.drain(&mut |reply, pointer| replies.push((reply, pointer)));
        assert!(drained.redraw);
        assert_eq!(replies.len(), 1);
        assert!(replies[0].1 && replies[0].0.redraw);
    }

    /// The page of a module a program opened: it asks the program for what it shows, takes
    /// what the program pushes, redraws once for it, and a page at rest after that wakes nobody.
    #[test]
    #[ignore = "heavy: needs the page modules of the pencere checkout; make test-heavy (make guest)"]
    fn a_page_asks_its_program_for_readings_takes_them_and_is_at_rest_after() {
        let Some((renderer, bytes)) = rig() else {
            return;
        };
        let theme = Theme::BATERI;
        let (told, woken) = channel();
        let told = std::sync::Mutex::new(told);
        let (mut page, handle) = module_page(
            bytes,
            ModuleLimits::default(),
            None,
            32.0,
            None,
            None,
            move || {
                let _ = told.lock().unwrap().send(());
            },
        );
        build(&mut page, &renderer, &theme, 0.0);
        assert!(wait(&woken));
        let drained = handle.drain(&mut |_, _| {});
        // Said at its start, to a program that may be there: one message, a subscription.
        assert_eq!(
            drained.sent,
            [r#"{"subscribe":{"id":1,"topic":"stats"}}"#.to_owned()]
        );
        build(&mut page, &renderer, &theme, 0.1);
        while woken.recv_timeout(Duration::from_millis(300)).is_ok() {
            let _ = handle.drain(&mut |_, _| {});
            build(&mut page, &renderer, &theme, 0.2);
        }

        // The program pushes a reading: the page changes, once, and nothing is said back.
        let push = r#"{"push":{"id":1,"value":{"readings":[{"name":"api","cpu":42.5,"memory":"130MiB","share":0.3}]}}}"#;
        handle.send_message(push);
        assert!(wait(&woken), "a push did not reach the application");
        let drained = handle.drain(&mut |_, _| {});
        assert!(drained.redraw, "{drained:?}");
        assert!(drained.sent.is_empty());
        build(&mut page, &renderer, &theme, 0.3);
        while woken.recv_timeout(Duration::from_millis(300)).is_ok() {
            let _ = handle.drain(&mut |_, _| {});
            build(&mut page, &renderer, &theme, 0.4);
        }
        for step in 0..3 {
            build(&mut page, &renderer, &theme, 1.0 + f64::from(step));
        }
        assert!(
            woken.recv_timeout(Duration::from_millis(300)).is_err(),
            "a page at rest woke the application"
        );

        // A message that is not one of the program's, or is for nobody: nothing changes.
        for noise in [
            "not json",
            r#"{"push":{"id":9,"value":{}}}"#,
            r#"{"response":{"id":1,"result":{}}}"#,
        ] {
            handle.send_message(noise);
        }
        assert!(
            woken.recv_timeout(Duration::from_millis(300)).is_err(),
            "noise made the page change"
        );
    }

    /// What a module is held to in what it says is its program's flow: told *full* when the
    /// program has not read what it was told. The module is not closed for it.
    #[test]
    fn a_modules_program_is_the_flow_of_its_pane() {
        let flow = Flow::new();
        let held = Held(flow.clone());
        let mut said = 0;
        while held.may_say(64 << 10) {
            said += 1;
            assert!(said < 100, "never told full");
        }
        assert_eq!(said, bt_core::BACKLOG_LIMIT / (64 << 10));
        // What the module takes goes to the flow too (nothing was counted: it stays at zero).
        held.taken(100);
        assert_eq!(flow.inbox(), 0);
    }

    /// The limit of one message toward the program is the protocol's, held in one place.
    #[test]
    fn a_message_to_the_program_is_held_to_the_protocols_limit() {
        assert_eq!(
            ModuleLimits::default().message,
            bt_core::pencere::MAX_VIEW_MESSAGE
        );
    }

    /// A module that is not a page is closed with why, and the pane is told once.
    #[test]
    fn a_module_that_is_not_a_page_ends_with_a_reason_told_once() {
        let (told, woken) = channel();
        let told = std::sync::Mutex::new(told);
        let (_page, handle) = module_page(
            b"not a component".to_vec(),
            ModuleLimits::default(),
            None,
            32.0,
            None,
            None,
            move || {
                let _ = told.lock().unwrap().send(());
            },
        );
        assert!(wait(&woken));
        let drained = handle.drain(&mut |_, _| {});
        let why = drained.ended.expect("a reason");
        assert!(why.contains("loaded"), "{why}");
        assert_eq!(handle.drain(&mut |_, _| {}).ended, None);
    }

    /// Specification §5.4: a component written against another version of the interface than
    /// the one the terminal opens cannot run, and its page ends in words that name both versions
    /// — those words are what the pane hands to the session for the program to read.
    #[test]
    fn a_component_of_another_interface_ends_in_words_that_name_both_versions() {
        // `(component (instance $page) (export "pencere:view/page@0.5.0" (instance $page)))`,
        // in the binary form: an empty instance, exported under the name the interface had.
        let name = b"pencere:view/page@0.5.0";
        let mut old = vec![0x00, 0x61, 0x73, 0x6d, 0x0d, 0x00, 0x01, 0x00];
        old.extend_from_slice(&[0x05, 0x03, 0x01, 0x01, 0x00]);
        old.extend_from_slice(&[0x0b, 6 + u8::try_from(name.len()).unwrap(), 0x01, 0x00]);
        old.push(u8::try_from(name.len()).unwrap());
        old.extend_from_slice(name);
        old.extend_from_slice(&[0x05, 0x00, 0x00]);
        let (told, woken) = channel();
        let told = std::sync::Mutex::new(told);
        let (_page, handle) = module_page(
            old,
            ModuleLimits::default(),
            None,
            32.0,
            None,
            None,
            move || {
                let _ = told.lock().unwrap().send(());
            },
        );
        assert!(wait(&woken));
        let why = handle.drain(&mut |_, _| {}).ended.expect("a reason");
        assert!(
            why.contains("written against `pencere:view@0.5.0`"),
            "{why}"
        );
        assert!(why.contains("opens `pencere:view@2.0.0`"), "{why}");
    }

    /// A script that makes an error ends its page with the error and the line it is on, which
    /// is what the program is told, once.
    #[test]
    fn a_script_that_makes_an_error_ends_with_its_line_told_once() {
        let (told, woken) = channel();
        let told = std::sync::Mutex::new(told);
        let (_page, handle) = script_page(
            Script {
                path: "page.lua".to_owned(),
                source: b"local ok = 1\nerror('no page for you', 0)".to_vec(),
                bundle: None,
            },
            ModuleLimits::default(),
            None,
            32.0,
            None,
            move || {
                let _ = told.lock().unwrap().send(());
            },
        );
        assert!(wait(&woken));
        let drained = handle.drain(&mut |_, _| {});
        assert_eq!(drained.ended.as_deref(), Some("no page for you"));
        assert_eq!(handle.drain(&mut |_, _| {}).ended, None);

        let (told, woken) = channel();
        let told = std::sync::Mutex::new(told);
        let (_page, handle) = script_page(
            Script {
                path: "page.lua".to_owned(),
                source: b"local x = 1\nlocal y = = 2".to_vec(),
                bundle: None,
            },
            ModuleLimits::default(),
            None,
            32.0,
            None,
            move || {
                let _ = told.lock().unwrap().send(());
            },
        );
        assert!(wait(&woken));
        let why = handle.drain(&mut |_, _| {}).ended.expect("a reason");
        assert!(why.starts_with("page.lua:2:"), "{why}");
    }

    /// A script's page through the pane's page, like a module's: drawn once the interpreter has
    /// made it.
    #[test]
    fn a_scripts_page_is_drawn_through_the_pane() {
        let Ok(renderer) = Renderer::system_default() else {
            eprintln!("skipped: no GPU device");
            return;
        };
        let theme = Theme::BATERI;
        let (told, woken) = channel();
        let told = std::sync::Mutex::new(told);
        let (mut page, handle) = script_page(
            Script {
                path: "page.lua".to_owned(),
                source: br#"return { render = function()
                return pencere.div():bg("raised1"):p(8):child(pencere.text("hello")):child(
                    pencere.div():size(20, 20):bg("accent"))
            end }"#
                    .to_vec(),
                bundle: None,
            },
            ModuleLimits::default(),
            None,
            32.0,
            None,
            move || {
                let _ = told.lock().unwrap().send(());
            },
        );
        assert_eq!(boxes(&build(&mut page, &renderer, &theme, 0.0)), 0);
        assert!(wait(&woken), "the script handed out no page");
        let drained = handle.drain(&mut |_, _| {});
        assert!(drained.redraw && drained.ended.is_none(), "{drained:?}");
        let list = build(&mut page, &renderer, &theme, 0.1);
        assert!(boxes(&list) >= 2, "the two boxes of the page");
    }

    /// A key that does a command of the page is told to it as the command, and so is a command
    /// picked from the host's list; the page's commands are in its outline, and a figure it draws
    /// reaches the renderer as an image.
    #[test]
    fn a_key_that_does_a_command_is_the_command_and_a_canvas_is_an_image() {
        let Ok(renderer) = Renderer::system_default() else {
            eprintln!("skipped: no GPU device");
            return;
        };
        let theme = Theme::BATERI;
        let (told, woken) = channel();
        let told = std::sync::Mutex::new(told);
        let (mut page, handle) = script_page(
            Script {
                path: "page.lua".to_owned(),
                source: br#"local said = "nothing"
                return {
                    render = function()
                        return pencere.div()
                            :command("find", "Find", "cmd+f")
                            :child(pencere.text(said))
                            :child(pencere.canvas({ pencere.path():circle(10, 10, 8):fill("accent") }):size(20, 20))
                    end,
                    handle = function(event)
                        said = event.type .. " " .. tostring(event.id or event.key)
                        return true
                    end,
                }"#
                .to_vec(),
                bundle: None,
            },
            ModuleLimits::default(),
            None,
            32.0,
            None,
            move || {
                let _ = told.lock().unwrap().send(());
            },
        );
        let outline = Rc::new(RefCell::new(Outline::default()));
        let seen = Rc::clone(&outline);
        handle.watch_outline(Box::new(move |now| *seen.borrow_mut() = now.clone()));
        build(&mut page, &renderer, &theme, 0.0);
        assert!(wait(&woken), "the script handed out no page");
        handle.drain(&mut |_, _| {});
        let list = build(&mut page, &renderer, &theme, 0.1);
        assert_eq!(
            list.items()
                .iter()
                .filter(|item| matches!(item, UiItem::Image { .. }))
                .count(),
            1,
            "the canvas is drawn as an image"
        );
        let commands = outline.borrow().commands.clone();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].title, "Find");

        // What the page says after it is told something: the frame that follows its answer.
        let text_of = |handle: &ModuleHandle, was: &str| {
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                let _ = woken.recv_timeout(Duration::from_millis(200));
                handle.drain(&mut |_, _| {});
                let words = handle
                    .shared
                    .borrow()
                    .root
                    .as_ref()
                    .map(|root| {
                        root.fold()
                            .into_iter()
                            .find_map(|node| match node.kind {
                                pencere_view::wire::Kind::Text(words) => Some(words),
                                _ => None,
                            })
                            .unwrap_or_default()
                    })
                    .unwrap_or_default();
                if words != was || Instant::now() > deadline {
                    return words;
                }
            }
        };
        handle.send_input(Input::Key {
            key: pencere_view::Key::Character('f'),
            mods: pencere_view::Mods {
                command: true,
                ..pencere_view::Mods::default()
            },
        });
        assert_eq!(text_of(&handle, "nothing"), "command find");
        handle.send_input(Input::Key {
            key: pencere_view::Key::Character('g'),
            mods: pencere_view::Mods::default(),
        });
        assert_eq!(text_of(&handle, "command find"), "key g");
        handle.send_command(Id::named("find"));
        assert_eq!(text_of(&handle, "key g"), "command find");
    }

    /// A script's page, its handle, and what woke the application: measured by the tests'
    /// stand-in ([`super::editing::Even`]), frames and editors alike, so that no GPU is needed.
    fn editor_rig(source: &str) -> (ModulePage, ModuleHandle, Receiver<()>) {
        let (told, woken) = channel();
        let told = std::sync::Mutex::new(told);
        let script = Script {
            path: "page.lua".to_owned(),
            source: source.as_bytes().to_vec(),
            bundle: None,
        };
        let (page, handle) = parts(
            |program, wake| Module::from_script(script, ModuleLimits::default(), program, wake),
            None,
            32.0,
            None,
            None,
            move || {
                let _ = told.lock().unwrap().send(());
            },
        );
        handle.shared.borrow_mut().measure_with = MeasureWith::Even;
        (page, handle, woken)
    }

    /// A frame of 800 × 600 pixels at one pixel to a point, at `now`.
    fn frame_at(now: f64) -> FrameAt {
        FrameAt {
            size_px: (800, 600),
            scale: 1.0,
            now,
            reduce_motion: false,
        }
    }

    /// Frames and drains, as the pane does, until the page has nothing more to say: the last
    /// frame's list.
    fn settle(page: &mut ModulePage, handle: &ModuleHandle, woken: &Receiver<()>) -> DrawList {
        settle_at(page, handle, woken, frame_at(0.0))
    }

    /// The same, with the frames `at` asks for.
    fn settle_at(
        page: &mut ModulePage,
        handle: &ModuleHandle,
        woken: &Receiver<()>,
        at: FrameAt,
    ) -> DrawList {
        let mut list = page.frame(at, &mut super::super::editing::Even).0;
        for _ in 0..100 {
            let woke = woken.recv_timeout(Duration::from_millis(300)).is_ok();
            let drained = handle.drain(&mut |_, _| {});
            if !woke && !drained.redraw && !page.animating() {
                return list;
            }
            list = page.frame(at, &mut super::super::editing::Even).0;
        }
        panic!("the page never came to rest");
    }

    /// The words of the page as the module last made it.
    fn page_words(handle: &ModuleHandle) -> Vec<String> {
        handle
            .shared
            .borrow()
            .root
            .as_ref()
            .map(|root| {
                root.fold()
                    .into_iter()
                    .filter_map(|node| match node.kind {
                        pencere_view::wire::Kind::Text(words) => Some(words),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The words drawn in `list`.
    fn drawn(list: &DrawList) -> Vec<String> {
        list.items
            .iter()
            .filter_map(|item| match item {
                pencere_view::Paint::Text(run) => Some(run.text.clone()),
                _ => None,
            })
            .collect()
    }

    fn editor_text(handle: &ModuleHandle) -> String {
        handle
            .shared
            .borrow()
            .editing
            .editors
            .buffer(Id::named("e"))
            .map(pencere_editor::Buffer::text)
            .unwrap_or_default()
    }

    /// A page with an editor 200 points high below 40 of its own, and a line of what it was told:
    /// `l` loads the editor and gives it the keyboard; an edit is read back from the text the host
    /// posted; ⌘⏎ is a command of the page's.
    const EDITOR_PAGE: &str = r#"local said = {}
local function note(words) said[#said + 1] = words end
return {
    render = function()
        return pencere.div():size_full()
            :command("run", "Run", "cmd+enter")
            :child(pencere.div():h(40))
            :child(pencere.editor("e", { gutter = false }):h(200):w(400))
            :child(pencere.text(table.concat(said, "|")))
    end,
    handle = function(event)
        if event.type == "key" and event.key == "l" then
            note("load")
            return { redraw = true, keyboard = { field = "e" }, editor = { e = { load = "hello\nworld" } } }
        elseif event.type == "edit" then
            note("edit " .. event.origin .. " " .. pencere.layout.editor_text("e", 0, pencere.layout.editor_length("e")))
        elseif event.type == "focus" then
            note("focus " .. tostring(event.field))
        elseif event.type == "press" then
            note("press " .. (event.place and (event.place.editor .. "@" .. event.place.at) or "page"))
        elseif event.type == "command" then
            note("command " .. event.id)
        elseif event.type == "key" then
            note("key " .. event.key)
        end
        return true
    end,
}"#;

    /// The page's editor through the pane's page: a reply loads it and gives it the keyboard, the
    /// frame draws its lines in its room; the pane's keys and text reach it, the page reads the
    /// edit from the text posted before it is told; the page's command comes first.
    #[test]
    fn a_pages_editor_is_loaded_by_its_reply_drawn_and_typed_into() {
        let (mut page, handle, woken) = editor_rig(EDITOR_PAGE);
        // The first frame asks the script for its page.
        page.frame(frame_at(0.0), &mut super::super::editing::Even);
        assert!(wait(&woken), "the script handed out no page");
        settle(&mut page, &handle, &woken);
        let editors = handle.editors().expect("a module's page has editors");
        assert_eq!(editors.focused(), None);
        // `l` is the page's while no editor has the keyboard.
        handle.send_input(Input::Key {
            key: Key::Character('l'),
            mods: Mods::default(),
        });
        let list = settle(&mut page, &handle, &woken);
        assert_eq!(editor_text(&handle), "hello\nworld");
        assert_eq!(editors.focused(), Some(Id::named("e")));
        let words = drawn(&list);
        assert!(words.iter().any(|w| w == "hello"), "{words:?}");
        assert!(words.iter().any(|w| w == "world"), "{words:?}");
        assert!(
            list.items
                .iter()
                .all(|item| !matches!(item, pencere_view::Paint::Editor(_))),
            "every editor's place is filled"
        );
        // A character types (the pane's system says what), and the page reads it back.
        assert_eq!(
            editors.key(Key::Character('x'), Mods::default()),
            Route::Type
        );
        editors.type_text("x");
        settle(&mut page, &handle, &woken);
        assert_eq!(editor_text(&handle), "xhello\nworld");
        let said = page_words(&handle).join(" ");
        assert!(said.contains("edit person xhello\nworld"), "{said}");
        // ⌥ types what it makes (⌥Q on a Turkish keyboard).
        let alt = Mods {
            alt: true,
            ..Mods::default()
        };
        assert_eq!(editors.key(Key::Character('@'), alt), Route::Type);
        // The editor's own keys; the pasteboard is the pane's.
        let command = Mods {
            command: true,
            ..Mods::default()
        };
        assert_eq!(editors.key(Key::Character('a'), command), Route::Done);
        assert_eq!(
            editors.key(Key::Character('c'), command),
            Route::Copy("xhello\nworld".into())
        );
        assert_eq!(editors.key(Key::Character('v'), command), Route::Paste);
        editors.paste("pasted");
        settle(&mut page, &handle, &woken);
        assert_eq!(editor_text(&handle), "pasted");
        assert_eq!(editors.key(Key::Character('z'), command), Route::Done);
        settle(&mut page, &handle, &woken);
        assert_eq!(editor_text(&handle), "xhello\nworld");
        // The page's command for a key comes before the editor's.
        assert_eq!(
            editors.key(Key::Enter, command),
            Route::Command(Id::named("run"))
        );
        settle(&mut page, &handle, &woken);
        assert!(page_words(&handle).join(" ").contains("command run"));
        // Esc with one caret is the page's, told as a key.
        assert_eq!(editors.key(Key::Escape, Mods::default()), Route::Page);
        settle(&mut page, &handle, &woken);
        assert!(page_words(&handle).join(" ").contains("key escape"));
    }

    /// What an input method composes is in the document until it is done, and it asks where the
    /// caret is in the pane's points and what is around it, in units of UTF-16.
    #[test]
    fn an_input_method_composes_in_the_editor_and_is_answered_in_units_of_utf16() {
        let (mut page, handle, woken) = editor_rig(EDITOR_PAGE);
        // The first frame asks the script for its page.
        page.frame(frame_at(0.0), &mut super::super::editing::Even);
        assert!(wait(&woken), "the script handed out no page");
        settle(&mut page, &handle, &woken);
        handle.send_input(Input::Key {
            key: Key::Character('l'),
            mods: Mods::default(),
        });
        settle(&mut page, &handle, &woken);
        let editors = handle.editors().expect("editors");
        assert_eq!(editors.selected(), Some((0, 0)));
        editors.compose("k", (1, 0), None);
        assert_eq!(editors.marked(), Some((0, 1)));
        assert_eq!(editors.key(Key::Down, Mods::default()), Route::Type);
        editors.compose("か", (1, 0), None);
        editors.type_text("か");
        settle(&mut page, &handle, &woken);
        assert_eq!(editors.marked(), None);
        assert_eq!(editor_text(&handle), "かhello\nworld");
        assert_eq!(editors.selected(), Some((1, 0)));
        assert_eq!(editors.text(0, 3).as_deref(), Some("かhe"));
        assert_eq!(editors.length(), Some(12));
        // The caret's box: in the editor's room, below the strip and the page's own 40 points.
        let caret = editors.rect(1).expect("a caret");
        assert!(
            caret.y >= 32.0 + 40.0 && caret.y < 32.0 + 240.0,
            "{caret:?}"
        );
        assert!(caret.h > 0.0, "{caret:?}");
        // What is under a point of the pane, there.
        let under = Point::new(caret.x + 0.5, caret.y + caret.h / 2.0);
        assert_eq!(editors.index_at(under), Some(1));
        let (text, selected) = editors.contents(Id::named("e")).expect("contents");
        assert_eq!((text.as_str(), selected), ("かhello\nworld", (1, 0)));
    }

    /// A press in the editor is the editor's — it takes the keyboard and puts the caret — and the
    /// page hears it with the place under it; a press elsewhere on the page gives the keyboard back
    /// to the page.
    #[test]
    fn a_press_in_an_editor_puts_its_caret_and_one_elsewhere_takes_the_keyboard_back() {
        let (mut page, handle, woken) = editor_rig(EDITOR_PAGE);
        // The first frame asks the script for its page.
        page.frame(frame_at(0.0), &mut super::super::editing::Even);
        assert!(wait(&woken), "the script handed out no page");
        settle(&mut page, &handle, &woken);
        handle.send_input(Input::Key {
            key: Key::Character('l'),
            mods: Mods::default(),
        });
        settle(&mut page, &handle, &woken);
        handle.send_input(Input::Key {
            key: Key::Character('l'),
            mods: Mods::default(),
        });
        settle(&mut page, &handle, &woken);
        let editors = handle.editors().expect("editors");
        // The keyboard back to the page by a press below the editor, then into it by one on the
        // second line (the pane's points: the strip is 32 high, the room starts 40 below it).
        let press = |at: Point| {
            handle.send_input(Input::Press {
                at,
                button: Button::Left,
                mods: Mods::default(),
                count: 1,
            });
            handle.send_input(Input::Release {
                at,
                button: Button::Left,
            });
        };
        press(Point::new(500.0, 32.0 + 300.0));
        settle(&mut page, &handle, &woken);
        assert_eq!(editors.focused(), None);
        let said = page_words(&handle).join(" ");
        assert!(said.contains("focus nil|press page"), "{said}");
        let line = editors_line_height(&handle);
        press(Point::new(390.0, 32.0 + 40.0 + line * 1.5 + 4.0));
        settle(&mut page, &handle, &woken);
        assert_eq!(editors.focused(), Some(Id::named("e")));
        // At the end of the second line: "hello\n" is 6, "world" 5.
        assert_eq!(editors.selected(), Some((11, 0)));
        let said = page_words(&handle).join(" ");
        assert!(said.contains("focus e|press e@11"), "{said}");
    }

    /// A caret moved out of the room is brought into view by the pane's scrolling of the room, and
    /// the page is told the lines in view; the caret blinks on the frames' clock, which is all a
    /// page at rest with an editor that has the keyboard wakes for.
    #[test]
    fn the_caret_is_kept_in_view_by_the_rooms_scrolling_and_blinks_at_rest() {
        let long: String = (0..60).map(|n| format!("line {n}\n")).collect();
        let source = EDITOR_PAGE
            .replace(r#"load = "hello\nworld""#, &format!("load = {long:?}"))
            .replace(
                r#"elseif event.type == "command" then"#,
                r#"elseif event.type == "viewport" then
            note("viewport " .. event.first .. "-" .. event.last)
        elseif event.type == "command" then"#,
            );
        let (mut page, handle, woken) = editor_rig(&source);
        page.frame(frame_at(0.0), &mut super::super::editing::Even);
        assert!(wait(&woken), "the script handed out no page");
        settle(&mut page, &handle, &woken);
        handle.send_input(Input::Key {
            key: Key::Character('l'),
            mods: Mods::default(),
        });
        settle(&mut page, &handle, &woken);
        let editors = handle.editors().expect("editors");
        let command = Mods {
            command: true,
            ..Mods::default()
        };
        assert_eq!(editors.key(Key::Down, command), Route::Done);
        settle(&mut page, &handle, &woken);
        let offset = handle
            .shared
            .borrow()
            .laid
            .as_ref()
            .and_then(|laid| laid.scroll_extent(Id::named("e")))
            .map(|extent| extent.offset.y)
            .expect("the room scrolls");
        assert!(offset > 0.0, "the room is scrolled to the caret: {offset}");
        let said = page_words(&handle).join(" ");
        let told = said
            .rsplit("viewport ")
            .next()
            .and_then(|lines| lines.split(['-', '|']).nth(1))
            .and_then(|last| last.trim().parse::<usize>().ok());
        assert!(told.is_some_and(|last| last >= 59), "{said}");
        // At rest: no frame is asked for but the caret's next blink.
        assert!(!page.animating());
        assert_eq!(page.wake_at(), Some(0.5));
    }

    /// On a screen of two pixels to a point, the page drawn half as large again as its own size:
    /// the caret's box an input method is told and the place under a point are each other's, a
    /// press there puts the caret there, and a press below the room gives the keyboard back.
    #[test]
    fn an_editor_answers_in_the_panes_points_at_any_scale_and_zoom() {
        let (mut page, handle, woken) = editor_rig(EDITOR_PAGE);
        handle.set_zoom(1.5);
        let at = FrameAt {
            size_px: (1600, 1200),
            scale: 2.0,
            now: 0.0,
            reduce_motion: false,
        };
        page.frame(at, &mut super::super::editing::Even);
        assert!(wait(&woken), "the script handed out no page");
        settle_at(&mut page, &handle, &woken, at);
        handle.send_input(Input::Key {
            key: Key::Character('l'),
            mods: Mods::default(),
        });
        settle_at(&mut page, &handle, &woken, at);
        let editors = handle.editors().expect("editors");
        assert_eq!(editors.focused(), Some(Id::named("e")));
        let middle = |rect: Rect| Point::new(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
        for k in [0, 3, 6, 8, 11] {
            let caret = editors.rect(k).expect("a caret");
            assert!(
                caret.y >= 32.0 + 40.0 * 1.5,
                "below the strip and 40 points: {caret:?}"
            );
            assert_eq!(editors.index_at(middle(caret)), Some(k), "{caret:?}");
        }
        let press = |at: Point| {
            handle.send_input(Input::Press {
                at,
                button: Button::Left,
                mods: Mods::default(),
                count: 1,
            });
            handle.send_input(Input::Release {
                at,
                button: Button::Left,
            });
        };
        press(middle(editors.rect(8).expect("a caret")));
        settle_at(&mut page, &handle, &woken, at);
        assert_eq!(editors.selected(), Some((8, 0)));
        let room = {
            let shared = handle.shared.borrow();
            let laid = shared.laid.clone().expect("laid out");
            let room = laid.editors().into_iter().next().expect("the room").rect;
            shared.place.to_pane(room)
        };
        press(Point::new(room.x + 10.0, room.y + room.h + 5.0));
        settle_at(&mut page, &handle, &woken, at);
        assert_eq!(editors.focused(), None, "{room:?}");
    }

    /// An editor whose long lines wrap comes to rest like any page: its room's width is what its
    /// rows are, and nothing is laid out again for ever.
    #[test]
    fn an_editor_of_wrapped_lines_comes_to_rest() {
        let long: String = (0..40)
            .map(|n| format!("{n} {}\n", "word ".repeat(60)))
            .collect();
        let source = EDITOR_PAGE
            .replace("{ gutter = false }", "{ gutter = false, wrap = true }")
            .replace(r#"load = "hello\nworld""#, &format!("load = {long:?}"));
        let (mut page, handle, woken) = editor_rig(&source);
        page.frame(frame_at(0.0), &mut super::super::editing::Even);
        assert!(wait(&woken), "the script handed out no page");
        settle(&mut page, &handle, &woken);
        handle.send_input(Input::Key {
            key: Key::Character('l'),
            mods: Mods::default(),
        });
        settle(&mut page, &handle, &woken);
        assert!(editor_text(&handle).starts_with("0 word"));
        let extent = handle
            .shared
            .borrow()
            .laid
            .as_ref()
            .and_then(|laid| laid.scroll_extent(Id::named("e")))
            .expect("the room scrolls");
        assert!(extent.content.w <= extent.view.w + 0.5, "{extent:?}");
        assert!(extent.content.h > extent.view.h, "{extent:?}");
        for step in 0..3 {
            page.frame(
                frame_at(0.1 * f64::from(step)),
                &mut super::super::editing::Even,
            );
            assert!(!page.animating(), "frame {step} asked for another");
        }
    }

    /// The height of a line of the editor, points (one pixel to a point in these tests).
    fn editors_line_height(handle: &ModuleHandle) -> f32 {
        let shared = handle.shared.borrow();
        let laid = shared.laid.clone().expect("laid out");
        let room = laid.editors().into_iter().next().expect("the room");
        room.font.size * 1.25
    }

    /// The files of a bundle as a manifest types them: a path, a media type and the text.
    struct Typed(Vec<(&'static str, &'static str, &'static str)>);

    impl Assets for Typed {
        fn read(&self, path: &str) -> Option<Arc<[u8]>> {
            self.0
                .iter()
                .find(|(name, _, _)| *name == path)
                .map(|(_, _, text)| Arc::from(text.as_bytes()))
        }

        fn media_type(&self, path: &str) -> Option<String> {
            self.0
                .iter()
                .find(|(name, _, _)| *name == path)
                .map(|(_, media, _)| (*media).to_owned())
        }
    }

    const LUA: &str = "text/x-lua";

    /// The page of the script `page.lua` of a bundle of `files`, started.
    fn several_files(
        files: Vec<(&'static str, &'static str, &'static str)>,
    ) -> (Box<dyn UiPage>, ModuleHandle, Receiver<()>) {
        let page = files
            .iter()
            .find(|(path, _, _)| *path == "page.lua")
            .map(|(_, _, text)| text.as_bytes().to_vec())
            .expect("a page among the files");
        let (told, woken) = channel();
        let told = std::sync::Mutex::new(told);
        let (page, handle) = script_page(
            Script {
                path: "page.lua".to_owned(),
                source: page,
                bundle: Some(Arc::new(Typed(files)) as Arc<dyn Assets>),
            },
            ModuleLimits::default(),
            None,
            32.0,
            None,
            move || {
                let _ = told.lock().unwrap().send(());
            },
        );
        (page, handle, woken)
    }

    const SPLIT_PAGE: &str = "local box = require('./parts/box')\nreturn { render = function()\n  return pencere.div():bg('raised1'):p(8):child(pencere.text('two files')):child(box())\nend }";
    const SPLIT_BOX: &str = "return function() return pencere.div():size(20, 20):bg('accent') end";

    /// A script of several files through the pane's page, like one of a single file: the script
    /// it requires is read from the bundle the pane holds, and what it makes is drawn.
    #[test]
    fn a_scripts_page_of_several_files_is_drawn_through_the_pane() {
        let Ok(renderer) = Renderer::system_default() else {
            eprintln!("skipped: no GPU device");
            return;
        };
        let theme = Theme::BATERI;
        let (mut page, handle, woken) = several_files(vec![
            ("page.lua", LUA, SPLIT_PAGE),
            ("parts/box.lua", LUA, SPLIT_BOX),
        ]);
        assert_eq!(boxes(&build(&mut page, &renderer, &theme, 0.0)), 0);
        assert!(wait(&woken), "the script handed out no page");
        let drained = handle.drain(&mut |_, _| {});
        assert!(drained.redraw && drained.ended.is_none(), "{drained:?}");
        let list = build(&mut page, &renderer, &theme, 0.1);
        // The page's own box, and the small one the other file made.
        assert!(boxes(&list) >= 2, "the two boxes of the page");
    }

    /// What the page requires that the bundle does not have ends the page, with the path the
    /// page wrote and the line it wrote it on.
    #[test]
    fn a_script_that_requires_what_the_bundle_lacks_ends_with_the_path_and_the_line() {
        let (_page, handle, woken) = several_files(vec![(
            "page.lua",
            LUA,
            "local ok = 1\nrequire('./parts/missing')\nreturn {}",
        )]);
        assert!(wait(&woken));
        let why = handle.drain(&mut |_, _| {}).ended.expect("a reason");
        assert!(why.starts_with("page.lua:2:"), "{why}");
        assert!(why.contains("\"./parts/missing\""), "{why}");
    }

    /// An error in the script that is required says that script's file and line, and a file the
    /// bundle holds that its manifest does not call a script is not one to require.
    #[test]
    fn a_required_scripts_error_says_its_file_and_a_file_that_is_not_a_script_is_not_one() {
        let (_page, handle, woken) = several_files(vec![
            ("page.lua", LUA, "require('./parts/box')\nreturn {}"),
            ("parts/box.lua", LUA, "local x = nil\n\nreturn x.y"),
        ]);
        assert!(wait(&woken));
        let why = handle.drain(&mut |_, _| {}).ended.expect("a reason");
        assert!(why.starts_with("parts/box.lua:3:"), "{why}");

        let (_page, handle, woken) = several_files(vec![
            ("page.lua", LUA, "require('./parts/box')\nreturn {}"),
            ("parts/box.lua", "text/plain", "return {}"),
        ]);
        assert!(wait(&woken));
        let why = handle.drain(&mut |_, _| {}).ended.expect("a reason");
        assert!(
            why.contains("the bundle has no script at \"parts/box\""),
            "{why}"
        );
    }

    /// The page of a real pane: a program (`demopen --lua-files`) opens a view of two files in a
    /// session of the application, and the page is made from what the session holds — the
    /// script it requires is read through `BundleAssets`, with the type the program gave it.
    #[test]
    #[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
    fn a_script_of_several_files_from_a_real_session_is_drawn_through_the_pane() {
        use crate::child::{SilentWake, wait_until};
        use bt_core::pencere::{Action, ViewOptions};
        use bt_core::{CaretShape, CursorBlink, Osc52, Session, SessionOptions, TerminalOptions};
        use std::collections::HashMap;

        let program = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent()?.parent().map(std::path::Path::to_path_buf))
            .map(|profile| profile.join("examples").join("demopen"))
            .expect("the path of the test binary");
        assert!(
            program.is_file(),
            "the test program is not built: {} (`cargo build -p bt-core --example demopen`, which \
             `make test-heavy` does first)",
            program.display()
        );
        let mut views = ViewOptions::new("bateri-test", "0.0.0");
        views.native_entry_limit = Some(ModuleLimits::default().component as u64);
        let session = Arc::new(
            Session::spawn(
                SessionOptions {
                    command: Some((
                        "/bin/sh".to_owned(),
                        vec![
                            "-c".to_owned(),
                            "set -m; \"$1\" --lua-files".to_owned(),
                            "sh".to_owned(),
                            program.display().to_string(),
                        ],
                    )),
                    working_directory: None,
                    home: None,
                    env: HashMap::new(),
                    cols: 80,
                    rows: 10,
                    cell_px: (9, 18),
                    terminal: TerminalOptions {
                        scrollback: 100,
                        osc52: Osc52::Off,
                        cursor: CaretShape::default(),
                        blink: CursorBlink::default(),
                    },
                    theme: Theme::BATERI,
                    dock: false,
                    cluster: false,
                    initial_input: None,
                    shell_marks: false,
                    pane_uuid: None,
                    hostname: None,
                    replay: None,
                    views: Some(views),
                },
                Arc::new(SilentWake),
            )
            .expect("a session"),
        );
        let mut started = false;
        wait_until("the view to start", || {
            for action in session.take_view_actions() {
                started |= matches!(action, Action::StartView(_));
            }
            started
        });
        // What the pane does with the view it is shown (`TerminalPane::start_view`).
        let entry = session.view_entry().expect("an entry");
        let (_, bytes) = session.serve_view_file(&entry).expect("the entry is held");
        let assets: Arc<dyn Assets> = Arc::new(crate::native_view::BundleAssets::new(&session));
        let (told, woken) = channel();
        let told = std::sync::Mutex::new(told);
        let (mut page, handle) = script_page(
            Script {
                path: entry,
                source: bytes.to_vec(),
                bundle: Some(assets),
            },
            ModuleLimits::default(),
            None,
            32.0,
            Some(session.flow()),
            move || {
                let _ = told.lock().unwrap().send(());
            },
        );
        // Asked for a frame (a GPU, if the machine has one, draws it; the module makes the page
        // either way, from the two files).
        if let Ok(renderer) = Renderer::system_default() {
            let theme = Theme::BATERI;
            assert_eq!(boxes(&build(&mut page, &renderer, &theme, 0.0)), 0);
            assert!(wait(&woken), "the script handed out no page");
            let drained = handle.drain(&mut |_, _| {});
            assert!(drained.redraw && drained.ended.is_none(), "{drained:?}");
            assert!(boxes(&build(&mut page, &renderer, &theme, 0.1)) >= 2);
        } else {
            eprintln!("no GPU device: the page is not drawn, only started");
            let drained = handle.drain(&mut |_, _| {});
            assert_eq!(drained.ended, None);
        }
        session.shutdown();
    }

    /// A 2 × 2 PNG: red, green, blue, cream.
    const PNG: [u8; 84] = [
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02, 0x08, 0x06, 0x00, 0x00, 0x00, 0x72,
        0xb6, 0x0d, 0x24, 0x00, 0x00, 0x00, 0x1b, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x38,
        0xe1, 0xe0, 0xf0, 0xdf, 0x61, 0x41, 0xc0, 0x7f, 0x06, 0x87, 0x84, 0x0b, 0xff, 0x1f, 0x5c,
        0x98, 0xf0, 0x1f, 0x00, 0x51, 0xbc, 0x0a, 0x25, 0x0f, 0x57, 0x03, 0xad, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12"><rect width="12" height="12" fill="#c84040"/></svg>"##;

    /// The files of a bundle that are read only when the test lets them be: the images' worker
    /// waits at the gate, so the test knows what the page was before a file came and what it
    /// is after, and counts what the application was woken for in between.
    struct Gated {
        files: Vec<(&'static str, Vec<u8>)>,
        gate: std::sync::Mutex<Receiver<()>>,
        /// Told, from the worker, that a file is wanted and the worker is at the gate.
        asked: std::sync::Mutex<std::sync::mpsc::Sender<()>>,
        reads: std::sync::atomic::AtomicUsize,
    }

    impl Assets for Gated {
        fn read(&self, path: &str) -> Option<Arc<[u8]>> {
            self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let _ = self.asked.lock().unwrap().send(());
            let _ = self
                .gate
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(30));
            self.files
                .iter()
                .find(|(name, _)| *name == path)
                .map(|(_, bytes)| Arc::from(bytes.as_slice()))
        }

        fn media_type(&self, path: &str) -> Option<String> {
            self.files
                .iter()
                .any(|(name, _)| *name == path)
                .then(|| "application/octet-stream".to_owned())
        }
    }

    fn pictures(list: &UiList) -> usize {
        list.items()
            .iter()
            .filter(|item| matches!(item, UiItem::Image { .. }))
            .count()
    }

    /// What a page of a pane with a bundle is driven by in these tests.
    struct Rig {
        page: Box<dyn UiPage>,
        handle: ModuleHandle,
        /// What woke the application.
        woken: Receiver<()>,
        /// Lets the decoder have the file.
        open: std::sync::mpsc::Sender<()>,
        assets: Arc<Gated>,
    }

    /// A script that shows one file of its bundle, up in a pane, **with the decoder held at its
    /// gate**: the page has been laid out, the file asked for, and nothing more is to come until
    /// the test opens the gate. Returns what the page is until then.
    fn image_rig(
        renderer: &Renderer,
        theme: &Theme,
        file: &'static str,
        bytes: Vec<u8>,
    ) -> (Rig, UiList) {
        let (told, woken) = channel();
        let told = std::sync::Mutex::new(told);
        let (open, gate) = channel();
        let (asked, was_asked) = channel();
        let assets = Arc::new(Gated {
            files: vec![(file, bytes)],
            gate: std::sync::Mutex::new(gate),
            asked: std::sync::Mutex::new(asked),
            reads: 0.into(),
        });
        let script = format!(
            r#"return {{ render = function()
                return pencere.div():p(8):child(pencere.image("{file}"))
            end }}"#
        );
        let (mut page, handle) = script_page(
            Script {
                path: "page.lua".to_owned(),
                source: script.into_bytes(),
                bundle: Some(Arc::clone(&assets) as Arc<dyn Assets>),
            },
            ModuleLimits::default(),
            None,
            32.0,
            None,
            move || {
                let _ = told.lock().unwrap().send(());
            },
        );
        // The script's page comes, is laid out, and its image is asked for.
        build(&mut page, renderer, theme, 0.0);
        assert!(wait(&woken), "the script handed out no page");
        let _ = handle.drain(&mut |_, _| {});
        build(&mut page, renderer, theme, 0.1);
        assert!(
            was_asked.recv_timeout(Duration::from_secs(30)).is_ok(),
            "the page did not ask for its image"
        );
        // And what the script says after its page, if anything, is said before the test goes on.
        let mut list = build(&mut page, renderer, theme, 0.2);
        while woken.recv_timeout(Duration::from_millis(300)).is_ok() {
            let _ = handle.drain(&mut |_, _| {});
            list = build(&mut page, renderer, theme, 0.3);
        }
        (
            Rig {
                page,
                handle,
                woken,
                open,
                assets,
            },
            list,
        )
    }

    /// A decoded PNG wakes the application once, the drain says the page is to be drawn again,
    /// that draw has the picture, and after it the page at rest asks for and wakes nothing.
    #[test]
    fn a_decoded_image_wakes_the_page_once_and_a_page_at_rest_not_at_all() {
        let Ok(renderer) = Renderer::system_default() else {
            eprintln!("skipped: no GPU device");
            return;
        };
        let theme = Theme::BATERI;
        // The page is up and the file is asked for (the decoder is at its gate): a box with
        // nothing in it, and nothing more to come until the file does.
        let (
            Rig {
                mut page,
                handle,
                woken,
                open,
                assets,
            },
            list,
        ) = image_rig(&renderer, &theme, "dot.png", PNG.to_vec());
        assert_eq!(
            pictures(&list),
            0,
            "nothing to draw before the file is read"
        );
        assert_eq!(assets.reads.load(std::sync::atomic::Ordering::SeqCst), 1);

        // The file arrives: one wake for it, one drain that says the page is to be laid out and
        // drawn again, and that frame has the picture.
        open.send(()).unwrap();
        assert!(wait(&woken), "the decoded image woke nobody");
        let drained = handle.drain(&mut |_, _| {});
        assert!(drained.redraw, "{drained:?}");
        assert!(drained.diagnostics.is_empty(), "{drained:?}");
        let list = build(&mut page, &renderer, &theme, 0.4);
        assert_eq!(pictures(&list), 1, "the image is in the list");
        assert!(
            woken.recv_timeout(Duration::from_millis(300)).is_err(),
            "one image woke the application more than once"
        );

        // At rest: the frames that follow ask nothing, and the drain has nothing to say.
        for step in 0..3 {
            let list = build(&mut page, &renderer, &theme, 1.0 + f64::from(step));
            assert_eq!(pictures(&list), 1);
        }
        let drained = handle.drain(&mut |_, _| {});
        assert!(!drained.redraw, "{drained:?}");
        assert!(
            woken.recv_timeout(Duration::from_millis(300)).is_err(),
            "a page at rest woke the application"
        );
        assert_eq!(assets.reads.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    /// A frame that is built between the file's arrival and the drain of the wake that came with
    /// it is the frame that shows the picture; the drain after it has no other frame to ask for.
    #[test]
    fn a_frame_built_before_the_drain_shows_the_arrived_image_and_no_other_is_asked_for() {
        let Ok(renderer) = Renderer::system_default() else {
            eprintln!("skipped: no GPU device");
            return;
        };
        let theme = Theme::BATERI;
        let (
            Rig {
                mut page,
                handle,
                woken,
                open,
                ..
            },
            _,
        ) = image_rig(&renderer, &theme, "dot.png", PNG.to_vec());
        open.send(()).unwrap();
        assert!(wait(&woken), "the decoded image woke nobody");
        // The frame comes first, for a reason of its own (a pointer move, a tick).
        let list = build(&mut page, &renderer, &theme, 0.4);
        assert_eq!(pictures(&list), 1, "the frame has the picture");
        let drained = handle.drain(&mut |_, _| {});
        assert!(
            !drained.redraw,
            "the wake of an image a frame already drew asked for one more: {drained:?}"
        );
        assert!(woken.recv_timeout(Duration::from_millis(300)).is_err());
    }

    /// An SVG is two arrivals — its size, which lays the page out again, then its pixels at that
    /// size — and a page at rest after them wakes nobody.
    #[test]
    fn a_vector_image_wakes_the_page_for_its_size_then_for_its_pixels() {
        let Ok(renderer) = Renderer::system_default() else {
            eprintln!("skipped: no GPU device");
            return;
        };
        let theme = Theme::BATERI;
        let (
            Rig {
                mut page,
                handle,
                woken,
                open,
                ..
            },
            list,
        ) = image_rig(&renderer, &theme, "dot.svg", SVG.as_bytes().to_vec());
        assert_eq!(pictures(&list), 0);

        open.send(()).unwrap();
        // Its size: the page is laid out again, and the frame asks for the pixels at the size
        // the page gave it.
        assert!(wait(&woken), "the size of the image woke nobody");
        assert!(handle.drain(&mut |_, _| {}).redraw);
        let list = build(&mut page, &renderer, &theme, 0.4);
        assert_eq!(pictures(&list), 0, "no pixels at this size yet");
        // Its pixels.
        assert!(wait(&woken), "the pixels of the image woke nobody");
        assert!(handle.drain(&mut |_, _| {}).redraw);
        let list = build(&mut page, &renderer, &theme, 0.5);
        assert_eq!(pictures(&list), 1, "the image is in the list");
        assert!(
            woken.recv_timeout(Duration::from_millis(300)).is_err(),
            "the image woke the application a third time"
        );
        let drained = handle.drain(&mut |_, _| {});
        assert!(!drained.redraw, "{drained:?}");
    }

    /// A file the decoder refuses is a box with nothing in it and a word for the page's author,
    /// told once; the page goes on.
    #[test]
    fn an_image_that_is_not_one_is_left_out_with_a_word_for_the_author() {
        let Ok(renderer) = Renderer::system_default() else {
            eprintln!("skipped: no GPU device");
            return;
        };
        let theme = Theme::BATERI;
        let (
            Rig {
                mut page,
                handle,
                woken,
                open,
                ..
            },
            list,
        ) = image_rig(&renderer, &theme, "dot.png", b"not an image".to_vec());
        assert_eq!(pictures(&list), 0);
        open.send(()).unwrap();
        assert!(wait(&woken), "a refused image told nobody");
        let drained = handle.drain(&mut |_, _| {});
        assert_eq!(drained.diagnostics.len(), 1, "{drained:?}");
        assert!(drained.diagnostics[0].contains("dot.png"), "{drained:?}");
        assert_eq!(drained.ended, None);
        assert_eq!(pictures(&build(&mut page, &renderer, &theme, 0.4)), 0);
        assert!(handle.drain(&mut |_, _| {}).diagnostics.is_empty());
    }
}
