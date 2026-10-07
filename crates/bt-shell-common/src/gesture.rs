//! The mouse's **gesture ledger**: which press was reported to the application, which one started
//! a selection, where the motion report's throttle stands — a struct that never sees `NSEvent` and
//! is tested.
//!
//! The ledger once lived in `BateriView`'s `impl` body, and code next to `define_class!` could not
//! be tested: the four transitions that lock the route (press, drag, release, lost release) had
//! only been verified by eye.
//! There is no AppKit here: the view turns the event into a button, a click count and Shift, and
//! calls `Session` according to the ledger's answer.
//!
//! **The decision is still in `bt-core`.** Whether the gesture belongs to the application or the
//! terminal is told by the mode, and the mode lives in `Term` (`Session::mouse_button` →
//! [`Click`]); the ledger **records** that answer, it does not derive it again. That is why a
//! press takes two steps: [`Gesture::begin_press`] (before the answer, clears the stale trace) and
//! [`Gesture::pressed`] (after the answer, writes the new trace).

use bt_core::{Click, MouseButton, SelectKind, SelectionPoint};

/// The gesture's state. `Copy`: the view keeps it in a `Cell` and does take-modify-put on every
/// event — so that `RefCell`'s borrow panic is not put at risk in the middle of a `Session` call.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Gesture {
    /// Whether the left button is down and the selection started with this press.
    ///
    /// The anchor **itself** is not here: it is in `bt-core`, in absolute grid coordinates
    /// (`Session::set_selection`). The only remaining question is "is a drag in progress": a
    /// press-less `mouseDragged:` must not move the old selection's end.
    dragging: bool,
    /// Whether the selection drag is on the **dock's** input line.
    ///
    /// The target is locked at the press, like the report's route: even if the drag runs out of
    /// the band it grows the dock's selection (clamped to the input block),
    /// it does not cross over to the grid. It only has meaning while `dragging` is set.
    dock: bool,
    /// Buttons whose press was **reported to the application**, one bit per button
    /// ([`button_bit`]).
    ///
    /// The route is locked at the press: if Shift were read on every event, releasing
    /// Shift in the middle of a drag would turn a selection gesture into a report gesture. That is
    /// why the release asks **this bit**, not the mode.
    /// It sits next to `dragging` and not inside it: pressing the right button while a left-button
    /// selection is in progress produces both **at the same time**. A bitmask, because three
    /// buttons can be held down at once.
    sent: u8,
    /// The cell the last motion report went to — the throttle's notch.
    ///
    /// A report must go at most once **per cell**; the comparison runs **before** the `bt-core`
    /// call, so motion that stays in the same cell never touches the `Term` lock. The measure is
    /// the **visible window** cell, `half` does not enter — the report is at cell resolution.
    /// Press and release refresh the notch only **when they are reported** ([`Gesture::stamp`]):
    /// the criterion is "this was reported to the application".
    notch: Option<(u16, u16)>,
    /// Whether the left button's press was a ⌘-click on a **verified** link
    /// ([`Gesture::pressed_link`]).
    ///
    /// Only the route is here, not the link's range: the ledger stays `Copy` and the view
    /// already holds the verified hover the press was matched against, so the release compares
    /// with that (the hit test does not run again).
    link: bool,
    /// Where a ⌘-press on a **draggable** link went down (window points) — `None`
    /// for a link that cannot be dragged (only a remote one can) and
    /// for every other press. The first held motion farther than
    /// [`LINK_DRAG_THRESHOLD`] from it turns the gesture into a drag
    /// ([`Drag::Link`]) and takes the press point and `link` down with it.
    link_from: Option<(f64, f64)>,
    /// Where in the thumb the left button holds the scroll bar — the grab,
    /// physical pixels below the thumb's top — while a press in the bar's
    /// strip drags it ([`Gesture::pressed_scrollbar`]); `None` for every
    /// other press. Like the link's, only the route is here: the view
    /// turns each drag into a position with the drawn frame's layout.
    scrollbar: Option<f32>,
}

