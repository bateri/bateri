//! Session restore: the saved layout's model, its versioned line format and the life of
//! the directory that holds it.
//!
//! **The model** is what comes back after a quit: windows (frame, selected tab,
//! key, the side column's width, its tab groups), tabs (the split tree's shape, focus, zoom) and panes (the persistent `PaneUuid`, the local
//! directory, the point-size step, the remote target's line, whether a history file was written).
//! The tree's leaves are **indices** into the tab's pane list ([`Shape`]): the in-process `u64`
//! pane identity means nothing in the next process, so it never reaches the disk —
//! [`Shape::from_tree`] turns identities into indices when saving, [`Shape::to_tree`] indices into
//! the new identities when restoring.
//!
//! **The format** follows `remote-hosts` (`ssh_wrap`): line based, the first line names the
//! version ([`HEADER`] + [`VERSION`]), then one `W` line per window, one `T` line per tab under it
//! and one `P` line per pane under that; the tree is a pre-order token run (`S h|v {ratio} … L
//! {index}`). **Every version the writer still writes is read**: version 6 gave a kept terminal
//! whether its pane closes when the terminal ends and who asked for the pane (two fields after
//! the terminal's name: `0`/`1`, and `-` or `+{words}`), version 5 gave a pane the
//! terminal a server keeps for it (a `P` line's last fields: `-`, or `+{account} {terminal}`
//! and the `ssh` arguments that name the server), version 4 gave a window its tab
//! groups (a `G {name} {collapsed} {resident}` line each, right after the `W` line and before any
//! of its tabs, and a group field on every `T` line: the group's index among them, `-` for an
//! ungrouped tab), version 3 the width its side column was dragged to (a `W` line's seventh
//! field), version 2 a tab its own name (a `T` line's third field), version 1 has none of them.
//! The writer says the oldest version that holds what there is — 1 while no tab is named, no
//! column was resized and no group exists, 2 while only names are, 3 once a width is, 4 once a
//! window has a group, 5 once a pane shows a terminal a server keeps, 6 once such a pane was
//! asked for by a program on the server (it closes with its terminal, or says who asked) — so a
//! session that uses
//! nothing new stays readable by the bateri before
//! it, whose reader drops any other number and, with it, a holder's programs. A version's frozen
//! text stays in the tests: its reader must keep reading it. The strip a window saves is the
//! flat order the groups were kept in (`groups::Groups::check`: the ungrouped tabs, then each
//! group's tabs together, in the groups' order), and the reader builds the grouping through that
//! check — one copy of the rule. A group remembers the tab the user was last on in it; the
//! ungrouped tabs' own is not kept (the first one is entered again). Text fields (directory, remote line) are escaped so that a field never holds a
//! space, a tab or a line break ([`escape`]); an absent field is `-`, a present one `+` followed
//! by the escaped text, so the empty string and absence stay apart. Reading is strict: an
//! unknown version, a malformed line, an index out of range, a tree whose leaves are not exactly
//! the tab's panes or a ratio outside `(0, 1)` makes the whole file `None` — a half-understood
//! layout would put panes in the wrong place, and the caller's fallback (one fresh window) is
//! today's launch. No panic on any input.
//!
//! **The directory** is given by the caller ([`directory`] names the production
//! one), created `0700`, its files `0600`. A [`Lock`] owns it: a second instance of the same
//! bundle neither reads nor writes. [`save`] writes the histories first and the layout **last**,
//! each through a temporary name and a `rename` — the layout's rename is the commit, so a save cut
//! halfway leaves the previous layout readable. [`take`] reads the layout and **deletes** it
//! before anything is replayed (a layout that crashes the launch must not come back on the next
//! one); [`history`] reads and deletes one pane's history; [`clear`] removes everything.
//! Histories no layout names, and leftover temporaries, are swept — the palette's Recent
//! ([`read_recent`], [`write_recent`], [`clear_recent`]) is application state that lives in the
//! directory without being part of the layout, so the sweep and the layout's clears leave it, and
//! `restore_windows = "off"` removes it. [`save_layout`] writes the
//! layout alone, without histories, while bateri runs — the way back after a crash nobody held
//! the programs through (no holder, or `keep_running = "update"`).
//!
//! **The attempt marker** ([`bump_attempt`], [`attempt_mode`]) counts, in a holder's instance
//! directory, the launches that started taking that holder's programs and never settled: a
//! restore that crashes the launch would otherwise crash every launch after it — the holder
//! waits for an acknowledgement that never comes. The second attempt restores no screen, the
//! third gives the programs up.
//!
//! Pure except the file bodies, which are thin; it sees no AppKit (the `split`/`zoom`
//! precedent).

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

use bt_core::PaneUuid;
use kablo_proto::SessionId;

use crate::groups::{Groups, Part};
use crate::split::{Axis, Tree};
use crate::tabs::Tabs;

/// The first line's word; the version follows it after one space.
pub const HEADER: &str = "bateri-session";

/// The format's current version: a kept terminal may say that its pane closes when it ends and
/// who asked for the pane. The ones before it ([`KEPT`], [`GROUPED`], [`SIDED`], [`NAMED`],
/// [`UNNAMED`]) are read too; any other number is not read (a newer bateri's file after a
/// downgrade is dropped, not misread).
pub const VERSION: u32 = 6;

/// The version before a program's panes, which a session whose kept terminals were all opened by
/// the user is still written in.
const KEPT: u32 = 5;

/// The version before kept terminals, which a session with groups and no pane on a server's
/// terminal is still written in.
const GROUPED: u32 = 4;

/// The version before groups, which a session with no group and a resized column is still written
/// in.
const SIDED: u32 = 3;

/// The version before widths, which a session with a named tab and no resized column or group is
/// still written in.
const NAMED: u32 = 2;

/// The version before names, which a session with no named tab, no resized column and no group is
/// still written in.
const UNNAMED: u32 = 1;

/// The layout file's name inside the directory.
const LAYOUT: &str = "layout";
/// The lock file's name: the layout cannot carry the lock, every save renames a new inode onto
/// its path (`ssh_wrap`'s lock precedent).
const LOCK: &str = "lock";
/// The ledger of the terminals bateri keeps on servers (`kablo::ledger`): application state that
/// lives beside the layout without being part of it, so the layout's sweeps and clears leave it
/// — `restore_windows = "off"` is no reason to forget which terminals are still to be ended.
const LEDGER: &str = "kablo";
/// A history file's extension: `{PaneUuid}.vt`.
const HISTORY_EXT: &str = "vt";

/// The palette's Recent: what the user chose last in the command palette. Application state
/// that lives beside the layout without being part of it, so the layout's saves and sweeps leave
/// it alone and only [`clear_recent`] removes it.
const RECENT: &str = "palette-recent";
/// The temporary-name suffix; anything ending in it is a leftover of an interrupted write.
const TEMP_SUFFIX: &str = ".tmp";

// ─── the model ───────────────────────────────────────────────────────────

/// Everything that is restored.
#[derive(Clone, Debug, PartialEq)]
pub struct Saved {
    /// In front-to-back order is the caller's choice; restored in this order.
    pub windows: Vec<SavedWindow>,
}

/// A window's frame in the platform's screen coordinates (AppKit: bottom-left origin), points.
/// Clamping onto a visible screen is the restoring side's job.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A window: its tabs in tab-bar order.
#[derive(Clone, Debug, PartialEq)]
pub struct SavedWindow {
    pub frame: Frame,
    pub tabs: Vec<SavedTab>,
    /// The selected tab's index in `tabs`.
    pub selected: usize,
    /// Whether this was the key window.
    pub key: bool,
    /// The width the user dragged the side column to (`tabs::side_width` fits it to the window);
    /// `None` keeps the default. Kept while the tabs are in the title row as well, so the column
    /// comes back as it was.
    pub side_width: Option<f64>,
    /// The window's tab groups in the order they are listed; none for a plain list of tabs. The
    /// tabs name theirs by index ([`SavedTab::group`]).
    pub groups: Vec<SavedGroup>,
}

/// A group of a window's tabs.
#[derive(Clone, Debug, PartialEq)]
pub struct SavedGroup {
    pub name: String,
    /// The column shows the header and, while it is of the group, the tab on screen only.
    pub collapsed: bool,
    /// The tab the user was last on in the group: its index in [`SavedWindow::tabs`].
    pub resident: Option<usize>,
}

/// A tab: the split tree's shape over `panes`.
#[derive(Clone, Debug, PartialEq)]
pub struct SavedTab {
    pub shape: Shape,
    /// The tab's panes; [`Shape`]'s leaves index this list.
    pub panes: Vec<SavedPane>,
    /// The focused pane's index.
    pub focused: usize,
    /// The zoomed pane's index (⇧⌘↩), if any.
    pub zoomed: Option<usize>,
    /// The name the user gave the tab; `None` shows the tab's own title.
    pub name: Option<String>,
    /// The group the tab is in: its index in [`SavedWindow::groups`]; `None` for an ungrouped tab.
    pub group: Option<usize>,
}

/// A pane.
#[derive(Clone, Debug, PartialEq)]
pub struct SavedPane {
    /// The persistent identity (`TERM_SESSION_ID`, `bateri://tab/<id>`); also the history
    /// file's name.
    pub uuid: PaneUuid,
    /// The local working directory (OSC 7). A path that is not UTF-8 is saved as absent.
    pub dir: Option<PathBuf>,
    /// The temporary point-size offset, in steps (`zoom::Zoom::steps`).
    pub zoom_steps: i32,
    /// The remote target's command line (`ssh prod`), restored as a ready, not run, first input.
    pub remote_line: Option<String>,
    /// The terminal a server keeps that the pane showed: restored, the pane's first program is
    /// the pipe to it again (`bateri ssh-hold --terminal … --account …`), not the line above.
    pub kept: Option<SavedKept>,
    /// Whether a history file was written for this pane.
    pub history: bool,
}

/// A terminal a server keeps (kablo), as far as a restored pane needs to find it again: which
/// account holds it, its name there, and the `ssh` arguments that name the server (the
/// program's name left out) — the pane's remote session is read from the command line of the
/// pipe that shows it, so the arguments go back onto it. Nothing here is a secret: the arguments
/// are the ones `remote_line` already keeps.
///
/// A pane a program on the server asked for also keeps what made it that program's: that it
/// closes when its terminal ends, and who asked for it. Both were the pane's alone — the pipe's
/// command line and the app's memory — so a pane born again after a restart would otherwise come
/// back as one the user opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedKept {
    /// The server's account ([`crate::ssh_wrap::host_key`]).
    pub account: String,
    /// The terminal's name on the server.
    pub terminal: SessionId,
    /// The `ssh` arguments, the program's name left out; never empty.
    pub args: Vec<String>,
    /// The pane closes when the terminal ends instead of handing over to a shell.
    pub close_at_end: bool,
    /// Who asked for the pane, in the words its tab's card says them ("Claude (t4)"); `None` for
    /// a pane the user opened.
    pub opener: Option<String>,
}

/// The split tree's twin whose leaves are pane **indices** (see the module doc).
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Leaf(usize),
    Split {
        axis: Axis,
        ratio: f64,
        first: Box<Shape>,
        second: Box<Shape>,
    },
}

impl Shape {
    /// The tree with each identity replaced by its position in `order` (the tab's pane list).
    /// `None` if a leaf is not in `order` or the leaves are not exactly `order`'s identities.
    /// A ratio outside `(0, 1)` is written as an even split rather than dropping the tab: the
    /// reader would reject it, and losing the whole layout over one divider is the worse wrong.
    pub fn from_tree(tree: &Tree, order: &[u64]) -> Option<Shape> {
        let shape = Self::from_tree_inner(tree, order)?;
        shape.covers(order.len()).then_some(shape)
    }

    fn from_tree_inner(tree: &Tree, order: &[u64]) -> Option<Shape> {
        Some(match tree {
            Tree::Leaf(id) => Shape::Leaf(order.iter().position(|o| o == id)?),
            Tree::Split {
                axis,
                ratio,
                first,
                second,
            } => Shape::Split {
                axis: *axis,
                ratio: if valid_ratio(*ratio) { *ratio } else { 0.5 },
                first: Box::new(Self::from_tree_inner(first, order)?),
                second: Box::new(Self::from_tree_inner(second, order)?),
            },
        })
    }

