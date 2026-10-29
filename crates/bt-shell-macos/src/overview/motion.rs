//! What moves in the big view when what it shows changes: the ring glides to the new selection,
//! the tiles that stay slide to their new places, the ones that go fade and shrink a little, the
//! ones that come fade in once those are clearing, a pane that dims or lights does it softly, and
//! in the column the selected row's fill slides to its machine.
//!
//! **Drawn, not layered.** The view paints every frame from where things stood when the change
//! came ([`From`], what was *on screen* then — a change in the middle of another starts from
//! where that one had got to, so nothing jumps) and where the model puts them now, at the
//! progress the view's display link has reached ([`Motion::at`]). The pictures stay the
//! drawing's: no tile becomes a layer. A tile that leaves is drawn from the state it was in
//! ([`Frame`]), whose pictures the frame keeps — the view lets go of a tab's pictures as soon as
//! the tab leaves the grid.
//!
//! None with Reduce Motion: the view does not start one.

use std::collections::HashMap;
use std::rc::Rc;

use bt_shell_common::overview::{Key, Layout, Machine, Overview};
use objc2::rc::Retained;
use objc2_app_kit::NSImage;
use objc2_foundation::{NSPoint, NSRect, NSSize};

/// The selection moving (an arrow, the pointer, a machine glanced at): the ring's glide.
pub(super) const SELECT_SECS: f64 = 0.14;
/// The grid changing what it holds (⇥, a click on a machine, the search).
pub(super) const FILTER_SECS: f64 = 0.26;
/// A tile that goes is gone after this share of the motion, shrunk to this much.
const LEAVE_SHARE: f64 = 0.6;
pub(super) const LEAVE_SCALE: f64 = 0.96;
/// A tile that comes waits this share of the motion, while the ones going clear, then grows
/// from this much.
const ARRIVE_DELAY: f64 = 0.2;
pub(super) const ARRIVE_SCALE: f64 = 0.96;

/// A cell across two states: a tab by its id, a terminal in the background by its key.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum Id {
    Tab(u64),
    Away(Key),
}

/// How a cell shows at a moment: where its picture is in the view, how much of it is seen, and
/// how selected it looks (the name's ink, the verb, a terminal's dimness).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Placed {
    pub(super) rect: NSRect,
    pub(super) alpha: f64,
    pub(super) chosen: f64,
}

/// The view as it was when a change came, for the tiles that leave to be drawn as they were.
pub(super) struct Frame {
    pub(super) state: Overview,
    pub(super) layout: Layout,
    pub(super) shots: HashMap<(u64, u64), Retained<NSImage>>,
    pub(super) away_shots: HashMap<Key, Retained<NSImage>>,
}

/// A cell as it was on screen, and — when the change may take it away — the state it can be
/// drawn from (the frame and its number there).
pub(super) struct Was {
    pub(super) placed: Placed,
    pub(super) ghost: Option<(Rc<Frame>, usize)>,
}

/// A heading as it was on screen: its text, whether it carries *Open All*, where, how seen.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Heading {
    pub(super) text: String,
    pub(super) button: bool,
    pub(super) rect: NSRect,
    pub(super) alpha: f64,
}

/// Everything as it was on screen when the change came.
pub(super) struct From {
    pub(super) cells: HashMap<Id, Was>,
    /// Each pane's dimness, `(tab, pane)`: 0 lit, 1 dim.
    pub(super) dims: HashMap<(u64, u64), f64>,
    /// The selected picture the ring went round, and how seen the ring was.
    pub(super) ring: Option<(NSRect, f64)>,
    pub(super) headings: Vec<Heading>,
    /// The column's rows, and the selected row's fill.
    pub(super) rows: HashMap<Machine, (NSRect, f64)>,
    pub(super) fill: Option<NSRect>,
    /// The machine that was selected, and how seen its load was.
    pub(super) machine: Machine,
    pub(super) load: f64,
}

