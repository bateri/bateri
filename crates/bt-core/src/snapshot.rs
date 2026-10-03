//! The scrollback's snapshot as VT bytes: what a pane leaves behind
//! when bateri quits, so the next launch can replay it into a fresh `Term`
//! ([`crate::SessionOptions::replay`]).
//!
//! **Two kinds, one row walk.** [`encode`] is session restore's: the primary history, a
//! finished block's anchor saved, nothing else — byte for byte as it was.
//! [`encode_live`] is the update handover's: the **whole** terminal state of
//! a pane whose processes live on — both screens, links as they are, the
//! cursors, the modes and what only a destructive probe reads; its doc
//! holds the order. [`Tail`] keeps the sequence a read stopped in. The
//! paragraphs below describe the restore kind.
//!
//! **Bytes, not a grid dump**: the replay goes through the same parser and the same
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
//! **The block anchor is rewritten, not copied** (user decision 2026-10-03): the new shell numbers its blocks from one, so a replayed
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

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::{Charsets, Cursor, Dimensions, Grid};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::{Cell as TermCell, Flags, Hyperlink};
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{
    CharsetIndex, Color, CursorShape, CursorStyle, Handler, KeyboardModes, NamedColor,
    NamedPrivateMode, PrivateMode, Rgb, StandardCharset,
};
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
/// the new shell's first
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
    // URI → saved anchor; `k` counts the saved blocks in order.
    let mut anchors: HashMap<String, Option<String>> = HashMap::new();
    let mut saved_blocks: u32 = 0;
    let mut link_of = |cell: &TermCell| {
        cell.hyperlink()
            .and_then(|link| {
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
            })
            // No parameters: the saved anchor carries no `id`.
            .map(|anchor| format!(";{anchor}"))
    };
    write_rows(&mut out, &rows[..=last], &mut link_of, true);
    out
}

/// Writes `rows` as VT bytes — the one row walk of both kinds ([`encode`],
/// [`encode_live`]).
///
/// `link_of` answers a cell's OSC 8 body (what follows `8;`: the parameters,
/// `;` and the URI); `None` → no link. Every row that does not wrap into the
/// next ends with the link closed, the pen back at the default and `\r\n` —
/// except the last one when `break_last` is `false`: a live screen's last row
/// must not scroll the screen one row into the history.
fn write_rows(
    out: &mut Vec<u8>,
    rows: &[Vec<&TermCell>],
    link_of: &mut dyn FnMut(&TermCell) -> Option<String>,
    break_last: bool,
) {
    let last = rows.len().saturating_sub(1);
    let mut pen = Pen::DEFAULT;
    let mut open: Option<String> = None;
    for (index, cells) in rows.iter().enumerate() {
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
            let anchor = link_of(cell);
            if anchor != open {
                out.extend_from_slice(b"\x1b]8;");
                out.extend_from_slice(anchor.as_deref().unwrap_or(";").as_bytes());
                out.push(0x07);
                open = anchor;
            }
            let next = Pen::of(cell);
            transition(out, pen, next);
            pen = next;
            push_cell_text(out, cell);
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
            if index < last || break_last {
                out.extend_from_slice(b"\r\n");
            }
        }
    }
}

/// A cell's base character and its zero-width characters behind it.
fn push_cell_text(out: &mut Vec<u8>, cell: &TermCell) {
    push_char(out, cell.c);
    for &extra in cell.zerowidth().unwrap_or_default() {
        push_char(out, extra);
    }
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

// ─── the live kind ───────────────────────────────────────────────────────

/// alacritty's keyboard-mode stack depth (`KEYBOARD_MODE_STACK_MAX_DEPTH`,
/// private in 0.26.0); [`probe_keyboard`] pops one past it.
const KEYBOARD_DEPTH: usize = 4096;

/// alacritty's title stack depth (`TITLE_STACK_MAX_DEPTH`, private).
const TITLE_DEPTH: usize = 4096;

/// What [`probe_titles`] writes before each pop: a pop from an empty stack
/// leaves it in place. The parser never produces it — an OSC cannot carry a
/// control character.
const TITLE_SENTINEL: &str = "\u{1}bateri-title-probe";

/// The mode bits [`encode_live`] writes, each with its set and reset
/// sequence. The rest of `TermMode` is `MODES_NOT_WRITTEN`; a test pins
/// that the two cover every bit.
const MODES: [(TermMode, &str, &str); 15] = [
    (TermMode::SHOW_CURSOR, "\x1b[?25h", "\x1b[?25l"),
    (TermMode::APP_CURSOR, "\x1b[?1h", "\x1b[?1l"),
    (TermMode::APP_KEYPAD, "\x1b=", "\x1b>"),
    (TermMode::MOUSE_REPORT_CLICK, "\x1b[?1000h", "\x1b[?1000l"),
    (TermMode::MOUSE_DRAG, "\x1b[?1002h", "\x1b[?1002l"),
    (TermMode::MOUSE_MOTION, "\x1b[?1003h", "\x1b[?1003l"),
    (TermMode::BRACKETED_PASTE, "\x1b[?2004h", "\x1b[?2004l"),
    (TermMode::SGR_MOUSE, "\x1b[?1006h", "\x1b[?1006l"),
    (TermMode::UTF8_MOUSE, "\x1b[?1005h", "\x1b[?1005l"),
    (TermMode::LINE_WRAP, "\x1b[?7h", "\x1b[?7l"),
    (TermMode::LINE_FEED_NEW_LINE, "\x1b[20h", "\x1b[20l"),
    (TermMode::ORIGIN, "\x1b[?6h", "\x1b[?6l"),
    (TermMode::INSERT, "\x1b[4h", "\x1b[4l"),
    (TermMode::FOCUS_IN_OUT, "\x1b[?1004h", "\x1b[?1004l"),
    (TermMode::ALTERNATE_SCROLL, "\x1b[?1007h", "\x1b[?1007l"),
];

/// The mode bits that are not a mode sequence: the alternate screen is the
/// content's (`?1049h`), the kitty bits the keyboard stacks' (`CSI > u`,
/// `CSI = u`), and `VI` and `URGENCY_HINTS` are the terminal's own — no
/// application turns vi mode on, and the urgency hint is a window setting.
#[cfg(test)]
const MODES_NOT_WRITTEN: TermMode = TermMode::ALT_SCREEN
    .union(TermMode::KITTY_KEYBOARD_PROTOCOL)
    .union(TermMode::VI)
    .union(TermMode::URGENCY_HINTS);

/// The kitty bits of `TermMode` and the keyboard mode each one is.
const KITTY: [(TermMode, KeyboardModes); 5] = [
    (
        TermMode::DISAMBIGUATE_ESC_CODES,
        KeyboardModes::DISAMBIGUATE_ESC_CODES,
    ),
    (
        TermMode::REPORT_EVENT_TYPES,
        KeyboardModes::REPORT_EVENT_TYPES,
    ),
    (
        TermMode::REPORT_ALTERNATE_KEYS,
        KeyboardModes::REPORT_ALTERNATE_KEYS,
    ),
    (
        TermMode::REPORT_ALL_KEYS_AS_ESC,
        KeyboardModes::REPORT_ALL_KEYS_AS_ESC,
    ),
    (
        TermMode::REPORT_ASSOCIATED_TEXT,
        KeyboardModes::REPORT_ASSOCIATED_TEXT,
    ),
];

/// The four designations, in index order: `ESC ( 0` designates G0 and so on.
const DESIGNATORS: [(CharsetIndex, u8); 4] = [
    (CharsetIndex::G0, b'('),
    (CharsetIndex::G1, b')'),
    (CharsetIndex::G2, b'*'),
    (CharsetIndex::G3, b'+'),
];

/// The highest palette index an OSC sets: 0–255 with OSC 4, then the
/// foreground, background and cursor colours with OSC 10/11/12.
const LAST_SETTABLE_COLOR: usize = NamedColor::Cursor as usize;

/// What only a **destructive probe** reads from `Term`: alacritty keeps these fields private and no getter exists.
/// [`encode_live`] writes them; the round-trip tests compare them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Probed {
    /// The tab stops a forward tab lands on, ascending; the last column
    /// (where a tab always stops) and column 0 (where none goes) are not
    /// observable and not listed.
    pub(crate) tabs: Vec<usize>,
    /// The scrolling region's first and last row, inclusive.
    pub(crate) region: (usize, usize),
    /// The active character set (SI/SO).
    pub(crate) charset: CharsetIndex,
    /// The primary and the alternate screen's keyboard-mode stacks, bottom
    /// to top. A `NO_MODE` entry at the bottom is not observable and is
    /// dropped — popping down to it or past it reads `NO_MODE` alike.
    pub(crate) keyboard: [Vec<KeyboardModes>; 2],
    /// The title stack, bottom to top (`CSI 22 t` pushes, `CSI 23 t` pops).
    pub(crate) titles: Vec<Option<String>>,
    /// The current title as the listener knows it.
    pub(crate) title: Option<String>,
    /// The application's cursor style; `None` → the terminal's default.
    pub(crate) cursor_style: Option<CursorStyle>,
}

