//! Images shown by Unicode placeholders.
//!
//! A program that can neither pass escape codes through to the terminal nor
//! keep track of where an image sits — an editor, a multiplexer — still moves
//! text around, and text is all a placeholder is. The image is sent the usual
//! way and given a *virtual placement* (`U=1`): a box of `c` × `r` cells it
//! will be fitted to, with no place on the screen. Then the character
//! [`PLACEHOLDER`] stands for one cell of that box wherever it is printed: the
//! cell's foreground colour is the image's id, its underline colour the
//! placement's, and up to three combining marks after it say which row and
//! column of the box the cell shows and the top byte of an id wider than 24
//! bits. Whatever moves, scrolls or erases the text moves, scrolls or erases
//! the image.
//!
//! **Nothing is kept about the cells.** The frame reads the grid's visible
//! lines when a virtual placement exists (and not at all when none does), turns
//! the cells into runs — a stretch of one line's cells that show one row of
//! the box, side by side — and makes a [`Picture`](super::Picture) of each
//! run, so a screenful of image is one picture a line. The cells are read as
//! they are, which is why a marks-only cell can leave the row, the column and
//! the top byte out: it takes what the cell to its left says
//! ([`Reader::cell`]).
//!
//! The marks' numbers are the position of the mark in [`DIACRITICS`], the
//! table kitty's clients write with.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::Cell;
use alacritty_terminal::vte::ansi::Color;

use super::store::Window;

/// The character a cell holds when it shows part of an image.
pub(crate) const PLACEHOLDER: char = '\u{10EEEE}';

/// Whether a cell holding `c` has a glyph to draw. A placeholder has none: its
/// image is drawn in its place, and a font that has no such character would
/// draw a box.
pub(crate) fn has_glyph(c: char) -> bool {
    c != ' ' && c != PLACEHOLDER
}

/// The layer the pictures of placeholders are in: over the cells' backgrounds
/// and under their text, so a program's own text on top of an image shows.
pub(super) const LAYER: i32 = -1;

/// The most cells a virtual placement may cover in either direction, so that
/// the arithmetic on a hostile `c=` or `r=` stays small.
const MAX_CELLS: u32 = u16::MAX as u32;

