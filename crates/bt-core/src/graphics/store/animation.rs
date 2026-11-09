//! An image that plays its frames by itself.
//!
//! `a=a` tells the terminal to run the animation (`s=2` or `s=3`), how many
//! times (`v=`), how long a frame stays (`r=` with `z=`) or which frame is on
//! show (`c=`). From then on the terminal changes the frame on show when the
//! gap of the one on show has passed, **as long as the image is on the
//! screen**: an image that nothing shows has no clock, and asks the render
//! loop for nothing.
//!
//! Time is whatever the caller says it is: a command carries the instant it
//! came and a frame is made at the instant the render loop gives, so no test
//! waits for a gap. What follows from the protocol:
//!
//! - a frame with no gap is passed over, and the root frame has none until
//!   something sets it;
//! - in loading mode (`s=2`) the animation waits on the last frame for more
//!   frames instead of going back to the first;
//! - `v=1` is for ever and `v=n` is `n - 1` loops; stopping resets the count;
//! - one frame is taken at a time, so an animation that was not drawn for a
//!   long time goes on from where it was instead of catching up.

use std::time::{Duration, Instant};

use super::super::command::Command;
use super::super::{Echo, Error, Reply};
use super::frames::gap_of;
use super::{Frame, Image, Images, Placement, Screen, Spots, Store, View, Window};

/// What the terminal does with the frames of an image.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum State {
    /// The frame on show stays.
    #[default]
    Stopped,
    /// Runs, and waits on the last frame for frames to come.
    Loading,
    /// Runs and begins again after the last frame.
    Running,
}

/// How an image plays: the state and what it keeps while it runs.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Animation {
    pub(crate) state: State,
    /// How many times it runs; 0 is for ever.
    pub(crate) loops: u32,
    /// How many times it has gone from the last frame to the first.
    pub(crate) done: u32,
    /// When the frame on show came on show; `None` before the first time it
    /// is looked at while it runs.
    pub(crate) shown_at: Option<Instant>,
}

impl Image {
    /// Whether the animation has something to do: it runs, it has a frame to
    /// go to and a gap to wait, and it has not run as many times as it was told.
    fn plays(&self) -> bool {
        self.animation.state != State::Stopped
            && self.frames.len() > 1
            && self.frames.iter().any(|frame| frame.gap != 0)
            && (self.animation.loops == 0 || self.animation.done < self.animation.loops)
    }

    /// Takes the frame that is due at `now`, if one is, and says when the one
    /// after it is: `None` for an animation that waits for nothing. `stamp` names
    /// the pixels of the frame that comes on show.
    pub(super) fn play(&mut self, now: Instant, stamp: &mut dyn FnMut() -> u64) -> Option<Instant> {
        if !self.plays() {
            return None;
        }
        let count = self.frames.len();
        let shown_at = *self.animation.shown_at.get_or_insert(now);
        let gap = |frames: &[Frame], at: usize| {
            Duration::from_millis(u64::from(frames.get(at).map_or(0, |frame| frame.gap)))
        };
        let due = shown_at + gap(&self.frames, self.current);
        if now < due {
            return Some(due);
        }
        let mut at = self.current;
        for _ in 0..=count {
            at = (at + 1) % count;
            if at == 0 {
                if self.animation.state == State::Loading {
                    // The next frame is not here yet: nothing to wake for until
                    // a frame comes.
                    return None;
                }
                self.animation.done += 1;
                if self.animation.loops != 0 && self.animation.done >= self.animation.loops {
                    return None;
                }
            }
            if self.frames.get(at).is_some_and(|frame| frame.gap != 0) {
                break;
            }
        }
        self.current = at;
        self.animation.shown_at = Some(now);
        self.changed(stamp(), None);
        Some(now + gap(&self.frames, at))
    }
}