/// A cursor as written: where it is, the cell to write again when
/// `input_needs_wrap` is set, its pen, its open link and its four
/// designations.
struct CursorShot {
    line: i32,
    column: usize,
    /// `input_needs_wrap` is not settable by a sequence: only writing the
    /// last column sets it. The cell is written again — the real one, with
    /// its pen and link — and the cursor lands where it was (a wide
    /// character's spacer: the character itself, one column back).
    rewrite: Option<(usize, TermCell)>,
    pen: Pen,
    /// The template's link — bateri's prompt anchor is open while the user
    /// types, and the characters typed after the handover must carry it.
    link: Option<String>,
    charsets: Charsets,
}

impl CursorShot {
    fn of(grid: &Grid<TermCell>, cursor: &Cursor<TermCell>) -> Self {
        let point = cursor.point;
        let rewrite = cursor
            .input_needs_wrap
            .then(|| {
                let in_grid = point.line >= grid.topmost_line()
                    && point.line <= grid.bottommost_line()
                    && point.column.0 < grid.columns();
                in_grid.then_some(())?;
                let row = &grid[point.line];
                let mut column = point.column.0;
                if column > 0 && row[point.column].flags.contains(Flags::WIDE_CHAR_SPACER) {
                    column -= 1;
                }
                Some((column, row[Column(column)].clone()))
            })
            .flatten();
        Self {
            line: point.line.0,
            column: point.column.0,
            rewrite,
            pen: Pen::of(&cursor.template),
            link: cursor.template.hyperlink().as_ref().map(live_link),
            charsets: cursor.charsets,
        }
    }

    /// Writes the cursor; `origin` is the scrolling region's first row when
    /// origin mode is on (`CUP` is then relative to it). Expects the pen at
    /// the default, no link open and ASCII in all four sets.
    fn write(&self, out: &mut Vec<u8>, origin: Option<usize>) {
        let row = (self.line - origin.map_or(0, |top| top as i32)).max(0);
        match &self.rewrite {
            Some((column, cell)) => {
                cup(out, row, *column);
                if let Some(link) = cell.hyperlink().as_ref().map(live_link) {
                    osc8(out, &link);
                }
                transition(out, Pen::DEFAULT, Pen::of(cell));
                push_cell_text(out, cell);
                out.extend_from_slice(b"\x1b]8;;\x07\x1b[0m");
            }
            None => cup(out, row, self.column),
        }
        transition(out, Pen::DEFAULT, self.pen);
        if let Some(link) = &self.link {
            osc8(out, link);
        }
        for (index, intro) in DESIGNATORS {
            if self.charsets[index] == StandardCharset::SpecialCharacterAndLineDrawing {
                out.extend_from_slice(&[0x1b, intro, b'0']);
            }
        }
    }
}

/// Back to the state [`CursorShot::write`] expects: no link, the default
/// pen, ASCII in all four sets.
fn reset_pen(out: &mut Vec<u8>) {
    out.extend_from_slice(b"\x1b]8;;\x07\x1b[0m\x1b(B\x1b)B\x1b*B\x1b+B");
}

fn cup(out: &mut Vec<u8>, row: i32, column: usize) {
    out.extend_from_slice(format!("\x1b[{};{}H", row + 1, column + 1).as_bytes());
}

