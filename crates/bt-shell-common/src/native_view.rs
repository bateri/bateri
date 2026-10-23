//! A page of the UI kit in a pane: `bt-view`'s draw list on `bt-gpu`'s renderer.
//!
//! The engine (`bt-view`) writes a page as boxes and lines of text and knows neither the
//! terminal's fonts, nor its colours, nor its renderer. This is where the three meet:
//!
//! - **Text** is shaped and measured by the renderer ([`bt_gpu::Renderer::ui_shape`]) — the
//!   object that measures is the object that draws, so the width the layout was given is the
//!   width of the glyphs on screen. The terminal's own face is the pane's font family.
//! - **Colours** are the theme's: the engine names a [`Tone`], the theme says what it is
//!   ([`tone_linear`]) — the arithmetic of colours stays in `bt_core::Theme`, a page keeps
//!   none, and a theme change is a new list and nothing else.
//! - **The list** is the renderer's ([`bt_gpu::UiList`]), in pixels, drawn in place of the
//!   terminal's grid while the page is up ([`bt_gpu::DisplayLink::set_page`]).
//!
//! A page is a function from the size it has (points) to an [`Element`]; [`EnginePage`] calls
//! it for every frame the link asks for — the pane resized, the theme or the font changed —
//! and lays the result out again, so a pane that is split or resized finds the page laid out
//! for its new size.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use bt_core::{LinearRgba, Theme};
use bt_gpu::{
    Renderer, ShapedText, TextFamily, TextFontId, TextStyle, UiContext, UiList, UiPage, UiQuad,
    UiRect, tint,
};
use bt_view::demo::Containers;

mod allowance;
mod module;
pub use bt_view::{
    Button, Cursor, Input, Key, Keyboard, Menu, MenuEntry, Mods, Point, Rect, Reply, Tone,
};
use bt_view::{Color, DrawList, Element, Family, Font, Laid, Measure, Metrics, Paint, Quad, Size};
pub use bt_view_wasm::Limits;
pub use module::{ModuleHandle, module_page};

/// What `tone` is in `theme`: the nine roles are the theme's own, the rest are the values it
/// derives from them (surfaces a step above the ground, the hairline, the shadow's colour).
pub fn tone_linear(theme: &Theme, tone: Tone) -> LinearRgba {
    match tone {
        Tone::Background => theme.background_linear(),
        Tone::Foreground => theme.foreground_linear(),
        Tone::Dim => theme.dim_linear(),
        Tone::Accent => theme.accent_linear(),
        Tone::Cursor => theme.cursor_linear(),
        Tone::Success => theme.success_linear(),
        Tone::Error => theme.error_linear(),
        Tone::Info => theme.info_linear(),
        Tone::Warning => theme.warning_linear(),
        Tone::Raised(steps) => theme.raised_linear(steps),
        Tone::Quiet => theme.quiet_linear(),
        Tone::Line => theme.separator_linear(),
        Tone::Shadow => theme.dock_shadow_linear(),
    }
}

/// `color` as the renderer takes it: linear straight RGBA.
pub fn resolve(theme: &Theme, color: Color) -> [f32; 4] {
    tint(tone_linear(theme, color.tone), color.alpha)
}

/// The engine's box as the renderer's, moved to `origin` pixels.
pub fn quad(theme: &Theme, quad: &Quad, origin: (f32, f32)) -> UiQuad {
    let at =
        |rect: bt_view::Rect| UiRect::new(rect.x + origin.0, rect.y + origin.1, rect.w, rect.h);
    let mut out = UiQuad::new(at(quad.rect))
        .radii(quad.radii)
        .clip(at(quad.clip));
    if let Some(fill) = quad.fill {
        out = out.fill(resolve(theme, fill));
    }
    if let Some(border) = quad.border {
        out = out.border(border.width, resolve(theme, border.color));
    }
    if let Some(shadow) = quad.shadow {
        out = out.shadow(
            shadow.dx,
            shadow.dy,
            shadow.blur,
            resolve(theme, shadow.color),
        );
    }
    out
}

