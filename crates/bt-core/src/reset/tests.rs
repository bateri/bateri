//! The reset against `RIS`: the same stream, written to two terminals, ends
//! the same in everything but the history and the titles.
//!
//! Each scenario is a **pane before the disconnect** (a long history, a pen,
//! a link, every mode on, a region, tab stops, the sets, both screens' saved
//! cursors and keyboard stacks, pointer shapes, DECSET 1016 and 2048, the
//! titles, the palette) that then takes `RIS` or the reset and, in some
//! tests, the picture of a server's screen. The picture writes the modes
//! itself, so the tests without it are the ones that reach every part of the
//! reset.

use super::*;
use crate::graphics::{Hub, Sink};
use crate::handler::{ClusterHandler, KeptModes};
use crate::pointer::{PointerShape, PointerShapes};
use crate::snapshot::{Probed, encode_live, tests::readable};
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::Processor;
use std::sync::{Arc, Mutex};

const COLUMNS: usize = 40;
const ROWS: usize = 10;
const RIS: &str = "\x1bc";

/// The session's side of a pane that the wrapper writes to.
struct Rig {
    hub: Hub,
    modes: KeptModes,
    pointer: PointerShapes,
}

impl Sink for Rig {
    fn hub(&self) -> &Hub {
        &self.hub
    }

    fn cell(&self) -> (u16, u16) {
        (10, 20)
    }

    fn reply(&self, _: Vec<u8>) {}

    fn kept_modes(&self) -> &KeptModes {
        &self.modes
    }

    fn pointer(&self) -> &PointerShapes {
        &self.pointer
    }

    fn pointer_changed(&self) {}
}

/// What the terminal's title is, as the events say it (`RIS` sends none).
#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Option<String>>>);

impl Recorder {
    fn title(&self) -> Option<String> {
        self.0.lock().expect("the title is plain data").clone()
    }
}

impl EventListener for Recorder {
    fn send_event(&self, event: Event) {
        match event {
            Event::Title(title) => *self.0.lock().expect("the title is plain data") = Some(title),
            Event::ResetTitle => *self.0.lock().expect("the title is plain data") = None,
            _ => {}
        }
    }
}

/// A terminal as a pane has it: the wrapper, the sink, the kitty protocol on.
struct Pane {
    term: Term<Recorder>,
    recorder: Recorder,
    parser: Processor,
    rig: Rig,
}

impl Pane {
    fn new(columns: usize) -> Self {
        let config = Config {
            scrolling_history: 1000,
            kitty_keyboard: true,
            ..Config::default()
        };
        let recorder = Recorder::default();
        Self {
            term: Term::new(config, &TermSize::new(columns, ROWS), recorder.clone()),
            recorder,
            parser: Processor::new(),
            rig: Rig {
                hub: Hub::new(),
                modes: KeptModes::default(),
                pointer: PointerShapes::default(),
            },
        }
    }

    fn feed(&mut self, bytes: impl AsRef<[u8]>) {
        let mut last_input = false;
        self.parser.advance(
            &mut ClusterHandler::new(&mut self.term, true, &mut last_input)
                .graphics(Some(&self.rig)),
            bytes.as_ref(),
        );
    }

    /// The same bytes in `size`-byte reads, as a PTY gives them.
    fn feed_in(&mut self, bytes: &[u8], size: usize) {
        for read in bytes.chunks(size) {
            self.feed(read);
        }
    }

    fn history(&self) -> usize {
        self.term.grid().history_size()
    }
}

/// A row's cells with their pens, to compare; a blank one with the default
/// pen is dropped from the end.
fn row(term: &Term<Recorder>, line: i32) -> String {
    let grid = term.grid();
    let cells: Vec<String> = (0..grid.columns())
        .map(|column| format!("{:?}", grid[Line(line)][Column(column)]))
        .collect();
    let blank = format!("{:?}", alacritty_terminal::term::cell::Cell::default());
    let end = cells
        .iter()
        .rposition(|cell| *cell != blank)
        .map_or(0, |at| at + 1);
    cells[..end].join("")
}

