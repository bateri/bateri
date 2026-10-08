//! Session restore: the saved layout's model, its versioned line format and the life of
//! the directory that holds it.
//!
//! **The model** is what comes back after a quit: windows (frame, selected tab,
//! key), tabs (the split tree's shape, focus, zoom) and panes (the persistent `TabId`, the local
//! directory, the point-size step, the remote target's line, whether a history file was written).
//! The tree's leaves are **indices** into the tab's pane list ([`Shape`]): the in-process `u64`
//! pane identity means nothing in the next process, so it never reaches the disk —
//! [`Shape::from_tree`] turns identities into indices when saving, [`Shape::to_tree`] indices into
//! the new identities when restoring.
//!
//! **The format** follows `remote-hosts` (`ssh_wrap`): line based, the first line names the
//! version ([`HEADER`] + [`VERSION`]), then one `W` line per window, one `T` line per tab under it
//! and one `P` line per pane under that; the tree is a pre-order token run (`S h|v {ratio} … L
//! {index}`). **Two versions are read**, the current one and the one before it: version 2 gave a
//! tab its own name (a `T` line's third field), version 1 has none. The writer says version 1
//! whenever no tab is named, so a session nobody named stays readable by the bateri that only
//! knew version 1 — whose reader drops any other number and, with it, a holder's programs.
//! A version's frozen text stays in the tests: its reader must keep reading it. Text fields (directory, remote line) are escaped so that a field never holds a
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
//! Histories no layout names, and leftover temporaries, are swept. [`save_layout`] writes the
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

use bt_core::TabId;

use crate::split::{Axis, Tree};

/// The first line's word; the version follows it after one space.
pub const HEADER: &str = "bateri-session";

/// The format's current version: a tab may carry its own name. The previous one ([`UNNAMED`]) is
/// read too; any other number is not read (a newer bateri's file after a downgrade is dropped, not
/// misread).
pub const VERSION: u32 = 2;

/// The version before names, which a session with no named tab is still written in.
const UNNAMED: u32 = 1;

/// The layout file's name inside the directory.
const LAYOUT: &str = "layout";
/// The lock file's name: the layout cannot carry the lock, every save renames a new inode onto
/// its path (`ssh_wrap`'s lock precedent).
const LOCK: &str = "lock";
/// A history file's extension: `{TabId}.vt`.
const HISTORY_EXT: &str = "vt";
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
}

/// A pane.
#[derive(Clone, Debug, PartialEq)]
pub struct SavedPane {
    /// The persistent identity (`TERM_SESSION_ID`, `bateri://tab/<id>`); also the history
    /// file's name.
    pub tab_id: TabId,
    /// The local working directory (OSC 7). A path that is not UTF-8 is saved as absent.
    pub dir: Option<PathBuf>,
    /// The temporary point-size offset, in steps (`zoom::Zoom::steps`).
    pub zoom_steps: i32,
    /// The remote target's command line (`ssh prod`), restored as a ready, not run, first input.
    pub remote_line: Option<String>,
    /// Whether a history file was written for this pane.
    pub history: bool,
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
    fn covers(&self, count: usize) -> bool {
        let mut leaves = self.leaves();
        leaves.sort_unstable();
        leaves.len() == count && leaves.iter().enumerate().all(|(i, leaf)| i == *leaf)
    }

