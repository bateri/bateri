//! The editors of a module's page, the host's half of them: the kit's editor (`pencere-editor`, the
//! same in every host) in each editor's room, its document kept for as long as an editor of its
//! name is on the page, what the person does done to the one with the keyboard, what the page asks
//! done, each drawn in its place, its caret blinked on the pane's clock.
//!
//! A frame lays the page out measuring with the editors (`Editors::measure`), so that a room
//! scrolls over its editor's content and what is anchored to a caret stands by it; then the editors
//! are made for the rooms there and what the page's replies asked of them is done
//! ([`Editing::settle`]); where a caret an overlay stands by moved, the same tree is laid out again,
//! the page asked nothing. After anything is done to them — in a frame or between frames, where the
//! pane's keys, input method and pointer reach them — what they have for the page is told it, its
//! texts first ([`Editing::told`]).

use std::sync::Arc;
use std::time::Instant;

use pencere_editor::{EditorEvent, Editors};
use pencere_view::edit::{EditorAsk, TextSnapshot};
use pencere_view::{Event, Id, Keyboard, Laid, Measure, Point, Reply, ScrollTo};

pub use pencere_editor::Route;

/// How long the caret stays lit, then dark: the terminal's cursor's half period
/// (`bt_core::CURSOR_BLINK_INTERVAL`).
const HALF_PERIOD: f64 = bt_core::CURSOR_BLINK_INTERVAL;

/// How long after the caret last moved it stops blinking, lit: the terminal cursor's rule (`bt_gpu`'s
/// blink), so that a page at rest goes back to drawing no frames.
const IDLE_STOP: f64 = 15.0;

/// What the host keeps of a page's editors.
pub(super) struct Editing {
    pub(super) editors: Editors,
    /// What the page's replies asked of its editors: done in the next frame, once the editors are
    /// made for the page it laid out (a reply may come with the page that has the editor).
    asks: Vec<(Id, EditorAsk)>,
    /// The editor a reply sent the keyboard to that was not there yet: given it once the editors
    /// are made.
    focus: Option<Id>,
    /// The editors' clock: when an edit is done, for what is undone together (typing that pauses
    /// is a group of its own).
    born: Instant,
    pub(super) blink: Blink,
}

/// What came of something done to the editors, for the page and for the host.
#[derive(Default)]
pub(super) struct Told {
    /// The text of each editor: posted to the page before the events, so that it reads the version
    /// they tell of. Empty where there are no events.
    pub texts: Vec<(Id, Arc<dyn TextSnapshot>)>,
    pub events: Vec<Event>,
    /// Where an editor asks to be scrolled, to keep its caret in view.
    pub scrolls: Vec<ScrollTo>,
    /// The page is to be laid out again as it is: an editor's content grew or shrank, or a caret
    /// an overlay stands by moved.
    pub relayout: bool,
}

impl Told {
    /// Whether there is anything to tell or to do.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty() && self.scrolls.is_empty() && !self.relayout
    }
}

impl Editing {
    pub(super) fn new() -> Self {
        Self {
            editors: Editors::new(),
            asks: Vec::new(),
            focus: None,
            born: Instant::now(),
            blink: Blink::new(),
        }
    }

    /// Now, on the editors' clock (seconds).
    pub(super) fn now(&self) -> f64 {
        self.born.elapsed().as_secs_f64()
    }

    /// Takes what `reply` asks of the editors out of it, to be done at the next frame, and gives
    /// the keyboard where it says (`Keyboard::Field` of an editor; a field or the page is none of
    /// them). Whether there was anything for the editors.
    pub(super) fn take_reply(&mut self, reply: &mut Reply) -> bool {
        let asks = std::mem::take(&mut reply.editor);
        let any = !asks.is_empty();
        self.asks.extend(asks);
        match reply.keyboard {
            Some(Keyboard::Field(id)) => {
                self.editors.set_focus(Some(id));
                self.focus = (self.editors.focused() != Some(id)).then_some(id);
                self.blink.moved();
                true
            }
            Some(Keyboard::Page) => {
                self.editors.set_focus(None);
                self.focus = None;
                true
            }
            None => any,
        }
    }

    /// The keyboard went where the person put it (a field, the page, an editor): a reply's that
    /// was waiting for its editor is not given any more.
    pub(super) fn person_moved_keyboard(&mut self) {
        self.focus = None;
    }

    /// After the page was laid out (`laid`): an editor for each room there, the keyboard where a
    /// reply sent it, and what the replies asked done.
    pub(super) fn settle(&mut self, laid: &Laid, measure: &mut dyn Measure) {
        self.editors.sync(laid, measure);
        if let Some(id) = self.focus.take() {
            self.editors.set_focus(Some(id));
        }
        let now = self.now();
        for (id, ask) in std::mem::take(&mut self.asks) {
            self.editors.ask(laid, id, ask, measure, now);
        }
    }

