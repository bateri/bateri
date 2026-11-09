//! The wrapper between the parser and `Term`.
//!
//! The reader loop ([`crate::reader`]) hands `Term` to the parser not
//! directly but through this type: clustering has to step **in
//! between** `input`s and close the cluster on every other intervening call,
//! so it must see all of the parser's `Handler` calls. With clustering off
//! (`SessionOptions::cluster`) every call goes to `Term` as is — the
//! behavior is alacritty's, byte for byte. The one departure is the view:
//! dropping the wrapper keeps a scrolled-back view inside the history (the
//! `Drop` impl).
//!
//! **Clustering happens in `input` and only there.** If the incoming code
//! point does not extend the open cluster it goes to `Term::input`; if it
//! extends it and the cluster's column count does not change it lands in the
//! head cell's `zerowidth`; if it grows the cluster from one column to two
//! the head cell is rewritten through alacritty's **own** wide path (see
//! [`ClusterHandler::widen`]). The rule is in [`crate::cluster`]. A
//! zero-width code point that finds the head cell already holding
//! [`crate::cluster::MAX_CODE_POINTS`] is dropped, on the open and the
//! closed path alike, so no cell's cluster grows past the ceiling.
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
//! **Graphics commands end here.** The vendored parser hands the body of
//! `ESC _ … ESC \` over in slices ([`Handler::apc_start`], [`Handler::apc_put`],
//! [`Handler::apc_end`]); the stream's [`Commands`] (lent to this wrapper, which
//! is reborn on every `advance`) put a graphics command together, and where
//! it ends it takes effect, on the screen showing at that point of the stream:
//! the cursor moves down, the placement is anchored at the cursor, the cursor
//! moves right ([`Motion`]), and the program is answered. A command that
//! needs the expensive half (decoding an image, reading the file it names)
//! cannot have it under `Term`'s lock: the wrapper parks the record in the hub
//! and asks the parser to halt right behind the command
//! ([`Handler::halt_requested`]); the read loop lets go of the lock, decodes,
//! takes the lock back and calls [`ClusterHandler::resume`], which applies the
//! record. Without a [`Sink`] (a bare replay) the command only moves
//! the cursor, by the same numbers, and the parser never halts. A move to or
//! from the alternate screen drops what that screen's store holds; a clear of
//! the screen (`CSI 2 J`, `CSI 3 J`) and a reset delete the placements in
//! stream order, before the rows move.
//!
//! **DECSET 1016 and 2048 are kept here too** ([`KeptModes`]). alacritty
//! parses both modes as unknown and drops them, and `TermMode` has no bit for
//! either; the wrapper keeps the flags (the pointer reports read 1016, the
//! session's resize reads 2048) and answers DECRQM for them. A stream with no
//! modes to keep reads them and does nothing.
//!
//! **So is OSC 22, the pointer's shape** ([`crate::pointer`]): the fork hands
//! the request over as written, the shapes are the session's ([`Sink`]), one
//! stack per screen, so a move to or from the alternate screen and a reset
//! change the shape shown. A stream without a sink drops the request.
//!
//! **The forwarding list is in one macro and guarded.** Every `Handler`
//! method has an empty default; a method dropped from the list still
//! compiles but never reaches `Term`'s implementation, and the symptom is
//! silent (e.g. an escape sequence is ignored). The
//! `clippy::missing_trait_methods` above the `impl` turns that omission red
//! in `make clippy`; the same place catches a new method arriving in vte.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::Point;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::text_width;
use alacritty_terminal::term::{Term, TermMode};
use alacritty_terminal::vte::ansi::{
    Attr, CharsetIndex, ClearMode, CursorShape, CursorStyle, Handler, Hyperlink, KeyboardModes,
    KeyboardModesApplyBehavior, LineClearMode, Mode, ModifyOtherKeys, NamedPrivateMode,
    PrivateMode, Rgb, ScpCharPath, ScpUpdateMode, StandardCharset, TabulationClearMode,
};
// vte does not re-export this type; the reason for the edge is in the root
// `Cargo.toml`.
use cursor_icon::CursorIcon;
use unicode_width::UnicodeWidthChar;

use crate::graphics::{Commands, Env, Hub, Motion, Outcome, Pending, Sink, screen_of};
use crate::input::PIXEL_MOUSE_MODE;

/// DECSET 2048: the program wants the terminal's size in the stream, as
/// `CSI 48 ; rows ; cols ; height ; width t` ([`resize_report`]), whenever it
/// changes and once when the mode is set — the size and the pixels in one
/// sequence, in order with the output, where `SIGWINCH` leaves the program to
/// ask the kernel and races the redraw.
pub(crate) const RESIZE_REPORT_MODE: u16 = 2048;

/// The answer to DA1 (`CSI c`) and DECID (`ESC Z`): a VT220 (`62`) with ANSI
/// colour (`22`), as kitty and Ghostty answer.
pub(crate) const PRIMARY_DEVICE_ATTRIBUTES: &[u8] = b"\x1b[?62;22c";

/// The DEC private modes the stream's wrapper keeps because alacritty does
/// not know them: `Term` has no bit for either, so they live with the session
/// ([`Sink::kept_modes`]). Every stream that rebuilds a session's `Term` binds
/// a `KeptModes` of its own ([`ClusterHandler::kept_modes`]); one held
/// together, so a path cannot carry one mode across and drop the other.
#[derive(Debug, Default)]
pub(crate) struct KeptModes {
    /// DECSET 1016 ([`PIXEL_MOUSE_MODE`]): pointer reports in pixels.
    pixel_mouse: AtomicBool,
    /// DECSET 2048 ([`RESIZE_REPORT_MODE`]): the size in the stream.
    resize_reports: AtomicBool,
}

impl KeptModes {
    pub(crate) fn pixel_mouse(&self) -> bool {
        self.pixel_mouse.load(Ordering::Acquire)
    }

    pub(crate) fn resize_reports(&self) -> bool {
        self.resize_reports.load(Ordering::Acquire)
    }

    /// The mode's number if it is one of the kept ones, `None` for a mode
    /// `Term` knows (or drops).
    fn number(mode: PrivateMode) -> Option<u16> {
        match mode {
            PrivateMode::Unknown(number @ (PIXEL_MOUSE_MODE | RESIZE_REPORT_MODE)) => Some(number),
            _ => None,
        }
    }

