//! A page of the UI kit in a pane: `pencere-view`'s draw list on `bt-gpu`'s renderer.
//!
//! The engine (`pencere-view`) writes a page as boxes and lines of text and knows neither the
//! terminal's fonts, nor its colours, nor its renderer. This is where the three meet:
//!
//! - **Text** is shaped and measured by the renderer ([`bt_gpu::Renderer::ui_shape`]) — the
//!   object that measures is the object that draws, so the width the layout was given is the
//!   width of the glyphs on screen. The terminal's own face is the pane's font family.
//! - **Colours** are the theme's: the engine names a [`Role`], the theme says what it is
//!   ([`tone_linear`]) — the arithmetic of colours stays in `bt_core::Theme`, a page keeps
//!   none, and a theme change is a new list and nothing else. A colour the page gives as a
//!   value takes the form that goes with the theme's brightness ([`resolve`]). **The words of
//!   a page are held to the contrast floor of the program's words in the grid**
//!   ([`ink`]): the page is the program's, and what it writes is read against the fills under
//!   it, not against the pane's ground alone.
//! - **Images** are files of the program's bundle, decoded off the thread that draws by
//!   `pencere-sandbox` ([`Images`]) and drawn as textured rectangles ([`bt_gpu::UiImage`]):
//!   the layout asks the decoder for a file's size ([`Measure::image`]) and the list gives the
//!   renderer its pixels ([`image`]). A file that is not decoded yet is a box with nothing in
//!   it; the decoder wakes the pane **once** for what it learned, and a page at rest is asked
//!   for nothing and wakes nothing.
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

use std::sync::{Arc, Weak};

use bt_core::{LinearRgba, Session, Theme};
use bt_gpu::{
    Renderer, ShapedText, TextFamily, TextFontId, TextStyle, UiBitmap, UiContext, UiImage, UiList,
    UiPage, UiQuad, UiRect, tint,
};
use containers::Containers;

mod containers;
mod module;
pub use module::{ModuleHandle, module_page, script_page};
use pencere_sandbox::Images;
pub use pencere_sandbox::{Assets, ImageLimits, ModuleLimits, Script};
pub use pencere_view::{
    Accessible, AccessibleRole, Button, Cursor, FieldKind, Id, Input, Key, Keyboard, Menu,
    MenuEntry, Mods, Point, Rect, Reply, Role,
};
use pencere_view::{
    Color, DrawList, Element, Family, Font, Laid, Measure, Metrics, Paint, Picture, Quad, Run,
    Size, Source,
};

/// A text field the host puts over a page: its name (the id of the element that is its room), its
/// kind, where its room stands in the pane's points, what it says while it is empty — the words
/// the page put in that room — and what it is to a screen reader.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub id: Id,
    pub kind: FieldKind,
    pub rect: Rect,
    pub hint: Option<String>,
    pub label: Option<String>,
}

/// Where a page stands in its pane: below the strip's `inset` points, and `zoom` of the pane's
/// points to one of the page's — the page is drawn as large as the terminal's text is to the
/// kit's own size (`ZOOM_TEXT`), so its words are the size of the terminal's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub inset: f32,
    pub zoom: f32,
}

/// The size of the kit's text where a page does not say one (`pencere-view`'s default, 12.5
/// points): a terminal whose text is this size draws a page at its own size.
pub const ZOOM_TEXT: f32 = 12.5;

/// The least and the most a page is drawn at, against its own size: a terminal set in a tiny or a
/// huge face keeps a page that can still be laid out in its pane.
const ZOOM_RANGE: (f32, f32) = (0.5, 4.0);

impl Placement {
    /// `zoom` for a terminal whose text is `points` large ([`ZOOM_TEXT`]); 1 for one that is
    /// not a size.
    pub fn zoom_for(points: f64) -> f32 {
        #[allow(clippy::cast_possible_truncation)]
        let zoom = (points as f32) / ZOOM_TEXT;
        if zoom.is_finite() && zoom > 0.0 {
            zoom.clamp(ZOOM_RANGE.0, ZOOM_RANGE.1)
        } else {
            1.0
        }
    }

    /// A box of the page in the pane's points.
    pub fn to_pane(self, rect: Rect) -> Rect {
        let z = self.zoom;
        Rect::new(rect.x * z, rect.y * z + self.inset, rect.w * z, rect.h * z)
    }

    /// A point of the pane as the page's.
    pub fn to_page(self, at: Point) -> Point {
        Point::new(at.x / self.zoom, (at.y - self.inset) / self.zoom)
    }
}

/// What is called when a page's outline changes ([`PageDrive::watch_outline`]).
pub(crate) type OnOutline = Rc<dyn Fn(&Outline)>;

/// What the host keeps of a page besides its pixels, in the pane's points: the fields it puts
/// over it and what a screen reader says of it. Told to the host when it changes
/// ([`PageDrive::watch_outline`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outline {
    pub fields: Vec<Field>,
    pub accessible: Vec<Accessible>,
    /// What the page says of a part of it while the pointer rests there, innermost last.
    pub tips: Vec<Tip>,
}

/// A tooltip of the page: where it is in the pane, and what it says (the keys that do the same in
/// brackets, as the system's own tooltips write a shortcut).
#[derive(Clone, Debug, PartialEq)]
pub struct Tip {
    pub rect: Rect,
    pub words: String,
}

impl Outline {
    /// The outline of `laid`, placed in the pane as `at` says.
    pub(crate) fn of(laid: &Laid, at: Placement) -> Self {
        Self {
            fields: laid
                .fields()
                .into_iter()
                .map(|field| Field {
                    id: field.id,
                    kind: field.kind,
                    rect: at.to_pane(field.rect),
                    hint: field.hint,
                    label: field.label,
                })
                .collect(),
            accessible: laid
                .accessible()
                .into_iter()
                .map(|item| Accessible {
                    rect: at.to_pane(item.rect),
                    ..item
                })
                .collect(),
            tips: laid
                .tips()
                .into_iter()
                .map(|(rect, tip)| Tip {
                    rect: at.to_pane(rect),
                    words: match tip.keys {
                        Some(keys) if !keys.is_empty() => format!("{} ({keys})", tip.words),
                        _ => tip.words,
                    },
                })
                .collect(),
        }
    }
}