    fn render(&self, out: &mut String) {
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
    fn parse<'a>(tokens: &mut impl Iterator<Item = &'a str>, depth: usize) -> Option<Shape> {
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
    pub fn place_after(&self, taken: &mut Vec<TabId>) -> Saved {
        let mut windows = Vec::new();
        for window in &self.windows {
            let mut tabs = Vec::new();
            let mut selected = None;
            for (index, tab) in window.tabs.iter().enumerate() {
                let keep: Vec<bool> = tab
                    .panes
                    .iter()
                    .map(|pane| !taken.contains(&pane.tab_id))
                    .collect();
                let Some(tab) = tab.retain(&keep) else {
                    continue;
                };
                taken.extend(tab.panes.iter().map(|pane| pane.tab_id.clone()));
                if index == window.selected {
                    selected = Some(tabs.len());
                }
                tabs.push(tab);
            }
            if tabs.is_empty() {
                continue;
            }
            windows.push(SavedWindow {
                frame: window.frame,
                tabs,
                selected: selected.unwrap_or(0),
                key: window.key,
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
        })
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
fn escape(text: &str) -> String {
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
fn unescape(text: &str) -> Option<String> {
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
        let named = self
            .windows
            .iter()
            .flat_map(|window| &window.tabs)
            .any(|tab| tab.name.is_some());
        let version = if named { VERSION } else { UNNAMED };
        let mut out = format!("{HEADER} {version}\n");
        for window in &self.windows {
            let Frame {
                x,
                y,
                width,
                height,
            } = window.frame;
            out.push_str(&format!(
                "W {x} {y} {width} {height} {} {}\n",
                window.selected,
                u8::from(window.key)
            ));
            for tab in &window.tabs {
                let zoomed = tab.zoomed.map_or("-".to_owned(), |z| z.to_string());
                out.push_str(&format!("T {} {zoomed}", tab.focused));
                // The name sits before the shape: a field of fixed place, where the shape is a
                // run of tokens of any length.
                if named {
                    out.push(' ');
                    out.push_str(&render_optional(tab.name.as_deref()));
                }
                tab.shape.render(&mut out);
                out.push('\n');
                for pane in &tab.panes {
                    let dir = pane.dir.as_deref().and_then(Path::to_str);
                    out.push_str(&format!(
                        "P {} {} {} {} {}\n",
                        pane.tab_id.as_str(),
                        pane.zoom_steps,
                        u8::from(pane.history),
                        render_optional(dir),
                        render_optional(pane.remote_line.as_deref()),
                    ));
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
        if ![UNNAMED, VERSION].contains(&version) || header.next().is_some() {
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
                "W" => windows.push(parse_window(&mut tokens)?),
                "T" => windows
                    .last_mut()?
                    .tabs
                    .push(parse_tab(&mut tokens, version)?),
                "P" => windows
                    .last_mut()?
                    .tabs
                    .last_mut()?
                    .panes
                    .push(parse_pane(&mut tokens)?),
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
        self.selected < self.tabs.len() && self.tabs.iter().all(SavedTab::is_consistent)
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

fn parse_window<'a>(tokens: &mut impl Iterator<Item = &'a str>) -> Option<SavedWindow> {
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
    Some(SavedWindow {
        frame,
        tabs: Vec::new(),
        selected: tokens.next()?.parse().ok()?,
        key: parse_bit(tokens.next()?)?,
    })
}

fn parse_tab<'a>(tokens: &mut impl Iterator<Item = &'a str>, version: u32) -> Option<SavedTab> {
    let focused = tokens.next()?.parse().ok()?;
    let zoomed = match tokens.next()? {
        "-" => None,
        index => Some(index.parse().ok()?),
    };
    // Version 1 has no name field. An empty name is no name: the title it would show is blank.
    let name = if version >= VERSION {
        parse_optional(tokens.next()?)?.filter(|name| !name.is_empty())
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
    })
}

fn parse_pane<'a>(tokens: &mut impl Iterator<Item = &'a str>) -> Option<SavedPane> {
    Some(SavedPane {
        tab_id: TabId::parse(tokens.next()?)?,
        zoom_steps: tokens.next()?.parse().ok()?,
        history: parse_bit(tokens.next()?)?,
        dir: parse_optional(tokens.next()?)?.map(PathBuf::from),
        remote_line: parse_optional(tokens.next()?)?,
    })
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

fn history_path(dir: &Path, tab_id: &TabId) -> PathBuf {
    dir.join(format!("{}.{HISTORY_EXT}", tab_id.as_str()))
}

/// Writes `bytes` to `path` through a temporary name (`0600`) and a `rename`; the temporary is
/// removed on failure.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
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
pub fn save(lock: &Lock, saved: &Saved, histories: &[(TabId, Vec<u8>)]) -> io::Result<()> {
    if saved.windows.is_empty() {
        return clear(lock);
    }
    write_histories(lock, histories)?;
    write_atomic(&lock.dir.join(LAYOUT), saved.render().as_bytes())?;
    sweep(lock, &kept_histories(saved));
    Ok(())
}

fn write_histories(lock: &Lock, histories: &[(TabId, Vec<u8>)]) -> io::Result<()> {
    for (tab_id, bytes) in histories {
        write_atomic(&history_path(&lock.dir, tab_id), bytes)?;
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
        .map(|pane| format!("{}.{HISTORY_EXT}", pane.tab_id.as_str()))
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
        if name == LOCK || name == LAYOUT || keep.contains(name) {
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
pub fn history(lock: &Lock, tab_id: &TabId) -> Option<Vec<u8>> {
    let path = history_path(&lock.dir, tab_id);
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
    const B: &str = "11111111-2222-3333-4444-555555555555";
    const C: &str = "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE";

    fn id(text: &str) -> TabId {
        TabId::parse(text).expect("test id must parse")
    }

    fn pane(tab: &str) -> SavedPane {
        SavedPane {
            tab_id: id(tab),
            dir: None,
            zoom_steps: 0,
            remote_line: None,
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
                    tab_id: id(A),
                    dir: Some(PathBuf::from("/Users/ömer/My Drive/çalışma\\dosya")),
                    zoom_steps: 3,
                    remote_line: None,
                    history: true,
                },
                SavedPane {
                    tab_id: id(B),
                    dir: Some(PathBuf::from("/tmp/tab\there\nnew line\rcr -")),
                    zoom_steps: -2,
                    remote_line: Some("ssh -p 2222 prod".to_owned()),
                    history: false,
                },
                SavedPane {
                    tab_id: id(C),
                    dir: Some(PathBuf::from("")),
                    zoom_steps: 0,
                    remote_line: Some(String::new()),
                    history: true,
                },
            ],
            focused: 1,
            zoomed: Some(2),
            name: None,
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

    #[test]
    fn the_previous_version_is_read_byte_for_byte() {
        assert_eq!(Saved::parse(V1_FIXTURE), Some(rich()));
    }

    #[test]
    fn the_current_version_is_read_byte_for_byte() {
        assert_eq!(Saved::parse(V2_FIXTURE), Some(rich_named()));
    }

    /// The writer says the version before names while no tab is named — the bateri that only
    /// knew that version still reads such a session — and the current one as soon as one is.
    #[test]
    fn an_unnamed_session_is_written_as_the_previous_version() {
        assert_eq!(rich().render(), V1_FIXTURE);
        assert_eq!(rich_named().render(), V2_FIXTURE);
    }

    #[test]
    fn format_round_trips() {
        for (saved, header) in [
            (rich(), "bateri-session 1\n"),
            (rich_named(), "bateri-session 2\n"),
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
