//! The page's elements and the chain that writes them: `div().row().gap(8.0).child(...)`.
//!
//! A page is a tree of three kinds of element — a **box** ([`div`]), a line of **text**
//! ([`text`]) and a small **sparkline** ([`spark`]). Every value is in points. A box lays
//! its children out as flexbox or as a grid (the way a style sheet would), paints a fill, a
//! border and a shadow, and sets the text of the boxes under it; a box is a column unless
//! told otherwise.

use crate::style::{
    Border, Direction, Display, Items, Justify, Length, Overflow, Shadow, Style, Track,
};
use crate::{Align, Color, Family, Id};

/// What an element is.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Box,
    Text(String),
    Spark(Spark),
}

/// The data of a sparkline: values over time, newest last, drawn as a line with the area
/// under it softly filled.
#[derive(Clone, Debug, PartialEq)]
pub struct Spark {
    pub values: Vec<f32>,
    pub color: Color,
    /// The value the top of the box stands for; the largest value (plus a little head room)
    /// when `None`.
    pub peak: Option<f32>,
}

/// What an element is to the pointer and the keyboard, apart from how it looks.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Meta {
    pub id: Option<Id>,
    /// The pointer can hover it and press it (it answers a click).
    pub pressable: bool,
    /// The keyboard can be on it.
    pub focusable: bool,
    /// The host puts a text field of its own over this box ([`Element::text_field`]).
    pub text_field: bool,
}

/// One element of a page, and the elements inside it.
#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    pub(crate) kind: Kind,
    pub(crate) style: Style,
    pub(crate) children: Vec<Element>,
    pub(crate) meta: Meta,
}

/// A box: a column of its children until told otherwise.
pub fn div() -> Element {
    Element::of(Kind::Box)
}

/// One line of text, in the font, size and colour of the boxes it is in.
pub fn text(content: impl Into<String>) -> Element {
    Element::of(Kind::Text(content.into()))
}

/// A sparkline of `values` (newest last), `color` the line.
pub fn spark(values: &[f32], color: impl Into<Color>) -> Element {
    Element::of(Kind::Spark(Spark {
        values: values.to_vec(),
        color: color.into(),
        peak: None,
    }))
}

/// A box that takes the room left over: what pushes the elements after it to the end.
pub fn spacer() -> Element {
    div().grow(1.0)
}

impl Element {
    fn of(kind: Kind) -> Self {
        Self {
            kind,
            style: Style::default(),
            children: Vec::new(),
            meta: Meta::default(),
        }
    }

    fn set(mut self, change: impl FnOnce(&mut Style)) -> Self {
        change(&mut self.style);
        self
    }

    // --- Children --------------------------------------------------------------------

    /// Adds `child` after the ones already here.
    #[must_use]
    pub fn child(mut self, child: Element) -> Self {
        self.children.push(child);
        self
    }

    /// Adds every element of `children`.
    #[must_use]
    pub fn children(mut self, children: impl IntoIterator<Item = Element>) -> Self {
        self.children.extend(children);
        self
    }

    /// Adds `child` when there is one.
    #[must_use]
    pub fn child_if(self, child: Option<Element>) -> Self {
        match child {
            Some(child) => self.child(child),
            None => self,
        }
    }

    // --- Names and answers ---------------------------------------------------------------

    /// Names the element, so what is kept about it between builds (a scroll position, a
    /// fade) and what the host asks of it (its box) can find it.
    #[must_use]
    pub fn id(mut self, id: Id) -> Self {
        self.meta.id = Some(id);
        self
    }

    /// Names the element and makes it something the pointer answers: it can be hovered,
    /// pressed and clicked (`Ui::hover`, `Ui::press`), and the pointer takes the hand's
    /// shape over it.
    #[must_use]
    pub fn press(mut self, id: Id) -> Self {
        self.meta.id = Some(id);
        self.meta.pressable = true;
        self
    }