/// The greatest level of `raised` that is drawn (the kit's specification, §3.1): the interface
/// carries a byte and a page may say 255, and a host treats a level above this as this one — a
/// page that says more gets the same surface, not a tone that runs out of the theme's room
/// between its ground and its foreground.
pub const MAX_RAISED: u8 = 8;

/// What `tone` is in `theme`: the nine roles are the theme's own, the sixteen colours of the
/// ANSI palette and the selection are the ones the grid draws, and the rest are the values it
/// derives from them (surfaces a step above the ground, up to [`MAX_RAISED`] of them, the
/// hairline, the shadow's colour).
pub fn tone_linear(theme: &Theme, tone: Role) -> LinearRgba {
    match tone {
        Role::Background => theme.background_linear(),
        Role::Foreground => theme.foreground_linear(),
        Role::Dim => theme.dim_linear(),
        Role::Accent => theme.accent_linear(),
        Role::Cursor => theme.cursor_linear(),
        Role::Success => theme.success_linear(),
        Role::Error => theme.error_linear(),
        Role::Info => theme.info_linear(),
        Role::Warning => theme.warning_linear(),
        Role::Raised(steps) => theme.raised_linear(steps.min(MAX_RAISED)),
        Role::Quiet => theme.quiet_linear(),
        Role::Border => theme.separator_linear(),
        Role::Shadow => theme.dock_shadow_linear(),
        Role::Selection => theme.selection_linear(),
        // The sixteen of the palette, by their place in it; a tone that is none of the others
        // would be the foreground, the colour a theme always has.
        palette => palette.ansi_index().map_or_else(
            || theme.foreground_linear(),
            |index| theme.ansi_linear(index),
        ),
    }
}

/// A source of a colour in `theme`: the colour (solid) and its own opacity. A role is the theme's;
/// a value is the form that goes with the theme's brightness ([`Theme::is_dark`]).
fn source_parts(theme: &Theme, source: Source) -> (LinearRgba, f32) {
    match source {
        Source::Role(tone) => (tone_linear(theme, tone), 1.0),
        Source::Rgba { .. } => match source.value(theme.is_dark()) {
            Some(value) => (
                LinearRgba::from_srgb(value.r, value.g, value.b),
                f32::from(value.a) / 255.0,
            ),
            None => (theme.foreground_linear(), 1.0),
        },
    }
}