/// The combining marks that stand for the numbers `0..`, in order: kitty's
/// table of the marks of one combining class that normalization leaves alone.
/// Sorted, so that a mark is found by its position.
const DIACRITICS: [u32; 297] = [
    0x0305, 0x030D, 0x030E, 0x0310, 0x0312, 0x033D, 0x033E, 0x033F, 0x0346, 0x034A, 0x034B, 0x034C,
    0x0350, 0x0351, 0x0352, 0x0357, 0x035B, 0x0363, 0x0364, 0x0365, 0x0366, 0x0367, 0x0368, 0x0369,
    0x036A, 0x036B, 0x036C, 0x036D, 0x036E, 0x036F, 0x0483, 0x0484, 0x0485, 0x0486, 0x0487, 0x0592,
    0x0593, 0x0594, 0x0595, 0x0597, 0x0598, 0x0599, 0x059C, 0x059D, 0x059E, 0x059F, 0x05A0, 0x05A1,
    0x05A8, 0x05A9, 0x05AB, 0x05AC, 0x05AF, 0x05C4, 0x0610, 0x0611, 0x0612, 0x0613, 0x0614, 0x0615,
    0x0616, 0x0617, 0x0657, 0x0658, 0x0659, 0x065A, 0x065B, 0x065D, 0x065E, 0x06D6, 0x06D7, 0x06D8,
    0x06D9, 0x06DA, 0x06DB, 0x06DC, 0x06DF, 0x06E0, 0x06E1, 0x06E2, 0x06E4, 0x06E7, 0x06E8, 0x06EB,
    0x06EC, 0x0730, 0x0732, 0x0733, 0x0735, 0x0736, 0x073A, 0x073D, 0x073F, 0x0740, 0x0741, 0x0743,
    0x0745, 0x0747, 0x0749, 0x074A, 0x07EB, 0x07EC, 0x07ED, 0x07EE, 0x07EF, 0x07F0, 0x07F1, 0x07F3,
    0x0816, 0x0817, 0x0818, 0x0819, 0x081B, 0x081C, 0x081D, 0x081E, 0x081F, 0x0820, 0x0821, 0x0822,
    0x0823, 0x0825, 0x0826, 0x0827, 0x0829, 0x082A, 0x082B, 0x082C, 0x082D, 0x0951, 0x0953, 0x0954,
    0x0F82, 0x0F83, 0x0F86, 0x0F87, 0x135D, 0x135E, 0x135F, 0x17DD, 0x193A, 0x1A17, 0x1A75, 0x1A76,
    0x1A77, 0x1A78, 0x1A79, 0x1A7A, 0x1A7B, 0x1A7C, 0x1B6B, 0x1B6D, 0x1B6E, 0x1B6F, 0x1B70, 0x1B71,
    0x1B72, 0x1B73, 0x1CD0, 0x1CD1, 0x1CD2, 0x1CDA, 0x1CDB, 0x1CE0, 0x1DC0, 0x1DC1, 0x1DC3, 0x1DC4,
    0x1DC5, 0x1DC6, 0x1DC7, 0x1DC8, 0x1DC9, 0x1DCB, 0x1DCC, 0x1DD1, 0x1DD2, 0x1DD3, 0x1DD4, 0x1DD5,
    0x1DD6, 0x1DD7, 0x1DD8, 0x1DD9, 0x1DDA, 0x1DDB, 0x1DDC, 0x1DDD, 0x1DDE, 0x1DDF, 0x1DE0, 0x1DE1,
    0x1DE2, 0x1DE3, 0x1DE4, 0x1DE5, 0x1DE6, 0x1DFE, 0x20D0, 0x20D1, 0x20D4, 0x20D5, 0x20D6, 0x20D7,
    0x20DB, 0x20DC, 0x20E1, 0x20E7, 0x20E9, 0x20F0, 0x2CEF, 0x2CF0, 0x2CF1, 0x2DE0, 0x2DE1, 0x2DE2,
    0x2DE3, 0x2DE4, 0x2DE5, 0x2DE6, 0x2DE7, 0x2DE8, 0x2DE9, 0x2DEA, 0x2DEB, 0x2DEC, 0x2DED, 0x2DEE,
    0x2DEF, 0x2DF0, 0x2DF1, 0x2DF2, 0x2DF3, 0x2DF4, 0x2DF5, 0x2DF6, 0x2DF7, 0x2DF8, 0x2DF9, 0x2DFA,
    0x2DFB, 0x2DFC, 0x2DFD, 0x2DFE, 0x2DFF, 0xA66F, 0xA67C, 0xA67D, 0xA6F0, 0xA6F1, 0xA8E0, 0xA8E1,
    0xA8E2, 0xA8E3, 0xA8E4, 0xA8E5, 0xA8E6, 0xA8E7, 0xA8E8, 0xA8E9, 0xA8EA, 0xA8EB, 0xA8EC, 0xA8ED,
    0xA8EE, 0xA8EF, 0xA8F0, 0xA8F1, 0xAAB0, 0xAAB2, 0xAAB3, 0xAAB7, 0xAAB8, 0xAABE, 0xAABF, 0xAAC1,
    0xFE20, 0xFE21, 0xFE22, 0xFE23, 0xFE24, 0xFE25, 0xFE26, 0x10A0F, 0x10A38, 0x1D185, 0x1D186,
    0x1D187, 0x1D188, 0x1D189, 0x1D1AA, 0x1D1AB, 0x1D1AC, 0x1D1AD, 0x1D242, 0x1D243, 0x1D244,
];

/// The combining mark that stands for `number`.
#[cfg(test)]
pub(super) fn mark_for(number: usize) -> char {
    char::from_u32(DIACRITICS[number]).expect("a table of characters")
}

/// The number a combining mark stands for.
fn number_of(mark: char) -> Option<u32> {
    DIACRITICS
        .binary_search(&u32::from(mark))
        .ok()
        .and_then(|at| u32::try_from(at).ok())
}

