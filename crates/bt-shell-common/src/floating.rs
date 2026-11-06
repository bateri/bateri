//! Floating panes: the **pure** model of a tab that is two things — the flat area (the split
//! tree, [`crate::split::Tree`]) and a list of floating cards standing over it. A card is a small
//! tree of its own (a floating pane can be split), the share of the tab's content area it takes
//! ([`Area`]) and whether it is minimized. Everything a rule says lives here, in std only: the
//! one invariant, the two sizes, the clamping into the tab, the edges a pointer grips, the dock
//! place, the order the keyboard walks and the two minimize keys. The AppKit part (`split_view`)
//! only applies the frames this module answers with.
//!
//! **The one invariant: the flat area always holds a pane.** It is true by type — the flat part
//! is a [`Tree`], which is never empty — and every operation keeps it true: the flat area's
//! only pane cannot float ([`Layers::float`]), and when it closes the front card comes down
//! whole and its tree *is* the flat area's tree ([`Layers::remove`]). So "the last visible pane
//! cannot be minimized" never arises, and the tab closes only with its last pane, floating ones
//! included.
//!
//! **Order is the data.** [`Layers::cards`] runs from the back to the front. A card that is
//! raised (clicked, given the keyboard, opened from its capsule) goes to the end; a card that is
//! minimized goes to the end too, so among the minimized cards the list order *is* the order they
//! were minimized in — the capsules run in it, the newest at the right — and the session file
//! (`restore`, one `F` line per card, back to front) needs no field for it.
//!
//! **Sizes are shares.** An [`Area`] is `[x, y, width, height]` as fractions of the content area,
//! so a window that is resized keeps every card's place and proportions. The least a card can be
//! is the least its tree can be (`split::Tree::min_length`), asked from a [`Ground`]; a card that
//! the window has made too small is shown at the least and keeps its share.

use crate::split::{Direction, Layout, Rect, Removal, Room, Size, Spacing, Tree, Verdict};

/// How much of the content area a new floating pane takes: 55% wide, 60% tall, in the middle
/// (measured in Rex Beta 1829). A design constant.
pub const NEW_SHARE: (f64, f64) = (0.55, 0.60);

/// How much of the place it stood in a pane that floats takes: four fifths of it each way, in
/// its middle. A design constant.
pub const FLOATED_SHARE: f64 = 0.80;

/// The tolerance of the geometry's comparisons.
const EPSILON: f64 = 1e-9;

/// The fraction of a tab's content area a card takes: left, top, width and height, each 0–1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// An edge or a corner of a card a pointer takes hold of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grip {
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Grip {
    fn west(self) -> bool {
        matches!(self, Self::Left | Self::TopLeft | Self::BottomLeft)
    }

    fn east(self) -> bool {
        matches!(self, Self::Right | Self::TopRight | Self::BottomRight)
    }

    fn north(self) -> bool {
        matches!(self, Self::Top | Self::TopLeft | Self::TopRight)
    }

    fn south(self) -> bool {
        matches!(self, Self::Bottom | Self::BottomLeft | Self::BottomRight)
    }
}

impl Area {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// `width` × `height` in the middle of the content area.
    pub fn centred(width: f64, height: f64) -> Self {
        Self::new((1.0 - width) / 2.0, (1.0 - height) / 2.0, width, height)
    }

    /// Where a new floating pane opens ([`NEW_SHARE`]).
    pub fn opening() -> Self {
        Self::centred(NEW_SHARE.0, NEW_SHARE.1)
    }

    /// Where a pane that stood in `place` floats to: [`FLOATED_SHARE`] of it, in its middle.
    pub fn floated_from(place: Area) -> Self {
        let (width, height) = (place.width * FLOATED_SHARE, place.height * FLOATED_SHARE);
        let (cx, cy) = place.centre();
        Self::new(cx - width / 2.0, cy - height / 2.0, width, height)
    }

    /// An area from whole percents of the content area (`X,Y,W,H` of a command).
    pub fn percent(x: u32, y: u32, width: u32, height: u32) -> Self {
        let share = |value: u32| f64::from(value) / 100.0;
        Self::new(share(x), share(y), share(width), share(height))
    }

    pub fn centre(self) -> (f64, f64) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    /// The area in points, inside `bounds`.
    pub fn rect(self, bounds: Rect) -> Rect {
        Rect::new(
            bounds.x + self.x * bounds.width,
            bounds.y + self.y * bounds.height,
            self.width * bounds.width,
            self.height * bounds.height,
        )
    }

    /// The share `rect` takes of `bounds`; the whole area when `bounds` has no size.
    pub fn of(rect: Rect, bounds: Rect) -> Self {
        if bounds.width <= 0.0 || bounds.height <= 0.0 {
            return Self::new(0.0, 0.0, 1.0, 1.0);
        }
        Self::new(
            (rect.x - bounds.x) / bounds.width,
            (rect.y - bounds.y) / bounds.height,
            rect.width / bounds.width,
            rect.height / bounds.height,
        )
    }

    /// This area as a command wished it (`--at X,Y`, `--size W,H`; fractions): `self` is what
    /// the wish leaves out. `at` is the top-left corner; a size alone keeps the middle where it
    /// was. Kept inside the tab and no smaller than `least`.
    pub fn wished(
        self,
        at: Option<(f64, f64)>,
        size: Option<(f64, f64)>,
        least: (f64, f64),
    ) -> Self {
        let (width, height) = size.unwrap_or((self.width, self.height));
        let (x, y) = at.unwrap_or_else(|| {
            let (cx, cy) = self.centre();
            (cx - width / 2.0, cy - height / 2.0)
        });
        Self::new(x, y, width, height).within(least)
    }

    /// The area inside the tab and no smaller than `least` (width, height; a least above the
    /// whole is the whole): too large is cut down, outside is pulled in.
    pub fn within(self, least: (f64, f64)) -> Self {
        let (least_width, least_height) = (least.0.clamp(0.0, 1.0), least.1.clamp(0.0, 1.0));
        let width = self.width.clamp(least_width, 1.0);
        let height = self.height.clamp(least_height, 1.0);
        Self::new(
            self.x.clamp(0.0, 1.0 - width),
            self.y.clamp(0.0, 1.0 - height),
            width,
            height,
        )
    }

    /// The same size moved by `dx`, `dy` (fractions), kept inside the tab.
    pub fn moved(self, dx: f64, dy: f64, least: (f64, f64)) -> Self {
        let kept = self.within(least);
        Self::new(kept.x + dx, kept.y + dy, kept.width, kept.height).within(least)
    }

    /// The area with the edges `grip` holds moved by `dx`, `dy` (fractions): an edge stops at
    /// the tab's edge and at `least` from the opposite one, the opposite edge never moves.
    pub fn dragged(self, grip: Grip, dx: f64, dy: f64, least: (f64, f64)) -> Self {
        let kept = self.within(least);
        let (least_width, least_height) = (least.0.clamp(0.0, 1.0), least.1.clamp(0.0, 1.0));
        let (mut left, mut top) = (kept.x, kept.y);
        let (mut right, mut bottom) = (kept.x + kept.width, kept.y + kept.height);
        if grip.west() {
            left = (left + dx).clamp(0.0, right - least_width);
        }
        if grip.east() {
            right = (right + dx).clamp(left + least_width, 1.0);
        }
        if grip.north() {
            top = (top + dy).clamp(0.0, bottom - least_height);
        }
        if grip.south() {
            bottom = (bottom + dy).clamp(top + least_height, 1.0);
        }
        Self::new(left, top, right - left, bottom - top)
    }
}