/// What a pane drives a page of the kit by, whatever the page is made of: one that lives in this
/// process ([`ContainersHandle`]) or a module in a sandbox ([`ModuleHandle`]).
///
/// The pane **tells** (what the person did) and **asks what has come back** ([`PageDrive::drain`]);
/// it never waits for a page to answer. A page of this process answers at once, so the pane drains
/// right after it tells; a module answers when it has, and the pane drains when it is woken.
pub trait PageDrive {
    /// The person did something with the pointer, the wheel or a key, in the pane's points (the
    /// top-left corner of the pane is the origin; the strip's room is included).
    fn send_input(&self, input: Input);
    /// The filter field's text changed.
    fn send_query(&self, text: &str);
    /// The person picked the `index`th entry of the menu the last reply asked for.
    fn send_pick(&self, index: usize);
    /// The program that opened the page said something: one JSON object, as it wrote it. A page
    /// that no program opened (the engine's own, in this process) hears none.
    fn send_message(&self, _text: &str) {}
    /// `told` is called, on the thread that builds the frame, when the place of the text field the
    /// page leaves room for changes (the pane resized, the page laid out another way): the host
    /// moves its field to it. Called once with the place the page has now, if it has one.
    fn watch_field(&self, told: Box<dyn Fn(Option<Rect>)>);
    /// Hands over what the page has said since the last call: each reply, in order, with whether
    /// the event it answers was the pointer's (the pointer's shape after a key is nobody's to
    /// set). Never waits.
    fn drain(&self, reply: &mut dyn FnMut(Reply, bool)) -> Drained;
}

/// What a page had besides replies.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Drained {
    /// The page has something new to show: a frame of it is worth drawing.
    pub redraw: bool,
    /// Why the page is gone, the first time it is told: the pane takes it down.
    pub ended: Option<String>,
    /// What the page said to its program since the last call, in order: JSON objects, as it
    /// wrote them. The pane carries them to the program, and only while the page's view is
    /// open (`bt_core::Session::module_message`). Told before `ended`: what a page said before
    /// it went is said.
    pub sent: Vec<String>,
}

/// The fonts opened and the lines shaped so far. They outlive a build: a page is laid out
/// again for every size the pane has, and the same words in the same fonts are shaped once.
#[derive(Default)]
struct Shaped {
    fonts: HashMap<Font, Option<TextFontId>>,
    lines: HashMap<(Font, String), Option<ShapedText>>,
}

/// More lines than a page of this kind has words: the cache starts again past it, so a page
/// whose words keep changing does not collect every one it ever said.
const SHAPED_LIMIT: usize = 4096;

/// The renderer as the engine's measure: opens and shapes through it, remembering what it
/// did, so a line measured to be laid out is not shaped again to be drawn.
struct Typeset<'a> {
    renderer: &'a Renderer,
    /// The terminal's face: what `Family::Terminal` is.
    terminal: &'a Option<String>,
    shaped: &'a mut Shaped,
}

impl Typeset<'_> {
    fn shaped(&mut self, font: &Font, text: &str) -> Option<&ShapedText> {
        let (renderer, terminal) = (self.renderer, self.terminal);
        let id = (*self.shaped.fonts.entry(*font).or_insert_with(|| {
            renderer.ui_font(&TextStyle {
                family: match font.family {
                    Family::System => TextFamily::System,
                    Family::Terminal => TextFamily::Terminal(terminal.clone()),
                },
                size_px: f64::from(font.size),
                weight: font.weight,
            })
        }))?;
        self.shaped
            .lines
            .entry((*font, text.to_owned()))
            .or_insert_with(|| renderer.ui_shape(id, text))
            .as_ref()
    }
}

impl Measure for Typeset<'_> {
    fn measure(&mut self, font: &Font, text: &str) -> Metrics {
        self.shaped(font, text)
            .map_or_else(Metrics::default, |line| Metrics {
                width: line.width as f32,
                height: line.height() as f32,
            })
    }
}

