//! Where an image sits on a screen: a placement, the cells it covers, and
//! how placing it moves the cursor.
//!
//! Everything here is arithmetic on numbers and none of it reads the grid:
//! the command's side (which works out how far the cursor moves), the
//! parser's side (which anchors the placement) and the frame (which hands the
//! placement to the drawing) all ask the same functions, so the three agree
//! on how many cells an image takes. The rules are kitty's: a placement
//! covers `c` × `r` cells, a missing one of the two follows the image's aspect
//! ratio, and with neither the image keeps its own pixel size.

use std::sync::Arc;

use super::command::Command;

/// The largest number of cells a placement may cover in either direction.
/// Past it the numbers saturate, so that arithmetic on a hostile `c=` or
/// `r=` stays small.
const MAX_CELLS: u32 = u16::MAX as u32;

/// What the parser's side knows about the screen a command is applied to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Screen {
    /// Whether it is the alternate screen.
    pub(crate) alt: bool,
    /// The cursor: its grid line (negative is the history) and its column.
    pub(crate) cursor: (i32, u16),
    /// The screen's size in cells.
    pub(crate) lines: u16,
    pub(crate) columns: u16,
    /// A cell's size in pixels.
    pub(crate) cell: (u16, u16),
}

impl Screen {
    /// A blank screen, for the tests that have no grid.
    #[cfg(test)]
    pub(crate) fn blank(alt: bool) -> Self {
        Self {
            alt,
            cursor: (0, 0),
            lines: 24,
            columns: 80,
            cell: (10, 20),
        }
    }
}

/// A rectangle of the image's pixels, `[x, y, width, height]`.
pub(crate) type Source = [u32; 4];

/// What a placement asks for, kept in the units the program wrote.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Spec {
    /// The image's size in pixels.
    pub(crate) size: (u32, u32),
    /// The part of the image shown (`x`, `y`, `w`, `h`), already cut to the
    /// image: a `w` or `h` of 0 means to the edge.
    pub(crate) source: Source,
    /// `c=` and `r=` as written; 0 is "work it out".
    pub(crate) columns: u32,
    pub(crate) rows: u32,
    /// `X=` and `Y=` as written; cut to the cell when it is used.
    pub(crate) offset: (u32, u32),
    /// `z=`.
    pub(crate) z: i32,
}

/// `value` over `unit` rounded up, as a cell count.
fn ceil_div(value: u64, unit: u64) -> u32 {
    u32::try_from(value.div_ceil(unit.max(1)))
        .unwrap_or(u32::MAX)
        .min(MAX_CELLS)
}

/// `ceil(extent * ratio / unit)` for the aspect-preserving side of a
/// placement that fixed only the other one.
fn follow_ratio(extent: u64, num: u32, den: u32, unit: u64) -> u32 {
    if den == 0 {
        return 0;
    }
    let scaled = extent as f64 * f64::from(num) / f64::from(den);
    let cells = (scaled / unit.max(1) as f64).ceil();
    if cells >= f64::from(MAX_CELLS) {
        MAX_CELLS
    } else {
        cells as u32
    }
}

impl Spec {
    /// What `command` asks of an image `size` pixels across.
    pub(crate) fn new(command: &Command, size: (u32, u32)) -> Self {
        let (width, height) = size;
        let x = command.x.min(width);
        let y = command.y.min(height);
        let w = if command.w == 0 { width } else { command.w }.min(width - x);
        let h = if command.h == 0 { height } else { command.h }.min(height - y);
        Self {
            size,
            source: [x, y, w, h],
            columns: command.columns,
            rows: command.rows,
            offset: (command.cell_x, command.cell_y),
            z: command.z,
        }
    }

    /// The starting offset inside the first cell, cut to the cell.
    pub(crate) fn offset_in(&self, cell: (u16, u16)) -> (u32, u32) {
        let (cw, ch) = cell_size(cell);
        (self.offset.0.min(cw - 1), self.offset.1.min(ch - 1))
    }

