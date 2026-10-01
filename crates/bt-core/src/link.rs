//! Finding links in plain text (044): the URL or the path candidates under a
//! point of one logical line's string.
//!
//! A hand-written scanner, not a regex: trimming trailing punctuation, keeping
//! brackets balanced and splitting off the `:line:col` suffix are rules a regex
//! can't express, and the three surfaces (grid, fill band, dock) must call one
//! function (`.tasks/044-tiklanabilir-baglantilar/discussion.md` → Karar 1).
//!
//! **A URL is one token** ([`links_at`]): whitespace ends it. **A path is not**: a
//! name with spaces (`My Drive`, `4.04.2022 06.29.36.pklg`) is found the way
//! iTerm2's semantic history finds it ([`path_candidates`], set sonrası —
//! `phase-1.md` → Uygulama Notları): the text around the point is cut into
//! chunks at `\t ():",`, and growing combinations of chunks — rightwards first,
//! then one more chunk to the left — are the candidates, shortest first. The
//! first one that **exists** wins.
//!
//! **Syntax only:** whether a path exists is not asked here (no file I/O in
//! `bt-core`); the shell layer resolves the candidates off the main thread.
//!
//! Every index is a **char** index, not a byte index: the caller maps chars to
//! cells and a Turkish path would make byte offsets drift.

use std::collections::HashSet;
use std::ops::Range;

/// What a found range is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FoundKind {
    /// A URL with one of the known schemes ([`SCHEMES`]).
    Url,
    /// A path candidate with its optional line/column suffix (`:line:col`,
    /// `(line,col)`, Python's `", line N`, …), split off the target.
    Path { line: Option<u32>, col: Option<u32> },
}

/// One link candidate in the scanned string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Found {
    /// The chars the link covers on screen — the suffix included (it is part of
    /// what gets underlined).
    pub(crate) range: Range<usize>,
    /// The target: the URL, or the path **without** its suffix (escapes undone).
    pub(crate) target: String,
    pub(crate) kind: FoundKind,
}

/// The recognized URL schemes (case-insensitive). `file://` is here too; whether
/// its authority is this machine is the caller's question
/// ([`crate::shell::is_local_authority`]).
const SCHEMES: [&str; 5] = ["https://", "http://", "ftp://", "file://", "mailto:"];

/// How many chars on each side of the point a path may reach — iTerm2's
/// `maxSemanticHistoryPrefixOrSuffix`.
const CONTEXT: usize = 2000;

/// How many distinct chunk combinations are tried — iTerm2's bail-out.
const MAX_TRIES: usize = 100;

/// How many candidates (variants included) one point yields: the shell layer
/// `stat`s each, so this is its upper bound per hover.
const MAX_CANDIDATES: usize = 100;

/// How many chunks the right side grows by before one more is taken from the
/// left — iTerm2's "do not starve the leftward search".
const MAX_RIGHT_CHUNKS: usize = 10;

/// The trailing text a program may print after a file name (`see foo.txt.`):
/// each is tried stripped too, the unstripped form first — iTerm2's
/// "questionable suffixes".
const QUESTIONABLE_SUFFIXES: [&str; 8] = ["!", "?", ".", ",", ";", ":", "...", "…"];

/// The chars that end a URL token besides whitespace: quotes and angle brackets
/// (`<https://x.dev>`).
fn is_delimiter(c: char) -> bool {
    c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '<' | '>')
}

/// Trailing punctuation that is sentence text, not part of a URL.
fn is_trailing_punctuation(c: char) -> bool {
    matches!(c, '.' | ',' | ';' | ':' | '!' | '?')
}

/// The URLs in `text`, left to right; ranges don't overlap ([`links_at`]'s
/// first question, here for the tests).
#[cfg(test)]
fn scan(text: &str) -> Vec<Found> {
    let chars: Vec<char> = text.chars().collect();
    scan_chars(&chars)
}

fn scan_chars(chars: &[char]) -> Vec<Found> {
    let mut found = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        if is_delimiter(chars[at]) {
            at += 1;
            continue;
        }
        let start = at;
        while at < chars.len() && !is_delimiter(chars[at]) {
            at += 1;
        }
        if let Some(link) = scheme_start(chars, start..at).and_then(|from| url(chars, from..at)) {
            found.push(link);
        }
    }
    found
}

