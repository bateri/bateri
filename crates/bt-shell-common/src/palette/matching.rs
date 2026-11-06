//! Finding a row from a few typed letters.
//!
//! The rule, in the numbers the design was tried with (they are chosen, not measured):
//!
//! - **A substring of the title** scores 100, +40 at the very start, +25 at the start of a word,
//!   −0.5 per letter of position and −0.05 per letter of title.
//! - **Letters in order, skipping** score 40, +8 for a letter that starts a word, +3 for one
//!   next to the last, −1 for the rest — **unless the match is scattered**: every run of
//!   matched letters must start a word ("spr" finds Split Right), or there are two runs at most
//!   and the first starts a word. "theme" finding Python Environment (t-h in "python", e in
//!   "environment") is noise, not a hit.
//! - **A keyword** the word begins scores 60, **the second line** containing the word 45. The
//!   row's cluster heading counts as a keyword ("themes" finds the themes).
//! - A query of several words needs **every** word to hit; the scores add.
//!
//! Case and accents are folded ([`super::fold`]). Only the title's letters are marked: a hit
//! through a keyword or the second line marks nothing.

use super::fold::{Folded, word};
use super::row::Row;

const SUBSTRING: f64 = 100.0;
const AT_START: f64 = 40.0;
const AT_WORD: f64 = 25.0;
const POSITION_COST: f64 = 0.5;
const SUBSEQUENCE: f64 = 40.0;
const WORD_LETTER: f64 = 8.0;
const NEXT_LETTER: f64 = 3.0;
const OTHER_LETTER: f64 = -1.0;
const LENGTH_COST: f64 = 0.05;
const KEYWORD: f64 = 60.0;
const SUBTITLE: f64 = 45.0;

/// A hit: how well, and which letters of the title to mark (positions in the title as written).
#[derive(Clone, Debug, PartialEq)]
pub struct Match {
    pub score: f64,
    pub marks: Vec<usize>,
}

/// Where a word can begin inside a title: after a space, `/`, `·`, `-`, `_`, `.` or `(`.
fn is_boundary(c: char) -> bool {
    c.is_whitespace() || matches!(c, '/' | '·' | '-' | '_' | '.' | '(')
}

fn starts_word(letters: &[char], index: usize) -> bool {
    index == 0 || is_boundary(letters[index - 1])
}

/// One query word against a title.
pub(crate) fn fuzzy(query: &[char], title: &Folded) -> Option<Match> {
    if query.is_empty() {
        return None;
    }
    let letters = title.chars();
    if let Some(at) = title.find(query) {
        let bonus = if at == 0 {
            AT_START
        } else if starts_word(letters, at) {
            AT_WORD
        } else {
            0.0
        };
        return Some(Match {
            score: SUBSTRING + bonus - at as f64 * POSITION_COST - title.len() as f64 * LENGTH_COST,
            marks: (at..at + query.len()).map(|i| title.origin(i)).collect(),
        });
    }
    let mut score = SUBSEQUENCE;
    let mut marks = Vec::with_capacity(query.len());
    let mut matched = 0;
    let mut last: Option<usize> = None;
    let (mut runs, mut word_runs, mut first_word) = (0, 0, false);
    for (i, &letter) in letters.iter().enumerate() {
        if matched == query.len() {
            break;
        }
        if letter != query[matched] {
            continue;
        }
        let at_word = starts_word(letters, i);
        let next = last.is_some_and(|l| i == l + 1);
        if !next {
            runs += 1;
            if at_word {
                word_runs += 1;
            }
            if runs == 1 {
                first_word = at_word;
            }
        }
        score += if at_word {
            WORD_LETTER
        } else if next {
            NEXT_LETTER
        } else {
            OTHER_LETTER
        };
        marks.push(title.origin(i));
        last = Some(i);
        matched += 1;
    }
    if matched < query.len() || !(word_runs == runs || (runs <= 2 && first_word)) {
        return None;
    }
    Some(Match {
        score: score - title.len() as f64 * LENGTH_COST,
        marks,
    })
}

/// A whole query against a row; `None` when the row is not a hit. An empty query matches
/// everything with no score.
pub fn score(row: &Row, query: &str) -> Option<Match> {
    let words: Vec<Vec<char>> = query
        .split_whitespace()
        .map(word)
        .filter(|w| !w.is_empty())
        .collect();
    if words.is_empty() {
        return Some(Match {
            score: 0.0,
            marks: Vec::new(),
        });
    }
    let title = Folded::new(&row.title);
    let subtitle = Folded::new(&row.subtitle);
    let keywords: Vec<Folded> = row
        .keywords
        .split_whitespace()
        .chain(row.kind.cluster().split_whitespace())
        .map(Folded::new)
        .collect();
    let mut total = 0.0;
    let mut marks = Vec::new();
    for w in &words {
        let by_title = fuzzy(w, &title);
        let by_keyword = if keywords.iter().any(|k| k.starts_with(w)) {
            KEYWORD
        } else {
            0.0
        };
        let by_subtitle = if subtitle.find(w).is_some() {
            SUBTITLE
        } else {
            0.0
        };
        let title_score = by_title.as_ref().map_or(0.0, |m| m.score);
        let best = title_score.max(by_keyword).max(by_subtitle);
        if best <= 0.0 {
            return None;
        }
        if let Some(m) = by_title
            && m.score >= by_keyword
            && m.score >= by_subtitle
        {
            marks.extend(m.marks);
        }
        total += best;
    }
    marks.sort_unstable();
    marks.dedup();
    Some(Match {
        score: total,
        marks,
    })
}
