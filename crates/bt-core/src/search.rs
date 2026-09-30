//! Scrollback search (⌘F, 033): compiling the query and the matches of the
//! visible rows.
//!
//! **There are two budgets and this module is the first's** (`.tasks/033-gecmiste-arama/
//! discussion.md` → Karar 2, Muhakeme): the highlight runs on every content
//! frame, within the `Term` lock turn that [`crate::Session::frame`] already
//! takes, over only the **drawn** rows — its cost is bounded by the screen's
//! height. Counting the whole scrollback is a separate path: an anchorless
//! index built bottom-up piece by piece ([`SearchIndex`],
//! [`crate::Session::search_step`]).
//!
//! The matcher itself is alacritty's (`RegexSearch`, `RegexIter`) and that type
//! is not visible in the `pub` API (`lib.rs` → the encapsulation contract): the
//! outside sees only [`SearchQuery`], [`SearchStatus`] and the result runs
//! ([`SearchRuns`]). There is no match across a hard line end — alacritty's
//! scanner resets its state at an unwrapped line end — and it skips the empty
//! match (`^`, `a*`) itself.

use std::ops::RangeInclusive;

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Boundary, Column, Direction, Line, Point, Side};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::{Cell as TermCell, Flags};
use alacritty_terminal::term::search::{Match, RegexIter, RegexSearch};

use crate::color::{LinearRgba, Theme};

/// The user's query: the text and the two switches in the panel.
///
/// Keeping it per tab and not writing it to the settings file is the caller's
/// job (Karar 6); this crate only compiles.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchQuery {
    /// The text searched for; **plain** text when `regex` is off ([`escape`]).
    pub text: String,
    /// The `.*` switch: the text is a regular expression.
    pub regex: bool,
    /// The `Aa` switch: when on, always case-sensitive; when off, **smart** —
    /// sensitive if the text has an uppercase letter, insensitive otherwise
    /// (alacritty's own rule, Karar 11).
    pub case_sensitive: bool,
}

/// The compiled state of the query — the input of the panel's label (Karar 3).
///
/// An invalid pattern is **a state, not a panic** (R1): the moment the user
/// types `(` the screen must not break, the label must say "Invalid pattern".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchStatus {
    /// Empty query: no scan, no highlight.
    Empty,
    /// A pattern that didn't compile (syntax, or alacritty's complexity
    /// limit); no scan.
    Invalid,
    /// The pattern compiled, the visible rows are scanned on every content frame.
    Ready,
}

/// `regex-syntax`'s meta characters — the same set as
/// `regex_syntax::is_meta_character`'s.
///
/// The set is a **copy** here and deliberately: making `regex-syntax` a direct
/// dependency would add an edge to `Cargo.lock` (Karar 11). The copy's guard is
/// a test: every character, once escaped, must match itself literally.
const META: &[char] = &[
    '\\', '.', '+', '*', '?', '(', ')', '|', '[', ']', '{', '}', '^', '$', '#', '&', '-', '~',
];