    /// What the editors have for the page and the host now. What moved a caret puts it back lit.
    pub(super) fn told(&mut self) -> Told {
        let scrolls = self.editors.take_scrolls();
        let relayout = self.editors.wants_layout();
        let events = self.editors.take_events();
        if events.iter().any(|event| {
            matches!(
                event,
                EditorEvent::Edited { .. }
                    | EditorEvent::Selection { .. }
                    | EditorEvent::Focus(Some(_))
            )
        }) {
            self.blink.moved();
        }
        let texts = if events.is_empty() {
            Vec::new()
        } else {
            self.editors
                .snapshots()
                .into_iter()
                .map(|(id, text)| (id, Arc::new(text) as Arc<dyn TextSnapshot>))
                .collect()
        };
        Told {
            texts,
            events: events.into_iter().map(Event::from).collect(),
            scrolls,
            relayout,
        }
    }
}

/// Whether `at` (points of the page) is in the room of an editor that shows: a press there is not
/// one that takes the keyboard back to the page, though what is over the room (an overlay by the
/// caret) is not the editor's.
pub(super) fn in_a_room(laid: &Laid, at: Point) -> bool {
    laid.editors().iter().any(|room| {
        let r = room.rect;
        !room.hidden && at.x >= r.x && at.y >= r.y && at.x < r.x + r.w && at.y < r.y + r.h
    })
}

/// The caret's blink: lit for a half period, dark for one, from the frame after it last moved (a
/// caret that moved is lit), lit and still after [`IDLE_STOP`] or with Reduce Motion on, and not drawn
/// while the pane's keyboard is not in the page of the person's window. The times are the frames'.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Blink {
    /// The frame the caret was last put back lit at.
    since: Option<f64>,
    /// It moved since: the next frame puts it back lit.
    moved: bool,
    /// The pane's keyboard is in the page, in the person's window.
    active: bool,
}

impl Blink {
    pub(super) fn new() -> Self {
        Self {
            since: None,
            moved: true,
            active: true,
        }
    }

    /// Something moved the caret, or the editor took the keyboard.
    pub(super) fn moved(&mut self) {
        self.moved = true;
    }

    /// Whether the pane's keyboard is in the page in the person's window. Coming back puts the
    /// caret back lit.
    pub(super) fn set_active(&mut self, active: bool) {
        if active && !self.active {
            self.moved = true;
        }
        self.active = active;
    }

    /// Whether the caret is drawn in the frame at `now`, where an editor has the keyboard
    /// (`shown`), and when that is next to change: `None` until something changes it.
    pub(super) fn at(&mut self, now: f64, still: bool, shown: bool) -> (bool, Option<f64>) {
        if !shown || !self.active {
            self.moved = true;
            return (false, None);
        }
        let since = match self.since {
            Some(since) if !self.moved && now >= since => since,
            _ => {
                self.moved = false;
                self.since = Some(now);
                now
            }
        };
        let lit_for = now - since;
        if still || lit_for >= IDLE_STOP {
            return (true, None);
        }
        let phase = (lit_for / HALF_PERIOD).floor();
        let lit = phase.rem_euclid(2.0) < 1.0;
        let next = since + (phase + 1.0) * HALF_PERIOD;
        (lit, Some(next.min(since + IDLE_STOP)))
    }
}

/// Text measured with no renderer, for the tests: each character half its font's size across.
#[cfg(test)]
pub(super) struct Even;

#[cfg(test)]
impl Measure for Even {
    fn measure(&mut self, font: &pencere_view::Font, text: &str) -> pencere_view::Metrics {
        pencere_view::Metrics {
            width: text.chars().count() as f32 * font.size * 0.5,
            height: font.size * 1.25,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_caret_that_moved_is_lit_then_blinks_on_the_frames_clock_and_stops_lit() {
        let mut blink = Blink::new();
        assert_eq!(blink.at(10.0, false, true), (true, Some(10.5)));
        assert_eq!(blink.at(10.5, false, true), (false, Some(11.0)));
        assert_eq!(blink.at(10.7, false, true), (false, Some(11.0)));
        assert_eq!(blink.at(11.0, false, true), (true, Some(11.5)));
        // A key: lit again, from the frame after it.
        blink.moved();
        assert_eq!(blink.at(11.25, false, true), (true, Some(11.75)));
        // Still after a while without a change: lit, and nothing more to wake for.
        assert_eq!(blink.at(25.75, false, true), (false, Some(26.25)));
        assert_eq!(blink.at(26.25, false, true), (true, None));
        assert_eq!(blink.at(40.0, false, true), (true, None));
    }

    #[test]
    fn a_caret_is_not_drawn_away_from_the_keyboard_and_is_still_with_reduce_motion() {
        let mut blink = Blink::new();
        assert_eq!(
            blink.at(1.0, false, false),
            (false, None),
            "no editor has it"
        );
        blink.set_active(false);
        assert_eq!(
            blink.at(2.0, false, true),
            (false, None),
            "another pane has it"
        );
        blink.set_active(true);
        assert_eq!(
            blink.at(3.5, false, true),
            (true, Some(4.0)),
            "back, and lit"
        );
        blink.moved();
        assert_eq!(blink.at(4.0, true, true), (true, None), "Reduce Motion");
    }
}