    /// The keyboard can be on this element (it is named too: see [`Element::id`]); focus
    /// moves between such elements in the order they are written.
    #[must_use]
    pub fn focusable(mut self) -> Self {
        self.meta.focusable = true;
        self
    }

    /// The box is the room of a text field the host puts over the page — the page does not
    /// draw text that is typed into, a field of the host's does (the host asks where it
    /// stands: [`crate::Laid::text_field`]). Named too: see [`Element::id`].
    #[must_use]
    pub fn text_field(mut self) -> Self {
        self.meta.text_field = true;
        self
    }

    // --- Layout: the box ---------------------------------------------------------------

    /// Lays the children out in a row, left to right.
    #[must_use]
    pub fn row(self) -> Self {
        self.set(|s| {
            s.display = Display::Flex;
            s.direction = Direction::Row;
        })
    }

    /// A row (or column) that is too long for its box goes on in a new line.
    #[must_use]
    pub fn wrap(self) -> Self {
        self.set(|s| s.wrap = true)
    }

    /// Lays the children out in a column, top to bottom (the default).
    #[must_use]
    pub fn col(self) -> Self {
        self.set(|s| {
            s.display = Display::Flex;
            s.direction = Direction::Column;
        })
    }

    /// Lays the children out in a grid of `columns`; the children fill it row by row, new
    /// rows as many as they need.
    #[must_use]
    pub fn grid(self, columns: impl IntoIterator<Item = Track>) -> Self {
        let columns: Vec<Track> = columns.into_iter().collect();
        self.set(|s| {
            s.display = Display::Grid;
            s.columns = columns;
        })
    }

    /// The grid's rows, where they are not simply as tall as their content.
    #[must_use]
    pub fn grid_rows(self, rows: impl IntoIterator<Item = Track>) -> Self {
        let rows: Vec<Track> = rows.into_iter().collect();
        self.set(|s| s.rows = rows)
    }

    /// Takes `columns` grid columns.
    #[must_use]
    pub fn span(self, columns: u16) -> Self {
        self.set(|s| s.column_span = columns)
    }

    /// Takes no room and draws nothing.
    #[must_use]
    pub fn hidden(self) -> Self {
        self.set(|s| s.display = Display::None)
    }

    /// The children are aligned to the start of the cross axis (the top of a row).
    #[must_use]
    pub fn items_start(self) -> Self {
        self.set(|s| s.items = Some(Items::Start))
    }

    /// The children are centred on the cross axis (vertically in a row).
    #[must_use]
    pub fn items_center(self) -> Self {
        self.set(|s| s.items = Some(Items::Center))
    }

    /// The children are aligned to the end of the cross axis.
    #[must_use]
    pub fn items_end(self) -> Self {
        self.set(|s| s.items = Some(Items::End))
    }

    /// The children fill the cross axis (the default).
    #[must_use]
    pub fn items_stretch(self) -> Self {
        self.set(|s| s.items = Some(Items::Stretch))
    }

    /// The children start at the start of the main axis (the default).
    #[must_use]
    pub fn justify_start(self) -> Self {
        self.set(|s| s.justify = Some(Justify::Start))
    }

    /// The children are centred on the main axis.
    #[must_use]
    pub fn justify_center(self) -> Self {
        self.set(|s| s.justify = Some(Justify::Center))
    }

    /// The children are pushed to the end of the main axis.
    #[must_use]
    pub fn justify_end(self) -> Self {
        self.set(|s| s.justify = Some(Justify::End))
    }

    /// The room left over goes between the children.
    #[must_use]
    pub fn justify_between(self) -> Self {
        self.set(|s| s.justify = Some(Justify::Between))
    }

    /// The same gap between children on both axes, points.
    #[must_use]
    pub fn gap(self, points: f32) -> Self {
        self.set(|s| s.gap = (points, points))
    }

    /// The gap between columns (the children of a row), points.
    #[must_use]
    pub fn gap_x(self, points: f32) -> Self {
        self.set(|s| s.gap.0 = points)
    }