fn osc8(out: &mut Vec<u8>, body: &str) {
    out.extend_from_slice(b"\x1b]8;");
    out.extend_from_slice(body.as_bytes());
    out.push(0x07);
}

/// A link's OSC 8 body with its `id` — alacritty's own ids (`N_alacritty`)
/// included, so the replay gives every cell the same link back.
fn live_link(link: &Hyperlink) -> String {
    format!("id={};{}", link.id(), link.uri())
}

/// The rows from `top` through the bottom of the screen, with links as they
/// are and no break after the last row.
fn live_rows(grid: &Grid<TermCell>, top: i32) -> Vec<u8> {
    let rows: Vec<Vec<&TermCell>> = (top..=grid.bottommost_line().0)
        .map(|line| grid[Line(line)].into_iter().collect())
        .collect();
    let mut out = Vec::new();
    let mut link_of = |cell: &TermCell| cell.hyperlink().as_ref().map(live_link);
    write_rows(&mut out, &rows, &mut link_of, false);
    out
}

/// The kitty bits of `mode` as a keyboard mode.
fn keyboard_bits(mode: TermMode) -> KeyboardModes {
    KITTY
        .iter()
        .filter(|(bit, _)| mode.contains(*bit))
        .fold(KeyboardModes::NO_MODE, |all, (_, keys)| all | *keys)
}

/// The active screen's keyboard-mode stack, bottom to top — **destructive**,
/// it empties the stack.
///
/// The top entry is not observable by popping (`pop_keyboard_modes` shows
/// the entry **under** the popped one, and `mode()` may differ from the top
/// after `CSI = u`), so a dummy is pushed first. A no-op while
/// `kitty_keyboard` is off (bateri's config): push and pop return at once
/// and the stack reads empty.
fn probe_keyboard<T: EventListener>(term: &mut Term<T>) -> Vec<KeyboardModes> {
    term.push_keyboard_mode(KeyboardModes::NO_MODE);
    let mut stack: Vec<KeyboardModes> = (0..=KEYBOARD_DEPTH)
        .map(|_| {
            term.pop_keyboard_modes(1);
            keyboard_bits(*term.mode())
        })
        .collect();
    while stack.last() == Some(&KeyboardModes::NO_MODE) {
        stack.pop();
    }
    stack.reverse();
    stack
}

/// The current title and the title stack, bottom to top — **destructive**,
/// it empties the stack; the listener's title is put back.
///
/// `title` reads the listener's title, which `Title`/`ResetTitle` write. A
/// pop from an empty stack sends no event, so the sentinel written before
/// each pop stays — that is the stack's bottom.
fn probe_titles<T: EventListener>(
    term: &mut Term<T>,
    title: &dyn Fn() -> Option<String>,
) -> (Option<String>, Vec<Option<String>>) {
    let current = title();
    let mut stack = Vec::new();
    for _ in 0..TITLE_DEPTH {
        term.set_title(Some(TITLE_SENTINEL.to_owned()));
        term.pop_title();
        let popped = title();
        if popped.as_deref() == Some(TITLE_SENTINEL) {
            break;
        }
        stack.push(popped);
    }
    stack.reverse();
    term.set_title(current.clone());
    (current, stack)
}

/// The tab stops — **destructive**: a tab writes into a blank cell it
/// passes (`put_tab`).
fn probe_tabs<T: EventListener>(term: &mut Term<T>) -> Vec<usize> {
    let last = term.columns().saturating_sub(1);
    let cursor = &mut term.grid_mut().cursor;
    cursor.point = Point::new(Line(0), Column(0));
    cursor.input_needs_wrap = false;
    let mut stops = Vec::new();
    loop {
        term.put_tab(1);
        let column = term.grid().cursor.point.column.0;
        if column >= last {
            return stops;
        }
        stops.push(column);
    }
}

/// The scrolling region — **destructive**: origin mode is left on and the
/// cursor moved. Under origin mode `goto` clamps into the region, so the
/// home position is its first row and a goto far below its last one; no row
/// scrolls.
fn probe_region<T: EventListener>(term: &mut Term<T>) -> (usize, usize) {
    let lines = term.screen_lines() as i32;
    term.set_private_mode(PrivateMode::Named(NamedPrivateMode::Origin));
    let top = term.grid().cursor.point.line.0;
    term.goto(lines, 0);
    let bottom = term.grid().cursor.point.line.0;
    (top.max(0) as usize, bottom.max(0) as usize)
}

/// The active character set — **destructive**: it writes into the top-left
/// cell. One set at a time maps `q` to the line-drawing `─`; the active one
/// is the one that does.
fn probe_charset<T: EventListener>(term: &mut Term<T>) -> CharsetIndex {
    for (index, _) in DESIGNATORS {
        let cursor = &mut term.grid_mut().cursor;
        cursor.charsets = Charsets::default();
        cursor.charsets[index] = StandardCharset::SpecialCharacterAndLineDrawing;
        cursor.point = Point::new(Line(0), Column(0));
        cursor.input_needs_wrap = false;
        term.input('q');
        if term.grid()[Line(0)][Column(0)].c == '─' {
            return index;
        }
    }
    CharsetIndex::G0
}

/// Whether the application set the cursor style — **destructive**: the
/// config is replaced. `cursor_style()` answers the application's style or
/// the config's default; with a different default only the former stays.
fn probe_cursor_style<T: EventListener>(term: &mut Term<T>) -> Option<CursorStyle> {
    let before = term.cursor_style();
    let shape = if before.shape == CursorShape::Block {
        CursorShape::Beam
    } else {
        CursorShape::Block
    };
    let flipped = CursorStyle {
        shape,
        blinking: !before.blinking,
    };
    term.set_options(Config {
        default_cursor_style: flipped,
        ..Config::default()
    });
    (term.cursor_style() == before).then_some(before)
}

/// One screen as read before the probes: its rows and its two cursors.
struct ScreenShot {
    rows: Vec<u8>,
    cursor: CursorShot,
    saved: CursorShot,
    keyboard: Vec<KeyboardModes>,
}

