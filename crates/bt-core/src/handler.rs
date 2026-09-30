//! The wrapper between the parser and `Term`.
//!
//! The reader loop ([`crate::reader`]) hands `Term` to the parser not
//! directly but through this type: clustering (035) has to step **in
//! between** `input`s and close the cluster on every other intervening call,
//! so it must see all of the parser's `Handler` calls. With clustering off
//! (`SessionOptions::cluster`) every call goes to `Term` as is — the
//! behavior is alacritty's, byte for byte.
//!
//! **Clustering happens in `input` and only there.** If the incoming code
//! point does not extend the open cluster it goes to `Term::input`; if it
//! extends it and the cluster's column count does not change it lands in the
//! head cell's `zerowidth`; if it grows the cluster from one column to two
//! the head cell is rewritten through alacritty's **own** wide path (see
//! [`ClusterHandler::widen`]). The rule is in [`crate::cluster`].
//!
//! **The open cluster's position is not stored, it is derived from the
//! grid** — by the method of alacritty's `zerowidth` branch (cursor − 1, the
//! cursor itself on a pending wrap, back off a spacer). Reads arrive in
//! chunks and in between the main thread may change `Term` (resize, ⌘K); a
//! stored position would go stale in that gap, while the derived position
//! has the same openness as alacritty's present `zerowidth` path. The one
//! state is the "was the last `Handler` call `input`" bit, which lives in
//! the loop's `State` because this type is reborn on every `advance`; every
//! other forwarding clears the bit — an intervening `CUP` or SGR closes the
//! cluster.
//!
//! **The forwarding list is in one macro and guarded.** Every `Handler`
//! method has an empty default; a method dropped from the list still
//! compiles but never reaches `Term`'s implementation, and the symptom is
//! silent (e.g. an escape sequence is ignored). The
//! `clippy::missing_trait_methods` above the `impl` turns that omission red
//! in `make clippy`; the same place catches a new method arriving in vte.

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::Point;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Term, TermMode};
use alacritty_terminal::vte::ansi::{
    Attr, CharsetIndex, ClearMode, CursorShape, CursorStyle, Handler, Hyperlink, KeyboardModes,
    KeyboardModesApplyBehavior, LineClearMode, Mode, ModifyOtherKeys, PrivateMode, Rgb,
    ScpCharPath, ScpUpdateMode, StandardCharset, TabulationClearMode,
};
// vte does not re-export this type; the reason for the edge is in the root
// `Cargo.toml`.
use cursor_icon::CursorIcon;

/// The `Handler` the parser sees: it borrows `Term` and forwards calls to
/// it. It is reborn **on every `advance` and `stop_sync` call** — one lock
/// round can hold several `advance`s (`pty_read`'s read loop) — so it has no
/// state of its own: the one bit of a cluster split across two `read`
/// chunks (`last_input`) lives in the loop's `State`.
pub(crate) struct ClusterHandler<'a, U: EventListener> {
    term: &'a mut Term<U>,
    /// Whether clustering is on — the value of `SessionOptions::cluster`.
    cluster: bool,
    /// Whether the last `Handler` call was `input`: is there an open cluster.
    last_input: &'a mut bool,
}

impl<'a, U: EventListener> ClusterHandler<'a, U> {
    pub(crate) fn new(term: &'a mut Term<U>, cluster: bool, last_input: &'a mut bool) -> Self {
        Self {
            term,
            cluster,
            last_input,
        }
    }

    /// The open cluster's head cell — by the method of alacritty's
    /// `zerowidth` branch (`Term::input`): the cursor's own cell on a pending
    /// wrap, otherwise the one to its left; back to the wide cell if it lands
    /// on a spacer.
    ///
    /// The column is also clamped: by alacritty's contract the cursor is
    /// inside the grid, but indexing panics are forbidden in `bt-core` too.
    fn head(&self) -> Point {
        let grid = self.term.grid();
        let cursor = &grid.cursor;
        let mut column = cursor.point.column;
        if !cursor.input_needs_wrap {
            column.0 = column.saturating_sub(1);
        }
        column.0 = column.0.min(grid.columns().saturating_sub(1));
        let line = cursor.point.line;
        if grid[line][column].flags.contains(Flags::WIDE_CHAR_SPACER) {
            column.0 = column.saturating_sub(1);
        }
        Point::new(line, column)
    }

    /// The head cell's cluster: base character + `zerowidth`.
    fn open(&self, at: Point) -> String {
        let cell = &self.term.grid()[at.line][at.column];
        let mut open = String::new();
        open.push(cell.c);
        open.extend(cell.zerowidth().unwrap_or_default());
        open
    }