/// The links under char `at` of `text`, in the order they are to be tried: a
/// URL covering `at` alone; otherwise the path candidates
/// ([`path_candidates`]) that don't overlap a URL — none if the token under `at` looks like a URL with
/// an unknown scheme (`vscode://x/y` is not a path, nor is its `//x/y`).
pub(crate) fn links_at(text: &str, at: usize) -> Vec<Found> {
    let chars: Vec<char> = text.chars().collect();
    if at >= chars.len() {
        return Vec::new();
    }
    let urls = scan_chars(&chars);
    if let Some(url) = urls.iter().find(|found| found.range.contains(&at)) {
        return vec![url.clone()];
    }
    let start = chars[..at]
        .iter()
        .rposition(|&c| is_delimiter(c))
        .map_or(0, |i| i + 1);
    let end = chars[at..]
        .iter()
        .position(|&c| is_delimiter(c))
        .map_or(chars.len(), |i| at + i);
    if contains_scheme_mark(&chars[start..end]) {
        return Vec::new();
    }
    // A name never runs into a URL (the blank before `see https://…`).
    let mut paths = path_candidates(&chars, at);
    paths.retain(|path| {
        urls.iter()
            .all(|url| url.range.end <= path.range.start || path.range.end <= url.range.start)
    });
    paths
}

fn contains_scheme_mark(chars: &[char]) -> bool {
    chars.windows(3).any(|w| w == [':', '/', '/'])
}

/// Whether `c` can be part of a file name — iTerm2's `filenameCharacterSet`:
/// whitespace (not a line break), letters and digits of any script and the URL
/// punctuation `.?\/:;$%=&_-,+~#@!*'()|[]`. Not `"`, `<>`, `` ` ``, `{}`, `^`.
fn is_filename_char(c: char) -> bool {
    if matches!(c, '\n' | '\r') {
        return false;
    }
    c.is_whitespace()
        || c.is_alphanumeric()
        || (!c.is_ascii() && !c.is_control())
        || ".?\\/:;$%=&_-,+~#@!*'()|[]".contains(c)
}

/// The suffix side also takes `"`: Python's `File "x.py", line 12` (iTerm2).
fn is_suffix_char(c: char) -> bool {
    is_filename_char(c) || c == '"'
}

/// The chars a name is cut at — iTerm2's `splitString` set.
fn is_separator(c: char) -> bool {
    matches!(c, '\t' | ' ' | '(' | ')' | ':' | '"' | ',')
}

/// iTerm2's `splitString`: `([^\t ():",]*)([\t ():",])` repeated, the word and
/// its separator each a chunk (the word may be empty), and the trailing word
/// last. `"My Drive"` → `My`, ` `, `Drive`.
fn chunks(chars: &[char], range: Range<usize>) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut word = range.start;
    for at in range.clone() {
        if is_separator(chars[at]) {
            out.push(word..at);
            out.push(at..at + 1);
            word = at + 1;
        }
    }
    if word < range.end {
        out.push(word..range.end);
    }
    out
}