/// The characters of a row, trailing blanks off.
fn text(term: &Term<Recorder>, line: i32) -> String {
    let grid = term.grid();
    let text: String = (0..grid.columns())
        .map(|column| grid[Line(line)][Column(column)].c)
        .collect();
    text.trim_end().to_owned()
}

/// Every line from the oldest in the history to the last of the screen.
fn lines(term: &Term<Recorder>) -> Vec<String> {
    let grid = term.grid();
    (grid.topmost_line().0..=grid.bottommost_line().0)
        .map(|line| text(term, line))
        .collect()
}

/// What a terminal is, minus its history: the screen with its pens, the cursors,
/// the modes, the palette and the style (`readable`), the kept modes, the
/// shown pointer shape, and what only a probe reads (tab stops, region, sets,
/// both keyboard stacks, titles, the cursor style).
#[derive(Debug, PartialEq)]
struct Outcome {
    screen: Vec<String>,
    state: Vec<String>,
    kept: (bool, bool),
    pointer: Option<PointerShape>,
    /// The alternate screen entered and its saved cursor restored: what
    /// remains of it, which no getter shows.
    entered: Vec<String>,
    probed: Probed,
}

fn without_history(shown: String) -> Vec<String> {
    shown
        .lines()
        .filter(|line| !line.starts_with("history="))
        .map(str::to_owned)
        .collect()
}

fn outcome(pane: &mut Pane) -> Outcome {
    let screen = (0..ROWS as i32).map(|line| row(&pane.term, line)).collect();
    let state = without_history(readable(&pane.term));
    let kept = (
        pane.rig.modes.pixel_mouse(),
        pane.rig.modes.resize_reports(),
    );
    let pointer = pane.rig.pointer.shown();
    // Entering the alternate screen copies the cursor in and leaves its saved
    // cursor as it was; a `DECRC` there shows what it was.
    pane.feed("\x1b[?1049h\x1b8");
    let entered = without_history(readable(&pane.term));
    pane.feed("\x1b[?1049l");
    let recorder = pane.recorder.clone();
    let (_, probed) = encode_live(
        &mut pane.term,
        &move || recorder.title(),
        &|| (),
        &KeptModes::default(),
    );
    Outcome {
        screen,
        state,
        kept,
        pointer,
        entered,
        probed,
    }
}

/// A pane before the disconnect. `alternate`: a full-screen program has the
/// alternate screen.
fn before(columns: usize, alternate: bool) -> Pane {
    let mut pane = Pane::new(columns);
    let mut bytes: String = (0..30).map(|n| format!("line {n}\r\n")).collect();
    // The main screen's pen and sets, saved with the cursor.
    bytes.push_str("\x1b[1;31;44m\x1b(0\x1b)0\x0e\x1b[4;6H\x1b7");
    // The pointer shapes and keyboard modes of the main screen.
    bytes.push_str("\x1b]22;>pointer\x1b\\\x1b]22;>text\x1b\\\x1b[>1u\x1b[>1u");
    // A program that took the alternate screen and left it without cleaning
    // up: its saved cursor, keyboard stack and pointer shapes stay.
    bytes.push_str("\x1b[?1049h\x1b[2;3H\x1b[3m\x1b7\x1b[>1u\x1b]22;>grab\x1b\\alt\x1b[?1049l");
    bytes.push_str("\x1b]2;one\x1b\\\x1b[22t\x1b]2;two\x1b\\\x1b[22t\x1b]2;three\x1b\\");
    bytes.push_str("\x1b]4;1;rgb:aa/bb/cc\x07\x1b]10;rgb:11/22/33\x07\x1b[5 q");
    bytes.push_str("\x1b[?1016h\x1b[?2048h");
    // Tab stops other than every eighth column.
    bytes.push_str("\x1b[3g\x1b[5G\x1bH\x1b[20G\x1bH");
    // Every mode a program can turn on.
    bytes.push_str(
        "\x1b[?1h\x1b=\x1b[?7l\x1b[?25l\x1b[?1000h\x1b[?1003h\x1b[?1006h\x1b[?1004h\
         \x1b[?2004h\x1b[?1007l\x1b[?1042l\x1b[4h\x1b[20h",
    );
    // A region, origin mode, a link left open, and the last column written.
    bytes.push_str("\x1b[3;8r\x1b[?6h\x1b]8;id=a;https://x.dev\x07\x1b[2;1Hlinked");
    bytes.push_str(&format!("\x1b[5;{COLUMNS}Hx"));
    pane.feed(&bytes);
    if alternate {
        // A full-screen program: its own region, a status line, a pen, a
        // saved cursor.
        pane.feed("\x1b[?1049h\x1b[2;7r\x1b[H~\r\n~\x1b[8;1H\x1b[7mINSERT\x1b[0m\x1b[4;3H\x1b7");
        assert!(pane.term.mode().contains(TermMode::ALT_SCREEN));
    }
    pane
}