/// The draw list on the renderer's, at `origin` pixels of the surface.
fn to_list(
    renderer: &Renderer,
    theme: &Theme,
    typeset: &mut Typeset<'_>,
    draws: &DrawList,
    origin: (f32, f32),
) -> UiList {
    let mut list = UiList::new();
    for item in &draws.items {
        match item {
            Paint::Quad(q) => list.quad(quad(theme, q, origin)),
            Paint::Text(run) => {
                let Some(line) = typeset.shaped(&run.font, &run.text) else {
                    continue;
                };
                let clip = UiRect::new(
                    run.clip.x + origin.0,
                    run.clip.y + origin.1,
                    run.clip.w,
                    run.clip.h,
                );
                renderer.ui_text(
                    &mut list,
                    line,
                    [run.origin.x + origin.0, run.origin.y + origin.1],
                    resolve(theme, run.color),
                    clip,
                );
            }
        }
    }
    list
}

/// A page written with the engine, shown as a pane's [`UiPage`].
///
/// `view` is the page: given the size of the area it has, in points, it returns the elements.
/// The area is the pane's less `inset` points at the top — what the application keeps for its
/// own strip over the page (`view_strip`), which the page never draws into.
pub struct EnginePage<F> {
    terminal: Option<String>,
    inset: f32,
    view: F,
    shaped: Shaped,
}

impl<F: FnMut(Size) -> Element> EnginePage<F> {
    /// `terminal` is the pane's font family (`None`: the default chain), the face the page's
    /// `Family::Terminal` text is set in.
    pub fn new(terminal: Option<String>, inset: f32, view: F) -> Self {
        Self {
            terminal,
            inset,
            view,
            shaped: Shaped::default(),
        }
    }
}

impl<F: FnMut(Size) -> Element> UiPage for EnginePage<F> {
    fn build(&mut self, cx: &UiContext<'_>) -> UiList {
        let scale = cx.scale as f32;
        let inset = (self.inset * scale).round();
        let width = cx.size_px.0 as f32;
        let height = (cx.size_px.1 as f32 - inset).max(0.0);
        let root = (self.view)(Size::new(width / scale, height / scale));
        if self.shaped.lines.len() > SHAPED_LIMIT {
            self.shaped.lines.clear();
        }
        let mut typeset = Typeset {
            renderer: cx.renderer,
            terminal: &self.terminal,
            shaped: &mut self.shaped,
        };
        let laid = bt_view::layout(root, Size::new(width, height), scale, &mut typeset);
        let draws = laid.paint(&mut typeset);
        to_list(cx.renderer, cx.theme, &mut typeset, &draws, (0.0, inset))
    }
}

/// The container list of the engine's demo as a pane's page, and the handle the pane drives it
/// by. `terminal` is the pane's font family; `inset` the points at the top the application
/// keeps for its own strip.
///
/// The page is the engine's [`Containers`] with what the person does to it: the handle takes
/// the pane's input ([`ContainersHandle::input`]) in the pane's points, answers what the page
/// says back ([`Reply`]) and tells the host where the filter's text field stands
/// ([`ContainersHandle::on_field`]). **A page at rest costs no frames**: the page draws when the
/// handle's input changed what it shows, and keeps asking for frames only while a fade of its
/// is moving ([`UiPage::animating`]).
pub fn containers_demo(
    terminal: Option<String>,
    inset: f32,
) -> (Box<dyn UiPage>, ContainersHandle) {
    let shared = Rc::new(RefCell::new(Shared {
        page: Containers::fixture(),
        laid: bt_view::layout(bt_view::div(), Size::new(1.0, 1.0), 1.0, &mut NoText),
        inset,
        field: None,
        on_field: None,
        replies: Vec::new(),
    }));
    let page = ContainersPage {
        terminal,
        inset,
        shaped: Shaped::default(),
        shared: Rc::clone(&shared),
    };
    (Box::new(page), ContainersHandle { shared })
}

/// A host that has no text to measure: the page laid out before the first frame has no words.
struct NoText;

impl Measure for NoText {
    fn measure(&mut self, _font: &Font, _text: &str) -> Metrics {
        Metrics::default()
    }
}

