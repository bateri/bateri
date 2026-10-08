//! Tabs of one window: the **pure** half of the tab bar. The order of the tabs, which one is
//! selected, what ⌘1…⌘9 reach and where every part of the strip sits horizontally are decided
//! here; the platform shell only builds views at these positions and calls these operations from
//! its single place that changes a window's tabs.
//!
//! It does not see a UI toolkit and has its own tests (`split`'s precedent), so a Linux shell can
//! draw the same strip from the same numbers.
//!
//! **A tab is not a `bt_core::TabId`.** A tab is the container of a split tree's panes; `TabId` is
//! one *pane's* identity (`TERM_SESSION_ID`, `bateri://tab/<id>`) — a tab holds one or more of
//! them, so the relation is one to many and nothing here assumes otherwise. The identity type
//! is generic: the shell picks whatever names its tabs.
//!
//! Coordinates are points, horizontal only, in the **bar's** space: the bar spans the window's
//! full width and its origin is the window's left edge. The title row's height is not here — it
//! is whatever AppKit reports for the window's title row, a single copy read from the window.

use std::path::PathBuf;
use std::time::Duration;

use bt_core::HostMark;

/// A window's tabs in strip order, and the selected one.
///
/// Invariant: a selection exists exactly when there is at least one tab. A window is born with
/// one tab ([`Tabs::new`]); closing the last one leaves the model empty, which is the window's
/// cue to close.
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
    /// another window (Merge All Windows) does not change what this window shows. `false` if the
    /// id is already here.
    pub fn append(&mut self, id: K) -> bool {
        if self.index_of(id).is_some() {
            return false;
        }
        self.order.push(id);
        true
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

/// What a tab's indicator slot shows, left of its title: one glyph, the most urgent of the tab's
/// signals ([`indicator`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Indicator {
    /// A question of the tab waits for an answer the user cannot see — asked while the tab was in
    /// the background, or left up when the user switched away. First: nothing goes on in that tab
    /// until it is answered.
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