/// Turns plain text into a regular expression that matches **itself** as a
/// pattern: a backslash before every meta character.
///
/// `pub`, because its second consumer is ⌘E: in regex mode the selected text is
/// entered escaped (Karar 6).
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if META.contains(&ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Compiles the query: `Empty` if empty, `Invalid` if it doesn't compile,
/// otherwise the pattern.
///
/// Plain text is escaped first, then — if `Aa` is on — the `(?-i)` prefix comes:
/// in the reverse order the prefix itself would be escaped. The smart mode is
/// not separate code but alacritty's default (`RegexSearch::new` looks for an
/// uppercase letter in the pattern); the prefix only **forces** sensitivity.
pub(crate) fn compile(query: &SearchQuery) -> (SearchStatus, Option<RegexSearch>) {
    if query.text.is_empty() {
        return (SearchStatus::Empty, None);
    }
    let body = if query.regex {
        query.text.clone()
    } else {
        escape(&query.text)
    };
    let pattern = if query.case_sensitive {
        format!("(?-i){body}")
    } else {
        body
    };
    match RegexSearch::new(&pattern) {
        Ok(regex) => (SearchStatus::Ready, Some(regex)),
        Err(_) => (SearchStatus::Invalid, None),
    }
}

/// The most rows to look at, above and below the visible window, for the
/// continuation of a wrapped line.
///
/// **Why it exists:** if the visible top's row comes wrapped from above, the
/// match doesn't start there but at the line's logical start; had the scan
/// started at the top it would find a half match (the tail of `o+`) or miss a
/// whole match. The ceiling keeps a stream that emits no line ends (a
/// single-line file through `cat`) from spreading the scan over the whole
/// scrollback.
///
/// **Not measured**, a design constant: the same number as alacritty's own
/// visible search limit (`MAX_SEARCH_LINES`). A match starting in the middle of
/// a line cut at the ceiling is a hundred rows up and produces no visible run.
const WRAP_REACH: i32 = 100;

/// Whether the row's last cell carries the wrap flag — that is, whether the next
/// row is its continuation.
fn wraps<T>(term: &Term<T>, line: Line) -> bool {
    term.grid()[line]
        .last()
        .is_some_and(|cell| cell.flags.contains(Flags::WRAPLINE))
}

/// Yields the matches touching rows `top..=bottom`, left to right, top to
/// bottom; the range is extended to the logical ends of wrapped rows
/// ([`WRAP_REACH`]).
///
/// Called **while the `Term` lock is held** ([`crate::Session::frame`]). Both
/// ends must be inside the scrollback (`topmost_line..screen_lines`); the caller
/// builds them from its own channel's rows, so they are.
pub(crate) fn scan<T>(
    term: &Term<T>,
    regex: &mut RegexSearch,
    top: Line,
    bottom: Line,
    mut each: impl FnMut(&Match),
) {
    let highest = Line((top.0 - WRAP_REACH).max(term.topmost_line().0));
    let lowest = Line((bottom.0 + WRAP_REACH).min(term.bottommost_line().0));
    let mut start = top;
    while start > highest && wraps(term, Line(start.0 - 1)) {
        start = Line(start.0 - 1);
    }
    let mut end = bottom;
    while end < lowest && wraps(term, end) {
        end = Line(end.0 + 1);
    }
    let from = Point::new(start, Column(0));
    let to = Point::new(end, term.last_column());
    for found in RegexIter::new(from, to, Direction::Right, term, regex) {
        each(&found);
    }
}

/// Whether the cell has **ink**: a character that is not hidden, not a spacer,
/// not a blank.
fn inked(cell: &TermCell) -> bool {
    const BLANK: Flags = Flags::HIDDEN
        .union(Flags::WIDE_CHAR_SPACER)
        .union(Flags::LEADING_WIDE_CHAR_SPACER);
    !cell.flags.intersects(BLANK) && cell.c != ' '
}

/// Whether the match touches at least one **inked** cell.
///
/// **A highlight creates no content** (the selection's rule from 031, for
/// search): a match made only of blanks (the query ` `, `\s+`) would paint the
/// grid's empty rows and the invisible tail of line ends — blocks that say
/// "there is something here" on screen but show nothing. Hidden text (`\e[8m`)
/// isn't counted either: the place of undrawn text shouldn't be highlighted.
pub(crate) fn has_ink<T>(term: &Term<T>, found: &Match) -> bool {
    let (start, end) = (*found.start(), *found.end());
    (start.line.0..=end.line.0).any(|line| {
        let first = if line == start.line.0 {
            start.column.0
        } else {
            0
        };
        let last = if line == end.line.0 {
            end.column.0
        } else {
            usize::MAX
        };
        term.grid()[Line(line)]
            .into_iter()
            .enumerate()
            .skip(first)
            .take_while(|&(col, _)| col <= last)
            .any(|(_, cell)| inked(cell))
    })
}

/// One row's piece of the search highlight: columns `first..=last` on row `row`
/// (the space of [`crate::SelectionRun`]).
///
/// It has two bits more than a selection run and both are inputs of the
/// drawing (phase-2):
///
/// - `current` — the current match's run; drawn with the `search_current`
///   color, the others with `search_match`.
/// - `continues` — the run is the continuation **of the same match** of the run
///   on the previous row. Corners are computed per match (Karar 7): two
///   separate matches on consecutive rows mustn't fuse into one shape, a single
///   wrapped match must — this bit is the only thing that tells them apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchRun {
    pub row: u16,
    pub first: u16,
    /// Inclusive. For a wide character the spacer's column — both halves are
    /// highlighted.
    pub last: u16,
    pub current: bool,
    pub continues: bool,
}