    /// How many columns and rows the placement covers at this cell size —
    /// kitty's `update_dest_rect`. Never 0 by itself except for an empty
    /// source rectangle.
    pub(crate) fn span(&self, cell: (u16, u16)) -> (u32, u32) {
        let (cw, ch) = cell_size(cell);
        let (ox, oy) = self.offset_in(cell);
        let (sw, sh) = (self.source[2], self.source[3]);
        match (self.columns, self.rows) {
            (0, 0) => (
                ceil_div(u64::from(sw) + u64::from(ox), u64::from(cw)),
                ceil_div(u64::from(sh) + u64::from(oy), u64::from(ch)),
            ),
            (0, rows) => {
                let rows = rows.min(MAX_CELLS);
                let tall = u64::from(ch) * u64::from(rows) + u64::from(oy);
                (follow_ratio(tall, sw, sh, u64::from(cw)), rows)
            }
            (columns, 0) => {
                let columns = columns.min(MAX_CELLS);
                let wide = u64::from(cw) * u64::from(columns) + u64::from(ox);
                (columns, follow_ratio(wide, sh, sw, u64::from(ch)))
            }
            (columns, rows) => (columns.min(MAX_CELLS), rows.min(MAX_CELLS)),
        }
    }

    /// The size the image is drawn at, in pixels: the cells asked for, the
    /// missing side following the aspect ratio, and the image's own size
    /// when no cells were asked for.
    pub(crate) fn dest_px(&self, cell: (u16, u16)) -> (u32, u32) {
        let (cw, ch) = cell_size(cell);
        let (sw, sh) = (self.source[2], self.source[3]);
        let scaled = |extent: u32, num: u32, den: u32| -> u32 {
            if den == 0 {
                return 0;
            }
            let value = f64::from(extent) * f64::from(num) / f64::from(den);
            value.round().min(f64::from(u32::MAX)) as u32
        };
        let capped = |cells: u32, unit: u32| cells.min(MAX_CELLS).saturating_mul(unit);
        match (self.columns, self.rows) {
            (0, 0) => (sw, sh),
            (0, rows) => {
                let tall = capped(rows, ch);
                (scaled(tall, sw, sh), tall)
            }
            (columns, 0) => {
                let wide = capped(columns, cw);
                (wide, scaled(wide, sh, sw))
            }
            (columns, rows) => (capped(columns, cw), capped(rows, ch)),
        }
    }
}

fn cell_size(cell: (u16, u16)) -> (u32, u32) {
    (u32::from(cell.0).max(1), u32::from(cell.1).max(1))
}

/// How placing an image moves the cursor (`C=0`, the default): kitty puts it
/// at the cell after the image on the image's last row, scrolling the screen
/// if that row is below the bottom.
///
/// The parser's wrapper makes the movement where the command ends, in two
/// halves around the anchoring on purpose: at the placement the cursor is in
/// the image's *first* column, on its *last* row, so the placement is anchored
/// from the grid itself, whatever the movement scrolled, and the clamp of the
/// columns' movement at the right edge cannot hide where the image began. A
/// stream that has no store to place into makes the same movement from the
/// same numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Motion {
    /// How many rows below the placement's top the cursor is when it is anchored.
    pub(crate) rows: u32,
    /// How many columns the cursor moves right after the placement.
    pub(crate) cols: u32,
    /// The image is as wide as the screen: the cursor goes to the start of
    /// the next line instead (kitty wraps a cursor that leaves the screen).
    pub(crate) wrap: bool,
}

impl Motion {
    /// The cursor stays: `C=1`, or the size is not known.
    pub(crate) const NONE: Self = Self {
        rows: 0,
        cols: 0,
        wrap: false,
    };

    /// The movement of placing `spec` on a screen `columns` wide.
    pub(crate) fn of(spec: &Spec, cell: (u16, u16), columns: u16) -> Self {
        let (cols, rows) = spec.span(cell);
        let wrap = cols >= u32::from(columns).max(1);
        Self {
            rows: rows.saturating_sub(1),
            cols: if wrap { 0 } else { cols },
            wrap,
        }
    }

    /// The rows the placement's top is above the cursor where it is anchored.
    pub(crate) fn above(&self) -> i32 {
        i32::try_from(self.rows).unwrap_or(i32::MAX)
    }
}

/// One image shown on a screen.
#[derive(Clone, Debug)]
pub(crate) struct Placement {
    /// Counts the placements made on the session, so that two on the same
    /// layer are drawn in the order they were made.
    pub(crate) seq: u64,
    /// The generation of the image shown.
    pub(crate) image: u64,
    /// The program's id for this placement; 0 for none.
    pub(crate) id: u32,
    /// The grid line of the placement's top row; negative is the history.
    pub(crate) line: i32,
    /// The column of its first cell.
    pub(crate) col: u16,
    pub(crate) spec: Spec,
}