/// The bytes of a server's screen, as a client gets them: `build` runs on a
/// server's terminal of `columns` columns, which is then written out.
fn picture(columns: usize, build: &str) -> Vec<u8> {
    let mut server = Pane::new(columns);
    server.feed(build);
    let recorder = server.recorder.clone();
    encode_live(
        &mut server.term,
        &move || recorder.title(),
        &|| (),
        &KeptModes::default(),
    )
    .0
}

fn main_picture(columns: usize) -> Vec<u8> {
    let build: String = (0..25)
        .map(|n| format!("server {n}\r\n"))
        .collect::<String>()
        + "\x1b[1;32mgreen\x1b[0m $ \x1b[?2004h\x1b]2;server\x1b\\";
    picture(columns, &build)
}

fn alternate_picture(columns: usize) -> Vec<u8> {
    let build: String = (0..8)
        .map(|n| format!("server {n}\r\n"))
        .collect::<String>()
        + "\x1b[?1049h\x1b[2;7r\x1b[H~\r\n~\x1b[8;1H\x1b[7mVIM\x1b[0m\x1b[?1h\x1b[>1u";
    picture(columns, &build)
}

/// The titles `RIS` empties and the reset leaves, for the comparison.
fn the_deltas(mut ris: Outcome, mut new: Outcome) -> (Outcome, Outcome) {
    assert!(
        ris.probed.titles.is_empty(),
        "RIS empties the title stack: {:?}",
        ris.probed.titles
    );
    assert_eq!(
        new.probed.titles.len(),
        2,
        "the reset leaves the stack the local shell pushed to: {:?}",
        new.probed.titles
    );
    new.probed.titles.clear();
    ris.probed.titles.clear();
    (ris, new)
}

fn both(columns: usize, alternate: bool, after: &[u8]) -> (Pane, Pane) {
    let mut ris = before(columns, alternate);
    let mut new = before(columns, alternate);
    ris.feed(RIS);
    new.feed(keeping_history(columns as u16));
    ris.feed(after);
    new.feed(after);
    (ris, new)
}

#[test]
fn the_pane_before_is_as_dirty_as_the_tests_say() {
    // A guard for the others: a scenario that leaves nothing to reset proves
    // nothing about the reset.
    let mut pane = before(COLUMNS, false);
    assert!(pane.history() > 15);
    let dirty = outcome(&mut pane);
    assert_eq!(dirty.kept, (true, true));
    assert_eq!(dirty.pointer, Some(PointerShape::Text));
    assert_eq!(dirty.probed.keyboard[0].len(), 2);
    assert_eq!(dirty.probed.keyboard[1].len(), 1);
    assert_eq!(dirty.probed.titles.len(), 2);
    assert_eq!(dirty.probed.region, (2, 7));
    assert_eq!(dirty.probed.tabs, vec![4, 19]);
    assert!(dirty.probed.cursor_style.is_some());
    for needle in [
        "APP_CURSOR",
        "APP_KEYPAD",
        "ORIGIN",
        "DISAMBIGUATE_ESC_CODES",
        "MOUSE_MOTION",
        "SGR_MOUSE",
        "FOCUS_IN_OUT",
        "BRACKETED_PASTE",
        "INSERT",
        "LINE_FEED_NEW_LINE",
    ] {
        assert!(
            dirty.state[0].contains(needle),
            "{needle} is on: {}",
            dirty.state[0]
        );
    }
    let mut alternate = before(COLUMNS, true);
    assert!(alternate.term.mode().contains(TermMode::ALT_SCREEN));
    assert_eq!(outcome(&mut alternate).probed.region, (1, 6));
}

