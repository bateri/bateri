//! Tabs of one window: the **pure** half of the tab bar. The order of the tabs, which one is
//! selected, what ⌘1…⌘9 reach and where every part of the strip sits horizontally are decided
//! here; the platform shell only builds views at these positions and calls these operations from
//! its single place that changes a window's tabs.
//!
//! It does not see a UI toolkit and has its own tests (`split`'s precedent), so a Linux shell can
//! draw the same strip from the same numbers.
//!
//! **A tab has no `bt_core::PaneUuid`.** A tab is the container of a split tree's panes;
//! `PaneUuid` is one *pane's* identity (`TERM_SESSION_ID`, `bateri://tab/<id>`, the path's word
//! notwithstanding) — a tab holds one or more of them, so the relation is one to many and nothing
//! here assumes otherwise. The identity type is generic: the shell picks whatever names its tabs.
//!
//! **A dragged tab** is pure here too: [`Grip`] turns the pointer's travel after a press into
//! "not yet a drag", "the tab is at this x and would take this place" or "it left the bar"
//! (the slop, the place and the tear-off distance are decided here once), [`Strip::slot_at`]
//! says which slot of a strip is under a point, and [`landing`] what letting go
//! comes to — a place in its own strip, a place in another window's, or a window of its own.
//!
//! **A carried pane** is read here too: [`Strip::pane_over`] says whether a point over the strip
//! means "into this tab" or "a new tab at this place", and the pane operations of [`Tabs`]
//! ([`Tabs::pane_to_tab`], [`Tabs::pane_to_new_tab`], [`Tabs::place_new`]) are the list's half
//! of the ways a pane changes tab within a window: which tab opens where, which one closes
//! because it was emptied and which one stays selected. Across windows a tab leaves and joins
//! whole ([`Tabs::close`], [`Tabs::insert_at`]); what each move comes to is `moves`'.
//!
//! Coordinates are points in the **bar's** space (the layout is horizontal only; the other axis
//! appears only in how far a dragged pointer is from the bar): the bar spans the window's
//! full width and its origin is the window's left edge. The title row's height is not here — it
//! is whatever AppKit reports for the window's title row, a single copy read from the window.

use std::path::PathBuf;
use std::time::Duration;

use bt_core::{HostMark, ProgramActivity};

/// A window's tabs in strip order, and the selected one.
///
/// Invariant: a selection exists exactly when there is at least one tab. A bateri window is born
/// with one tab ([`Tabs::new`]); closing the last one leaves the model empty, which is its cue to
/// close. A host whose windows outlive their tabs keeps an empty one, and may have one born
/// without a tab ([`Tabs::empty`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tabs<K> {
    order: Vec<K>,
    selected: Option<usize>,
}

impl<K: Copy + Eq> Tabs<K> {
    /// A window's tabs at its birth: one tab, selected.
    pub fn new(first: K) -> Self {
        Self {
            order: vec![first],
            selected: Some(0),
        }
    }

    /// No tabs and nothing selected: a window that holds none yet, or no more.
    pub fn empty() -> Self {
        Self {
            order: Vec::new(),
            selected: None,
        }
    }

    /// The tabs, left to right.
    pub fn ids(&self) -> &[K] {
        &self.order
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// The selected tab; `None` only when there are no tabs.
    pub fn selected(&self) -> Option<K> {
        self.selected.map(|index| self.order[index])
    }

    /// The selected tab's position in the strip.
    pub fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    /// The tab's position in the strip, `None` if it is not here.
    pub fn index_of(&self, id: K) -> Option<usize> {
        self.order.iter().position(|&other| other == id)
    }

    /// Adds a tab **right of the selected one** and selects it — where ⌘T opens a tab, so the
    /// new one appears next to what the user was looking at, not at the far end. An id already
    /// here is selected instead of being added twice.
    pub fn insert(&mut self, id: K) {
        if let Some(index) = self.index_of(id) {
            self.selected = Some(index);
            return;
        }
        let index = self
            .selected
            .map_or(self.order.len(), |selected| selected + 1);
        self.order.insert(index, id);
        self.selected = Some(index);
    }

    /// Adds a tab at the **end** and leaves the selection where it was — a tab that joins from
    /// another window (Merge All Windows) does not change what this window shows; into an empty
    /// strip it is the one shown. `false` if the id is already here.
    pub fn append(&mut self, id: K) -> bool {
        if self.index_of(id).is_some() {
            return false;
        }
        self.order.push(id);
        self.selected.get_or_insert(self.order.len() - 1);
        true
    }

    /// Puts a tab **at** `index` (past the end is the end) and selects it — where a tab let go on
    /// the strip lands: the user chose the place, and the tab they carried is the one they look
    /// at. A tab already here moves instead of being added twice.
    pub fn insert_at(&mut self, id: K, index: usize) {
        if self.index_of(id).is_some() {
            self.move_to(id, index);
        } else {
            self.place(id, index);
        }
        self.select(id);
    }

    /// Puts a tab at `index` (past the end is the end) and leaves the selection on the tab it was
    /// on. Only for an id that is not here.
    fn place(&mut self, id: K, index: usize) {
        let at = index.min(self.order.len());
        self.order.insert(at, id);
        // The selection is an index into the order: it follows its tab past the new one.
        match self.selected.as_mut() {
            Some(selected) if *selected >= at => *selected += 1,
            Some(_) => {}
            None => self.selected = Some(at),
        }
    }

    /// Removes a tab; `false` if it was not here. Closing the selected tab selects its **right**
    /// neighbour, or the left one when it was the last — the tab that slides under the pointer
    /// that just closed it. Closing another tab leaves the selection on the same tab.
    pub fn close(&mut self, id: K) -> bool {
        let Some(index) = self.index_of(id) else {
            return false;
        };
        self.order.remove(index);
        self.selected = match self.selected {
            _ if self.order.is_empty() => None,
            Some(selected) if selected == index => Some(index.min(self.order.len() - 1)),
            Some(selected) if selected > index => Some(selected - 1),
            other => other,
        };
        true
    }

    /// Selects a tab; `true` if the selection changed (an absent id or the selected one is a
    /// no-op).
    pub fn select(&mut self, id: K) -> bool {
        match self.index_of(id) {
            Some(index) if self.selected != Some(index) => {
                self.selected = Some(index);
                true
            }
            _ => false,
        }
    }

    /// Moves a tab to `index` (clamped to the last position); the selection stays on the same
    /// tab. `true` if the order changed.
    pub fn move_to(&mut self, id: K, index: usize) -> bool {
        let Some(from) = self.index_of(id) else {
            return false;
        };
        let to = index.min(self.order.len() - 1);
        if from == to {
            return false;
        }
        let selected = self.selected();
        let moved = self.order.remove(from);
        self.order.insert(to, moved);
        self.selected = selected.and_then(|id| self.index_of(id));
        true
    }

    /// A pane of tab `source` went into tab `target` of this window; `emptied` says it was
    /// `source`'s last. The pane going in changes nothing here — the tab it went into is not
    /// selected by it — but an emptied `source` closes. If it was the selected one the tab the
    /// pane went to is selected in its place, not the neighbour a plain close would pick: the
    /// pane's new place is what the user follows. `true` if `source` was removed (the shell then
    /// tears the tab down).
    ///
    /// A whole tab taken into another as panes is the same event with `emptied`.
    pub fn pane_to_tab(&mut self, source: K, emptied: bool, target: K) -> bool {
        if !emptied
            || source == target
            || self.index_of(source).is_none()
            || self.index_of(target).is_none()
        {
            return false;
        }
        let selected = self.selected() == Some(source);
        self.close(source);
        if selected {
            self.select(target);
        }
        true
    }

    /// A pane of tab `source`, which holds `panes` of them, was let go between the tabs at `gap`
    /// (the tab it goes before; `len` is the end). From a tab with several panes it becomes a new
    /// tab `new` at that place — **not** selected, the user stays where they were
    /// ([`NewTab::Created`]; the shell makes the tab). A tab's only pane is the tab: letting it go
    /// between tabs moves the tab to that place, name and identity and all, and `new` is not used
    /// ([`NewTab::Reordered`], or [`NewTab::Unchanged`] when the place is its own).
    pub fn pane_to_new_tab(&mut self, source: K, panes: usize, new: K, gap: usize) -> NewTab {
        let Some(from) = self.index_of(source) else {
            return NewTab::Unchanged;
        };
        if panes <= 1 {
            return if self.move_to(source, gap_to_index(from, gap)) {
                NewTab::Reordered
            } else {
                NewTab::Unchanged
            };
        }
        if self.index_of(new).is_some() {
            return NewTab::Unchanged;
        }
        self.place(new, gap);
        NewTab::Created
    }

    /// A new tab `new` before tab number `gap` (`len` is the end), the selection left where it
    /// was — a pane of this window let go between its tabs, once [`Self::pane_to_new_tab`] has
    /// said it makes one. `false` if the id is already here.
    pub fn place_new(&mut self, new: K, gap: usize) -> bool {
        if self.index_of(new).is_some() {
            return false;
        }
        self.place(new, gap);
        true
    }

    /// The tab Show Next Tab (`forward`) or Show Previous Tab selects: the selected tab's
    /// neighbour, wrapping around at both ends. With one tab it is that tab.
    pub fn adjacent(&self, forward: bool) -> Option<K> {
        let selected = self.selected?;
        let count = self.order.len();
        let index = if forward {
            (selected + 1) % count
        } else {
            (selected + count - 1) % count
        };
        Some(self.order[index])
    }

    /// The tab a Select Tab ▸ item reaches ([`tab_index`]).
    pub fn by_shortcut(&self, tag: u8) -> Option<K> {
        tab_index(tag, self.order.len()).map(|index| self.order[index])
    }
}

/// The Select Tab ▸ item's `tag` + tab count → index of the tab to select.
///
/// ⌘1…⌘8 is the nth tab, `None` if absent (no-op); ⌘9 is the **last** tab —
/// the shared rule of Safari, Terminal.app and browsers: with more than nine
/// tabs too the last one is reached with a single key.
pub fn tab_index(tag: u8, count: usize) -> Option<usize> {
    match tag {
        1..=8 => Some(usize::from(tag) - 1).filter(|&index| index < count),
        9 => count.checked_sub(1),
        _ => None,
    }
}

/// The shortcut hint a tab shows while ⌘ is held: `⌘n` on the first eight, `⌘9` on the last one
/// when it is past them, nothing on the tabs in between — exactly the tabs [`tab_index`] reaches,
/// each with the key that reaches it.
pub fn shortcut_hint(index: usize, count: usize) -> Option<String> {
    shortcut_digit(index, count).map(|digit| format!("⌘{digit}"))
}

/// The digit of [`shortcut_hint`]: the key that reaches the tab at `index` with ⌘. The Show All
/// Tabs list gives its rows the same keys.
pub fn shortcut_digit(index: usize, count: usize) -> Option<u8> {
    if index >= count {
        return None;
    }
    match index {
        0..=7 => u8::try_from(index + 1).ok(),
        _ if index + 1 == count => Some(9),
        _ => None,
    }
}

/// The name a rename field's text asks a tab to carry: the draft without the whitespace round it,
/// and no name at all when that is empty or says what the tab's own title (`automatic`) already
/// says — emptying the field, or typing the title back, returns the tab to its title.
pub fn custom_name(draft: &str, automatic: &str) -> Option<String> {
    let name = draft.trim();
    (!name.is_empty() && name != automatic.trim()).then(|| name.to_owned())
}

/// A window's title as the system lists it — the Dock icon's menu, the Window menu — at most
/// `max` wide as `width` measures it: whole when it fits, otherwise cut where it still fits with a
/// trailing "…". Those lists are as wide as their longest title and a program's title can be a
/// whole sentence; a cap keeps them at one width, whatever runs.
///
/// The cut falls between characters, never inside one, and takes back what would dangle before
/// the "…": a space, or the joiner or variation selector of an emoji it split.
pub fn fit_title(title: &str, max: f64, width: impl Fn(&str) -> f64) -> String {
    if width(title) <= max {
        return title.to_owned();
    }
    let cut = |chars: usize| -> String {
        let end = title
            .char_indices()
            .nth(chars)
            .map_or(title.len(), |(at, _)| at);
        let kept = title[..end].trim_end_matches(|c: char| {
            c.is_whitespace() || matches!(c, '\u{200d}' | '\u{fe0e}' | '\u{fe0f}')
        });
        format!("{kept}…")
    };
    // The longest cut that fits: a width grows with what it holds.
    let (mut fits, mut over) = (0, title.chars().count());
    while over - fits > 1 {
        let middle = (fits + over) / 2;
        if width(&cut(middle)) <= max {
            fits = middle;
        } else {
            over = middle;
        }
    }
    cut(fits)
}

/// The `tag` a menu item carries for tab `id` — the chip's context menu acts on the chip it was
/// opened on, not the selected tab. Zero is "no tab", which the menu bar's items carry and which
/// then means the selected one; ids count from zero, so the tag is the id plus one.
pub fn menu_tag(id: u64) -> isize {
    isize::try_from(id.saturating_add(1)).unwrap_or(isize::MAX)
}

/// [`menu_tag`]'s inverse: the tab a `tag` names, `None` for zero and anything below it.
pub fn tab_of_menu_tag(tag: isize) -> Option<u64> {
    u64::try_from(tag).ok()?.checked_sub(1)
}

/// The role of a Show All Tabs row's status dot for what its tab reports: a running command, a
/// question and a transfer in `accent`, a failure in `error`, a success in `success` — the
/// colours the chip's own indicators wear.
pub fn list_dot(indicator: Indicator) -> Tone {
    match indicator {
        Indicator::Question | Indicator::Running | Indicator::Uploading => Tone::Accent,
        Indicator::Failed => Tone::Error,
        Indicator::Finished => Tone::Success,
    }
}

/// Space between two neighbouring tabs. Also where the separator line sits.
pub const GAP: f64 = 2.0;

/// The widest a tab gets: a few tabs in a wide window do not stretch across it.
pub const MAX_WIDTH: f64 = 184.0;

/// The narrowest a tab gets; past this the strip scrolls instead of shrinking the tabs further,
/// so a title stays readable.
pub const MIN_WIDTH: f64 = 120.0;

/// Empty space always kept between the last tab and the buttons on the right, with [`EDGE`]
/// beside it: the visible gap from the last tab to the leftmost button is 24 pt = 16 + 8. The
/// window is moved from there; tabs shrink before giving it up. Enough to grab with the pointer
/// without pushing `+` away from the tabs it adds to.
pub const DRAG_MARGIN: f64 = 16.0;

/// Side of a square button on the right of the bar (`+`, Show All Tabs, the settings warning).
pub const BUTTON: f64 = 28.0;

/// Space between the window's right edge and `+` where the window's corner is square or small
/// ([`Fit::of`]), and between the drag margin and the leftmost button.
pub const EDGE: f64 = 8.0;

/// A tab's corner radius; also the bar's buttons' where the window's corner is square or small
/// ([`Fit::of`]).
pub const TAB_RADIUS: f64 = 7.0;

/// Space between two neighbouring buttons on the right.
pub const BUTTON_GAP: f64 = 4.0;

/// Width of the fade at a scrolled strip's edge, where tabs run under the edge.
pub const FADE: f64 = 28.0;

/// How far from the strip's edge a selected tab is brought when it is scrolled into view, so
/// its neighbour still peeks out and says there is more.
pub const REVEAL_MARGIN: f64 = 24.0;

/// A scroll within this distance of an end counts as at that end: a sub-point remainder must not
/// light a fade over nothing.
const SCROLL_SLACK: f64 = 1.0;

/// Where a button sits left of the button at `x`: one [`BUTTON_GAP`] away.
fn beside(x: f64) -> f64 {
    x - BUTTON_GAP - BUTTON
}

/// A horizontal extent in the bar's space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub x: f64,
    pub width: f64,
}