    /// The tree with each index replaced by `ids[index]` — the restored panes' new identities.
    /// `None` if an index is out of range.
    pub fn to_tree(&self, ids: &[u64]) -> Option<Tree> {
        Some(match self {
            Shape::Leaf(index) => Tree::Leaf(*ids.get(*index)?),
            Shape::Split {
                axis,
                ratio,
                first,
                second,
            } => Tree::Split {
                axis: *axis,
                ratio: *ratio,
                first: Box::new(first.to_tree(ids)?),
                second: Box::new(second.to_tree(ids)?),
            },
        })
    }

    /// The leaves in tree order.
    pub fn leaves(&self) -> Vec<usize> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect(&self, out: &mut Vec<usize>) {
        match self {
            Shape::Leaf(index) => out.push(*index),
            Shape::Split { first, second, .. } => {
                first.collect(out);
                second.collect(out);
            }
        }
    }

    /// Whether the leaves are exactly `0..count`, each once.
    pub(crate) fn covers(&self, count: usize) -> bool {
        let mut leaves = self.leaves();
        leaves.sort_unstable();
        leaves.len() == count && leaves.iter().enumerate().all(|(i, leaf)| i == *leaf)
    }

    pub(crate) fn render(&self, out: &mut String) {
        match self {
            Shape::Leaf(index) => out.push_str(&format!(" L {index}")),
            Shape::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let axis = match axis {
                    Axis::Horizontal => "h",
                    Axis::Vertical => "v",
                };
                out.push_str(&format!(" S {axis} {ratio}"));
                first.render(out);
                second.render(out);
            }
        }
    }

    /// Reads one pre-order tree from `tokens`; the depth bound keeps a hostile file from
    /// recursing without limit.
    pub(crate) fn parse<'a>(
        tokens: &mut impl Iterator<Item = &'a str>,
        depth: usize,
    ) -> Option<Shape> {
        if depth > MAX_DEPTH {
            return None;
        }
        match tokens.next()? {
            "L" => Some(Shape::Leaf(tokens.next()?.parse().ok()?)),
            "S" => {
                let axis = match tokens.next()? {
                    "h" => Axis::Horizontal,
                    "v" => Axis::Vertical,
                    _ => return None,
                };
                let ratio: f64 = tokens.next()?.parse().ok()?;
                if !valid_ratio(ratio) {
                    return None;
                }
                let first = Box::new(Shape::parse(tokens, depth + 1)?);
                let second = Box::new(Shape::parse(tokens, depth + 1)?);
                Some(Shape::Split {
                    axis,
                    ratio,
                    first,
                    second,
                })
            }
            _ => None,
        }
    }
}

impl Shape {
    /// The tree without the leaves `keep` says no to (`keep[index]`; an index past its end is
    /// dropped), each kept index counted again over the kept ones: a split left with one side
    /// becomes that side. `None` if no leaf is left.
    fn retain(&self, keep: &[bool]) -> Option<Shape> {
        match self {
            Shape::Leaf(index) => keep
                .get(*index)
                .copied()
                .unwrap_or(false)
                .then(|| Shape::Leaf(keep[..*index].iter().filter(|kept| **kept).count())),
            Shape::Split {
                axis,
                ratio,
                first,
                second,
            } => match (first.retain(keep), second.retain(keep)) {
                (Some(first), Some(second)) => Some(Shape::Split {
                    axis: *axis,
                    ratio: *ratio,
                    first: Box::new(first),
                    second: Box::new(second),
                }),
                (Some(side), None) | (None, Some(side)) => Some(side),
                (None, None) => None,
            },
        }
    }
}

impl Saved {
    /// This layout without the panes `taken` names — they come back where an
    /// earlier layout placed them — and without a tab or a window left
    /// empty; every pane it keeps joins `taken`. Several holders' layouts can
    /// name one program (a holder's, and the holder of the bateri that took
    /// it and crashed): walked newest first, each comes once.
    pub fn place_after(&self, taken: &mut Vec<PaneUuid>) -> Saved {
        let mut windows = Vec::new();
        for window in &self.windows {
            let mut tabs = Vec::new();
            let mut kept = Vec::new();
            let mut selected = None;
            for (index, tab) in window.tabs.iter().enumerate() {
                let keep: Vec<bool> = tab
                    .panes
                    .iter()
                    .map(|pane| !taken.contains(&pane.uuid))
                    .collect();
                let Some(tab) = tab.retain(&keep) else {
                    continue;
                };
                taken.extend(tab.panes.iter().map(|pane| pane.uuid.clone()));
                if index == window.selected {
                    selected = Some(tabs.len());
                }
                tabs.push(tab);
                kept.push(index);
            }
            if tabs.is_empty() {
                continue;
            }
            let (groups, grouped) = window.keeping(&kept);
            for (tab, group) in tabs.iter_mut().zip(grouped) {
                tab.group = group;
            }
            windows.push(SavedWindow {
                frame: window.frame,
                tabs,
                selected: selected.unwrap_or(0),
                key: window.key,
                side_width: window.side_width,
                groups,
            });
        }
        Saved { windows }
    }
}

impl SavedTab {
    /// The tab without the panes `keep` says no to (`keep` runs over `panes`): the tree loses
    /// their leaves ([`Shape`]'s split with one side left becomes that side), the indices count
    /// again, the focus moves to the first pane left if its own went and the zoom goes with its
    /// pane. `None` if no pane is left — a pane that cannot come back takes nothing else with it.
    pub fn retain(&self, keep: &[bool]) -> Option<SavedTab> {
        let shape = self.shape.retain(keep)?;
        let kept = |index: usize| keep.get(index).copied().unwrap_or(false);
        let moved = |index: usize| keep[..index].iter().filter(|kept| **kept).count();
        let panes: Vec<SavedPane> = self
            .panes
            .iter()
            .enumerate()
            .filter(|(index, _)| kept(*index))
            .map(|(_, pane)| pane.clone())
            .collect();
        Some(SavedTab {
            shape,
            focused: if kept(self.focused) {
                moved(self.focused)
            } else {
                0
            },
            zoomed: self.zoomed.filter(|index| kept(*index)).map(moved),
            panes,
            name: self.name.clone(),
            group: self.group,
        })
    }
}

/// A live window's groups as the save keeps them, over the tabs that are saved: `kept` names
/// them (ids, in strip order). A group none of them is in is left out, the others count again,
/// and a group's resident that is not saved is forgotten. The second list is the saved group of
/// each tab of `kept` ([`SavedTab::group`]).
pub fn saved_groups<K: Copy + Eq>(
    groups: &Groups<K>,
    kept: &[K],
) -> (Vec<SavedGroup>, Vec<Option<usize>>) {
    let mut saved = Vec::new();
    let mut grouped = vec![None; kept.len()];
    for group in groups.groups() {
        let members: Vec<usize> = (0..kept.len())
            .filter(|&index| groups.group_of(kept[index]) == Some(group.id))
            .collect();
        if members.is_empty() {
            continue;
        }
        for index in members {
            grouped[index] = Some(saved.len());
        }
        saved.push(SavedGroup {
            name: group.name.clone(),
            collapsed: group.collapsed,
            resident: group
                .resident()
                .and_then(|tab| kept.iter().position(|&candidate| candidate == tab)),
        });
    }
    (saved, grouped)
}

impl SavedWindow {
    /// The groups over the tabs that were built again: `built` pairs a tab's index here with the
    /// id it was given. A group none of them is in is left out; a resident that was not built is
    /// forgotten. Tabs left out leave the rest a grouping of the strip — a part of one is.
    pub fn parts(&self, built: &[(usize, u64)]) -> Vec<Part<u64>> {
        let group_of = |index: usize| self.tabs.get(index).and_then(|tab| tab.group);
        self.groups
            .iter()
            .enumerate()
            .filter_map(|(at, group)| {
                let tabs: Vec<u64> = built
                    .iter()
                    .filter(|&&(index, _)| group_of(index) == Some(at))
                    .map(|&(_, id)| id)
                    .collect();
                if tabs.is_empty() {
                    return None;
                }
                Some(Part {
                    name: group.name.clone(),
                    collapsed: group.collapsed,
                    resident: group.resident.and_then(|tab| {
                        built
                            .iter()
                            .find(|&&(index, _)| index == tab)
                            .map(|&(_, id)| id)
                    }),
                    tabs,
                })
            })
            .collect()
    }

    /// The groups once only the tabs `kept` (their indices here, in order) remain: a group none of
    /// them is in is gone, the others count again, and a group's resident that did not stay is
    /// forgotten. The second list is the new group of each kept tab.
    fn keeping(&self, kept: &[usize]) -> (Vec<SavedGroup>, Vec<Option<usize>>) {
        let mut groups = Vec::new();
        let mut moved = Vec::new();
        for (at, group) in self.groups.iter().enumerate() {
            if kept.iter().any(|&old| self.tabs[old].group == Some(at)) {
                moved.push(Some(groups.len()));
                groups.push(SavedGroup {
                    name: group.name.clone(),
                    collapsed: group.collapsed,
                    resident: group
                        .resident
                        .and_then(|tab| kept.iter().position(|&old| old == tab)),
                });
            } else {
                moved.push(None);
            }
        }
        let grouped = kept
            .iter()
            .map(|&old| {
                self.tabs[old]
                    .group
                    .and_then(|group| moved.get(group).copied().flatten())
            })
            .collect();
        (groups, grouped)
    }

    /// The window's strip and groups as the models the rule lives in, tabs named by their index;
    /// `None` when the groups are no grouping of the strip.
    fn grouping(&self) -> Option<(Tabs<usize>, Groups<usize>)> {
        let count = self.tabs.len();
        if count == 0
            || self
                .tabs
                .iter()
                .any(|tab| tab.group.is_some_and(|group| group >= self.groups.len()))
        {
            return None;
        }
        let mut tabs = Tabs::new(0);
        for index in 1..count {
            tabs.append(index);
        }
        tabs.select(self.selected);
        let parts = self
            .groups
            .iter()
            .enumerate()
            .map(|(at, group)| Part {
                name: group.name.clone(),
                collapsed: group.collapsed,
                resident: group.resident,
                tabs: (0..count)
                    .filter(|&index| self.tabs[index].group == Some(at))
                    .collect(),
            })
            .collect();
        let groups = Groups::from_parts(&tabs, parts)?;
        Some((tabs, groups))
    }
}

/// How deep a read tree may nest. Far above any pane count the minimum pane size allows on a
/// screen; only a bound against a hostile file, not a product limit.
const MAX_DEPTH: usize = 64;

fn valid_ratio(ratio: f64) -> bool {
    ratio.is_finite() && ratio > 0.0 && ratio < 1.0
}

// ─── the format ──────────────────────────────────────────────────────────

/// Escapes a text field: `\` → `\\`, space → `\s`, tab → `\t`, line feed → `\n`, carriage
/// return → `\r`. Everything else passes as is (UTF-8 included).
pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            ' ' => out.push_str("\\s"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out
}

/// [`escape`]'s inverse; an unknown escape or a trailing `\` is `None`.
pub(crate) fn unescape(text: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        out.push(match chars.next()? {
            '\\' => '\\',
            's' => ' ',
            't' => '\t',
            'n' => '\n',
            'r' => '\r',
            _ => return None,
        });
    }
    Some(out)
}

/// An optional text field: `-` absent, `+{escaped}` present.
fn render_optional(text: Option<&str>) -> String {
    match text {
        None => "-".to_owned(),
        Some(text) => format!("+{}", escape(text)),
    }
}

fn parse_optional(token: &str) -> Option<Option<String>> {
    if token == "-" {
        return Some(None);
    }
    Some(Some(unescape(token.strip_prefix('+')?)?))
}

fn parse_bit(token: &str) -> Option<bool> {
    match token {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    }
}

