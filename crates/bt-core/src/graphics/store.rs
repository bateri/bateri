//! The decoded images a screen holds, and where they are shown.
//!
//! The main and the alternate screen each have a store: the protocol ties an
//! image to the screen it was sent to, an alternate-screen program's images
//! go when it leaves, and the main screen's wait for it to come back. A
//! store also holds the screen's placements, which are what the frame draws;
//! how they keep their rows is [`super::follow`]'s.
//!
//! **A store is bounded three ways**, because what fills it is whatever a
//! program prints: the bytes of its pixels ([`QUOTA_BYTES`]), the number of
//! its images ([`IMAGE_LIMIT`]) and the number of its placements
//! ([`PLACEMENT_LIMIT`]). The quota alone lets in millions of one-pixel
//! images, and a put with no placement id adds a placement every time it is
//! repeated. What walks the lot (a store, a delete, every frame) looks through
//! each list once, not once for every entry of the other.
//!
//! **An image is its frames.** The first is the one that was transmitted and
//! the rest are what `a=f` and `a=c` made; each is kept whole — straight RGBA,
//! the size of the image — so that the one on show, an edit of it and a frame
//! composed on another are all the same kind of buffer ([`frames`]). Every one
//! counts against the quota, and an image keeps at most [`IMAGE_LIMIT`] of them.
//!
//! [`QUOTA_BYTES`]: super::QUOTA_BYTES

mod animation;
mod frames;

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::{Term, TermMode};

use super::canvas::Rect;
use super::command::{Action, Command, Scope};
use super::edits::Edits;
use super::follow::{self, Seen};
use super::place::{self, Motion, Parent, Picture, Placement, Screen, Spec};
use super::placeholder::{self, Marks, Run, Virtual};
use super::receive::Decoded;
use super::{Echo, Error, IMAGE_LIMIT, PLACEMENT_LIMIT, Reply};
use animation::Animation;

/// Ids the terminal hands out for an image sent by number (`I=`) start
/// here, far from the small ids programs choose themselves.
const AUTO_ID_BASE: u32 = 0x8000_0000;

/// The gap a frame is given when its command names none, milliseconds.
pub(super) const DEFAULT_GAP: u32 = 40;

/// How many placements a relative one may hang above: its parent, that one's
/// parent, and so on up to one that hangs on nothing.
const PARENT_DEPTH_LIMIT: usize = 8;

/// Where the cells of the virtual placements start on what a frame draws, by
/// the placement's `seq`: grid line and column of the first of its
/// placeholder cells that shows something.
type Anchors = HashMap<u64, (i32, i32)>;

/// Where the relative placements are on what a frame draws, by `seq`: the grid
/// line and the column of their first cell. Either can lie off the surface;
/// one whose parent has no place is not in it.
type Spots = HashMap<u64, (i32, i32)>;

/// One frame of an image: all of it, as straight RGBA.
pub(crate) struct Frame {
    /// RGBA, `width * height * 4` bytes, shared with whoever draws it.
    pub(crate) pixels: Arc<Vec<u8>>,
    /// The milliseconds the frame stays before the next one, when the
    /// animation runs; 0 for a frame that is passed over.
    pub(crate) gap: u32,
}

/// One stored image.
pub(crate) struct Image {
    /// The id programs address it by; 0 for an anonymous one.
    pub(crate) id: u32,
    /// The number it was sent with (`I=`); 0 for none.
    pub(crate) number: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// The frames, the transmitted image first; never none.
    pub(crate) frames: Vec<Frame>,
    /// The index of the frame on show.
    pub(crate) current: usize,
    /// Counts the images stored on this session, so a re-sent id is told
    /// apart from the image it replaced. Placements name their image by it, and
    /// it stays the image's for as long as the image is stored.
    pub(crate) generation: u64,
    /// Names the pixels on show: it is the generation when the image is stored
    /// and a number nothing else has had each time those pixels change (an
    /// edit of the frame on show, another frame shown). A texture cached under
    /// it never shows stale pixels, and nothing that places the image notices
    /// the change.
    pub(crate) revision: u64,
    /// Where those pixels changed lately, for a copy kept elsewhere that holds
    /// an older revision. Shared with the pictures that carry it, so a change
    /// while one is in a frame being drawn copies it (a few rectangles).
    pub(crate) edits: Arc<Edits>,
    /// Whether the terminal plays the frames by itself and how far it is.
    pub(crate) animation: Animation,
}

impl Image {
    /// The pixels on show changed in `rect` (all of the image when `None`) and
    /// are known as `stamp` from now on.
    pub(super) fn changed(&mut self, stamp: u64, rect: Option<Rect>) {
        let rect = rect.unwrap_or(Rect {
            x: 0,
            y: 0,
            width: self.width,
            height: self.height,
        });
        self.revision = stamp;
        Arc::make_mut(&mut self.edits).note(stamp, [rect.x, rect.y, rect.width, rect.height]);
    }