impl ScreenShot {
    /// Reads the active grid from `top` (the history's top on the primary
    /// screen, `0` on the alternate one) and then probes its keyboard stack.
    fn read<T: EventListener>(term: &mut Term<T>, primary: bool) -> Self {
        let grid = term.grid();
        let top = if primary { grid.topmost_line().0 } else { 0 };
        let rows = live_rows(grid, top);
        let cursor = CursorShot::of(grid, &grid.cursor);
        let saved = CursorShot::of(grid, &grid.saved_cursor);
        let keyboard = probe_keyboard(term);
        Self {
            rows,
            cursor,
            saved,
            keyboard,
        }
    }
}

/// The pane's **whole** terminal state as VT bytes (the update handover): a
/// fresh `Term` of the **same size** with at least as much `scrolling_history`
/// that parses them through [`crate::handler::ClusterHandler`] reads back
/// every field `Term` shows and every field [`Probed`] lists. Resizing is the
/// caller's, after the replay.
///
/// **Destructive**: what alacritty keeps private is probed **after** the content is
/// read, by driving `Term` itself — on the alternate screen the primary is
/// reached with `swap_alt`, the keyboard and title stacks are popped empty,
/// tabs and the active set write cells, the config is replaced. The `Term`
/// must not be drawn or fed again; quit-only, like
/// [`crate::Session::final_history`].
///
/// `title` reads the listener's title (`Title`/`ResetTitle`).
///
/// **The order is the contract** (alacritty 0.26.0's `swap_alt`,
/// `term/mod.rs:714`: entering the alternate screen copies the primary
/// cursor into it, **overwrites the primary's saved cursor** with its cursor
/// and swaps the keyboard stacks; leaving restores no cursor):
///
/// 1. the primary rows, links as they are (`id` included);
/// 2. off the alternate screen, its kept keyboard stack: `?1049h`, pushes,
///    `?1049l` — before the saved cursor, which `?1049h` overwrites;
/// 3. the primary keyboard stack (`CSI > n u`);
/// 4. off the alternate screen the primary saved cursor and `ESC 7`; on it
///    the primary cursor (equal to its saved one — entering overwrote it),
///    then `?1049h`, the alternate rows, its keyboard stack, its saved
///    cursor and `ESC 7`. Each cursor is position (the cell written again
///    when `input_needs_wrap` is set), pen, open link and designations, and
///    the pen goes back to the default before more rows: rows are written
///    with ASCII in all four sets — the cells hold mapped characters;
/// 5. the scrolling region, the tab stops, the title stack (`CSI 22 t`) and
///    the title;
/// 6. the changed palette (OSC 4/10/11/12) and the application's cursor
///    style (DECSCUSR);
/// 7. every mode bit outside `MODES_NOT_WRITTEN`, all resets before all
///    sets (the mouse modes and encodings exclude one another), and the
///    active kitty bits when they differ from the stack's top (`CSI = n u`);
/// 8. the active screen's cursor — **after** the modes: `?6h` homes the
///    cursor and under origin mode `CUP` is region-relative — and SI/SO
///    last, since it maps what the cursor's rewritten cell writes.
///
/// **Known limits:** a title of `None` in the stack, or after one, comes
/// back as `""` (no sequence resets the title to none; the window title
/// reads the two alike); G2/G3 as the active set comes back as G0 (no
/// sequence invokes them); off the alternate screen its saved cursor is not
/// carried (the next `?1049h` keeps it, invisible until a `DECRC` without a
/// `DECSC`).
pub(crate) fn encode_live<T: EventListener>(
    term: &mut Term<T>,
    title: &dyn Fn() -> Option<String>,
) -> (Vec<u8>, Probed) {
    let mode = *term.mode();
    let colors: Vec<Option<Rgb>> = (0..=LAST_SETTABLE_COLOR)
        .map(|index| term.colors()[index])
        .collect();
    let alt = mode.contains(TermMode::ALT_SCREEN);
    let (title_now, titles) = probe_titles(term, title);
    // The active screen first: the other one is behind `swap_alt`.
    let (primary, alternate, kept) = if alt {
        let alternate = ScreenShot::read(term, false);
        term.swap_alt();
        (ScreenShot::read(term, true), Some(alternate), Vec::new())
    } else {
        let primary = ScreenShot::read(term, true);
        // Entering resets the alternate grid; only its kept stack is read.
        term.swap_alt();
        (primary, None, probe_keyboard(term))
    };
    let lines = term.screen_lines();
    let columns = term.columns();
    let probed = Probed {
        tabs: probe_tabs(term),
        region: probe_region(term),
        charset: probe_charset(term),
        keyboard: [
            primary.keyboard.clone(),
            alternate
                .as_ref()
                .map_or_else(|| kept.clone(), |screen| screen.keyboard.clone()),
        ],
        titles,
        title: title_now,
        // Last: it replaces the config, and a different `kitty_keyboard`
        // would empty the stacks.
        cursor_style: probe_cursor_style(term),
    };

    // 1–4: the screens.
    let mut out = primary.rows;
    if !kept.is_empty() {
        out.extend_from_slice(b"\x1b[?1049h");
        push_keyboard(&mut out, &kept);
        out.extend_from_slice(b"\x1b[?1049l");
    }
    push_keyboard(&mut out, &primary.keyboard);
    match &alternate {
        None => {
            primary.saved.write(&mut out, None);
            out.extend_from_slice(b"\x1b7");
            reset_pen(&mut out);
        }
        Some(screen) => {
            primary.cursor.write(&mut out, None);
            out.extend_from_slice(b"\x1b[?1049h");
            reset_pen(&mut out);
            out.extend_from_slice(b"\x1b[H");
            out.extend_from_slice(&screen.rows);
            push_keyboard(&mut out, &screen.keyboard);
            screen.saved.write(&mut out, None);
            out.extend_from_slice(b"\x1b7");
            reset_pen(&mut out);
        }
    }

    // 5: the region, the tab stops, the titles.
    let (top, bottom) = probed.region;
    if (top, bottom) != (0, lines.saturating_sub(1)) {
        out.extend_from_slice(format!("\x1b[{};{}r", top + 1, bottom + 1).as_bytes());
    }
    let last = columns.saturating_sub(1);
    let default_tabs: Vec<usize> = (8..last).step_by(8).collect();
    if probed.tabs != default_tabs {
        out.extend_from_slice(b"\x1b[3g");
        for stop in &probed.tabs {
            out.extend_from_slice(format!("\x1b[{}G\x1bH", stop + 1).as_bytes());
        }
    }
    for entry in &probed.titles {
        osc2(&mut out, entry.as_deref().unwrap_or_default());
        out.extend_from_slice(b"\x1b[22t");
    }
    match &probed.title {
        Some(title) => osc2(&mut out, title),
        None if !probed.titles.is_empty() => osc2(&mut out, ""),
        None => {}
    }

    // 6: the palette and the cursor style.
    for (index, color) in colors.iter().enumerate() {
        let Some(rgb) = color else {
            continue;
        };
        let spec = format!("rgb:{:02x}/{:02x}/{:02x}", rgb.r, rgb.g, rgb.b);
        let osc = if index < 256 {
            format!("\x1b]4;{index};{spec}\x07")
        } else {
            format!("\x1b]{};{spec}\x07", 10 + index - 256)
        };
        out.extend_from_slice(osc.as_bytes());
    }
    if let Some(style) = probed.cursor_style {
        let base = match style.shape {
            CursorShape::Underline => 3,
            CursorShape::Beam => 5,
            // A hollow block and a hidden cursor are the terminal's, no
            // sequence asks for them.
            CursorShape::Block | CursorShape::HollowBlock | CursorShape::Hidden => 1,
        };
        let code = if style.blinking { base } else { base + 1 };
        out.extend_from_slice(format!("\x1b[{code} q").as_bytes());
    }

    // 7: the modes.
    for (bit, _, reset) in MODES {
        if !mode.contains(bit) {
            out.extend_from_slice(reset.as_bytes());
        }
    }
    for (bit, set, _) in MODES {
        if mode.contains(bit) {
            out.extend_from_slice(set.as_bytes());
        }
    }
    let active_keys = alternate
        .as_ref()
        .map_or(&primary.keyboard, |screen| &screen.keyboard);
    let keys = keyboard_bits(mode);
    if keys
        != active_keys
            .last()
            .copied()
            .unwrap_or(KeyboardModes::NO_MODE)
    {
        out.extend_from_slice(format!("\x1b[={};1u", keys.bits()).as_bytes());
    }

    // 8: the active cursor, then the active set.
    let origin = mode.contains(TermMode::ORIGIN).then_some(top);
    let active = alternate
        .as_ref()
        .map_or(&primary.cursor, |screen| &screen.cursor);
    active.write(&mut out, origin);
    if probed.charset == CharsetIndex::G1 {
        out.push(0x0e);
    }
    (out, probed)
}

