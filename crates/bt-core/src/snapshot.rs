//! The scrollback's snapshot as VT bytes (053): what a pane leaves behind
//! when bateri quits, so the next launch can replay it into a fresh `Term`
//! ([`crate::SessionOptions::replay`]).
//!
//! **Bytes, not a grid dump** (`.tasks/053-oturum-geri-yukleme/discussion.md`
//! → Karar 2): the replay goes through the same parser and the same
//! [`crate::handler::ClusterHandler`] as the shell's output, so colour, style,
//! the underline colour and emoji clusters come back by the path the parser
//! already knows, and a wrapped row — which carries no line break — rewraps
//! by itself at a new width. alacritty's types do not reach the file and the
//! format does not follow alacritty's internal layout.
//!
//! What is written: text, SGR (foreground and background — named, 256 and
//! truecolour kept apart —, bold, dim, italic, inverse, hidden, the five
//! underline styles and the `58` colour, strikeout) and a cell's zero-width
//! characters behind its base character. What is **not**: OSC 8 (links and
//! our block anchors — the new shell numbers its blocks afresh), modes,
//! the cursor, the alternate screen.
//!
//! Only the **difference** of the pen is written between two cells; at the
//! end of an unwrapped row the pen goes back to the default before `\r\n`,
//! because alacritty fills the rows a line feed opens with the cursor's
//! template (bce) and a background left set would paint the next row.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::Line;
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::{Cell as TermCell, Flags};
use alacritty_terminal::vte::ansi::{Color, NamedColor};

/// The flags the pen carries; the rest (wrap, wide, spacer) are geometry.
const STYLE: Flags = Flags::BOLD
    .union(Flags::DIM)
    .union(Flags::ITALIC)
    .union(Flags::INVERSE)
    .union(Flags::HIDDEN)
    .union(Flags::STRIKEOUT)
    .union(Flags::ALL_UNDERLINES);

/// The flags that make a blank cell visible; a trailing blank without them
/// (and on the default background) is dropped.
const VISIBLE_BLANK: Flags = Flags::INVERSE
    .union(Flags::STRIKEOUT)
    .union(Flags::ALL_UNDERLINES);

/// The cell attributes SGR can carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pen {
    fg: Color,
    bg: Color,
    underline_color: Option<Color>,
    flags: Flags,
}

impl Pen {
    const DEFAULT: Self = Self {
        fg: Color::Named(NamedColor::Foreground),
        bg: Color::Named(NamedColor::Background),
        underline_color: None,
        flags: Flags::empty(),
    };

    fn of(cell: &TermCell) -> Self {
        Self {
            fg: cell.fg,
            bg: cell.bg,
            underline_color: cell.underline_color(),
            flags: cell.flags & STYLE,
        }
    }
}

/// The rows from the top of the scrollback down to `end` (exclusive, a
/// screen line — `Line(0)` is the screen's top row) as VT bytes.
///
/// Trailing empty rows are dropped and the stream ends with a line break
/// (`.tasks/053-oturum-geri-yukleme/plan.md` → R1.2): the new shell's first
/// prompt starts on a fresh row, without zsh's `PROMPT_SP` mark. An empty
/// history gives an empty vector.
pub(crate) fn encode<T>(term: &Term<T>, end: i32) -> Vec<u8> {
    let grid = term.grid();
    let top = grid.topmost_line().0;
    let end = end.min(grid.bottommost_line().0 + 1);
    let rows: Vec<Vec<&TermCell>> = (top..end)
        .map(|line| grid[Line(line)].into_iter().collect())
        .collect();
    let last = rows
        .iter()
        .rposition(|cells| wrapped(cells) || content_end(cells) > 0);
    let Some(last) = last else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut pen = Pen::DEFAULT;
    for (index, cells) in rows[..=last].iter().enumerate() {
        let wrap = wrapped(cells) && index < last;
        let end = if wrap {
            cells.len()
        } else {
            content_end(cells)
        };
        for cell in &cells[..end] {
            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            let next = Pen::of(cell);
            transition(&mut out, pen, next);
            pen = next;
            push_char(&mut out, cell.c);
            for &extra in cell.zerowidth().unwrap_or_default() {
                push_char(&mut out, extra);
            }
        }
        if !wrap {
            if pen != Pen::DEFAULT {
                out.extend_from_slice(b"\x1b[0m");
                pen = Pen::DEFAULT;
            }
            out.extend_from_slice(b"\r\n");
        }
    }
    out
}