    fn bytes(&self) -> usize {
        self.frames.iter().map(|frame| frame.pixels.len()).sum()
    }

    /// The frame on show.
    pub(crate) fn shown(&self) -> Option<&Frame> {
        self.frames.get(self.current).or(self.frames.first())
    }
}

/// `images` by generation, for what looks one up for every placement: one
/// look through the images, not one for each.
fn by_generation(images: &[Image]) -> HashMap<u64, &Image> {
    images
        .iter()
        .map(|image| (image.generation, image))
        .collect()
}

/// The lines of a surface a frame draws and how they are numbered there.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Window {
    /// The first and the last grid line the surface shows.
    pub(crate) first: i32,
    pub(crate) last: i32,
    /// Added to a grid line, it is the surface's own row.
    pub(crate) shift: i32,
}

/// What one frame draws of a screen: the lines of the grid, those of the fill
/// band when it shows any, and the placeholder cells read from them.
pub(crate) struct View<'a> {
    pub(crate) grid: Window,
    pub(crate) band: Option<Window>,
    pub(crate) marks: &'a Marks,
}

/// One screen's images, oldest first, and their placements.
pub(crate) struct Store {
    images: Vec<Image>,
    bytes: usize,
    quota: usize,
    next_auto: u32,
    /// In the order they were made: the oldest, which goes past the limit,
    /// at the front.
    pub(super) placements: VecDeque<Placement>,
    /// The virtual placements (`U=1`), in the order they were made: boxes the
    /// cells that hold the placeholder character show an image through. They
    /// are on no line and no scroll, clear or resize moves or ends them.
    virtuals: VecDeque<Virtual>,
    next_seq: u64,
    /// Whether some placement is relative: where they are is looked for only
    /// then.
    has_relatives: bool,
    /// The main screen's rows as of the last look ([`follow`]).
    pub(super) seen: Option<Seen>,
    /// The alternate screen's rows as of the last look.
    pub(super) rows: Vec<usize>,
}

impl Store {
    fn new(quota: usize) -> Self {
        Self {
            images: Vec::new(),
            bytes: 0,
            quota,
            next_auto: AUTO_ID_BASE,
            placements: VecDeque::new(),
            virtuals: VecDeque::new(),
            next_seq: 0,
            has_relatives: false,
            seen: None,
            rows: Vec::new(),
        }
    }

    fn has(&self, id: u32) -> bool {
        self.images.iter().any(|image| image.id == id)
    }

    fn fresh_id(&mut self) -> u32 {
        loop {
            let id = self.next_auto;
            self.next_auto = id.checked_add(1).unwrap_or(AUTO_ID_BASE);
            if !self.has(id) {
                return id;
            }
        }
    }

    /// The images some placement shows, by generation: one look through the
    /// placements for all the images that ask. A virtual placement shows its
    /// image too: it is what the placeholders in the text will show.
    fn shown(&self) -> HashSet<u64> {
        self.placements
            .iter()
            .map(|placement| placement.image)
            .chain(self.virtuals.iter().map(|proto| proto.image))
            .collect()
    }

    /// Takes an image out, and its placements with it.
    fn remove(&mut self, index: usize) {
        let gone = self.images.remove(index);
        self.bytes -= gone.bytes();
        self.placements
            .retain(|placement| placement.image != gone.generation);
        self.virtuals.retain(|proto| proto.image != gone.generation);
    }

    /// Ends the relative placements whose parent is gone, and those hanging
    /// on them: a placement that hangs on another lives as long as it does.
    /// When `free`, an image that is left with no placement goes too, as in
    /// kitty, where the layout takes it out with its last placement.
    /// What takes an image out ([`Store::remove`]) is followed by this, for
    /// what hung on its placements. The generations of the images of the
    /// placements that ended are what it returns.
    fn prune_relatives(&mut self, free: bool) -> Vec<u64> {
        let mut ended = Vec::new();
        if !self.has_relatives {
            return ended;
        }
        loop {
            let alive: HashSet<u64> = self
                .placements
                .iter()
                .map(|placement| placement.seq)
                .chain(self.virtuals.iter().map(|proto| proto.seq))
                .collect();
            let before = self.placements.len();
            let mut lost = Vec::new();
            self.placements.retain(|placement| {
                let gone = placement
                    .parent
                    .is_some_and(|parent| !alive.contains(&parent.seq));
                if gone {
                    lost.push(placement.image);
                }
                !gone
            });
            if self.placements.len() == before {
                break;
            }
            if free {
                let shown = self.shown();
                for generation in &lost {
                    if shown.contains(generation) {
                        continue;
                    }
                    if let Some(index) = self
                        .images
                        .iter()
                        .position(|image| image.generation == *generation)
                    {
                        self.remove(index);
                    }
                }
            }
            ended.extend(lost);
        }
        self.has_relatives = self.placements.iter().any(Placement::is_relative);
        ended
    }

