//! The input line read back from the grid, for a shell that cannot mirror it.
//!
//! zsh hands the dock its line editor's state ([`crate::shell::DockState`] by
//! way of OSC 8133 `u`). fish keeps `BUFFER`, the caret and the suggestion to
//! itself, but it draws all of them, so the dock's line can be recovered from
//! the cells fish painted. [`locate`] finds the rows that line occupies and
//! [`read`] turns them into the same record the mirror produces; the frame
//! hides exactly those rows in the grid and publishes the record to the dock
//! like any other mirror.
//!
//! **The prompt is a contract.** The shell script prints the prompt as the
//! block's OSC 8 anchor followed by two real spaces (the same shape zsh's dock
//! prompt has), so a row that belongs to the line starts at its third column
//! and the second space — printed in the shell's autosuggestion style — is the
//! style sample that tells the suggestion apart from what was typed.
//!
//! **Which anchor is a parameter.** Both functions take the question "does
//! this link's URI anchor the prompt I am looking for" from the caller, so the
//! local prompt is not baked in here and a remote shell's blocks can use the
//! same walk.
//!
//! Everything here reads the grid by line number, under the `Term` lock the
//! frame already holds; nothing allocates in steady state because the output
//! record keeps its capacities across frames.

use alacritty_terminal::grid::{Dimensions, Row};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::{Cell, Flags, Hyperlink};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};

use crate::color::{self, Theme};
use crate::shell::{DockState, DockStatus, Highlight, HighlightColor, HighlightStyle};

/// How many columns the prompt takes before the line starts: the anchor's
/// cell and the style sample's.
const PROMPT_COLS: usize = 2;

/// The grid rows an input line occupies: both ends inclusive, in grid line
/// numbers (the scrollback is negative), so the answer does not depend on how
/// far the window is scrolled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InputLines {
    pub(crate) top: i32,
    pub(crate) bottom: i32,
}

/// A place on the grid: the line in grid line numbers (the scrollback is
/// negative) and the column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GridPos {
    pub(crate) line: i32,
    pub(crate) col: u16,
}

/// The row at `line`, if the grid has one. Indexing is guarded here so the
/// frame path never takes the grid's panic.
fn row_at<T>(term: &Term<T>, line: i32) -> Option<&Row<Cell>> {
    let line = Line(line);
    (line >= term.topmost_line() && line <= term.bottommost_line()).then(|| &term.grid()[line])
}

/// The first link on the row that anchors the prompt being looked for.
fn anchor_in(row: &Row<Cell>, is_anchor: &impl Fn(&str) -> bool) -> Option<Hyperlink> {
    row.into_iter()
        .find_map(|cell| cell.hyperlink().filter(|link| is_anchor(link.uri())))
}

/// Whether any cell of the row carries the link `id`.
fn carries(row: &Row<Cell>, id: &str) -> bool {
    row.into_iter()
        .any(|cell| cell.hyperlink().is_some_and(|link| link.id() == id))
}

/// Whether the row's text continues on the next row (a soft wrap).
fn wraps(row: &Row<Cell>) -> bool {
    row.into_iter()
        .next_back()
        .is_some_and(|cell| cell.flags.contains(Flags::WRAPLINE))
}

/// The rows of the input line the cursor is on.
///
/// - **Top:** the topmost row of the run that carries the link id of the
///   anchored cell on the cursor's row. The id, not the block: the shell can
///   repaint a cached prompt and every parse of the anchor gets a fresh id, so
///   a job notification printed under the old prompt keeps the old id and
///   stays out of the range.
/// - **Bottom:** the cursor's row and its soft-wrapped continuation. Hard
///   line breaks after the cursor are not part of the range — a completion
///   list, or the lines of a multi-line command below the cursor, stay in the
///   grid.
///
/// A cursor row that holds no anchored cell (the shell has just moved to a
/// fresh line inside the command) borrows the nearest anchored row above it.
/// An error when no anchor is within `reach` rows, or the line is taller than
/// `reach`: the grid keeps showing it, the safe direction — and the dock has
/// to step aside for a line that is taller ([`Unlocated::Taller`]).
pub(crate) fn locate<T>(
    term: &Term<T>,
    cursor_line: i32,
    reach: i32,
    is_anchor: impl Fn(&str) -> bool,
) -> Result<InputLines, Unlocated> {
    let floor = cursor_line.saturating_sub(reach);
    let mut line = cursor_line;
    let link = loop {
        if line < floor {
            return Err(Unlocated::NoAnchor);
        }
        if let Some(link) = row_at(term, line).and_then(|row| anchor_in(row, &is_anchor)) {
            break link;
        }
        line -= 1;
    };
    let mut top = line;
    while row_at(term, top - 1).is_some_and(|row| carries(row, link.id())) {
        if top <= floor {
            return Err(Unlocated::Taller);
        }
        top -= 1;
    }
    let mut bottom = cursor_line;
    while bottom < term.bottommost_line().0 && row_at(term, bottom).is_some_and(wraps) {
        bottom += 1;
    }
    Ok(InputLines { top, bottom })
}