/// A virtual placement: the box of cells an image is fitted to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Virtual {
    /// Names the placement for what hangs on it: the same count as the real
    /// placements'.
    pub(super) seq: u64,
    /// The generation of the image.
    pub(super) image: u64,
    /// The program's id for this placement; 0 for none.
    pub(super) id: u32,
    /// `c=` and `r=` as written; 0 is "as the image's own size asks".
    pub(super) columns: u32,
    pub(super) rows: u32,
}

/// What one placeholder cell says, as written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Mark {
    /// The foreground colour as a number: the image's id, 24 bits at most.
    /// `None` for a colour that is no number (the default ones, the 16 named).
    pub(super) id: Option<u32>,
    /// The underline colour as a number: the placement's id, 0 for none.
    pub(super) placement: u32,
    /// The first three marks after the character: the row, the column and
    /// the id's top byte, each left out when it is not there.
    pub(super) row: Option<u32>,
    pub(super) column: Option<u32>,
    pub(super) top: Option<u32>,
}

/// A colour as the number a client wrote into it.
fn ink(color: Color) -> Option<u32> {
    match color {
        Color::Indexed(index) => Some(u32::from(index)),
        Color::Spec(rgb) => Some(u32::from(rgb.r) << 16 | u32::from(rgb.g) << 8 | u32::from(rgb.b)),
        Color::Named(_) => None,
    }
}

/// What `cell` says, if it is a placeholder.
pub(super) fn mark_of(cell: &Cell) -> Option<Mark> {
    if cell.c != PLACEHOLDER {
        return None;
    }
    // A mark that is not in the table ends the numbers: what follows it is
    // not meant for them.
    let mut numbers = cell
        .zerowidth()
        .unwrap_or_default()
        .iter()
        .map_while(|&mark| number_of(mark));
    Some(Mark {
        id: ink(cell.fg),
        placement: cell.underline_color().and_then(ink).unwrap_or(0),
        row: numbers.next(),
        column: numbers.next(),
        top: numbers.next(),
    })
}

/// A stretch of one line's cells that show one row of a virtual placement's
/// box, side by side and in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Run {
    /// The grid line and the first cell's column.
    pub(super) line: i32,
    pub(super) col: u16,
    /// How many cells.
    pub(super) count: u16,
    /// The image's id, top byte and all, and the placement's.
    pub(super) image: u32,
    pub(super) placement: u32,
    /// The row of the box the cells show, and the column of the first.
    pub(super) row: u32,
    pub(super) first: u32,
}

/// What the placeholder cell to the left settled on.
#[derive(Clone, Copy)]
struct Left {
    col: u16,
    id: Option<u32>,
    placement: u32,
    row: u32,
    column: u32,
    top: u32,
}

/// What a cell with `mark` settles on, given the placeholder cell to its left
/// (`left`, which is the cell right next to it): the row, the column and the
/// top byte, where the cell left them out as far as the cell to its left can
/// tell and as zero where it cannot.
///
/// - No marks: the next column of the same row, if the cell to the left has
///   the same colours.
/// - A row only: the next column, if the cell to the left has the same colours
///   and the same row.
/// - A row and a column: the top byte, if the cell to the left has the same
///   colours and the same row and stands in the column before.
fn settle(mark: &Mark, left: Option<&Left>) -> (u32, u32, u32) {
    let same = left.filter(|left| left.id == mark.id && left.placement == mark.placement);
    match (mark.row, mark.column, mark.top) {
        (Some(row), Some(column), Some(top)) => (row, column, top),
        (Some(row), Some(column), None) => {
            let top = same
                .filter(|left| left.row == row && left.column.checked_add(1) == Some(column))
                .map_or(0, |left| left.top);
            (row, column, top)
        }
        (Some(row), None, _) => match same.filter(|left| left.row == row) {
            Some(left) => (row, left.column.saturating_add(1), left.top),
            None => (row, 0, 0),
        },
        (None, _, _) => match same {
            Some(left) => (left.row, left.column.saturating_add(1), left.top),
            None => (0, 0, 0),
        },
    }
}

