//! Effects of the glyphs typed and erased in the dock — **pure**, no ObjC, no
//! locks.
//!
//! Which glyph arrived or left is told by `bt-core`
//! ([`bt_core::DockEdit`]); here there is only **time**: the list of in-flight
//! entries, the elapsed time and the stop condition. Drawing is the job of
//! `Frame` and the `glyph_fx` pipeline.
//!
//! **Beside `Motion`, not inside it** ([`crate::blink`]'s precedent): `Motion` is
//! `Copy` and lives in a `Cell`; a list would either take it out of `Copy` or
//! have it copied on every `get`/`set`.
//!
//! **The stop condition is an empty list.** An entry whose time is up drops and
//! if the list is empty there is nothing left to do ([`GlyphFx::is_empty`] is a
//! term of the link's sleep test). The frame request is motion's: `Waker::wake`
//! is not touched, no damage is planted — the effect changes not the content but
//! how the content is drawn.

use bt_core::{Cell, Clusters, DockEdit, EDIT_MAX, Erase, Keypress};

use crate::frame::copy_cluster;
use crate::motion::Motion;

/// Duration of an arrival, seconds — **chosen, not measured**; a base duration
/// of 240 ms.
///
/// The first choice was 120 ms and the user said in a real window "the
/// animations are not noticeable at all": the visible part fit in three or four
/// frames and the first moment of the arrival passed inside the caret that had
/// not yet moved away. In fast typing two or three arrivals are in flight on top
/// of each other and this is not a flicker: each is in its own cell, and the
/// position rule ([`GlyphFx::apply`]) ends only the one on the right.
pub(crate) const KEYPRESS_DURATION: f32 = 0.24;

/// Duration of a ghost, seconds — **chosen, not measured**.
///
/// A notch longer than the arrival: departure is a movement the eye follows
/// (reading what was erased), while arrival is an already-known confirmation of
/// what was typed. The first choice was 160 ms and it was lengthened for the same
/// reason as the arrival's; after Backspace the caret arrives right at the
/// ghost's column, so to be visible the ghost has to be drawn above the caret
/// (`Renderer::encode_fx`).
pub(crate) const ERASE_DURATION: f32 = 0.30;

/// Ceiling of in-flight entries — **a design constant**.
///
/// A held Backspace gives birth to at most one ghost per frame, typing two or
/// three arrivals per frame, and each lives [`ERASE_DURATION`]: the ceiling is a
/// number ordinary typing never touches. When full the **oldest** entry ends —
/// if an arrival its static glyph comes back, if a ghost it goes; the error's
/// direction is safe, the effect shortens but no glyph vanishes.
pub(crate) const FX_MAX: usize = 32;

/// The shader's id of an effect name (`shaders/glyph_fx.wgsl` → `FX_*`).
///
/// The dictionary of names is `bt-core`'s ([`Keypress`], [`Erase`] — the settings
/// model, `CursorMotion`'s precedent); the id is the drawing's knowledge and is
/// here. An exhaustive `match`: the moment a new name enters `bt-core` this does
/// not compile, so a name without an id cannot leak into the popup.
pub(crate) trait Effect: Copy + PartialEq + 'static {
    /// Arrivals are in `1..16`, ghosts in `16..32`: the shader reads the input's kind
    /// from the id and carries no second bit. `Off` is not drawn, it has no id.
    fn id(self) -> Option<u32>;

    /// All the effects that draw (except `Off`), in `NAMES` order — the loop of the
    /// hermetic invariants: every name that enters the settings
    /// model is under the tests the moment it enters.
    #[cfg(test)]
    fn effects() -> Vec<Self>;
}

impl Effect for Keypress {
    fn id(self) -> Option<u32> {
        match self {
            Self::Off => None,
            Self::Fade => Some(1),
            Self::Rise => Some(2),
            Self::Pop => Some(3),
            Self::Extrude => Some(4),
            Self::Heat => Some(5),
            Self::Echo => Some(6),
            Self::Drop => Some(7),
            Self::Ink => Some(8),
            Self::Squeeze => Some(9),
        }
    }

    #[cfg(test)]
    fn effects() -> Vec<Self> {
        drawn(Self::NAMES)
    }
}

