//! What the dock draws: [`DockState`] with [`DockContext`] → dock cells.
//!
//! What [`crate::Session::frame`] does for the grid, this module does for the
//! dock, and by the same rule: **the decision here, the painting there**. Not
//! text but **cells** cross the boundary (color, style, column, row), plus the
//! caret's column and the surface's two colors; the shell's phase, the syntax
//! of `region_highlight`, the `PREDISPLAY`/`POSTDISPLAY` split **and the
//! overflow rule of the context line** stay on this side — the drawing side
//! does not know "what it means". The overflow stopping here is not a matter
//! of placement: which half gets shortened (the path is shortened, the branch
//! is not) is a product decision, not a pixel decision.
//!
//! **Two rows, two lifetimes:** the top row comes from the mirror and is
//! refreshed per key, the bottom row comes from the context and per prompt.
//!
//! The body is **pure**: it takes no lock, it does not see `Session`. Its
//! only caller is [`crate::Session::dock`], which takes and releases the leaf
//! lock; tests enter here without a PTY.

use unicode_width::UnicodeWidthChar;

use crate::cluster::{ClusterId, Clusters, Walk};
use crate::color::{self, LinearRgba, Theme};
use crate::session::{Cell, CellHalf, SelectKind, SelectionRun, UnderlineStyle, WORD_SEPARATORS};

use crate::settings::HostMark;
use crate::shell::{
    ButtonState, DockContext, DockState, DockStatus, HighlightColor, HighlightStyle, ProgramBar,
    ProgramTone, Reconnect, RemoteStats, STATS_HISTORY, ShellPhase, ShellState, StatsForm,
    Transfer, TransferAction, TransferTone,
};

/// The dock's surface in a frame — everything **outside** the cells, resolved.
///
/// Cells flow from the sink (the `frame()` precedent); only values that are
/// single per frame are here. The phase and exit code do **not** cross the
/// boundary: the `>` mark's color is resolved here, the drawing side takes it
/// as an ordinary glyph.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dock {
    /// The surface's ground; it must be **opaque** (see [`render`]).
    pub ground: LinearRgba,
    /// Color of the **top** hairline separating the dock from the grid: in a
    /// remote session the color of the host's mark (the theme's `info` when
    /// unmarked), under a program's guide bar the bar's tone, otherwise
    /// [`Self::separator`].
    ///
    /// A separate field, because the second hairline (between the input block
    /// and the context row) does not convey distance — that is a division,
    /// this is the surface's edge.
    pub edge: LinearRgba,
    /// The hairline separating the input block from the context row.
    pub separator: LinearRgba,
    /// The caret's place in the input block — row **and** column: a long
    /// line wraps, so the caret can stand on the second visual row too. The
    /// row is inside the vertical window, dock-local (`0` = the first drawn
    /// input row). `None` → no caret is drawn (ZLE is not editing the line or
    /// the mirror could not be read).
    pub caret: Option<DockCaret>,
    /// Color of the text under the caret block — the same rule and the same
    /// value as [`crate::Cursor::text`] in the grid.
    pub caret_text: LinearRgba,
    /// The prompt mark's color: the shell's phase.
    ///
    /// **The color crosses, the shape does not.** The mark is not a cell: if
    /// it crossed the boundary as a character, the user's font's `>` would be
    /// drawn, yet it is the terminal's own mark (`bt_atlas::RuleKind::Chevron`).
    /// The decision is here — which color, i.e. what the shell is doing — the
    /// painting there.
    ///
    /// The **same vocabulary** as the grid's block stripe
    /// ([`crate::Block::stripe`]) and now the same shape: both are a prompt
    /// mark in the phase color.
    ///
    /// `None` → no mark is drawn: the vertical window has scrolled and the
    /// **first** row of the input is not on screen. The mark is the
    /// prompt's place; standing next to a continuation row it would read as
    /// if the command started there.
    ///
    /// The runs of the mouse selection are **not** in this type: one run per
    /// visual row and their count depends on the row count, so they do not
    /// fit a `Copy` field — they flow into the caller's buffer
    /// ([`crate::Session::dock`]'s `selection`, the [`crate::SelectionRuns`]
    /// precedent).
    pub sigil: Option<LinearRgba>,
    /// The **filled** share of the top hairline, in ten-thousandths
    /// (`0..=10_000`): while an upload to a remote directory is running the
    /// line is a progress bar. The filled
    /// part is in [`Self::edge`]'s color, the rest in [`Self::track`]'s;
    /// `None` → the line is entirely `edge`.
    pub progress: Option<u16>,
    /// The progress bar's **empty track**: the mark's color on a
    /// marked host, the separator's on an unmarked one. The filled part is
    /// always the theme's `info` ([`Self::edge`]) — a red filling bar on prod
    /// read like an error (the user, visual check). A separate field, because
    /// [`Self::separator`] is also the second hairline's color.
    pub track: LinearRgba,
    /// The buttons of the upload row, left to right; at most
    /// two. The cells (label) flow from the sink, the fill and border from
    /// here — the layout's decision, not the drawing's
    /// ([`transfer_button_at`] reads the same layout).
    pub buttons: [Option<DockButton>; 2],
}

/// A button of the upload row: the **dock-local** column range `[start, end)`
/// on the context row (at the small class's pitch), its color and its state.
///
/// The range is the whole of the fill and the very hit area: the inner
/// padding is inside the range (the label starts at `start + 1`), so the
/// fill's edge and the hit's edge are the same column boundary.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DockButton {
    pub start: u16,
    pub end: u16,
    /// Color of the fill and border: the color of the host's mark; the alpha
    /// comes from the state, in `bt-gpu`.
    pub color: LinearRgba,
    pub state: ButtonState,
}

/// The dock caret's place: the **screen** column in the input block and the
/// row inside the vertical window.
///
/// The two numbers are in named fields, not two `u16`s side by side — the
/// reason of [`DockCols`]: were they passed swapped, the symptom would show
/// only on a wrapped row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DockCaret {
    pub col: u16,
    pub row: u16,
}

/// The column budget of the dock's two rows.
///
/// **One type, two numbers**, and they are not carried as separate
/// parameters: both are `u16` and both are "how many columns" — side by side
/// in a signature the caller could silently pass them swapped, and the
/// symptom would show only in a narrow window, only on the context row. For
/// the same reason [`Session::dock`] takes this type too
/// ([`crate::Session::dock`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DockCols {
    /// The width of the input block: the grid's column count. The dock uses
    /// the same columns and a line that overflows **wraps**.
    pub grid: u16,
    /// The context row's budget. A separate number, because that row is drawn
    /// in a **small point size**: more letters fit in the same pixel strip.
    /// The drawing side supplies the number (`bt_gpu`'s `context_cols`), this
    /// crate sees no pixels — the value is a **budget**, not a point-size
    /// decision. Passed equal to `grid`, the row behaves as it does today.
    pub context: u16,
}

/// The room that can be given to the dock's input block: up to what share of
/// the grid's rows and wrapping at how many columns — the argument of
/// [`crate::Session::frame`].
///
/// **The layout decision is the drawer's**, `bt-gpu` supplies the numbers
/// (the precedent of [`DockCols`]): the ceiling is a design ratio
/// (`bt_gpu`'s `DOCK_MAX_SHARE`) and this crate sees neither pixels nor the
/// window. **The ratio crosses, not the row count**: the grid's row count is
/// read only once, in `frame()` under the `Term` lock ([`crate::Cursor::rows`])
/// and the drawing side keeps no second copy of it — the budget is applied to
/// that read. `frame()` clamps the number of input rows to draw
/// ([`crate::Cursor::input_rows`]) with this budget **in the same read** as
/// the suppression decision, and the dock's drawing takes the number as an
/// argument, without deriving it a second time (the mirror itself advancing
/// between two rounds is separate, a known one-frame limit: [`render_with`]).
///
/// The two numbers are in one type and in named fields, for the same reason
/// as `DockCols`: two numbers side by side could be silently passed swapped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DockBudget {
    /// The ceiling of input rows, as a **ratio** of the grid's rows (`0.5` →
    /// half); rounded down, and `0` also means at least one row (the dock's
    /// input row never disappears).
    pub share: f32,
    /// The width of the wrap, in columns: the width the dock's input block
    /// shares with the grid ([`DockCols::grid`]).
    pub cols: u16,
}

impl DockBudget {
    /// The number of input rows to draw with this budget in a grid of
    /// `grid_rows` rows, for a display that wants `needed` rows: clamped to
    /// the ceiling and **at least one**.
    pub(crate) fn fit(self, needed: usize, grid_rows: u16) -> u16 {
        // audit: the ratio is expected in `[0, 1]` but an out-of-range value
        // is not a panic either — `as` saturates, `min` still cuts at the row count.
        let cap = (f32::from(grid_rows) * self.share).floor() as u16;
        let needed = u16::try_from(needed).unwrap_or(u16::MAX);
        needed.min(cap).max(1)
    }
}

/// The column where the text starts: the mark is one cell, plus one cell of
/// breathing room.
///
/// Fixed, because the mark is **one** character and the mirror does not see
/// it — the mirror's `PREDISPLAY` is the shell's prompt, this is the
/// terminal's own mark.
///
/// **Only the alignment of the input row**; the context row starts at the
/// left edge ([`CONTEXT_COL`]).
///
/// It leaves the crate as `DOCK_TEXT_COL`: `bt-gpu`'s typing effects do not
/// leave a ghost moving with the window outside the text's columns (over the
/// mark). Not a second copy, a reader of the same constant.
pub const TEXT_COL: u16 = 2;

/// The mark separating the two sides of the context row; a space on each side.
const SEPARATOR: &str = " | ";

/// The column where the context row starts: the dock's **left edge**.
///
/// Aligned with the `>` mark, not with the input row's text. Had it started
/// from [`TEXT_COL`] — and it did — the context row would look indented for no
/// reason (the user: "why does this path display look indented"): the text's
/// alignment makes the room the mark opens read like an indent, yet the context
/// is not a continuation of the input row, it is the dock's **footer**.
const CONTEXT_COL: u16 = 0;

/// The dock-local row number of the context row in a dock with a single input
/// row — the number this module's tests use.
///
/// Not a constant in production: the context row is **below** the input
/// block, so its row is the very number of input rows
/// ([`crate::Cursor::input_rows`], [`render_with`]'s `input_rows`). `bt-gpu`
/// places it at the bottom of the band from the same number.
#[cfg(test)]
const CONTEXT_ROW: u16 = 1;

/// The mark at the start of a path shortened from the left.
const ELLIPSIS: char = '…';

/// The remote session's mark: in front of the host on the
/// context row, and a prefix in the title and the tab
/// ([`crate::shell::title_of`]). The **single copy** in this crate.
///
/// Not procedural, from the font: an ordinary cell of the context row. The
/// gate checking that no box appears in the small class is in `bt-atlas`
/// (`the_remote_mark_is_a_glyph_in_the_small_class`, by the Menlo name) and
/// since that crate cannot see this, it writes the character by hand; the two
/// copies are tied by `the_remote_mark_is_the_one_the_atlas_checks`.
pub(crate) const REMOTE_MARK: char = '⇄';

/// The part of the reconnect offer's placeholder after the host — a UI
/// string. A single text: ssh's 255 does not tell a broken
/// connection from a failed one, what tells them apart is ssh's own line
/// right above.
const RECONNECT_HINT: &str = "  Connection lost · ⏎ reconnect";

/// The gap between host and path in the remote form: two columns — no `|`
/// separator, because there is no branch and the two sides are two parts of
/// the same thing (the remote location).
const REMOTE_GAP: &str = "  ";

/// The non-ASCII characters of the transfer row (`↓` the download) — the
/// **vocabulary** of the text `bt-shell` formats. The text is born there, but
/// the gate checking that no box appears in the small class is in `bt-atlas`
/// and that crate cannot see either side; the copies are tied to this list
/// (`the_upload_row_is_the_one_the_atlas_checks`). The buttons' `⌘` is here
/// too: this crate writes the label but its glyph is again in the small class.
pub const UPLOAD_GLYPHS: [char; 8] = ['↑', '↓', '⌘', '✓', '—', '·', '…', '→'];

/// A program guide bar's non-ASCII characters: the separator and the
/// shortening mark drawn here, and the **vocabulary** of the hints
/// `bt-shell` writes (`⌃D exit`). `bt-atlas` checks a hand copy of this list
/// in Menlo's small class (`the_program_bar_has_no_box_in_the_small_class`);
/// the two are tied by `the_program_bar_is_the_one_the_atlas_checks`, and
/// `bt-shell`'s recognizer checks its strings against it.
pub const PROGRAM_GLYPHS: [char; 3] = ['⌃', '·', '…'];

/// The separator between a guide bar's title and its detail
/// (`Python 3.14.5 · venv`).
const PROGRAM_DETAIL: &str = " · ";

/// The load indicator's non-ASCII characters drawn from the **font**: the
/// critical mark and the alerts form's calm dot. The sparkline's `▁…█` are not
/// here — they are procedural in the small class too. `bt-atlas` checks a hand
/// copy of this list in Menlo's small class
/// (`the_stats_glyphs_have_no_box_in_the_small_class`); the two are tied by
/// `the_stats_glyphs_are_the_ones_the_atlas_checks`.
pub const STATS_GLYPHS: [char; 2] = [STATS_CRITICAL, STATS_CALM];

/// The mark in front of a number past its second threshold.
const STATS_CRITICAL: char = '▲';

/// The alerts form while nothing is past its threshold.
const STATS_CALM: char = '●';

/// The lowest sparkline block (U+2581); level `n` is this plus `n`.
const SPARK_BASE: u32 = 0x2581;

/// The most glyphs an edit can carry — a **design constant**.
///
/// The edit that comes alive is the typing itself: a held Backspace is one
/// glyph per frame, fast typing two or three. An edit exceeding the limit
/// does not read as typing (the frame path has paused for a while and input
/// has piled up) and falls to [`DockEdit::Reset`] — the wrong side is the
/// safe side, the text appears instantly. A fixed capacity keeps per-frame
/// allocation at zero.
pub const EDIT_MAX: usize = 8;

/// What changed in the dock's input row **in this frame** — the input of the
/// typing animations.
///
/// The decision is here: which glyph the user typed, which they deleted,
/// which change must not come alive (paste, history, completion). Time and
/// drawing are in `bt-gpu`.
///
/// **Position on two axes**: `(row, col)` are the vertical
/// window's row and the screen column, and the cells carry their own
/// `(row, col)` — in a wrapped input the letter filling the row crosses to
/// the next row with its effect. Text that moves by wrapping **behind** the
/// edit does not enter the edit: it is without animation at its new position
/// (in-flight ones in `bt-gpu` cannot find their static glyph and end).
/// **`shift` and `Shift` are in rows**: when the vertical window's top moves,
/// in-flight ones move with the text (the vertical counterpart of the shift
/// of the old single-row dock's horizontal window, retired when the input
/// started wrapping). The side comparing the top is [`crate::Session::dock`], because the
/// last **drawn** top is knowledge of the permission, not of the drawing
/// ([`with_shift`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DockEdit {
    /// Glyphs arrived. `(row, col)` is the **first** cell of the run (in the
    /// new window); the cells also go to the normal sink, which one gets drawn
    /// is the painting's decision.
    Arrive {
        row: u16,
        col: u16,
        cells: EditCells,
        shift: i32,
    },
    /// Glyphs left. `(row, col)` is the place of the deletion (the caret, in
    /// the new window) and the ghosts are at the **old** layout's positions,
    /// resolved with the old row's highlight.
    Erase {
        row: u16,
        col: u16,
        ghosts: EditCells,
        shift: i32,
    },
    /// The text did not change but the vertical window scrolled `by` rows
    /// (the caret changed row in an input past the ceiling, or the wheel):
    /// in-flight effects only move, none ends.
    Shift { by: i32 },
    /// A change that does not come alive: every in-flight effect must end.
    Reset,
}

/// The cells of [`DockEdit`]: a list of fixed capacity ([`EDIT_MAX`]).
///
/// Only cells **with a glyph**: a space and a spacer column carry no ink to
/// move, their grounds are drawn from the normal sink.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditCells {
    len: usize,
    cells: [Cell; EDIT_MAX],
}

impl EditCells {
    fn empty() -> Self {
        Self {
            len: 0,
            cells: [Cell::default(); EDIT_MAX],
        }
    }

    /// The cells, in column order.
    pub fn as_slice(&self) -> &[Cell] {
        self.cells.get(..self.len).unwrap_or(&[])
    }

    /// Silently drops if full; [`diff`] already bounds the capacity.
    fn push(&mut self, cell: Cell) {
        if let Some(slot) = self.cells.get_mut(self.len) {
            *slot = cell;
            self.len += 1;
        }
    }
}

/// A container from cells; a cell beyond [`EDIT_MAX`] silently drops.
///
/// So that the other side of the boundary (`bt-gpu`'s tests) can build edits:
/// in production only [`render`] fills the container.
impl FromIterator<Cell> for EditCells {
    fn from_iter<I: IntoIterator<Item = Cell>>(cells: I) -> Self {
        let mut out = Self::empty();
        for cell in cells {
            out.push(cell);
        }
        out
    }
}

/// What changed since the mirror's last drawn state — the raw form that
/// [`render`] will turn into a [`DockEdit`].
///
/// **Raw, because there is no position:** the screen column comes out of the
/// layout and [`render`] already computes it in its own walk; a second copy
/// is not born here. From the old side only what is no longer in the new
/// buffer is carried — the ghost's character and highlight — because the
/// caller ([`crate::Session::dock`]) overwrites the buffer with the new
/// mirror right after this computation.
// `Delete`'s ghost list is large by its code point capacity (~0.8 KB,
// [`GHOST_CHARS`]): the value is born once per frame and on the stack, while
// a `Box` would be both a per-frame allocation and the loss of `Copy`.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Change {
    /// A change that does not come alive.
    Reset,
    /// The mirror advanced but `BUFFER` is the same (the suggestion changed,
    /// the caret moved): in-flight effects stay in place.
    Same,
    /// The `start..end` characters of the new display were inserted.
    Insert { start: usize, end: usize },
    /// Glyphs were deleted from the old `BUFFER`; they are absent in the new.
    Delete { ghosts: Ghosts },
}

/// The deleted code points and the highlights on the old row.
///
/// **All code points**, not just glyphs: so that the ghosts' layout
/// can build the cluster the same as in the new layout — the ghost of `🇹🇷`
/// is one glyph, the VS16 of `❤️` carries its base character's emoji
/// presentation. A zero-width code point takes no cell in the layout, so with
/// clustering off the ghosts' position is the same as today's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Ghosts {
    len: usize,
    chars: [(char, HighlightStyle); GHOST_CHARS],
}

/// The code point capacity of [`Ghosts`]: each of [`EDIT_MAX`] glyphs can be
/// a cluster of a few code points (`👍🏽` two, `❤️` two, a family five). A
/// deletion exceeding it falls to [`Change::Reset`] — [`EDIT_MAX`]'s rule.
const GHOST_CHARS: usize = EDIT_MAX * 4;

/// The range of the cluster containing `index`, `[start, end)` — so that the
/// dock's selection ends, the ⇧←/⇧→ step and the four editing keys do
/// not split a cluster. A single code point with clustering off;
/// `None` if `index` is outside the text.
///
/// The single cluster rule ([`Walk`]): the same as the layout, the grid and
/// the freshness gate.
pub(crate) fn cluster_span(
    chars: impl IntoIterator<Item = char>,
    index: usize,
    cluster: bool,
) -> Option<(usize, usize)> {
    if !cluster {
        return chars.into_iter().nth(index).map(|_| (index, index + 1));
    }
    let mut found = None;
    Walk::new().run(chars, |span| {
        if (span.start..span.end).contains(&index) {
            found = Some((span.start, span.end));
        }
    });
    found
}

/// Is `index` a cluster boundary in `text`: if an edit starts or
/// ends inside a cluster (only the `🇷` of `🇹🇷` was deleted, a skin tone was
/// added to a `👍`) what comes alive would be half a glyph and the diff falls
/// to [`Change::Reset`] — the text appears instantly. The single cluster rule
/// ([`Walk`]); an index equal to the end is a boundary.
fn is_cluster_boundary(text: &str, index: usize) -> bool {
    let mut boundary = index == 0;
    let mut len = 0;
    Walk::new().run(text.chars(), |cluster| {
        boundary |= cluster.start == index;
        len = cluster.end;
    });
    boundary || index >= len
}

impl Ghosts {
    fn as_slice(&self) -> &[(char, HighlightStyle)] {
        self.chars.get(..self.len).unwrap_or(&[])
    }
}

/// The frame gate: if the mirror's stamp or status did not advance there is
/// no change and [`diff`] does not run at all.
///
/// With no new input there is no edit to bring alive, so the cost of an
/// ordinary content frame (a running command's output, the counter) is one
/// comparison. The status is asked too, because a transition that carries no
/// stamp (`Unavailable`, stamped zero) must end in-flight effects.
///
/// `old` must be the **last drawn** mirror: the caller's buffer
/// ([`crate::Session::dock`]'s `into`), before it is overwritten with the new
/// mirror.
pub(crate) fn change(old: &DockState, new: &DockState) -> Option<Change> {
    (old.answers != new.answers || old.status != new.status).then(|| diff(old, new))
}

/// The edit between two mirrors: only a single contiguous insertion or
/// deletion comes alive, and the glyph count cannot exceed the number of
/// inputs in between.
///
/// **Only `BUFFER`:** `POSTDISPLAY` (autosuggestions' suggestion) changes
/// wholesale on every key and is not what the user typed. **The direction
/// comes from `CURSOR`**: an insertion ends at the new caret, a deletion
/// (Backspace or forward delete) starts at the new caret. The hypothesis is
/// not derived, it is **tested** — if it fails, `Reset`.
///
/// **Glyph = a character of width above zero:** `❤️` is two code points but
/// one input and one glyph; the combiner takes no cell in [`render`] either.
///
/// **The base is `Live` or `Idle`:** `Idle` is an empty line — the base of
/// the first key after Enter. If `PREDISPLAY` or `PREBUFFER` changed the text
/// has shifted, `Reset`.
pub(crate) fn diff(old: &DockState, new: &DockState) -> Change {
    let old_buffer = match old.status {
        DockStatus::Live => {
            // If `PREBUFFER` changed, ZLE accepted or left a line: `BUFFER`'s
            // row shifted, the edit is not typing.
            if old.predisplay != new.predisplay || old.prebuffer != new.prebuffer {
                return Change::Reset;
            }
            old.buffer.as_str()
        }
        // Nothing on screen.
        DockStatus::Idle => "",
        _ => return Change::Reset,
    };
    if new.status != DockStatus::Live {
        return Change::Reset;
    }
    if old_buffer == new.buffer {
        return Change::Same;
    }
    let Some(inputs) = new.answers.checked_sub(old.answers) else {
        return Change::Reset;
    };
    let pre = new.predisplay.chars().count();
    let Some(caret) = new.cursor.checked_sub(pre) else {
        return Change::Reset;
    };
    let old_len = old_buffer.chars().count();
    let new_len = new.buffer.chars().count();
    // Glyph count: with clustering on, **clusters** — `🇹🇷` is one input
    // and one glyph, not two RIs. The two ends of the range are tested below
    // as cluster boundaries, so clustering the range alone is the same as
    // clustering it in its context.
    let glyphs = |run: &mut dyn Iterator<Item = char>| {
        if new.cluster {
            let mut count = 0;
            Walk::new().run(run, |cluster| count += usize::from(cluster.width > 0));
            count
        } else {
            run.filter(|&ch| column_width(ch) > 0).count()
        }
    };
    let bounds = |text: &str, at: &[usize]| {
        !new.cluster || at.iter().all(|&index| is_cluster_boundary(text, index))
    };
    let fits = |count: usize| count > 0 && count <= EDIT_MAX && count as u64 <= inputs;

    if new_len > old_len {
        // Insertion: `old == new[..start] ++ new[caret..]`.
        let Some(start) = caret.checked_sub(new_len - old_len) else {
            return Change::Reset;
        };
        let rest = new
            .buffer
            .chars()
            .take(start)
            .chain(new.buffer.chars().skip(caret));
        if !old_buffer.chars().eq(rest) {
            return Change::Reset;
        }
        // The two ends of the insertion must be cluster boundaries in the new
        // text, the joining point one in the old text: a skin tone added to a
        // `👍` is not a new glyph.
        if !bounds(&new.buffer, &[start, caret]) || !bounds(old_buffer, &[start]) {
            return Change::Reset;
        }
        if !fits(glyphs(
            &mut new.buffer.chars().skip(start).take(caret - start),
        )) {
            return Change::Reset;
        }
        Change::Insert {
            start: pre + start,
            end: pre + caret,
        }
    } else {
        // Deletion: `new == old[..caret] ++ old[caret + k..]`. On an `Idle`
        // base the old line is empty, so an equal-length replacement lands
        // here and `k = 0` eliminates it in the `fits` below.
        let count = old_len - new_len;
        // A deleted line break breaks the ghosts' layout: the ghost list
        // carries only glyphs and those behind the `\n` would line up on the
        // same row.
        if old_buffer
            .chars()
            .skip(caret)
            .take(count)
            .any(|ch| ch == '\n')
        {
            return Change::Reset;
        }
        let rest = old_buffer
            .chars()
            .take(caret)
            .chain(old_buffer.chars().skip(caret + count));
        if count == 0 || !new.buffer.chars().eq(rest) {
            return Change::Reset;
        }
        // The two ends of the deleted range must be cluster boundaries in the
        // old text, the joining point one in the new text: half of a `🇹🇷` is
        // not a deleted glyph, and deleting the `x` from `🇹x🇷` joins the two
        // halves into a flag.
        if !bounds(old_buffer, &[caret, caret + count]) || !bounds(&new.buffer, &[caret]) {
            return Change::Reset;
        }
        let mut ghosts = Ghosts {
            len: 0,
            chars: [(' ', HighlightStyle::default()); GHOST_CHARS],
        };
        let run = old_buffer.chars().enumerate().skip(caret).take(count);
        for (index, ch) in run {
            let Some(slot) = ghosts.chars.get_mut(ghosts.len) else {
                return Change::Reset;
            };
            // The highlight from the **old** display: this character is not in the new.
            *slot = (ch, style_at(old, pre + index));
            ghosts.len += 1;
        }
        if !fits(glyphs(&mut ghosts.as_slice().iter().map(|&(ch, _)| ch))) {
            return Change::Reset;
        }
        Change::Delete { ghosts }
    }
}

/// The **display** of the mirror, character by character: `PREDISPLAY ++
/// BUFFER ++ POSTDISPLAY` — the space of [`DockState::cursor`] and of
/// `region_highlight`.
fn display(state: &DockState) -> impl Iterator<Item = char> + '_ {
    state
        .predisplay
        .chars()
        .chain(state.buffer.chars())
        .chain(state.postdisplay.chars())
}

/// The dock's **stream**: `PREBUFFER ++ PREDISPLAY ++ BUFFER ++ POSTDISPLAY`.
/// `PREBUFFER` is the earlier lines ZLE accepted and always
/// ends with `\n`, so the editable lines start on a row below and at the same
/// indent on their own. The stream's index is ahead of the display's by
/// [`prebuffer_chars`] — `CURSOR` and `region_highlight` are read with that
/// shift.
fn stream(state: &DockState) -> impl Iterator<Item = char> + '_ {
    state.prebuffer.chars().chain(display(state))
}

/// `PREBUFFER`'s character count: the shift between the stream and display
/// spaces, and the part of the **selectable text** (`PREBUFFER ++ BUFFER`,
/// [`selectable`]) before `BUFFER`.
pub(crate) fn prebuffer_chars(state: &DockState) -> usize {
    state.prebuffer.chars().count()
}

/// The **selectable** text in the dock: `PREBUFFER ++ BUFFER` —
/// the space of [`DockPoint`] and of the selection range. `PREBUFFER` can be
/// selected and copied (copying the whole loop is what is expected) but ZLE
/// cannot edit it: a range touching it yields no edit command
/// (`Session::dock_edit_line`). With `PREBUFFER` empty it is `BUFFER` itself,
/// no allocation.
pub(crate) fn selectable(state: &DockState) -> std::borrow::Cow<'_, str> {
    if state.prebuffer.is_empty() {
        std::borrow::Cow::Borrowed(&state.buffer)
    } else {
        std::borrow::Cow::Owned(format!("{}{}", state.prebuffer, state.buffer))
    }
}

/// The **dock** parametrization of [`layout`]: both the
/// first row and the continuation rows start from the text's column
/// ([`TEXT_COL`], hanging indent), the width is the grid's. The columns are
/// directly **screen** columns — the two columns of the mark and the breathing
/// room are counted inside the row.
///
/// It has four consumers and no copy: drawing ([`render_with`]), the mouse hit
/// ([`hit`]), `frame()`'s row count ([`needed_rows`]) and the deletion's
/// ghosts (the same walk starting from the caret). Had the hit test written
/// its own walk, the day the wrap or wide-character rule diverged between the
/// two the mouse would shift by a column — now a row too — and the symptom
/// would be silent (the walk's counterpart of the column arithmetic's "single table"
/// reason).
pub(crate) fn dock_layout<T>(
    items: impl IntoIterator<Item = (char, T)>,
    caret: usize,
    cols: u16,
    cluster: bool,
    line: impl FnMut(VisualLine),
    place: impl FnMut(Placed<T>),
) -> LayoutEnd {
    let text = usize::from(TEXT_COL);
    layout_with(
        items,
        caret,
        usize::from(cols),
        text,
        text,
        cluster,
        line,
        place,
    )
}

/// The number of input rows the display wants in the dock — wrapped, without
/// the ceiling. A non-`Live` mirror and a width the text does not fit in give
/// one row.
///
/// [`crate::Session::frame`] asks this in the **same lock round** as the
/// suppression decision and hands it over the boundary clamped with the
/// budget ([`DockBudget::fit`]); the dock's drawing takes the same number as
/// an argument, without deriving it a second time.
///
/// **The suggestion (`POSTDISPLAY`) does not grow the band**: rows are
/// counted up to the place the text (`PREDISPLAY` and `BUFFER`) and the caret
/// occupy. The autosuggestions' suggestion changes wholesale on every key and
/// its length fluctuates (`git ` → `git status --short`); were it counted,
/// the band would grow and shrink on every key at the wrap limit, and the
/// whole grid would breathe while typing. The suggestion **stays** in the walk
/// (one layout, the same columns) and what fits in the text's rows is drawn,
/// the overflow clipped — as it was cut at the right edge in the old
/// single-row dock.
///
/// The walk is the very walk of the drawing, not a stream without the
/// suggestion: the suggestion can change the caret's row (the first wide
/// suggestion letter that does not fit at the end of a row drops the caret to
/// the next row) and two separate streams would reserve a row from the
/// vertical window.
pub(crate) fn needed_rows(state: &DockState, cols: u16) -> usize {
    if state.status != DockStatus::Live || cols <= TEXT_COL {
        return 1;
    }
    measure(state, cols).1
}

/// [`needed_rows`]'s walk: the caret's row and the number of rows the text
/// (suggestion excluded) occupies. The drawing reads the same measure too
/// ([`render_with`]: the vertical window's top and the "single row?" gate),
/// so the band, the window and the effects' gate look at the same number — in
/// separate measures a one-row input wrapped by the suggestion reset its
/// effects on every key.
fn measure(state: &DockState, cols: u16) -> (usize, usize) {
    let shift = prebuffer_chars(state);
    let text = shift + state.predisplay.chars().count() + state.buffer.chars().count();
    // The text's last row: every row starting from the text, plus the empty
    // row opened by a line break at the end of the text (not the row opened by
    // wrapping — that belongs to the suggestion). The line-break row opens
    // from `end + 1`, wrapping from `end`.
    let mut lines = 0;
    let mut last = 0;
    let mut previous_end = None;
    let end = dock_layout(
        stream(state).map(|ch| (ch, ())),
        shift + state.cursor,
        cols,
        state.cluster,
        |line| {
            let after_newline = previous_end.is_some_and(|end: usize| end + 1 == line.start);
            if lines == 0 || line.start < text || (line.start == text && after_newline) {
                last = lines;
            }
            previous_end = Some(line.end);
            lines += 1;
        },
        |_| {},
    );
    (end.caret_row, end.caret_row.max(last) + 1)
}

/// The vertical window's first row: the **smallest** shift that keeps the
/// caret's row visible. Stateless — the vertical twin of the old
/// single-row dock's horizontal `window_skip`: the window follows the caret, it keeps no
/// history of its own. `shown` is the number of input rows to draw; `0` is
/// also one row.
fn window_top(caret_row: usize, shown: usize) -> usize {
    caret_row.saturating_sub(shown.max(1) - 1)
}