/// A colour as the page said it, in `theme`: the colour (solid) and the share of it that shows.
/// A mix is made in sRGB, channel by channel and the opacity with them, each channel rounded to
/// the nearest of the 256 (the kit's specification, §3); the page's opacity goes on top.
fn parts(theme: &Theme, color: Color) -> (LinearRgba, f32) {
    let (colour, own) = source_parts(theme, color.source);
    let Some(mix) = color.mix else {
        return (colour, color.alpha * own);
    };
    let (toward, toward_own) = source_parts(theme, mix.toward);
    let t = mix.amount.clamp(0.0, 1.0);
    let (from, to) = (colour.to_srgb(), toward.to_srgb());
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let channel = |at: usize| {
        ((from[at] + (to[at] - from[at]) * t) * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    (
        LinearRgba::from_srgb(channel(0), channel(1), channel(2)),
        color.alpha * (own + (toward_own - own) * t),
    )
}

/// `color` as the renderer takes it: linear straight RGBA.
pub fn resolve(theme: &Theme, color: Color) -> [f32; 4] {
    let (colour, alpha) = parts(theme, color);
    tint(colour, alpha)
}

/// What a line of text is read against: the fills under it laid over each other from the
/// farthest to the nearest, on the pane's ground — the engine names them nearest first and
/// stops at the first solid one, so the ground shows only where none is.
fn ground(theme: &Theme, run: &Run) -> LinearRgba {
    run.grounds
        .iter()
        .rev()
        .fold(theme.background_linear(), |below, under| {
            let (colour, alpha) = parts(theme, *under);
            colour.over(alpha, below)
        })
}

/// The colour a line of text is drawn in: the one the page said, moved only as far as it takes
/// to stand `minimum_contrast` (`[appearance] minimum_contrast`) apart from what it is on
/// ([`LinearRgba::legible_on`], the rule the program's words in the grid, the fill band and
/// the dock are drawn by). Lines, fills and borders are not text and are drawn as they are
/// said ([`resolve`]).
///
/// **What is held to the floor is the colour as it shows**, the word's colour at its own opacity
/// laid on the ground it stands on: a word that is clear enough to vanish into its ground is
/// not a word that reads because the colour it was said in does. A word that reads as it shows
/// is drawn as it was said, opacity and all; one that does not is moved as it shows (the blend,
/// moved only as far as it takes) and drawn solid, because moving only the colour under the
/// opacity would take a fraction of the way the word needs to go.
pub fn ink(theme: &Theme, run: &Run, minimum_contrast: f64) -> [f32; 4] {
    let (colour, alpha) = parts(theme, run.color);
    let under = ground(theme, run);
    let shown = colour.over(alpha, under);
    let held = shown.legible_on(under, minimum_contrast);
    if held == shown {
        return tint(colour, alpha);
    }
    tint(held, 1.0)
}

/// The engine's box as the renderer's, moved to `origin` pixels.
pub fn quad(theme: &Theme, quad: &Quad, origin: (f32, f32)) -> UiQuad {
    let at = |rect: Rect| UiRect::new(rect.x + origin.0, rect.y + origin.1, rect.w, rect.h);
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

/// The engine's image as the renderer's, moved to `origin` pixels: the part of the file the
/// rectangle shows, its corners and its clip as a box has them, its opacity, and the tint — a
/// colour the page said, resolved in the theme like a fill ([`resolve`]) and not held to the
/// contrast floor: it paints a shape, not words.
pub fn image(theme: &Theme, picture: &Picture, origin: (f32, f32)) -> UiImage {
    let at = |rect: Rect| UiRect::new(rect.x + origin.0, rect.y + origin.1, rect.w, rect.h);
    let crop = picture.crop;
    let mut out = UiImage::new(at(picture.rect))
        .crop(UiRect::new(crop.x, crop.y, crop.w, crop.h))
        .opacity(picture.opacity)
        .radii(picture.radii)
        .clip(at(picture.clip));
    if let Some(color) = picture.tint {
        out = out.tint(resolve(theme, color));
    }
    out
}

/// The files of the open view's bundle, as a program sent them and the session keeps them
/// ([`Session::serve_view_file`]): what a page's images, and the scripts a Lua page requires,
/// are read from. Holds the session
/// weakly — the images of a page that outlives its program's view find no file, and are
/// refused with a word for the page's author, not read from a session that is gone.
///
/// Read on the images' worker thread ([`Assets::read`]) and on a script's own, for the scripts it
/// requires, never the one that draws: the session's lock and the disk are both waited for.
pub struct BundleAssets {
    session: Weak<Session>,
}

impl BundleAssets {
    pub fn new(session: &Arc<Session>) -> Self {
        Self {
            session: Arc::downgrade(session),
        }
    }
}

impl Assets for BundleAssets {
    fn read(&self, path: &str) -> Option<Arc<[u8]>> {
        let (_, bytes) = self.session.upgrade()?.serve_view_file(path)?;
        Some(bytes)
    }

    fn media_type(&self, path: &str) -> Option<String> {
        // The type is the manifest's word on the file: the scripts a Lua page requires are the
        // files it types `text/x-lua`, and the sandbox asks it of every path the page might name,
        // so it is not read from the store (`serve_view_file` reads the file).
        self.session.upgrade()?.view_file_type(path)
    }
}

/// Where the page modules the sandbox is tried with are, in a development tree: the
/// `BT_VIEW_GUEST_DIR` folder, else `target/guest/dist`, where `make guest` builds them from the
/// pencere checkout the build is pinned to (bateri carries no module of its own, and no
/// checkout of pencere is expected to sit beside this one). `containers.lua`, the container
/// list as a script, is copied there with them. The debug menu's demos and the tests that need
/// a real module read it; the tests that do fail where it is not there, they do not skip.
#[cfg(any(test, debug_assertions))]
pub fn guest_dir() -> std::path::PathBuf {
    std::env::var_os("BT_VIEW_GUEST_DIR").map_or_else(
        || std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/guest/dist"),
        std::path::PathBuf::from,
    )
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
    /// The text of the field named `field` changed: the person typed, pasted or deleted.
    fn send_text(&self, field: Id, text: &str);
    /// The person clicked into the field named `field`, or the keyboard went back to the page of
    /// itself (`None`); not told when it went where the page's reply sent it.
    fn send_focus(&self, field: Option<Id>);
    /// The person picked the `index`th entry of the menu the last reply asked for.
    fn send_pick(&self, index: usize);
    /// The program that opened the page said something: one JSON object, as it wrote it. A page
    /// that no program opened (the engine's own, in this process) hears none.
    fn send_message(&self, _text: &str) {}
    /// The program has read enough that the page may say what it was told the program had no
    /// room for. A page that no program opened is never told.
    fn send_room(&self) {}
    /// `told` is called, on the thread that builds the frame, when the page's outline changes
    /// (the pane resized, the page laid out another way, a field or a label came or went): the
    /// host puts its fields where the page leaves room for them and tells the screen reader.
    /// Called once with the outline the page has now.
    fn watch_outline(&self, told: Box<dyn Fn(&Outline)>);
    /// The terminal's text is this much larger than the kit's own (`Placement::zoom_for`): the
    /// page is drawn that much larger, and laid out again.
    fn set_zoom(&self, zoom: f32);
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
    /// Images the page named that could not be shown, each once, a sentence for the page's
    /// author (`image "x" is …`). The page goes on without them.
    pub diagnostics: Vec<String>,
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
    /// The page's images; a page that has no bundle has none and its pictures are boxes.
    images: Option<&'a Images>,
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
                italic: font.italic,
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

    /// The size of the file in its own pixels, once the decoder knows it. The first time a file
    /// is asked for it does not: it starts to be decoded, and the page is laid out again when
    /// it is known.
    fn image(&mut self, src: &str) -> Option<Size> {
        self.images.and_then(|images| images.size(src))
    }
}

/// The draw list on the renderer's, at `origin` pixels of the surface, its words held to
/// `minimum_contrast` ([`ink`]).
fn to_list(
    renderer: &Renderer,
    theme: &Theme,
    minimum_contrast: f64,
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
                let color = ink(theme, run, minimum_contrast);
                let at = [run.origin.x + origin.0, run.origin.y + origin.1];
                renderer.ui_text(&mut list, line, at, color, clip);
                for rule in decorations(run, line) {
                    let rule = UiRect::new(rule.x + at[0], rule.y + at[1], rule.w, rule.h);
                    list.quad(UiQuad::new(rule).fill(color).clip(clip));
                }
            }
            // The file's pixels, drawn at the size the page gave them (an SVG is drawn at it).
            // None yet — being decoded, or an SVG not yet drawn at its first size — is a box with
            // nothing in it until the decoder wakes the pane; one it refused stays so.
            Paint::Image(picture) => {
                let Some(bitmap) = typeset
                    .images
                    .and_then(|images| images.bitmap(&picture.src, picture.full()))
                else {
                    continue;
                };
                renderer.ui_image(
                    &mut list,
                    &UiBitmap {
                        id: bitmap.id,
                        width: bitmap.width,
                        height: bitmap.height,
                        pixels: bitmap.pixels(),
                    },
                    image(theme, picture, origin),
                );
            }
        }
    }
    list
}