/// A change on its way: from what was on screen, over `secs` from `start` (Core Animation's
/// clock, which the display link ticks on).
pub(super) struct Motion {
    start: f64,
    secs: f64,
    pub(super) from: From,
}

impl Motion {
    pub(super) fn new(from: From, start: f64, secs: f64) -> Self {
        Self { start, secs, from }
    }

    /// How far along it is at `now`: 0 to 1, even in time.
    pub(super) fn at(&self, now: f64) -> f64 {
        if self.secs <= 0.0 {
            return 1.0;
        }
        ((now - self.start) / self.secs).clamp(0.0, 1.0)
    }
}

/// The curve things move on: quick at first, settling at the end (ease-out quart).
pub(super) fn ease(t: f64) -> f64 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(4)
}

pub(super) fn lerp(from: f64, to: f64, k: f64) -> f64 {
    from + (to - from) * k
}

pub(super) fn lerp_rect(from: NSRect, to: NSRect, k: f64) -> NSRect {
    NSRect::new(
        NSPoint::new(
            lerp(from.origin.x, to.origin.x, k),
            lerp(from.origin.y, to.origin.y, k),
        ),
        NSSize::new(
            lerp(from.size.width, to.size.width, k),
            lerp(from.size.height, to.size.height, k),
        ),
    )
}

/// `rect` grown (above 1) or shrunk about its middle.
pub(super) fn scaled(rect: NSRect, scale: f64) -> NSRect {
    let (width, height) = (rect.size.width * scale, rect.size.height * scale);
    NSRect::new(
        NSPoint::new(
            rect.origin.x + (rect.size.width - width) / 2.0,
            rect.origin.y + (rect.size.height - height) / 2.0,
        ),
        NSSize::new(width, height),
    )
}

/// How much of a tile that goes is still seen at `t`.
pub(super) fn leaving(t: f64) -> f64 {
    1.0 - ease(t / LEAVE_SHARE)
}

/// How much of a tile that comes is seen at `t`.
pub(super) fn arriving(t: f64) -> f64 {
    ease((t - ARRIVE_DELAY) / (1.0 - ARRIVE_DELAY))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_curve_starts_still_and_ends_where_it_goes() {
        assert_eq!(ease(0.0), 0.0);
        assert_eq!(ease(1.0), 1.0);
        assert_eq!(ease(-1.0), 0.0);
        assert_eq!(ease(2.0), 1.0);
        assert!(ease(0.5) > 0.5, "most of the way is covered early");
    }

    #[test]
    fn a_tile_that_goes_is_gone_before_the_end_and_one_that_comes_waits() {
        assert_eq!(leaving(0.0), 1.0);
        assert_eq!(leaving(LEAVE_SHARE), 0.0);
        assert_eq!(leaving(1.0), 0.0);
        assert_eq!(arriving(0.0), 0.0);
        assert_eq!(arriving(ARRIVE_DELAY), 0.0);
        assert_eq!(arriving(1.0), 1.0);
    }

    #[test]
    fn a_motion_runs_from_its_start_over_its_length() {
        let from = From {
            cells: HashMap::new(),
            dims: HashMap::new(),
            ring: None,
            headings: Vec::new(),
            rows: HashMap::new(),
            fill: None,
            machine: Machine::All,
            load: 1.0,
        };
        let motion = Motion::new(from, 10.0, 0.2);
        assert_eq!(motion.at(9.0), 0.0);
        assert!((motion.at(10.1) - 0.5).abs() < 1e-9);
        assert_eq!(motion.at(11.0), 1.0);
    }

    #[test]
    fn a_rect_scales_about_its_middle() {
        let rect = NSRect::new(NSPoint::new(10.0, 20.0), NSSize::new(100.0, 50.0));
        let small = scaled(rect, 0.5);
        assert_eq!(small.origin, NSPoint::new(35.0, 32.5));
        assert_eq!(small.size, NSSize::new(50.0, 25.0));
        assert_eq!(lerp_rect(rect, small, 0.0), rect);
        assert_eq!(lerp_rect(rect, small, 1.0), small);
    }
}