    /// What a put that names a parent (`P=`) hangs on, or why it cannot: the
    /// parent image has to be here with a placement of the id `Q=` (any one
    /// when it names none: the newest), and the chain above it may be
    /// [`PARENT_DEPTH_LIMIT`] placements long and cannot lead back to the
    /// placement `own`, which the put is making again.
    fn relate(&self, command: &Command, own: Option<u64>) -> Result<Option<Parent>, Error> {
        if !command.is_relative() {
            return Ok(None);
        }
        let Some(image) = self
            .images
            .iter()
            .find(|image| image.id == command.parent_image)
        else {
            return Err(Error::no_parent(format!(
                "the parent image with id {} does not exist",
                command.parent_image
            )));
        };
        let wanted = command.parent_placement;
        let parent = self
            .placements
            .iter()
            .map(|placement| (placement.image, placement.id, placement.seq))
            .chain(
                self.virtuals
                    .iter()
                    .map(|proto| (proto.image, proto.id, proto.seq)),
            )
            .filter(|&(shown, id, _)| shown == image.generation && (wanted == 0 || id == wanted))
            .map(|(_, _, seq)| seq)
            .max();
        let Some(parent) = parent else {
            return Err(Error::no_parent(format!(
                "the parent image with id {} has no placement with id {wanted}",
                command.parent_image
            )));
        };
        if own == Some(parent) {
            return Err(Error::invalid("a placement cannot be its own parent"));
        }
        let by_seq: HashMap<u64, &Placement> = self
            .placements
            .iter()
            .map(|placement| (placement.seq, placement))
            .collect();
        let mut at = parent;
        let mut above = 1;
        while let Some(link) = by_seq.get(&at).and_then(|placement| placement.parent) {
            if own == Some(link.seq) {
                return Err(Error::cycle(
                    "the parent reference leads back to the placement",
                ));
            }
            above += 1;
            if above > PARENT_DEPTH_LIMIT {
                return Err(Error::too_deep("too many levels of parent references"));
            }
            at = link.seq;
        }
        Ok(Some(Parent {
            seq: parent,
            offset: command.offset,
        }))
    }

    /// Keeps `image`, replacing a stored image of the same id (and ending its
    /// placements), and makes room — in bytes, and in images
    /// ([`IMAGE_LIMIT`]): images no placement shows go first, oldest first,
    /// then the oldest of the rest. The new image is never the one that goes.
    fn insert(&mut self, image: Image) -> Result<(), Error> {
        let size = image.bytes();
        if size > self.quota {
            return Err(Error::no_space("the image does not fit the storage quota"));
        }
        if image.id != 0
            && let Some(old) = self.images.iter().position(|kept| kept.id == image.id)
        {
            self.remove(old);
        }
        self.bytes += size;
        self.images.push(image);
        // Taken once: an image that goes unshown takes no placement with it,
        // and one that goes shown is not looked for again.
        let shown = self.shown();
        while self.bytes > self.quota || self.images.len() > IMAGE_LIMIT {
            let newest = self.images.len() - 1;
            let victim = self
                .images
                .iter()
                .take(newest)
                .position(|image| !shown.contains(&image.generation))
                .unwrap_or(0);
            self.remove(victim);
        }
        self.prune_relatives(true);
        Ok(())
    }

    /// The newest image with this number.
    fn numbered(&self, number: u32) -> Option<&Image> {
        self.images
            .iter()
            .rev()
            .find(|image| image.number == number)
    }

    /// The image a put names: by id, or else by number.
    fn named(&self, echo: Echo) -> Option<&Image> {
        if echo.id != 0 {
            self.images.iter().find(|image| image.id == echo.id)
        } else if echo.number != 0 {
            self.numbered(echo.number)
        } else {
            None
        }
    }