/// Why [`locate`] found no input line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unlocated {
    /// No anchor of the prompt near the cursor: the prompt is not on the grid,
    /// or not yet — while an image is decoded mid-read, or a synchronized
    /// update holds the bytes back, a frame can see a new prompt's marks before
    /// its anchor is drawn. Nothing is known about the line, so nothing about
    /// the dock changes.
    NoAnchor,
    /// The anchor is near, but the run of rows that carries its link goes on
    /// past the reach: the line is taller than that. Every cell the shell
    /// draws for the line carries the link (the scripts close it only when
    /// the command starts), so this is the line's own height, not a guess.
    Taller,
}

/// A color as the mirror carries it: resolved, `0xRRGGBB`.
fn highlight_color(rgb: Rgb) -> HighlightColor {
    HighlightColor::Rgb(u32::from(rgb.r) << 16 | u32::from(rgb.g) << 8 | u32::from(rgb.b))
}

/// The style a cell is drawn with, in the vocabulary of the mirror's
/// highlights.
///
/// Colors go through the same resolver the grid's cells use
/// ([`color::resolve_fg`] / [`color::resolve`]), so the application's palette
/// overrides and the dim rule are applied in one place; the result is the raw
/// color, and the dock lifts it to the minimum contrast itself. A default
/// color stays `None` so the dock uses its own base. Reverse video is carried
/// as `standout` with the colors unswapped: the dock does the swap.
fn style_of(cell: &Cell, colors: &Colors, theme: &Theme) -> HighlightStyle {
    let flags = cell.flags;
    let dim = flags.contains(Flags::DIM);
    let plain_fg = matches!(cell.fg, Color::Named(NamedColor::Foreground)) && !dim;
    let plain_bg = matches!(cell.bg, Color::Named(NamedColor::Background));
    HighlightStyle {
        fg: (!plain_fg).then(|| highlight_color(color::resolve_fg(cell.fg, dim, colors, theme))),
        bg: (!plain_bg).then(|| highlight_color(color::resolve(cell.bg, colors, theme))),
        bold: flags.contains(Flags::BOLD),
        underline: flags.intersects(Flags::ALL_UNDERLINES),
        standout: flags.contains(Flags::INVERSE),
        italic: flags.contains(Flags::ITALIC),
    }
}

/// Ends a run of equal style; a default run is not a highlight.
fn close_run(into: &mut DockState, start: usize, end: usize, style: HighlightStyle) {
    if style != HighlightStyle::default() && end > start {
        into.highlights.push(Highlight { start, end, style });
    }
}