/// The offer's placeholder: `⇄ {host}` in the mark's color, [`RECONNECT_HINT`]
/// in `dim`, after the caret when the line is empty.
///
/// The same layer as the suggestion and **the same walk** ([`dock_layout`]):
/// the stream is `PREDISPLAY ++ placeholder`, so the placeholder starts at the
/// caret's column. The line is **not wrapped**: only the row of its first
/// character is drawn, the tail that does not fit is clipped — and it does not
/// enter the row count ([`needed_rows`] does not see it), or the band would
/// grow for a hint.
fn render_reconnect(
    state: &DockState,
    offer: &Reconnect,
    theme: &Theme,
    cols: u16,
    window: &std::ops::Range<usize>,
    top: usize,
    sink: &mut impl FnMut(Cell),
) {
    let accent = theme.mark_linear(offer.mark);
    let dim = theme.dim_linear();
    // `PREDISPLAY` carries only the column (`None`): its cells were drawn in
    // the main walk.
    let items = state
        .predisplay
        .chars()
        .map(|ch| (ch, None))
        .chain(
            [REMOTE_MARK, ' ']
                .into_iter()
                .chain(offer.host.chars())
                .map(|ch| (ch, Some(accent))),
        )
        .chain(RECONNECT_HINT.chars().map(|ch| (ch, Some(dim))));
    let mut first_row = None;
    dock_layout(
        items,
        state.predisplay.chars().count(),
        cols,
        state.cluster,
        |_| {},
        |placed| {
            let Some(color) = placed.tag else {
                return;
            };
            let row = *first_row.get_or_insert(placed.row);
            if placed.row != row || !window.contains(&row) || !placed.fits(cols) {
                return;
            }
            // audit: `row - top < input_rows` and `col + width ≤ cols`; both
            // come from `u16`, they cannot overflow.
            let lead = Cell {
                row: (row - top) as u16,
                ..cell(
                    placed.ch,
                    placed.col as u16,
                    color,
                    HighlightStyle::default(),
                    theme,
                    placed.width == 2,
                    false,
                )
            };
            if lead.ch.is_some() {
                sink(lead);
            }
        },
    );
}

/// Which string a character of the stream belongs to: only `PREBUFFER ++
/// BUFFER` is selectable, `PREDISPLAY` and the
/// suggestion land on the two ends of `BUFFER` in the hit test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Pre,
    /// This character index of the selectable text ([`selectable`]) — `PREBUFFER`
    /// or `BUFFER`.
    Buffer(usize),
    Post,
}

/// A point in the dock's input block: the character index of the selectable
/// text ([`selectable`], `PREBUFFER ++ BUFFER`; `BUFFER` itself when
/// `PREBUFFER` is empty) and which half of the character — the
/// **text-space** counterpart of the grid's [`crate::SelectionPoint`]
/// (alacritty's `Anchor`: point + side). There is no row: wrapping is a
/// display decision, the index points at the same character whichever visual
/// row it lands on.
///
/// `index ≥ the text's length` is valid and means "the blank at the end of
/// the row", like the empty cells to the right of a row in the grid: the
/// column right next to the text is `len`, the one beyond `len + 1`… A click
/// on the suggestion or the blank lands there; the boundary (`Simple`) is
/// clamped to `len`, while the word (`Word`) takes the last word only in the
/// adjacent column, as in the grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DockPoint {
    pub(crate) index: usize,
    pub(crate) half: CellHalf,
}

/// Mouse hit test: in the dock's input block, half `half` of column `col` of
/// row `row` (inside the vertical window) → a point in the selectable text
/// ([`selectable`]). `None` if the mirror is not `Live` or the text does not
/// fit (no text to select).
///
/// **The walk is the very walk of [`render_with`]** ([`dock_layout`]): the
/// same stream, the same width, the same wrap — so the point lands where it
/// is drawn, wide characters and line breaks included. `top` comes from the
/// caller, because what is asked is the window **on screen** (the trace
/// [`crate::Session::dock`] leaves), not the window the live mirror would
/// compute today.
/// Kurallar:
/// - Within a `BUFFER` character the half is the **glyph's** half: a wide
///   character's left column is the left half, its right column (spacer) the
///   right half — glyph, not cell.
/// - `PREDISPLAY` lands on the start of `BUFFER`, the suggestion
///   (`POSTDISPLAY`) on the end of `BUFFER`: neither is selectable but where
///   the click goes is clear.
/// - `PREBUFFER` is the start of the selectable text: it lands on its
///   own character. The caret cannot move there — that decision is at the
///   editing gate, not here.
/// - The column to the left of a row (mark, breathing room, hanging indent)
///   is the left half of that row's first drawn character.
/// - The blank to the right of a row: if `BUFFER` continues on the row below
///   (wrapping) the right half of that row's last character — the rule of a
///   wrapped row in the grid; if the row is the end of `BUFFER`, the end of
///   `BUFFER` and beyond.
pub(crate) fn hit(
    state: &DockState,
    top: usize,
    cols: u16,
    row: u16,
    col: u16,
    half: CellHalf,
) -> Option<DockPoint> {
    if state.status != DockStatus::Live || cols <= TEXT_COL {
        return None;
    }
    let shift = prebuffer_chars(state);
    let buffer = state.buffer.chars().count();
    // The selectable text's length: the blank and the suggestion land beyond it.
    let len = shift + buffer;
    let pre = state.predisplay.chars().count();
    let part = |index: usize| match index.checked_sub(shift) {
        None => Part::Buffer(index),
        Some(shown) => match shown.checked_sub(pre) {
            None => Part::Pre,
            Some(offset) if offset < buffer => Part::Buffer(shift + offset),
            Some(_) => Part::Post,
        },
    };
    // The blank and the suggestion land beyond `len` by their distance to one
    // to the right of the drawn last column of the text (`PREDISPLAY` + `BUFFER`).
    let blank = |text_end: u16| DockPoint {
        index: len + usize::from(col.saturating_sub(text_end)),
        half: CellHalf::Left,
    };
    let at = |part: Part, half: CellHalf, text_end: u16| match part {
        Part::Pre => DockPoint {
            index: shift,
            half: CellHalf::Left,
        },
        Part::Buffer(index) => DockPoint { index, half },
        Part::Post => blank(text_end),
    };
    let target = top + usize::from(row);
    let mut lines = 0;
    let mut target_line = None;
    let mut found = None;
    let mut last = None;
    let mut text_end = TEXT_COL;
    dock_layout(
        stream(state).map(|ch| (ch, ())),
        shift + state.cursor,
        cols,
        state.cluster,
        |line| {
            if lines == target {
                target_line = Some(line);
            }
            lines += 1;
        },
        |placed| {
            if found.is_some() || placed.row != target || !placed.fits(cols) {
                return;
            }
            let part = part(placed.index);
            // audit: `fits` → `col + width ≤ cols` and `cols` is `u16`.
            let (start, end) = (placed.col as u16, (placed.col + placed.width) as u16);
            if col < start {
                // The left of the row: the left half of the first drawn
                // character. Columns are contiguous within a row, so this
                // can be reached only at the first character.
                found = Some(at(part, CellHalf::Left, text_end));
            } else if col < end {
                // The glyph's half in half-columns: `2 · width` half-columns
                // and the first `width` are the left half.
                let halves = usize::from(col - start) * 2 + usize::from(half == CellHalf::Right);
                let side = if halves < placed.width {
                    CellHalf::Left
                } else {
                    CellHalf::Right
                };
                found = Some(at(part, side, text_end));
            } else {
                if part != Part::Post {
                    text_end = end;
                }
                // With the cluster's code point count: the wrap question
                // looks **behind** the cluster, not at the head character.
                last = Some((part, placed.end - placed.index));
            }
        },
    );
    if found.is_some() {
        return found;
    }
    Some(match last {
        // `BUFFER` continues after this row (wrapping): the right half of the
        // last drawn one. If the last drawn is a cluster, what is asked is
        // the cluster's back (`span`): in a `BUFFER` ending with `🇹🇷` the `🇷`
        // must not count as continuing.
        Some((Part::Buffer(index), span)) if index + span < len => {
            at(Part::Buffer(index), CellHalf::Right, text_end)
        }
        Some((Part::Pre, _)) if buffer > 0 => at(Part::Buffer(shift), CellHalf::Left, text_end),
        // A row without a drawn character (the empty row a line break
        // opened, or beyond the window): where the row started.
        None => match target_line.map(|line| part(line.start)) {
            Some(Part::Buffer(index)) => DockPoint {
                index,
                half: CellHalf::Left,
            },
            Some(Part::Pre) => at(Part::Pre, CellHalf::Left, TEXT_COL),
            _ => blank(TEXT_COL),
        },
        _ => blank(text_end),
    })
}

/// The vertical window's first row and the input's ceiling-free row count for
/// a `Live` mirror drawn with `input_rows ≥ 1` rows at width `cols`: the
/// wheel's chosen top (clamped) or the caret-following one. The single
/// formula of [`render_with`] and of the dock link stamp's check
/// (`Session::dock`: a window that moved left the view's cells stale).
pub(crate) fn window_of(
    state: &DockState,
    cols: u16,
    input_rows: u16,
    scroll: Option<usize>,
) -> (usize, usize) {
    let (caret_row, rows) = measure(state, cols);
    let shown = usize::from(input_rows);
    let top = scroll.map_or_else(
        || window_top(caret_row, shown),
        |top| top.min(rows.saturating_sub(shown)),
    );
    (top, rows)
}

/// A stream index ([`stream`]) → its index in the **selectable** text
/// ([`selectable`]): `PREBUFFER` is in the same place, `BUFFER` is ahead by
/// `PREDISPLAY` in the stream; `PREDISPLAY` and the suggestion fall on no
/// index. `shift`, `pre` and `buffer` are the three parts' character counts.
///
/// One mapping, three readers: the selection's and the link's drawing
/// ([`render_with`]) and the link hit test ([`link_at`]).
fn selectable_index(index: usize, shift: usize, pre: usize, buffer: usize) -> Option<usize> {
    if index < shift {
        Some(index)
    } else {
        let offset = (index - shift).checked_sub(pre)?;
        (offset < buffer).then_some(shift + offset)
    }
}

/// The links under row `row`, column `col` of the dock's drawn vertical window:
/// the found candidates in the **selectable** text
/// ([`selectable`], their char ranges; a URL alone or the path candidates in
/// the order they are to be tried — [`crate::link::links_at`]) and each one's
/// cells as window-local spans — the rows of the input block inside the
/// window, screen columns (`dock_select`'s point space). Empty if the mirror
/// is not `Live`, the point is outside the window, on no character, on
/// `PREDISPLAY` or the suggestion, or on no link.
///
/// **Not [`hit`]**: that one lands the padding, the blank and the suggestion
/// on the nearest character (a click there must go somewhere), while a link is
/// lit only under the character it is drawn on. The walk is the same
/// ([`dock_layout`]), twice — once to find the character, once for the cells
/// of the window, from which every candidate's spans are cut (a wrapped link
/// is one link, one span per row; a wide character takes its spacer column).
pub(crate) fn link_at(
    state: &DockState,
    top: usize,
    shown: u16,
    cols: u16,
    row: u16,
    col: u16,
) -> Vec<(crate::link::Found, Vec<crate::session::LinkSpan>)> {
    if state.status != DockStatus::Live || cols <= TEXT_COL || row >= shown {
        return Vec::new();
    }
    let shift = prebuffer_chars(state);
    let pre = state.predisplay.chars().count();
    let buffer = state.buffer.chars().count();
    let caret = shift + state.cursor;
    let target = top + usize::from(row);
    let col = usize::from(col);
    let mut index = None;
    let mut seen = false;
    dock_layout(
        stream(state).map(|ch| (ch, ())),
        caret,
        cols,
        state.cluster,
        |_| {},
        |placed| {
            if seen || placed.row != target || !placed.fits(cols) {
                return;
            }
            if (placed.col..placed.col + placed.width).contains(&col) {
                seen = true;
                index = selectable_index(placed.index, shift, pre, buffer);
            }
        },
    );
    let Some(index) = index else {
        return Vec::new();
    };
    let text = selectable(state);
    let found = crate::link::links_at(&text, index);
    if found.is_empty() {
        return Vec::new();
    }
    // The window's cells: (selectable index, row, first, last), in reading order.
    let window = top..top + usize::from(shown);
    let mut placed_cells: Vec<(usize, i32, u16, u16)> = Vec::new();
    dock_layout(
        stream(state).map(|ch| (ch, ())),
        caret,
        cols,
        state.cluster,
        |_| {},
        |placed| {
            if !window.contains(&placed.row) || !placed.fits(cols) {
                return;
            }
            let Some(at) = selectable_index(placed.index, shift, pre, buffer) else {
                return;
            };
            // audit: `row - top < shown` and `fits` → `col + width ≤ cols`; all `u16`.
            let row = (placed.row - top) as i32;
            let (first, last) = (placed.col as u16, (placed.col + placed.width - 1) as u16);
            placed_cells.push((at, row, first, last));
        },
    );
    found
        .into_iter()
        .map(|found| {
            let mut spans: Vec<crate::session::LinkSpan> = Vec::new();
            for &(at, row, first, last) in &placed_cells {
                if !found.range.contains(&at) {
                    continue;
                }
                match spans.last_mut() {
                    Some(span) if span.row == row => span.last = last,
                    _ => spans.push(crate::session::LinkSpan { row, first, last }),
                }
            }
            (found, spans)
        })
        .collect()
}

/// The character range of the dock selection in `BUFFER`, `[start, end)` —
/// from the two ends and the step. `start == end` for an empty selection.
///
/// **The behavior's owner is alacritty** and this function is its copy in
/// text space: `Simple` draws a boundary from the halves of the
/// ends (`range_simple`), `Word` widens both ends to a word boundary
/// (`range_semantic` — [`WORD_SEPARATORS`], including the bracket matching and
/// the double-click-on-a-separator rule), `Line` the **logical line**:
/// the rows of the two ends, between `\n`s and together with their
/// wrapped visual rows — the grid's triple click also selects a wrapped
/// logical line, and so does the paragraph selection of macOS text fields.
/// The line break does not enter the range. On a single logical line (a
/// `BUFFER` without `\n`) the result is the whole `BUFFER`, i.e. the
/// single-line answer; the whole `BUFFER` stays ⌘A's job. In the grid the same string
/// gives the same range; its guard is `a_dock_word_matches_the_grid_word`
/// (`session.rs`).
///
/// It does **not run** on the frame path: the range is resolved once when the
/// selection changes and stored next to the selection
/// ([`crate::shell::DockSelection`]).
///
/// `cluster` is the mirror's clustering flag ([`DockState::cluster`]): the
/// ends land on cluster boundaries ([`boundary`]). The word and line steps
/// are already on boundaries — separators and `\n` are inside no cluster.
pub(crate) fn selection_range(
    buffer: &str,
    kind: SelectKind,
    anchor: DockPoint,
    head: DockPoint,
    cluster: bool,
) -> (usize, usize) {
    let chars: Vec<char> = buffer.chars().collect();
    let len = chars.len();
    match kind {
        SelectKind::Line => {
            let (low, high) = (
                anchor.index.min(head.index).min(len),
                anchor.index.max(head.index).min(len),
            );
            let start = (0..low)
                .rev()
                .find(|&index| chars[index] == '\n')
                .map_or(0, |index| index + 1);
            let end = (high..len)
                .find(|&index| chars[index] == '\n')
                .unwrap_or(len);
            (start, end.max(start))
        }
        SelectKind::Simple => {
            let (a, h) = (
                boundary(&chars, anchor, cluster),
                boundary(&chars, head, cluster),
            );
            (a.min(h), a.max(h))
        }
        SelectKind::Word => {
            let (start, end) = if anchor.index <= head.index {
                (anchor.index, head.index)
            } else {
                (head.index, anchor.index)
            };
            // Bracket only on a **point** selection (double click, no drag):
            // alacritty's rule; a dragged word selection does not look for a match.
            if start == end
                && let Some(matching) = bracket_match(&chars, start)
            {
                return (start.min(matching), start.max(matching) + 1);
            }
            let start = word_start(&chars, start).min(len);
            let end = (word_end(&chars, end) + 1).min(len);
            (start.min(end), end)
        }
    }
}

/// The point's boundary: the left half is the front of the character, the
/// right half its back — the zero-width ones behind it (combiners) stay with
/// the character, or an `é` would be separated from its accent.
///
/// **With clustering on the unit is the cluster**: [`hit`] gives
/// the cluster's head character and the right half lands on the **end** of
/// the cluster — a click on the right half of `🇹🇷` falls behind the flag,
/// not between the two RIs. The left half lands on the cluster's start too:
/// an end never stays inside a cluster by any path.
fn boundary(chars: &[char], point: DockPoint, cluster: bool) -> usize {
    let len = chars.len();
    if cluster && point.index < len {
        let (start, end) = cluster_span(chars.iter().copied(), point.index, true)
            .unwrap_or((point.index, point.index + 1));
        return if point.half == CellHalf::Left {
            start
        } else {
            end
        };
    }
    if point.half == CellHalf::Left || point.index >= len {
        return point.index.min(len);
    }
    let mut next = point.index + 1;
    while chars.get(next).is_some_and(|&ch| column_width(ch) == 0) {
        next += 1;
    }
    next
}

/// The character at `index`; beyond the end of `BUFFER` is a **blank**, i.e.
/// a separator — the counterpart of the empty cells to the right of a row in
/// the grid.
fn char_at(chars: &[char], index: usize) -> char {
    chars.get(index).copied().unwrap_or(' ')
}

fn is_separator(ch: char) -> bool {
    WORD_SEPARATORS.contains(ch)
}

/// The word's start: one to the right of the first separator to the **left**
/// of `point` (`semantic_search_left`). The point itself is not looked at —
/// this is why a double click on a separator takes the words on both sides.
fn word_start(chars: &[char], point: usize) -> usize {
    (0..point)
        .rev()
        .find(|&index| is_separator(char_at(chars, index)))
        .map_or(0, |index| index + 1)
}

/// The word's end (inclusive): one to the left of the first separator to the
/// **right** of `point` (`semantic_search_right`). Beyond the end of `BUFFER`
/// is a separator, so the search ends there at the latest.
fn word_end(chars: &[char], point: usize) -> usize {
    (point + 1..)
        .find(|&index| is_separator(char_at(chars, index)))
        .map_or(point, |index| index - 1)
}

/// The match of the bracket at `index` — alacritty's `bracket_search`: every
/// bracket of the same kind skips one match. `None` if it is not a bracket or
/// has no match.
fn bracket_match(chars: &[char], index: usize) -> Option<usize> {
    const PAIRS: [(char, char); 4] = [('(', ')'), ('[', ']'), ('{', '}'), ('<', '>')];
    let start = *chars.get(index)?;
    let (forward, end) = PAIRS.iter().find_map(|&(open, close)| {
        if open == start {
            Some((true, close))
        } else if close == start {
            Some((false, open))
        } else {
            None
        }
    })?;
    let mut depth = 0usize;
    let mut probe = |candidate: usize| {
        let ch = chars[candidate];
        if ch == end {
            if depth == 0 {
                return true;
            }
            depth -= 1;
        } else if ch == start {
            depth += 1;
        }
        false
    };
    if forward {
        (index + 1..chars.len()).find(|&candidate| probe(candidate))
    } else {
        (0..index).rev().find(|&candidate| probe(candidate))
    }
}

/// Turns the mirror into this frame's dock cells.
///
/// **The ground must be opaque** and this is not a taste but a structural
/// condition: during a slide the grid's offset is larger than its target, so
/// the bottom row overflows onto the dock. Since the dock is drawn **after**
/// the grid and its ground is opaque, the overflowing pixels are not visible;
/// a translucent ground would flicker in the slide frames.
///
/// `cols` is the grid's width: the dock uses the same columns and text that
/// overflows is **wrapped** (the old left windowing retired) —
/// continuation rows start from the text's column, a wide glyph is **not
/// split** at the end of a row, if it does not fit it moves to the next row
/// ([`dock_layout`]). If the wrapped input exceeds `input_rows`
/// ([`crate::Cursor::input_rows`], clamped to the ceiling) the **vertical
/// window** keeps the caret's row visible ([`window_top`]); rows outside the
/// window never reach the sink — or they would land on top of the context
/// row.
///
/// `context_cols` is the context row's budget and a separate number, because
/// that row is drawn in a **small point size**: more letters fit in the same
/// width. The drawing side supplies the number (`bt-gpu`), this crate sees no
/// pixels — the same contract as `cols` itself. Passed equal, the row behaves
/// as it does today, i.e. the value is a **budget**, not a point-size
/// decision.
///
/// `change` is what changed since the last drawn mirror ([`change`]'s
/// answer); it is turned into a [`DockEdit`] **with this layout's** columns
/// and pushed to `edits` — at most once per frame. Coming alive applies only
/// in the arm where the text is drawn and the caret is in the dock: if the
/// line is in the grid the effect has no subject, and every arm that does not
/// come alive ends the in-flight ones (`Reset`). The position is **(row,
/// column)** and the vertical window's row; the shift of the
/// window's top (`shift`) is not here but in the caller ([`with_shift`]).
///
/// `selection` is the character range of the dock selection in `BUFFER`
/// ([`crate::shell::DockSelection::range`]); it is turned into `runs` as **one
/// run per visual row**, with the vertical window's rows and the screen
/// columns — the same as the grid's [`crate::SelectionRun`] and by the same
/// rule: the run spans from the row's first drawable selected
/// cell to the last, the gaps between bridged. `runs` is emptied first.
///
/// The second half of the return is the vertical window's first row
/// ([`window_top`]) and the input's ceiling-free row count: the trace for the
/// hit test and the wheel ([`crate::Session::dock`] writes it).
///
/// **`input_rows == 0` means no input row** (remote session):
/// only the context row is printed, on row 0, there is no prompt mark and no
/// caret, the trace is `(0, 0)` — there is no input block to click or scroll.
/// **`None` means no band at all** (a program reading the keyboard itself,
/// [`crate::Cursor::band_hidden`]): nothing is printed — the context row and
/// its buttons neither, so nothing on a row the drawing does not show can be
/// hit — and the trace is `(0, 0)`. The surface's colours still come back:
/// the band slides to zero height with them.
///
/// `scroll` is the window top the user chose with the wheel
/// ([`crate::shell::ShellLog::dock_scroll`]); `None` → the window follows the
/// caret. The chosen top is clamped to the row count; if the caret falls
/// outside the window it is not drawn (a text field scrolled away from the
/// caret) — typing or moving the caret brings the following back.
///
/// **The walk runs twice** and both are the same function
/// ([`dock_layout`]): the window's top depends on the caret's row and that
/// row is known only at the end of the walk ([`measure`]), while the cells
/// cannot be printed without knowing the top. Not a second **producer** of
/// the number, two readings of the same walk.
///
/// **A known limit, one frame:** `input_rows` comes from `frame()`'s mirror,
/// the drawing from this call's mirror; if a key that crosses the wrap limit
/// has its mirror drop between the two lock rounds, that frame draws the
/// window a row off (the rule of an input past the ceiling) and the next
/// frame corrects it — a sibling of `line-finish`'s known limit in the same
/// gap (`Session::frame`). The remote session's two edges are in the same
/// class: `frame()`'s remote decision and this call's context come from
/// separate lock rounds and a `set_remote` or `D` falling in between can make
/// the input row count and the context row's form (and the top line's color)
/// diverge for one frame; the next frame corrects it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_with(
    state: &DockState,
    context: &DockContext,
    shell: Option<ShellState>,
    theme: &Theme,
    cols: DockCols,
    band: Option<u16>,
    scroll: Option<usize>,
    owned: bool,
    selection: Option<(usize, usize)>,
    link: Option<(std::ops::Range<usize>, UnderlineStyle)>,
    change: Option<&Change>,
    runs: &mut Vec<SelectionRun>,
    clusters: &mut Clusters,
    mut sink: impl FnMut(Cell),
    mut edits: impl FnMut(DockEdit),
) -> (Dock, usize, usize) {
    runs.clear();
    let input_rows = band.unwrap_or(0);
    let mut surface = Dock {
        ground: theme.background_linear(),
        // While an upload runs the line is in the color of the queue's host
        // (also after ssh closes, as long as the result row is shown).
        // While progress runs the filled part is `info` (the mark's meaning is in the empty track).
        edge: match (&context.transfer, &context.remote, &context.program) {
            (Some(transfer), _, _) if transfer.progress.is_some() => theme.info_linear(),
            (Some(transfer), _, _) => theme.mark_linear(transfer.mark),
            (None, Some(_), _) => theme.mark_linear(context.remote_mark),
            // A program's guide bar, in its tone: the band is the program's.
            (None, None, Some(bar)) => program_color(bar.tone, theme),
            (None, None, None) => theme.separator_linear(),
        },
        separator: theme.separator_linear(),
        caret: None,
        caret_text: theme.background_linear(),
        // With no input row (remote session) there is no mark either:
        // the mark is the start of the input row and must not sit on the
        // context row. Before all the early returns, so no arm brings it back.
        sigil: (input_rows > 0).then(|| sigil_color(shell, theme)),
        progress: context
            .transfer
            .as_ref()
            .and_then(|transfer| transfer.progress),
        track: match &context.transfer {
            Some(transfer) if transfer.mark != HostMark::None => theme.mark_linear(transfer.mark),
            _ => theme.separator_linear(),
        },
        buttons: [None; 2],
    };
    // **No band**: the surface without a cell. Before the context row, so its
    // buttons are not laid out either; the mark is already off (no input row).
    if band.is_none() {
        settle(change, &mut edits);
        return (surface, 0, 0);
    }
    if cols.grid == 0 {
        settle(change, &mut edits);
        // With no input row the trace is zero rows (the rule of the
        // `input_rows == 0` arm below): the wheel must find no window to scroll.
        return (surface, 0, usize::from(input_rows.min(1)));
    }
    // **The caret's owner is not asked here, the answer comes ready**
    // (`owned`). This used to call [`caret_home`] a second time and that call
    // did **not know** `Session::frame`'s three preconditions (does the
    // window have a dock, are we on the alternate screen, is the mirror
    // fresh): on a stale mirror the grid showed the cursor visible while the
    // dock also gave its caret, the drawing side chose the dock and the fresh
    // line the user typed was left without a caret. One predicate, one
    // computation — and the same change also closed the race of deriving it
    // from two separate lock rounds.
    // The mark **does not go through the sink**: it is not a cell but a field
    // of the surface ([`Dock::sigil`]). Were it a cell, the user's font's `>`
    // would be drawn.
    //
    // The context row **before the mirror's status**: the directory and branch
    // are correct even when ZLE is not editing the line and the user looks at
    // them while a command runs. Under the `Live` gate below it would vanish
    // on every command.
    surface.buttons = render_context(context, theme, cols.context, input_rows, &mut sink);

    // **Zero input rows**: the band is only the context row.
    // No caret (`owned` is already `false` — `frame()`'s fourth
    // precondition), the row count is zero too: there is no window the wheel
    // could scroll ([`crate::Session::dock_scroll`], `rows <= shown`).
    if input_rows == 0 {
        settle(change, &mut edits);
        return (surface, 0, 0);
    }
    if cols.grid <= TEXT_COL {
        settle(change, &mut edits);
        return (surface, 0, 1);
    }
    // A non-`Live` mirror draws no text and both are the right answer: in
    // `Idle` ZLE is not editing the line, in `Unavailable` there is a line we
    // cannot show and its fields are already empty (`DockState::reset`). The
    // place that consumes the distinction is the suppression decision, not
    // here.
    //
    // **The caret can still be drawn**: a row without text does not mean a
    // row without a caret. At startup and between two commands the mirror is
    // `Idle` and the dock empty, but that is where the user will start
    // typing — keeping the cursor in the grid in those windows would make
    // the caret jump when the prompt arrives.
    if state.status != DockStatus::Live {
        if owned {
            surface.caret = Some(DockCaret {
                col: TEXT_COL,
                row: 0,
            });
        }
        settle(change, &mut edits);
        return (surface, 0, 1);
    }
    let fixed = theme.foreground_linear();
    // The suggestion is dim: "text not yet typed" and what SGR 2 asks are the
    // same thing.
    let suggestion = theme.dim_linear();
    // **`PREBUFFER` at the start of the stream** ([`stream`]):
    // the earlier lines ZLE accepted are above the editable rows, in the same
    // color and at the same indent — the dock is an editor, a `for` loop is
    // one piece of text. The display's indices (`CURSOR`,
    // `region_highlight`, effects) are ahead in the stream by `shift`.
    let shift = prebuffer_chars(state);
    let pre = state.predisplay.chars().count();
    let buffer = state.buffer.chars().count();
    let text = shift + pre + buffer;
    let stream = stream(state)
        .enumerate()
        .map(|(index, ch)| (ch, if index < text { fixed } else { suggestion }));

    // **The caret's place from the layout**: `CURSOR` is a character index
    // (ZLE's unit), while the display's unit is (row, column) — a wide
    // character's index advances by one, its columns by two, wrapping the
    // row. A caret after a completely full row is at the start of the next
    // row ([`layout`]'s caret rule): the same as zsh's grid, i.e. a row typed
    // at full width grows the dock by a row too.
    // The arm above took zero: here `input_rows ≥ 1`.
    let shown = usize::from(input_rows);
    let (top, rows) = window_of(state, cols.grid, input_rows, scroll);
    let window = top..top + shown;
    // If the first row is outside the window so is the mark: the prompt's place is not on screen.
    if top > 0 {
        surface.sigil = None;
    }
    let arriving = match change {
        Some(&Change::Insert { start, end }) if owned => shift + start..shift + end,
        _ => 0..0,
    };
    let mut arrive_at = None;
    let mut arrived = EditCells::empty();
    // The selection is in the selectable text's ([`selectable`]) space; in the
    // stream `PREBUFFER` is in the same place, `BUFFER` is ahead by
    // `PREDISPLAY`. The stream index is turned into a selectable index,
    // `PREDISPLAY` and the suggestion fall on no index.
    let selected = selection.map_or(0..0, |(start, end)| start..end);
    let selectable_at = |index: usize| selectable_index(index, shift, pre, buffer);
    // The ⌘-hovered link is in the same space: its line goes on
    // the cells whose selectable index is inside the range.
    let linked = |index: usize| {
        link.as_ref().and_then(|(range, style)| {
            selectable_at(index)
                .is_some_and(|at| range.contains(&at))
                .then_some(*style)
        })
    };
    let mut run: Option<SelectionRun> = None;

    let end = dock_layout(
        stream,
        shift + state.cursor,
        cols.grid,
        state.cluster,
        |_| {},
        |placed| {
            // A row outside the window (vertical window) and a degenerate
            // character that does not fit (a wide glyph in a one-column
            // window, [`layout`]'s "overflows" rule) are not drawn: one would
            // fall outside the context row, the other outside the grid.
            if !window.contains(&placed.row) || !placed.fits(cols.grid) {
                return;
            }
            let Placed {
                index,
                end,
                ch,
                width,
                row,
                col,
                tag: base,
            } = placed;
            // audit: `row - top < shown ≤ input_rows` and `col + width ≤ cols`;
            // both come from `u16`, they cannot overflow.
            let (row, col) = ((row - top) as u16, col as u16);
            // `PREBUFFER` has no highlight: `region_highlight` belongs only to the display.
            let style = index
                .checked_sub(shift)
                .map_or_else(HighlightStyle::default, |shown| style_at(state, shown));
            let is_selected = selectable_at(index).is_some_and(|at| selected.contains(&at));
            let mut lead = Cell {
                row,
                // The cluster's text from the stream: clusters are rare and
                // the stream short, so the walk has no second buffer
                // ([`placed_cluster`]).
                cluster: placed_cluster(end - index, width, clusters, || {
                    self::stream(state).skip(index).take(end - index)
                }),
                ..cell(ch, col, base, style, theme, width == 2, is_selected)
            };
            // **A selection creates no content** (the grid's rule):
            // the run extends from the first drawable selected cell to the
            // last. The criterion is the drawability of the state without the
            // selection — a selected cell's ground drops out, and looking at
            // it would drop a cell consisting only of ground from the run.
            if is_selected
                && (lead.ch.is_some()
                    || style.bg.is_some()
                    || style.standout
                    || lead.underline != UnderlineStyle::None)
            {
                // In a wide character the spacer's column too: both halves are highlighted.
                let last = col + width as u16 - 1;
                match &mut run {
                    Some(open) if open.row == row => open.last = last,
                    _ => {
                        // The walk is in row order, so the first selected cell
                        // of a new row closes the previous one's run.
                        let done = run.replace(SelectionRun {
                            row,
                            first: col,
                            last,
                        });
                        runs.extend(done);
                    }
                }
            }
            // **The link's line after the selection's drawability**:
            // the hover creates no selection content — the grid's
            // rule that keeps the hover out of `ruled`. The single override
            // helper, the grid's and the band's.
            crate::session::underline_link(&mut lead, linked(index));
            // Arriving glyphs from the **same** loop and the same cell: the
            // wrap, wide-character and edge rules are not written a second time.
            if arriving.contains(&index) {
                arrive_at.get_or_insert((row, col));
                if lead.ch.is_some() {
                    arrived.push(lead);
                }
            }
            // The dock counterpart of `frame()`'s skip gate: a cell with no
            // ink, no ground and no line never reaches the sink. On a row
            // without highlights most blanks are eliminated here and, since the
            // `cells=` token has no dock sibling, this saving's only consumer.
            if lead.ch.is_some() || lead.bg.is_some() || lead.underline != UnderlineStyle::None {
                sink(lead);
            }
            if width == 2 {
                // **Ground for the spacer column.** It has no glyph (the head
                // cell's `wide` draws it) but it has a ground and rules: the
                // same as the grid's `WIDE_CHAR_SPACER` arm and its reason is
                // written in `frame()` — "eliminating the cell altogether
                // would leave its right half uncolored". Without this
                // `region_highlight`'s ground would end at the right half of a
                // wide character.
                let spacer = Cell {
                    col: col + 1,
                    ch: None,
                    wide: false,
                    ..lead
                };
                if spacer.bg.is_some() || spacer.underline != UnderlineStyle::None {
                    sink(spacer);
                }
            }
        },
    );
    runs.extend(run);
    // **The reconnect offer's placeholder**: when the line is
    // empty, after the caret — the suggestion's layer.
    // The gate is the same as ⏎'s (`Session::reconnect`): the visible hint
    // must work. Freshness cannot be asked here (the generation is not in
    // this module) but need not be — every input deletes the offer, so while
    // an offer exists the mirror is the last input's answer.
    if let Some(offer) = &context.reconnect
        && owned
        && state.insert_keymap
        && state.buffer.is_empty()
        && state.prebuffer.is_empty()
        && state.postdisplay.is_empty()
    {
        render_reconnect(state, offer, theme, cols.grid, &window, top, &mut sink);
    }

    // audit: the caret's column is `< cols` ([`layout`]'s caret rule: where
    // the next one-column character fits) and its row is inside `window` —
    // if it is outside in a wheel-scrolled window it is not drawn below.
    let caret = DockCaret {
        col: end.caret_col as u16,
        row: end.caret_row.saturating_sub(top) as u16,
    };
    let caret_shown = window.contains(&end.caret_row);
    match change {
        None | Some(Change::Same) => {}
        Some(Change::Insert { .. }) if owned => {
            // If none of the run's cells was drawn (a degenerate glyph that
            // does not fit or outside the window) the position is still the caret's place.
            let (row, col) = arrive_at.unwrap_or((caret.row, caret.col));
            edits(DockEdit::Arrive {
                row,
                col,
                cells: arrived,
                shift: 0,
            });
        }
        Some(Change::Delete { ghosts }) if owned => {
            // The ghosts start from the caret, at the **old layout's**
            // positions: the deletion (Backspace and forward delete alike)
            // starts at the new caret and the prefix up to the caret is the
            // same in both mirrors, so the old layout from that point is the
            // same walk starting from the caret — the ghost of a deletion
            // spanning rows lands at the start of the next row, a wide glyph is
            // not split. A ghost falling outside the window is not drawn (it
            // would overlap the context row).
            let mut cells = EditCells::empty();
            layout_with(
                ghosts.as_slice().iter().copied(),
                usize::MAX,
                usize::from(cols.grid),
                end.caret_col,
                usize::from(TEXT_COL),
                // The ghost list carries **all** the deleted code points
                // ([`Ghosts`]), so the cluster is built the same as in the new
                // layout: the ghost of `🇹🇷` is one glyph, not half a flag.
                state.cluster,
                |_| {},
                |placed| {
                    let row = end.caret_row + placed.row;
                    if !window.contains(&row) || !placed.fits(cols.grid) {
                        return;
                    }
                    // audit: `row - top < shown ≤ input_rows` and `fits` →
                    // `col < cols`; both from `u16`.
                    let ghost = Cell {
                        row: (row - top) as u16,
                        cluster: placed_cluster(
                            placed.end - placed.index,
                            placed.width,
                            clusters,
                            || {
                                ghosts
                                    .as_slice()
                                    .iter()
                                    .skip(placed.index)
                                    .take(placed.end - placed.index)
                                    .map(|&(ch, _)| ch)
                            },
                        ),
                        ..cell(
                            placed.ch,
                            placed.col as u16,
                            fixed,
                            placed.tag,
                            theme,
                            placed.width == 2,
                            false,
                        )
                    };
                    if ghost.ch.is_some() {
                        cells.push(ghost);
                    }
                },
            );
            edits(DockEdit::Erase {
                row: caret.row,
                col: caret.col,
                ghosts: cells,
                shift: 0,
            });
        }
        Some(_) => edits(DockEdit::Reset),
    }

    (
        Dock {
            caret: (owned && caret_shown).then_some(caret),
            ..surface
        },
        top,
        rows,
    )
}