/// What the page and its handle share.
struct Shared {
    page: Containers,
    /// The page as it was last laid out: what the pointer is asked against.
    laid: Laid,
    /// Points at the top of the pane the application keeps.
    inset: f32,
    /// Where the filter's field was last said to stand, in the pane's points.
    field: Option<Rect>,
    on_field: Option<Rc<dyn Fn(Option<Rect>)>>,
    /// What the page said that the pane has not taken ([`PageDrive::drain`]).
    replies: Vec<(Reply, bool)>,
}

/// The drive of the container-list page: the pane's input in, the page's reply out. Cloned
/// freely; all of it on the main thread.
#[derive(Clone)]
pub struct ContainersHandle {
    shared: Rc<RefCell<Shared>>,
}

impl ContainersHandle {
    /// What the person did, in the pane's points (the top-left corner of the pane is the
    /// origin; the strip's room is included). The reply's points are the pane's too.
    pub fn input(&self, input: Input) -> Reply {
        let mut shared = self.shared.borrow_mut();
        let inset = shared.inset;
        let Shared { page, laid, .. } = &mut *shared;
        from_pane(page.input(into_page(input, inset), laid), inset)
    }

    /// The filter field's text changed.
    pub fn set_query(&self, text: &str) -> Reply {
        let mut shared = self.shared.borrow_mut();
        let inset = shared.inset;
        from_pane(shared.page.set_query(text), inset)
    }

    /// The person picked the `index`th entry of the menu the last reply asked for.
    pub fn pick(&self, index: usize) -> Reply {
        let mut shared = self.shared.borrow_mut();
        let inset = shared.inset;
        from_pane(shared.page.pick(index), inset)
    }

    /// Where the filter's text field stands, in the pane's points, as of the last frame.
    pub fn field(&self) -> Option<Rect> {
        self.shared.borrow().field
    }

    /// `told` is called, on the thread that builds the frame, when the filter field's place
    /// changes (the pane resized, the page laid out another way): the host moves its field to
    /// it. Called once with the place the page has now, if it has one.
    pub fn on_field(&self, told: impl Fn(Option<Rect>) + 'static) {
        let (field, told) = {
            let mut shared = self.shared.borrow_mut();
            let told: Rc<dyn Fn(Option<Rect>)> = Rc::new(told);
            shared.on_field = Some(Rc::clone(&told));
            (shared.field, told)
        };
        if field.is_some() {
            told(field);
        }
    }
}

impl PageDrive for ContainersHandle {
    fn send_input(&self, input: Input) {
        let pointer = !matches!(input, Input::Key { .. });
        let reply = ContainersHandle::input(self, input);
        self.shared.borrow_mut().replies.push((reply, pointer));
    }

    fn send_query(&self, text: &str) {
        let reply = self.set_query(text);
        self.shared.borrow_mut().replies.push((reply, false));
    }

    fn send_pick(&self, index: usize) {
        let reply = self.pick(index);
        self.shared.borrow_mut().replies.push((reply, false));
    }

    fn watch_field(&self, told: Box<dyn Fn(Option<Rect>)>) {
        self.on_field(told);
    }

    fn drain(&self, reply: &mut dyn FnMut(Reply, bool)) -> Drained {
        // Taken before they are handed over: the pane may take the page down in the middle.
        let replies = std::mem::take(&mut self.shared.borrow_mut().replies);
        for (said, pointer) in replies {
            reply(said, pointer);
        }
        Drained::default()
    }
}

/// An input in the pane's points as the page's: the strip's room is not the page's.
fn into_page(input: Input, inset: f32) -> Input {
    let up = |at: Point| Point::new(at.x, at.y - inset);
    match input {
        Input::Move(at) => Input::Move(up(at)),
        Input::Press { at, button, mods } => Input::Press {
            at: up(at),
            button,
            mods,
        },
        Input::Release { at, button } => Input::Release { at: up(at), button },
        Input::Scroll { at, dy } => Input::Scroll { at: up(at), dy },
        other @ (Input::Leave | Input::Key { .. }) => other,
    }
}

/// A reply of the page with its points in the pane's.
fn from_pane(mut reply: Reply, inset: f32) -> Reply {
    if let Some(menu) = &mut reply.menu {
        menu.at.y += inset;
    }
    reply
}

