//! Clustering of emoji sequences (035): does a code point extend the open
//! cluster, and how many columns does the cluster take.
//!
//! **The single authority.** The grid's wrapper ([`crate::handler`]), the
//! dock's layout (`dock::layout_with`), the suppression's grid walk
//! (`dock::grid_span`) and the mirror half of the freshness gate
//! (`last_ink`) ask only the two functions here. The day the two rules
//! diverge, the dock shifts by a column or the freshness gate says "stale"
//! permanently, and the symptom is silent (the walk's counterpart of 024
//! Karar 1).
//!
//! **The rule comes from the emoji arms only**, not all of UAX #29
//! (`.tasks/035-grapheme-dizileri/discussion.md` → Muhakeme, first item): a
//! general "the table swallowed the sequence" arm also collapsed Arabic `لا`
//! and `⌚︎` into one cluster, which produced two side effects that diverge
//! from a wcwidth-counting shell and want to narrow a wide cell. Non-emoji
//! clusters stay code point by code point as today.
//!
//! **Width only grows.** The grid can take a cell from 1 to 2 but never
//! narrows at any intermediate step ([`width`]): narrowing would mean
//! taking a wide cell back, i.e. rewriting alacritty's special paths. The
//! dock reads the same number from here, so it keeps bit-for-bit the same
//! column as the grid.

use std::num::NonZeroU32;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Zero-width joiner (U+200D). The emoji-presentation code point behind it
/// joins the cluster — the counterpart of UAX #29 GB11.
const ZWJ: char = '\u{200D}';

/// Emoji presentation selector (VS16). [`emoji_capable`]'s question is "does
/// this code point become a two-column emoji with VS16".
const VS16: char = '\u{FE0F}';

/// Regional indicator (RI) — the two halves of a flag pair.
fn is_regional_indicator(c: char) -> bool {
    ('\u{1F1E6}'..='\u{1F1FF}').contains(&c)
}

/// Fitzpatrick skin tone modifier.
fn is_skin_tone(c: char) -> bool {
    ('\u{1F3FB}'..='\u{1F3FF}').contains(&c)
}

/// Can `c` take emoji presentation: `c ++ VS16` is two columns. The `❤`
/// behind a ZWJ is **text** presentation without VS16 and the intermediate
/// string (`…‍❤`) is not in the table; had the criterion been the code
/// point's own width, the ten-code-point kiss would split in two
/// (`discussion.md` → Karar 3, fourth arm).
fn emoji_capable(c: char) -> bool {
    let mut buf = [0u8; 8];
    let len = c.encode_utf8(&mut buf).len();
    let vs16 = VS16.encode_utf8(&mut buf[len..]).len();
    // The buffer is enough for two code points (4 + 3 bytes); `get` is for the
    // no-panic rule.
    buf.get(..len + vs16)
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .is_some_and(|pair| pair.width() == 2)
}

/// Can `c` extend **any** cluster — the prefilter of [`extends`] that does
/// not look at the cluster. The grid's wrapper asks this for every code
/// point and, if the answer is `false`, never reads the head cell: in
/// flowing plain text no more than one table question per code point is
/// paid.
pub(crate) fn may_extend(c: char) -> bool {
    // In ASCII only digits, `#` and `*` can take emoji presentation (keycap
    // `1️⃣`); the rest enter no arm.
    if c.is_ascii() {
        return c.is_ascii_digit() || c == '#' || c == '*';
    }
    UnicodeWidthChar::width(c) == Some(0)
        || is_regional_indicator(c)
        || is_skin_tone(c)
        || emoji_capable(c)
}