    /// Turns the narrow head cell into a wide cell and writes the cluster
    /// (`open`, its extended form) into it.
    ///
    /// **Widening goes through alacritty's own wide path**: the cursor is
    /// moved back to the head cell and `Term::input` is called with a
    /// placeholder wide character — the end-of-line
    /// `LEADING_WIDE_CHAR_SPACER`, the bottom of the scroll region and DECAWM
    /// stay in alacritty, its special paths (`write_at_cursor`, `wrapline`)
    /// are not rewritten. The template of the written cell (color, flags,
    /// link) is the cursor's template: it was what wrote the head cell too,
    /// because an intervening SGR would have closed the cluster.
    ///
    /// **Under IRM the head cell's insertion is undone first**
    /// (`delete_chars`): writing the narrow head cell shifted the line by one
    /// column, the placeholder would shift it two more — a two-column cluster
    /// would push its neighbors three columns.
    fn widen(&mut self, at: Point, open: &str) {
        let cursor = &mut self.term.grid_mut().cursor;
        cursor.point = at;
        cursor.input_needs_wrap = false;
        if self.term.mode().contains(TermMode::INSERT) {
            self.term.delete_chars(1);
        }
        self.term.input(WIDE_PLACEHOLDER);
        // The written cell by the same method again: at the end of a line it
        // may have dropped to the next line. With DECAWM off, alacritty
        // returns without writing anything in the last column; then the cell
        // found stays narrow and the cluster lands on top of it — a column
        // short but the glyph is not lost.
        let at = self.head();
        let cell = &mut self.term.grid_mut()[at.line][at.column];
        let mut chars = open.chars();
        let head = chars.next().unwrap_or(' ');
        // An unwritten cell (DECAWM off, last column) still carries the
        // cluster's old remainder: printed again, the ZWJ of `❤‍🔥` would
        // enter twice and the cluster would not shape. A written cell's `c`
        // is the placeholder, so no remainder is carried there.
        let kept = if cell.c == head {
            cell.zerowidth().map_or(0, <[char]>::len)
        } else {
            0
        };
        cell.c = head;
        chars.skip(kept).for_each(|c| cell.push_zerowidth(c));
    }
}

/// The placeholder for widening: `Term::input` needs a two-column code point
/// and the cell's `c` turns back into the base character right after. Its
/// value is immaterial; a wide character that alacritty's charset mapping
/// (DEC special graphics) does not touch.
const WIDE_PLACEHOLDER: char = '\u{3000}';

/// Forwards all `Handler` methods **other than** `input` to `Term`. `input`
/// is hand-written because it is the one door clustering enters through.
macro_rules! forward {
    ($( fn $name:ident(&mut self $(, $arg:ident: $ty:ty)*); )*) => {
        $(
            #[inline]
            fn $name(&mut self $(, $arg: $ty)*) {
                // Every intervening call closes the open cluster.
                *self.last_input = false;
                self.term.$name($($arg),*)
            }
        )*
    };
}