/// `CSI > n u` for each entry, bottom first.
fn push_keyboard(out: &mut Vec<u8>, stack: &[KeyboardModes]) {
    for modes in stack {
        out.extend_from_slice(format!("\x1b[>{}u", modes.bits()).as_bytes());
    }
}

/// OSC 2: the title.
fn osc2(out: &mut Vec<u8>, title: &str) {
    out.extend_from_slice(b"\x1b]2;");
    out.extend_from_slice(title.as_bytes());
    out.push(0x07);
}

// ─── the cut sequence ────────────────────────────────────────────────────

/// Where the parser stands, as far as [`Tail`] needs: vte's states folded
/// into the ones that end a sequence alike (`vte-0.15.0/src/lib.rs`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Seq {
    #[default]
    Ground,
    Escape,
    EscapeIntermediate,
    /// Any CSI state: every one ends on `0x40..=0x7E`.
    Csi,
    /// DCS before its final byte: vte's entry, parameter and intermediate
    /// states, apart because they go to the ignored state on different
    /// bytes.
    DcsEntry,
    DcsParam,
    DcsIntermediate,
    /// DCS passthrough: CAN, SUB, ESC and the 8-bit ST end it.
    DcsBody,
    /// SOS/PM/APC and an ignored DCS: only CAN, SUB and ESC end them — not
    /// `0x9c`, which a UTF-8 payload carries as a continuation byte.
    Ignored,
    Osc,
}

/// How many bytes of capacity [`Tail`] keeps once a sequence ends — one
/// large OSC 52 must not pin its size for the session's life.
const TAIL_KEEP: usize = 4096;

/// The bytes since the parser last stood in its **ground state**: a read
/// that ended inside a CSI/OSC/DCS or inside one character's UTF-8 left
/// them with no effect on `Term` yet — vte dispatches only at the final
/// byte — so the new side puts them **before** the bytes that follow and a
/// fresh parser picks the sequence up where the old one stood.
///
/// vte does not say where it stands; this follows the
/// same bytes on the read path. The C0 controls vte **executes** inside an
/// escape or a CSI are left out — they already took effect.
///
/// **Known limit:** the cut is per code point, not per cluster
/// ([`crate::handler::ClusterHandler`]'s `last_input` is the reader's): an
/// emoji sequence cut **between** two code points comes back as two
/// clusters.
#[derive(Debug, Default)]
pub(crate) struct Tail {
    state: Seq,
    /// UTF-8 continuation bytes still missing in the ground state.
    utf8_need: u8,
    bytes: Vec<u8>,
}