/// The edge or corner of `card` (the card's frame, in points) that a pointer at `point` grips,
/// if any: within `slop` points of an edge, on either side of it — a handle straddles the border.
/// Near two edges at once it is the corner. Inside the card, away from the edges, it is `None`:
/// the pointer is on the card.
pub fn grip_at(card: Rect, point: (f64, f64), slop: f64) -> Option<Grip> {
    let (x, y) = point;
    let (right, bottom) = (card.x + card.width, card.y + card.height);
    if x < card.x - slop || x > right + slop || y < card.y - slop || y > bottom + slop {
        return None;
    }
    let west = (x - card.x).abs() <= slop;
    let east = (x - right).abs() <= slop;
    let north = (y - card.y).abs() <= slop;
    let south = (y - bottom).abs() <= slop;
    // A card narrower than two handles gives the nearer edge, not both.
    let (west, east) = if west && east {
        let nearer_west = (x - card.x).abs() <= (x - right).abs();
        (nearer_west, !nearer_west)
    } else {
        (west, east)
    };
    let (north, south) = if north && south {
        let nearer_north = (y - card.y).abs() <= (y - bottom).abs();
        (nearer_north, !nearer_north)
    } else {
        (north, south)
    };
    match (west, east, north, south) {
        (true, _, true, _) => Some(Grip::TopLeft),
        (_, true, true, _) => Some(Grip::TopRight),
        (true, _, _, true) => Some(Grip::BottomLeft),
        (_, true, _, true) => Some(Grip::BottomRight),
        (true, ..) => Some(Grip::Left),
        (_, true, ..) => Some(Grip::Right),
        (_, _, true, _) => Some(Grip::Top),
        (_, _, _, true) => Some(Grip::Bottom),
        _ => None,
    }
}

/// One floating card.
#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    /// The card's panes; a floating pane can be split like any other.
    pub tree: Tree,
    /// The share of the tab's content area it takes.
    pub area: Area,
    /// Shown as a capsule: the programs run on, nothing is drawn.
    pub minimized: bool,
}

impl Card {
    /// A card of one pane.
    pub fn of(pane: u64, area: Area) -> Self {
        Self {
            tree: Tree::Leaf(pane),
            area,
            minimized: false,
        }
    }
}

/// What the keyboard does after a rule moved a card away from where it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    /// Stays where it is.
    Keep,
    /// Goes to this pane.
    Pane(u64),
    /// Goes back to where it was before (the tab's `handover` memory); [`Layers::resting`] when
    /// there is no memory.
    Back,
}

/// What closing a pane did ([`Layers::remove`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gone {
    /// The pane left its place; its layer goes on. The keyboard goes to `focus`, a neighbour in
    /// the same tree, or back to where it was when `None` — the pane was its card's last and
    /// the card is gone.
    Closed { focus: Option<u64> },
    /// The flat area's last pane closed and the front card came down whole: its tree is the flat
    /// area's now. The keyboard goes to `focus`, the first pane of that tree.
    Landed { focus: u64 },
    /// The tab's only pane: the tab closes.
    Last,
    /// The pane is not in the tab.
    Missing,
}

/// The ground a question about the geometry stands on, as [`Room`] is for a tree: the content
/// area (points), the window's scale, the spacing of the flat area and of the inside of a card,
/// and the smallest pane per pane identity.
pub struct Ground<'a> {
    /// The tab's content area, in points: the flat area is laid out in it and a card's [`Area`]
    /// is a share of it.
    pub bounds: Rect,
    pub scale: f64,
    /// The spacing of the flat area.
    pub flat: Spacing,
    /// The spacing of the panes inside one card.
    pub card: Spacing,
    pub min: &'a dyn Fn(u64) -> Size,
}

impl<'a> Ground<'a> {
    fn flat_room(&self) -> Room<'a> {
        Room {
            bounds: self.bounds,
            scale: self.scale,
            spacing: self.flat,
            min: self.min,
        }
    }

    fn card_room(&self, bounds: Rect) -> Room<'a> {
        Room {
            bounds,
            scale: self.scale,
            spacing: self.card,
            min: self.min,
        }
    }

    /// The least a card holding `tree` can be, as shares of the content area (width, height):
    /// what its tree needs to keep every pane at its smallest, the margins of the card included.
    pub fn least(&self, tree: &Tree) -> (f64, f64) {
        let room = self.card_room(self.bounds);
        let wide = tree.min_length(crate::split::Axis::Horizontal, &room)
            + self.card.left_px()
            + self.card.around_px();
        let tall = tree.min_length(crate::split::Axis::Vertical, &room)
            + self.card.top_px()
            + self.card.around_px();
        let share = |pixels: f64, whole: f64| {
            if whole <= 0.0 {
                1.0
            } else {
                (pixels / self.scale / whole).min(1.0)
            }
        };
        (
            share(wide, self.bounds.width),
            share(tall, self.bounds.height),
        )
    }

    /// The card's frame in points: its share, at least what its tree needs.
    pub fn rect_of(&self, card: &Card) -> Rect {
        card.area.within(self.least(&card.tree)).rect(self.bounds)
    }
}

/// A tab laid out ([`Layers::layout`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Arranged {
    pub flat: Layout,
    /// Each card's frame and its panes' frames, in [`Layers::cards`] order. A minimized card is
    /// laid out too (its place is where it opens again).
    pub cards: Vec<(Rect, Layout)>,
}

/// A tab's two layers: the flat area and the floating cards.
#[derive(Clone, Debug, PartialEq)]
pub struct Layers {
    /// The flat area.
    pub flat: Tree,
    /// The cards, back to front.
    pub cards: Vec<Card>,
}

impl Layers {
    /// A tab with a flat area and no card.
    pub fn flat(flat: Tree) -> Self {
        Self {
            flat,
            cards: Vec::new(),
        }
    }

    /// Every pane of the tab: the flat area's in tree order, then each card's, back to front.
    pub fn panes(&self) -> Vec<u64> {
        let mut out = self.flat.leaves();
        for card in &self.cards {
            out.extend(card.tree.leaves());
        }
        out
    }

    /// The index of the card holding `pane`.
    pub fn card_of(&self, pane: u64) -> Option<usize> {
        self.cards
            .iter()
            .position(|card| card.tree.leaves().contains(&pane))
    }

    /// Whether `pane` is in a card.
    pub fn floating(&self, pane: u64) -> bool {
        self.card_of(pane).is_some()
    }

    /// The front card: the front-most that is shown, else — when every card is a capsule — the
    /// one minimized last.
    pub fn front(&self) -> Option<usize> {
        self.cards
            .iter()
            .rposition(|card| !card.minimized)
            .or_else(|| self.cards.len().checked_sub(1))
    }

    /// Where the keyboard goes when it has nowhere to go back to: the front shown card's first
    /// pane, else the flat area's.
    pub fn resting(&self) -> u64 {
        match self.cards.iter().rposition(|card| !card.minimized) {
            Some(index) => self.cards[index].tree.leaves()[0],
            None => self.flat.leaves()[0],
        }
    }

    /// The capsules, in the order the cards were minimized: the first pane of each minimized
    /// card (its icon and title are the capsule's). The newest is last — at the right.
    pub fn capsules(&self) -> Vec<u64> {
        self.cards
            .iter()
            .filter(|card| card.minimized)
            .map(|card| card.tree.leaves()[0])
            .collect()
    }

    /// Splits `target`, wherever it is, in two along `axis`: `new` takes the second half.
    pub fn split(&mut self, target: u64, axis: crate::split::Axis, new: u64) -> bool {
        self.flat.split(target, axis, new)
            || self
                .cards
                .iter_mut()
                .any(|card| card.tree.split(target, axis, new))
    }