/// [`crate::Session::frame`]'s search runs — the precedent of
/// [`crate::SelectionRuns`], a buffer the caller spreads across frames (no
/// per-frame allocation).
///
/// **Two lists, two coordinate spaces** (the precedent of [`crate::Blocks`]'s
/// `fill_slice`): the grid's screen rows and the fill channel's fill-local rows
/// (`0..top_row + fill`, the fraction's top row included,
/// [`crate::Cursor::top_row`]). The two are drawn in separate `setViewport`s.
///
/// When search is off, or the query is empty or invalid, both lists are
/// **empty** and the scan never runs (R2.2's stopping condition).
///
/// **The colors are ready from the boundary too** (the precedent of
/// [`crate::SelectionRuns`], 031 Karar 9): the two roles and their unfocused
/// counterparts are written from the theme copy `frame()` already takes; which
/// one gets drawn is the decision of `bt-gpu`, which knows the focus.
#[derive(Debug)]
pub struct SearchRuns {
    pub(crate) runs: Vec<SearchRun>,
    pub(crate) fill_runs: Vec<SearchRun>,
    pub(crate) colors: SearchColors,
}

/// [`SearchRuns`]'s four colors, linear: two roles × focus.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SearchColors {
    pub(crate) matched: LinearRgba,
    pub(crate) matched_unfocused: LinearRgba,
    pub(crate) current: LinearRgba,
    pub(crate) current_unfocused: LinearRgba,
}

impl SearchColors {
    pub(crate) const fn of(theme: &Theme) -> Self {
        Self {
            matched: theme.search_match_linear(),
            matched_unfocused: theme.search_match_unfocused_linear(),
            current: theme.search_current_linear(),
            current_unfocused: theme.search_current_unfocused_linear(),
        }
    }
}

/// An empty buffer with no runs; the colors are from the embedded theme, the
/// first frame writes over them (the same reasoning as [`crate::SelectionRuns`]'s
/// `Default`: `LinearRgba` has no `Default`, no color is invented).
impl Default for SearchRuns {
    fn default() -> Self {
        Self {
            runs: Vec::new(),
            fill_runs: Vec::new(),
            colors: SearchColors::of(&Theme::BATERI),
        }
    }
}

impl SearchRuns {
    /// The matches' highlight: `search_match` in a focused window, otherwise its
    /// counterpart faded toward the background.
    pub fn match_color(&self, focused: bool) -> LinearRgba {
        if focused {
            self.colors.matched
        } else {
            self.colors.matched_unfocused
        }
    }

    /// The current match's highlight; [`SearchRuns::match_color`]'s rule.
    pub fn current_color(&self, focused: bool) -> LinearRgba {
        if focused {
            self.colors.current
        } else {
            self.colors.current_unfocused
        }
    }

    /// The grid's runs, in row order; excluding the suppressed input row.
    pub fn as_slice(&self) -> &[SearchRun] {
        &self.runs
    }

    /// The fill channel's runs; rows are fill-local.
    pub fn fill_slice(&self) -> &[SearchRun] {
        &self.fill_runs
    }

    pub(crate) fn clear(&mut self) {
        self.runs.clear();
        self.fill_runs.clear();
    }
}

