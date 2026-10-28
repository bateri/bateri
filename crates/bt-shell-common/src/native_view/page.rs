//! A page of the kit in a pane: pencere's drive of it ([`pencere_drive::Page`]), and what only the
//! pane can give it — the host's part of the drive (`pencere-drive`'s README, "What is the drive's
//! and what is the host's").
//!
//! The drive tells the page what the person does, lays its frames out with its editors and its
//! scrolling, and hands back what the page said in the order every host does it
//! ([`pencere_drive::Drained::deliver`]). What is the pane's is here, each part named by the duty it
//! is:
//!
//! - **The draw list in its renderer and theme** — [`PageDrawing`], the page as the link's
//!   [`UiPage`]: the drive's frame turned into the renderer's list ([`super::to_list`]: the theme's
//!   colours, the contrast floor, figures and images as textures).
//! - **Measuring text in its fonts** — [`PaneMeasure`], the renderer as the page's measure, in frames
//!   and between them (the object that measures is the object that draws), and the lines it shaped
//!   kept for the list.
//! - **The program, and the clock** — [`PaneProgram`]: the program's flow (what it can take of what
//!   the page says) and the pane's clock, the frames' own (the base of the link's tick stamps), so
//!   that the page's time between frames and its frames' times are one clock.
//! - **Its keyboard and its input method** — [`PageEditors`]: AppKit's text input comes back into the
//!   pane between any two of its calls, so the pane reaches the page's editors through a handle
//!   that borrows the page for each call alone.
//! - **Its fields and its screen reader** — [`PageHandle::watch_outline`]: the outline a frame says
//!   changed, told after the page is let go of, since the pane moves its fields in that call.
//! - **What a reply asked of a field its own frame brings** — [`PageHandle::watch_held`]: a find bar
//!   opened, filled and given the keyboard in one reply has no field until the frame that lays it out
//!   has been drawn and the outline told, so the drive holds the texts and the keyboard
//!   ([`pencere_drive::Painted::held`]) and the pane takes them right after the outline.
//! - **The time of day, and the keyboard** — [`PaneProgram`]: the person's offset from UTC, which the
//!   sandbox cannot know, and the system's keyboard convention.
//!
//! The pane's pasteboard, pointer, menus, browser and the session it speaks through are
//! `bt-shell-macos`'s (its `pencere_drive::Host`); the clock that wakes a page whose pane is not seen
//! is its own too (`Page::wake_unseen`).

use std::cell::{RefCell, RefMut};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use bt_core::Flow;
use bt_gpu::{Renderer, UiContext, UiList, UiPage};
use pencere_drive::{FrameAt, Outline, Page, Painted, Placement, Route, Setup};
use pencere_sandbox::{Assets, ModuleLimits, Program, Script};
use pencere_view::{Font, Id, Key, Measure, Metrics, Mods, Point, Rect, WallTime};

use super::containers::Demo;
use super::{SHAPED_LIMIT, Shaped, Typeset, to_list};

/// What a pane opens a page of the kit with, whatever the page is.
pub struct Opening {
    /// The pane's font family: what the page's `Family::Terminal` is (`None`: the default chain).
    pub terminal: Option<String>,
    /// The points at the top the application keeps for its own strip over the page, which the page
    /// never draws into.
    pub inset: f32,
    /// The flow of the program that opened the page, if one did: what the page says is held to the
    /// room the program has, and what it takes of the program's words lets the program say more. A
    /// page no program opened (a debug build's demo) has none.
    pub flow: Option<Flow>,
    /// The pane's clock, read from any thread: the frames' clock, the base of the link's tick stamps
    /// (`CACurrentMediaTime` on macOS).
    pub clock: fn() -> f64,
    /// What measures and draws the page's text.
    pub renderer: Rc<Renderer>,
}

/// The page of a component (`bytes`, binary or text) held to `limits`, as the link's [`UiPage`], and
/// the handle the pane drives it by. `assets` is where the files of its bundle are read from, if it
/// has one: a page without them draws no image. `wake` is called from the module's thread — and
/// from the images' — when something is put out; it hops to the main thread and has the pane drain
/// the page, **and never lays out or draws itself**: the images' decoder may call it from inside a
/// build. Returns at once: the module is compiled and started on its thread.
pub fn module_page(
    bytes: Vec<u8>,
    limits: ModuleLimits,
    assets: Option<Arc<dyn Assets>>,
    opening: Opening,
    wake: impl Fn() + Send + Sync + 'static,
) -> (Box<dyn UiPage>, PageHandle) {
    let (setup, drawing) = opening.setup(limits, assets);
    drawing.with(Page::component(bytes, setup, wake))
}

/// The page of a script, as [`module_page`]: the interpreter of the script's language as the module
/// and `script` as what it runs (its text, its path in its bundle, and the bundle, which is where its
/// images come from too).
pub fn script_page(
    script: Script,
    limits: ModuleLimits,
    opening: Opening,
    wake: impl Fn() + Send + Sync + 'static,
) -> (Box<dyn UiPage>, PageHandle) {
    let (setup, drawing) = opening.setup(limits, None);
    drawing.with(Page::script(script, setup, wake))
}

/// The container list of a debug build's demo ([`Demo`]), a page written in Rust run as a module is
/// ([`Page::spawn`]): the same drive, the same answers on the module's thread, as the page a program
/// sends. Otherwise as [`module_page`].
pub fn containers_demo(
    opening: Opening,
    wake: impl Fn() + Send + Sync + 'static,
) -> (Box<dyn UiPage>, PageHandle) {
    let (setup, drawing) = opening.setup(ModuleLimits::default(), None);
    drawing.with(Page::spawn(|| Ok(Demo::new()), setup, wake))
}

