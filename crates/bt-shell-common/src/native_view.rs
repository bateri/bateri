//! A page of the UI kit in a pane: the host's part of it. The page is driven by pencere's drive
//! (`pencere_drive::Page`, the kit's specification, §4, written once for every host): what the
//! person does told to the page, its frames laid out with its editors and its scrolling, its wakes,
//! and what it said handed back in one order. bateri keeps no copy of it. What is here is what only
//! the pane can do — the rows of the drive's README marked "the host's":
//!
//! - **Measuring and drawing text in its fonts, the draw list in its renderer and theme.** Text is
//!   shaped and measured by the renderer ([`bt_gpu::Renderer::ui_shape`]) — the object that
//!   measures is the object that draws, so the width the layout was given is the width of the
//!   glyphs on screen, and the terminal's own face is the pane's font family. **Colours** are the
//!   theme's: the engine names a [`Role`], the theme says what it is ([`tone_linear`]) — the
//!   arithmetic of colours stays in `bt_core::Theme`, a page keeps none, and a theme change is a
//!   new list and nothing else. A colour the page gives as a value takes the form that goes with
//!   the theme's brightness ([`resolve`]). **The words of a page are held to the contrast floor of
//!   the program's words in the grid** ([`ink`]): the page is the program's, and what it writes is
//!   read against the fills under it, not against the pane's ground alone. **Figures** — the paths
//!   of a canvas — are drawn into pixels by `pencere-sandbox` ([`draw_figure`], the one rasteriser
//!   every host of the kit draws them with) in the theme's colours, and drawn as an image is
//!   ([`figure`]); **images**, decoded by the drive's `Images` off the thread that draws, are
//!   textured rectangles ([`bt_gpu::UiImage`], [`image`]). The list is the renderer's
//!   ([`bt_gpu::UiList`]), in pixels, drawn in place of the terminal's grid while the page is up
//!   ([`bt_gpu::DisplayLink::set_page`]). The page as the link's `UiPage` and the renderer as its
//!   measure are `page`'s.
//! - **Its keyboard, its input method, its fields and its screen reader**: the handle the pane
//!   reaches the page and its editors by ([`PageHandle`], [`PageEditors`]) and the outline it is
//!   told ([`PageHandle::watch_outline`]). The keys themselves, the pasteboard, the pointer's
//!   shapes, the menus and the browser are `bt-shell-macos`'s, as its `pencere_drive::Host`.
//! - **The program and the session it speaks through**: the files of the view's bundle
//!   ([`BundleAssets`]), and the program's flow and the frames' clock a page is given ([`Opening`]).
//! - **The demo of a debug build**: the container list ([`containers_demo`]), a page written in
//!   Rust and run by the drive as a module is.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use bt_core::{LinearRgba, Session, Theme};
use bt_gpu::{
    Renderer, ShapedText, TextFamily, TextFontId, TextStyle, UiBitmap, UiImage, UiList, UiQuad,
    UiRect, tint,
};