    /// The tab laid out in `ground`.
    pub fn layout(&self, ground: &Ground<'_>) -> Arranged {
        Arranged {
            flat: self
                .flat
                .layout_spaced(ground.bounds, ground.scale, ground.flat),
            cards: self
                .cards
                .iter()
                .map(|card| {
                    let rect = ground.rect_of(card);
                    let inner = card.tree.layout_spaced(rect, ground.scale, ground.card);
                    (rect, inner)
                })
                .collect(),
        }
    }

    /// A new floating pane `pane` in front, as the wish says (`None`: the opening place and size,
    /// [`NEW_SHARE`]). `false` and no change when the tab already holds `pane`.
    pub fn open(
        &mut self,
        pane: u64,
        at: Option<(f64, f64)>,
        size: Option<(f64, f64)>,
        ground: &Ground<'_>,
    ) -> bool {
        if self.panes().contains(&pane) {
            return false;
        }
        let area = Area::opening().wished(at, size, ground.least(&Tree::Leaf(pane)));
        self.cards.push(Card::of(pane, area));
        true
    }

    /// Floats flat pane `pane` as a card of its own, in front, at `area`. The flat area's only
    /// pane cannot float: `false`, like for a pane that is not flat; nothing changes.
    pub fn float(&mut self, pane: u64, area: Area) -> bool {
        if self.flat.leaves().len() < 2
            || !matches!(self.flat.remove(pane), Removal::Removed { .. })
        {
            return false;
        }
        self.cards.push(Card::of(pane, area));
        true
    }

    /// [`Layers::float`] where the pane's place makes it float to: [`FLOATED_SHARE`] of the place
    /// it stands in, in its middle, as a wish (`--at`, `--size`) adjusts it.
    pub fn float_out(
        &mut self,
        pane: u64,
        at: Option<(f64, f64)>,
        size: Option<(f64, f64)>,
        ground: &Ground<'_>,
    ) -> bool {
        let layout = self
            .flat
            .layout_spaced(ground.bounds, ground.scale, ground.flat);
        let Some((_, place)) = layout.panes.iter().find(|(id, _)| *id == pane) else {
            return false;
        };
        let base = Area::floated_from(Area::of(*place, ground.bounds));
        let area = base.wished(at, size, ground.least(&Tree::Leaf(pane)));
        self.float(pane, area)
    }

    /// Docks floating pane `pane` into the flat area. It leaves its card (the card's other panes
    /// go on floating; an emptied card is gone). Its place is asked of the one decision a drop
    /// stands on — `Tree::verdict` with the pane's centre as the pointer. Where that gives no
    /// place (no room there, or none at all) the pane goes beside `beside`, the flat pane the
    /// keyboard was in, on the right, else below, left, above, the drop climbing to a larger
    /// group where the pane is too small. `false` and no change when there is room nowhere, or
    /// `pane` is not floating.
    pub fn dock(&mut self, pane: u64, beside: u64, ground: &Ground<'_>) -> bool {
        let Some(index) = self.card_of(pane) else {
            return false;
        };
        let arranged = self.layout(ground);
        let centre = arranged.cards[index]
            .1
            .panes
            .iter()
            .find(|(id, _)| *id == pane)
            .map_or_else(
                || arranged.cards[index].0.center(),
                |(_, frame)| frame.center(),
            );
        let room = ground.flat_room();
        let incoming = Tree::Leaf(pane);
        let placed = match self.flat.verdict(&incoming, &room, centre) {
            Verdict::Lands { placement, .. } => Some(placement.tree),
            _ => {
                let target = if self.flat.leaves().contains(&beside) {
                    beside
                } else {
                    self.flat.leaves()[0]
                };
                [
                    Direction::Right,
                    Direction::Down,
                    Direction::Left,
                    Direction::Up,
                ]
                .into_iter()
                .find_map(|side| self.flat.plan_beside(target, side, &incoming, &room))
                .map(|placement| placement.tree)
            }
        };
        let Some(tree) = placed else {
            return false;
        };
        match self.cards[index].tree.remove(pane) {
            Removal::Removed { .. } => {}
            Removal::Last => {
                self.cards.remove(index);
            }
            Removal::Missing => return false,
        }
        self.flat = tree;
        true
    }

    /// Closes `pane`, wherever it is (see [`Gone`]).
    pub fn remove(&mut self, pane: u64) -> Gone {
        if let Some(index) = self.card_of(pane) {
            return match self.cards[index].tree.remove(pane) {
                Removal::Removed { focus } => Gone::Closed { focus: Some(focus) },
                Removal::Last => {
                    self.cards.remove(index);
                    Gone::Closed { focus: None }
                }
                Removal::Missing => Gone::Missing,
            };
        }
        match self.flat.remove(pane) {
            Removal::Removed { focus } => Gone::Closed { focus: Some(focus) },
            Removal::Missing => Gone::Missing,
            Removal::Last => match self.front() {
                None => Gone::Last,
                Some(index) => {
                    let card = self.cards.remove(index);
                    let focus = card.tree.leaves()[0];
                    self.flat = card.tree;
                    Gone::Landed { focus }
                }
            },
        }
    }

    /// Brings the card holding `pane` to the front and opens it if it was a capsule. `false`
    /// when `pane` is not floating. What a click on a card, the keyboard arriving in one and a
    /// capsule do.
    pub fn reveal(&mut self, pane: u64) -> bool {
        let Some(index) = self.card_of(pane) else {
            return false;
        };
        let mut card = self.cards.remove(index);
        card.minimized = false;
        self.cards.push(card);
        true
    }

    /// Minimizes the card holding `pane`: it goes to the end of the list, the newest capsule.
    /// `false` when `pane` is not floating or its card is a capsule already.
    pub fn minimize(&mut self, pane: u64) -> bool {
        let Some(index) = self.card_of(pane) else {
            return false;
        };
        if self.cards[index].minimized {
            return false;
        }
        let mut card = self.cards.remove(index);
        card.minimized = true;
        self.cards.push(card);
        true
    }

    /// ⌃⌘M with the keyboard in `focus`: in a floating pane its card is minimized and the
    /// keyboard goes back; in the flat area the card minimized last is opened and takes the
    /// keyboard. `None` when there is nothing to do (no minimized card).
    pub fn minimize_key(&mut self, focus: u64) -> Option<Focus> {
        if self.floating(focus) {
            return self.minimize(focus).then_some(Focus::Back);
        }
        let index = self.cards.iter().rposition(|card| card.minimized)?;
        let first = self.cards[index].tree.leaves()[0];
        self.reveal(first);
        Some(Focus::Pane(first))
    }

    /// ⌥⇧⌘↩ with the keyboard in `focus`: when a card is shown, all of them are minimized (back
    /// to front, so the front one is the newest capsule) and a keyboard that was in one goes
    /// back; when none is shown, all of the capsules open, each where it was. `None` when the
    /// tab has no card.
    pub fn hide_key(&mut self, focus: u64) -> Option<Focus> {
        if self.cards.is_empty() {
            return None;
        }
        if self.cards.iter().all(|card| card.minimized) {
            for card in &mut self.cards {
                card.minimized = false;
            }
            return Some(Focus::Keep);
        }
        let inside = self.floating(focus);
        let (mut capsules, mut shown): (Vec<Card>, Vec<Card>) = std::mem::take(&mut self.cards)
            .into_iter()
            .partition(|card| card.minimized);
        for card in &mut shown {
            card.minimized = true;
        }
        capsules.extend(shown);
        self.cards = capsules;
        Some(if inside { Focus::Back } else { Focus::Keep })
    }