/// The direction of navigation (Karar 3): a terminal reads with the newest at
/// the bottom, ⏎ and ⌘G go **up**, to the older; ⇧⏎ and ⇧⌘G go down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchDirection {
    /// Up, to the older match (⏎, ⌘G).
    Older,
    /// Down, to the newer match (⇧⏎, ⇧⌘G).
    Newer,
}

/// The area the search panel covers over the grid, in **rows and columns** —
/// `bt-core` sees no pixels, the conversion is done by `bt-shell`, which places
/// the panel.
///
/// `first_row` is the **first fully visible** row below the panel, relative to
/// the grid's screen row 0: `0` covers no row, a negative value says that that
/// many rows of the fill band are exposed too. Of the covered rows only
/// `from_col` and to its right are under the panel; a match to its left is
/// visible (Karar 4: "the window doesn't move if it isn't under the panel").
///
/// Its default **covers nothing** (`first_row` is the smallest value): `0`
/// would count the band's rows as covered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchCover {
    pub first_row: i32,
    pub from_col: u16,
}

impl Default for SearchCover {
    fn default() -> Self {
        Self {
            first_row: i32::MIN,
            from_col: 0,
        }
    }
}

/// The search's answer to the panel — the input of the label ("3 of 17",
/// "3 of 17…") (Karar 3).
///
/// The count and order are **from the whole scrollback's index**
/// ([`SearchIndex`]): while the index is being built piece by piece `complete`
/// is false and the count is what has been counted so far. The same set as the
/// highlight's (a match touching the suppressed row and an inkless one isn't
/// counted, [`eligible`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchReport {
    /// Whether there is a current match — or the scrollback's shift lost it and
    /// it will be reselected at the end of the index ([`Relocate`]).
    pub found: bool,
    /// The number of matches counted; so far, if not `complete`.
    pub total: usize,
    /// The current match's ordinal, newest (bottommost) = 1; `None` if the index
    /// hasn't reached it yet.
    pub ordinal: Option<usize>,
    /// The index has counted the whole scrollback and no scrollback change is
    /// pending.
    pub complete: bool,
}

/// Whether the match is **in the highlight's set** (phase-1's two exclusions):
/// it doesn't touch the suppressed input row and it has ink. Navigation and
/// counting are asked from the same set, or ⏎ would take the window to an
/// invisible row.
///
/// `hidden` is the **absolute** range (`Line`) of the suppressed rows, from the
/// last content frame ([`SearchSlot::hidden`]).
pub(crate) fn eligible<T>(
    term: &Term<T>,
    found: &Match,
    hidden: Option<&RangeInclusive<i32>>,
) -> bool {
    let lines = found.start().line.0..=found.end().line.0;
    let touches =
        hidden.is_some_and(|hidden| lines.start() <= hidden.end() && hidden.start() <= lines.end());
    !touches && has_ink(term, found)
}

/// The first match **in the highlight's set** from `origin` in `direction`;
/// wraps at the scrollback's end (alacritty's `search_next` walks the whole
/// scrollback with `max_lines = None`).
///
/// A match outside the set is skipped and the skip is a loop: if the turn comes
/// back to the first found, there is no match in the set at all. The ceiling
/// ([`SKIP_LIMIT`]) is a second safety — the excluded matches are as many as
/// the single suppressed row and the inkless matches, i.e. a few in practice.
pub(crate) fn next_eligible<T>(
    term: &Term<T>,
    regex: &mut RegexSearch,
    origin: Point,
    direction: Direction,
    hidden: Option<&RangeInclusive<i32>>,
) -> Option<Match> {
    let first = term.search_next(regex, origin, direction, Side::Left, None)?;
    let mut found = first.clone();
    for _ in 0..SKIP_LIMIT {
        if eligible(term, &found, hidden) {
            return Some(found);
        }
        found = term.search_next(
            regex,
            step_past(term, &found, direction),
            direction,
            Side::Left,
            None,
        )?;
        if found == first {
            return None;
        }
    }
    None
}