    /// Shows an image at the cursor. A placement of the same id on the same
    /// image is the one being moved, not a second one; one with no id is
    /// always another, and past [`PLACEMENT_LIMIT`] the oldest placement goes
    /// for it. Showing an image makes it the newest, so the quota goes after
    /// the ones nobody looked at. A virtual placement (`U=1`) is kept apart:
    /// it has no line and no column, only the box of cells its image will be
    /// fitted to. A relative one (`P=`) has no line and column either: it is
    /// where its parent is, wherever that is when the frame is drawn.
    fn place(
        &mut self,
        generation: u64,
        command: &Command,
        screen: &Screen,
        motion: Motion,
    ) -> Result<(), Error> {
        if command.unicode && command.is_relative() {
            return Err(Error::invalid(
                "a virtual placement cannot hang on a parent",
            ));
        }
        let Some(at) = self
            .images
            .iter()
            .position(|image| image.generation == generation)
        else {
            return Ok(());
        };
        let id = command.echo.placement;
        // A placement of this id is the one being moved, whichever kind it is.
        let moved = (id != 0)
            .then(|| {
                let same = |image: u64, own: u32| image == generation && own == id;
                self.placements
                    .iter()
                    .find(|placement| same(placement.image, placement.id))
                    .map(|placement| placement.seq)
                    .or_else(|| {
                        self.virtuals
                            .iter()
                            .find(|proto| same(proto.image, proto.id))
                            .map(|proto| proto.seq)
                    })
            })
            .flatten();
        let parent = self.relate(command, moved)?;
        let image = self.images.remove(at);
        let size = (image.width, image.height);
        self.images.push(image);
        if id != 0 {
            self.placements
                .retain(|placement| !(placement.image == generation && placement.id == id));
            self.virtuals
                .retain(|proto| !(proto.image == generation && proto.id == id));
        }
        self.next_seq += 1;
        let seq = self.next_seq;
        if let Some(moved) = moved {
            // What hung on the placement that was here hangs on this one.
            for placement in &mut self.placements {
                if let Some(link) = &mut placement.parent
                    && link.seq == moved
                {
                    link.seq = seq;
                }
            }
        }
        if command.unicode {
            self.virtuals.push_back(Virtual {
                seq,
                image: generation,
                id,
                columns: command.columns,
                rows: command.rows,
            });
            if self.virtuals.len() > PLACEMENT_LIMIT {
                self.virtuals.pop_front();
                self.prune_relatives(true);
            }
            return Ok(());
        }
        self.has_relatives |= parent.is_some();
        let (line, col) = match parent {
            Some(_) => (0, 0),
            None => (
                screen.cursor.0.saturating_sub(motion.above()),
                screen.cursor.1,
            ),
        };
        self.placements.push_back(Placement {
            seq,
            image: generation,
            id,
            line,
            col,
            spec: Spec::new(command, size),
            parent,
        });
        // The image of the one that goes stays, for the program to show again
        // or for the quota to take first.
        if self.placements.len() > PLACEMENT_LIMIT {
            self.placements.pop_front();
            self.prune_relatives(true);
        }
        Ok(())
    }

    /// Takes out the images no placement shows that `only` names — those
    /// the program can still address too when `free`, else only the
    /// anonymous ones, which nothing can address again.
    fn sweep(&mut self, free: bool, only: impl Fn(&Image) -> bool) {
        // What goes is shown by nothing: the placements stay as they are.
        let shown = self.shown();
        let mut index = self.images.len();
        while index > 0 {
            index -= 1;
            let image = &self.images[index];
            if !shown.contains(&image.generation) && (free || image.id == 0) && only(image) {
                self.remove(index);
            }
        }
        self.prune_relatives(true);
    }

    /// Removes what a delete command names. A delete ends the placements it
    /// matches; the image goes with its last placement when the command is
    /// in upper case, and an anonymous image (nothing can address it again)
    /// goes either way. Images the command does not name stay.
    fn delete(&mut self, command: &Command, screen: &Screen, stamp: u64) {
        if command.scope == Scope::Frames {
            self.delete_frames(command, stamp);
            return;
        }
        let cell = screen.cell;
        let echo = command.echo;
        let (line, col) = (
            i32::try_from(command.y).map_or(-1, |y| y - 1),
            i64::from(command.x) - 1,
        );
        let numbered = (command.scope == Scope::Number && echo.number != 0)
            .then(|| self.numbered(echo.number).map(|image| image.generation))
            .flatten();
        let in_range = |image: &Image| image.id != 0 && (command.x..=command.y).contains(&image.id);
        // A relative placement is on no line of its own: what a delete names
        // by place leaves it, and it ends with its parent. Its image stays
        // then, as it does for any placement a delete takes.
        let by_place = matches!(
            command.scope,
            Scope::Visible
                | Scope::Cursor
                | Scope::Cell
                | Scope::CellZ
                | Scope::Column
                | Scope::Row
        );
        let mut touched = HashSet::new();
        {
            let Self {
                images,
                placements,
                virtuals,
                ..
            } = self;
            let images = by_generation(images);
            let image_of = |generation: u64| images.get(&generation).copied();
            let named =
                |placement: &Placement| echo.placement == 0 || placement.id == echo.placement;
            // The virtual placements are on no cell and no layer: only a
            // delete that names their image reaches them.
            virtuals.retain(|proto| {
                let named = echo.placement == 0 || proto.id == echo.placement;
                let hit = match command.scope {
                    Scope::Id => {
                        echo.id != 0
                            && image_of(proto.image).is_some_and(|image| image.id == echo.id)
                            && named
                    }
                    Scope::Number => numbered == Some(proto.image) && named,
                    Scope::Range => image_of(proto.image).is_some_and(in_range),
                    _ => false,
                };
                if hit {
                    touched.insert(proto.image);
                }
                !hit
            });
            placements.retain(|placement| {
                if by_place && placement.is_relative() {
                    return true;
                }
                let covers_cell = || placement.covers(line, col, cell);
                let hit = match command.scope {
                    // What reaches onto the screen: everything but what has
                    // wholly gone up into the history.
                    Scope::Visible => placement.bottom(cell) >= 0,
                    Scope::Id => {
                        echo.id != 0
                            && image_of(placement.image).is_some_and(|image| image.id == echo.id)
                            && named(placement)
                    }
                    Scope::Number => numbered == Some(placement.image) && named(placement),
                    Scope::Range => image_of(placement.image).is_some_and(in_range),
                    Scope::Cursor => {
                        placement.covers(screen.cursor.0, i64::from(screen.cursor.1), cell)
                    }
                    Scope::Cell => covers_cell(),
                    Scope::CellZ => covers_cell() && placement.spec.z == command.z,
                    Scope::Column => placement.holds_column(col, cell),
                    Scope::Row => placement.holds_line(line, cell),
                    Scope::Z => placement.spec.z == command.z,
                    // Done above: it ends no placement.
                    Scope::Frames => false,
                };
                if hit {
                    touched.insert(placement.image);
                }
                !hit
            });
        }
        touched.extend(self.prune_relatives(!by_place));
        let free = command.free;
        match command.scope {
            Scope::Visible => self.sweep(free, |_| true),
            Scope::Id => self.sweep(free, |image| image.id != 0 && image.id == echo.id),
            Scope::Number => self.sweep(free, |image| Some(image.generation) == numbered),
            Scope::Range => self.sweep(free, in_range),
            _ => self.sweep(free, |image| touched.contains(&image.generation)),
        }
    }