impl Span {
    pub const fn new(x: f64, width: f64) -> Self {
        Self { x, width }
    }

    pub fn end(self) -> f64 {
        self.x + self.width
    }
}

/// What the strip is laid out from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bar {
    /// The bar's width — the window's.
    pub width: f64,
    /// Where the strip may start: right of the traffic lights, with breathing room. The caller
    /// reads it from the window, because the lights move (and hide in full screen).
    pub leading: f64,
    /// How many tabs. One tab is today's window: a centred title and `+` alone.
    pub count: usize,
    pub selected: usize,
    pub hovered: Option<usize>,
    pub dragged: Option<usize>,
    /// The strip's scroll, as last stored; clamped here.
    pub scroll: f64,
    /// A settings diagnostic is showing: its warning button joins the right side. With one tab
    /// the diagnostic stays beside the title instead, so this is ignored there.
    pub warning: bool,
    /// The radius of the window's own top-right corner, the one `+` sits in; 0 where the corner
    /// is square (full screen). The caller gives what it measured for its window, as the
    /// corner changes with the window's state ([`Fit::of`]).
    pub corner: f64,
}

/// How the buttons on the right meet the window's top-right corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    /// Space between the window's right edge and `+`.
    pub inset: f64,
    /// The buttons' corner radius.
    pub radius: f64,
}

impl Fit {
    /// The fit for a window corner of radius `corner`. `+`'s corner shares the window corner's
    /// centre across — `inset + radius == corner` — so the space round the corner does not pinch
    /// at the diagonal: with a 20 pt corner and the tabs' radius it pinched to 4.3 pt against
    /// 6 pt at the top (measured on screen). The radius gives way before the inset: it is at
    /// least a tab's, so a square or small corner keeps [`EDGE`] and the tabs' look, and at
    /// most a circle, past which a rounder corner draws `+` inwards.
    ///
    /// Up and down the button is centred in the title row, whose height is the platform's
    /// (40 pt on macOS 26.4.1, so a 6 pt top gap): with a 20 pt corner the button's corner
    /// centre sits 2 pt above the window's, and the gap runs from 6 pt at the top to [`EDGE`]
    /// at the side. Exactly concentric in a row twice the corner's radius would take a circle.
    pub fn of(corner: f64) -> Self {
        let radius = (corner - EDGE).clamp(TAB_RADIUS, BUTTON / 2.0);
        Self {
            inset: (corner - radius).max(EDGE),
            radius,
        }
    }
}

/// Where everything in the bar sits ([`Bar::layout`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Strip {
    /// The strip's visible extent; tabs outside it are clipped.
    pub span: Span,
    /// Every tab, scrolled; with one tab, the centred title's box.
    pub chips: Vec<Span>,
    /// Tabs no longer fit at [`MIN_WIDTH`]: the strip scrolls and Show All Tabs appears.
    pub overflow: bool,
    /// The scroll actually applied, within `0..=max_scroll`.
    pub scroll: f64,
    pub max_scroll: f64,
    /// The fades at the strip's leading and trailing edge: lit only where tabs run under it.
    pub fade_leading: bool,
    pub fade_trailing: bool,
    /// Centres of the separators that show: only between two tabs neither of which is selected,
    /// hovered or dragged — a lit tab already has an edge.
    pub separators: Vec<f64>,
    /// `+`, always at the right end.
    pub new_tab: f64,
    /// Show All Tabs, only when the strip overflows; left of `+`.
    pub list: Option<f64>,
    /// The settings warning, left of everything else on the right.
    pub warning: Option<f64>,
    /// How the buttons meet the window's corner: `+`'s inset and every button's radius.
    pub fit: Fit,
    /// A tab's width plus [`GAP`]: the step from one tab to the next.
    slot: f64,
}

impl Bar {
    /// Lays the bar out — one function for every case (one tab, tabs that fit, tabs that
    /// overflow), so the strip, the buttons and the scroll can never disagree.
    ///
    /// With several tabs they share the strip evenly, at most [`MAX_WIDTH`], in whole points;
    /// the strip ends [`DRAG_MARGIN`] + [`EDGE`] left of the leftmost button. When even
    /// [`MIN_WIDTH`] does not fit, the tabs stay at that width, Show All Tabs joins the buttons
    /// (taking its space from the strip) and the strip scrolls.
    pub fn layout(&self) -> Strip {
        let fit = Fit::of(self.corner);
        let new_tab = self.width - fit.inset - BUTTON;
        let left_of = beside;
        let strip_until = |leftmost: f64| {
            Span::new(
                self.leading,
                (leftmost - EDGE - DRAG_MARGIN - self.leading).max(0.0),
            )
        };

        if self.count == 1 {
            // The title is centred on the window, as far from both edges as it must be to clear
            // the lights on the left and `+` on the right.
            let margin = self.leading.max(self.width - (new_tab - EDGE));
            return Strip {
                span: Span::new(self.leading, (new_tab - EDGE - self.leading).max(0.0)),
                chips: vec![Span::new(margin, (self.width - 2.0 * margin).max(0.0))],
                overflow: false,
                scroll: 0.0,
                max_scroll: 0.0,
                fade_leading: false,
                fade_trailing: false,
                separators: Vec::new(),
                new_tab,
                list: None,
                warning: None,
                fit,
                slot: 0.0,
            };
        }

        let count = self.count as f64;
        let shared = |span: Span| ((span.width + GAP) / count - GAP).floor().min(MAX_WIDTH);
        let mut list = None;
        let mut warning = self.warning.then(|| left_of(new_tab));
        let mut span = strip_until(warning.unwrap_or(new_tab));
        let mut width = if self.count == 0 {
            MAX_WIDTH
        } else {
            shared(span)
        };
        let overflow = width < MIN_WIDTH;
        if overflow {
            let shown = left_of(new_tab);
            list = Some(shown);
            warning = self.warning.then(|| left_of(shown));
            span = strip_until(warning.unwrap_or(shown));
            width = MIN_WIDTH;
        }

        let slot = width + GAP;
        let total = (count * slot - GAP).max(0.0);
        let max_scroll = if overflow {
            (total - span.width).max(0.0)
        } else {
            0.0
        };
        let scroll = self.scroll.clamp(0.0, max_scroll);
        let chips: Vec<Span> = (0..self.count)
            .map(|index| Span::new(span.x + index as f64 * slot - scroll, width))
            .collect();
        let lit = |index: usize| {
            index == self.selected || self.hovered == Some(index) || self.dragged == Some(index)
        };
        let separators = (1..self.count)
            .filter(|&right| !lit(right - 1) && !lit(right))
            .map(|right| chips[right - 1].end() + GAP / 2.0)
            .collect();
        Strip {
            span,
            chips,
            overflow,
            scroll,
            max_scroll,
            fade_leading: overflow && scroll > SCROLL_SLACK,
            fade_trailing: overflow && scroll < max_scroll - SCROLL_SLACK,
            separators,
            new_tab,
            list,
            warning,
            fit,
            slot,
        }
    }
}

impl Strip {
    /// Where Show All Tabs sits — or would, while the tabs fit and its button is not there: the
    /// list opens at that place from its shortcut too.
    pub fn list_slot(&self) -> f64 {
        self.list.unwrap_or_else(|| beside(self.new_tab))
    }

    /// The scroll that brings tab `index` into view, [`REVEAL_MARGIN`] from the edge it was
    /// beyond; the current scroll if it is already in view (or nothing scrolls).
    pub fn revealing(&self, index: usize) -> f64 {
        let Some(chip) = self.chips.get(index).filter(|_| self.overflow) else {
            return self.scroll;
        };
        let left = index as f64 * self.slot;
        let right = left + chip.width;
        let target = if left < self.scroll + REVEAL_MARGIN {
            left - REVEAL_MARGIN
        } else if right > self.scroll + self.span.width - REVEAL_MARGIN {
            right - self.span.width + REVEAL_MARGIN
        } else {
            self.scroll
        };
        target.clamp(0.0, self.max_scroll)
    }

    /// The scroll after a wheel or trackpad step: the dominant axis moves the strip, positive
    /// towards the later tabs, clamped to the ends — a vertical wheel scrolls a horizontal strip
    /// too, as nothing else in the bar takes it.
    pub fn wheeled(&self, dx: f64, dy: f64) -> f64 {
        let step = if dx.abs() > dy.abs() { dx } else { dy };
        (self.scroll + step).clamp(0.0, self.max_scroll)
    }
}

/// How far along the strip the pointer travels from a press before the press is a drag: a click
/// that slides a pixel or two is still a click, and its tab stays where it is.
pub const DRAG_SLOP: f64 = 4.0;

/// How far outside the bar the pointer may wander with a tab held before the tab comes away from
/// it. The tab is 28 pt in a row of about 40, so the pointer is already near the row's edge when
/// it leaves the tab, and a gesture along the strip drifts a few points up or down without
/// meaning to leave; this much beyond the row is a pull, not a drift. Measured from the bar's
/// edge on all four sides, so a pointer taken out of the window sideways tears too.
pub const TEAR_DISTANCE: f64 = 12.0;

/// How far `(x, y)` is outside a `width × height` bar at the origin: 0 inside, otherwise the
/// larger of the two overshoots — a corner is no further than its worse side.
pub fn distance_outside(x: f64, y: f64, width: f64, height: f64) -> f64 {
    let along = (-x).max(x - width).max(0.0);
    let across = (-y).max(y - height).max(0.0);
    along.max(across)
}

