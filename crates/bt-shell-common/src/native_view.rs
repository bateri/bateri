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

use std::collections::HashMap;

use bt_core::{LinearRgba, Theme};
use bt_gpu::{
    Renderer, ShapedText, TextFamily, TextFontId, TextStyle, UiContext, UiList, UiPage, UiQuad,
    UiRect, tint,
};
use bt_view::{Color, DrawList, Element, Family, Font, Measure, Metrics, Paint, Quad, Size, Tone};

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

/// The container-list page of the engine's demo (`bt_view::demo`) as a pane's page: fixed
/// data, laid out again for every size the pane has. `terminal` is the pane's font family;
/// `inset` the points at the top the application keeps for its own strip.
pub fn containers_demo(terminal: Option<String>, inset: f32) -> Box<dyn UiPage> {
    let page = bt_view::demo::Containers::fixture();
    Box::new(EnginePage::new(terminal, inset, move |size| {
        page.view(size)
    }))
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
        };
        let mut page = containers_demo(None, 32.0);
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
}
