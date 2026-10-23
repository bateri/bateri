//! Laying a page out: the element tree becomes a Taffy tree (flexbox and grid, in pixels),
//! the host measures the lines of text, and what comes back is every element's box in the
//! page's pixels, ready to be painted ([`Laid::paint`]).

use std::collections::HashMap;

use taffy::geometry::{Line, Point as TPoint, Rect as TRect, Size as TSize};
use taffy::style::{
    AlignItems, AvailableSpace, Dimension, Display as TDisplay, FlexDirection, FlexWrap,
    GridPlacement, GridTemplateComponent, JustifyContent, LengthPercentage, LengthPercentageAuto,
    MaxTrackSizingFunction, MinTrackSizingFunction, Overflow as TOverflow, Position,
    Style as TStyle,
};
use taffy::{NodeId, TaffyTree};

use crate::element::{Kind, Meta, Spark};
use crate::style::{
    Border, Direction, Display, Items, Justify, Length, Overflow, Shadow, Style, TextStyle, Track,
};
use crate::{Align, Color, Element, Family, Font, Id, Measure, Metrics, Point, Rect, Size, Tone};

/// The text a page is written in until a box says otherwise: the terminal's face at 12.5
/// points, regular, in the foreground, at the start of its box.
const DEFAULT_TEXT: TextStyle = TextStyle {
    family: Some(Family::Terminal),
    size: Some(12.5),
    weight: Some(400),
    color: Some(Color {
        tone: Tone::Foreground,
        alpha: 1.0,
    }),
    align: Some(Align::Start),
};

/// A line of text laid out.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TextLine {
    pub text: String,
    pub font: Font,
    pub color: Color,
    pub align: Align,
    pub truncate: bool,
}

/// What a laid-out element draws besides its box.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Content {
    None,
    Text(TextLine),
    Spark(Spark),
}

/// An element after layout: its box on the page, in pixels, and how it is drawn.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Node {
    /// The box, in the page's pixels.
    pub rect: Rect,
    /// What the elements around it let be drawn.
    pub clip: Rect,
    /// A box nothing of which is drawn (it, or an ancestor, is `hidden`).
    pub hidden: bool,
    pub content: Content,
    pub fill: Option<Color>,
    /// In pixels.
    pub border: Option<Border>,
    pub radii: [f32; 4],
    pub shadow: Option<Shadow>,
    /// The element that holds this one (its place in the paint order).
    pub parent: Option<usize>,
    pub meta: Meta,
    /// For a box that scrolls: how tall its content is and how tall the box is, pixels.
    pub extent: Option<(f32, f32)>,
}

/// A page laid out: every element's box, in the order they are painted.
#[derive(Clone, Debug, PartialEq)]
pub struct Laid {
    pub(crate) nodes: Vec<Node>,
    /// The page's size, pixels.
    pub size: Size,
    /// Pixels per point.
    pub scale: f32,
}

/// What a Taffy leaf needs to be measured.
struct Leaf {
    text: String,
    font: Font,
}

/// An element between the walk down (the tree being made) and the walk back (the boxes
/// being read).
struct Pending {
    id: NodeId,
    parent: Option<usize>,
    meta: Meta,
    overflow: Overflow,
    /// Pixels.
    scroll: (f32, f32),
    /// The room inside the box below its content, pixels.
    pad_bottom: f32,
    hidden: bool,
    content: Content,
    fill: Option<Color>,
    border: Option<Border>,
    radii: [f32; 4],
    shadow: Option<Shadow>,
}

