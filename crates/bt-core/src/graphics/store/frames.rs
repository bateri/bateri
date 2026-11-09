//! The frames of a stored image: making them, changing them, removing them.
//!
//! `a=f` lays a decoded rectangle on a canvas, which is the frame it edits (`r=`)
//! or, for a new frame, a copy of another (`c=`) or a plain color (`Y=`);
//! `a=c` lays a rectangle of one frame on another; `d=f` takes frames away.
//! Whatever the frame is, the result is kept whole.
//!
//! **A change that reaches the frame on show takes a new revision**, which
//! is what tells the drawing the pixels are not the ones it has; a change to
//! any other frame is not seen until that frame is shown. The decoding is not
//! done here: it happened where no lock is held, and what is done here under the
//! terminal's lock is the lay-down of the rectangle alone, in place (the pixels
//! are copied only if a frame being drawn still holds them).

use std::sync::Arc;

use super::super::canvas::{self, Mode, Rect};
use super::super::command::Command;
use super::super::receive::Decoded;
use super::super::{Echo, Error, IMAGE_LIMIT, Reply};
use super::{DEFAULT_GAP, Frame, Image, Images, Screen, Store};

/// A gap as a command gives it: positive is the milliseconds, negative a
/// frame that is passed over.
pub(super) fn gap_of(z: i32) -> u32 {
    u32::try_from(z).unwrap_or(0)
}

/// The color `Y=` names, as RGBA: the number is `0xRRGGBBAA`.
fn color_of(number: u32) -> [u8; 4] {
    number.to_be_bytes()
}

fn mismatch() -> Error {
    Error::invalid("the frame data does not fit the image")
}

impl Store {
    /// The place of the image a command names, by id or else by number.
    pub(super) fn index_of(&self, echo: Echo) -> Option<usize> {
        if echo.id != 0 {
            self.images.iter().position(|image| image.id == echo.id)
        } else if echo.number != 0 {
            self.images
                .iter()
                .rposition(|image| image.number == echo.number)
        } else {
            None
        }
    }

    pub(super) fn missing_image(echo: Echo) -> Error {
        Error::missing(format!(
            "the image with id {} and number {} does not exist",
            echo.id, echo.number
        ))
    }

    /// Makes room for `extra` more bytes by taking images out but `keep`: those
    /// no placement shows first, oldest first, then the oldest of the rest, as a
    /// stored image does ([`Store::insert`]). `ENOSPC` when what is left is not
    /// enough.
    fn make_room(&mut self, extra: usize, keep: u64) -> Result<(), Error> {
        // Nothing goes for a frame that cannot fit: the image's own, and the
        // frame, are all that is certain to stay.
        let own = self
            .images
            .iter()
            .find(|image| image.generation == keep)
            .map_or(0, Image::bytes);
        if own.saturating_add(extra) > self.quota {
            return Err(Error::no_space("the frame does not fit the storage quota"));
        }
        let shown = self.shown();
        while self.bytes.saturating_add(extra) > self.quota {
            let victim = self
                .images
                .iter()
                .position(|image| image.generation != keep && !shown.contains(&image.generation))
                .or_else(|| {
                    self.images
                        .iter()
                        .position(|image| image.generation != keep)
                });
            let Some(victim) = victim else {
                return Err(Error::no_space("the frame does not fit the storage quota"));
            };
            self.remove(victim);
        }
        self.prune_relatives(true);
        Ok(())
    }