/// The path candidates under char `at`, in iTerm2's order
/// (`iTermPathFinder.searchSynchronously`; `discussion.md` and
/// `phase-1.md` → Uygulama Notları, set sonrası):
///
/// 1. The run of file-name chars around `at` ([`is_filename_char`]), at most
///    [`CONTEXT`] chars each side, is cut at the point into a left and a right
///    part and each into [`chunks`].
/// 2. `left` starts empty and takes one more chunk from the left per round
///    (the last round takes all); in each round `right` grows chunk by chunk,
///    at most [`MAX_RIGHT_CHUNKS`]. `left + right` is a candidate; a repeated
///    one is skipped; [`MAX_TRIES`] distinct ones end the search.
/// 3. `\ `, `\(`, `\[`, `\]`, `\\`, `\)` are unescaped; the candidate and its
///    [`QUESTIONABLE_SUFFIXES`]-stripped forms follow, unstripped first. A
///    line/column suffix right after the candidate's chunks
///    ([`line_col`]) is split off into the kind and covered by the range.
///
/// So the shortest combination that exists wins: `ls`'s `My Drive` beside
/// `Screen Studio Projects` is found without swallowing its neighbour column.
///
/// Three choices of ours on top (Uygulama Notları → SAPMA):
/// - **The point is word-aligned**: the cut is at the start of the chunk under
///   `at`, not at `at` itself — iTerm2 would try the partial word (`rive` of
///   `Drive`), and every cell of a word would give another list (another `stat`
///   batch, a flickering hover).
/// - **A candidate must cover `at`** and not be blank: the hover underlines
///   what is under the pointer, and a click is matched against those cells.
/// - **The stripped bracket form is the last variant**: `[src/x.rs]` keeps
///   working (`[`/`]` are file-name chars, `(`/`)` are separators already).
///
/// At most [`MAX_CANDIDATES`] come back, a target never twice (an earlier
/// equal one would already have won).
pub(crate) fn path_candidates(chars: &[char], at: usize) -> Vec<Found> {
    let Some(&under) = chars.get(at) else {
        return Vec::new();
    };
    if !is_suffix_char(under) {
        return Vec::new();
    }
    let floor = at.saturating_sub(CONTEXT);
    let mut start = at;
    while start > floor && is_filename_char(chars[start - 1]) {
        start -= 1;
    }
    let ceiling = (at + CONTEXT).min(chars.len());
    let mut end = at;
    while end < ceiling && is_suffix_char(chars[end]) {
        end += 1;
    }
    let mut split = at;
    if !is_separator(under) {
        while split > start && !is_separator(chars[split - 1]) {
            split -= 1;
        }
    }
    let before = chunks(chars, start..split);
    let after = chunks(chars, split..end);
    let mut out = Candidates {
        found: Vec::new(),
        targets: HashSet::new(),
        at,
    };
    let mut seen = HashSet::new();
    let mut tries = MAX_TRIES;
    let rights = after.len().clamp(1, MAX_RIGHT_CHUNKS);
    'search: for i in (0..=before.len()).rev() {
        let left = before.get(i).map_or(split, |chunk| chunk.start);
        for j in 0..rights {
            let right = after.get(j).map_or(split, |chunk| chunk.end);
            if !seen.insert((left, right)) {
                continue;
            }
            let suffix = (j + 1 < after.len())
                .then(|| line_col(&chars[right..end]))
                .flatten();
            if !out.variants(chars, left..right, suffix) {
                break 'search;
            }
            tries -= 1;
            if tries == 0 {
                break 'search;
            }
        }
    }
    out.found
}

/// The candidate list being built: the point every range must cover and the
/// targets already in it.
struct Candidates {
    found: Vec<Found>,
    targets: HashSet<String>,
    at: usize,
}

impl Candidates {
    /// One chunk combination's variants (step 3 of [`path_candidates`]);
    /// `false` once the list is full.
    fn variants(&mut self, chars: &[char], range: Range<usize>, suffix: Option<Suffix>) -> bool {
        if chars[range.clone()].iter().all(|c| c.is_whitespace()) {
            return true;
        }
        let text = unescape(&collect(chars, range.clone()));
        let (line, col) = suffix.map_or((None, None), |s| (s.line, s.col));
        let extra = suffix.map_or(0, |s| s.len);
        let mut forms = vec![(text.clone(), 0)];
        for bad in QUESTIONABLE_SUFFIXES {
            if let Some(stripped) = text.strip_suffix(bad)
                && !stripped.is_empty()
            {
                forms.push((stripped.to_owned(), bad.chars().count()));
            }
        }
        for (target, dropped) in forms {
            // iTerm2: with a line/column suffix the stripped text stays covered.
            let covered = match suffix {
                Some(_) => range.start..range.end + extra,
                None => range.start..range.end - dropped,
            };
            if !self.push(covered, target, line, col) {
                return false;
            }
        }
        // The point on the suffix itself (`12` of `src/main.rs:12`): every
        // combination then holds the suffix, so it is cut off the candidate's
        // own end too — iTerm2's path cleaner does the same.
        if suffix.is_none()
            && let Some((cut, inner)) = trailing_suffix(&chars[range.clone()])
        {
            let target = unescape(&collect(chars, range.start..range.start + cut));
            if !self.push(range.clone(), target, inner.line, inner.col) {
                return false;
            }
        }
        let cleaned = clean_brackets(chars, range.clone());
        if cleaned != range && !cleaned.is_empty() {
            let target = unescape(&collect(chars, cleaned.clone()));
            // A suffix after the brackets' name stays its own: `[src/x.rs:12]`.
            let covered = match suffix {
                Some(_) => cleaned.start..range.end + extra,
                None => cleaned,
            };
            return self.push(covered, target, line, col);
        }
        true
    }

