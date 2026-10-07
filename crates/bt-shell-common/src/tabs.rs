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
    if index >= count {
        return None;
    }
    let digit = match index {
        0..=7 => index + 1,
        _ if index + 1 == count => 9,
        _ => return None,
    };
    Some(format!("⌘{digit}"))
}

/// Space between two neighbouring tabs. Also where the separator line sits.
pub const GAP: f64 = 2.0;

/// The widest a tab gets: a few tabs in a wide window do not stretch across it.
pub const MAX_WIDTH: f64 = 184.0;

/// The narrowest a tab gets; past this the strip scrolls instead of shrinking the tabs further,
/// so a title stays readable.
pub const MIN_WIDTH: f64 = 120.0;

/// Empty space always kept between the last tab and the buttons on the right: the window is
/// moved from there. Tabs shrink before giving it up.
pub const DRAG_MARGIN: f64 = 48.0;

/// Side of a square button on the right of the bar (`+`, Show All Tabs, the settings warning).
pub const BUTTON: f64 = 28.0;

/// Space between the window's right edge and `+`, and between the drag margin and the leftmost
/// button.
pub const EDGE: f64 = 8.0;

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
        let new_tab = self.width - EDGE - BUTTON;
        let left_of = |x: f64| x - BUTTON_GAP - BUTTON;
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
            slot,
        }
    }
}

impl Strip {
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
            Span::new(84.0, 824.0),
            "plus − 8 − 48 is the strip's end"
        );
        assert_eq!(xs(&strip), [(84.0, 184.0), (270.0, 184.0), (456.0, 184.0)]);
        assert!(!strip.overflow);
        assert_eq!((strip.max_scroll, strip.scroll), (0.0, 0.0));
        assert!(!strip.fade_leading && !strip.fade_trailing);
    }

    #[test]
    fn more_tabs_shrink_to_share_the_strip() {
        let strip = bar(1000.0, 5).layout();
        // (824 + 2) / 5 − 2 = 163.2, whole points.
        assert_eq!(strip.chips[0].width, 163.0);
        assert_eq!(strip.chips[4].x, 84.0 + 4.0 * 165.0);
        assert!(!strip.overflow);
        assert_eq!(bar(1000.0, 6).layout().chips[0].width, 135.0);
        let seven = bar(1000.0, 7).layout();
        assert!(seven.overflow, "116 would be below the minimum");
        assert_eq!(seven.chips[0].width, MIN_WIDTH);
    }

    #[test]
    fn overflowing_tabs_stop_at_the_minimum_and_scroll() {
        // Twelve tabs in a 720 pt window: well past what fits.
        let strip = bar(720.0, 12).layout();
        assert!(strip.overflow);
        assert_eq!(strip.new_tab, 684.0);
        assert_eq!(strip.list, Some(652.0), "Show All Tabs, 4 left of +");
        assert_eq!(strip.span, Span::new(84.0, 512.0));
        assert!(strip.chips.iter().all(|chip| chip.width == MIN_WIDTH));
        assert_eq!(strip.chips[11].x, 84.0 + 11.0 * 122.0);
        // 12 × 122 − 2 = 1462 of tabs in a 512 strip.
        assert_eq!(strip.max_scroll, 950.0);
        assert!(!strip.fade_leading && strip.fade_trailing, "at the start");

        let end = Bar {
            scroll: 950.0,
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
        assert_eq!(past.scroll, 950.0, "clamped");
        let before = Bar {
            scroll: -3.0,
            ..bar(720.0, 12)
        }
        .layout();
        assert_eq!(before.scroll, 0.0);
        let almost = Bar {
            scroll: 949.5,
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
            Span::new(84.0, 792.0),
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
        assert_eq!(overflowing.span, Span::new(84.0, 480.0));
    }

    #[test]
    fn a_window_too_narrow_for_its_tabs_still_lays_out() {
        let strip = bar(200.0, 3).layout();
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
        assert_eq!(strip.revealing(11), 950.0, "the last one: the strip's end");
        // Tab 5 spans 610..730; brought to 24 from the trailing edge.
        assert_eq!(strip.revealing(5), 730.0 - 512.0 + REVEAL_MARGIN);
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
        assert_eq!(strip.wheeled(0.0, 5000.0), 950.0);
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
}