/// Fills `into` with the line `found` covers, as the mirror would state it.
///
/// - **Text:** the cells from the third column of the top row, wrapped rows
///   joined and hard line ends written as `\n`. A cell's zero-width
///   characters follow it, so an emoji sequence arrives whole and the dock
///   clusters it with its own walk. Spaces a hard-ended row has after its
///   last ink are not text — the grid cannot tell them from untouched cells —
///   except up to the caret, so a caret after `git ` sits after the space.
/// - **Caret:** the cursor's cell, as a character offset.
/// - **Colors:** runs of equal [`style_of`], default runs left out.
/// - **Suggestion:** the trailing run, after the caret, that is styled like
///   the prompt's second space. Without a distinct sample style everything
///   is `buffer`. Only the dock's typing animation reads the split.
///
/// - **Places:** `cells` gets one [`GridPos`] per character of the display
///   (typed text and suggestion, a line break's too), then one for the place
///   just after the last — the cell a click "at the end" lands on. A cell's
///   zero-width characters share its place. The dock turns a click into the
///   character it landed before ([`crate::shell::DockSelection::click`]) and
///   the place says where the shell has to be clicked for its caret to land
///   there.
///
/// `into.answers` and `into.cluster` are the caller's.
pub(crate) fn read<T>(
    term: &Term<T>,
    found: InputLines,
    cursor: Point,
    colors: &Colors,
    theme: &Theme,
    into: &mut DockState,
    cells: &mut Vec<GridPos>,
) {
    into.status = DockStatus::Live;
    into.predisplay.clear();
    into.buffer.clear();
    into.postdisplay.clear();
    into.prebuffer.clear();
    into.highlights.clear();
    into.insert_keymap = false;
    into.last_ink = None;
    cells.clear();

    let columns = term.columns();
    let last_column = columns.saturating_sub(1);
    let place = |line: i32, col: usize| GridPos {
        line,
        col: u16::try_from(col.min(last_column)).unwrap_or(u16::MAX),
    };
    let mut after = place(found.bottom, 0);
    let mut sample = HighlightStyle::default();
    let mut chars = 0usize;
    let mut caret = None;
    let mut run_start = 0usize;
    let mut run_style = HighlightStyle::default();

    for line in found.top..=found.bottom {
        let Some(row) = row_at(term, line) else {
            continue;
        };
        let first = if line == found.top { PROMPT_COLS } else { 0 };
        if line == found.top {
            sample = row
                .into_iter()
                .nth(PROMPT_COLS - 1)
                .map(|cell| style_of(cell, colors, theme))
                .unwrap_or_default();
        }
        let soft = wraps(row);
        let cursor_here = line == cursor.line.0;
        let row_start = chars;
        let mut end = if soft {
            columns
        } else {
            row.into_iter()
                .enumerate()
                .filter(|(_, cell)| {
                    cell.c != ' '
                        && cell.c != '\0'
                        && !cell
                            .flags
                            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                })
                .map(|(col, _)| col + 1)
                .max()
                .unwrap_or(0)
        };
        if cursor_here && !soft {
            end = end.max(cursor.column.0);
        }
        let mut row_after = first;
        for (col, cell) in row.into_iter().enumerate().take(end).skip(first) {
            if cursor_here && Column(col) == cursor.column {
                caret = Some(chars);
            }
            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            // The place after this cell: a wide character takes two.
            row_after = col
                + if cell.flags.contains(Flags::WIDE_CHAR) {
                    2
                } else {
                    1
                };
            let style = style_of(cell, colors, theme);
            if style != run_style {
                close_run(into, run_start, chars, run_style);
                run_start = chars;
                run_style = style;
            }
            let head = if cell.c == '\0' { ' ' } else { cell.c };
            into.buffer.push(head);
            chars += 1;
            cells.push(place(line, col));
            for &extra in cell.zerowidth().unwrap_or(&[]) {
                into.buffer.push(extra);
                chars += 1;
                cells.push(place(line, col));
            }
        }
        after = place(line, row_after);
        if cursor_here && caret.is_none() {
            // Left of the line's start (inside the prompt) or past its last cell.
            caret = Some(if cursor.column.0 < first {
                row_start
            } else {
                chars
            });
        }
        if !soft && line < found.bottom {
            close_run(into, run_start, chars, run_style);
            run_start = chars + 1;
            run_style = HighlightStyle::default();
            into.buffer.push('\n');
            chars += 1;
            cells.push(after);
        }
    }
    close_run(into, run_start, chars, run_style);
    cells.push(after);

    let caret = caret.unwrap_or(chars).min(chars);
    into.cursor = caret;
    into.display_chars = chars;

    // The suggestion: the last run, from the caret on, drawn like the sample.
    if sample != HighlightStyle::default()
        && let Some(last) = into.highlights.last()
        && last.end == chars
        && last.style == sample
    {
        let split = last.start.max(caret);
        if split < chars
            && let Some((byte, _)) = into.buffer.char_indices().nth(split)
        {
            let DockState {
                buffer,
                postdisplay,
                ..
            } = into;
            if let Some(tail) = buffer.get(byte..) {
                postdisplay.push_str(tail);
            }
            buffer.truncate(byte);
        }
    }
}
