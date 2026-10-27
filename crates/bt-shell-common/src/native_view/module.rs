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

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use bt_core::Flow;
use pencere_sandbox::{
    Assets, ImageLimits, Images, Module, ModuleLimits, Program, Script, Unbounded,
};
use pencere_view::{Element, Event, Laid, Motions, Phase, Role, ScrollTo, Size, div, text};

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
        self.replies.extend(
            update
                .replies
                .into_iter()
                .map(|answer| (from_pane(answer.reply, place), answer.pointer)),
        );
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
    }));
    let page = ModulePage {
        terminal,
        inset,
        shaped: Shaped::default(),
        images,
        shared: Rc::clone(&shared),
        motions: Motions::default(),
    };
    (Box::new(page), ModuleHandle { shared })
}

/// The drive of a module's page: the pane's input in, what the module says out. Cloned freely;
/// all of it on the main thread.
#[derive(Clone)]
pub struct ModuleHandle {
    shared: Rc<RefCell<Shared>>,
}

impl ModuleHandle {
    /// The page's fields, in the pane's points, as of the last frame.
    pub fn fields(&self) -> Vec<Field> {
        self.shared.borrow().outline.fields.clone()
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
        shared.module.event(Event::Input(input));
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
        self.shared.borrow().module.event(Event::Focus(field));
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
    shaped: Shaped,
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

impl UiPage for ModulePage {
    fn build(&mut self, cx: &UiContext<'_>) -> UiList {
        let zoom = self.shared.borrow().place.zoom;
        let inset = (self.inset * cx.scale as f32).round();
        let scale = cx.scale as f32 * zoom;
        let width = cx.size_px.0 as f32;
        let height = (cx.size_px.1 as f32 - inset).max(0.0);
        let size = Size::new(width / scale, height / scale);
        let (root, cached) = {
            let mut shared = self.shared.borrow_mut();
            shared.sync(true);
            // A page for this size, the next step of one that is moving and the page whose time
            // has come; a page that is none of these is not asked for.
            let due = shared.wake_at.is_some_and(|at| cx.now >= at);
            let resized = shared.asked.is_some_and(|asked| asked != (size, scale));
            if shared.asked != Some((size, scale)) || shared.animating || due {
                shared.asked = Some((size, scale));
                shared.wake_at = None;
                shared.module.render(size, cx.now, cx.reduce_motion);
            }
            // A resized pane waits a little for the page made for its size rather than draw,
            // for a frame, the one laid out for the size it was: that frame is what flickers.
            // A page that comes meanwhile for another size (one asked before) does not end it.
            if resized && shared.module.wait_for_frame(size, RESIZE_WAIT) {
                shared.sync(true);
            }
            shared.scrolls.frame(cx.now);
            let asks = std::mem::take(&mut shared.asks);
            shared.scrolls.ask(asks);
            // Only the scrolling changed: the boxes move on the page as it was laid out.
            let still =
                !shared.fresh && shared.laid_for == Some((size, scale)) && !self.motions.moving();
            let cached = shared.laid.clone().filter(|_| still);
            (shared.root.clone(), cached)
        };
        let starting = root.is_none();
        if self.shaped.lines.len() > super::SHAPED_LIMIT {
            self.shaped.lines.clear();
        }
        let mut typeset = Typeset {
            renderer: cx.renderer,
            terminal: &self.terminal,
            shaped: &mut self.shaped,
            images: self.images.as_deref(),
        };
        let mut laid = match cached {
            Some(laid) if !starting => (*laid).clone(),
            _ => {
                let mut root = root.unwrap_or_else(starting_line);
                self.shared.borrow().scrolls.scrolls.apply(&mut root);
                self.motions.layout(
                    root,
                    Size::new(width, height),
                    scale,
                    &mut typeset,
                    cx.now,
                    cx.reduce_motion,
                )
            }
        };
        if !starting {
            self.shared
                .borrow_mut()
                .scrolls
                .scrolls
                .place(&mut laid, cx.now, cx.reduce_motion);
        }
        let laid = Arc::new(laid);
        let draws = laid.paint(&mut typeset);
        let list = to_list(
            cx.renderer,
            cx.theme,
            cx.minimum_contrast,
            &mut typeset,
            &draws,
            (0.0, inset),
        );
        let told = {
            let mut shared = self.shared.borrow_mut();
            // The module may ask of this layout from now on, and brings what depends on it back
            // within it; a page that was already within it answers nothing. The line that stands
            // in for a page is not the module's.
            if !starting {
                shared.module.lay_out(Arc::clone(&laid));
                shared.laid = Some(Arc::clone(&laid));
                shared.laid_for = Some((size, scale));
                shared.fresh = false;
                // Where the boxes went: the page builds what came into view.
                let moved = shared.scrolls.scrolls.take_moved();
                if !moved.is_empty() {
                    shared.module.event(Event::Scrolled(moved));
                }
            }
            let outline = Outline::of(&laid, shared.place);
            let told = (outline != shared.outline)
                .then(|| {
                    shared
                        .on_outline
                        .clone()
                        .map(|told| (told, outline.clone()))
                })
                .flatten();
            shared.outline = outline;
            told
        };
        // After the page is let go of: the host moves its fields in this call.
        if let Some((told, outline)) = told {
            told(&outline);
        }
        list
    }

    fn animating(&self) -> bool {
        let shared = self.shared.borrow();
        shared.animating || self.motions.moving() || shared.scrolls.moving()
    }

    fn wake_at(&self) -> Option<f64> {
        let shared = self.shared.borrow();
        match (shared.wake_at, shared.scrolls.wake_at()) {
            (Some(page), Some(bars)) => Some(page.min(bars)),
            (page, bars) => page.or(bars),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_view::guest_dir;
    use bt_core::{MINIMUM_CONTRAST, Theme};
    use bt_gpu::{Renderer, UiItem};
    use pencere_view::Point;
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
        let places: Rc<RefCell<Vec<Vec<Field>>>> = Rc::default();
        {
            let places = Rc::clone(&places);
            handle.watch_outline(Box::new(move |outline| {
                places.borrow_mut().push(outline.fields.clone());
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
        assert_eq!(
            places
                .borrow()
                .iter()
                .map(|fields| fields.iter().map(|f| f.rect).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            [Vec::new(), vec![field]]
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
        assert_eq!(places.borrow().len(), 1, "the field's place is told once");

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