    /// Adds a candidate unless it doesn't cover the point, can't be a path or
    /// is already in; `false` once the list is full.
    fn push(
        &mut self,
        range: Range<usize>,
        target: String,
        line: Option<u32>,
        col: Option<u32>,
    ) -> bool {
        if self.found.len() >= MAX_CANDIDATES {
            return false;
        }
        if range.contains(&self.at)
            && looks_like_path(&target)
            && self.targets.insert(target.clone())
        {
            self.found.push(Found {
                range,
                target,
                kind: FoundKind::Path { line, col },
            });
        }
        self.found.len() < MAX_CANDIDATES
    }
}

/// iTerm2's escape removal: `\x` → `x` for `x` in ` ([])\`.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match chars.peek() {
            Some(&next) if c == '\\' && matches!(next, ' ' | '(' | '[' | ']' | '\\' | ')') => {
                out.push(next);
                chars.next();
            }
            _ => out.push(c),
        }
    }
    out
}

/// A line/column suffix after a candidate: how many chars it covers and the
/// numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Suffix {
    len: usize,
    line: Option<u32>,
    col: Option<u32>,
}

/// One piece of a suffix pattern.
enum Piece {
    Lit(&'static str),
    Num,
    /// An optional space.
    Space,
    /// The end of the run.
    End,
}

/// iTerm2's `columnAndLineNumberFromChunks` regexes, in its order: `:12:5`,
/// `:12`, `[12, 5]`, `", line 12, column 5`, `", line 12, in`, `(12, 5)`,
/// `(12)`, ` line 12:` at the end.
const SUFFIXES: [&[Piece]; 8] = {
    use Piece::{End, Lit, Num, Space};
    [
        &[Lit(":"), Num, Lit(":"), Num],
        &[Lit(":"), Num],
        &[Lit("["), Num, Lit(","), Space, Num, Lit("]")],
        &[Lit("\", line "), Num, Lit(", column "), Num],
        &[Lit("\", line "), Num, Lit(", in")],
        &[Lit("("), Num, Lit(","), Space, Num, Lit(")")],
        &[Lit("("), Num, Lit(")")],
        &[Lit(" line "), Num, Lit(":"), End],
    ]
};

/// A [`SUFFIXES`] pattern that ends `text` exactly, starting as early as
/// possible: where the name ends and the suffix. `None` without one or if
/// nothing would be left of the name.
fn trailing_suffix(text: &[char]) -> Option<(usize, Suffix)> {
    (1..text.len()).find_map(|cut| {
        line_col(&text[cut..])
            .filter(|suffix| suffix.len == text.len() - cut)
            .map(|suffix| (cut, suffix))
    })
}

/// The first [`SUFFIXES`] pattern `rest` starts with.
fn line_col(rest: &[char]) -> Option<Suffix> {
    SUFFIXES.iter().find_map(|pattern| suffix_of(rest, pattern))
}

fn suffix_of(rest: &[char], pattern: &[Piece]) -> Option<Suffix> {
    let mut at = 0;
    let mut numbers = Vec::new();
    for piece in pattern {
        match piece {
            Piece::Lit(text) => {
                for want in text.chars() {
                    if rest.get(at) != Some(&want) {
                        return None;
                    }
                    at += 1;
                }
            }
            Piece::Num => {
                let start = at;
                while rest.get(at).is_some_and(char::is_ascii_digit) {
                    at += 1;
                }
                if at == start {
                    return None;
                }
                numbers.push(collect(rest, start..at).parse::<u32>().ok()?);
            }
            Piece::Space => {
                if rest.get(at) == Some(&' ') {
                    at += 1;
                }
            }
            Piece::End => {
                if at != rest.len() {
                    return None;
                }
            }
        }
    }
    Some(Suffix {
        len: at,
        line: numbers.first().copied(),
        col: numbers.get(1).copied(),
    })
}

fn scheme_start(chars: &[char], range: Range<usize>) -> Option<usize> {
    (range.start..range.end).find(|&at| {
        let boundary = at == range.start || !chars[at - 1].is_alphanumeric();
        boundary
            && SCHEMES
                .iter()
                .any(|scheme| starts_with_ignore_case(&chars[at..range.end], scheme))
    })
}

fn starts_with_ignore_case(chars: &[char], prefix: &str) -> bool {
    let mut rest = chars.iter();
    prefix
        .chars()
        .all(|want| rest.next().is_some_and(|c| c.eq_ignore_ascii_case(&want)))
}

/// Count of a char in a range.
fn count(chars: &[char], range: &Range<usize>, c: char) -> usize {
    chars[range.clone()].iter().filter(|&&x| x == c).count()
}

/// Trims trailing punctuation and **unbalanced** closing brackets from the end:
/// `https://x.dev).` loses `).`, `…/wiki/X_(Y)` keeps its `)`.
fn trim_end(chars: &[char], mut range: Range<usize>) -> Range<usize> {
    while range.end > range.start {
        let last = chars[range.end - 1];
        let drop = match last {
            ')' => count(chars, &range, '(') < count(chars, &range, ')'),
            ']' => count(chars, &range, '[') < count(chars, &range, ']'),
            '}' => count(chars, &range, '{') < count(chars, &range, '}'),
            c => is_trailing_punctuation(c),
        };
        if !drop {
            break;
        }
        range.end -= 1;
    }
    range
}

/// Drops leading **unbalanced** opening brackets: `[src/x.rs` → `src/x.rs`.
fn trim_start(chars: &[char], mut range: Range<usize>) -> Range<usize> {
    while range.start < range.end {
        let first = chars[range.start];
        let drop = match first {
            '(' => count(chars, &range, '(') > count(chars, &range, ')'),
            '[' => count(chars, &range, '[') > count(chars, &range, ']'),
            '{' => count(chars, &range, '{') > count(chars, &range, '}'),
            _ => false,
        };
        if !drop {
            break;
        }
        range.start += 1;
    }
    range
}

/// A candidate without its enclosing pair and unbalanced brackets
/// (`[src/x.rs]` → `src/x.rs`) — the single-token rule this module had before
/// the iTerm2 search, kept as the last variant.
fn clean_brackets(chars: &[char], range: Range<usize>) -> Range<usize> {
    let mut range = trim_end(chars, range);
    while range.len() >= 2
        && matches!(
            (chars[range.start], chars[range.end - 1]),
            ('(', ')') | ('[', ']') | ('{', '}')
        )
    {
        range = trim_end(chars, range.start + 1..range.end - 1);
    }
    trim_start(chars, range)
}

fn collect(chars: &[char], range: Range<usize>) -> String {
    chars[range].iter().collect()
}

/// A URL from its scheme to the token's end; the scheme alone is not a link.
fn url(chars: &[char], range: Range<usize>) -> Option<Found> {
    let range = trim_end(chars, range);
    let scheme = SCHEMES
        .iter()
        .find(|scheme| starts_with_ignore_case(&chars[range.clone()], scheme))?;
    if range.len() <= scheme.len() {
        return None;
    }
    Some(Found {
        target: collect(chars, range.clone()),
        range,
        kind: FoundKind::Url,
    })
}

/// Whether the text can be a path at all. Any word is a candidate (`src`,
/// `Makefile`, `main.rs`) — iTerm2's semantic history rule: the shape doesn't
/// decide, the existence check does (`bt-shell-common::links::resolve_first`,
/// nothing unverified is underlined). What's left out can never name a file
/// worth opening: only separators (`/`, `..`) and a URL-looking text with an
/// unknown scheme (`vscode://…`).
fn looks_like_path(text: &str) -> bool {
    !text.is_empty() && !text.contains("://") && !text.chars().all(|c| matches!(c, '/' | '.'))
}

/// The authority of a `file://` URL (`file://AUTH/path` → `AUTH`); `None` if
/// the URL isn't `file://` or has no path.
pub(crate) fn file_authority(url: &str) -> Option<&str> {
    let rest = url
        .get(..7)
        .filter(|head| head.eq_ignore_ascii_case("file://"))?;
    let rest = &url[rest.len()..];
    let slash = rest.find('/')?;
    Some(&rest[..slash])
}

/// Whether the URL is a `file://` URL.
pub(crate) fn is_file_url(url: &str) -> bool {
    url.get(..7)
        .is_some_and(|head| head.eq_ignore_ascii_case("file://"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The covered texts and kinds — the readable form of `scan`'s answer.
    fn found(text: &str) -> Vec<(String, String, FoundKind)> {
        let chars: Vec<char> = text.chars().collect();
        scan(text)
            .into_iter()
            .map(|f| (collect(&chars, f.range), f.target, f.kind))
            .collect()
    }

    fn url_of(text: &str) -> (String, String, FoundKind) {
        (text.to_owned(), text.to_owned(), FoundKind::Url)
    }

    /// The char index of `needle`'s first occurrence in `text`.
    fn index_of(text: &str, needle: &str) -> usize {
        let byte = text.find(needle).expect("needle in text");
        text[..byte].chars().count()
    }

    /// The candidates under `needle`'s first char.
    fn under(text: &str, needle: &str) -> Vec<Found> {
        links_at(text, index_of(text, needle))
    }

    /// Just the targets, in order.
    fn targets(text: &str, needle: &str) -> Vec<String> {
        under(text, needle).into_iter().map(|f| f.target).collect()
    }

    /// The candidate whose target is `target`, with its covered text.
    fn covering(text: &str, needle: &str, target: &str) -> (String, FoundKind) {
        let chars: Vec<char> = text.chars().collect();
        let found = under(text, needle)
            .into_iter()
            .find(|f| f.target == target)
            .unwrap_or_else(|| panic!("{target:?} is a candidate"));
        (collect(&chars, found.range), found.kind)
    }

    fn position(list: &[String], target: &str) -> usize {
        list.iter()
            .position(|t| t == target)
            .unwrap_or_else(|| panic!("{target:?} in {list:?}"))
    }

    const PLAIN: FoundKind = FoundKind::Path {
        line: None,
        col: None,
    };

    #[test]
    fn trailing_punctuation_and_an_unbalanced_paren_are_trimmed() {
        assert_eq!(found("(https://x.dev)."), vec![url_of("https://x.dev")]);
        assert_eq!(found("https://x.dev,"), vec![url_of("https://x.dev")]);
    }

    #[test]
    fn balanced_parens_stay_in_the_url() {
        let wiki = "https://tr.wikipedia.org/wiki/X_(Y)";
        assert_eq!(found(&format!("({wiki})")), vec![url_of(wiki)]);
        assert_eq!(found(&format!("[{wiki}]")), vec![url_of(wiki)]);
    }

    #[test]
    fn schemes_are_recognized_case_insensitively() {
        assert_eq!(found("mailto:a@b.dev"), vec![url_of("mailto:a@b.dev")]);
        assert_eq!(found("ftp://f.dev/x"), vec![url_of("ftp://f.dev/x")]);
        assert_eq!(found("file:///tmp/a"), vec![url_of("file:///tmp/a")]);
        assert_eq!(found("HTTPS://X.DEV"), vec![url_of("HTTPS://X.DEV")]);
        assert_eq!(
            found("url=https://x.dev/a?b=1"),
            vec![url_of("https://x.dev/a?b=1")]
        );
        assert_eq!(found("https://"), vec![], "a bare scheme is not a link");
        assert_eq!(found("<https://x.dev>"), vec![url_of("https://x.dev")]);
        assert_eq!(found("ls foo.txt"), vec![], "scan finds URLs only");
    }

    #[test]
    fn two_links_separated_by_a_space_are_two() {
        assert_eq!(
            found("https://a.dev https://b.dev"),
            vec![url_of("https://a.dev"), url_of("https://b.dev")]
        );
        let ranges: Vec<_> = scan("https://a.dev https://b.dev")
            .into_iter()
            .map(|f| f.range)
            .collect();
        assert_eq!(ranges, vec![0..13, 14..27]);
    }

    #[test]
    fn a_url_under_the_point_beats_the_paths() {
        let text = "see https://x.dev/a b";
        let hits = under(text, "x.dev");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].kind, FoundKind::Url);
        assert_eq!(hits[0].target, "https://x.dev/a");
        // The word next to it is a path candidate of its own.
        assert_eq!(targets(text, "see")[0], "see");
    }

    #[test]
    fn a_name_with_spaces_is_found_shortest_first() {
        // `ls`'s two columns: `My Drive` and `Screen Studio Projects`.
        let line = "My Drive    Screen Studio Projects";
        let studio = targets(line, "Studio");
        assert_eq!(
            studio[..3],
            ["Studio", "Studio ", "Studio Projects"],
            "rightwards first"
        );
        assert_eq!(studio[3], " Studio", "then one chunk to the left");
        let whole = position(&studio, "Screen Studio Projects");
        assert!(position(&studio, "Screen Studio") < whole);
        // The neighbour column comes only after the whole name: the first
        // existing candidate never swallows it while the name exists.
        let first_with_drive = studio
            .iter()
            .position(|t| t.contains("Drive"))
            .expect("the search reaches left");
        assert!(whole < first_with_drive);
        assert_eq!(
            covering(line, "Studio", "Screen Studio Projects"),
            ("Screen Studio Projects".to_owned(), PLAIN)
        );

        let drive = targets(line, "Drive");
        assert_eq!(drive[0], "Drive");
        let my_drive = position(&drive, "My Drive");
        assert!(position(&drive, "My Drive    Screen") > my_drive);
        assert_eq!(
            covering(line, "Drive", "My Drive"),
            ("My Drive".to_owned(), PLAIN)
        );
        // The same list from every cell of a word (the point is word-aligned).
        let mid = links_at(line, index_of(line, "Drive") + 3);
        assert_eq!(mid, under(line, "Drive"));
        // Every candidate covers the point.
        let at = index_of(line, "Studio");
        assert!(under(line, "Studio").iter().all(|f| f.range.contains(&at)));
    }

    #[test]
    fn a_dated_file_name_is_one_candidate() {
        let line = "4.04.2022 06.29.36.pklg  notes.txt";
        let from_time = targets(line, "06");
        assert_eq!(from_time[0], "06.29.36.pklg");
        assert!(from_time.contains(&"4.04.2022 06.29.36.pklg".to_owned()));
        let from_date = targets(line, "4.04");
        assert_eq!(
            from_date[..3],
            ["4.04.2022", "4.04.2022 ", "4.04.2022 06.29.36.pklg"]
        );
        assert_eq!(
            covering(line, "4.04", "4.04.2022 06.29.36.pklg"),
            ("4.04.2022 06.29.36.pklg".to_owned(), PLAIN)
        );
    }

    #[test]
    fn the_point_on_a_space_finds_the_names_through_it() {
        let line = "My Drive";
        let at = index_of(line, " ");
        let hits = links_at(line, at);
        assert!(hits.iter().all(|f| f.range.contains(&at)));
        assert!(hits.iter().any(|f| f.target == "My Drive"));
        assert!(!hits.iter().any(|f| f.target == "My" || f.target == "Drive"));
    }

    #[test]
    fn compiler_suffixes_are_split_off_the_target() {
        let line = "foo.txt:12:5: error";
        assert_eq!(targets(line, "foo")[0], "foo.txt");
        assert_eq!(
            covering(line, "foo", "foo.txt"),
            (
                "foo.txt:12:5".to_owned(),
                FoundKind::Path {
                    line: Some(12),
                    col: Some(5)
                }
            )
        );
        assert_eq!(
            covering("at src/main.rs:12", "src", "src/main.rs"),
            (
                "src/main.rs:12".to_owned(),
                FoundKind::Path {
                    line: Some(12),
                    col: None
                }
            )
        );
        // The point on the suffix finds the file too, the suffix underlined.
        for needle in ["12", "5:"] {
            assert_eq!(
                covering(line, needle, "foo.txt"),
                (
                    "foo.txt:12:5".to_owned(),
                    FoundKind::Path {
                        line: Some(12),
                        col: Some(5)
                    }
                ),
                "{needle}"
            );
        }
        // A name that really ends in `:12` is still tried as written.
        assert!(targets("a:12", "a").contains(&"a:12".to_owned()));
    }

    #[test]
    fn the_other_suffixes_of_iterm2() {
        let five = FoundKind::Path {
            line: Some(12),
            col: Some(5),
        };
        let line_only = FoundKind::Path {
            line: Some(12),
            col: None,
        };
        assert_eq!(
            covering("Program.cs(12,5): warning", "Program", "Program.cs"),
            ("Program.cs(12,5)".to_owned(), five.clone())
        );
        assert_eq!(
            covering("Program.cs(12)", "Program", "Program.cs"),
            ("Program.cs(12)".to_owned(), line_only.clone())
        );
        let python = "  File \"/x/app.py\", line 12, in <module>";
        assert_eq!(
            covering(python, "/x/", "/x/app.py"),
            ("/x/app.py\", line 12, in".to_owned(), line_only)
        );
    }

    #[test]
    fn escaped_spaces_are_unescaped() {
        let line = r"cd My\ Drive";
        assert_eq!(
            covering(line, "My", "My Drive"),
            (r"My\ Drive".to_owned(), PLAIN)
        );
        assert_eq!(unescape(r"a\(b\)\[c\]\\d\x"), r"a(b)[c]\d\x");
    }

    #[test]
    fn questionable_suffixes_are_tried_after_the_text() {
        assert_eq!(targets("see e.", "e.")[..3], ["e.", "e", " e."]);
        // The stripped form must still cover the point.
        assert_eq!(targets("e.", "."), ["e."]);
        assert_eq!(covering("foo.txt...", "foo", "foo.txt").0, "foo.txt");
    }

    #[test]
    fn brackets_are_not_part_of_the_name() {
        assert_eq!(targets("(src/x.rs)", "src")[0], "src/x.rs");
        assert_eq!(
            covering("(src/x.rs)", "src", "src/x.rs").0,
            "src/x.rs",
            "`(` and `)` are separators"
        );
        assert!(targets("[src/x.rs]", "src").contains(&"src/x.rs".to_owned()));
        assert_eq!(covering("[src/x.rs]", "src", "src/x.rs").0, "src/x.rs");
        assert_eq!(
            covering("[src/x.rs:12]", "src", "src/x.rs"),
            (
                "src/x.rs:12".to_owned(),
                FoundKind::Path {
                    line: Some(12),
                    col: None
                }
            )
        );
        assert_eq!(targets("{a}", "a"), ["a"], "`{{` ends the run");
    }

    #[test]
    fn non_paths_are_not_candidates() {
        for text in ["/", "..", ".", "vscode://x/y", "<"] {
            assert_eq!(links_at(text, 0), vec![], "{text:?}");
        }
        assert_eq!(links_at("", 0), vec![]);
        assert_eq!(targets("vscode://x/y z", "x/y"), Vec::<String>::new());
    }

    #[test]
    fn the_search_is_bounded() {
        let line = vec!["w"; 400].join(" ");
        let hits = links_at(&line, line.chars().count() / 2);
        assert!(!hits.is_empty() && hits.len() <= MAX_CANDIDATES);
        let long = "x".repeat(5000);
        let hits = links_at(&long, 2500);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].range, 500..4500, "{CONTEXT} chars each side");
    }

    #[test]
    fn the_chars_index_turkish_text_not_bytes() {
        let text = "ğüş, https://a.dev/ç";
        let hits = links_at(text, 6);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].range, 5..20);
        assert_eq!(hits[0].target, "https://a.dev/ç");
        let paths = links_at("~/Belgeler/çalışma ağacı.txt", 3);
        assert_eq!(paths[0].target, "~/Belgeler/çalışma");
        assert!(
            paths
                .iter()
                .any(|f| f.target == "~/Belgeler/çalışma ağacı.txt" && f.range == (0..28))
        );
    }

    #[test]
    fn file_authorities() {
        assert_eq!(file_authority("file:///tmp/a"), Some(""));
        assert_eq!(file_authority("FILE://host/tmp/a"), Some("host"));
        assert_eq!(file_authority("file://host"), None);
        assert_eq!(file_authority("https://x.dev/"), None);
        assert!(is_file_url("file:///x"));
        assert!(!is_file_url("https://x"));
    }
}