impl Placement {
    /// The last line the placement covers.
    pub(crate) fn bottom(&self, cell: (u16, u16)) -> i32 {
        let rows = i32::try_from(self.spec.span(cell).1).unwrap_or(i32::MAX);
        self.line.saturating_add(rows.saturating_sub(1))
    }

    /// Whether it covers the line `line`.
    pub(crate) fn holds_line(&self, line: i32, cell: (u16, u16)) -> bool {
        line >= self.line && line <= self.bottom(cell)
    }

    /// Whether it covers the column `col`.
    pub(crate) fn holds_column(&self, col: i64, cell: (u16, u16)) -> bool {
        let (cols, _) = self.spec.span(cell);
        col >= i64::from(self.col) && col < i64::from(self.col) + i64::from(cols)
    }

    /// Whether it covers the cell at (`line`, `col`).
    pub(crate) fn covers(&self, line: i32, col: i64, cell: (u16, u16)) -> bool {
        self.holds_line(line, cell) && self.holds_column(col, cell)
    }
}

/// One placement as the drawing sees it: pixels and rectangles, nothing of
/// the terminal.
#[derive(Clone, Debug)]
pub struct Picture {
    /// The first cell's column.
    pub col: u16,
    /// The first cell's row in the surface's own rows: the grid's, or the
    /// fill band's. Negative when the picture starts above the surface and
    /// past its last row when it runs below it; each surface clips.
    pub row: i32,
    /// Where in the first cell the picture starts, pixels.
    pub x_offset: u32,
    pub y_offset: u32,
    /// The size it is drawn at, pixels.
    pub width: u32,
    pub height: u32,
    /// The part of the image shown, `[x, y, width, height]` in the image's
    /// pixels.
    pub source: [u32; 4],
    /// The layer: below `-1 073 741 824` under the cells' backgrounds, other
    /// negatives between the backgrounds and the text, 0 and above over the
    /// text.
    pub z: i32,
    /// Names the stored image: it changes when an id is sent again, so a
    /// texture cached under it never shows stale pixels.
    pub image: u64,
    /// The image's size in pixels.
    pub size: (u32, u32),
    /// The image as straight RGBA, 8 bits a channel, `size.0 * size.1 * 4`
    /// bytes, shared with the store.
    pub pixels: Arc<Vec<u8>>,
}

/// Where in the stack of a cell a picture is drawn, from its `z`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PictureLayer {
    /// Under the cells' backgrounds: the picture shows only where a cell has
    /// no background of its own.
    Ground,
    /// Over the backgrounds, under the text.
    Text,
    /// Over the text.
    Over,
}

impl Picture {
    /// The layer the picture's `z` puts it in (the protocol's three: below
    /// `-1 073 741 824`, the other negatives, zero and above).
    pub fn layer(&self) -> PictureLayer {
        match self.z {
            i32::MIN..-1_073_741_824 => PictureLayer::Ground,
            -1_073_741_824..0 => PictureLayer::Text,
            0..=i32::MAX => PictureLayer::Over,
        }
    }
}

/// The pictures of one frame: the grid's and the fill band's, each in the
/// order they are drawn (by layer, then by age). The caller keeps the buffer
/// so a frame allocates nothing once it has grown.
#[derive(Debug, Default)]
pub struct Pictures {
    pub(crate) grid: Vec<Picture>,
    pub(crate) band: Vec<Picture>,
}

impl Pictures {
    /// A list built by hand: what a layer that draws pictures is tested with.
    /// Each list is in drawing order, as [`Session::frame`] fills them.
    ///
    /// [`Session::frame`]: crate::Session::frame
    pub fn new(grid: Vec<Picture>, band: Vec<Picture>) -> Self {
        Self { grid, band }
    }

    /// The pictures that touch the grid's rows.
    pub fn grid(&self) -> &[Picture] {
        &self.grid
    }

    /// The pictures that touch the fill band's rows (its rows are the
    /// band's own, counted from its top).
    pub fn band(&self) -> &[Picture] {
        &self.band
    }