    /// The screen was cleared (`CSI 2 J`, or `CSI 3 J` when `everything`):
    /// the placements that reach onto the screen go, or all of them. The
    /// images stay for the program to show again, but for those nothing can
    /// address.
    fn clear_placements(&mut self, screen: &Screen, everything: bool) {
        let cell = screen.cell;
        self.placements.retain(|placement| {
            placement.is_relative() || (!everything && placement.bottom(cell) < 0)
        });
        self.prune_relatives(false);
        self.sweep(false, |_| true);
    }

    /// The history above grid line `top` was erased and the lines from `top`
    /// are the screen's top now (⌘K): what shows on none of them goes, the
    /// rest moves up.
    fn wipe(&mut self, top: i32, cell: (u16, u16)) {
        self.placements
            .retain(|placement| placement.is_relative() || placement.bottom(cell) >= top);
        for placement in self.placements.iter_mut().filter(|p| !p.is_relative()) {
            placement.line = placement.line.saturating_sub(top);
        }
        self.prune_relatives(false);
        self.sweep(false, |_| true);
    }

    /// Moves every placement up `rows` lines (down when negative) and drops
    /// those left wholly above `top` or at or below `limit`.
    pub(super) fn lift(&mut self, rows: i32, top: i32, limit: Option<i32>, cell: (u16, u16)) {
        for placement in self.placements.iter_mut().filter(|p| !p.is_relative()) {
            placement.line = placement.line.saturating_sub(rows);
        }
        self.placements.retain(|placement| {
            placement.is_relative()
                || (placement.bottom(cell) >= top
                    && limit.is_none_or(|limit| placement.line < limit))
        });
        self.prune_relatives(false);
    }

    fn clear(&mut self) {
        self.images.clear();
        self.bytes = 0;
        self.placements.clear();
        self.virtuals.clear();
        self.has_relatives = false;
        self.seen = None;
        self.rows.clear();
    }

    /// The image and the virtual placement a run of placeholder cells names:
    /// the placement with the run's id, or the newest one the image has when
    /// the run names none.
    fn prototype(&self, run: &Run) -> Option<(&Image, &Virtual)> {
        let image = self.images.iter().find(|image| image.id == run.image)?;
        let proto = self.virtuals.iter().rev().find(|proto| {
            proto.image == image.generation && (run.placement == 0 || proto.id == run.placement)
        })?;
        Some((image, proto))
    }