impl Effect for Erase {
    fn id(self) -> Option<u32> {
        match self {
            Self::Off => None,
            // `recede` stayed at 16 (its original id); the other seven were lined up behind
            // it — the id is only a contract with the shader, the order has
            // no product meaning.
            Self::Recede => Some(16),
            Self::Iris => Some(17),
            Self::Undertow => Some(18),
            Self::Echo => Some(19),
            Self::Bleed => Some(20),
            Self::Unravel => Some(21),
            Self::Sublime => Some(22),
            Self::Shatter => Some(23),
        }
    }

    #[cfg(test)]
    fn effects() -> Vec<Self> {
        drawn(Self::NAMES)
    }
}

#[cfg(test)]
fn drawn<T: Effect>(names: &[(&str, T)]) -> Vec<T> {
    names
        .iter()
        .map(|&(_, effect)| effect)
        .filter(|effect| effect.id().is_some())
        .collect()
}

/// The kind of an entry: an arriving glyph or the ghost of a departing one.
///
/// The drawing order depends on it (`Renderer::encode_dock`): ghosts **before**
/// the dock's glyphs — the letter that slides into the place of a letter erased
/// mid-line stands on top of it —, arrivals **after**.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Arrival,
    Ghost,
}

/// An entry going to the drawing: its cell, kind, effect and progress.
///
/// The input of `Frame`, not [`GlyphFx`]'s inner state: tests can set an effect's
/// wanted progress ([`Fx::t`]) directly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Fx {
    /// The boundary's resolved cell — its column is **this frame's** screen column.
    pub(crate) cell: Cell,
    pub(crate) kind: Kind,
    /// The shader's effect id.
    pub(crate) effect: u32,
    /// Progress, `0..=1`; the curve is in the shader.
    pub(crate) t: f32,
    /// The entry's seed — the randomness of the fragmented effects (`shatter`,
    /// `unravel`). It is given once when the entry is born and is constant for its
    /// life, so the fragments do not flicker in a motion frame. It derives from the
    /// order, not the position: when the window slides the row changes and the
    /// fragments' pattern must not.
    pub(crate) seed: f32,
}

#[derive(Clone, Copy, Debug)]
struct Entry {
    fx: Fx,
    elapsed: f32,
    duration: f32,
}

/// In-flight effects.
///
/// The list has a fixed ceiling ([`FX_MAX`]) and its capacity is kept: after the
/// first effect there is no allocation per frame.
#[derive(Debug, Default)]
pub(crate) struct GlyphFx {
    entries: Vec<Entry>,
    /// The arrival effect the user chose — raw, not reduced. The reduction
    /// ([`Motion::glyph_fx`]) is asked again on every edit, so a change in Reduce
    /// Motion takes effect on the next key.
    keypress: Keypress,
    erase: Erase,
    /// The source of the seed; wrapping is harmless.
    serial: u32,
    /// The entries' cluster table: the edit's cells point into the dock's frame
    /// table and that table is cleared in every content frame while the effect lives
    /// for frames. The ids are copied here and the table is rebuilt from the living
    /// entries on every edit — its size does not exceed [`FX_MAX`] clusters.
    clusters: Clusters,
    /// The second buffer of the rebuild; its capacity is kept.
    scratch: Clusters,
}