/// The lines a run asks for under and through its words, relative to the line's top-left, pixels:
/// as wide as the words, as thick as a twelfth of the line's ascent (a pixel at least), under the
/// baseline by a third of the descent, and through the middle of the lower-case letters (about
/// half the height of an `x`, which is about half the ascent).
fn decorations(run: &Run, line: &ShapedText) -> Vec<Rect> {
    #[allow(clippy::cast_possible_truncation)]
    let (width, ascent, descent) = (line.width as f32, line.ascent as f32, line.descent as f32);
    let thick = (ascent / 12.0).round().max(1.0);
    let mut rules = Vec::new();
    if run.underline {
        rules.push(Rect::new(
            0.0,
            (ascent + descent / 3.0).round(),
            width,
            thick,
        ));
    }
    if run.strike {
        rules.push(Rect::new(
            0.0,
            (ascent * 0.7 - thick / 2.0).round(),
            width,
            thick,
        ));
    }
    rules
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
            images: None,
        };
        let laid = pencere_view::layout(root, Size::new(width, height), scale, &mut typeset);
        let draws = laid.paint(&mut typeset);
        to_list(
            cx.renderer,
            cx.theme,
            cx.minimum_contrast,
            &mut typeset,
            &draws,
            (0.0, inset),
        )
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
        laid: pencere_view::layout(pencere_view::div(), Size::new(1.0, 1.0), 1.0, &mut NoText),
        place: Placement { inset, zoom: 1.0 },
        outline: Outline::default(),
        on_outline: None,
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
    /// Where the page stands in the pane.
    place: Placement,
    /// The fields and what a screen reader says, as last told, in the pane's points.
    outline: Outline,
    on_outline: Option<OnOutline>,
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
        let place = shared.place;
        let Shared { page, laid, .. } = &mut *shared;
        from_pane(page.input(into_page(input, place), laid), place)
    }

    /// The filter field's text changed (the page has the one field).
    pub fn set_query(&self, text: &str) -> Reply {
        let mut shared = self.shared.borrow_mut();
        let place = shared.place;
        from_pane(shared.page.set_query(text), place)
    }

    /// The person picked the `index`th entry of the menu the last reply asked for.
    pub fn pick(&self, index: usize) -> Reply {
        let mut shared = self.shared.borrow_mut();
        let place = shared.place;
        from_pane(shared.page.pick(index), place)
    }

    /// The page's fields, in the pane's points, as of the last frame.
    pub fn fields(&self) -> Vec<Field> {
        self.shared.borrow().outline.fields.clone()
    }

    /// `told` is called, on the thread that builds the frame, when the outline changes (the pane
    /// resized, the page laid out another way): the host moves its fields to it. Called once
    /// with the outline the page has now.
    pub fn on_outline(&self, told: impl Fn(&Outline) + 'static) {
        let (outline, told) = {
            let mut shared = self.shared.borrow_mut();
            let told: Rc<dyn Fn(&Outline)> = Rc::new(told);
            shared.on_outline = Some(Rc::clone(&told));
            (shared.outline.clone(), told)
        };
        told(&outline);
    }
}

impl PageDrive for ContainersHandle {
    fn send_input(&self, input: Input) {
        let pointer = !matches!(input, Input::Key { .. });
        let reply = ContainersHandle::input(self, input);
        self.shared.borrow_mut().replies.push((reply, pointer));
    }

    fn send_text(&self, _field: Id, text: &str) {
        let reply = self.set_query(text);
        self.shared.borrow_mut().replies.push((reply, false));
    }

    fn send_focus(&self, _field: Option<Id>) {}

    fn send_pick(&self, index: usize) {
        let reply = self.pick(index);
        self.shared.borrow_mut().replies.push((reply, false));
    }

    fn watch_outline(&self, told: Box<dyn Fn(&Outline)>) {
        self.on_outline(told);
    }