/// Does the open cluster `open` (non-empty) extend with `c`. Four arms, no
/// others:
///
/// 1. **Zero width** — alacritty's present `zerowidth` branch (VS16, ZWJ,
///    combiners other than skin tones, tag characters).
/// 2. **An RI behind an unpaired RI** — a flag pair. If the cluster consists
///    of a single RI; a third RI starts a new flag.
/// 3. **A skin tone behind a two-column cluster** — `a🏽` stays two clusters.
/// 4. **A code point that can take emoji presentation behind a ZWJ** —
///    [`emoji_capable`]. In `a‍b`, `b` is a separate cluster.
///
/// A control character (width `None`) enters no arm: a line break always
/// closes the cluster. A cluster with a columnless head (a combiner at the
/// start of the stream or after a line break) does not extend either: there
/// is no such head cell in the grid — the combiner lands on the previous
/// cell — and the ZWJ of `‍👍` would become the head of a two-column
/// cluster.
pub(crate) fn extends(open: &str, c: char) -> bool {
    if open
        .chars()
        .next()
        .is_none_or(|head| crate::dock::column_width(head) == 0)
    {
        return false;
    }
    if UnicodeWidthChar::width(c) == Some(0) {
        return true;
    }
    if is_regional_indicator(c) {
        let mut chars = open.chars();
        return chars.next().is_some_and(is_regional_indicator) && chars.next().is_none();
    }
    if is_skin_tone(c) {
        return width(open) == 2;
    }
    open.ends_with(ZWJ) && emoji_capable(c)
}

/// The columns a cluster takes in the grid: the base character's width, 2 if
/// any prefix of the cluster reached two columns; it never narrows.
///
/// **Every prefix is asked, not just the result:** the grid asks about
/// widening after every extension, and `1` + VS16 + `U+20E3` widens at the
/// VS16 — a rule looking at the final string would miss that step.
///
/// **A single-code-point cluster uses [`crate::dock::column_width`]'s
/// table** (a control character is 1, tab included): `UnicodeWidthStr` says
/// `0` for a tab and the dock's clusterless arithmetic must be preserved bit
/// for bit.
pub(crate) fn width(cluster: &str) -> usize {
    let mut chars = cluster.char_indices();
    let Some((_, head)) = chars.next() else {
        return 0;
    };
    let base = crate::dock::column_width(head);
    if base >= 2 {
        return 2;
    }
    let widened = chars.any(|(at, c)| {
        cluster
            .get(..at + c.len_utf8())
            .is_some_and(|prefix| prefix.width() >= 2)
    });
    if widened { 2 } else { base }
}

/// The walk that splits a stream into clusters: for each cluster the
/// character range in the stream (`start..end`, half-open), its head
/// character and its column count ([`width`]).
///
/// The suppression's grid walk and the freshness gate use it; the dock's
/// layout runs the same loop itself over a tagged stream
/// (`dock::layout_with`), and the grid derives the cluster from the cell —
/// all three read the same two functions ([`extends`], [`width`]).
///
/// **The cluster's text is lazy**: it is built only if the next code point
/// passes [`may_extend`]. In plain text (the walks running per frame, the
/// mirror decoder running per key) there is not even one allocation; on a
/// line with emoji the buffer grows once.
pub(crate) struct Walk {
    open: String,
}

/// [`Walk`]'s per-cluster output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Cluster {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) head: char,
    pub(crate) width: usize,
}

impl Walk {
    pub(crate) fn new() -> Self {
        Self {
            open: String::new(),
        }
    }

    /// Splits `chars` into clusters and hands each to `each`.
    pub(crate) fn run(
        &mut self,
        chars: impl IntoIterator<Item = char>,
        mut each: impl FnMut(Cluster),
    ) {
        let mut chars = chars.into_iter().enumerate().peekable();
        while let Some((start, head)) = chars.next() {
            let mut end = start + 1;
            let mut columns = crate::dock::column_width(head);
            if chars.peek().is_some_and(|&(_, next)| may_extend(next)) {
                self.open.clear();
                self.open.push(head);
                while let Some((at, c)) = chars.next_if(|&(_, next)| extends(&self.open, next)) {
                    self.open.push(c);
                    end = at + 1;
                }
                columns = width(&self.open);
            }
            each(Cluster {
                start,
                end,
                head,
                width: columns,
            });
        }
    }
}

/// A cluster's identity in a frame — its index in [`Clusters`].
///
/// `NonZeroU32` (index + 1): `Option<ClusterId>` is 4 bytes via the niche, so
/// the boundary [`crate::Cell`] carries the same pattern in a clusterless
/// cell too and a clusterless cell does not even pay for one extra branch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClusterId(NonZeroU32);

