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
//! characters behind its base character, and a **finished** block's anchor
//! rewritten into our saved namespace (`bateri://sblock/<k>.<role>`, below).
//! What is **not**: other OSC 8 links (their text stays), modes, the cursor,
//! the alternate screen.
//!
//! **The block anchor is rewritten, not copied** (user decision 2026-10-03,
//! `.tasks/053-oturum-geri-yukleme/discussion.md` → Set sonrası
//! düzeltmeler): the new shell numbers its blocks from one, so a replayed
//! `block/N` would take the colour of the new session's block `N`. The
//! saved anchor carries the colour's **role** (`success`/`error`, resolved
//! from the ledger at quit) and the live theme paints it; `k` only keeps two
//! blocks apart, so a multi-row command's continuation rows stay one block.
//! A block running, pending or with an unreadable code at quit gets no
//! anchor — unknown is not drawn. No duration counter comes back.
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
use std::collections::HashMap;

use crate::shell::{BlockKey, Stripe};

/// The saved block anchor's prefix — written by [`encode`], read by
/// [`saved_key`].
const SAVED_PREFIX: &str = "bateri://sblock/";

/// The URI of saved block `id` with `stripe`'s role; `None` for a running
/// block, which is never saved.
fn saved_anchor(id: u32, stripe: Stripe) -> Option<String> {
    let role = match stripe {
        Stripe::Success => "success",
        Stripe::Error => "error",
        Stripe::Running => return None,
    };
    Some(format!("{SAVED_PREFIX}{id}.{role}"))
}

