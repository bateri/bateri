//! Finding links in plain text (044): URLs and path candidates in one logical
//! line's string.
//!
//! A hand-written scanner, not a regex: trimming trailing punctuation, keeping
//! brackets balanced and splitting off the `:line:col` suffix are rules a regex
//! can't express, and the three surfaces (grid, fill band, dock) must call one
//! function (`.tasks/044-tiklanabilir-baglantilar/discussion.md` → Karar 1).
//!
//! **Syntax only:** whether a path exists is not asked here (no file I/O in
//! `bt-core`); the shell layer resolves candidates off the main thread.
//!
//! Every index is a **char** index, not a byte index: the caller maps chars to
//! cells and a Turkish path would make byte offsets drift.

use std::ops::Range;

/// What a found range is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FoundKind {
    /// A URL with one of the known schemes ([`SCHEMES`]).
    Url,
    /// A path candidate with its optional `:line`, `:line:col` or
    /// `(line,col)` suffix, split off the target.
    Path { line: Option<u32>, col: Option<u32> },
}

/// One link candidate in the scanned string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Found {
    /// The chars the link covers on screen — the suffix included (it is part of
    /// what gets underlined).
    pub(crate) range: Range<usize>,
    /// The target: the URL, or the path **without** its suffix.
    pub(crate) target: String,
    pub(crate) kind: FoundKind,
}

/// The recognized URL schemes (case-insensitive). `file://` is here too; whether
/// its authority is this machine is the caller's question
/// ([`crate::shell::is_local_authority`]).
const SCHEMES: [&str; 5] = ["https://", "http://", "ftp://", "file://", "mailto:"];

/// The chars that end a token besides whitespace: quotes and angle brackets
/// (`<https://x.dev>`, `"~/a b"` is two tokens — escaped spaces are not followed).
fn is_delimiter(c: char) -> bool {
    c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '<' | '>')
}

/// Trailing punctuation that is sentence text, not part of a link.
fn is_trailing_punctuation(c: char) -> bool {
    matches!(c, '.' | ',' | ';' | ':' | '!' | '?')
}

/// Scans `text` for links, left to right; ranges don't overlap.
pub(crate) fn scan(text: &str) -> Vec<Found> {
    let chars: Vec<char> = text.chars().collect();
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
        if let Some(link) = token(&chars, start..at) {
            found.push(link);
        }
    }
    found
}

/// The link in one token, if any.
fn token(chars: &[char], range: Range<usize>) -> Option<Found> {
    if let Some(scheme_at) = scheme_start(chars, range.clone()) {
        return url(chars, scheme_at..range.end);
    }
    path(chars, range)
}

/// Where a scheme starts inside the token: at its start or after a char that
/// can't belong to a scheme name (`(https://…`, `url=https://…`).
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

/// Drops leading **unbalanced** opening brackets: `(src/x.rs` → `src/x.rs`.
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

/// A path candidate: trimmed, its suffix split off, and qualified by shape.
fn path(chars: &[char], range: Range<usize>) -> Option<Found> {
    let mut range = trim_end(chars, range);
    // An enclosing pair is not part of the path: `(src/x.rs)`.
    while range.len() >= 2
        && matches!(
            (chars[range.start], chars[range.end - 1]),
            ('(', ')') | ('[', ']') | ('{', '}')
        )
    {
        range = trim_end(chars, range.start + 1..range.end - 1);
    }
    let range = trim_start(chars, range);
    let (target, line, col) = split_suffix(chars, range.clone());
    let text = collect(chars, target);
    if !looks_like_path(&text) {
        return None;
    }
    Some(Found {
        range,
        target: text,
        kind: FoundKind::Path { line, col },
    })
}

/// A run of ASCII digits as a number; `None` if empty or too large.
fn number(chars: &[char]) -> Option<u32> {
    if chars.is_empty() || !chars.iter().all(char::is_ascii_digit) {
        return None;
    }
    collect(chars, 0..chars.len()).parse().ok()
}

/// Splits `(line,col)`, `(line)`, `:line:col` or `:line` off the end; the
/// target range and the numbers.
fn split_suffix(chars: &[char], range: Range<usize>) -> (Range<usize>, Option<u32>, Option<u32>) {
    let body = &chars[range.clone()];
    // `(line,col)` / `(line)` — MSVC and .NET style.
    if body.last() == Some(&')')
        && let Some(open) = body.iter().rposition(|&c| c == '(')
        && open > 0
    {
        let inner = &body[open + 1..body.len() - 1];
        let (line, col) = match inner.iter().position(|&c| c == ',') {
            Some(comma) => (number(&inner[..comma]), number(&inner[comma + 1..])),
            None => (number(inner), Some(0)),
        };
        if let (Some(line), Some(col)) = (line, col) {
            let col = (inner.contains(&',')).then_some(col);
            return (range.start..range.start + open, Some(line), col);
        }
    }
    // `:line:col` / `:line` — compiler style.
    let mut end = body.len();
    let mut numbers = Vec::new();
    while numbers.len() < 2 {
        let digits_start = body[..end]
            .iter()
            .rposition(|c| !c.is_ascii_digit())
            .map_or(0, |at| at + 1);
        if digits_start == end || digits_start == 0 || body[digits_start - 1] != ':' {
            break;
        }
        match number(&body[digits_start..end]) {
            Some(n) => numbers.push(n),
            None => break,
        }
        end = digits_start - 1;
    }
    let target = range.start..range.start + end;
    match numbers.as_slice() {
        [line] => (target, Some(*line), None),
        [col, line] => (target, Some(*line), Some(*col)),
        _ => (range, None, None),
    }
}