/// Lays `root` out on a page of `viewport` pixels. A page is written in points: `scale` is
/// the pixels to a point. `measure` is asked for every line of text.
pub fn layout(mut root: Element, viewport: Size, scale: f32, measure: &mut dyn Measure) -> Laid {
    // The root is exactly the page: the percentages below it are the page's.
    root.style.width = Length::Px(viewport.w / scale);
    root.style.height = Length::Px(viewport.h / scale);
    let mut tree: TaffyTree<Leaf> = TaffyTree::new();
    let mut pending = Vec::new();
    let (_, root_id) = build(&mut tree, &mut pending, root, None, DEFAULT_TEXT, scale);

    let mut cache: HashMap<(Font, String), Metrics> = HashMap::new();
    let space = TSize {
        width: AvailableSpace::Definite(viewport.w),
        height: AvailableSpace::Definite(viewport.h),
    };
    let measured = tree.compute_layout_with_measure(
        root_id,
        space,
        |known, _available, _node, leaf, _style| {
            let natural = match leaf {
                Some(Leaf { text, font }) => *cache
                    .entry((*font, text.clone()))
                    .or_insert_with(|| measure.measure(font, text)),
                None => Metrics::default(),
            };
            TSize {
                width: known.width.unwrap_or(natural.width),
                height: known.height.unwrap_or(natural.height),
            }
        },
    );
    if measured.is_err() {
        return Laid {
            nodes: Vec::new(),
            size: viewport,
            scale,
        };
    }

    let page = Rect::new(0.0, 0.0, viewport.w, viewport.h);
    let mut nodes: Vec<Node> = Vec::with_capacity(pending.len());
    // Where each element's children are measured from, and the clip they inherit.
    let mut origins: Vec<(f32, f32, Rect, bool)> = Vec::with_capacity(pending.len());
    // How far down each box's children reach, where the box does not scroll them: the length
    // of what a scrolling box holds.
    let mut reach: Vec<f32> = Vec::with_capacity(pending.len());
    // What each box scrolls its children by, and the room it keeps below them, pixels.
    let mut inside: Vec<(f32, f32)> = Vec::with_capacity(pending.len());
    for entry in pending {
        let Ok(laid) = tree.layout(entry.id) else {
            // A node Taffy does not know: nothing of it (cannot happen — its own ids).
            nodes.push(empty(page));
            origins.push((0.0, 0.0, page, true));
            reach.push(0.0);
            inside.push((0.0, 0.0));
            continue;
        };
        let (ox, oy, outer_clip, outer_hidden) = entry
            .parent
            .and_then(|parent| origins.get(parent).copied())
            .unwrap_or((0.0, 0.0, page, false));
        let rect = Rect::new(
            ox + laid.location.x,
            oy + laid.location.y,
            laid.size.width,
            laid.size.height,
        );
        let hidden = outer_hidden || entry.hidden;
        let own_clip = if entry.overflow == Overflow::Visible {
            outer_clip
        } else {
            outer_clip.intersect(rect)
        };
        origins.push((
            rect.x - entry.scroll.0,
            rect.y - entry.scroll.1,
            own_clip,
            hidden,
        ));
        if let Some(parent) = entry.parent {
            let held = rect.bottom() + inside[parent].0;
            reach[parent] = reach[parent].max(held);
        }
        reach.push(0.0);
        inside.push((entry.scroll.1, entry.pad_bottom));
        nodes.push(Node {
            rect,
            clip: outer_clip,
            hidden,
            content: entry.content,
            fill: entry.fill,
            border: entry.border,
            radii: entry.radii,
            shadow: entry.shadow,
            parent: entry.parent,
            meta: entry.meta,
            extent: (entry.overflow == Overflow::Scroll).then_some((0.0, rect.h)),
        });
    }
    for (index, node) in nodes.iter_mut().enumerate() {
        if let Some((content, _)) = &mut node.extent {
            *content = (reach[index] - node.rect.y).max(0.0) + inside[index].1;
        }
    }
    Laid {
        nodes,
        size: viewport,
        scale,
    }
}

fn empty(page: Rect) -> Node {
    Node {
        rect: Rect::default(),
        clip: page,
        hidden: true,
        content: Content::None,
        fill: None,
        border: None,
        radii: [0.0; 4],
        shadow: None,
        parent: None,
        meta: Meta::default(),
        extent: None,
    }
}