impl Opening {
    /// What the drive is given and what the link draws with: the measure and the list share the
    /// lines shaped.
    fn setup(self, limits: ModuleLimits, assets: Option<Arc<dyn Assets>>) -> (Setup, Drawing) {
        let shaped = Rc::new(RefCell::new(Shaped::default()));
        let measure = PaneMeasure {
            renderer: Rc::clone(&self.renderer),
            terminal: self.terminal.clone(),
            shaped: Rc::clone(&shaped),
        };
        let setup = setup(
            limits,
            self.inset,
            PaneProgram {
                flow: self.flow,
                clock: Some(self.clock),
            },
            assets,
            Box::new(measure),
        );
        let drawing = Drawing {
            terminal: self.terminal,
            shaped,
        };
        (setup, drawing)
    }
}

/// What a page in a pane is given, besides what measures it: what it is held to, its program and
/// clock, its files, its place below the strip, and the terminal's caret's blink. How its boxes
/// scroll is the pane's to say once it is up (`Page::set_scrolling`).
fn setup(
    limits: ModuleLimits,
    inset: f32,
    program: PaneProgram,
    assets: Option<Arc<dyn Assets>>,
    measure: Box<dyn Measure>,
) -> Setup {
    Setup {
        limits,
        program: Arc::new(program),
        assets,
        placement: Placement { inset, zoom: 1.0 },
        blink: bt_core::CURSOR_BLINK_INTERVAL,
        measure,
        ..Setup::default()
    }
}

/// What the link draws a page with, besides the page.
struct Drawing {
    terminal: Option<String>,
    shaped: Rc<RefCell<Shaped>>,
}

impl Drawing {
    fn with(self, page: Page) -> (Box<dyn UiPage>, PageHandle) {
        let handle = PageHandle::new(page);
        let drawing = PageDrawing {
            handle: handle.clone(),
            terminal: self.terminal,
            shaped: self.shaped,
        };
        (Box::new(drawing), handle)
    }
}

/// The program a page in a pane speaks to, as far as the page's speed goes, and the pane's clock
/// (`Setup::program`): the flow says whether the program has the room for what the page says, and
/// is told what the page takes of what the program says; a page no program opened has whatever room
/// it wants. The clock is the frames' own, so that what the person does between frames and the
/// frames themselves are on one clock (the drive's rule: a debug build stops on two).
struct PaneProgram {
    flow: Option<Flow>,
    clock: Option<fn() -> f64>,
}

impl Program for PaneProgram {
    fn may_say(&self, bytes: usize) -> bool {
        self.flow.as_ref().is_none_or(|flow| flow.may_say(bytes))
    }

    fn taken(&self, bytes: usize) {
        if let Some(flow) = &self.flow {
            flow.taken(bytes);
        }
    }

    fn now(&self) -> Option<f64> {
        self.clock.map(|clock| clock())
    }

    /// The time of day with the person's offset from UTC as it is now (daylight saving time
    /// included): the sandbox knows no time zone, and a page that shows the time of day shows the
    /// person's.
    fn wall_clock(&self) -> WallTime {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0.0, |since| since.as_secs_f64());
        WallTime {
            seconds,
            utc_offset: local_utc_offset(seconds),
        }
    }

    // `keyboard` is the sandbox's own answer: the system this is built for (a Mac's on macOS, which
    // is the only shell there is; `bt-shell-linux` will be Linux's), `a_pages_keyboard_is_the_systems`.
}

/// How far the person's local time is ahead of UTC at `unix` seconds, in seconds (the system's
/// time zone rules, so daylight saving time is in it); 0 where the system cannot say.
fn local_utc_offset(unix: f64) -> i32 {
    #[allow(clippy::cast_possible_truncation)]
    let time = unix.floor() as libc::time_t;
    // SAFETY: `localtime_r` writes only the `tm` it is given, and reads the time it is given and
    // the system's zone rules, which it guards itself. A zeroed `tm` is a valid one to fill.
    let mut broken: libc::tm = unsafe { std::mem::zeroed() };
    let filled = unsafe { libc::localtime_r(&raw const time, &raw mut broken) };
    if filled.is_null() {
        0
    } else {
        i32::try_from(broken.tm_gmtoff).unwrap_or(0)
    }
}

/// The renderer as a page's measure, in frames and between them: it opens and shapes through the
/// renderer and keeps the lines it shaped, which the link's list draws ([`PageDrawing`]) without
/// shaping them again.
struct PaneMeasure {
    renderer: Rc<Renderer>,
    terminal: Option<String>,
    shaped: Rc<RefCell<Shaped>>,
}

impl PaneMeasure {
    fn typeset<T>(&self, run: impl FnOnce(&mut Typeset<'_>) -> T) -> T {
        let mut shaped = self.shaped.borrow_mut();
        run(&mut Typeset {
            renderer: &self.renderer,
            terminal: &self.terminal,
            shaped: &mut shaped,
        })
    }
}

impl Measure for PaneMeasure {
    fn measure(&mut self, font: &Font, text: &str) -> Metrics {
        self.typeset(|typeset| typeset.measure(font, text))
    }