    /// The order ⌘[ ⌘] walks: the flat area's panes in tree order, then the shown cards from the
    /// front to the back, each in tree order. Capsules are skipped.
    pub fn order(&self) -> Vec<u64> {
        let mut out = self.flat.leaves();
        for card in self.cards.iter().rev().filter(|card| !card.minimized) {
            out.extend(card.tree.leaves());
        }
        out
    }

    /// The pane after (`forward`) or before `from` in [`Layers::order`], cyclic. `None` with
    /// fewer than two panes to walk, or when `from` is not in the order (it is in a capsule).
    pub fn cycle(&self, from: u64, forward: bool) -> Option<u64> {
        let order = self.order();
        if order.len() < 2 {
            return None;
        }
        let index = order.iter().position(|id| *id == from)?;
        let next = if forward {
            (index + 1) % order.len()
        } else {
            (index + order.len() - 1) % order.len()
        };
        Some(order[next])
    }

    /// The pane ⌥⌘ + arrow moves the keyboard to. In the flat area it is the neighbour there
    /// (cards are not in the way: from the flat area a card is not entered by an arrow). In a card
    /// it is the neighbour inside the card; with none that way the flat pane just beyond the
    /// card's edge on the line through the pane's middle — what the user sees next to the card
    /// that way. `None` where that point is outside the tab, or from a capsule.
    pub fn toward(&self, from: u64, direction: Direction, ground: &Ground<'_>) -> Option<u64> {
        let arranged = self.layout(ground);
        let Some(index) = self.card_of(from) else {
            return arranged.flat.neighbour(from, direction);
        };
        if self.cards[index].minimized {
            return None;
        }
        let inner = &arranged.cards[index].1;
        if let Some(next) = inner.neighbour(from, direction) {
            return Some(next);
        }
        let (_, frame) = inner.panes.iter().find(|(id, _)| *id == from)?;
        // Out of the card's edge on the line through the pane's middle, a point's width beyond:
        // the flat pane there (the one nearest, if the point is in a gap or a margin).
        let (cx, cy) = frame.center();
        let point = match direction {
            Direction::Right => (frame.x + frame.width + 1.0, cy),
            Direction::Left => (frame.x - 1.0, cy),
            Direction::Down => (cx, frame.y + frame.height + 1.0),
            Direction::Up => (cx, frame.y - 1.0),
        };
        let area = ground.bounds;
        if point.0 < area.x
            || point.0 > area.x + area.width
            || point.1 < area.y
            || point.1 > area.y + area.height
        {
            return None;
        }
        let away = |rect: &Rect| {
            let dx = (rect.x - point.0)
                .max(0.0)
                .max(point.0 - (rect.x + rect.width));
            let dy = (rect.y - point.1)
                .max(0.0)
                .max(point.1 - (rect.y + rect.height));
            dx * dx + dy * dy
        };
        arranged
            .flat
            .panes
            .iter()
            .min_by(|a, b| away(&a.1).total_cmp(&away(&b.1)))
            .map(|(id, _)| *id)
    }

    /// ⌃⌘ + arrow on floating pane `pane`: its card's right and bottom edges move by `step`
    /// points — right and down grow it, left and up shrink it — stopping at the tab's edge and
    /// at the least. `false` when `pane` is not floating or nothing moved.
    pub fn resize_key(
        &mut self,
        pane: u64,
        direction: Direction,
        step: f64,
        ground: &Ground<'_>,
    ) -> bool {
        let (grip, dx, dy) = match direction {
            Direction::Right => (Grip::Right, step, 0.0),
            Direction::Left => (Grip::Right, -step, 0.0),
            Direction::Down => (Grip::Bottom, 0.0, step),
            Direction::Up => (Grip::Bottom, 0.0, -step),
        };
        self.edit_area(pane, ground, |area, least| {
            area.dragged(
                grip,
                dx / ground.bounds.width,
                dy / ground.bounds.height,
                least,
            )
        })
    }

    /// ⌃⌥⌘ + arrow on floating pane `pane`: its card moves by `step` points, staying in the tab.
    /// `false` when `pane` is not floating or nothing moved.
    pub fn move_key(
        &mut self,
        pane: u64,
        direction: Direction,
        step: f64,
        ground: &Ground<'_>,
    ) -> bool {
        let (dx, dy) = match direction {
            Direction::Right => (step, 0.0),
            Direction::Left => (-step, 0.0),
            Direction::Down => (0.0, step),
            Direction::Up => (0.0, -step),
        };
        self.edit_area(pane, ground, |area, least| {
            area.moved(dx / ground.bounds.width, dy / ground.bounds.height, least)
        })
    }

    /// Sets the share of the card holding `pane` to what `change` makes of its shown share;
    /// `true` if the share changed. The card's front/back place is not touched.
    fn edit_area(
        &mut self,
        pane: u64,
        ground: &Ground<'_>,
        change: impl FnOnce(Area, (f64, f64)) -> Area,
    ) -> bool {
        if ground.bounds.width <= 0.0 || ground.bounds.height <= 0.0 {
            return false;
        }
        let Some(index) = self.card_of(pane) else {
            return false;
        };
        let card = &mut self.cards[index];
        let least = ground.least(&card.tree);
        let shown = card.area.within(least);
        let area = change(shown, least);
        let moved = (area.x - shown.x).abs() > EPSILON
            || (area.y - shown.y).abs() > EPSILON
            || (area.width - shown.width).abs() > EPSILON
            || (area.height - shown.height).abs() > EPSILON;
        if moved {
            card.area = area;
        }
        moved
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::split::Axis;

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn same(a: Area, b: Area) -> bool {
        near(a.x, b.x) && near(a.y, b.y) && near(a.width, b.width) && near(a.height, b.height)
    }

    /// `1 | 2` side by side.
    fn pair() -> Tree {
        let mut tree = Tree::Leaf(1);
        assert!(tree.split(1, Axis::Horizontal, 2));
        tree
    }

    fn small(_: u64) -> Size {
        Size::new(100.0, 60.0)
    }

    fn ground(min: &dyn Fn(u64) -> Size) -> Ground<'_> {
        Ground {
            bounds: Rect::new(0.0, 0.0, 1000.0, 600.0),
            scale: 1.0,
            flat: Spacing::DIVIDED,
            card: Spacing::DIVIDED,
            min,
        }
    }

    fn with_cards(cards: Vec<Card>) -> Layers {
        Layers {
            flat: pair(),
            cards,
        }
    }

    fn card(pane: u64) -> Card {
        Card::of(pane, Area::new(0.2, 0.2, 0.4, 0.4))
    }

    fn shown(pane: u64) -> Card {
        card(pane)
    }

    fn capsule(pane: u64) -> Card {
        Card {
            minimized: true,
            ..card(pane)
        }
    }

    // ---- the invariant -------------------------------------------------------------------

    #[test]
    fn the_flat_areas_only_pane_cannot_float() {
        let mut layers = Layers::flat(Tree::Leaf(1));
        assert!(!layers.float(1, Area::opening()));
        assert_eq!(layers, Layers::flat(Tree::Leaf(1)));
    }

    #[test]
    fn a_flat_pane_floats_and_the_rest_stays_flat() {
        let mut layers = Layers::flat(pair());
        assert!(layers.float(2, Area::opening()));
        assert_eq!(layers.flat, Tree::Leaf(1));
        assert_eq!(layers.cards, vec![Card::of(2, Area::opening())]);
        // It is not flat any more, and the flat area's only pane stays put.
        assert!(!layers.float(2, Area::opening()));
        assert!(!layers.float(1, Area::opening()));
    }

