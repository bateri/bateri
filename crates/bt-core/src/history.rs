//! The command memory: what each command's line said when it started.
//!
//! A command's text lives only in the rows of the prompt that carried it, and those rows scroll
//! away (and a full-screen program leaves the screen they are on). The palette's command history
//! has to outlive them, so the text is read **once, when the command starts**
//! ([`crate::Session::remember_commands`]) and kept here, bounded ([`MEMORY_LIMIT`]) and in memory
//! only — nothing of it is written to disk. What a command came to (its exit, its duration, its
//! clock) is not kept: it is asked of the ledger when the history is listed, so it is never stale.
//!
//! **Where the text is read from.** Usually the main queue's turn after the `C` mark finds the
//! prompt's rows on the grid. A program that takes the alternate screen in the same read as the
//! mark (`vim file`) is on that screen before the main queue looks, and the rows are on the
//! primary screen behind it, which the turn reads in the same round ([`Rows`]). The parser's
//! wrapper also reads the newest block's text **just before it swaps screens** ([`newest_block`])
//! and leaves it with the session (`Adapter::captured`), where the turn takes it from first.
//!
//! The reading functions here take the rows they read and nothing else ([`Rows`]: a `Term`, or the
//! primary screen's grid behind a full screen program), so the caller holds the lock; none of
//! them writes to it.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Duration;

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::Line;
use alacritty_terminal::term::Term;

use crate::commands::CommandId;
use crate::session::{Rows, block_row_continues, block_text, row_block, row_text};
use crate::shell::{BlockKey, Cursors};

/// How many commands a pane remembers. The oldest go first.
pub const MEMORY_LIMIT: usize = 500;

/// The most characters of one command's output a copy takes: the end of it, where the error is.
pub const OUTPUT_LIMIT: usize = 1 << 20;

/// How many rows above the bottom [`newest_block`] looks: the command was just started, so its
/// prompt is on the screen or right above it.
const CAPTURE_REACH: i32 = 64;

/// One command as the memory holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Remembered {
    pub(crate) key: BlockKey,
    pub(crate) text: String,
    /// Where it was started, for a local command whose start was seen.
    pub(crate) directory: Option<PathBuf>,
}

/// The commands of one pane, oldest first, and where the ledgers' walks ended.
#[derive(Debug, Default)]
pub(crate) struct CommandMemory {
    entries: VecDeque<Remembered>,
    cursors: Cursors,
    /// The restored screen's blocks have been taken ([`Self::adopt_restored`]).
    restored: bool,
}

impl CommandMemory {
    pub(crate) fn cursors(&self) -> Cursors {
        self.cursors
    }

    pub(crate) fn set_cursors(&mut self, cursors: Cursors) {
        self.cursors = cursors;
    }

    /// The commands, oldest first.
    pub(crate) fn entries(&self) -> &VecDeque<Remembered> {
        &self.entries
    }

    pub(crate) fn newest(&self) -> Option<&Remembered> {
        self.entries.back()
    }

    /// Remembers `entry`, once: a key the memory has keeps its text. The oldest entry goes when
    /// the memory is full.
    pub(crate) fn push(&mut self, entry: Remembered) {
        if self.entries.iter().any(|known| known.key == entry.key) {
            return;
        }
        if self.entries.len() == MEMORY_LIMIT {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    /// The local ledger started over (a new shell numbers its blocks from one): the local keys
    /// the memory holds are another shell's and would be read as this one's.
    pub(crate) fn forget_local(&mut self) {
        self.entries
            .retain(|known| !matches!(known.key, BlockKey::Local(_)));
    }

    pub(crate) fn restored(&self) -> bool {
        self.restored
    }

    /// The blocks of a restored screen, oldest first, go before everything else: they are older
    /// than any command this process saw. Once.
    pub(crate) fn adopt_restored(&mut self, found: Vec<Remembered>) {
        if self.restored {
            return;
        }
        self.restored = true;
        let room = MEMORY_LIMIT.saturating_sub(self.entries.len());
        let skip = found.len().saturating_sub(room);
        for entry in found.into_iter().skip(skip).rev() {
            self.entries.push_front(entry);
        }
    }
}

/// How a remembered command stands, as the ledger knows it now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordState {
    /// It runs; for how long, when the ledger's clock is its.
    Running(Option<Duration>),
    /// It ended, or the ledger no longer says how.
    Finished {
        exit: Option<i32>,
        elapsed: Option<Duration>,
    },
    /// A block of the screen a restored session came back with: the ledger of that run is gone,
    /// what is left is the stripe — whether it failed.
    Restored { failed: bool },
}

/// One command of a pane's history ([`crate::Session::command_records`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandRecord {
    pub id: CommandId,
    /// The command's line as the prompt's rows held it when it started.
    pub text: String,
    /// The directory it started in: a local command whose start was seen.
    pub directory: Option<PathBuf>,
    /// It ran in our remote shell, on the far end of ssh.
    pub remote: bool,
    pub state: RecordState,
    /// When it started, seconds since the Unix epoch.
    pub started: Option<u32>,
    /// Its row, rows from the history's top, while the row is still in the history on the primary
    /// screen: where ⌘⏎ takes the window. `None`: scrolled away or cleared, or the pane is on the
    /// alternate screen (the window cannot be taken there).
    pub depth: Option<u32>,
    /// Its output can be read ([`crate::Session::command_output`]): its row is still in the
    /// primary screen's rows, which a program on the whole screen leaves where they were — except
    /// for the command that is that program, whose output is the screen it draws. `depth` being
    /// set implies it.
    pub readable: bool,
}