/// The engine's container list as a [`UiPage`]: built from the shared state for every frame
/// the link asks for.
struct ContainersPage {
    terminal: Option<String>,
    inset: f32,
    shaped: Shaped,
    shared: Rc<RefCell<Shared>>,
}

/// The most times a page is built for one frame: the rows in view depend on how far the list is
/// scrolled, and what the scroll comes to depends on the rows — a second build settles it.
const BUILDS: usize = 3;

impl UiPage for ContainersPage {
    fn build(&mut self, cx: &UiContext<'_>) -> UiList {
        let scale = cx.scale as f32;
        let inset = (self.inset * scale).round();
        let width = cx.size_px.0 as f32;
        let height = (cx.size_px.1 as f32 - inset).max(0.0);
        let size = Size::new(width / scale, height / scale);
        if self.shaped.lines.len() > SHAPED_LIMIT {
            self.shaped.lines.clear();
        }
        let mut typeset = Typeset {
            renderer: cx.renderer,
            terminal: &self.terminal,
            shaped: &mut self.shaped,
        };
        let (list, told) = {
            let mut shared = self.shared.borrow_mut();
            shared.page.advance(cx.now, cx.reduce_motion);
            let mut laid = bt_view::layout(
                shared.page.view(size),
                Size::new(width, height),
                scale,
                &mut typeset,
            );
            for _ in 1..BUILDS {
                if !shared.page.settle(&laid) {
                    break;
                }
                laid = bt_view::layout(
                    shared.page.view(size),
                    Size::new(width, height),
                    scale,
                    &mut typeset,
                );
            }
            let draws = laid.paint(&mut typeset);
            let list = to_list(cx.renderer, cx.theme, &mut typeset, &draws, (0.0, inset));
            // The field's room, in the pane's points: the strip's room above the page is added.
            let field = laid
                .text_field()
                .map(|r| Rect::new(r.x, r.y + shared.inset, r.w, r.h));
            let told = (field != shared.field)
                .then(|| shared.on_field.clone().map(|told| (told, field)))
                .flatten();
            shared.field = field;
            shared.laid = laid;
            (list, told)
        };
        // After the page is let go of: the host moves its field in this call.
        if let Some((told, field)) = told {
            told(field);
        }
        list
    }

