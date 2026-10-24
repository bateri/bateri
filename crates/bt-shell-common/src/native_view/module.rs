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

use bt_core::Flow;
use pencere_sandbox::{Assets, ImageLimits, Images, Module, ModuleLimits, Program, Unbounded};
use pencere_view::{Element, Event, Role, Size, div, text};

use super::{
    Drained, Input, PageDrive, Rect, Reply, Shaped, Typeset, UiContext, UiList, UiPage, from_pane,
    into_page, to_list,
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
    /// The page is moving: it is asked for again at every frame, until it says it is not.
    animating: bool,
    /// The time the page at rest wants to be asked for again, on the frames' clock.
    wake_at: Option<f64>,
    /// The viewport (points) and the scale the module was last asked for a page for.
    asked: Option<(Size, f32)>,
    /// Points at the top of the pane the application keeps.
    inset: f32,
    /// Where the text field's room was last said to stand, in the pane's points.
    field: Option<Rect>,
    on_field: Option<Rc<dyn Fn(Option<Rect>)>>,
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
            self.redraw |= !building;
        }
        if let Some(images) = &self.images {
            let arrived = images.poll();
            // A size the layout was waiting for, pixels the paint was: either way a frame of the
            // page, the one that lays it out again and draws it.
            if (arrived.relayout || arrived.redraw) && !building {
                self.redraw = true;
            }
            self.diagnostics
                .extend(arrived.diagnostics.iter().map(ToString::to_string));
        }
        let inset = self.inset;
        self.replies.extend(
            update
                .replies
                .into_iter()
                .map(|answer| (from_pane(answer.reply, inset), answer.pointer)),
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
/// interpreter of the script's language as the module and `script` (its text) as what it runs.
pub fn script_page(
    script: Vec<u8>,
    limits: ModuleLimits,
    terminal: Option<String>,
    inset: f32,
    flow: Option<Flow>,
    assets: Option<Arc<dyn Assets>>,
    wake: impl Fn() + Send + Sync + 'static,
) -> (Box<dyn UiPage>, ModuleHandle) {
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
        animating: false,
        wake_at: None,
        asked: None,
        inset,
        field: None,
        on_field: None,
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
    /// Where the text field's room stands, in the pane's points, as of the last frame.
    pub fn field(&self) -> Option<Rect> {
        self.shared.borrow().field
    }
}

impl PageDrive for ModuleHandle {
    fn send_input(&self, input: Input) {
        let shared = self.shared.borrow();
        let input = into_page(input, shared.inset);
        shared.module.event(Event::Input(input));
    }

    fn send_query(&self, text: &str) {
        self.shared
            .borrow()
            .module
            .event(Event::Text(text.to_owned()));
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

    fn watch_field(&self, told: Box<dyn Fn(Option<Rect>)>) {
        let (field, told) = {
            let mut shared = self.shared.borrow_mut();
            let told: Rc<dyn Fn(Option<Rect>)> = Rc::from(told);
            shared.on_field = Some(Rc::clone(&told));
            (shared.field, told)
        };
        if field.is_some() {
            told(field);
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
}

impl UiPage for ModulePage {
    fn build(&mut self, cx: &UiContext<'_>) -> UiList {
        let scale = cx.scale as f32;
        let inset = (self.inset * scale).round();
        let width = cx.size_px.0 as f32;
        let height = (cx.size_px.1 as f32 - inset).max(0.0);
        let size = Size::new(width / scale, height / scale);
        let root = {
            let mut shared = self.shared.borrow_mut();
            shared.sync(true);
            // A page for this size, the next step of one that is moving and the page whose time
            // has come; a page that is none of these is not asked for.
            let due = shared.wake_at.is_some_and(|at| cx.now >= at);
            if shared.asked != Some((size, scale)) || shared.animating || due {
                shared.asked = Some((size, scale));
                shared.wake_at = None;
                shared.module.render(size, cx.now, cx.reduce_motion);
            }
            shared.root.clone()
        };
        let starting = root.is_none();
        let root = root.unwrap_or_else(starting_line);
        if self.shaped.lines.len() > super::SHAPED_LIMIT {
            self.shaped.lines.clear();
        }
        let mut typeset = Typeset {
            renderer: cx.renderer,
            terminal: &self.terminal,
            shaped: &mut self.shaped,
            images: self.images.as_deref(),
        };
        let laid = Arc::new(pencere_view::layout(
            root,
            Size::new(width, height),
            scale,
            &mut typeset,
        ));
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
            }
            let field = laid
                .text_field()
                .map(|r| Rect::new(r.x, r.y + shared.inset, r.w, r.h));
            let told = (field != shared.field)
                .then(|| shared.on_field.clone().map(|told| (told, field)))
                .flatten();
            shared.field = field;
            told
        };
        // After the page is let go of: the host moves its field in this call.
        if let Some((told, field)) = told {
            told(field);
        }
        list
    }

    fn animating(&self) -> bool {
        self.shared.borrow().animating
    }

    fn wake_at(&self) -> Option<f64> {
        self.shared.borrow().wake_at
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
        let places: Rc<RefCell<Vec<Option<Rect>>>> = Rc::default();
        {
            let places = Rc::clone(&places);
            handle.watch_field(Box::new(move |place| places.borrow_mut().push(place)));
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
        let field = handle.field().expect("room for the filter's field");
        assert!(field.y >= 32.0, "below the strip: {field:?}");
        assert_eq!(places.borrow().as_slice(), &[Some(field)]);

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
            script,
            ModuleLimits::default(),
            None,
            32.0,
            None,
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
        let field = handle.field().expect("room for the filter's field");
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

    /// A script that makes an error ends its page with the error and the line it is on, which
    /// is what the program is told, once.
    #[test]
    fn a_script_that_makes_an_error_ends_with_its_line_told_once() {
        let (told, woken) = channel();
        let told = std::sync::Mutex::new(told);
        let (_page, handle) = script_page(
            b"local ok = 1\nerror('no page for you', 0)".to_vec(),
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
        assert_eq!(drained.ended.as_deref(), Some("no page for you"));
        assert_eq!(handle.drain(&mut |_, _| {}).ended, None);

        let (told, woken) = channel();
        let told = std::sync::Mutex::new(told);
        let (_page, handle) = script_page(
            b"local x = 1\nlocal y = = 2".to_vec(),
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
        assert!(why.starts_with("script:2:"), "{why}");
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
            br#"return { render = function()
                return pencere.div():bg("raised1"):p(8):child(pencere.text("hello")):child(
                    pencere.div():size(20, 20):bg("accent"))
            end }"#
                .to_vec(),
            ModuleLimits::default(),
            None,
            32.0,
            None,
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
            script.into_bytes(),
            ModuleLimits::default(),
            None,
            32.0,
            None,
            Some(Arc::clone(&assets) as Arc<dyn Assets>),
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