/// How far (window points) a ⌘-press on a draggable link must move before the
/// gesture is a drag, not a click. A design constant, not a
/// measurement and not AppKit's own drag threshold: a hand that trembles while
/// clicking must still open the link, a deliberate pull must not wait.
pub const LINK_DRAG_THRESHOLD: f64 = 4.0;

/// The work the terminal does from a press.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    /// A new selection, with the click count's step (a single click without a drag is an empty
    /// selection and removes the old highlight).
    Select(SelectKind),
    /// Shift+click: move the end of the existing selection (`Session::extend_selection`).
    Extend,
}

/// The path of a held drag — from the route locked at the press.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Drag {
    /// The press was reported: motion is a report too.
    Report,
    /// The press started a selection in the grid: the selection's end is moved.
    Select,
    /// The press started a selection in the dock: the dock selection's end is moved.
    SelectDock,
    /// A ⌘-press on a draggable (remote) link moved past [`LINK_DRAG_THRESHOLD`]: the view
    /// starts the file promise drag. Returned **once**; the gesture is then over in the
    /// ledger — AppKit owns the mouse for the drag session and the release does not open the link.
    Link,
    /// The left button pressed in the scroll bar's strip: the thumb follows the pointer, held
    /// this far (physical pixels) below its top — in or out of the strip, until the release.
    Scrollbar(f32),
    /// Neither (the right/middle button has no gesture in the terminal, a press-less drag): the
    /// event is dropped.
    Ignore,
}

/// The path of a release.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Release {
    /// The press was reported: the release is reported too (so no button is left stuck).
    Report,
    /// The end of a selection gesture in the grid, or a release without a gesture: nothing to
    /// send.
    Done,
    /// The end of a selection gesture **in the dock**: if it was a single click without a drag,
    /// the caret moves to the clicked spot (`Session::dock_click`). Whether the click
    /// had no drag is told by `bt-core`'s selection, not the ledger
    /// — a `Simple` selection that stayed empty.
    Dock,
    /// The end of a ⌘-click on a link: nothing is reported and no selection ends;
    /// the view opens the link if the pointer is still over the range locked at the press and
    /// the click count is one.
    Link,
    /// The end of a drag on the scroll bar: the thumb is let go; nothing is reported and no
    /// selection ends.
    Scrollbar,
}

impl Gesture {
    /// A new press is a new gesture: the same button's **stale** trace comes down here.
    ///
    /// A lost `mouseUp:` (a modal in the middle of a drag, a system gesture) can leave a bit or
    /// `dragging` behind; if it did not come down, when the mode closed in the meantime the press
    /// would become a selection and the release would find the stale bit and take the report path
    /// — or, next to a newly reported press, a stale `dragging` would extend the old selection on
    /// every following scroll.
    pub fn begin_press(&mut self, button: MouseButton) {
        self.sent &= !button_bit(button);
        if button == MouseButton::Left {
            self.dragging = false;
            self.dock = false;
            self.link = false;
            self.link_from = None;
            self.scrollbar = None;
        }
    }

    /// A left-button press with ⌘ inside the **verified** link hover's range: the gesture is
    /// the link's in **every** mode. The caller must have called
    /// [`Gesture::begin_press`] first and must **not** call `Session::mouse_button` — so no
    /// report goes to the application (vim, htop, Claude Code) and no selection starts, Shift
    /// or not.
    ///
    /// The route is locked here like the report's: a drag does nothing ([`Drag::Ignore`]) and
    /// the release is [`Release::Link`]. No `sent` bit is set, so the lost-release path
    /// ([`Gesture::take_lost_releases`]) never reports a release the application never saw a
    /// press for.
    ///
    /// **Why here and not in `bt-core`'s `button_route`** (the road map's "fourth arm"
    /// sketch): the link bit can only come from the view's verified hover — whether a path
    /// exists is unknown to `bt-core` — so a `bt-core` arm would only reflect the decision
    /// back. The dock press is the precedent ([`Gesture::pressed_dock`]).
    ///
    /// `drag_from` is the press point (window points) when the link can be dragged out — a
    /// remote one; `None` keeps today's click-only route at any distance.
    pub fn pressed_link(&mut self, drag_from: Option<(f64, f64)>) {
        self.dragging = false;
        self.dock = false;
        self.link = true;
        self.link_from = drag_from;
        self.scrollbar = None;
    }