#[test]
fn the_reset_alone_leaves_the_terminal_as_ris_does_off_the_alternate_screen() {
    let (mut ris, mut new) = both(COLUMNS, false, b"");
    let (ris, new) = the_deltas(outcome(&mut ris), outcome(&mut new));
    assert_eq!(new, ris);
}

#[test]
fn the_reset_alone_leaves_the_terminal_as_ris_does_on_the_alternate_screen() {
    let (mut ris, mut new) = both(COLUMNS, true, b"");
    let (ris, new) = the_deltas(outcome(&mut ris), outcome(&mut new));
    assert_eq!(new, ris);
}

#[test]
fn a_picture_after_the_reset_is_a_picture_after_ris() {
    for (alternate, picture) in [
        (false, main_picture(COLUMNS)),
        (false, alternate_picture(COLUMNS)),
        (true, main_picture(COLUMNS)),
        (true, alternate_picture(COLUMNS)),
    ] {
        let (mut ris, mut new) = both(COLUMNS, alternate, &picture);
        let (ris, new) = the_deltas(outcome(&mut ris), outcome(&mut new));
        assert_eq!(new, ris, "alternate={alternate}");
    }
}

#[test]
fn every_way_the_mouse_is_reported_is_reset() {
    // The ways to report are exclusive, so one scenario holds one of each.
    for report in [1000, 1002, 1003] {
        for encoding in [1005, 1006] {
            let mut ris = Pane::new(COLUMNS);
            let mut new = Pane::new(COLUMNS);
            let on = format!("\x1b[?{report}h\x1b[?{encoding}h");
            ris.feed(&on);
            new.feed(&on);
            assert!(ris.term.mode().intersects(TermMode::MOUSE_MODE));
            ris.feed(RIS);
            new.feed(keeping_history(COLUMNS as u16));
            assert!(!new.term.mode().intersects(TermMode::MOUSE_MODE));
            assert_eq!(outcome(&mut new), outcome(&mut ris), "{report} {encoding}");
        }
    }
}

#[test]
fn the_reset_does_not_depend_on_how_the_reads_cut_it() {
    let reset = keeping_history(COLUMNS as u16);
    let mut cut = before(COLUMNS, false);
    cut.feed_in(&reset, 1);
    let mut one = before(COLUMNS, false);
    one.feed(&reset);
    assert_eq!(outcome(&mut cut), outcome(&mut one));
    assert_eq!(cut.history(), one.history());
}

#[test]
fn ris_forgets_the_history_and_the_reset_keeps_it_with_the_screen_in_it() {
    let pane = before(COLUMNS, false);
    let seen = lines(&pane.term);
    // What was there, up to the last row with anything in it.
    let seen = &seen[..seen.iter().rposition(|line| !line.is_empty()).unwrap() + 1];

    let (ris, new) = both(COLUMNS, false, b"");
    assert_eq!(ris.history(), 0, "RIS drops it");
    let kept: Vec<String> = lines(&new.term).into_iter().take(new.history()).collect();
    assert_eq!(kept, seen, "the screen follows the lines it was under");
    assert!(
        (0..ROWS as i32).all(|line| text(&new.term, line).is_empty()),
        "the screen is blank for the picture"
    );
}