/// The depth of each of `wanted`'s start rows, in one pass over the history: `None` for a block
/// whose row is no longer there. A pass instead of a search per key — the history lists hundreds.
pub(crate) fn depths<R: Rows>(rows: &R, wanted: &[BlockKey], boundary: usize) -> Vec<Option<u32>> {
    let mut found = vec![None; wanted.len()];
    let (top, bottom) = (rows.topmost_line().0, rows.bottommost_line().0);
    let history = i64::try_from(rows.history_size()).unwrap_or(0);
    for line in top..=bottom {
        let Some(key) = row_block(rows, Line(line)) else {
            continue;
        };
        let Some(at) = wanted.iter().position(|wanted| *wanted == key) else {
            continue;
        };
        if found[at].is_none() && !block_row_continues(rows, Line(line - 1), key, boundary) {
            found[at] = u32::try_from(i64::from(line) + history).ok();
        }
    }
    found
}

/// The blocks a restored screen came back with, oldest first: their start rows' text.
pub(crate) fn saved_blocks<R: Rows>(rows: &R) -> Vec<Remembered> {
    let mut blocks: Vec<Remembered> = Vec::new();
    let (top, bottom) = (rows.topmost_line().0, rows.bottommost_line().0);
    for line in top..=bottom {
        let Some(key @ BlockKey::Saved { .. }) = row_block(rows, Line(line)) else {
            continue;
        };
        if blocks.iter().any(|known| known.key == key)
            || block_row_continues(rows, Line(line - 1), key, 0)
        {
            continue;
        }
        let text = block_text(rows, line, key);
        if !text.is_empty() {
            blocks.push(Remembered {
                key,
                text,
                directory: None,
            });
        }
    }
    blocks
}

/// The newest block on the screen being left — its key and its command's text — read just before
/// a program takes the alternate screen, from the rows near the bottom. `None` when nothing
/// anchored is there (a shell without our marks).
pub(crate) fn newest_block<T>(term: &Term<T>) -> Option<(BlockKey, String)> {
    let (top, bottom) = (term.topmost_line().0, term.bottommost_line().0);
    let reach = bottom.saturating_sub(CAPTURE_REACH).max(top);
    let last = (reach..=bottom)
        .rev()
        .find(|&probe| row_block(term, Line(probe)).is_some())?;
    let key = row_block(term, Line(last))?;
    let mut start = last;
    while start > top && block_row_continues(term, Line(start - 1), key, 0) {
        start -= 1;
    }
    let text = block_text(term, start, key);
    (!text.is_empty()).then_some((key, text))
}