    /// A left-button press in the scroll bar's strip: the gesture is the **bar's** in every
    /// mode — the strip is checked before anything else, so no report goes to the application,
    /// no selection starts and no ⌘-link opens, whatever the modifiers ([`Gesture::pressed_link`]'s
    /// precedent: the caller must have called [`Gesture::begin_press`] first and must not call
    /// `Session::mouse_button`). `grab` is where in the thumb the pointer holds it, physical
    /// pixels below its top.
    ///
    /// The route is locked here: every drag is [`Drag::Scrollbar`] — out of the strip too, the
    /// thumb follows the pointer as long as the button is down — and the release is
    /// [`Release::Scrollbar`]. No `sent` bit and no `dragging`: the lost-release path reports
    /// nothing and a scroll during the drag moves no selection's end.
    pub fn pressed_scrollbar(&mut self, grab: f32) {
        self.dragging = false;
        self.dock = false;
        self.link = false;
        self.link_from = None;
        self.scrollbar = Some(grab);
    }

    /// The scroll bar's drag lost its release (AppKit sends a buttonless motion only while no
    /// button is down): the route comes down; `true` if there was one — the caller lets the
    /// thumb go, or it would stay held and dark until the next press.
    pub fn lost_scrollbar(&mut self) -> bool {
        self.scrollbar.take().is_some()
    }

    /// A left-button press on the dock's input line: the gesture is the **terminal's** (mouse mode
    /// never applies to the dock — the band is not the application's screen) and the click count
    /// and Shift are read with the grid's rule ([`Gesture::pressed`]). The caller must have called
    /// [`Gesture::begin_press`] first.
    pub fn pressed_dock(&mut self, clicks: isize, shift: bool) -> Press {
        self.dragging = true;
        self.dock = true;
        if shift {
            Press::Extend
        } else {
            Press::Select(click_kind(clicks))
        }
    }

    /// Writes `bt-core`'s answer ([`Click`]) into the ledger and tells the terminal's work.
    /// `clicks` is AppKit's `clickCount` ([`click_kind`]).
    ///
    /// **Shift comes before the count**: Shift+click extends the selection whatever the click
    /// count, and the rule is the same in both modes — in mouse mode a Shift press already falls
    /// to selection (`bt-core`'s arbitration) and there Shift is the only way to select. If
    /// there is no selection, the extension starts from the clicked point; that
    /// decision is `Session::extend_selection`'s.
    ///
    /// Only the **left** button starts a selection: a right or middle click would produce an
    /// unexpected highlight. If a report was sent, `dragging` is not set, otherwise
    /// `mouseDragged:` would grow the old selection's end.
    pub fn pressed(
        &mut self,
        button: MouseButton,
        answer: Click,
        clicks: isize,
        shift: bool,
    ) -> Option<Press> {
        match answer {
            Click::Sent => {
                self.sent |= button_bit(button);
                None
            }
            Click::Select if button == MouseButton::Left => {
                self.dragging = true;
                Some(if shift {
                    Press::Extend
                } else {
                    Press::Select(click_kind(clicks))
                })
            }
            Click::Select | Click::Ignored => None,
        }
    }

    /// A held drag at `at` (window points): the route was locked at the press and is not asked
    /// again here. Both halves of the lock are read and the report comes first — both can be set
    /// at the same time (pressing the right button while a left selection is in progress), but the
    /// bit is **per button**.
    ///
    /// A ⌘-press on a draggable link answers [`Drag::Link`] at the first motion past
    /// [`LINK_DRAG_THRESHOLD`] and forgets the link — so it answers it once, and the release
    /// after it is [`Release::Done`].
    pub fn dragged(&mut self, button: MouseButton, at: (f64, f64)) -> Drag {
        if button == MouseButton::Left
            && let Some(grab) = self.scrollbar
        {
            return Drag::Scrollbar(grab);
        }
        if button == MouseButton::Left
            && self.link
            && let Some((x, y)) = self.link_from
        {
            let (dx, dy) = (at.0 - x, at.1 - y);
            if dx.hypot(dy) >= LINK_DRAG_THRESHOLD {
                self.link = false;
                self.link_from = None;
                return Drag::Link;
            }
        }
        if self.sent & button_bit(button) != 0 {
            Drag::Report
        } else if button == MouseButton::Left && self.dragging && self.dock {
            Drag::SelectDock
        } else if button == MouseButton::Left && self.dragging {
            Drag::Select
        } else {
            Drag::Ignore
        }
    }