    #[test]
    fn a_floating_pane_does_not_float_again_and_a_missing_one_not_at_all() {
        let mut layers = with_cards(vec![card(3)]);
        let before = layers.clone();
        assert!(!layers.float(3, Area::opening()));
        assert!(!layers.float(9, Area::opening()));
        assert_eq!(layers, before);
    }

    #[test]
    fn the_flat_areas_last_pane_closing_brings_the_front_card_down_whole() {
        let mut two = Tree::Leaf(3);
        assert!(two.split(3, Axis::Vertical, 4));
        let mut layers = Layers::flat(Tree::Leaf(1));
        layers.cards = vec![
            card(2),
            Card {
                tree: two.clone(),
                ..card(3)
            },
        ];
        assert_eq!(layers.remove(1), Gone::Landed { focus: 3 });
        // The card's tree is the flat tree, ratios and all; the other card stays.
        assert_eq!(layers.flat, two);
        assert_eq!(layers.cards, vec![card(2)]);
    }

    #[test]
    fn with_every_card_a_capsule_the_one_minimized_last_comes_down() {
        let mut layers = Layers::flat(Tree::Leaf(1));
        layers.cards = vec![capsule(2), capsule(3)];
        assert_eq!(layers.remove(1), Gone::Landed { focus: 3 });
        assert_eq!(layers.flat, Tree::Leaf(3));
        assert_eq!(layers.cards, vec![capsule(2)]);
    }

    #[test]
    fn a_shown_card_comes_down_before_a_capsule_that_is_later_in_the_list() {
        let mut layers = Layers::flat(Tree::Leaf(1));
        layers.cards = vec![shown(2), capsule(3)];
        assert_eq!(layers.remove(1), Gone::Landed { focus: 2 });
        assert_eq!(layers.cards, vec![capsule(3)]);
    }

    #[test]
    fn the_tabs_only_pane_closing_ends_the_tab_and_a_stranger_is_missing() {
        let mut layers = Layers::flat(Tree::Leaf(1));
        assert_eq!(layers.remove(1), Gone::Last);
        assert_eq!(layers.remove(7), Gone::Missing);
        assert_eq!(layers, Layers::flat(Tree::Leaf(1)));
    }

    #[test]
    fn a_flat_pane_closing_while_others_remain_flat_behaves_as_ever() {
        let mut layers = with_cards(vec![card(3)]);
        assert_eq!(layers.remove(2), Gone::Closed { focus: Some(1) });
        assert_eq!(layers.flat, Tree::Leaf(1));
        assert_eq!(layers.cards, vec![card(3)]);
    }

    #[test]
    fn a_floating_pane_closing_hands_the_keyboard_to_its_cards_neighbour_or_back() {
        let mut inner = Tree::Leaf(3);
        assert!(inner.split(3, Axis::Horizontal, 4));
        let mut layers = with_cards(vec![Card {
            tree: inner,
            ..card(3)
        }]);
        assert_eq!(layers.remove(3), Gone::Closed { focus: Some(4) });
        assert_eq!(layers.cards.len(), 1);
        // The card's last pane: the card is gone and the keyboard goes back.
        assert_eq!(layers.remove(4), Gone::Closed { focus: None });
        assert!(layers.cards.is_empty());
    }

    // ---- sizes, clamping ----------------------------------------------------------------

    #[test]
    fn a_new_floating_pane_is_55_by_60_in_the_middle() {
        let area = Area::opening();
        assert!(same(area, Area::new(0.225, 0.2, 0.55, 0.6)));
        let g = ground(&small);
        let mut layers = Layers::flat(pair());
        assert!(layers.open(3, None, None, &g));
        assert!(same(layers.cards[0].area, Area::new(0.225, 0.2, 0.55, 0.6)));
        // Opening a pane the tab holds is refused.
        assert!(!layers.open(3, None, None, &g));
        assert!(!layers.open(1, None, None, &g));
    }

    #[test]
    fn a_pane_that_floats_takes_four_fifths_of_its_place_in_its_middle() {
        let g = ground(&small);
        let mut layers = Layers::flat(pair());
        // Pane 2 stands in the right half (a divider's width off): 0.5 wide, full height.
        assert!(layers.float_out(2, None, None, &g));
        let area = layers.cards[0].area;
        let frame = Area::of(
            Layers::flat(pair())
                .layout(&g)
                .flat
                .panes
                .iter()
                .find(|(id, _)| *id == 2)
                .map(|(_, rect)| *rect)
                .unwrap(),
            g.bounds,
        );
        assert!(same(area, Area::floated_from(frame)));
        assert!(near(area.width, frame.width * 0.8));
        assert!(near(area.height, frame.height * 0.8));
        assert!(near(area.centre().0, frame.centre().0));
        assert!(near(area.centre().1, frame.centre().1));
    }

    #[test]
    fn a_wish_sets_the_corner_and_the_size_and_a_size_alone_keeps_the_middle() {
        let least = (0.1, 0.1);
        let base = Area::opening();
        let at = base.wished(Some((0.1, 0.2)), Some((0.3, 0.4)), least);
        assert!(same(at, Area::new(0.1, 0.2, 0.3, 0.4)));
        let sized = base.wished(None, Some((0.3, 0.4)), least);
        assert!(same(sized, Area::centred(0.3, 0.4)));
        let placed = base.wished(Some((0.0, 0.0)), None, least);
        assert!(same(placed, Area::new(0.0, 0.0, 0.55, 0.6)));
        assert!(same(base.wished(None, None, least), base));
    }

    #[test]
    fn percents_are_shares() {
        assert!(same(
            Area::percent(10, 20, 30, 40),
            Area::new(0.1, 0.2, 0.3, 0.4)
        ));
    }

    #[test]
    fn an_area_is_clipped_into_the_tab_and_held_to_the_least() {
        let least = (0.2, 0.3);
        // Too far right and down: pulled back in, size kept.
        assert!(same(
            Area::new(0.9, 0.9, 0.4, 0.4).within(least),
            Area::new(0.6, 0.6, 0.4, 0.4)
        ));
        // Too small: the least. Too large: the tab.
        assert!(same(
            Area::new(0.1, 0.1, 0.05, 0.05).within(least),
            Area::new(0.1, 0.1, 0.2, 0.3)
        ));
        assert!(same(
            Area::new(-0.5, -0.5, 3.0, 3.0).within(least),
            Area::new(0.0, 0.0, 1.0, 1.0)
        ));
        // A least above the whole is the whole.
        assert!(same(
            Area::new(0.2, 0.2, 0.5, 0.5).within((1.5, 1.5)),
            Area::new(0.0, 0.0, 1.0, 1.0)
        ));
    }

    #[test]
    fn the_least_is_what_the_cards_tree_needs() {
        let g = ground(&small);
        // One pane: 100 × 60 points of 1000 × 600.
        let (w, h) = g.least(&Tree::Leaf(1));
        assert!(near(w, 0.1) && near(h, 0.1));
        // Two side by side: two minimums and the divider between them.
        let (w, h) = g.least(&pair());
        assert!(near(w, 201.0 / 1000.0) && near(h, 0.1));
        // Never more than the whole.
        let wide = |_: u64| Size::new(5000.0, 5000.0);
        let g = ground(&wide);
        assert_eq!(g.least(&Tree::Leaf(1)), (1.0, 1.0));
    }

