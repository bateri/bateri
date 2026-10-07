//! The command blocks of the **whole history** — where the scroll bar's
//! block marks come from (`crate::TrackMarks`).
//!
//! **Why not the frame's anchors**: [`crate::Session::frame`] collects the
//! anchors of the rows it draws, a screenful; the track shows every block in
//! the history. Scanning the whole history in every frame would hold `Term`
//! for ten thousand rows, and building the index afresh on every piece of
//! output — the search's way — would never finish at the bottom of a full
//! history, where every line of output is an unknowable shift. So the index
//! is **kept** and **moved**: a list of block starts `(line, key)` that slides
//! up by the rows output pushes into the history, and is scanned piece by
//! piece where it has not looked yet.
//!
//! **The shift comes from a probe, never from a key.** At every look the
//! identity of the row at the screen's top is kept ([`row_identity`]); at the
//! next look it is searched for in the history ([`probe_depth`], the slide's
//! own search) and found `k` rows up — the rows pushed in between. Keys only
//! check: Ctrl-L, a job notice and `zle -I` print the same key again, so a
//! key searched for would find the new copy and the shift would be silently
//! and cumulatively wrong.
//!
//! **What checks the shift**: while the history still grows nothing in it is
//! reused, so the probe is exact and `k` must equal the growth. Once it is
//! full the ring reuses its oldest rows as new bottom ones, so after more
//! than a history's worth of output the probe can be met again at the wrong
//! depth; there the newest **carried** entry — one that was there before the
//! shift, a screen entry that slid into the history included (a row scanned
//! after the shift proves nothing) — must still carry its key where the
//! shift put it. With no entry to carry (no prompt left in the history: a
//! long build's output, a shell without integration) the probe's own row
//! must still read as it did — its text is kept with its identity; a reused
//! row was cleared and written anew.
//!
//! **One boundary**: rows `[boundary, -1]` of the history are not scanned
//! yet. A cold index puts it at the history's top, and new rows pushed in
//! fall below it by themselves — the shift moves it with the entries. Each
//! step scans at most `chunk` of those rows, top down so the range stays one
//! piece, and **the whole screen**, every time: the screen is where prompts
//! appear, and a gate on "did a prompt arrive" would miss the newest one
//! (the shell's mark comes before its anchor).
//!
//! **Pulled to the top** — a full, piecewise rescan — when the shift cannot
//! be trusted: a cold index, the history laid out anew
//! ([`search::layout_changed`]: a resize, the alternate screen, a clear, a
//! history that shrank), a probe not found, a growth that disagrees, a
//! carried key that does not hold, and in a full history with no carried
//! entry a probe row whose text changed. The last one is the only pull that
//! **waits** for a scan in flight to reach the bottom (the search's
//! convergence rule): a program rewriting the screen's top row while output
//! streams would bring it every step, and restarting each time no scan would
//! ever finish.
//!
//! **Known limit**: in a full history, after a long gap the probe can alias
//! and the newest carried key — or with none, the probe row's text — can by
//! chance stand where the wrong shift puts it; then the shift is taken and
//! marks are missing, never a mark in the wrong place. A shift of exactly a
//! whole ring's rows leaves the probe at the screen's top and reads as no
//! shift, unchecked.
//!
//! **A block starts** where the frame's stripe does: a row carrying a block
//! key whose row above does not, the last clear's remnant excepted
//! ([`block_row_continues`]) — the mark on the track and the stripe beside the
//! command are the same row.

use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::sync::Arc;

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::Line;
use alacritty_terminal::term::Term;

use crate::search::{self, LedgerMark};
use crate::session::{block_row_continues, probe_depth, row_block, row_identity};
use crate::shell::BlockKey;

/// The index of block starts in the history and on the screen (module
/// header).
#[derive(Debug, Default)]
pub(crate) struct BlockIndex {
    /// Block starts, by line (negative: the history), oldest first.
    entries: Vec<(i32, BlockKey)>,
    /// The top of the history's unscanned rows: `[boundary, -1]` are not
    /// scanned yet; `0` → none.
    boundary: i32,
    /// A pull to the top waiting for the scan in flight to reach the bottom.
    again: bool,
    /// The last look; `None` → never looked (cold).
    seen: Option<Seen>,
    /// A step's fresh starts before they replace a range of `entries` — kept
    /// so a step allocates nothing once warm.
    scratch: Vec<(i32, BlockKey)>,
    /// The rows the last step scanned — the per-step bound's witness.
    #[cfg(test)]
    scanned: usize,
}