    fn set_zoom(&self, zoom: f32) {
        self.shared.borrow_mut().place.zoom = zoom;
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

/// An input in the pane's points as the page's: the strip's room is not the page's, and a page
/// drawn larger has fewer points.
fn into_page(input: Input, place: Placement) -> Input {
    let at = |at: Point| place.to_page(at);
    match input {
        Input::Move(here) => Input::Move(at(here)),
        Input::Press {
            at: here,
            button,
            mods,
            count,
        } => Input::Press {
            at: at(here),
            button,
            mods,
            count,
        },
        Input::Release { at: here, button } => Input::Release {
            at: at(here),
            button,
        },
        Input::Scroll { at: here, dx, dy } => Input::Scroll {
            at: at(here),
            dx: dx / place.zoom,
            dy: dy / place.zoom,
        },
        other @ (Input::Leave | Input::Key { .. }) => other,
    }
}

/// A reply of the page with its points in the pane's.
fn from_pane(mut reply: Reply, place: Placement) -> Reply {
    if let Some(menu) = &mut reply.menu {
        let at = place.to_pane(Rect::new(menu.at.x, menu.at.y, 0.0, 0.0));
        menu.at = Point::new(at.x, at.y);
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
        let zoom = self.shared.borrow().place.zoom;
        let inset = (self.inset * cx.scale as f32).round();
        let scale = cx.scale as f32 * zoom;
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
            images: None,
        };
        let (list, told) = {
            let mut shared = self.shared.borrow_mut();
            shared.page.advance(cx.now, cx.reduce_motion);
            let mut laid = pencere_view::layout(
                shared.page.view(size),
                Size::new(width, height),
                scale,
                &mut typeset,
            );
            for _ in 1..BUILDS {
                if !shared.page.settle(&laid) {
                    break;
                }
                laid = pencere_view::layout(
                    shared.page.view(size),
                    Size::new(width, height),
                    scale,
                    &mut typeset,
                );
            }
            let draws = laid.paint(&mut typeset);
            let list = to_list(
                cx.renderer,
                cx.theme,
                cx.minimum_contrast,
                &mut typeset,
                &draws,
                (0.0, inset),
            );
            // The fields and what a screen reader says, in the pane's points.
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
            shared.laid = laid;
            (list, told)
        };
        // After the page is let go of: the host moves its fields in this call.
        if let Some((told, outline)) = told {
            told(&outline);
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
    use bt_core::MINIMUM_CONTRAST;
    use pencere_view::{Border, Rect, Shadow};

    const THEMES: [Theme; 3] = [Theme::BATERI, Theme::BATERI_LIGHT, Theme::LINEN];

    #[test]
    fn every_tone_is_the_themes_own_value() {
        for theme in THEMES {
            assert_eq!(
                tone_linear(&theme, Role::Background),
                theme.background_linear()
            );
            assert_eq!(
                tone_linear(&theme, Role::Foreground),
                theme.foreground_linear()
            );
            assert_eq!(tone_linear(&theme, Role::Dim), theme.dim_linear());
            assert_eq!(tone_linear(&theme, Role::Accent), theme.accent_linear());
            assert_eq!(tone_linear(&theme, Role::Cursor), theme.cursor_linear());
            assert_eq!(tone_linear(&theme, Role::Success), theme.success_linear());
            assert_eq!(tone_linear(&theme, Role::Error), theme.error_linear());
            assert_eq!(tone_linear(&theme, Role::Info), theme.info_linear());
            assert_eq!(tone_linear(&theme, Role::Warning), theme.warning_linear());
            assert_eq!(tone_linear(&theme, Role::Raised(2)), theme.raised_linear(2));
            // A level above the greatest is the greatest, up to the byte the interface carries.
            for level in [MAX_RAISED, MAX_RAISED + 1, 100, u8::MAX] {
                assert_eq!(
                    tone_linear(&theme, Role::Raised(level)),
                    theme.raised_linear(MAX_RAISED),
                    "raised({level})"
                );
            }
            assert_ne!(
                tone_linear(&theme, Role::Raised(1)),
                tone_linear(&theme, Role::Raised(MAX_RAISED)),
                "the levels below it are their own"
            );
            assert_eq!(tone_linear(&theme, Role::Quiet), theme.quiet_linear());
            assert_eq!(tone_linear(&theme, Role::Border), theme.separator_linear());
            assert_eq!(
                tone_linear(&theme, Role::Shadow),
                theme.dock_shadow_linear()
            );
        }
    }

    #[test]
    fn a_tone_with_an_opacity_is_the_themes_colour_with_that_alpha() {
        let theme = Theme::BATERI;
        let [r, g, b, _] = theme.accent_linear().to_array();
        assert_eq!(resolve(&theme, Role::Accent.alpha(0.13)), [r, g, b, 0.13]);
        assert_eq!(resolve(&theme, Role::Accent.solid())[3], 1.0);
    }

    #[test]
    fn the_palette_and_the_selection_are_the_grids_own_colours() {
        for theme in THEMES {
            for (index, tone) in Role::ANSI.into_iter().enumerate() {
                let [r, g, b] = [16, 8, 0].map(|shift| {
                    u8::try_from((theme.ansi[index] >> shift) & 0xff).expect("a byte")
                });
                assert_eq!(
                    tone_linear(&theme, tone),
                    LinearRgba::from_srgb(r, g, b),
                    "{tone:?}"
                );
            }
            assert_eq!(
                tone_linear(&theme, Role::Selection),
                theme.selection_linear()
            );
        }
        // The sixteen are different colours, so a tone is not answered by a neighbour's.
        let theme = Theme::BATERI;
        assert_ne!(
            tone_linear(&theme, Role::Red),
            tone_linear(&theme, Role::BrightRed)
        );
        assert_ne!(
            tone_linear(&theme, Role::Red),
            tone_linear(&theme, Role::Green)
        );
    }

    #[test]
    fn a_value_takes_the_form_that_goes_with_the_themes_brightness_and_keeps_its_own_opacity() {
        use pencere_view::Rgba;
        let colour = Rgba::rgb(0x10, 0x20, 0x30).or_dark(Rgba::new(0xe0, 0xd0, 0xc0, 0x80));
        // The dark form on a dark theme, with the value's opacity under the page's.
        let dark = resolve(&Theme::BATERI, colour);
        let [r, g, b, _] = LinearRgba::from_srgb(0xe0, 0xd0, 0xc0).to_array();
        assert_eq!(dark[..3], [r, g, b]);
        assert!((dark[3] - 128.0 / 255.0).abs() < 1e-6, "{dark:?}");
        // The light form on a light theme, solid.
        let light = resolve(&Theme::BATERI_LIGHT, colour);
        let [r, g, b, _] = LinearRgba::from_srgb(0x10, 0x20, 0x30).to_array();
        assert_eq!(light, [r, g, b, 1.0]);
        // A page's alpha on top of a value's.
        let faded = resolve(
            &Theme::BATERI,
            Color {
                alpha: 0.5,
                ..Rgba::new(1, 2, 3, 128).solid()
            },
        );
        assert!((faded[3] - 0.5 * 128.0 / 255.0).abs() < 1e-6, "{faded:?}");
        // A value that gives no dark form is the same on a dark theme.
        let same = Rgba::rgb(9, 9, 9).solid();
        assert_eq!(
            resolve(&Theme::BATERI, same)[..3],
            resolve(&Theme::BATERI_LIGHT, same)[..3]
        );
    }

    /// A line of `text` in `color` standing on `grounds`, nearest first.
    fn run(color: Color, grounds: Vec<Color>) -> Run {
        Run {
            origin: Point::new(0.0, 0.0),
            text: "text".to_owned(),
            font: Font {
                family: Family::System,
                size: 13.0,
                weight: 400,
                italic: false,
            },
            color,
            grounds,
            clip: Rect::new(0.0, 0.0, 100.0, 20.0),
            underline: false,
            strike: false,
        }
    }

    #[test]
    fn a_word_is_held_to_the_floor_against_the_fill_it_stands_on_and_not_the_panes_ground() {
        use pencere_view::Rgba;
        let theme = Theme::BATERI;
        // Text that reads on the pane's ground stands too close to a fill of its own colour.
        let text = Rgba::rgb(0xe0, 0xe0, 0xe0).solid();
        let on_ground = run(text, Vec::new());
        assert_eq!(
            ink(&theme, &on_ground, 3.0),
            resolve(&theme, text),
            "it reads on the ground"
        );
        let on_fill = run(text, vec![Rgba::rgb(0xe8, 0xe8, 0xe8).solid()]);
        let moved = ink(&theme, &on_fill, 3.0);
        assert_ne!(moved, resolve(&theme, text), "it does not read on its fill");
        let fill = LinearRgba::from_srgb(0xe8, 0xe8, 0xe8);
        let [r, g, b, a] = moved;
        assert_eq!(a, 1.0, "the colour moves, its opacity does not");
        // The moved colour is the grid's rule applied to the same two colours.
        assert_eq!([r, g, b], {
            let [r, g, b, _] = LinearRgba::from_srgb(0xe0, 0xe0, 0xe0)
                .legible_on(fill, 3.0)
                .to_array();
            [r, g, b]
        });
        // The rule is off at 1.
        assert_eq!(ink(&theme, &on_fill, 1.0), resolve(&theme, text));
    }

    #[test]
    fn the_fills_under_a_word_are_laid_over_each_other_from_the_farthest_to_the_nearest() {
        use pencere_view::Rgba;
        let theme = Theme::BATERI;
        // A half-clear white over a solid black card is a grey; the card over the ground
        // would be black. Red text reads against the grey, not against either.
        let grounds = vec![
            Color {
                alpha: 0.5,
                ..Rgba::rgb(0xff, 0xff, 0xff).solid()
            },
            Rgba::rgb(0, 0, 0).solid(),
        ];
        let seen = ground(&theme, &run(Role::Foreground.solid(), grounds));
        let mix = LinearRgba::from_srgb(0xff, 0xff, 0xff).over(0.5, LinearRgba::from_srgb(0, 0, 0));
        assert_eq!(seen, mix);
        // With no fill at all the ground is the pane's; a clear fill leaves it as it was.
        let bare = ground(&theme, &run(Role::Foreground.solid(), Vec::new()));
        assert_eq!(bare, theme.background_linear());
        let clear = ground(
            &theme,
            &run(Role::Foreground.solid(), vec![Role::Accent.alpha(0.0)]),
        );
        assert_eq!(clear, theme.background_linear());
    }

    /// The container list, laid out and painted with a stand-in for the renderer's text.
    fn container_list(size: Size) -> DrawList {
        struct Mono;
        impl Measure for Mono {
            fn measure(&mut self, font: &Font, text: &str) -> Metrics {
                Metrics {
                    width: text.chars().count() as f32 * font.size * 0.55,
                    height: font.size * 1.3,
                }
            }
        }
        let laid = pencere_view::layout(Containers::fixture().view(size), size, 1.0, &mut Mono);
        laid.paint(&mut Mono)
    }

    /// WCAG 2's ratio between a word as it shows — the colour it is drawn in, at the opacity it
    /// is drawn with, laid on what it stands on — and what it stands on. A guard that read the
    /// colour alone would call a word that fades into its ground legible.
    fn read_against(theme: &Theme, run: &Run, drawn: [f32; 4]) -> f64 {
        let under = ground(theme, run);
        let [r, g, b, a] = drawn;
        // Straight RGBA: the colour comes out as its share of the colour and the rest of the ground.
        let seen = {
            let [ur, ug, ub, _] = under.to_array();
            let mix = |top: f32, below: f32| top * a + below * (1.0 - a);
            [mix(r, ur), mix(g, ug), mix(b, ub), 1.0]
        };
        let weigh = |c: [f32; 4]| {
            0.2126 * f64::from(c[0]) + 0.7152 * f64::from(c[1]) + 0.0722 * f64::from(c[2])
        };
        let (word, under) = (weigh(seen), f64::from(under.luminance()));
        (word.max(under) + 0.05) / (word.min(under) + 0.05)
    }

    #[test]
    fn a_word_that_is_clear_enough_to_vanish_is_held_to_the_floor_as_it_shows() {
        use pencere_view::Rgba;
        let theme = Theme::BATERI;
        // White is far from the black ground, a fifth of it is not: the colour passes the floor
        // and the word does not.
        let faint = Color {
            alpha: 0.05,
            ..Rgba::rgb(0xff, 0xff, 0xff).solid()
        };
        let word = run(faint, Vec::new());
        let said = resolve(&theme, faint);
        assert!(
            read_against(&theme, &word, said) < 3.0,
            "the word as said does not read"
        );
        let held = ink(&theme, &word, 3.0);
        assert_eq!(held[3], 1.0, "drawn solid, in the colour it shows");
        assert!(
            read_against(&theme, &word, held) >= 3.0 - 1e-3,
            "and it reads"
        );
        assert!(
            read_against(&theme, &word, held) < 3.0 + 1e-2,
            "only as far as it takes"
        );
        // One that reads as it shows keeps its colour and its opacity.
        let firm = Color {
            alpha: 0.8,
            ..Rgba::rgb(0xff, 0xff, 0xff).solid()
        };
        let word = run(firm, Vec::new());
        assert_eq!(ink(&theme, &word, 3.0), resolve(&theme, firm));
        // On a light ground too, where the clear colour fades toward white.
        let light = Theme::BATERI_LIGHT;
        let faint = Color {
            alpha: 0.15,
            ..Rgba::rgb(0, 0, 0).solid()
        };
        let word = run(faint, Vec::new());
        let held = ink(&light, &word, 3.0);
        assert!(read_against(&light, &word, held) >= 3.0 - 1e-3, "{held:?}");
        // The rule is off at 1.
        assert_eq!(ink(&light, &word, 1.0), resolve(&light, faint));
    }

    #[test]
    fn every_word_of_the_container_list_reads_at_the_floor_in_every_theme() {
        let sizes = [Size::new(1400.0, 800.0), Size::new(700.0, 600.0)];
        let mut words = 0;
        for name in Theme::embedded_names() {
            let theme = Theme::embedded(name).expect("an embedded theme");
            for size in sizes {
                for item in &container_list(size).items {
                    let Paint::Text(run) = item else { continue };
                    words += 1;
                    let ratio = read_against(&theme, run, ink(&theme, run, MINIMUM_CONTRAST));
                    assert!(
                        ratio >= MINIMUM_CONTRAST - 1e-3,
                        "{name}: {:?} on {:?} reads at {ratio:.2}",
                        run.color.source,
                        run.grounds.first().map(|g| g.source)
                    );
                }
            }
        }
        assert!(words > 1000, "the themes' pages had words: {words}");
    }

    #[test]
    fn the_floor_moves_a_word_only_where_it_cannot_be_read_and_only_as_far_as_it_takes() {
        let theme = Theme::BATERI_LIGHT;
        let mut moved = 0;
        for item in &container_list(Size::new(1400.0, 800.0)).items {
            let Paint::Text(run) = item else { continue };
            let said = resolve(&theme, run.color);
            let held = ink(&theme, run, MINIMUM_CONTRAST);
            if read_against(&theme, run, said) >= MINIMUM_CONTRAST {
                assert_eq!(held, said, "a word that reads is drawn as it is said");
            } else {
                moved += 1;
                let ratio = read_against(&theme, run, held);
                assert!(
                    (ratio - MINIMUM_CONTRAST).abs() < 1e-2,
                    "just far enough: {ratio:.3}"
                );
            }
        }
        assert!(moved > 0, "the light theme's quiet ink is below the floor");
    }

    #[test]
    fn a_page_follows_the_theme_it_is_painted_in() {
        let on = |theme: &Theme| resolve(theme, Role::Raised(1).solid());
        assert_ne!(on(&Theme::BATERI), on(&Theme::BATERI_LIGHT));
    }

    #[test]
    fn a_box_goes_to_the_renderer_with_everything_it_has_moved_by_the_origin() {
        let theme = Theme::BATERI;
        let drawn = Quad {
            rect: Rect::new(10.0, 20.0, 30.0, 40.0),
            radii: [1.0, 2.0, 3.0, 4.0],
            fill: Some(Role::Accent.alpha(0.5)),
            border: Some(Border {
                width: 2.0,
                color: Role::Border.solid(),
            }),
            shadow: Some(Shadow {
                dx: 0.0,
                dy: 3.0,
                blur: 6.0,
                color: Role::Shadow.alpha(0.4),
            }),
            clip: Rect::new(0.0, 0.0, 100.0, 200.0),
        };
        let expected = UiQuad::new(UiRect::new(10.0, 52.0, 30.0, 40.0))
            .radii([1.0, 2.0, 3.0, 4.0])
            .clip(UiRect::new(0.0, 32.0, 100.0, 200.0))
            .fill(resolve(&theme, Role::Accent.alpha(0.5)))
            .border(2.0, resolve(&theme, Role::Border.solid()))
            .shadow(0.0, 3.0, 6.0, resolve(&theme, Role::Shadow.alpha(0.4)));
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
            minimum_contrast: MINIMUM_CONTRAST,
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
        use pencere_view::{Button, Menu, Mods};
        let place = Placement {
            inset: 32.0,
            zoom: 1.0,
        };
        let moved = into_page(Input::Move(Point::new(10.0, 50.0)), place);
        assert_eq!(moved, Input::Move(Point::new(10.0, 18.0)));
        let pressed = into_page(
            Input::Press {
                at: Point::new(1.0, 40.0),
                button: Button::Right,
                mods: Mods::default(),
                count: 2,
            },
            place,
        );
        assert!(matches!(
            pressed,
            Input::Press { at, count: 2, .. } if at == Point::new(1.0, 8.0)
        ));
        assert_eq!(into_page(Input::Leave, place), Input::Leave);
        let reply = Reply {
            menu: Some(Menu {
                at: Point::new(4.0, 8.0),
                entries: Vec::new(),
            }),
            ..Reply::default()
        };
        assert_eq!(
            from_pane(reply, place).menu.map(|menu| menu.at),
            Some(Point::new(4.0, 40.0))
        );
    }

    #[test]
    fn a_page_drawn_as_large_as_the_terminals_text_has_fewer_points_in_the_same_pane() {
        use pencere_view::Menu;
        // A terminal at 15 points draws the kit's 12.5 at 1.2.
        let place = Placement {
            inset: 32.0,
            zoom: Placement::zoom_for(15.0),
        };
        assert!((place.zoom - 1.2).abs() < 1e-6);
        assert_eq!(Placement::zoom_for(f64::NAN), 1.0);
        assert_eq!(Placement::zoom_for(1000.0), 4.0, "within the range");
        let moved = into_page(Input::Move(Point::new(12.0, 32.0 + 24.0)), place);
        assert_eq!(moved, Input::Move(Point::new(10.0, 20.0)));
        let wheel = into_page(
            Input::Scroll {
                at: Point::new(0.0, 32.0),
                dx: 6.0,
                dy: 12.0,
            },
            place,
        );
        assert!(matches!(wheel, Input::Scroll { dx, dy, .. } if (dx, dy) == (5.0, 10.0)));
        let reply = Reply {
            menu: Some(Menu {
                at: Point::new(10.0, 20.0),
                entries: Vec::new(),
            }),
            ..Reply::default()
        };
        let at = from_pane(reply, place).menu.map(|menu| menu.at).unwrap();
        assert!(
            (at.x - 12.0).abs() < 1e-4 && (at.y - 56.0).abs() < 1e-4,
            "{at:?}"
        );
        let room = place.to_pane(Rect::new(10.0, 10.0, 100.0, 20.0));
        let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
        assert!(
            near(room.x, 12.0) && near(room.y, 44.0) && near(room.w, 120.0) && near(room.h, 24.0),
            "{room:?}"
        );
    }

    #[test]
    fn a_colour_taken_toward_another_is_mixed_in_srgb_in_the_theme() {
        let theme = Theme::BATERI;
        let dim = resolve(&theme, Role::Dim.solid());
        let fore = resolve(&theme, Role::Foreground.solid());
        assert_eq!(
            resolve(&theme, Role::Dim.solid().toward(Role::Foreground, 0.0)),
            dim
        );
        let whole = resolve(&theme, Role::Dim.solid().toward(Role::Foreground, 1.0));
        for at in 0..3 {
            assert!((whole[at] - fore[at]).abs() < 0.01, "{whole:?} {fore:?}");
        }
        let half = resolve(&theme, Role::Dim.alpha(0.5).toward(Role::Foreground, 0.5));
        assert!(
            (half[3] - 0.5).abs() < 1e-6,
            "the opacity is on top of the mix"
        );
        for at in 0..3 {
            let (low, high) = (dim[at].min(fore[at]), dim[at].max(fore[at]));
            assert!(
                half[at] >= low && half[at] <= high,
                "between the two: {half:?}"
            );
        }
    }

    #[test]
    fn the_lines_under_and_through_words_are_as_wide_as_the_words_and_below_and_across_them() {
        let line = ShapedText {
            glyphs: Vec::new(),
            width: 80.0,
            ascent: 24.0,
            descent: 6.0,
            leading: 0.0,
        };
        let mut said = run(Role::Foreground.solid(), Vec::new());
        assert!(decorations(&said, &line).is_empty());
        said.underline = true;
        said.strike = true;
        let rules = decorations(&said, &line);
        assert_eq!(rules.len(), 2);
        let (under, through) = (rules[0], rules[1]);
        assert_eq!((under.w, under.h), (80.0, 2.0));
        assert!(
            under.y > 24.0 && under.y < 30.0,
            "under the baseline: {under:?}"
        );
        assert!(
            through.y > 6.0 && through.y < 24.0,
            "across the letters: {through:?}"
        );
    }

    #[test]
    fn the_field_is_the_room_the_page_leaves_moved_below_the_strip_with_the_words_in_it() {
        struct Fixed;
        impl Measure for Fixed {
            fn measure(&mut self, font: &Font, text: &str) -> Metrics {
                #[allow(clippy::cast_precision_loss)]
                let width = text.chars().count() as f32 * font.size * 0.5;
                Metrics {
                    width,
                    height: font.size * 1.25,
                }
            }
        }
        let page = |hint: Option<&str>| {
            let room = pencere_view::div()
                .text_field(Id::named("filter"))
                .w(120.0)
                .h(18.0);
            let room = match hint {
                Some(hint) => room.child(pencere_view::text(hint)),
                None => room,
            };
            pencere_view::div().p(10.0).child(room)
        };
        let laid = pencere_view::layout(
            page(Some("Filter images")),
            Size::new(400.0, 100.0),
            1.0,
            &mut Fixed,
        );
        let at = |inset| Placement { inset, zoom: 1.0 };
        assert_eq!(
            Outline::of(&laid, at(32.0)).fields,
            [Field {
                id: Id::named("filter"),
                kind: FieldKind::Line,
                rect: Rect::new(10.0, 42.0, 120.0, 18.0),
                hint: Some("Filter images".to_owned()),
                label: None,
            }]
        );
        let laid = pencere_view::layout(page(None), Size::new(400.0, 100.0), 1.0, &mut Fixed);
        assert_eq!(
            Outline::of(&laid, at(0.0)).fields[0].hint,
            None,
            "no words, no hint"
        );
        let laid = pencere_view::layout(
            pencere_view::div(),
            Size::new(400.0, 100.0),
            1.0,
            &mut Fixed,
        );
        assert!(
            Outline::of(&laid, at(0.0)).fields.is_empty(),
            "no room, no field"
        );
    }

    #[test]
    fn the_keyboard_is_answered_before_the_first_frame() {
        let (_page, handle) = containers_demo(None, 32.0);
        let escape = Input::Key {
            key: pencere_view::Key::Escape,
            mods: pencere_view::Mods::default(),
        };
        assert!(handle.input(escape).close);
        assert!(
            handle.fields().is_empty(),
            "no frame, no place for the field"
        );
        // A pointer over a page that was never laid out is over nothing.
        assert!(!handle.input(Input::Move(Point::new(5.0, 50.0))).redraw);
    }

    #[test]
    fn a_page_of_this_process_answers_at_once_and_the_pane_takes_the_answers_by_draining() {
        let (_page, handle) = containers_demo(None, 32.0);
        let drive: &dyn PageDrive = &handle;
        drive.send_input(Input::Move(Point::new(5.0, 50.0)));
        drive.send_input(Input::Key {
            key: pencere_view::Key::Escape,
            mods: pencere_view::Mods::default(),
        });
        drive.send_text(containers::FIELD, "api");
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
                minimum_contrast: MINIMUM_CONTRAST,
            })
        };
        let (mut page, handle) = containers_demo(None, 32.0);
        let told: Rc<RefCell<Vec<Vec<Field>>>> = Rc::default();
        {
            let told = Rc::clone(&told);
            handle.on_outline(move |outline| told.borrow_mut().push(outline.fields.clone()));
        }
        assert_eq!(
            told.borrow().as_slice(),
            &[Vec::<Field>::new()],
            "told at once"
        );
        build(&mut page, 1.0, false);
        let field = handle
            .fields()
            .first()
            .expect("the filter's field has a place")
            .rect;
        assert_eq!(
            told.borrow().last().cloned(),
            Some(vec![Field {
                id: containers::FIELD,
                kind: FieldKind::Line,
                rect: field,
                hint: Some(containers::FIELD_HINT.to_owned()),
                label: None,
            }]),
            "its place, and what the page says the field says while it is empty"
        );
        assert!(field.y >= 32.0, "below the strip: {field:?}");
        // Laid out again the same way, the host is not told again.
        build(&mut page, 1.1, false);
        assert_eq!(told.borrow().len(), 2);
        assert!(!page.animating());

        // A press on the field's box asks for the keyboard for the host's field.
        let middle = Point::new(field.x + field.w / 2.0, field.y + field.h / 2.0);
        let press = handle.input(Input::Press {
            at: middle,
            button: pencere_view::Button::Left,
            mods: pencere_view::Mods::default(),
            count: 1,
        });
        assert_eq!(
            press.keyboard,
            Some(pencere_view::Keyboard::Field(containers::FIELD))
        );

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