    fn offsets(&mut self, font: &Font, text: &str, at: &[usize]) -> Vec<f32> {
        self.typeset(|typeset| typeset.offsets(font, text, at))
    }
}

/// A page as the link's [`UiPage`]: each frame the link asks for is the drive's
/// ([`Page::frame`]), turned into the renderer's list in the pane's theme.
struct PageDrawing {
    handle: PageHandle,
    terminal: Option<String>,
    shaped: Rc<RefCell<Shaped>>,
}

impl UiPage for PageDrawing {
    fn build(&mut self, cx: &UiContext<'_>) -> UiList {
        // A page whose words keep changing does not collect every line it ever said: the cache
        // starts again, before the frame measures with it.
        {
            let mut shaped = self.shaped.borrow_mut();
            if shaped.lines.len() > SHAPED_LIMIT {
                shaped.lines.clear();
            }
        }
        let (list, mut painted) = {
            let mut page = self.handle.page();
            let painted = page.frame(FrameAt {
                size: cx.size_px,
                scale: cx.scale,
                now: cx.now,
                reduce_motion: cx.reduce_motion,
            });
            #[allow(clippy::cast_possible_truncation)]
            let inset = (page.placement().inset * cx.scale as f32).round();
            let mut shaped = self.shaped.borrow_mut();
            let mut typeset = Typeset {
                renderer: cx.renderer,
                terminal: &self.terminal,
                shaped: &mut shaped,
            };
            let list = to_list(
                cx.theme,
                cx.minimum_contrast,
                &mut typeset,
                page.images(),
                &painted.list,
                (0.0, inset),
            );
            (list, painted)
        };
        // After the page is let go of: the pane moves its fields in this call, and that may come
        // back to the page (the keyboard put in a field). The one place a frame's fields are
        // applied, and right after it what the drive held for the fields a reply opened
        // (`Painted::deliver_held`) is given to the pane, once its fields exist. Not only when the
        // outline changed: what is held can wait for a frame whose outline is the one the pane has.
        self.handle.tell_frame(&mut painted);
        list
    }

    fn animating(&self) -> bool {
        self.handle.page.borrow().animating()
    }

    fn wake_at(&self) -> Option<f64> {
        self.handle.page.borrow().wake_at()
    }
}

/// What is called when a page's outline changes ([`PageHandle::watch_outline`]).
type OnOutline = Rc<dyn Fn(&Outline)>;

/// What is called with what a frame brings besides its outline ([`PageHandle::watch_held`]).
type OnHeld = Rc<dyn Fn(&mut Painted)>;

/// The pane's handle on a page of the kit: the drive's page, which the link's frames of it share.
/// Cloned freely; all of it on the main thread.
#[derive(Clone)]
pub struct PageHandle {
    page: Rc<RefCell<Page>>,
    on_outline: Rc<RefCell<Option<OnOutline>>>,
    on_held: Rc<RefCell<Option<OnHeld>>>,
}

impl PageHandle {
    fn new(page: Page) -> Self {
        Self {
            page: Rc::new(RefCell::new(page)),
            on_outline: Rc::new(RefCell::new(None)),
            on_held: Rc::new(RefCell::new(None)),
        }
    }

    /// The drive's page, for one call: what the pane tells it (`input`, `wheel`, `text`, `focus`,
    /// `pick`, `command`, `files`, `message`, `room`), how it is to be (`set_zoom`,
    /// `set_scrolling`, `set_active`), its wakes while the pane is not seen (`wake_unseen`) and what
    /// it said (`drain`, done after the page is let go of: `Drained::deliver` calls back into the
    /// pane, which may call the page again).
    pub fn page(&self) -> RefMut<'_, Page> {
        self.page.borrow_mut()
    }

    /// `told` is called, on the thread that builds the frame, when the page's outline changes (the
    /// pane resized, the page laid out another way, a field or a label came or went): the pane puts
    /// its fields where the page leaves room for them and tells the screen reader. Called once with
    /// the outline the page has now.
    pub fn watch_outline(&self, told: Box<dyn Fn(&Outline)>) {
        let told: OnOutline = Rc::from(told);
        self.on_outline.replace(Some(Rc::clone(&told)));
        let outline = self.page.borrow().outline().clone();
        told(&outline);
    }

    fn tell_outline(&self, outline: &Outline) {
        let told = self.on_outline.borrow().clone();
        if let Some(told) = told {
            told(outline);
        }
    }

    /// `told` is called, on the thread that builds the frame, with the frame ([`Painted`]) right after
    /// its outline was told, so that the pane has the fields, when replies asked something of fields
    /// that frame lays out (their texts and the keyboard): `told` hands it to the pane's host with
    /// [`Painted::deliver_held`]. The page is not borrowed.
    pub fn watch_held(&self, told: Box<dyn Fn(&mut Painted)>) {
        self.on_held.replace(Some(Rc::from(told)));
    }

    /// A frame's outline if it changed, then what the drive held for its fields: the order a pane
    /// needs, for a field is the pane's only once the outline was told. The page is not borrowed.
    fn tell_frame(&self, painted: &mut Painted) {
        if let Some(outline) = painted.outline.take() {
            self.tell_outline(&outline);
        }
        if painted.held.is_empty() {
            return;
        }
        let told = self.on_held.borrow().clone();
        if let Some(told) = told {
            told(painted);
        }
    }

    /// The page's editors, as the pane's keys, input method, pasteboard and screen reader reach
    /// them.
    pub fn editors(&self) -> PageEditors {
        PageEditors {
            page: Rc::clone(&self.page),
        }
    }
}

/// The editors of a page in a pane, as the pane's keys, input method, pasteboard and screen reader
/// reach them ([`pencere_drive::PageEditors`]): each call borrows the page for itself alone, since
/// AppKit's text input comes back into the pane between any two of them. The ranges an input method
/// speaks of are in units of UTF-16, as macOS counts them; the points are the pane's.
#[derive(Clone)]
pub struct PageEditors {
    page: Rc<RefCell<Page>>,
}

impl PageEditors {
    /// The editor with the keyboard.
    pub fn focused(&self) -> Option<Id> {
        self.page.borrow().focused_editor()
    }

    /// A key pressed while an editor has the keyboard, and where it went ([`Route`]): the page's
    /// command for it first, then the editor's own, then the text — the system's to type, the
    /// pasteboard's to copy, cut or paste.
    pub fn key(&self, key: Key, mods: Mods) -> Route {
        self.page.borrow_mut().editors().key(key, mods)
    }

