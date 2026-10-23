//! A long list's arithmetic: where each row stands, which rows a window of the list shows, and
//! how far to scroll to see one.
//!
//! A page that shows a list builds only the rows in view — the elements of ten thousand
//! rows are not made to show thirty — and keeps the rest of the list's length with a spacer
//! above and one below. The rows' heights are known without laying them out (a list row is
//! the same height every time), so their places are a sum.

use std::ops::Range;

/// The heights of a list's rows, and the sums of them.
#[derive(Clone, Debug, PartialEq)]
pub struct Rows {
    /// `tops[i]` is where row `i` starts; the last is where the last row ends. Never empty.
    tops: Vec<f32>,
    /// The room kept below the last row.
    trail: f32,
}

impl Default for Rows {
    fn default() -> Self {
        Self {
            tops: vec![0.0],
            trail: 0.0,
        }
    }
}

impl Rows {
    /// A list whose rows are `heights` high, points.
    pub fn new(heights: impl IntoIterator<Item = f32>) -> Self {
        let mut tops = vec![0.0];
        let mut sum = 0.0;
        for height in heights {
            sum += height.max(0.0);
            tops.push(sum);
        }
        Self { tops, trail: 0.0 }
    }

    /// The same list with `lead` points of room before the first row and `trail` after the last.
    #[must_use]
    pub fn padded(mut self, lead: f32, trail: f32) -> Self {
        for top in &mut self.tops {
            *top += lead;
        }
        self.trail = trail;
        self
    }