mod containers;
mod page;
#[cfg(test)]
mod pane_tests;
pub use page::{Opening, PageEditors, PageHandle, containers_demo, module_page, script_page};
pub use pencere_drive::{
    Drained, Field, Host, Outline, Placement, Route, Scrolling, Tip, ZOOM_TEXT,
};
pub use pencere_sandbox::{Assets, Fault, ImageLimits, ModuleLimits, Script};
use pencere_sandbox::{Images, draw_figure};
pub use pencere_view::{
    Accessible, AccessibleRole, Bars, Button, Command, Cursor, FieldKind, Id, Input, Key, Keyboard,
    Menu, MenuEntry, Mods, Phase, Point, Rect, Role, Shortcut, State,
};
use pencere_view::{
    Color, DrawList, Family, Figure, Font, Measure, Metrics, Paint, Picture, Quad, Run, Source,
};

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
///
/// **The opacity of its element is laid over what is held** ([`Run::opacity`]: the page's
/// `opacity` on it and the boxes that hold it, and what a motion fades): a word fades with its
/// box all the way, as the box's fill does, instead of stopping at the floor and vanishing at
/// once, and a page can make words fainter than the floor.
pub fn ink(theme: &Theme, run: &Run, minimum_contrast: f64) -> [f32; 4] {
    let (colour, alpha) = parts(theme, run.color);
    let under = ground(theme, run);
    let shown = colour.over(alpha, under);
    let held = shown.legible_on(under, minimum_contrast);
    let opacity = run.opacity.clamp(0.0, 1.0);
    if held == shown {
        return tint(colour, alpha * opacity);
    }
    tint(held, opacity)
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
    if let Some(gradient) = quad.gradient {
        out = out.gradient(
            gradient.angle,
            resolve(theme, gradient.from),
            resolve(theme, gradient.to),
        );
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

/// The paths of a canvas as the renderer's image: drawn into pixels in `theme`'s colours by the kit's
/// own rasteriser ([`draw_figure`]: a colour resolved as a fill is, in sRGB, the figure's
/// opacity on it), placed at the figure's box moved to `origin` pixels and cut at its clip.
/// `None` for a figure with no room or nothing drawn.
pub fn figure(
    theme: &Theme,
    figure: &Figure,
    origin: (f32, f32),
) -> Option<(pencere_sandbox::Bitmap, UiImage)> {
    let bitmap = draw_figure(figure, |color| {
        let (colour, alpha) = parts(theme, color);
        let [r, g, b, _] = colour.to_srgb();
        [r, g, b, alpha]
    })?;
    let at = |rect: Rect| UiRect::new(rect.x + origin.0, rect.y + origin.1, rect.w, rect.h);
    // The pixels are the box's, whole pixels rounded up: drawn at their own size, a pixel to a
    // pixel.
    #[allow(clippy::cast_precision_loss)]
    let rect = Rect::new(
        figure.rect.x,
        figure.rect.y,
        bitmap.width as f32,
        bitmap.height as f32,
    );
    let image = UiImage::new(at(rect)).clip(at(figure.clip));
    Some((bitmap, image))
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
/// did, so a line measured to be laid out is not shaped again to be drawn. The page's pictures
/// are the drive's to measure (`pencere_sandbox::Images`), not the renderer's.
struct Typeset<'a> {
    renderer: &'a Renderer,
    /// The terminal's face: what `Family::Terminal` is.
    terminal: &'a Option<String>,
    shaped: &'a mut Shaped,
}

impl Typeset<'_> {
    /// The renderer's font for `font`, opened the first time it is asked for.
    fn font(&mut self, font: &Font) -> Option<TextFontId> {
        let (renderer, terminal) = (self.renderer, self.terminal);
        *self.shaped.fonts.entry(*font).or_insert_with(|| {
            renderer.ui_font(&TextStyle {
                family: match font.family {
                    Family::System => TextFamily::System,
                    Family::Terminal => TextFamily::Terminal(terminal.clone()),
                },
                size_px: f64::from(font.size),
                weight: font.weight,
                italic: font.italic,
            })
        })
    }

    fn shaped(&mut self, font: &Font, text: &str) -> Option<&ShapedText> {
        let renderer = self.renderer;
        let id = self.font(font)?;
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

    /// Where an editor's carets stand in a line: the renderer's own carets of the line shaped once
    /// ([`Renderer::ui_carets`], inside a ligature too), not each start of it shaped and kept in
    /// the cache of lines — an editor asks this of every line it lays out, with every place in it.
    fn offsets(&mut self, font: &Font, text: &str, at: &[usize]) -> Vec<f32> {
        let renderer = self.renderer;
        #[allow(clippy::cast_possible_truncation)]
        self.font(font)
            .and_then(|id| renderer.ui_carets(id, text, at))
            .map_or_else(
                || vec![0.0; at.len()],
                |carets| carets.into_iter().map(|x| x as f32).collect(),
            )
    }
}

/// The draw list on the renderer's, at `origin` pixels of the surface, its words held to
/// `minimum_contrast` ([`ink`]) and its pictures the decoded files of `images`, the page's.
fn to_list(
    theme: &Theme,
    minimum_contrast: f64,
    typeset: &mut Typeset<'_>,
    images: Option<&Images>,
    draws: &DrawList,
    origin: (f32, f32),
) -> UiList {
    let renderer = typeset.renderer;
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
                let Some(bitmap) =
                    images.and_then(|images| images.bitmap(&picture.src, picture.full()))
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
            Paint::Figure(drawn) => {
                let Some((bitmap, image)) = figure(theme, drawn, origin) else {
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
                    image,
                );
            }
            // An editor's place that no editor filled (the drive's editors fill every one they
            // have): its room's fill, under it, is what shows.
            Paint::Editor(_) => {}
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

#[cfg(test)]
mod tests {
    use super::*;
    use bt_core::MINIMUM_CONTRAST;
    use containers::Containers;
    use pencere_view::{Border, Gradient, Rect, Shadow, Size, path};

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
            opacity: 1.0,
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
    fn a_word_fades_with_its_element_past_the_floor() {
        use pencere_view::Rgba;
        let theme = Theme::BATERI;
        let fill = vec![Rgba::rgb(0xe8, 0xe8, 0xe8).solid()];
        // Held to the floor or not, the word is drawn as faint as its element is: a row that
        // goes takes its words all the way with it.
        for (text, held) in [
            (Rgba::rgb(0xe0, 0xe0, 0xe0).solid(), true),
            (Rgba::rgb(0x20, 0x20, 0x20).solid(), false),
        ] {
            let whole = ink(&theme, &run(text, fill.clone()), 3.0);
            let faint = ink(
                &theme,
                &Run {
                    opacity: 0.25,
                    ..run(text, fill.clone())
                },
                3.0,
            );
            assert_eq!(faint[..3], whole[..3], "its colour, held or not");
            assert_eq!(faint[3], whole[3] * 0.25, "held: {held}");
            assert_ne!(whole == resolve(&theme, text), held);
        }
        let gone = Run {
            opacity: 0.0,
            ..run(Rgba::rgb(0xe0, 0xe0, 0xe0).solid(), fill)
        };
        assert_eq!(ink(&theme, &gone, 3.0)[3], 0.0, "nothing of it at nothing");
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
            gradient: Some(Gradient {
                angle: 90.0,
                from: Role::Accent.solid(),
                to: Role::Background.alpha(0.5),
            }),
        };
        let expected = UiQuad::new(UiRect::new(10.0, 52.0, 30.0, 40.0))
            .radii([1.0, 2.0, 3.0, 4.0])
            .clip(UiRect::new(0.0, 32.0, 100.0, 200.0))
            .fill(resolve(&theme, Role::Accent.alpha(0.5)))
            .gradient(
                90.0,
                resolve(&theme, Role::Accent.solid()),
                resolve(&theme, Role::Background.alpha(0.5)),
            )
            .border(2.0, resolve(&theme, Role::Border.solid()))
            .shadow(0.0, 3.0, 6.0, resolve(&theme, Role::Shadow.alpha(0.4)));
        assert_eq!(quad(&theme, &drawn, (0.0, 32.0)), expected);
    }

    #[test]
    fn a_figure_is_drawn_in_the_themes_colours_at_its_box_a_pixel_to_a_pixel() {
        let theme = Theme::BATERI;
        let ring = path().circle(10.0, 10.0, 8.0).stroke(2.0, Role::Accent);
        let drawn = Figure {
            rect: Rect::new(4.0, 6.0, 39.5, 40.0),
            scale: 2.0,
            paths: vec![ring],
            clip: Rect::new(0.0, 0.0, 30.0, 30.0),
        };
        let (bitmap, image) = figure(&theme, &drawn, (0.0, 32.0)).expect("drawn");
        // Whole pixels, the box's rounded up.
        assert_eq!((bitmap.width, bitmap.height), (40, 40));
        assert_eq!(
            image,
            UiImage::new(UiRect::new(4.0, 38.0, 40.0, 40.0))
                .clip(UiRect::new(0.0, 32.0, 30.0, 30.0))
        );
        // The ring is the accent, as the theme says it, in sRGB: at its top, (20, 4) pixels.
        let at = |x: u32, y: u32| {
            let i = ((y * bitmap.width + x) * 4) as usize;
            [
                bitmap.pixels()[i],
                bitmap.pixels()[i + 1],
                bitmap.pixels()[i + 2],
                bitmap.pixels()[i + 3],
            ]
        };
        let [r, g, b, _] = theme.accent_linear().to_srgb();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let srgb = |c: f32| (c * 255.0).round() as u8;
        let ink = at(20, 4);
        assert_eq!(ink[3], 255);
        assert!(
            ink[0].abs_diff(srgb(r)) <= 1
                && ink[1].abs_diff(srgb(g)) <= 1
                && ink[2].abs_diff(srgb(b)) <= 1
        );
        assert_eq!(at(20, 20), [0, 0, 0, 0]);
        // In another theme, another bitmap.
        let (other, _) = figure(&Theme::BATERI_LIGHT, &drawn, (0.0, 0.0)).expect("drawn");
        assert_ne!(other.id, bitmap.id);
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
}