    /// The gap between rows (the children of a column), points.
    #[must_use]
    pub fn gap_y(self, points: f32) -> Self {
        self.set(|s| s.gap.1 = points)
    }

    // --- Layout: the element in its parent ---------------------------------------------

    /// How much of the room left over this element takes, relative to its siblings.
    #[must_use]
    pub fn grow(self, share: f32) -> Self {
        self.set(|s| s.grow = share)
    }

    /// Never gives way when the line is too long.
    #[must_use]
    pub fn no_shrink(self) -> Self {
        self.set(|s| s.shrink = Some(0.0))
    }

    /// Takes the width of the parent.
    #[must_use]
    pub fn w_full(self) -> Self {
        self.set(|s| s.width = Length::Percent(100.0))
    }

    /// Takes the height of the parent.
    #[must_use]
    pub fn h_full(self) -> Self {
        self.set(|s| s.height = Length::Percent(100.0))
    }

    /// Takes the width and the height of the parent.
    #[must_use]
    pub fn size_full(self) -> Self {
        self.w_full().h_full()
    }

    /// The width, points.
    #[must_use]
    pub fn w(self, points: f32) -> Self {
        self.set(|s| s.width = Length::Px(points))
    }

    /// A share of the parent's width, `0.0..=100.0`.
    #[must_use]
    pub fn w_pct(self, percent: f32) -> Self {
        self.set(|s| s.width = Length::Percent(percent))
    }

    /// The height, points.
    #[must_use]
    pub fn h(self, points: f32) -> Self {
        self.set(|s| s.height = Length::Px(points))
    }

    /// The width and the height, points.
    #[must_use]
    pub fn size(self, width: f32, height: f32) -> Self {
        self.w(width).h(height)
    }

    /// The least width, points. A line of text that may be cut (`truncate`) has none by
    /// default.
    #[must_use]
    pub fn min_w(self, points: f32) -> Self {
        self.set(|s| s.min_width = Length::Px(points))
    }

    /// The least height, points.
    #[must_use]
    pub fn min_h(self, points: f32) -> Self {
        self.set(|s| s.min_height = Length::Px(points))
    }

    /// The greatest width, points.
    #[must_use]
    pub fn max_w(self, points: f32) -> Self {
        self.set(|s| s.max_width = Length::Px(points))
    }

    /// Room inside the box on all four sides, points.
    #[must_use]
    pub fn p(self, points: f32) -> Self {
        self.set(|s| s.padding = [points; 4])
    }

    /// Room inside the box left and right, points.
    #[must_use]
    pub fn px(self, points: f32) -> Self {
        self.set(|s| {
            s.padding[1] = points;
            s.padding[3] = points;
        })
    }

    /// Room inside the box above and below, points.
    #[must_use]
    pub fn py(self, points: f32) -> Self {
        self.set(|s| {
            s.padding[0] = points;
            s.padding[2] = points;
        })
    }

    #[must_use]
    pub fn pt(self, points: f32) -> Self {
        self.set(|s| s.padding[0] = points)
    }

    #[must_use]
    pub fn pr(self, points: f32) -> Self {
        self.set(|s| s.padding[1] = points)
    }

    #[must_use]
    pub fn pb(self, points: f32) -> Self {
        self.set(|s| s.padding[2] = points)
    }

    #[must_use]
    pub fn pl(self, points: f32) -> Self {
        self.set(|s| s.padding[3] = points)
    }

    /// Room outside the box above it, points.
    #[must_use]
    pub fn mt(self, points: f32) -> Self {
        self.set(|s| s.margin[0] = points)
    }

    /// Room outside the box to its right, points.
    #[must_use]
    pub fn mr(self, points: f32) -> Self {
        self.set(|s| s.margin[1] = points)
    }

    /// Room outside the box below it, points.
    #[must_use]
    pub fn mb(self, points: f32) -> Self {
        self.set(|s| s.margin[2] = points)
    }

    /// Room outside the box to its left, points.
    #[must_use]
    pub fn ml(self, points: f32) -> Self {
        self.set(|s| s.margin[3] = points)
    }