    /// Release: if the press was reported, the bit comes down and the report goes; otherwise the
    /// left button's selection gesture ends (the selection stays on screen, Cmd-C copies it).
    pub fn released(&mut self, button: MouseButton) -> Release {
        let bit = button_bit(button);
        if button == MouseButton::Left && self.scrollbar.take().is_some() {
            return Release::Scrollbar;
        }
        if button == MouseButton::Left && std::mem::take(&mut self.link) {
            self.link_from = None;
            return Release::Link;
        }
        if self.sent & bit == 0 {
            if button == MouseButton::Left {
                let dock = self.dragging && self.dock;
                self.dragging = false;
                if dock {
                    return Release::Dock;
                }
            }
            return Release::Done;
        }
        self.sent &= !bit;
        Release::Report
    }

    /// The buttons a lost `mouseUp:` left held down in the application; the ledger forgets them,
    /// and the caller must **report** the release for each one (the application still thinks the
    /// button is down).
    ///
    /// The evidence is the caller's selector: AppKit sends `mouseMoved:` only while no button is
    /// down, so a bit set there means exactly one thing — the release never reached this view.
    pub fn take_lost_releases(&mut self) -> impl Iterator<Item = MouseButton> {
        let lost = std::mem::take(&mut self.sent);
        [MouseButton::Left, MouseButton::Middle, MouseButton::Right]
            .into_iter()
            .filter(move |&button| lost & button_bit(button) != 0)
    }

    /// The left button is **not down** in the system but `dragging` is set: the release never
    /// reached this view. The stale flag comes down; otherwise every button-less scroll would
    /// silently extend the old selection, and the next Cmd-C would copy it.
    pub fn lost_drag(&mut self) {
        self.dragging = false;
    }

    /// Whether a selection drag is in progress **in the grid** — scrolling's question of whether
    /// to move the end to the mouse. A dock drag is `false` here: the dock does not scroll, so
    /// there is no end to move when the window scrolls.
    pub fn dragging(&self) -> bool {
        self.dragging && !self.dock
    }

    /// Moves the notch to the fresh cell and tells whether the cell **changed** — `true` on first
    /// sight, `false` on repeat. The half is not read.
    pub fn moved_to(&mut self, cell: SelectionPoint) -> bool {
        let now = (cell.col, cell.row);
        self.notch.replace(now) != Some(now)
    }

    /// Stamps the cell of a reported press or release onto the notch: so the first motion to come
    /// in the same cell does not produce a second report.
    pub fn stamp(&mut self, cell: SelectionPoint) {
        self.moved_to(cell);
    }
}

/// The selection's step from AppKit's `clickCount`: 1 character, 2 word, 3 line.
///
/// A count above three **stays on the line**: a quadruple click (smart selection) is out of scope
/// and a fast-clicking user's fourth click must not drop the line. Zero or negative (a synthetic
/// event) counts as a single click.
pub(crate) fn click_kind(clicks: isize) -> SelectKind {
    match clicks {
        2 => SelectKind::Word,
        3.. => SelectKind::Line,
        _ => SelectKind::Simple,
    }
}

/// The button's bit in the ledger. **Separate** from the report's button code (inside
/// [`bt_core`]): this is a mask, that is a byte value.
fn button_bit(button: MouseButton) -> u8 {
    match button {
        MouseButton::Left => 1,
        MouseButton::Middle => 2,
        MouseButton::Right => 4,
    }
}

#[cfg(test)]
mod tests {
    use bt_core::CellHalf;

    use super::*;