    fn animating(&self) -> bool {
        self.shared.borrow().page.animating()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_view::{Border, Rect, Shadow};

    const THEMES: [Theme; 3] = [Theme::BATERI, Theme::BATERI_LIGHT, Theme::LINEN];

    #[test]
    fn every_tone_is_the_themes_own_value() {
        for theme in THEMES {
            assert_eq!(
                tone_linear(&theme, Tone::Background),
                theme.background_linear()
            );
            assert_eq!(
                tone_linear(&theme, Tone::Foreground),
                theme.foreground_linear()
            );
            assert_eq!(tone_linear(&theme, Tone::Dim), theme.dim_linear());
            assert_eq!(tone_linear(&theme, Tone::Accent), theme.accent_linear());
            assert_eq!(tone_linear(&theme, Tone::Cursor), theme.cursor_linear());
            assert_eq!(tone_linear(&theme, Tone::Success), theme.success_linear());
            assert_eq!(tone_linear(&theme, Tone::Error), theme.error_linear());
            assert_eq!(tone_linear(&theme, Tone::Info), theme.info_linear());
            assert_eq!(tone_linear(&theme, Tone::Warning), theme.warning_linear());
            assert_eq!(tone_linear(&theme, Tone::Raised(2)), theme.raised_linear(2));
            assert_eq!(tone_linear(&theme, Tone::Quiet), theme.quiet_linear());
            assert_eq!(tone_linear(&theme, Tone::Line), theme.separator_linear());
            assert_eq!(
                tone_linear(&theme, Tone::Shadow),
                theme.dock_shadow_linear()
            );
        }
    }

    #[test]
    fn a_tone_with_an_opacity_is_the_themes_colour_with_that_alpha() {
        let theme = Theme::BATERI;
        let [r, g, b, _] = theme.accent_linear().to_array();
        assert_eq!(resolve(&theme, Tone::Accent.alpha(0.13)), [r, g, b, 0.13]);
        assert_eq!(resolve(&theme, Tone::Accent.solid())[3], 1.0);
    }

    #[test]
    fn a_page_follows_the_theme_it_is_painted_in() {
        let on = |theme: &Theme| resolve(theme, Tone::Raised(1).solid());
        assert_ne!(on(&Theme::BATERI), on(&Theme::BATERI_LIGHT));
    }

    #[test]
    fn a_box_goes_to_the_renderer_with_everything_it_has_moved_by_the_origin() {
        let theme = Theme::BATERI;
        let drawn = Quad {
            rect: Rect::new(10.0, 20.0, 30.0, 40.0),
            radii: [1.0, 2.0, 3.0, 4.0],
            fill: Some(Tone::Accent.alpha(0.5)),
            border: Some(Border {
                width: 2.0,
                color: Tone::Line.solid(),
            }),
            shadow: Some(Shadow {
                dx: 0.0,
                dy: 3.0,
                blur: 6.0,
                color: Tone::Shadow.alpha(0.4),
            }),
            clip: Rect::new(0.0, 0.0, 100.0, 200.0),
        };
        let expected = UiQuad::new(UiRect::new(10.0, 52.0, 30.0, 40.0))
            .radii([1.0, 2.0, 3.0, 4.0])
            .clip(UiRect::new(0.0, 32.0, 100.0, 200.0))
            .fill(resolve(&theme, Tone::Accent.alpha(0.5)))
            .border(2.0, resolve(&theme, Tone::Line.solid()))
            .shadow(0.0, 3.0, 6.0, resolve(&theme, Tone::Shadow.alpha(0.4)));
        assert_eq!(quad(&theme, &drawn, (0.0, 32.0)), expected);
    }

    /// The demo through the bridge and the real renderer: boxes and, where the platform has
    /// proportional text, the glyphs. Skipped where no GPU device can be had.
    #[test]
    fn the_container_list_becomes_boxes_and_glyphs_on_the_renderer() {
        let Ok(renderer) = Renderer::system_default() else {
            eprintln!("skipped: no GPU device");
            return;
        };
        let theme = Theme::BATERI;
        let cx = UiContext {
            renderer: &renderer,
            size_px: (2000, 1200),
            scale: 2.0,
            theme: &theme,
            now: 0.0,
            reduce_motion: false,
        };
        let (mut page, _) = containers_demo(None, 32.0);
        let list = page.build(&cx);
        let boxes = list
            .items()
            .iter()
            .filter(|item| matches!(item, bt_gpu::UiItem::Quad(_)))
            .count();
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

    #[test]
    fn the_strips_room_is_not_the_pages_and_a_menu_comes_back_in_the_panes_points() {
        use bt_view::{Button, Menu, Mods};
        let moved = into_page(Input::Move(Point::new(10.0, 50.0)), 32.0);
        assert_eq!(moved, Input::Move(Point::new(10.0, 18.0)));
        let pressed = into_page(
            Input::Press {
                at: Point::new(1.0, 40.0),
                button: Button::Right,
                mods: Mods::default(),
            },
            32.0,
        );
        assert!(matches!(pressed, Input::Press { at, .. } if at == Point::new(1.0, 8.0)));
        assert_eq!(into_page(Input::Leave, 32.0), Input::Leave);
        let reply = Reply {
            menu: Some(Menu {
                at: Point::new(4.0, 8.0),
                entries: Vec::new(),
            }),
            ..Reply::default()
        };
        assert_eq!(
            from_pane(reply, 32.0).menu.map(|menu| menu.at),
            Some(Point::new(4.0, 40.0))
        );
    }

    #[test]
    fn the_keyboard_is_answered_before_the_first_frame() {
        let (_page, handle) = containers_demo(None, 32.0);
        let escape = Input::Key {
            key: bt_view::Key::Escape,
            mods: bt_view::Mods::default(),
        };
        assert!(handle.input(escape).close);
        assert_eq!(handle.field(), None, "no frame, no place for the field");
        // A pointer over a page that was never laid out is over nothing.
        assert!(!handle.input(Input::Move(Point::new(5.0, 50.0))).redraw);
    }

    #[test]
    fn a_page_of_this_process_answers_at_once_and_the_pane_takes_the_answers_by_draining() {
        let (_page, handle) = containers_demo(None, 32.0);
        let drive: &dyn PageDrive = &handle;
        drive.send_input(Input::Move(Point::new(5.0, 50.0)));
        drive.send_input(Input::Key {
            key: bt_view::Key::Escape,
            mods: bt_view::Mods::default(),
        });
        drive.send_query("api");
        let mut said = Vec::new();
        let drained = drive.drain(&mut |reply, pointer| said.push((reply, pointer)));
        assert_eq!(drained, Drained::default());
        assert_eq!(said.len(), 3, "one reply for each thing told");
        // Told in order, each with whether it was the pointer's: the move, then the key.
        assert!(said[0].1);
        assert!(!said[1].1 && said[1].0.close);
        assert!(!said[2].1);
        // Taken once.
        assert_eq!(
            drive.drain(&mut |_, _| panic!("nothing is left to take")),
            Drained::default()
        );
    }

    /// The page on the real renderer: it draws on a request and rests, the fades that start
    /// with the pointer end by themselves, and the host is told where its field goes.
    /// Skipped where no GPU device can be had.
    #[test]
    fn the_page_rests_between_the_inputs_that_change_it_and_tells_the_host_its_field() {
        let Ok(renderer) = Renderer::system_default() else {
            eprintln!("skipped: no GPU device");
            return;
        };
        let theme = Theme::BATERI;
        let build = |page: &mut Box<dyn UiPage>, now: f64, reduce_motion: bool| {
            page.build(&UiContext {
                renderer: &renderer,
                size_px: (2000, 1200),
                scale: 2.0,
                theme: &theme,
                now,
                reduce_motion,
            })
        };
        let (mut page, handle) = containers_demo(None, 32.0);
        let told: Rc<RefCell<Vec<Option<Rect>>>> = Rc::default();
        {
            let told = Rc::clone(&told);
            handle.on_field(move |field| told.borrow_mut().push(field));
        }
        build(&mut page, 1.0, false);
        let field = handle.field().expect("the filter's field has a place");
        assert_eq!(told.borrow().as_slice(), &[Some(field)]);
        assert!(field.y >= 32.0, "below the strip: {field:?}");
        // Laid out again the same way, the host is not told again.
        build(&mut page, 1.1, false);
        assert_eq!(told.borrow().len(), 1);
        assert!(!page.animating());

        // A press on the field's box asks for the keyboard for the host's field.
        let middle = Point::new(field.x + field.w / 2.0, field.y + field.h / 2.0);
        let press = handle.input(Input::Press {
            at: middle,
            button: bt_view::Button::Left,
            mods: bt_view::Mods::default(),
        });
        assert_eq!(press.keyboard, Some(bt_view::Keyboard::Filter));

        // The pointer over a row: one frame's worth of change, then fades until they end.
        let over_a_row = Point::new(300.0, 32.0 + 150.0);
        let moved = handle.input(Input::Move(over_a_row));
        assert!(moved.redraw);
        let same = handle.input(Input::Move(Point::new(301.0, 32.0 + 151.0)));
        assert!(!same.redraw, "the same row: no frame");
        let mut now = 2.0;
        let mut frames = 0;
        loop {
            build(&mut page, now, false);
            frames += 1;
            now += 1.0 / 60.0;
            if !page.animating() {
                break;
            }
            assert!(frames < 40, "the page never came to rest");
        }
        assert!(frames > 2, "a fade takes a few frames, not one: {frames}");
        // Under Reduce Motion the same change is one frame.
        let (mut page, handle) = containers_demo(None, 32.0);
        build(&mut page, 1.0, true);
        assert!(handle.input(Input::Move(over_a_row)).redraw);
        build(&mut page, 1.1, true);
        assert!(!page.animating());
    }
}
