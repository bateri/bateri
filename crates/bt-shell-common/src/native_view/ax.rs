//! The tree a screen reader is given of a page: how the elements of its reading hold each other,
//! and what each one is in a table, an outline or a list, worked out apart from any platform's
//! accessibility API. `bt-shell-macos` makes AppKit's elements from it (`NSAccessibilityElement`
//! with the properties of `NSAccessibilityTable`, `NSAccessibilityRow` and `NSAccessibilityOutline`);
//! a host of another platform makes its own from the same plan, and so reads the same tree.
//!
//! The reading gives a flat list of [`Accessible`] where an element that is a group (a dialog, a
//! row, a tab that carries controls) is before what it holds and says so by `parent`. What a
//! modal layer in front shuts out is not in the list ([`pencere_view::Laid::accessible_behind`]),
//! so it is in no tree either. Counts and indexes are the platform's: **from 0** here (the page's
//! are from 1).

use pencere_view::{Accessible, AccessibleRole, Id, Sort};

/// What the plan says of one element of the reading (the one at the same place in the list).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AxNode {
    /// The element that holds this one: a dialog, a row, a table. `None` for what is given to the
    /// screen reader as the page's own.
    pub parent: Option<usize>,
    /// What this element holds, in the order it reads them.
    pub children: Vec<usize>,
    /// What the element is called as a kind, where the page said so or the role has a name of its own
    /// that the platform has no word for (a column header, an alert).
    pub role_description: Option<String>,
    /// What it is for and the keys that do what it does, in one line.
    pub help: Option<String>,
    /// Which one it is of its rows (a row of a table, a row of an outline, an option), from 0, and of
    /// its columns on a header.
    pub index: Option<i64>,
    /// How deep a row of an outline is, from 0.
    pub disclosure_level: Option<i64>,
    /// The row of an outline that this one is under, and the rows directly under this one.
    pub disclosed_by: Option<usize>,
    pub disclosed_rows: Vec<usize>,
    /// Of a table or an outline: its rows (those of the table that are not its head), its head, the
    /// headers of its columns, and the rows chosen.
    pub rows: Vec<usize>,
    pub header: Option<usize>,
    pub column_headers: Vec<usize>,
    pub selected_rows: Vec<usize>,
    /// Of a list or a row of tabs: what is chosen in it.
    pub selected_children: Vec<usize>,
    /// Of a table: how many rows and columns the whole of it has, which a page that builds only what is
    /// in view has more of than it has rows here.
    pub row_count: Option<i64>,
    pub column_count: Option<i64>,
    /// Of a cell or a header: the rows and the columns it is in, as (first, how many).
    pub row_range: Option<(i64, i64)>,
    pub column_range: Option<(i64, i64)>,
    /// How the table is sorted by this column (on a header).
    pub sort: Option<Sort>,
    /// What can be done with the element besides pressing it: the commands the page defined on it
    /// that can be done now, by the name of each and the command's id.
    pub actions: Vec<(String, Id)>,
}