    const LEFT: MouseButton = MouseButton::Left;
    const RIGHT: MouseButton = MouseButton::Right;
    /// Where the pointer is when the route does not depend on the distance.
    const HERE: (f64, f64) = (100.0, 50.0);

    /// A left-button press: first the stale trace comes down, then `bt-core`'s answer is written.
    fn press(gesture: &mut Gesture, answer: Click, clicks: isize, shift: bool) -> Option<Press> {
        gesture.begin_press(LEFT);
        gesture.pressed(LEFT, answer, clicks, shift)
    }

    #[test]
    fn a_selecting_press_drags_the_selection_until_release() {
        let mut gesture = Gesture::default();
        // No press before the drag: the event is dropped.
        assert_eq!(gesture.dragged(LEFT, HERE), Drag::Ignore);
        assert_eq!(
            press(&mut gesture, Click::Select, 1, false),
            Some(Press::Select(SelectKind::Simple))
        );
        assert_eq!(gesture.dragged(LEFT, HERE), Drag::Select);
        assert_eq!(gesture.released(LEFT), Release::Done);
        // No drag after the release.
        assert_eq!(gesture.dragged(LEFT, HERE), Drag::Ignore);
        assert!(!gesture.dragging());
    }

    #[test]
    fn a_reported_press_is_reported_to_its_release() {
        let mut gesture = Gesture::default();
        assert_eq!(press(&mut gesture, Click::Sent, 1, false), None);
        assert_eq!(gesture.dragged(LEFT, HERE), Drag::Report);
        assert!(!gesture.dragging(), "a report is not a selection drag");
        assert_eq!(gesture.released(LEFT), Release::Report);
        // The bit came down: a second release is not reported.
        assert_eq!(gesture.released(LEFT), Release::Done);
    }

    #[test]
    fn the_click_count_picks_the_step() {
        let mut gesture = Gesture::default();
        for (clicks, kind) in [
            (0, SelectKind::Simple),
            (1, SelectKind::Simple),
            (2, SelectKind::Word),
            (3, SelectKind::Line),
            (4, SelectKind::Line),
            (7, SelectKind::Line),
        ] {
            assert_eq!(
                press(&mut gesture, Click::Select, clicks, false),
                Some(Press::Select(kind)),
                "{clicks}"
            );
        }
    }

    #[test]
    fn a_shift_click_extends_whatever_the_count() {
        // Same in mouse mode: a Shift press falls to selection in `bt-core`
        // (`Click::Select`, guarded in `input::tests`) and the ledger reads it as an extension,
        // not a report.
        let mut gesture = Gesture::default();
        for clicks in 1..=3 {
            assert_eq!(
                press(&mut gesture, Click::Select, clicks, true),
                Some(Press::Extend)
            );
            // An extension drags too: Shift+click and drag moves the end.
            assert_eq!(gesture.dragged(LEFT, HERE), Drag::Select);
        }
    }

    #[test]
    fn only_the_left_button_selects() {
        let mut gesture = Gesture::default();
        gesture.begin_press(RIGHT);
        assert_eq!(gesture.pressed(RIGHT, Click::Select, 2, false), None);
        assert_eq!(gesture.dragged(RIGHT, HERE), Drag::Ignore);
        assert!(!gesture.dragging());
        // A press that sent no report leaves no trace.
        assert_eq!(press(&mut gesture, Click::Ignored, 1, false), None);
        assert_eq!(gesture.dragged(LEFT, HERE), Drag::Ignore);
    }

    #[test]
    fn a_lost_release_is_reported_and_forgotten() {
        let mut gesture = Gesture::default();
        press(&mut gesture, Click::Sent, 1, false);
        gesture.begin_press(RIGHT);
        gesture.pressed(RIGHT, Click::Sent, 1, false);
        // `mouseUp:` never arrived; button-less motion releases both.
        let lost: Vec<_> = gesture.take_lost_releases().collect();
        assert_eq!(lost, [LEFT, RIGHT]);
        assert_eq!(gesture.take_lost_releases().count(), 0);
        assert_eq!(gesture.dragged(LEFT, HERE), Drag::Ignore);
    }