/// The key of a saved block anchor (`bateri://sblock/<k>.<role>`); `None`
/// for any other URI or an unknown role.
pub(crate) fn saved_key(uri: &str) -> Option<BlockKey> {
    let (id, role) = uri.strip_prefix(SAVED_PREFIX)?.split_once('.')?;
    let stripe = match role {
        "success" => Stripe::Success,
        "error" => Stripe::Error,
        _ => return None,
    };
    Some(BlockKey::Saved {
        id: id.parse().ok()?,
        stripe,
    })
}

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
///
/// `stripe` answers a cell link's URI: `Some` → a finished block of that
/// colour, written as a saved anchor; `None` → the link is dropped.
pub(crate) fn encode<T>(
    term: &Term<T>,
    end: i32,
    mut stripe: impl FnMut(&str) -> Option<Stripe>,
) -> Vec<u8> {
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
    // URI → saved anchor; `k` counts the saved blocks in order.
    let mut anchors: HashMap<String, Option<String>> = HashMap::new();
    let mut saved_blocks: u32 = 0;
    let mut open: Option<String> = None;
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
            let anchor = cell.hyperlink().and_then(|link| {
                let uri = link.uri();
                if let Some(saved) = anchors.get(uri) {
                    return saved.clone();
                }
                let saved = stripe(uri)
                    .filter(|stripe| *stripe != Stripe::Running)
                    .and_then(|stripe| {
                        saved_blocks = saved_blocks.saturating_add(1);
                        saved_anchor(saved_blocks, stripe)
                    });
                anchors.insert(uri.to_owned(), saved.clone());
                saved
            });
            if anchor != open {
                out.extend_from_slice(b"\x1b]8;;");
                out.extend_from_slice(anchor.as_deref().unwrap_or_default().as_bytes());
                out.push(0x07);
                open = anchor;
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
            // The link closes before the break like the pen: the rows a line
            // feed opens must not inherit it.
            if open.take().is_some() {
                out.extend_from_slice(b"\x1b]8;;\x07");
            }
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

    /// Everything up to and including the cursor's row, no block resolved.
    fn snapshot(term: &Term<VoidListener>) -> Vec<u8> {
        snapshot_with(term, |_| None)
    }

    fn snapshot_with(
        term: &Term<VoidListener>,
        stripe: impl FnMut(&str) -> Option<Stripe>,
    ) -> Vec<u8> {
        encode(term, term.grid().cursor.point.line.0 + 1, stripe)
    }

    /// The saved block key of every cell of `line`.
    fn keys(term: &Term<VoidListener>, line: i32) -> Vec<Option<BlockKey>> {
        (0..term.grid().columns())
            .map(|col| {
                term.grid()[Line(line)][Column(col)]
                    .hyperlink()
                    .and_then(|link| saved_key(link.uri()))
            })
            .collect()
    }

    fn saved(id: u32, stripe: Stripe) -> Option<BlockKey> {
        Some(BlockKey::Saved { id, stripe })
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
                        let link = cell.hyperlink().map(|link| link.uri().to_owned());
                        assert!(
                            link.as_deref()
                                .is_none_or(|uri| uri.starts_with(SAVED_PREFIX)),
                            "a link survived: {link:?}"
                        );
                        format!(
                            "{:?}{:?}|{:?}|{:?}|{:?}|{:?}|{link:?}",
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
            "{:?}{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
            ' ',
            <&[char]>::default(),
            Pen::DEFAULT.fg,
            Pen::DEFAULT.bg,
            None::<Color>,
            Flags::empty(),
            None::<String>,
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

    /// The ledger of the block-anchor tests: 3 succeeded, 4 failed, 5 runs.
    fn ledger(uri: &str) -> Option<Stripe> {
        match uri {
            "bateri://block/3" => Some(Stripe::Success),
            "bateri://block/4" => Some(Stripe::Error),
            "bateri://block/5" => Some(Stripe::Running),
            _ => saved_key(uri).and_then(|key| match key {
                BlockKey::Saved { stripe, .. } => Some(stripe),
                _ => None,
            }),
        }
    }

    #[test]
    fn a_finished_blocks_anchor_comes_back_saved_with_its_role() {
        // 053, seen in the real window: the restored commands had no chevron.
        let bytes = b"\x1b]8;;bateri://block/3\x07$ ls\x1b]8;;\x07\r\nout\r\n\
                      \x1b]8;;bateri://block/4\x07$ false\x1b]8;;\x07 \
                      \x1b]8;;https://x.dev\x07x\x1b]8;;\x07\r\n\
                      \x1b]8;;bateri://block/5\x07$ run\x1b]8;;\x07\r\n";
        let mut original = term(40, 6);
        feed(&mut original, bytes);
        let snap = snapshot_with(&original, ledger);
        let text = String::from_utf8(snap.clone()).unwrap();
        assert!(
            text.starts_with("\x1b]8;;bateri://sblock/1.success\x07$ ls\x1b]8;;\x07\r\nout\r\n")
        );
        assert!(text.contains("\x1b]8;;bateri://sblock/2.error\x07$ false\x1b]8;;\x07 x\r\n"));
        // The running block and the foreign link leave only their text.
        assert!(
            !text.contains("block/5") && !text.contains("x.dev"),
            "{text:?}"
        );
        let mut replayed = term(40, 6);
        feed(&mut replayed, &snap);
        let success = saved(1, Stripe::Success);
        let error = saved(2, Stripe::Error);
        assert_eq!(
            keys(&replayed, 0)[..5],
            [success, success, success, success, None]
        );
        assert_eq!(keys(&replayed, 1)[0], None);
        let row = keys(&replayed, 2);
        assert!(row[..7].iter().all(|key| *key == error), "{row:?}");
        assert_eq!(row[7], None);
        assert!(keys(&replayed, 3).iter().all(Option::is_none));
        // The text and the pen are what they were.
        let text_of = |term: &Term<VoidListener>| -> Vec<String> {
            (0..4)
                .map(|line| {
                    (0..40)
                        .map(|col| {
                            let cell = &term.grid()[Line(line)][Column(col)];
                            format!("{:?}{:?}{:?}", cell.c, cell.fg, cell.flags)
                        })
                        .collect()
                })
                .collect()
        };
        assert_eq!(text_of(&replayed), text_of(&original));
    }

    #[test]
    fn a_multi_row_command_stays_one_saved_block() {
        // The continuation rule (`block_row_continues`) needs every row of
        // the command to carry the **same** key: the wrapped row and the
        // `PS2` row after a line break alike, at the old and a new width.
        let bytes = b"\x1b]8;;bateri://block/3\x07$ abcdefghijkl\r\n> done\x1b]8;;\x07\r\n";
        let mut original = term(10, 6);
        feed(&mut original, bytes);
        let snap = snapshot_with(&original, ledger);
        let success = saved(1, Stripe::Success);
        for width in [10, 40] {
            let mut replayed = term(width, 6);
            feed(&mut replayed, &snap);
            let rows = if width == 10 { 3 } else { 2 };
            for line in 0..rows {
                assert_eq!(
                    keys(&replayed, line)[0],
                    success,
                    "width {width}, row {line}"
                );
            }
        }
    }

    #[test]
    fn a_second_save_writes_the_same_bytes() {
        // A restored pane quit again: its anchors are already saved ones and
        // no ledger knows them — the key carries the colour.
        let bytes = b"\x1b]8;;bateri://block/4\x07$ false\x1b]8;;\x07\r\nout\r\n\
                      \x1b]8;;bateri://block/3\x07$ ls\x1b]8;;\x07\r\n";
        let mut original = term(40, 6);
        feed(&mut original, bytes);
        let first = snapshot_with(&original, ledger);
        let mut replayed = term(40, 6);
        feed(&mut replayed, &first);
        assert_eq!(snapshot_with(&replayed, ledger), first);
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