impl GlyphFx {
    /// Processes this frame's edit.
    ///
    /// Order: first the vertical window's shift (the in-flight entries slide with the
    /// text and those that overflow the window's rows — `rows` — drop), then the
    /// **position rule** — the arrivals that stand at the new edit's position or
    /// after it in reading order end (for a user who moves the caret to the left of an
    /// in-flight glyph and types, that glyph sits early; the rule is on two axes — in a
    /// wrapped input the text behind the edit slides to the lower lines too), last the
    /// new entries.
    ///
    /// `rows` is the number of input rows the dock draws (the vertical window's
    /// size); `table` is the cluster table of the edit's cells (the dock's).
    pub(crate) fn apply(&mut self, edit: DockEdit, motion: Motion, rows: u16, table: &Clusters) {
        let (keypress, erase) = motion.glyph_fx(self.keypress, self.erase);
        let (at, cells, shift, kind, effect, duration) = match edit {
            DockEdit::Reset => {
                self.finish();
                return;
            }
            DockEdit::Shift { by } => {
                self.shift(by, rows);
                return;
            }
            DockEdit::Arrive {
                row,
                col,
                cells,
                shift,
            } => (
                (row, col),
                cells,
                shift,
                Kind::Arrival,
                keypress.id(),
                KEYPRESS_DURATION,
            ),
            DockEdit::Erase {
                row,
                col,
                ghosts,
                shift,
            } => (
                (row, col),
                ghosts,
                shift,
                Kind::Ghost,
                erase.id(),
                ERASE_DURATION,
            ),
        };
        self.shift(shift, rows);
        // **Erasing an in-flight arrival does not give birth to a ghost**: the ghost
        // starts fully opaque at `t = 0`, so when a half-faded letter was erased it
        // would jump to full colour for a frame and go from there — every time a fast
        // typo is corrected. The letter vanishes from where it is.
        let mut unborn = [false; EDIT_MAX];
        if kind == Kind::Ghost {
            for (slot, cell) in unborn.iter_mut().zip(cells.as_slice()) {
                *slot = self.entries.iter().any(|entry| {
                    entry.fx.kind == Kind::Arrival
                        && (entry.fx.cell.row, entry.fx.cell.col) == (cell.row, cell.col)
                        && entry.fx.cell.ch == cell.ch
                });
            }
        }
        self.entries.retain(|entry| {
            entry.fx.kind == Kind::Ghost || (entry.fx.cell.row, entry.fx.cell.col) < at
        });
        let Some(effect) = effect else {
            return;
        };
        self.rebuild_clusters();
        for (&cell, _) in cells
            .as_slice()
            .iter()
            .zip(unborn)
            .filter(|&(_, unborn)| !unborn)
        {
            if self.entries.len() >= FX_MAX {
                // The oldest entry ends: the list is in insertion order.
                self.entries.remove(0);
            }
            self.serial = self.serial.wrapping_add(1);
            let cell = Cell {
                cluster: copy_cluster(cell.cluster, table, &mut self.clusters),
                ..cell
            };
            self.entries.push(Entry {
                fx: Fx {
                    cell,
                    kind,
                    effect,
                    t: 0.0,
                    // audit: the seed is only a pattern; kept small so that it is represented
                    // exactly in `f32`.
                    seed: (self.serial % 1024) as f32,
                },
                elapsed: 0.0,
                duration,
            });
        }
    }

    /// Rebuilds the cluster table from the living entries: so the strings of dropped
    /// entries do not pile up in the table.
    fn rebuild_clusters(&mut self) {
        self.scratch.clear();
        for entry in &mut self.entries {
            entry.fx.cell.cluster =
                copy_cluster(entry.fx.cell.cluster, &self.clusters, &mut self.scratch);
        }
        std::mem::swap(&mut self.clusters, &mut self.scratch);
    }

    /// The cluster table of the entries ([`GlyphFx::iter`]).
    pub(crate) fn clusters(&self) -> &Clusters {
        &self.clusters
    }

    /// Sets the two effects the user chose — raw names, the reduction on every edit
    /// ([`Motion::glyph_fx`]).
    ///
    /// **In-flight ones end on change** (`Motion::set_style`'s precedent): there is no
    /// meaning in continuing an entry started with the old effect on the new effect's
    /// curve, and what a user who switched to `off` sees must be exactly "instant".
    /// The same selection is a no-op. The return is whether something was ended: the
    /// ended entry's final state only drops from the screen if a frame is drawn, and
    /// that frame's request is the caller's (`DisplayLink::set_glyph_fx`).
    pub(crate) fn set_effects(&mut self, keypress: Keypress, erase: Erase) -> bool {
        if (self.keypress, self.erase) == (keypress, erase) {
            return false;
        }
        self.keypress = keypress;
        self.erase = erase;
        let finished = !self.is_empty();
        self.finish();
        finished
    }

    /// Shifts the in-flight ones by the vertical window's shift (rows); an entry that
    /// overflows the window drops (so the ghost does not hang above the context line
    /// or the grid, and the arrival's static glyph is already not in that window).
    /// The column does not shift: there is no horizontal window, a long line wraps.
    ///
    /// It runs in a frame without a shift too: while the window's top stays in place
    /// the band can shrink (the last letter of a wrapped line was erased) and the
    /// ghost on a row that no longer exists would be drawn above the context line.
    fn shift(&mut self, by: i32, rows: u16) {
        self.entries.retain_mut(|entry| {
            let cell = &mut entry.fx.cell;
            let row = i32::from(cell.row) + by;
            if !(0..i32::from(rows)).contains(&row) {
                return false;
            }
            // audit: `0 ≤ row < rows` and `rows` is a `u16`.
            cell.row = row as u16;
            true
        });
    }