/// Adds the vertical window's shift to the frame's edit: `by` rows (`last
/// drawn top − new top`; when the window goes down the in-flight ones shift
/// up, negative).
///
/// If there is an edit the shift goes in its field (`bt-gpu` takes a single
/// edit per frame; a separate `Shift` would overwrite it), otherwise a lone
/// [`DockEdit::Shift`] — the caret changed row in an input past the ceiling
/// or the wheel scrolled the window but the text is the same. `Reset` already
/// ends everything. A frame with no shift passes the edit through as is.
pub(crate) fn with_shift(edit: Option<DockEdit>, by: i32) -> Option<DockEdit> {
    if by == 0 {
        return edit;
    }
    Some(match edit {
        None => DockEdit::Shift { by },
        Some(DockEdit::Arrive {
            row, col, cells, ..
        }) => DockEdit::Arrive {
            row,
            col,
            cells,
            shift: by,
        },
        Some(DockEdit::Erase {
            row, col, ghosts, ..
        }) => DockEdit::Erase {
            row,
            col,
            ghosts,
            shift: by,
        },
        Some(DockEdit::Shift { by: before }) => DockEdit::Shift {
            by: before.saturating_add(by),
        },
        Some(DockEdit::Reset) => DockEdit::Reset,
    })
}

/// [`render_with`] without selection and with a single input row — the call
/// of this module's tests; the trace (`top`) and the runs are discarded.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn render(
    state: &DockState,
    context: &DockContext,
    shell: Option<ShellState>,
    theme: &Theme,
    cols: DockCols,
    owned: bool,
    change: Option<&Change>,
    sink: impl FnMut(Cell),
    edits: impl FnMut(DockEdit),
) -> Dock {
    render_with(
        state,
        context,
        shell,
        theme,
        cols,
        Some(CONTEXT_ROW),
        None,
        owned,
        None,
        None,
        change,
        &mut Vec::new(),
        &mut Clusters::default(),
        sink,
        edits,
    )
    .0
}

/// The edit of the arm where text is not drawn: every arm where the mirror
/// advanced ends the in-flight ones, a mirror whose `BUFFER` did not change
/// prints nothing.
fn settle(change: Option<&Change>, edits: &mut impl FnMut(DockEdit)) {
    if matches!(change, Some(change) if *change != Change::Same) {
        edits(DockEdit::Reset);
    }
}

/// The dock's **bottom** row: `{full path} | {branch}`, bottom left and dim;
/// in a remote session `⇄ {host}  {remote path}` ([`render_remote_context`]),
/// and while a recognized program reads the keyboard its guide bar
/// ([`render_program`]).
///
/// **On overflow the path is shortened from the left, the branch never.**
/// Two separate reasons: the path's information is in its tail (which folder
/// you are in), so cutting from the front would throw away the most
/// informative half; while **no** half of the branch can be thrown away — a
/// shortened branch name (`mai…`) can make the user think they are on another
/// branch, and that is the "silently wrong" class this repository forbids.
///
/// The shortening is in **character** units and does not lean on component
/// boundaries: leaning on a boundary would leave some of the available columns
/// empty, and its gain would be taste, its loss information. **This row stays
/// in character units** and its reason differs from the input row's: the
/// context row is drawn in the **small size class**, the column pitch is the
/// small face's advance and the wide path is closed there (the precedent of the
/// procedural characters). So a path with CJK still shifts columns here — a
/// known limit, guarded by `the_context_line_keeps_character_columns`.
///
/// **The separator is drawn if both sides are filled.** A dangling `|` in a
/// directory that is not a repo would say "the branch could not be read";
/// there is no branch to read.
///
/// The return is the upload row's buttons ([`Dock::buttons`]); none in other
/// forms.
fn render_context(
    context: &DockContext,
    theme: &Theme,
    cols: u16,
    row: u16,
    sink: &mut impl FnMut(Cell),
) -> [Option<DockButton>; 2] {
    let available = usize::from(cols.saturating_sub(CONTEXT_COL));
    if available == 0 {
        return [None; 2];
    }
    // The upload row **before** the remote form: it carries its own host and
    // must show its result after ssh has closed too.
    if let Some(transfer) = &context.transfer {
        return render_transfer(transfer, theme, available, row, sink);
    }
    if let Some(host) = context.remote_host() {
        let color = theme.mark_linear(context.remote_mark);
        return render_remote_context(context, host, color, theme, available, row, sink);
    }
    // A recognized program's guide bar after both: the upload's result and
    // the remote session say more about where the keys go.
    if let Some(bar) = &context.program {
        render_program(bar, theme, available, row, sink);
        return [None; 2];
    }
    let branch_chars = context.branch.chars().count();
    // The budget is set aside **for the branch first**; the path gets the
    // rest. The separator is counted on the path's side, because if the path
    // drops the separator drops too.
    //
    // **A branch that does not fit is not clipped, it drops.** This is the
    // degenerate-width counterpart of the branch's "no half can be thrown
    // away" rule (doc above): showing the branch `release/2.1` as `release`
    // in twelve columns would tell the user they are on **a branch that does
    // not exist**, and putting a marker (`rele…`) would not fix that either —
    // a shortened branch name can be misread anyway. Not showing it at all is
    // a loss of information but not wrong information; a window that narrow
    // is unreadable anyway.
    let shows_branch = branch_chars > 0 && branch_chars <= available;
    let path_budget = if shows_branch {
        available
            .saturating_sub(branch_chars)
            .saturating_sub(SEPARATOR.chars().count())
    } else {
        // If the branch is not drawn the whole width is the path's: its
        // shortening is **marked** (`…`), so it cannot be misread.
        available
    };

    let normal = theme.dim_linear();
    let quiet = theme.quiet_linear();
    let (shows_path, path) = path_cells(&context.cwd, path_budget, normal, quiet);
    let separator = if shows_path && shows_branch {
        SEPARATOR
    } else {
        ""
    };
    let line = path
        // The separator is a division mark, not content: in the quietest tone.
        .chain(separator.chars().map(|ch| (ch, quiet)))
        .chain(
            shows_branch
                .then(|| context.branch.chars().map(|ch| (ch, normal)))
                .into_iter()
                .flatten(),
        );
    emit_context(line, available, row, sink);
    [None; 2]
}

/// The context row's **remote** form: `⇄ {host}` in the mark's
/// color (`color`, the theme's `info` when unmarked), two
/// spaces, then the remote path in the two tiers of the local path; no branch
/// and no `|` — the branch belongs to the local repo, the remote side's is
/// unknown. The remote host's load indicator, if any, is right-aligned.
///
/// **The budget goes to `⇄ host` first.** The host is **not shortened**, for
/// the same reason as the branch rule: a shortened host name (`prod-we…`) can
/// be read as another machine. If it does not fit only `⇄` remains — saying
/// we are remote is still correct information. The path and the indicator
/// share the rest by [`stats_layout`]'s ladder; the path is shortened from
/// the left; if the remote shell prints no OSC 7 there is no path at all.
///
/// The return is the Sign In… button, in the upload buttons'
/// place and drawing: label in the foreground, fill and border in the mark's
/// color.
fn render_remote_context(
    context: &DockContext,
    host: &str,
    info: LinearRgba,
    theme: &Theme,
    available: usize,
    row: u16,
    sink: &mut impl FnMut(Cell),
) -> [Option<DockButton>; 2] {
    let (remote_cwd, stats) = (&context.remote_cwd, context.stats.as_ref());
    let mark = std::iter::once((REMOTE_MARK, info));
    let layout = stats_layout(
        host,
        remote_cwd,
        stats,
        context.sign_in.is_some(),
        available,
    );
    if !layout.head {
        emit_context(mark, available, row, sink);
        return [None; 2];
    }
    let (_, path) = path_cells(
        remote_cwd,
        layout.path_budget,
        theme.dim_linear(),
        theme.quiet_linear(),
    );
    let line = mark
        .chain(std::iter::once((' ', info)))
        .chain(host.chars().map(|ch| (ch, info)))
        .chain(REMOTE_GAP.chars().map(|ch| (ch, info)))
        .chain(path);
    emit_context(line, available, row, sink);
    if let (Some(stats), Some(span)) = (stats, layout.gauge) {
        let gauge = gauge(stats, span.step);
        let cells = gauge.cells().iter().map(|&(ch, tone)| {
            let color = match tone {
                Tone::Quiet | Tone::Level(StatsLevel::Normal) => theme.dim_linear(),
                Tone::Level(StatsLevel::Warning) => theme.warning_linear(),
                Tone::Level(StatsLevel::Critical) => theme.error_linear(),
                Tone::Calm => theme.success_linear(),
            };
            (ch, color)
        });
        emit_context_at(span.start, cells, available, row, sink);
    }
    let (Some(sign_in), Some((start, end))) = (context.sign_in, layout.sign_in) else {
        return [None; 2];
    };
    let label = ButtonLabel::SignIn
        .chars()
        .map(|ch| (ch, theme.foreground_linear()));
    emit_context_at(start + BUTTON_PAD, label, available, row, sink);
    [
        None,
        Some(DockButton {
            // audit: `end ≤ available ≤ cols` and `cols` is `u16`.
            start: CONTEXT_COL + start as u16,
            end: CONTEXT_COL + end as u16,
            color: info,
            state: if sign_in.hover {
                ButtonState::Hover
            } else {
                ButtonState::Idle
            },
        }),
    ]
}

/// A guide bar's tone as a color: the title and the top hairline.
fn program_color(tone: ProgramTone, theme: &Theme) -> LinearRgba {
    match tone {
        ProgramTone::Info => theme.info_linear(),
    }
}

/// Which parts of a guide bar show on `available` columns
/// ([`program_layout`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ProgramLayout {
    /// The title shows; when it does not, nothing does.
    title: bool,
    /// The separator and the detail show after the title.
    detail: bool,
    /// The path's budget in characters ([`path_cells`]); `0` = no path.
    path_budget: usize,
    /// The hint's context-local start column; `None` = dropped.
    hint: Option<usize>,
}

/// A guide bar's layout: `{title} · {detail}  {path}` from the left, the
/// hint right-aligned.
///
/// **What drops first is what the user needs least**: the hint (how to
/// leave — the program's own prompt usually says it too), then the path is
/// shortened from the left (its tail names the interpreter) and dropped,
/// then the detail; **the title is never shortened** — a cut version
/// (`Python 3.1…`) reads as another one, the remote host's rule. A title
/// that does not fit leaves the row empty; the hairline still says the band
/// is a program's. Monotonic: a part that dropped does not come back when
/// a more important one drops in turn.
fn program_layout(bar: &ProgramBar, available: usize) -> ProgramLayout {
    let none = ProgramLayout {
        title: false,
        detail: false,
        path_budget: 0,
        hint: None,
    };
    let title = bar.title.chars().count();
    if title == 0 || title > available {
        return none;
    }
    let detail_chars = bar.detail.chars().count();
    let detail_cols = if detail_chars == 0 {
        0
    } else {
        PROGRAM_DETAIL.chars().count() + detail_chars
    };
    if title + detail_cols > available {
        return ProgramLayout {
            title: true,
            ..none
        };
    }
    let head = title + detail_cols;
    let path = bar.path.chars().count();
    let path_cols = if path == 0 {
        0
    } else {
        REMOTE_GAP.chars().count() + path
    };
    let hint = bar.hint.chars().count();
    let placed = ProgramLayout {
        title: true,
        detail: detail_chars > 0,
        path_budget: path,
        hint: None,
    };
    if hint > 0 && head + path_cols + STATS_GAP + hint <= available {
        return ProgramLayout {
            hint: Some(available - hint),
            ..placed
        };
    }
    ProgramLayout {
        path_budget: if path == 0 {
            0
        } else {
            available.saturating_sub(head + REMOTE_GAP.chars().count())
        },
        ..placed
    }
}

/// The context row's **program** form ([`DockContext::program`]): the
/// title in the bar's tone (`info`, the unmarked remote host's color — both
/// bars are the same kind of guide), the separator quiet, the detail dim,
/// the path quiet (a location, the remote path's quieter tier) and the
/// hint dim; [`program_layout`] decides what shows.
fn render_program(
    bar: &ProgramBar,
    theme: &Theme,
    available: usize,
    row: u16,
    sink: &mut impl FnMut(Cell),
) {
    let layout = program_layout(bar, available);
    if !layout.title {
        return;
    }
    let tone = program_color(bar.tone, theme);
    let (dim, quiet) = (theme.dim_linear(), theme.quiet_linear());
    let (shows_path, path) = path_cells(&bar.path, layout.path_budget, quiet, quiet);
    let line = bar
        .title
        .chars()
        .map(|ch| (ch, tone))
        .chain(
            layout
                .detail
                .then(|| {
                    PROGRAM_DETAIL
                        .chars()
                        .map(|ch| (ch, quiet))
                        .chain(bar.detail.chars().map(|ch| (ch, dim)))
                })
                .into_iter()
                .flatten(),
        )
        .chain(
            shows_path
                .then(|| REMOTE_GAP.chars().map(|ch| (ch, quiet)))
                .into_iter()
                .flatten(),
        )
        .chain(path);
    emit_context(line, available, row, sink);
    if let Some(start) = layout.hint {
        let hint = bar.hint.chars().map(|ch| (ch, dim));
        emit_context_at(start, hint, available, row, sink);
    }
}

/// One of the load indicator's three values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatsMetric {
    Cpu,
    Mem,
    /// The root file system.
    Disk,
}

/// A value's two thresholds, in percent: at `warning` the number takes the
/// theme's `warning`, at `critical` its `error` and a `▲`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatsThreshold {
    pub warning: u8,
    pub critical: u8,
}

/// The thresholds of [`StatsMetric::Cpu`], `Mem` and `Disk`, in that order — a
/// **design constant**, not a measurement (the approved design's numbers). The
/// context row's colors and the popover's bars read this single table. Disk's
/// warning is also the line below which disk is not shown at all: a full disk
/// is news, a half-full one is not.
pub const STATS_THRESHOLDS: [StatsThreshold; 3] = [
    StatsThreshold {
        warning: 70,
        critical: 90,
    },
    StatsThreshold {
        warning: 80,
        critical: 92,
    },
    StatsThreshold {
        warning: 85,
        critical: 95,
    },
];

/// How severe a value is ([`StatsMetric::level`]); ordered, the worst is the
/// largest.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum StatsLevel {
    #[default]
    Normal,
    Warning,
    Critical,
}

impl StatsMetric {
    /// The label drawn before the number — a UI string.
    pub fn label(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Mem => "mem",
            Self::Disk => "disk",
        }
    }

    /// This value's thresholds, from [`STATS_THRESHOLDS`].
    pub fn threshold(self) -> StatsThreshold {
        STATS_THRESHOLDS[self as usize]
    }

    /// The severity of `percent`; a threshold is reached **at** its value.
    pub fn level(self, percent: u8) -> StatsLevel {
        let threshold = self.threshold();
        if percent >= threshold.critical {
            StatsLevel::Critical
        } else if percent >= threshold.warning {
            StatsLevel::Warning
        } else {
            StatsLevel::Normal
        }
    }
}

/// A rung of the indicator's ladder, widest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GaugeStep {
    /// `cpu ▂▃▅▇▅▃▂▁ 23%  mem 61%`.
    Spark,
    /// `cpu 23%  mem 61%`.
    Numbers,
    /// `●`, or only the values past their threshold.
    Alerts,
    /// The worst single value: severity first, then the number.
    Worst,
}

/// The ladder of each form; the last rung is always [`GaugeStep::Worst`].
fn ladder(form: StatsForm) -> &'static [GaugeStep] {
    match form {
        StatsForm::Sparkline => &[GaugeStep::Spark, GaugeStep::Numbers, GaugeStep::Worst],
        StatsForm::Numbers => &[GaugeStep::Numbers, GaugeStep::Worst],
        StatsForm::Alerts => &[GaugeStep::Alerts, GaugeStep::Worst],
    }
}

/// A gauge character's tone; the color is resolved at drawing (the theme is
/// not the layout's input).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tone {
    /// Labels, the sparkline and the gaps: dim.
    Quiet,
    /// A number and its `▲`: dim below the threshold, then `warning`/`error`.
    Level(StatsLevel),
    /// The alerts form's `●`: `success`.
    Calm,
}

/// The widest gauge: `cpu ▁▁▁▁▁▁▁▁ ▲100%  mem ▲100%  disk ▲100%` is 41
/// characters; a fixed capacity keeps per-frame allocation at zero.
const GAUGE_MAX: usize = 48;

/// A rung's characters, in a fixed buffer. The context row counts characters
/// (`render_context`'s doc), and every character here is one column.
#[derive(Clone, Copy)]
struct Gauge {
    cells: [(char, Tone); GAUGE_MAX],
    len: usize,
}

impl Gauge {
    fn new() -> Self {
        Self {
            cells: [(' ', Tone::Quiet); GAUGE_MAX],
            len: 0,
        }
    }

    fn cells(&self) -> &[(char, Tone)] {
        &self.cells[..self.len]
    }

    fn width(&self) -> usize {
        self.len
    }

    /// The capacity is a guard, not a policy: [`GAUGE_MAX`] holds the widest rung.
    fn push(&mut self, ch: char, tone: Tone) {
        if let Some(slot) = self.cells.get_mut(self.len) {
            *slot = (ch, tone);
            self.len += 1;
        }
    }

    fn text(&mut self, text: &str, tone: Tone) {
        for ch in text.chars() {
            self.push(ch, tone);
        }
    }

    /// The gap between two values: two columns, the remote form's own gap;
    /// nothing before the first.
    fn gap(&mut self) {
        if self.len > 0 {
            self.text(REMOTE_GAP, Tone::Quiet);
        }
    }

    /// `[label ][▲]{n}%`: the label dim, the number in its severity; `▲` glued
    /// to the number when critical.
    fn value(&mut self, metric: StatsMetric, percent: u8, label: bool) {
        if label {
            self.text(metric.label(), Tone::Quiet);
            self.push(' ', Tone::Quiet);
        }
        let level = metric.level(percent);
        if level == StatsLevel::Critical {
            self.push(STATS_CRITICAL, Tone::Level(level));
        }
        for digit in decimal(u16::from(percent)) {
            self.push(digit, Tone::Level(level));
        }
        self.push('%', Tone::Level(level));
    }

    /// The sparkline's eight columns, right-aligned: missing samples on the
    /// left are blank — a group whose width changed with every sample would
    /// move the path's budget too.
    fn spark(&mut self, history: &[u8]) {
        let shown = &history[history.len().saturating_sub(STATS_HISTORY)..];
        for _ in shown.len()..STATS_HISTORY {
            self.push(' ', Tone::Quiet);
        }
        for &level in shown {
            let block = char::from_u32(SPARK_BASE + u32::from(level.min(7))).unwrap_or(' ');
            self.push(block, Tone::Quiet);
        }
    }
}

/// The values shown at all: CPU once it has a value (the first sample has
/// none), memory always, disk only past its warning.
fn shown_values(stats: &RemoteStats) -> impl Iterator<Item = (StatsMetric, u8)> {
    let disk = (StatsMetric::Disk.level(stats.disk) > StatsLevel::Normal).then_some(stats.disk);
    stats
        .cpu
        .map(|cpu| (StatsMetric::Cpu, cpu))
        .into_iter()
        .chain(std::iter::once((StatsMetric::Mem, stats.mem)))
        .chain(disk.map(|disk| (StatsMetric::Disk, disk)))
}

/// The worst shown value: severity first, then the number; on a tie the
/// first in `cpu, mem, disk` order.
fn worst(stats: &RemoteStats) -> (StatsMetric, u8) {
    let rank = |(metric, value): (StatsMetric, u8)| (metric.level(value), value);
    let mut shown = shown_values(stats);
    // Memory is always shown, so the first value always exists.
    let first = shown.next().unwrap_or((StatsMetric::Mem, stats.mem));
    shown.fold(
        first,
        |best, next| if rank(next) > rank(best) { next } else { best },
    )
}

/// A rung's characters. **CPU without a value is left out** rather than
/// guessed: the first sample carries only counters and the second follows a
/// second later.
fn gauge(stats: &RemoteStats, step: GaugeStep) -> Gauge {
    let mut gauge = Gauge::new();
    match step {
        GaugeStep::Spark => {
            for (metric, value) in shown_values(stats) {
                if metric == StatsMetric::Cpu {
                    gauge.text(metric.label(), Tone::Quiet);
                    gauge.push(' ', Tone::Quiet);
                    gauge.spark(stats.history());
                    gauge.push(' ', Tone::Quiet);
                    gauge.value(metric, value, false);
                } else {
                    gauge.gap();
                    gauge.value(metric, value, true);
                }
            }
        }
        GaugeStep::Numbers => {
            for (metric, value) in shown_values(stats) {
                gauge.gap();
                gauge.value(metric, value, true);
            }
        }
        GaugeStep::Alerts => {
            for (metric, value) in shown_values(stats) {
                if metric.level(value) > StatsLevel::Normal {
                    gauge.gap();
                    gauge.value(metric, value, true);
                }
            }
            if gauge.width() == 0 {
                gauge.push(STATS_CALM, Tone::Calm);
            }
        }
        GaugeStep::Worst => {
            let (metric, value) = worst(stats);
            gauge.value(metric, value, true);
        }
    }
    gauge
}

/// The minimum gap between the path and the indicator.
const STATS_GAP: usize = 2;

/// Where the indicator sits: the rung and its context-local column range
/// `[start, end)` — right-aligned, so `end` is the row's budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GaugeSpan {
    step: GaugeStep,
    start: usize,
    end: usize,
}

/// The remote form's layout ([`stats_layout`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RemoteLayout {
    /// Whether `⇄ host` fit; if not, the row is only `⇄`.
    head: bool,
    /// The path's budget, for [`path_cells`].
    path_budget: usize,
    /// The indicator; `None` → not drawn.
    gauge: Option<GaugeSpan>,
    /// The Sign In… button's context-local range `[start, end)` — the whole
    /// fill and the hit area; `None` → not drawn.
    sign_in: Option<(usize, usize)>,
}

/// The remote form's layout: `⇄ {host}  {path}` on the left, the load
/// indicator right-aligned.
///
/// **The ladder** — the indicator is less important than the path, because
/// the row's real answer is "where am I": each rung of the form is tried with
/// the **whole** path and at least [`STATS_GAP`] columns between them, widest
/// first. If none fits and the worst value is past its threshold, that value
/// stays as long as `⇄ host` + gap + it fits and the path is shortened from
/// the left into the rest — at that moment "disk 96%" matters more than the
/// path. Otherwise the indicator drops and the path takes today's budget. The
/// host is never shortened.
///
/// **The Sign In… button** takes the indicator's place — there is
/// no sample without a login — and goes **before the path**: it is the row's
/// only action and the path is shortened from the left into the rest. If even
/// `⇄ host` + gap + button does not fit it drops and the path takes today's
/// budget.
///
/// Drawing ([`render_remote_context`]), the mouse ([`stats_at`],
/// [`sign_in_span`]) and the popover's anchor ([`stats_span`]) read this; had
/// they diverged a click would fall next to the indicator or the button.
fn stats_layout(
    host: &str,
    remote_cwd: &str,
    stats: Option<&RemoteStats>,
    sign_in: bool,
    available: usize,
) -> RemoteLayout {
    // `⇄` + space + host.
    let head_chars = 2 + host.chars().count();
    if head_chars > available {
        return RemoteLayout {
            head: false,
            path_budget: 0,
            gauge: None,
            sign_in: None,
        };
    }
    let left = head_chars + REMOTE_GAP.chars().count();
    let path_budget = available.saturating_sub(left);
    let bare = RemoteLayout {
        head: true,
        path_budget,
        gauge: None,
        sign_in: None,
    };
    if sign_in {
        let width = button_width(ButtonLabel::SignIn, false);
        if left + STATS_GAP + width > available {
            return bare;
        }
        return RemoteLayout {
            path_budget: available - left - STATS_GAP - width,
            sign_in: Some((available - width, available)),
            ..bare
        };
    }
    let Some(stats) = stats else {
        return bare;
    };
    let placed = |step: GaugeStep, width: usize| RemoteLayout {
        head: true,
        path_budget: available - left - STATS_GAP - width,
        gauge: Some(GaugeSpan {
            step,
            start: available - width,
            end: available,
        }),
        sign_in: None,
    };
    let path_chars = remote_cwd.chars().count();
    for &step in ladder(stats.form) {
        let width = gauge(stats, step).width();
        if left + path_chars + STATS_GAP + width <= available {
            return placed(step, width);
        }
    }
    let (metric, value) = worst(stats);
    let width = gauge(stats, GaugeStep::Worst).width();
    if metric.level(value) > StatsLevel::Normal && left + STATS_GAP + width <= available {
        return placed(GaugeStep::Worst, width);
    }
    bare
}

/// The drawn indicator's context-local range; `None` while the upload row
/// stands in the context row's place, locally, without a value or
/// when it did not fit.
fn stats_range(context: &DockContext, budget: u16) -> Option<GaugeSpan> {
    remote_layout(context, budget)?.gauge
}

/// The remote form's layout of `context` on a `budget`-column context row;
/// `None` while the upload row stands in its place or locally.
fn remote_layout(context: &DockContext, budget: u16) -> Option<RemoteLayout> {
    if context.transfer.is_some() {
        return None;
    }
    let host = context.remote_host()?;
    let available = usize::from(budget.saturating_sub(CONTEXT_COL));
    Some(stats_layout(
        host,
        &context.remote_cwd,
        context.stats.as_ref(),
        context.sign_in.is_some(),
        available,
    ))
}

/// The Sign In… button's **dock-local** column range `[start, end)` on the
/// context row — the whole fill, the click's and the hand cursor's range;
/// `None` if it is not drawn. From the drawing's layout
/// ([`stats_layout`]), so a click cannot fall next to it.
pub fn sign_in_span(context: &DockContext, budget: u16) -> Option<(u16, u16)> {
    context.sign_in?;
    // audit: `end ≤ available ≤ budget` and `budget` is `u16`.
    remote_layout(context, budget)?
        .sign_in
        .map(|(start, end)| (CONTEXT_COL + start as u16, CONTEXT_COL + end as u16))
}

/// Whether the dock-local column `col` of the context row falls on the load
/// indicator; `budget` is the context row's budget ([`DockCols::context`]).
/// The mouse's only input — the twin of [`transfer_button_at`], from the same
/// layout as the drawing ([`stats_layout`]); the range is the whole indicator,
/// the sparkline's blank columns included.
pub fn stats_at(context: &DockContext, budget: u16, col: u16) -> bool {
    let Some(col) = col.checked_sub(CONTEXT_COL) else {
        return false;
    };
    stats_range(context, budget)
        .is_some_and(|span| (span.start..span.end).contains(&usize::from(col)))
}

/// The indicator's **dock-local** column range `[start, end)` on the context
/// row; `None` if it is not drawn. The popover's anchor — the
/// inverse of [`stats_at`], from the same layout.
pub fn stats_span(context: &DockContext, budget: u16) -> Option<(u16, u16)> {
    // audit: `end ≤ available ≤ budget` and `budget` is `u16`.
    stats_range(context, budget).map(|span| {
        (
            CONTEXT_COL + span.start as u16,
            CONTEXT_COL + span.end as u16,
        )
    })
}

/// The upload row's layout ([`transfer_layout`]): how far the row shows what.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TransferLayout {
    /// Whether `⇄ host` fit; if not, the row is only `⇄`.
    head: bool,
    /// The number of body characters shown (excluding the clipping mark).
    body: usize,
    /// Whether the body was clipped (`…` at the end).
    clipped: bool,
    /// The buttons, left to right; `None` for one that does not fit or is absent.
    buttons: [Option<ButtonSpan>; 2],
}

/// A button of the layout: the context-local column range `[start, end)` —
/// inner padding included, i.e. the whole of the fill and the hit area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ButtonSpan {
    action: TransferAction,
    label: ButtonLabel,
    /// Whether the `⌘.` hint is to the right of the label.
    hint: bool,
    start: usize,
    end: usize,
}

/// The button's label — a UI string. **A verb, not an icon** (the user,
/// visual check: `✕` also read as "close", `▴` could not be told from text at
/// the small point size).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ButtonLabel {
    Cancel,
    CancelAll,
    /// With the number of items in the list; the same label while the list is
    /// open (`Hide files` was dropped, the button is in the pressed tone).
    /// "Transfers", not "files": the list carries both directions.
    ShowFiles(u16),
    /// The ssh status bar's login: the ellipsis says a sheet opens.
    SignIn,
}

impl ButtonLabel {
    /// The label's characters — no per-frame allocation, the number is printed in place.
    fn chars(self) -> impl Iterator<Item = char> + Clone {
        let (head, count, tail) = match self {
            Self::Cancel => ("Cancel", None, ""),
            Self::CancelAll => ("Cancel all", None, ""),
            Self::ShowFiles(items) => ("Show transfers (", Some(items), ")"),
            Self::SignIn => ("Sign In\u{2026}", None, ""),
        };
        head.chars()
            .chain(count.into_iter().flat_map(decimal))
            .chain(tail.chars())
    }

    fn len(self) -> usize {
        self.chars().count()
    }
}

/// The decimal digits of `n`, without separators.
fn decimal(n: u16) -> impl Iterator<Item = char> + Clone {
    let n = u32::from(n);
    let digits = n.checked_ilog10().unwrap_or(0) + 1;
    (0..digits)
        .rev()
        // audit: `n / 10^p % 10` 0..=9, `from_digit` hep `Some`.
        .map(move |p| char::from_digit(n / 10u32.pow(p) % 10, 10).unwrap_or('0'))
}

/// The minimum gap between the body and the buttons.
const CONTROLS_GAP: usize = 2;

/// The button's inner padding, in columns — one empty column on each side of
/// the label and the fill covers them too. A design constant: one column of
/// the small class is roughly half a large cell, the approved design's inner
/// padding.
const BUTTON_PAD: usize = 1;

/// The gap between the two buttons, in columns: so the fills do not touch.
const BUTTON_GAP: usize = 1;

/// The cancel's keyboard hint — a UI string; the menu's Cancel Upload (⌘.)
/// key. Inside the button and dim: it teaches cancelling from the keyboard,
/// it does not compete with the label.
const CANCEL_HINT: &str = "⌘.";

/// A button's width, in columns: `pad + label [+ space + ⌘.] + pad`.
fn button_width(label: ButtonLabel, hint: bool) -> usize {
    let hint = if hint {
        1 + CANCEL_HINT.chars().count()
    } else {
        0
    };
    BUTTON_PAD + label.len() + hint + BUTTON_PAD
}