/// The plan of the tree of `items`, one node for each item and in the same order.
#[must_use]
pub fn plan(items: &[Accessible]) -> Vec<AxNode> {
    let mut nodes: Vec<AxNode> = vec![AxNode::default(); items.len()];
    // Who holds whom: a parent comes before what it holds, so an index that is not before is none.
    for (at, item) in items.iter().enumerate() {
        let Some(outer) = item.parent.filter(|outer| *outer < at) else {
            continue;
        };
        nodes[at].parent = Some(outer);
        nodes[outer].children.push(at);
    }
    for (at, item) in items.iter().enumerate() {
        let state = &item.state;
        let node = &mut nodes[at];
        node.role_description = item
            .detail
            .role_description
            .clone()
            .or_else(|| default_description(item.role).map(str::to_owned));
        node.help = match (
            item.detail.description.as_deref(),
            item.detail.keys.as_deref(),
        ) {
            (Some(what), Some(keys)) => Some(format!("{what}. Keys: {keys}")),
            (Some(what), None) => Some(what.to_owned()),
            (None, Some(keys)) => Some(format!("Keys: {keys}")),
            (None, None) => None,
        };
        node.actions = item
            .detail
            .actions
            .iter()
            .filter(|action| action.enabled)
            .map(|action| (action.title.clone(), action.id))
            .collect();
        match item.role {
            AccessibleRole::Row => {
                node.index = from_zero(state.rowindex.or(state.posinset));
            }
            AccessibleRole::ListOption | AccessibleRole::ListItem => {
                node.index = from_zero(state.posinset);
            }
            AccessibleRole::Cell => {
                node.column_range = from_zero(state.colindex).map(|first| (first, 1));
            }
            AccessibleRole::ColumnHeader => {
                node.index = from_zero(state.colindex);
                node.column_range = node.index.map(|first| (first, 1));
                node.sort = state.sort;
            }
            _ => {}
        }
    }
    // A cell is in the row it is under, where it does not say.
    for at in 0..items.len() {
        if items[at].role != AccessibleRole::Cell {
            continue;
        }
        let row = nodes[at]
            .parent
            .and_then(|outer| from_zero(items[outer].state.rowindex))
            .or_else(|| from_zero(items[at].state.rowindex));
        nodes[at].row_range = row.map(|first| (first, 1));
    }
    for at in 0..items.len() {
        match items[at].role {
            AccessibleRole::Table => table(items, &mut nodes, at),
            AccessibleRole::Tree => outline(items, &mut nodes, at),
            AccessibleRole::ListBox | AccessibleRole::List | AccessibleRole::TabList => {
                let chosen = below(&nodes, at)
                    .into_iter()
                    .filter(|held| items[*held].state.selected)
                    .collect();
                nodes[at].selected_children = chosen;
            }
            _ => {}
        }
    }
    nodes
}

/// What the screen reader is given of the page as its own: the elements nothing holds, in the order
/// the reading has them.
#[must_use]
pub fn top_level(nodes: &[AxNode]) -> Vec<usize> {
    nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| node.parent.is_none())
        .map(|(at, _)| at)
        .collect()
}

/// The words of what the page says out loud (an `alert`): what a screen reader is to be told as it
/// comes up, in the order of the reading.
#[must_use]
pub fn alerts(items: &[Accessible]) -> Vec<&str> {
    items
        .iter()
        .filter(|item| item.role == AccessibleRole::Alert && !item.words.trim().is_empty())
        .map(|item| item.words.as_str())
        .collect()
}

/// A count or a place the page gives from 1, as the platform has it, from 0.
fn from_zero(number: Option<u32>) -> Option<i64> {
    number
        .filter(|number| *number >= 1)
        .map(|number| i64::from(number) - 1)
}

/// What a platform has no word for in a role: its kind said in words.
fn default_description(role: AccessibleRole) -> Option<&'static str> {
    match role {
        AccessibleRole::ColumnHeader => Some("column header"),
        AccessibleRole::Alert => Some("alert"),
        AccessibleRole::Status => Some("status"),
        AccessibleRole::AlertDialog => Some("alert dialog"),
        _ => None,
    }
}

/// Everything under `at`, at any depth, in the order of the reading.
fn below(nodes: &[AxNode], at: usize) -> Vec<usize> {
    let mut found = Vec::new();
    let mut waiting: Vec<usize> = nodes[at].children.iter().rev().copied().collect();
    while let Some(next) = waiting.pop() {
        found.push(next);
        waiting.extend(nodes[next].children.iter().rev().copied());
    }
    found
}

/// A table: the rows that are its body, the row that is its head, the headers of its columns, the
/// rows chosen, and how many rows and columns the whole of it has.
fn table(items: &[Accessible], nodes: &mut [AxNode], at: usize) {
    let held = below(nodes, at);
    let heads: Vec<usize> = held
        .iter()
        .copied()
        .filter(|held| items[*held].role == AccessibleRole::ColumnHeader)
        .collect();
    let header = held.iter().copied().find(|held| {
        items[*held].role == AccessibleRole::Row
            && nodes[*held]
                .children
                .iter()
                .any(|child| items[*child].role == AccessibleRole::ColumnHeader)
    });
    let rows: Vec<usize> = held
        .iter()
        .copied()
        .filter(|held| items[*held].role == AccessibleRole::Row && Some(*held) != header)
        .collect();
    let state = &items[at].state;
    let node = &mut nodes[at];
    node.selected_rows = rows
        .iter()
        .copied()
        .filter(|row| items[*row].state.selected)
        .collect();
    node.row_count = state
        .rowcount
        .map(i64::from)
        .or_else(|| i64::try_from(rows.len()).ok());
    node.column_count = state
        .colcount
        .map(i64::from)
        .or_else(|| i64::try_from(heads.len()).ok());
    node.rows = rows;
    node.header = header;
    node.column_headers = heads;
}

