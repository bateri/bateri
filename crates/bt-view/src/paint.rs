//! What a laid-out page draws, as a flat list in painter's order: later items cover earlier
//! ones. Everything is in **pixels** of the page, top-left origin, and colours are still
//! names ([`Color`]): the host resolves them in the pane's theme and turns the items into
//! whatever its renderer draws.

use crate::layout::{Content, Laid, Node, TextLine};
use crate::style::{Border, Shadow};
use crate::{Align, Color, Font, Measure, Point, Rect};

/// A box: an optional fill, border (drawn inside, over the fill) and soft shadow (never
/// under the box), cut at `clip`. `radii` are top-left, top-right, bottom-right, bottom-left.
#[derive(Clone, Debug, PartialEq)]
pub struct Quad {
    pub rect: Rect,
    pub radii: [f32; 4],
    pub fill: Option<Color>,
    pub border: Option<Border>,
    pub shadow: Option<Shadow>,
    pub clip: Rect,
}

/// One line of text: `origin` is the top-left of its line box. Already cut to fit its box
/// (ending in "…" where it did not) — what the host shapes is what to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub origin: Point,
    pub text: String,
    pub font: Font,
    pub color: Color,
    pub clip: Rect,
}

/// One thing the page draws.
#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    Quad(Quad),
    Text(Run),
}

/// A page's draw list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawList {
    pub items: Vec<Paint>,
}

/// How much of a sparkline's colour the area under it is.
const SPARK_AREA: f32 = 0.18;
/// A sparkline holds this many samples across its width: older ones are cut, a shorter
/// history leaves the left of the box empty.
const SPARK_SLOTS: usize = 30;

impl Laid {
    /// The page's draw list. `measure` is asked again for a line that has to be cut to fit.
    pub fn paint(&self, measure: &mut dyn Measure) -> DrawList {
        let mut list = DrawList::default();
        for node in &self.nodes {
            if node.hidden || node.rect.w <= 0.0 || node.rect.h <= 0.0 {
                continue;
            }
            if node.fill.is_some() || node.border.is_some() || node.shadow.is_some() {
                list.items.push(Paint::Quad(Quad {
                    rect: node.rect,
                    radii: node.radii,
                    fill: node.fill,
                    border: node.border,
                    shadow: node.shadow,
                    clip: node.clip,
                }));
            }
            match &node.content {
                Content::None => {}
                Content::Text(line) => {
                    if let Some(run) = place_text(node, line, measure) {
                        list.items.push(Paint::Text(run));
                    }
                }
                Content::Spark(spark) => {
                    let clip = node.clip.intersect(node.rect);
                    paint_spark(
                        &mut list,
                        node.rect,
                        clip,
                        &spark.values,
                        spark.color,
                        spark.peak,
                        self.scale,
                    );
                }
            }
        }
        list
    }
}

/// Where a line stands in its box, cut where it does not fit and is allowed to be cut.
fn place_text(node: &Node, line: &TextLine, measure: &mut dyn Measure) -> Option<Run> {
    if line.text.is_empty() {
        return None;
    }
    let whole = measure.measure(&line.font, &line.text);
    if whole.width <= 0.0 {
        // The host draws no words (no proportional text): nothing to place.
        return None;
    }
    let box_width = node.rect.w;
    let (text, width) = if line.truncate && whole.width > box_width + 1.0 {
        cut(&line.text, &line.font, box_width, measure)?
    } else {
        (line.text.clone(), whole.width)
    };
    let x = match line.align {
        Align::Start => node.rect.x,
        Align::Center => node.rect.x + (box_width - width) / 2.0,
        Align::End => node.rect.right() - width,
    };
    let y = node.rect.y + (node.rect.h - whole.height) / 2.0;
    // A line that may be cut stays in its box; the rest is the boxes around it.
    let clip = if line.truncate {
        node.clip.intersect(node.rect)
    } else {
        node.clip
    };
    Some(Run {
        origin: Point::new(x, y),
        text,
        font: line.font,
        color: line.color,
        clip,
    })
}