/// The output of block `key`: the rows after the ones that carry its command, up to the next
/// anchored row — the next prompt — or the bottom, the end trimmed. At most [`OUTPUT_LIMIT`]
/// characters, the end of it. `None` when the block's row is no longer in the history.
pub(crate) fn output_of<R: Rows>(rows: &R, key: BlockKey, boundary: usize) -> Option<String> {
    let (top, bottom) = (rows.topmost_line().0, rows.bottommost_line().0);
    let start = (top..=bottom).rev().find(|&probe| {
        row_block(rows, Line(probe)) == Some(key)
            && !block_row_continues(rows, Line(probe - 1), key, boundary)
    })?;
    let mut line = start;
    while line <= bottom && row_block(rows, Line(line)) == Some(key) {
        line += 1;
    }
    let mut lines: Vec<String> = Vec::new();
    let mut joined = String::new();
    while line <= bottom && row_block(rows, Line(line)).is_none() {
        let (piece, wrapped) = row_text(rows, line);
        if wrapped {
            joined.push_str(&piece);
        } else {
            joined.push_str(piece.trim_end());
            lines.push(std::mem::take(&mut joined));
        }
        line += 1;
    }
    if !joined.is_empty() {
        lines.push(joined.trim_end().to_owned());
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    Some(keep_end(&lines, OUTPUT_LIMIT).join("\n"))
}

/// The last of `lines` that fit in `limit` characters, newlines counted: whole lines from the back.
fn keep_end(lines: &[String], limit: usize) -> &[String] {
    let mut size = 0usize;
    let mut keep = lines.len();
    while keep > 0 {
        let next = size + lines[keep - 1].chars().count() + 1;
        if next > limit {
            break;
        }
        size = next;
        keep -= 1;
    }
    &lines[keep..]
}

#[cfg(test)]
mod tests {
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::vte::ansi::Processor;

    use super::*;
    use crate::handler::ClusterHandler;
    use crate::shell::Stripe;

    const COLS: usize = 20;
    const ROWS: usize = 5;

    fn term(history: usize) -> Term<VoidListener> {
        let config = Config {
            scrolling_history: history,
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

    /// A prompt row whose anchor covers `$ ` and the command, as the shell prints it.
    fn prompt(id: u32, command: &str) -> String {
        format!("\x1b]8;;bateri://block/{id}\x1b\\$ {command}\x1b]8;;\x1b\\\r\n")
    }

    fn entry(id: u32, text: &str) -> Remembered {
        Remembered {
            key: BlockKey::Local(id),
            text: text.to_owned(),
            directory: None,
        }
    }

    #[test]
    fn a_command_is_remembered_once_and_the_oldest_go_first() {
        let mut memory = CommandMemory::default();
        memory.push(entry(1, "ls"));
        memory.push(entry(1, "changed"));
        assert_eq!(memory.entries().len(), 1);
        assert_eq!(memory.newest().map(|e| e.text.as_str()), Some("ls"));
        for id in 2..=(MEMORY_LIMIT as u32 + 5) {
            memory.push(entry(id, "x"));
        }
        assert_eq!(memory.entries().len(), MEMORY_LIMIT);
        assert_eq!(
            memory.entries().front().map(|e| e.key),
            Some(BlockKey::Local(6)),
            "the first five were pushed out"
        );
    }

    #[test]
    fn a_new_shells_counter_makes_the_local_keys_foreign() {
        let mut memory = CommandMemory::default();
        memory.push(entry(1, "ls"));
        memory.forget_local();
        assert!(memory.entries().is_empty());
    }

    #[test]
    fn restored_blocks_go_before_everything_and_are_taken_once() {
        let mut memory = CommandMemory::default();
        memory.push(entry(1, "live"));
        let old = |id| Remembered {
            key: BlockKey::Local(id),
            text: format!("old {id}"),
            directory: None,
        };
        memory.adopt_restored(vec![old(100), old(101)]);
        memory.adopt_restored(vec![old(102)]);
        let texts: Vec<&str> = memory.entries().iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts, ["old 100", "old 101", "live"]);
        assert!(memory.restored());
    }

    #[test]
    fn restored_blocks_that_do_not_fit_are_the_oldest_ones_left_out() {
        let mut memory = CommandMemory::default();
        for id in 1..=(MEMORY_LIMIT as u32 - 1) {
            memory.push(entry(id, "live"));
        }
        let old = |id: u32| Remembered {
            key: BlockKey::Local(1000 + id),
            text: format!("old {id}"),
            directory: None,
        };
        memory.adopt_restored(vec![old(1), old(2), old(3)]);
        assert_eq!(memory.entries().len(), MEMORY_LIMIT);
        assert_eq!(
            memory.entries().front().map(|e| e.text.as_str()),
            Some("old 3")
        );
    }

    #[test]
    fn the_rows_of_every_wanted_block_are_found_in_one_pass_and_a_lost_one_is_none() {
        let mut term = term(100);
        feed(
            &mut term,
            &format!("{}a\r\nb\r\n{}x\r\n", prompt(1, "ls"), prompt(2, "make")),
        );
        let found = depths(
            &term,
            &[BlockKey::Local(2), BlockKey::Local(1), BlockKey::Local(9)],
            0,
        );
        let (second, first) = (found[0].expect("block 2"), found[1].expect("block 1"));
        assert!(first < second, "block 1 is above block 2");
        assert_eq!(second - first, 3, "its row, its two output rows");
        assert_eq!(found[2], None, "no row carries block 9");
    }

    #[test]
    fn a_row_scrolled_out_of_the_history_is_no_longer_there() {
        let mut term = term(3);
        let mut bytes = prompt(1, "yes");
        for n in 0..30 {
            bytes.push_str(&format!("line {n}\r\n"));
        }
        bytes.push_str(&prompt(2, "ls"));
        feed(&mut term, &bytes);
        let found = depths(&term, &[BlockKey::Local(1), BlockKey::Local(2)], 0);
        assert_eq!(found[0], None);
        assert!(found[1].is_some());
        assert_eq!(output_of(&term, BlockKey::Local(1), 0), None);
    }

    #[test]
    fn the_output_is_the_rows_after_the_command_up_to_the_next_prompt() {
        let mut term = term(100);
        feed(
            &mut term,
            &format!(
                "{}one\r\n\r\ntwo   \r\n{}{}three\r\n",
                prompt(1, "ls"),
                prompt(2, "true"),
                prompt(3, "pwd"),
            ),
        );
        assert_eq!(
            output_of(&term, BlockKey::Local(1), 0).as_deref(),
            Some("one\n\ntwo"),
            "the blank line inside stays, the trailing blanks do not"
        );
        assert_eq!(
            output_of(&term, BlockKey::Local(2), 0).as_deref(),
            Some(""),
            "a command that printed nothing"
        );
        assert_eq!(
            output_of(&term, BlockKey::Local(3), 0).as_deref(),
            Some("three"),
            "the last one runs to the bottom, the empty rows below trimmed"
        );
    }

    #[test]
    fn a_line_the_screen_wrapped_is_one_line_of_output() {
        let mut term = term(100);
        feed(
            &mut term,
            &format!("{}01234567890123456789ABCDE\r\nend\r\n", prompt(1, "cat f")),
        );
        assert_eq!(
            output_of(&term, BlockKey::Local(1), 0).as_deref(),
            Some("01234567890123456789ABCDE\nend")
        );
    }

    #[test]
    fn only_the_end_of_an_output_longer_than_the_limit_is_kept() {
        let lines: Vec<String> = ["aaa", "bbb", "ccc"].map(str::to_owned).to_vec();
        assert_eq!(keep_end(&lines, 100), lines.as_slice());
        // Four characters each with the newline: two fit in 8.
        assert_eq!(keep_end(&lines, 8), &lines[1..]);
        assert_eq!(keep_end(&lines, 3), &lines[3..]);
    }

    #[test]
    fn the_newest_block_is_read_with_its_wrapped_command_whole() {
        let mut term = term(100);
        feed(
            &mut term,
            &format!("{}ok\r\n{}", prompt(1, "ls"), prompt(2, "echo 0123456789")),
        );
        let (key, text) = newest_block(&term).expect("a block on the screen");
        assert_eq!(key, BlockKey::Local(2));
        assert_eq!(
            text, "$ echo 0123456789",
            "a command that wrapped is not cut at the edge"
        );
        assert_eq!(newest_block(&self::term(10)), None, "nothing anchored");
    }

    #[test]
    fn the_blocks_a_restored_screen_came_back_with_are_read_oldest_first() {
        let mut term = term(100);
        let saved = |id: u32, role: &str, command: &str| {
            format!("\x1b]8;;bateri://sblock/{id}.{role}\x1b\\$ {command}\x1b]8;;\x1b\\\r\nout\r\n")
        };
        feed(
            &mut term,
            &format!(
                "{}{}{}",
                saved(1, "success", "ls"),
                saved(2, "error", "make"),
                prompt(1, "live")
            ),
        );
        let found = saved_blocks(&term);
        let texts: Vec<&str> = found.iter().map(|b| b.text.as_str()).collect();
        assert_eq!(texts, ["$ ls", "$ make"], "the live block is not restored");
        assert_eq!(
            found[1].key,
            BlockKey::Saved {
                id: 2,
                stripe: Stripe::Error
            }
        );
    }
}