/// An outline: its rows, at every level, in order, how deep each is, which row each is under and
/// which rows are directly under it.
fn outline(items: &[Accessible], nodes: &mut [AxNode], at: usize) {
    let rows: Vec<usize> = below(nodes, at)
        .into_iter()
        .filter(|held| items[*held].role == AccessibleRole::TreeItem)
        .collect();
    let level = |row: usize| i64::from(items[row].state.level.unwrap_or(1).max(1)) - 1;
    for (place, row) in rows.iter().copied().enumerate() {
        let deep = level(row);
        let node = &mut nodes[row];
        node.index = i64::try_from(place).ok();
        node.disclosure_level = Some(deep);
        node.disclosed_by = rows[..place]
            .iter()
            .rev()
            .copied()
            .find(|before| level(*before) == deep - 1);
        let under: Vec<usize> = rows[place + 1..]
            .iter()
            .copied()
            .take_while(|after| level(*after) > deep)
            .filter(|after| level(*after) == deep + 1)
            .collect();
        node.disclosed_rows = under;
    }
    let node = &mut nodes[at];
    node.selected_rows = rows
        .iter()
        .copied()
        .filter(|row| items[*row].state.selected)
        .collect();
    node.rows = rows;
}

#[cfg(test)]
mod tests {
    use super::*;
    use pencere_view::{Action, Detail, Rect, State};

    fn item(role: AccessibleRole, words: &str, parent: Option<usize>) -> Accessible {
        Accessible {
            role,
            id: Some(Id::named(words)),
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            words: words.to_owned(),
            state: State::default(),
            pressable: false,
            parent,
            detail: Detail::default(),
        }
    }

    fn with(mut item: Accessible, state: impl FnOnce(&mut State)) -> Accessible {
        state(&mut item.state);
        item
    }

    /// A dialog open over a page: what the screen reader is given of the page is the dialog alone,
    /// with what is in it under it.
    #[test]
    fn a_page_with_a_dialog_in_front_gives_the_screen_reader_the_dialog_and_what_is_in_it() {
        let items = vec![
            item(AccessibleRole::Dialog, "Save changes?", None),
            item(AccessibleRole::Text, "Your changes will be lost.", Some(0)),
            item(AccessibleRole::Button, "Save", Some(0)),
        ];
        let nodes = plan(&items);
        assert_eq!(top_level(&nodes), [0]);
        assert_eq!(nodes[0].children, [1, 2]);
        assert_eq!(nodes[2].parent, Some(0));
    }

    /// The table of a grid whose 300 rows are built 3 at a time: the rows, the head, the headers with
    /// their columns and their sort, the row chosen, and the sizes of the whole table.
    #[test]
    fn a_table_has_its_rows_its_head_its_headers_and_the_size_of_the_whole_of_it() {
        let mut table = item(AccessibleRole::Table, "Table", None);
        table.state.rowcount = Some(300);
        table.state.colcount = Some(3);
        let sorted = with(
            item(AccessibleRole::ColumnHeader, "name", Some(1)),
            |state| {
                state.colindex = Some(2);
                state.sort = Some(Sort::Ascending);
            },
        );
        let items = vec![
            table,
            item(AccessibleRole::Row, "Header", Some(0)),
            with(item(AccessibleRole::ColumnHeader, "id", Some(1)), |state| {
                state.colindex = Some(1);
            }),
            sorted,
            with(item(AccessibleRole::Row, "Row 1", Some(0)), |state| {
                state.rowindex = Some(1);
            }),
            with(item(AccessibleRole::Row, "Row 2", Some(0)), |state| {
                state.rowindex = Some(2);
                state.selected = true;
            }),
            with(item(AccessibleRole::Cell, "b", Some(5)), |state| {
                state.colindex = Some(2);
            }),
        ];
        let nodes = plan(&items);
        let table = &nodes[0];
        assert_eq!(table.rows, [4, 5]);
        assert_eq!(table.header, Some(1));
        assert_eq!(table.column_headers, [2, 3]);
        assert_eq!(table.selected_rows, [5]);
        assert_eq!((table.row_count, table.column_count), (Some(300), Some(3)));
        assert_eq!(nodes[3].index, Some(1));
        assert_eq!(nodes[3].sort, Some(Sort::Ascending));
        assert_eq!(nodes[3].role_description.as_deref(), Some("column header"));
        assert_eq!(nodes[5].index, Some(1));
        // A cell is in the row it is under, and in its column.
        assert_eq!(nodes[6].row_range, Some((1, 1)));
        assert_eq!(nodes[6].column_range, Some((1, 1)));
        assert_eq!(top_level(&nodes), [0]);
    }