/// Turns lines of cells into [`Run`]s, a cell at a time from the left.
#[derive(Default)]
pub(super) struct Reader {
    pub(super) runs: Vec<Run>,
    line: i32,
    left: Option<Left>,
}

impl Reader {
    /// The next cells are on `line`, from its first.
    pub(super) fn line(&mut self, line: i32) {
        self.line = line;
        self.left = None;
    }

    /// The cell at `col` says `mark`, or is no placeholder.
    pub(super) fn cell(&mut self, col: u16, mark: Option<Mark>) {
        let Some(mark) = mark else {
            self.left = None;
            return;
        };
        let left = self
            .left
            .filter(|left| u32::from(left.col) + 1 == u32::from(col));
        let (row, column, top) = settle(&mark, left.as_ref());
        self.left = Some(Left {
            col,
            id: mark.id,
            placement: mark.placement,
            row,
            column,
            top,
        });
        // An id of 0 names no image; the colours of text that was never meant
        // to be one are most often the default's.
        let image = mark.id.unwrap_or(0) | (top & 0xff) << 24;
        if image == 0 {
            return;
        }
        if let Some(run) = self.runs.last_mut()
            && run.line == self.line
            && u32::from(run.col) + u32::from(run.count) == u32::from(col)
            && run.image == image
            && run.placement == mark.placement
            && run.row == row
            && u64::from(run.first) + u64::from(run.count) == u64::from(column)
        {
            run.count += 1;
            return;
        }
        self.runs.push(Run {
            line: self.line,
            col,
            count: 1,
            image,
            placement: mark.placement,
            row,
            first: column,
        });
    }
}

/// The placeholder cells of the lines a frame draws, as runs: the grid's and
/// the fill band's.
#[derive(Debug, Default)]
pub(crate) struct Marks {
    pub(super) grid: Vec<Run>,
    pub(super) band: Option<Vec<Run>>,
}

impl Marks {
    /// The runs of the grid's lines `grid` and, if the band shows any, of the
    /// lines `band`.
    pub(super) fn read<T>(
        term: &Term<T>,
        grid: Window,
        band: Option<Window>,
        columns: u16,
    ) -> Self {
        Self {
            grid: read(term, grid, columns),
            band: band.map(|band| read(term, band, columns)),
        }
    }

    /// All the runs, the grid's first.
    pub(super) fn all(&self) -> impl Iterator<Item = &Run> {
        self.grid.iter().chain(self.band.iter().flatten())
    }
}

/// The runs of the placeholder cells in the grid lines of `window`.
fn read<T>(term: &Term<T>, window: Window, columns: u16) -> Vec<Run> {
    let grid = term.grid();
    let first = window.first.max(term.topmost_line().0);
    let last = window.last.min(term.bottommost_line().0);
    let width = usize::from(columns).min(term.columns());
    let mut reader = Reader::default();
    for line in first..=last {
        reader.line(line);
        let row = &grid[Line(line)];
        for col in 0..width {
            let mark = mark_of(&row[Column(col)]);
            reader.cell(u16::try_from(col).unwrap_or(u16::MAX), mark);
        }
    }
    reader.runs
}

/// How a run's part of an image is drawn: which cell it starts in and how far
/// into it, the size on the screen and the part of the image's pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Fitted {
    /// Cells right of the run's first the picture starts at.
    pub(super) col: u16,
    /// Where in that cell it starts, pixels.
    pub(super) x_offset: u32,
    pub(super) y_offset: u32,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) source: [u32; 4],
}