    #[test]
    fn a_window_too_small_for_a_card_shows_it_at_the_least_and_keeps_its_share() {
        let big = |_: u64| Size::new(400.0, 300.0);
        let g = ground(&big);
        let layers = with_cards(vec![Card::of(3, Area::new(0.1, 0.1, 0.2, 0.2))]);
        let rect = g.rect_of(&layers.cards[0]);
        assert!(near(rect.width, 400.0) && near(rect.height, 300.0));
        assert_eq!(layers.cards[0].area, Area::new(0.1, 0.1, 0.2, 0.2));
    }

    // ---- edges and corners --------------------------------------------------------------

    #[test]
    fn a_pointer_near_an_edge_grips_it_and_near_two_grips_the_corner() {
        let card = Rect::new(100.0, 100.0, 200.0, 100.0);
        let slop = 4.0;
        assert_eq!(grip_at(card, (100.0, 150.0), slop), Some(Grip::Left));
        assert_eq!(grip_at(card, (97.0, 150.0), slop), Some(Grip::Left));
        assert_eq!(grip_at(card, (303.0, 150.0), slop), Some(Grip::Right));
        assert_eq!(grip_at(card, (200.0, 98.0), slop), Some(Grip::Top));
        assert_eq!(grip_at(card, (200.0, 203.0), slop), Some(Grip::Bottom));
        assert_eq!(grip_at(card, (101.0, 101.0), slop), Some(Grip::TopLeft));
        assert_eq!(grip_at(card, (300.0, 101.0), slop), Some(Grip::TopRight));
        assert_eq!(grip_at(card, (99.0, 200.0), slop), Some(Grip::BottomLeft));
        assert_eq!(grip_at(card, (301.0, 202.0), slop), Some(Grip::BottomRight));
    }

    #[test]
    fn a_pointer_on_the_card_or_away_from_it_grips_nothing() {
        let card = Rect::new(100.0, 100.0, 200.0, 100.0);
        assert_eq!(grip_at(card, (200.0, 150.0), 4.0), None);
        assert_eq!(grip_at(card, (50.0, 150.0), 4.0), None);
        assert_eq!(grip_at(card, (200.0, 300.0), 4.0), None);
        // Past the slop of an edge but level with the card: not a grip either.
        assert_eq!(grip_at(card, (95.0, 150.0), 4.0), None);
    }

    #[test]
    fn a_card_narrower_than_two_handles_gives_the_nearer_edge() {
        let card = Rect::new(100.0, 100.0, 6.0, 100.0);
        assert_eq!(grip_at(card, (101.0, 150.0), 4.0), Some(Grip::Left));
        assert_eq!(grip_at(card, (105.0, 150.0), 4.0), Some(Grip::Right));
    }

    #[test]
    fn dragging_a_grip_moves_its_edges_and_stops_at_the_tab_and_the_least() {
        let least = (0.1, 0.1);
        let area = Area::new(0.2, 0.2, 0.4, 0.4);
        let right = area.dragged(Grip::Right, 0.1, 0.5, least);
        assert!(same(right, Area::new(0.2, 0.2, 0.5, 0.4)));
        let past = area.dragged(Grip::Right, 5.0, 0.0, least);
        assert!(same(past, Area::new(0.2, 0.2, 0.8, 0.4)));
        let tiny = area.dragged(Grip::Right, -5.0, 0.0, least);
        assert!(same(tiny, Area::new(0.2, 0.2, 0.1, 0.4)));
        let corner = area.dragged(Grip::TopLeft, -0.1, -0.05, least);
        assert!(same(corner, Area::new(0.1, 0.15, 0.5, 0.45)));
        let far = area.dragged(Grip::TopLeft, -5.0, -5.0, least);
        assert!(same(far, Area::new(0.0, 0.0, 0.6, 0.6)));
        let squeezed = area.dragged(Grip::BottomLeft, 5.0, 5.0, least);
        assert!(same(squeezed, Area::new(0.5, 0.2, 0.1, 0.8)));
    }

    #[test]
    fn moving_a_card_keeps_its_size_and_the_tab() {
        let least = (0.1, 0.1);
        let area = Area::new(0.2, 0.2, 0.4, 0.4);
        assert!(same(
            area.moved(0.1, -0.1, least),
            Area::new(0.3, 0.1, 0.4, 0.4)
        ));
        assert!(same(
            area.moved(5.0, 5.0, least),
            Area::new(0.6, 0.6, 0.4, 0.4)
        ));
        assert!(same(
            area.moved(-5.0, -5.0, least),
            Area::new(0.0, 0.0, 0.4, 0.4)
        ));
    }

    #[test]
    fn the_resize_keys_move_the_right_and_bottom_edges() {
        let g = ground(&small);
        let mut layers = with_cards(vec![Card::of(3, Area::new(0.2, 0.2, 0.4, 0.4))]);
        // 50 points of 1000 / 600.
        assert!(layers.resize_key(3, Direction::Right, 50.0, &g));
        assert!(same(layers.cards[0].area, Area::new(0.2, 0.2, 0.45, 0.4)));
        assert!(layers.resize_key(3, Direction::Left, 100.0, &g));
        assert!(same(layers.cards[0].area, Area::new(0.2, 0.2, 0.35, 0.4)));
        assert!(layers.resize_key(3, Direction::Down, 60.0, &g));
        assert!(same(layers.cards[0].area, Area::new(0.2, 0.2, 0.35, 0.5)));
        assert!(layers.resize_key(3, Direction::Up, 60.0, &g));
        assert!(same(layers.cards[0].area, Area::new(0.2, 0.2, 0.35, 0.4)));
        // At the tab's edge nothing moves, and the answer says so.
        layers.cards[0].area = Area::new(0.2, 0.2, 0.8, 0.4);
        assert!(!layers.resize_key(3, Direction::Right, 50.0, &g));
        // At the least, shrinking does nothing either.
        layers.cards[0].area = Area::new(0.2, 0.2, 0.1, 0.1);
        assert!(!layers.resize_key(3, Direction::Left, 50.0, &g));
        assert!(!layers.resize_key(3, Direction::Up, 50.0, &g));
        // A flat pane has no card to resize.
        assert!(!layers.resize_key(1, Direction::Right, 50.0, &g));
    }

    #[test]
    fn the_move_keys_move_the_card_inside_the_tab() {
        let g = ground(&small);
        let mut layers = with_cards(vec![Card::of(3, Area::new(0.2, 0.2, 0.4, 0.4))]);
        assert!(layers.move_key(3, Direction::Right, 100.0, &g));
        assert!(same(layers.cards[0].area, Area::new(0.3, 0.2, 0.4, 0.4)));
        assert!(layers.move_key(3, Direction::Up, 60.0, &g));
        assert!(same(layers.cards[0].area, Area::new(0.3, 0.1, 0.4, 0.4)));
        layers.cards[0].area = Area::new(0.6, 0.6, 0.4, 0.4);
        assert!(!layers.move_key(3, Direction::Right, 100.0, &g));
        assert!(!layers.move_key(3, Direction::Down, 100.0, &g));
        assert!(!layers.move_key(2, Direction::Down, 100.0, &g));
    }

    // ---- docking ------------------------------------------------------------------------

    #[test]
    fn a_pane_docks_where_its_centre_would_be_let_go() {
        let g = ground(&small);
        // The card sits over the right half of the flat area, its centre in pane 2's right
        // edge half: the pane lands beside 2, on its right.
        let mut layers = with_cards(vec![Card::of(3, Area::new(0.6, 0.1, 0.35, 0.8))]);
        assert!(layers.dock(3, 1, &g));
        assert!(layers.cards.is_empty());
        assert_eq!(layers.flat.leaves(), vec![1, 2, 3]);
        // A docked pane is a flat one: it can float again.
        assert!(layers.float(3, Area::opening()));
    }