impl Saved {
    /// The file's text. Only a model that [`Saved::parse`] accepts round-trips; the caller builds
    /// it from live windows, which are always consistent.
    pub fn render(&self) -> String {
        let sided = self
            .windows
            .iter()
            .any(|window| window.side_width.is_some());
        let named = self
            .windows
            .iter()
            .flat_map(|window| &window.tabs)
            .any(|tab| tab.name.is_some());
        let grouped = self.windows.iter().any(|window| !window.groups.is_empty());
        let kept_terminals = || {
            self.windows
                .iter()
                .flat_map(|window| &window.tabs)
                .flat_map(|tab| &tab.panes)
                .filter_map(|pane| pane.kept.as_ref())
        };
        let asked_for = kept_terminals().any(|kept| kept.close_at_end || kept.opener.is_some());
        let kept = kept_terminals().next().is_some();
        let version = if asked_for {
            VERSION
        } else if kept {
            KEPT
        } else if grouped {
            GROUPED
        } else if sided {
            SIDED
        } else if named {
            NAMED
        } else {
            UNNAMED
        };
        let mut out = format!("{HEADER} {version}\n");
        for window in &self.windows {
            let Frame {
                x,
                y,
                width,
                height,
            } = window.frame;
            out.push_str(&format!(
                "W {x} {y} {width} {height} {} {}",
                window.selected,
                u8::from(window.key)
            ));
            if version >= SIDED {
                match window.side_width {
                    Some(side) => out.push_str(&format!(" {side}")),
                    None => out.push_str(" -"),
                }
            }
            out.push('\n');
            if version >= GROUPED {
                for group in &window.groups {
                    out.push_str(&format!(
                        "G {} {} {}\n",
                        escape(&group.name),
                        u8::from(group.collapsed),
                        group.resident.map_or("-".to_owned(), |tab| tab.to_string()),
                    ));
                }
            }
            for tab in &window.tabs {
                let zoomed = tab.zoomed.map_or("-".to_owned(), |z| z.to_string());
                out.push_str(&format!("T {} {zoomed}", tab.focused));
                // The name sits before the shape: a field of fixed place, where the shape is a
                // run of tokens of any length.
                if version >= NAMED {
                    out.push(' ');
                    out.push_str(&render_optional(tab.name.as_deref()));
                }
                // The group sits after the name for the same reason.
                if version >= GROUPED {
                    out.push(' ');
                    out.push_str(&tab.group.map_or("-".to_owned(), |group| group.to_string()));
                }
                tab.shape.render(&mut out);
                out.push('\n');
                for pane in &tab.panes {
                    let dir = pane.dir.as_deref().and_then(Path::to_str);
                    out.push_str(&format!(
                        "P {} {} {} {} {}",
                        pane.uuid.as_str(),
                        pane.zoom_steps,
                        u8::from(pane.history),
                        render_optional(dir),
                        render_optional(pane.remote_line.as_deref()),
                    ));
                    // The kept terminal ends the line: its arguments are a run of tokens of
                    // any length.
                    if version >= KEPT {
                        match &pane.kept {
                            None => out.push_str(" -"),
                            Some(kept) => {
                                out.push_str(&format!(
                                    " +{} {}",
                                    escape(&kept.account),
                                    kept.terminal.as_str()
                                ));
                                // Fields of fixed place, before the arguments' run.
                                if version >= VERSION {
                                    out.push_str(&format!(
                                        " {} {}",
                                        u8::from(kept.close_at_end),
                                        render_optional(kept.opener.as_deref())
                                    ));
                                }
                                for arg in &kept.args {
                                    out.push(' ');
                                    out.push_str(&escape(arg));
                                }
                            }
                        }
                    }
                    out.push('\n');
                }
            }
        }
        out
    }

    /// Reads the file's text; anything not exactly understood is `None` (module doc).
    pub fn parse(text: &str) -> Option<Saved> {
        let mut lines = text.split('\n');
        let mut header = lines.next()?.split(' ');
        if header.next()? != HEADER {
            return None;
        }
        let version = header.next()?.parse::<u32>().ok()?;
        if ![UNNAMED, NAMED, SIDED, GROUPED, KEPT, VERSION].contains(&version)
            || header.next().is_some()
        {
            return None;
        }
        let mut windows: Vec<SavedWindow> = Vec::new();
        let mut ended = false;
        for line in lines {
            // Only the final line break may leave an empty line.
            if ended {
                return None;
            }
            if line.is_empty() {
                ended = true;
                continue;
            }
            let mut tokens = line.split(' ');
            match tokens.next()? {
                "W" => windows.push(parse_window(&mut tokens, version)?),
                // The groups come before the window's first tab, the tabs name them by index.
                "G" => {
                    let window = windows.last_mut()?;
                    if version < GROUPED || !window.tabs.is_empty() {
                        return None;
                    }
                    window.groups.push(parse_group(&mut tokens)?);
                }
                "T" => windows
                    .last_mut()?
                    .tabs
                    .push(parse_tab(&mut tokens, version)?),
                "P" => windows
                    .last_mut()?
                    .tabs
                    .last_mut()?
                    .panes
                    .push(parse_pane(&mut tokens, version)?),
                _ => return None,
            }
            if tokens.next().is_some() {
                return None;
            }
        }
        if !ended || windows.is_empty() {
            return None;
        }
        windows
            .iter()
            .all(SavedWindow::is_consistent)
            .then_some(Saved { windows })
    }
}

impl SavedWindow {
    fn is_consistent(&self) -> bool {
        self.selected < self.tabs.len()
            && self.tabs.iter().all(SavedTab::is_consistent)
            && self.grouping().is_some()
    }
}

impl SavedTab {
    fn is_consistent(&self) -> bool {
        let count = self.panes.len();
        self.shape.covers(count)
            && self.focused < count
            && self.zoomed.is_none_or(|zoomed| zoomed < count)
    }
}

fn parse_window<'a>(
    tokens: &mut impl Iterator<Item = &'a str>,
    version: u32,
) -> Option<SavedWindow> {
    let mut number = || -> Option<f64> {
        let value: f64 = tokens.next()?.parse().ok()?;
        value.is_finite().then_some(value)
    };
    let frame = Frame {
        x: number()?,
        y: number()?,
        width: number()?,
        height: number()?,
    };
    if frame.width <= 0.0 || frame.height <= 0.0 {
        return None;
    }
    let selected = tokens.next()?.parse().ok()?;
    let key = parse_bit(tokens.next()?)?;
    // Versions before 3 have no width: the column is the default.
    let side_width = if version >= SIDED {
        match tokens.next()? {
            "-" => None,
            side => {
                let side: f64 = side.parse().ok()?;
                if !side.is_finite() || side <= 0.0 {
                    return None;
                }
                Some(side)
            }
        }
    } else {
        None
    };
    Some(SavedWindow {
        frame,
        tabs: Vec::new(),
        selected,
        key,
        side_width,
        groups: Vec::new(),
    })
}

/// A `G` line's fields after the letter: the name (escaped), whether the group is closed, and the
/// index of the tab it was last on (`-` for none).
fn parse_group<'a>(tokens: &mut impl Iterator<Item = &'a str>) -> Option<SavedGroup> {
    let name = unescape(tokens.next()?)?;
    if name.trim().is_empty() {
        return None;
    }
    Some(SavedGroup {
        name,
        collapsed: parse_bit(tokens.next()?)?,
        resident: match tokens.next()? {
            "-" => None,
            index => Some(index.parse().ok()?),
        },
    })
}

fn parse_tab<'a>(tokens: &mut impl Iterator<Item = &'a str>, version: u32) -> Option<SavedTab> {
    let focused = tokens.next()?.parse().ok()?;
    let zoomed = match tokens.next()? {
        "-" => None,
        index => Some(index.parse().ok()?),
    };
    // Version 1 has no name field. An empty name is no name: the title it would show is blank.
    let name = if version >= NAMED {
        parse_optional(tokens.next()?)?.filter(|name| !name.is_empty())
    } else {
        None
    };
    // Versions before 4 have no groups: every tab is ungrouped.
    let group = if version >= GROUPED {
        match tokens.next()? {
            "-" => None,
            index => Some(index.parse().ok()?),
        }
    } else {
        None
    };
    let shape = Shape::parse(tokens, 0)?;
    Some(SavedTab {
        shape,
        panes: Vec::new(),
        focused,
        zoomed,
        name,
        group,
    })
}

fn parse_pane<'a>(tokens: &mut impl Iterator<Item = &'a str>, version: u32) -> Option<SavedPane> {
    let uuid = PaneUuid::parse(tokens.next()?)?;
    let zoom_steps = tokens.next()?.parse().ok()?;
    let history = parse_bit(tokens.next()?)?;
    let dir = parse_optional(tokens.next()?)?.map(PathBuf::from);
    let remote_line = parse_optional(tokens.next()?)?;
    // Versions before 5 have no kept terminal.
    let kept = if version >= KEPT {
        parse_kept(tokens, version)?
    } else {
        None
    };
    Some(SavedPane {
        uuid,
        dir,
        zoom_steps,
        remote_line,
        kept,
        history,
    })
}

/// A `P` line's last fields: `-`, or `+{account}`, the terminal's name, from version 6 whether
/// the pane closes with it and who asked for it, and the `ssh` arguments to the end of the line
/// (at least one: a server is named by something). The outer `None` is a line not understood.
fn parse_kept<'a>(
    tokens: &mut impl Iterator<Item = &'a str>,
    version: u32,
) -> Option<Option<SavedKept>> {
    let first = tokens.next()?;
    if first == "-" {
        return Some(None);
    }
    let account = unescape(first.strip_prefix('+')?)?;
    let terminal = SessionId::new(tokens.next()?).ok()?;
    let (close_at_end, opener) = if version >= VERSION {
        let close_at_end = parse_bit(tokens.next()?)?;
        // An empty name says nobody: the card would say "Opened by" and nothing.
        let opener = parse_optional(tokens.next()?)?.filter(|opener| !opener.is_empty());
        (close_at_end, opener)
    } else {
        (false, None)
    };
    let args = tokens.map(unescape).collect::<Option<Vec<String>>>()?;
    (!account.is_empty() && !args.is_empty()).then_some(Some(SavedKept {
        account,
        terminal,
        args,
        close_at_end,
        opener,
    }))
}

// ─── the directory ───────────────────────────────────────────────────────

/// The production directory: `{application support}/bateri/session/{bundle id}`.
/// Named by the bundle so that a development build and the installed app never share a layout.
pub fn directory(application_support: &Path, bundle_id: &str) -> PathBuf {
    application_support
        .join("bateri")
        .join("session")
        .join(bundle_id)
}

/// Ownership of the directory: an exclusive `flock` on its lock file, released when this drops.
#[derive(Debug)]
pub struct Lock {
    _file: File,
    dir: PathBuf,
}

impl Lock {
    /// The directory this lock owns.
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

/// Creates `dir` (owner only) and takes its lock without waiting; held by another instance, or
/// any I/O failure, is `None` — the caller then neither restores nor saves.
pub fn lock(dir: &Path) -> Option<Lock> {
    fs::DirBuilder::new().recursive(true).create(dir).ok()?;
    // `recursive` applies the mode only to directories it creates; the last one is ours, so its
    // mode is set even if it already existed.
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).ok()?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(dir.join(LOCK))
        .ok()?;
    // SAFETY: `flock` on a descriptor this function owns; no memory is passed.
    let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
    locked.then(|| Lock {
        _file: file,
        dir: dir.to_path_buf(),
    })
}

/// Where the ledger of the terminals kept on servers lives in the session directory
/// (`kablo::ledger::Ledger::open`). It is application state, not a layout: [`clear`], [`take`]
/// and the sweeps leave it, and `restore_windows` does not decide whether it exists.
pub fn ledger_path(dir: &Path) -> PathBuf {
    dir.join(LEDGER)
}

/// The temporary name the ledger is written through ([`write_atomic`]'s rule): a sweep that
/// found it would delete a write in flight.
fn ledger_temporary() -> String {
    format!("{LEDGER}{TEMP_SUFFIX}")
}

fn history_path(dir: &Path, uuid: &PaneUuid) -> PathBuf {
    dir.join(format!("{}.{HISTORY_EXT}", uuid.as_str()))
}