    /// The generations of the images `runs` show, each once.
    fn marked<'a>(&self, runs: impl Iterator<Item = &'a Run>) -> Vec<u64> {
        let mut shown = Vec::new();
        for run in runs {
            if let Some((image, _)) = self.prototype(run)
                && !shown.contains(&image.generation)
            {
                shown.push(image.generation);
            }
        }
        shown
    }

    /// The pictures of the placeholder cells of `window`, a picture for each
    /// of the `runs` that has an image to show.
    fn collect_marked(
        &self,
        window: Window,
        cell: (u16, u16),
        runs: &[Run],
        out: &mut Vec<Picture>,
    ) {
        for run in runs {
            let Some((image, proto)) = self.prototype(run) else {
                continue;
            };
            let Some(shown) = image.shown() else {
                continue;
            };
            let size = (image.width, image.height);
            let Some(fitted) = placeholder::fit(run, cell, size, (proto.columns, proto.rows))
            else {
                continue;
            };
            out.push(Picture {
                col: run.col.saturating_add(fitted.col),
                row: run.line.saturating_add(window.shift),
                x_offset: fitted.x_offset,
                y_offset: fitted.y_offset,
                width: fitted.width,
                height: fitted.height,
                source: fitted.source,
                z: placeholder::LAYER,
                image: image.revision,
                key: image.generation,
                edits: Arc::clone(&image.edits),
                size,
                pixels: Arc::clone(&shown.pixels),
            });
        }
    }

    /// Where the cells of each virtual placement start: the top row and the
    /// left column of the placeholder cells of `marks` that show it. Only the
    /// placements something hangs on are looked for.
    fn anchors(&self, marks: &Marks, cell: (u16, u16)) -> Anchors {
        let mut anchors = Anchors::new();
        if !self.has_relatives {
            return anchors;
        }
        let hung_on: HashSet<u64> = self
            .placements
            .iter()
            .filter_map(|placement| placement.parent.map(|parent| parent.seq))
            .collect();
        for run in marks.all() {
            let Some((image, proto)) = self.prototype(run) else {
                continue;
            };
            if !hung_on.contains(&proto.seq)
                || placeholder::fit(
                    run,
                    cell,
                    (image.width, image.height),
                    (proto.columns, proto.rows),
                )
                .is_none()
            {
                continue;
            }
            let start = (run.line, i32::from(run.col));
            anchors
                .entry(proto.seq)
                .and_modify(|first| *first = (first.0.min(start.0), first.1.min(start.1)))
                .or_insert(start);
        }
        anchors
    }

    /// Where the relative placements are: each one's parent's place and its
    /// offsets from it, through all the placements it hangs above. One whose
    /// parent is nowhere on the surface (a virtual placement with no cell
    /// showing it) or that has more than [`PARENT_DEPTH_LIMIT`] above it has
    /// none.
    fn locate(&self, marks: &Marks, cell: (u16, u16)) -> Spots {
        if !self.has_relatives {
            return Spots::new();
        }
        let anchors = self.anchors(marks, cell);
        let by_seq: HashMap<u64, &Placement> = self
            .placements
            .iter()
            .map(|placement| (placement.seq, placement))
            .collect();
        let spot = |placement: &Placement| -> Option<(i32, i32)> {
            let mut link = placement.parent?;
            let (mut line, mut col) = (0i32, 0i32);
            for _ in 0..PARENT_DEPTH_LIMIT {
                line = line.saturating_add(link.offset.1);
                col = col.saturating_add(link.offset.0);
                let Some(above) = by_seq.get(&link.seq) else {
                    let (at_line, at_col) = anchors.get(&link.seq)?;
                    return Some((at_line.saturating_add(line), at_col.saturating_add(col)));
                };
                let Some(next) = above.parent else {
                    return Some((
                        above.line.saturating_add(line),
                        i32::from(above.col).saturating_add(col),
                    ));
                };
                link = next;
            }
            None
        };
        self.placements
            .iter()
            .filter(|placement| placement.is_relative())
            .filter_map(|placement| spot(placement).map(|at| (placement.seq, at)))
            .collect()
    }

    /// The pictures of the placements that reach into `window`, in drawing
    /// order: by layer, then by age.
    fn collect(
        &self,
        window: Window,
        cell: (u16, u16),
        columns: u16,
        spots: &Spots,
        out: &mut Vec<Picture>,
    ) {
        let mut shown: Vec<(&Placement, i32, i32)> =
            self.in_window(window, cell, columns, spots).collect();
        if shown.is_empty() {
            return;
        }
        shown.sort_by_key(|(placement, _, _)| (placement.spec.z, placement.seq));
        let images = by_generation(&self.images);
        for (placement, line, col) in shown {
            let Some(image) = images.get(&placement.image) else {
                continue;
            };
            let Some(shown) = image.shown() else {
                continue;
            };
            let spec = &placement.spec;
            let (mut width, height) = spec.dest_px(cell);
            let (mut x_offset, y_offset) = spec.offset_in(cell);
            let mut source = spec.source;
            if spec.source[2] == 0 || spec.source[3] == 0 || width == 0 || height == 0 {
                continue;
            }
            // A relative placement can start left of the surface: what is
            // there is cut off.
            if col < 0 {
                let Some((kept, rest)) = place::cut_left(
                    col.unsigned_abs(),
                    u32::from(cell.0),
                    x_offset,
                    width,
                    source,
                ) else {
                    continue;
                };
                (width, source, x_offset) = (kept, rest, 0);
            }
            out.push(Picture {
                col: u16::try_from(col.max(0)).unwrap_or(u16::MAX),
                row: line.saturating_add(window.shift),
                x_offset,
                y_offset,
                width,
                height,
                source,
                z: spec.z,
                image: image.revision,
                key: image.generation,
                edits: Arc::clone(&image.edits),
                size: (image.width, image.height),
                pixels: Arc::clone(&shown.pixels),
            });
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.images.len()
    }

    #[cfg(test)]
    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }

    #[cfg(test)]
    pub(crate) fn get(&self, id: u32) -> Option<&Image> {
        self.images.iter().find(|image| image.id == id)
    }

    #[cfg(test)]
    pub(crate) fn ids(&self) -> Vec<u32> {
        self.images.iter().map(|image| image.id).collect()
    }

    #[cfg(test)]
    pub(crate) fn placements(&self) -> &VecDeque<Placement> {
        &self.placements
    }

    /// Each virtual placement as (image id, placement id, columns, rows), in
    /// the order they were made.
    #[cfg(test)]
    pub(crate) fn virtuals(&self) -> Vec<(u32, u32, u32, u32)> {
        self.virtuals
            .iter()
            .map(|proto| {
                let id = self
                    .images
                    .iter()
                    .find(|image| image.generation == proto.image)
                    .map_or(0, |image| image.id);
                (id, proto.id, proto.columns, proto.rows)
            })
            .collect()
    }

    /// Each placement as (image id, line, column), in the order they were made.
    #[cfg(test)]
    pub(crate) fn placed(&self) -> Vec<(u32, i32, u16)> {
        self.placements
            .iter()
            .map(|placement| {
                let id = self
                    .images
                    .iter()
                    .find(|image| image.generation == placement.image)
                    .map_or(0, |image| image.id);
                (id, placement.line, placement.col)
            })
            .collect()
    }

    /// Shows the image `id` somewhere, so that the quota sees it as shown.
    #[cfg(test)]
    pub(crate) fn pin(&mut self, id: u32) {
        let Some(generation) = self.get(id).map(|image| image.generation) else {
            return;
        };
        self.next_seq += 1;
        let size = self
            .get(id)
            .map_or((1, 1), |image| (image.width, image.height));
        self.placements.push_back(Placement {
            seq: self.next_seq,
            image: generation,
            id: 0,
            line: 0,
            col: 0,
            spec: Spec::new(&Command::parse(b"").expect("an empty command"), size),
            parent: None,
        });
    }
}