    /// `a=f`: lays `decoded` on a frame of the image the command names.
    /// The id of the image, for the answer.
    pub(super) fn edit_frame(
        &mut self,
        command: &Command,
        decoded: &Decoded,
        stamp: u64,
    ) -> Result<u32, Error> {
        let echo = command.echo;
        let at = self
            .index_of(echo)
            .ok_or_else(|| Self::missing_image(echo))?;
        let image = self
            .images
            .get(at)
            .ok_or_else(|| Self::missing_image(echo))?;
        let (size, generation, id) = ((image.width, image.height), image.generation, image.id);
        let rect = Rect {
            x: command.x,
            y: command.y,
            width: decoded.width,
            height: decoded.height,
        };
        if !rect.fits(size) {
            return Err(Error::invalid("the frame is larger than the image"));
        }
        let mode = if command.cell_x == 1 {
            Mode::Replace
        } else {
            Mode::Blend
        };
        let count = image.frames.len();
        // A frame number past the last frame makes a new one, as it does in
        // kitty; 0 is "a new one" too.
        let edited = usize::try_from(command.rows)
            .ok()
            .filter(|number| (1..=count).contains(number));
        if let Some(number) = edited {
            let image = self
                .images
                .get_mut(at)
                .ok_or_else(|| Self::missing_image(echo))?;
            let current = image.current;
            let frame = image
                .frames
                .get_mut(number - 1)
                .ok_or_else(|| Error::missing("the frame does not exist"))?;
            canvas::lay(
                Arc::make_mut(&mut frame.pixels).as_mut_slice(),
                size.0,
                rect,
                &decoded.pixels,
                rect.width,
                (0, 0),
                mode,
            )
            .ok_or_else(mismatch)?;
            if command.z != 0 {
                frame.gap = gap_of(command.z);
            }
            if number - 1 == current {
                image.changed(stamp, Some(rect));
            }
            return Ok(id);
        }
        if count >= IMAGE_LIMIT {
            return Err(Error::no_space("the image has as many frames as it may"));
        }
        let mut canvas = match usize::try_from(command.columns) {
            Ok(0) => canvas::filled(size, color_of(command.cell_y)),
            Ok(base) if base <= count => image
                .frames
                .get(base - 1)
                .map(|frame| frame.pixels.as_ref().clone())
                .ok_or_else(|| Error::missing("the base frame does not exist"))?,
            _ => return Err(Error::missing("the base frame does not exist")),
        };
        canvas::lay(
            &mut canvas,
            size.0,
            rect,
            &decoded.pixels,
            rect.width,
            (0, 0),
            mode,
        )
        .ok_or_else(mismatch)?;
        self.make_room(canvas.len(), generation)?;
        // The images that went for the room may have been in front of this one.
        let image = self
            .images
            .iter_mut()
            .find(|image| image.generation == generation)
            .ok_or_else(|| Self::missing_image(echo))?;
        self.bytes += canvas.len();
        image.frames.push(Frame {
            pixels: Arc::new(canvas),
            gap: match command.z {
                0 => DEFAULT_GAP,
                z => gap_of(z),
            },
        });
        Ok(id)
    }