/// The upload row's layout: `⇄ {host}  {body}` on the left, the buttons
/// **right-aligned**.
///
/// **The budget goes first to `⇄ host`, then the buttons, the rest to the
/// body.** The host is not shortened (the remote form's reason: a shortened
/// host reads as another machine); the buttons are not shortened either — a
/// half label does not say what to click. If they do not fit they drop in
/// order: first the `⌘.` hint, then the list button, `Cancel` last — cancel is
/// the row's only urgent job. The body is shortened from the right with `…`:
/// its information is at the start (which file, which number).
///
/// Right-aligned, because the body changes size on every refresh (speed,
/// remaining time) and buttons stuck behind it would slide out from under the
/// mouse.
///
/// Drawing ([`render_transfer`]) and the mouse ([`transfer_button_at`]) read
/// this; had the two arithmetics diverged a click would fall next to the
/// button.
fn transfer_layout(transfer: &Transfer, available: usize) -> TransferLayout {
    let head_chars = 2 + transfer.host.chars().count();
    if head_chars > available {
        return TransferLayout {
            head: false,
            body: 0,
            clipped: false,
            buttons: [None; 2],
        };
    }
    let rest = available - head_chars;
    let rest = rest.saturating_sub(REMOTE_GAP.chars().count());
    let body_chars = transfer.body.chars().count();

    let controls = transfer.controls;
    let cancel = if controls.items > 1 {
        ButtonLabel::CancelAll
    } else {
        ButtonLabel::Cancel
    };
    let list = (controls.items > 1).then_some(ButtonLabel::ShowFiles(controls.items));
    // Drop order: hint, list, (if there is no cancel there is no button).
    let options = [(list, true), (list, false), (None, false)];
    let chosen = (controls.items > 0)
        .then(|| {
            options.into_iter().find(|&(list, hint)| {
                let list_width = list.map_or(0, |label| button_width(label, false) + BUTTON_GAP);
                list_width + button_width(cancel, hint) <= rest
            })
        })
        .flatten();
    let mut buttons = [None; 2];
    let mut controls_width = 0;
    if let Some((list, hint)) = chosen {
        let cancel_width = button_width(cancel, hint);
        // The right edge is the context-local `head + gap + rest`, i.e.
        // `available` itself (if the gap was clipped `rest` has dropped to zero and there is no button).
        let right = head_chars + REMOTE_GAP.chars().count() + rest;
        let cancel_start = right - cancel_width;
        buttons[1] = Some(ButtonSpan {
            action: TransferAction::Cancel,
            label: cancel,
            hint,
            start: cancel_start,
            end: right,
        });
        controls_width = cancel_width;
        if let Some(label) = list {
            let end = cancel_start - BUTTON_GAP;
            let start = end - button_width(label, false);
            buttons[0] = Some(ButtonSpan {
                action: TransferAction::List,
                label,
                hint: false,
                start,
                end,
            });
            controls_width = right - start;
        }
    }
    let with_controls = controls_width > 0;
    let budget = rest - controls_width;
    // A gap between the body and the buttons only if both exist; if there is
    // no room for the gap the body withdraws, not the buttons.
    let budget = if with_controls && body_chars > 0 {
        budget.saturating_sub(CONTROLS_GAP)
    } else {
        budget
    };
    let (body, clipped) = if body_chars <= budget {
        (body_chars, false)
    } else {
        // The mark itself is a column too.
        (budget.saturating_sub(1), budget > 0)
    };
    TransferLayout {
        head: true,
        body,
        clipped,
        buttons,
    }
}

/// Which button of the upload row the dock-local column `col` of the context
/// row falls in; `None` → none, or there is no button. `context` is the
/// context row's budget ([`DockCols::context`]).
///
/// The mouse's only input: from the same layout as the drawing
/// ([`transfer_layout`]) and the range is the whole of the fill — inner
/// padding included, so a click that lands on the empty column next to the
/// label still finds the button.
pub fn transfer_button_at(transfer: &Transfer, context: u16, col: u16) -> Option<TransferAction> {
    let available = usize::from(context.saturating_sub(CONTEXT_COL));
    let col = usize::from(col.checked_sub(CONTEXT_COL)?);
    transfer_layout(transfer, available)
        .buttons
        .into_iter()
        .flatten()
        .find(|button| (button.start..button.end).contains(&col))
        .map(|button| button.action)
}

/// The button's **dock-local** column range `[start, end)` on the context row
/// — the whole of the fill; `None` if there is no button or it did not fit.
/// The anchor of the list popover: the popover is tied to the
/// button, not to the clicked point. The inverse of [`transfer_button_at`],
/// from the same layout.
pub fn transfer_button_span(
    transfer: &Transfer,
    context: u16,
    action: TransferAction,
) -> Option<(u16, u16)> {
    let available = usize::from(context.saturating_sub(CONTEXT_COL));
    transfer_layout(transfer, available)
        .buttons
        .into_iter()
        .flatten()
        .find(|button| button.action == action)
        // audit: `end ≤ available ≤ context` and `context` is `u16`.
        .map(|button| {
            (
                CONTEXT_COL + button.start as u16,
                CONTEXT_COL + button.end as u16,
            )
        })
}

/// The context row's **upload** form: `⇄ {host}` in the mark's color (the remote form's prefix
/// and color are kept), the body dim, buttons on the right.
///
/// The button's label is in the **foreground** — the row's only foreground
/// text, so what is to be clicked stands apart from what is to be read; the
/// `⌘.` hint is dim, in the foreground when the mouse is over. The fill and
/// border are not cells, they are in the return ([`Dock::buttons`]).
fn render_transfer(
    transfer: &Transfer,
    theme: &Theme,
    available: usize,
    row: u16,
    sink: &mut impl FnMut(Cell),
) -> [Option<DockButton>; 2] {
    let accent = theme.mark_linear(transfer.mark);
    let layout = transfer_layout(transfer, available);
    let mark = std::iter::once((REMOTE_MARK, accent));
    if !layout.head {
        emit_context(mark, available, row, sink);
        return [None; 2];
    }
    let dim = theme.dim_linear();
    // The result's tone at the start of the body: success green,
    // error text red; the rest dim.
    let toned = match transfer.tone {
        TransferTone::Quiet => dim,
        TransferTone::Success => theme.success_linear(),
        TransferTone::Error => theme.error_linear(),
    };
    let lead = transfer.lead;
    let line = mark
        .chain(std::iter::once((' ', accent)))
        .chain(transfer.host.chars().map(move |ch| (ch, accent)))
        .chain(REMOTE_GAP.chars().map(move |ch| (ch, accent)))
        .chain(
            transfer
                .body
                .chars()
                .take(layout.body)
                .enumerate()
                .map(move |(i, ch)| (ch, if i < lead { toned } else { dim })),
        )
        .chain(layout.clipped.then_some((ELLIPSIS, dim)));
    emit_context(line, available, row, sink);

    let controls = transfer.controls;
    layout.buttons.map(|button| {
        let button = button?;
        let state = match button.action {
            TransferAction::List if controls.list_open => ButtonState::Pressed,
            action if controls.hover == Some(action) => ButtonState::Hover,
            _ => ButtonState::Idle,
        };
        let hint_color = if state == ButtonState::Idle {
            dim
        } else {
            theme.foreground_linear()
        };
        let label = button
            .label
            .chars()
            .map(|ch| (ch, theme.foreground_linear()))
            .chain(
                button
                    .hint
                    .then(|| {
                        std::iter::once((' ', dim))
                            .chain(CANCEL_HINT.chars().map(|ch| (ch, hint_color)))
                    })
                    .into_iter()
                    .flatten(),
            );
        emit_context_at(button.start + BUTTON_PAD, label, available, row, sink);
        Some(DockButton {
            // audit: `end ≤ available ≤ context` and `context` is `u16`.
            start: CONTEXT_COL + button.start as u16,
            end: CONTEXT_COL + button.end as u16,
            color: accent,
            state,
        })
    })
}

/// A path's cells on the context row, shortened **from the left** to `budget`
/// characters and in two tiers; the first value is whether the path shows.
///
/// The part common to the local and remote forms: the rule is the same in
/// both, because both are the answer to the "which folder are you in"
/// question.
fn path_cells(
    path: &str,
    budget: usize,
    normal: LinearRgba,
    quiet: LinearRgba,
) -> (bool, impl Iterator<Item = (char, LinearRgba)> + '_) {
    let path_chars = path.chars().count();
    // `skip` is the number of characters dropped from the **start** of the
    // path; `mark` is the shortening's visible mark. If the path is not drawn
    // at all both are silent from the start.
    let (mark, skip) = if budget == 0 || path_chars == 0 {
        (None, path_chars)
    } else if path_chars <= budget {
        (None, 0)
    } else {
        // The mark itself is a column too: `budget - 1` characters from the tail.
        (Some(ELLIPSIS), path_chars - (budget - 1))
    };
    let shows = mark.is_some() || skip < path_chars;

    // **The path's last component stands out, what precedes it recedes.** The
    // information the user looks for is "which folder am I in"; the parent
    // directories are the context that places it. With both in the same tone
    // the eye had to search for the last component.
    //
    // The dim one is **not a new color**: the dim of the dim
    // (`Theme::quiet_linear`), i.e. the second application of the same rule
    // (`dim_toward`). The hairline is one step further out and there is a
    // reason it stops there: it is **not ink**, this is still a path that
    // needs to be read.
    //
    // The **character** index of the last component in the path: what is
    // after the last `/`. No splitting, `enumerate` not `char_indices`: the
    // `skip` above also counts characters and the two must be in the same unit.
    let head_end = path
        .chars()
        .enumerate()
        .filter(|(_, ch)| *ch == '/')
        .map(|(index, _)| index + 1)
        .last()
        .unwrap_or(0);
    // If the last component is empty (`/`, or a trailing slash) no distinction
    // is made: the whole path stands out. The wrong side is the safe side —
    // over-emphasizing hides no information, dimming everything would.
    let head_end = if head_end >= path_chars { 0 } else { head_end };

    let cells = mark
        // The shortening mark stands in the place of the dropped **parent**
        // directories, i.e. in the same tone as them.
        .map(|ch| (ch, quiet))
        .into_iter()
        .chain(
            path.chars()
                .skip(skip)
                .enumerate()
                .map(move |(offset, ch)| {
                    (
                        ch,
                        if skip + offset < head_end {
                            quiet
                        } else {
                            normal
                        },
                    )
                }),
        );
    (shows, cells)
}

/// Prints the context row's cells to the sink within `available` columns.
fn emit_context(
    line: impl Iterator<Item = (char, LinearRgba)>,
    available: usize,
    row: u16,
    sink: &mut impl FnMut(Cell),
) {
    emit_context_at(0, line, available, row, sink);
}

/// [`emit_context`], starting at the context-local column `start` (the upload
/// row's right-aligned buttons).
fn emit_context_at(
    start: usize,
    line: impl Iterator<Item = (char, LinearRgba)>,
    available: usize,
    row: u16,
    sink: &mut impl FnMut(Cell),
) {
    // `take` is a guard, not a policy: the caller's budget already does not
    // exceed `available` columns. A cell overflowing on the right would write
    // outside the grid and that arithmetic error stops silently here.
    for (offset, (ch, fg)) in line.take(available.saturating_sub(start)).enumerate() {
        let offset = start + offset;
        // A space produces no glyph (`cell`'s rule); both sides of the
        // separator are eliminated here.
        if ch == ' ' {
            continue;
        }
        sink(Cell {
            // audit: `offset < available ≤ cols` and `cols` is `u16`; the sum cannot overflow.
            col: CONTEXT_COL + offset as u16,
            row,
            ch: Some(ch),
            // The whole row stays dim — the context is readable but does not
            // compete with the input row — and there is a second tier **inside**
            // it (above). The remote form's host is the one exception: distance
            // is this row's actual news.
            fg,
            ..Cell::default()
        });
    }
}

/// The color of the `>` mark: the shell's phase.
///
/// The **same vocabulary** as the block stripe (`ShellLog::stripe`): a
/// running command is the accent, a finished command success or error by exit
/// code. Had a separate color been chosen, the window would tell the same
/// fact in two ways in two places.
///
/// A session without integration (`None`) does not come here — a window with
/// no dock never calls this module — but the answer is the accent anyway: the
/// mark is the input row's mark even when we do not know the phase.
fn sigil_color(shell: Option<ShellState>, theme: &Theme) -> LinearRgba {
    match shell {
        Some(ShellState {
            phase: ShellPhase::Finished,
            last_exit: Some(code),
        }) => {
            if code == 0 {
                theme.success_linear()
            } else {
                theme.error_linear()
            }
        }
        _ => theme.accent_linear(),
    }
}

/// The style applied to the display's character number `index`.
///
/// The records are applied **in order** and the later one wins: zsh also
/// applies `region_highlight` in the list's order, so a plugin that writes on
/// top (syntax highlighting over autosuggestions) stays on top here too.
///
/// Walking the whole list per character is quadratic but both factors are
/// small: the column count is one window, the record count is one row's
/// tokens. Sorting the ranges into a single pass asks for more than sorting,
/// because the records **can overlap**; it was not written ahead of a measured
/// need.
fn style_at(state: &DockState, index: usize) -> HighlightStyle {
    let mut style = HighlightStyle::default();
    for highlight in &state.highlights {
        if !(highlight.start..highlight.end).contains(&index) {
            continue;
        }
        let applied = highlight.style;
        style = HighlightStyle {
            fg: applied.fg.or(style.fg),
            bg: applied.bg.or(style.bg),
            bold: style.bold || applied.bold,
            underline: style.underline || applied.underline,
            standout: style.standout || applied.standout,
        };
    }
    style
}

/// How many **columns** a character takes; `0` if it is zero-width.
///
/// The source is `unicode-width` and this is not a preference but a
/// **necessity**: the grid uses the same crate (alacritty sets
/// `Flags::WIDE_CHAR` with it) and the day a second width source diverged the
/// symptom would be silent — the dock would shift by a column.
///
/// **There are two distinct zeros and the `Option` carries the
/// distinction.** `width()` returns `None` for control characters and
/// `Some(0)` for combiners (VS16, ZWJ, accent) (measured) and the two are
/// handled **separately** here:
///
/// - `Some(0)` → **0 columns.** A combiner has no cell of its own in the
///   grid either (alacritty `CellExtra`), so not consuming a column is
///   right.
/// - `None` → **1 column.** A control character is not drawn in the dock
///   ([`cell`]) but before column arithmetic it **held its column** (every index was a
///   column) and dropping it to zero would be a regression: the words on both
///   sides of a TAB inserted with `Ctrl-V` would merge and the caret would
///   shift left by one column per control character. Code review
///   caught this.
///
/// **A known limit, and its direction changed.** The right display is
/// neither 0 nor 1: zsh shows a control character as `^C` in **two**
/// columns. [`cell`]'s doc had written the reason for not drawing a
/// placeholder as "it takes the column arithmetic out of character units" and
/// that constraint was **lifted** since — the arithmetic is now columns
/// anyway. So drawing `^C` is possible today; it was not done because nobody
/// asked. **Since [`DockStatus::Control`] the limit has narrowed to the
/// tab:** a row carrying the other control characters stays
/// in the grid with [`DockStatus::Control`] and never reaches this function.
pub(crate) fn column_width(ch: char) -> usize {
    // `unwrap_or(1)`, not `unwrap_or(0)`: see the doc.
    UnicodeWidthChar::width(ch).unwrap_or(1)
}

/// A **visual** row of the layout: which character range, from which column.
///
/// The range is in the character index of the stream given to [`layout`] and
/// is **half-open**; the `\n` that breaks the row is in no row's range (no
/// glyph, no column). The rows left by wrapping and by a line break are the
/// same type: the distinction is not the consumer's question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VisualLine {
    /// The index of the first character.
    pub(crate) start: usize,
    /// One past the last character; `start` in an empty row.
    pub(crate) end: usize,
    /// The column the row starts from ([`layout`]'s `first`/`rest`).
    pub(crate) col: usize,
}

/// [`layout`]'s answer that is single per frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LayoutEnd {
    /// The caret's visual row, from `0`.
    pub(crate) caret_row: usize,
    /// The caret's column.
    pub(crate) caret_col: usize,
    /// The number of visual rows, including the caret's row; at least 1.
    pub(crate) rows: usize,
}

/// The **row-aware** layout of the display — a single walk.
///
/// It splits the stream at `\n`s and wraps at `width` columns; the first row
/// starts from column `first`, all continuation rows (those opened by wrapping
/// and by `\n`) from `rest`. The visual rows flow into `line` in order, with no
/// allocation — the frame path runs this every frame.
///
/// **The single authority for column counts is again [`column_width`]:** the
/// grid's wrapping, the dock's drawing and the suppression's row arithmetic
/// count from the same table, or one would be hidden while the
/// other showed.
///
/// The rules and the reason for each:
///
/// - **`\n` takes no column, it breaks the row.** It has no glyph; before multi-line
///   input the dock flattened it into a single row and it consumed a column and
///   squashed the text.
/// - **A wide character is not split**, if it does not fit it moves to the
///   next row and an empty column remains behind it — the grid's rule
///   (`LEADING_WIDE_CHAR_SPACER`); the dock wraps from the same walk
///   ([`dock_layout`]).
/// - **Wrapping is lazy:** a row opens only when a character does not fit, so
///   no empty row is born after a completely full row.
/// - **The caret is where the next character would go**; at the end, where a
///   one-column character would go. So a caret at the end of a completely full
///   row is **at the start of the next row** and that row is counted: zsh
///   does not leave the cursor in the pending-wrap state at the end of a row,
///   it drops it to the next row — the counterpart of the `saturating_sub(1)`
///   rule in the suppression's old formula.
/// - **An empty row that does not fit overflows, it does not wrap forever:** a
///   character that does not fit even at the start of a continuation row (a
///   wide glyph in a one-column window) stays in place. If `first` is to the
///   right of `rest` the first row can stay empty and wrap — the row the
///   prompt finished in the grid.
///
/// `caret` is in the stream's index; if larger than the stream it is clamped
/// to the end.
pub(crate) fn layout(
    chars: impl IntoIterator<Item = char>,
    caret: usize,
    width: usize,
    first: usize,
    rest: usize,
    cluster: bool,
    line: impl FnMut(VisualLine),
) -> LayoutEnd {
    layout_with(
        chars.into_iter().map(|ch| (ch, ())),
        caret,
        width,
        first,
        rest,
        cluster,
        line,
        |_| {},
    )
}

/// A character's place in the layout — [`layout_with`]'s per-character output.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Placed<T> {
    /// Its order in the stream (zero-widths and `\n` also count): the
    /// display's character index, the unit of `region_highlight` and `CURSOR`.
    pub(crate) index: usize,
    /// One past the cluster: with clustering on, the cluster's remaining
    /// code points (the `🇷` of `🇹🇷`, VS16, the ZWJ pieces) are in the range
    /// `index..end` and have no `Placed` of their own; `index + 1` when off.
    pub(crate) end: usize,
    /// The cluster's **head** character — the `c` in the grid's cell.
    pub(crate) ch: char,
    /// The column width, `1` or `2` ([`column_width`], with clustering on
    /// [`crate::cluster::width`]; zero does not come here).
    pub(crate) width: usize,
    /// The visual row, from `0`.
    pub(crate) row: usize,
    /// The column (including [`layout`]'s `first`/`rest`).
    pub(crate) col: usize,
    /// The data the caller carries with the character (base color, highlight).
    pub(crate) tag: T,
}

impl<T> Placed<T> {
    /// Whether the character fits in width `cols`. The one exception is
    /// [`layout`]'s "an empty row that does not fit overflows" rule (a wide
    /// glyph in a one-column window): the drawing side does not draw it, or it
    /// would write outside the grid.
    pub(crate) fn fits(&self, cols: u16) -> bool {
        self.col + self.width <= usize::from(cols)
    }
}

/// [`layout`] in the form that also gives the characters: `place` receives the
/// place of every **visible** character (column width above zero) — a
/// zero-width code point takes no cell (it has no cell of its own in the grid
/// either, alacritty `CellExtra`), `\n` has no glyph and no column.
///
/// **One walk, two kinds of reader:** those asking for the row count
/// (`layout`, the suppression's grid computation) pass `place` empty, those
/// printing cells (the dock's drawing, the hit test) use it. Had there been two
/// separate walks, the day the wrap or wide-character rule diverged between
/// them the band would shift by a row and the mouse by a column.
///
/// **With clustering on (`cluster`) the unit is the cluster**, not the
/// code point: the same rule as the grid's wrapper
/// ([`crate::cluster::extends`]) and the same column
/// ([`crate::cluster::width`]), so `👨‍👩‍👧` is two columns in the grid and the dock
/// alike and the suppression's span does not diverge from the grid. A cluster
/// is a single `Placed` (the head character and the head character's tag), the
/// wrap decision applies to the whole cluster; if `caret` falls **inside** a
/// cluster the caret is at the cluster's start — there is no column
/// to write in the middle of a cluster.
// The eighth argument is `cluster`: the session's one flag; since all
// four callers share the same walk, wrapping it in a struct would add a constructor.
#[allow(clippy::too_many_arguments)]
pub(crate) fn layout_with<T>(
    items: impl IntoIterator<Item = (char, T)>,
    caret: usize,
    width: usize,
    first: usize,
    rest: usize,
    cluster: bool,
    mut line: impl FnMut(VisualLine),
    mut place: impl FnMut(Placed<T>),
) -> LayoutEnd {
    let width = width.max(1);
    let first = first.min(width);
    let mut row = 0;
    let mut col = first;
    let mut visual = VisualLine {
        start: 0,
        end: 0,
        col: first,
    };
    let mut at_caret = None;
    // Does the character fit at `col`; if not, can the row be wrapped. On an
    // empty row not to the right of `rest`, wrapping would be returning to the same place.
    let fits = |col: usize, w: usize, visual: &VisualLine, at: usize| {
        col + w <= width || (visual.start == at && col <= rest)
    };
    let mut count = 0;
    // The open cluster's text; filled only when clustering is on.
    let mut text = String::new();
    let mut items = items.into_iter().enumerate().peekable();
    while let Some((index, (ch, tag))) = items.next() {
        count = index + 1;
        if ch == '\n' {
            if index == caret {
                at_caret = Some(if fits(col, 1, &visual, index) {
                    (row, col)
                } else {
                    (row + 1, rest)
                });
            }
            visual.end = index;
            line(visual);
            row += 1;
            col = rest;
            visual = VisualLine {
                start: index + 1,
                end: index + 1,
                col: rest,
            };
            continue;
        }
        // The cluster's text is built only if the next code point could extend
        // it ([`crate::cluster::Walk`]'s reason): in plain text there is no
        // per-frame allocation. A single-code-point cluster's column is
        // [`column_width`] itself, so clustering changes nothing in plain text.
        let (end, w) = if cluster
            && items
                .peek()
                .is_some_and(|&(_, (next, _))| crate::cluster::may_extend(next))
        {
            text.clear();
            text.push(ch);
            // The rest of the cluster gets no `Placed` of its own; its tag is
            // the head character's (the rule for zero-widths today).
            while let Some((next, (c, _))) =
                items.next_if(|(_, (next, _))| crate::cluster::extends(&text, *next))
            {
                text.push(c);
                count = next + 1;
            }
            (count, crate::cluster::width(&text))
        } else {
            (index + 1, column_width(ch))
        };
        if !fits(col, w, &visual, index) {
            visual.end = index;
            line(visual);
            row += 1;
            col = rest;
            visual = VisualLine {
                start: index,
                end: index,
                col: rest,
            };
        }
        if (index..end).contains(&caret) {
            at_caret = Some((row, col));
        }
        if w > 0 {
            place(Placed {
                index,
                end,
                ch,
                width: w,
                row,
                col,
                tag,
            });
        }
        col += w;
    }
    visual.end = count;
    let (caret_row, caret_col) = match at_caret {
        Some(at) => at,
        None if fits(col, 1, &visual, count) => (row, col),
        None => {
            // A caret at the end behind a completely full row: close the row
            // and open its own (empty) row for the caret.
            line(visual);
            row += 1;
            visual = VisualLine {
                start: count,
                end: count,
                col: rest,
            };
            (row, rest)
        }
    };
    line(visual);
    LayoutEnd {
        caret_row,
        caret_col,
        rows: row + 1,
    }
}

/// The suppression's row arithmetic: how many rows **above** the cursor's
/// grid row the input starts and how many rows **below** it extends.
///
/// The **grid** parametrization of [`layout`]: zsh's layout —
/// the first row from the column where the prompt ended, continuation rows
/// from `0`. The prompt's width is not in the mirror but it is observed: the
/// cursor's column in the grid (`cursor_col`) minus the column of the text
/// before the cursor, in `width` mode.
///
/// **The observation is exact if the cursor's logical row is the first row.**
/// If the cursor is behind a `\n` the start of the first row cannot be derived
/// from this column and [`TEXT_COL`] is assumed: the suppression runs only at
/// the dock tier and there `PS1` is exactly that many columns (a test ties the
/// script's two spaces to this constant). If the assumption is wrong the upper
/// end is clamped at the caller with the anchor's row (`from.max(floor)`), so
/// it cannot overflow above the prompt. **A known limit:** on a `PS2` row
/// (`for> `) the first row of `BUFFER` starts behind the `PS2` and its width
/// is not in the mirror; in that case `PREBUFFER` is full and the upper floor
/// is already the anchor's row
/// ([`crate::shell::SuppressedInput::from_anchor`]), so this number is not
/// used.
///
/// **A known limit, in the safe direction:** if the empty column a wide
/// character leaves at the end of a row is **before** the cursor the observed
/// start shifts right by that much and the upper end can come out a row too
/// many; since the upper end is clamped at the caller with the anchor's row
/// (`from.max(floor)`) it cannot overflow above the prompt. The old column
/// division had the same limit. **After** the cursor it is now correct: there
/// the division did not see the blank and counted the tail short.
pub(crate) fn grid_span(
    display: &str,
    caret: usize,
    cursor_col: usize,
    width: usize,
    cluster: bool,
) -> (usize, usize) {
    let width = width.max(1);
    // The columns before the cursor on the cursor's logical row — with the
    // cluster when clustering is on ([`layout_with`]'s unit). **The cluster
    // the cursor falls inside is not counted**: ZLE does not know clusters
    // (wcwidth, code point by code point), so after `👍🏽` a ← puts `CURSOR`
    // before the `🏽` and the grid's cursor is at the cluster's head column;
    // the layout also seats the caret at the cluster's start.
    // Counting a half cluster would shift the start column two columns left
    // and at the wrap limit the suppression would be off by a row.
    let mut on_line = 0;
    let mut first_line = true;
    if cluster {
        crate::cluster::Walk::new().run(display.chars(), |at| {
            if at.end > caret {
                return;
            }
            if at.head == '\n' {
                on_line = 0;
                first_line = false;
            } else {
                on_line += at.width;
            }
        });
    } else {
        for ch in display.chars().take(caret) {
            if ch == '\n' {
                on_line = 0;
                first_line = false;
            } else {
                on_line += column_width(ch);
            }
        }
    }
    let first = if first_line {
        (cursor_col % width + width - on_line % width) % width
    } else {
        usize::from(TEXT_COL)
    };
    let end = layout(display.chars(), caret, width, first, 0, cluster, |_| {});
    (end.caret_row, end.rows - 1 - end.caret_row)
}

/// A character's cell: the base color + the range's style.
///
/// **A selected cell by the grid's rule**: the text in its own
/// foreground, reverse video resolved, the ground dropped — the selection's
/// color takes its place. `region_highlight`'s `standout` (zsh's paste
/// highlight is that by default) reads in its normal foreground in a selection.
fn cell(
    ch: char,
    col: u16,
    base: LinearRgba,
    style: HighlightStyle,
    theme: &Theme,
    wide: bool,
    selected: bool,
) -> Cell {
    let mut fg = style.fg.map_or(base, |color| resolve(color, theme));
    let mut bg = style.bg.map(|color| resolve(color, theme));
    if selected {
        bg = None;
    } else if style.standout {
        // Reverse video: the two colors swap. If the range has no ground of
        // its own the surface's ground takes its place — `frame()`'s `INVERSE`
        // arm also concretizes the cell's default background the same way.
        let behind = bg.unwrap_or_else(|| theme.background_linear());
        bg = Some(fg);
        fg = behind;
    }
    Cell {
        col,
        row: 0,
        // The rule for an inkless cell is the same as `frame()`'s: a space
        // produces no glyph (it spends a slot in the atlas, paints not a
        // single pixel). Control characters produce none either, but now only
        // the **tab** reaches here: a row carrying the other control
        // characters stays in the grid with [`DockStatus::Control`] and the
        // dock never draws it — ZLE prints the raw byte in the grid as a
        // readable `^A`, while the dock would leave that column empty. Drawing
        // a placeholder in place (`^C`) is possible now (the
        // arithmetic is already columns) and on that day the `Control` arm is
        // deleted. Details in [`column_width`]'s doc.
        ch: (!ch.is_control() && ch != ' ').then_some(ch),
        fg,
        bg,
        bold: style.bold,
        italic: false,
        underline: if style.underline {
            UnderlineStyle::Single
        } else {
            UnderlineStyle::None
        },
        // There is no counterpart of SGR 58 in `region_highlight`: the line takes the foreground.
        underline_color: None,
        strikeout: false,
        // **The wide path is now open in the dock**: the column above
        // accumulates from **width**, not from the character index, so the
        // right column of a two-cell glyph is really reserved and does not
        // paint over its neighbor. Once this line was a constant `false` and
        // its reason was sound *with that arithmetic*; when the arithmetic
        // changed the invariant went.
        wide,
        // The caller sets the cluster: this function sees a single character.
        cluster: None,
    }
}

/// A cluster's boundary identity in the layout: only
/// **wide** clusters of more than one code point go into the table — the
/// grid's rule (`session::cell_cluster`). `chars` is the cluster's code
/// points; read only if a cluster will be born, so plain text pays no more
/// than a comparison. With clustering off `len` is always `1`.
fn placed_cluster<I: Iterator<Item = char>>(
    len: usize,
    width: usize,
    clusters: &mut Clusters,
    chars: impl FnOnce() -> I,
) -> Option<ClusterId> {
    (len > 1 && width == 2)
        .then(|| clusters.push_chars(chars()))
        .flatten()
}