    /// How many rows.
    pub fn len(&self) -> usize {
        self.tops.len() - 1
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The length of the whole list, with the room around the rows, points.
    pub fn total(&self) -> f32 {
        self.tops.last().copied().unwrap_or(0.0) + self.trail
    }

    /// Where row `index` starts (the list's length for a row past the end).
    pub fn top(&self, index: usize) -> f32 {
        self.tops[index.min(self.len())]
    }

    /// The height of row `index`.
    pub fn height(&self, index: usize) -> f32 {
        self.top(index + 1) - self.top(index)
    }

    /// The row at `y` from the list's start (the last row for a point past the end).
    pub fn at(&self, y: f32) -> Option<usize> {
        if self.is_empty() {
            return None;
        }
        // The first top greater than `y` is the start of the row after the one at `y`.
        let after = self.tops.partition_point(|top| *top <= y);
        Some(after.saturating_sub(1).min(self.len() - 1))
    }

    /// The rows that touch the window `offset..offset + view`, `overscan` points more at each
    /// end (rows that are not in view yet, so a scroll finds them built).
    pub fn window(&self, offset: f32, view: f32, overscan: f32) -> Range<usize> {
        if self.is_empty() || view <= 0.0 {
            return 0..0;
        }
        let from = (offset - overscan).max(0.0);
        let to = offset + view + overscan;
        if from >= self.total() {
            return self.len()..self.len();
        }
        let start = self.at(from).unwrap_or(0);
        // The first row that starts at or after `to` is the first one out of the window.
        let end = self.tops.partition_point(|top| *top < to).min(self.len());
        start..end.max(start)
    }

    /// The offset that shows all of row `index` in a window of `view` points that is at
    /// `offset` now: `offset` itself when the row is in view, the nearest edge otherwise. The
    /// first row is shown from the start of the list and the last to its end, room around
    /// them and all.
    pub fn reveal(&self, index: usize, offset: f32, view: f32) -> f32 {
        let top = if index == 0 { 0.0 } else { self.top(index) };
        let bottom = if index + 1 >= self.len() {
            self.total()
        } else {
            self.top(index + 1)
        };
        if top < offset {
            top
        } else if bottom > offset + view {
            (bottom - view).max(0.0).min(top)
        } else {
            offset
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(count: usize) -> Rows {
        Rows::new(std::iter::repeat_n(32.0, count))
    }

    #[test]
    fn rows_stand_one_after_another_and_the_list_is_their_sum() {
        let rows = Rows::new([38.0, 32.0, 32.0]);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows.top(0), 0.0);
        assert_eq!(rows.top(1), 38.0);
        assert_eq!(rows.top(2), 70.0);
        assert_eq!(rows.total(), 102.0);
        assert_eq!(rows.height(0), 38.0);
        assert_eq!(rows.top(9), 102.0, "past the end is the end");
    }

    #[test]
    fn a_point_is_in_the_row_that_stands_there() {
        let rows = Rows::new([38.0, 32.0, 32.0]);
        assert_eq!(rows.at(0.0), Some(0));
        assert_eq!(rows.at(37.9), Some(0));
        assert_eq!(rows.at(38.0), Some(1));
        assert_eq!(rows.at(500.0), Some(2));
        assert_eq!(Rows::default().at(3.0), None);
    }

    #[test]
    fn a_window_holds_the_rows_that_touch_it_and_no_others() {
        let rows = flat(10_000);
        // 600 points from the top: rows 0..19 touch it (19 × 32 = 608).
        assert_eq!(rows.window(0.0, 600.0, 0.0), 0..19);
        // Half a row down: the first row is still touched by its bottom half.
        assert_eq!(rows.window(16.0, 64.0, 0.0), 0..3);
        assert_eq!(rows.window(32.0, 64.0, 0.0), 1..3);
        // Far down the list the window is as small as at the top.
        let far = rows.window(160_000.0, 600.0, 0.0);
        assert_eq!(far, 5000..5019);
        // The overscan takes rows on both sides.
        assert_eq!(rows.window(160_000.0, 600.0, 64.0), 4998..5021);
        // Past the end, and nothing to show.
        assert_eq!(rows.window(1.0e9, 600.0, 0.0).len(), 0);
        assert_eq!(flat(0).window(0.0, 600.0, 0.0), 0..0);
        assert_eq!(rows.window(0.0, 0.0, 0.0), 0..0);
    }

    #[test]
    fn the_spacers_and_the_rows_in_the_window_make_up_the_whole_list() {
        let rows = Rows::new((0..500).map(|i| if i % 7 == 0 { 38.0 } else { 32.0 }));
        for offset in [0.0, 13.0, 4000.0, 9999.0, 15_000.0] {
            let window = rows.window(offset, 500.0, 32.0);
            let built: f32 = window.clone().map(|i| rows.height(i)).sum();
            let before = rows.top(window.start);
            let after = rows.total() - rows.top(window.end);
            assert!(
                (before + built + after - rows.total()).abs() < 0.01,
                "offset {offset}"
            );
        }
    }

    #[test]
    fn scrolling_to_a_row_moves_the_least_that_shows_it() {
        let rows = flat(100);
        // In view: stays where it is.
        assert_eq!(rows.reveal(3, 0.0, 320.0), 0.0);
        // Below the window: its bottom edge goes to the window's.
        assert_eq!(rows.reveal(15, 0.0, 320.0), 16.0 * 32.0 - 320.0);
        // Above it: its top edge goes to the window's.
        assert_eq!(rows.reveal(2, 640.0, 320.0), 64.0);
        // A window shorter than the row shows the row's start.
        assert_eq!(rows.reveal(5, 0.0, 10.0), 160.0);
    }

    #[test]
    fn room_around_the_rows_is_in_every_sum_and_the_ends_show_it() {
        let rows = Rows::new([32.0, 32.0, 32.0]).padded(6.0, 8.0);
        assert_eq!(rows.top(0), 6.0);
        assert_eq!(rows.top(1), 38.0);
        assert_eq!(rows.total(), 6.0 + 96.0 + 8.0);
        assert_eq!(
            rows.at(2.0),
            Some(0),
            "the room before the first row is the first row's"
        );
        // The first row is revealed from the very start, the last to the very end.
        assert_eq!(rows.reveal(0, 50.0, 40.0), 0.0);
        assert_eq!(rows.reveal(2, 0.0, 40.0), rows.total() - 40.0);
        // The spacers and the rows still make the whole list.
        let window = rows.window(30.0, 40.0, 0.0);
        let built: f32 = window.clone().map(|i| rows.height(i)).sum();
        assert_eq!(
            rows.top(window.start) + built + (rows.total() - rows.top(window.end)),
            rows.total()
        );
    }
}
