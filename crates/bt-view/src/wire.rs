//! A page as a flat list, the form it takes where a page is made apart from the host that
//! shows it: a module in a sandbox hands its tree over the border as nodes, depth-first, each
//! with the count of its children, and the host builds the [`Element`] again from them.
//!
//! The nodes carry the same fields the chain of an [`Element`] sets (`Style`, `Kind`, `Meta`),
//! so nothing about a page is lost on the way and nothing is added: this is a way of laying a
//! tree down in a list and picking it up again. What the host takes in is checked
//! ([`Element::unfold`]) — a tree is as deep and as large as the border lets it be, not as the
//! other side says.

use crate::Element;
pub use crate::element::{Kind, Meta, Spark};
pub use crate::style::{Direction, Display, Items, Justify, Length, Overflow, Style, TextStyle};

/// One element of a tree laid down in a list.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub kind: Kind,
    pub style: Style,
    pub meta: Meta,
    /// How many of the nodes after this one are its children (not its grandchildren): each of
    /// them is followed by its own.
    pub children: u32,
}

/// Why a list is not a tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Malformed {
    /// No node at all.
    Empty,
    /// The list ends with elements still owed children.
    Truncated,
    /// More nodes after the root's tree is whole.
    Trailing,
    /// The tree is deeper than the limit.
    TooDeep,
}

impl std::fmt::Display for Malformed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Empty => "the tree has no element",
            Self::Truncated => "the tree ends before its elements have their children",
            Self::Trailing => "there are elements after the tree is whole",
            Self::TooDeep => "the tree is deeper than the limit",
        })
    }
}

impl std::error::Error for Malformed {}

impl Element {
    /// The tree as a list, depth-first: an element, then its children, each with its own.
    pub fn fold(&self) -> Vec<Node> {
        let mut nodes = Vec::new();
        self.fold_into(&mut nodes);
        nodes
    }

    fn fold_into(&self, nodes: &mut Vec<Node>) {
        nodes.push(Node {
            kind: self.kind.clone(),
            style: self.style.clone(),
            meta: self.meta,
            children: u32::try_from(self.children.len()).unwrap_or(u32::MAX),
        });
        for child in &self.children {
            child.fold_into(nodes);
        }
    }

    /// The tree a list is, no deeper than `max_depth` (the root is depth 1).
    ///
    /// Built without recursion, so a list that is a thousand elements deep costs the limit
    /// check and not the stack; a node's claim to have more children than there are nodes left
    /// is found out when the list runs short, not by reserving room for them.
    pub fn unfold(nodes: Vec<Node>, max_depth: usize) -> Result<Self, Malformed> {
        let mut open: Vec<(Self, u32)> = Vec::new();
        let mut root: Option<Self> = None;
        for node in nodes {
            if root.is_some() {
                return Err(Malformed::Trailing);
            }
            open.push((
                Self {
                    kind: node.kind,
                    style: node.style,
                    children: Vec::new(),
                    meta: node.meta,
                },
                node.children,
            ));
            if open.len() > max_depth {
                return Err(Malformed::TooDeep);
            }
            // Every element that has all its children is closed into the one that holds it.
            while let Some((_, 0)) = open.last() {
                let Some((done, _)) = open.pop() else { break };
                match open.last_mut() {
                    Some((parent, owed)) => {
                        parent.children.push(done);
                        *owed -= 1;
                    }
                    None => root = Some(done),
                }
            }
        }
        if !open.is_empty() {
            return Err(Malformed::Truncated);
        }
        root.ok_or(Malformed::Empty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Tone, div, spacer, spark, text};

    fn page() -> Element {
        div()
            .row()
            .gap(8.0)
            .bg(Tone::Raised(1))
            .id(crate::Id::named("page"))
            .child(text("one").weight(700))
            .child(
                div()
                    .press(crate::Id::named("two"))
                    .focusable()
                    .child(text("two"))
                    .child(spark(&[1.0, 2.0, 3.0], Tone::Accent)),
            )
            .child(spacer())
            .child(div().text_field().h(18.0))
    }

    #[test]
    fn a_tree_folded_and_unfolded_is_the_tree() {
        let tree = page();
        let nodes = tree.fold();
        assert_eq!(nodes.len(), 7);
        assert_eq!(nodes[0].children, 4);
        assert_eq!(Element::unfold(nodes, 16), Ok(tree));
    }

    #[test]
    fn a_list_that_is_not_a_tree_is_refused() {
        let nodes = page().fold();
        assert_eq!(Element::unfold(Vec::new(), 16), Err(Malformed::Empty));
        assert_eq!(
            Element::unfold(nodes[..nodes.len() - 1].to_vec(), 16),
            Err(Malformed::Truncated)
        );
        let mut more = nodes.clone();
        more.push(nodes[0].clone());
        assert_eq!(Element::unfold(more, 16), Err(Malformed::Trailing));
        let mut greedy = nodes;
        greedy[0].children = u32::MAX;
        assert_eq!(Element::unfold(greedy, 16), Err(Malformed::Truncated));
    }

    #[test]
    fn a_tree_deeper_than_the_limit_is_refused_without_the_stack() {
        let mut deep = div();
        for _ in 0..50 {
            deep = div().child(deep);
        }
        assert_eq!(Element::unfold(deep.fold(), 51), Ok(deep.clone()));
        assert_eq!(Element::unfold(deep.fold(), 50), Err(Malformed::TooDeep));

        // A hundred thousand nested elements are refused at the limit, not at the stack's end.
        let node = |children| Node {
            kind: Kind::Box,
            style: Style::default(),
            meta: Meta::default(),
            children,
        };
        let nodes = (0..100_000).map(|_| node(1)).collect::<Vec<_>>();
        assert_eq!(Element::unfold(nodes, 64), Err(Malformed::TooDeep));
    }
}
