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
//! [`QUOTA_BYTES`]: super::QUOTA_BYTES

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::{Term, TermMode};

use super::command::{Action, Command, Scope};
use super::follow::{self, Seen};
use super::place::{Motion, Picture, Placement, Screen, Spec};
use super::receive::Decoded;
use super::{Echo, Error, IMAGE_LIMIT, PLACEMENT_LIMIT, Reply};

/// Ids the terminal hands out for an image sent by number (`I=`) start
/// here, far from the small ids programs choose themselves.
const AUTO_ID_BASE: u32 = 0x8000_0000;

/// One stored image.
pub(crate) struct Image {
    /// The id programs address it by; 0 for an anonymous one.
    pub(crate) id: u32,
    /// The number it was sent with (`I=`); 0 for none.
    pub(crate) number: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// RGBA, shared with whoever draws it.
    pub(crate) pixels: Arc<Vec<u8>>,
    /// Counts the images stored on this session, so a re-sent id is told
    /// apart from the image it replaced. Placements name their image by it.
    pub(crate) generation: u64,
}

impl Image {
    fn bytes(&self) -> usize {
        self.pixels.len()
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

/// One screen's images, oldest first, and their placements.
pub(crate) struct Store {
    images: Vec<Image>,
    bytes: usize,
    quota: usize,
    next_auto: u32,
    /// In the order they were made: the oldest, which goes past the limit,
    /// at the front.
    pub(super) placements: VecDeque<Placement>,
    next_seq: u64,
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
            next_seq: 0,
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
    /// placements for all the images that ask.
    fn shown(&self) -> HashSet<u64> {
        self.placements
            .iter()
            .map(|placement| placement.image)
            .collect()
    }

    /// Takes an image out, and its placements with it.
    fn remove(&mut self, index: usize) {
        let gone = self.images.remove(index);
        self.bytes -= gone.bytes();
        self.placements
            .retain(|placement| placement.image != gone.generation);
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
    /// the ones nobody looked at.
    fn place(&mut self, generation: u64, command: &Command, screen: &Screen, motion: Motion) {
        let Some(at) = self
            .images
            .iter()
            .position(|image| image.generation == generation)
        else {
            return;
        };
        let image = self.images.remove(at);
        let size = (image.width, image.height);
        self.images.push(image);
        let id = command.echo.placement;
        if id != 0 {
            self.placements
                .retain(|placement| !(placement.image == generation && placement.id == id));
        }
        self.next_seq += 1;
        self.placements.push_back(Placement {
            seq: self.next_seq,
            image: generation,
            id,
            line: screen.cursor.0.saturating_sub(motion.above()),
            col: screen.cursor.1,
            spec: Spec::new(command, size),
        });
        // The image of the one that goes stays, for the program to show again
        // or for the quota to take first.
        if self.placements.len() > PLACEMENT_LIMIT {
            self.placements.pop_front();
        }
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
    }

    /// Removes what a delete command names. A delete ends the placements it
    /// matches; the image goes with its last placement when the command is
    /// in upper case, and an anonymous image (nothing can address it again)
    /// goes either way. Images the command does not name stay.
    fn delete(&mut self, command: &Command, screen: &Screen) {
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
        let mut touched = Vec::new();
        {
            let Self {
                images, placements, ..
            } = self;
            let images = by_generation(images);
            let image_of = |generation: u64| images.get(&generation).copied();
            let named =
                |placement: &Placement| echo.placement == 0 || placement.id == echo.placement;
            placements.retain(|placement| {
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
                    Scope::Frames => false,
                };
                if hit {
                    touched.push(placement.image);
                }
                !hit
            });
        }
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
        self.placements
            .retain(|placement| !everything && placement.bottom(cell) < 0);
        self.sweep(false, |_| true);
    }

    /// The history above grid line `top` was erased and the lines from `top`
    /// are the screen's top now (⌘K): what shows on none of them goes, the
    /// rest moves up.
    fn wipe(&mut self, top: i32, cell: (u16, u16)) {
        self.placements
            .retain(|placement| placement.bottom(cell) >= top);
        for placement in &mut self.placements {
            placement.line = placement.line.saturating_sub(top);
        }
        self.sweep(false, |_| true);
    }

    /// Moves every placement up `rows` lines (down when negative) and drops
    /// those left wholly above `top` or at or below `limit`.
    pub(super) fn lift(&mut self, rows: i32, top: i32, limit: Option<i32>, cell: (u16, u16)) {
        for placement in &mut self.placements {
            placement.line = placement.line.saturating_sub(rows);
        }
        self.placements.retain(|placement| {
            placement.bottom(cell) >= top && limit.is_none_or(|limit| placement.line < limit)
        });
    }

    fn clear(&mut self) {
        self.images.clear();
        self.bytes = 0;
        self.placements.clear();
        self.seen = None;
        self.rows.clear();
    }

    /// The pictures of the placements that reach into `window`, in drawing
    /// order: by layer, then by age.
    fn collect(&self, window: Window, cell: (u16, u16), columns: u16, out: &mut Vec<Picture>) {
        let mut shown: Vec<&Placement> = self
            .placements
            .iter()
            .filter(|placement| {
                placement.line <= window.last
                    && placement.bottom(cell) >= window.first
                    && placement.col < columns
            })
            .collect();
        if shown.is_empty() {
            return;
        }
        shown.sort_by_key(|placement| (placement.spec.z, placement.seq));
        let images = by_generation(&self.images);
        for placement in shown {
            let Some(image) = images.get(&placement.image) else {
                continue;
            };
            let spec = &placement.spec;
            let (width, height) = spec.dest_px(cell);
            let (x_offset, y_offset) = spec.offset_in(cell);
            if spec.source[2] == 0 || spec.source[3] == 0 || width == 0 || height == 0 {
                continue;
            }
            out.push(Picture {
                col: placement.col,
                row: placement.line.saturating_add(window.shift),
                x_offset,
                y_offset,
                width,
                height,
                source: spec.source,
                z: spec.z,
                image: image.generation,
                size: (image.width, image.height),
                pixels: Arc::clone(&image.pixels),
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
        self.generation += 1;
        let generation = self.generation;
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
            pixels: Arc::new(decoded.pixels),
            generation,
        };
        match store.insert(image) {
            Ok(()) => {
                if command.action == Action::TransmitAndPut {
                    store.place(generation, command, screen, motion);
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
        store.place(generation, command, screen, motion);
        Reply::ok(echo)
    }

    pub(crate) fn delete(&mut self, screen: &Screen, command: &Command) {
        self.screen(screen.alt).delete(command, screen);
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
        } else {
            self.main.lift(moved, i32::MIN, None, cell);
        }
        self.alt.placements.clear();
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

    /// Whether the screen shows anything at all.
    pub(crate) fn shows(&self, alt: bool) -> bool {
        !(if alt { &self.alt } else { &self.main })
            .placements
            .is_empty()
    }

    /// Adds the pictures of the active screen's placements that reach into
    /// the grid's `window` and the band's.
    pub(crate) fn collect(
        &self,
        alt: bool,
        cell: (u16, u16),
        columns: u16,
        grid: Window,
        band: Option<Window>,
        out: &mut super::place::Pictures,
    ) {
        let store = if alt { &self.alt } else { &self.main };
        store.collect(grid, cell, columns, &mut out.grid);
        if let Some(band) = band {
            store.collect(band, cell, columns, &mut out.band);
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