impl Store {
    /// The placements that touch `window`, as a frame draws them, each with
    /// the grid line and the column of its first cell: a relative placement
    /// is where `spots` has it, and is not here when it has no place.
    pub(super) fn in_window<'a>(
        &'a self,
        window: Window,
        cell: (u16, u16),
        columns: u16,
        spots: &'a Spots,
    ) -> impl Iterator<Item = (&'a Placement, i32, i32)> {
        self.placements.iter().filter_map(move |placement| {
            let (line, col) = if placement.is_relative() {
                *spots.get(&placement.seq)?
            } else {
                (placement.line, i32::from(placement.col))
            };
            let cols = i32::try_from(placement.spec.span(cell).0).unwrap_or(i32::MAX);
            let reaches = line <= window.last
                && placement.bottom_at(line, cell) >= window.first
                && col < i32::from(columns)
                && col.saturating_add(cols) > 0;
            reaches.then_some((placement, line, col))
        })
    }

    /// Plays the images the view shows up to `now` — through a placement or,
    /// for those the placeholder cells show, through them; when the next frame
    /// of any of them is due, if one will be.
    pub(super) fn advance(
        &mut self,
        view: &View,
        cell: (u16, u16),
        columns: u16,
        now: Instant,
        stamp: &mut dyn FnMut() -> u64,
    ) -> Option<Instant> {
        let spots = self.locate(view.marks, cell);
        let shown: Vec<u64> = std::iter::once(view.grid)
            .chain(view.band)
            .flat_map(|window| self.in_window(window, cell, columns, &spots))
            .map(|(placement, _, _)| placement.image)
            .chain(self.marked(view.marks.all()))
            .collect();
        if shown.is_empty() {
            return None;
        }
        let mut next: Option<Instant> = None;
        for image in &mut self.images {
            if !shown.contains(&image.generation) {
                continue;
            }
            if let Some(due) = image.play(now, stamp) {
                next = Some(next.map_or(due, |held| held.min(due)));
            }
        }
        next
    }

    /// `a=a`: what the command changes of how the image plays.
    fn control(&mut self, command: &Command, now: Instant, stamp: u64) -> Result<(), Error> {
        let echo: Echo = command.echo;
        let at = self
            .index_of(echo)
            .ok_or_else(|| Self::missing_image(echo))?;
        let image = self
            .images
            .get_mut(at)
            .ok_or_else(|| Self::missing_image(echo))?;
        let count = image.frames.len();
        let frame_at = |number: u32| {
            usize::try_from(number)
                .ok()
                .filter(|number| (1..=count).contains(number))
                .map(|number| number - 1)
        };
        if command.z != 0
            && let Some(frame) = frame_at(command.rows).and_then(|at| image.frames.get_mut(at))
        {
            frame.gap = gap_of(command.z);
        }
        if let Some(at) = frame_at(command.columns)
            && at != image.current
        {
            image.current = at;
            image.animation.shown_at = Some(now);
            image.changed(stamp, None);
        }
        let state = match command.width {
            1 => Some(State::Stopped),
            2 => Some(State::Loading),
            3 => Some(State::Running),
            _ => None,
        };
        if let Some(state) = state {
            if state != State::Stopped && image.animation.state == State::Stopped {
                image.animation.shown_at = Some(now);
            }
            image.animation.state = state;
            image.animation.done = 0;
        }
        if command.height != 0 {
            image.animation.loops = command.height - 1;
        }
        Ok(())
    }
}

impl Images {
    /// Plays the images of the screen showing that the view shows, up to
    /// `now`, and says when the next frame of any of them is due, if one is.
    pub(crate) fn advance(
        &mut self,
        alt: bool,
        cell: (u16, u16),
        columns: u16,
        view: &View,
        now: Instant,
    ) -> Option<Instant> {
        let mut counter = self.generation;
        let due = self.screen(alt).advance(view, cell, columns, now, &mut || {
            counter += 1;
            counter
        });
        self.generation = counter;
        due
    }

    /// `a=a`: the answer is an error for an image that is not there, and
    /// nothing otherwise, as it is in kitty.
    pub(crate) fn animate(
        &mut self,
        screen: &Screen,
        command: &Command,
        now: Instant,
    ) -> Option<Reply> {
        let stamp = self.next_stamp();
        match self.screen(screen.alt).control(command, now, stamp) {
            Ok(()) => None,
            Err(error) => Some(Reply::failed(command.echo, error)),
        }
    }
}