    /// Takes the box out of the flow: it is placed against the parent's box by `inset`
    /// (and the size it is given), over its siblings.
    #[must_use]
    pub fn absolute(self) -> Self {
        self.set(|s| s.absolute = true)
    }

    /// For an absolute box: its edges' distances from the parent's, points (top, right,
    /// bottom, left; `None` leaves an edge free).
    #[must_use]
    pub fn inset(
        self,
        top: Option<f32>,
        right: Option<f32>,
        bottom: Option<f32>,
        left: Option<f32>,
    ) -> Self {
        self.set(|s| s.inset = [top, right, bottom, left])
    }

    /// Nothing is drawn outside the box.
    #[must_use]
    pub fn clip(self) -> Self {
        self.set(|s| s.overflow = Overflow::Hidden)
    }

    /// A box whose content is bigger than itself and moves behind it: nothing is drawn
    /// outside the box, and the content is moved up by `offset` points.
    #[must_use]
    pub fn scroll_y(self, offset: f32) -> Self {
        self.set(|s| {
            s.overflow = Overflow::Scroll;
            s.scroll.1 = offset;
        })
    }

    // --- Looks -------------------------------------------------------------------------

    /// The fill.
    #[must_use]
    pub fn bg(self, color: impl Into<Color>) -> Self {
        let color = color.into();
        self.set(|s| s.fill = Some(color))
    }

    /// A border of `width` points inside the box, over the fill.
    #[must_use]
    pub fn border(self, width: f32, color: impl Into<Color>) -> Self {
        let color = color.into();
        self.set(|s| s.border = Some(Border { width, color }))
    }

    /// The same radius on every corner, points.
    #[must_use]
    pub fn rounded(self, points: f32) -> Self {
        self.set(|s| s.radii = [points; 4])
    }

    /// A radius per corner: top-left, top-right, bottom-right, bottom-left; points.
    #[must_use]
    pub fn rounded_corners(self, radii: [f32; 4]) -> Self {
        self.set(|s| s.radii = radii)
    }

    /// A soft shadow `(dx, dy)` away from the box, blurred over `blur`; points.
    #[must_use]
    pub fn shadow(self, dx: f32, dy: f32, blur: f32, color: impl Into<Color>) -> Self {
        let color = color.into();
        self.set(|s| {
            s.shadow = Some(Shadow {
                dx,
                dy,
                blur,
                color,
            })
        })
    }

    // --- Text (set here, inherited by the elements inside) -------------------------------

    /// The size of the text, points.
    #[must_use]
    pub fn text_size(self, points: f32) -> Self {
        self.set(|s| s.text.size = Some(points))
    }

    /// The weight of the text on the CSS scale (400 regular, 700 bold).
    #[must_use]
    pub fn weight(self, weight: u16) -> Self {
        self.set(|s| s.text.weight = Some(weight))
    }

    /// The colour of the text.
    #[must_use]
    pub fn color(self, color: impl Into<Color>) -> Self {
        let color = color.into();
        self.set(|s| s.text.color = Some(color))
    }

    /// The face of the text.
    #[must_use]
    pub fn family(self, family: Family) -> Self {
        self.set(|s| s.text.family = Some(family))
    }

    /// Where a line stands in the box it is given: the start (default), the middle or the
    /// end.
    #[must_use]
    pub fn align(self, align: Align) -> Self {
        self.set(|s| s.text.align = Some(align))
    }

    /// A line of text that does not fit ends in "…" and is the first to give its room away.
    #[must_use]
    pub fn truncate(self) -> Self {
        self.set(|s| s.truncate = true)
    }

    /// The largest value a sparkline's top stands for (the greatest value of the line, with
    /// a little head room, when not set).
    #[must_use]
    pub fn peak(mut self, value: f32) -> Self {
        if let Kind::Spark(spark) = &mut self.kind {
            spark.peak = Some(value);
        }
        self
    }
}