/// One look at the grid ([`BlockIndex::seen`]).
#[derive(Clone, Copy, Debug)]
struct Seen {
    /// The identity of the row at the screen's top ([`row_identity`]).
    probe: usize,
    /// That row's text ([`row_text`]) — the shift's check when no entry is
    /// carried.
    text: u64,
    /// The scrollback's state.
    mark: LedgerMark,
}

/// What a step says of its look ([`BlockIndex::step`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Stepped {
    /// The history moved since the previous look ([`moved`]): output is
    /// streaming, so the step after this one has news to wait for.
    pub(crate) moved: bool,
    /// The step pulled the boundary to the top: what the index knew is gone,
    /// so is any picture drawn from it.
    pub(crate) restarted: bool,
}

/// What a step reads besides the grid — under the same `Term` lock.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Observation {
    /// The scrollback as this step sees it.
    pub(crate) now: LedgerMark,
    /// The scrollback's limit, rows: below it the history still grows.
    pub(crate) limit: usize,
    /// The last clear's remnant row ([`block_row_continues`]).
    pub(crate) clear_boundary: usize,
}

impl BlockIndex {
    /// One step: moves the entries to this look (or pulls the boundary to
    /// the top), scans at most `chunk` unscanned history rows and the whole
    /// screen. While the `Term` lock is held; never on the alternate screen —
    /// its grid is not the history's.
    pub(crate) fn step<T>(&mut self, term: &Term<T>, at: Observation, chunk: i32) -> Stepped {
        #[cfg(test)]
        {
            self.scanned = 0;
        }
        let top = term.topmost_line().0;
        let stepped = Stepped {
            moved: self.seen.is_some_and(|seen| moved(seen.mark, at.now)),
            restarted: self.follow(term, at, top),
        };
        self.seen = Some(Seen {
            probe: row_identity(term, Line(0)),
            text: row_text(term, Line(0)),
            mark: at.now,
        });
        self.scan_history(term, top, chunk, at.clear_boundary);
        let bottom = i32::try_from(term.screen_lines()).unwrap_or(i32::MAX) - 1;
        self.replace(term, 0, bottom, at.clear_boundary);
        stepped
    }

    /// Whether every row has been scanned and no pull to the top is waiting —
    /// the entries are the history's blocks as of the last look.
    pub(crate) fn complete(&self) -> bool {
        self.boundary >= 0 && !self.again
    }

    /// The scrollback as of the last look; `None` → never looked.
    pub(crate) fn seen(&self) -> Option<LedgerMark> {
        self.seen.map(|seen| seen.mark)
    }

