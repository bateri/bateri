//! A split tree as text, for a host that keeps its layout on disk: versioned, so a layout written
//! today is read by every later build.
//!
//! **The format.** The first line names it and its version (`bateri-tree 1`); a `P` line lists
//! the pane identities in tree order; a `T` line carries the tree as session restore writes it —
//! a pre-order token run (`S h|v {ratio} … L {position}`) whose leaves are positions in the `P`
//! list. The grammar is restore's ([`crate::restore::Shape`]), one copy: its ratio check and its
//! depth bound hold here too. Identities are written as given; what they mean in another process
//! is the host's to say.
//!
//! **A version is read forever.** Layouts sit on disk for months and are read by builds that did
//! not write them, so a reader takes every version up to its own, and each version's frozen text
//! stays in the tests (`VERSION_1`): changing the format adds a version and its fixture, it never
//! rewrites one. A newer version than the reader knows is not read — a half-understood tree would
//! put panes in the wrong place.
//!
//! Reading is strict and never panics: a wrong header, a malformed line, an identity twice, a
//! position out of range or a leaf missing makes the whole text `None`.

use std::collections::HashSet;

use crate::restore::Shape;
use crate::split::Tree;

/// The first line's word; the version follows it after one space.
pub const HEADER: &str = "bateri-tree";

/// The version this build writes. Every version from 1 up to it is read.
pub const VERSION: u32 = 1;

/// `tree` as text; `None` for a tree that holds a pane twice, which no layout does.
pub fn encode(tree: &Tree) -> Option<String> {
    let ids = tree.leaves();
    let shape = Shape::from_tree(tree, &ids)?;
    let mut out = format!("{HEADER} {VERSION}\nP");
    for id in &ids {
        out.push_str(&format!(" {id}"));
    }
    out.push_str("\nT");
    shape.render(&mut out);
    out.push('\n');
    Some(out)
}

/// The tree `text` holds, written by this build or an earlier one; `None` if it is not one.
pub fn decode(text: &str) -> Option<Tree> {
    let mut lines = text.lines();
    let version: u32 = lines
        .next()?
        .strip_prefix(HEADER)?
        .strip_prefix(' ')?
        .parse()
        .ok()?;
    if !(1..=VERSION).contains(&version) {
        return None;
    }
    let ids: Vec<u64> = lines
        .next()?
        .strip_prefix('P')?
        .split_whitespace()
        .map(|id| id.parse().ok())
        .collect::<Option<_>>()?;
    let mut tokens = lines.next()?.strip_prefix('T')?.split_whitespace();
    let shape = Shape::parse(&mut tokens, 0)?;
    if tokens.next().is_some() || lines.any(|line| !line.trim().is_empty()) {
        return None;
    }
    let distinct: HashSet<u64> = ids.iter().copied().collect();
    if distinct.len() != ids.len() || !shape.covers(ids.len()) {
        return None;
    }
    shape.to_tree(&ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::split::Axis;

    /// Version 1's text, frozen: every later reader must read it as it stands.
    const VERSION_1: &str =
        "bateri-tree 1\nP 42 7 18446744073709551615\nT S h 0.5 L 0 S v 0.25 L 1 L 2\n";

    fn tree() -> Tree {
        Tree::Split {
            axis: Axis::Horizontal,
            ratio: 0.5,
            first: Box::new(Tree::Leaf(42)),
            second: Box::new(Tree::Split {
                axis: Axis::Vertical,
                ratio: 0.25,
                first: Box::new(Tree::Leaf(7)),
                second: Box::new(Tree::Leaf(u64::MAX)),
            }),
        }
    }

    #[test]
    fn the_first_version_is_read_byte_for_byte() {
        assert_eq!(decode(VERSION_1), Some(tree()));
    }

    #[test]
    fn this_build_writes_what_it_reads() {
        let text = encode(&tree()).expect("a tree with distinct panes is written");
        assert_eq!(text, VERSION_1, "version 1 is still what this build writes");
        assert_eq!(decode(&text), Some(tree()));
        let lone = Tree::Leaf(3);
        assert_eq!(decode(&encode(&lone).expect("a lone pane")), Some(lone));
    }

    #[test]
    fn identities_are_kept_as_given_whatever_their_size() {
        let far = Tree::Split {
            axis: Axis::Vertical,
            ratio: 0.75,
            first: Box::new(Tree::Leaf(1 << 60)),
            second: Box::new(Tree::Leaf(0)),
        };
        assert_eq!(decode(&encode(&far).expect("written")), Some(far));
    }

    #[test]
    fn a_newer_version_or_a_broken_text_is_not_read() {
        for broken in [
            "bateri-tree 2\nP 1\nT L 0\n",
            "bateri-tree 0\nP 1\nT L 0\n",
            "bateri-session 1\nP 1\nT L 0\n",
            "bateri-tree 1\nP 1 1\nT S h 0.5 L 0 L 1\n",
            "bateri-tree 1\nP 1 2\nT S h 0.5 L 0 L 0\n",
            "bateri-tree 1\nP 1\nT L 1\n",
            "bateri-tree 1\nP 1 2\nT L 0\n",
            "bateri-tree 1\nP 1 2\nT S h 1.5 L 0 L 1\n",
            "bateri-tree 1\nP 1\nT L 0 L 0\n",
            "bateri-tree 1\nP 1\nT L 0\nextra\n",
            "bateri-tree 1\nP x\nT L 0\n",
            "bateri-tree 1\nP 1\n",
            "",
        ] {
            assert_eq!(decode(broken), None, "{broken:?}");
        }
        let twice = Tree::Split {
            axis: Axis::Horizontal,
            ratio: 0.5,
            first: Box::new(Tree::Leaf(1)),
            second: Box::new(Tree::Leaf(1)),
        };
        assert_eq!(encode(&twice), None);
    }
}