    #[test]
    fn a_new_press_clears_a_stale_report_bit() {
        let mut gesture = Gesture::default();
        press(&mut gesture, Click::Sent, 1, false);
        // The release was lost, the app closed the mode meanwhile: the new press selects.
        press(&mut gesture, Click::Select, 1, false);
        assert_eq!(gesture.dragged(LEFT, HERE), Drag::Select);
        // The release does not find the stale bit and take the report path.
        assert_eq!(gesture.released(LEFT), Release::Done);
        assert!(!gesture.dragging());
    }

    #[test]
    fn a_stale_drag_comes_down() {
        // The selection drag's release was lost.
        let mut gesture = Gesture::default();
        press(&mut gesture, Click::Select, 1, false);
        // Scrolling learns from the system that the button is not down.
        gesture.lost_drag();
        assert_eq!(gesture.dragged(LEFT, HERE), Drag::Ignore);
        // A newly reported press also brings down the stale `dragging`.
        press(&mut gesture, Click::Select, 1, false);
        press(&mut gesture, Click::Sent, 1, false);
        assert!(
            !gesture.dragging(),
            "stale selection next to a reported press"
        );
        assert_eq!(gesture.dragged(LEFT, HERE), Drag::Report);
    }

    #[test]
    fn a_dock_press_drags_the_dock_selection() {
        let mut gesture = Gesture::default();
        gesture.begin_press(LEFT);
        assert_eq!(
            gesture.pressed_dock(2, false),
            Press::Select(SelectKind::Word)
        );
        assert_eq!(gesture.dragged(LEFT, HERE), Drag::SelectDock);
        // Scrolling must not move the grid's end.
        assert!(!gesture.dragging(), "dock drag counted as a grid drag");
        // The release is the dock's: the click-to-caret gate. Once —
        // a second release has no gesture.
        assert_eq!(gesture.released(LEFT), Release::Dock);
        assert_eq!(gesture.released(LEFT), Release::Done);
        assert_eq!(gesture.dragged(LEFT, HERE), Drag::Ignore);
        // Shift extends by the same rule.
        gesture.begin_press(LEFT);
        assert_eq!(gesture.pressed_dock(1, true), Press::Extend);
        // A new press in the grid takes the target back.
        press(&mut gesture, Click::Select, 1, false);
        assert_eq!(gesture.dragged(LEFT, HERE), Drag::Select);
        assert!(gesture.dragging());
    }

    #[test]
    fn a_link_press_beats_the_report_and_the_selection() {
        // Mouse mode on (vim `:set mouse=a`): the view sees ⌘ over a verified link and takes
        // the ledger's pre-route — `mouse_button` is never called, so there is no `Click` to
        // write. Shift held or not, the call is the same: the route has no Shift input.
        for _shift in [false, true] {
            let mut gesture = Gesture::default();
            gesture.begin_press(LEFT);
            gesture.pressed_link(None);
            // A drag neither selects nor reports.
            assert_eq!(gesture.dragged(LEFT, HERE), Drag::Ignore);
            assert!(!gesture.dragging(), "a link press is not a selection drag");
            // The release is the link's, once, and no report follows.
            assert_eq!(gesture.released(LEFT), Release::Link);
            assert_eq!(gesture.released(LEFT), Release::Done);
            // No `sent` bit: the lost-release path reports nothing.
            assert_eq!(gesture.take_lost_releases().count(), 0);
        }
    }

    #[test]
    fn a_link_press_leaves_other_buttons_and_the_next_press_alone() {
        let mut gesture = Gesture::default();
        // A reported right-button press stays reported across a link click.
        gesture.begin_press(RIGHT);
        gesture.pressed(RIGHT, Click::Sent, 1, false);
        gesture.begin_press(LEFT);
        gesture.pressed_link(None);
        assert_eq!(gesture.dragged(RIGHT, HERE), Drag::Report);
        assert_eq!(gesture.released(RIGHT), Release::Report);
        assert_eq!(gesture.released(LEFT), Release::Link);
        // A link press clears a stale selection drag.
        press(&mut gesture, Click::Select, 1, false);
        gesture.begin_press(LEFT);
        gesture.pressed_link(None);
        assert!(!gesture.dragging());
        // A lost link release does not leak into the next press: a new selection releases as
        // a selection.
        press(&mut gesture, Click::Select, 1, false);
        assert_eq!(gesture.dragged(LEFT, HERE), Drag::Select);
        assert_eq!(gesture.released(LEFT), Release::Done);
        // Nor into a reported one.
        gesture.begin_press(LEFT);
        gesture.pressed_link(None);
        press(&mut gesture, Click::Sent, 1, false);
        assert_eq!(gesture.released(LEFT), Release::Report);
    }