/// Makes the Taffy node of `element` and of what is inside it; returns the element's place in
/// `pending` (the pre-order the boxes are painted in) and its node.
fn build(
    tree: &mut TaffyTree<Leaf>,
    pending: &mut Vec<Pending>,
    element: Element,
    parent: Option<usize>,
    outer: TextStyle,
    scale: f32,
) -> (usize, NodeId) {
    let Element {
        kind,
        style,
        children,
        meta,
    } = element;
    let text = style.text.over(outer);
    let font = Font {
        family: text.family.unwrap_or(Family::Terminal),
        size: text.size.unwrap_or(12.5) * scale,
        weight: text.weight.unwrap_or(400),
    };
    let color = text.color.unwrap_or(Tone::Foreground.solid());

    let index = pending.len();
    let px = |points: f32| points * scale;
    pending.push(Pending {
        id: NodeId::new(0),
        parent,
        meta,
        overflow: style.overflow,
        scroll: (px(style.scroll.0), px(style.scroll.1)),
        pad_bottom: px(style.padding[2]),
        hidden: style.display == Display::None,
        content: Content::None,
        fill: style.fill,
        border: style.border.map(|b| Border {
            width: (b.width * scale).round().max(1.0),
            color: b.color,
        }),
        radii: style.radii.map(px),
        shadow: style.shadow.map(|s| Shadow {
            dx: px(s.dx),
            dy: px(s.dy),
            blur: px(s.blur),
            color: s.color,
        }),
    });

    let taffy_style = convert(&style, scale, matches!(kind, Kind::Text(_)));
    let id = match kind {
        Kind::Text(content) => {
            let leaf = Leaf {
                text: content.clone(),
                font,
            };
            pending[index].content = Content::Text(TextLine {
                text: content,
                font,
                color,
                align: text.align.unwrap_or(Align::Start),
                truncate: style.truncate,
            });
            tree.new_leaf_with_context(taffy_style, leaf)
        }
        Kind::Spark(spark) => {
            pending[index].content = Content::Spark(spark);
            tree.new_leaf(taffy_style)
        }
        Kind::Box => {
            let ids: Vec<NodeId> = children
                .into_iter()
                .map(|child| build(tree, pending, child, Some(index), text, scale).1)
                .collect();
            tree.new_with_children(taffy_style, &ids)
        }
    };
    // Taffy cannot refuse a node it is given a valid style for; if it ever does, the
    // placeholder id stands and the element lays out as nothing.
    let id = id.unwrap_or(NodeId::new(0));
    pending[index].id = id;
    (index, id)
}

fn dimension(length: Length, scale: f32) -> Dimension {
    match length {
        Length::Auto => Dimension::auto(),
        Length::Px(points) => Dimension::length(points * scale),
        Length::Percent(percent) => Dimension::percent(percent / 100.0),
    }
}

fn convert(style: &Style, scale: f32, is_text: bool) -> TStyle {
    let px = |points: f32| points * scale;
    let edge = |points: f32| LengthPercentage::length(points * scale);
    let margin = |points: f32| LengthPercentageAuto::length(points * scale);
    let inset = |value: Option<f32>| value.map_or_else(LengthPercentageAuto::auto, margin);
    let [top, right, bottom, left] = style.padding;
    let [m_top, m_right, m_bottom, m_left] = style.margin;
    let [i_top, i_right, i_bottom, i_left] = style.inset;
    let mut min_size = TSize {
        width: dimension(style.min_width, scale),
        height: dimension(style.min_height, scale),
    };
    // A line that may be cut gives its room away down to nothing, as a style sheet's
    // `min-width: 0` on an ellipsis would.
    if is_text && style.truncate && style.min_width == Length::Auto {
        min_size.width = Dimension::length(0.0);
    }
    let overflow = match style.overflow {
        Overflow::Visible => TOverflow::Visible,
        Overflow::Hidden => TOverflow::Hidden,
        Overflow::Scroll => TOverflow::Scroll,
    };
    TStyle {
        display: match style.display {
            Display::Flex => TDisplay::Flex,
            Display::Grid => TDisplay::Grid,
            Display::None => TDisplay::None,
        },
        flex_direction: match style.direction {
            Direction::Row => FlexDirection::Row,
            Direction::Column => FlexDirection::Column,
        },
        flex_wrap: if style.wrap {
            FlexWrap::Wrap
        } else {
            FlexWrap::NoWrap
        },
        position: if style.absolute {
            Position::Absolute
        } else {
            Position::Relative
        },
        inset: TRect {
            top: inset(i_top),
            right: inset(i_right),
            bottom: inset(i_bottom),
            left: inset(i_left),
        },
        size: TSize {
            width: dimension(style.width, scale),
            height: dimension(style.height, scale),
        },
        min_size,
        max_size: TSize {
            width: dimension(style.max_width, scale),
            height: dimension(style.max_height, scale),
        },
        margin: TRect {
            top: margin(m_top),
            right: margin(m_right),
            bottom: margin(m_bottom),
            left: margin(m_left),
        },
        padding: TRect {
            top: edge(top),
            right: edge(right),
            bottom: edge(bottom),
            left: edge(left),
        },
        gap: TSize {
            width: edge(style.gap.0),
            height: edge(style.gap.1),
        },
        flex_grow: style.grow,
        flex_shrink: style.shrink.unwrap_or(1.0),
        align_items: style.items.map(|items| match items {
            Items::Start => AlignItems::FlexStart,
            Items::Center => AlignItems::Center,
            Items::End => AlignItems::FlexEnd,
            Items::Stretch => AlignItems::Stretch,
        }),
        justify_content: style.justify.map(|justify| match justify {
            Justify::Start => JustifyContent::FlexStart,
            Justify::Center => JustifyContent::Center,
            Justify::End => JustifyContent::FlexEnd,
            Justify::Between => JustifyContent::SpaceBetween,
        }),
        grid_template_columns: style.columns.iter().map(|t| track(*t, px)).collect(),
        grid_template_rows: style.rows.iter().map(|t| track(*t, px)).collect(),
        grid_column: Line {
            start: GridPlacement::Auto,
            end: if style.column_span > 1 {
                GridPlacement::Span(style.column_span)
            } else {
                GridPlacement::Auto
            },
        },
        overflow: TPoint {
            x: overflow,
            y: overflow,
        },
        scrollbar_width: 0.0,
        ..TStyle::default()
    }
}