    #[test]
    fn a_pane_over_the_left_half_docks_on_the_left_of_what_is_under_it() {
        let g = ground(&small);
        let mut layers = with_cards(vec![Card::of(3, Area::new(0.05, 0.1, 0.2, 0.8))]);
        assert!(layers.dock(3, 2, &g));
        assert_eq!(layers.flat.leaves(), vec![3, 1, 2]);
    }

    #[test]
    fn a_dock_leaves_the_cards_other_panes_floating_and_an_emptied_card_is_gone() {
        let g = ground(&small);
        let mut inner = Tree::Leaf(3);
        assert!(inner.split(3, Axis::Horizontal, 4));
        let mut layers = with_cards(vec![Card {
            tree: inner,
            area: Area::new(0.05, 0.1, 0.3, 0.8),
            minimized: false,
        }]);
        assert!(layers.dock(3, 1, &g));
        assert_eq!(layers.cards.len(), 1);
        assert_eq!(layers.cards[0].tree, Tree::Leaf(4));
        assert_eq!(layers.flat.leaves().len(), 3);
        assert!(layers.dock(4, 1, &g));
        assert!(layers.cards.is_empty());
        assert_eq!(layers.panes().len(), 4);
    }

    #[test]
    fn with_no_room_where_the_centre_is_the_pane_goes_beside_the_keyboards_pane() {
        // Two stacked panes in a tall, narrow-minded tab: nothing fits beside a pane
        // horizontally (the least width is 600 of 1000), so the pointer's place is refused with
        // the edges that fit, and the fallback finds the one below.
        let wide = |_: u64| Size::new(600.0, 100.0);
        let g = Ground {
            bounds: Rect::new(0.0, 0.0, 1000.0, 400.0),
            scale: 1.0,
            flat: Spacing::DIVIDED,
            card: Spacing::DIVIDED,
            min: &wide,
        };
        let mut stack = Tree::Leaf(1);
        assert!(stack.split(1, Axis::Vertical, 2));
        // Centre of the card: near the left edge of pane 1 — a drop on its left half.
        let mut layers = Layers {
            flat: stack.clone(),
            cards: vec![Card::of(3, Area::new(0.0, 0.0, 0.2, 0.3))],
        };
        let room = g.flat_room();
        let probe = layers.flat.verdict(&Tree::Leaf(3), &room, (100.0, 50.0));
        assert!(matches!(probe, Verdict::TooSmall { .. }), "{probe:?}");
        assert!(layers.dock(3, 1, &g));
        assert_eq!(layers.flat.leaves().len(), 3);
        assert!(layers.cards.is_empty());
        // The pane went below, in the stack: the root is still a vertical split.
        assert!(matches!(
            layers.flat,
            Tree::Split {
                axis: Axis::Vertical,
                ..
            }
        ));
    }

    #[test]
    fn with_room_nowhere_nothing_docks_and_nothing_changes() {
        // A tab whose smallest pane is the whole tab: no pane fits beside another.
        let huge = |_: u64| Size::new(900.0, 500.0);
        let g = Ground {
            bounds: Rect::new(0.0, 0.0, 1000.0, 600.0),
            scale: 1.0,
            flat: Spacing::DIVIDED,
            card: Spacing::DIVIDED,
            min: &huge,
        };
        let mut layers = Layers {
            flat: Tree::Leaf(1),
            cards: vec![Card::of(3, Area::new(0.1, 0.1, 0.8, 0.8))],
        };
        let before = layers.clone();
        assert!(!layers.dock(3, 1, &g));
        assert_eq!(layers, before);
    }

    #[test]
    fn a_flat_pane_does_not_dock_and_a_stranger_neither() {
        let g = ground(&small);
        let mut layers = with_cards(vec![card(3)]);
        let before = layers.clone();
        assert!(!layers.dock(1, 1, &g));
        assert!(!layers.dock(9, 1, &g));
        assert_eq!(layers, before);
    }

    // ---- the order the keyboard walks --------------------------------------------------

    #[test]
    fn the_keyboard_walks_the_flat_area_then_the_cards_front_to_back() {
        // Back to front: 3, 4, 5. Front to back: 5, 4, 3.
        let layers = with_cards(vec![card(3), card(4), card(5)]);
        assert_eq!(layers.order(), vec![1, 2, 5, 4, 3]);
        assert_eq!(layers.cycle(2, true), Some(5));
        assert_eq!(layers.cycle(5, true), Some(4));
        assert_eq!(layers.cycle(3, true), Some(1));
        assert_eq!(layers.cycle(1, false), Some(3));
        assert_eq!(layers.cycle(5, false), Some(2));
    }

    #[test]
    fn the_walk_skips_capsules_and_counts_a_cards_panes_in_tree_order() {
        let mut inner = Tree::Leaf(4);
        assert!(inner.split(4, Axis::Vertical, 6));
        let layers = with_cards(vec![
            capsule(3),
            Card {
                tree: inner,
                ..card(4)
            },
        ]);
        assert_eq!(layers.order(), vec![1, 2, 4, 6]);
        // A pane in a capsule is not in the walk.
        assert_eq!(layers.cycle(3, true), None);
        // A tab of one pane has nothing to walk to.
        assert_eq!(Layers::flat(Tree::Leaf(1)).cycle(1, true), None);
    }

    #[test]
    fn an_arrow_in_the_flat_area_never_enters_a_card() {
        let g = ground(&small);
        let layers = with_cards(vec![Card::of(3, Area::new(0.05, 0.1, 0.2, 0.8))]);
        assert_eq!(layers.toward(1, Direction::Right, &g), Some(2));
        assert_eq!(layers.toward(2, Direction::Left, &g), Some(1));
        assert_eq!(layers.toward(1, Direction::Left, &g), None);
        assert_eq!(layers.toward(1, Direction::Down, &g), None);
    }

    #[test]
    fn an_arrow_in_a_card_looks_inside_first_then_beyond_the_cards_edge() {
        let g = ground(&small);
        let mut inner = Tree::Leaf(3);
        assert!(inner.split(3, Axis::Horizontal, 4));
        // The card covers x 300–700, y 60–360 of the 1000 × 600 tab; the flat area is 1 | 2.
        let layers = with_cards(vec![Card {
            tree: inner,
            area: Area::new(0.3, 0.1, 0.4, 0.5),
            minimized: false,
        }]);
        // Inside the card: 3 then 4.
        assert_eq!(layers.toward(3, Direction::Right, &g), Some(4));
        assert_eq!(layers.toward(4, Direction::Left, &g), Some(3));
        // Out of the card's sides: what is next to it that way, pane 2 on the right and 1 on
        // the left.
        assert_eq!(layers.toward(4, Direction::Right, &g), Some(2));
        assert_eq!(layers.toward(3, Direction::Left, &g), Some(1));
        // Up from the card: a point above it, still in the tab — the flat pane there. Below:
        // likewise.
        assert!(layers.toward(3, Direction::Up, &g).is_some());
        assert!(layers.toward(3, Direction::Down, &g).is_some());
    }