    #[test]
    fn a_remote_link_press_drags_once_past_the_threshold() {
        // A ⌘-press on a remote link that moves past the threshold is a file
        // promise drag — once.
        let mut gesture = Gesture::default();
        gesture.begin_press(LEFT);
        gesture.pressed_link(Some(HERE));
        let (x, y) = HERE;
        // Inside the threshold (a trembling click): nothing yet.
        let near = LINK_DRAG_THRESHOLD - 0.5;
        assert_eq!(gesture.dragged(LEFT, (x + near, y)), Drag::Ignore);
        assert_eq!(gesture.dragged(LEFT, (x, y - near)), Drag::Ignore);
        // Past it, in any direction: the drag starts.
        let far = (x - LINK_DRAG_THRESHOLD * 0.8, y + LINK_DRAG_THRESHOLD * 0.8);
        assert_eq!(gesture.dragged(LEFT, far), Drag::Link);
        // Exactly once: the next motions are dropped, even farther away.
        assert_eq!(gesture.dragged(LEFT, (x + 40.0, y)), Drag::Ignore);
        assert_eq!(gesture.dragged(LEFT, far), Drag::Ignore);
        assert!(!gesture.dragging(), "a link drag is not a selection drag");
        // The release does not open the link (AppKit swallows it anyway).
        assert_eq!(gesture.released(LEFT), Release::Done);
        assert_eq!(gesture.take_lost_releases().count(), 0);
    }

    #[test]
    fn a_remote_link_click_below_the_threshold_still_opens() {
        let mut gesture = Gesture::default();
        gesture.begin_press(LEFT);
        gesture.pressed_link(Some(HERE));
        let (x, y) = HERE;
        assert_eq!(gesture.dragged(LEFT, (x + 1.0, y + 1.0)), Drag::Ignore);
        // The release is the link's: the view opens (previews) it.
        assert_eq!(gesture.released(LEFT), Release::Link);
        assert_eq!(gesture.released(LEFT), Release::Done);
    }

    #[test]
    fn a_local_link_never_drags() {
        // Local links are not dragged — any distance stays a click.
        let mut gesture = Gesture::default();
        gesture.begin_press(LEFT);
        gesture.pressed_link(None);
        for distance in [0.0, LINK_DRAG_THRESHOLD, 10.0, 500.0] {
            assert_eq!(
                gesture.dragged(LEFT, (HERE.0 + distance, HERE.1)),
                Drag::Ignore,
                "{distance}"
            );
        }
        assert_eq!(gesture.released(LEFT), Release::Link);
    }

    #[test]
    fn a_new_press_forgets_the_link_drag() {
        let mut gesture = Gesture::default();
        gesture.begin_press(LEFT);
        gesture.pressed_link(Some(HERE));
        // The release was lost; the next press is a selection: its drag selects, never `Link`.
        press(&mut gesture, Click::Select, 1, false);
        assert_eq!(gesture.dragged(LEFT, (HERE.0 + 50.0, HERE.1)), Drag::Select);
        // Nor does a later local link press inherit the old press point.
        gesture.begin_press(LEFT);
        gesture.pressed_link(None);
        assert_eq!(gesture.dragged(LEFT, (HERE.0 + 50.0, HERE.1)), Drag::Ignore);
        // Only the left button drags a link.
        gesture.begin_press(LEFT);
        gesture.pressed_link(Some(HERE));
        assert_eq!(
            gesture.dragged(RIGHT, (HERE.0 + 50.0, HERE.1)),
            Drag::Ignore
        );
        assert_eq!(gesture.dragged(LEFT, (HERE.0 + 50.0, HERE.1)), Drag::Link);
    }