    /// Processes the elapsed time; an entry whose time is up drops.
    ///
    /// `dt` is clamped to [`crate::motion::DT_MAX`], the same reason as the other
    /// animators: the first interval of a link waking from sleep can be unbounded.
    pub(crate) fn advance(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, crate::motion::DT_MAX);
        self.entries.retain_mut(|entry| {
            entry.elapsed += dt;
            entry.elapsed < entry.duration
        });
    }

    /// Ends all the in-flight ones: arrivals sit on their static glyphs, ghosts go.
    ///
    /// Its callers are the same as `Motion::finish`'s (an occluded window, a setting
    /// that switches to `snap` or Reduce Motion, a drawing error) and so is its
    /// reason: an effect frozen in a background tab would continue from a phase never
    /// seen when it came back, and on a permanent error the motion frame would keep
    /// spinning.
    pub(crate) fn finish(&mut self) {
        self.entries.clear();
    }

    /// Is there nothing in flight — a term of the link's sleep test.
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Keeps only the wanted entries — `Frame::suppress_dock`'s branch: an arrival
    /// whose static glyph is not found ends.
    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&Fx) -> bool) {
        self.entries.retain(|entry| keep(&entry.fx));
    }

    /// Bu karenin girdileri, ilerlemeleriyle.
    pub(crate) fn iter(&self) -> impl Iterator<Item = Fx> + '_ {
        self.entries.iter().map(|entry| Fx {
            t: (entry.elapsed / entry.duration).min(1.0),
            ..entry.fx
        })
    }
}

#[cfg(test)]
mod tests {
    use bt_core::{CursorMotion, DOCK_TEXT_COL, DockEdit, EDIT_MAX, EditCells};

    use super::*;

    /// The vertical window's size: a single input row.
    const WINDOW: u16 = 1;

    fn cell_at(row: u16, col: u16, ch: char) -> Cell {
        Cell {
            row,
            col,
            ch: Some(ch),
            ..Cell::default()
        }
    }

    fn cells(list: &[Cell]) -> EditCells {
        assert!(list.len() <= EDIT_MAX);
        list.iter().copied().collect()
    }

    fn arrive(col: u16, ch: char) -> DockEdit {
        arrive_at(0, col, ch)
    }

    fn arrive_at(row: u16, col: u16, ch: char) -> DockEdit {
        DockEdit::Arrive {
            row,
            col,
            cells: cells(&[cell_at(row, col, ch)]),
            shift: 0,
        }
    }

    fn erase(col: u16, ch: char) -> DockEdit {
        erase_at(0, col, ch)
    }

    fn erase_at(row: u16, col: u16, ch: char) -> DockEdit {
        DockEdit::Erase {
            row,
            col,
            ghosts: cells(&[cell_at(row, col, ch)]),
            shift: 0,
        }
    }

    fn cols(fx: &GlyphFx) -> Vec<(u16, Kind)> {
        fx.iter().map(|fx| (fx.cell.col, fx.kind)).collect()
    }

    fn places(fx: &GlyphFx) -> Vec<(u16, u16, Kind)> {
        fx.iter()
            .map(|fx| (fx.cell.row, fx.cell.col, fx.kind))
            .collect()
    }

    #[test]
    fn a_new_choice_finishes_what_is_in_flight() {
        let mut fx = GlyphFx::default();
        fx.apply(
            arrive(4, 'a'),
            Motion::default(),
            WINDOW,
            &Clusters::default(),
        );
        // A save that rewrites the same selection is a no-op: the effect continues and
        // no frame is asked for either.
        assert!(!fx.set_effects(Keypress::Fade, Erase::Recede));
        assert_eq!(cols(&fx), [(4, Kind::Arrival)]);
        // The change ends the in-flight one and says so (the link asks for a frame).
        assert!(fx.set_effects(Keypress::Off, Erase::Recede));
        assert!(fx.is_empty());
        // The new selection takes effect at the next edit.
        fx.apply(
            arrive(5, 'b'),
            Motion::default(),
            WINDOW,
            &Clusters::default(),
        );
        fx.apply(
            erase(7, 'c'),
            Motion::default(),
            WINDOW,
            &Clusters::default(),
        );
        assert_eq!(cols(&fx), [(7, Kind::Ghost)]);
        assert!(fx.set_effects(Keypress::Fade, Erase::Off));
        fx.apply(
            erase(7, 'c'),
            Motion::default(),
            WINDOW,
            &Clusters::default(),
        );
        assert!(fx.is_empty());
        // In an empty list the change ended nothing: no frame is needed.
        assert!(!fx.set_effects(Keypress::Off, Erase::Off));
    }