    /// The flag of a kept mode's number ([`KeptModes::number`]).
    fn flag(&self, number: u16) -> &AtomicBool {
        if number == PIXEL_MOUSE_MODE {
            &self.pixel_mouse
        } else {
            &self.resize_reports
        }
    }

    /// RIS: both off, as on a fresh terminal.
    fn reset(&self) {
        self.pixel_mouse.store(false, Ordering::Release);
        self.resize_reports.store(false, Ordering::Release);
    }
}

/// DECSET 2048's report of a size: `rows` × `cols` cells of `cell` pixels —
/// the cell the PTY is told (`TIOCSWINSZ`'s pixel fields are cols and rows
/// times it, and so are `CSI 14 t`'s and `CSI 16 t`'s answers), so the
/// pixels the program is told are the pixels it is given.
pub(crate) fn resize_report(rows: u16, cols: u16, (cell_w, cell_h): (u16, u16)) -> String {
    let height = u32::from(rows) * u32::from(cell_h);
    let width = u32::from(cols) * u32::from(cell_w);
    format!("\x1b[48;{rows};{cols};{height};{width}t")
}

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
    /// The session's graphics ([`crate::graphics`]): the store the commands
    /// are applied to and where the program is answered. `None` for every
    /// stream that is not the live one (a bare replay): there a command only
    /// moves the cursor.
    graphics: Option<&'a dyn Sink>,
    /// The session's DECSET 1016 and 2048 ([`KeptModes`]) — where the modes
    /// land, since `Term` does not keep them. `None` for a stream with no
    /// modes to keep: they are read and do nothing, as the graphics commands
    /// do without a [`Sink`].
    modes: Option<&'a KeptModes>,
    /// The stream's graphics commands, between this wrapper's lives
    /// ([`ClusterHandler::commands`]). `None`: the parser's application
    /// program commands are let go.
    commands: Option<&'a mut Commands>,
    /// Where an open cluster's text is put together to be asked about: kept
    /// between the calls of one `advance`, so a stream of marks on narrow
    /// letters does not pay an allocation for every mark.
    scratch: String,
    /// Whether the open cluster's newest code point is known not to be a ZWJ — the one
    /// thing a code point with columns that is no flag half and no skin tone asks of the
    /// cluster behind it. Known from what this wrapper itself wrote in its own life, which
    /// is the only time nothing else touches the grid: it is false when the wrapper is
    /// born, where the grid may have been changed (a resize) since the last read, and then
    /// the cell is read. Of use only while `last_input` is set.
    plain: bool,
}

impl<'a, U: EventListener> ClusterHandler<'a, U> {
    pub(crate) fn new(term: &'a mut Term<U>, cluster: bool, last_input: &'a mut bool) -> Self {
        Self {
            term,
            cluster,
            last_input,
            graphics: None,
            modes: None,
            commands: None,
            scratch: String::new(),
            plain: false,
        }
    }

    /// Lets the wrapper apply the graphics commands the stream carries (it
    /// needs [`ClusterHandler::commands`] too). The session's kept modes come
    /// with the sink: the live stream keeps them and answers their queries.
    pub(crate) fn graphics(mut self, graphics: Option<&'a dyn Sink>) -> Self {
        self.graphics = graphics;
        self.modes = graphics.map(|sink| sink.kept_modes());
        self
    }

    /// Lets the wrapper keep DECSET 1016 and 2048 in `modes` for a stream
    /// that has no sink: the replays that rebuild a session's `Term` carry the
    /// modes across with it.
    pub(crate) fn kept_modes(mut self, modes: &'a KeptModes) -> Self {
        self.modes = Some(modes);
        self
    }

    fn on_alternate_screen(&self) -> bool {
        self.term.mode().contains(TermMode::ALT_SCREEN)
    }

    /// Gives the wrapper the stream's graphics commands to put a command
    /// together in. They are the owner's of the parser, not the wrapper's: it
    /// is made anew for every `advance`, and a command is cut by the reads.
    pub(crate) fn commands(mut self, commands: &'a mut Commands) -> Self {
        self.commands = Some(commands);
        self
    }

    /// The parser was halted behind a command ([`Handler::halt_requested`])
    /// and what it waited for is done: the record the hub parked takes effect
    /// here, where the command ended, and the parser may go on.
    pub(crate) fn resume(&mut self) {
        if let Some(commands) = self.commands.as_deref_mut() {
            commands.resume();
        }
        if let Some(graphics) = self.graphics
            && let Some(pending) = graphics.hub().take_parked()
        {
            self.place(pending);
        }
    }

    /// A message of the view protocol ended: the desk takes it, with whether
    /// the pane is on the alternate screen **here**, where the program wrote
    /// it — not where the frame path's atomic says, which is the last drawn
    /// frame's — and the parser stops right behind it, for the answer is not
    /// for the terminal's lock ([`ClusterHandler::resume`]).
    fn park_message(&mut self, message: ::pencere_host::frame::Message) {
        let Some(desk) = self.graphics.and_then(|graphics| graphics.desk()) else {
            return;
        };
        desk.park(message, self.on_alternate_screen());
        if let Some(commands) = self.commands.as_deref_mut() {
            commands.halt();
        }
    }