/// What the pointer's travel since a press on a tab means ([`Grip::track`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Drag {
    /// Not far enough yet: still a click.
    Press,
    /// The tab follows the pointer along the strip. `left` is where its left edge is drawn (the
    /// bar's space), `to` the place it would take if let go now.
    Reorder { left: f64, to: usize },
    /// The pointer left the bar: the tab comes away from the strip.
    TearOff,
}

/// A press on a tab, until the pointer is let go or the tab comes away: where in the tab it was
/// held and whether it has become a drag. Once a drag, a press is one for good — moving back to
/// where it began does not take the tab out of the pointer's hand.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grip {
    from: usize,
    /// Where along the bar the pointer went down.
    press: f64,
    /// How far into the tab it was held: the tab's left edge is the pointer less this.
    grab: f64,
    moving: bool,
}

impl Grip {
    /// The grip of a press at `x` (the bar's space) on tab `from` of `strip`; `None` for a tab
    /// that is not there and for a lone tab, whose title is not a tab to carry.
    pub fn new(strip: &Strip, from: usize, x: f64) -> Option<Self> {
        let chip = strip.chips.get(from).filter(|_| strip.slot > 0.0)?;
        Some(Self {
            from,
            press: x,
            grab: x - chip.x,
            moving: false,
        })
    }

    /// The tab pressed: its place in the strip when the press began.
    pub fn from(&self) -> usize {
        self.from
    }

    /// Reads the pointer at `(x, y)` in a `bar` of `(width, height)`. The tab is held where it was
    /// grabbed, kept within what the strip shows (a scrolled-away place cannot be dropped on) and
    /// among the places that exist; `strip` is the layout now, so a strip the wheel has moved
    /// since the press is read where it is.
    pub fn track(&mut self, strip: &Strip, (x, y): (f64, f64), bar: (f64, f64)) -> Drag {
        if distance_outside(x, y, bar.0, bar.1) > TEAR_DISTANCE {
            return Drag::TearOff;
        }
        if !self.moving && (x - self.press).abs() < DRAG_SLOP {
            return Drag::Press;
        }
        self.moving = true;
        let last = strip.chips.len().saturating_sub(1);
        let width = strip
            .chips
            .get(self.from)
            .map_or(MIN_WIDTH, |chip| chip.width);
        let least = strip.span.x;
        let most = (strip.span.end() - width)
            .min(strip.span.x - strip.scroll + last as f64 * strip.slot)
            .max(least);
        let left = (x - self.grab).clamp(least, most);
        let place = (left - strip.span.x + strip.scroll) / strip.slot;
        Drag::Reorder {
            left,
            to: (place.round().max(0.0) as usize).min(last),
        }
    }
}

impl Strip {
    /// The place under `x`: the slot of the strip that holds that point, among `0..chips.len()`
    /// — the first for a point left of the strip, the last from its visible end on (over the
    /// buttons, in the drag margin). It is a function of the layout and `x` alone, so a tab carried over a
    /// strip can be placed by it while the strip is laid out **with room made** at the answer: the
    /// room opens where the pointer is, and moves nothing the answer depends on.
    pub fn slot_at(&self, x: f64) -> usize {
        let last = self.chips.len().saturating_sub(1);
        if self.slot <= 0.0 || x < self.span.x {
            return 0;
        }
        if x >= self.span.end() {
            // Past the strip's visible end, over the buttons or the drag margin: the end of the
            // tabs, whichever of them are scrolled in view.
            return last;
        }
        let place = ((x - self.span.x + self.scroll) / self.slot).floor();
        (place.max(0.0) as usize).min(last)
    }
}

/// How far into a tab, as a fraction of its width, the middle that means "into this tab" begins.
/// Left of it the point is at the tab's left edge: the gap before it. A design constant: the
/// middle is 64% of the tab, wide enough to hit without aiming, and each edge keeps 18% (about
/// 33 pt at the widest tab) for "between".
pub const TAB_CORE_FROM: f64 = 0.18;

/// Where the middle of a tab ends, as a fraction of its width ([`TAB_CORE_FROM`]).
pub const TAB_CORE_TO: f64 = 0.82;

/// How long a carried pane rests on a tab's middle before that tab opens under it: long enough
/// that passing over tabs on the way to another does not open them, short enough not to feel
/// like waiting. A design constant, the pace of a spring-loaded folder.
pub const SPRING_DELAY: Duration = Duration::from_millis(550);

/// What a carried pane is over on the strip ([`Strip::pane_over`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneOver {
    /// The middle of the tab at `index`: the pane joins that tab.
    OnTab { index: usize },
    /// Between tabs, before the tab at `gap` (`len` is the end): the pane becomes a tab there.
    Between { gap: usize },
}

impl Strip {
    /// What a carried pane at `x` (the bar's space) is over: the middle of a tab
    /// ([`TAB_CORE_FROM`] to [`TAB_CORE_TO`] of its width, both ends in) means that tab; the
    /// edge of a tab, the gap between two tabs and anywhere else along the strip mean a new tab,
    /// at the place of the tabs' centres the point stands between. Left of the strip's visible
    /// extent is the first place and right of it the end, as for [`Strip::slot_at`]. A lone
    /// tab's title is a tab like the rest.
    pub fn pane_over(&self, x: f64) -> PaneOver {
        if x < self.span.x {
            return PaneOver::Between { gap: 0 };
        }
        if x >= self.span.end() {
            return PaneOver::Between {
                gap: self.chips.len(),
            };
        }
        let held = self
            .chips
            .iter()
            .position(|chip| chip.width > 0.0 && x >= chip.x && x < chip.end());
        if let Some(index) = held {
            let chip = self.chips[index];
            let at = (x - chip.x) / chip.width;
            return if at < TAB_CORE_FROM {
                PaneOver::Between { gap: index }
            } else if at > TAB_CORE_TO {
                PaneOver::Between { gap: index + 1 }
            } else {
                PaneOver::OnTab { index }
            };
        }
        PaneOver::Between {
            gap: self
                .chips
                .iter()
                .filter(|chip| chip.x + chip.width / 2.0 < x)
                .count(),
        }
    }
}

/// The layout's place for the tab at `index` when room is made at `gap` (a tab on its way in):
/// the tabs from the gap on take the next place.
pub fn seat(index: usize, gap: Option<usize>) -> usize {
    index + usize::from(gap.is_some_and(|gap| index >= gap))
}

/// `items` with the one at `from` taken out and put back so that it stands at `to` (past the end
/// is the end) — the order a dragged tab's neighbours lay out in while it is held.
pub fn reordered<T>(items: &mut Vec<T>, from: usize, to: usize) {
    if from < items.len() {
        let item = items.remove(from);
        items.insert(to.min(items.len()), item);
    }
}

/// What letting a pane go between tabs did to the list ([`Tabs::pane_to_new_tab`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewTab {
    /// A tab was added: the shell builds it, holding the pane.
    Created,
    /// The pane was its tab's only one, so the tab itself moved to the place.
    Reordered,
    /// Nothing changed: the place is the tab's own, or the tab is not here.
    Unchanged,
}

/// Where a tab at `from` ends up when it is let go in the gap before tab `gap` (`len` is the
/// end): the index [`Tabs::move_to`] takes. The gap counts the tabs **before** the tab is taken
/// out, so a gap to its right is one less once it has gone — `[A, B, C]` with A let go in gap 2
/// is `[B, A, C]`, index 1.
pub fn gap_to_index(from: usize, gap: usize) -> usize {
    if gap > from { gap - 1 } else { gap }
}

/// What a carried tab is let go over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Over {
    /// Its own window's strip, over the slot `place` ([`Strip::slot_at`] of the strip as it is).
    Own { place: usize },
    /// Another bateri window's strip, over the slot `gap` of that strip **with the room made**
    /// for it ([`Strip::slot_at`]): where the tab is inserted.
    Other { gap: usize },
}

/// What letting a carried tab go does ([`landing`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Landing {
    /// It stays in its window, at this place in the order.
    Reorder(usize),
    /// It joins another window, at this place.
    Join(usize),
    /// It was let go over no strip: a window of its own, where the pointer was.
    Detach,
}

/// The decision of a release: over a strip (`Some`) the tab lands there, over nothing — the
/// desktop, a pane, another application — it becomes a window.
pub fn landing(over: Option<Over>) -> Landing {
    match over {
        Some(Over::Own { place }) => Landing::Reorder(place),
        Some(Over::Other { gap }) => Landing::Join(gap),
        None => Landing::Detach,
    }
}

/// What a tab's indicator slot shows, left of its title: one glyph, the most urgent of the tab's
/// signals ([`indicator`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Indicator {
    /// A question of the tab waits for an answer the user cannot see — asked while the tab was in
    /// the background, or left up when the user switched away — or a program in it reported itself
    /// blocked on the user (`OSC 7501`: a permission, a question, a login). First: nothing goes on
    /// in that tab until it is answered.
    Question,
    /// A command runs.
    Running,
    /// A command failed and the user has not looked at the tab since.
    Failed,
    /// A command finished and the user has not looked at the tab since.
    Finished,
    /// An upload or download flows.
    Uploading,
}

impl Indicator {
    /// What VoiceOver says after the tab's title.
    pub fn spoken(self) -> &'static str {
        match self {
            Indicator::Question => "waiting for an answer",
            Indicator::Running => "running",
            Indicator::Failed => "failed",
            Indicator::Finished => "finished",
            Indicator::Uploading => "uploading",
        }
    }
}

/// The facts a tab's indicator comes from; the platform shell reads each from the tab's panes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Signals {
    pub question: bool,
    pub running: bool,
    pub failed: bool,
    pub finished: bool,
    pub uploading: bool,
}

/// The tab's one indicator: question > running > failed > finished > uploading; `None` when
/// nothing is going on. A question outranks everything because the tab is stuck until it is
/// answered; a failure outranks a success because it is the one the user must not miss; an
/// upload is last because its progress also shows elsewhere (the underline, the Dock icon).
pub fn indicator(signals: Signals) -> Option<Indicator> {
    let Signals {
        question,
        running,
        failed,
        finished,
        uploading,
    } = signals;
    [
        (question, Indicator::Question),
        (running, Indicator::Running),
        (failed, Indicator::Failed),
        (finished, Indicator::Finished),
        (uploading, Indicator::Uploading),
    ]
    .into_iter()
    .find_map(|(on, indicator)| on.then_some(indicator))
}

/// A marked host's kind as the bar says it — the summary card's line and VoiceOver; `None` for an
/// unmarked host, which shows no line either (the top line's rule: an unmarked host must not
/// water down Production's red).
pub fn host_name(mark: HostMark) -> Option<&'static str> {
    match mark {
        HostMark::Production => Some("Production"),
        HostMark::Staging => Some("Staging"),
        HostMark::Development => Some("Development"),
        HostMark::Rgb(_) => Some("Marked"),
        HostMark::None => None,
    }
}

/// What VoiceOver says for a chip: its title, its indicator — an upload with its percentage — and
/// a marked host's kind ("Production host"), the things the chip shows beside its title.
pub fn spoken(
    title: &str,
    indicator: Option<Indicator>,
    upload: Option<u8>,
    mark: HostMark,
) -> String {
    let mut said = title.to_owned();
    match (indicator, upload) {
        (Some(Indicator::Uploading), Some(percent)) => {
            said.push_str(&format!(", uploading {percent}%"));
        }
        (Some(indicator), _) => {
            said.push_str(", ");
            said.push_str(indicator.spoken());
        }
        (None, _) => {}
    }
    if let Some(name) = host_name(mark) {
        said.push_str(&format!(", {name} host"));
    }
    said
}

/// What a tab's newest command did, for its summary card ([`Card`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CardCommand {
    /// It runs for `elapsed`. `command` is its row's text, empty where the row cannot be read (a
    /// full-screen program covers it).
    Running { command: String, elapsed: Duration },
    /// It ended with `0`; `duration` is the duration counter's settled text.
    Finished {
        command: String,
        duration: Option<String>,
    },
    /// It ended with another code.
    Failed { command: String, exit: i32 },
}

/// What a tab's summary card tells ([`card_lines`]); the platform shell reads each from the tab's
/// focused pane.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Card {
    pub title: String,
    /// The shell's directory, absolute; shown with the home as `~`.
    pub directory: Option<PathBuf>,
    pub home: Option<PathBuf>,
    /// In a remote session: the host and the remote directory, empty when the server says none.
    pub remote: Option<(String, String)>,
    pub command: Option<CardCommand>,
    /// A transfer flowing: the item's name, the percentage, and `true` when it is a download.
    pub upload: Option<(String, u8, bool)>,
    pub mark: HostMark,
    pub panes: usize,
}