#[test]
fn a_view_scrolled_up_stays_where_it_was_read() {
    let mut pane = before(COLUMNS, false);
    pane.term.scroll_display(Scroll::Delta(5));
    let read = pane.term.grid().display_offset();
    assert_eq!(read, 5);
    pane.feed(keeping_history(COLUMNS as u16));
    assert!(
        pane.term.grid().display_offset() >= read,
        "the lines it showed are still above the screen"
    );
}

#[test]
fn the_screen_of_a_full_screen_program_is_not_the_histories() {
    let pane = before(COLUMNS, false);
    let main = lines(&pane.term);
    let main = &main[..main.iter().rposition(|line| !line.is_empty()).unwrap() + 1];

    let (_, new) = both(COLUMNS, true, b"");
    let kept: Vec<String> = lines(&new.term).into_iter().take(new.history()).collect();
    // The main screen's rows are the history's; the program's `~` rows are
    // not in it.
    assert_eq!(kept, main);
    assert!(kept.iter().all(|line| !line.contains('~')));
}

#[test]
fn a_stack_as_deep_as_the_protocol_allows_is_emptied() {
    let (mut ris, mut new) = both(COLUMNS, false, b"");
    for pane in [&mut ris, &mut new] {
        // More pushes than the stack holds, on both screens.
        let mut deep = "\x1b[>1u".repeat(5000);
        deep.push_str("\x1b[?1049h");
        deep.push_str(&"\x1b[>1u".repeat(5000));
        deep.push_str("\x1b[?1049l");
        pane.feed(deep);
    }
    ris.feed(RIS);
    new.feed(keeping_history(COLUMNS as u16));
    let (ris, new) = (outcome(&mut ris), outcome(&mut new));
    assert!(new.probed.keyboard[0].is_empty() && new.probed.keyboard[1].is_empty());
    assert_eq!(new.probed.keyboard, ris.probed.keyboard);
}

#[test]
fn the_tab_stops_are_those_of_a_new_terminal_of_every_width() {
    for columns in [1, 7, 8, 9, 16, 17, 24, 40, 80, 81, 250] {
        let mut ris = Pane::new(columns);
        let mut new = Pane::new(columns);
        // Stops where a new terminal has none, and none where it has.
        let scramble = "\x1b[3g\x1b[3G\x1bH\x1b[7G\x1bH";
        ris.feed(scramble);
        new.feed(scramble);
        ris.feed(RIS);
        new.feed(keeping_history(columns as u16));
        let ris = outcome(&mut ris);
        let new = outcome(&mut new);
        assert_eq!(new.probed.tabs, ris.probed.tabs, "{columns} columns");
        assert_eq!(
            new.probed.last_tab, ris.probed.last_tab,
            "{columns} columns"
        );
    }
}

#[test]
fn the_palette_and_the_title_stack_are_left_alone() {
    // RIS does not reset the palette (alacritty's does not), and neither does
    // the reset: a pane's palette is as likely the local shell's theme as the
    // server's. The title stack is where the two differ on purpose: RIS
    // empties it, and the entry the local shell pushed when it ran the command
    // (to restore its title from when the command ends) is in it.
    let (mut ris, mut new) = both(COLUMNS, false, b"");
    for pane in [&mut ris, &mut new] {
        let colors = pane.term.colors()[1].expect("the palette entry the pane set is still set");
        assert_eq!((colors.r, colors.g, colors.b), (0xaa, 0xbb, 0xcc));
    }
    assert!(outcome(&mut ris).probed.titles.is_empty());
    assert_eq!(outcome(&mut new).probed.titles.len(), 2);
}

#[test]
fn a_pane_with_no_width_gets_the_reset_without_tab_stops() {
    let reset = String::from_utf8(keeping_history(0)).expect("the reset is text");
    assert!(!reset.contains("\x1b[3g"));
    assert!(reset.contains("\x1b[2J"));
}