/// Writes `bytes` to `path` through a temporary name (`0600`) and a `rename`; the temporary is
/// removed on failure.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(TEMP_SUFFIX);
    let temporary = path.with_file_name(name);
    let written = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&temporary)
        .and_then(|mut file| {
            // An existing temporary keeps its old mode under `open`; force it.
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
            file.write_all(bytes)
        })
        .and_then(|()| fs::rename(&temporary, path));
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

/// Saves the layout and the histories (`(tab id, bytes)` pairs). Histories first, the layout
/// last — its rename is the commit — then whatever the new layout does not name is swept. A
/// layout without windows is [`clear`]: there is nothing to restore.
pub fn save(lock: &Lock, saved: &Saved, histories: &[(PaneUuid, Vec<u8>)]) -> io::Result<()> {
    if saved.windows.is_empty() {
        return clear(lock);
    }
    write_histories(lock, histories)?;
    write_atomic(&lock.dir.join(LAYOUT), saved.render().as_bytes())?;
    sweep(lock, &kept_histories(saved));
    Ok(())
}

fn write_histories(lock: &Lock, histories: &[(PaneUuid, Vec<u8>)]) -> io::Result<()> {
    for (uuid, bytes) in histories {
        write_atomic(&history_path(&lock.dir, uuid), bytes)?;
    }
    Ok(())
}

/// The history file names a layout refers to.
fn kept_histories(saved: &Saved) -> HashSet<String> {
    saved
        .windows
        .iter()
        .flat_map(|window| &window.tabs)
        .flat_map(|tab| &tab.panes)
        .filter(|pane| pane.history)
        .map(|pane| format!("{}.{HISTORY_EXT}", pane.uuid.as_str()))
        .collect()
}