    /// Empties both lists and keeps their buffers.
    pub fn clear(&mut self) {
        self.grid.clear();
        self.band.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL: (u16, u16) = (10, 20);

    fn spec(control: &str, size: (u32, u32)) -> Spec {
        Spec::new(&Command::parse(control.as_bytes()).expect("parses"), size)
    }

    #[test]
    fn an_image_with_no_cells_asked_keeps_its_own_size() {
        let spec = spec("a=p,i=1", (95, 41));
        assert_eq!(spec.span(CELL), (10, 3), "ceil(95 / 10) by ceil(41 / 20)");
        assert_eq!(spec.dest_px(CELL), (95, 41));
    }

    #[test]
    fn the_starting_offset_counts_toward_the_cells() {
        let spec = spec("a=p,i=1,X=5,Y=19", (95, 21));
        assert_eq!(spec.span(CELL), (10, 2));
        let spec = self::spec("a=p,i=1,X=500,Y=500", (10, 20));
        assert_eq!(spec.offset_in(CELL), (9, 19), "cut to the cell");
        assert_eq!(spec.span(CELL), (2, 2));
    }

    #[test]
    fn a_missing_side_follows_the_aspect_ratio() {
        let wide = spec("a=p,i=1,c=10", (200, 100));
        assert_eq!(wide.span(CELL), (10, 3), "100 px wide by 50 px high");
        assert_eq!(wide.dest_px(CELL), (100, 50));
        let tall = spec("a=p,i=1,r=2", (200, 100));
        assert_eq!(tall.span(CELL), (8, 2), "40 px high by 80 px wide");
        assert_eq!(tall.dest_px(CELL), (80, 40));
    }

    #[test]
    fn both_sides_asked_stretch_the_image() {
        let spec = spec("a=p,i=1,c=4,r=3", (200, 100));
        assert_eq!(spec.span(CELL), (4, 3));
        assert_eq!(spec.dest_px(CELL), (40, 60));
    }

    #[test]
    fn the_source_rectangle_is_cut_to_the_image() {
        assert_eq!(
            spec("x=10,y=5,w=500,h=0", (100, 50)).source,
            [10, 5, 90, 45]
        );
        assert_eq!(spec("x=500,y=500", (100, 50)).source, [100, 50, 0, 0]);
        let spec = spec("a=p,x=50,w=20,h=10", (100, 50));
        assert_eq!(spec.span(CELL), (2, 1), "the cells follow the source");
    }

    #[test]
    fn hostile_numbers_stay_small() {
        let spec = spec("a=p,c=4294967295,r=4294967295", (10, 10));
        assert_eq!(spec.span(CELL), (65535, 65535));
        let spec = self::spec("a=p,c=4294967295", (10, 10));
        assert_eq!(spec.span((1, 1)).0, 65535);
        let spec = self::spec("a=p,r=4294967295", (10, 10));
        assert_eq!(spec.span((1, 1)).1, 65535);
        assert_eq!(spec.dest_px((0, 0)).1, 65535, "a zero cell is one pixel");
    }

    #[test]
    fn the_cursor_goes_to_the_cell_after_the_image_on_its_last_row() {
        let spec = spec("a=p,i=1,c=4,r=3", (40, 60));
        let motion = Motion::of(&spec, CELL, 80);
        assert_eq!(
            motion,
            Motion {
                rows: 2,
                cols: 4,
                wrap: false
            }
        );
        assert_eq!(motion.above(), 2);
    }

    #[test]
    fn an_image_as_wide_as_the_screen_ends_on_the_next_line() {
        let spec = spec("a=p,i=1,c=80,r=2", (40, 60));
        let motion = Motion::of(&spec, CELL, 80);
        assert!(motion.wrap);
    }

    #[test]
    fn a_tall_image_anchors_its_top_far_above_the_cursor() {
        let spec = spec("a=p,i=1,c=1,r=500", (40, 60));
        let motion = Motion::of(&spec, CELL, 80);
        assert_eq!(motion.above(), 499, "the anchor still lies 499 rows up");
    }

    #[test]
    fn a_placement_covers_the_cells_of_its_span() {
        let placement = Placement {
            seq: 1,
            image: 1,
            id: 0,
            line: -2,
            col: 5,
            spec: spec("a=p,c=3,r=4", (30, 80)),
        };
        assert_eq!(placement.bottom(CELL), 1);
        assert!(placement.covers(-2, 5, CELL) && placement.covers(1, 7, CELL));
        assert!(!placement.covers(2, 5, CELL) && !placement.covers(0, 8, CELL));
        assert!(!placement.covers(0, 4, CELL) && !placement.covers(-3, 5, CELL));
    }
}