/// [`next_eligible`]'s skip ceiling. **Not measured**, a safety constant: every
/// step is one `search_next`, i.e. one scan per scrollback; the number is far
/// above the realistic count of matches that fall outside the set.
const SKIP_LIMIT: usize = 64;

/// The cell after `found` in `direction` — the new start of navigation
/// (alacritty's own `advance_search_origin`): the old match itself isn't found
/// again. Wraps at the scrollback's end.
pub(crate) fn step_past<T>(term: &Term<T>, found: &Match, direction: Direction) -> Point {
    match direction {
        Direction::Right => found.end().add(term, Boundary::None, 1),
        Direction::Left => found.start().sub(term, Boundary::None, 1),
    }
}

/// Whether two matches are the same place — marking the current match in the
/// frame.
///
/// One end matching is enough: the frame finds the match by a left-to-right scan
/// (`RegexIter`), navigation in two directions (`search_next`), and with a greedy
/// pattern the two paths can stop at different ends of the same place. Two
/// different matches can't start and end in the same cell.
pub(crate) fn same_place(a: &Match, b: &Match) -> bool {
    a.start() == b.start() || a.end() == b.end()
}

/// The number of rows in one piece of the index (Karar 2-B):
/// [`crate::Session::search_step`] holds the `Term` lock long enough to scan
/// that many rows, then returns to the main queue and key events slip in
/// between the pieces.
///
/// **Not measured**, a design constant (the precedent of `GUTTER_PT`); it has no
/// derivation. There is no hook that measures the scan time under the lock and
/// the claim ("the piece size doesn't feel like key latency") is in
/// `docs/OLCUMLER.md` → Bekleyen iddialar. Its safety comes not from the number
/// but from the piece being bounded and cancellable: a wrapped row extends a
/// piece by at most [`WRAP_REACH`].
pub(crate) const CHUNK_LINES: i32 = 500;

/// Counting the whole scrollback (phase-5): **anchorless** and bottom-up piece
/// by piece (`discussion.md` → Muhakeme: `row_identity` can't be a long-held
/// anchor).
///
/// Matches are **not stored**, they are counted: what the label wants is the
/// count and the current match's ordinal, and a pattern like `.` means millions
/// of matches in ten thousand rows. Navigation doesn't use the index
/// (`Term::search_next`, phase-4); the ordinal is carried ±1 on navigation
/// ([`crate::Session::search_next`]).
///
/// It is rebuilt **from scratch** when the query changes
/// ([`SearchSlot::generation`]) and when the scrollback changes (the pending
/// notice, [`crate::Session::search_step`]).
#[derive(Debug, Default)]
pub(crate) struct SearchIndex {
    /// The index's **own** copy of the pattern (Muhakeme: so it doesn't race
    /// with the pattern the frame path borrows); `None` while a piece is in
    /// flight.
    pub(crate) pattern: Option<RegexSearch>,
    /// The **bottom** row of the next piece (absolute); `None` → the pass is
    /// done. [`INDEX_START`] is "from the scrollback's bottom".
    pub(crate) next: Option<i32>,
    /// The matches counted so far.
    pub(crate) total: usize,
    /// The current match's ordinal (newest = 1).
    pub(crate) ordinal: Option<usize>,
    /// The candidate for the lost current match ([`Relocate`]): the match and its
    /// ordinal.
    pub(crate) candidate: Option<(Match, usize, i32)>,
}

/// [`SearchIndex::next`]'s "from scratch" value: the first piece from the
/// scrollback's bottom.
pub(crate) const INDEX_START: i32 = i32::MAX;

impl SearchIndex {
    /// Rebuilds the index from scratch: `pattern` is the index's new copy (`None`
    /// → the one in the slot stays).
    pub(crate) fn restart(&mut self, pattern: Option<RegexSearch>) {
        if pattern.is_some() {
            self.pattern = pattern;
        }
        self.next = Some(INDEX_START);
        self.total = 0;
        self.ordinal = None;
        self.candidate = None;
    }
}