    /// Text typed, from the system's handling of text: in place of what an input method composed,
    /// where it did.
    pub fn type_text(&self, text: &str) {
        self.page.borrow_mut().editors().type_text(text);
    }

    /// The pasteboard's text, pasted.
    pub fn paste(&self, text: &str) {
        self.page.borrow_mut().editors().paste(text);
    }

    /// An input method composes `text`: the part of it selected (where it starts, how long), and
    /// the range of the document it replaces (`None`: what it composed before, else the selection).
    pub fn compose(&self, text: &str, selected: (usize, usize), replace: Option<(usize, usize)>) {
        self.page
            .borrow_mut()
            .editors()
            .compose(text, selected, replace);
    }

    /// The input method is done: what it composed stays, as typed.
    pub fn unmark(&self) {
        self.page.borrow_mut().editors().unmark();
    }

    /// The main selection of the editor with the keyboard: where it starts and how long it is.
    pub fn selected(&self) -> Option<(usize, usize)> {
        self.page.borrow_mut().editors().selected()
    }

    /// What the input method composes there, likewise.
    pub fn marked(&self) -> Option<(usize, usize)> {
        self.page.borrow_mut().editors().marked()
    }

    /// The text of a range of it (cut to the document).
    pub fn text(&self, start: usize, len: usize) -> Option<String> {
        self.page.borrow_mut().editors().text(start, len)
    }

    /// How long its text is.
    pub fn length(&self) -> Option<usize> {
        self.page.borrow_mut().editors().length()
    }

    /// The box, in the pane's points, of the place `units` into its text: where an input method
    /// opens its window.
    pub fn rect(&self, units: usize) -> Option<Rect> {
        self.page.borrow_mut().editors().rect(units)
    }

    /// The place of its text under `at` (the pane's points), where `at` is over it.
    pub fn index_at(&self, at: Point) -> Option<usize> {
        self.page.borrow_mut().editors().index_at(at)
    }