/// The longest start of `text` that fits `budget` pixels with a "…" after it; `None` when not
/// even the "…" fits.
fn cut(text: &str, font: &Font, budget: f32, measure: &mut dyn Measure) -> Option<(String, f32)> {
    const ELLIPSIS: char = '\u{2026}';
    let ends: Vec<usize> = text
        .char_indices()
        .map(|(at, c)| at + c.len_utf8())
        .collect();
    let with = |count: usize| {
        let start = if count == 0 { 0 } else { ends[count - 1] };
        let mut cut = text[..start].trim_end().to_owned();
        cut.push(ELLIPSIS);
        cut
    };
    // `fits` characters are known to fit (0: only the ellipsis, checked last), `over` is a
    // count known not to.
    let (mut fits, mut over) = (0_usize, ends.len());
    let mut best: Option<(String, f32)> = None;
    while fits + 1 < over {
        let middle = fits + (over - fits) / 2;
        let candidate = with(middle);
        let width = measure.measure(font, &candidate).width;
        if width <= budget {
            fits = middle;
            best = Some((candidate, width));
        } else {
            over = middle;
        }
    }
    if best.is_none() {
        let candidate = with(0);
        let width = measure.measure(font, &candidate).width;
        if width <= budget {
            best = Some((candidate, width));
        }
    }
    best
}

/// A sparkline in `rect`: the area under the line softly filled and the line a thin stroke on
/// top, in boxes one point wide — the kit draws boxes, and a line read at every point of its
/// width and drawn as a box per point (as tall as the line climbs or falls there) is the line
/// to the eye at a sparkline's size.
fn paint_spark(
    list: &mut DrawList,
    rect: Rect,
    clip: Rect,
    values: &[f32],
    color: Color,
    peak: Option<f32>,
    scale: f32,
) {
    let kept = &values[values.len().saturating_sub(SPARK_SLOTS)..];
    let Some(last) = kept.len().checked_sub(1) else {
        return;
    };
    let peak = peak
        .unwrap_or_else(|| kept.iter().copied().fold(0.0_f32, f32::max).max(1e-9) * 1.15)
        .max(1e-9);
    let offset = (SPARK_SLOTS - kept.len()) as f32;
    let stroke = (1.1 * scale).round().max(1.0);
    let margin = scale.max(1.0);
    let height = (rect.h - 2.0 * margin).max(0.0);
    let y_at = |v: f32| rect.y + margin + height * (1.0 - v.clamp(0.0, peak) / peak);
    // Where the line is `x` pixels from the box's left: between two samples, a straight line;
    // nothing before the first.
    let line_at = |x: f32| -> Option<f32> {
        let at = x / rect.w * (SPARK_SLOTS - 1) as f32 - offset;
        if at < 0.0 {
            return None;
        }
        let at = at.min(last as f32);
        let (i, along) = (at.floor() as usize, at.fract());
        let (a, b) = (kept[i], kept[(i + 1).min(last)]);
        Some(y_at(a + (b - a) * along))
    };
    let solid = |rect: Rect, color: Color| {
        Paint::Quad(Quad {
            rect,
            radii: [0.0; 4],
            fill: Some(color),
            border: None,
            shadow: None,
            clip,
        })
    };
    let area = Color {
        alpha: color.alpha * SPARK_AREA,
        ..color
    };
    let step = scale.round().max(1.0);
    let columns = (rect.w / step).floor() as usize;
    for column in 0..columns {
        let x = column as f32 * step;
        let (here, next) = (line_at(x), line_at(x + step));
        let (y0, y1) = match (here, next) {
            (Some(y0), Some(y1)) => (y0, y1),
            (None, Some(y1)) => (y1, y1),
            _ => continue,
        };
        let left = rect.x + x;
        let mid = (y0 + y1) / 2.0;
        let floor = rect.bottom();
        if floor > mid {
            list.items
                .push(solid(Rect::new(left, mid, step, floor - mid), area));
        }
        // The line: the rise or fall across this column, at least a stroke thick.
        let top = y0.min(y1) - stroke / 2.0;
        let bottom = y0.max(y1) + stroke / 2.0;
        list.items
            .push(solid(Rect::new(left, top, step, bottom - top), color));
    }
}
