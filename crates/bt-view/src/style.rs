//! The style of an element: what it takes, where its children stand, how it looks and how
//! the text inside it is set. Written in the builder methods of [`crate::Element`]; the
//! values are **points** (a page does not know the screen's density), the engine turns them
//! into pixels when it lays the page out.

use crate::{Align, Color, Family};

/// A size along one axis.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Length {
    /// As much as the content and the siblings leave.
    #[default]
    Auto,
    /// Points.
    Px(f32),
    /// A share of the parent's size, `0.0..=100.0`.
    Percent(f32),
}

/// A column or a row of a grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Track {
    /// Points.
    Px(f32),
    /// A share of what the fixed tracks leave (CSS's `fr`).
    Fr(f32),
    /// At least `min` points, then a share of what is left (CSS's `minmax(min, fr)`).
    Range { min: f32, fr: f32 },
    /// As much as the content wants.
    Auto,
}

impl Track {
    pub const fn px(points: f32) -> Self {
        Self::Px(points)
    }

    pub const fn fr(share: f32) -> Self {
        Self::Fr(share)
    }

    /// `minmax(min points, share fr)`.
    pub const fn range(min: f32, share: f32) -> Self {
        Self::Range { min, fr: share }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Display {
    #[default]
    Flex,
    Grid,
    None,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    Row,
    #[default]
    Column,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Items {
    Start,
    Center,
    End,
    Stretch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Justify {
    Start,
    Center,
    End,
    Between,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Overflow {
    /// Children may spill out of the box.
    #[default]
    Visible,
    /// Nothing is drawn outside the box, and the box does not grow to its children's size.
    Hidden,
    /// As `Hidden`, and the children move with the box's scroll offset.
    Scroll,
}

/// A border drawn inside the box, over its fill.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Border {
    pub width: f32,
    pub color: Color,
}

/// A soft shadow: the box's silhouette moved by `(dx, dy)` and blurred over `blur`, never
/// drawn under the box itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub dx: f32,
    pub dy: f32,
    pub blur: f32,
    pub color: Color,
}

/// The text properties an element sets for itself and the elements under it, as a style
/// sheet's `font` and `color` are inherited. A property an element does not set is the
/// nearest ancestor's.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextStyle {
    pub family: Option<Family>,
    /// Points.
    pub size: Option<f32>,
    pub weight: Option<u16>,
    pub color: Option<Color>,
    pub align: Option<Align>,
}

impl TextStyle {
    /// `self` over `outer`: what this element sets, then what it inherits.
    pub(crate) fn over(self, outer: Self) -> Self {
        Self {
            family: self.family.or(outer.family),
            size: self.size.or(outer.size),
            weight: self.weight.or(outer.weight),
            color: self.color.or(outer.color),
            align: self.align.or(outer.align),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Style {
    pub display: Display,
    pub direction: Direction,
    /// A row that is too long goes on in a new line below.
    pub wrap: bool,
    /// Taken out of the flow and placed by `inset` against the parent's box.
    pub absolute: bool,
    /// Top, right, bottom, left; points.
    pub inset: [Option<f32>; 4],
    pub width: Length,
    pub height: Length,
    pub min_width: Length,
    pub min_height: Length,
    pub max_width: Length,
    pub max_height: Length,
    /// Top, right, bottom, left; points.
    pub padding: [f32; 4],
    pub margin: [f32; 4],
    /// Between columns, between rows; points.
    pub gap: (f32, f32),
    pub grow: f32,
    /// `None` is the usual 1: the item gives way when the line is too long.
    pub shrink: Option<f32>,
    pub items: Option<Items>,
    pub justify: Option<Justify>,
    pub columns: Vec<Track>,
    pub rows: Vec<Track>,
    /// How many grid columns the element takes (`0` or `1`: one).
    pub column_span: u16,
    pub overflow: Overflow,
    /// How far a scrolling box's content is moved up and left, points.
    pub scroll: (f32, f32),
    pub fill: Option<Color>,
    pub border: Option<Border>,
    /// Top-left, top-right, bottom-right, bottom-left; points.
    pub radii: [f32; 4],
    pub shadow: Option<Shadow>,
    pub text: TextStyle,
    /// A line that does not fit ends in "…" and gives its room away first.
    pub truncate: bool,
}