/// Whether the row continues on the next one (alacritty puts the flag on the
/// row's last cell).
fn wrapped(cells: &[&TermCell]) -> bool {
    cells
        .last()
        .is_some_and(|cell| cell.flags.contains(Flags::WRAPLINE))
}

/// The number of cells up to the last one that is not a default blank.
fn content_end(cells: &[&TermCell]) -> usize {
    cells
        .iter()
        .rposition(|cell| !default_blank(cell))
        .map_or(0, |index| index + 1)
}

/// A blank a line break can stand in for: a space (or a spacer) on the
/// default background, with nothing drawn over it. The foreground is
/// invisible on a space.
fn default_blank(cell: &TermCell) -> bool {
    (cell.c == ' ' || cell.c == '\0')
        && cell.bg == Color::Named(NamedColor::Background)
        && !cell.flags.intersects(VISIBLE_BLANK)
        && cell.zerowidth().is_none_or(<[char]>::is_empty)
}

fn push_char(out: &mut Vec<u8>, ch: char) {
    // A NUL cell is an untouched cell; it reads as a space.
    let ch = if ch == '\0' { ' ' } else { ch };
    let mut buf = [0; 4];
    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
}

/// Writes the SGR that turns `from` into `to`; nothing if they are equal.
///
/// `22` cancels bold **and** dim, so when one of the two goes the survivor is
/// set again. The underline styles exclude one another (alacritty clears all
/// five before setting one), so a change is a single `4:n` or `24`.
fn transition(out: &mut Vec<u8>, from: Pen, to: Pen) {
    if from == to {
        return;
    }
    let mut codes: Vec<String> = Vec::new();
    let removed = from.flags - to.flags;
    let mut added = to.flags - from.flags;
    if removed.intersects(Flags::BOLD | Flags::DIM) {
        codes.push("22".to_owned());
        added |= to.flags & (Flags::BOLD | Flags::DIM);
    }
    for (flag, code) in [
        (Flags::ITALIC, "23"),
        (Flags::INVERSE, "27"),
        (Flags::HIDDEN, "28"),
        (Flags::STRIKEOUT, "29"),
    ] {
        if removed.contains(flag) {
            codes.push(code.to_owned());
        }
    }
    let underline = to.flags & Flags::ALL_UNDERLINES;
    if from.flags & Flags::ALL_UNDERLINES != underline {
        codes.push(underline_code(underline).to_owned());
    }
    for (flag, code) in [
        (Flags::BOLD, "1"),
        (Flags::DIM, "2"),
        (Flags::ITALIC, "3"),
        (Flags::INVERSE, "7"),
        (Flags::HIDDEN, "8"),
        (Flags::STRIKEOUT, "9"),
    ] {
        if added.contains(flag) {
            codes.push(code.to_owned());
        }
    }
    if from.fg != to.fg {
        codes.push(color_code(to.fg, Layer::Foreground));
    }
    if from.bg != to.bg {
        codes.push(color_code(to.bg, Layer::Background));
    }
    if from.underline_color != to.underline_color {
        codes.push(match to.underline_color {
            Some(color) => color_code(color, Layer::Underline),
            None => "59".to_owned(),
        });
    }
    out.extend_from_slice(b"\x1b[");
    out.extend_from_slice(codes.join(";").as_bytes());
    out.push(b'm');
}

fn underline_code(underline: Flags) -> &'static str {
    if underline.contains(Flags::DOUBLE_UNDERLINE) {
        "4:2"
    } else if underline.contains(Flags::UNDERCURL) {
        "4:3"
    } else if underline.contains(Flags::DOTTED_UNDERLINE) {
        "4:4"
    } else if underline.contains(Flags::DASHED_UNDERLINE) {
        "4:5"
    } else if underline.contains(Flags::UNDERLINE) {
        "4"
    } else {
        "24"
    }
}

/// Which colour an SGR sets.
#[derive(Clone, Copy)]
enum Layer {
    Foreground,
    Background,
    Underline,
}

