//! What a page remembers between builds: what the pointer is over and has pressed, where the
//! keyboard is, how far each list is scrolled, how far each highlight has faded.
//!
//! The elements are made afresh for every frame, so none of this can be in them; it is kept by
//! [`Id`] and the page reads it when it builds. Every method that takes the pointer or the
//! wheel answers whether anything it keeps *changed* — a pointer moving inside the row it
//! was already over changes nothing, and the host draws no frame for it.

use std::collections::HashMap;

use crate::{Fades, Id, Point, Queries};

/// The remembered state of a page.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ui {
    hover: Option<Id>,
    pressed: Option<Id>,
    focus: Option<Id>,
    /// How far each scrolling box is scrolled, points.
    scroll: HashMap<Id, f32>,
    /// The highlights coming and going.
    pub fades: Fades,
}

impl Ui {
    /// The pressable element the pointer is over.
    pub fn hover(&self) -> Option<Id> {
        self.hover
    }

    /// The pressable element a button is held on.
    pub fn pressed(&self) -> Option<Id> {
        self.pressed
    }

    /// The element the keyboard is on.
    pub fn focus(&self) -> Option<Id> {
        self.focus
    }

    // --- The pointer ---------------------------------------------------------------------

    /// The pointer is at `at` (`None`: it left the page). Returns whether the element it is
    /// over changed.
    pub fn point(&mut self, laid: &dyn Queries, at: Option<Point>) -> bool {
        let over = at.and_then(|at| laid.hit(at));
        std::mem::replace(&mut self.hover, over) != over
    }

    /// A button went down at `at`: the element under it is the pressed one, and the keyboard
    /// moves to the nearest element around it that takes it (`focusable`). Returns the element
    /// pressed.
    pub fn press(&mut self, laid: &dyn Queries, at: Point) -> Option<Id> {
        let hit = laid.hit(at);
        self.pressed = hit;
        let under = laid.ids_at(at);
        if let Some(focus) = under.into_iter().find(|id| self.takes_focus(laid, *id)) {
            self.focus = Some(focus);
        }
        hit
    }

    /// The button came up at `at`. A click is a release over the element the press was on:
    /// returns it.
    pub fn release(&mut self, laid: &dyn Queries, at: Point) -> Option<Id> {
        let pressed = self.pressed.take()?;
        (laid.hit(at) == Some(pressed)).then_some(pressed)
    }

    // --- The wheel -----------------------------------------------------------------------

    /// How far the scrolling box named `id` is scrolled, points.
    pub fn scroll_offset(&self, id: Id) -> f32 {
        self.scroll.get(&id).copied().unwrap_or(0.0)
    }

    /// Scrolls the box named `id` to `offset`, kept within the box's reach on the page `laid`
    /// (or at least 0 where the box is not known yet). Returns whether it moved.
    pub fn scroll_to(&mut self, laid: &dyn Queries, id: Id, offset: f32) -> bool {
        let reach = laid.scroll_extent(id).map_or(f32::MAX, |e| e.max_offset());
        let offset = offset.clamp(0.0, reach);
        let was = self.scroll.insert(id, offset).unwrap_or(0.0);
        was != offset
    }

    /// Sets how far the box named `id` is scrolled, without knowing yet how far it can go: the
    /// next [`Ui::settle`] brings it within its reach. For a page that has just changed what
    /// the box holds.
    pub fn set_scroll(&mut self, id: Id, offset: f32) {
        self.scroll.insert(id, offset.max(0.0));
    }

    /// The wheel turned over `at` by `dy` points (positive: down the page). The innermost
    /// scrolling box under the pointer that has room to go that way moves; one already at its
    /// end passes the turn on to the box around it. Returns whether anything moved.
    pub fn scroll_by(&mut self, laid: &dyn Queries, at: Point, dy: f32) -> bool {
        for id in laid.ids_at(at) {
            let Some(extent) = laid.scroll_extent(id) else {
                continue;
            };
            let was = self.scroll_offset(id);
            let to = (was + dy).clamp(0.0, extent.max_offset());
            if to != was {
                self.scroll.insert(id, to);
                return true;
            }
        }
        false
    }

    /// Brings every scrolled box back within its reach on the page `laid` — a list that got
    /// shorter (a filter) or a window that got taller. Returns whether any moved: the page
    /// is laid out again then, as its rows depend on the offset.
    pub fn settle(&mut self, laid: &dyn Queries) -> bool {
        let mut moved = false;
        self.scroll.retain(|id, offset| {
            let Some(extent) = laid.scroll_extent(*id) else {
                return *offset > 0.0;
            };
            let to = offset.clamp(0.0, extent.max_offset());
            moved |= to != *offset;
            *offset = to;
            to > 0.0
        });
        moved
    }

    // --- The keyboard --------------------------------------------------------------------

    fn takes_focus(&self, laid: &dyn Queries, id: Id) -> bool {
        laid.focus_order().contains(&id)
    }

    /// Puts the keyboard on `id`; returns whether it moved.
    pub fn focus_on(&mut self, id: Option<Id>) -> bool {
        std::mem::replace(&mut self.focus, id) != id
    }

    /// Moves the keyboard to the next element that takes it (the previous one when `back`),
    /// around from the last to the first. From nowhere it goes to the first (the last, back).
    /// Returns the element the keyboard is on now.
    pub fn focus_step(&mut self, laid: &dyn Queries, back: bool) -> Option<Id> {
        let order = laid.focus_order();
        if order.is_empty() {
            self.focus = None;
            return None;
        }
        let at = self
            .focus
            .and_then(|focus| order.iter().position(|id| *id == focus));
        let next = match (at, back) {
            (None, false) => 0,
            (None, true) => order.len() - 1,
            (Some(at), false) => (at + 1) % order.len(),
            (Some(at), true) => (at + order.len() - 1) % order.len(),
        };
        self.focus = Some(order[next]);
        self.focus
    }

    /// Where a key goes: the element the keyboard is on, then the named elements around it,
    /// the innermost first. The page offers the key to each in turn and the first that
    /// takes it ends the walk.
    pub fn key_path(&self, laid: &dyn Queries) -> Vec<Id> {
        self.focus
            .map_or_else(Vec::new, |focus| laid.path_of(focus))
    }
}