    #[test]
    fn an_arrival_and_a_ghost_live_for_their_duration() {
        let mut fx = GlyphFx::default();
        fx.apply(
            arrive(4, 'a'),
            Motion::default(),
            WINDOW,
            &Clusters::default(),
        );
        fx.apply(
            erase(6, 'b'),
            Motion::default(),
            WINDOW,
            &Clusters::default(),
        );
        assert_eq!(cols(&fx), [(4, Kind::Arrival), (6, Kind::Ghost)]);
        assert!(fx.iter().all(|fx| fx.t == 0.0));
        // The steps are under `DT_MAX`: a single big `dt` would be clamped.
        let step = KEYPRESS_DURATION / 4.0;
        assert!(step <= crate::motion::DT_MAX);
        // Halfway, the progress follows the time.
        fx.advance(step);
        fx.advance(step);
        let t: Vec<f32> = fx.iter().map(|fx| fx.t).collect();
        assert!((t[0] - 0.5).abs() < 1e-6, "{t:?}");
        assert!(t[1] < t[0], "the ghost's duration is longer: {t:?}");
        // The arrival ends, the ghost continues; then it ends too and the list empties.
        fx.advance(step);
        fx.advance(step);
        assert_eq!(cols(&fx), [(6, Kind::Ghost)]);
        for _ in 0..4 {
            fx.advance(ERASE_DURATION / 4.0);
        }
        assert!(fx.is_empty(), "the entry whose time was up did not drop");
    }

    #[test]
    fn a_reset_finishes_everything() {
        let mut fx = GlyphFx::default();
        fx.apply(
            arrive(4, 'a'),
            Motion::default(),
            WINDOW,
            &Clusters::default(),
        );
        fx.apply(
            erase(6, 'b'),
            Motion::default(),
            WINDOW,
            &Clusters::default(),
        );
        fx.apply(
            DockEdit::Reset,
            Motion::default(),
            WINDOW,
            &Clusters::default(),
        );
        assert!(fx.is_empty());
    }

    #[test]
    fn a_new_edit_settles_the_arrivals_at_and_right_of_its_column() {
        let mut fx = GlyphFx::default();
        fx.apply(
            arrive(4, 'a'),
            Motion::default(),
            WINDOW,
            &Clusters::default(),
        );
        fx.apply(
            arrive(5, 'b'),
            Motion::default(),
            WINDOW,
            &Clusters::default(),
        );
        fx.apply(
            erase(9, 'z'),
            Motion::default(),
            WINDOW,
            &Clusters::default(),
        );
        // Normal typing: the next key adds to the right, the earlier arrivals continue.
        fx.apply(
            arrive(6, 'c'),
            Motion::default(),
            WINDOW,
            &Clusters::default(),
        );
        assert_eq!(
            cols(&fx),
            [
                (4, Kind::Arrival),
                (5, Kind::Arrival),
                (9, Kind::Ghost),
                (6, Kind::Arrival)
            ]
        );
        // The caret moved left and typed at 5: 5 and the arrivals to its right sit, the
        // ghost stays in place.
        fx.apply(
            arrive(5, 'x'),
            Motion::default(),
            WINDOW,
            &Clusters::default(),
        );
        assert_eq!(
            cols(&fx),
            [(4, Kind::Arrival), (9, Kind::Ghost), (5, Kind::Arrival)]
        );
    }