fn track(track: Track, px: impl Fn(f32) -> f32) -> GridTemplateComponent<String> {
    let (min, max) = match track {
        Track::Px(points) => (
            MinTrackSizingFunction::length(px(points)),
            MaxTrackSizingFunction::length(px(points)),
        ),
        Track::Fr(share) => (
            MinTrackSizingFunction::auto(),
            MaxTrackSizingFunction::fr(share),
        ),
        Track::Range { min, fr } => (
            MinTrackSizingFunction::length(px(min)),
            MaxTrackSizingFunction::fr(fr),
        ),
        Track::Auto => (
            MinTrackSizingFunction::auto(),
            MaxTrackSizingFunction::auto(),
        ),
    };
    GridTemplateComponent::Single(taffy::MinMax { min, max })
}

impl Laid {
    /// The number of elements on the page.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The boxes of the elements in paint order, pixels — what a test or a host that has to
    /// know where things stand reads.
    pub fn rects(&self) -> impl Iterator<Item = Rect> + '_ {
        self.nodes.iter().map(|node| node.rect)
    }

    // --- Asking the page where things are ------------------------------------------------
    //
    // Everything below speaks in points, the page's own unit: a pointer's place goes in and
    // a box comes out the way the page was written.

    /// Pixels to a point.
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// The part of `node` that shows, pixels: its box where the boxes around it let it be seen.
    fn shown(node: &Node) -> Rect {
        node.rect.intersect(node.clip)
    }

    fn named(&self, id: Id) -> Option<&Node> {
        self.nodes.iter().find(|node| node.meta.id == Some(id))
    }

    /// The box of the element named `id`, points (where it is on the page now, whatever of it
    /// is cut off by a box around it).
    pub fn bounds(&self, id: Id) -> Option<Rect> {
        let node = self.named(id)?;
        let s = self.scale;
        Some(Rect::new(
            node.rect.x / s,
            node.rect.y / s,
            node.rect.w / s,
            node.rect.h / s,
        ))
    }

    /// The named elements whose visible part holds `at`, the innermost first. An element that is
    /// `hidden`, or whose place is cut off by a box around it, is not there.
    pub fn ids_at(&self, at: Point) -> Vec<Id> {
        let (x, y) = (at.x * self.scale, at.y * self.scale);
        self.nodes
            .iter()
            .rev()
            .filter(|node| !node.hidden && Self::shown(node).contains(x, y))
            .filter_map(|node| node.meta.id)
            .collect()
    }

    /// The element under `at` that answers the pointer ([`crate::Element::press`]): the topmost.
    pub fn hit(&self, at: Point) -> Option<Id> {
        let (x, y) = (at.x * self.scale, at.y * self.scale);
        self.nodes
            .iter()
            .rev()
            .find(|node| node.meta.pressable && !node.hidden && Self::shown(node).contains(x, y))
            .and_then(|node| node.meta.id)
    }

    /// How tall the content of the scrolling box named `id` is and how tall the box is.
    pub fn scroll_extent(&self, id: Id) -> Option<Extent> {
        let (content, view) = self.named(id)?.extent?;
        Some(Extent {
            content: content / self.scale,
            view: view / self.scale,
        })
    }

    /// The elements the keyboard can be on, in the order they are written.
    pub fn focus_order(&self) -> Vec<Id> {
        self.nodes
            .iter()
            .filter(|node| node.meta.focusable && !node.hidden)
            .filter_map(|node| node.meta.id)
            .collect()
    }

    /// The box of the element that stands for a text field of the host's
    /// ([`crate::Element::text_field`]), points: where the host puts it.
    pub fn text_field(&self) -> Option<Rect> {
        let node = self
            .nodes
            .iter()
            .find(|node| node.meta.text_field && !node.hidden)?;
        let s = self.scale;
        Some(Rect::new(
            node.rect.x / s,
            node.rect.y / s,
            node.rect.w / s,
            node.rect.h / s,
        ))
    }

    /// `id` and the named elements that hold it, innermost first: where a key goes, from the
    /// element the keyboard is on out to the boxes around it.
    pub fn path_of(&self, id: Id) -> Vec<Id> {
        let mut path = Vec::new();
        let mut at = self.nodes.iter().position(|node| node.meta.id == Some(id));
        while let Some(index) = at {
            if let Some(named) = self.nodes[index].meta.id {
                path.push(named);
            }
            at = self.nodes[index].parent;
        }
        path
    }
}