/// The scrollback's state at one observation — the input of the current match's
/// shift ([`ledger_shift`]). Read under the `Term` lock at every observation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LedgerMark {
    /// `history_size()`.
    pub(crate) history: usize,
    /// `display_offset()`.
    pub(crate) offset: usize,
    /// The accumulated offset difference of the user's scrolling
    /// ([`crate::Session::scroll_user`]): the share of the offset that comes
    /// from output is separated from it.
    pub(crate) user: i64,
    /// The PTY output's generation (one per `Wakeup`): whether there was output
    /// in between.
    pub(crate) epoch: u64,
    /// The generation of clearing the screen ([`crate::Session::clear_to_start`],
    /// [`crate::Session::clear_scrollback`]): whether there was a terminal-side
    /// clear in between. Separate from `epoch`, because on an unsaturated
    /// scrollback `epoch` isn't read and in a session with empty scrollback a
    /// clear leaves the `history` difference at zero while still shifting the
    /// rows (0 → 0).
    pub(crate) wipes: u64,
    pub(crate) columns: usize,
    pub(crate) lines: usize,
    pub(crate) alt: bool,
}

/// How far the scrollback's rows shifted between two observations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shift {
    /// Didn't shift at all.
    Still,
    /// Every row shifted up (older) by `n` rows.
    By(i32),
    /// The shift is unknowable: the current match is lost.
    Lost,
}

/// The current match's shift (Karar 9, Muhakeme) — **only from definite
/// sources**:
///
/// - While the scrollback is unsaturated, the `history_size` difference: the
///   only thing that grows history is the output's scrolling, whether or not
///   the window is scrolled.
/// - On a saturated scrollback, if the window is scrolled, the `display_offset`
///   difference — minus the user's own scrolling: alacritty offsets the window
///   by exactly the number of rows the new output scrolls (up to the ceiling);
///   if the difference is negative, lost.
/// - Everything else (the bottom of a saturated scrollback, an offset at the
///   ceiling, a size change, an alternate-screen switch, deleted history) is
///   **lost**: better to show none than to show the wrong row as current.
/// - A terminal-side clear (`wipes`, ⌘K/⌥⌘K) is **lost** in every arm — both
///   when history stays 0 → 0 (the `history` difference can't see it) and when
///   no row shifted (⌥⌘K's empty history): a clear doesn't say the shift, it
///   only says it happened, and an unknown shift is lost.
pub(crate) fn ledger_shift(prev: LedgerMark, now: LedgerMark, limit: usize) -> Shift {
    if prev.columns != now.columns
        || prev.lines != now.lines
        || prev.alt != now.alt
        || prev.wipes != now.wipes
        || now.history < prev.history
    {
        return Shift::Lost;
    }
    if now.history < limit {
        let grown = now.history - prev.history;
        return match i32::try_from(grown).unwrap_or(i32::MAX) {
            0 => Shift::Still,
            grown => Shift::By(grown),
        };
    }
    // Saturated and scrolled: the offset difference **without looking at the
    // generation** — alacritty's synchronized-update timeout sends `Wakeup`
    // after releasing the lock, so the generation can lag the content by one
    // observation; the offset's only writer other than the user is the output's
    // scrolling.
    if prev.offset > 0 && now.offset > 0 && now.offset < limit {
        let moved = now.offset as i64 - prev.offset as i64 - (now.user - prev.user);
        return match i32::try_from(moved) {
            Ok(0) => Shift::Still,
            Ok(moved) if moved > 0 => Shift::By(moved),
            _ => Shift::Lost,
        };
    }
    // Saturated bottom: the shift isn't visible. No output means it didn't
    // shift; with output it is unknowable. If the generation lagged, the loss is
    // caught at the next observation.
    if now.epoch == prev.epoch {
        Shift::Still
    } else {
        Shift::Lost
    }
}