/// Removes every file in the directory except the lock, the layout and the named histories.
/// Best effort: a file that cannot be removed is left for the next sweep.
fn sweep(lock: &Lock, keep: &HashSet<String>) {
    let Ok(entries) = fs::read_dir(&lock.dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name == LOCK
            || name == LAYOUT
            || name == RECENT
            || name == LEDGER
            || name == ledger_temporary()
            || keep.contains(name)
        {
            continue;
        }
        if entry.file_type().is_ok_and(|kind| kind.is_file()) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Reads the layout and deletes it — before anything is replayed (module doc). A missing or
/// unreadable layout is `None`; one that does not parse is `None` and takes its histories with
/// it. Histories the layout does not name are swept.
///
/// Without `histories` (`restore_windows = "layout"`) every history is swept **unread** and no
/// returned pane claims one: a user who said the scrollback should not come back must not see an
/// earlier `"all"` save's once more.
pub fn take(lock: &Lock, histories: bool) -> Option<Saved> {
    let path = lock.dir.join(LAYOUT);
    let text = fs::read_to_string(&path);
    let _ = fs::remove_file(&path);
    let mut saved = text.ok().as_deref().and_then(Saved::parse);
    if !histories {
        for pane in saved
            .iter_mut()
            .flat_map(|saved| &mut saved.windows)
            .flat_map(|window| &mut window.tabs)
            .flat_map(|tab| &mut tab.panes)
        {
            pane.history = false;
        }
    }
    let keep = saved.as_ref().map(kept_histories).unwrap_or_default();
    sweep(lock, &keep);
    saved
}

/// Reads one pane's history and deletes it; missing or unreadable is `None` (that pane starts
/// empty, the others are not affected).
pub fn history(lock: &Lock, uuid: &PaneUuid) -> Option<Vec<u8>> {
    let path = history_path(&lock.dir, uuid);
    let bytes = fs::read(&path);
    let _ = fs::remove_file(&path);
    bytes.ok()
}

/// Writes the layout alone, every pane's `history` flag low — while bateri runs, so that a
/// crash nobody held the programs through still brings the windows back. No history is written,
/// read or swept: the clean quit's [`save`] overwrites this with the whole save. A layout without
/// windows removes the layout file only.
pub fn save_layout(lock: &Lock, saved: &Saved) -> io::Result<()> {
    let path = lock.dir.join(LAYOUT);
    if saved.windows.is_empty() {
        return match fs::remove_file(path) {
            Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
            _ => Ok(()),
        };
    }
    let mut layout = saved.clone();
    for pane in layout
        .windows
        .iter_mut()
        .flat_map(|window| &mut window.tabs)
        .flat_map(|tab| &mut tab.panes)
    {
        pane.history = false;
    }
    write_atomic(&path, layout.render().as_bytes())
}

/// The palette's Recent text; missing or unreadable is `None`.
pub fn read_recent(lock: &Lock) -> Option<String> {
    fs::read_to_string(lock.dir.join(RECENT)).ok()
}

/// Writes the palette's Recent through a temporary name and a `rename`.
pub fn write_recent(lock: &Lock, text: &str) -> io::Result<()> {
    write_atomic(&lock.dir.join(RECENT), text.as_bytes())
}

/// Removes the palette's Recent (`restore_windows = "off"` deletes what was saved). A layout
/// that is cleared because no window is left does not: the user's last commands are not a
/// window.
pub fn clear_recent(lock: &Lock) -> io::Result<()> {
    match fs::remove_file(lock.dir.join(RECENT)) {
        Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
        _ => Ok(()),
    }
}

/// Removes the layout and every history (`restore_windows = "off"`, no windows at quit).
pub fn clear(lock: &Lock) -> io::Result<()> {
    match fs::remove_file(lock.dir.join(LAYOUT)) {
        Err(err) if err.kind() != io::ErrorKind::NotFound => return Err(err),
        _ => {}
    }
    sweep(lock, &HashSet::new());
    Ok(())
}

// ─── the attempt marker ──────────────────────────────────────────────────

/// The attempt marker's name in an instance directory (the module doc).
const ATTEMPT: &str = "restore-attempt";

/// What a launch does with a holder's programs, from the attempts before it that never
/// settled ([`bump_attempt`]'s answer).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttemptMode {
    /// The first attempt: everything comes back.
    Normal,
    /// The second: the programs are taken, but nothing that went through the parser before the
    /// crash is replayed again — no screen, no state, no carried output, no history.
    Safe,
    /// The third: the programs are given up, nothing is restored, one empty window.
    GiveUp,
}

/// The mode after `previous` unsettled attempts.
pub fn attempt_mode(previous: u32) -> AttemptMode {
    match previous {
        0 => AttemptMode::Normal,
        1 => AttemptMode::Safe,
        _ => AttemptMode::GiveUp,
    }
}

/// Counts one more attempt in `dir` (`0600`, through a temporary name) and returns the count
/// before it. A missing or unreadable marker counts as none — the wrong way is one attempt too
/// many, never a lost program. A write that fails is not an error the launch can act on: the
/// count before it is still the answer.
pub fn bump_attempt(dir: &Path) -> u32 {
    let path = dir.join(ATTEMPT);
    let previous = fs::read_to_string(&path)
        .ok()
        .and_then(|text| text.trim().parse::<u32>().ok())
        .unwrap_or(0);
    let _ = write_atomic(
        &path,
        format!("{}\n", previous.saturating_add(1)).as_bytes(),
    );
    previous
}

/// Removes the marker in `dir`: the launch settled, or bateri quit cleanly.
pub fn clear_attempt(dir: &Path) {
    let _ = fs::remove_file(dir.join(ATTEMPT));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::TempRoot;

    const A: &str = "0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0";
    /// A width with a long fraction: a dragged edge stops at any point.
    const SIDE_SAMPLE: f64 = 213.333_333_333_333_34;
    const B: &str = "11111111-2222-3333-4444-555555555555";
    const C: &str = "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE";

    fn id(text: &str) -> PaneUuid {
        PaneUuid::parse(text).expect("test id must parse")
    }

    fn pane(tab: &str) -> SavedPane {
        SavedPane {
            uuid: id(tab),
            dir: None,
            zoom_steps: 0,
            remote_line: None,
            kept: None,
            history: false,
        }
    }

    fn single(tab: &str) -> SavedTab {
        SavedTab {
            shape: Shape::Leaf(0),
            panes: vec![pane(tab)],
            focused: 0,
            zoomed: None,
            name: None,
            group: None,
        }
    }

    fn window(tabs: Vec<SavedTab>) -> SavedWindow {
        SavedWindow {
            frame: Frame {
                x: 120.5,
                y: -30.0,
                width: 800.0,
                height: 600.25,
            },
            tabs,
            selected: 0,
            key: false,
            side_width: None,
            groups: Vec::new(),
        }
    }

    /// Two windows, a nested split with a zoomed pane, tricky directories and a remote line.
    fn rich() -> Saved {
        let split = SavedTab {
            shape: Shape::Split {
                axis: Axis::Horizontal,
                ratio: 0.3333333333333333,
                first: Box::new(Shape::Leaf(2)),
                second: Box::new(Shape::Split {
                    axis: Axis::Vertical,
                    ratio: 0.71,
                    first: Box::new(Shape::Leaf(0)),
                    second: Box::new(Shape::Leaf(1)),
                }),
            },
            panes: vec![
                SavedPane {
                    uuid: id(A),
                    dir: Some(PathBuf::from("/Users/ömer/My Drive/çalışma\\dosya")),
                    zoom_steps: 3,
                    remote_line: None,
                    kept: None,
                    history: true,
                },
                SavedPane {
                    uuid: id(B),
                    dir: Some(PathBuf::from("/tmp/tab\there\nnew line\rcr -")),
                    zoom_steps: -2,
                    remote_line: Some("ssh -p 2222 prod".to_owned()),
                    kept: None,
                    history: false,
                },
                SavedPane {
                    uuid: id(C),
                    dir: Some(PathBuf::from("")),
                    zoom_steps: 0,
                    remote_line: Some(String::new()),
                    kept: None,
                    history: true,
                },
            ],
            focused: 1,
            zoomed: Some(2),
            name: None,
            group: None,
        };
        let mut first = window(vec![single(B), split]);
        first.selected = 1;
        first.key = true;
        let second = window(vec![single(C)]);
        Saved {
            windows: vec![first, second],
        }
    }

    /// [`rich`] with two tabs named: the model of [`V2_FIXTURE`].
    fn rich_named() -> Saved {
        let mut saved = rich();
        saved.windows[0].tabs[1].name = Some("release notes".to_owned());
        saved.windows[1].tabs[0].name = Some("ünï çode".to_owned());
        saved
    }

    /// [`rich`] with the first window's side column dragged wider: the model of [`V3_FIXTURE`].
    fn rich_sided() -> Saved {
        let mut saved = rich();
        saved.windows[0].side_width = Some(260.5);
        saved
    }

    /// [`rich_sided`] with the first window's tabs gathered: `api server` (the split tab and a
    /// single one, last on the single) and a closed `ünï` (one tab), the first tab ungrouped; the
    /// second window stays a plain list. The model of [`V4_FIXTURE`].
    fn rich_grouped() -> Saved {
        let mut saved = rich_sided();
        let first = &mut saved.windows[0];
        first.tabs.push(single(C));
        first.tabs.push(single(A));
        first.tabs[1].group = Some(0);
        first.tabs[2].group = Some(0);
        first.tabs[3].group = Some(1);
        first.groups = vec![
            SavedGroup {
                name: "api server".to_owned(),
                collapsed: false,
                resident: Some(2),
            },
            SavedGroup {
                name: "ünï".to_owned(),
                collapsed: true,
                resident: None,
            },
        ];
        saved
    }

    /// [`rich_grouped`] with two panes on terminals a server keeps (one with a space and an
    /// unusual argument in what names the server): the model of [`V5_FIXTURE`].
    fn rich_kept() -> Saved {
        let mut saved = rich_grouped();
        let kept = |account: &str, terminal: &str, args: &[&str]| SavedKept {
            account: account.to_owned(),
            terminal: SessionId::new(terminal).expect("an id"),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            close_at_end: false,
            opener: None,
        };
        saved.windows[0].tabs[1].panes[1].kept = Some(kept(
            "ada@prod:2222",
            "b-0123456789abcdef0123456789abcdef",
            &["-p", "2222", "ada@prod"],
        ));
        saved.windows[1].tabs[0].panes[0].kept = Some(kept(
            "root@rasp:22 via jump:22",
            "b-fedcba9876543210fedcba9876543210",
            &["-J", "my jump", "root@rasp"],
        ));
        saved
    }

    /// [`rich_kept`] with its panes asked for by a program on the server: one closes with its
    /// terminal and says who asked (a space in the words), the other was asked to stay open and
    /// says it too. The model of [`V6_FIXTURE`].
    fn rich_asked() -> Saved {
        let mut saved = rich_kept();
        if let Some(kept) = &mut saved.windows[0].tabs[1].panes[1].kept {
            kept.close_at_end = true;
            kept.opener = Some("Claude (t4)".to_owned());
        }
        if let Some(kept) = &mut saved.windows[1].tabs[0].panes[0].kept {
            kept.opener = Some("t12".to_owned());
        }
        saved
    }

    /// Version 1 as the writer of that version produced it for [`rich`], frozen: a bateri that
    /// only knows version 1 reads what an unnamed session is still written as, and this
    /// bateri reads what that one wrote. Never updated — a change of the format is a new
    /// version with a new fixture beside this one.
    const V1_FIXTURE: &str = concat!(
        "bateri-session 1\n",
        "W 120.5 -30 800 600.25 1 1\n",
        "T 0 - L 0\n",
        "P 11111111-2222-3333-4444-555555555555 0 0 - -\n",
        "T 1 2 S h 0.3333333333333333 L 2 S v 0.71 L 0 L 1\n",
        r"P 0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0 3 1 +/Users/ömer/My\sDrive/çalışma\\dosya -",
        "\n",
        r"P 11111111-2222-3333-4444-555555555555 -2 0 +/tmp/tab\there\nnew\sline\rcr\s- +ssh\s-p\s2222\sprod",
        "\n",
        "P AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE 0 1 + +\n",
        "W 120.5 -30 800 600.25 0 0\n",
        "T 0 - L 0\n",
        "P AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE 0 0 - -\n",
    );

    /// Version 2 for [`rich_named`], frozen the same way: the name is the third field of a `T`
    /// line, `-` where a tab has none.
    const V2_FIXTURE: &str = concat!(
        "bateri-session 2\n",
        "W 120.5 -30 800 600.25 1 1\n",
        "T 0 - - L 0\n",
        "P 11111111-2222-3333-4444-555555555555 0 0 - -\n",
        r"T 1 2 +release\snotes S h 0.3333333333333333 L 2 S v 0.71 L 0 L 1",
        "\n",
        r"P 0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0 3 1 +/Users/ömer/My\sDrive/çalışma\\dosya -",
        "\n",
        r"P 11111111-2222-3333-4444-555555555555 -2 0 +/tmp/tab\there\nnew\sline\rcr\s- +ssh\s-p\s2222\sprod",
        "\n",
        "P AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE 0 1 + +\n",
        "W 120.5 -30 800 600.25 0 0\n",
        "T 0 - +ünï\\sçode L 0\n",
        "P AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE 0 0 - -\n",
    );

    /// Version 3 for [`rich_sided`], frozen the same way: the width is the seventh field of a `W`
    /// line, `-` where a window keeps the default, and the `T` lines carry version 2's name field.
    const V3_FIXTURE: &str = concat!(
        "bateri-session 3\n",
        "W 120.5 -30 800 600.25 1 1 260.5\n",
        "T 0 - - L 0\n",
        "P 11111111-2222-3333-4444-555555555555 0 0 - -\n",
        "T 1 2 - S h 0.3333333333333333 L 2 S v 0.71 L 0 L 1\n",
        r"P 0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0 3 1 +/Users/ömer/My\sDrive/çalışma\\dosya -",
        "\n",
        r"P 11111111-2222-3333-4444-555555555555 -2 0 +/tmp/tab\there\nnew\sline\rcr\s- +ssh\s-p\s2222\sprod",
        "\n",
        "P AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE 0 1 + +\n",
        "W 120.5 -30 800 600.25 0 0 -\n",
        "T 0 - - L 0\n",
        "P AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE 0 0 - -\n",
    );

    /// Version 4 for [`rich_grouped`], frozen the same way: the `G` lines follow their window's
    /// `W` line, and every `T` line carries its group's index after the name, `-` where it has
    /// none (a window without groups too).
    const V4_FIXTURE: &str = concat!(
        "bateri-session 4\n",
        "W 120.5 -30 800 600.25 1 1 260.5\n",
        "G api\\sserver 0 2\n",
        "G ünï 1 -\n",
        "T 0 - - - L 0\n",
        "P 11111111-2222-3333-4444-555555555555 0 0 - -\n",
        "T 1 2 - 0 S h 0.3333333333333333 L 2 S v 0.71 L 0 L 1\n",
        r"P 0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0 3 1 +/Users/ömer/My\sDrive/çalışma\\dosya -",
        "\n",
        r"P 11111111-2222-3333-4444-555555555555 -2 0 +/tmp/tab\there\nnew\sline\rcr\s- +ssh\s-p\s2222\sprod",
        "\n",
        "P AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE 0 1 + +\n",
        "T 0 - - 0 L 0\n",
        "P AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE 0 0 - -\n",
        "T 0 - - 1 L 0\n",
        "P 0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0 0 0 - -\n",
        "W 120.5 -30 800 600.25 0 0 -\n",
        "T 0 - - - L 0\n",
        "P AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE 0 0 - -\n",
    );

    /// Version 5 for [`rich_kept`], frozen the same way: every `P` line ends with the kept
    /// terminal, `-` where the pane shows none, else `+{account}`, the terminal's name and the
    /// `ssh` arguments to the end of the line.
    const V5_FIXTURE: &str = concat!(
        "bateri-session 5\n",
        "W 120.5 -30 800 600.25 1 1 260.5\n",
        "G api\\sserver 0 2\n",
        "G ünï 1 -\n",
        "T 0 - - - L 0\n",
        "P 11111111-2222-3333-4444-555555555555 0 0 - - -\n",
        "T 1 2 - 0 S h 0.3333333333333333 L 2 S v 0.71 L 0 L 1\n",
        r"P 0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0 3 1 +/Users/ömer/My\sDrive/çalışma\\dosya - -",
        "\n",
        r"P 11111111-2222-3333-4444-555555555555 -2 0 +/tmp/tab\there\nnew\sline\rcr\s- +ssh\s-p\s2222\sprod +ada@prod:2222 b-0123456789abcdef0123456789abcdef -p 2222 ada@prod",
        "\n",
        "P AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE 0 1 + + -\n",
        "T 0 - - 0 L 0\n",
        "P AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE 0 0 - - -\n",
        "T 0 - - 1 L 0\n",
        "P 0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0 0 0 - - -\n",
        "W 120.5 -30 800 600.25 0 0 -\n",
        "T 0 - - - L 0\n",
        r"P AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE 0 0 - - +root@rasp:22\svia\sjump:22 b-fedcba9876543210fedcba9876543210 -J my\sjump root@rasp",
        "\n",
    );

    /// Version 6 for [`rich_asked`], frozen the same way: a kept terminal's name is followed by
    /// whether its pane closes with it and who asked for the pane, then the `ssh` arguments.
    const V6_FIXTURE: &str = concat!(
        "bateri-session 6\n",
        "W 120.5 -30 800 600.25 1 1 260.5\n",
        "G api\\sserver 0 2\n",
        "G ünï 1 -\n",
        "T 0 - - - L 0\n",
        "P 11111111-2222-3333-4444-555555555555 0 0 - - -\n",
        "T 1 2 - 0 S h 0.3333333333333333 L 2 S v 0.71 L 0 L 1\n",
        r"P 0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0 3 1 +/Users/ömer/My\sDrive/çalışma\\dosya - -",
        "\n",
        r"P 11111111-2222-3333-4444-555555555555 -2 0 +/tmp/tab\there\nnew\sline\rcr\s- +ssh\s-p\s2222\sprod +ada@prod:2222 b-0123456789abcdef0123456789abcdef 1 +Claude\s(t4) -p 2222 ada@prod",
        "\n",
        "P AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE 0 1 + + -\n",
        "T 0 - - 0 L 0\n",
        "P AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE 0 0 - - -\n",
        "T 0 - - 1 L 0\n",
        "P 0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0 0 0 - - -\n",
        "W 120.5 -30 800 600.25 0 0 -\n",
        "T 0 - - - L 0\n",
        r"P AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE 0 0 - - +root@rasp:22\svia\sjump:22 b-fedcba9876543210fedcba9876543210 0 +t12 -J my\sjump root@rasp",
        "\n",
    );

    #[test]
    fn the_previous_version_is_read_byte_for_byte() {
        assert_eq!(Saved::parse(V1_FIXTURE), Some(rich()));
        assert_eq!(Saved::parse(V2_FIXTURE), Some(rich_named()));
        assert_eq!(Saved::parse(V3_FIXTURE), Some(rich_sided()));
        assert_eq!(Saved::parse(V4_FIXTURE), Some(rich_grouped()));
        assert_eq!(Saved::parse(V5_FIXTURE), Some(rich_kept()));
    }

    #[test]
    fn the_current_version_is_read_byte_for_byte() {
        assert_eq!(Saved::parse(V6_FIXTURE), Some(rich_asked()));
    }

    /// The writer says the oldest version that holds the session — the one before names while no
    /// tab is named, no column was resized and no group exists, the one before widths while only
    /// names are, the one before groups while a width is too — so the bateri that only knew that
    /// version still reads it; the current one as soon as a window has a group.
    #[test]
    fn an_unnamed_session_is_written_as_the_previous_version() {
        assert_eq!(rich().render(), V1_FIXTURE);
        assert_eq!(rich_named().render(), V2_FIXTURE);
        assert_eq!(rich_sided().render(), V3_FIXTURE);
        assert_eq!(rich_grouped().render(), V4_FIXTURE);
        assert_eq!(rich_kept().render(), V5_FIXTURE);
        assert_eq!(rich_asked().render(), V6_FIXTURE);
    }

    /// A pane a program asked for is what makes the file version 6, either half of it alone: a
    /// pane that closes with its terminal, a pane that says who asked. Without both the same
    /// session is written as version 5, which the bateri before this one reads.
    #[test]
    fn a_session_with_no_pane_a_program_asked_for_is_written_as_version_five() {
        let only = |close_at_end: bool, opener: Option<&str>| {
            let mut saved = rich_kept();
            if let Some(kept) = &mut saved.windows[1].tabs[0].panes[0].kept {
                kept.close_at_end = close_at_end;
                kept.opener = opener.map(str::to_owned);
            }
            saved.render()
        };
        assert!(only(true, None).starts_with("bateri-session 6\n"));
        assert!(only(false, Some("t4")).starts_with("bateri-session 6\n"));
        assert_eq!(only(false, None), V5_FIXTURE);
    }

    /// A pane on a kept terminal is what makes the file version 5 and nothing else does: take
    /// it away and the same session is written as the one before, which the bateri before this
    /// one reads.
    #[test]
    fn a_session_with_no_kept_terminal_is_not_written_as_version_five() {
        let mut saved = rich_kept();
        for pane in saved
            .windows
            .iter_mut()
            .flat_map(|window| &mut window.tabs)
            .flat_map(|tab| &mut tab.panes)
        {
            pane.kept = None;
        }
        assert_eq!(saved.render(), V4_FIXTURE);
    }

    /// A kept terminal that is not whole is a layout not understood: an account with nothing to
    /// name the server by, a name no terminal has, a field missing, and the field in a file whose
    /// version has none.
    #[test]
    fn a_kept_terminal_that_is_not_whole_drops_the_layout() {
        let account = "+ada@prod:2222 b-0123456789abcdef0123456789abcdef -p 2222 ada@prod";
        for (from, to) in [
            (account, "+ada@prod:2222 b-0123456789abcdef0123456789abcdef"),
            (account, "+ada@prod:2222"),
            (account, "+"),
            (account, "+ada@prod:2222 not/a/name ada@prod"),
            (
                account,
                "+ada@prod:2222 b-0123456789abcdef0123456789abcdef bad\\x",
            ),
            (
                account,
                "ada@prod:2222 b-0123456789abcdef0123456789abcdef ada@prod",
            ),
        ] {
            assert!(V5_FIXTURE.contains(from), "{from:?}");
            let text = V5_FIXTURE.replacen(from, to, 1);
            assert_eq!(Saved::parse(&text), None, "{to:?}");
        }
        assert!(Saved::parse(V5_FIXTURE).is_some(), "the control");
        // A file of version 4 has no such field: one there is a stray token.
        let text = V4_FIXTURE.replacen(
            "P 11111111-2222-3333-4444-555555555555 0 0 - -\n",
            "P 11111111-2222-3333-4444-555555555555 0 0 - - -\n",
            1,
        );
        assert_eq!(Saved::parse(&text), None);
    }

    /// Version 6's two fields are whole or the layout is not understood: a bit that is no bit, the
    /// words without their mark, a field missing. An empty name is nobody.
    #[test]
    fn a_program_s_pane_that_is_not_whole_drops_the_layout() {
        let fields = "b-0123456789abcdef0123456789abcdef 1 +Claude\\s(t4) -p";
        for to in [
            "b-0123456789abcdef0123456789abcdef 2 +Claude\\s(t4) -p",
            "b-0123456789abcdef0123456789abcdef 1 Claude\\s(t4) -p",
            "b-0123456789abcdef0123456789abcdef +Claude\\s(t4) -p",
            "b-0123456789abcdef0123456789abcdef 1",
        ] {
            assert!(V6_FIXTURE.contains(fields), "{fields:?}");
            let text = V6_FIXTURE.replacen(fields, to, 1);
            assert_eq!(Saved::parse(&text), None, "{to:?}");
        }
        let nobody = V6_FIXTURE.replacen(" 0 +t12 ", " 0 + ", 1);
        let read = Saved::parse(&nobody).expect("an empty name is a layout understood");
        let kept = read.windows[1].tabs[0].panes[0]
            .kept
            .as_ref()
            .expect("kept");
        assert_eq!(kept.opener, None);
        // Version 5 has no such fields: there the words read as the first argument.
        let text = V5_FIXTURE.replacen(
            "b-fedcba9876543210fedcba9876543210 -J",
            "b-fedcba9876543210fedcba9876543210 0 +t12 -J",
            1,
        );
        let read = Saved::parse(&text).expect("arguments");
        let kept = read.windows[1].tabs[0].panes[0]
            .kept
            .as_ref()
            .expect("kept");
        assert_eq!(kept.args[..2], ["0", "+t12"]);
        assert!(!kept.close_at_end);
    }

    /// A width is a positive number of points or `-`; anything else is a layout not understood.
    #[test]
    fn a_side_width_that_is_no_width_drops_the_layout() {
        for side in ["0", "-4", "inf", "NaN", "wide", "+"] {
            let text = V3_FIXTURE.replace("1 1 260.5\n", &format!("1 1 {side}\n"));
            assert_eq!(Saved::parse(&text), None, "{side}");
        }
        for side in ["0", "-4", "inf", "NaN", "wide", "+"] {
            let text = V4_FIXTURE.replace("1 1 260.5\n", &format!("1 1 {side}\n"));
            assert_eq!(Saved::parse(&text), None, "{side}");
        }
        // Version 2's window line has no width field; one there is a stray token.
        let text = V2_FIXTURE.replace("600.25 1 1\n", "600.25 1 1 260.5\n");
        assert_eq!(Saved::parse(&text), None);
    }

    #[test]
    fn format_round_trips() {
        let mut both = rich_named();
        both.windows[1].side_width = Some(SIDE_SAMPLE);
        for (saved, header) in [
            (rich(), "bateri-session 1\n"),
            (rich_named(), "bateri-session 2\n"),
            (rich_sided(), "bateri-session 3\n"),
            (both, "bateri-session 3\n"),
            (rich_grouped(), "bateri-session 4\n"),
            (rich_kept(), "bateri-session 5\n"),
            (rich_asked(), "bateri-session 6\n"),
        ] {
            let text = saved.render();
            assert!(text.starts_with(header), "{text}");
            assert_eq!(Saved::parse(&text), Some(saved));
        }
    }

    /// A name is whatever the user typed: it is escaped like a path, so spaces, escapes, a
    /// leading `-` or `+` and non-ASCII text come back as written, and it stays on its line.
    #[test]
    fn a_name_round_trips_whatever_it_holds() {
        for name in [
            "-",
            "+",
            "a b",
            "\\s",
            "ğüşiöç",
            "x\\",
            "\t\n\r \\",
            " lead and trail ",
        ] {
            let mut saved = rich();
            saved.windows[1].tabs[0].name = Some(name.to_owned());
            let text = saved.render();
            assert_eq!(
                text.lines().count(),
                rich().render().lines().count(),
                "{name:?}"
            );
            assert_eq!(Saved::parse(&text), Some(saved), "{name:?}");
        }
    }

    /// An empty name would show a blank title: read as no name.
    #[test]
    fn an_empty_name_is_no_name() {
        let text = V2_FIXTURE.replace("T 0 - - L 0", "T 0 - + L 0");
        assert_eq!(Saved::parse(&text), Some(rich_named()));
    }

    /// Retaining some panes of a tab keeps the tab's name with it.
    #[test]
    fn retaining_panes_keeps_the_name() {
        let tab = rich_named().windows[0].tabs[1].clone();
        let kept = tab
            .retain(&[true, false, true])
            .expect("two panes are left");
        assert_eq!(kept.name.as_deref(), Some("release notes"));
    }

    /// Retaining some panes of a tab keeps the tab's group with it.
    #[test]
    fn retaining_panes_keeps_the_group() {
        let tab = rich_grouped().windows[0].tabs[1].clone();
        let kept = tab
            .retain(&[true, false, true])
            .expect("two panes are left");
        assert_eq!(kept.group, Some(0));
    }

    /// A group's name is what the user typed, escaped like a path, and stays on its line.
    #[test]
    fn a_group_name_round_trips_whatever_it_holds() {
        for name in [
            "-",
            "+",
            "a b",
            "\\s",
            "ğüşiöç",
            "x\\",
            "\t\n\r \\",
            " lead and trail ",
        ] {
            let mut saved = rich_grouped();
            saved.windows[0].groups[0].name = name.to_owned();
            let text = saved.render();
            assert_eq!(
                text.lines().count(),
                rich_grouped().render().lines().count(),
                "{name:?}"
            );
            assert_eq!(Saved::parse(&text), Some(saved), "{name:?}");
        }
    }

    /// The reader asks the groups' own rule whether what is saved is a grouping of the strip, and
    /// a layout that is not is not read — a half-understood grouping would put tabs in the wrong
    /// group.
    #[test]
    fn groups_that_are_no_grouping_of_the_strip_drop_the_layout() {
        let read = |from: &str, to: &str| {
            assert!(V4_FIXTURE.contains(from), "{from:?}");
            Saved::parse(&V4_FIXTURE.replacen(from, to, 1))
        };
        // The sanity of the helper: a replacement that changes nothing reads.
        assert_eq!(read("G ünï 1 -", "G ünï 1 -"), Some(rich_grouped()));
        // A listed group no tab is in, a tab in a group that is not listed.
        assert_eq!(read("T 0 - - 1 L 0", "T 0 - - 0 L 0"), None);
        assert_eq!(read("T 0 - - 1 L 0", "T 0 - - 2 L 0"), None);
        assert_eq!(read("T 0 - - 1 L 0", "T 0 - - x L 0"), None);
        // An ungrouped tab behind a group's tabs, or a group's behind the next group's.
        assert_eq!(read("T 0 - - - L 0\nP 1", "T 0 - - 1 L 0\nP 1"), None);
        assert_eq!(read("T 1 2 - 0 S", "T 1 2 - 1 S"), None);
        // A resident that is another group's, or no tab, or no number; a closed flag that is no flag.
        assert_eq!(read("G api\\sserver 0 2", "G api\\sserver 0 3"), None);
        assert_eq!(read("G api\\sserver 0 2", "G api\\sserver 0 9"), None);
        assert_eq!(read("G api\\sserver 0 2", "G api\\sserver 0 x"), None);
        assert_eq!(read("G api\\sserver 0 2", "G api\\sserver 2 2"), None);
        // A resident that is an ungrouped tab.
        assert_eq!(read("G api\\sserver 0 2", "G api\\sserver 0 0"), None);
        // A blank name, a missing name, a group line with a field too many or too few.
        assert_eq!(read("G api\\sserver 0 2", "G \\s 0 2"), None);
        assert_eq!(read("G api\\sserver 0 2", "G  0 2"), None);
        assert_eq!(read("G api\\sserver 0 2", "G api 0 2 -"), None);
        assert_eq!(read("G api\\sserver 0 2", "G api 0"), None);
        // A group line after the window's first tab, or in a version that has no groups.
        assert_eq!(
            read("T 0 - - - L 0\nP 1", "T 0 - - - L 0\nG late 0 -\nP 1"),
            None
        );
        let before = V4_FIXTURE.replacen("bateri-session 4", "bateri-session 3", 1);
        assert_eq!(Saved::parse(&before), None);
        // A window of a grouped file with no group of its own is fine: its tabs say `-`.
        assert!(rich_grouped().windows[1].groups.is_empty());
    }

    /// Tabs another layout placed already leave this one; a group keeps the tabs that stay, counts
    /// again, forgets a resident that left, and goes with its last tab.
    #[test]
    fn placing_after_another_layout_regroups_what_stays() {
        let uuid = |n: u32| format!("00000000-0000-0000-0000-{n:012}");
        let mut tabs: Vec<SavedTab> = (1..=5).map(|n| single(&uuid(n))).collect();
        for (tab, group) in tabs
            .iter_mut()
            .zip([None, Some(0), Some(0), Some(1), Some(1)])
        {
            tab.group = group;
        }
        let mut grouped = window(tabs);
        grouped.selected = 1;
        grouped.groups = vec![
            SavedGroup {
                name: "a".to_owned(),
                collapsed: false,
                resident: Some(2),
            },
            SavedGroup {
                name: "b".to_owned(),
                collapsed: true,
                resident: Some(3),
            },
        ];
        let saved = Saved {
            windows: vec![grouped],
        };
        assert_eq!(Saved::parse(&saved.render()), Some(saved.clone()));
        let id = |n: u32| id(&uuid(n));
        // The second tab and the fourth are taken: `a` keeps its resident, `b` loses its own.
        let placed = saved.place_after(&mut vec![id(2), id(4)]);
        let window = &placed.windows[0];
        assert_eq!(window.tabs.len(), 3);
        assert_eq!(
            window.tabs.iter().map(|tab| tab.group).collect::<Vec<_>>(),
            [None, Some(0), Some(1)]
        );
        assert_eq!(window.selected, 0, "the selected tab went: the first stays");
        assert_eq!(
            window
                .groups
                .iter()
                .map(|group| (group.name.as_str(), group.collapsed, group.resident))
                .collect::<Vec<_>>(),
            [("a", false, Some(1)), ("b", true, None)]
        );
        assert_eq!(Saved::parse(&placed.render()), Some(placed.clone()));
        // Both of `b`'s tabs: the group goes, and with no group left the layout is a plain list.
        let placed = saved.place_after(&mut vec![id(4), id(5)]);
        assert_eq!(placed.windows[0].groups.len(), 1);
        assert_eq!(placed.windows[0].groups[0].name, "a");
        assert_eq!(Saved::parse(&placed.render()), Some(placed));
        let placed = saved.place_after(&mut vec![id(2), id(3), id(4), id(5)]);
        assert!(placed.windows[0].groups.is_empty());
        assert!(placed.render().starts_with("bateri-session 1\n"));
    }

    /// The groups a live window saves are the ones over the tabs that are saved: a tab that is
    /// left out keeps its group's others, a group it emptied is not written, and a resident that
    /// is not saved is forgotten.
    #[test]
    fn a_live_windows_groups_are_saved_over_the_tabs_that_are_saved() {
        use crate::groups::Target;
        let mut tabs = Tabs::new(1u64);
        for tab in 2..=8 {
            tabs.append(tab);
        }
        let mut groups = Groups::new();
        groups.place(&mut tabs, &[1, 2, 3], Target::New("api".to_owned()));
        groups.place(&mut tabs, &[4, 5], Target::New("web".to_owned()));
        groups.select(&mut tabs, 2);
        // The strip reads `6 7 8 | 1 2 3 | 4 5`, `api` last left on 2.
        let (saved, of) = saved_groups(&groups, tabs.ids());
        let group = |name: &str, resident| SavedGroup {
            name: name.to_owned(),
            collapsed: false,
            resident,
        };
        assert_eq!(saved, [group("api", Some(4)), group("web", None)]);
        assert_eq!(
            of,
            [
                None,
                None,
                None,
                Some(0),
                Some(0),
                Some(0),
                Some(1),
                Some(1)
            ]
        );
        // Tab 2 and `web`'s tabs are not saved.
        let (saved, of) = saved_groups(&groups, &[6, 7, 8, 1, 3]);
        assert_eq!(saved, [group("api", None)]);
        assert_eq!(of, [None, None, None, Some(0), Some(0)]);
        // No group: no saved group, whatever the strip.
        let (saved, of) = saved_groups(&Groups::<u64>::new(), &[1, 2]);
        assert!(saved.is_empty());
        assert_eq!(of, [None, None]);
    }

    /// What a restoring window builds its groups from: the saved ones over the tabs that were
    /// built again, which can be fewer.
    #[test]
    fn the_groups_of_the_tabs_that_came_back_are_the_saved_ones_over_those() {
        let saved = rich_grouped().windows[0].clone();
        let everything: Vec<(usize, u64)> =
            (0..4).map(|index| (index, 10 + index as u64)).collect();
        let parts = saved.parts(&everything);
        assert_eq!(
            parts
                .iter()
                .map(|part| (
                    part.name.as_str(),
                    part.collapsed,
                    part.resident,
                    part.tabs.clone()
                ))
                .collect::<Vec<_>>(),
            [
                ("api server", false, Some(12), vec![11, 12]),
                ("ünï", true, None, vec![13])
            ]
        );
        // The last two tabs did not come back: `api server` keeps its first, loses its resident,
        // and `ünï` is gone. What is left is a grouping of the strip it came from.
        let parts = saved.parts(&[(0, 10), (1, 11)]);
        assert_eq!(parts.len(), 1);
        assert_eq!((parts[0].tabs.clone(), parts[0].resident), (vec![11], None));
        let mut tabs = Tabs::new(10u64);
        tabs.append(11);
        assert!(Groups::from_parts(&tabs, parts).is_some());
        // A window with no group has no part.
        assert!(rich().windows[0].parts(&[(0, 10)]).is_empty());
    }

    #[test]
    fn escaping_round_trips_and_keeps_fields_apart() {
        for text in ["", "-", "+", "a b", "\\s", "ğüşiöç", "x\\", "\t\n\r \\"] {
            let escaped = escape(text);
            assert!(!escaped.contains([' ', '\t', '\n', '\r']), "{escaped:?}");
            assert_eq!(unescape(&escaped).as_deref(), Some(text));
        }
        assert_eq!(unescape("a\\"), None);
        assert_eq!(unescape("\\q"), None);
        // Absent and empty stay apart.
        assert_eq!(parse_optional(&render_optional(None)), Some(None));
        assert_eq!(
            parse_optional(&render_optional(Some(""))),
            Some(Some(String::new()))
        );
        assert_eq!(parse_optional("x"), None);
    }

    #[test]
    fn rejections_are_none() {
        let good = rich().render();
        let replaced = |from: &str, to: &str| {
            assert!(good.contains(from), "fixture lacks {from:?}");
            good.replacen(from, to, 1)
        };
        let cases = [
            // Unknown version, wrong header.
            replaced("bateri-session 1", "bateri-session 3"),
            replaced("bateri-session 1", "bateri-session 0"),
            replaced("bateri-session 1", "bateri-sessions 1"),
            replaced("bateri-session 1", "bateri-session 1 x"),
            // Truncated: no final line break, cut mid-file, empty.
            good.trim_end_matches('\n').to_owned(),
            good[..good.len() / 2].to_owned(),
            String::new(),
            "bateri-session 1\n".to_owned(),
            // Broken tree: unknown axis, ratio out of range, missing subtree, extra token.
            replaced(" S h ", " S x "),
            replaced("0.71", "1"),
            replaced("0.71", "0"),
            replaced("0.71", "NaN"),
            replaced(" L 1\n", "\n"),
            replaced(" L 1\n", " L 1 L 0\n"),
            // Inconsistent indices: a leaf out of range, a repeated leaf, focus, zoom, selected.
            replaced(" L 1\n", " L 3\n"),
            replaced(" L 1\n", " L 0\n"),
            replaced("T 1 2", "T 3 2"),
            replaced("T 1 2", "T 1 5"),
            replaced(" 1 1\n", " 2 1\n"),
            // A pane too many / too few for the tree.
            replaced(&format!("P {B}"), &format!("P {A} 0 0 - -\nP {B}")),
            // Broken fields: bad id, bad bit, bad escape, bad frame.
            replaced(A, "not-a-uuid"),
            replaced(" 3 1 +", " 3 2 +"),
            replaced("My\\sDrive", "My\\qDrive"),
            replaced("800", "-800"),
            replaced("120.5", "inf"),
            // A pane or tab before its parent, an unknown line, a blank line inside.
            good.replacen("\nW", "\nP 0 0 0 - -\nW", 1),
            replaced("\nT", "\nX\nT"),
            replaced("\nT", "\n\nT"),
        ];
        for text in cases {
            assert_eq!(Saved::parse(&text), None, "{text}");
        }
        // Version 2 without its name field, version 1 with one, a name that does not escape.
        let named = rich_named().render();
        for text in [
            named.replacen("T 0 - - L 0", "T 0 - L 0", 1),
            named.replacen("+release\\snotes", "release", 1),
            named.replacen("+release\\snotes", "+release\\q", 1),
            good.replacen("T 0 - L 0", "T 0 - +x L 0", 1),
            named.replacen("bateri-session 2", "bateri-session 3", 1),
        ] {
            assert_eq!(Saved::parse(&text), None, "{text}");
        }
        // Nothing panics on arbitrary prefixes and garbage either.
        for text in [&good, &named] {
            for end in 0..text.len() {
                if text.is_char_boundary(end) {
                    let _ = Saved::parse(&text[..end]);
                }
            }
        }
        let deep = format!(
            "bateri-session 1\nW 0 0 1 1 0 0\nT 0 -{}\n",
            " S h 0.5".repeat(10_000)
        );
        assert_eq!(Saved::parse(&deep), None);
    }

    #[test]
    fn tree_conversion_maps_identities_to_indices_and_back() {
        let tree = Tree::Split {
            axis: Axis::Vertical,
            ratio: 0.4,
            first: Box::new(Tree::Leaf(42)),
            second: Box::new(Tree::Split {
                axis: Axis::Horizontal,
                ratio: 0.5,
                first: Box::new(Tree::Leaf(7)),
                second: Box::new(Tree::Leaf(9)),
            }),
        };
        let shape = Shape::from_tree(&tree, &[7, 9, 42]).expect("all leaves known");
        assert_eq!(shape.leaves(), vec![2, 0, 1]);
        let restored = shape.to_tree(&[100, 101, 102]).expect("indices in range");
        assert_eq!(restored.leaves(), vec![102, 100, 101]);
        // A leaf not in the order, an order with an extra pane, an index out of range.
        assert_eq!(Shape::from_tree(&tree, &[7, 9]), None);
        assert_eq!(Shape::from_tree(&tree, &[7, 9, 42, 5]), None);
        assert_eq!(shape.to_tree(&[1, 2]), None);
        // An invalid ratio becomes an even split instead of losing the tab.
        let odd = Tree::Split {
            axis: Axis::Horizontal,
            ratio: 1.5,
            first: Box::new(Tree::Leaf(1)),
            second: Box::new(Tree::Leaf(2)),
        };
        assert!(matches!(
            Shape::from_tree(&odd, &[1, 2]),
            Some(Shape::Split { ratio, .. }) if ratio == 0.5
        ));
    }

    /// A pane carried from one tab to another leaves both tabs saved whole,
    /// and the carried pane's identity (its `bateri://tab/` URL, its history
    /// file) is in the tab it went to: what a crash restores is the layout the
    /// move made.
    #[test]
    fn a_moved_pane_is_saved_in_the_tab_it_went_to() {
        use crate::split::{Direction, Removal, Room, Size, Spacing};

        let ids = [
            A,
            B,
            C,
            "22222222-3333-4444-5555-666666666666",
            "33333333-4444-5555-6666-777777777777",
        ];
        let leaf = |n: u64| Box::new(Tree::Leaf(n));
        let split = |axis, ratio, first, second| Tree::Split {
            axis,
            ratio,
            first,
            second,
        };
        // Tab one: 1 | (2 / 3). Tab two: 4 | 5. Pane 3 goes under pane 5.
        let mut source = split(
            Axis::Horizontal,
            0.4,
            leaf(1),
            Box::new(split(Axis::Vertical, 0.5, leaf(2), leaf(3))),
        );
        let target = split(Axis::Horizontal, 0.5, leaf(4), leaf(5));
        assert!(matches!(source.remove(3), Removal::Removed { .. }));
        let min = |_: u64| Size::new(80.0, 60.0);
        let room = Room {
            bounds: crate::split::Rect::new(0.0, 0.0, 1000.0, 700.0),
            scale: 2.0,
            spacing: Spacing::gapped(6.0, 6.0, 2.0),
            min: &min,
        };
        let placed = target
            .plan_beside(5, Direction::Down, &Tree::Leaf(3), &room)
            .expect("room for one pane");
        let saved_tab = |tree: &Tree| {
            let order = tree.leaves();
            SavedTab {
                shape: Shape::from_tree(tree, &order).expect("every leaf is known"),
                panes: order.iter().map(|&n| pane(ids[n as usize - 1])).collect(),
                focused: 0,
                zoomed: None,
                name: None,
                group: None,
            }
        };
        let saved = Saved {
            windows: vec![window(vec![saved_tab(&source), saved_tab(&placed.tree)])],
        };
        let back = Saved::parse(&saved.render()).expect("the layout reads back");
        assert_eq!(back, saved);
        let [first, second] = &back.windows[0].tabs[..] else {
            panic!("two tabs");
        };
        let held = |tab: &SavedTab| -> Vec<PaneUuid> {
            tab.panes.iter().map(|pane| pane.uuid.clone()).collect()
        };
        assert!(!held(first).contains(&id(ids[2])));
        assert!(held(second).contains(&id(ids[2])));
        // Both shapes rebuild the trees the move made.
        assert_eq!(first.shape.to_tree(&source.leaves()), Some(source));
        assert_eq!(
            second.shape.to_tree(&placed.tree.leaves()),
            Some(placed.tree)
        );
    }

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).expect("metadata").permissions().mode() & 0o777
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .expect("read_dir")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .into_string()
                    .expect("utf-8")
            })
            .collect();
        names.sort();
        names
    }

    #[test]
    fn file_life() {
        let root = TempRoot::new("restore-life");
        let dir = directory(&root.0, "dev.bateri.test");
        assert!(dir.ends_with("bateri/session/dev.bateri.test"));
        let lock = lock(&dir).expect("first lock");
        assert_eq!(mode(&dir), 0o700);
        // A second owner (another instance) is refused while the first holds it.
        assert!(super::lock(&dir).is_none());

        // An orphan from an earlier session and a leftover temporary.
        fs::write(history_path(&dir, &id(B)), b"old").expect("orphan");
        fs::write(dir.join("layout.tmp"), b"half").expect("temporary");

        let saved = rich();
        let histories = vec![(id(A), b"\x1b[1mhello\r\n".to_vec()), (id(C), Vec::new())];
        save(&lock, &saved, &histories).expect("save");
        assert_eq!(
            names(&dir),
            vec![
                format!("{A}.vt"),
                format!("{C}.vt"),
                "layout".into(),
                "lock".into()
            ]
        );
        for name in [format!("{A}.vt"), "layout".to_owned()] {
            assert_eq!(mode(&dir.join(name)), 0o600);
        }

        // `take` returns the layout and deletes it; the histories wait for `history`.
        assert_eq!(take(&lock, true), Some(saved));
        assert!(!dir.join(LAYOUT).exists());
        assert_eq!(
            history(&lock, &id(A)).as_deref(),
            Some(&b"\x1b[1mhello\r\n"[..])
        );
        assert_eq!(history(&lock, &id(A)), None, "read once");
        assert_eq!(history(&lock, &id(B)), None);
        // Without a layout, the histories nobody read are orphans and go with the next take.
        assert_eq!(take(&lock, true), None);
        assert_eq!(names(&dir), vec!["lock".to_owned()]);

        // `clear` leaves only the lock.
        save(&lock, &rich(), &histories).expect("save again");
        clear(&lock).expect("clear");
        assert_eq!(names(&dir), vec!["lock".to_owned()]);

        // The lock is released when it drops.
        drop(lock);
        assert!(super::lock(&dir).is_some());
    }

    #[test]
    fn an_interrupted_save_keeps_the_previous_layout() {
        let root = TempRoot::new("restore-interrupted");
        let lock = lock(&root.0.join("s")).expect("lock");
        let first = rich();
        save(&lock, &first, &[(id(A), b"first".to_vec())]).expect("save");
        // The next save wrote its histories and died before the layout's rename.
        write_histories(&lock, &[(id(A), b"second".to_vec())]).expect("histories");
        fs::write(lock.dir().join("layout.tmp"), b"bateri-session 1\nW").expect("temporary");
        assert_eq!(take(&lock, true), Some(first));
        assert!(!lock.dir().join("layout.tmp").exists(), "temporary swept");
        assert_eq!(history(&lock, &id(A)).as_deref(), Some(&b"second"[..]));
    }

    #[test]
    fn a_broken_layout_takes_its_histories_with_it() {
        let root = TempRoot::new("restore-broken");
        let lock = lock(&root.0.join("s")).expect("lock");
        save(&lock, &rich(), &[(id(A), b"x".to_vec())]).expect("save");
        fs::write(lock.dir().join(LAYOUT), b"bateri-session 9\n").expect("overwrite");
        assert_eq!(take(&lock, true), None);
        assert_eq!(names(lock.dir()), vec!["lock".to_owned()]);
    }

    #[test]
    fn a_layout_only_take_deletes_the_histories_unread() {
        let root = TempRoot::new("restore-layout-only");
        let lock = lock(&root.0.join("s")).expect("lock");
        let saved = rich();
        assert!(
            saved
                .windows
                .iter()
                .flat_map(|window| &window.tabs)
                .flat_map(|tab| &tab.panes)
                .any(|pane| pane.history),
            "the fixture claims a history"
        );
        save(&lock, &saved, &[(id(A), b"secret".to_vec())]).expect("save");
        let taken = take(&lock, false).expect("the layout still comes back");
        assert_eq!(names(lock.dir()), vec!["lock".to_owned()]);
        assert!(
            taken
                .windows
                .iter()
                .flat_map(|window| &window.tabs)
                .flat_map(|tab| &tab.panes)
                .all(|pane| !pane.history)
        );
        assert_eq!(history(&lock, &id(A)), None);
        // Everything but the history bit is the save's.
        let mut expected = saved;
        for pane in expected
            .windows
            .iter_mut()
            .flat_map(|window| &mut window.tabs)
            .flat_map(|tab| &mut tab.panes)
        {
            pane.history = false;
        }
        assert_eq!(taken, expected);
    }

    /// The ledger of kept terminals is not a layout: a clear, a take and the sweeps leave it
    /// (and the name it is written through), because `restore_windows = "off"` is no reason to
    /// forget which terminals on servers are still to be ended.
    #[test]
    fn the_ledger_of_kept_terminals_outlasts_every_clear_and_sweep() {
        let root = TempRoot::new("ledger-sweep");
        let lock = lock(&directory(&root.0, "dev.bateri.test")).expect("the lock");
        let ledger = ledger_path(lock.dir());
        let writing = lock.dir().join("kablo.tmp");
        fs::write(&ledger, "bateri-kablo 1\n").expect("write");
        fs::write(&writing, "half").expect("write");
        fs::write(lock.dir().join("stray"), "x").expect("write");
        clear(&lock).expect("clear");
        let _ = take(&lock, true);
        sweep(&lock, &HashSet::new());
        assert!(ledger.exists() && writing.exists());
        assert!(!lock.dir().join("stray").exists());
    }

    #[test]
    fn the_palettes_recent_lives_beside_the_layout_and_outlasts_its_saves() {
        let root = TempRoot::new("restore-recent");
        let lock = lock(&root.0.join("s")).expect("lock");
        assert_eq!(read_recent(&lock), None);
        write_recent(&lock, "bateri-palette-recent 1\ncmd.newTab\n").expect("write");
        assert_eq!(
            read_recent(&lock).as_deref(),
            Some("bateri-palette-recent 1\ncmd.newTab\n")
        );
        // Neither a save, a take, a sweep nor a clear of the layout touches it.
        save(&lock, &rich(), &[(id(A), b"x".to_vec())]).expect("save");
        let _ = take(&lock, true);
        clear(&lock).expect("clear");
        save(
            &lock,
            &Saved {
                windows: Vec::new(),
            },
            &[],
        )
        .expect("empty save");
        assert!(read_recent(&lock).is_some());
        assert_eq!(
            names(lock.dir()),
            vec!["lock".to_owned(), "palette-recent".to_owned()]
        );
        // Only its own removal deletes it, and removing what is not there is not an error.
        clear_recent(&lock).expect("clear recent");
        clear_recent(&lock).expect("again");
        assert_eq!(read_recent(&lock), None);
    }

    #[test]
    fn no_windows_is_a_clear() {
        let root = TempRoot::new("restore-empty");
        let lock = lock(&root.0.join("s")).expect("lock");
        save(&lock, &rich(), &[(id(A), b"x".to_vec())]).expect("save");
        save(
            &lock,
            &Saved {
                windows: Vec::new(),
            },
            &[],
        )
        .expect("empty save");
        assert_eq!(names(lock.dir()), vec!["lock".to_owned()]);
        assert_eq!(take(&lock, true), None);
    }

    #[test]
    fn a_layout_written_while_running_leaves_the_histories_alone() {
        let root = TempRoot::new("restore-running");
        let lock = lock(&root.0.join("s")).expect("lock");
        save(&lock, &rich(), &[(id(A), b"kept".to_vec())]).expect("save");
        let mut later = rich();
        later.windows.truncate(1);
        save_layout(&lock, &later).expect("save layout");
        // The history file is untouched; the layout claims none.
        assert_eq!(
            fs::read(history_path(lock.dir(), &id(A))).expect("history"),
            b"kept"
        );
        assert_eq!(mode(&lock.dir().join(LAYOUT)), 0o600);
        let taken = take(&lock, true).expect("the layout comes back");
        assert_eq!(taken.windows.len(), 1);
        assert!(
            taken
                .windows
                .iter()
                .flat_map(|window| &window.tabs)
                .flat_map(|tab| &tab.panes)
                .all(|pane| !pane.history)
        );
        // Without windows only the layout goes.
        save_layout(&lock, &later).expect("save layout");
        fs::write(history_path(lock.dir(), &id(B)), b"other").expect("history");
        save_layout(
            &lock,
            &Saved {
                windows: Vec::new(),
            },
        )
        .expect("empty layout");
        assert!(!lock.dir().join(LAYOUT).exists());
        assert!(history_path(lock.dir(), &id(B)).exists());
    }

    #[test]
    fn a_tab_loses_panes_and_keeps_its_shape() {
        let tab = rich().windows[0].tabs[1].clone();
        // Leaves: 2 | (0 / 1); focus 1, zoom 2.
        assert_eq!(tab.retain(&[true, true, true]), Some(tab.clone()));
        // Without pane 2 the outer split goes: what is left is the inner one, reindexed.
        let without_two = tab.retain(&[true, true, false]).expect("two panes left");
        assert_eq!(
            without_two.shape,
            Shape::Split {
                axis: Axis::Vertical,
                ratio: 0.71,
                first: Box::new(Shape::Leaf(0)),
                second: Box::new(Shape::Leaf(1)),
            }
        );
        assert_eq!((without_two.focused, without_two.zoomed), (1, None));
        assert_eq!(without_two.panes, tab.panes[..2]);
        // Without pane 0: the inner split collapses to pane 1, which is index 0 now.
        let without_zero = tab.retain(&[false, true, true]).expect("two panes left");
        assert_eq!(
            without_zero.shape,
            Shape::Split {
                axis: Axis::Horizontal,
                ratio: 0.3333333333333333,
                first: Box::new(Shape::Leaf(1)),
                second: Box::new(Shape::Leaf(0)),
            }
        );
        assert_eq!((without_zero.focused, without_zero.zoomed), (0, Some(1)));
        // The focused pane goes: the focus moves to the first left.
        let without_one = tab.retain(&[true, false, true]).expect("two panes left");
        assert_eq!(without_one.focused, 0);
        assert!(without_one.shape.covers(without_one.panes.len()));
        assert_eq!(tab.retain(&[false, false, false]), None);
        assert_eq!(tab.retain(&[]), None);
    }

    #[test]
    fn the_attempt_marker_counts_until_cleared() {
        let root = TempRoot::new("restore-attempt");
        let dir = root.0.join("instance");
        fs::create_dir_all(&dir).expect("dir");
        assert_eq!(bump_attempt(&dir), 0);
        assert_eq!(mode(&dir.join(ATTEMPT)), 0o600);
        assert_eq!(bump_attempt(&dir), 1);
        assert_eq!(bump_attempt(&dir), 2);
        assert_eq!(bump_attempt(&dir), 3);
        clear_attempt(&dir);
        assert_eq!(bump_attempt(&dir), 0);
        fs::write(dir.join(ATTEMPT), b"garbage").expect("garbage");
        assert_eq!(bump_attempt(&dir), 0, "an unreadable marker counts as none");
        assert_eq!(
            [0, 1, 2, 7].map(attempt_mode),
            [
                AttemptMode::Normal,
                AttemptMode::Safe,
                AttemptMode::GiveUp,
                AttemptMode::GiveUp
            ]
        );
    }

    #[test]
    fn a_pane_two_layouts_name_comes_once_where_the_first_places_it() {
        let newest = Saved {
            windows: vec![window(vec![single(A), single(B)]), window(vec![single(C)])],
        };
        let mut taken = Vec::new();
        assert_eq!(newest.place_after(&mut taken), newest, "nothing taken yet");
        assert_eq!(
            taken,
            [id(A), id(B), id(C)],
            "every pane of the first layout"
        );
        // An older layout: B alone in a tab (taken), a tab of A beside a new
        // pane, and a window of C only (taken).
        let new = "22222222-3333-4444-5555-666666666666";
        let mut older_split = single(A);
        older_split.panes.push(pane(new));
        older_split.shape = Shape::Split {
            axis: Axis::Vertical,
            ratio: 0.5,
            first: Box::new(Shape::Leaf(0)),
            second: Box::new(Shape::Leaf(1)),
        };
        let mut first = window(vec![single(B), older_split]);
        first.selected = 1;
        let older = Saved {
            windows: vec![first, window(vec![single(C)])],
        };
        let placed = older.place_after(&mut taken);
        assert_eq!(placed.windows.len(), 1, "the window of C alone went");
        let window = &placed.windows[0];
        assert_eq!(window.tabs.len(), 1, "the tab of B alone went");
        assert_eq!(window.selected, 0, "the selected tab moved with the tabs");
        assert_eq!(window.tabs[0].panes, vec![pane(new)]);
        assert_eq!(window.tabs[0].shape, Shape::Leaf(0));
        assert!(taken.contains(&id(new)));
        // Nothing left to place.
        assert_eq!(older.place_after(&mut taken).windows, Vec::new());
    }
}