/// The mirror's color record → the color to draw.
fn resolve(color: HighlightColor, theme: &Theme) -> LinearRgba {
    match color {
        HighlightColor::Indexed(index) => theme.indexed_linear(index),
        HighlightColor::Rgb(hex) => color::linear_hex(hex),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Ownership is now `render`'s argument; only this module calls the
    // predicate, in production `Session::frame` gives the answer.
    use crate::settings::HostMark;
    use crate::shell::{
        CaretHome, DockFault, Highlight, RemoteTarget, TransferControls, caret_home,
    };

    const THEME: Theme = Theme::BATERI;

    /// The column count: most tests do not ask about wrapping and this width
    /// holds their text comfortably.
    const COLS: u16 = 40;

    fn live(predisplay: &str, buffer: &str, postdisplay: &str, cursor: usize) -> DockState {
        DockState {
            status: DockStatus::Live,
            predisplay: predisplay.into(),
            buffer: buffer.into(),
            postdisplay: postdisplay.into(),
            prebuffer: String::new(),
            cursor,
            highlights: Vec::new(),
            // The length the decoder counts; set by hand here because this
            // module's tests never touch the wire. `render` does not read it —
            // its consumer is the suppression (`ShellLog::suppressed_input`).
            display_chars: predisplay.chars().count()
                + buffer.chars().count()
                + postdisplay.chars().count(),
            last_ink: predisplay
                .chars()
                .chain(buffer.chars())
                .chain(postdisplay.chars())
                .filter(|ch| !ch.is_whitespace())
                .next_back(),
            // This module does not read it (its consumer is
            // `Session::can_be_typed`); a live row's usual state is the insert keymap.
            insert_keymap: true,
            // The freshness gate's stamp; `render` does not read it.
            answers: 0,
            // Read without clusters: the clustered tests turn this on.
            cluster: false,
        }
    }

    /// Context-free drawing: both path and branch empty (the state of this
    /// module's old tests). The two rows' budgets are equal: this module's
    /// tests ask about **drawing**, not the point size. The state where the
    /// budget diverges has its own test
    /// (`context_line_spends_its_own_budget`).
    fn same(cols: u16) -> DockCols {
        DockCols {
            grid: cols,
            context: cols,
        }
    }

    fn draw(state: &DockState, cols: u16) -> (Vec<Cell>, Dock) {
        draw_with(state, &DockContext::default(), cols)
    }

    /// The drawn cells, in column order.
    fn draw_with(state: &DockState, context: &DockContext, cols: u16) -> (Vec<Cell>, Dock) {
        let mut cells = Vec::new();
        // Ownership is not the test's input: in production `Session::frame`
        // gives it, here it is derived from the same predicate so that this
        // module's tests exercise **the drawing**, not the handover's rule.
        // The hold is off for that reason too (`held: false`): hysteresis
        // changes **when** the handover becomes visible, not its drawing.
        let owned = caret_home(None, state.status, false) == CaretHome::Dock;
        let dock = render(
            state,
            context,
            None,
            &THEME,
            same(cols),
            owned,
            None,
            |cell| cells.push(cell),
            |_| (),
        );
        (cells, dock)
    }

    /// For the tests asking about the phase's effect on the caret: the shell's
    /// state from the caller.
    fn draw_as(state: &DockState, shell: Option<ShellState>, cols: u16) -> (Vec<Cell>, Dock) {
        let mut cells = Vec::new();
        let owned = caret_home(shell, state.status, false) == CaretHome::Dock;
        let dock = render(
            state,
            &DockContext::default(),
            shell,
            &THEME,
            same(cols),
            owned,
            None,
            |cell| cells.push(cell),
            |_| (),
        );
        (cells, dock)
    }

    fn context(cwd: &str, branch: &str) -> DockContext {
        DockContext {
            cwd: cwd.into(),
            branch: branch.into(),
            ..DockContext::default()
        }
    }

    /// The **column-by-column** display of the input row.
    fn text(cells: &[Cell]) -> String {
        row_text(cells, 0)
    }

    /// A row's **column-by-column** display: a column that produced no cell
    /// and an inkless cell are both a space. Laying the cells out in sequence
    /// would not be enough — the breathing room between the mark and the text
    /// (it produces no cell at all) would be invisible in that string and the
    /// column arithmetic would go untested.
    fn row_text(cells: &[Cell], row: u16) -> String {
        let on_row = || cells.iter().filter(|cell| cell.row == row);
        let width = on_row().map(|cell| cell.col + 1).max().unwrap_or(0);
        let mut line = vec![' '; usize::from(width)];
        for cell in on_row() {
            line[usize::from(cell.col)] = cell.ch.unwrap_or(' ');
        }
        line.into_iter().collect()
    }

    /// The caret's expected place in a single-row dock.
    fn caret_at(col: u16) -> Option<DockCaret> {
        Some(DockCaret { col, row: 0 })
    }

    /// Drawing with `input_rows` input rows, with selection: cells, surface,
    /// runs and the vertical window's top (the hit test's trace).
    fn draw_rows(
        state: &DockState,
        cols: u16,
        input_rows: u16,
        selection: Option<(usize, usize)>,
    ) -> (Vec<Cell>, Dock, Vec<SelectionRun>, usize) {
        draw_scrolled(state, cols, input_rows, None, selection)
    }

    /// [`draw_rows`] with a wheel-scrolled window.
    fn draw_scrolled(
        state: &DockState,
        cols: u16,
        input_rows: u16,
        scroll: Option<usize>,
        selection: Option<(usize, usize)>,
    ) -> (Vec<Cell>, Dock, Vec<SelectionRun>, usize) {
        let mut cells = Vec::new();
        let mut runs = Vec::new();
        let (dock, top, _) = render_with(
            state,
            &DockContext::default(),
            None,
            &THEME,
            same(cols),
            Some(input_rows),
            scroll,
            true,
            selection,
            None,
            None,
            &mut runs,
            &mut Clusters::default(),
            |cell| cells.push(cell),
            |_| (),
        );
        (cells, dock, runs, top)
    }

    #[test]
    fn the_sigil_leads_and_the_text_follows_it() {
        // `cursor` is in **display** space (`DockState::cursor` arrives
        // normalized): `% ` is two characters, the cursor at the end of `ls -la`, i.e. 8.
        let (cells, dock) = draw(&live("% ", "ls -la", "", 8), COLS);
        // The mark is **not a cell**: a field of the surface ([`Dock::sigil`])
        // and `bt-gpu` draws its shape. That is why the text starts with two
        // columns of blank — the mark's and the breathing room's place.
        assert_eq!(text(&cells), "  % ls -la");
        // `%lsla` + `-`: the two unhighlighted spaces draw nothing and do not
        // reach the sink (the dock counterpart of `frame()`'s skip gate).
        assert_eq!(cells.len(), 6, "spaces produced cells");
        // `PREDISPLAY`'s first character is in the text's first column: two
        // strings, one display, no blank between them.
        assert_eq!(cells[0].col, TEXT_COL);
        // The caret is `CURSOR`'s place in display space (`DockState::cursor`
        // is already normalized): `% ` is two characters, the cursor at the end of `ls -la`.
        assert_eq!(dock.caret, caret_at(TEXT_COL + 8));
    }

    #[test]
    fn the_suggestion_is_dim_and_the_typed_text_is_not() {
        // Of the three strings the mirror carries two are text the user sees,
        // one is the suggestion; the distinction is carried by **color**.
        // Reduced to one color, in a session with autosuggestions installed
        // what is typed could not be told from what is suggested.
        let (cells, _) = draw(&live("% ", "cd", " ~/src", 2), COLS);
        let typed = cells.iter().find(|cell| cell.ch == Some('c')).expect("c");
        let suggested = cells.iter().find(|cell| cell.ch == Some('~')).expect("~");
        assert_eq!(typed.fg, THEME.foreground_linear());
        assert_eq!(suggested.fg, THEME.dim_linear());
    }

    #[test]
    fn a_highlight_paints_its_range_and_nothing_else() {
        let mut state = live("", "echo hi", "", 7);
        state.highlights.push(Highlight {
            start: 0,
            end: 4,
            style: HighlightStyle {
                fg: Some(HighlightColor::Indexed(2)),
                bold: true,
                ..HighlightStyle::default()
            },
        });
        let (cells, _) = draw(&state, COLS);

        let green = THEME.indexed_linear(2);
        for cell in &cells[0..4] {
            assert_eq!(cell.fg, green, "the range was not painted");
            assert!(cell.bold);
        }
        // What is outside the range must stay at the base: the end must be
        // **excluded**. The `i` of `hi` was chosen because `h` also occurs in
        // `echo` and inside the range there — the first match would have turned
        // the test against its own claim.
        let outside = cells.iter().find(|cell| cell.ch == Some('i')).expect("i");
        assert_eq!(outside.fg, THEME.foreground_linear());
        assert!(!outside.bold);
    }

    #[test]
    fn standout_swaps_the_two_colors() {
        // zsh's `standout` is the counterpart of SGR 7 and reverse video swaps
        // the cell's two colors. If the range has no ground of its own the
        // surface's ground takes its place — otherwise the swap would be done
        // "with a colorless background" and the letter would be invisible.
        let mut state = live("", "x", "", 1);
        state.highlights.push(Highlight {
            start: 0,
            end: 1,
            style: HighlightStyle {
                standout: true,
                ..HighlightStyle::default()
            },
        });
        let (cells, _) = draw(&state, COLS);
        let x = cells.iter().find(|cell| cell.ch == Some('x')).expect("x");
        assert_eq!(x.fg, THEME.background_linear());
        assert_eq!(x.bg, Some(THEME.foreground_linear()));
    }

    #[test]
    fn the_sigil_takes_the_phase_color() {
        // The mark says the phase and the vocabulary is the same as the block
        // stripe's — now the shape is the same too
        // (`bt_atlas::RuleKind::Chevron`). Only the color crosses the boundary:
        // had the character crossed, the user's font's `>` would be drawn.
        let state = live("", "", "", 0);
        let color = |shell| draw_as(&state, shell, COLS).1.sigil.expect("no mark");
        assert_eq!(color(None), THEME.accent_linear());
        assert_eq!(
            color(Some(ShellState {
                phase: ShellPhase::Running,
                last_exit: None
            })),
            THEME.accent_linear()
        );
        assert_eq!(
            color(Some(ShellState {
                phase: ShellPhase::Finished,
                last_exit: Some(0)
            })),
            THEME.success_linear()
        );
        assert_eq!(
            color(Some(ShellState {
                phase: ShellPhase::Finished,
                last_exit: Some(1)
            })),
            THEME.error_linear()
        );
        // A `D` whose code could not be read is not counted as an error:
        // "finished but I do not know the code" is not an error (`Mark::CommandEnd`).
        assert_eq!(
            color(Some(ShellState {
                phase: ShellPhase::Finished,
                last_exit: None
            })),
            THEME.accent_linear()
        );
    }

    #[test]
    fn an_idle_or_unavailable_mirror_draws_only_the_sigil() {
        // Neither draws **text**: in `Idle` ZLE is not editing a line, in
        // `Unavailable` the fields are already empty. The caret is a separate
        // question and its answers diverge — see the two tests below.
        for status in [
            DockStatus::Idle,
            DockStatus::Unavailable(DockFault::Overflow),
        ] {
            let state = DockState {
                status,
                ..live("% ", "ls", "", 2)
            };
            let (cells, _) = draw(&state, COLS);
            assert_eq!(text(&cells), "", "{status:?} drew text");
        }
    }

    #[test]
    fn an_idle_mirror_still_keeps_the_caret_unless_a_command_runs() {
        // **A row without text does not mean a row without a caret.** At
        // startup and between two commands the mirror is `Idle`, but where the
        // user will start typing is the dock. Keeping the caret in the grid in
        // those windows would make it **jump** when the prompt arrives — the
        // observed defect was this.
        let state = DockState {
            status: DockStatus::Idle,
            ..live("% ", "ls", "", 2)
        };
        for shell in [
            None,
            Some(ShellState {
                phase: ShellPhase::Prompt,
                last_exit: None,
            }),
            Some(ShellState {
                phase: ShellPhase::Finished,
                last_exit: Some(0),
            }),
        ] {
            let (_, dock) = draw_as(&state, shell, COLS);
            assert_eq!(dock.caret, caret_at(TEXT_COL), "{shell:?} gave no caret");
        }
        // While a command runs the row's owner is the grid: `cat`'s awaited
        // input and `ssh`'s password prompt live there.
        let (_, dock) = draw_as(
            &state,
            Some(ShellState {
                phase: ShellPhase::Running,
                last_exit: None,
            }),
            COLS,
        );
        assert_eq!(dock.caret, None, "caret given while a command runs");
    }

    #[test]
    fn an_unavailable_mirror_leaves_the_caret_to_the_grid() {
        // The row we cannot show stays in the grid; its caret must stay
        // there too, or the user cannot see where they type. This is the one
        // point where it departs from `Idle` and the reason for
        // [`DockStatus`]'s existence is this distinction.
        let state = DockState {
            status: DockStatus::Unavailable(DockFault::Overflow),
            ..live("% ", "ls", "", 2)
        };
        let (_, dock) = draw_as(&state, None, COLS);
        assert_eq!(dock.caret, None);
    }

    #[test]
    fn a_multiline_mirror_draws_its_rows_and_keeps_the_caret() {
        // **This mirror was once `Multiline` and the dock drew nothing**
        // (both the row and the caret were in the grid; flattening to a single
        // row squashed the text with invisible blanks). Now a line break
        // breaks the row: each logical row on its own visual row, from the
        // text's column, the caret on its own row.
        let state = live("", "echo a\necho b", "", 9);
        assert_eq!(needed_rows(&state, COLS), 2);
        let (cells, dock, _, top) = draw_rows(&state, COLS, 2, None);
        assert_eq!(top, 0);
        assert_eq!(row_text(&cells, 0), "  echo a");
        assert_eq!(row_text(&cells, 1), "  echo b");
        assert_eq!(
            dock.caret,
            Some(DockCaret {
                col: TEXT_COL + 2,
                row: 1
            })
        );
    }

    /// **`PREBUFFER` above the editable rows**: the `for` row ZLE
    /// accepted is in the dock, at the same indent and in the same color; the
    /// mark on the row where the command started, the caret on `BUFFER`'s row.
    /// A selection extends over it too (selectable, copyable), and the hit test
    /// lands there as well.
    #[test]
    fn the_prebuffer_rows_sit_above_the_editable_rows() {
        let state = DockState {
            prebuffer: "for i in 1 2; do\n".into(),
            ..live("", "echo $i", "", 7)
        };
        assert_eq!(needed_rows(&state, COLS), 2);
        let (cells, dock, _, top) = draw_rows(&state, COLS, 2, None);
        assert_eq!(top, 0);
        assert_eq!(row_text(&cells, 0), "  for i in 1 2; do");
        assert_eq!(row_text(&cells, 1), "  echo $i");
        assert_eq!(
            dock.caret,
            Some(DockCaret {
                col: TEXT_COL + 7,
                row: 1
            })
        );
        assert!(dock.sigil.is_some(), "mark on the command's first row");
        let fg = THEME.foreground_linear();
        assert!(
            cells
                .iter()
                .filter(|cell| cell.ch.is_some())
                .all(|cell| cell.fg == fg),
            "PREBUFFER must be drawn in the text's color"
        );

        // Hit: a point falling on `PREBUFFER` is at the start of the selectable
        // text, one falling on `BUFFER` is ahead by `PREBUFFER`'s length.
        let shift = prebuffer_chars(&state);
        assert_eq!(shift, 17);
        let at = |row, col| hit(&state, 0, COLS, row, col, CellHalf::Left).map(|p| p.index);
        assert_eq!(at(0, TEXT_COL + 4), Some(4));
        assert_eq!(at(1, TEXT_COL), Some(shift));
        assert_eq!(at(1, TEXT_COL + 3), Some(shift + 3));
        assert_eq!(selectable(&state), "for i in 1 2; do\necho $i");

        // The selection spans two rows: from the end of `PREBUFFER` to the start of `BUFFER`.
        let (_, _, runs, _) = draw_rows(&state, COLS, 2, Some((10, shift + 4)));
        assert_eq!(
            runs,
            [
                run(0, TEXT_COL + 11, TEXT_COL + 15),
                run(1, TEXT_COL, TEXT_COL + 3)
            ]
        );
    }

    #[test]
    fn the_context_row_sits_under_the_input_block() {
        // **The context row is under the input block**: its row
        // number is the very number of input rows to draw, not a fixed `1`.
        // `bt-gpu` places it at the bottom of the band from the same number;
        // were they to diverge here the context row would land in the place of
        // an input row.
        let mut cells = Vec::new();
        render_with(
            &live("", "ls", "", 2),
            &context("/tmp/x", "main"),
            None,
            &THEME,
            same(COLS),
            Some(3),
            None,
            true,
            None,
            None,
            None,
            &mut Vec::new(),
            &mut Clusters::default(),
            |cell| cells.push(cell),
            |_| (),
        );
        assert_eq!(row_text(&cells, 3), "/tmp/x | main");
        assert_eq!(
            row_text(&cells, CONTEXT_ROW),
            "",
            "the context stayed on the old row"
        );
        assert_eq!(
            row_text(&cells, 0).trim(),
            "ls",
            "the input row moved from its place"
        );
    }

    #[test]
    fn the_budget_is_a_share_of_the_grid_and_keeps_one_row() {
        // The ceiling is a share of the grid's rows, rounded
        // down; the dock's input row never disappears: a zero share and a
        // one-row grid both give one row.
        let half = DockBudget {
            share: 0.5,
            cols: 80,
        };
        assert_eq!(half.fit(1, 8), 1);
        assert_eq!(half.fit(9, 8), 4, "the ceiling is half the grid");
        assert_eq!(half.fit(9, 9), 4, "half a row must round down");
        assert_eq!(half.fit(3, 1), 1);
        assert_eq!(half.fit(0, 10), 1);
        let none = DockBudget { share: 0.0, ..half };
        assert_eq!(none.fit(3, 8), 1);
    }

    #[test]
    fn a_long_line_wraps_under_the_text_column() {
        // The counterpart of the old left windowing: a row that
        // overflows is now **wrapped** and the whole command is visible.
        // Continuation rows start from the text's column (hanging indent), the
        // caret at its own column on the wrapped row.
        let cols = 10;
        let buffer: String = ('a'..='z').collect();
        let state = live("", &buffer, "", 26);
        assert_eq!(needed_rows(&state, cols), 4);
        let (cells, dock, _, top) = draw_rows(&state, cols, 4, None);
        assert_eq!(top, 0);
        let rows: Vec<String> = (0..4).map(|row| row_text(&cells, row)).collect();
        assert_eq!(rows, ["  abcdefgh", "  ijklmnop", "  qrstuvwx", "  yz"]);
        assert_eq!(
            dock.caret,
            Some(DockCaret {
                col: TEXT_COL + 2,
                row: 3
            })
        );
        assert!(dock.sigil.is_some(), "first row visible, so is the mark");

        // With the caret back at the start all rows are still in place: no window, wrapping.
        let (cells, dock, _, _) = draw_rows(&live("", &buffer, "", 0), cols, 4, None);
        assert_eq!(row_text(&cells, 2), "  qrstuvwx");
        assert_eq!(dock.caret, caret_at(TEXT_COL));
    }

    /// **An input past the ceiling opens a vertical window**:
    /// the smallest shift that keeps the caret's row visible, stateless — the
    /// vertical twin of the old horizontal window. A row outside the window
    /// never reaches the sink (it would land on the context row); if the first
    /// row is outside, so is the mark.
    #[test]
    fn a_line_past_the_ceiling_keeps_the_caret_row_in_a_vertical_window() {
        let cols = 10;
        let buffer: String = ('a'..='z').collect();
        // Caret at the end: the last two of the four rows.
        let (cells, dock, _, top) = draw_rows(&live("", &buffer, "", 26), cols, 2, None);
        assert_eq!(top, 2);
        assert_eq!(row_text(&cells, 0), "  qrstuvwx");
        assert_eq!(row_text(&cells, 1), "  yz");
        assert!(
            cells.iter().all(|cell| cell.row < 2),
            "rows outside the window were drawn: {cells:?}"
        );
        assert_eq!(
            dock.caret,
            Some(DockCaret {
                col: TEXT_COL + 2,
                row: 1
            })
        );
        assert_eq!(dock.sigil, None, "first row off screen, mark must go");
        // The window's two rows are the hit test's two rows too: `top` is the trace.
        let state = live("", &buffer, "", 26);
        assert_eq!(
            hit(&state, top, cols, 0, TEXT_COL, CellHalf::Left),
            Some(point(16, CellHalf::Left)),
            "the window's first row is `q`"
        );
        // Caret on the second row: the window stays up, the mark in place.
        let (cells, dock, _, top) = draw_rows(&live("", &buffer, "", 9), cols, 2, None);
        assert_eq!(top, 0);
        assert_eq!(row_text(&cells, 1), "  ijklmnop");
        assert!(dock.sigil.is_some());
    }

    /// **The window top chosen with the wheel**: in an input
    /// past the ceiling the mouse also reaches the rows other than the
    /// caret's. The top is clamped to the row count; if the caret is outside
    /// the window it is not drawn, the mark comes back if the first row is on
    /// screen.
    #[test]
    fn a_scrolled_window_shows_the_rows_the_wheel_chose() {
        let cols = 10;
        let buffer: String = ('a'..='z').collect();
        let state = live("", &buffer, "", 26);
        let (cells, dock, _, top) = draw_scrolled(&state, cols, 2, Some(0), None);
        assert_eq!(top, 0);
        assert_eq!(row_text(&cells, 0), "  abcdefgh");
        assert_eq!(row_text(&cells, 1), "  ijklmnop");
        assert_eq!(dock.caret, None, "the caret is outside the window");
        assert!(dock.sigil.is_some(), "first row visible, so is the mark");
        // An overflowing top is clamped to the last window: the last two of the four rows.
        let (cells, dock, _, top) = draw_scrolled(&state, cols, 2, Some(9), None);
        assert_eq!(top, 2);
        assert_eq!(row_text(&cells, 1), "  yz");
        assert_eq!(
            dock.caret,
            Some(DockCaret {
                col: TEXT_COL + 2,
                row: 1
            })
        );
    }

    /// `frame()`'s row count comes from the dock's own layout: a non-`Live`
    /// mirror and a width the text does not fit in give one row; a caret
    /// behind a completely full row opens a row (the same rule as zsh's grid,
    /// [`layout`]); the suggestion does not grow the band (its length changes
    /// on every key).
    #[test]
    fn the_needed_rows_come_from_the_dock_layout() {
        let cols = TEXT_COL + 4;
        assert_eq!(needed_rows(&live("", "abc", "", 3), cols), 1);
        assert_eq!(needed_rows(&live("", "abcd", "", 0), cols), 1);
        assert_eq!(
            needed_rows(&live("", "abcd", "", 4), cols),
            2,
            "the caret rule"
        );
        // The suggestion does not grow the band: the part that wraps is clipped.
        assert_eq!(needed_rows(&live("", "ab", "cdef", 2), cols), 1, "hint");
        assert_eq!(needed_rows(&live("", "abcde", "fghijk", 5), cols), 2);
        let (cells, _, _, _) = draw_rows(&live("", "ab", "cdef", 2), cols, 1, None);
        assert_eq!(row_text(&cells, 0), "  abcd", "the fitting part is drawn");
        assert!(cells.iter().all(|cell| cell.row == 0), "{cells:?}");
        assert_eq!(needed_rows(&live("", "abcdefghij", "", 0), TEXT_COL), 1);
        let idle = DockState {
            status: DockStatus::Idle,
            ..live("", "abcdefghij", "", 0)
        };
        assert_eq!(needed_rows(&idle, cols), 1);
    }

    #[test]
    fn a_window_too_narrow_for_text_still_answers() {
        // Degenerate widths: nothing at zero columns, only the mark at a width
        // where the mark fits but the text does not. Neither is a panic — these
        // widths really do arrive while the window is being minimized
        // (`split_into_grid`).
        let (cells, dock) = draw(&live("", "ls", "", 2), 0);
        assert!(cells.is_empty());
        assert_eq!(dock.caret, None);

        let (cells, dock) = draw(&live("", "ls", "", 2), TEXT_COL);
        assert_eq!(text(&cells), "");
        assert_eq!(dock.caret, None);
    }

    #[test]
    fn the_shell_prompt_is_as_wide_as_the_dock_indent() {
        // **Two sources, one number.** The dock's text starts `TEXT_COL`
        // columns after the mark; what gives the same alignment in the grid is
        // the zsh script's prompt, because the command really does start that
        // far in there. The constant cannot be shared — one is
        // Rust, one is shell — but their divergence would be **silent**: the
        // grid and the dock would start from different columns, nobody gets
        // angry.
        //
        // The criterion is the number of spaces inside the quotes. The spaces
        // must be outside `%{…%}` (zsh must count them); were they put inside
        // the width would drop to zero and the mark would cover the command's
        // first letter.
        let script = include_str!("../../../assets/shell/zsh/bateri.zsh");
        let line = script
            .lines()
            .find(|line| line.contains("__bateri_ps1="))
            .expect("no `__bateri_ps1` assignment in the script");
        let spaces = format!("'{}'", " ".repeat(usize::from(TEXT_COL)));
        assert!(
            line.contains(&spaces),
            "the prompt width diverged from `TEXT_COL` ({TEXT_COL}): {line}"
        );
        // Not one more either: `contains` alone would say "at least".
        let wider = format!("'{}'", " ".repeat(usize::from(TEXT_COL) + 1));
        assert!(
            !line.contains(&wider),
            "the prompt is one column wider: {line}"
        );
    }

    #[test]
    fn the_context_line_sits_under_the_input_and_is_dim() {
        let (cells, _) = draw_with(&live("", "ls", "", 2), &context("/tmp/x", "main"), COLS);
        assert_eq!(text(&cells), "  ls");
        // Path, separator, branch — side by side and at the dock's **left
        // edge**, i.e. aligned with the `>` mark. Aligned with the input text
        // the context would look indented for no reason (see [`CONTEXT_COL`]).
        assert_eq!(row_text(&cells, 1), "/tmp/x | main");
        // **There are two tiers inside the row.** The information sought is
        // "which folder am I in", i.e. the path's last component; the parent
        // directories are the context that places it and recede. The branch is
        // sought information too, so in the same tone as the one standing out.
        // The separator is a division mark, not content.
        let tone = |col: u16| {
            cells
                .iter()
                .find(|cell| cell.row == 1 && cell.col == col)
                .unwrap_or_else(|| panic!("no column {col} on the context row"))
                .fg
        };
        let normal = THEME.dim_linear();
        let quiet = THEME.quiet_linear();
        assert_ne!(
            normal, quiet,
            "the two tiers fell to the same color: the distinction is invisible"
        );
        for col in 0..=4 {
            assert_eq!(tone(col), quiet, "`/tmp/` stood out (column {col})");
        }
        assert_eq!(tone(5), normal, "the active folder (`x`) receded");
        assert_eq!(tone(7), quiet, "the separator was drawn like content");
        for col in 9..=12 {
            assert_eq!(tone(col), normal, "the branch receded (column {col})");
        }
    }

    #[test]
    fn a_rootless_or_root_path_is_all_foreground() {
        // Two degenerate states and in both the "last component" distinction is
        // meaningless: at the root (`/`) there is no parent directory to make
        // the distinction, nor in a path without slashes. The wrong side is the
        // **safe** side: all of it stands out. The opposite choice (all dim)
        // would hide the one piece of information the user looks for.
        for path in ["/", "tmp"] {
            let (cells, _) = draw_with(&live("", "", "", 0), &context(path, ""), COLS);
            for cell in cells.iter().filter(|cell| cell.row == 1) {
                assert_eq!(cell.fg, THEME.dim_linear(), "{path}: {cell:?}");
            }
        }
    }

    #[test]
    fn the_context_line_lives_even_when_the_mirror_does_not() {
        // The context does not depend on the mirror's lifetime: while a command
        // runs ZLE leaves the line (`Idle`) but the directory is still correct
        // and the user looks at it.
        for status in [
            DockStatus::Idle,
            DockStatus::Unavailable(DockFault::Overflow),
        ] {
            let state = DockState {
                status,
                ..live("", "ls", "", 2)
            };
            let (cells, _) = draw_with(&state, &context("/tmp/x", "main"), COLS);
            assert_eq!(text(&cells), "", "{status:?} drew text");
            assert_eq!(row_text(&cells, 1), "/tmp/x | main", "{status:?}");
        }
    }

    #[test]
    fn a_missing_branch_takes_the_separator_with_it() {
        // In a directory that is not a repo only the path; a dangling separator
        // would say "the branch could not be read" and that would be wrong.
        let state = live("", "", "", 0);
        let (cells, _) = draw_with(&state, &context("/tmp/x", ""), COLS);
        assert_eq!(row_text(&cells, 1), "/tmp/x");
        // Symmetric: with no path (OSC 7 has not arrived yet) there is no separator either.
        let (cells, _) = draw_with(&state, &context("", "main"), COLS);
        assert_eq!(row_text(&cells, 1), "main");
        // If neither exists the row is not born at all.
        let (cells, _) = draw_with(&state, &DockContext::default(), COLS);
        assert_eq!(row_text(&cells, 1), "");
    }

    fn remote(host: &str, remote_cwd: &str) -> DockContext {
        DockContext {
            // The local path and branch are **filled**: the remote form must not show them at all.
            cwd: "/Users/me/proj".into(),
            branch: "main".into(),
            remote: Some(RemoteTarget::ssh(host)),
            remote_mark: HostMark::None,
            remote_cwd: remote_cwd.into(),
            reconnect: None,
            transfer: None,
            stats: None,
            sign_in: None,
            remote_setup: None,
            program: None,
        }
    }

    /// A load sample: `history` oldest first.
    fn load(form: StatsForm, cpu: Option<u8>, mem: u8, disk: u8, history: &[u8]) -> RemoteStats {
        let mut stats = RemoteStats {
            form,
            cpu,
            mem,
            disk,
            ..RemoteStats::default()
        };
        stats.history[..history.len()].copy_from_slice(history);
        stats.len = history.len() as u8;
        stats
    }

    /// The design's calm sample: `cpu ▂▃▅▇▅▃▂▁ 23%  mem 61%`, disk at 54.
    fn calm(form: StatsForm) -> RemoteStats {
        load(form, Some(23), 61, 54, &[1, 2, 4, 6, 4, 2, 1, 0])
    }

    /// `⇄ prod  /srv/app` with the indicator.
    fn loaded(stats: RemoteStats) -> DockContext {
        DockContext {
            stats: Some(stats),
            ..remote("prod", "/srv/app")
        }
    }

    /// The context row as drawn at `cols`.
    fn load_row(context: &DockContext, cols: u16) -> (String, Vec<Cell>) {
        let (cells, _) = draw_with(&live("", "", "", 0), context, cols);
        let row = row_text(&cells, 1);
        (row, cells)
    }

    #[test]
    fn the_load_forms_draw_their_text_right_aligned_and_dim() {
        // Three forms, right-aligned; label, sparkline and the
        // numbers below their threshold dim — the brief, not the draft's
        // foreground.
        let dim = Some(THEME.dim_linear());
        let (row, cells) = load_row(&loaded(calm(StatsForm::Sparkline)), 80);
        assert_eq!(
            row,
            format!("{:<55}cpu ▂▃▅▇▅▃▂▁ 23%  mem 61%", "⇄ prod  /srv/app")
        );
        for col in [55, 59, 66, 68, 73, 77, 79] {
            assert_eq!(color_at(&cells, 1, col), dim, "col {col}");
        }
        let (row, _) = load_row(&loaded(calm(StatsForm::Numbers)), 80);
        assert_eq!(row, format!("{:<64}cpu 23%  mem 61%", "⇄ prod  /srv/app"));
        // Alerts with nothing past a threshold: only the calm dot, `success`.
        let (row, cells) = load_row(&loaded(calm(StatsForm::Alerts)), 80);
        assert_eq!(row, format!("{:<79}●", "⇄ prod  /srv/app"));
        assert_eq!(color_at(&cells, 1, 79), Some(THEME.success_linear()));
        // The host keeps the mark's color.
        assert_eq!(color_at(&cells, 1, 2), Some(THEME.info_linear()));
    }

    #[test]
    fn a_value_past_its_threshold_takes_its_color() {
        let (warning, error, dim) = (
            Some(THEME.warning_linear()),
            Some(THEME.error_linear()),
            Some(THEME.dim_linear()),
        );
        // Disk joins only from 85% on: 84 no, 85 yes, in `warning`.
        let (row, _) = load_row(&loaded(load(StatsForm::Numbers, Some(23), 61, 84, &[])), 60);
        assert!(row.ends_with("cpu 23%  mem 61%"), "{row}");
        let (row, cells) = load_row(&loaded(load(StatsForm::Numbers, Some(23), 61, 85, &[])), 60);
        assert!(row.ends_with("cpu 23%  mem 61%  disk 85%"), "{row}");
        assert_eq!(color_at(&cells, 1, 57), warning, "disk's number");
        assert_eq!(color_at(&cells, 1, 52), dim, "disk's label");
        // Critical: `▲` glued to the number, both `error`; the label stays dim.
        let (row, cells) = load_row(&loaded(load(StatsForm::Numbers, Some(95), 85, 0, &[])), 60);
        assert!(row.ends_with("cpu ▲95%  mem 85%"), "{row}");
        let cpu = row.chars().count() - "cpu ▲95%  mem 85%".chars().count();
        let cpu = cpu as u16;
        assert_eq!(color_at(&cells, 1, cpu), dim, "label");
        assert_eq!(color_at(&cells, 1, cpu + 4), error, "▲");
        assert_eq!(color_at(&cells, 1, cpu + 5), error, "number");
        assert_eq!(
            color_at(&cells, 1, cpu + 14),
            warning,
            "mem 85% is a warning"
        );
        // Thresholds are reached **at** their value.
        assert_eq!(StatsMetric::Cpu.level(69), StatsLevel::Normal);
        assert_eq!(StatsMetric::Cpu.level(70), StatsLevel::Warning);
        assert_eq!(StatsMetric::Cpu.level(90), StatsLevel::Critical);
        assert_eq!(StatsMetric::Mem.level(92), StatsLevel::Critical);
        assert_eq!(StatsMetric::Disk.level(95), StatsLevel::Critical);
        // Alerts: only the values past their threshold.
        let (row, _) = load_row(&loaded(load(StatsForm::Alerts, Some(75), 93, 40, &[])), 60);
        assert!(row.ends_with("cpu 75%  mem ▲93%"), "{row}");
    }

    #[test]
    fn the_sparkline_keeps_eight_columns_with_few_samples() {
        // Fewer than eight samples: blank on the left, the same width — the
        // path's budget must not move with every sample.
        let few = loaded(load(StatsForm::Sparkline, Some(23), 61, 0, &[3, 5]));
        let (row, _) = load_row(&few, 80);
        assert_eq!(
            row,
            format!("{:<55}cpu       ▄▆ 23%  mem 61%", "⇄ prod  /srv/app")
        );
        assert_eq!(
            stats_span(&few, 80),
            stats_span(&loaded(calm(StatsForm::Sparkline)), 80)
        );
        // No CPU yet (the first sample): no made-up number, memory alone.
        let first = loaded(load(StatsForm::Sparkline, None, 61, 0, &[]));
        let (row, _) = load_row(&first, 80);
        assert_eq!(row, format!("{:<73}mem 61%", "⇄ prod  /srv/app"));
    }

    #[test]
    fn a_narrowing_row_drops_the_indicator_rung_by_rung() {
        // Full → numbers → worst → none, each with the whole path
        // and two columns before the indicator.
        let context = loaded(calm(StatsForm::Sparkline));
        let left = "⇄ prod  /srv/app";
        for (cols, gauge) in [
            (43, "cpu ▂▃▅▇▅▃▂▁ 23%  mem 61%"),
            (42, "cpu 23%  mem 61%"),
            (34, "cpu 23%  mem 61%"),
            (33, "mem 61%"),
            (25, "mem 61%"),
        ] {
            let (row, _) = load_row(&context, cols);
            let start = usize::from(cols) - gauge.chars().count();
            assert_eq!(row, format!("{left:<start$}{gauge}"), "{cols} columns");
        }
        for cols in [24, 20, 16] {
            let (row, _) = load_row(&context, cols);
            assert_eq!(
                row, left,
                "{cols} columns: the indicator drops, the path stays"
            );
            assert_eq!(stats_span(&context, cols), None);
        }
        // Below that the path is shortened as today.
        let (row, _) = load_row(&context, 12);
        assert_eq!(row, "⇄ prod  …app");
        // A tie goes to the first value in `cpu, mem, disk` order.
        let tie = loaded(load(StatsForm::Numbers, Some(50), 50, 0, &[]));
        let (row, _) = load_row(&tie, 25);
        assert!(row.ends_with("cpu 50%"), "{row}");
    }

    #[test]
    fn an_alarm_comes_before_the_path() {
        // The worst value past its threshold stays and the path shortens from
        // the left with `…`; the host never does.
        let context = loaded(load(StatsForm::Sparkline, Some(23), 95, 0, &[]));
        let (row, cells) = load_row(&context, 20);
        assert_eq!(row, "⇄ prod  …p  mem ▲95%");
        assert_eq!(color_at(&cells, 1, 17), Some(THEME.error_linear()));
        let (row, _) = load_row(&context, 19);
        assert_eq!(
            row, "⇄ prod  …  mem ▲95%",
            "a one-column path budget is the mark alone"
        );
        // `⇄ prod` + gap + the alarm no longer fit: the alarm drops too.
        let (row, _) = load_row(&context, 17);
        assert_eq!(row, "⇄ prod  /srv/app");
        // A calm worst value never pushes the path.
        let calm = loaded(calm(StatsForm::Sparkline));
        let (row, _) = load_row(&calm, 20);
        assert_eq!(row, "⇄ prod  /srv/app");
    }

    #[test]
    fn the_host_is_never_shortened_by_the_load() {
        let context = loaded(load(StatsForm::Sparkline, Some(99), 99, 99, &[7; 8]));
        for cols in 0..=70 {
            let (row, _) = load_row(&context, cols);
            assert!(
                row.is_empty() || row == "⇄" || row.starts_with("⇄ prod"),
                "{cols} columns: {row:?}"
            );
        }
        let (row, _) = load_row(&context, 5);
        assert_eq!(row, "⇄");
        assert_eq!(stats_span(&context, 5), None);
    }

    #[test]
    fn a_transfer_hides_the_load() {
        // The upload row takes the context row's place; the
        // indicator is neither drawn nor hit.
        let context = DockContext {
            stats: Some(calm(StatsForm::Sparkline)),
            ..uploading("↑ a.tar", 1, Some(2_500))
        };
        let (row, _) = load_row(&context, 80);
        assert!(!row.contains("cpu") && !row.contains("mem"), "{row}");
        assert!((0..80).all(|col| !stats_at(&context, 80, col)));
        assert_eq!(stats_span(&context, 80), None);
        // Locally there is no indicator even with a value.
        let local = DockContext {
            remote: None,
            ..loaded(calm(StatsForm::Sparkline))
        };
        assert_eq!(stats_span(&local, 80), None);
    }

    #[test]
    fn the_load_span_is_the_drawn_cells() {
        // Drawing, the mouse and the popover's anchor read one layout.
        for stats in [
            calm(StatsForm::Sparkline),
            calm(StatsForm::Numbers),
            calm(StatsForm::Alerts),
            load(StatsForm::Sparkline, Some(23), 95, 0, &[]),
            load(StatsForm::Sparkline, Some(23), 61, 0, &[3]),
        ] {
            let context = loaded(stats);
            for cols in 0..=80 {
                let (row, _) = load_row(&context, cols);
                let Some((start, end)) = stats_span(&context, cols) else {
                    assert!(!row.contains('%') && !row.contains('●'), "{cols}: {row:?}");
                    assert!((0..cols).all(|col| !stats_at(&context, cols, col)));
                    continue;
                };
                assert_eq!(end, cols, "right-aligned");
                let chars: Vec<char> = row.chars().collect();
                let gauge: String = chars[usize::from(start)..].iter().collect();
                let expected = gauge.trim_start();
                assert!(
                    expected.starts_with("cpu")
                        || expected.starts_with("mem")
                        || expected.starts_with('●'),
                    "{cols}: {gauge:?}"
                );
                assert_eq!(
                    chars.len(),
                    usize::from(end),
                    "{cols}: the last cell ends the span"
                );
                assert_eq!(chars[usize::from(start) - 1], ' ', "{cols}: a gap before");
                for col in 0..cols {
                    assert_eq!(
                        stats_at(&context, cols, col),
                        (start..end).contains(&col),
                        "{cols} columns, col {col}"
                    );
                }
            }
        }
    }

    /// The Sign In… button takes the indicator's place, right-aligned
    /// in the upload buttons' drawing; the click's range is the drawn fill.
    #[test]
    fn the_sign_in_button_is_drawn_where_it_is_hit() {
        let context = DockContext {
            stats: Some(calm(StatsForm::Sparkline)),
            sign_in: Some(crate::SignIn::default()),
            ..remote("prod", "/srv/app")
        };
        let (cells, dock) = draw_with(&live("", "", "", 0), &context, 60);
        let row = row_text(&cells, 1);
        assert_eq!(row, format!("{:<51}Sign In\u{2026}", "⇄ prod  /srv/app"));
        assert!(!row.contains("cpu"), "no indicator without a login: {row}");
        assert_eq!(
            dock.buttons,
            [
                None,
                Some(DockButton {
                    start: 50,
                    end: 60,
                    color: THEME.info_linear(),
                    state: ButtonState::Idle,
                })
            ]
        );
        assert_eq!(color_at(&cells, 1, 51), Some(THEME.foreground_linear()));
        assert_eq!(sign_in_span(&context, 60), Some((50, 60)));
        assert_eq!(stats_span(&context, 60), None);
        // The hover darkens the fill only.
        let hovered = DockContext {
            sign_in: Some(crate::SignIn { hover: true }),
            ..context.clone()
        };
        let (_, dock) = draw_with(&live("", "", "", 0), &hovered, 60);
        assert_eq!(dock.buttons[1].map(|b| b.state), Some(ButtonState::Hover));
        // At every width the hit range is the drawn button, and the path gives
        // way before it does.
        for cols in 0..=60 {
            let (cells, dock) = draw_with(&live("", "", "", 0), &context, cols);
            let drawn = dock.buttons[1].map(|b| (b.start, b.end));
            assert_eq!(sign_in_span(&context, cols), drawn, "{cols} columns");
            if let Some((start, _)) = drawn {
                let row = row_text(&cells, 1);
                assert!(row.starts_with("⇄ prod"), "{cols}: {row:?}");
                assert!(row.ends_with("Sign In\u{2026}"), "{cols}: {row:?}");
                assert_eq!(row.chars().nth(usize::from(start) - 1), Some(' '));
            }
        }
        // The upload row wins; locally and without the flag there is none.
        let uploading = DockContext {
            sign_in: Some(crate::SignIn::default()),
            ..uploading("↑ a.tar", 1, Some(2_500))
        };
        assert_eq!(sign_in_span(&uploading, 60), None);
        assert_eq!(sign_in_span(&remote("prod", "/srv/app"), 60), None);
        let local = DockContext {
            remote: None,
            ..context
        };
        assert_eq!(sign_in_span(&local, 60), None);
    }

    #[test]
    fn the_stats_glyphs_are_the_ones_the_atlas_checks() {
        // `bt-atlas` asks by hand about the indicator's font glyphs in the small
        // class (`the_stats_glyphs_have_no_box_in_the_small_class`); every
        // non-ASCII character any rung can draw is either there or a
        // procedural sparkline block.
        assert_eq!(STATS_GLYPHS, ['▲', '●']);
        let samples = [
            calm(StatsForm::Sparkline),
            load(
                StatsForm::Sparkline,
                Some(100),
                100,
                100,
                &[0, 1, 2, 3, 4, 5, 6, 7],
            ),
            load(StatsForm::Alerts, Some(1), 1, 1, &[]),
            load(StatsForm::Alerts, Some(95), 95, 95, &[]),
        ];
        let mut seen = Vec::new();
        for stats in samples {
            for step in [
                GaugeStep::Spark,
                GaugeStep::Numbers,
                GaugeStep::Alerts,
                GaugeStep::Worst,
            ] {
                for &(ch, _) in gauge(&stats, step).cells() {
                    if !ch.is_ascii() {
                        assert!(
                            STATS_GLYPHS.contains(&ch) || ('\u{2581}'..='\u{2588}').contains(&ch),
                            "{ch:?}"
                        );
                        seen.push(ch);
                    }
                }
            }
        }
        for glyph in STATS_GLYPHS {
            assert!(seen.contains(&glyph), "{glyph} is never drawn");
        }
        // The widest rung fits the fixed buffer.
        let widest = gauge(&samples[1], GaugeStep::Spark);
        assert_eq!(widest.width(), 41);
        assert!(widest.width() <= GAUGE_MAX);
    }

    fn uploading(body: &str, items: u16, progress: Option<u16>) -> DockContext {
        DockContext {
            transfer: Some(Transfer {
                host: "prod".into(),
                mark: HostMark::Production,
                body: body.into(),
                controls: TransferControls {
                    items,
                    ..TransferControls::default()
                },
                progress,
                ..Transfer::default()
            }),
            ..remote("prod", "/srv")
        }
    }

    fn with_controls(
        mut context: DockContext,
        change: impl FnOnce(&mut TransferControls),
    ) -> DockContext {
        if let Some(transfer) = &mut context.transfer {
            change(&mut transfer.controls);
        }
        context
    }

    fn color_at(cells: &[Cell], row: u16, col: u16) -> Option<LinearRgba> {
        cells
            .iter()
            .find(|cell| cell.row == row && cell.col == col)
            .map(|cell| cell.fg)
    }

    #[test]
    fn an_upload_takes_over_the_context_row_and_the_edge() {
        // The `⇄ host` prefix and color are kept,
        // the status next to it; the top line is a bar — the filled part
        // `info`, the empty track in the mark's color. With a single
        // item only `Cancel ⌘.`, right-aligned.
        let state = live("", "", "", 0);
        let context = uploading("↑ a.tar", 1, Some(2_500));
        let (cells, dock) = draw_with(&state, &context, COLS);
        assert_eq!(
            row_text(&cells, 1),
            format!("{:<30}Cancel ⌘.", "⇄ prod  ↑ a.tar")
        );
        assert_eq!(color_at(&cells, 1, 2), Some(THEME.error_linear()), "host");
        assert_eq!(color_at(&cells, 1, 8), Some(THEME.dim_linear()), "body");
        assert_eq!(
            color_at(&cells, 1, 30),
            Some(THEME.foreground_linear()),
            "label in the foreground"
        );
        assert_eq!(
            color_at(&cells, 1, 37),
            Some(THEME.dim_linear()),
            "hint dim"
        );
        assert_eq!(
            dock.edge,
            THEME.info_linear(),
            "the filled part is always info"
        );
        assert_eq!(
            dock.track,
            THEME.error_linear(),
            "the empty track in the mark's color"
        );
        assert_eq!(dock.progress, Some(2_500));
        assert_eq!(
            dock.buttons,
            [
                None,
                Some(DockButton {
                    start: 29,
                    end: 40,
                    color: THEME.error_linear(),
                    state: ButtonState::Idle,
                })
            ]
        );
        // The hit area is the whole of the fill: inner padding included, the edge columns too.
        let transfer = context.transfer.as_ref().unwrap();
        assert_eq!(transfer_button_at(transfer, COLS, 28), None);
        assert_eq!(
            transfer_button_at(transfer, COLS, 29),
            Some(TransferAction::Cancel)
        );
        assert_eq!(
            transfer_button_at(transfer, COLS, 39),
            Some(TransferAction::Cancel)
        );
        assert_eq!(transfer_button_at(transfer, COLS, 40), None);
        // Also after the remote state is over (ssh closed) the row keeps its own
        // host; the result row has no buttons.
        let closed = DockContext {
            remote: None,
            ..uploading("Connection closed", 0, None)
        };
        let (cells, dock) = draw_with(&state, &closed, COLS);
        assert_eq!(row_text(&cells, 1), "⇄ prod  Connection closed");
        assert_eq!(dock.edge, THEME.error_linear());
        assert_eq!(dock.progress, None);
        assert_eq!(dock.buttons, [None; 2]);
        let transfer = closed.transfer.as_ref().unwrap();
        assert!((0..COLS).all(|col| transfer_button_at(transfer, COLS, col).is_none()));
    }

    #[test]
    fn no_band_prints_no_cell_and_lays_out_no_button() {
        // A program reading the keyboard itself: the band is gone, so neither
        // the context row nor a button on it may exist — a row the drawing does
        // not show must not be hit. A context that would draw both (an upload
        // row with a button, a live mirror with a caret) proves the arm.
        let state = live("", "echo hi", "", 7);
        let context = uploading("↑ a.tar", 1, Some(2_500));
        let draw = |band: Option<u16>| {
            let mut cells = Vec::new();
            let mut runs = Vec::new();
            let mut edits = Vec::new();
            let (dock, top, rows) = render_with(
                &state,
                &context,
                None,
                &THEME,
                same(COLS),
                band,
                None,
                true,
                Some((0, 4)),
                None,
                Some(&Change::Insert { start: 0, end: 1 }),
                &mut runs,
                &mut Clusters::default(),
                |cell| cells.push(cell),
                |edit| edits.push(edit),
            );
            (cells, dock, top, rows, runs, edits)
        };
        let (cells, dock, top, rows, runs, edits) = draw(None);
        assert!(cells.is_empty(), "{cells:?}");
        assert_eq!(dock.buttons, [None; 2]);
        assert_eq!(dock.sigil, None);
        assert_eq!(dock.caret, None);
        assert_eq!((top, rows), (0, 0), "nothing to click or scroll");
        assert!(runs.is_empty(), "no selection on a row that is not drawn");
        assert_eq!(edits, [DockEdit::Reset], "in-flight effects end");
        // The same context with the context row alone draws both.
        let (cells, dock, ..) = draw(Some(0));
        assert!(!cells.is_empty());
        assert!(dock.buttons.iter().any(Option::is_some));
    }

    #[test]
    fn a_queue_gets_a_list_button_and_the_pointer_state() {
        let state = live("", "", "", 0);
        let context = uploading("↑ 1 of 2 · a.tar", 2, None);
        let (cells, dock) = draw_with(&state, &context, 64);
        assert_eq!(
            row_text(&cells, 1),
            format!(
                "{:<29}{:<21}Cancel all ⌘.",
                "⇄ prod  ↑ 1 of 2 · a.tar", "Show transfers (2)"
            )
        );
        assert_eq!(
            dock.buttons.map(|b| b.map(|b| (b.start, b.end))),
            [Some((28, 48)), Some((49, 64))]
        );
        let transfer = context.transfer.as_ref().unwrap();
        assert_eq!(
            transfer_button_at(transfer, 64, 28),
            Some(TransferAction::List)
        );
        assert_eq!(
            transfer_button_at(transfer, 64, 47),
            Some(TransferAction::List)
        );
        assert_eq!(
            transfer_button_at(transfer, 64, 48),
            None,
            "the gap between the two buttons"
        );
        assert_eq!(
            transfer_button_at(transfer, 64, 49),
            Some(TransferAction::Cancel)
        );

        // Mouse over cancel: state and hint in the foreground; the list is unaffected.
        let hovered = with_controls(context.clone(), |c| c.hover = Some(TransferAction::Cancel));
        let (cells, dock) = draw_with(&state, &hovered, 64);
        assert_eq!(
            dock.buttons.map(|b| b.map(|b| b.state)),
            [Some(ButtonState::Idle), Some(ButtonState::Hover)]
        );
        assert_eq!(
            color_at(&cells, 1, 61),
            Some(THEME.foreground_linear()),
            "hint"
        );

        // List open: the label does not change (no `Hide files`), the
        // button in the pressed tone; the popover's anchor is the button's exact range.
        let open = with_controls(context, |c| c.list_open = true);
        let (cells, dock) = draw_with(&state, &open, 64);
        assert!(row_text(&cells, 1).contains("Show transfers (2)"));
        assert!(!row_text(&cells, 1).contains("Hide"));
        assert_eq!(dock.buttons[0].map(|b| b.state), Some(ButtonState::Pressed));
        let transfer = open.transfer.as_ref().unwrap();
        assert_eq!(
            transfer_button_span(transfer, 64, TransferAction::List),
            Some((28, 48))
        );
        assert_eq!(
            transfer_button_span(transfer, 64, TransferAction::Cancel),
            Some((49, 64))
        );
    }

    #[test]
    fn the_end_line_carries_the_outcome_colour_and_an_unmarked_track() {
        // Success green, error text red and its tally dim, cancel
        // dim. On an unmarked host the empty track is in the separator's color.
        let state = live("", "", "", 0);
        let line = |body: &str, tone: TransferTone, lead: usize| DockContext {
            transfer: Some(Transfer {
                host: "vm".into(),
                body: body.into(),
                tone,
                lead,
                ..Transfer::default()
            }),
            ..remote("vm", "/srv")
        };
        let done = "✓ a.tar → /srv";
        let (cells, _) = draw_with(&state, &line(done, TransferTone::Success, 14), COLS);
        assert_eq!(row_text(&cells, 1), format!("⇄ vm  {done}"));
        assert_eq!(color_at(&cells, 1, 6), Some(THEME.success_linear()));
        assert_eq!(color_at(&cells, 1, 19), Some(THEME.success_linear()));
        let failed = "Failed — disk full · 0 of 3 uploaded";
        let (cells, _) = draw_with(&state, &line(failed, TransferTone::Error, 18), 60);
        assert_eq!(color_at(&cells, 1, 6), Some(THEME.error_linear()));
        assert_eq!(
            color_at(&cells, 1, 23),
            Some(THEME.error_linear()),
            "reason"
        );
        assert_eq!(color_at(&cells, 1, 25), Some(THEME.dim_linear()), "tally");
        let (cells, _) = draw_with(&state, &line("Cancelled", TransferTone::Quiet, 9), COLS);
        assert_eq!(color_at(&cells, 1, 6), Some(THEME.dim_linear()));

        let mut running = line("↑ a", TransferTone::Quiet, 0);
        if let Some(transfer) = &mut running.transfer {
            transfer.progress = Some(10);
        }
        let (_, dock) = draw_with(&state, &running, COLS);
        assert_eq!(dock.edge, THEME.info_linear());
        assert_eq!(dock.track, THEME.separator_linear());
    }

    #[test]
    fn the_buttons_drop_the_hint_then_the_list_then_cancel() {
        let state = live("", "", "", 0);
        let labels = |context: &DockContext, cols: u16| {
            let (cells, _) = draw_with(&state, context, cols);
            row_text(&cells, 1)
        };
        // `⇄ prod  ` is eight columns; the remaining budget is `cols - 8`.
        let queue = uploading("↑ a", 2, None);
        assert!(labels(&queue, 44).ends_with("Show transfers (2)   Cancel all ⌘."));
        assert!(labels(&queue, 43).ends_with("Show transfers (2)   Cancel all"));
        let cancel_only = labels(&queue, 40);
        assert!(cancel_only.ends_with("Cancel all"));
        assert!(!cancel_only.contains("Show"));
        assert!(
            !labels(&queue, 19).contains("Cancel"),
            "cancel does not fit either"
        );

        let single = uploading("↑ a", 1, None);
        assert!(labels(&single, 18).ends_with("Cancel"));
        assert!(!labels(&single, 18).contains('⌘'));
        assert!(!labels(&single, 15).contains("Cancel"));
    }

    #[test]
    fn a_still_pointer_stays_on_its_button_while_the_row_refreshes() {
        // The refresh changes the body on every tick (bytes, speed, remaining
        // time) and when the mouse stops the hover's only input is the column:
        // had the buttons moved with the body the highlight would come and go
        // under a stationary mouse. The state (hover, open list) must not move
        // the width either.
        let bodies = [
            "↑ 1 of 3 · a  1 / 44.6 MB",
            "↑ 1 of 3 · a  12.4 / 44.6 MB · 10.1 MB/s · 3s",
            "↑ 1 of 3 · a  18.2 / 44.6 MB · 0.1 MB/s · 12m 05s",
            "↑ 3 of 3 · a-very-long-file-name-that-clips.tar.gz  44.6 / 44.6 MB",
        ];
        let spans = |body: &str, hover: Option<TransferAction>, list_open: bool| {
            let mut context = uploading(body, 3, Some(5_000));
            let transfer = context.transfer.as_mut().unwrap();
            transfer.controls.hover = hover;
            transfer.controls.list_open = list_open;
            let transfer = context.transfer.as_ref().unwrap();
            (0..80)
                .map(|col| transfer_button_at(transfer, 80, col))
                .collect::<Vec<_>>()
        };
        let first = spans(bodies[0], None, false);
        assert!(first.contains(&Some(TransferAction::List)));
        assert!(first.contains(&Some(TransferAction::Cancel)));
        for body in bodies {
            for hover in [
                None,
                Some(TransferAction::List),
                Some(TransferAction::Cancel),
            ] {
                for list_open in [false, true] {
                    assert_eq!(spans(body, hover, list_open), first, "{body}");
                }
            }
        }
    }

    #[test]
    fn an_upload_row_clips_the_body_and_keeps_the_buttons_whole() {
        let state = live("", "", "", 0);
        let context = uploading("↑ backup.tar.gz  18.2 / 44.6 MB", 1, None);
        let transfer = context.transfer.as_ref().unwrap();
        // At 30 columns: 6 (`⇄ prod`) + 2 + body + 2 + 11 (button), the body is 9.
        let (cells, _) = draw_with(&state, &context, 30);
        assert_eq!(
            row_text(&cells, 1),
            format!("{:<20}Cancel ⌘.", "⇄ prod  ↑ backup…")
        );
        assert_eq!(
            transfer_button_at(transfer, 30, 19),
            Some(TransferAction::Cancel)
        );
        // If the button does not fit there is none; the body takes the rest.
        let (cells, _) = draw_with(&state, &context, 14);
        assert_eq!(row_text(&cells, 1), "⇄ prod  ↑ bac…");
        assert!((0..14).all(|col| transfer_button_at(transfer, 14, col).is_none()));
        // If even the host does not fit, only the mark.
        let (cells, dock) = draw_with(&state, &context, 4);
        assert_eq!(row_text(&cells, 1), "⇄");
        assert_eq!(dock.buttons, [None; 2]);
    }

    #[test]
    fn the_upload_row_is_the_one_the_atlas_checks() {
        // `bt-atlas` asks by hand about the status row's non-ASCII characters
        // in the small class (`the_upload_row_has_no_box_in_the_small_class`);
        // the string is in `bt-shell` (`upload`) but the character set is pinned here.
        assert_eq!(UPLOAD_GLYPHS, ['↑', '↓', '⌘', '✓', '—', '·', '…', '→']);
        assert!(
            CANCEL_HINT
                .chars()
                .filter(|ch| !ch.is_ascii())
                .all(|ch| UPLOAD_GLYPHS.contains(&ch))
        );
    }

    /// A program's guide bar; the local path and branch are filled, so the
    /// bar must hide them.
    fn program(title: &str, detail: &str, path: &str, hint: &str) -> DockContext {
        DockContext {
            program: Some(ProgramBar {
                title: title.into(),
                detail: detail.into(),
                path: path.into(),
                hint: hint.into(),
                tone: ProgramTone::Info,
            }),
            ..context("/Users/me/proj", "main")
        }
    }

    /// The band's context row alone (a raw program's band) at `cols`: the
    /// row's text, its cells and the surface.
    fn program_row(context: &DockContext, cols: u16) -> (String, Vec<Cell>, Dock) {
        let mut cells = Vec::new();
        let (dock, _, rows) = render_with(
            &live("", "", "", 0),
            context,
            None,
            &THEME,
            same(cols),
            Some(0),
            None,
            false,
            None,
            None,
            None,
            &mut Vec::new(),
            &mut Clusters::default(),
            |cell| cells.push(cell),
            |_| (),
        );
        assert_eq!(rows, 0, "no input row to scroll");
        (row_text(&cells, 0), cells, dock)
    }

    #[test]
    fn a_program_bar_takes_the_context_row_and_the_edge() {
        // `Python 3.14.5 · venv  ~/proj/.venv/bin/python3` on the left, the
        // hint right-aligned; the title in the bar's tone, the detail dim,
        // the path quiet, the hint dim; the top hairline `info`.
        let context = program(
            "Python 3.14.5",
            "venv",
            "~/proj/.venv/bin/python3",
            "⌃D exit",
        );
        let (row, cells, dock) = program_row(&context, 60);
        let left = "Python 3.14.5 · venv  ~/proj/.venv/bin/python3";
        assert_eq!(row, format!("{left:<53}⌃D exit"));
        assert_eq!(color_at(&cells, 0, 0), Some(THEME.info_linear()), "title");
        assert_eq!(color_at(&cells, 0, 7), Some(THEME.info_linear()), "version");
        assert_eq!(color_at(&cells, 0, 14), Some(THEME.quiet_linear()), "·");
        assert_eq!(color_at(&cells, 0, 16), Some(THEME.dim_linear()), "detail");
        assert_eq!(color_at(&cells, 0, 22), Some(THEME.quiet_linear()), "path");
        assert_eq!(
            color_at(&cells, 0, 45),
            Some(THEME.quiet_linear()),
            "path's tail"
        );
        assert_eq!(color_at(&cells, 0, 53), Some(THEME.dim_linear()), "hint");
        assert_eq!(dock.edge, THEME.info_linear());
        assert_eq!(dock.sigil, None, "no input row, no prompt mark");
        assert_eq!(dock.caret, None);
        assert_eq!(dock.buttons, [None; 2]);
        // Nothing of the local context: the bar is the row.
        assert!(!row.contains("main") && !row.contains("/Users/me"), "{row}");
        // No detail, no separator; no path, no gap.
        let bare = program("Node v22.13.0", "", "", "⌃D exit");
        let (row, _, _) = program_row(&bare, 30);
        assert_eq!(row, format!("{:<23}⌃D exit", "Node v22.13.0"));
    }

    #[test]
    fn a_narrow_program_bar_drops_the_hint_then_shortens_the_path() {
        let context = program(
            "Node v22.13.0",
            "nvm",
            "~/.nvm/versions/node/v22.13.0/bin/node",
            "⌃D exit",
        );
        let full = "Node v22.13.0 · nvm  ~/.nvm/versions/node/v22.13.0/bin/node";
        // Everything fits with the hint's two-column gap…
        let width = full.chars().count() + 2 + "⌃D exit".chars().count();
        let (row, _, _) = program_row(&context, width as u16);
        assert_eq!(row, format!("{full}  ⌃D exit"));
        // …one column less and the hint drops first: the path stays whole.
        let (row, _, _) = program_row(&context, width as u16 - 1);
        assert_eq!(row, full);
        // Then the path is shortened from the left, its tail kept.
        let (row, _, _) = program_row(&context, 40);
        assert_eq!(row, "Node v22.13.0 · nvm  …/v22.13.0/bin/node");
        assert_eq!(row.chars().count(), 40);
        // The path goes before the detail, and does not come back when the
        // detail drops in turn.
        let (row, _, _) = program_row(&context, 19);
        assert_eq!(row, "Node v22.13.0 · nvm");
        let (row, _, _) = program_row(&context, 18);
        assert_eq!(row, "Node v22.13.0");
        // The title is never shortened: no room for it, nothing at all.
        let (row, cells, _) = program_row(&context, 13);
        assert_eq!(row, "Node v22.13.0");
        assert_eq!(cells.len(), 12, "one space, no cell");
        let (row, cells, dock) = program_row(&context, 12);
        assert_eq!(row, "");
        assert!(cells.is_empty());
        assert_eq!(dock.edge, THEME.info_linear(), "the edge still says where");
    }

    #[test]
    fn the_remote_status_bar_and_the_upload_row_come_before_a_program_bar() {
        let bar = program("Python 3.14.5", "", "/usr/bin/python3", "⌃D exit");
        let remote = DockContext {
            program: bar.program.clone(),
            ..remote("prod", "/srv/app")
        };
        let (row, _, _) = program_row(&remote, 60);
        assert_eq!(row, "⇄ prod  /srv/app");
        let upload = DockContext {
            program: bar.program.clone(),
            remote: None,
            ..uploading("Connection closed", 0, None)
        };
        let (row, _, _) = program_row(&upload, 60);
        assert_eq!(row, "⇄ prod  Connection closed");
    }

    #[test]
    fn the_program_bar_is_the_one_the_atlas_checks() {
        // `bt-atlas` asks by hand about the guide bar's non-ASCII characters
        // in the small class (`the_program_bar_has_no_box_in_the_small_class`):
        // the separator and the shortening mark drawn here, and the hints'
        // vocabulary (`bt-shell`'s `program` checks its strings against this).
        assert_eq!(PROGRAM_GLYPHS, ['⌃', '·', '…']);
        assert!(
            PROGRAM_DETAIL
                .chars()
                .chain(std::iter::once(ELLIPSIS))
                .filter(|ch| !ch.is_ascii())
                .all(|ch| PROGRAM_GLYPHS.contains(&ch))
        );
    }

    fn offered(host: &str, mark: HostMark) -> DockContext {
        DockContext {
            reconnect: Some(Reconnect {
                host: host.into(),
                mark,
                line: format!("ssh {host}"),
            }),
            ..context("/Users/me", "")
        }
    }

    #[test]
    fn the_reconnect_hint_is_the_one_the_atlas_checks() {
        // `bt-atlas` asks by hand about the placeholder's non-ASCII characters
        // in the large class (`the_reconnect_placeholder_has_no_box_in_the_normal_class`):
        // if the string changes this fails and points to that test.
        let outside: Vec<char> = RECONNECT_HINT.chars().filter(|ch| !ch.is_ascii()).collect();
        assert_eq!(outside, ['·', '⏎']);
    }

    #[test]
    fn a_reconnect_offer_fills_the_empty_line() {
        // On an empty input row, after the caret, `⇄ host` in the
        // mark's color, the rest `dim`; the caret at the start of the row, the
        // context row local.
        let state = live("", "", "", 0);
        let (cells, dock) = draw_with(&state, &offered("prod", HostMark::Production), COLS);
        assert_eq!(text(&cells), "  ⇄ prod  Connection lost · ⏎ reconnect");
        let color = |col: u16| {
            cells
                .iter()
                .find(|cell| cell.row == 0 && cell.col == col)
                .map(|cell| cell.fg)
        };
        assert_eq!(color(2), Some(THEME.error_linear()), "mark");
        assert_eq!(color(4), Some(THEME.error_linear()), "host");
        assert_eq!(color(10), Some(THEME.dim_linear()), "text");
        assert_eq!(dock.caret, caret_at(TEXT_COL));
        assert_eq!(row_text(&cells, 1), "/Users/me");
        // An unmarked host is `info`, the same as the context row's remote form.
        let (cells, _) = draw_with(&state, &offered("prod", HostMark::None), COLS);
        let mark = cells.iter().find(|cell| cell.row == 0 && cell.col == 2);
        assert_eq!(mark.map(|cell| cell.fg), Some(THEME.info_linear()));
    }

    #[test]
    fn the_reconnect_placeholder_is_clipped_not_wrapped() {
        let state = live("", "", "", 0);
        let (cells, _) = draw_with(&state, &offered("prod", HostMark::None), 20);
        assert_eq!(text(&cells), "  ⇄ prod  Connection");
        // It does not wrap: there is no row other than the context row and the band does not grow.
        assert!(cells.iter().all(|cell| cell.row <= 1), "{cells:?}");
        assert_eq!(needed_rows(&state, 20), 1);
    }

    #[test]
    fn the_reconnect_placeholder_only_fills_an_empty_line() {
        let offer = offered("prod", HostMark::None);
        for state in [
            live("", "ls", "", 2),
            // The suggestion fills the row too: the same layer.
            live("", "", "ls -la", 0),
        ] {
            let (cells, _) = draw_with(&state, &offer, COLS);
            assert!(!text(&cells).contains('⇄'), "{:?}", text(&cells));
        }
        let mut state = live("", "", "", 0);
        state.prebuffer = "for x in 1\n".into();
        let (cells, _) = draw_with(&state, &offer, COLS);
        assert!(cells.iter().all(|cell| cell.ch != Some('⇄')));
        // In `vicmd` ⏎ does not send the line, so there is no hint either.
        let mut state = live("", "", "", 0);
        state.insert_keymap = false;
        let (cells, _) = draw_with(&state, &offer, COLS);
        assert!(cells.iter().all(|cell| cell.ch != Some('⇄')));
        // Without an offer an empty row is empty.
        let (cells, _) = draw_with(&live("", "", "", 0), &context("/Users/me", ""), COLS);
        assert_eq!(text(&cells), "");
    }

    #[test]
    fn the_remote_mark_is_the_one_the_atlas_checks() {
        // `bt-atlas` cannot see `bt-core` and writes the character by hand
        // (`the_remote_mark_is_a_glyph_in_the_small_class`): if the mark
        // changes this fails and points to that test's constant.
        assert_eq!(REMOTE_MARK, '⇄');
    }

    #[test]
    fn a_remote_session_shows_the_host_and_the_remote_path() {
        // `⇄ host`, two spaces, the remote path; no local path and no branch.
        let state = live("", "", "", 0);
        let (cells, dock) = draw_with(&state, &remote("prod", "/var/www/app"), COLS);
        assert_eq!(row_text(&cells, 1), "⇄ prod  /var/www/app");
        // The mark and host are `info`, the path the local one's two tiers.
        let color = |col: u16| {
            cells
                .iter()
                .find(|cell| cell.row == 1 && cell.col == col)
                .map(|cell| cell.fg)
        };
        // Columns: `⇄` 0, host 2..6, path from 8 (`/var/www/` 8..17, `app` 17..).
        assert_eq!(color(0), Some(THEME.info_linear()));
        assert_eq!(color(2), Some(THEME.info_linear()), "host");
        assert_eq!(color(9), Some(THEME.quiet_linear()), "parent directory");
        assert_eq!(color(17), Some(THEME.dim_linear()), "last component");
        // The top hairline is `info`, the second stays in the separator's color.
        assert_eq!(dock.edge, THEME.info_linear());
        assert_eq!(dock.separator, THEME.separator_linear());
    }

    #[test]
    fn a_marked_host_takes_its_mark_color() {
        // `⇄ host` and the top line in the mark's color; the path
        // tiers and the second line do not change.
        let state = live("", "", "", 0);
        for (mark, expected) in [
            (HostMark::Production, THEME.error_linear()),
            (HostMark::Staging, THEME.warning_linear()),
            (HostMark::Development, THEME.success_linear()),
            (
                HostMark::Rgb(0xc678dd),
                LinearRgba::from_srgb(0xc6, 0x78, 0xdd),
            ),
            (HostMark::None, THEME.info_linear()),
        ] {
            let context = DockContext {
                remote_mark: mark,
                ..remote("prod", "/var/www/app")
            };
            let (cells, dock) = draw_with(&state, &context, COLS);
            let color = |col: u16| {
                cells
                    .iter()
                    .find(|cell| cell.row == 1 && cell.col == col)
                    .map(|cell| cell.fg)
            };
            assert_eq!(color(0), Some(expected), "{mark:?}: mark");
            assert_eq!(color(2), Some(expected), "{mark:?}: host");
            assert_eq!(color(17), Some(THEME.dim_linear()), "{mark:?}: path");
            assert_eq!(dock.edge, expected, "{mark:?}: top line");
            assert_eq!(dock.separator, THEME.separator_linear());
        }
    }

    #[test]
    fn a_remote_session_without_a_path_shows_only_the_host() {
        // If the remote shell prints no OSC 7, only the host.
        let state = live("", "", "", 0);
        let (cells, _) = draw_with(&state, &remote("deploy@10.0.0.5", ""), COLS);
        assert_eq!(row_text(&cells, 1), "⇄ deploy@10.0.0.5");
    }

    #[test]
    fn a_narrow_remote_line_trims_the_path_and_never_the_host() {
        let state = live("", "", "", 0);
        let context = remote("prod", "/var/www/app");
        // 14 columns: `⇄ prod` six, two spaces, six for the path — the last five with `…`.
        let (cells, _) = draw_with(&state, &context, 14);
        assert_eq!(row_text(&cells, 1), "⇄ prod  …w/app");
        // If there is no room for the path, only `⇄ host`.
        let (cells, _) = draw_with(&state, &context, 6);
        assert_eq!(row_text(&cells, 1), "⇄ prod");
        // If the host does not fit it is **not clipped**: only the mark remains.
        let (cells, _) = draw_with(&state, &context, 5);
        assert_eq!(row_text(&cells, 1), "⇄");
        let (cells, _) = draw_with(&state, &context, 1);
        assert_eq!(row_text(&cells, 1), "⇄");
    }

    #[test]
    fn a_local_session_keeps_the_separator_on_the_edge() {
        // Locally the top line is today's separator color: both lines the same color.
        let state = live("", "", "", 0);
        let (_, dock) = draw_with(&state, &context("/tmp", "main"), COLS);
        assert_eq!(dock.edge, THEME.separator_linear());
        assert_eq!(dock.separator, THEME.separator_linear());
    }

    #[test]
    fn a_narrow_dock_trims_the_path_from_the_left_and_keeps_the_branch() {
        // The tail is more informative: which repo you are in is written in the
        // trailing components. The branch is **never** shortened — a shortened
        // branch name would suggest being on the wrong branch.
        let state = live("", "", "", 0);
        let path = "/a/bb/ccc/dddd";

        // 20 columns: since the context starts at the left edge all twenty are
        // its, ` | main` takes seven, 13 for the path — i.e. with the `…` the
        // last twelve characters. Moving to the left edge **gained the path two columns**.
        let (cells, _) = draw_with(&state, &context(path, "main"), 20);
        assert_eq!(row_text(&cells, 1), "…/bb/ccc/dddd | main");

        // When narrowed it is always the path that is clipped: at nine columns
        // `…d` is left of it, `main` stands whole.
        let (cells, _) = draw_with(&state, &context(path, "main"), 9);
        assert_eq!(row_text(&cells, 1), "…d | main");

        // When not even one column is left for the path only the branch remains,
        // with no separator: the branch is not what is to be clipped.
        let (cells, _) = draw_with(&state, &context(path, "main"), 7);
        assert_eq!(row_text(&cells, 1), "main");

        // When the branch fits **exactly** it drops the path altogether: the budget is the branch's first.
        let (cells, _) = draw_with(&state, &context(path, "main"), 4);
        assert_eq!(row_text(&cells, 1), "main");

        // **If not even the branch fits it is not drawn at all**, not clipped:
        // showing `main` as `ma` would tell the user they are on a branch that
        // does not exist. The remaining width is the path's and its shortening is marked.
        let (cells, _) = draw_with(&state, &context(path, "main"), 3);
        assert_eq!(row_text(&cells, 1), "…dd");
        // If the branch does not fit and there is no path the row is entirely
        // empty — nothing rather than something wrong.
        let (cells, _) = draw_with(&state, &context("", "main"), 3);
        assert_eq!(row_text(&cells, 1), "");

        // A path that fits is not shortened and no `…` is added.
        let (cells, _) = draw_with(&state, &context(path, "main"), 40);
        assert_eq!(row_text(&cells, 1), "/a/bb/ccc/dddd | main");
    }

    #[test]
    fn context_line_spends_its_own_budget() {
        // **The context row's budget is separate from the input row's** and the
        // reason is the point size: that row is drawn with the small face, more
        // letters fit in the same pixel strip. The drawing side supplies the
        // number (`bt_gpu::context_cols`); this crate takes it as a **budget**,
        // not as a point size.
        let state = live("", "ls", "", 2);
        let path = "/a/bb/ccc/dddd";
        let ctx = context(path, "main");

        // In a nine-column grid the input row fits nine, the context row
        // twenty-one (path 14 + separator 3 + branch 4): the shortening is
        // computed by the **large** budget and the path comes out whole.
        let mut cells = Vec::new();
        let owned = caret_home(None, state.status, false) == CaretHome::Dock;
        let wide = DockCols {
            grid: 9,
            context: 21,
        };
        render(
            &state,
            &ctx,
            None,
            &THEME,
            wide,
            owned,
            None,
            |cell| cells.push(cell),
            |_| (),
        );
        assert_eq!(row_text(&cells, 1), "/a/bb/ccc/dddd | main");
        // The input row is **untouched**: the two budgets do not mix.
        assert_eq!(text(&cells), "  ls");

        // The same grid, the budget narrow: the shortening comes back. So the
        // number the row sees really is `context_cols`, not `cols`.
        let mut narrow = Vec::new();
        render(
            &state,
            &ctx,
            None,
            &THEME,
            same(9),
            owned,
            None,
            |cell| narrow.push(cell),
            |_| (),
        );
        assert_eq!(row_text(&narrow, 1), "…d | main");
    }

    #[test]
    fn the_surface_colors_come_from_the_theme() {
        // The ground is **opaque** and the theme's own: the grid row that
        // overflows during a slide must stay under it. The separator is a
        // derived value, not a new role.
        let (_, dock) = draw(&live("", "", "", 0), COLS);
        assert_eq!(dock.ground, THEME.background_linear());
        assert_eq!(dock.ground.to_array()[3], 1.0, "ground is translucent");
        assert_eq!(dock.separator, THEME.separator_linear());
        assert_ne!(
            dock.separator, dock.ground,
            "separator is the same color as the ground"
        );
        // The text under the caret is by the same rule as in the grid: the ground color.
        assert_eq!(dock.caret_text, THEME.background_linear());
    }

    /// **A wide character takes two columns in the dock** and its head cell
    /// ends up marked.
    ///
    /// This guard was once the guard of the **opposite**
    /// (`the_dock_never_marks_a_cell_wide`) and its reason was sound with that
    /// arithmetic: while the column derived from the character index a
    /// two-cell glyph would paint over its neighbor. The column arithmetic
    /// changed, so the invariant went too — the box was not deleted, its
    /// **claim** changed. The lesson: a truly mandatory constraint could not
    /// have been lifted later.
    ///
    /// The context row is **outside** the scope: the small class, the column
    /// pitch is the small face's advance (the precedent of the procedural
    /// characters).
    #[test]
    fn a_wide_char_takes_two_columns_in_the_dock() {
        let state = live("", "漢ls", "", 3);
        let (cells, dock) = draw(&state, COLS);
        let lead = cells
            .iter()
            .find(|cell| cell.ch == Some('漢'))
            .expect("so the test is not left without a subject: the CJK must have been drawn");
        assert!(lead.wide, "the head cell was not marked: {lead:?}");
        assert_eq!(lead.col, TEXT_COL, "the text starts from the first column");
        // The neighboring column **takes no glyph**: the head cell's `wide`
        // draws it (`bt_gpu::AtlasTexture::prepare` fans it out). A second glyph
        // cell would print two quads in the same place.
        assert!(
            !cells
                .iter()
                .any(|cell| cell.col == TEXT_COL + 1 && cell.ch.is_some()),
            "a glyph landed on the spacer column: {cells:?}"
        );
        // And the next letter is **two** columns later: the whole of the arithmetic is on this line.
        let l = cells
            .iter()
            .find(|cell| cell.ch == Some('l'))
            .expect("'l' must be drawn");
        assert_eq!(l.col, TEXT_COL + 2, "the wide character took two columns");
        // The caret is at the sum of the **widths** before the cursor: for
        // `漢ls` the index is 3 but the column 4.
        assert_eq!(
            dock.caret,
            caret_at(TEXT_COL + 4),
            "the caret counted indices, not columns"
        );
    }

    /// A zero-width code point **takes no cell**.
    ///
    /// Combiners (VS16, ZWJ, skin tone) have no cell of their own in the grid
    /// either — alacritty keeps them in `CellExtra`. Had a cell been given, a
    /// second cell would land on the previous character's column and cover its
    /// glyph.
    #[test]
    fn a_zero_width_codepoint_gets_no_cell() {
        // `❤` + VS16: two characters, **one** column (`❤` is one column).
        let state = live("", "\u{2764}\u{fe0f}x", "", 3);
        let (cells, _) = draw(&state, COLS);
        assert_eq!(
            cells.iter().filter(|c| c.ch.is_some()).count(),
            2,
            "VS16 took a cell of its own: {cells:?}"
        );
        let x = cells
            .iter()
            .find(|cell| cell.ch == Some('x'))
            .expect("'x' must be drawn");
        assert_eq!(x.col, TEXT_COL + 1, "VS16 consumed a column");
    }

    /// A highlight spreads over a wide character's **two cells**.
    ///
    /// `region_highlight`'s ranges are in character indices (ZLE's unit) but
    /// the painted ground is per cell: if no ground cell landed on the spacer
    /// column the yellow ground of the string `"fix 🎉"` would end at the
    /// emoji's right half. The same as the grid's `WIDE_CHAR_SPACER` arm.
    #[test]
    fn a_highlight_covers_both_cells_of_a_wide_char() {
        let mut state = live("", "漢", "", 1);
        state.highlights.push(Highlight {
            start: 0,
            end: 1,
            style: HighlightStyle {
                bg: Some(HighlightColor::Indexed(3)),
                ..HighlightStyle::default()
            },
        });
        let (cells, _) = draw(&state, COLS);
        let painted: Vec<u16> = cells
            .iter()
            .filter(|cell| cell.bg.is_some())
            .map(|cell| cell.col)
            .collect();
        assert_eq!(
            painted,
            vec![TEXT_COL, TEXT_COL + 1],
            "the highlight painted only half of the wide character"
        );
    }

    /// A wide glyph is **not split** at the row end (the wrapping counterpart
    /// of the old horizontal window's edge guard).
    ///
    /// The character that does not fit moves to the next row and an empty
    /// column remains behind it: the wide-glyph contract is "a box or a whole
    /// glyph" and half a glyph is a **silent** corruption. The same place as
    /// the grid's `LEADING_WIDE_CHAR_SPACER` rule.
    #[test]
    fn a_wide_char_is_never_split_at_the_row_end() {
        // **Two** columns are left for the text: `a` eats one, `漢` wants two
        // and does not fit — it lands at the start of the next row, the first row's last column empty.
        let cols = TEXT_COL + 2;
        let state = live("", "a漢", "", 0);
        let (cells, _, _, _) = draw_rows(&state, cols, 2, None);
        assert_eq!(row_text(&cells, 0), "  a", "first row: {cells:?}");
        let lead = cells
            .iter()
            .find(|cell| cell.ch == Some('漢'))
            .expect("the wide character that did not fit vanished");
        assert_eq!(
            (lead.row, lead.col),
            (1, TEXT_COL),
            "it was split: {lead:?}"
        );
        assert!(lead.wide);
    }

    /// A control character **keeps its column**, even though it is not drawn.
    ///
    /// The distinction of the two zeros: a combiner consumes no column (it has
    /// no cell of its own in the grid either), a control character does.
    /// Dropping it to zero would be a regression — the words on both sides of
    /// a TAB inserted with `Ctrl-V` would merge and the caret would shift left
    /// by one column per control character. Code review
    /// caught this and this guard pins it.
    ///
    /// The right display is neither 0 nor 1 (zsh shows `^C` in **two**
    /// columns) and that known limit is in [`column_width`]'s doc; the guard
    /// preserves today's behavior, it does not impose the ideal.
    #[test]
    fn a_control_char_keeps_its_column() {
        // `a` + TAB + `b`: three columns, the middle one is not drawn.
        let state = live("", "a\tb", "", 3);
        let (cells, dock) = draw(&state, COLS);
        let drawn: Vec<(u16, Option<char>)> =
            cells.iter().map(|cell| (cell.col, cell.ch)).collect();
        assert_eq!(
            drawn,
            vec![(TEXT_COL, Some('a')), (TEXT_COL + 2, Some('b'))],
            "the control character lost its column: the words merged"
        );
        assert_eq!(
            dock.caret,
            caret_at(TEXT_COL + 3),
            "the caret did not count the control character's column"
        );
    }

    /// **A wide character under the caret is drawn whole and the caret is on
    /// top of it** — the wrapping counterpart of the old horizontal window guard.
    ///
    /// The regression code review found: when the caret
    /// stood on a wide glyph the glyph was not drawn at all and the caret
    /// stayed over an empty cell. In wrapping, if the character does not fit
    /// at the row end it drops to the next row and the caret with it; in a
    /// one-row window (the ceiling) that row stays visible.
    #[test]
    fn the_caret_stays_on_the_whole_wide_char_under_it() {
        // A width of two columns, the caret on the wide character (index 1).
        let cols = TEXT_COL + 2;
        let state = live("", "a漢", "", 1);
        let mut cells = Vec::new();
        let owned = caret_home(None, state.status, false) == CaretHome::Dock;
        let dock = render(
            &state,
            &DockContext::default(),
            None,
            &THEME,
            same(cols),
            owned,
            None,
            |cell| cells.push(cell),
            |_| (),
        );
        let lead = cells
            .iter()
            .find(|cell| cell.ch == Some('漢'))
            .expect("the character under the caret was not drawn");
        assert!(lead.wide, "{lead:?}");
        assert_eq!(
            dock.caret,
            Some(DockCaret {
                col: lead.col,
                row: lead.row
            }),
            "the caret must stand on its own character"
        );
    }

    /// **The context row stays in character units** — a known limit.
    ///
    /// Its reason is not the absence of a wide glyph but the **small size
    /// class**: the column pitch is the small face's advance and the wide path
    /// is closed there (the precedent of the procedural characters). So a path
    /// with CJK still shifts columns here.
    ///
    /// The guard fills the gap left by an earlier deleted test: it was the **only**
    /// test passing `render_context` a CJK `cwd` and the four that replaced it
    /// never touched the context row (found in code review).
    #[test]
    fn the_context_line_keeps_character_columns() {
        let state = live("", "ls", "", 2);
        let (cells, _) = draw_with(
            &state,
            &DockContext {
                cwd: "/tmp/漢字".into(),
                branch: "主".into(),
                ..DockContext::default()
            },
            COLS,
        );
        let context: Vec<&Cell> = cells.iter().filter(|cell| cell.row == 1).collect();
        assert!(
            context.iter().any(|cell| cell.ch == Some('漢')),
            "so the test is not left without a subject: the context row must draw CJK"
        );
        // **None is marked wide** and none should be: in the small class
        // `Atlas::slot` already normalizes `Half` to `Whole`, so even if the
        // flag were set the fanning out would not run — but setting the flag
        // would mean holding the contract in two places.
        assert!(
            context.iter().all(|cell| !cell.wide),
            "the context row set the wide flag: {context:?}"
        );
        // The column advances per **character**: `漢` and `字` are in neighboring columns.
        let cols_of: Vec<u16> = context
            .iter()
            .filter(|cell| cell.ch == Some('漢') || cell.ch == Some('字'))
            .map(|cell| cell.col)
            .collect();
        assert_eq!(cols_of.len(), 2, "two CJK cells expected: {context:?}");
        assert_eq!(
            cols_of[1] - cols_of[0],
            1,
            "the context row has moved to counting columns (if the limit was lifted, fix the doc)"
        );
    }

    /// No overflow **if the row is narrower** than the caret's character.
    ///
    /// One column for the text and a two-column character under the caret:
    /// [`layout`]'s "an empty row that does not fit overflows" arm. In the old
    /// horizontal window `caret_col - skip` dropped to negative here (found
    /// in code review); in wrapping the character overflows the row and is
    /// **not drawn** (it would write outside the grid), the caret stays in the
    /// text column and no number overflows — in debug there is no panic on the
    /// frame path in `bt-core`.
    #[test]
    fn a_row_narrower_than_the_caret_char_does_not_underflow() {
        for (label, buffer, cursor) in [
            ("caret on the wide character", "漢", 0),
            ("a wide character + tail in a single column", "漢a", 0),
        ] {
            let state = live("", buffer, "", cursor);
            let mut cells = Vec::new();
            let owned = caret_home(None, state.status, false) == CaretHome::Dock;
            let dock = render(
                &state,
                &DockContext::default(),
                None,
                &THEME,
                same(TEXT_COL + 1),
                owned,
                None,
                |cell| cells.push(cell),
                |_| (),
            );
            // The caret **inside** the text area: it does not fall into the
            // mark's share and does not overflow the window either.
            let caret = dock.caret.expect("{label}: the dock's caret");
            assert_eq!(caret.col, TEXT_COL, "{label}: caret {caret:?}");
            assert!(
                cells.iter().all(|cell| cell.col < TEXT_COL + 1),
                "{label}: written outside the window: {cells:?}"
            );
        }
    }

    // ---- Row-aware layout ----

    /// The layout's visual rows, as text; and its end.
    fn laid_out(
        text: &str,
        caret: usize,
        width: usize,
        first: usize,
        rest: usize,
    ) -> (Vec<(String, usize)>, LayoutEnd) {
        let chars: Vec<char> = text.chars().collect();
        let mut lines = Vec::new();
        let end = layout(text.chars(), caret, width, first, rest, false, |line| {
            lines.push((chars[line.start..line.end].iter().collect(), line.col));
        });
        (lines, end)
    }

    fn end(caret_row: usize, caret_col: usize, rows: usize) -> LayoutEnd {
        LayoutEnd {
            caret_row,
            caret_col,
            rows,
        }
    }

    #[test]
    fn layout_breaks_at_newlines_and_the_newline_takes_no_column() {
        // Continuation rows from `rest`, the first row from `first`; `\n` is in no
        // row's range.
        let (lines, at) = laid_out("for i\ndo\ndone", 14, 20, 2, 2);
        assert_eq!(
            lines,
            vec![("for i".into(), 2), ("do".into(), 2), ("done".into(), 2)]
        );
        assert_eq!(at, end(2, 6, 3));
    }

    #[test]
    fn layout_wraps_at_the_width_and_lazily() {
        // Six columns, the first row from 2: `abcd` fits, `efghij` fills the
        // second row exactly and **no empty row is born** behind it — the caret
        // is in the middle of the text.
        let (lines, at) = laid_out("abcdefghij", 1, 6, 2, 0);
        assert_eq!(lines, vec![("abcd".into(), 2), ("efghij".into(), 0)]);
        assert_eq!(at, end(0, 3, 2));
    }

    #[test]
    fn a_caret_after_a_full_row_starts_the_next_row() {
        // zsh does not leave the cursor in the pending-wrap state: a caret at
        // the end of a completely full row is at the start of the next row and
        // that row is counted.
        let (lines, at) = laid_out("abcd", 4, 4, 0, 0);
        assert_eq!(lines, vec![("abcd".into(), 0), (String::new(), 0)]);
        assert_eq!(at, end(1, 0, 2));
        // On a row that is not full the caret stays at the row's end.
        let (_, at) = laid_out("abc", 3, 4, 0, 0);
        assert_eq!(at, end(0, 3, 1));
    }

    #[test]
    fn a_wide_char_is_not_split_at_the_end_of_a_row() {
        // Five columns, `abcd` is four; `日` is two columns and does not fit in
        // the fifth: it drops whole to the next row, an empty column remains on the right.
        let (lines, at) = laid_out("abcd日x", 4, 5, 0, 0);
        assert_eq!(lines, vec![("abcd".into(), 0), ("日x".into(), 0)]);
        // The caret is in front of the wide character: where the character
        // **would go**, i.e. at the start of the next row, not at the end of the old row.
        assert_eq!(at, end(1, 0, 2));
        // When it fits exactly it does not drop.
        let (lines, _) = laid_out("abc日", 0, 5, 0, 0);
        assert_eq!(lines, vec![("abc日".into(), 0)]);
    }

    #[test]
    fn a_trailing_newline_leaves_an_empty_last_row() {
        // `echo a` + line break: the second row is empty but it exists, the caret is there.
        let (lines, at) = laid_out("echo a\n", 7, 20, 2, 2);
        assert_eq!(lines, vec![("echo a".into(), 2), (String::new(), 2)]);
        assert_eq!(at, end(1, 2, 2));
    }

    #[test]
    fn a_caret_right_after_a_newline_sits_at_the_next_row_start() {
        let (_, at) = laid_out("ab\ncd", 3, 20, 2, 2);
        assert_eq!(at, end(1, 2, 2));
        // Caret **in front of** the `\n`: at the end of the previous row.
        let (_, at) = laid_out("ab\ncd", 2, 20, 2, 2);
        assert_eq!(at, end(0, 4, 2));
        // A `\n` behind a completely full row opens no empty row: the row the
        // `\n` opens is the same row wrapping would open, and the caret in front of it is there.
        let (lines, at) = laid_out("abcd\ne", 4, 4, 0, 0);
        assert_eq!(lines, vec![("abcd".into(), 0), ("e".into(), 0)]);
        assert_eq!(at, end(1, 0, 2));
    }

    #[test]
    fn a_first_row_past_the_margin_wraps_before_its_first_char() {
        // In the grid the prompt has filled the row completely: the first row
        // stays empty, the text starts from the row below. A character that
        // does not fit even at the start of a continuation row does not wrap
        // forever, it overflows.
        let (lines, at) = laid_out("ab", 0, 4, 4, 0);
        assert_eq!(lines, vec![(String::new(), 4), ("ab".into(), 0)]);
        assert_eq!(at, end(1, 0, 2));
        let (lines, _) = laid_out("日", 0, 1, 0, 0);
        assert_eq!(lines, vec![("日".into(), 0)]);
    }

    #[test]
    fn grid_span_matches_the_column_division_on_one_line() {
        // **Equivalence guard**: the suppression's row arithmetic
        // moved from column division to the layout walk and on a one-row display
        // the result **must stay the same** — including the `saturating_sub(1)`
        // rule of a completely full row. The old formula stands here as it was;
        // the sweep tries all small grids, every column of the cursor and every
        // length on both sides of the caret.
        let old = |cursor_col: usize, before: usize, after: usize, cols: usize| {
            let above = before.saturating_sub(cursor_col).div_ceil(cols);
            let below = (cursor_col + after).saturating_sub(1) / cols;
            (above, below)
        };
        for cols in 1..=10 {
            for cursor_col in 0..cols {
                for before in 0..3 * cols {
                    for after in 0..3 * cols {
                        let text = "x".repeat(before + after);
                        assert_eq!(
                            grid_span(&text, before, cursor_col, cols, false),
                            old(cursor_col, before, after, cols),
                            "cols={cols} cursor_col={cursor_col} before={before} after={after}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn grid_span_counts_the_row_a_wide_char_is_pushed_to() {
        // The one distinction the old division did not see, and in the safe
        // direction: in a five-column grid the cursor at column 0, `abcd日日日`
        // behind it — the first `日` does not fit the row and drops to the next
        // row, so the third is one more row down. The division split 10 columns
        // by 5 and said one row under the cursor (`(10 - 1) / 5`); in the grid
        // the tail is two rows down.
        assert_eq!(grid_span("abcd日日日", 0, 0, 5, false), (0, 2));
    }

    // ---- Clustering ----

    /// The pieces of the clustered sequences and the single wide character
    /// that stands in for each: in the clustered reading `👍🏽` must take as
    /// much room as a `日`.
    const CLUSTERED: [(&str, &str); 6] = [
        ("🇹🇷", "日"),
        ("👍🏽", "日"),
        ("👨\u{200D}👩\u{200D}👧", "日"),
        ("❤\u{FE0F}", "日"),
        ("1\u{FE0F}\u{20E3}", "日"),
        ("x", "x"),
    ];

    /// In the clustered reading the suppression's grid walk counts every
    /// cluster as one wide character — the same as the unclustered walk gives
    /// on the string with `日`s, including a cluster landing at the end of a
    /// row (`👍🏽` in the last two columns, or not fitting and dropping to the
    /// next row). The grid's same equivalence is in `handler::tests`; together
    /// the two say "`grid_span` matches the grid".
    #[test]
    fn clustered_grid_span_counts_a_cluster_as_one_wide_char() {
        // Piece sequences: at every length, a few of every piece combination.
        let sequences: Vec<Vec<usize>> = (0..CLUSTERED.len())
            .flat_map(|a| (0..CLUSTERED.len()).map(move |b| vec![a, 5, b, a, 5, 5, b, a, b]))
            .collect();
        for parts in &sequences {
            let clustered: Vec<&str> = parts.iter().map(|&i| CLUSTERED[i].0).collect();
            let wide: Vec<&str> = parts.iter().map(|&i| CLUSTERED[i].1).collect();
            for caret_part in 0..=parts.len() {
                let caret_of =
                    |pieces: &[&str]| -> usize { pieces[..caret_part].concat().chars().count() };
                for cols in 2..=9 {
                    for cursor_col in 0..cols {
                        assert_eq!(
                            grid_span(
                                &clustered.concat(),
                                caret_of(&clustered),
                                cursor_col,
                                cols,
                                true
                            ),
                            grid_span(&wide.concat(), caret_of(&wide), cursor_col, cols, false),
                            "{clustered:?} caret={caret_part} cols={cols} cursor_col={cursor_col}"
                        );
                    }
                }
            }
        }
    }

    /// ZLE does not know clusters: after `👍🏽` a ← puts `CURSOR` before the
    /// `🏽` and the grid's cursor is at the cluster's head column. The walk
    /// must count that state as if the caret were at the cluster's start — had
    /// a half cluster been counted the start column would shift two to the
    /// left.
    #[test]
    fn a_caret_inside_a_cluster_counts_like_its_head_in_grid_span() {
        let text = "abc👍🏽de\u{1F1F9}\u{1F1F7}f";
        for cols in 2..=9 {
            for cursor_col in 0..cols {
                assert_eq!(
                    grid_span(text, 4, cursor_col, cols, true),
                    grid_span(text, 3, cursor_col, cols, true),
                    "skin tone, cols={cols} cursor_col={cursor_col}"
                );
                assert_eq!(
                    grid_span(text, 8, cursor_col, cols, true),
                    grid_span(text, 7, cursor_col, cols, true),
                    "RI pair, cols={cols} cursor_col={cursor_col}"
                );
            }
        }
    }

    /// There is no clustering in the off reading: `👍🏽` is two wide characters.
    #[test]
    fn unclustered_grid_span_keeps_code_points() {
        assert_eq!(grid_span("👍🏽👍🏽", 0, 0, 4, false), (0, 1));
        assert_eq!(grid_span("👍🏽👍🏽", 0, 0, 4, true), (0, 0));
    }

    /// The dock's layout draws a cluster as one wide glyph: a family is two
    /// columns, one cell; the letter behind it two columns to the right. In the
    /// off reading the family is three wide glyphs (the ZWJs columnless).
    #[test]
    fn a_clustered_family_takes_two_dock_columns() {
        let family = "👨\u{200D}👩\u{200D}👧";
        let text = format!("{family}x");
        let count = text.chars().count();
        let mut state = live("", &text, "", count);
        state.cluster = true;
        let (cells, dock) = draw(&state, COLS);
        let glyphs: Vec<(char, u16)> = cells
            .iter()
            .filter_map(|cell| cell.ch.map(|ch| (ch, cell.col)))
            .collect();
        assert_eq!(glyphs, vec![('👨', TEXT_COL), ('x', TEXT_COL + 2)]);
        assert_eq!(dock.caret, caret_at(TEXT_COL + 3));
        state.cluster = false;
        let (cells, _) = draw(&state, COLS);
        let x = cells
            .iter()
            .find(|cell| cell.ch == Some('x'))
            .expect("'x' must be drawn");
        assert_eq!(x.col, TEXT_COL + 6, "the off reading is as today");
    }

    /// If `CURSOR` falls **inside** a cluster the caret is at the cluster's
    /// start: between the two RIs and in the middle of a ZWJ sequence alike.
    #[test]
    fn a_caret_inside_a_cluster_sits_at_its_head() {
        for (text, inside) in [("a🇹🇷b", 2), ("a👨\u{200D}👩\u{200D}👧b", 3), ("a👍🏽b", 2)]
        {
            let mut state = live("", text, "", inside);
            state.cluster = true;
            let (_, dock) = draw(&state, COLS);
            assert_eq!(dock.caret, caret_at(TEXT_COL + 1), "{text:?} @ {inside}");
        }
    }

    /// The row count comes from the same walk: `👍🏽` fits in the last two
    /// columns and the band stays one row; in the off reading the skin tone
    /// drops to the next row.
    #[test]
    fn a_cluster_on_the_last_two_columns_keeps_one_row() {
        // `TEXT_COL + 2` letters + the cluster = a completely full row; caret at the start.
        let cols = TEXT_COL + 4;
        let mut state = live("", "ab👍🏽", "", 0);
        state.cluster = true;
        assert_eq!(needed_rows(&state, cols), 1);
        state.cluster = false;
        assert_eq!(needed_rows(&state, cols), 2);
    }

    // ---- The typing animations' edit ----
    //
    // One test per row of the edit rule's table.

    /// The row the user typed: `PREDISPLAY` empty, the caret in `BUFFER`, the
    /// stamp `answers`.
    fn typed(buffer: &str, cursor: usize, answers: u64) -> DockState {
        DockState {
            answers,
            ..live("", buffer, "", cursor)
        }
    }

    /// The caret at the end of the row.
    fn at_end(buffer: &str, answers: u64) -> DockState {
        typed(buffer, buffer.chars().count(), answers)
    }

    /// The empty mirror after `line-finish`, with its stamp (the `End` arm).
    fn idle(answers: u64) -> DockState {
        DockState {
            status: DockStatus::Idle,
            answers,
            ..DockState::default()
        }
    }

    /// From the old mirror to the new: the production order — gate, then
    /// drawing — and the edits the drawing printed.
    fn edits_between(old: &DockState, new: &DockState, cols: u16) -> Vec<DockEdit> {
        let change = change(old, new);
        let owned = caret_home(None, new.status, false) == CaretHome::Dock;
        let mut edits = Vec::new();
        render(
            new,
            &DockContext::default(),
            None,
            &THEME,
            same(cols),
            owned,
            change.as_ref(),
            |_| (),
            |edit| edits.push(edit),
        );
        edits
    }

    /// The edit's characters and columns; `Shift` and `Reset` → `None`.
    fn glyphs(edit: &DockEdit) -> Option<(u16, String, Vec<u16>)> {
        let (col, cells) = match edit {
            DockEdit::Arrive { col, cells, .. }
            | DockEdit::Erase {
                col, ghosts: cells, ..
            } => (*col, cells.as_slice()),
            DockEdit::Shift { .. } | DockEdit::Reset => return None,
        };
        Some((
            col,
            cells.iter().filter_map(|cell| cell.ch).collect(),
            cells.iter().map(|cell| cell.col).collect(),
        ))
    }

    fn only(edits: &[DockEdit]) -> &DockEdit {
        assert_eq!(
            edits.len(),
            1,
            "a single edit was expected in the frame: {edits:?}"
        );
        &edits[0]
    }

    fn arrive(edits: &[DockEdit]) -> (u16, String) {
        match only(edits) {
            edit @ DockEdit::Arrive { .. } => {
                let (col, text, _) = glyphs(edit).expect("arrival");
                (col, text)
            }
            other => panic!("an arrival was expected: {other:?}"),
        }
    }

    fn erase(edits: &[DockEdit]) -> (u16, String) {
        match only(edits) {
            edit @ DockEdit::Erase { .. } => {
                let (col, text, _) = glyphs(edit).expect("deletion");
                (col, text)
            }
            other => panic!("a deletion was expected: {other:?}"),
        }
    }

    fn reset(edits: &[DockEdit], label: &str) {
        assert_eq!(edits, [DockEdit::Reset], "{label}");
    }

    #[test]
    fn a_typed_letter_arrives_left_of_the_caret() {
        let edits = edits_between(&at_end("l", 1), &at_end("ls", 2), COLS);
        assert_eq!(arrive(&edits), (TEXT_COL + 1, "s".into()));
    }

    #[test]
    fn two_keys_in_one_frame_arrive_together() {
        // The base is the last **drawn** mirror: the mirror in between was skipped, two keys one run.
        let edits = edits_between(&at_end("l", 1), &at_end("lsa", 3), COLS);
        assert_eq!(arrive(&edits), (TEXT_COL + 1, "sa".into()));
    }

    #[test]
    fn backspace_leaves_a_ghost_at_the_caret() {
        let edits = edits_between(&at_end("ls", 2), &at_end("l", 3), COLS);
        assert_eq!(erase(&edits), (TEXT_COL + 1, "s".into()));
        // Held Backspace: two deletions in one frame, the ghosts going to the right.
        let edits = edits_between(&at_end("lsa", 3), &at_end("l", 5), COLS);
        let DockEdit::Erase { ghosts, .. } = only(&edits) else {
            panic!("{edits:?}");
        };
        let cols: Vec<u16> = ghosts.as_slice().iter().map(|cell| cell.col).collect();
        assert_eq!(cols, [TEXT_COL + 1, TEXT_COL + 2]);
    }

    #[test]
    fn forward_delete_leaves_its_ghost_at_the_caret_too() {
        // `lsa`, the caret over `s`, forward delete: the caret stays in place.
        let edits = edits_between(&typed("lsa", 1, 1), &typed("la", 1, 2), COLS);
        assert_eq!(erase(&edits), (TEXT_COL + 1, "s".into()));
    }

    #[test]
    fn bulk_changes_do_not_animate() {
        for (label, old, new) in [
            // A single input, many glyphs.
            ("paste", at_end("", 1), at_end("hello", 2)),
            ("Ctrl-U", at_end("git status", 2), at_end("", 3)),
            (
                "Tab completion",
                at_end("git st", 1),
                at_end("git status", 2),
            ),
            // Neither an insertion nor a deletion: a replacement.
            ("history", at_end("ls", 2), at_end("git status", 3)),
            ("equal-length history", at_end("ab", 2), at_end("cd", 3)),
            // At the capacity limit: even if the inputs suffice, nine glyphs do not read as typing.
            ("capacity", at_end("", 0), at_end("abcdefghi", 9)),
        ] {
            reset(&edits_between(&old, &new, COLS), label);
        }
    }

    #[test]
    fn a_one_char_completion_animates_like_typing() {
        let edits = edits_between(&at_end("cd src", 1), &at_end("cd src/", 2), COLS);
        assert_eq!(arrive(&edits), (TEXT_COL + 6, "/".into()));
    }

    #[test]
    fn a_dead_key_is_two_inputs_for_one_glyph() {
        let edits = edits_between(&at_end("", 0), &at_end("~", 2), COLS);
        assert_eq!(arrive(&edits), (TEXT_COL, "~".into()));
    }

    #[test]
    fn a_combining_mark_is_not_a_glyph() {
        // `❤️` is two code points, one input (emoji palette): the glyph counted is one.
        let edits = edits_between(&at_end("", 0), &at_end("❤\u{FE0F}", 1), COLS);
        assert_eq!(arrive(&edits), (TEXT_COL, "❤".into()));
    }

    #[test]
    fn a_mirror_without_input_changes_nothing() {
        // Neither the stamp nor the status changed: the gate is closed and
        // `diff` does not run at all — even if the content differs (a mirror
        // without input: a prompt refresh, the timer).
        let old = at_end("ls", 4);
        let new = at_end("ls -la", 4);
        assert_eq!(change(&old, &new), None);
        assert!(edits_between(&old, &new, COLS).is_empty());
    }

    #[test]
    fn a_new_suggestion_over_the_same_buffer_draws_nothing() {
        let old = at_end("l", 1);
        let new = DockState {
            answers: 2,
            ..live("", "l", "s -la", 1)
        };
        assert_eq!(change(&old, &new), Some(Change::Same));
        assert!(edits_between(&old, &new, COLS).is_empty());
    }

    #[test]
    fn leaving_live_resets() {
        // Enter: `line-finish` drops the mirror to `Idle`, the line has moved to the grid.
        reset(
            &edits_between(&at_end("ls", 2), &idle(3), COLS),
            "Live → Idle",
        );
        let broken = DockState {
            status: DockStatus::Unavailable(DockFault::Malformed),
            ..DockState::default()
        };
        reset(
            &edits_between(&at_end("ls", 2), &broken, COLS),
            "Live → Unavailable",
        );
        reset(
            &edits_between(&broken, &at_end("l", 1), COLS),
            "Unavailable → Live",
        );
    }

    #[test]
    fn the_first_letter_after_the_prompt_arrives() {
        // Base `Idle`, empty row: had the rule been "both sides Live" the
        // first letter of every command would not have come alive.
        let edits = edits_between(&idle(5), &at_end("l", 6), COLS);
        assert_eq!(arrive(&edits), (TEXT_COL, "l".into()));
    }

    #[test]
    fn a_paste_as_the_first_action_does_not_animate() {
        // The `Idle` base is stamped (the `End` arm): a single input, exceeds five glyphs.
        reset(
            &edits_between(&idle(5), &at_end("hello", 6), COLS),
            "the first paste at the prompt",
        );
    }

    #[test]
    fn a_wide_char_arrives_as_one_glyph_over_two_columns() {
        let edits = edits_between(&at_end("a", 1), &at_end("a漢", 2), COLS);
        let DockEdit::Arrive { col, cells, .. } = only(&edits) else {
            panic!("{edits:?}");
        };
        assert_eq!(*col, TEXT_COL + 1);
        let [lead] = cells.as_slice() else {
            panic!("a single cell was expected: {cells:?}");
        };
        assert_eq!(
            (lead.ch, lead.col, lead.wide),
            (Some('漢'), TEXT_COL + 1, true)
        );
        // On deletion too a single ghost, two columns.
        let edits = edits_between(&at_end("a漢", 2), &at_end("a", 3), COLS);
        let DockEdit::Erase { ghosts, .. } = only(&edits) else {
            panic!("{edits:?}");
        };
        assert!(ghosts.as_slice()[0].wide, "{ghosts:?}");
    }

    #[test]
    fn a_typed_space_still_marks_its_column() {
        // A space is not a glyph but it shifts a column: the ending of
        // in-flight arrivals looks at this column.
        let edits = edits_between(&at_end("ls", 1), &at_end("ls ", 2), COLS);
        let DockEdit::Arrive { col, cells, .. } = only(&edits) else {
            panic!("{edits:?}");
        };
        assert_eq!(*col, TEXT_COL + 2);
        assert!(cells.as_slice().is_empty(), "{cells:?}");
    }

    #[test]
    fn edits_follow_the_prompt_width() {
        // `PREDISPLAY` pushes the text right; the column comes from the layout itself.
        let old = DockState {
            answers: 1,
            ..live("% ", "l", "", 3)
        };
        let new = DockState {
            answers: 2,
            ..live("% ", "ls", "", 4)
        };
        assert_eq!(
            arrive(&edits_between(&old, &new, COLS)),
            (TEXT_COL + 3, "s".into())
        );
        // If `PREDISPLAY` changed the text has shifted: no coming alive.
        let moved = DockState {
            answers: 2,
            ..live("%% ", "ls", "", 5)
        };
        reset(&edits_between(&old, &moved, COLS), "PREDISPLAY changed");
    }

    #[test]
    fn a_ghost_keeps_the_color_of_the_old_line() {
        let mut old = at_end("ls", 2);
        old.highlights.push(Highlight {
            start: 1,
            end: 2,
            style: HighlightStyle {
                fg: Some(HighlightColor::Indexed(2)),
                ..HighlightStyle::default()
            },
        });
        // The new row has no highlight: the color is only in the old buffer.
        let edits = edits_between(&old, &at_end("l", 3), COLS);
        let DockEdit::Erase { ghosts, .. } = only(&edits) else {
            panic!("{edits:?}");
        };
        assert_eq!(ghosts.as_slice()[0].fg, THEME.indexed_linear(2));
    }

    /// [`edits_between`] with the input row count coming from the caller: so
    /// that the rows of a wrapped input fit the window (in a one-row window the
    /// second row is not drawn and its effect is not born).
    fn edits_in_rows(old: &DockState, new: &DockState, cols: u16, rows: u16) -> Vec<DockEdit> {
        let change = change(old, new);
        let owned = caret_home(None, new.status, false) == CaretHome::Dock;
        let mut edits = Vec::new();
        render_with(
            new,
            &DockContext::default(),
            None,
            &THEME,
            same(cols),
            Some(rows),
            None,
            owned,
            None,
            None,
            change.as_ref(),
            &mut Vec::new(),
            &mut Clusters::default(),
            |_| (),
            |edit| edits.push(edit),
        );
        edits
    }

    type Placement = ((u16, u16), Vec<(u16, u16, char)>);

    /// The edit's position and cells as `(row, column, character)`.
    fn placed(edit: &DockEdit) -> Placement {
        let (at, cells) = match edit {
            DockEdit::Arrive {
                row, col, cells, ..
            } => ((*row, *col), cells.as_slice()),
            DockEdit::Erase {
                row, col, ghosts, ..
            } => ((*row, *col), ghosts.as_slice()),
            other => panic!("an arrival or a deletion was expected: {other:?}"),
        };
        let cells = cells
            .iter()
            .filter_map(|cell| cell.ch.map(|ch| (cell.row, cell.col, ch)))
            .collect();
        (at, cells)
    }

    /// Effects at **(row, column)** positions in a wrapped input: the
    /// letter filling the row comes with its effect as it wraps
    /// to the next row, Backspace on the second row leaves its ghost on that
    /// row, the ghosts of a multi-character deletion drop to the next row with
    /// the old layout's wrapping. Sliding letters (the tail changing row by
    /// wrapping) do not enter the edit — they are without animation at their
    /// new positions.
    #[test]
    fn effects_land_on_their_row_and_column_across_a_wrapped_line() {
        let cols = TEXT_COL + 4;
        // The row is full (`abcd`), `e` wraps to the start of the next row.
        let edits = edits_in_rows(&at_end("abcd", 1), &at_end("abcde", 2), cols, 2);
        let (at, cells) = placed(only(&edits));
        assert!(matches!(edits[0], DockEdit::Arrive { .. }), "{edits:?}");
        assert_eq!(at, (1, TEXT_COL));
        assert_eq!(cells, [(1, TEXT_COL, 'e')]);
        // The letter filling the row: on its own row, the caret drops to the next row.
        let edits = edits_in_rows(&at_end("abc", 1), &at_end("abcd", 2), cols, 2);
        assert_eq!(placed(only(&edits)).1, [(0, TEXT_COL + 3, 'd')]);
        // Backspace on the second row: the ghost on the second row, at the caret's column.
        let edits = edits_in_rows(&at_end("abcdef", 1), &at_end("abcde", 2), cols, 2);
        assert!(matches!(edits[0], DockEdit::Erase { .. }), "{edits:?}");
        let (at, ghosts) = placed(only(&edits));
        assert_eq!(at, (1, TEXT_COL + 1));
        assert_eq!(ghosts, [(1, TEXT_COL + 1, 'f')]);
        // Forward delete on the first row: the tail wraps up a row but the
        // edit is only the deleted letter.
        let edits = edits_in_rows(&typed("abcdefgh", 1, 1), &typed("acdefgh", 1, 2), cols, 2);
        assert_eq!(placed(only(&edits)).1, [(0, TEXT_COL + 1, 'b')]);
        // A three-letter deletion crosses the row end: the ghosts at the old
        // layout — two at the end of the first row, the third at the start of the next row.
        let edits = edits_in_rows(&typed("abcdef", 2, 1), &typed("abf", 2, 4), cols, 2);
        assert_eq!(
            placed(only(&edits)).1,
            [
                (0, TEXT_COL + 2, 'c'),
                (0, TEXT_COL + 3, 'd'),
                (1, TEXT_COL, 'e')
            ]
        );
        // No shift: the window's top did not change.
        let DockEdit::Erase { shift, .. } = only(&edits) else {
            panic!("{edits:?}");
        };
        assert_eq!(*shift, 0);
    }

    /// The vertical window's shift is in rows and **inside** the edit: `bt-gpu`
    /// takes a single edit per frame, a separate `Shift` would overwrite it.
    /// If the text did not change the shift goes alone; a frame with no shift
    /// leaves the edit as it is.
    #[test]
    fn the_window_shift_rides_on_the_edit_in_rows() {
        let cols = TEXT_COL + 4;
        let edits = edits_in_rows(&at_end("abcdefgh", 1), &at_end("abcdefghi", 2), cols, 2);
        let edit = *only(&edits);
        assert_eq!(with_shift(Some(edit), 0), Some(edit));
        let Some(DockEdit::Arrive { row, shift, .. }) = with_shift(Some(edit), -1) else {
            panic!("{edit:?}");
        };
        assert_eq!((row, shift), (1, -1));
        assert_eq!(with_shift(None, -1), Some(DockEdit::Shift { by: -1 }));
        assert_eq!(with_shift(None, 0), None);
        assert_eq!(with_shift(Some(DockEdit::Reset), 2), Some(DockEdit::Reset));
    }

    /// Typing on the row under `PREBUFFER`: the effect on `BUFFER`'s row, as
    /// many rows down as `PREBUFFER`'s rows.
    #[test]
    fn an_edit_under_the_prebuffer_lands_on_the_buffer_row() {
        let cols = TEXT_COL + 20;
        let old = DockState {
            prebuffer: "for i in 1 2\n".into(),
            ..at_end("ech", 1)
        };
        let new = DockState {
            prebuffer: "for i in 1 2\n".into(),
            ..at_end("echo", 2)
        };
        let edits = edits_in_rows(&old, &new, cols, 2);
        assert_eq!(placed(only(&edits)).1, [(1, TEXT_COL + 3, 'o')]);
        // `PREBUFFER` changed (ZLE accepted one more row): no coming alive.
        let accepted = DockState {
            prebuffer: "for i in 1 2\ndo\n".into(),
            ..at_end("echo", 3)
        };
        reset(
            &edits_in_rows(&new, &accepted, cols, 3),
            "PREBUFFER changed",
        );
    }

    /// An input that wraps with the suggestion but whose text is a single row
    /// still comes alive: the "single row?" gate is from the band's measure
    /// (suggestion excluded), or a long history suggestion would reset every
    /// key's effect.
    #[test]
    fn a_wrapping_suggestion_does_not_stop_the_effects() {
        let cols = TEXT_COL + 4;
        let old = at_end("a", 1);
        let new = DockState {
            answers: 2,
            ..live("", "ab", "cdefgh", 2)
        };
        assert_eq!(
            arrive(&edits_between(&old, &new, cols)),
            (TEXT_COL + 1, "b".into())
        );
    }

    #[test]
    fn typing_on_one_row_animates_without_a_shift() {
        let cols = TEXT_COL + 4;
        let edits = edits_between(&at_end("ab", 1), &at_end("abc", 2), cols);
        assert_eq!(arrive(&edits), (TEXT_COL + 2, "c".into()));
        let DockEdit::Arrive { shift, .. } = only(&edits) else {
            panic!("{edits:?}");
        };
        assert_eq!(*shift, 0);
    }

    #[test]
    fn a_caret_move_over_a_wrapped_line_draws_no_edit() {
        // The text is the same, the caret moved between rows: wrapping is a
        // display decision, not an edit — nothing is printed.
        let cols = TEXT_COL + 4;
        let edits = edits_between(&typed("abcdefgh", 8, 1), &typed("abcdefgh", 2, 2), cols);
        assert!(edits.is_empty(), "{edits:?}");
    }

    #[test]
    fn a_line_owned_by_the_grid_does_not_animate() {
        // If the caret is in the grid the row is there too: the effect's subject is typing in the dock.
        let old = at_end("l", 1);
        let new = at_end("ls", 2);
        let change = change(&old, &new);
        let mut edits = Vec::new();
        render(
            &new,
            &DockContext::default(),
            None,
            &THEME,
            same(COLS),
            false,
            change.as_ref(),
            |_| (),
            |edit| edits.push(edit),
        );
        reset(&edits, "caret in the grid");
    }

    // ---- Mouse selection ----

    /// Drawing with a selection and a single input row: cells, surface and runs.
    fn draw_selected(
        state: &DockState,
        cols: u16,
        selection: Option<(usize, usize)>,
    ) -> (Vec<Cell>, Dock, Vec<SelectionRun>) {
        let (cells, dock, runs, _) = draw_rows(state, cols, CONTEXT_ROW, selection);
        (cells, dock, runs)
    }

    fn run(row: u16, first: u16, last: u16) -> SelectionRun {
        SelectionRun { row, first, last }
    }

    fn point(index: usize, half: CellHalf) -> DockPoint {
        DockPoint { index, half }
    }

    /// The hit test on the **drawn** block: including a wrapped row, a wide
    /// character that does not fit the row end and drops to the next row, and
    /// the continuation row's hanging indent, the (row, column) of every drawn
    /// cell lands on that cell's character. The expectation comes from
    /// `render`'s own output — not a hand-written table, so it goes red the
    /// day the two walks diverge.
    #[test]
    fn the_hit_test_lands_on_the_character_drawn_there() {
        // Twelve columns: ten for the text. `% a界bcde` is nine columns, `漢`
        // does not fit and drops to the next row — the first row's last column is empty.
        let buffer = "a界bcde漢fghi";
        let chars: Vec<char> = buffer.chars().collect();
        let state = live("% ", buffer, "ZQ", 2 + chars.len());
        let cols = TEXT_COL + 10;
        let (cells, _, _, top) = draw_rows(&state, cols, 2, None);
        assert_eq!(top, 0);
        let wrapped = cells
            .iter()
            .find(|cell| cell.ch == Some('漢'))
            .expect("漢 must be drawn");
        assert_eq!(
            (wrapped.row, wrapped.col),
            (1, TEXT_COL),
            "the test does not test wrapping"
        );
        let at = |row, col, half| hit(&state, top, cols, row, col, half).expect("no hit");
        let mut drawn = 0;
        for lead in cells.iter().filter(|cell| cell.ch.is_some()) {
            let ch = lead.ch.unwrap_or(' ');
            // The suggestion lands at the end of `BUFFER`.
            if "ZQ".contains(ch) {
                let hit = at(lead.row, lead.col, CellHalf::Left);
                assert!(hit.index >= chars.len(), "{ch}: {hit:?}");
                continue;
            }
            if "% ".contains(ch) {
                assert_eq!(
                    at(lead.row, lead.col, CellHalf::Right),
                    point(0, CellHalf::Left)
                );
                continue;
            }
            drawn += 1;
            let left = at(lead.row, lead.col, CellHalf::Left);
            assert_eq!(
                chars[left.index], ch,
                "({}, {}) landed on another character",
                lead.row, lead.col
            );
            assert_eq!(left.half, CellHalf::Left);
            // A wide character's **spacer** column is the same character's right half.
            let last = lead.col + u16::from(lead.wide);
            assert_eq!(
                at(lead.row, last, CellHalf::Right),
                point(left.index, CellHalf::Right),
                "{ch}"
            );
            if lead.wide {
                assert_eq!(
                    at(lead.row, last, CellHalf::Left),
                    point(left.index, CellHalf::Right),
                    "{ch}"
                );
            }
        }
        assert_eq!(
            drawn,
            chars.len(),
            "a drawn character is missing: {cells:?}"
        );
        // The empty column at the end of the first row: `BUFFER` continues on
        // the row below, i.e. `e`'s right half — the rule of a wrapped row in the grid.
        assert_eq!(
            at(0, TEXT_COL + 9, CellHalf::Left),
            point(5, CellHalf::Right)
        );
        // The continuation row's hanging indent lands on that row's first character.
        assert_eq!(at(1, 0, CellHalf::Right), point(6, CellHalf::Left));
    }

    #[test]
    fn the_hit_test_maps_the_prompt_and_the_blank_tail_to_the_buffer_ends() {
        let state = live("% ", "ls", "", 4);
        let (_, _, _, top) = draw_rows(&state, COLS, 1, None);
        let at = |col, half| hit(&state, top, COLS, 0, col, half);
        // `PREDISPLAY` is not selectable: it lands at the start.
        assert_eq!(
            at(TEXT_COL + 1, CellHalf::Right),
            Some(point(0, CellHalf::Left))
        );
        // The blank to the right of the text is the end of `BUFFER` and beyond:
        // the adjacent column `len`, a far column further beyond like an empty
        // cell of the grid.
        assert_eq!(
            at(TEXT_COL + 4, CellHalf::Left),
            Some(point(2, CellHalf::Left))
        );
        let far = at(TEXT_COL + 20, CellHalf::Left).expect("no hit");
        assert_eq!(far, point(18, CellHalf::Left));
        // The boundary is clamped to `len`; the word takes the last word, as in
        // the grid, only when adjacent, nothing when far.
        assert_eq!(
            selection_range("ls", SelectKind::Simple, far, far, false),
            (2, 2)
        );
        assert_eq!(
            selection_range("ls", SelectKind::Word, far, far, false),
            (2, 2)
        );
        let near = point(2, CellHalf::Left);
        assert_eq!(
            selection_range("ls", SelectKind::Word, near, near, false),
            (0, 2)
        );
        // There is no text to select in a non-`Live` mirror.
        let idle = DockState {
            status: DockStatus::Idle,
            ..DockState::default()
        };
        assert_eq!(hit(&idle, 0, COLS, 0, TEXT_COL, CellHalf::Left), None);
    }

    /// The selection looks like the grid's: a single-row run, the blank
    /// between words bridged, the blank in the tail unhighlighted, both halves
    /// of a wide character inside; the selected text in its own foreground,
    /// reverse video resolved and the ground dropped.
    #[test]
    fn a_dock_selection_is_one_run_over_what_is_drawn() {
        let mut state = live("% ", "ls 漢 x  ", "", 2);
        state.highlights.push(Highlight {
            start: 2,
            end: 4,
            style: HighlightStyle {
                standout: true,
                ..HighlightStyle::default()
            },
        });
        // All of `BUFFER`: the two spaces in the tail are selected but not drawable.
        let len = state.buffer.chars().count();
        let (cells, _, runs) = draw_selected(&state, COLS, Some((0, len)));
        // `l` is at the text's column 2 (`% ` prefix), `x` at 8 (`漢` is two columns).
        assert_eq!(runs, [run(0, TEXT_COL + 2, TEXT_COL + 8)]);
        let l = cells.iter().find(|cell| cell.ch == Some('l')).expect("l");
        assert_eq!(
            l.fg,
            THEME.foreground_linear(),
            "reverse video was not resolved"
        );
        assert_eq!(l.bg, None, "the selected cell's ground did not drop");

        // On a wide character the end is the spacer's column.
        let (_, _, runs) = draw_selected(&state, COLS, Some((3, 4)));
        assert_eq!(runs, [run(0, TEXT_COL + 5, TEXT_COL + 6)]);
        // Only blanks: nothing to draw, no run (it creates no content).
        let (_, _, runs) = draw_selected(&state, COLS, Some((7, 9)));
        assert!(runs.is_empty(), "{runs:?}");
        // The row without a selection keeps its standout.
        let (cells, _, runs) = draw_selected(&state, COLS, None);
        assert!(runs.is_empty());
        let l = cells.iter().find(|cell| cell.ch == Some('l')).expect("l");
        assert_eq!(l.bg, Some(THEME.foreground_linear()));
    }

    /// **A selection across rows is one run per visual row**:
    /// the same shape as the grid's runs, so the drawing side draws the
    /// corners as one piece by looking at the neighboring row's run.
    #[test]
    fn a_dock_selection_across_wrapped_rows_is_one_run_per_row() {
        let cols = TEXT_COL + 8;
        let state = live("", "abcdefghijkl", "", 0);
        let (_, _, runs, _) = draw_rows(&state, cols, 2, Some((5, 10)));
        assert_eq!(
            runs,
            [
                run(0, TEXT_COL + 5, TEXT_COL + 7),
                run(1, TEXT_COL, TEXT_COL + 1)
            ]
        );
    }

    /// On a wide character that does not fit the row end the walk **moves to
    /// the next row** and the narrow character behind it lands next to it, it
    /// does not slip into the first row's empty column (the wrapping form of
    /// the old "the walk ends" guard; found in code review).
    #[test]
    fn a_wide_char_that_does_not_fit_wraps_and_the_next_follows_it() {
        let mut placed = Vec::new();
        layout_with(
            "abc漢d".chars().map(|ch| (ch, ())),
            0,
            4,
            0,
            0,
            false,
            |_| {},
            |at| placed.push((at.ch, at.row, at.col)),
        );
        assert_eq!(
            placed,
            [
                ('a', 0, 0),
                ('b', 0, 1),
                ('c', 0, 2),
                ('漢', 1, 0),
                ('d', 1, 2)
            ]
        );
    }

    #[test]
    fn a_dock_selection_outside_the_vertical_window_draws_nothing() {
        // Caret at the end, window two rows: `BUFFER`'s first row is not on screen.
        let buffer = "abcdefghijklmnop";
        let cols = TEXT_COL + 8;
        let state = live("", buffer, "", buffer.len());
        let (_, _, runs, top) = draw_rows(&state, cols, 2, Some((0, 3)));
        assert_eq!(
            top, 1,
            "the caret is behind a completely full row: the third row"
        );
        assert!(runs.is_empty(), "{runs:?}");
        // A partly visible selection is clamped to the window's rows.
        let (_, _, runs, _) = draw_rows(&state, cols, 2, Some((0, buffer.len())));
        assert_eq!(runs, [run(0, TEXT_COL, TEXT_COL + 7)]);
    }

    #[test]
    fn simple_and_line_selections_resolve_to_buffer_ranges() {
        let buffer = "ls -la";
        let range = |kind, a, b| selection_range(buffer, kind, a, b, false);
        // A click without drag is empty.
        let at = point(2, CellHalf::Left);
        assert_eq!(range(SelectKind::Simple, at, at), (2, 2));
        // From the left half to the right half: both ends included.
        assert_eq!(
            range(
                SelectKind::Simple,
                point(3, CellHalf::Left),
                point(5, CellHalf::Right)
            ),
            (3, 6)
        );
        // The reverse direction, the same range.
        assert_eq!(
            range(
                SelectKind::Simple,
                point(5, CellHalf::Right),
                point(3, CellHalf::Left)
            ),
            (3, 6)
        );
        // Line: on a single logical row the whole of `BUFFER`, independent of the point.
        assert_eq!(range(SelectKind::Line, at, at), (0, 6));
        // In a `BUFFER` with line breaks the **logical row**:
        // between `\n`s, line break excluded; if the two ends are on two rows, both and what is between.
        let lines = "echo a\necho b\nx";
        let line = |a: usize, b: usize| {
            selection_range(
                lines,
                SelectKind::Line,
                point(a, CellHalf::Left),
                point(b, CellHalf::Left),
                false,
            )
        };
        assert_eq!(line(9, 9), (7, 13));
        assert_eq!(line(0, 0), (0, 6));
        assert_eq!(line(2, 14), (0, 15));
        assert_eq!(line(6, 6), (0, 6), "on the line break is its own row");
        assert_eq!(line(40, 40), (14, 15), "beyond the end is the last row");
        // The right half takes the combiner along with its character.
        let composed = "e\u{301}x";
        assert_eq!(
            selection_range(
                composed,
                SelectKind::Simple,
                point(0, CellHalf::Left),
                point(0, CellHalf::Right),
                false
            ),
            (0, 2)
        );
    }

    #[test]
    fn a_word_selection_follows_alacritty_semantic_rules() {
        let word = |buffer: &str, index: usize| {
            let at = point(index, CellHalf::Left);
            let (start, end) = selection_range(buffer, SelectKind::Word, at, at, false);
            buffer
                .chars()
                .skip(start)
                .take(end - start)
                .collect::<String>()
        };
        // A path, `host:port` and both sides of `=`.
        assert_eq!(word("cd ~/src/a-b.rs", 5), "~/src/a-b.rs");
        assert_eq!(word("ssh me@host:22", 6), "me@host:22");
        assert_eq!(word("KEY=value", 6), "value");
        // A double click on a separator takes the words on both sides.
        assert_eq!(word("foo bar baz", 3), "foo bar");
        // It finds the bracket's pair, nested too.
        assert_eq!(word("f (a (b) c) x", 2), "(a (b) c)");
        assert_eq!(word("f (a (b) c) x", 10), "(a (b) c)");
        // The blank at the end of the row takes the last word (an empty cell in the grid).
        assert_eq!(word("git status", 10), "status");
        // A dragged word selection widens both ends.
        let (start, end) = selection_range(
            "one two three",
            SelectKind::Word,
            point(1, CellHalf::Left),
            point(9, CellHalf::Left),
            false,
        );
        assert_eq!((start, end), (0, 13));
    }

    // ---- Drawing and editing a cluster ----

    /// A clustered mirror (`cluster` on), the caret at the end of the row.
    fn clustered(buffer: &str, answers: u64) -> DockState {
        DockState {
            cluster: true,
            ..at_end(buffer, answers)
        }
    }

    /// Clustered drawing from the old mirror to the new: cells, edits and the table.
    fn clustered_render(old: &DockState, new: &DockState) -> (Vec<Cell>, Vec<DockEdit>, Clusters) {
        let change = change(old, new);
        let (mut cells, mut edits, mut clusters) = (Vec::new(), Vec::new(), Clusters::default());
        render_with(
            new,
            &DockContext::default(),
            None,
            &THEME,
            same(COLS),
            Some(CONTEXT_ROW),
            None,
            true,
            None,
            None,
            change.as_ref(),
            &mut Vec::new(),
            &mut clusters,
            |cell| cells.push(cell),
            |edit| edits.push(edit),
        );
        (cells, edits, clusters)
    }

    fn cluster_text(clusters: &Clusters, cell: &Cell) -> Option<String> {
        cell.cluster
            .and_then(|id| clusters.get(id))
            .map(str::to_owned)
    }

    #[test]
    fn a_dock_cluster_reaches_the_sink_as_one_string() {
        let state = clustered("a🇹🇷e\u{301}", 1);
        let (cells, _, clusters) = clustered_render(&state, &state);
        let texts: Vec<(Option<char>, Option<String>)> = cells
            .iter()
            .filter(|cell| cell.ch.is_some())
            .map(|cell| (cell.ch, cluster_text(&clusters, cell)))
            .collect();
        assert_eq!(
            texts,
            vec![
                (Some('a'), None),
                (Some('🇹'), Some("🇹🇷".into())),
                // A one-column combiner with the base character.
                (Some('e'), None),
            ]
        );
    }

    #[test]
    fn a_whole_cluster_erased_leaves_one_clustered_ghost() {
        // The widget deletes `[S,E)`: a single input, a single glyph, a single ghost.
        let (_, edits, clusters) = clustered_render(&clustered("a🇹🇷", 1), &clustered("a", 2));
        let DockEdit::Erase { ghosts, .. } = only(&edits) else {
            panic!("a deletion was expected: {edits:?}");
        };
        let ghosts = ghosts.as_slice();
        assert_eq!(ghosts.len(), 1, "{ghosts:?}");
        assert!(ghosts[0].wide);
        assert_eq!(cluster_text(&clusters, &ghosts[0]).as_deref(), Some("🇹🇷"));
        // The arriving flag is also a single glyph and with its cluster.
        let (_, edits, clusters) = clustered_render(&clustered("a", 1), &clustered("a🇹🇷", 2));
        let DockEdit::Arrive { cells, .. } = only(&edits) else {
            panic!("an arrival was expected: {edits:?}");
        };
        assert_eq!(cells.as_slice().len(), 1);
        assert_eq!(
            cluster_text(&clusters, &cells.as_slice()[0]).as_deref(),
            Some("🇹🇷")
        );
    }

    #[test]
    fn an_edit_inside_a_cluster_does_not_animate_half_of_it() {
        // With the gate closed ZLE deletes a code point: half a flag does not
        // come alive, the text changes instantly. Adding a skin tone is not a new glyph either.
        for (old, new) in [("a🇹🇷", "a🇹"), ("a👍", "a👍🏽"), ("🇹x🇷", "🇹🇷")]
        {
            let (_, edits, _) = clustered_render(&clustered(old, 1), &clustered(new, 2));
            assert_eq!(edits, vec![DockEdit::Reset], "{old:?} → {new:?}");
        }
        // With clustering off as today: half a flag, a single RI's deletion.
        let (old, new) = (at_end("a🇹🇷", 1), at_end("a🇹", 2));
        assert!(matches!(
            only(&edits_between(&old, &new, COLS)),
            DockEdit::Erase { .. }
        ));
    }

    #[test]
    fn selection_ends_snap_to_cluster_bounds() {
        let point = |index, half| DockPoint { index, half };
        // `a🇹🇷b`: the flag's right half lands behind it, the left half in front.
        let simple = |a, h, cluster| selection_range("a🇹🇷b", SelectKind::Simple, a, h, cluster);
        assert_eq!(
            simple(point(0, CellHalf::Left), point(1, CellHalf::Right), true),
            (0, 3)
        );
        assert_eq!(
            simple(point(4, CellHalf::Left), point(2, CellHalf::Left), true),
            (1, 4),
            "an end falling inside a cluster goes to its start"
        );
        assert_eq!(
            simple(point(0, CellHalf::Left), point(1, CellHalf::Right), false),
            (0, 2),
            "the off reading is the code point"
        );
        // A double click takes the whole flag.
        let at = point(2, CellHalf::Left);
        assert_eq!(
            selection_range("a 🇹🇷 b", SelectKind::Word, at, at, true),
            (2, 4)
        );
    }

    #[test]
    fn the_right_half_of_a_cluster_hits_past_it() {
        // `🇹🇷` in the text column: a click on the right half is the head
        // character's right half and the boundary is behind the cluster — not between `🇹`/`🇷`.
        let state = clustered("🇹🇷", 1);
        let right = hit(&state, 0, COLS, 0, TEXT_COL + 1, CellHalf::Right).expect("hit");
        assert_eq!(
            right,
            DockPoint {
                index: 0,
                half: CellHalf::Right
            }
        );
        let chars: Vec<char> = state.buffer.chars().collect();
        assert_eq!(boundary(&chars, right, true), 2);
        // The blank to the right of the text goes beyond `BUFFER` (end +
        // distance, the word selection's "blank" rule): the last cluster is not
        // counted as the continuation of a wrapped row — were it counted the
        // answer would be the flag's right half.
        assert_eq!(
            hit(&state, 0, COLS, 0, TEXT_COL + 5, CellHalf::Left),
            Some(DockPoint {
                index: 5,
                half: CellHalf::Left
            }),
        );
    }
}