/// Where the lost current match will move at the end of the index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Relocate {
    /// To the match nearest the window (the shift couldn't be known).
    Nearest,
    /// To the oldest remaining match: the current match fell off the top of the
    /// saturated scrollback (Karar 9).
    Oldest,
}

/// The tracked state of the current match — copied from the slot, shifted under
/// the `Term` lock with [`track`], then written back (if its trace didn't
/// change).
#[derive(Clone, Debug, Default)]
pub(crate) struct Tracking {
    pub(crate) current: Option<Match>,
    pub(crate) origin: Option<Point>,
    pub(crate) mark: Option<LedgerMark>,
    pub(crate) relocate: Option<Relocate>,
}

/// Moves the point up `by` rows.
fn lift(point: Point, by: i32) -> Point {
    Point::new(Line(point.line.0 - by), point.column)
}

/// Shifts the current match and the origin according to the scrollback's `now`
/// state ([`ledger_shift`]); a match that fell off the top is marked to move to
/// the oldest, one that got lost to the nearest ([`Relocate`]). While the `Term`
/// lock is held.
pub(crate) fn track<T>(term: &Term<T>, tracking: &mut Tracking, now: LedgerMark, limit: usize) {
    let Some(prev) = tracking.mark.replace(now) else {
        return;
    };
    match ledger_shift(prev, now, limit) {
        Shift::Still | Shift::By(0) => {}
        Shift::By(by) => {
            tracking.origin = tracking.origin.map(|point| lift(point, by));
            if let Some(found) = tracking.current.take() {
                let start = lift(*found.start(), by);
                if start.line < term.topmost_line() {
                    tracking.relocate = Some(Relocate::Oldest);
                } else {
                    tracking.current = Some(start..=lift(*found.end(), by));
                }
            }
        }
        Shift::Lost => {
            tracking.origin = None;
            if tracking.current.take().is_some() {
                tracking.relocate = Some(Relocate::Nearest);
            }
        }
    }
}

/// One piece of the index: counts `chunk` rows upward from `index.next` (in
/// production [`CHUNK_LINES`]; tests shrink it to see the seam often).
///
/// **No loss or double counting at a piece boundary:** the scan extends to a
/// wrapped row's logical start ([`scan`]), but a match is counted only if its
/// **last row** is inside the piece — a match touching two pieces goes to the
/// lower one. Counting is bottom-up: the piece's matches in reverse order.
///
/// If the ordinal of `tracking`'s current match is found it is written, if it
/// got lost the candidate by its rule ([`Relocate`]); `window` is the window's
/// drawn rows (the measure of the nearest). While the `Term` lock is held.
pub(crate) fn index_chunk<T>(
    term: &Term<T>,
    regex: &mut RegexSearch,
    index: &mut SearchIndex,
    hidden: Option<&RangeInclusive<i32>>,
    tracking: &Tracking,
    window: RangeInclusive<i32>,
    chunk: i32,
) {
    let (current, relocate) = (tracking.current.as_ref(), tracking.relocate);
    let Some(next) = index.next else {
        return;
    };
    let top = term.topmost_line().0;
    let high = next.min(term.bottommost_line().0);
    if high < top {
        index.next = None;
        return;
    }
    let low = (high - chunk.max(1) + 1).max(top);
    let mut found = Vec::new();
    scan(term, regex, Line(low), Line(high), |each| {
        let end = each.end().line.0;
        if (low..=high).contains(&end) && eligible(term, each, hidden) {
            found.push(each.clone());
        }
    });
    for each in found.into_iter().rev() {
        index.total += 1;
        let ordinal = index.total;
        if current.is_some_and(|current| same_place(&each, current)) {
            index.ordinal = Some(ordinal);
        }
        match relocate {
            Some(Relocate::Oldest) => index.candidate = Some((each, ordinal, 0)),
            Some(Relocate::Nearest) => {
                let lines = each.start().line.0..=each.end().line.0;
                let distance = if lines.start() > window.end() {
                    lines.start() - window.end()
                } else if lines.end() < window.start() {
                    window.start() - lines.end()
                } else {
                    0
                };
                if index
                    .candidate
                    .as_ref()
                    .is_none_or(|(_, _, best)| distance < *best)
                {
                    index.candidate = Some((each, ordinal, distance));
                }
            }
            None => {}
        }
    }
    index.next = (low > top).then_some(low - 1);
}