/// The theme role a card line is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Title,
    Dim,
    Accent,
    Success,
    Error,
    /// The host mark's own colour.
    Mark,
}

/// The card's lines, top to bottom: the title; the directory (`host:path` in a remote session);
/// what the newest command did — "Running · {command} · {time}", "Finished · {command} · {time}",
/// "Exit {n} · {command}"; a flowing transfer; a marked host's kind; how many panes, past one.
/// A part nobody knows is left out rather than shown empty.
pub fn card_lines(card: &Card) -> Vec<(String, Tone)> {
    let mut lines = vec![(card.title.clone(), Tone::Title)];
    let directory = match &card.remote {
        Some((host, dir)) if dir.is_empty() => Some(host.clone()),
        Some((host, dir)) => Some(format!("{host}:{dir}")),
        None => card
            .directory
            .as_deref()
            .map(|dir| crate::program::tilde(dir, card.home.as_deref())),
    };
    if let Some(directory) = directory {
        lines.push((directory, Tone::Dim));
    }
    let joined = |parts: &[&str]| {
        parts
            .iter()
            .filter(|part| !part.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join(" · ")
    };
    match &card.command {
        Some(CardCommand::Running { command, elapsed }) => {
            let time = bt_core::running_counter(*elapsed).unwrap_or_default();
            lines.push((joined(&["Running", command, &time]), Tone::Accent));
        }
        Some(CardCommand::Finished { command, duration }) => {
            let time = duration.as_deref().unwrap_or_default();
            lines.push((joined(&["Finished", command, time]), Tone::Success));
        }
        Some(CardCommand::Failed { command, exit }) => {
            lines.push((joined(&[&format!("Exit {exit}"), command]), Tone::Error));
        }
        None => {}
    }
    if let Some((name, percent, down)) = &card.upload {
        let verb = if *down {
            "↓ Downloading"
        } else {
            "↑ Uploading"
        };
        lines.push((
            joined(&[&format!("{verb} {name}"), &format!("{percent}%")]),
            Tone::Accent,
        ));
    }
    if let Some(name) = host_name(card.mark) {
        lines.push((format!("{name} host"), Tone::Mark));
    }
    if card.panes > 1 {
        lines.push((format!("{} panes", card.panes), Tone::Dim));
    }
    lines
}

/// How many of a pane's commands have ended, by outcome, as last looked at — `bt_core::Activity`'s
/// two counts. Counts only grow, so the difference between two looks is what ended in between,
/// however fast: a command that started and ended between them still counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub finished: u64,
    pub failed: u64,
}

/// A tab's commands that ended while the user was not looking at it: the "finished" tick and the
/// "failed" dot, until the tab is selected ([`Unseen::looked`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Unseen {
    pub finished: bool,
    pub failed: bool,
}

impl Unseen {
    /// A pane of the tab moved from `seen` to `now`. What ended while the tab was **not selected**
    /// marks it — a failure as failed, a success as finished, both kept until the tab is looked
    /// at, so a later success does not hide an earlier failure. While the tab is selected nothing
    /// is marked, even with its window behind another: the tab is the one the user left on screen.
    pub fn observe(&mut self, seen: Tally, now: Tally, selected: bool) {
        if selected {
            return;
        }
        self.failed |= now.failed != seen.failed;
        self.finished |= now.finished != seen.finished;
    }

    /// The tab came on screen: what it showed is seen.
    pub fn looked(&mut self) {
        *self = Self::default();
    }
}

/// How long a pane has worked **as the tab's ring counts it**. A program that reports its status
/// (`program`, `bt_core::Activity::program`) **owns** the ring: it turns while the program says it
/// works and stands still while it waits for its user — the shell's command stays open for as long
/// as the program does, and a ring turning beside an agent that has asked a question reads as a tab
/// that is still working. Without a report it is the command: `running` (the pane's
/// `bt_core::Activity::running`), `None` while the pane is on the alternate screen. A full-screen
/// program — an editor, a pager, `htop`, an agent's full-screen interface — runs for as long as it
/// is open, and a ring turning beside it for hours reads as a tab that is still loading. Only the
/// ring forgets it: the summary card reads the pane itself and still says how long it has been
/// open, and its end marks the tab like any command's.
pub fn ring_running(
    running: Option<Duration>,
    program: Option<ProgramActivity>,
    full_screen: bool,
) -> Option<Duration> {
    match program {
        Some(program) => program.working,
        None => running.filter(|_| !full_screen),
    }
}

/// The step the running ring shows for a command that has run `elapsed`: one step per tick of the
/// command's duration counter (`bt_core::counter_period`, the tiers `bt_core::next_tick` wakes
/// on — a second, from an hour on a minute), twelve to a turn. Under Reduce Motion the ring
/// stands still at its first step.
pub fn ring_step(elapsed: Duration, reduce_motion: bool) -> u8 {
    if reduce_motion {
        return 0;
    }
    let period = bt_core::counter_period(elapsed).as_secs().max(1);
    ((elapsed.as_secs() / period) % 12) as u8
}

/// The ring's turn for [`ring_step`], degrees clockwise from the top.
pub const RING_STEP_DEGREES: f64 = 30.0;

/// What the bar's one delayed wake is set from ([`Clock::delay`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Clock {
    /// The window is on screen (its occlusion state): an occluded or minimized window's bar
    /// changes nothing anybody sees.
    pub visible: bool,
    pub reduce_motion: bool,
    /// How long each tab's running command has run, for every tab with one.
    pub running: Vec<Duration>,
    /// How long the command of the tab whose summary card is open has run, when it runs.
    pub card: Option<Duration>,
}