    /// The entries as depths from the history's top (the scroll position's
    /// space, `crate::TrackMark::position`) as of the last look, oldest
    /// first.
    pub(crate) fn depths(&self) -> impl Iterator<Item = (u32, BlockKey)> + '_ {
        let history = self.seen.map_or(0, |seen| seen.mark.history);
        let history = i64::try_from(history).unwrap_or(i64::MAX);
        self.entries.iter().filter_map(move |&(line, key)| {
            u32::try_from(i64::from(line) + history)
                .ok()
                .map(|depth| (depth, key))
        })
    }

    /// Moves the entries and the boundary from the last look to this one,
    /// or pulls the boundary to the top (module header); `true` when what
    /// the index knew is dropped.
    fn follow<T>(&mut self, term: &Term<T>, at: Observation, top: i32) -> bool {
        let Some(seen) = self.seen else {
            return self.restart(top);
        };
        if search::layout_changed(seen.mark, at.now) {
            return self.restart(top);
        }
        let shift = if row_identity(term, Line(0)) == seen.probe {
            Some(0)
        } else {
            probe_depth(term, seen.probe)
        };
        let Some(shift) = shift else {
            return self.restart(top);
        };
        let full = at.now.history >= at.limit;
        let grown = at.now.history as i64 - seen.mark.history as i64;
        if !full && i64::from(shift) != grown {
            return self.restart(top);
        }
        if shift == 0 {
            return false;
        }
        // A scan already in flight before this shift; a finished one only
        // gains the rows just pushed in.
        let in_flight = self.boundary < 0;
        self.shift(shift, top);
        let carried = self.entries.iter().rev().find(|&&(line, _)| line < 0);
        match carried {
            Some(&(line, key)) => {
                if row_block(term, Line(line)) != Some(key) {
                    return self.restart(top);
                }
            }
            None if full && row_text(term, Line(-shift)) != seen.text => {
                if in_flight {
                    self.again = true;
                } else {
                    self.boundary = top;
                }
            }
            None => {}
        }
        false
    }

    /// Every entry and the boundary `rows` lines up; the entries that fell
    /// off the history's top go.
    fn shift(&mut self, rows: i32, top: i32) {
        for entry in &mut self.entries {
            entry.0 = entry.0.saturating_sub(rows);
        }
        let gone = self.entries.partition_point(|&(line, _)| line < top);
        self.entries.drain(..gone);
        self.boundary = self.boundary.saturating_sub(rows).max(top);
    }

    /// Nothing is known: no entries, the whole history unscanned. `true`,
    /// [`BlockIndex::follow`]'s answer.
    fn restart(&mut self, top: i32) -> bool {
        self.entries.clear();
        self.boundary = top;
        self.again = false;
        true
    }

    /// At most `chunk` rows of the unscanned history, top down; a scan that
    /// reaches the bottom takes up a waiting pull to the top.
    fn scan_history<T>(&mut self, term: &Term<T>, top: i32, chunk: i32, clear_boundary: usize) {
        let low = self.boundary.max(top);
        if low < 0 {
            let high = low.saturating_add(chunk.max(1) - 1).min(-1);
            self.replace(term, low, high, clear_boundary);
            self.boundary = high + 1;
        } else {
            self.boundary = 0;
        }
        if self.boundary >= 0 && self.again {
            self.again = false;
            self.boundary = top;
        }
    }

    /// Scans lines `low..=high` and puts their block starts in place of the
    /// entries there.
    fn replace<T>(&mut self, term: &Term<T>, low: i32, high: i32, clear_boundary: usize) {
        #[cfg(test)]
        {
            self.scanned += usize::try_from(high - low + 1).unwrap_or(0);
        }
        let Self {
            entries, scratch, ..
        } = self;
        scratch.clear();
        for line in low..=high {
            if let Some(key) = row_block(term, Line(line))
                && !block_row_continues(term, Line(line - 1), key, clear_boundary)
            {
                scratch.push((line, key));
            }
        }
        let from = entries.partition_point(|&(line, _)| line < low);
        let to = entries.partition_point(|&(line, _)| line <= high);
        entries.splice(from..to, scratch.drain(..));
    }
}

