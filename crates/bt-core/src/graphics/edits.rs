//! What changed in an image's pixels, for whatever keeps a copy of them.
//!
//! A program that edits a frame a few times a second edits a small part of
//! the image each time, and a copy kept elsewhere (a texture on the GPU) is
//! brought up to date by that part alone. The store remembers the last few
//! edits, each with the revision it made ([`super::place::Picture::image`]),
//! and a copy that holds revision `r` asks what changed since: the rectangle
//! that covers the edits after `r`, or "all of it" when the memory does not
//! reach back to `r` — which is also what a copy gets that was never told
//! anything.
//!
//! **A change that is not here is a stale copy that nothing notices**, so every
//! way the pixels on show change records itself ([`Edits::note`]) and the
//! memory says plainly where it stops ([`Edits::since`]).

use std::collections::VecDeque;

/// How many edits are remembered. A copy that fell further behind is given
/// the whole image, which is what it would have taken anyway for that many
/// edits to a small part of it.
const REMEMBERED: usize = 8;

/// What a copy that holds an older revision is to take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dirty {
    /// The pixels are the ones it has.
    Nothing,
    /// These alone, `[x, y, width, height]` in the image's pixels.
    Rect([u32; 4]),
    /// All of them.
    Whole,
}

/// The last few edits of the pixels on show.
#[derive(Clone, Debug)]
pub struct Edits {
    /// Every change after this revision is in `list`; a copy older than that
    /// is not told what changed.
    floor: u64,
    /// The edits, oldest first, each with the revision it made and where.
    list: VecDeque<(u64, [u32; 4])>,
}

impl Default for Edits {
    /// The memory of an image nothing is known about: every copy is given all
    /// of it.
    fn default() -> Self {
        Self {
            floor: u64::MAX,
            list: VecDeque::new(),
        }
    }
}

impl Edits {
    /// The memory of an image whose pixels were first on show as `revision`.
    pub(crate) fn from(revision: u64) -> Self {
        Self {
            floor: revision,
            list: VecDeque::new(),
        }
    }

    /// A memory built by hand: what a layer that keeps copies is tested with.
    /// It starts at the revision `first` and has had `edits`, each the revision
    /// it made and the rectangle `[x, y, width, height]` it changed.
    pub fn built(first: u64, edits: &[(u64, [u32; 4])]) -> Self {
        let mut made = Self::from(first);
        for &(revision, rect) in edits {
            made.note(revision, rect);
        }
        made
    }

    /// The pixels in `rect` changed, and the image on show is `revision` from
    /// here on.
    pub(crate) fn note(&mut self, revision: u64, rect: [u32; 4]) {
        if self.list.len() == REMEMBERED
            && let Some((forgotten, _)) = self.list.pop_front()
        {
            self.floor = forgotten;
        }
        self.list.push_back((revision, rect));
    }

    /// What a copy that holds `known` is to take to hold the image on show.
    pub fn since(&self, known: u64) -> Dirty {
        if known < self.floor {
            return Dirty::Whole;
        }
        let mut covered: Option<[u32; 4]> = None;
        for &(_, rect) in self.list.iter().filter(|(revision, _)| *revision > known) {
            covered = Some(match covered {
                None => rect,
                Some(held) => join(held, rect),
            });
        }
        covered.map_or(Dirty::Nothing, Dirty::Rect)
    }
}

/// The smallest rectangle that holds both, in 64 bits so that nothing a
/// program wrote can overflow it.
fn join(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    let left = a[0].min(b[0]);
    let top = a[1].min(b[1]);
    let right = (u64::from(a[0]) + u64::from(a[2])).max(u64::from(b[0]) + u64::from(b[2]));
    let bottom = (u64::from(a[1]) + u64::from(a[3])).max(u64::from(b[1]) + u64::from(b[3]));
    let span = |end: u64, start: u32| u32::try_from(end - u64::from(start)).unwrap_or(u32::MAX);
    [left, top, span(right, left), span(bottom, top)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_that_is_up_to_date_takes_nothing() {
        let mut edits = Edits::from(5);
        assert_eq!(edits.since(5), Dirty::Nothing);
        edits.note(9, [1, 1, 2, 2]);
        assert_eq!(edits.since(9), Dirty::Nothing);
    }

    #[test]
    fn a_copy_takes_the_rectangle_that_covers_what_came_after_it() {
        let mut edits = Edits::from(5);
        edits.note(6, [0, 0, 2, 2]);
        edits.note(7, [10, 4, 2, 2]);
        edits.note(8, [3, 8, 1, 1]);
        assert_eq!(edits.since(5), Dirty::Rect([0, 0, 12, 9]));
        assert_eq!(edits.since(6), Dirty::Rect([3, 4, 9, 5]));
        assert_eq!(edits.since(8), Dirty::Nothing);
    }

    #[test]
    fn a_copy_older_than_the_memory_takes_the_whole_image() {
        let mut edits = Edits::from(1);
        for step in 10..10 + REMEMBERED {
            edits.note(step as u64, [0, 0, 1, 1]);
        }
        // Every change since the first one is still known.
        assert_eq!(edits.since(1), Dirty::Rect([0, 0, 1, 1]));
        // One more and the memory begins after the first revision.
        edits.note(100, [0, 0, 1, 1]);
        assert_eq!(edits.since(1), Dirty::Whole);
        assert_eq!(edits.since(9), Dirty::Whole);
        assert_eq!(edits.since(10), Dirty::Rect([0, 0, 1, 1]));
    }

    #[test]
    fn an_image_nothing_is_known_about_is_taken_whole() {
        let edits = Edits::default();
        assert_eq!(edits.since(0), Dirty::Whole);
        assert_eq!(edits.since(u64::MAX - 1), Dirty::Whole);
    }

    #[test]
    fn joining_does_not_overflow() {
        assert_eq!(join([u32::MAX, 0, 1, 1], [0, 0, 1, 1]), [0, 0, u32::MAX, 1]);
    }
}