/// Both screens' stores.
pub(crate) struct Images {
    main: Store,
    alt: Store,
    generation: u64,
}

impl Images {
    pub(crate) fn with_quota(quota: usize) -> Self {
        Self {
            main: Store::new(quota),
            alt: Store::new(quota),
            generation: 0,
        }
    }

    fn screen(&mut self, alt: bool) -> &mut Store {
        if alt { &mut self.alt } else { &mut self.main }
    }

    /// A number no image has been stored under and no pixels have been named
    /// by: the next generation, and the next revision.
    fn next_stamp(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }

    /// Stores a transmitted image on `screen` and answers the command; `a=T`
    /// shows it at the cursor too. An image sent by number gets an id of the
    /// terminal's choosing, and the answer says which.
    pub(crate) fn store(
        &mut self,
        screen: &Screen,
        command: &Command,
        decoded: Decoded,
        motion: Motion,
    ) -> Reply {
        let generation = self.next_stamp();
        let store = self.screen(screen.alt);
        let mut echo: Echo = command.echo;
        if echo.id == 0 && echo.number != 0 {
            echo.id = store.fresh_id();
        }
        let image = Image {
            id: echo.id,
            number: echo.number,
            width: decoded.width,
            height: decoded.height,
            frames: vec![Frame {
                pixels: Arc::new(decoded.pixels),
                gap: 0,
            }],
            current: 0,
            generation,
            revision: generation,
            edits: Arc::new(Edits::from(generation)),
            animation: Animation::default(),
        };
        match store.insert(image) {
            Ok(()) => {
                // The image is kept when the placement it came with is
                // refused, and the answer says why.
                if command.action == Action::TransmitAndPut
                    && let Err(error) = store.place(generation, command, screen, motion)
                {
                    return Reply::failed(echo, error);
                }
                Reply::ok(echo)
            }
            Err(error) => Reply::failed(echo, error),
        }
    }

    /// Shows a stored image at the cursor (`a=p`).
    pub(crate) fn put(&mut self, screen: &Screen, command: &Command, motion: Motion) -> Reply {
        let store = self.screen(screen.alt);
        let mut echo: Echo = command.echo;
        if command.unicode && command.is_relative() {
            return Reply::failed(
                echo,
                Error::invalid("a virtual placement cannot hang on a parent"),
            );
        }
        let Some(image) = store.named(echo) else {
            return Reply::failed(
                echo,
                Error::missing(format!(
                    "the image with id {} and number {} does not exist",
                    echo.id, echo.number
                )),
            );
        };
        let generation = image.generation;
        echo.id = image.id;
        match store.place(generation, command, screen, motion) {
            Ok(()) => Reply::ok(echo),
            Err(error) => Reply::failed(echo, error),
        }
    }