    /// What a command that ended comes to, in the order it came.
    fn dispatch(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Query(reply) => {
                if let Some(graphics) = self.graphics
                    && let Some(bytes) = reply.encode()
                {
                    graphics.reply(bytes);
                }
            }
            Outcome::Pending(pending) => match self.graphics {
                // The decoding is not for the terminal's lock: the record
                // waits and the parser stops behind the command.
                Some(graphics) if matches!(pending, Pending::Raw { .. }) => {
                    graphics.hub().park(pending);
                    if let Some(commands) = self.commands.as_deref_mut() {
                        commands.halt();
                    }
                }
                // With no store (a replay) it is the movement alone.
                _ => self.place(pending),
            },
        }
    }

    /// A record takes effect on the screen showing, between the two halves of
    /// the cursor's `motion` (down first, so that the cursor is on the
    /// image's last row where the placement is anchored; right after), and the
    /// program is answered.
    fn place(&mut self, pending: Pending) {
        let motion = pending.motion();
        self.move_down(motion);
        if let Some(graphics) = self.graphics {
            let (hub, cell) = (graphics.hub(), graphics.cell());
            // The placements already here are brought to the grid as it is
            // now before a new one is anchored in it.
            hub.look(&*self.term, cell);
            let screen = screen_of(&*self.term, cell);
            let reply = hub.apply(pending, &screen);
            hub.settle(&*self.term);
            if let Some(reply) = reply {
                graphics.reply(reply);
            }
        }
        self.move_across(motion);
    }

    /// The first half of a command's movement: down to the image's last row.
    /// No more than a screen's worth of rows, past that the screen is already
    /// all new.
    fn move_down(&mut self, motion: Motion) {
        *self.last_input = false;
        let screen_lines = u32::try_from(self.term.screen_lines()).unwrap_or(u32::MAX);
        for _ in 0..motion.rows.min(screen_lines) {
            self.term.linefeed();
        }
    }

    /// The second half: right of the image, or to the start of the next line
    /// for an image as wide as the screen.
    fn move_across(&mut self, motion: Motion) {
        if motion.wrap {
            self.term.linefeed();
            self.term.carriage_return();
        } else if motion.cols > 0 {
            self.term
                .move_forward(usize::try_from(motion.cols).unwrap_or(usize::MAX));
        }
    }

    /// Tells the hub whether a command is half-received.
    fn publish_open(&self) {
        if let (Some(graphics), Some(commands)) = (self.graphics, self.commands.as_deref()) {
            graphics.hub().set_open(commands.open());
        }
    }

    /// DECSET/DECRST of a kept mode: 1016 turns the pointer reports to pixels
    /// or back, 2048 turns the size reports on or off. Setting 2048 reports
    /// the size there and then, so the program has it without waiting for
    /// the window to move — through the sink, the parser's own replies'
    /// FIFO; a replay has no sink and tells nobody. `false`: not a kept mode.
    fn kept_mode(&mut self, mode: PrivateMode, on: bool) -> bool {
        let Some(number) = KeptModes::number(mode) else {
            return false;
        };
        *self.last_input = false;
        let Some(modes) = self.modes else {
            return true;
        };
        modes.flag(number).store(on, Ordering::Release);
        if on
            && number == RESIZE_REPORT_MODE
            && let Some(graphics) = self.graphics
        {
            let rows = u16::try_from(self.term.screen_lines()).unwrap_or(u16::MAX);
            let cols = u16::try_from(self.term.columns()).unwrap_or(u16::MAX);
            graphics.reply(resize_report(rows, cols, graphics.cell()).into_bytes());
        }
        true
    }

    /// DECRQM for a kept mode, answered the way alacritty answers the modes
    /// it knows (`CSI ? Ps ; Pm $ y`: 1 set, 2 reset) — through the sink, so
    /// it takes the same FIFO as the parser's own replies. A stream without a
    /// sink is a replay and replies to nobody. `false`: not a kept mode.
    fn report_kept_mode(&mut self, mode: PrivateMode) -> bool {
        let Some(number) = KeptModes::number(mode) else {
            return false;
        };
        *self.last_input = false;
        if let (Some(graphics), Some(modes)) = (self.graphics, self.modes) {
            let state = if modes.flag(number).load(Ordering::Acquire) {
                1
            } else {
                2
            };
            graphics.reply(format!("\x1b[?{number};{state}$y").into_bytes());
        }
        true
    }

    /// `CSI 2 J` deletes the placements that reach onto the screen, `CSI 3 J`
    /// all of them; the rows are cleared right after, so the lines the
    /// placements were anchored to are still the grid's.
    fn clearing_screen(&mut self, mode: ClearMode) {
        *self.last_input = false;
        if let Some(graphics) = self.graphics
            && matches!(mode, ClearMode::All | ClearMode::Saved)
        {
            let (hub, cell) = (graphics.hub(), graphics.cell());
            hub.look(&*self.term, cell);
            let screen = screen_of(&*self.term, cell);
            hub.clear_screen(&screen, matches!(mode, ClearMode::Saved));
        }
        self.term.clear_screen(mode);
    }

    /// Runs a private mode change and drops the alternate screen's images
    /// when it moved to or from that screen.
    fn switching_screens(&mut self, change: impl FnOnce(&mut Term<U>)) {
        *self.last_input = false;
        let before = self.on_alternate_screen();
        change(self.term);
        if let Some(graphics) = self.graphics
            && self.on_alternate_screen() != before
        {
            graphics.hub().drop_alt();
            if graphics.pointer().show(!before) {
                graphics.pointer_changed();
            }
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

    /// Does the head cell's cluster hold [`crate::cluster::MAX_CODE_POINTS`]
    /// already. Read from the length of `zerowidth`, not from the cluster's
    /// text: the answer costs the same for a cell of two code points as for
    /// one that alacritty's own path filled before clustering was asked.
    fn is_full(&self, at: Point) -> bool {
        let cell = &self.term.grid()[at.line][at.column];
        1 + cell.zerowidth().map_or(0, <[char]>::len) >= crate::cluster::MAX_CODE_POINTS
    }

    /// The head cell's cluster's newest code point: the last of `zerowidth`,
    /// or the base character.
    fn newest(&self, at: Point) -> char {
        let cell = &self.term.grid()[at.line][at.column];
        cell.zerowidth()
            .and_then(<[char]>::last)
            .copied()
            .unwrap_or(cell.c)
    }

    /// The head cell's cluster: base character + `zerowidth`, put together in
    /// the wrapper's own buffer (taken out of it; [`ClusterHandler::keep`]
    /// puts it back).
    fn open(&mut self, at: Point) -> String {
        let mut open = std::mem::take(&mut self.scratch);
        open.clear();
        let cell = &self.term.grid()[at.line][at.column];
        open.push(cell.c);
        open.extend(cell.zerowidth().unwrap_or_default());
        open
    }

    /// Gives the buffer [`ClusterHandler::open`] took back.
    fn keep(&mut self, open: String) {
        self.scratch = open;
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

    /// [`Handler::input`] past what needs no cluster: the code points that can
    /// take part in one. Apart from the call because it is large and the
    /// common code point does not come here: the plain one is inlined into the
    /// parser's loop.
    #[inline(never)]
    fn input_extending(&mut self, c: char, open_cluster: bool) {
        // The one question of the table that every arm below is answered from.
        let width = UnicodeWidthChar::width(c);
        let mark = width == Some(0);
        // A code point with columns extends a cluster only behind an open one,
        // and most of running text — CJK, accented letters — has none to look
        // at: it goes in without the head cell being read. So does one behind a
        // cluster known to end in anything but a ZWJ, unless it is a flag half
        // or a skin tone, which need no ZWJ.
        if !mark && (!open_cluster || (self.plain && !crate::cluster::joins_alone(c))) {
            return self.input_written(c, width);
        }
        let at = self.head();
        if mark {
            // A zero-width code point lands on the head cell whether the cluster
            // is open or not — alacritty's own zero-width branch picks the same
            // cell —, so the ceiling is asked before either path: past it the code
            // point is dropped. Asked of the cell's length, before `open` is
            // built, so a long run costs the same per mark however long it is.
            if self.is_full(at) {
                return;
            }
            if !open_cluster {
                return self.input_mark(c);
            }
            // A mark behind a head with columns extends it, whatever the rest of
            // the cluster is ([`crate::cluster::extends`]'s first rule and its
            // zero-width arm): a wide cell takes it without the cluster being put
            // together to measure.
            let cell = &self.term.grid()[at.line][at.column];
            if !crate::cluster::has_columns(cell.c) {
                return self.input_mark(c);
            }
            if cell.flags.contains(Flags::WIDE_CHAR) {
                self.term.grid_mut()[at.line][at.column].push_zerowidth(c);
                self.plain = c != crate::cluster::ZWJ;
                return;
            }
        } else {
            // Most of what reaches here — a CJK character behind another — is
            // turned back by the cluster's newest code point, before the cluster
            // is copied out of the cell to be asked.
            if !crate::cluster::may_follow(self.newest(at), c, width) {
                return self.input_written(c, width);
            }
        }
        let mut open = self.open(at);
        if !mark && !crate::cluster::extends(&open, c) {
            self.keep(open);
            return self.input_written(c, width);
        }
        let narrow = !self.term.grid()[at.line][at.column]
            .flags
            .contains(Flags::WIDE_CHAR);
        open.push(c);
        // Width is asked after **every** extension, including those from the
        // zero-width arm: `1` + VS16 + `U+20E3` widens at the VS16. Only the
        // newest prefix is measured ([`crate::cluster::widens`]): the shorter
        // ones were measured as they arrived, and a narrow cell says none of
        // them reached two columns.
        if narrow && crate::cluster::widens(&open) {
            self.widen(at, &open);
        } else {
            self.term.grid_mut()[at.line][at.column].push_zerowidth(c);
        }
        // The code point is the cluster's newest, either way it came in.
        self.plain = c != crate::cluster::ZWJ;
        self.keep(open);
    }

    /// `Term::input` of a code point that is not a mark, and what that leaves of
    /// [`ClusterHandler::plain`]: the code point is the newest of the cell it starts
    /// — if the grid writes it. A code point with no width is not written, and a wide
    /// one that finds no room for both columns with wrapping off is not either.
    fn input_written(&mut self, c: char, width: Option<usize>) {
        let written = match width {
            Some(1) => true,
            Some(0) | None => false,
            Some(_) => {
                self.term.mode().contains(TermMode::LINE_WRAP)
                    || self.term.grid().cursor.point.column.0 + 1 < self.term.columns()
            }
        };
        self.term.input(c);
        self.plain = written;
    }

    /// Code points that join nothing, written in one piece. With wrapping off a wide one may find
    /// no room and not be written, so what stands last is not known then.
    fn input_unjoined(&mut self, run: &str) {
        if run.is_empty() {
            return;
        }
        *self.last_input = true;
        self.plain = self.term.mode().contains(TermMode::LINE_WRAP);
        self.term.input_text(run);
    }

    /// `Term::input` of a mark that lands on the cell before the cursor: it is that
    /// cell's newest code point.
    fn input_mark(&mut self, c: char) {
        self.term.input(c);
        self.plain = c != crate::cluster::ZWJ;
    }
}

/// The view goes back inside the history once the parser is done with
/// `Term`. alacritty 0.26.0's `Grid::scroll_up` moves a scrolled-back view up
/// by every line a scrolling region scrolls, also when the region does not
/// start at the top and no line enters the history (a line feed at the
/// bottom of a region below a fixed row — apt's progress bar —, `CSI M`): the
/// view ends past the history and the next frame indexes outside the grid.
/// The clamp moves only the view, which the snapshot does not read; the view
/// lands on the history's top, where alacritty's arithmetic was heading anyway.
impl<U: EventListener> Drop for ClusterHandler<'_, U> {
    fn drop(&mut self) {
        let grid = self.term.grid();
        if grid.display_offset() > grid.history_size() {
            self.term.scroll_display(Scroll::Delta(0));
        }
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
    #[inline]
    fn input(&mut self, c: char) {
        if !self.cluster {
            return self.term.input(c);
        }
        let open_cluster = std::mem::replace(self.last_input, true);
        // In ASCII only digits, `#` and `*` enter an arm.
        if c.is_ascii() && !crate::cluster::may_extend(c) {
            // Printable, so written, and not a ZWJ.
            self.plain = matches!(c, ' '..='~');
            return self.term.input(c);
        }
        self.input_extending(c, open_cluster);
    }

    /// A run of printable ASCII. Only its first byte has anything before it that it could join:
    /// a digit, `#` or `*` behind a ZWJ. Every later byte stands behind an ASCII character, which
    /// no ASCII character extends, so the run past the first goes to the grid in one piece.
    fn input_ascii(&mut self, s: &str) {
        if !self.cluster {
            return self.term.input_ascii(s);
        }
        let open_cluster = std::mem::replace(self.last_input, true);
        match s.chars().next() {
            Some(first) if open_cluster && crate::cluster::may_extend(first) => {
                self.input_extending(first, true);
                let rest = s.get(1..).unwrap_or_default();
                self.plain |= !rest.is_empty();
                self.term.input_ascii(rest);
            }
            _ => {
                self.plain = !s.is_empty();
                self.term.input_ascii(s);
            }
        }
    }

    /// A run of text with something outside ASCII in it. Behind a code point this wrapper wrote,
    /// and that is not a ZWJ, a code point with columns joins nothing unless it is a flag half or
    /// a skin tone ([`crate::cluster::joins_alone`]), so most of running text (CJK, accented
    /// letters) asks nothing of the cell and goes to the grid in one piece. The rest, and the
    /// first code point when what stands before it is not known, take [`Handler::input`].
    fn input_text(&mut self, s: &str) {
        if !self.cluster {
            return self.term.input_text(s);
        }
        let mut from = 0;
        for (at, c) in s.char_indices() {
            let known = from < at || !*self.last_input || self.plain;
            let columns = !matches!(text_width(c), Some(0) | None);
            if !known || !columns || crate::cluster::joins_alone(c) {
                self.input_unjoined(s.get(from..at).unwrap_or_default());
                self.input(c);
                from = at + c.len_utf8();
            }
        }
        self.input_unjoined(s.get(from..).unwrap_or_default());
    }

    fn set_private_mode(&mut self, mode: PrivateMode) {
        if self.kept_mode(mode, true) {
            return;
        }
        // The command that took the screen is on the screen being left until this very call:
        // its text is read now, or it is out of reach for as long as the program runs.
        if mode == PrivateMode::Named(NamedPrivateMode::SwapScreenAndSetRestoreCursor)
            && !self.on_alternate_screen()
            && let Some(graphics) = self.graphics
        {
            graphics.leaving_primary_screen(crate::history::newest_block(&*self.term));
        }
        self.switching_screens(|term| term.set_private_mode(mode));
    }

    fn unset_private_mode(&mut self, mode: PrivateMode) {
        if self.kept_mode(mode, false) {
            return;
        }
        self.switching_screens(|term| term.unset_private_mode(mode));
    }

    fn report_private_mode(&mut self, mode: PrivateMode) {
        if self.report_kept_mode(mode) {
            return;
        }
        *self.last_input = false;
        self.term.report_private_mode(mode);
    }

    /// DA1 (and DECID) answered as a VT220 with ANSI colour, the way kitty
    /// and Ghostty answer, not as alacritty's VT102 `CSI ? 6 c`: kitty's
    /// `kitten icat` ends its graphics probe only on an answer longer than
    /// `?6c` and otherwise waits out its timeout. Through the sink, so it
    /// keeps its place behind a graphics command's answer; a stream without a
    /// sink replies to nobody and is left to `Term`, like DA2.
    fn identify_terminal(&mut self, intermediate: Option<char>) {
        *self.last_input = false;
        match (intermediate, self.graphics) {
            (None, Some(graphics)) => graphics.reply(PRIMARY_DEVICE_ATTRIBUTES.to_vec()),
            _ => self.term.identify_terminal(intermediate),
        }
    }

    fn clear_screen(&mut self, mode: ClearMode) {
        self.clearing_screen(mode);
    }

    fn reset_state(&mut self) {
        *self.last_input = false;
        if let Some(graphics) = self.graphics {
            graphics.hub().reset();
        }
        if let Some(modes) = self.modes {
            modes.reset();
        }
        if let Some(graphics) = self.graphics
            && graphics.pointer().clear()
        {
            graphics.pointer_changed();
        }
        self.term.reset_state();
    }

    /// The kitty keyboard protocol's `CSI > flags u`: every flag the program
    /// pushes is kept, because every one of them is encoded for
    /// ([`crate::input`]), and `CSI ? u` says what is kept.
    ///
    /// Typing (`last_input`) is over for the shell: a program that pushes the
    /// protocol reads keys itself from here on.
    fn push_keyboard_mode(&mut self, mode: KeyboardModes) {
        *self.last_input = false;
        self.term.push_keyboard_mode(mode);
    }

    /// `CSI = flags ; how u`. The library applies it to the active flags but
    /// not to the stack entry `CSI ? u` reads and a pop restores, so the entry
    /// is replaced by what is active: the answer to a query is what keys are
    /// encoded for, whichever way the flags were asked for.
    fn set_keyboard_mode(&mut self, mode: KeyboardModes, behavior: KeyboardModesApplyBehavior) {
        *self.last_input = false;
        self.term.set_keyboard_mode(mode, behavior);
        let active = keyboard_flags(*self.term.mode());
        self.term.pop_keyboard_modes(1);
        self.term.push_keyboard_mode(active);
    }

    /// OSC 22 ([`crate::pointer`]): the request goes to the current screen's
    /// stack, a query is answered the way the terminal's other answers go and
    /// the shell hears of a new shape. Without a sink (a replay) it is read
    /// and dropped.
    fn set_pointer_shape(&mut self, request: &[u8], terminator: &str) {
        *self.last_input = false;
        let Some(graphics) = self.graphics else {
            return;
        };
        let (reply, changed) =
            graphics
                .pointer()
                .request(self.on_alternate_screen(), request, terminator);
        if let Some(reply) = reply {
            graphics.reply(reply);
        }
        if changed {
            graphics.pointer_changed();
        }
    }

    fn apc_start(&mut self) {
        // Like every other sequence, it closes the open cluster.
        *self.last_input = false;
        let seen = self
            .graphics
            .is_some_and(|graphics| graphics.hub().remote_seen());
        let hears_frames = self
            .graphics
            .is_some_and(|graphics| graphics.desk().is_some());
        if let Some(commands) = self.commands.as_deref_mut() {
            commands.start(seen);
            if hears_frames {
                commands.start_frames();
            }
        }
    }

    fn apc_put(&mut self, data: &[u8]) {
        if let Some(commands) = self.commands.as_deref_mut() {
            commands.put(data);
            self.publish_open();
        }
    }

    fn apc_end(&mut self, cancelled: bool) {
        *self.last_input = false;
        let hub = self.graphics.map(|graphics| graphics.hub());
        let cell = self
            .graphics
            .map(|graphics| graphics.cell())
            .or_else(|| self.commands.as_deref().map(Commands::cell))
            .unwrap_or_default();
        let env = Env {
            cell,
            columns: u16::try_from(self.term.columns()).unwrap_or(u16::MAX),
            lines: u16::try_from(self.term.screen_lines()).unwrap_or(u16::MAX),
        };
        let Some(commands) = self.commands.as_deref_mut() else {
            return;
        };
        let stored = |echo| hub.and_then(|hub| hub.stored_size(echo));
        let remote = || hub.is_some_and(Hub::in_remote_session);
        let seen = hub.is_some_and(Hub::remote_seen);
        let outcomes = commands.end(cancelled, Instant::now(), env, &stored, (seen, &remote));
        let message = commands.take_message();
        self.publish_open();
        for outcome in outcomes {
            self.dispatch(outcome);
        }
        if let Some(message) = message {
            self.park_message(message);
        }
    }

    /// Whether the parser is to stop right behind the command it just ended,
    /// for the decoding that cannot be done under `Term`'s lock
    /// ([`ClusterHandler::resume`]).
    #[inline(always)]
    fn halt_requested(&self) -> bool {
        self.commands.as_deref().is_some_and(Commands::halted)
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
        fn clear_tabs(&mut self, mode: TabulationClearMode);
        fn set_tabs(&mut self, interval: u16);
        fn reverse_index(&mut self);
        fn terminal_attribute(&mut self, attr: Attr);
        fn set_mode(&mut self, mode: Mode);
        fn unset_mode(&mut self, mode: Mode);
        fn report_mode(&mut self, mode: Mode);
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
        fn pop_keyboard_modes(&mut self, to_pop: u16);
        fn set_modify_other_keys(&mut self, mode: ModifyOtherKeys);
        fn report_modify_other_keys(&mut self);
        fn set_scp(&mut self, char_path: ScpCharPath, update_mode: ScpUpdateMode);
    }
}

/// The kitty keyboard flags a terminal mode holds: what keys are encoded for.
fn keyboard_flags(mode: TermMode) -> KeyboardModes {
    [
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
    ]
    .into_iter()
    .filter(|(bit, _)| mode.contains(*bit))
    .fold(KeyboardModes::NO_MODE, |all, (_, flag)| all | flag)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::MAX_CODE_POINTS;
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

    /// `input` as it was when every question of the table was asked afresh at
    /// every step: the rule of [`crate::cluster`] applied to the cluster put
    /// together from the cell. What the quicker `input` is held to.
    fn input_by_asking_everything(handler: &mut ClusterHandler<'_, VoidListener>, c: char) {
        let open_cluster = std::mem::replace(handler.last_input, true);
        if !crate::cluster::may_extend(c) {
            return handler.term.input(c);
        }
        let at = handler.head();
        if crate::dock::column_width(c) == 0 && handler.is_full(at) {
            return;
        }
        if !open_cluster {
            return handler.term.input(c);
        }
        if !crate::cluster::may_follow(handler.newest(at), c, UnicodeWidthChar::width(c)) {
            return handler.term.input(c);
        }
        let mut open = handler.open(at);
        if !crate::cluster::extends(&open, c) {
            return handler.term.input(c);
        }
        let narrow = !handler.term.grid()[at.line][at.column]
            .flags
            .contains(Flags::WIDE_CHAR);
        open.push(c);
        if narrow && crate::cluster::widens(&open) {
            handler.widen(at, &open);
        } else {
            handler.term.grid_mut()[at.line][at.column].push_zerowidth(c);
        }
    }

    /// The quick `input` leaves the grid, the cursor and the cluster left open
    /// as the one that asks everything at every step does, on streams made of
    /// the code points that move a cluster — marks, joiners, selectors, flag
    /// halves, skin tones, wide and narrow letters, digits — with the calls
    /// that close one between them, on a grid narrow enough to wrap.
    #[test]
    fn the_quick_input_leaves_what_the_one_that_asks_everything_leaves() {
        let alphabet: Vec<char> = [
            "a",
            "e",
            "1",
            "#",
            "*",
            " ",
            "é",
            "ß",
            "旦",
            "海",
            "😀",
            "👍",
            "👨",
            "👩",
            "🔥",
            "❤",
            "☺",
            "©",
            "🏳",
            "🌈",
            "🇹",
            "🇷",
            "🏽",
            "\u{200D}",
            "\u{FE0F}",
            "\u{FE0E}",
            "\u{20E3}",
            "\u{301}",
            "\u{302}",
            "\u{300}",
            "\u{E0067}",
            "\u{0}",
            "\u{85}",
            "ل",
            "ا",
            "\u{3000}",
        ]
        .iter()
        .flat_map(|text| text.chars())
        .collect();
        let dump = |term: &Term<VoidListener>, open: bool| {
            let cursor = &term.grid().cursor;
            (rows(term), cursor.point, cursor.input_needs_wrap, open)
        };
        let mut seed: u64 = 0x1234_5678_9abc_def1;
        let mut next = |bound: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            usize::try_from(seed % bound as u64).unwrap_or(0)
        };
        for round in 0..1500 {
            let (mut quick, mut slow) = (term(7, 4), term(7, 4));
            let (mut quick_open, mut slow_open) = (false, false);
            // Some rounds with a cluster that runs to the ceiling.
            let run = if round % 8 == 0 { 120 } else { 40 };
            // The quicker wrapper is reborn between stretches, as the parser's is on every
            // `advance`, with the cluster left open and what it knew of it gone.
            for _ in 0..run / 8 {
                let mut quick_handler = ClusterHandler::new(&mut quick, true, &mut quick_open);
                let mut slow_handler = ClusterHandler::new(&mut slow, true, &mut slow_open);
                for _ in 0..8 {
                    match next(27) {
                        0 => {
                            quick_handler.carriage_return();
                            slow_handler.carriage_return();
                        }
                        1 => {
                            quick_handler.linefeed();
                            slow_handler.linefeed();
                        }
                        2 => {
                            quick_handler.backspace();
                            slow_handler.backspace();
                        }
                        // Wrapping off, where a wide code point at the last column is not written.
                        3 => {
                            let wrap = PrivateMode::Named(NamedPrivateMode::LineWrap);
                            if next(2) == 0 {
                                quick_handler.unset_private_mode(wrap);
                                slow_handler.unset_private_mode(wrap);
                            } else {
                                quick_handler.set_private_mode(wrap);
                                slow_handler.set_private_mode(wrap);
                            }
                        }
                        // A ZWJ on the last column's neighbour with wrapping off, and then a wide
                        // code point the grid has no room to write.
                        4 => {
                            let wrap = PrivateMode::Named(NamedPrivateMode::LineWrap);
                            quick_handler.unset_private_mode(wrap);
                            slow_handler.unset_private_mode(wrap);
                            quick_handler.carriage_return();
                            slow_handler.carriage_return();
                            for c in "abcdef\u{200D}旦".chars() {
                                quick_handler.input(c);
                                input_by_asking_everything(&mut slow_handler, c);
                            }
                        }
                        pick => {
                            // A run of the same mark is how a cluster is filled to its ceiling.
                            let c = alphabet[next(alphabet.len())];
                            let times = if pick == 5 { 1 + next(40) } else { 1 };
                            for _ in 0..times {
                                quick_handler.input(c);
                                input_by_asking_everything(&mut slow_handler, c);
                            }
                        }
                    }
                }
            }
            assert_eq!(
                dump(&quick, quick_open),
                dump(&slow, slow_open),
                "round {round}"
            );
        }
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

    /// A scrolled-back view stays inside the history when a scrolling region
    /// that does not start at the top scrolls (a line feed at its bottom, a
    /// `CSI M` inside it): no line goes into the history there, yet
    /// alacritty 0.26.0 moves the view up by every scrolled line, and the
    /// next frame would index past the grid's top.
    #[test]
    fn a_region_scroll_keeps_a_scrolled_back_view_inside_the_history() {
        for cluster in [false, true] {
            for scroll in [
                "\x1b[2;5r\x1b[5H\n\n\n\n\n\n\n\x1b[r",
                "\x1b[2;5r\x1b[3H\x1b[3M\x1b[r",
            ] {
                let mut t = term(10, 5);
                feed(&mut t, cluster, &"x\r\n".repeat(8));
                assert_eq!(t.grid().history_size(), 4);
                t.scroll_display(Scroll::Delta(3));
                feed(&mut t, cluster, scroll);
                let grid = t.grid();
                assert!(
                    grid.display_offset() <= grid.history_size(),
                    "cluster={cluster} {scroll:?}: the view {} is past the history {}",
                    grid.display_offset(),
                    grid.history_size(),
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

    /// `bytes` fed through one parser in the slices `cuts` marks — the way
    /// the reader gets a stream, a read at a time.
    fn feed_cut(term: &mut Term<VoidListener>, cluster: bool, bytes: &[u8], cuts: &[usize]) {
        let mut parser: Processor = Processor::new();
        let mut last_input = false;
        let mut at = 0;
        for &cut in cuts.iter().chain(std::iter::once(&bytes.len())) {
            parser.advance(
                &mut ClusterHandler::new(term, cluster, &mut last_input),
                &bytes[at..cut],
            );
            at = cut;
        }
    }

    /// A read that ends inside `·` (`C2` | `B7 20 C2 B7`) keeps the space
    /// after it. The parser stock `vte` drops it; the fork (kept in kablo's
    /// repository) keeps it and has the sweep of its own — this is the witness
    /// on this side, red the day the patch stops applying.
    #[test]
    fn a_character_cut_by_a_read_keeps_the_byte_after_it() {
        let mut t = term(10, 1);
        feed_cut(&mut t, true, "· ·".as_bytes(), &[1]);
        assert_eq!(rows(&t)[0], "·| |·");
    }

    /// An application program command is a sequence like any other: between
    /// the two halves of an emoji it closes the cluster, as a cursor movement
    /// does, whether or not anything is collecting commands.
    #[test]
    fn an_application_program_command_between_two_code_points_closes_the_cluster() {
        let mut joined = term(10, 1);
        feed(&mut joined, true, "👍🏽");
        assert_eq!(rows(&joined)[0], "[👍🏽]|·");
        let mut cut = term(10, 1);
        feed(&mut cut, true, "👍\x1b_Xnot ours\x1b\\🏽");
        assert_eq!(rows(&cut)[0], "[👍]|·|[🏽]|·");
    }

    /// A run of combining marks behind a narrow letter — the input that used
    /// to cost the cube of its length under `Term`'s lock — goes through in
    /// time linear in the run, and the cell keeps [`MAX_CODE_POINTS`] code
    /// points. The parse runs on a thread of its own and is waited for with a
    /// deadline, so a regression fails here instead of hanging the suite.
    #[test]
    fn a_long_run_of_combining_marks_is_cut_at_the_ceiling_and_stays_fast() {
        let (done, finished) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut t = term(10, 2);
            feed(&mut t, true, &format!("a{}b", "\u{301}".repeat(50_000)));
            let _ = done.send((rows(&t), t.grid().cursor.point.column));
        });
        let (rows, cursor) = finished
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("50 000 marks took more than two seconds");
        let kept = format!("a{}", "\u{301}".repeat(MAX_CODE_POINTS - 1));
        assert_eq!(rows[0], format!("{kept}|b"));
        assert_eq!(cursor, Column(2));
    }

    /// The ceiling holds when every mark arrives with the cluster closed (an
    /// SGR in front of each): alacritty's own zero-width branch would push
    /// them all onto the same cell, and the next mark through the wrapper
    /// would rebuild them all.
    #[test]
    fn marks_behind_a_closed_cluster_stop_at_the_ceiling_too() {
        let mut t = term(10, 2);
        feed(&mut t, true, &format!("a{}", "\x1b[m\u{301}".repeat(100)));
        let kept = format!("a{}", "\u{301}".repeat(MAX_CODE_POINTS - 1));
        assert_eq!(rows(&t)[0], kept);
        // With clustering off the bytes are alacritty's, ceiling or not.
        let mut off = term(10, 2);
        feed(
            &mut off,
            false,
            &format!("a{}", "\x1b[m\u{301}".repeat(100)),
        );
        assert_eq!(rows(&off)[0], format!("a{}", "\u{301}".repeat(100)));
    }

    /// Past the ceiling the grid takes the columns the cluster walk says —
    /// the dock's and the freshness gate's reading of the same text: a
    /// dropped mark takes none, an emoji behind a full ZWJ run starts a wide
    /// cell of its own, and a mark behind a wide head that has room still
    /// joins it.
    #[test]
    fn past_the_ceiling_the_grid_keeps_the_walks_columns() {
        let marks = "\u{301}".repeat(MAX_CODE_POINTS + 8);
        let joiners = "\u{200D}".repeat(MAX_CODE_POINTS + 3);
        for text in [
            format!("a{marks}b"),
            format!("👨{joiners}👩x"),
            format!("x👍🏽{marks}y"),
            format!("1\u{FE0F}{marks}\u{20E3}z"),
        ] {
            let mut walked = 0;
            crate::cluster::Walk::new().run(text.chars(), |cluster| walked += cluster.width);
            let mut t = term(80, 2);
            feed(&mut t, true, &text);
            assert_eq!(t.grid().cursor.point.column, Column(walked), "{text:?}");
            let longest = rows(&t)[0]
                .split('|')
                .map(|cell| cell.trim_matches(['[', ']']).chars().count())
                .max();
            assert_eq!(longest, Some(MAX_CODE_POINTS), "{text:?}");
        }
    }

    /// Wherever one read or two cut the stream — inside a two-, three- or
    /// four-byte character, an escape sequence or an emoji cluster — the
    /// cells are the ones a single read gives.
    #[test]
    fn every_cut_gives_the_cells_of_one_read() {
        let text = "a·b ğ│é \x1b[31mx🙂y\x1b[m ş─z 👍🏽 ü!\u{301}";
        let bytes = text.as_bytes();
        for cluster in [false, true] {
            let mut whole = term(40, 2);
            feed(&mut whole, cluster, text);
            let want = rows(&whole);
            for i in 1..bytes.len() {
                let mut t = term(40, 2);
                feed_cut(&mut t, cluster, bytes, &[i]);
                assert_eq!(rows(&t), want, "one cut at {i}, cluster {cluster}");
                for j in i + 1..bytes.len() {
                    let mut t = term(40, 2);
                    feed_cut(&mut t, cluster, bytes, &[i, j]);
                    assert_eq!(rows(&t), want, "cuts at {i} and {j}, cluster {cluster}");
                }
            }
        }
    }

    /// The terminal's answers (`Event::PtyWrite`), in order.
    #[derive(Clone, Default)]
    struct Answers(std::sync::Arc<std::sync::Mutex<Vec<String>>>);

    impl EventListener for Answers {
        fn send_event(&self, event: alacritty_terminal::event::Event) {
            if let alacritty_terminal::event::Event::PtyWrite(text) = event {
                self.0.lock().unwrap().push(text);
            }
        }
    }

    #[test]
    fn a_program_gets_every_keyboard_flag_it_asks_for_and_is_told_so() {
        let config = Config {
            kitty_keyboard: true,
            ..Config::default()
        };
        let answers = Answers::default();
        let mut term = Term::new(config, &TermSize::new(10, 2), answers.clone());
        let feed = |term: &mut Term<Answers>, bytes: &[u8]| {
            let mut parser: Processor = Processor::new();
            let mut last_input = false;
            parser.advance(&mut ClusterHandler::new(term, true, &mut last_input), bytes);
        };
        let level = |term: &Term<Answers>| *term.mode() & TermMode::KITTY_KEYBOARD_PROTOCOL;
        let one = TermMode::DISAMBIGUATE_ESC_CODES;
        let events = TermMode::REPORT_EVENT_TYPES;
        let alternates = TermMode::REPORT_ALTERNATE_KEYS;
        let all = TermMode::REPORT_ALL_KEYS_AS_ESC;
        let text = TermMode::REPORT_ASSOCIATED_TEXT;

        // Codex's push: disambiguate, alternate keys, event types.
        feed(&mut term, b"\x1b[>7u\x1b[?u");
        assert_eq!(level(&term), one | events | alternates);
        // A push on top of it, all five, and the pop that answers it.
        feed(&mut term, b"\x1b[>31u\x1b[?u");
        assert_eq!(level(&term), one | events | alternates | all | text);
        feed(&mut term, b"\x1b[<u\x1b[?u");
        assert_eq!(level(&term), one | events | alternates);
        // A set changes the flags the way it says and the query tells the
        // flags it left, whichever way they were asked for.
        feed(&mut term, b"\x1b[=8;2u\x1b[?u");
        assert_eq!(level(&term), one | events | alternates | all);
        feed(&mut term, b"\x1b[=2;3u\x1b[?u");
        assert_eq!(level(&term), one | alternates | all);
        feed(&mut term, b"\x1b[=1;1u\x1b[?u");
        assert_eq!(level(&term), one);
        // The pop gives back what is under the entry that was set: nothing.
        feed(&mut term, b"\x1b[<u\x1b[?u");
        assert_eq!(level(&term), TermMode::empty());
        // A set with nothing pushed is the program's own flags: asked, kept,
        // and a pop puts the keys back as they were.
        feed(&mut term, b"\x1b[=5;1u\x1b[?u");
        assert_eq!(level(&term), one | alternates);
        feed(&mut term, b"\x1b[<u\x1b[?u");
        assert_eq!(level(&term), TermMode::empty());
        assert_eq!(
            *answers.0.lock().unwrap(),
            [
                "\x1b[?7u",
                "\x1b[?31u",
                "\x1b[?7u",
                "\x1b[?15u",
                "\x1b[?13u",
                "\x1b[?1u",
                "\x1b[?0u",
                "\x1b[?5u",
                "\x1b[?0u"
            ]
        );
    }

    /// The parser hands a run of text over in one piece; the grid must come out as if it had
    /// been handed over a character at a time, whatever stands around the run, and wherever the
    /// reads cut it.
    #[test]
    fn a_run_of_text_lands_as_its_characters_would_one_at_a_time() {
        const PIECES: [&str; 22] = [
            "a",
            "1",
            "#",
            "*",
            "abc",
            "12",
            "a1b",
            " ",
            "\u{200d}",
            "\u{fe0f}",
            "\u{20e3}",
            "\u{1f600}",
            "\u{1f468}",
            "\u{1f469}",
            "\u{1f1f9}",
            "\u{1f1f7}",
            "\u{1f3fb}",
            "\u{301}",
            "\u{4e2d}",
            "\u{2764}",
            "\u{1f}",
            "long enough to wrap",
        ];
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = move |bound: usize| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            usize::try_from(state % bound as u64).unwrap_or(0)
        };
        for round in 0..3000 {
            // With wrapping on and off: a wide character at the last column is only written
            // with it on, and what the wrapper knows of the cell behind depends on that.
            let wrap = if round % 3 == 0 { "\x1b[?7l" } else { "" };
            let text: String = (0..1 + next(24))
                .map(|_| PIECES[next(PIECES.len())])
                .collect();
            let text = text.replace('\u{1f}', "\r\n");

            let mut whole = term(7, 4);
            feed(&mut whole, true, wrap);
            feed(&mut whole, true, &text);

            let mut single = term(7, 4);
            feed(&mut single, true, wrap);
            let mut last_input = false;
            let mut handler = ClusterHandler::new(&mut single, true, &mut last_input);
            for c in text.chars() {
                match c {
                    '\r' => handler.carriage_return(),
                    '\n' => handler.linefeed(),
                    c => handler.input(c),
                }
            }
            drop(handler);
            assert_eq!(rows(&whole), rows(&single), "{text:?}");

            // The same bytes cut by a read, with what the loop keeps between the reads.
            let cut = 1 + next(text.len().max(2) - 1);
            let mut parts = term(7, 4);
            feed(&mut parts, true, wrap);
            let mut parser: Processor = Processor::new();
            let mut last_input = false;
            for piece in [&text.as_bytes()[..cut], &text.as_bytes()[cut..]] {
                parser.advance(
                    &mut ClusterHandler::new(&mut parts, true, &mut last_input),
                    piece,
                );
            }
            assert_eq!(rows(&whole), rows(&parts), "{text:?} cut at {cut}");
        }
    }
}