    /// An outline of three levels: each row's depth, the row it is under, and the rows directly under it.
    #[test]
    fn an_outline_gives_each_row_its_depth_the_row_it_is_under_and_the_rows_under_it() {
        let row = |words: &str, level: u32, open: Option<bool>| {
            with(item(AccessibleRole::TreeItem, words, Some(0)), |state| {
                state.level = Some(level);
                state.expanded = open;
            })
        };
        let items = vec![
            item(AccessibleRole::Tree, "Tree", None),
            row("src", 1, Some(true)),
            row("main.rs", 2, None),
            row("view", 2, Some(true)),
            row("lib.rs", 3, None),
            row("docs", 1, Some(false)),
        ];
        let nodes = plan(&items);
        assert_eq!(nodes[0].rows, [1, 2, 3, 4, 5]);
        assert_eq!(nodes[1].disclosure_level, Some(0));
        assert_eq!(nodes[1].disclosed_by, None);
        assert_eq!(nodes[1].disclosed_rows, [2, 3]);
        assert_eq!(nodes[3].disclosure_level, Some(1));
        assert_eq!(nodes[3].disclosed_by, Some(1));
        assert_eq!(nodes[3].disclosed_rows, [4]);
        assert_eq!(nodes[4].disclosure_level, Some(2));
        assert_eq!(nodes[4].disclosed_by, Some(3));
        assert_eq!(nodes[5].disclosed_rows, Vec::<usize>::new());
        assert_eq!(nodes[5].index, Some(4));
    }

    /// A list of options says which is chosen and where each is; a page's words for a kind, its
    /// commands and its keys come as the page wrote them.
    #[test]
    fn a_list_says_what_is_chosen_and_each_element_its_kind_its_help_and_its_actions() {
        let mut stop = with(item(AccessibleRole::ListOption, "stop", Some(0)), |state| {
            state.posinset = Some(2);
            state.selected = true;
        });
        stop.detail = Detail {
            description: Some("Ends the process".to_owned()),
            keys: Some("⌘K".to_owned()),
            role_description: Some("process".to_owned()),
            actions: vec![
                Action {
                    id: Id::named("kill"),
                    title: "Kill".to_owned(),
                    enabled: true,
                },
                Action {
                    id: Id::named("renice"),
                    title: "Renice".to_owned(),
                    enabled: false,
                },
            ],
        };
        let items = vec![
            item(AccessibleRole::ListBox, "List", None),
            with(
                item(AccessibleRole::ListOption, "start", Some(0)),
                |state| {
                    state.posinset = Some(1);
                },
            ),
            stop,
        ];
        let nodes = plan(&items);
        assert_eq!(nodes[0].selected_children, [2]);
        assert_eq!(nodes[2].index, Some(1));
        assert_eq!(nodes[2].role_description.as_deref(), Some("process"));
        assert_eq!(nodes[2].help.as_deref(), Some("Ends the process. Keys: ⌘K"));
        assert_eq!(nodes[2].actions, [("Kill".to_owned(), Id::named("kill"))]);
    }

    #[test]
    fn what_a_page_says_out_loud_is_listed_in_order_and_a_parent_that_is_not_before_is_none() {
        let items = vec![
            item(AccessibleRole::Button, "Go", None),
            item(AccessibleRole::Alert, "Disk is full", None),
            item(AccessibleRole::Status, "3 results", None),
            item(AccessibleRole::Alert, "  ", None),
            // A reading made by hand, whose parent is after it: no parent.
            item(AccessibleRole::Text, "x", Some(9)),
        ];
        assert_eq!(alerts(&items), ["Disk is full"]);
        let nodes = plan(&items);
        assert_eq!(nodes[4].parent, None);
        assert_eq!(nodes[1].role_description.as_deref(), Some("alert"));
        assert_eq!(top_level(&nodes), [0, 1, 2, 3, 4]);
    }
}