/// Whether the text can be a path at all. Any bare word is a candidate
/// (`src`, `Makefile`, `main.rs`) — iTerm2's semantic history rule: the shape
/// doesn't decide, the existence check does (`bt-shell-common::links::resolve`,
/// nothing unverified is underlined). What's left out can never name a file
/// worth opening: only separators (`/`, `..`) and a URL-looking token with an
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
    }

    #[test]
    fn two_links_separated_by_a_space_are_two() {
        assert_eq!(
            found("https://a.dev https://b.dev"),
            vec![url_of("https://a.dev"), url_of("https://b.dev")]
        );
        let chars: Vec<char> = "https://a.dev https://b.dev".chars().collect();
        let ranges: Vec<_> = scan(&chars.iter().collect::<String>())
            .into_iter()
            .map(|f| f.range)
            .collect();
        assert_eq!(ranges, vec![0..13, 14..27]);
    }

    #[test]
    fn compiler_suffixes_are_split_off_the_target() {
        assert_eq!(
            found("src/main.rs:12:5:"),
            vec![(
                "src/main.rs:12:5".to_owned(),
                "src/main.rs".to_owned(),
                FoundKind::Path {
                    line: Some(12),
                    col: Some(5)
                }
            )]
        );
        assert_eq!(
            found("src/main.rs:12"),
            vec![(
                "src/main.rs:12".to_owned(),
                "src/main.rs".to_owned(),
                FoundKind::Path {
                    line: Some(12),
                    col: None
                }
            )]
        );
    }

    #[test]
    fn the_parenthesized_suffix_is_split_off_the_target() {
        assert_eq!(
            found("Program.cs(12,5):"),
            vec![(
                "Program.cs(12,5)".to_owned(),
                "Program.cs".to_owned(),
                FoundKind::Path {
                    line: Some(12),
                    col: Some(5)
                }
            )]
        );
        assert_eq!(
            found("Program.cs(12)"),
            vec![(
                "Program.cs(12)".to_owned(),
                "Program.cs".to_owned(),
                FoundKind::Path {
                    line: Some(12),
                    col: None
                }
            )]
        );
    }

    #[test]
    fn path_shapes() {
        let path = |text: &str| {
            (
                text.to_owned(),
                text.to_owned(),
                FoundKind::Path {
                    line: None,
                    col: None,
                },
            )
        };
        assert_eq!(found("~/x"), vec![path("~/x")]);
        assert_eq!(
            found("cat /etc/hosts"),
            vec![path("cat"), path("/etc/hosts")]
        );
        assert_eq!(found("./run ../up"), vec![path("./run"), path("../up")]);
        assert_eq!(found("ls foo.txt"), vec![path("ls"), path("foo.txt")]);
        // A bare word is a candidate too (a directory from `ls`, `Makefile`):
        // the existence check, not the shape, decides (iTerm2's rule).
        assert_eq!(
            found("src Makefile 1.5"),
            vec![path("src"), path("Makefile"), path("1.5")]
        );
        assert_eq!(
            found("e."),
            vec![path("e")],
            "sentence punctuation is trimmed"
        );
        assert_eq!(found("(src/x.rs)"), vec![path("src/x.rs")]);
        assert_eq!(
            found("~/Belgeler/çalışma ağacı.txt"),
            vec![path("~/Belgeler/çalışma"), path("ağacı.txt")],
            "a space ends a token; escaped spaces are not followed"
        );
        assert_eq!(
            found("~/Belgeler/çalışma.txt"),
            vec![path("~/Belgeler/çalışma.txt")]
        );
    }

    #[test]
    fn non_paths_are_not_candidates() {
        for text in ["/", "..", ".", "./", "vscode://x/y", ""] {
            assert_eq!(found(text), vec![], "{text:?}");
        }
    }

    #[test]
    fn the_chars_index_turkish_text_not_bytes() {
        let text = "ğüş, https://a.dev/ç";
        let hits = scan(text);
        assert_eq!(hits.len(), 2, "the bare word is a candidate too");
        assert_eq!(hits[1].range, 5..20);
        assert_eq!(hits[1].target, "https://a.dev/ç");
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