#[deny(clippy::missing_trait_methods)]
impl<U: EventListener> Handler for ClusterHandler<'_, U> {
    fn input(&mut self, c: char) {
        if !self.cluster {
            return self.term.input(c);
        }
        let open_cluster = std::mem::replace(self.last_input, true);
        if !open_cluster || !crate::cluster::may_extend(c) {
            return self.term.input(c);
        }
        let at = self.head();
        let mut open = self.open(at);
        if !crate::cluster::extends(&open, c) {
            return self.term.input(c);
        }
        let narrow = !self.term.grid()[at.line][at.column]
            .flags
            .contains(Flags::WIDE_CHAR);
        open.push(c);
        // Width is asked after **every** extension, including those from the
        // zero-width arm: `1` + VS16 + `U+20E3` widens at the VS16.
        if narrow && crate::cluster::width(&open) >= 2 {
            self.widen(at, &open);
        } else {
            self.term.grid_mut()[at.line][at.column].push_zerowidth(c);
        }
    }

    forward! {
        fn set_title(&mut self, title: Option<String>);
        fn set_cursor_style(&mut self, style: Option<CursorStyle>);
        fn set_cursor_shape(&mut self, shape: CursorShape);
        fn goto(&mut self, line: i32, col: usize);
        fn goto_line(&mut self, line: i32);
        fn goto_col(&mut self, col: usize);
        fn insert_blank(&mut self, count: usize);
        fn move_up(&mut self, rows: usize);
        fn move_down(&mut self, rows: usize);
        fn identify_terminal(&mut self, intermediate: Option<char>);
        fn device_status(&mut self, arg: usize);
        fn move_forward(&mut self, cols: usize);
        fn move_backward(&mut self, cols: usize);
        fn move_down_and_cr(&mut self, rows: usize);
        fn move_up_and_cr(&mut self, rows: usize);
        fn put_tab(&mut self, count: u16);
        fn backspace(&mut self);
        fn carriage_return(&mut self);
        fn linefeed(&mut self);
        fn bell(&mut self);
        fn substitute(&mut self);
        fn newline(&mut self);
        fn set_horizontal_tabstop(&mut self);
        fn scroll_up(&mut self, rows: usize);
        fn scroll_down(&mut self, rows: usize);
        fn insert_blank_lines(&mut self, rows: usize);
        fn delete_lines(&mut self, rows: usize);
        fn erase_chars(&mut self, count: usize);
        fn delete_chars(&mut self, count: usize);
        fn move_backward_tabs(&mut self, count: u16);
        fn move_forward_tabs(&mut self, count: u16);
        fn save_cursor_position(&mut self);
        fn restore_cursor_position(&mut self);
        fn clear_line(&mut self, mode: LineClearMode);
        fn clear_screen(&mut self, mode: ClearMode);
        fn clear_tabs(&mut self, mode: TabulationClearMode);
        fn set_tabs(&mut self, interval: u16);
        fn reset_state(&mut self);
        fn reverse_index(&mut self);
        fn terminal_attribute(&mut self, attr: Attr);
        fn set_mode(&mut self, mode: Mode);
        fn unset_mode(&mut self, mode: Mode);
        fn report_mode(&mut self, mode: Mode);
        fn set_private_mode(&mut self, mode: PrivateMode);
        fn unset_private_mode(&mut self, mode: PrivateMode);
        fn report_private_mode(&mut self, mode: PrivateMode);
        fn set_scrolling_region(&mut self, top: usize, bottom: Option<usize>);
        fn set_keypad_application_mode(&mut self);
        fn unset_keypad_application_mode(&mut self);
        fn set_active_charset(&mut self, index: CharsetIndex);
        fn configure_charset(&mut self, index: CharsetIndex, charset: StandardCharset);
        fn set_color(&mut self, index: usize, color: Rgb);
        fn dynamic_color_sequence(&mut self, prefix: String, index: usize, terminator: &str);
        fn reset_color(&mut self, index: usize);
        fn clipboard_store(&mut self, clipboard: u8, base64: &[u8]);
        fn clipboard_load(&mut self, clipboard: u8, terminator: &str);
        fn decaln(&mut self);
        fn push_title(&mut self);
        fn pop_title(&mut self);
        fn text_area_size_pixels(&mut self);
        fn text_area_size_chars(&mut self);
        fn set_hyperlink(&mut self, hyperlink: Option<Hyperlink>);
        fn set_mouse_cursor_icon(&mut self, icon: CursorIcon);
        fn report_keyboard_mode(&mut self);
        fn push_keyboard_mode(&mut self, mode: KeyboardModes);
        fn pop_keyboard_modes(&mut self, to_pop: u16);
        fn set_keyboard_mode(&mut self, mode: KeyboardModes, behavior: KeyboardModesApplyBehavior);
        fn set_modify_other_keys(&mut self, mode: ModifyOtherKeys);
        fn report_modify_other_keys(&mut self);
        fn set_scp(&mut self, char_path: ScpCharPath, update_mode: ScpUpdateMode);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::index::{Column, Line};
    use alacritty_terminal::term::Config;
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::vte::ansi::Processor;

    fn term(cols: usize, rows: usize) -> Term<VoidListener> {
        Term::new(Config::default(), &TermSize::new(cols, rows), VoidListener)
    }

    /// Runs `bytes` through the wrapper in a single `advance`.
    fn feed(term: &mut Term<VoidListener>, cluster: bool, bytes: &str) {
        let mut parser: Processor = Processor::new();
        let mut last_input = false;
        parser.advance(
            &mut ClusterHandler::new(term, cluster, &mut last_input),
            bytes.as_bytes(),
        );
    }

    /// The grid's rows, cell by cell: a wide cell `[…]`, a spacer `·`, an
    /// end-of-line spacer `↵`; a cell's text is `c` + `zerowidth`. Trailing
    /// empty cells are dropped.
    fn rows(term: &Term<VoidListener>) -> Vec<String> {
        let grid = term.grid();
        (0..grid.screen_lines())
            .map(|line| {
                let mut cells: Vec<String> = (0..grid.columns())
                    .map(|col| {
                        let cell = &grid[Line(line as i32)][Column(col)];
                        let mut text = String::from(cell.c);
                        text.extend(cell.zerowidth().unwrap_or_default());
                        if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                            "·".to_owned()
                        } else if cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER) {
                            "↵".to_owned()
                        } else if cell.flags.contains(Flags::WIDE_CHAR) {
                            format!("[{text}]")
                        } else {
                            text
                        }
                    })
                    .collect();
                while cells.last().is_some_and(|cell| cell == " ") {
                    cells.pop();
                }
                cells.join("|")
            })
            .collect()
    }

    /// The same bytes, clustering off and on.
    fn both(cols: usize, rows_: usize, bytes: &str) -> (Vec<String>, Vec<String>) {
        let mut off = term(cols, rows_);
        feed(&mut off, false, bytes);
        let mut on = term(cols, rows_);
        feed(&mut on, true, bytes);
        (rows(&off), rows(&on))
    }

    #[test]
    fn emoji_sequences_become_one_wide_cell() {
        let mut t = term(20, 2);
        feed(
            &mut t,
            true,
            "🇹🇷 👍🏽 👨\u{200D}👩\u{200D}👧 ❤\u{FE0F} 🏳\u{FE0F}\u{200D}🌈",
        );
        assert_eq!(
            rows(&t)[0],
            "[🇹🇷]|·| |[👍🏽]|·| |[👨\u{200D}👩\u{200D}👧]|·| |[❤\u{FE0F}]|·| \
             |[🏳\u{FE0F}\u{200D}🌈]|·"
        );
        assert_eq!(t.grid().cursor.point.column, Column(14));
    }

    /// A non-emoji cluster, VS15, a skin tone behind a narrow cluster and a
    /// lone RI give today's cells.
    #[test]
    fn non_clusters_keep_todays_cells() {
        for text in ["لا", "⌚\u{FE0E}", "a🏽", "🇹", "e\u{301}x", "a\u{200D}b"] {
            let (off, on) = both(10, 2, text);
            assert_eq!(on, off, "{text:?}");
        }
    }

    /// The flag off is alacritty itself: the sequence is fragmented as today.
    #[test]
    fn the_flag_off_is_alacritty() {
        let mut t = term(10, 2);
        feed(&mut t, false, "🇹🇷👍🏽");
        assert_eq!(rows(&t)[0], "🇹|🇷|[👍]|·|[🏽]|·");
    }

    /// The result of widening equals the result of alacritty's **own** wide
    /// character — last column, IRM and the bottom of the scroll region. The
    /// comparison is a `👍` printed at the same place with clustering off.
    fn widening_matches_a_native_wide_char(cols: usize, rows_: usize, before: &str) {
        let mut native = term(cols, rows_);
        feed(&mut native, false, &format!("{before}👍"));
        let mut widened = term(cols, rows_);
        feed(&mut widened, true, &format!("{before}❤\u{FE0F}"));
        let expected: Vec<String> = rows(&native)
            .into_iter()
            .map(|row| row.replace('👍', "❤\u{FE0F}"))
            .collect();
        assert_eq!(rows(&widened), expected, "{before:?}");
        assert_eq!(
            widened.grid().cursor.point,
            native.grid().cursor.point,
            "{before:?}"
        );
    }

    #[test]
    fn widening_at_the_last_column_wraps_like_alacritty() {
        widening_matches_a_native_wide_char(10, 3, "123456789");
        let mut t = term(10, 3);
        feed(&mut t, true, "123456789❤\u{FE0F}");
        assert_eq!(rows(&t)[0], "1|2|3|4|5|6|7|8|9|↵");
        assert_eq!(rows(&t)[1], "[❤\u{FE0F}]|·");
    }

    #[test]
    fn widening_under_irm_shifts_the_neighbours_by_two() {
        // `abcdef`, cursor at column 1, IRM on.
        widening_matches_a_native_wide_char(10, 2, "abcdef\r\x1b[C\x1b[4h");
        let mut t = term(10, 2);
        feed(&mut t, true, "abcdef\r\x1b[C\x1b[4h❤\u{FE0F}");
        assert_eq!(rows(&t)[0], "a|[❤\u{FE0F}]|·|b|c|d|e|f");
    }

    #[test]
    fn widening_at_the_bottom_of_the_scroll_region_scrolls_the_region() {
        // The region is rows 1–3, the cursor in the last column of row 3;
        // row 4 is outside the region and must stay in place.
        let before = "top\x1b[4;1Hout\x1b[1;3r\x1b[3;10H";
        widening_matches_a_native_wide_char(10, 4, before);
        let mut t = term(10, 4);
        feed(&mut t, true, &format!("{before}❤\u{FE0F}"));
        assert_eq!(
            rows(&t),
            ["", " | | | | | | | | |↵", "[❤\u{FE0F}]|·", "o|u|t"],
            "the region scrolled one row, `top` is gone, `out` stayed in place"
        );
    }

    /// With DECAWM off, alacritty does not write a wide character in the last
    /// column; widening leaves the cluster in the narrow cell instead of
    /// panicking.
    #[test]
    fn widening_without_autowrap_keeps_the_cluster_narrow() {
        let mut t = term(10, 2);
        feed(&mut t, true, "\x1b[?7l123456789❤\u{FE0F}");
        assert_eq!(rows(&t)[0], "1|2|3|4|5|6|7|8|9|❤\u{FE0F}");
        // A cluster whose remainder is already in the cell (`❤` + ZWJ, then
        // `🔥`): the old remainder is not printed a second time.
        let mut t = term(10, 2);
        feed(&mut t, true, "\x1b[?7l123456789❤\u{200D}🔥");
        assert_eq!(rows(&t)[0], "1|2|3|4|5|6|7|8|9|❤\u{200D}🔥");
    }

    #[test]
    fn an_intervening_call_closes_the_cluster() {
        let mut t = term(10, 2);
        // `CUP` puts the cursor right behind the `👍`; still two clusters.
        feed(&mut t, true, "👍\x1b[1;3H🏽");
        assert_eq!(rows(&t)[0], "[👍]|·|[🏽]|·");
    }

    /// A cluster takes the place of one wide character in the grid — at every
    /// width, including a cluster landing at the end of a line (`👍🏽` in the
    /// last two columns, or not fitting and dropping to the next line). The
    /// grid half of `dock::tests`'s `grid_span` equivalence: together the two
    /// say that the suppression's span and the grid do not diverge.
    #[test]
    fn a_cluster_takes_the_cells_of_one_wide_char_at_every_width() {
        let parts = [
            ("🇹🇷", "日"),
            ("👍🏽", "日"),
            ("x", "x"),
            ("👨\u{200D}👩\u{200D}👧", "日"),
            ("x", "x"),
            ("1\u{FE0F}\u{20E3}", "日"),
            ("❤\u{FE0F}", "日"),
            ("x", "x"),
        ];
        let clustered: String = parts.iter().map(|p| p.0).collect();
        let wide: String = parts.iter().map(|p| p.1).collect();
        for cols in 2..=9 {
            for lead in 0..cols {
                let before = " ".repeat(lead);
                let mut on = term(cols, 12);
                feed(&mut on, true, &format!("{before}{clustered}"));
                let mut native = term(cols, 12);
                feed(&mut native, false, &format!("{before}{wide}"));
                let heads = |t: &Term<VoidListener>| -> Vec<String> {
                    rows(t)
                        .into_iter()
                        .map(|row| {
                            row.split('|')
                                .map(|cell| match cell.strip_prefix('[') {
                                    Some(_) => "[W]".to_owned(),
                                    None if cell.chars().count() > 1 => "W?".to_owned(),
                                    None => cell.to_owned(),
                                })
                                .collect::<Vec<_>>()
                                .join("|")
                        })
                        .collect()
                };
                assert_eq!(heads(&on), heads(&native), "cols={cols} lead={lead}");
                assert_eq!(
                    on.grid().cursor.point,
                    native.grid().cursor.point,
                    "cols={cols} lead={lead}"
                );
            }
        }
    }

    /// A cluster split across two `advance`s does not close: the bit is in
    /// the loop's `State`.
    #[test]
    fn a_cluster_split_across_reads_stays_open() {
        let mut t = term(10, 2);
        let mut parser: Processor = Processor::new();
        let mut last_input = false;
        for part in ["🇹", "🇷", "👍", "🏽"] {
            parser.advance(
                &mut ClusterHandler::new(&mut t, true, &mut last_input),
                part.as_bytes(),
            );
        }
        assert_eq!(rows(&t)[0], "[🇹🇷]|·|[👍🏽]|·");
    }
}