    #[test]
    fn from_a_card_the_flat_pane_is_the_one_beside_it_on_the_line_through_its_middle() {
        let g = ground(&small);
        // Flat: 1 on the left, 2 above 4 on the right (2 is y 0–300, 4 is y 300–600).
        let mut flat = pair();
        assert!(flat.split(2, Axis::Vertical, 4));
        let layers = Layers {
            flat,
            cards: vec![Card::of(3, Area::new(0.1, 0.55, 0.2, 0.2))],
        };
        // The card is y 330–450 at the left, over pane 1: the point just beyond its right edge
        // (x 301) is still in pane 1.
        assert_eq!(layers.toward(3, Direction::Right, &g), Some(1));
        // A card further right: the line y = 390 beyond its edge is pane 4.
        let layers = Layers {
            cards: vec![Card::of(3, Area::new(0.4, 0.55, 0.2, 0.2))],
            ..layers
        };
        assert_eq!(layers.toward(3, Direction::Right, &g), Some(4));
        assert_eq!(layers.toward(3, Direction::Left, &g), Some(1));
        // Above that card, on the line x = 500: pane 1/2 at the seam — either is a flat pane.
        assert!(layers.toward(3, Direction::Up, &g).is_some());
        // At the tab's edge there is no way out.
        let layers = Layers {
            cards: vec![Card::of(3, Area::new(0.8, 0.55, 0.2, 0.2))],
            ..layers
        };
        assert_eq!(layers.toward(3, Direction::Right, &g), None);
        let layers = Layers {
            cards: vec![Card::of(3, Area::new(0.8, 0.0, 0.2, 0.2))],
            ..layers
        };
        assert_eq!(layers.toward(3, Direction::Up, &g), None);
        assert_eq!(layers.toward(3, Direction::Down, &g), Some(2));
    }

    #[test]
    fn nothing_is_next_to_a_capsule() {
        let g = ground(&small);
        let layers = with_cards(vec![capsule(3)]);
        assert_eq!(layers.toward(3, Direction::Right, &g), None);
    }

    // ---- minimizing ---------------------------------------------------------------------

    #[test]
    fn minimizing_goes_to_the_end_and_the_capsules_run_in_that_order() {
        let mut layers = with_cards(vec![card(3), card(4), card(5)]);
        assert!(layers.minimize(4));
        assert!(layers.minimize(3));
        assert_eq!(layers.capsules(), vec![4, 3]);
        assert!(!layers.minimize(3), "already a capsule");
        assert!(!layers.minimize(1), "a flat pane has no card to minimize");
        // The shown card is the one left in front of the walk.
        assert_eq!(layers.order(), vec![1, 2, 5]);
        assert_eq!(layers.front(), Some(0));
    }

    #[test]
    fn revealing_a_capsule_opens_it_in_front_where_it_was() {
        let mut layers = with_cards(vec![card(3), card(4)]);
        assert!(layers.minimize(3));
        assert!(layers.reveal(3));
        assert!(!layers.cards[1].minimized);
        assert_eq!(layers.cards[1].area, Area::new(0.2, 0.2, 0.4, 0.4));
        assert_eq!(layers.order(), vec![1, 2, 3, 4]);
        // A click on a shown card raises it.
        assert!(layers.reveal(4));
        assert_eq!(layers.order(), vec![1, 2, 4, 3]);
        assert!(!layers.reveal(1));
    }

    #[test]
    fn the_minimize_key_in_a_floating_pane_minimizes_its_card() {
        let mut layers = with_cards(vec![card(3), card(4)]);
        assert_eq!(layers.minimize_key(4), Some(Focus::Back));
        assert_eq!(layers.capsules(), vec![4]);
        // Again on a capsule's pane does nothing.
        assert_eq!(layers.minimize_key(4), None);
    }

    #[test]
    fn the_minimize_key_in_the_flat_area_opens_the_card_minimized_last() {
        let mut layers = with_cards(vec![card(3), card(4), card(5)]);
        assert!(layers.minimize(3));
        assert!(layers.minimize(5));
        assert_eq!(layers.minimize_key(1), Some(Focus::Pane(5)));
        assert_eq!(layers.capsules(), vec![3]);
        assert_eq!(layers.order(), vec![1, 2, 5, 4]);
        assert_eq!(layers.minimize_key(2), Some(Focus::Pane(3)));
        // No capsule left: nothing to open.
        assert_eq!(layers.minimize_key(1), None);
        assert_eq!(Layers::flat(pair()).minimize_key(1), None);
    }

    #[test]
    fn the_hide_key_minimizes_all_shown_cards_back_to_front() {
        let mut layers = with_cards(vec![card(3), card(4), card(5)]);
        assert!(layers.minimize(4));
        // 4 is a capsule; 3 and 5 are shown. The keyboard was in 5.
        assert_eq!(layers.hide_key(5), Some(Focus::Back));
        assert_eq!(layers.capsules(), vec![4, 3, 5]);
        assert_eq!(layers.order(), vec![1, 2]);
    }

    #[test]
    fn the_hide_key_leaves_a_flat_keyboard_alone() {
        let mut layers = with_cards(vec![card(3), card(4)]);
        assert_eq!(layers.hide_key(1), Some(Focus::Keep));
        assert_eq!(layers.capsules(), vec![3, 4]);
    }

    #[test]
    fn the_hide_key_with_nothing_shown_opens_every_capsule_in_place() {
        let mut layers = with_cards(vec![card(3), card(4)]);
        assert_eq!(layers.hide_key(1), Some(Focus::Keep));
        let areas: Vec<Area> = layers.cards.iter().map(|card| card.area).collect();
        assert_eq!(layers.hide_key(1), Some(Focus::Keep));
        assert!(layers.cards.iter().all(|card| !card.minimized));
        assert_eq!(
            layers
                .cards
                .iter()
                .map(|card| card.area)
                .collect::<Vec<_>>(),
            areas
        );
        assert_eq!(layers.order(), vec![1, 2, 4, 3]);
        // And a tab with no card has nothing to hide or show.
        assert_eq!(Layers::flat(pair()).hide_key(1), None);
    }

    #[test]
    fn the_front_card_and_the_resting_place_of_the_keyboard() {
        let mut layers = with_cards(vec![card(3), card(4)]);
        assert_eq!(layers.front(), Some(1));
        assert_eq!(layers.resting(), 4);
        assert!(layers.minimize(4));
        assert_eq!(layers.front(), Some(0));
        assert_eq!(layers.resting(), 3);
        assert!(layers.minimize(3));
        // Every card a capsule: the front is the newest capsule, the keyboard rests in the flat
        // area.
        assert_eq!(layers.front(), Some(1));
        assert_eq!(layers.resting(), 1);
        assert_eq!(Layers::flat(pair()).front(), None);
    }

    // ---- the whole ---------------------------------------------------------------------

    #[test]
    fn a_split_inside_a_card_stays_in_the_card() {
        let mut layers = with_cards(vec![card(3)]);
        assert!(layers.split(3, Axis::Vertical, 4));
        assert_eq!(layers.cards[0].tree.leaves(), vec![3, 4]);
        assert_eq!(layers.flat.leaves(), vec![1, 2]);
        assert!(layers.split(1, Axis::Vertical, 5));
        assert_eq!(layers.flat.leaves(), vec![1, 5, 2]);
        assert!(!layers.split(9, Axis::Vertical, 6));
        assert_eq!(layers.panes(), vec![1, 5, 2, 3, 4]);
    }

    #[test]
    fn the_layout_places_every_card_inside_the_tab() {
        let g = ground(&small);
        let layers = with_cards(vec![card(3)]);
        let arranged = layers.layout(&g);
        assert_eq!(arranged.flat.panes.len(), 2);
        let (rect, inner) = &arranged.cards[0];
        assert!(near(rect.x, 200.0) && near(rect.y, 120.0));
        assert!(near(rect.width, 400.0) && near(rect.height, 240.0));
        assert_eq!(inner.panes.len(), 1);
    }
}