/// The per-frame cluster table (035 Karar 4B): the boundary cell's
/// [`crate::Cell::cluster`] points at a string here.
///
/// **The owner is the drawing side, the filler is [`crate::Session`]** — the
/// [`crate::SelectionRuns`] precedent: the table is the `&mut` argument of
/// `frame()` and `dock()`, and the caller keeps and clears it together with
/// its lists. Putting a string in the cell (4A) would grow every drawn cell
/// by ~40 bytes; a session-lifetime interner (4C) grows without bound and
/// would need a lock.
///
/// The strings are in one buffer, with their ends: no per-cluster allocation
/// per frame, `clear` keeps the capacity.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Clusters {
    text: String,
    /// Each cluster's **end** in `text`; its start is the previous one's end.
    ends: Vec<u32>,
}

impl Clusters {
    pub fn clear(&mut self) {
        self.text.clear();
        self.ends.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.ends.is_empty()
    }

    /// Appends the string and returns its identity. `None` if the identity or
    /// byte space ran out — the cell is then clusterless, i.e. drawn with its
    /// **base character** (the same answer as for a cluster that does not
    /// shape, 035 R1.1).
    pub fn push(&mut self, cluster: &str) -> Option<ClusterId> {
        self.push_chars(cluster.chars())
    }

    /// [`Clusters::push`] code point by code point: the grid's cell carries
    /// the cluster as base + `zerowidth`, the dock's layout as a range of the
    /// stream — both go straight into the buffer without building an
    /// intermediate `String`.
    pub(crate) fn push_chars(
        &mut self,
        chars: impl IntoIterator<Item = char>,
    ) -> Option<ClusterId> {
        let start = self.text.len();
        self.text.extend(chars);
        let id = u32::try_from(self.ends.len() + 1)
            .ok()
            .and_then(NonZeroU32::new);
        match (u32::try_from(self.text.len()), id) {
            (Ok(end), Some(id)) => {
                self.ends.push(end);
                Some(ClusterId(id))
            }
            _ => {
                self.text.truncate(start);
                None
            }
        }
    }