/// The part of an `image` (pixels) a `run` shows when the image is fitted to
/// the `cells` of its virtual placement (a side of 0 is as many cells as the
/// image's own size asks for), in cells of `cell` pixels. The image keeps its
/// aspect ratio and is centred in the box, so the cells the box has more of
/// than the image needs are empty; `None` for a run that is wholly in them.
///
/// The edges of what is shown are rounded, not its sizes, so that the runs of
/// two lines that meet at an edge of the image take the same row of pixels.
pub(super) fn fit(
    run: &Run,
    cell: (u16, u16),
    image: (u32, u32),
    cells: (u32, u32),
) -> Option<Fitted> {
    let (cw, ch) = (f64::from(cell.0.max(1)), f64::from(cell.1.max(1)));
    let (iw, ih) = (f64::from(image.0), f64::from(image.1));
    if image.0 == 0 || image.1 == 0 {
        return None;
    }
    let side = |cells: u32, natural: f64| {
        if cells == 0 {
            natural.ceil().clamp(1.0, f64::from(MAX_CELLS))
        } else {
            f64::from(cells.min(MAX_CELLS))
        }
    };
    let (box_w, box_h) = (side(cells.0, iw / cw) * cw, side(cells.1, ih / ch) * ch);
    let scale = (box_w / iw).min(box_h / ih);
    let (shown_w, shown_h) = (iw * scale, ih * scale);
    let (left, top) = ((box_w - shown_w) / 2.0, (box_h - shown_h) / 2.0);
    // The run's rectangle in the box, and what of the image is in it.
    let x0 = f64::from(run.first) * cw;
    let x1 = x0 + f64::from(run.count) * cw;
    let y0 = f64::from(run.row) * ch;
    let y1 = y0 + ch;
    let (vx0, vx1) = (x0.max(left), x1.min(left + shown_w));
    let (vy0, vy1) = (y0.max(top), y1.min(top + shown_h));
    if vx1 <= vx0 || vy1 <= vy0 {
        return None;
    }
    // On the screen, from the run's own corner.
    let (px0, px1) = ((vx0 - x0).round(), (vx1 - x0).round());
    let (py0, py1) = ((vy0 - y0).round(), (vy1 - y0).round());
    // In the image.
    let along = |from: f64, to: f64, origin: f64, extent: f64| {
        let edge = |at: f64| ((at - origin) / scale).round().clamp(0.0, extent);
        (edge(from), edge(to))
    };
    let (sx0, sx1) = along(vx0, vx1, left, iw);
    let (sy0, sy1) = along(vy0, vy1, top, ih);
    if px1 <= px0 || py1 <= py0 || sx1 <= sx0 || sy1 <= sy0 {
        return None;
    }
    let skipped = (px0 / cw).floor();
    let pixels = |value: f64| value as u32;
    Some(Fitted {
        col: u16::try_from(pixels(skipped)).unwrap_or(u16::MAX),
        x_offset: pixels(px0 - skipped * cw),
        y_offset: pixels(py0),
        width: pixels(px1 - px0),
        height: pixels(py1 - py0),
        source: [
            pixels(sx0),
            pixels(sy0),
            pixels(sx1 - sx0),
            pixels(sy1 - sy0),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A placeholder cell that wrote the marks as numbers.
    fn mark(id: u32, marks: &[u32]) -> Option<Mark> {
        let mut marks = marks.iter().copied();
        Some(Mark {
            id: Some(id),
            placement: 0,
            row: marks.next(),
            column: marks.next(),
            top: marks.next(),
        })
    }

    fn read_line(cells: &[Option<Mark>]) -> Vec<Run> {
        let mut reader = Reader::default();
        reader.line(0);
        for (col, cell) in cells.iter().enumerate() {
            reader.cell(col as u16, *cell);
        }
        reader.runs
    }

    fn run(col: u16, count: u16, image: u32, row: u32, first: u32) -> Run {
        Run {
            line: 0,
            col,
            count,
            image,
            placement: 0,
            row,
            first,
        }
    }

    #[test]
    fn the_table_is_kittys_and_a_mark_is_found_by_its_number() {
        assert_eq!(DIACRITICS.len(), 297);
        assert!(DIACRITICS.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(number_of('\u{305}'), Some(0));
        assert_eq!(number_of('\u{30D}'), Some(1));
        assert_eq!(number_of('\u{30E}'), Some(2));
        assert_eq!(number_of('\u{1D244}'), Some(296));
        assert_eq!(number_of('\u{301}'), None, "a mark normalization may fuse");
        assert_eq!(number_of('a'), None);
    }

    #[test]
    fn a_two_by_two_placeholder_is_a_run_a_line() {
        // `\e[38;5;42m` and (row, column) written out for every cell.
        let first = read_line(&[mark(42, &[0, 0]), mark(42, &[0, 1])]);
        let second = read_line(&[mark(42, &[1, 0]), mark(42, &[1, 1])]);
        assert_eq!(first, [run(0, 2, 42, 0, 0)]);
        assert_eq!(second, [run(0, 2, 42, 1, 0)]);
    }

    #[test]
    fn the_top_byte_is_the_third_mark() {
        let runs = read_line(&[mark(42, &[0, 0, 2]), mark(42, &[0, 1, 2])]);
        assert_eq!(runs, [run(0, 2, 42 + (2 << 24), 0, 0)]);
    }

    #[test]
    fn a_cell_with_no_marks_goes_on_from_the_one_to_its_left() {
        // The second line of the two-rows-by-three example: the row of the
        // first cell only.
        let runs = read_line(&[mark(42, &[1]), mark(42, &[]), mark(42, &[])]);
        assert_eq!(runs, [run(0, 3, 42, 1, 0)]);
    }

    #[test]
    fn what_a_cell_leaves_out_comes_from_the_left_only_when_the_colours_agree() {
        // The ids differ: the second cell starts over from the top left.
        let runs = read_line(&[mark(42, &[1, 4]), mark(43, &[])]);
        assert_eq!(runs, [run(0, 1, 42, 1, 4), run(1, 1, 43, 0, 0)]);
        // A row that is not the left cell's: no column to follow.
        let runs = read_line(&[mark(42, &[1, 4]), mark(42, &[2])]);
        assert_eq!(runs, [run(0, 1, 42, 1, 4), run(1, 1, 42, 2, 0)]);
        // The placements differ.
        let mut other = mark(42, &[]).expect("a mark");
        other.placement = 7;
        let runs = read_line(&[mark(42, &[0, 0]), Some(other)]);
        assert_eq!(runs.len(), 2);
        assert_eq!((runs[1].row, runs[1].first, runs[1].placement), (0, 0, 7));
    }

    #[test]
    fn the_top_byte_follows_a_neighbour_one_column_before_only() {
        let runs = read_line(&[mark(42, &[0, 0, 3]), mark(42, &[0, 1])]);
        assert_eq!(runs, [run(0, 2, 42 + (3 << 24), 0, 0)]);
        // A gap in the columns: the byte is not the left cell's.
        let runs = read_line(&[mark(42, &[0, 0, 3]), mark(42, &[0, 5])]);
        assert_eq!(runs, [run(0, 1, 42 + (3 << 24), 0, 0), run(1, 1, 42, 0, 5)]);
    }

    #[test]
    fn a_cell_that_is_not_next_to_the_last_placeholder_does_not_follow_it() {
        let runs = read_line(&[mark(42, &[0, 0]), None, mark(42, &[])]);
        assert_eq!(runs, [run(0, 1, 42, 0, 0), run(2, 1, 42, 0, 0)]);
    }

    #[test]
    fn cells_that_show_no_image_make_no_run() {
        let nothing = Some(Mark::default());
        assert!(read_line(&[nothing, mark(0, &[0, 0])]).is_empty());
    }

    #[test]
    fn a_new_line_starts_over() {
        let mut reader = Reader::default();
        reader.line(0);
        reader.cell(0, mark(42, &[0, 0]));
        reader.line(1);
        reader.cell(0, mark(42, &[]));
        assert_eq!(reader.runs.len(), 2);
        assert_eq!((reader.runs[1].line, reader.runs[1].row), (1, 0));
    }

    const CELL: (u16, u16) = (10, 20);

    #[test]
    fn an_image_that_fills_the_box_is_shown_whole_to_the_runs() {
        // 40 × 40 pixels in 4 × 2 cells of 10 × 20: it fills the box.
        let first = fit(&run(3, 4, 1, 0, 0), CELL, (40, 40), (4, 2)).expect("shown");
        assert_eq!(
            first,
            Fitted {
                col: 0,
                x_offset: 0,
                y_offset: 0,
                width: 40,
                height: 20,
                source: [0, 0, 40, 20],
            }
        );
        let second = fit(&run(3, 4, 1, 1, 0), CELL, (40, 40), (4, 2)).expect("shown");
        assert_eq!((second.source, second.height), ([0, 20, 40, 20], 20));
        // A run of half a line shows half the width.
        let half = fit(&run(3, 2, 1, 0, 2), CELL, (40, 40), (4, 2)).expect("shown");
        assert_eq!((half.source, half.width), ([20, 0, 20, 20], 20));
    }

    #[test]
    fn a_wide_image_is_centred_in_a_taller_box_and_the_empty_rows_show_nothing() {
        // 100 × 20 in 5 × 4 cells (50 × 80): scaled to 50 × 10, 35 px down.
        let image = (100, 20);
        assert_eq!(fit(&run(0, 5, 1, 0, 0), CELL, image, (5, 4)), None);
        let second = fit(&run(0, 5, 1, 1, 0), CELL, image, (5, 4)).expect("shown");
        assert_eq!(
            second,
            Fitted {
                col: 0,
                x_offset: 0,
                y_offset: 15,
                width: 50,
                height: 5,
                source: [0, 0, 100, 10],
            }
        );
        let third = fit(&run(0, 5, 1, 2, 0), CELL, image, (5, 4)).expect("shown");
        assert_eq!((third.y_offset, third.height), (0, 5));
        assert_eq!(third.source, [0, 10, 100, 10]);
        assert_eq!(fit(&run(0, 5, 1, 3, 0), CELL, image, (5, 4)), None);
    }

    #[test]
    fn a_tall_image_is_centred_in_a_wider_box_and_starts_in_a_later_cell() {
        // 20 × 80 in 4 × 4 cells (40 × 80): scaled to 20 × 80, 10 px right.
        let shown = fit(&run(0, 4, 1, 0, 0), CELL, (20, 80), (4, 4)).expect("shown");
        assert_eq!((shown.col, shown.x_offset, shown.width), (1, 0, 20));
        // With 6 columns it is 20 px right: two cells.
        let shown = fit(&run(0, 6, 1, 0, 0), CELL, (20, 80), (6, 4)).expect("shown");
        assert_eq!((shown.col, shown.x_offset, shown.width), (2, 0, 20));
        assert_eq!(shown.source, [0, 0, 20, 20]);
    }

    #[test]
    fn the_runs_of_two_lines_take_the_pixels_that_meet() {
        // An image whose scale is not a whole number: no row of pixels is
        // taken twice or lost between the lines.
        let image = (97, 61);
        let mut end = 0;
        for row in 0..3 {
            let shown = fit(&run(0, 5, 1, row, 0), CELL, image, (5, 3)).expect("shown");
            if row > 0 {
                assert_eq!(shown.source[1], end, "row {row}");
            }
            end = shown.source[1] + shown.source[3];
        }
        assert_eq!(end, 61);
    }

    #[test]
    fn a_side_of_zero_is_the_cells_the_image_needs() {
        // 95 × 41 pixels is 10 × 3 cells (100 × 60): scaled by 100 / 95 and
        // centred, so the last line holds only the bottom of the image.
        let shown = fit(&run(0, 10, 1, 2, 0), CELL, (95, 41), (0, 0)).expect("shown");
        assert_eq!((shown.width, shown.height), (100, 12));
        assert_eq!(shown.source, [0, 30, 95, 11]);
        // One side given: the other is the image's own.
        let shown = fit(&run(0, 10, 1, 2, 0), CELL, (95, 41), (10, 0)).expect("shown");
        assert_eq!(shown.source, [0, 30, 95, 11]);
    }

    #[test]
    fn an_image_with_no_pixels_shows_nothing() {
        assert_eq!(fit(&run(0, 1, 1, 0, 0), CELL, (0, 10), (1, 1)), None);
    }
}