/// What a page asks of itself laid out: where things stand, what is under the pointer, how far
/// a box can scroll. [`Laid`] answers it, and so does anything that can reach one — a page kept
/// elsewhere than the host that laid it out (a module in a sandbox, asking across the border)
/// is written against this and not against [`Laid`].
pub trait Queries {
    /// The box of the element named `id`, points.
    fn bounds(&self, id: Id) -> Option<Rect>;
    /// The named elements whose visible part holds `at`, innermost first.
    fn ids_at(&self, at: Point) -> Vec<Id>;
    /// The element under `at` that answers the pointer: the topmost.
    fn hit(&self, at: Point) -> Option<Id>;
    /// How tall the content of the scrolling box named `id` is and how tall the box is.
    fn scroll_extent(&self, id: Id) -> Option<Extent>;
    /// The elements the keyboard can be on, in the order they are written.
    fn focus_order(&self) -> Vec<Id>;
    /// `id` and the named elements that hold it, innermost first.
    fn path_of(&self, id: Id) -> Vec<Id>;
    /// The box of the text field the host puts over the page, points.
    fn text_field(&self) -> Option<Rect>;
}

impl Queries for Laid {
    fn bounds(&self, id: Id) -> Option<Rect> {
        Laid::bounds(self, id)
    }
    fn ids_at(&self, at: Point) -> Vec<Id> {
        Laid::ids_at(self, at)
    }
    fn hit(&self, at: Point) -> Option<Id> {
        Laid::hit(self, at)
    }
    fn scroll_extent(&self, id: Id) -> Option<Extent> {
        Laid::scroll_extent(self, id)
    }
    fn focus_order(&self) -> Vec<Id> {
        Laid::focus_order(self)
    }
    fn path_of(&self, id: Id) -> Vec<Id> {
        Laid::path_of(self, id)
    }
    fn text_field(&self) -> Option<Rect> {
        Laid::text_field(self)
    }
}

/// The length of what a scrolling box holds against the box's own, points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Extent {
    pub content: f32,
    pub view: f32,
}

impl Extent {
    /// The furthest the content can be scrolled: 0 when it fits.
    pub fn max_offset(self) -> f32 {
        (self.content - self.view).max(0.0)
    }
}