    #[test]
    fn a_new_edit_settles_the_arrivals_after_it_in_reading_order() {
        // In a wrapped input the rule is on two axes: the text behind the
        // edit slides to the lower lines too, so the arrivals on the lower line must sit
        // too; those on the upper line — even if in the right column — are in the prefix
        // and in place.
        let mut fx = GlyphFx::default();
        fx.apply(
            arrive_at(0, 5, 'a'),
            Motion::default(),
            2,
            &Clusters::default(),
        );
        fx.apply(
            arrive_at(1, 2, 'b'),
            Motion::default(),
            2,
            &Clusters::default(),
        );
        fx.apply(
            arrive_at(1, 3, 'c'),
            Motion::default(),
            2,
            &Clusters::default(),
        );
        fx.apply(
            erase_at(1, 6, 'z'),
            Motion::default(),
            2,
            &Clusters::default(),
        );
        // Typing in the 3rd column of the second line: (1, 3) and after it sit, the
        // upper line's 5th column (earlier in reading order) continues, the ghost in
        // place.
        fx.apply(
            arrive_at(1, 3, 'x'),
            Motion::default(),
            2,
            &Clusters::default(),
        );
        assert_eq!(
            places(&fx),
            [
                (0, 5, Kind::Arrival),
                (1, 2, Kind::Arrival),
                (1, 6, Kind::Ghost),
                (1, 3, Kind::Arrival)
            ]
        );
        // Typing on the upper line: all the arrivals on the lower line sit.
        fx.apply(
            arrive_at(0, 2, 'y'),
            Motion::default(),
            2,
            &Clusters::default(),
        );
        assert_eq!(places(&fx), [(1, 6, Kind::Ghost), (0, 2, Kind::Arrival)]);
        // Erasing an arrival that stands in the same column but on another row gives
        // birth to a ghost: the "unborn" criterion is the position's two axes.
        fx.apply(
            erase_at(1, 2, 'y'),
            Motion::default(),
            2,
            &Clusters::default(),
        );
        assert!(
            places(&fx).contains(&(1, 2, Kind::Ghost)),
            "{:?}",
            places(&fx)
        );
    }

    #[test]
    fn a_shift_moves_the_flight_with_the_text_and_drops_what_leaves() {
        let rows = 3;
        let mut fx = GlyphFx::default();
        fx.apply(
            arrive_at(1, 3, 'a'),
            Motion::default(),
            rows,
            &Clusters::default(),
        );
        fx.apply(
            erase_at(2, 9, 'b'),
            Motion::default(),
            rows,
            &Clusters::default(),
        );
        // Typing on the last line of an input that exceeds the ceiling: the vertical
        // window went down a row, the text slid up a row and the new letter is on the
        // last row.
        fx.apply(
            DockEdit::Arrive {
                row: 2,
                col: 8,
                cells: cells(&[cell_at(2, 8, 'c')]),
                shift: -1,
            },
            Motion::default(),
            rows,
            &Clusters::default(),
        );
        assert_eq!(
            places(&fx),
            [
                (0, 3, Kind::Arrival),
                (1, 9, Kind::Ghost),
                (2, 8, Kind::Arrival)
            ]
        );
        // The wheel moved the window up two rows: the bottom two entries fall outside
        // the window.
        fx.apply(
            DockEdit::Shift { by: 2 },
            Motion::default(),
            rows,
            &Clusters::default(),
        );
        assert_eq!(places(&fx), [(2, 3, Kind::Arrival)]);
        // What overflows upward drops too: it does not hang above the context line or
        // the grid.
        fx.apply(
            DockEdit::Shift { by: -3 },
            Motion::default(),
            rows,
            &Clusters::default(),
        );
        assert!(fx.is_empty(), "{:?}", places(&fx));
        // No shift but a shrinking window: the last letter of the wrapped line was
        // erased, the band went down to a single row and the ghost on the lower line
        // drops.
        fx.apply(
            erase_at(1, 2, 'e'),
            Motion::default(),
            2,
            &Clusters::default(),
        );
        fx.apply(
            erase_at(0, 5, 'd'),
            Motion::default(),
            1,
            &Clusters::default(),
        );
        assert_eq!(places(&fx), [(0, 5, Kind::Ghost)]);
    }

    #[test]
    fn a_full_list_finishes_the_oldest() {
        let mut fx = GlyphFx::default();
        for n in 0..FX_MAX + 3 {
            // Ghosts do not enter the position rule: all of them live.
            fx.apply(
                erase(DOCK_TEXT_COL, 'a'),
                Motion::default(),
                WINDOW,
                &Clusters::default(),
            );
            fx.advance(0.0001 * n as f32);
        }
        assert_eq!(fx.iter().count(), FX_MAX);
        // The oldest are gone: none of the remaining carries the seed of the first
        // three entries.
        assert!(fx.iter().all(|fx| fx.seed > 3.0), "the oldest did not drop");
    }