    pub(crate) fn delete(&mut self, screen: &Screen, command: &Command) {
        let stamp = self.next_stamp();
        self.screen(screen.alt).delete(command, screen, stamp);
    }

    /// The screen was cleared; see [`Store::clear_placements`].
    pub(crate) fn clear_screen(&mut self, screen: &Screen, everything: bool) {
        self.screen(screen.alt).clear_placements(screen, everything);
    }

    /// The terminal was reset: no image, no placement, on either screen.
    pub(crate) fn reset(&mut self) {
        self.main.clear();
        self.alt.clear();
    }

    /// The alternate screen was entered or left.
    pub(crate) fn drop_alt(&mut self) {
        self.alt.clear();
    }

    /// Edit ▸ Clear to Start: see [`Store::wipe`].
    pub(crate) fn wiped(&mut self, top: i32, cell: (u16, u16)) {
        self.main.wipe(top, cell);
    }

    /// The main screen was resized and its content moved `moved` lines up
    /// (down when negative) with the cursor; a change of width reflowed the
    /// rows and no line says where a placement went.
    pub(crate) fn resized(&mut self, moved: i32, reflowed: bool, cell: (u16, u16)) {
        if reflowed {
            self.main.placements.clear();
            self.main.prune_relatives(false);
        } else {
            self.main.lift(moved, i32::MIN, None, cell);
        }
        self.alt.placements.clear();
        self.alt.prune_relatives(false);
        self.alt.rows.clear();
    }

    /// Brings the active screen's placements to its grid as it is now.
    pub(crate) fn look<T>(&mut self, term: &Term<T>, cell: (u16, u16)) {
        if term.mode().contains(TermMode::ALT_SCREEN) {
            follow::follow_alt(&mut self.alt, term, cell);
        } else {
            follow::follow_main(&mut self.main, term, cell);
        }
    }

    /// Takes the active screen's look as it is now ([`follow`]).
    pub(crate) fn settle<T>(&mut self, term: &Term<T>) {
        if term.mode().contains(TermMode::ALT_SCREEN) {
            follow::settle_alt(&mut self.alt, term);
        } else {
            follow::settle_main(&mut self.main, term);
        }
    }

    /// Takes the main screen's look after something else than the active
    /// screen moved its placements.
    pub(crate) fn settle_main<T>(&mut self, term: &Term<T>) {
        follow::settle_main(&mut self.main, term);
    }

    /// The size of a stored image, from either screen.
    pub(crate) fn size_of(&self, echo: Echo) -> Option<(u32, u32)> {
        [&self.main, &self.alt]
            .into_iter()
            .find_map(|store| store.named(echo))
            .map(|image| (image.width, image.height))
    }

    /// Whether the screen shows anything at all: a placement, or a virtual
    /// one that the placeholders in its text may show.
    pub(crate) fn shows(&self, alt: bool) -> bool {
        let store = if alt { &self.alt } else { &self.main };
        !store.placements.is_empty() || !store.virtuals.is_empty()
    }

    /// Whether the screen has a virtual placement: no cell of its grid is
    /// looked at for a placeholder when it has none.
    pub(crate) fn holds_virtual(&self, alt: bool) -> bool {
        !(if alt { &self.alt } else { &self.main })
            .virtuals
            .is_empty()
    }

    /// Adds the pictures of the active screen's placements that reach into
    /// the view's grid lines and its band's, then those of its placeholder
    /// cells.
    pub(crate) fn collect(
        &self,
        alt: bool,
        cell: (u16, u16),
        columns: u16,
        view: &View,
        out: &mut super::place::Pictures,
    ) {
        let store = if alt { &self.alt } else { &self.main };
        let spots = store.locate(view.marks, cell);
        store.collect(view.grid, cell, columns, &spots, &mut out.grid);
        store.collect_marked(view.grid, cell, &view.marks.grid, &mut out.grid);
        if let Some(band) = view.band {
            store.collect(band, cell, columns, &spots, &mut out.band);
            if let Some(runs) = &view.marks.band {
                store.collect_marked(band, cell, runs, &mut out.band);
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn main(&self) -> &Store {
        &self.main
    }

    #[cfg(test)]
    pub(crate) fn alt(&self) -> &Store {
        &self.alt
    }

    #[cfg(test)]
    pub(crate) fn main_mut(&mut self) -> &mut Store {
        &mut self.main
    }
}

/// `Term`'s own size, for the places that need the screen's shape.
pub(crate) fn screen_of<T>(term: &Term<T>, cell: (u16, u16)) -> Screen {
    let cursor = term.grid().cursor.point;
    Screen {
        alt: term.mode().contains(TermMode::ALT_SCREEN),
        cursor: (
            cursor.line.0,
            u16::try_from(cursor.column.0).unwrap_or(u16::MAX),
        ),
        lines: u16::try_from(term.screen_lines()).unwrap_or(u16::MAX),
        columns: u16::try_from(term.columns()).unwrap_or(u16::MAX),
        cell,
    }
}