    /// What a screen reader is told of the editor named `id`, whether it has the keyboard or not:
    /// its text, and its main selection (where it starts, how long).
    pub fn contents(&self, id: Id) -> Option<(String, (usize, usize))> {
        self.page.borrow_mut().editors().contents(id)
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::native_view::pane_tests::Transcript;
    use bt_core::{MINIMUM_CONTRAST, Theme};
    use bt_gpu::UiItem;
    use pencere_drive::Host;
    use pencere_sandbox::Fault;
    use pencere_view::{
        Cursor, DrawList, Fill, Input, KeyConvention, Keyboard, Menu, Paint, Phase,
    };
    use std::sync::mpsc::{Receiver, channel};
    use std::time::Duration;

    /// Text measured with no renderer: each character half its font's size across, a line a
    /// quarter again as high as the font.
    pub(in crate::native_view) struct Even;

    impl Measure for Even {
        fn measure(&mut self, font: &Font, text: &str) -> Metrics {
            #[allow(clippy::cast_precision_loss)]
            let width = text.chars().count() as f32 * font.size * 0.5;
            Metrics {
                width,
                height: font.size * 1.25,
            }
        }
    }

    /// The words drawn in `list`.
    fn drawn(list: &DrawList) -> Vec<String> {
        list.items
            .iter()
            .filter_map(|item| match item {
                Paint::Text(run) => Some(run.text.clone()),
                _ => None,
            })
            .collect()
    }

    /// What the pane does with what a page said (its `pencere_drive::Host`), written down.
    struct Written(Rc<RefCell<Transcript>>);

    impl Host for Written {
        fn send(&mut self, message: String) {
            self.0.borrow_mut().send(&message);
        }
        fn close(&mut self) {
            self.0.borrow_mut().close();
        }
        fn redraw(&mut self) {
            self.0.borrow_mut().redraw();
        }
        fn cursor(&mut self, cursor: Cursor) {
            self.0.borrow_mut().cursor(cursor);
        }
        fn fill(&mut self, field: Id, fill: Fill) {
            self.0.borrow_mut().fill(field, &fill);
        }
        fn keyboard(&mut self, to: Keyboard) {
            self.0.borrow_mut().keyboard(to);
        }
        fn copy(&mut self, text: String) {
            self.0.borrow_mut().copy(&text);
        }
        fn link(&mut self, url: String) {
            self.0.borrow_mut().link(&url);
        }
        fn menu(&mut self, menu: Menu) {
            self.0.borrow_mut().menu(&menu);
        }
        fn diagnostic(&mut self, words: String) {
            self.0.borrow_mut().diagnostic(&words);
        }
        fn ended(&mut self, why: Fault) {
            self.0.borrow_mut().ended(&why.to_string());
        }
        fn rearm(&mut self) {
            self.0.borrow_mut().rearm();
        }
    }

    /// The pane in all but AppKit, around the drive, as `TerminalPane` drives a page: for the
    /// scenarios of `native_view::pane_tests`, which say what the pane does with a page and what
    /// it sees of it.
    pub(in crate::native_view) struct PaneRig {
        handle: PageHandle,
        list: DrawList,
        log: Rc<RefCell<Transcript>>,
    }

    impl PaneRig {
        /// The page of the script `source` below a strip of 32 points, measured evenly; the
        /// outline told as the pane is told it.
        pub(in crate::native_view) fn open(source: &str) -> Self {
            let script = Script {
                path: "page.lua".to_owned(),
                source: source.as_bytes().to_vec(),
                bundle: None,
            };
            let program = PaneProgram {
                flow: None,
                clock: None,
            };
            let setup = setup(ModuleLimits::default(), 32.0, program, None, Box::new(Even));
            let handle = PageHandle::new(Page::script(script, setup, || {}));
            let log = Rc::new(RefCell::new(Transcript::default()));
            let told = Rc::clone(&log);
            handle.watch_outline(Box::new(move |outline| {
                told.borrow_mut().outline(
                    &outline.fields,
                    &outline.accessible,
                    &outline.tips,
                    &outline.commands,
                );
            }));
            let held = Rc::clone(&log);
            handle.watch_held(Box::new(move |painted| {
                painted.deliver_held(&mut Written(Rc::clone(&held)));
            }));
            Self {
                handle,
                list: DrawList::default(),
                log,
            }
        }

        /// A frame of 800 × 600 pixels at a pixel to a point, at `now`, as the link's build of it
        /// ([`PageDrawing`]) takes it: the outline told after the page is let go of.
        pub(in crate::native_view) fn frame(&mut self, now: f64) {
            let painted = self.handle.page().frame(FrameAt {
                size: (800, 600),
                scale: 1.0,
                now,
                reduce_motion: false,
            });
            let mut painted = painted;
            self.handle.tell_frame(&mut painted);
            self.list = painted.list;
        }

        /// What the page said, done as the pane does it (`Drained::deliver`); whether a frame is
        /// wanted.
        fn drain(&mut self) -> bool {
            let drained = self.handle.page().drain();
            let redraw = drained.redraw && drained.ended.is_none();
            drained.deliver(&mut Written(Rc::clone(&self.log)));
            redraw
        }

        fn flush(&self) {
            assert!(
                self.handle.page().module().flush(Duration::from_secs(10)),
                "the module did not come to what it was told"
            );
        }

        /// Waits until the module has done what it was told, does what it said and draws the
        /// frames it wants at `now`, until it wants none.
        pub(in crate::native_view) fn settle(&mut self, now: f64) {
            for _ in 0..100 {
                self.flush();
                let redraw = self.drain();
                if !redraw && !self.handle.page().animating() {
                    return;
                }
                self.frame(now);
            }
            panic!("the page never came to rest");
        }

        /// The same for a pane that is not seen: no frame is drawn.
        pub(in crate::native_view) fn settle_unseen(&mut self) {
            self.flush();
            self.drain();
        }

        /// The pointer, in the pane's points: a press ends what an input method composes first —
        /// the editor keeps it as typed, the system forgets it — and then moves the caret
        /// (`TerminalPane::end_composition`).
        pub(in crate::native_view) fn input(&mut self, input: Input) {
            if matches!(input, Input::Press { .. }) {
                let editors = self.handle.editors();
                if editors.focused().is_some() && editors.marked().is_some() {
                    editors.unmark();
                    self.log.borrow_mut().say("discard marked text");
                }
            }
            self.handle.page().input(input);
        }

        /// A key: to the editor with the keyboard, where one has it, and to the page where none
        /// takes it.
        pub(in crate::native_view) fn key(&mut self, key: Key, mods: Mods) {
            let editors = self.handle.editors();
            let route = if editors.focused().is_some() {
                editors.key(key, mods)
            } else {
                Route::NotEditor
            };
            if route == Route::NotEditor {
                self.handle.page().input(Input::Key { key, mods });
            }
            self.log
                .borrow_mut()
                .say(format!("route {key:?} {route:?}"));
        }

        /// A key went up: nobody's while an editor has the keyboard.
        pub(in crate::native_view) fn key_up(&mut self, key: Key, mods: Mods) {
            self.handle.page().key_up(key, mods);
        }

        pub(in crate::native_view) fn wheel(&mut self, at: Point, dx: f32, dy: f32, phase: Phase) {
            self.handle.page().wheel(at, dx, dy, phase);
        }

        pub(in crate::native_view) fn text(&mut self, field: Id, text: &str) {
            self.handle.page().text(field, text);
        }

        pub(in crate::native_view) fn focus(&mut self, field: Option<Id>) {
            self.handle.page().focus(field);
        }

        pub(in crate::native_view) fn pick(&mut self, index: usize) {
            self.handle.page().pick(index);
        }

        pub(in crate::native_view) fn command(&mut self, id: Id) {
            self.handle.page().command(id);
        }

        pub(in crate::native_view) fn files(&mut self, at: Point, paths: Vec<String>) {
            self.handle.page().files(at, paths);
        }

        pub(in crate::native_view) fn set_scrolling(
            &mut self,
            scrolling: pencere_drive::Scrolling,
        ) {
            self.handle.page().set_scrolling(scrolling);
        }

        pub(in crate::native_view) fn set_zoom(&mut self, zoom: f32) {
            self.handle.page().set_zoom(zoom);
        }

        pub(in crate::native_view) fn set_active(&mut self, active: bool) {
            self.handle.page().set_active(active);
        }

        pub(in crate::native_view) fn wake_unseen(&mut self, now: f64) -> Option<f64> {
            self.handle.page().wake_unseen(now)
        }

        /// When the page at rest wants its next frame.
        pub(in crate::native_view) fn wake_at(&self) -> Option<f64> {
            self.handle.page().wake_at()
        }

        pub(in crate::native_view) fn editors(&self) -> Option<PageEditors> {
            Some(self.handle.editors())
        }

        /// The line of what the page heard, as the last frame drew it.
        pub(in crate::native_view) fn said(&self) -> String {
            drawn(&self.list)
                .into_iter()
                .find_map(|words| words.strip_prefix("said: ").map(str::to_owned))
                .unwrap_or_default()
        }

        /// How many boxes the last frame drew.
        pub(in crate::native_view) fn quads(&self) -> usize {
            self.list
                .items
                .iter()
                .filter(|item| matches!(item, Paint::Quad(_)))
                .count()
        }

        /// A line of the test's own.
        pub(in crate::native_view) fn note(&mut self, line: String) {
            self.log.borrow_mut().say(line);
        }

        /// The lines written since they were last taken.
        pub(in crate::native_view) fn lines(&mut self) -> Vec<String> {
            self.log.borrow_mut().take()
        }
    }

    /// What a page says is held to its program's flow: told *full* when the program has not read
    /// what it was told (the page is not closed for it), and what the page takes of the program's
    /// words is counted there. A page no program opened has whatever room it wants.
    #[test]
    fn a_pages_program_is_the_flow_of_its_pane() {
        let flow = Flow::new();
        let held = PaneProgram {
            flow: Some(flow.clone()),
            clock: None,
        };
        let mut said = 0;
        while held.may_say(64 << 10) {
            said += 1;
            assert!(said < 100, "never told full");
        }
        assert_eq!(said, bt_core::BACKLOG_LIMIT / (64 << 10));
        // What the module takes goes to the flow too (nothing was counted: it stays at zero).
        held.taken(100);
        assert_eq!(flow.inbox(), 0);
        let demo = PaneProgram {
            flow: None,
            clock: None,
        };
        assert!((0..1000).all(|_| demo.may_say(64 << 10)));
    }

    /// The link's build of a frame hands the pane the frame's outline and then what the drive held
    /// for the fields that outline brought (`Painted::held`), and does it after the page is let go
    /// of, not only when the outline changed: what is held can wait for a frame whose outline is the
    /// one the pane has. A source scan: the order is the pane's and needs the pane.
    #[test]
    fn a_frame_tells_the_outline_and_then_what_was_held_for_its_fields_with_the_page_let_go_of() {
        let source = include_str!("page.rs");
        let source = &source[..source
            .find("pub(super) mod tests")
            .expect("the tests follow")];
        let build = &source[source
            .find("    fn build(&mut self, cx: &UiContext<'_>)")
            .expect("build")..];
        let build = &build[..build.find("    fn animating(").expect("its end")];
        let told = build
            .find("self.handle.tell_frame(&mut painted)")
            .expect(build);
        assert!(
            told > build
                .find("(list, painted)\n        };")
                .expect("the page is let go of")
        );
        let frame = &source[source.find("    fn tell_frame(").expect("tell_frame")..];
        let frame = &frame[..frame.find("\n    }\n").expect("its end")];
        let outline = frame.find("self.tell_outline(&outline)").expect(frame);
        let held = frame.find("told(painted)").expect(frame);
        assert!(
            outline < held,
            "the outline makes the fields first: {frame}"
        );
        assert!(frame.contains("painted.held.is_empty()"), "{frame}");
    }

    /// The time of day a page reads is the system's with the person's offset from UTC as the system
    /// has it now, which the sandbox's own answer (UTC) leaves out: what `date` says the offset is,
    /// and a time of day that is now.
    #[test]
    fn a_pages_time_of_day_has_the_persons_offset_from_utc() {
        let program = PaneProgram {
            flow: None,
            clock: None,
        };
        let told = program.wall_clock();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("after 1970")
            .as_secs_f64();
        assert!(
            (told.seconds - now).abs() < 5.0,
            "{} vs {now}",
            told.seconds
        );
        // `date +%z` is the sign, the hours and the minutes of the system's own offset now.
        let said = std::process::Command::new("date")
            .arg("+%z")
            .output()
            .expect("date runs");
        let said = String::from_utf8(said.stdout).expect("text");
        let said = said.trim();
        let sign = if said.starts_with('-') { -1 } else { 1 };
        let hours: i32 = said[1..3].parse().expect("hours");
        let minutes: i32 = said[3..5].parse().expect("minutes");
        assert_eq!(
            told.utc_offset,
            sign * (hours * 3600 + minutes * 60),
            "{said}"
        );
        // Whatever the zone, an offset is within a day of UTC and in whole minutes.
        for unix in [0.0, 1.7e9, 2.0e9] {
            let offset = local_utc_offset(unix);
            assert!(
                offset.abs() <= 14 * 3600 && offset % 60 == 0,
                "{offset} at {unix}"
            );
        }
    }

    /// The keyboard a page reads is the system's own convention: a Mac's on the Mac (the sandbox
    /// says the system this is built for, and the pane has nothing to say against it).
    #[test]
    fn a_pages_keyboard_is_the_systems() {
        let program = PaneProgram {
            flow: None,
            clock: None,
        };
        let expected = if cfg!(target_os = "macos") {
            KeyConvention::Mac
        } else {
            KeyConvention::Linux
        };
        assert_eq!(program.keyboard(), expected);
    }

    /// The time a page reads between frames is the pane's clock, the frames' own (the drive's one
    /// clock); a page given none reads none, and counts from its frames.
    #[test]
    fn a_page_reads_the_panes_clock_the_one_its_frames_are_on() {
        fn frames_clock() -> f64 {
            1234.5
        }
        let program = PaneProgram {
            flow: None,
            clock: Some(frames_clock),
        };
        assert_eq!(program.now(), Some(1234.5));
        let none = PaneProgram {
            flow: None,
            clock: None,
        };
        assert_eq!(none.now(), None);
    }

    /// The limit of one message toward the program is the protocol's, held in one place.
    #[test]
    fn a_message_to_the_program_is_held_to_the_protocols_limit() {
        assert_eq!(
            ModuleLimits::default().message,
            bt_core::pencere::MAX_VIEW_MESSAGE
        );
    }

    fn wait(woken: &Receiver<()>) -> bool {
        woken.recv_timeout(Duration::from_secs(30)).is_ok()
    }

    fn woken() -> (impl Fn() + Send + Sync + 'static, Receiver<()>) {
        let (told, woken) = channel();
        let told = std::sync::Mutex::new(told);
        (
            move || {
                let _ = told.lock().unwrap().send(());
            },
            woken,
        )
    }

    fn opening(renderer: &Rc<Renderer>, flow: Option<Flow>) -> Opening {
        // The tests' frames are on no clock a function can read: a reading that is not a number
        // is none (the drive and the sandbox both say so), and the page counts from its frames.
        fn clock() -> f64 {
            f64::NAN
        }
        Opening {
            terminal: None,
            inset: 32.0,
            flow,
            clock,
            renderer: Rc::clone(renderer),
        }
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

    fn boxes(list: &UiList) -> usize {
        list.items()
            .iter()
            .filter(|item| matches!(item, UiItem::Quad(_)))
            .count()
    }

    /// A script's page through the pane's page on the renderer: the line that stands in for it
    /// until the module hands it out, then its boxes, measured and drawn by the renderer that
    /// made it.
    #[test]
    fn a_scripts_page_is_drawn_through_the_pane() {
        let Ok(renderer) = Renderer::system_default() else {
            eprintln!("skipped: no GPU device");
            return;
        };
        let renderer = Rc::new(renderer);
        let theme = Theme::BATERI;
        let (wake, woken) = woken();
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
            opening(&renderer, None),
            wake,
        );
        assert_eq!(boxes(&build(&mut page, &renderer, &theme, 0.0)), 0);
        assert!(wait(&woken), "the script handed out no page");
        let drained = handle.page().drain();
        assert!(drained.redraw && drained.ended.is_none(), "{drained:?}");
        let list = build(&mut page, &renderer, &theme, 0.1);
        assert!(boxes(&list) >= 2, "the two boxes of the page");
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
        let script = Script {
            path: entry,
            source: bytes.to_vec(),
            bundle: Some(assets),
        };
        // Asked for a frame (a GPU, if the machine has one, draws it; the module makes the page
        // either way, from the two files).
        if let Ok(renderer) = Renderer::system_default() {
            let renderer = Rc::new(renderer);
            let (wake, woken) = woken();
            let (mut page, handle) = script_page(
                script,
                ModuleLimits::default(),
                opening(&renderer, Some(session.flow())),
                wake,
            );
            let theme = Theme::BATERI;
            assert_eq!(boxes(&build(&mut page, &renderer, &theme, 0.0)), 0);
            assert!(wait(&woken), "the script handed out no page");
            let drained = handle.page().drain();
            assert!(drained.redraw && drained.ended.is_none(), "{drained:?}");
            assert!(boxes(&build(&mut page, &renderer, &theme, 0.1)) >= 2);
        } else {
            eprintln!("no GPU device: the page is not drawn, only started");
            let program = PaneProgram {
                flow: Some(session.flow()),
                clock: None,
            };
            let setup = setup(ModuleLimits::default(), 32.0, program, None, Box::new(Even));
            let mut page = Page::script(script, setup, || {});
            assert!(page.module().flush(Duration::from_secs(30)));
            assert_eq!(page.drain().ended, None);
        }
        session.shutdown();
    }

    /// Frames and drains the demo's page until it rests: frames a sixtieth of a second apart from
    /// `now`, as the link draws them while the page moves. How many frames it took.
    fn rest(
        page: &mut Box<dyn UiPage>,
        handle: &PageHandle,
        renderer: &Renderer,
        theme: &Theme,
        reduce_motion: bool,
        mut now: f64,
    ) -> usize {
        for frames in 0..60 {
            assert!(handle.page().module().flush(Duration::from_secs(30)));
            let drained = handle.page().drain();
            if !drained.redraw && !page.animating() {
                return frames;
            }
            page.build(&UiContext {
                renderer,
                size_px: (2000, 1200),
                scale: 2.0,
                theme,
                now,
                reduce_motion,
                minimum_contrast: MINIMUM_CONTRAST,
            });
            now += 1.0 / 60.0;
        }
        panic!("the page never came to rest");
    }

    /// What the demo answers to what the pane tells it, once its module has.
    fn answer(handle: &PageHandle, input: Input) -> pencere_view::Reply {
        handle.page().input(input);
        assert!(handle.page().module().flush(Duration::from_secs(30)));
        let mut drained = handle.page().drain();
        assert_eq!(drained.replies.len(), 1, "{drained:?}");
        drained.replies.remove(0).reply
    }

    /// The demo through the pane's page and the real renderer: boxes and, where the platform has
    /// proportional text, the glyphs. Skipped where no GPU device can be had.
    #[test]
    fn the_container_list_becomes_boxes_and_glyphs_on_the_renderer() {
        let Ok(renderer) = Renderer::system_default() else {
            eprintln!("skipped: no GPU device");
            return;
        };
        let renderer = Rc::new(renderer);
        let theme = Theme::BATERI;
        let (wake, woken) = woken();
        let (mut page, handle) = containers_demo(opening(&renderer, None), wake);
        build(&mut page, &renderer, &theme, 0.0);
        assert!(wait(&woken), "the demo handed out no page");
        assert!(handle.page().module().flush(Duration::from_secs(30)));
        assert!(handle.page().drain().redraw);
        let list = build(&mut page, &renderer, &theme, 0.1);
        let boxes = boxes(&list);
        let glyphs = list.len() - boxes;
        assert!(
            boxes > 100,
            "the rows, dots, tags and the sparklines: {boxes}"
        );
        if Renderer::ui_text_supported() {
            assert!(glyphs > 300, "the words of the page: {glyphs}");
        } else {
            assert_eq!(glyphs, 0);
        }
    }

    /// The demo on the real renderer: it draws when it changed and rests, the fades that start
    /// with the pointer end by themselves, and the pane is told where its field goes. Skipped
    /// where no GPU device can be had.
    #[test]
    fn the_demo_rests_between_the_inputs_that_change_it_and_tells_the_pane_its_field() {
        use pencere_drive::Field;
        use pencere_view::{Button, FieldKind};
        let Ok(renderer) = Renderer::system_default() else {
            eprintln!("skipped: no GPU device");
            return;
        };
        let renderer = Rc::new(renderer);
        let theme = Theme::BATERI;
        let (mut page, handle) = containers_demo(opening(&renderer, None), || {});
        let told: Rc<RefCell<Vec<Vec<Field>>>> = Rc::default();
        {
            let told = Rc::clone(&told);
            handle.watch_outline(Box::new(move |outline| {
                told.borrow_mut().push(outline.fields.clone());
            }));
        }
        assert_eq!(
            told.borrow().as_slice(),
            &[Vec::<Field>::new()],
            "told at once"
        );
        build(&mut page, &renderer, &theme, 1.0);
        rest(&mut page, &handle, &renderer, &theme, false, 1.0);
        let field = handle
            .page()
            .fields()
            .first()
            .expect("the filter's field has a place")
            .rect;
        assert_eq!(
            told.borrow().last().cloned(),
            Some(vec![Field {
                id: super::super::containers::FIELD,
                kind: FieldKind::Line,
                rect: field,
                hint: Some(super::super::containers::FIELD_HINT.to_owned()),
                label: None,
            }]),
            "its place, and what the page says the field says while it is empty"
        );
        assert!(field.y >= 32.0, "below the strip: {field:?}");
        // Laid out again the same way, the pane is not told again.
        let times = told.borrow().len();
        build(&mut page, &renderer, &theme, 1.5);
        assert_eq!(told.borrow().len(), times);
        assert!(!page.animating());

        // A press on the field's box asks for the keyboard for the pane's field.
        let middle = Point::new(field.x + field.w / 2.0, field.y + field.h / 2.0);
        let press = answer(
            &handle,
            Input::Press {
                at: middle,
                button: Button::Left,
                mods: Mods::default(),
                count: 1,
            },
        );
        assert_eq!(
            press.keyboard,
            Some(Keyboard::Field(super::super::containers::FIELD))
        );

        // The pointer over a row: one frame's worth of change, then fades until they end.
        let over_a_row = Point::new(300.0, 32.0 + 150.0);
        assert!(answer(&handle, Input::Move(over_a_row)).redraw);
        let same = answer(&handle, Input::Move(Point::new(301.0, 32.0 + 151.0)));
        assert!(!same.redraw, "the same row: no frame");
        build(&mut page, &renderer, &theme, 2.0);
        let frames = rest(&mut page, &handle, &renderer, &theme, false, 2.0);
        assert!(frames > 2, "a fade takes a few frames, not one: {frames}");
        // Under Reduce Motion the same change is one frame.
        let (mut page, handle) = containers_demo(opening(&renderer, None), || {});
        page.build(&UiContext {
            renderer: &renderer,
            size_px: (2000, 1200),
            scale: 2.0,
            theme: &theme,
            now: 1.0,
            reduce_motion: true,
            minimum_contrast: MINIMUM_CONTRAST,
        });
        rest(&mut page, &handle, &renderer, &theme, true, 1.0);
        assert!(answer(&handle, Input::Move(over_a_row)).redraw);
        let frames = rest(&mut page, &handle, &renderer, &theme, true, 1.1);
        assert!(frames <= 2, "{frames}");
    }

    /// The demo's list scrolled by the pane, as the drive scrolls every page's boxes: the list is
    /// where the wheel took it and built where it went, the row the cursor goes to is shown by
    /// the next frame (the page's ask), and a turn over what does not scroll is the page's.
    #[test]
    fn the_demos_list_is_scrolled_by_the_drive_and_built_where_it_went() {
        let list_id = super::super::containers::LIST;
        let program = PaneProgram {
            flow: None,
            clock: None,
        };
        let setup = setup(ModuleLimits::default(), 0.0, program, None, Box::new(Even));
        let mut page = Page::spawn(|| Ok(Demo::new()), setup, || {});
        let mut now = 0.0;
        let mut settle = |page: &mut Page| -> Vec<pencere_sandbox::Answer> {
            let mut replies = Vec::new();
            for _ in 0..100 {
                page.frame(FrameAt {
                    size: (1000, 330),
                    scale: 1.0,
                    now,
                    reduce_motion: false,
                });
                now += 0.1;
                assert!(page.module().flush(Duration::from_secs(30)));
                let drained = page.drain();
                replies.extend(drained.replies);
                if !drained.redraw && !page.animating() && !page.unsettled() {
                    return replies;
                }
            }
            panic!("the page never came to rest");
        };
        settle(&mut page);
        let list = page
            .laid()
            .and_then(|laid| laid.bounds(list_id))
            .expect("the list");
        let over = Point::new(list.x + list.w / 2.0, list.y + list.h / 2.0);
        page.wheel(over, 0.0, 120.0, Phase::Wheel);
        assert!(page.drain().redraw, "a frame is worth drawing");
        settle(&mut page);
        let offset = |page: &Page| {
            page.laid()
                .and_then(|laid| laid.scroll_extent(list_id))
                .map(|extent| (extent.offset.y, extent.max_offset()))
        };
        assert_eq!(
            offset(&page).map(|(y, _)| y),
            Some(120.0),
            "the list is where the wheel took it"
        );
        // The keys move the cursor to the end: the page asks for its row to be shown, and the
        // next frame shows it.
        page.input(Input::Key {
            key: Key::End,
            mods: Mods::default(),
        });
        settle(&mut page);
        let (y, end) = offset(&page).expect("the list");
        assert_eq!(y, end);
        // A wheel over what does not scroll is the page's: answered as the pointer is.
        page.wheel(Point::new(2.0, 2.0), 0.0, 10.0, Phase::Wheel);
        let answers = settle(&mut page);
        assert!(
            answers.iter().any(|answer| answer.pointer),
            "the wheel is the page's: {answers:?}"
        );
    }
}