    /// The identity's string; the identity of another table (or of a cleared
    /// frame) gives `None` — on the draw path, i.e. the base character, not a
    /// panic.
    pub fn get(&self, id: ClusterId) -> Option<&str> {
        let index = id.0.get() as usize - 1;
        let end = *self.ends.get(index)? as usize;
        let start = match index.checked_sub(1) {
            Some(before) => *self.ends.get(before)? as usize,
            None => 0,
        };
        self.text.get(start..end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cluster_table_returns_what_was_pushed() {
        let mut table = Clusters::default();
        let flag = table.push("🇹🇷").expect("id");
        let family = table.push("👨\u{200D}👩\u{200D}👧").expect("id");
        assert_eq!(table.get(flag), Some("🇹🇷"));
        assert_eq!(table.get(family), Some("👨\u{200D}👩\u{200D}👧"));
        assert_eq!(std::mem::size_of::<Option<ClusterId>>(), 4);
        table.clear();
        assert!(table.is_empty());
        assert_eq!(table.get(flag), None, "an identity of the cleared table");
    }

    /// Splits a string into clusters: each cluster's text and columns.
    fn split(text: &str) -> Vec<(String, usize)> {
        let chars: Vec<char> = text.chars().collect();
        let mut out = Vec::new();
        Walk::new().run(text.chars(), |cluster| {
            let run = chars
                .get(cluster.start..cluster.end)
                .unwrap_or_default()
                .iter()
                .collect();
            out.push((run, cluster.width));
        });
        out
    }

    fn one(text: &str) -> Vec<(String, usize)> {
        vec![(text.to_owned(), 2)]
    }

    /// The seventeen samples of the scratchpad measurement (`discussion.md` →
    /// Karar 3): each is a single cluster of two columns, or the measured
    /// split.
    #[test]
    fn the_measured_sequences_are_single_two_column_clusters() {
        for sequence in [
            "🇹🇷",
            "🇬🇧",
            "👨\u{200D}👩\u{200D}👧",
            "👍🏽",
            "❤\u{FE0F}",
            "☺\u{FE0F}",
            "🏳\u{FE0F}\u{200D}🌈",
            "1\u{FE0F}\u{20E3}",
            "🌡\u{FE0F}",
            // The Scottish flag: a tag sequence.
            "🏴\u{E0067}\u{E0062}\u{E0073}\u{E0063}\u{E0074}\u{E007F}",
            // The ten-code-point kiss: the `❤` behind a ZWJ has text
            // presentation.
            "🧑🏻\u{200D}❤\u{FE0F}\u{200D}💋\u{200D}🧑🏼",
        ] {
            assert_eq!(split(sequence), one(sequence), "{sequence:?}");
        }
        assert_eq!(
            split("🇹🇷🇬"),
            vec![("🇹🇷".to_owned(), 2), ("🇬".to_owned(), 1)],
            "pair + unpaired RI"
        );
        assert_eq!(
            split("a🏽"),
            vec![("a".to_owned(), 1), ("🏽".to_owned(), 2)],
            "a skin tone does not join a narrow cluster"
        );
        assert_eq!(
            split("👨\u{200D}a"),
            vec![("👨\u{200D}".to_owned(), 2), ("a".to_owned(), 1)],
            "a letter behind a ZWJ is a new cluster"
        );
        assert_eq!(
            split("a\u{200D}b"),
            vec![("a\u{200D}".to_owned(), 1), ("b".to_owned(), 1)]
        );
        assert_eq!(
            split("e\u{301}"),
            vec![("e\u{301}".to_owned(), 1)],
            "accent"
        );
    }

    /// A non-emoji cluster and VS15 as today: `لا` is two clusters, `⌚︎` stays
    /// wide (does not narrow) — the two side effects of the general "the table
    /// swallowed it" arm.
    #[test]
    fn non_emoji_clusters_keep_todays_cells() {
        assert_eq!(split("لا"), vec![("ل".to_owned(), 1), ("ا".to_owned(), 1)]);
        assert_eq!(
            split("⌚\u{FE0E}"),
            vec![("⌚\u{FE0E}".to_owned(), 2)],
            "VS15 goes to zerowidth, the cell does not narrow"
        );
    }

    #[test]
    fn regional_indicators_pair_up() {
        assert_eq!(split("🇹"), vec![("🇹".to_owned(), 1)], "single RI");
        assert_eq!(split("🇹🇷"), one("🇹🇷"), "pair");
        assert_eq!(
            split("🇹🇷🇬🇧"),
            vec![("🇹🇷".to_owned(), 2), ("🇬🇧".to_owned(), 2)],
            "two flags"
        );
        assert_eq!(
            split("🇹🇷🇬"),
            vec![("🇹🇷".to_owned(), 2), ("🇬".to_owned(), 1)],
            "triple"
        );
    }

    /// The prefilter misses no extension: if `extends` says yes, `may_extend`
    /// does too. If the wrapper turned back at the prefilter the cluster would
    /// silently split.
    #[test]
    fn the_prefilter_never_drops_an_extension() {
        let opens = ["a", "1", "👍", "🇹", "👨\u{200D}", "❤", "日", "a\u{200D}"];
        let probes = (0x20..0x3000)
            .chain(0x1F1E0..0x1F200)
            .chain(0x1F300..0x1FA00)
            .chain([0x200D, 0xFE0F, 0xFE0E, 0x20E3, 0xE0067, 0xE007F])
            .filter_map(char::from_u32);
        for c in probes {
            for open in opens {
                if extends(open, c) {
                    assert!(may_extend(c), "{open:?} + {c:?}");
                }
            }
        }
    }

    /// A single-code-point cluster by the dock's table: a tab and a control
    /// character are 1, a line break closes the cluster.
    #[test]
    fn single_code_points_keep_the_column_width_table() {
        assert_eq!(width("\t"), 1);
        assert_eq!(width("a"), 1);
        assert_eq!(width("日"), 2);
        assert_eq!(width(""), 0);
        assert!(!extends("a", '\n'));
        assert_eq!(
            split("\u{301}\u{301}a"),
            vec![
                ("\u{301}".to_owned(), 0),
                ("\u{301}".to_owned(), 0),
                ("a".to_owned(), 1)
            ],
            "a headless combiner is columnless and on its own"
        );
        assert_eq!(
            split("\u{200D}👍"),
            vec![("\u{200D}".to_owned(), 0), ("👍".to_owned(), 2)]
        );
    }
}