/// The session's search slot — a **leaf lock** (the precedent of `theme`).
///
/// The compiled pattern wants `&mut` under the `Term` lock (`RegexIter`) and a
/// "search lock → `Term`" order would break the module's contract. The frame
/// path takes the pattern from the slot **before** the `Term` lock and owns it,
/// and after the turn puts it back if the generation is still the same; no lock
/// is taken under `Term` (`discussion.md` → Muhakeme). It is not copied, it is
/// **lent**: the pattern carries the cache of four lazy DFAs and cloning it per
/// frame would be both an allocation and a cold cache.
#[derive(Debug, Default)]
pub(crate) struct SearchSlot {
    /// Increases on every `set_search`/`clear_search`; the borrowed pattern is
    /// put back only if the generation didn't change — a new query that came in
    /// between wins.
    pub(crate) generation: u64,
    /// The pattern sitting in the slot; `None` while a frame has borrowed it.
    pub(crate) pattern: Option<RegexSearch>,
    /// Whether there is a pattern (even if lent) — the gate of the frame request.
    pub(crate) active: bool,
    /// The **current match** (Karar 3), in the scrollback's absolute coordinates.
    /// It is reselected when the query changes, navigation moves it, and the
    /// frame marks it with the `search_current` color ([`same_place`]).
    ///
    /// An absolute row slides with output; at every observation it is stuck to
    /// its content by the scrollback's shift ([`track`]).
    pub(crate) current: Option<Match>,
    /// The bottom of the window where the search started: while typing, the
    /// current match, if there is no visible match in the window, is the first
    /// match upward from here. It is set on the first query and dropped when the
    /// search closes (`clear_search`); navigation pulls it to the current match
    /// so that narrowing the query stays near where it was found.
    pub(crate) origin: Option<Point>,
    /// The input rows suppressed in the last content frame, as an **absolute**
    /// `Line` range — the frame's own answer, so that navigation and counting
    /// exclude what the highlight excludes; it isn't derived a second time
    /// (015's lesson).
    pub(crate) hidden: Option<RangeInclusive<i32>>,
    /// The scrollback's last observation — [`current`](SearchSlot::current) and
    /// [`origin`](SearchSlot::origin) are according to this state ([`track`]).
    /// `None` when search is off.
    pub(crate) mark: Option<LedgerMark>,
    /// The current match was lost by the scrollback's shift: it will be
    /// reselected at the end of the index.
    pub(crate) relocate: Option<Relocate>,
    /// The count of the whole scrollback (phase-5).
    pub(crate) index: SearchIndex,
}

impl SearchSlot {
    /// A copy of the tracked state ([`Tracking`]).
    pub(crate) fn tracking(&self) -> Tracking {
        Tracking {
            current: self.current.clone(),
            origin: self.origin,
            mark: self.mark,
            relocate: self.relocate,
        }
    }

    /// Writes `tracking` back — **only** if the slot's trace is the one the copy
    /// was taken at (`taken`): if another observation advanced the slot in the
    /// meantime, its state is newer and this copy would apply a stale shift a
    /// second time.
    pub(crate) fn settle(&mut self, taken: Option<LedgerMark>, tracking: Tracking) -> bool {
        if self.mark != taken {
            return false;
        }
        let changed = self.current != tracking.current;
        self.current = tracking.current;
        self.origin = tracking.origin;
        self.mark = tracking.mark;
        self.relocate = tracking.relocate;
        changed
    }
}