impl Clock {
    /// When the bar must look again, `None` for never: the nearest moment a running ring or the
    /// open card's running time changes what it shows — `bt_core::next_tick`, the duration
    /// counter's own clock, so both step on the counter's tick.
    ///
    /// **Two arms, one wake.** The rings turn only while the window is visible and Reduce Motion is
    /// off; the card's seconds are text, not motion, so they go on under Reduce Motion — still
    /// only while the window is visible. No running command and no card: no wake, the bar is idle.
    pub fn delay(&self) -> Option<Duration> {
        if !self.visible {
            return None;
        }
        let rings = self
            .running
            .iter()
            .filter(|_| !self.reduce_motion)
            .map(|&elapsed| bt_core::next_tick(elapsed));
        rings.chain(self.card.map(bt_core::next_tick)).min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tabs(ids: &[u32], selected: u32) -> Tabs<u32> {
        let mut tabs = Tabs::new(ids[0]);
        for &id in &ids[1..] {
            tabs.insert(id);
        }
        tabs.select(selected);
        tabs
    }

    #[test]
    fn a_new_tab_opens_right_of_the_selected_one_and_is_selected() {
        let mut t = tabs(&[1, 2, 3], 1);
        t.insert(9);
        assert_eq!(t.ids(), &[1, 9, 2, 3]);
        assert_eq!(t.selected(), Some(9));
        t.select(3);
        t.insert(8);
        assert_eq!(
            t.ids(),
            &[1, 9, 2, 3, 8],
            "after the last tab it is the new last"
        );
        t.insert(2);
        assert_eq!(t.ids(), &[1, 9, 2, 3, 8], "a known id is not added twice");
        assert_eq!(t.selected(), Some(2), "it is selected instead");
    }

    #[test]
    fn closing_the_selected_tab_selects_its_right_neighbour_else_the_left() {
        let mut t = tabs(&[1, 2, 3], 2);
        assert!(t.close(2));
        assert_eq!((t.ids(), t.selected()), (&[1, 3][..], Some(3)));
        assert!(t.close(3));
        assert_eq!(
            (t.ids(), t.selected()),
            (&[1][..], Some(1)),
            "the last: left neighbour"
        );
        assert!(t.close(1));
        assert!(t.is_empty());
        assert_eq!(t.selected(), None);
        assert!(!t.close(1), "an absent tab is a no-op");
    }

    #[test]
    fn closing_another_tab_keeps_the_selection() {
        let mut t = tabs(&[1, 2, 3, 4], 3);
        assert!(t.close(1));
        assert_eq!((t.selected(), t.selected_index()), (Some(3), Some(1)));
        assert!(t.close(4));
        assert_eq!((t.selected(), t.selected_index()), (Some(3), Some(1)));
    }

    #[test]
    fn a_moved_tab_takes_the_selection_with_it() {
        let mut t = tabs(&[1, 2, 3, 4], 2);
        assert!(t.move_to(2, 3));
        assert_eq!(t.ids(), &[1, 3, 4, 2]);
        assert_eq!(t.selected(), Some(2));
        assert!(t.move_to(4, 0));
        assert_eq!(t.ids(), &[4, 1, 3, 2]);
        assert_eq!((t.selected(), t.selected_index()), (Some(2), Some(3)));
        assert!(t.move_to(4, 99), "past the end is the end");
        assert_eq!(t.ids(), &[1, 3, 2, 4]);
        assert!(!t.move_to(4, 3), "already there");
        assert!(!t.move_to(7, 0), "absent");
    }

    #[test]
    fn next_and_previous_wrap_around() {
        let t = tabs(&[1, 2, 3], 3);
        assert_eq!(t.adjacent(true), Some(1));
        assert_eq!(t.adjacent(false), Some(2));
        let t = tabs(&[1, 2, 3], 1);
        assert_eq!(t.adjacent(false), Some(3));
        assert_eq!(Tabs::new(5).adjacent(true), Some(5), "one tab: itself");
        let mut empty = Tabs::new(5);
        empty.close(5);
        assert_eq!(empty.adjacent(true), None);
        assert!(!empty.select(5));
    }

    #[test]
    fn selecting_reports_a_change_only() {
        let mut t = tabs(&[1, 2], 1);
        assert!(t.select(2));
        assert!(!t.select(2), "already selected");
        assert!(!t.select(7), "absent");
        assert_eq!(t.selected(), Some(2));
    }

    #[test]
    fn numbered_tabs_select_the_nth_or_nothing() {
        assert_eq!(tab_index(1, 3), Some(0));
        assert_eq!(tab_index(3, 3), Some(2));
        assert_eq!(tab_index(4, 3), None, "a nonexistent tab is a no-op");
        assert_eq!(tab_index(8, 8), Some(7));
        assert_eq!(
            tab_index(8, 20),
            Some(7),
            "⌘8 is the eighth even with more than nine tabs"
        );
    }

    #[test]
    fn nine_selects_the_last_tab() {
        assert_eq!(tab_index(9, 1), Some(0), "with a single tab ⌘9 is that tab");
        assert_eq!(tab_index(9, 3), Some(2));
        assert_eq!(tab_index(9, 20), Some(19));
    }

    #[test]
    fn no_tabs_or_unknown_tags_select_nothing() {
        assert_eq!(tab_index(1, 0), None);
        assert_eq!(tab_index(9, 0), None);
        assert_eq!(tab_index(0, 3), None);
        assert_eq!(tab_index(10, 12), None);
    }

    #[test]
    fn the_shortcut_reaches_the_tab_by_its_order() {
        let t = tabs(&[10, 20, 30], 10);
        assert_eq!(t.by_shortcut(2), Some(20));
        assert_eq!(t.by_shortcut(9), Some(30));
        assert_eq!(t.by_shortcut(4), None);
    }

    #[test]
    fn hints_name_the_first_eight_and_the_last() {
        let hints = |count| -> Vec<Option<String>> {
            (0..count)
                .map(|index| shortcut_hint(index, count))
                .collect()
        };
        let three: Vec<_> = hints(3).into_iter().flatten().collect();
        assert_eq!(
            three,
            ["⌘1", "⌘2", "⌘3"],
            "the last of three is ⌘3, its own key"
        );
        let twelve = hints(12);
        assert_eq!(twelve[7].as_deref(), Some("⌘8"));
        assert_eq!(
            &twelve[8..11],
            &[None, None, None],
            "the ones in between have none"
        );
        assert_eq!(twelve[11].as_deref(), Some("⌘9"));
        assert_eq!(shortcut_hint(8, 9).as_deref(), Some("⌘9"));
        assert_eq!(shortcut_hint(3, 3), None, "past the end");
    }

    #[test]
    fn every_hint_names_the_key_that_reaches_its_tab() {
        for count in 1..=20 {
            for index in 0..count {
                let Some(hint) = shortcut_hint(index, count) else {
                    continue;
                };
                let digit = hint.trim_start_matches('⌘').parse::<u8>().unwrap();
                assert_eq!(tab_index(digit, count), Some(index), "{hint} of {count}");
            }
        }
    }

    /// A tab that joins from another window goes to the end and leaves the selection where
    /// it was — Merge All Windows brings tabs in without changing what the window shows.
    #[test]
    fn an_appended_tab_goes_last_and_selects_nothing() {
        let mut t = tabs(&[1, 2, 3], 2);
        assert!(t.append(9));
        assert_eq!(t.ids(), &[1, 2, 3, 9]);
        assert_eq!((t.selected(), t.selected_index()), (Some(2), Some(1)));
        assert!(!t.append(2), "a known id is not added twice");
        assert_eq!(t.ids(), &[1, 2, 3, 9]);
        assert_eq!(t.selected(), Some(2));
    }

    /// A tab let go on a strip lands where the pointer let it go and is the one on screen.
    #[test]
    fn a_dropped_tab_lands_at_its_place_and_is_selected() {
        let mut t = tabs(&[1, 2, 3], 2);
        t.insert_at(9, 1);
        assert_eq!(t.ids(), &[1, 9, 2, 3]);
        assert_eq!((t.selected(), t.selected_index()), (Some(9), Some(1)));
        t.insert_at(8, 0);
        assert_eq!(t.ids(), &[8, 1, 9, 2, 3], "before the first");
        t.insert_at(7, 99);
        assert_eq!(t.ids(), &[8, 1, 9, 2, 3, 7], "past the end is the end");
        assert_eq!(t.selected(), Some(7));
        t.insert_at(8, 5);
        assert_eq!(
            t.ids(),
            &[1, 9, 2, 3, 7, 8],
            "a known id moves instead of being added twice"
        );
        assert_eq!(t.selected(), Some(8));
    }

    /// One unit a character: a title's width is how many it has.
    fn counted(text: &str) -> f64 {
        text.chars().count() as f64
    }

    #[test]
    fn a_title_that_fits_is_left_whole() {
        assert_eq!(fit_title("zsh", 10.0, counted), "zsh");
        assert_eq!(
            fit_title("exactly10!", 10.0, counted),
            "exactly10!",
            "to the edge"
        );
        assert_eq!(fit_title("", 0.0, counted), "");
    }

    #[test]
    fn a_long_title_is_cut_to_fit_with_an_ellipsis() {
        let fitted = fit_title("Reviewing the tab drag in the title bar", 12.0, counted);
        assert_eq!(fitted, "Reviewing t…");
        assert!(counted(&fitted) <= 12.0);
        // Nothing dangles before the ellipsis: the space before a word goes.
        assert_eq!(fit_title("abc def ghi", 5.0, counted), "abc…");
        // Too narrow for any of it: the ellipsis alone.
        assert_eq!(fit_title("abcdef", 1.0, counted), "…");
    }

    #[test]
    fn a_cut_falls_between_characters_and_takes_back_an_emojis_joiner() {
        // Two-byte letters are cut whole.
        assert_eq!(fit_title("çğıöşüçğıöşü", 5.0, counted), "çğıö…");
        // The family emoji's joiner, left at the end of a cut, goes too.
        let family = "ab\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}cdef";
        let fitted = fit_title(family, 5.0, counted);
        assert_eq!(fitted, "ab\u{1f468}…");
        assert_eq!(
            fit_title("ab\u{2764}\u{fe0f}cdefgh", 4.0, counted),
            "ab\u{2764}…"
        );
    }

    #[test]
    fn the_cut_is_the_longest_that_fits_whatever_the_widths() {
        // Wide and narrow characters: the cut is measured, not counted.
        let width =
            |text: &str| -> f64 { text.chars().map(|c| if c == 'W' { 3.0 } else { 1.0 }).sum() };
        let fitted = fit_title("WWiiiiiiiiii", 9.0, width);
        assert_eq!(fitted, "WWii…");
        assert!(width(&fitted) <= 9.0);
        assert!(width("WWiii…") > 9.0, "one more would not fit");
    }

    /// The name a rename field's text asks for: trimmed, and no name at all when it is empty or
    /// says what the tab's own title already says — the tab goes back to its title.
    #[test]
    fn a_name_is_the_trimmed_draft_unless_it_is_empty_or_the_title() {
        assert_eq!(custom_name("build", "zsh"), Some("build".to_owned()));
        assert_eq!(
            custom_name("  build server\t", "zsh"),
            Some("build server".to_owned())
        );
        assert_eq!(custom_name("", "zsh"), None);
        assert_eq!(custom_name(" \t\n ", "zsh"), None, "blank is empty");
        assert_eq!(custom_name("zsh", "zsh"), None, "the title is no name");
        assert_eq!(
            custom_name(" zsh ", "zsh "),
            None,
            "up to the edges of both"
        );
        assert_eq!(
            custom_name("Zsh", "zsh"),
            Some("Zsh".to_owned()),
            "case counts"
        );
        assert_eq!(custom_name("ünï çode", ""), Some("ünï çode".to_owned()));
    }

    /// A menu item carries a tab in its `tag`; zero is the selected tab, which no tab id may
    /// be taken for (ids count from zero).
    #[test]
    fn a_menu_tag_names_a_tab_and_zero_names_none() {
        assert_eq!(tab_of_menu_tag(0), None);
        assert_eq!(tab_of_menu_tag(-1), None);
        for id in [0, 1, 7, 12_345, u32::MAX.into()] {
            assert_eq!(tab_of_menu_tag(menu_tag(id)), Some(id));
            assert_ne!(menu_tag(id), 0);
        }
    }

    #[test]
    fn the_list_dot_wears_the_role_of_what_the_tab_reports() {
        assert_eq!(list_dot(Indicator::Running), Tone::Accent);
        assert_eq!(list_dot(Indicator::Question), Tone::Accent);
        assert_eq!(list_dot(Indicator::Uploading), Tone::Accent);
        assert_eq!(list_dot(Indicator::Failed), Tone::Error);
        assert_eq!(list_dot(Indicator::Finished), Tone::Success);
    }

    /// The list's row key is the digit of the hint, for exactly the tabs that have a hint.
    #[test]
    fn a_list_row_has_the_digit_its_hint_names() {
        for count in 1..=12 {
            for index in 0..count {
                let digit = shortcut_digit(index, count);
                let hint = shortcut_hint(index, count);
                assert_eq!(
                    hint,
                    digit.map(|digit| format!("⌘{digit}")),
                    "{index}/{count}"
                );
            }
        }
        assert_eq!(shortcut_digit(3, 3), None);
    }

    /// Show All Tabs opens where its button is — or would be, when the tabs fit and the button
    /// is not there: one place for both, the same step left of `+`.
    #[test]
    fn the_list_has_a_place_even_when_no_button_shows() {
        let overflowing = bar(720.0, 12).layout();
        assert_eq!(overflowing.list, Some(652.0));
        assert_eq!(overflowing.list_slot(), 652.0);
        let fitting = bar(1000.0, 3).layout();
        assert_eq!(fitting.list, None);
        assert_eq!(fitting.list_slot(), fitting.new_tab - BUTTON_GAP - BUTTON);
        let single = bar(1000.0, 1).layout();
        assert_eq!(single.list_slot(), single.new_tab - BUTTON_GAP - BUTTON);
    }

    fn bar(width: f64, count: usize) -> Bar {
        Bar {
            width,
            leading: 84.0,
            count,
            selected: 0,
            hovered: None,
            dragged: None,
            scroll: 0.0,
            warning: false,
            corner: 20.0,
        }
    }

    fn xs(strip: &Strip) -> Vec<(f64, f64)> {
        strip
            .chips
            .iter()
            .map(|chip| (chip.x, chip.width))
            .collect()
    }

    #[test]
    fn one_tab_is_a_centred_title_and_a_plus() {
        let strip = bar(1000.0, 1).layout();
        assert_eq!(strip.new_tab, 964.0, "8 from the right edge");
        assert_eq!(
            xs(&strip),
            [(84.0, 832.0)],
            "as far from both edges as the lights"
        );
        assert_eq!(
            strip.span,
            Span::new(84.0, 872.0),
            "no drag margin with one tab"
        );
        assert!(!strip.overflow && strip.separators.is_empty());
        assert_eq!((strip.list, strip.warning), (None, None));
        let warned = Bar {
            warning: true,
            ..bar(1000.0, 1)
        }
        .layout();
        assert_eq!(
            warned.warning, None,
            "the diagnostic stays beside the title"
        );
    }

    #[test]
    fn the_buttons_follow_the_windows_corner() {
        let fit = |inset: f64, radius: f64| Fit { inset, radius };
        assert_eq!(
            Fit::of(0.0),
            fit(EDGE, TAB_RADIUS),
            "square: the tabs' look"
        );
        assert_eq!(
            Fit::of(12.0),
            fit(EDGE, TAB_RADIUS),
            "small: still the tabs' look"
        );
        assert_eq!(Fit::of(20.0), fit(EDGE, 12.0), "the radius gives way first");
        assert_eq!(Fit::of(30.0), fit(16.0, BUTTON / 2.0), "then the inset");
        for corner in [16.0, 20.0, 24.0, 30.0] {
            let Fit { inset, radius } = Fit::of(corner);
            assert_eq!(
                inset + radius,
                corner,
                "a {corner} pt corner shares its centre across"
            );
        }

        // The whole right side moves with `+`; the strip yields the space.
        let square = Bar {
            warning: true,
            corner: 0.0,
            ..bar(1000.0, 3)
        }
        .layout();
        assert_eq!((square.new_tab, square.warning), (964.0, Some(932.0)));
        assert_eq!(square.fit.radius, TAB_RADIUS);
        let round = Bar {
            warning: true,
            corner: 30.0,
            ..bar(1000.0, 3)
        }
        .layout();
        assert_eq!((round.new_tab, round.warning), (956.0, Some(924.0)));
        assert_eq!(round.span, Span::new(84.0, 816.0));
        let single = Bar {
            corner: 30.0,
            ..bar(1000.0, 1)
        }
        .layout();
        assert_eq!(single.new_tab, 956.0);
        assert!(single.chips[0].end() <= single.new_tab - EDGE);
    }

    #[test]
    fn the_plus_keeps_an_even_gap_round_a_rounded_corner() {
        // macOS 26.4.1's: a 20 pt corner and a 40 pt title row the button is centred in.
        let (corner, row) = (20.0, 40.0);
        let Fit { inset, radius } = Fit::of(corner);
        let top = (row - BUTTON) / 2.0;
        // Distance from a point of the button's outline to the window's outline, in points
        // from the right edge (`x`) and from the top (`y`).
        let to_window = |x: f64, y: f64| {
            if x < corner && y < corner {
                corner - (x - corner).hypot(y - corner)
            } else {
                x.min(y)
            }
        };
        // The narrowest gap round the corner of a button `inset` from the right edge.
        let narrowest = |inset: f64, radius: f64| {
            let (cx, cy) = (inset + radius, top + radius);
            (0..=90)
                .map(|degree| f64::from(degree).to_radians())
                .map(|angle| to_window(cx - radius * angle.cos(), cy - radius * angle.sin()))
                .fold(f64::INFINITY, f64::min)
        };
        let gap = narrowest(inset, radius);
        assert!(
            gap >= top - 1e-9,
            "pinches to {gap} below the top gap {top}"
        );
        let pinched = narrowest(EDGE, TAB_RADIUS);
        assert!(pinched < top - 1.0, "the tabs' radius pinched: {pinched}");
    }

    #[test]
    fn the_centred_title_keeps_clear_of_the_plus_without_lights() {
        // Full screen: the lights are hidden and the strip may start near the edge.
        let strip = Bar {
            leading: 8.0,
            ..bar(1000.0, 1)
        }
        .layout();
        assert_eq!(xs(&strip), [(44.0, 912.0)]);
        assert!(strip.chips[0].end() <= strip.new_tab - EDGE);
    }

    #[test]
    fn a_few_tabs_sit_left_at_their_widest() {
        let strip = bar(1000.0, 3).layout();
        assert_eq!(
            strip.span,
            Span::new(84.0, 856.0),
            "plus − 8 − 16 is the strip's end: a 24 pt gap"
        );
        assert_eq!(xs(&strip), [(84.0, 184.0), (270.0, 184.0), (456.0, 184.0)]);
        assert!(!strip.overflow);
        assert_eq!((strip.max_scroll, strip.scroll), (0.0, 0.0));
        assert!(!strip.fade_leading && !strip.fade_trailing);
    }

    #[test]
    fn more_tabs_shrink_to_share_the_strip() {
        let strip = bar(1000.0, 5).layout();
        // (856 + 2) / 5 − 2 = 169.6, whole points.
        assert_eq!(strip.chips[0].width, 169.0);
        assert_eq!(strip.chips[4].x, 84.0 + 4.0 * 171.0);
        assert!(!strip.overflow);
        assert_eq!(bar(1000.0, 6).layout().chips[0].width, 141.0);
        let seven = bar(1000.0, 7).layout();
        assert!(!seven.overflow, "120.57 still fits");
        assert_eq!(seven.chips[0].width, MIN_WIDTH);
        let eight = bar(1000.0, 8).layout();
        assert!(eight.overflow, "105 would be below the minimum");
        assert_eq!(eight.chips[0].width, MIN_WIDTH);
    }

    #[test]
    fn overflowing_tabs_stop_at_the_minimum_and_scroll() {
        // Twelve tabs in a 720 pt window: well past what fits.
        let strip = bar(720.0, 12).layout();
        assert!(strip.overflow);
        assert_eq!(strip.new_tab, 684.0);
        assert_eq!(strip.list, Some(652.0), "Show All Tabs, 4 left of +");
        assert_eq!(strip.span, Span::new(84.0, 544.0));
        assert!(strip.chips.iter().all(|chip| chip.width == MIN_WIDTH));
        assert_eq!(strip.chips[11].x, 84.0 + 11.0 * 122.0);
        // 12 × 122 − 2 = 1462 of tabs in a 544 strip.
        assert_eq!(strip.max_scroll, 918.0);
        assert!(!strip.fade_leading && strip.fade_trailing, "at the start");

        let end = Bar {
            scroll: 918.0,
            ..bar(720.0, 12)
        }
        .layout();
        assert!(end.fade_leading && !end.fade_trailing, "at the end");
        assert_eq!(
            end.chips[11].end(),
            end.span.end(),
            "the last tab ends at the edge"
        );

        let middle = Bar {
            scroll: 400.0,
            ..bar(720.0, 12)
        }
        .layout();
        assert!(middle.fade_leading && middle.fade_trailing);
        assert_eq!(middle.chips[0].x, 84.0 - 400.0);

        let past = Bar {
            scroll: 5000.0,
            ..bar(720.0, 12)
        }
        .layout();
        assert_eq!(past.scroll, 918.0, "clamped");
        let before = Bar {
            scroll: -3.0,
            ..bar(720.0, 12)
        }
        .layout();
        assert_eq!(before.scroll, 0.0);
        let almost = Bar {
            scroll: 917.5,
            ..bar(720.0, 12)
        }
        .layout();
        assert!(
            !almost.fade_trailing,
            "half a point from the end is the end"
        );
    }

    #[test]
    fn the_warning_joins_the_buttons_on_the_left() {
        let strip = Bar {
            warning: true,
            ..bar(1000.0, 3)
        }
        .layout();
        assert_eq!((strip.new_tab, strip.warning), (964.0, Some(932.0)));
        assert_eq!(
            strip.span,
            Span::new(84.0, 824.0),
            "the strip yields its space"
        );
        let overflowing = Bar {
            warning: true,
            ..bar(720.0, 12)
        }
        .layout();
        assert_eq!(
            overflowing.list,
            Some(652.0),
            "Show All Tabs stays next to +"
        );
        assert_eq!(overflowing.warning, Some(620.0));
        assert_eq!(overflowing.span, Span::new(84.0, 512.0));
    }

    #[test]
    fn a_window_too_narrow_for_its_tabs_still_lays_out() {
        let strip = bar(170.0, 3).layout();
        assert!(strip.overflow);
        assert_eq!(strip.span.width, 0.0, "no room left, not negative");
        assert_eq!(strip.max_scroll, 3.0 * 122.0 - 2.0);
        assert!(strip.chips.iter().all(|chip| chip.x.is_finite()));
        let none = bar(1000.0, 0).layout();
        assert!(none.chips.is_empty() && none.separators.is_empty() && !none.overflow);
    }

    #[test]
    fn separators_show_only_between_two_quiet_tabs() {
        let strip = Bar {
            selected: 1,
            ..bar(1000.0, 4)
        }
        .layout();
        // Gaps 0–1 and 1–2 touch the selected tab; only 2–3 shows, in the middle of its gap.
        assert_eq!(strip.separators, [strip.chips[2].end() + GAP / 2.0]);
        let hovered = Bar {
            selected: 1,
            hovered: Some(3),
            ..bar(1000.0, 4)
        }
        .layout();
        assert!(hovered.separators.is_empty());
        let dragged = Bar {
            selected: 0,
            dragged: Some(2),
            ..bar(1000.0, 5)
        }
        .layout();
        assert_eq!(dragged.separators, [dragged.chips[3].end() + GAP / 2.0]);
    }

    #[test]
    fn selecting_a_tab_scrolls_it_into_view_with_a_margin() {
        let strip = bar(720.0, 12).layout();
        assert_eq!(strip.revealing(11), 918.0, "the last one: the strip's end");
        // Tab 5 spans 610..730; brought to 24 from the trailing edge.
        assert_eq!(strip.revealing(5), 730.0 - 544.0 + REVEAL_MARGIN);
        assert_eq!(strip.revealing(0), 0.0);
        let scrolled = Bar {
            scroll: 242.0,
            ..bar(720.0, 12)
        }
        .layout();
        assert_eq!(scrolled.revealing(3), 242.0, "already in view");
        // Tab 2 spans 244..364: within 24 of the leading edge.
        assert_eq!(scrolled.revealing(2), 244.0 - REVEAL_MARGIN);
        assert_eq!(bar(1000.0, 3).layout().revealing(2), 0.0, "nothing scrolls");
    }

    #[test]
    fn the_wheel_moves_along_its_dominant_axis_within_the_ends() {
        let strip = Bar {
            scroll: 100.0,
            ..bar(720.0, 12)
        }
        .layout();
        assert_eq!(strip.wheeled(30.0, 5.0), 130.0);
        assert_eq!(
            strip.wheeled(-2.0, -40.0),
            60.0,
            "vertical wheels scroll too"
        );
        assert_eq!(strip.wheeled(-500.0, 0.0), 0.0);
        assert_eq!(strip.wheeled(0.0, 5000.0), 918.0);
        assert_eq!(bar(1000.0, 3).layout().wheeled(40.0, 0.0), 0.0);
    }

    /// A strip of `count` tabs in a 1000 pt window, chips at 84, 270, 456 …, and a grip on
    /// `from` pressed `grab` points into it.
    fn gripped(count: usize, from: usize, grab: f64) -> (Strip, Grip) {
        let strip = bar(1000.0, count).layout();
        let grip = Grip::new(&strip, from, strip.chips[from].x + grab).unwrap();
        (strip, grip)
    }

    /// The pointer inside the bar, level with the chips: a 40 pt row.
    fn level(x: f64) -> (f64, f64) {
        (x, 20.0)
    }

    const ROW: (f64, f64) = (1000.0, 40.0);

    #[test]
    fn a_press_becomes_a_drag_past_the_slop_and_stays_one() {
        let (strip, mut grip) = gripped(3, 0, 16.0);
        assert_eq!(grip.track(&strip, level(102.0), ROW), Drag::Press);
        assert_eq!(
            grip.track(&strip, level(97.0), ROW),
            Drag::Press,
            "either way"
        );
        assert_eq!(
            grip.track(&strip, level(105.0), ROW),
            Drag::Reorder { left: 89.0, to: 0 }
        );
        assert_eq!(
            grip.track(&strip, level(101.0), ROW),
            Drag::Reorder { left: 85.0, to: 0 },
            "back near the press it is still the drag it became"
        );
    }

    #[test]
    fn a_dragged_tab_takes_the_place_its_left_edge_is_nearest() {
        let (strip, mut grip) = gripped(3, 0, 16.0);
        // The chip's left edge is the pointer less where it was held; slots are 186 apart.
        assert_eq!(
            grip.track(&strip, level(290.0), ROW),
            Drag::Reorder { left: 274.0, to: 1 }
        );
        assert_eq!(
            grip.track(&strip, level(190.0), ROW),
            Drag::Reorder { left: 174.0, to: 0 },
            "less than half a slot is still the first place"
        );
        assert_eq!(
            grip.track(&strip, level(192.0), ROW),
            Drag::Reorder { left: 176.0, to: 0 },
            "92 of the 186 points to the next place"
        );
        assert_eq!(
            grip.track(&strip, level(193.0), ROW),
            Drag::Reorder { left: 177.0, to: 1 },
            "…and 93, half, is the next one"
        );
        // A tab dragged left from the last place.
        let (strip, mut grip) = gripped(3, 2, 100.0);
        assert_eq!(
            grip.track(&strip, level(500.0), ROW),
            Drag::Reorder { left: 400.0, to: 2 }
        );
        assert_eq!(
            grip.track(&strip, level(300.0), ROW),
            Drag::Reorder { left: 200.0, to: 1 }
        );
    }

    #[test]
    fn a_dragged_tab_stays_inside_the_strip_and_its_places() {
        let (strip, mut grip) = gripped(3, 1, 50.0);
        assert_eq!(
            grip.track(&strip, level(5.0), ROW),
            Drag::Reorder { left: 84.0, to: 0 },
            "not left of the strip"
        );
        assert_eq!(
            grip.track(&strip, level(990.0), ROW),
            Drag::Reorder { left: 456.0, to: 2 },
            "not right of the last place, though the strip goes on"
        );
        // A scrolled strip: the places are the ones in view, and the place is read from the
        // scroll now, not the one at the press.
        let scrolled = Bar {
            scroll: 400.0,
            ..bar(720.0, 12)
        }
        .layout();
        let mut grip = Grip::new(&scrolled, 5, scrolled.chips[5].x + 10.0).unwrap();
        let end = scrolled.span.end() - MIN_WIDTH;
        assert_eq!(
            grip.track(&scrolled, level(719.0), (720.0, 40.0)),
            Drag::Reorder {
                left: end,
                to: ((end - scrolled.span.x + 400.0) / 122.0).round() as usize
            },
            "no further than the edge it can be seen at"
        );
        assert_eq!(
            grip.track(&scrolled, level(0.0), (720.0, 40.0)),
            Drag::Reorder {
                left: scrolled.span.x,
                to: 3
            },
            "the first place in view: 400 / 122 rounds to 3"
        );
    }

    #[test]
    fn a_tab_pulled_away_from_the_bar_leaves_it() {
        let (strip, mut grip) = gripped(3, 1, 20.0);
        // Within TEAR_DISTANCE of the row, below or above: still reordering.
        assert!(matches!(
            grip.track(&strip, (300.0, 40.0 + TEAR_DISTANCE), ROW),
            Drag::Reorder { .. }
        ));
        assert!(matches!(
            grip.track(&strip, (300.0, -TEAR_DISTANCE), ROW),
            Drag::Reorder { .. }
        ));
        // Past it: the tab comes away, with no slop to cross first.
        let (strip, mut fresh) = gripped(3, 1, 20.0);
        let press = strip.chips[1].x + 20.0;
        assert_eq!(
            fresh.track(&strip, (press, 41.0 + TEAR_DISTANCE), ROW),
            Drag::TearOff
        );
        assert_eq!(
            grip.track(&strip, (300.0, -TEAR_DISTANCE - 1.0), ROW),
            Drag::TearOff,
            "up and out of the window too"
        );
        assert_eq!(
            grip.track(&strip, (-TEAR_DISTANCE - 1.0, 20.0), ROW),
            Drag::TearOff,
            "and sideways"
        );
        assert_eq!(
            grip.track(&strip, (1000.0 + TEAR_DISTANCE + 1.0, 20.0), ROW),
            Drag::TearOff
        );
    }

    #[test]
    fn a_lone_tab_or_an_absent_one_cannot_be_gripped() {
        let single = bar(1000.0, 1).layout();
        assert_eq!(
            Grip::new(&single, 0, 500.0),
            None,
            "its title moves the window"
        );
        let three = bar(1000.0, 3).layout();
        assert_eq!(Grip::new(&three, 3, 500.0), None);
    }

    #[test]
    fn a_carried_pane_over_a_tabs_middle_means_that_tab() {
        let strip = bar(1000.0, 3).layout();
        // Tabs are 184 wide from 84, 270 and 456; the middle is 18%..82% of a tab.
        let first = strip.chips[0];
        assert_eq!(
            strip.pane_over(first.x + first.width / 2.0),
            PaneOver::OnTab { index: 0 }
        );
        assert_eq!(
            strip.pane_over(strip.chips[1].x + 92.0),
            PaneOver::OnTab { index: 1 }
        );
        assert_eq!(
            strip.pane_over(strip.chips[2].x + 92.0),
            PaneOver::OnTab { index: 2 }
        );
        let core_from = first.x + TAB_CORE_FROM * first.width;
        let core_to = first.x + TAB_CORE_TO * first.width;
        assert_eq!(
            strip.pane_over(core_from - 1e-6),
            PaneOver::Between { gap: 0 }
        );
        assert_eq!(
            strip.pane_over(core_from + 1e-6),
            PaneOver::OnTab { index: 0 }
        );
        assert_eq!(
            strip.pane_over(core_to - 1e-6),
            PaneOver::OnTab { index: 0 }
        );
        assert_eq!(
            strip.pane_over(core_to + 1e-6),
            PaneOver::Between { gap: 1 }
        );
        assert_eq!(
            strip.pane_over(first.x),
            PaneOver::Between { gap: 0 },
            "a tab's own edge"
        );
    }

    #[test]
    fn a_carried_pane_between_tabs_means_a_new_tab_at_that_place() {
        let strip = bar(1000.0, 3).layout();
        // The 2 pt gaps: 268..270 and 454..456. A point there is between the tabs either side.
        assert_eq!(strip.pane_over(269.0), PaneOver::Between { gap: 1 });
        assert_eq!(strip.pane_over(455.0), PaneOver::Between { gap: 2 });
        // The edges of a tab lead to the gap on their side.
        assert_eq!(
            strip.pane_over(strip.chips[1].x + 5.0),
            PaneOver::Between { gap: 1 }
        );
        assert_eq!(
            strip.pane_over(strip.chips[1].end() - 5.0),
            PaneOver::Between { gap: 2 }
        );
        // Past the last tab but inside the strip, and outside the strip altogether.
        assert_eq!(
            strip.pane_over(strip.chips[2].end() + 10.0),
            PaneOver::Between { gap: 3 }
        );
        assert_eq!(
            strip.pane_over(strip.span.end()),
            PaneOver::Between { gap: 3 }
        );
        assert_eq!(strip.pane_over(9999.0), PaneOver::Between { gap: 3 });
        assert_eq!(strip.pane_over(10.0), PaneOver::Between { gap: 0 });
    }

    #[test]
    fn a_lone_tabs_title_takes_a_carried_pane_like_any_tab() {
        let strip = bar(1000.0, 1).layout();
        let title = strip.chips[0];
        assert_eq!(
            strip.pane_over(title.x + title.width / 2.0),
            PaneOver::OnTab { index: 0 }
        );
        assert_eq!(
            strip.pane_over(title.x + 0.1 * title.width),
            PaneOver::Between { gap: 0 }
        );
        assert_eq!(
            strip.pane_over(title.end() - 0.1 * title.width),
            PaneOver::Between { gap: 1 }
        );
    }

    #[test]
    fn a_scrolled_strip_is_read_where_its_tabs_are_in_view() {
        let scrolled = Bar {
            scroll: 242.0,
            ..bar(720.0, 12)
        }
        .layout();
        // Tab 2 is 86..206 on screen.
        assert_eq!(scrolled.pane_over(146.0), PaneOver::OnTab { index: 2 });
        assert_eq!(scrolled.pane_over(100.0), PaneOver::Between { gap: 2 });
        assert_eq!(
            scrolled.pane_over(10.0),
            PaneOver::Between { gap: 0 },
            "left of the strip"
        );
        assert_eq!(SPRING_DELAY, Duration::from_millis(550));
    }

    #[test]
    fn a_pane_going_into_a_tab_does_not_select_it() {
        let mut t = tabs(&[1, 2, 3], 2);
        assert!(
            !t.pane_to_tab(2, false, 3),
            "the source tab keeps its other panes"
        );
        assert_eq!((t.ids(), t.selected()), (&[1, 2, 3][..], Some(2)));
        assert!(!t.pane_to_tab(2, true, 2), "into its own tab is no move");
        assert!(!t.pane_to_tab(2, true, 9), "into a tab that is not here");
        assert!(!t.pane_to_tab(9, true, 3), "out of a tab that is not here");
        assert_eq!((t.ids(), t.selected()), (&[1, 2, 3][..], Some(2)));
    }

    #[test]
    fn an_emptied_tab_closes_and_the_tab_the_pane_went_to_comes_forward() {
        // The selected tab emptied: the pane's new tab is selected, not the right neighbour.
        let mut t = tabs(&[1, 2, 3], 2);
        assert!(t.pane_to_tab(2, true, 3));
        assert_eq!((t.ids(), t.selected()), (&[1, 3][..], Some(3)));
        // The other side, so the choice is not the neighbour by luck.
        let mut t = tabs(&[1, 2, 3], 2);
        assert!(t.pane_to_tab(2, true, 1));
        assert_eq!((t.ids(), t.selected()), (&[1, 3][..], Some(1)));
        // A background tab emptied: what is on screen stays.
        let mut t = tabs(&[1, 2, 3], 3);
        assert!(t.pane_to_tab(1, true, 2));
        assert_eq!((t.ids(), t.selected()), (&[2, 3][..], Some(3)));
        // The target was opened by resting on it, and the source — behind it — empties.
        let mut t = tabs(&[1, 2, 3], 3);
        assert!(t.pane_to_tab(2, true, 3));
        assert_eq!((t.ids(), t.selected()), (&[1, 3][..], Some(3)));
    }

    #[test]
    fn a_pane_let_go_between_tabs_becomes_a_tab_that_is_not_selected() {
        let mut t = tabs(&[1, 2, 3], 2);
        assert_eq!(t.pane_to_new_tab(2, 3, 9, 0), NewTab::Created);
        assert_eq!((t.ids(), t.selected()), (&[9, 1, 2, 3][..], Some(2)));
        let mut t = tabs(&[1, 2, 3], 1);
        assert_eq!(t.pane_to_new_tab(1, 2, 9, 2), NewTab::Created);
        assert_eq!((t.ids(), t.selected()), (&[1, 2, 9, 3][..], Some(1)));
        let mut t = tabs(&[1, 2, 3], 1);
        assert_eq!(t.pane_to_new_tab(1, 2, 9, 99), NewTab::Created);
        assert_eq!(
            (t.ids(), t.selected()),
            (&[1, 2, 3, 9][..], Some(1)),
            "past the end is the end"
        );
        assert_eq!(
            t.pane_to_new_tab(1, 2, 9, 0),
            NewTab::Unchanged,
            "the id is taken"
        );
        assert_eq!(
            t.pane_to_new_tab(8, 2, 10, 0),
            NewTab::Unchanged,
            "no such source"
        );
    }

    #[test]
    fn a_tabs_only_pane_let_go_between_tabs_moves_the_tab() {
        // [A, B, C] with A let go in gap 2 — between B and C — is [B, A, C], not [B, C, A].
        let mut t = tabs(&[1, 2, 3], 1);
        assert_eq!(t.pane_to_new_tab(1, 1, 9, 2), NewTab::Reordered);
        assert_eq!(
            (t.ids(), t.selected()),
            (&[2, 1, 3][..], Some(1)),
            "the tab keeps its id"
        );
        assert_eq!(t.selected_index(), Some(1), "and the selection stays on it");
        let mut t = tabs(&[1, 2, 3], 2);
        assert_eq!(t.pane_to_new_tab(1, 1, 9, 3), NewTab::Reordered);
        assert_eq!((t.ids(), t.selected()), (&[2, 3, 1][..], Some(2)));
        // Its own place, either side of it, is no change.
        for gap in [0, 1] {
            let mut t = tabs(&[1, 2, 3], 1);
            assert_eq!(
                t.pane_to_new_tab(1, 1, 9, gap),
                NewTab::Unchanged,
                "gap {gap}"
            );
            assert_eq!(t.ids(), &[1, 2, 3]);
        }
        for gap in [2, 3] {
            let mut t = tabs(&[1, 2, 3], 1);
            assert_eq!(
                t.pane_to_new_tab(3, 1, 9, gap),
                NewTab::Unchanged,
                "gap {gap}"
            );
        }
        let mut t = tabs(&[1, 2, 3], 1);
        assert_eq!(t.pane_to_new_tab(3, 1, 9, 0), NewTab::Reordered);
        assert_eq!(t.ids(), &[3, 1, 2]);
        assert_eq!(
            (0..=3).map(|gap| gap_to_index(1, gap)).collect::<Vec<_>>(),
            vec![0, 1, 1, 2]
        );
    }

    #[test]
    fn an_empty_strip_selects_the_first_tab_that_comes() {
        let mut t: Tabs<u64> = Tabs::empty();
        assert!(t.is_empty());
        assert_eq!((t.selected(), t.adjacent(true)), (None, None));
        t.insert(4);
        assert_eq!((t.ids(), t.selected()), (&[4][..], Some(4)));
        let mut t: Tabs<u64> = Tabs::empty();
        assert!(t.append(5));
        assert_eq!(t.selected(), Some(5), "a strip with a tab has a selection");
        let mut t: Tabs<u64> = Tabs::empty();
        t.insert_at(6, 3);
        assert_eq!((t.ids(), t.selected()), (&[6][..], Some(6)));
    }

    #[test]
    fn a_new_tab_placed_between_tabs_leaves_the_selection() {
        let mut t = tabs(&[1, 2], 1);
        assert!(t.place_new(9, 0));
        assert_eq!((t.ids(), t.selected()), (&[9, 1, 2][..], Some(1)));
        assert!(t.place_new(8, 99));
        assert_eq!((t.ids(), t.selected_index()), (&[9, 1, 2, 8][..], Some(1)));
        assert!(!t.place_new(8, 0), "already here");
    }

    #[test]
    fn the_slot_under_a_point_is_the_one_whose_extent_holds_it() {
        let strip = bar(1000.0, 3).layout();
        // Slots are 186 apart from 84: 84..270, 270..456, 456..642.
        assert_eq!(strip.slot_at(100.0), 0);
        assert_eq!(strip.slot_at(269.9), 0);
        assert_eq!(
            strip.slot_at(270.0),
            1,
            "the gap between two tabs is the later one's"
        );
        assert_eq!(strip.slot_at(500.0), 2);
        assert_eq!(strip.slot_at(9999.0), 2, "right of the strip: the last");
        assert_eq!(strip.slot_at(10.0), 0, "left of it: the first");
        let scrolled = Bar {
            scroll: 242.0,
            ..bar(720.0, 12)
        }
        .layout();
        // Slots of 122 from 84 − 242: tab 2 holds 86..208, so the places follow what is in view.
        assert_eq!(scrolled.slot_at(146.0), 2);
        assert_eq!(scrolled.slot_at(207.9), 2);
        assert_eq!(scrolled.slot_at(208.0), 3);
        assert_eq!(
            scrolled.slot_at(700.0),
            11,
            "over the buttons: the end, though the tabs there are scrolled out of sight"
        );
        let single = bar(1000.0, 1).layout();
        assert_eq!(single.slot_at(700.0), 0, "a lone title has one place");
    }

    #[test]
    fn room_made_for_a_tab_moves_the_places_not_the_answer() {
        // A tab carried over a strip of 5 is placed on the strip of 6 it will make: the room
        // opens under the pointer, whatever the tabs' widths become with one more.
        let six = bar(1000.0, 6).layout();
        let x = 400.0;
        let gap = six.slot_at(x);
        let seated: Vec<usize> = (0..5).map(|index| seat(index, Some(gap))).collect();
        assert!(
            !seated.contains(&gap),
            "no tab sits where the carried one goes: {seated:?} / {gap}"
        );
        assert!(
            six.chips[gap].x <= x && x < six.chips[gap].end() + GAP,
            "the pointer is over the room that opened"
        );
    }

    #[test]
    fn the_tabs_from_a_gap_on_take_the_next_place() {
        assert_eq!(seat(0, None), 0);
        assert_eq!(seat(3, None), 3);
        assert_eq!(seat(0, Some(2)), 0);
        assert_eq!(seat(1, Some(2)), 1);
        assert_eq!(seat(2, Some(2)), 3, "the tab at the gap moves on");
        assert_eq!(seat(4, Some(2)), 5);
        assert_eq!(seat(0, Some(0)), 1);
    }

    #[test]
    fn a_held_tab_stands_at_its_place_among_the_others() {
        let order = |from, to| {
            let mut items = vec!['a', 'b', 'c', 'd'];
            reordered(&mut items, from, to);
            items.into_iter().collect::<String>()
        };
        assert_eq!(order(0, 2), "bcad");
        assert_eq!(order(3, 0), "dabc");
        assert_eq!(order(1, 1), "abcd");
        assert_eq!(order(0, 99), "bcda", "past the end is the end");
        assert_eq!(order(9, 0), "abcd", "an absent tab is no move");
    }

    #[test]
    fn a_tab_let_go_lands_where_the_pointer_was() {
        assert_eq!(landing(Some(Over::Own { place: 2 })), Landing::Reorder(2));
        assert_eq!(landing(Some(Over::Other { gap: 1 })), Landing::Join(1));
        assert_eq!(
            landing(None),
            Landing::Detach,
            "over no strip: a window of its own"
        );
    }

    /// Every combination of the five signals: the indicator is the first one that is on, in
    /// the order question, running, failed, finished, uploading — and nothing when none is.
    #[test]
    fn the_most_urgent_signal_is_the_indicator() {
        let order = [
            Indicator::Question,
            Indicator::Running,
            Indicator::Failed,
            Indicator::Finished,
            Indicator::Uploading,
        ];
        for bits in 0u8..32 {
            let on = |index: usize| bits & (1 << index) != 0;
            let signals = Signals {
                question: on(0),
                running: on(1),
                failed: on(2),
                finished: on(3),
                uploading: on(4),
            };
            let expected = (0..order.len())
                .find(|&index| on(index))
                .map(|index| order[index]);
            assert_eq!(indicator(signals), expected, "{signals:?}");
        }
    }

    /// The table's named rows: what the user sees when two things happen in one tab.
    #[test]
    fn a_question_outranks_a_running_command_and_a_failure_a_success() {
        assert_eq!(indicator(Signals::default()), None);
        assert_eq!(
            indicator(Signals {
                question: true,
                running: true,
                ..Signals::default()
            }),
            Some(Indicator::Question)
        );
        assert_eq!(
            indicator(Signals {
                failed: true,
                finished: true,
                ..Signals::default()
            }),
            Some(Indicator::Failed)
        );
        assert_eq!(
            indicator(Signals {
                running: true,
                uploading: true,
                ..Signals::default()
            }),
            Some(Indicator::Running)
        );
        assert_eq!(
            indicator(Signals {
                uploading: true,
                ..Signals::default()
            }),
            Some(Indicator::Uploading)
        );
    }

    /// VoiceOver names every indicator, the question in words a listener acts on.
    #[test]
    fn every_indicator_is_spoken() {
        assert_eq!(Indicator::Question.spoken(), "waiting for an answer");
        for indicator in [
            Indicator::Running,
            Indicator::Failed,
            Indicator::Finished,
            Indicator::Uploading,
        ] {
            assert!(!indicator.spoken().is_empty());
        }
    }

    /// What ends while the tab is in the background marks it, by outcome; what ends while it is
    /// selected — in a window behind another too — marks nothing; looking clears both.
    #[test]
    fn an_end_the_user_did_not_see_marks_the_tab_until_it_is_selected() {
        let seen = Tally::default();
        let mut unseen = Unseen::default();
        unseen.observe(
            seen,
            Tally {
                finished: 1,
                failed: 0,
            },
            true,
        );
        assert_eq!(unseen, Unseen::default(), "the selected tab");
        unseen.observe(
            seen,
            Tally {
                finished: 1,
                failed: 0,
            },
            false,
        );
        assert_eq!(
            unseen,
            Unseen {
                finished: true,
                failed: false
            }
        );
        let after = Tally {
            finished: 1,
            failed: 0,
        };
        unseen.observe(
            after,
            Tally {
                finished: 1,
                failed: 1,
            },
            false,
        );
        assert!(unseen.failed && unseen.finished, "a failure joins");
        unseen.observe(
            Tally {
                finished: 1,
                failed: 1,
            },
            Tally {
                finished: 2,
                failed: 1,
            },
            false,
        );
        assert!(unseen.failed, "a later success does not hide the failure");
        assert_eq!(
            indicator(Signals {
                failed: unseen.failed,
                finished: unseen.finished,
                ..Signals::default()
            }),
            Some(Indicator::Failed)
        );
        unseen.looked();
        assert_eq!(unseen, Unseen::default());
        unseen.observe(after, after, false);
        assert_eq!(unseen, Unseen::default(), "nothing ended");
    }

    /// Several ends between two looks are one mark, the counts' wrap included.
    #[test]
    fn ends_between_two_looks_are_one_mark() {
        let mut unseen = Unseen::default();
        unseen.observe(
            Tally {
                finished: u64::MAX,
                failed: 3,
            },
            Tally {
                finished: 1,
                failed: 3,
            },
            false,
        );
        assert_eq!(
            unseen,
            Unseen {
                finished: true,
                failed: false
            }
        );
    }

    /// One step a second, twelve to a turn; a minute a step from an hour on, where the counter
    /// moves once a minute; still under Reduce Motion.
    #[test]
    fn the_ring_steps_with_the_counter() {
        let secs = Duration::from_secs;
        assert_eq!(ring_step(Duration::from_millis(900), false), 0);
        assert_eq!(ring_step(secs(1), false), 1);
        assert_eq!(ring_step(Duration::from_millis(11_999), false), 11);
        assert_eq!(ring_step(secs(12), false), 0, "a whole turn");
        assert_eq!(ring_step(secs(3599), false), 11);
        assert_eq!(ring_step(secs(3600), false), 0);
        assert_eq!(ring_step(secs(3660), false), 1, "a minute a step");
        assert_eq!(ring_step(secs(3661), false), 1, "not a second");
        assert_eq!(ring_step(secs(5), true), 0, "Reduce Motion: still");
        assert_eq!(
            f64::from(ring_step(secs(3), false)) * RING_STEP_DEGREES,
            90.0
        );
    }

    fn clock(visible: bool, reduce_motion: bool, running: &[u64], card: Option<u64>) -> Clock {
        Clock {
            visible,
            reduce_motion,
            running: running
                .iter()
                .map(|&ms| Duration::from_millis(ms))
                .collect(),
            card: card.map(Duration::from_millis),
        }
    }

    /// The bar's clock: set only while something on a visible bar changes with time, at the
    /// nearest tick of the duration counters.
    #[test]
    fn the_clock_runs_only_while_a_visible_ring_turns_or_a_card_counts() {
        let ms = Duration::from_millis;
        assert_eq!(clock(true, false, &[], None).delay(), None, "nothing runs");
        assert_eq!(
            clock(true, false, &[300], None).delay(),
            Some(ms(700)),
            "to the counter's first second"
        );
        assert_eq!(
            clock(true, false, &[2_400, 5_900], None).delay(),
            Some(ms(100)),
            "the nearest of the running tabs"
        );
        assert_eq!(
            clock(false, false, &[2_400], Some(2_400)).delay(),
            None,
            "an occluded window"
        );
        assert_eq!(
            clock(true, true, &[2_400], None).delay(),
            None,
            "Reduce Motion stops the rings"
        );
        assert_eq!(
            clock(true, true, &[2_400], Some(2_400)).delay(),
            Some(ms(600)),
            "but not the open card's seconds, which are text"
        );
        assert_eq!(
            clock(true, false, &[1_200], Some(4_900)).delay(),
            Some(ms(100)),
            "the card's tick is nearer"
        );
        assert_eq!(
            clock(true, false, &[3_600_000 + 20_000], None).delay(),
            Some(Duration::from_secs(40)),
            "an hour on: the counter's minute"
        );
    }

    /// A full-screen program runs as long as it is open: no ring, and so no clock for it — but the
    /// tab's other signals are read as before.
    #[test]
    fn a_full_screen_program_turns_no_ring() {
        let ten = Some(Duration::from_secs(10));
        assert_eq!(
            ring_running(ten, None, false),
            ten,
            "a command on the main screen"
        );
        assert_eq!(
            ring_running(ten, None, true),
            None,
            "vim, htop, an agent's interface"
        );
        assert_eq!(
            ring_running(None, None, true),
            None,
            "a shell at its prompt"
        );
        let signals = |full_screen| Signals {
            running: ring_running(ten, None, full_screen).is_some(),
            failed: true,
            ..Signals::default()
        };
        assert_eq!(indicator(signals(false)), Some(Indicator::Running));
        assert_eq!(
            indicator(signals(true)),
            Some(Indicator::Failed),
            "an earlier failure shows through"
        );
    }

    /// A program that reports its status owns the ring: it turns while the program works, stands
    /// still while it waits, and the full-screen rule does not apply to what the program says.
    #[test]
    fn a_reporting_program_owns_the_ring() {
        let ten = Some(Duration::from_secs(10));
        let five = Some(Duration::from_secs(5));
        let program = |working, blocked| Some(ProgramActivity { working, blocked });
        assert_eq!(
            ring_running(ten, program(five, false), false),
            five,
            "the program's clock, not the command's"
        );
        assert_eq!(
            ring_running(ten, program(None, false), false),
            None,
            "an agent waiting for its user: the command is open, nothing turns"
        );
        assert_eq!(
            ring_running(ten, program(None, true), true),
            None,
            "blocked is a question, not work"
        );
        assert_eq!(
            ring_running(None, program(five, false), true),
            five,
            "a full-screen program that says it works turns the ring"
        );
    }

    /// VoiceOver hears what the chip shows: the indicator — an upload with its percentage — and
    /// a marked host; an unmarked host says nothing.
    #[test]
    fn a_chip_says_its_indicator_and_its_host() {
        assert_eq!(spoken("make", None, None, HostMark::None), "make");
        assert_eq!(
            spoken("make", Some(Indicator::Running), None, HostMark::None),
            "make, running"
        );
        assert_eq!(
            spoken(
                "scp",
                Some(Indicator::Uploading),
                Some(42),
                HostMark::Production
            ),
            "scp, uploading 42%, Production host"
        );
        assert_eq!(
            spoken(
                "x",
                Some(Indicator::Failed),
                Some(42),
                HostMark::Rgb(0x123456)
            ),
            "x, failed, Marked host",
            "the upload's percentage only beside its own indicator"
        );
        assert_eq!(host_name(HostMark::None), None);
    }

    /// The card's lines in the design's order and roles; a part nobody knows is left out.
    #[test]
    fn the_card_tells_the_tabs_story_in_lines() {
        let card = Card {
            title: "api".into(),
            directory: Some("/Users/me/src/api".into()),
            home: Some("/Users/me".into()),
            command: Some(CardCommand::Running {
                command: "$ make test".into(),
                elapsed: Duration::from_secs(65),
            }),
            upload: Some(("logs.tgz".into(), 42, false)),
            mark: HostMark::Staging,
            panes: 2,
            ..Card::default()
        };
        assert_eq!(
            card_lines(&card),
            [
                ("api".to_owned(), Tone::Title),
                ("~/src/api".to_owned(), Tone::Dim),
                ("Running · $ make test · 1m 05s".to_owned(), Tone::Accent),
                ("↑ Uploading logs.tgz · 42%".to_owned(), Tone::Accent),
                ("Staging host".to_owned(), Tone::Mark),
                ("2 panes".to_owned(), Tone::Dim),
            ]
        );
        let remote = Card {
            title: "⇄ prod".into(),
            remote: Some(("prod".into(), "/var/www".into())),
            directory: Some("/Users/me".into()),
            command: Some(CardCommand::Failed {
                command: "r$ false".into(),
                exit: 1,
            }),
            panes: 1,
            ..Card::default()
        };
        assert_eq!(
            card_lines(&remote),
            [
                ("⇄ prod".to_owned(), Tone::Title),
                ("prod:/var/www".to_owned(), Tone::Dim),
                ("Exit 1 · r$ false".to_owned(), Tone::Error),
            ]
        );
        let quiet = Card {
            title: "vim".into(),
            remote: Some(("prod".into(), String::new())),
            command: Some(CardCommand::Running {
                command: String::new(),
                elapsed: Duration::from_millis(400),
            }),
            upload: Some(("a.txt".into(), 7, true)),
            ..Card::default()
        };
        assert_eq!(
            card_lines(&quiet),
            [
                ("vim".to_owned(), Tone::Title),
                ("prod".to_owned(), Tone::Dim),
                ("Running".to_owned(), Tone::Accent),
                ("↓ Downloading a.txt · 7%".to_owned(), Tone::Accent),
            ],
            "no row, no time below the counter's floor, no remote directory"
        );
        let done = Card {
            title: "t".into(),
            command: Some(CardCommand::Finished {
                command: "$ ls".into(),
                duration: Some("0.2s".into()),
            }),
            ..Card::default()
        };
        assert_eq!(
            card_lines(&done)[1],
            ("Finished · $ ls · 0.2s".to_owned(), Tone::Success)
        );
    }
}