/// A row's text, folded — the probe row's check ([`Seen::text`]): a row the
/// ring reused was cleared and written anew, so its text tells it from the
/// row that stood there. Every cell's character and its combining marks.
fn row_text<T>(term: &Term<T>, line: Line) -> u64 {
    let mut hasher = DefaultHasher::new();
    for cell in &term.grid()[line] {
        cell.c.hash(&mut hasher);
        if let Some(marks) = cell.zerowidth() {
            marks.hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// A picture of the index the frame draws from ([`crate::TrackMarks`]):
/// published by a step that completes, so a scan in flight — a cold build
/// climbing down from the top — is never drawn half done; the last picture
/// stays and the frame follows it until then.
#[derive(Debug)]
pub(crate) struct BlockPass {
    /// Block starts as depths from the history's top, oldest first.
    pub(crate) rows: Vec<(u32, BlockKey)>,
    /// The scrollback as of the look the rows are from.
    pub(crate) mark: LedgerMark,
    /// Which publication this is, counted per session: the frame does not
    /// bucket the same publication at the same place twice.
    pub(crate) serial: u64,
}

impl BlockPass {
    /// Whether `index`'s entries, as of its last look, are this
    /// publication's rows where they stand — the same blocks at the same
    /// depths ([`search::depth_moved`] zero): a step that moved nothing
    /// publishes nothing and asks for no frame.
    pub(crate) fn same_as(&self, index: &BlockIndex, limit: usize) -> bool {
        index
            .seen()
            .is_some_and(|mark| search::depth_moved(self.mark, mark, limit) == Some(0))
            && index.depths().eq(self.rows.iter().copied())
    }
}

/// The session's slot of the index — a **leaf lock**: the step takes the
/// index out and works under `Term` with the slot released
/// ([`crate::Session::block_step`]); the frame takes the publication after
/// `Term` and buckets it with the slot released.
#[derive(Debug, Default)]
pub(crate) struct BlockSlot {
    /// The index; a default one while a step holds it (`busy`).
    pub(crate) index: BlockIndex,
    /// A step has the index out.
    pub(crate) busy: bool,
    /// The last publication; shared, not copied.
    pub(crate) pass: Option<Arc<BlockPass>>,
    /// Publications so far ([`BlockPass::serial`]); never reset.
    pub(crate) passes: u64,
    /// The marks became wanted again: the next complete step publishes even
    /// an unchanged picture, so a frame is asked for — the lane was not kept
    /// up while nobody wanted it, and a finished command's colour changes no
    /// row the index sees.
    pub(crate) owed: bool,
}

/// Whether the history moved since `seen` in a way the index must look at:
/// output arrived or the history was laid out anew. Scrolling the view is not
/// such a move — the index keeps no view.
pub(crate) fn moved(seen: LedgerMark, now: LedgerMark) -> bool {
    seen.epoch != now.epoch || search::layout_changed(seen, now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handler::ClusterHandler;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::vte::ansi::Processor;

    const COLS: usize = 20;
    const ROWS: usize = 5;

    /// A grid of [`COLS`] × [`ROWS`] with a scrollback of `limit` rows.
    fn term(limit: usize) -> Term<VoidListener> {
        let config = Config {
            scrolling_history: limit,
            ..Config::default()
        };
        Term::new(config, &TermSize::new(COLS, ROWS), VoidListener)
    }

    fn feed(term: &mut Term<VoidListener>, bytes: &str) {
        let mut parser: Processor = Processor::new();
        let mut last_input = false;
        parser.advance(
            &mut ClusterHandler::new(term, true, &mut last_input),
            bytes.as_bytes(),
        );
    }

    /// A prompt row carrying block `id`'s anchor, then a line break.
    fn prompt(id: u32) -> String {
        format!("\x1b]8;;bateri://block/{id}\x1b\\$ cmd{id}\x1b]8;;\x1b\\\r\n")
    }

    /// `n` lines of output, no anchor.
    fn output(n: usize) -> String {
        (0..n).map(|i| format!("out {i}\r\n")).collect()
    }

    /// The test's own scrollback state: the epoch is the caller's counter of
    /// output, the clears the caller's too.
    fn mark(term: &Term<VoidListener>, epoch: u64, wipes: u64) -> LedgerMark {
        LedgerMark {
            history: term.history_size(),
            offset: term.grid().display_offset(),
            user: 0,
            epoch,
            wipes,
            columns: term.columns(),
            lines: term.screen_lines(),
            alt: false,
        }
    }

    fn look(term: &Term<VoidListener>, limit: usize, epoch: u64) -> Observation {
        Observation {
            now: mark(term, epoch, 0),
            limit,
            clear_boundary: 0,
        }
    }

    /// Steps until the index is complete; the number of steps.
    fn settle(
        index: &mut BlockIndex,
        term: &Term<VoidListener>,
        at: Observation,
        chunk: i32,
    ) -> usize {
        let mut steps = 0;
        loop {
            index.step(term, at, chunk);
            steps += 1;
            assert!(
                index.scanned <= usize::try_from(chunk).unwrap() + ROWS,
                "a step scanned {} rows",
                index.scanned
            );
            if index.complete() {
                return steps;
            }
            assert!(steps < 10_000, "the index never completed");
        }
    }

    /// What a cold index finds in the grid as it is.
    fn fresh(term: &Term<VoidListener>, limit: usize) -> Vec<(i32, BlockKey)> {
        let mut index = BlockIndex::default();
        settle(&mut index, term, look(term, limit, 0), 7);
        index.entries
    }

    fn lines(index: &BlockIndex) -> Vec<i32> {
        index.entries.iter().map(|&(line, _)| line).collect()
    }

    #[test]
    fn a_cold_index_finds_every_block_of_the_history_piece_by_piece() {
        let mut term = term(100);
        let mut bytes = String::new();
        for id in 1..=6 {
            bytes += &prompt(id);
            bytes += &output(4);
        }
        feed(&mut term, &bytes);
        let history = term.history_size();
        assert!(history > 20, "{history}");
        let mut index = BlockIndex::default();
        let steps = settle(&mut index, &term, look(&term, 100, 1), 3);
        assert!(
            steps > 5,
            "the history was not scanned piece by piece: {steps}"
        );
        let keys: Vec<BlockKey> = index.entries.iter().map(|&(_, key)| key).collect();
        assert_eq!(keys, (1..=6).map(BlockKey::Local).collect::<Vec<_>>());
        // Each one where its prompt is: five rows apart.
        let found = lines(&index);
        assert!(
            found.windows(2).all(|pair| pair[1] - pair[0] == 5),
            "{found:?}"
        );
        // Its depths are the scroll position's.
        let depths: Vec<u32> = index.depths().map(|(depth, _)| depth).collect();
        assert_eq!(depths.first(), Some(&0));
    }

    #[test]
    fn output_moves_the_entries_by_the_probe_and_scans_only_the_new_rows() {
        // A history that still grows and one that is full: either way the
        // rows output pushes in are the only ones scanned.
        for limit in [100, 12] {
            // The first prompt is in the history, the second on screen: a
            // carried entry checks every shift, in a full history too.
            let mut term = term(limit);
            feed(
                &mut term,
                &(output(20) + &prompt(1) + &output(8) + &prompt(2) + &output(1)),
            );
            let mut index = BlockIndex::default();
            settle(&mut index, &term, look(&term, limit, 1), 4);
            assert_eq!(index.entries.len(), 2, "limit {limit}");
            assert_eq!(term.history_size() >= limit, limit == 12);
            for (epoch, rows) in [(2, 1), (3, 3), (4, 2)] {
                feed(&mut term, &output(rows));
                index.step(&term, look(&term, limit, epoch), 4);
                assert!(
                    index.complete(),
                    "limit {limit}: {rows} rows asked for a rescan"
                );
                assert_eq!(index.scanned, 4.min(rows) + ROWS, "limit {limit}");
                assert_eq!(index.entries, fresh(&term, limit), "limit {limit}");
            }
        }
    }

    #[test]
    fn a_long_promptless_stream_costs_only_its_new_rows() {
        // `tail -f` after a prompt: every look scans what arrived and the screen.
        let limit = 1000;
        let mut term = term(limit);
        feed(&mut term, &(prompt(1) + &output(2)));
        let mut index = BlockIndex::default();
        settle(&mut index, &term, look(&term, limit, 1), 500);
        for epoch in 2..60 {
            let before = term.history_size();
            feed(&mut term, &output(3));
            let pushed = term.history_size() - before;
            index.step(&term, look(&term, limit, epoch), 500);
            assert!(index.complete());
            assert_eq!(index.scanned, pushed + ROWS, "look {epoch}");
        }
        assert_eq!(index.entries, fresh(&term, limit));
    }

    #[test]
    fn the_same_key_printed_again_does_not_move_the_shift() {
        // Ctrl-L clears the screen into the history and reprints the prompt
        // with the same key: the probe, not the key, says how far rows moved.
        let limit = 100;
        let mut term = term(limit);
        feed(
            &mut term,
            &(prompt(1) + &output(2) + "\x1b]8;;bateri://block/2\x1b\\$ "),
        );
        let mut index = BlockIndex::default();
        settle(&mut index, &term, look(&term, limit, 1), 50);
        feed(&mut term, "\x1b]8;;\x1b\\\x1b[H\x1b[2J");
        feed(&mut term, "\x1b]8;;bateri://block/2\x1b\\$ \x1b]8;;\x1b\\");
        index.step(&term, look(&term, limit, 2), 50);
        assert!(index.complete());
        assert_eq!(index.entries, fresh(&term, limit));
        // Both copies of block 2 are history rows now, the old one a
        // start, the reprint its continuation (no clear boundary given).
        assert_eq!(
            index
                .entries
                .iter()
                .filter(|&&(_, key)| key == BlockKey::Local(2))
                .count(),
            1
        );
    }

    #[test]
    fn a_wrapped_prompt_is_one_block() {
        let limit = 100;
        let mut term = term(limit);
        let long = "x".repeat(COLS + 5);
        feed(
            &mut term,
            &format!(
                "\x1b]8;;bateri://block/1\x1b\\$ {long}\x1b]8;;\x1b\\\r\n{}",
                output(8)
            ),
        );
        let entries = fresh(&term, limit);
        assert_eq!(entries.len(), 1, "{entries:?}");
    }

    #[test]
    fn a_layout_change_pulls_the_boundary_to_the_top() {
        let limit = 100;
        let mut term = term(limit);
        feed(
            &mut term,
            &(prompt(1) + &output(12) + &prompt(2) + &output(3)),
        );
        let mut index = BlockIndex::default();
        settle(&mut index, &term, look(&term, limit, 1), 4);
        // A clear from the terminal's side: nothing moved that the probe sees.
        let at = Observation {
            now: mark(&term, 1, 1),
            ..look(&term, limit, 1)
        };
        index.step(&term, at, 4);
        assert!(!index.complete(), "a clear did not rescan");
        // A resize re-wraps the history.
        let mut index = BlockIndex::default();
        settle(&mut index, &term, look(&term, limit, 1), 4);
        term.resize(TermSize::new(COLS + 3, ROWS));
        index.step(&term, look(&term, limit, 1), 4);
        assert!(!index.complete(), "a resize did not rescan");
        settle(&mut index, &term, look(&term, limit, 1), 4);
        assert_eq!(index.entries, fresh(&term, limit));
    }

    /// `n` lines of output numbered from `from` — no two alike, so a row's
    /// text tells it apart.
    fn numbered(from: usize, n: usize) -> String {
        (from..from + n).map(|i| format!("line {i}\r\n")).collect()
    }

    #[test]
    fn a_full_history_with_no_carried_entry_checks_the_probe_row() {
        // No prompt anywhere to check the shift by: the probe's own row,
        // read as it was, takes the shift; rewritten before it scrolled, the
        // whole history is looked at again.
        let limit = 12;
        let mut term = term(limit);
        feed(&mut term, &numbered(0, 40));
        let mut index = BlockIndex::default();
        let first = index.step(&term, look(&term, limit, 1), 3);
        assert!(first.restarted && !first.moved, "{first:?}");
        settle(&mut index, &term, look(&term, limit, 1), 3);
        feed(&mut term, &numbered(40, 2));
        let stepped = index.step(&term, look(&term, limit, 2), 3);
        assert!(index.complete(), "a checked shift asked for a rescan");
        assert_eq!(
            stepped,
            Stepped {
                moved: true,
                restarted: false
            }
        );
        // The screen's top row rewritten in place, then output.
        feed(&mut term, "\x1b7\x1b[Hrewritten\x1b8");
        feed(&mut term, &numbered(42, 2));
        index.step(&term, look(&term, limit, 3), 3);
        assert!(!index.complete(), "the unchecked shift was taken");
        settle(&mut index, &term, look(&term, limit, 3), 3);
        assert_eq!(index.entries, fresh(&term, limit));
    }

    #[test]
    fn a_probe_met_again_with_no_prompt_is_caught_by_its_text() {
        // The ring wrapped with no prompt to carry: the reused row was
        // written anew, so the probe's text no longer reads as it did.
        let limit = 20;
        let mut term = term(limit);
        feed(&mut term, &numbered(0, 30));
        let mut index = BlockIndex::default();
        settle(&mut index, &term, look(&term, limit, 1), 50);
        let probe = index.seen.map(|seen| seen.probe).unwrap();
        let (mut fed, mut left) = (0, false);
        loop {
            feed(&mut term, &numbered(30 + fed, 1));
            fed += 1;
            match probe_depth(&term, probe) {
                None => left = true,
                Some(depth) if left && depth >= 2 => break,
                _ => {}
            }
            assert!(fed < 10_000, "the probe was never met again");
        }
        index.step(&term, look(&term, limit, 2), 3);
        assert!(!index.complete(), "the aliased shift was taken");
    }

    #[test]
    fn a_rescan_in_flight_is_not_restarted_by_an_unchecked_shift() {
        // Output streaming into a full history with no prompt in it while a
        // program rewrites the screen's top row: each look pulls to the top,
        // so a pull waits for the scan to reach the bottom and the scan does
        // finish.
        let limit = 30;
        let mut term = term(limit);
        feed(&mut term, &output(60));
        let mut index = BlockIndex::default();
        let mut epoch = 1;
        let mut completed = false;
        let mut tick = |term: &mut Term<VoidListener>, index: &mut BlockIndex| {
            feed(term, &format!("\x1b7\x1b[Hrewritten {epoch}\x1b8"));
            feed(term, &output(1));
            epoch += 1;
            index.step(term, look(term, limit, epoch), 4);
        };
        for _ in 0..40 {
            tick(&mut term, &mut index);
            completed |= index.complete();
        }
        assert!(!completed, "a full pass with output streaming");
        // Every pass reached the bottom: the boundary went round, not stuck at the top.
        let mut reached = 0;
        let mut index = BlockIndex::default();
        for _ in 0..60 {
            tick(&mut term, &mut index);
            if index.boundary == term.topmost_line().0 {
                reached += 1;
            }
        }
        assert!(reached >= 2, "the scan never reached the bottom: {reached}");
    }

    #[test]
    fn a_probe_met_again_after_the_ring_wrapped_is_caught_by_the_key() {
        // A full history and more than a whole ring of output between two
        // looks: the probe's row left the history, was reused as a new
        // bottom row and is met again a few rows up — but the carried prompt
        // is not where that shift puts it.
        let limit = 20;
        let mut term = term(limit);
        feed(&mut term, &(output(30) + &prompt(1) + &output(2)));
        let mut index = BlockIndex::default();
        settle(&mut index, &term, look(&term, limit, 1), 50);
        assert_eq!(index.entries.len(), 1);
        let probe = index.seen.map(|seen| seen.probe).unwrap();
        // The ring is the storage's whole length, spare rows included —
        // alacritty's to size, so the wrap is found by feeding, not by sum.
        let (mut fed, mut left) = (0, false);
        loop {
            feed(&mut term, &output(1));
            fed += 1;
            match probe_depth(&term, probe) {
                None => left = true,
                Some(depth) if left && depth >= 2 => break,
                _ => {}
            }
            assert!(fed < 10_000, "the probe was never met again");
        }
        assert!(fed > limit + ROWS, "{fed}");
        index.step(&term, look(&term, limit, 2), 3);
        assert!(!index.complete(), "the aliased shift was taken");
        settle(&mut index, &term, look(&term, limit, 2), 3);
        assert_eq!(index.entries, fresh(&term, limit));
    }

    #[test]
    fn output_while_nobody_looked_ends_where_a_fresh_build_does() {
        // The index stops while the marks are not wanted and is stepped again
        // later: whatever came in between, the next settle is a fresh build.
        for (limit, between) in [(100, 7), (100, 60), (20, 9), (20, 26), (20, 80)] {
            let mut term = term(limit);
            feed(
                &mut term,
                &(prompt(1) + &output(5) + &prompt(2) + &output(5)),
            );
            let mut index = BlockIndex::default();
            settle(&mut index, &term, look(&term, limit, 1), 6);
            let mut bytes = String::new();
            for id in 3..6 {
                bytes += &prompt(id);
                bytes += &output(between / 3);
            }
            feed(&mut term, &bytes);
            settle(&mut index, &term, look(&term, limit, 2), 6);
            assert_eq!(
                index.entries,
                fresh(&term, limit),
                "limit {limit}, {between} rows between"
            );
        }
    }

    #[test]
    fn moved_ignores_the_view_and_sees_output_and_layout() {
        let term = term(100);
        let seen = mark(&term, 3, 0);
        let scrolled = LedgerMark {
            offset: 4,
            user: 4,
            ..seen
        };
        assert!(!moved(seen, scrolled));
        assert!(moved(seen, LedgerMark { epoch: 4, ..seen }));
        assert!(moved(seen, LedgerMark { wipes: 1, ..seen }));
        assert!(moved(seen, LedgerMark { columns: 9, ..seen }));
    }
}