impl Tail {
    /// Follows `bytes`, which the parser gets next and in this order.
    pub(crate) fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.step(byte);
        }
    }

    /// The bytes of the sequence (or the character) the stream stopped in;
    /// empty in the ground state.
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    fn ground(&mut self) {
        self.state = Seq::Ground;
        self.utf8_need = 0;
        self.bytes.clear();
        if self.bytes.capacity() > TAIL_KEEP {
            self.bytes.shrink_to(TAIL_KEEP);
        }
    }

    /// ESC from the ground or inside any sequence: a **new** sequence.
    fn escape(&mut self) {
        self.ground();
        self.state = Seq::Escape;
        self.bytes.push(0x1b);
    }

    /// The bytes vte handles the same in every state (its `anywhere`).
    fn anywhere(&mut self, byte: u8) {
        match byte {
            0x18 | 0x1a => self.ground(),
            0x1b => self.escape(),
            _ => {}
        }
    }

    fn enter(&mut self, byte: u8, state: Seq) {
        self.bytes.push(byte);
        self.state = state;
    }

    fn step(&mut self, byte: u8) {
        const EXECUTED: [std::ops::RangeInclusive<u8>; 3] = [0x00..=0x17, 0x19..=0x19, 0x1c..=0x1f];
        let executed = EXECUTED.iter().any(|range| range.contains(&byte));
        match self.state {
            Seq::Ground => {
                if byte == 0x1b {
                    return self.escape();
                }
                if self.utf8_need > 0 {
                    if byte & 0xc0 == 0x80 {
                        self.bytes.push(byte);
                        self.utf8_need -= 1;
                        if self.utf8_need == 0 {
                            self.ground();
                        }
                        return;
                    }
                    // Not a continuation: vte prints U+FFFD and reads this
                    // byte afresh.
                    self.ground();
                }
                let need = match byte {
                    0xc2..=0xdf => 1,
                    0xe0..=0xef => 2,
                    0xf0..=0xf4 => 3,
                    _ => 0,
                };
                if need > 0 {
                    self.bytes.push(byte);
                    self.utf8_need = need;
                }
            }
            Seq::Escape => match byte {
                0x18 | 0x1a => self.ground(),
                // A second ESC is ignored, not a restart (vte's `advance_esc`).
                0x1b => {}
                _ if executed => {}
                0x20..=0x2f => self.enter(byte, Seq::EscapeIntermediate),
                0x50 => self.enter(byte, Seq::DcsEntry),
                0x58 | 0x5e | 0x5f => self.enter(byte, Seq::Ignored),
                0x5b => self.enter(byte, Seq::Csi),
                0x5d => self.enter(byte, Seq::Osc),
                0x30..=0x7e => self.ground(),
                _ => {}
            },
            Seq::EscapeIntermediate => match byte {
                _ if executed => {}
                0x20..=0x2f => self.bytes.push(byte),
                0x30..=0x7e => self.ground(),
                _ => self.anywhere(byte),
            },
            Seq::Csi => match byte {
                _ if executed => {}
                0x20..=0x3f => self.bytes.push(byte),
                0x40..=0x7e => self.ground(),
                _ => self.anywhere(byte),
            },
            Seq::DcsEntry => match byte {
                0x20..=0x2f => self.enter(byte, Seq::DcsIntermediate),
                0x30..=0x3f => self.enter(byte, Seq::DcsParam),
                0x40..=0x7e => self.enter(byte, Seq::DcsBody),
                _ => self.anywhere(byte),
            },
            Seq::DcsParam => match byte {
                0x20..=0x2f => self.enter(byte, Seq::DcsIntermediate),
                0x30..=0x3b => self.bytes.push(byte),
                0x3c..=0x3f => self.enter(byte, Seq::Ignored),
                0x40..=0x7e => self.enter(byte, Seq::DcsBody),
                _ => self.anywhere(byte),
            },
            Seq::DcsIntermediate => match byte {
                0x20..=0x2f => self.bytes.push(byte),
                0x30..=0x3f => self.enter(byte, Seq::Ignored),
                0x40..=0x7e => self.enter(byte, Seq::DcsBody),
                _ => self.anywhere(byte),
            },
            Seq::Ignored => match byte {
                0x18 | 0x1a => self.ground(),
                0x1b => self.escape(),
                _ => self.bytes.push(byte),
            },
            Seq::DcsBody => match byte {
                0x18 | 0x1a | 0x9c => self.ground(),
                0x1b => self.escape(),
                _ => self.bytes.push(byte),
            },
            Seq::Osc => match byte {
                0x07 | 0x18 | 0x1a => self.ground(),
                0x1b => self.escape(),
                // Ignored inside an OSC.
                _ if executed => {}
                _ => self.bytes.push(byte),
            },
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
        // Seen in the real window: the restored commands had no chevron.
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

    // ─── the live kind ──────────────────────────────────────────────────

    use alacritty_terminal::event::Event;
    use std::sync::{Arc, Mutex};

    /// The title as `Title`/`ResetTitle` leave it — the listener the probe
    /// reads, like `Adapter`'s slot.
    #[derive(Clone, Default)]
    struct Recorder(Arc<Mutex<Option<String>>>);

    impl Recorder {
        fn title(&self) -> Option<String> {
            self.0.lock().unwrap().clone()
        }
    }

    impl EventListener for Recorder {
        fn send_event(&self, event: Event) {
            match event {
                Event::Title(title) => *self.0.lock().unwrap() = Some(title),
                Event::ResetTitle => *self.0.lock().unwrap() = None,
                _ => {}
            }
        }
    }

    /// A term with the kitty protocol on: bateri's is off, but the probe
    /// must read the stacks when it is not.
    fn live_term(cols: usize, rows: usize) -> (Term<Recorder>, Recorder) {
        let config = Config {
            scrolling_history: 1000,
            kitty_keyboard: true,
            ..Config::default()
        };
        let recorder = Recorder::default();
        let term = Term::new(config, &TermSize::new(cols, rows), recorder.clone());
        (term, recorder)
    }

    fn feed_into<T: EventListener>(term: &mut Term<T>, bytes: &[u8]) {
        let mut parser: Processor = Processor::new();
        let mut last_input = false;
        parser.advance(&mut ClusterHandler::new(term, true, &mut last_input), bytes);
    }

    fn encode_of(term: &mut Term<Recorder>, recorder: &Recorder) -> (Vec<u8>, Probed) {
        let recorder = recorder.clone();
        encode_live(term, &move || recorder.title())
    }

    /// What `Term` shows of the active screen without a probe.
    fn readable<T>(term: &Term<T>) -> String {
        let grid = term.grid();
        let cursor = |cursor: &Cursor<TermCell>| {
            format!(
                "{:?} wrap={} pen={:?} link={:?} sets={:?}",
                cursor.point,
                cursor.input_needs_wrap,
                Pen::of(&cursor.template),
                cursor.template.hyperlink().as_ref().map(live_link),
                cursor.charsets,
            )
        };
        let colors: Vec<Option<Rgb>> = (0..=LAST_SETTABLE_COLOR)
            .map(|index| term.colors()[index])
            .collect();
        format!(
            "mode={:?}\ncursor={}\nsaved={}\ncolors={colors:?}\nhistory={}\nstyle={:?}",
            term.mode(),
            cursor(&grid.cursor),
            cursor(&grid.saved_cursor),
            grid.history_size(),
            term.cursor_style(),
        )
    }

    struct LiveTrip {
        probed: Probed,
        title: Option<String>,
    }

    /// Feeds `bytes`, encodes, replays into a fresh term of the same size
    /// and checks the replay against the original: what `Term` shows, the
    /// bytes a second encode gives (every cell of both screens) and every
    /// probed field.
    fn live_trip(cols: usize, rows: usize, bytes: &[u8]) -> LiveTrip {
        let (mut original, recorder) = live_term(cols, rows);
        feed_into(&mut original, bytes);
        let shown = readable(&original);
        let (first, probed) = encode_of(&mut original, &recorder);
        let (mut replayed, replay_recorder) = live_term(cols, rows);
        feed_into(&mut replayed, &first);
        assert_eq!(
            readable(&replayed),
            shown,
            "{:?}",
            String::from_utf8_lossy(&first)
        );
        let title = replay_recorder.title();
        let (second, again) = encode_of(&mut replayed, &replay_recorder);
        assert_eq!(
            String::from_utf8_lossy(&second),
            String::from_utf8_lossy(&first)
        );
        assert_eq!(again, probed);
        LiveTrip { probed, title }
    }

    #[test]
    fn every_mode_bit_is_written_or_named_as_not_written() {
        let mut seen = 0;
        for (name, bit) in TermMode::all().iter_names() {
            if bit.bits().count_ones() != 1 {
                continue;
            }
            seen += 1;
            let written = MODES.iter().any(|(mode, _, _)| *mode == bit);
            assert!(
                written != MODES_NOT_WRITTEN.contains(bit),
                "{name} is written {written}"
            );
        }
        assert_eq!(seen, 23, "a new mode bit needs a place");
    }

    #[test]
    fn a_vim_like_alternate_screen_comes_back_with_both_cursors_and_the_region() {
        let mut bytes: String = (0..12).map(|n| format!("line {n}\r\n")).collect();
        // The primary's saved cursor (bold green, line drawing in G0), then
        // the shell's prompt.
        bytes.push_str("\x1b[3;5H\x1b[1;32m\x1b(0\x1b7\x1b(B\x1b[0m\x1b[8;1H$ vim");
        // vim: the alternate screen, a region, a status line, its own
        // saved cursor and a cursor with an open link.
        bytes.push_str("\x1b[?1049h\x1b[2;7r\x1b[H~\r\n~\x1b[8;1H\x1b[7m-- INSERT --\x1b[0m");
        bytes.push_str("\x1b[4;3H\x1b[4mx\x1b7\x1b[24m\x1b[5;10H\x1b]8;id=a;https://x.dev\x07li");
        let trip = live_trip(20, 8, bytes.as_bytes());
        assert_eq!(trip.probed.region, (1, 6));
    }

    #[test]
    fn the_primary_saved_cursor_survives_the_alternate_screen() {
        // Leaving vim: the primary's cursor and the rows are what they were.
        let bytes = "one\r\ntwo\x1b[1;2H\x1b7\x1b[2;3H\x1b[?1049hvim\x1b[?1049l";
        let (mut original, recorder) = live_term(20, 6);
        feed_into(&mut original, bytes.as_bytes());
        let (first, _) = encode_of(&mut original, &recorder);
        let (mut replayed, _) = live_term(20, 6);
        feed_into(&mut replayed, &first);
        // `?1049h` overwrote the saved cursor with the cursor (2;3): the
        // replay must carry that, not the `ESC 7` before it.
        assert_eq!(
            replayed.grid().saved_cursor.point,
            Point::new(Line(1), Column(2))
        );
        live_trip(20, 6, bytes.as_bytes());
    }

    #[test]
    fn modes_palette_style_keyboard_titles_tabs_and_the_active_set_come_back() {
        let bytes = "\x1b]4;1;rgb:12/34/56\x07\x1b]11;rgb:01/02/03\x07\x1b]12;#ff0000\x07\
                     \x1b[6 q\
                     \x1b[>1u\x1b[>3u\x1b[=5;1u\
                     \x1b]2;first\x07\x1b[22t\x1b]2;second\x07\x1b[22t\x1b]2;now\x07\
                     \x1b[3g\x1b[5G\x1bH\x1b[13G\x1bH\
                     \x1b)0\x0e\x1b[2;3Habc\
                     \x1b[?2004h\x1b[?1004h\x1b[?1002h\x1b[?1006h\x1b[?1h\x1b=\x1b[4h\x1b[?7l\x1b[20h\x1b[?25l";
        let trip = live_trip(20, 6, bytes.as_bytes());
        let probed = &trip.probed;
        assert_eq!(probed.tabs, [4, 12]);
        assert_eq!(probed.charset, CharsetIndex::G1);
        assert_eq!(
            probed.keyboard,
            [
                vec![
                    KeyboardModes::DISAMBIGUATE_ESC_CODES,
                    KeyboardModes::from_bits_truncate(3)
                ],
                Vec::new()
            ]
        );
        assert_eq!(
            probed.titles,
            [Some("first".to_owned()), Some("second".to_owned())]
        );
        assert_eq!(probed.title.as_deref(), Some("now"));
        assert_eq!(trip.title.as_deref(), Some("now"));
        assert_eq!(
            probed.cursor_style,
            Some(CursorStyle {
                shape: CursorShape::Beam,
                blinking: false
            })
        );
    }

    #[test]
    fn a_default_cursor_style_and_default_tabs_write_nothing() {
        let (mut term, recorder) = live_term(20, 6);
        feed_into(&mut term, b"$ ");
        let (bytes, probed) = encode_of(&mut term, &recorder);
        assert_eq!(probed.cursor_style, None);
        assert_eq!(probed.tabs, [8, 16]);
        let text = String::from_utf8(bytes).unwrap();
        assert!(
            !text.contains(" q") && !text.contains("\x1b[3g"),
            "{text:?}"
        );
    }

    #[test]
    fn the_keyboard_stack_kept_off_the_alternate_screen_comes_back() {
        // alacritty keeps the alternate stack when the screen is left and
        // gives it back on the next entry.
        let bytes = b"\x1b[>1u\x1b[?1049h\x1b[>2u\x1b[>4u\x1b[?1049l";
        let trip = live_trip(20, 6, bytes);
        assert_eq!(
            trip.probed.keyboard,
            [
                vec![KeyboardModes::DISAMBIGUATE_ESC_CODES],
                vec![
                    KeyboardModes::REPORT_EVENT_TYPES,
                    KeyboardModes::REPORT_ALTERNATE_KEYS
                ]
            ]
        );
        // And on the alternate screen, its stack is the active one.
        let trip = live_trip(20, 6, b"\x1b[>1u\x1b[?1049h\x1b[>2u");
        assert_eq!(
            trip.probed.keyboard,
            [
                vec![KeyboardModes::DISAMBIGUATE_ESC_CODES],
                vec![KeyboardModes::REPORT_EVENT_TYPES]
            ]
        );
    }

    #[test]
    fn links_with_their_ids_clusters_wide_characters_and_wrapped_rows_come_back() {
        let bytes = format!(
            "\x1b]8;;bateri://block/3\x07$ ls\x1b]8;;\x07\r\n\
             \x1b]8;id=k;https://x.dev\x07x\x1b]8;;\x07 \x1b]8;;file:///tmp\x07t\x1b]8;;\x07\r\n\
             漢字 a👍🏽b 🇹🇷 ❤️ é\r\n{}\r\n\
             \x1b]8;;bateri://block/4\x07$ typ",
            "abcdefghij".repeat(3)
        );
        live_trip(10, 6, bytes.as_bytes());
    }

    #[test]
    fn the_line_drawing_set_and_its_cells_come_back() {
        live_trip(20, 6, b"\x1b(0lqqk\x1b(B ok\r\n\x1b(0x");
    }

    #[test]
    fn a_cursor_waiting_to_wrap_comes_back_waiting() {
        // The saved cursor at the last column, then the cursor on a wide
        // character's spacer.
        let bytes = "abcdefghij\x1b7\r\n漢字漢字漢";
        let (mut original, _) = live_term(10, 6);
        feed_into(&mut original, bytes.as_bytes());
        assert!(original.grid().cursor.input_needs_wrap);
        assert!(original.grid().saved_cursor.input_needs_wrap);
        live_trip(10, 6, bytes.as_bytes());
    }

    #[test]
    fn origin_mode_keeps_the_cursor_inside_the_region() {
        let trip = live_trip(20, 8, b"\x1b[3;6r\x1b[?6h\x1b[2;4Hz");
        assert_eq!(trip.probed.region, (2, 5));
    }

    #[test]
    fn the_053_kind_is_untouched_by_the_live_kind() {
        // The same scrollback, the saved kind: no `id`, no live link, no mode.
        let bytes = b"\x1b]8;id=k;https://x.dev\x07x\x1b]8;;\x07\r\n\x1b[?2004h";
        let (_, _, snap) = round_trip(20, 20, bytes);
        assert_eq!(snap, b"x\r\n");
    }

    // ─── the cut sequence ───────────────────────────────────────────────

    #[test]
    fn a_stream_cut_anywhere_comes_back_whole_with_its_tail() {
        let stream = "ab\x1b[1;31mred\x1b[0m 👍 é\x1b]2;title\x07\x1b[2;3Hz\x1bP1$qm\x1b\\\r\nnext"
            .as_bytes();
        let (mut whole, whole_recorder) = live_term(20, 6);
        feed_into(&mut whole, stream);
        let (expected, _) = encode_of(&mut whole, &whole_recorder);
        for cut in 0..=stream.len() {
            let (mut old, recorder) = live_term(20, 6);
            feed_into(&mut old, &stream[..cut]);
            let mut tail = Tail::default();
            // The reader's reads are any split of the bytes it got.
            tail.feed(&stream[..cut / 2]);
            tail.feed(&stream[cut / 2..cut]);
            let (snapshot, _) = encode_of(&mut old, &recorder);
            let (mut new, new_recorder) = live_term(20, 6);
            let mut bytes = snapshot;
            bytes.extend_from_slice(tail.bytes());
            bytes.extend_from_slice(&stream[cut..]);
            feed_into(&mut new, &bytes);
            let (got, _) = encode_of(&mut new, &new_recorder);
            assert_eq!(
                String::from_utf8_lossy(&got),
                String::from_utf8_lossy(&expected),
                "cut at {cut}, tail {:?}",
                String::from_utf8_lossy(tail.bytes())
            );
        }
    }

    #[test]
    fn the_tail_holds_only_what_has_not_taken_effect() {
        let tail_of = |bytes: &[u8]| {
            let mut tail = Tail::default();
            tail.feed(bytes);
            tail.bytes().to_vec()
        };
        // A line feed vte executed inside the CSI is not replayed.
        assert_eq!(tail_of(b"x\x1b[3\n1"), b"\x1b[31");
        // An ESC ends the OSC and starts a new sequence.
        assert_eq!(tail_of(b"\x1b]2;t\x1b"), b"\x1b");
        assert_eq!(tail_of("a\u{1F44D}".as_bytes()), b"");
        assert_eq!(tail_of(&"a\u{1F44D}".as_bytes()[..3]), &[0xf0, 0x9f]);
        assert_eq!(tail_of(b"\x1b[0m"), b"");
        assert_eq!(tail_of(b"\x1b]8;;x\x07"), b"");
        assert_eq!(tail_of(b"\x1b\x1b["), b"\x1b[");
        // `0x9c` ends a DCS passthrough, not an APC: there it is a UTF-8
        // continuation byte (`Ŝ`).
        assert_eq!(tail_of(b"\x1bPq\x9c"), b"");
        assert_eq!(tail_of("\x1b_Ŝ".as_bytes()), "\x1b_Ŝ".as_bytes());
        assert_eq!(tail_of(b"\x1bP1<q\x9c"), b"\x1bP1<q\x9c");
    }

    #[test]
    fn a_large_osc_does_not_pin_the_tails_capacity() {
        let mut tail = Tail::default();
        let mut osc = b"\x1b]52;c;".to_vec();
        osc.extend(std::iter::repeat_n(b'A', 100_000));
        tail.feed(&osc);
        assert!(tail.bytes().len() > 100_000);
        tail.feed(b"\x07");
        assert!(tail.bytes.capacity() <= TAIL_KEEP);
    }
}