    /// `a=c`: lays a rectangle of one frame on another.
    pub(super) fn compose_frames(&mut self, command: &Command, stamp: u64) -> Result<u32, Error> {
        let echo = command.echo;
        let at = self
            .index_of(echo)
            .ok_or_else(|| Self::missing_image(echo))?;
        let image = self
            .images
            .get_mut(at)
            .ok_or_else(|| Self::missing_image(echo))?;
        let size = (image.width, image.height);
        let count = image.frames.len();
        let frame_of = |number: u32, what: &str| {
            usize::try_from(number)
                .ok()
                .filter(|number| (1..=count).contains(number))
                .map(|number| number - 1)
                .ok_or_else(|| Error::missing(format!("the {what} frame does not exist")))
        };
        let source = frame_of(command.rows, "source")?;
        let target = frame_of(command.columns, "destination")?;
        let (width, height) = (
            if command.w == 0 { size.0 } else { command.w },
            if command.h == 0 { size.1 } else { command.h },
        );
        let to = Rect {
            x: command.x,
            y: command.y,
            width,
            height,
        };
        let from = Rect {
            x: command.cell_x,
            y: command.cell_y,
            width,
            height,
        };
        if !to.fits(size) {
            return Err(Error::invalid("the destination rectangle is out of bounds"));
        }
        if !from.fits(size) {
            return Err(Error::invalid("the source rectangle is out of bounds"));
        }
        if source == target && to.overlaps(from) {
            return Err(Error::invalid(
                "the source and destination rectangles of a frame overlap",
            ));
        }
        let mode = if command.still {
            Mode::Replace
        } else {
            Mode::Blend
        };
        // What is laid is held apart from the frame it lands on: another
        // frame's pixels are shared for the time, the frame's own rectangle
        // is copied out.
        let (over, over_width, start) = if source == target {
            let own = image
                .frames
                .get(source)
                .ok_or_else(|| Error::missing("the source frame does not exist"))?;
            let mut part =
                vec![0; usize::try_from(u64::from(width) * u64::from(height) * 4).unwrap_or(0)];
            canvas::lay(
                &mut part,
                width,
                Rect { x: 0, y: 0, ..from },
                &own.pixels,
                size.0,
                (from.x, from.y),
                Mode::Replace,
            )
            .ok_or_else(mismatch)?;
            (Arc::new(part), width, (0, 0))
        } else {
            let other = image
                .frames
                .get(source)
                .ok_or_else(|| Error::missing("the source frame does not exist"))?;
            (Arc::clone(&other.pixels), size.0, (from.x, from.y))
        };
        let current = image.current;
        let frame = image
            .frames
            .get_mut(target)
            .ok_or_else(|| Error::missing("the destination frame does not exist"))?;
        canvas::lay(
            Arc::make_mut(&mut frame.pixels).as_mut_slice(),
            size.0,
            to,
            &over,
            over_width,
            start,
            mode,
        )
        .ok_or_else(mismatch)?;
        if target == current {
            image.changed(stamp, Some(to));
        }
        Ok(image.id)
    }

    /// `d=f` and `d=F`: takes out the frame `r=` of the image the command
    /// names — the first when there is no `r=`, the last when it is past the
    /// last — and the frame after the first becomes the first. An image with the
    /// one frame loses nothing, but for the upper case, which takes the image.
    pub(super) fn delete_frames(&mut self, command: &Command, stamp: u64) {
        let Some(at) = self.index_of(command.echo) else {
            return;
        };
        let Some(image) = self.images.get_mut(at) else {
            return;
        };
        let count = image.frames.len();
        if count <= 1 {
            if command.free {
                self.remove(at);
                self.prune_relatives(true);
            }
            return;
        }
        let number = usize::try_from(command.rows)
            .unwrap_or(usize::MAX)
            .clamp(1, count);
        let gone = image.frames.remove(number - 1);
        let kept = image.frames.len();
        let removed = number - 1;
        if image.current >= kept {
            image.current = kept - 1;
            image.changed(stamp, None);
        } else if image.current > removed {
            image.current -= 1;
        } else if image.current == removed {
            image.changed(stamp, None);
        }
        self.bytes = self.bytes.saturating_sub(gone.pixels.len());
    }
}

impl Images {
    /// `a=f`: a frame of a stored image, from a decoded rectangle.
    pub(crate) fn frame(&mut self, screen: &Screen, command: &Command, decoded: Decoded) -> Reply {
        let stamp = self.next_stamp();
        let mut echo = command.echo;
        let reply = match self.screen(screen.alt).edit_frame(command, &decoded, stamp) {
            Ok(id) => {
                echo.id = id;
                Reply::ok(echo)
            }
            Err(error) => Reply::failed(echo, error),
        };
        reply.of_frame(command)
    }

    /// `a=c`: one frame's rectangle laid on another's.
    pub(crate) fn compose(&mut self, screen: &Screen, command: &Command) -> Reply {
        let stamp = self.next_stamp();
        let mut echo = command.echo;
        match self.screen(screen.alt).compose_frames(command, stamp) {
            Ok(id) => {
                echo.id = id;
                Reply::ok(echo)
            }
            Err(error) => Reply::failed(echo, error),
        }
    }
}