    #[test]
    fn a_seed_is_fixed_for_the_life_of_an_entry() {
        // The fragmented effects' pattern comes from the seed: if the same entry does
        // not carry the same seed in two frames, `shatter`'s fragments are redistributed
        // in every motion frame. Sliding and progress do not touch the seed; the seeds
        // of two entries side by side are different (so they do not shatter with the
        // same pattern).
        let mut fx = GlyphFx::default();
        fx.apply(
            erase(6, 'a'),
            Motion::default(),
            WINDOW,
            &Clusters::default(),
        );
        fx.apply(
            erase(5, 'b'),
            Motion::default(),
            WINDOW,
            &Clusters::default(),
        );
        let seeds = |fx: &GlyphFx| fx.iter().map(|fx| fx.seed).collect::<Vec<f32>>();
        let born = seeds(&fx);
        assert_ne!(born[0], born[1], "two entries were born with the same seed");
        fx.advance(ERASE_DURATION / 3.0);
        assert_eq!(seeds(&fx), born, "the progress changed the seed");
        fx.apply(
            DockEdit::Shift { by: 1 },
            Motion::default(),
            2,
            &Clusters::default(),
        );
        assert_eq!(seeds(&fx), born, "the slide changed the seed");
    }

    #[test]
    fn erasing_an_arrival_in_flight_leaves_no_ghost() {
        // The ghost of a half-faded letter would be born fully opaque at `t = 0` and
        // jump for a frame: the erased arrival vanishes from where it is. Erasing
        // **another** letter in the same column (its arrival long ended) gets a ghost.
        let mut fx = GlyphFx::default();
        fx.apply(
            arrive(4, 'a'),
            Motion::default(),
            WINDOW,
            &Clusters::default(),
        );
        fx.apply(
            erase(4, 'a'),
            Motion::default(),
            WINDOW,
            &Clusters::default(),
        );
        assert!(fx.is_empty(), "{:?}", cols(&fx));
        fx.apply(
            erase(4, 'b'),
            Motion::default(),
            WINDOW,
            &Clusters::default(),
        );
        assert_eq!(cols(&fx), [(4, Kind::Ghost)]);
    }

    #[test]
    fn finish_empties_the_flight() {
        let mut fx = GlyphFx::default();
        fx.apply(
            arrive(4, 'a'),
            Motion::default(),
            WINDOW,
            &Clusters::default(),
        );
        fx.finish();
        assert!(fx.is_empty());
    }

    #[test]
    fn snap_and_reduce_motion_reduce_the_effects() {
        // The reduction table: `snap` turns both off, Reduce Motion
        // lowers the arrival to `fade` and turns the ghost off.
        let motion = |style: CursorMotion, reduce: bool| {
            let mut motion = Motion::default();
            motion.set_style(style);
            motion.set_reduce(reduce);
            motion
        };
        for reduce in [false, true] {
            let snap = motion(CursorMotion::Snap, reduce);
            let mut fx = GlyphFx::default();
            fx.apply(arrive(4, 'a'), snap, WINDOW, &Clusters::default());
            fx.apply(erase(6, 'b'), snap, WINDOW, &Clusters::default());
            assert!(
                fx.is_empty(),
                "snap gave birth to an animation (reduce={reduce})"
            );
        }
        for style in [CursorMotion::Ease, CursorMotion::Spring] {
            let reduced = motion(style, true);
            let mut fx = GlyphFx::default();
            fx.apply(arrive(4, 'a'), reduced, WINDOW, &Clusters::default());
            fx.apply(erase(6, 'b'), reduced, WINDOW, &Clusters::default());
            let kinds: Vec<(Kind, u32)> = fx.iter().map(|fx| (fx.kind, fx.effect)).collect();
            assert_eq!(
                kinds,
                [(Kind::Arrival, Keypress::Fade.id().expect("fade"))],
                "Reduce Motion: the arrival fades in, no ghost ({style:?})"
            );
            let plain = motion(style, false);
            assert_eq!(
                plain.glyph_fx(Keypress::Fade, Erase::Recede),
                (Keypress::Fade, Erase::Recede)
            );
        }
        // The accessibility setting **does not add** animation: an arrival that is off
        // stays off.
        assert_eq!(
            motion(CursorMotion::Spring, true).glyph_fx(Keypress::Off, Erase::Recede),
            (Keypress::Off, Erase::Off)
        );
    }
}