/// The SGR of a colour. A named colour stays named (`31`, not `38;5;1`),
/// because the parser keeps the two apart and the theme draws them apart.
/// The named colours SGR cannot produce (the cursor's, the dim and bright
/// foregrounds) fall to the default — they never come out of the parser.
fn color_code(color: Color, layer: Layer) -> String {
    let (base, bright, default, extended) = match layer {
        Layer::Foreground => (30, 90, "39", "38"),
        Layer::Background => (40, 100, "49", "48"),
        Layer::Underline => (0, 0, "59", "58"),
    };
    match color {
        Color::Spec(rgb) => format!("{extended};2;{};{};{}", rgb.r, rgb.g, rgb.b),
        Color::Indexed(index) => format!("{extended};5;{index}"),
        Color::Named(named) => {
            let index = named as usize;
            match (layer, index) {
                // `58` has no named form; the palette slot is the same colour.
                (Layer::Underline, 0..=15) => format!("58;5;{index}"),
                (_, 0..=7) => (base + index).to_string(),
                (_, 8..=15) => (bright + index - 8).to_string(),
                _ => default.to_owned(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handler::ClusterHandler;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::index::Column;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::vte::ansi::Processor;

    fn term(cols: usize, rows: usize) -> Term<VoidListener> {
        let config = Config {
            scrolling_history: 1000,
            ..Config::default()
        };
        Term::new(config, &TermSize::new(cols, rows), VoidListener)
    }

    fn feed(term: &mut Term<VoidListener>, bytes: &[u8]) {
        let mut parser: Processor = Processor::new();
        let mut last_input = false;
        parser.advance(&mut ClusterHandler::new(term, true, &mut last_input), bytes);
    }

    /// Everything up to and including the cursor's row.
    fn snapshot(term: &Term<VoidListener>) -> Vec<u8> {
        encode(term, term.grid().cursor.point.line.0 + 1)
    }

    /// Every cell from the top of the history to the last non-empty row, as
    /// the fields the snapshot promises to keep.
    fn cells(term: &Term<VoidListener>) -> Vec<Vec<String>> {
        let grid = term.grid();
        let mut rows: Vec<Vec<String>> = (grid.topmost_line().0..=grid.bottommost_line().0)
            .map(|line| {
                let row = &grid[Line(line)];
                (0..grid.columns())
                    .map(|col| {
                        let cell = &row[Column(col)];
                        assert!(cell.hyperlink().is_none(), "a link survived");
                        format!(
                            "{:?}{:?}|{:?}|{:?}|{:?}|{:?}",
                            cell.c,
                            cell.zerowidth().unwrap_or_default(),
                            cell.fg,
                            cell.bg,
                            cell.underline_color(),
                            cell.flags,
                        )
                    })
                    .collect()
            })
            .collect();
        let blank = format!(
            "{:?}{:?}|{:?}|{:?}|{:?}|{:?}",
            ' ',
            <&[char]>::default(),
            Pen::DEFAULT.fg,
            Pen::DEFAULT.bg,
            None::<Color>,
            Flags::empty()
        );
        while rows
            .last()
            .is_some_and(|row| row.iter().all(|cell| *cell == blank))
        {
            rows.pop();
        }
        rows
    }

    /// Feeds `bytes`, snapshots, replays into a fresh term of `cols` and
    /// returns both terms.
    fn round_trip(
        cols: usize,
        replay_cols: usize,
        bytes: &[u8],
    ) -> (Term<VoidListener>, Term<VoidListener>, Vec<u8>) {
        let mut original = term(cols, 6);
        feed(&mut original, bytes);
        let snap = snapshot(&original);
        let mut replayed = term(replay_cols, 6);
        feed(&mut replayed, &snap);
        (original, replayed, snap)
    }

    #[test]
    fn colours_and_styles_come_back_cell_for_cell() {
        let bytes = "\x1b[31mred\x1b[0m \x1b[38;5;1mindexed\x1b[0m \
                     \x1b[38;2;10;20;30mrgb\x1b[0m \x1b[44mbg\x1b[0m\r\n\
                     \x1b[1mbold\x1b[2mdim\x1b[22mx \x1b[3mitalic\x1b[23m \
                     \x1b[7minverse\x1b[27m \x1b[8mhidden\x1b[28m \x1b[9mstrike\x1b[29m\r\n\
                     \x1b[4mu\x1b[4:2md\x1b[4:3mc\x1b[4:4mo\x1b[4:5ma\x1b[24m \
                     \x1b[4:3;58;2;200;100;0mcurl\x1b[59mplain\x1b[58;5;9mpal\x1b[0m\r\n\
                     \x1b[95;104mbright\x1b[39;49m end\r\n";
        let (original, replayed, _) = round_trip(80, 80, bytes.as_bytes());
        assert_eq!(cells(&replayed), cells(&original));
    }

    #[test]
    fn a_named_colour_stays_apart_from_its_palette_slot() {
        let (_, replayed, _) = round_trip(20, 20, b"\x1b[31ma\x1b[38;5;1mb\r\n");
        let row = &replayed.grid()[Line(0)];
        assert_eq!(row[Column(0)].fg, Color::Named(NamedColor::Red));
        assert_eq!(row[Column(1)].fg, Color::Indexed(1));
    }

    #[test]
    fn an_unchanged_pen_writes_no_sgr() {
        let (_, _, snap) = round_trip(40, 40, b"\x1b[31mone two three\x1b[0m\r\n");
        assert_eq!(snap, b"\x1b[31mone two three\x1b[0m\r\n");
    }

    #[test]
    fn wide_characters_and_emoji_clusters_come_back() {
        let bytes = "漢字 a👍🏽b 🇹🇷 ❤️ é\r\n".as_bytes();
        let (original, replayed, _) = round_trip(20, 20, bytes);
        assert_eq!(cells(&replayed), cells(&original));
        let row = &replayed.grid()[Line(0)];
        assert!(row[Column(0)].flags.contains(Flags::WIDE_CHAR));
        assert!(row[Column(1)].flags.contains(Flags::WIDE_CHAR_SPACER));
    }

    #[test]
    fn a_wrapped_row_rewraps_at_the_new_width() {
        let long = format!("{}\r\nnext\r\n", "abcdefghij".repeat(3));
        // Narrow → wide: the two wrapped rows join into one.
        let (_, wide, snap) = round_trip(10, 40, long.as_bytes());
        assert!(!snap.windows(2).take(30).any(|w| w == b"\r\n"));
        let first: String = (0..30).map(|c| wide.grid()[Line(0)][Column(c)].c).collect();
        assert_eq!(first, "abcdefghij".repeat(3));
        assert_eq!(wide.grid()[Line(1)][Column(0)].c, 'n');
        // Wide → narrow: an unwrapped long row wraps again.
        let (_, narrow, _) = round_trip(40, 10, long.as_bytes());
        let grid = narrow.grid();
        let text: String = (grid.topmost_line().0..=grid.bottommost_line().0)
            .flat_map(|line| (0..10).map(move |c| (line, c)))
            .map(|(line, c)| grid[Line(line)][Column(c)].c)
            .collect();
        assert!(text.starts_with(&"abcdefghij".repeat(3)), "{text}");
        assert!(
            grid[Line(grid.topmost_line().0)][Column(9)]
                .flags
                .contains(Flags::WRAPLINE)
        );
    }

    #[test]
    fn a_wide_character_pushed_to_the_next_row_comes_back_whole() {
        // 9 columns of text, then a wide character that does not fit.
        let (original, replayed, _) = round_trip(10, 10, "123456789漢\r\n".as_bytes());
        assert_eq!(cells(&replayed), cells(&original));
    }

    #[test]
    fn links_are_dropped_and_their_text_kept() {
        let bytes = b"\x1b]8;;bateri://block/3\x07$ ls\x1b]8;;\x07 \x1b]8;;https://x.dev\x07x\x1b]8;;\x07\r\n";
        let (_, replayed, snap) = round_trip(20, 20, bytes);
        assert!(!snap.windows(2).any(|w| w == b"\x1b]"));
        let text: String = (0..6)
            .map(|c| replayed.grid()[Line(0)][Column(c)].c)
            .collect();
        assert_eq!(text, "$ ls x");
        // `cells` asserts there is no link left.
        cells(&replayed);
    }

    #[test]
    fn trailing_empty_rows_are_dropped_and_the_stream_ends_with_a_line_break() {
        let (_, _, snap) = round_trip(20, 20, b"one\r\n\r\ntwo\r\n\r\n\r\n");
        assert_eq!(snap, b"one\r\n\r\ntwo\r\n");
        let (_, _, empty) = round_trip(20, 20, b"\r\n\r\n");
        assert!(empty.is_empty());
    }

    #[test]
    fn a_coloured_blank_tail_is_kept_and_the_pen_is_reset_before_the_break() {
        let (original, replayed, snap) = round_trip(20, 20, b"a\x1b[41m   \x1b[0m\r\nb\r\n");
        assert_eq!(cells(&replayed), cells(&original));
        assert!(snap.starts_with(b"a\x1b[41m   \x1b[0m\r\nb"));
    }

    #[test]
    fn history_above_the_screen_is_included() {
        let text: String = (0..20).map(|n| format!("line {n}\r\n")).collect();
        let (original, replayed, _) = round_trip(20, 20, text.as_bytes());
        assert!(original.grid().history_size() > 0);
        assert_eq!(cells(&replayed), cells(&original));
    }
}