    #[test]
    fn a_scroll_bar_press_drags_the_thumb_until_release() {
        // The route is locked at the press: every left drag is the thumb's — out of the strip
        // too — with the grab it was pressed with, and the release is the bar's, once.
        let mut gesture = Gesture::default();
        gesture.begin_press(LEFT);
        gesture.pressed_scrollbar(7.5);
        assert_eq!(gesture.dragged(LEFT, HERE), Drag::Scrollbar(7.5));
        assert_eq!(
            gesture.dragged(LEFT, (-500.0, 9000.0)),
            Drag::Scrollbar(7.5)
        );
        // Neither a report nor a selection: no `sent` bit, no selection drag.
        assert!(!gesture.dragging(), "a scroll bar drag moves a selection");
        assert_eq!(gesture.take_lost_releases().count(), 0);
        assert_eq!(gesture.released(LEFT), Release::Scrollbar);
        assert_eq!(gesture.released(LEFT), Release::Done);
        assert_eq!(gesture.dragged(LEFT, HERE), Drag::Ignore);
    }

    #[test]
    fn a_scroll_bar_press_leaves_other_buttons_and_the_next_press_alone() {
        let mut gesture = Gesture::default();
        // A reported right-button press stays reported across a thumb drag.
        gesture.begin_press(RIGHT);
        gesture.pressed(RIGHT, Click::Sent, 1, false);
        gesture.begin_press(LEFT);
        gesture.pressed_scrollbar(3.0);
        assert_eq!(gesture.dragged(RIGHT, HERE), Drag::Report);
        assert_eq!(gesture.released(RIGHT), Release::Report);
        // The thumb's release was lost: the next press clears the route, a selection releases
        // as a selection.
        press(&mut gesture, Click::Select, 1, false);
        assert_eq!(gesture.dragged(LEFT, HERE), Drag::Select);
        assert_eq!(gesture.released(LEFT), Release::Done);
        // A press on the bar clears a stale selection drag and a link press.
        press(&mut gesture, Click::Select, 1, false);
        gesture.begin_press(LEFT);
        gesture.pressed_link(Some(HERE));
        gesture.begin_press(LEFT);
        gesture.pressed_scrollbar(0.0);
        assert!(!gesture.dragging());
        assert_eq!(
            gesture.dragged(LEFT, (HERE.0 + 50.0, HERE.1)),
            Drag::Scrollbar(0.0)
        );
        assert_eq!(gesture.released(LEFT), Release::Scrollbar);
        // A buttonless motion is the evidence of a lost release: the route comes down once.
        gesture.begin_press(LEFT);
        gesture.pressed_scrollbar(2.0);
        assert!(gesture.lost_scrollbar());
        assert!(!gesture.lost_scrollbar());
        assert_eq!(gesture.released(LEFT), Release::Done);
    }

    #[test]
    fn motion_is_throttled_to_one_report_per_cell() {
        // The throttle's single rule: `true` on first sight, `false` on a repeat of the same
        // cell. Without it every pixel of the pointer would produce a report and keep an idle
        // application redrawing constantly.
        let mut gesture = Gesture::default();
        let cell = |col, row, half| SelectionPoint { col, row, half };
        assert!(gesture.moved_to(cell(3, 7, CellHalf::Left)));
        assert!(!gesture.moved_to(cell(3, 7, CellHalf::Left)));
        // **The half is not read**: the report is at cell resolution and crossing to the
        // cell's other half must not produce a new report.
        assert!(!gesture.moved_to(cell(3, 7, CellHalf::Right)));
        // When the column or row changes, the report goes again.
        assert!(gesture.moved_to(cell(4, 7, CellHalf::Right)));
        assert!(gesture.moved_to(cell(4, 8, CellHalf::Right)));
        // Going back is a change too.
        assert!(gesture.moved_to(cell(4, 7, CellHalf::Right)));
        // A reported press's stamp swallows the first motion in the same cell.
        gesture.stamp(cell(9, 9, CellHalf::Left));
        assert!(!gesture.moved_to(cell(9, 9, CellHalf::Right)));
    }
}
