//! The tabs in a column down the window's left edge (`[appearance] tab_position = "side"`): the
//! bar's other mode.
//!
//! **One bar, two places.** The column is not a second view with its own copy of the bar's
//! doings: it is the same [`TabBar`] — the tabs it was given, its chips, the transfers' ends, the
//! ⌘ keys, the clock, the name field, the menus — put in the window's full height at the left
//! instead of in the title row ([`TabBar::set_side`], told by the window's root view with every
//! placement). What changes is where things stand, and that is the pure model's: the rows from
//! `tabs::Side::layout`, the parts of a row from `tabs::Row` ([`Chip::set`] reads it), so the
//! Linux shell can lay the same list out. This file is a child of `tab_bar` for the bar's
//! private state, which a sibling could not reach without every field going public.
//!
//! **Where it sits.** The column is as wide as the window makes the bar (`tabs::side_width` of the
//! width the user dragged its edge to, `side_edge`) and the window's height tall, the traffic
//! lights at its head: the list starts a title row down (`top`, read from the window, the
//! same in full screen so the rows do not move as it comes and goes) and ends at the column's
//! foot, or above the settings warning, which sits in the window's bottom-left corner and keeps
//! that corner's centre ([`tabs::Fit`]). The head holds the sidebar button right of the traffic
//! lights and New Tab at the column's end, bare glyphs (`tabs::Head`). The user may hide the
//! column ([`TabBar::set_column_hidden`]): the bar is then the title row — the sidebar button,
//! the tab on screen's name in the middle, New Tab in the window's corner — and the button's
//! corner carries what the tabs out of sight ask. The rows scroll by the wheel when they do not fit, run under the list's ends
//! and dissolve there by a mask on the list ([`Mask`]) — not a band of the ground's colour
//! painted over them, which would lighten a card's shadow falling under the column.
//!
//! **A press on the column's empty part moves the window** (the bar's `mouseDown:`,
//! `performWindowDragWithEvent:`), as it does on the bar's. The band of the title row over the
//! content is claimed by a [`RowClaim`] of the root view, so a press there is the pane's.
//!
//! **The summary card** opens at the row's right, [`CARD_BESIDE`](super::CARD_BESIDE) off the
//! column and level with the row, slid to fit the window (`card_beside`): the same card, the
//! same wait and the same map as the title row's, arriving from the row's side
//! ([`ROW_LEFT`](super::ROW_LEFT)).
//!
//! **Dragging** is the strip's, turned upright: a press on a row is a [`Grip`](crate::tabs::Grip)
//! read along y, and past the slop the row is carried in a drag session (`tab_drag`) — nothing
//! in the column moves while it is; the row waits, faint, in its place. The column answers what is
//! carried over it, its own row or another window's, with what the pointer is over: the middle of
//! a row lights it (a tab joins that tab, a pane joins it too), the place between two rows is
//! marked by a line ([`Insertion`]) — in a window that has groups a place of
//! [`crate::groups::Groups::side_spots`]: a group's border has two, the end of the group above
//! and the head of the group below, told apart by the middle of the header. Rows picked together
//! and a group's header are carried the same way and answered with the line alone
//! (`TabBar::aim`).

use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAccessibility, NSAffineTransformNSAppKitAdditions, NSBezierPath, NSBox, NSColor, NSFont,
    NSFontAttributeName, NSFontWeightMedium, NSFontWeightSemibold, NSForegroundColorAttributeName,
    NSGraphicsContext, NSLineBreakMode, NSMutableParagraphStyle, NSParagraphStyleAttributeName,
    NSStringDrawing, NSTextAlignment, NSView,
};
use objc2_foundation::{
    NSAffineTransform, NSAttributedStringKey, NSDictionary, NSObjectProtocol, NSPoint, NSRect,
    NSSize, NSString,
};
use objc2_quartz_core::{CALayer, CATransaction};

use super::tab_group::{GroupHeader, HeaderLook};
use super::{
    AgentLine, AgentTone, Chip, Dragging, Emblem, Heading, IconStyle, Motion, Palette, Seen, Shown,
    Standing, TabBar, Tint, draw_tile, glyph_slot, paint_in, plain_box, rounded,
};
use crate::card;
use crate::split::Rect;
use crate::tabs::{self, Column, Entry, FADE, Side, TAB_RADIUS};

/// A model rectangle as the view's: both are top-down, in points.
pub(super) fn ns_rect(rect: Rect) -> NSRect {
    NSRect::new(
        NSPoint::new(rect.x, rect.y),
        NSSize::new(rect.width, rect.height),
    )
}

/// A rectangle with its own radius at each corner — top-left, top-right, bottom-right,
/// bottom-left, as a top-down view draws them — filled, and its inside edge stroked with `line` if
/// any. [`super::rounded`] for a shape whose corners differ: the settings warning in the column
/// keeps the window's corner at its foot.
pub(super) fn rounded_corners(rect: NSRect, radii: [f64; 4], fill: Tint, line: Option<Tint>) {
    let path = corners_path(rect, radii);
    fill.color().setFill();
    path.fill();
    if let Some(line) = line {
        let inner = NSRect::new(
            NSPoint::new(rect.origin.x + 0.5, rect.origin.y + 0.5),
            NSSize::new(rect.size.width - 1.0, rect.size.height - 1.0),
        );
        let path = corners_path(inner, radii.map(|radius| (radius - 0.5).max(0.0)));
        path.setLineWidth(1.0);
        line.color().setStroke();
        path.stroke();
    }
}

/// The outline of [`rounded_corners`]: each corner an arc tangent to its two sides.
fn corners_path(
    rect: NSRect,
    [top_left, top_right, bottom_right, bottom_left]: [f64; 4],
) -> Retained<NSBezierPath> {
    let (left, top) = (rect.origin.x, rect.origin.y);
    let (right, bottom) = (left + rect.size.width, top + rect.size.height);
    let path = NSBezierPath::bezierPath();
    path.moveToPoint(NSPoint::new(left + top_left, top));
    path.lineToPoint(NSPoint::new(right - top_right, top));
    for (corner, towards, radius) in [
        (
            NSPoint::new(right, top),
            NSPoint::new(right, top + top_right),
            top_right,
        ),
        (
            NSPoint::new(right, bottom),
            NSPoint::new(right - bottom_right, bottom),
            bottom_right,
        ),
        (
            NSPoint::new(left, bottom),
            NSPoint::new(left, bottom - bottom_left),
            bottom_left,
        ),
        (
            NSPoint::new(left, top),
            NSPoint::new(left + top_left, top),
            top_left,
        ),
    ] {
        // The line from where the last arc ended is this arc's own tangent line.
        path.appendBezierPathWithArcFromPoint_toPoint_radius(corner, towards, radius);
    }
    path.closePath();
    path
}

/// The mask that fades the column's list where rows run under its ends, and what it was painted
/// for: a picture is made only when the list's size or its ends change.
#[derive(Default)]
pub(super) struct Mask {
    layer: RefCell<Option<Retained<CALayer>>>,
    painted: Cell<Option<Painted>>,
}

/// What a mask's picture was painted for.
#[derive(Clone, Copy, PartialEq)]
struct Painted {
    size: (f64, f64),
    top: bool,
    bottom: bool,
    scale: f64,
}

impl Mask {
    /// The list's mask for `column` at `scale`: opaque where rows show, running out to nothing
    /// over [`FADE`] at an end where more lies beyond (`Column::fade_top`/`fade_bottom`); no mask
    /// where nothing runs under an end.
    fn set(&self, list: &NSView, column: &Column, scale: f64) {
        let (top, bottom) = (column.fade_top, column.fade_bottom);
        if !top && !bottom {
            self.clear(list);
            return;
        }
        list.setWantsLayer(true);
        let Some(host) = list.layer() else {
            return;
        };
        let size = NSSize::new(column.list.width, column.list.height);
        let state = Painted {
            size: (size.width, size.height),
            top,
            bottom,
            scale,
        };
        let mut slot = self.layer.borrow_mut();
        let fresh = slot.is_none();
        let layer = slot.get_or_insert_with(CALayer::new);
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        layer.setFrame(NSRect::new(NSPoint::ZERO, size));
        if self.painted.replace(Some(state)) != Some(state) {
            paint_in(layer, size, scale, move |rect| {
                fade_picture(rect, top, bottom)
            });
        }
        if fresh {
            // SAFETY: a mask is any layer; this one is the host's alone and lives as long as
            // the mask slot holds it.
            unsafe { host.setMask(Some(layer)) };
        }
        CATransaction::commit();
    }

    /// Takes the mask off: every row shows to the list's ends.
    pub(super) fn clear(&self, list: &NSView) {
        if self.layer.take().is_none() {
            return;
        }
        self.painted.set(None);
        if let Some(host) = list.layer() {
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            // SAFETY: taking a mask off, which any layer allows.
            unsafe { host.setMask(None) };
            CATransaction::commit();
        }
    }
}

/// The mask's picture in `rect` (top-down): black — its alpha is what shows — from end to end,
/// less [`FADE`] at the ends that fade, where the alpha runs from 0 at the edge to 1 inside along
/// a smooth step.
fn fade_picture(rect: NSRect, top: bool, bottom: bool) {
    let (width, height) = (rect.size.width, rect.size.height);
    let fade = FADE.min(height / 2.0);
    let step = |t: f64| t * t * (3.0 - 2.0 * t);
    let black = |alpha: f64| NSColor::colorWithSRGBRed_green_blue_alpha(0.0, 0.0, 0.0, alpha);
    let from = if top { fade } else { 0.0 };
    let to = if bottom { height - fade } else { height };
    black(1.0).setFill();
    NSBezierPath::bezierPathWithRect(NSRect::new(
        NSPoint::new(0.0, from),
        NSSize::new(width, (to - from).max(0.0)),
    ))
    .fill();
    // A line each: the picture is painted when the list's size changes, not per frame.
    let lines = fade.ceil() as usize;
    for line in 0..lines {
        let y = line as f64;
        let alpha = step(((y + 0.5) / fade).min(1.0));
        black(alpha).setFill();
        if top {
            NSBezierPath::bezierPathWithRect(NSRect::new(
                NSPoint::new(0.0, y),
                NSSize::new(width, 1.0),
            ))
            .fill();
        }
        if bottom {
            NSBezierPath::bezierPathWithRect(NSRect::new(
                NSPoint::new(0.0, height - y - 1.0),
                NSSize::new(width, 1.0),
            ))
            .fill();
        }
    }
}

/// What is in flight over the column while it is laid out ([`TabBar::lay_out`] works the strip's
/// the same way): a row held by the pointer, and room a tab carried from another window opens.
#[derive(Clone, Copy)]
pub(super) struct Flight {
    pub(super) drag: Option<Dragging>,
    pub(super) gap: Option<usize>,
}

/// The ring's side and its stroke, and how far it hangs out to the left of the line it starts.
const RING: f64 = 8.0;
const RING_LINE: f64 = 2.0;

/// The line that marks "here" between two rows while a pane or a tab is carried over them: a ring
/// at the left and a line along the row's width, in the accent, standing in the gap
/// ([`Column::between`]) — or, between two chips of the strip, a line up the chip's height
/// ([`tabs::Strip::between`]). It lives in the list's view, so the list's edges cut it as they cut
/// the rows.
#[derive(Default)]
pub(super) struct Insertion {
    parts: RefCell<Option<(Retained<NSBox>, Retained<NSBox>)>>,
}

impl Insertion {
    /// Takes the line away.
    pub(super) fn clear(&self) {
        if let Some((line, ring)) = self.parts.take() {
            line.removeFromSuperview();
            ring.removeFromSuperview();
        }
    }

    /// Stands the line with its middle at `y` (the list's space) along rows `row_width` wide, or
    /// takes it away (`None`).
    fn set(&self, list: &NSView, at: Option<f64>, row_width: f64, palette: Palette) {
        let Some(y) = at else {
            self.clear();
            return;
        };
        let mut parts = self.parts.borrow_mut();
        let (line, ring) = parts.get_or_insert_with(|| {
            let mtm = list.mtm();
            let line = plain_box(mtm, Tint::of(palette.accent, 1.0), tabs::GAP / 2.0);
            let ring = plain_box(mtm, Tint::of(palette.accent, 0.0), RING / 2.0);
            ring.setBorderWidth(RING_LINE);
            list.addSubview(&line);
            list.addSubview(&ring);
            (line, ring)
        });
        let accent = Tint::of(palette.accent, 1.0).color();
        line.setFillColor(&accent);
        ring.setBorderColor(&accent);
        // The ring hangs out of the row's edge by its half, the line begins where it ends.
        let ring_x = tabs::EDGE - RING_LINE - 1.0;
        let line_x = ring_x + RING;
        ring.setFrame(NSRect::new(
            NSPoint::new(ring_x, y - RING / 2.0),
            NSSize::new(RING, RING),
        ));
        ring.setHidden(false);
        line.setFrame(NSRect::new(
            NSPoint::new(line_x, y - tabs::GAP / 2.0),
            NSSize::new((tabs::EDGE + row_width - line_x).max(0.0), tabs::GAP),
        ));
    }

    /// Stands the line upright with its middle at `x` (the strip's space), from `top` down
    /// `height`, with no ring; or takes it away (`None`).
    pub(super) fn set_upright(
        &self,
        list: &NSView,
        at: Option<f64>,
        top: f64,
        height: f64,
        palette: Palette,
    ) {
        let Some(x) = at else {
            self.clear();
            return;
        };
        let mut parts = self.parts.borrow_mut();
        let (line, ring) = parts.get_or_insert_with(|| {
            let mtm = list.mtm();
            let line = plain_box(mtm, Tint::of(palette.accent, 1.0), UPRIGHT / 2.0);
            let ring = plain_box(mtm, Tint::of(palette.accent, 0.0), RING / 2.0);
            list.addSubview(&line);
            list.addSubview(&ring);
            (line, ring)
        });
        line.setFillColor(&Tint::of(palette.accent, 1.0).color());
        line.setCornerRadius(UPRIGHT / 2.0);
        ring.setHidden(true);
        line.setFrame(NSRect::new(
            NSPoint::new(x - UPRIGHT / 2.0, top),
            NSSize::new(UPRIGHT, height),
        ));
    }
}

/// The upright line's width between two chips.
const UPRIGHT: f64 = 2.0;

impl TabBar {
    /// Puts the bar in the side column (`Some(top)`: the title row's height, where the list
    /// starts) or back in the title row (`None`). A change of place forgets what belonged to the
    /// other: the scroll, the layout, a drag, a gap, the card.
    pub(crate) fn set_side(&self, top: Option<f64>) {
        let iv = self.ivars();
        let before = iv.side.replace(top);
        if before.is_some() == top.is_some() {
            return;
        }
        self.close_card();
        iv.scroll.set(0.0);
        iv.revealed.set(None);
        iv.overflowed.set(false);
        iv.width.set(0.0);
        iv.laid.replace(None);
        iv.column.replace(None);
        iv.drag.set(None);
        iv.gap.set(None);
        iv.target.set(None);
        iv.dwelling.set(None);
        iv.mark.set(None);
        iv.insertion.clear();
        if top.is_some() {
            iv.fade_leading.setHidden(true);
            iv.fade_trailing.setHidden(true);
            iv.list.setHidden(true);
            iv.switch.setHidden(true);
            self.close_group_list(false);
            // The column's head: bare glyphs, a face only under the pointer.
            iv.sidebar.setHidden(false);
            iv.eye.set_bare(true);
            iv.new_tab.set_bare(true);
        } else {
            // The bar's own: the list had no mask and no headers, the warning a square foot, `+`
            // its frame, and the chips a hidden column had put away come back.
            iv.mask.clear(&iv.strip);
            iv.warning.set_foot(None);
            iv.sidebar.setHidden(true);
            iv.eye.setHidden(true);
            iv.new_tab.set_bare(false);
            for button in iv.counts.take() {
                button.removeFromSuperview();
            }
            for chip in iv.chips.borrow().iter() {
                chip.setHidden(false);
            }
            for header in iv.headers.take() {
                header.removeFromSuperview();
            }
            iv.strip.setNeedsDisplay(true);
        }
    }

    /// Hides the column (the bar becomes the title row: the sidebar button, the name of the tab
    /// on screen, New Tab) or shows it again; the window lays out after. Only the side mode reads
    /// it; the window keeps the choice ([`crate::window::RootView::column_hidden`]).
    pub(crate) fn set_column_hidden(&self, hidden: bool) {
        let iv = self.ivars();
        if iv.hidden.replace(hidden) == hidden {
            return;
        }
        self.close_card();
        iv.drag.set(None);
        iv.gap.set(None);
        iv.target.set(None);
        iv.mark.set(None);
        iv.insertion.clear();
        iv.revealed.set(None);
        iv.sidebar.retip(self.mtm());
        if !hidden {
            for chip in iv.chips.borrow().iter() {
                chip.setHidden(false);
            }
        }
    }

    /// Whether the column hides ([`Self::set_column_hidden`]).
    pub(crate) fn column_hidden(&self) -> bool {
        self.ivars().hidden.get()
    }

    /// The title row while the column hides: the sidebar button where the column's head has it
    /// with the badge of what the tabs out of sight ask, the tab on screen's name centred on the
    /// window as a lone tab's is in the title row, and New Tab in the window's top-right corner
    /// (`tabs::Head::row`). Nothing else of the column shows: the other rows, the headers and the
    /// settings warning wait for the column to come back.
    pub(super) fn lay_out_head(&self, top: f64, shown: &Shown, chips: &[Retained<Chip>]) {
        let iv = self.ivars();
        let (Some(palette), Some(theme)) = (iv.palette.get(), iv.theme.get()) else {
            return;
        };
        let bounds = self.bounds();
        let corner = card::window_corner_pt(self.window().as_deref());
        let head = tabs::Head::row(bounds.size.width, top, corner);
        iv.strip.setFrame(NSRect::new(NSPoint::ZERO, bounds.size));
        iv.mask.clear(&iv.strip);
        for header in iv.headers.take() {
            header.removeFromSuperview();
        }
        iv.strip.set_separators(Vec::new(), palette);
        let reduce = self.reduce_motion();
        let motion = Motion::of(reduce);
        let visible = self
            .terminal_window()
            .is_some_and(|window| window.window_visible());
        let seen = Seen {
            theme,
            count: shown.tabs.len(),
            visible,
            reduce,
            style: IconStyle::current(self.mtm()),
            hints: self.hint_mods(),
        };
        let chip_top = ((top - super::CHIP_HEIGHT) / 2.0).max(0.0);
        for (index, chip) in chips.iter().enumerate() {
            let on_screen = index == shown.selected;
            chip.setHidden(!on_screen);
            if !on_screen {
                continue;
            }
            let label = &shown.tabs[index];
            let title = super::single_label(&label.title, &shown.notice);
            let look = seen.look(
                label,
                &title,
                Standing {
                    index,
                    naming: false,
                    selected: true,
                    hovered: false,
                    picked: false,
                    glowing: false,
                    single: true,
                    ghost: false,
                    side: false,
                },
            );
            chip.set(
                NSRect::new(
                    NSPoint::new(head.title.x, chip_top),
                    NSSize::new(head.title.width, super::CHIP_HEIGHT),
                ),
                &look,
                palette,
                motion,
                None,
            );
        }
        iv.sidebar.setFrame(ns_rect(head.sidebar));
        iv.sidebar.set_look(palette, TAB_RADIUS);
        iv.sidebar.set_bare(true);
        iv.sidebar.set_badge(shown.unseen);
        iv.eye.setHidden(true);
        // The badge says what waits while the column hides; its counts wait with it.
        for button in iv.counts.borrow().iter() {
            button.setHidden(true);
        }
        if let Some(new_tab) = head.new_tab {
            iv.new_tab.setFrame(ns_rect(new_tab));
            iv.new_tab.set_look(palette, TAB_RADIUS);
        }
        iv.warning.setHidden(true);
        iv.list.setHidden(true);
        iv.column.replace(None);
        self.arm_clock();
    }

    /// Whether the bar is the side column.
    pub(crate) fn is_side(&self) -> bool {
        self.ivars().side.get().is_some()
    }

    /// One header per group, in order, each keeping its group across changes of the list: a group
    /// keeps its header, a new group gets one and a group gone loses its own.
    fn headers_for(&self, headings: &[Heading]) -> Vec<Retained<GroupHeader>> {
        let iv = self.ivars();
        let mut old = iv.headers.take();
        let mut headers = Vec::with_capacity(headings.len());
        for heading in headings {
            match old
                .iter()
                .position(|header| header.group() == Some(heading.group))
            {
                Some(at) => headers.push(old.remove(at)),
                None => {
                    let header = GroupHeader::new(self.mtm(), heading.group);
                    iv.strip.addSubview(&header);
                    headers.push(header);
                }
            }
        }
        for gone in old {
            gone.removeFromSuperview();
        }
        iv.headers.replace(headers.clone());
        headers
    }

    /// Places every part from the pure column ([`Side::layout`]) and gives the chips their tabs
    /// as rows. [`TabBar::lay_out`]'s other half: the same things in the same order, a list that
    /// scrolls to the selection and slides there when the applier changed the list, and the
    /// clock set again after it.
    pub(super) fn lay_out_column(
        &self,
        top: f64,
        shown: &Shown,
        chips: &[Retained<Chip>],
        glowing: &[bool],
        reflow: bool,
        flight: Flight,
    ) {
        let iv = self.ivars();
        let (Some(palette), Some(theme)) = (iv.palette.get(), iv.theme.get()) else {
            return;
        };
        let count = shown.tabs.len();
        let bounds = self.bounds();
        let position = |tab: u64| shown.tabs.iter().position(|label| label.tab == tab);
        let hovered = iv.hovered.get().and_then(position);
        // The carried row: it waits, faint, in its place.
        let dragged = flight.drag.and_then(|drag| position(drag.tab));
        // The row's place in the layout: past the gap a carried tab opens, one on.
        let seat = |index: usize| tabs::seat(index, flight.gap);
        let side = self.side_for(top, count + usize::from(flight.gap.is_some()));
        // The list's places, headers among them: a carried tab's room opens before the row it is
        // over, below any header that stands there. In Activity a tab's row holds its agents.
        let entries = side_entries(&shown.headings, count, flight.gap, 0);
        let room =
            (self.bounds().size.width - 2.0 * tabs::EDGE - DETAIL_X - tabs::ROW_END).max(0.0);
        let shapes: Vec<RowShape> = shown
            .tabs
            .iter()
            .map(|label| RowShape::of(label, shown.activity, shown.details, room))
            .collect();
        let entries = tall_entries(entries, &shapes);
        let mut column = side.layout_with(&entries);
        let selected_tab = shown.tabs.get(shown.selected).map(|label| label.tab);
        let was_overflowing = iv.overflowed.replace(column.overflow);
        let resized = (iv.width.replace(bounds.size.height) - bounds.size.height).abs() > 0.5;
        let selection_changed = iv.revealed.replace(selected_tab) != selected_tab;
        // The selection comes into sight when it changes, and when the rows stop fitting or the
        // room they scroll in changes while they do not: only a change of the selection slides.
        let kept_in_sight = column.overflow && (!was_overflowing || resized);
        let mut revealed = false;
        if selection_changed || kept_in_sight {
            let target = column.revealing(seat(shown.selected));
            if target != column.scroll {
                column = Side {
                    scroll: target,
                    ..side
                }
                .layout_with(&entries);
                revealed = selection_changed;
            }
        }
        iv.scroll.set(column.scroll);

        let reduce = self.reduce_motion();
        let motion = Motion::of(reduce);
        let glide = iv.glide.take();
        let slide = ((reflow || revealed || glide) && motion.reflow > 0.0).then_some(motion.reflow);
        let visible = self
            .terminal_window()
            .is_some_and(|window| window.window_visible());
        let seen = Seen {
            theme,
            count,
            visible,
            reduce,
            style: IconStyle::current(self.mtm()),
            hints: self.hint_mods(),
        };
        // The rows live in the list's space: its top-left corner is theirs.
        iv.strip.setFrame(ns_rect(column.list));
        let picks = iv.picks.borrow().tabs.clone();
        // In Activity the keys count agents ([`tabs::agent_digit`]).
        let counts: Vec<usize> = shown.tabs.iter().map(|label| label.agents.len()).collect();
        for (index, chip) in chips.iter().enumerate() {
            let label = &shown.tabs[index];
            let naming = iv.renaming.get() == Some(label.tab);
            let shape = &shapes[index];
            let agents = shape.whole;
            // An agent's lines show their own names; the chip shows none.
            let title = if naming || agents {
                String::new()
            } else {
                label.title.clone()
            };
            let row = column.rows[seat(index)];
            let frame = NSRect::new(
                NSPoint::new(row.x - column.list.x, row.y - column.list.y),
                NSSize::new(row.width, row.height),
            );
            let look = seen.look(
                label,
                &title,
                Standing {
                    index,
                    naming,
                    selected: index == shown.selected,
                    hovered: (hovered == Some(index) || iv.target.get() == Some(label.tab))
                        && !naming,
                    picked: picks.contains(&label.tab) && !naming,
                    glowing: glowing[index],
                    single: false,
                    ghost: flight
                        .drag
                        .is_some_and(|drag| drag.torn && dragged == Some(index)),
                    side: true,
                },
            );
            let mut look = look;
            if shape.lead.is_some() {
                // A plain row with details: its parts in its band, the details under it.
                look.band = Some(tabs::TAB_HEIGHT);
            }
            // In Activity the keys count agents ([`tabs::agent_digit`]).
            let hints: Vec<Option<String>> = seen.hints.map_or_else(Vec::new, |mods| {
                (0..label.agents.len())
                    .map(|agent| {
                        tabs::agent_digit(index, agent, &counts)
                            .map(|digit| bt_shell_common::shortcuts::digit_glyphs(mods, digit))
                    })
                    .collect()
            });
            if agents {
                // The lines draw the tiles, the names and what each agent is doing. One agent's
                // row is the chip's as any row is — its face, its hover, its `×` and its key; a
                // tab of several agents is one face the lines draw, each line its own hover.
                look.emblem = None;
                look.indicator = None;
                if label.agents.len() > 1 {
                    look.hint = None;
                    look.selected = false;
                    look.hovered = false;
                } else {
                    look.hint = hints.first().cloned().flatten();
                }
            }
            chip.set(frame, &look, palette, motion, slide);
            let lines = if agents {
                Some(label.agents.as_slice())
            } else {
                shape
                    .lead
                    .map(|lead| std::slice::from_ref(&label.agents[lead]))
            };
            chip.ivars().agents.set(
                lines,
                AgentLook {
                    hints,
                    heights: shape.heights.clone(),
                    whole: agents,
                    details: shown.details,
                    selected: index == shown.selected,
                    palette,
                    style: seen.style,
                },
            );
            if naming {
                self.place_row_field(row, palette);
            }
        }
        let headers = self.headers_for(&shown.headings);
        for ((header, heading), laid) in headers.iter().zip(&shown.headings).zip(&column.headers) {
            let naming = iv.renaming_group.get() == Some(heading.group);
            let frame = NSRect::new(
                NSPoint::new(laid.rect.x - column.list.x, laid.rect.y - column.list.y),
                NSSize::new(laid.rect.width, laid.rect.height),
            );
            header.set(
                frame,
                heading,
                palette,
                HeaderLook {
                    hints: iv.group_hints.get(),
                    naming,
                    reduce,
                    slide,
                    torn: iv.group_torn.get() == Some(heading.group),
                },
            );
            if naming {
                self.place_group_field(laid.rect, palette);
            }
        }
        iv.strip.set_separators(Vec::new(), palette);
        let scale = self
            .window()
            .map_or(2.0, |window| window.backingScaleFactor());
        iv.mask.set(&iv.strip, &column, scale);

        iv.sidebar.setFrame(ns_rect(column.sidebar));
        iv.sidebar.set_look(palette, TAB_RADIUS);
        iv.sidebar.set_badge(None);
        iv.eye.setFrame(ns_rect(column.activity));
        iv.eye.set_look(palette, TAB_RADIUS);
        iv.eye.set_on(shown.activity);
        iv.eye.setHidden(!shown.agents);
        iv.new_tab.setFrame(ns_rect(column.new_tab));
        iv.new_tab.set_look(palette, TAB_RADIUS);
        iv.list.setHidden(true);
        match column.warning {
            Some(button) => {
                iv.warning.setFrame(ns_rect(button));
                iv.warning.set_look(palette, TAB_RADIUS);
                iv.warning.set_foot(Some(column.fit.radius));
                let text = NSString::from_str(&shown.notice);
                iv.warning.setToolTip(Some(&text));
                iv.warning.setAccessibilityLabel(Some(&text));
                iv.warning.setHidden(false);
            }
            None => iv.warning.setHidden(true),
        }
        self.place_counts(&column, &shown.counts, &shown.tabs, palette);
        iv.insertion.set(
            &iv.strip,
            // `between` is the gap's top; the line's middle is half a gap down. A line a carry
            // over the column named stands at its own height of the content, scrolled as the rows.
            iv.mark
                .get()
                .map(|gap| column.between(gap) + tabs::GAP / 2.0 - column.list.y)
                .or_else(|| iv.cast.get().map(|content| content - column.scroll)),
            column.row_width(),
            palette,
        );
        iv.column.replace(Some(column));
        // An open card follows its row (a scroll, a new height, a closed tab above).
        self.place_card();
        self.arm_clock();
    }

    /// The counts of what waits, in their row at the list's end ([`tabs::Column::counts`]): a
    /// button each, the words dropped when the three do not fit with them; none without any.
    fn place_counts(
        &self,
        column: &Column,
        counts: &[tabs::Count<u64>],
        labels: &[super::Label],
        palette: Palette,
    ) {
        let iv = self.ivars();
        let mut buttons = iv.counts.borrow_mut();
        while buttons.len() > counts.len() {
            if let Some(gone) = buttons.pop() {
                gone.removeFromSuperview();
            }
        }
        while buttons.len() < counts.len() {
            let button = TallyButton::new(self.mtm());
            self.addSubview(&button);
            buttons.push(button);
        }
        let Some(place) = column.counts else {
            return;
        };
        for button in buttons.iter() {
            button.setHidden(false);
        }
        let words = {
            let wide: f64 = counts
                .iter()
                .map(|count| TallyButton::width(count, true))
                .sum();
            wide + tabs::GAP * counts.len().saturating_sub(1) as f64 <= place.rect.width
        };
        let mut x = place.rect.x;
        for (index, (button, count)) in buttons.iter().zip(counts).enumerate() {
            let width = TallyButton::width(count, words);
            button.setFrame(NSRect::new(
                NSPoint::new(x, place.rect.y),
                NSSize::new(width, place.rect.height),
            ));
            x += width + tabs::GAP;
            // The tip names whom it waits for, the longest waiting first.
            let names: Vec<&str> = count
                .tabs
                .iter()
                .filter_map(|tab| labels.iter().find(|label| label.tab == *tab))
                .map(|label| label.title.as_str())
                .collect();
            let more = if names.len() > 3 { " …" } else { "" };
            let tip = format!(
                "{} {}: {}{more}",
                count.n,
                count.wait.word(),
                names[..names.len().min(3)].join(", ")
            );
            button.set(
                count,
                words,
                tip,
                (index == 0).then_some(place.corner).flatten(),
                palette,
            );
        }
    }

    /// The pure column's input for `count` places: the bar's width (the window gives the bar the
    /// column's), the window's height, where the list starts, the settings diagnostic and the
    /// corner, and the scroll as it stands ([`TabBar::bar_for`]'s upright half).
    pub(super) fn side_for(&self, top: f64, count: usize) -> Side {
        let iv = self.ivars();
        let bounds = self.bounds();
        Side {
            width: bounds.size.width,
            height: bounds.size.height,
            top,
            count,
            scroll: iv.scroll.get(),
            warning: !iv.shown.borrow().notice.is_empty(),
            tally: !iv.shown.borrow().counts.is_empty(),
            corner: card::window_corner_pt(self.window().as_deref()),
        }
    }

    /// The column as the window's list lays it out — without a drag's preview or a gap — with
    /// `room` more places than it has tabs: 0 is the column as it is, 1 the column a carried tab
    /// would make ([`TabBar::strip_with_room`]'s upright half).
    pub(super) fn column_with_room(&self, room: usize) -> Option<Column> {
        let top = self.ivars().side.get()?;
        let shown = self.ivars().shown.borrow();
        let count = shown.tabs.len();
        let entries = side_entries(&shown.headings, count, None, room);
        (count > 0).then(|| self.side_for(top, count + room).layout_with(&entries))
    }

    /// Puts the name field over `row` (the bar's space), in the type the selected row's title
    /// wears, left-aligned where the row's leading element starts: the field covers icon and
    /// title alike.
    fn place_row_field(&self, row: Rect, palette: Palette) {
        let field = &self.ivars().field;
        field.setAlignment(NSTextAlignment::Left);
        self.dress_field(palette, false);
        let height = field.intrinsicContentSize().height;
        let covers = tabs::Row::field(row.width);
        field.setFrame(NSRect::new(
            NSPoint::new(
                row.x + covers.x,
                row.y + ((row.height - height) / 2.0).max(0.0),
            ),
            NSSize::new(covers.width, height),
        ));
    }
}

impl TabBar {
    /// Puts the name field over the name of the header at `rect` (the bar's space), in the type
    /// the header's name wears, left-aligned where the name starts.
    fn place_group_field(&self, rect: Rect, palette: Palette) {
        let field = &self.ivars().field;
        field.setAlignment(NSTextAlignment::Left);
        self.dress_field(palette, false);
        // SAFETY: AppKit's constant font weight.
        let weight = unsafe { NSFontWeightSemibold };
        field.setFont(Some(&NSFont::systemFontOfSize_weight(
            super::tab_group::NAME_TEXT,
            weight,
        )));
        let height = field.intrinsicContentSize().height;
        field.setFrame(NSRect::new(
            NSPoint::new(
                rect.x + super::tab_group::NAME_X - 2.0,
                rect.y + ((rect.height - height) / 2.0).max(0.0),
            ),
            NSSize::new(
                (rect.width - super::tab_group::NAME_X - super::tab_group::END_PAD).max(0.0),
                height,
            ),
        ));
    }
}

pub(crate) struct TallyIvars {
    wait: Cell<Option<tabs::Wait>>,
    n: Cell<usize>,
    words: Cell<bool>,
    hot: Cell<bool>,
    pressed: Cell<bool>,
    /// The bottom-left corner's radius at the window's foot ([`tabs::Counts::corner`]).
    foot: Cell<Option<f64>>,
    palette: Cell<Option<Palette>>,
}

define_class!(
    /// One count at the side column's list end: the small indicator of what waits, how many and
    /// the word (dropped where the counts do not fit with it). A press goes to the tab that has
    /// waited longest, and round again from there (`TerminalWindow::go_to_waiting`).
    // SAFETY: NSView is designed for subclassing; TallyButton implements no `Drop` and is born
    // with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTallyButton"]
    #[ivars = TallyIvars]
    pub(crate) struct TallyButton;

    unsafe impl NSObjectProtocol for TallyButton {}

    impl TallyButton {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&objc2_app_kit::NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDownCanMoveWindow))]
        fn can_move_window(&self) -> bool {
            false
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &objc2_app_kit::NSEvent) {
            self.ivars().pressed.set(true);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &objc2_app_kit::NSEvent) {
            let pressed = self.ivars().pressed.replace(false);
            let at = self.convertPoint_fromView(event.locationInWindow(), None);
            let bounds = self.bounds();
            let inside = at.x >= 0.0
                && at.y >= 0.0
                && at.x <= bounds.size.width
                && at.y <= bounds.size.height;
            if pressed && inside {
                self.act();
            }
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &objc2_app_kit::NSEvent) {
            self.ivars().hot.set(true);
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &objc2_app_kit::NSEvent) {
            self.ivars().hot.set(false);
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            self.draw();
        }
    }
);

/// A count's inner spaces: its sides, after the indicator, after the number.
const TALLY_PAD: f64 = 8.0;
const TALLY_IN: f64 = 4.0;
const TALLY_WORD_GAP: f64 = 3.0;
/// The count's type, points.
const TALLY_TEXT: f64 = 12.0;

impl TallyButton {
    pub(super) fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(TallyIvars {
            wait: Cell::new(None),
            n: Cell::new(0),
            words: Cell::new(true),
            hot: Cell::new(false),
            pressed: Cell::new(false),
            foot: Cell::new(None),
            palette: Cell::new(None),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        super::track_hover(&this);
        this
    }

    /// How wide `count`'s button is, with its word or without it.
    pub(super) fn width(count: &tabs::Count<u64>, words: bool) -> f64 {
        let text = |text: &str, strong: bool| {
            let (_, attributes) = line_type(TALLY_TEXT, strong, 0, NSLineBreakMode::ByClipping);
            // SAFETY: the dictionary holds a font, a colour and a paragraph style under their
            // keys.
            unsafe { NSString::from_str(text).sizeWithAttributes(Some(&attributes)) }
                .width
                .ceil()
        };
        let word = if words {
            TALLY_WORD_GAP + text(count.wait.word(), false)
        } else {
            0.0
        };
        2.0 * TALLY_PAD + tabs::INDICATOR_SIDE + TALLY_IN + text(&count.n.to_string(), true) + word
    }

    fn set(
        &self,
        count: &tabs::Count<u64>,
        words: bool,
        tip: String,
        foot: Option<f64>,
        palette: Palette,
    ) {
        let iv = self.ivars();
        let changed = [
            iv.wait.replace(Some(count.wait)) != Some(count.wait),
            iv.n.replace(count.n) != count.n,
            iv.words.replace(words) != words,
            iv.foot.replace(foot) != foot,
            iv.palette.replace(Some(palette)) != Some(palette),
        ]
        .contains(&true);
        self.setToolTip(Some(&NSString::from_str(&tip)));
        self.setAccessibilityLabel(Some(&NSString::from_str(&tip)));
        if changed {
            self.setNeedsDisplay(true);
        }
    }

    fn draw(&self) {
        let iv = self.ivars();
        let (Some(palette), Some(wait)) = (iv.palette.get(), iv.wait.get()) else {
            return;
        };
        let bounds = self.bounds();
        if iv.hot.get() {
            let radius = TAB_RADIUS;
            rounded_corners(
                bounds,
                [radius, radius, radius, iv.foot.get().unwrap_or(radius)],
                palette.hover,
                None,
            );
        }
        let middle = bounds.size.height / 2.0;
        super::draw_bare(
            wait.indicator(),
            false,
            NSPoint::new(TALLY_PAD, middle - tabs::INDICATOR_SIDE / 2.0),
            palette,
        );
        let x = TALLY_PAD + tabs::INDICATOR_SIDE + TALLY_IN;
        let number = iv.n.get().to_string();
        let room = (bounds.size.width - x).max(0.0);
        let row = |at: f64, text: &str, strong: bool, color: u32| {
            draw_line(
                text,
                NSRect::new(NSPoint::new(at, 0.0), NSSize::new(room, bounds.size.height)),
                TALLY_TEXT,
                strong,
                color,
            );
        };
        row(x, &number, true, palette.title);
        if iv.words.get() {
            let (_, attributes) = line_type(TALLY_TEXT, true, 0, NSLineBreakMode::ByClipping);
            // SAFETY: as in `width`.
            let wide = unsafe { NSString::from_str(&number).sizeWithAttributes(Some(&attributes)) }
                .width
                .ceil();
            row(x + wide + TALLY_WORD_GAP, wait.word(), false, palette.dim);
        }
    }

    fn act(&self) {
        let wait = self.ivars().wait.get();
        // SAFETY: reading the superview; we are on the main thread.
        let bar = unsafe { self.superview() }.and_then(|view| view.downcast::<TabBar>().ok());
        if let (Some(window), Some(wait)) = (bar.and_then(|bar| bar.terminal_window()), wait) {
            window.go_to_waiting(Some(wait));
        }
        self.ivars().hot.set(false);
        self.setNeedsDisplay(true);
    }
}

/// Where a row's text starts, from the row's left edge: after the tile and its gap.
const DETAIL_X: f64 = tabs::ROW_LEAD + tabs::TILE_WIDTH + tabs::ICON_GAP + 2.0;

/// How a listed tab's row is made: in Activity its agents' lines are the row (`whole`), each of
/// its `heights`; in the plain list with details its band is a chip's and the details of its
/// `lead` agent — the one asking, else the focused one — stand under it; otherwise it is a
/// plain row.
#[derive(Clone, Debug, PartialEq)]
struct RowShape {
    whole: bool,
    lead: Option<usize>,
    heights: Vec<f64>,
    height: f64,
}

impl RowShape {
    fn of(label: &super::Label, activity: bool, details: bool, room: f64) -> Self {
        let height_of = |line: &AgentLine, top: f64| {
            let asks = line.asks.as_deref().map(AgentRows::ask_width);
            tabs::detail_height(
                top,
                details && line.place.is_some(),
                if details {
                    tabs::ask_lines(asks, room)
                } else {
                    0
                },
            )
        };
        if activity && !label.agents.is_empty() {
            let heights: Vec<f64> = label
                .agents
                .iter()
                .map(|line| height_of(line, tabs::AGENT_ROW))
                .collect();
            let height = heights.iter().sum();
            return Self {
                whole: true,
                lead: None,
                heights,
                height,
            };
        }
        let lead = label
            .agents
            .iter()
            .position(|line| line.tone == AgentTone::Asking)
            .or_else(|| label.agents.iter().position(|line| line.focused))
            .filter(|_| details);
        let height = lead.map_or(tabs::TAB_HEIGHT, |lead| {
            height_of(&label.agents[lead], tabs::TAB_HEIGHT)
        });
        // A lead with nothing to say is a plain row.
        let lead = lead.filter(|_| height > tabs::TAB_HEIGHT);
        Self {
            whole: false,
            lead,
            heights: vec![height],
            height,
        }
    }
}

/// The listed tabs' rows: each tab's [`Entry::Tab`] in `entries` takes its shape's height
/// ([`RowShape`]); a plain row stays [`Entry::Tab`].
fn tall_entries(entries: Vec<Entry>, shapes: &[RowShape]) -> Vec<Entry> {
    let mut rows = shapes.iter();
    entries
        .into_iter()
        .map(|entry| match entry {
            Entry::Tab => rows.next().map_or(entry, |shape| {
                if shape.whole || shape.lead.is_some() {
                    Entry::Tall(shape.height)
                } else {
                    Entry::Tab
                }
            }),
            entry => entry,
        })
        .collect()
}

pub(crate) struct AgentIvars {
    lines: RefCell<Vec<AgentLine>>,
    /// Each line's height ([`tabs::detail_height`]); the lines stand one under the other.
    heights: RefCell<Vec<f64>>,
    /// The lines are the row (Activity): the face, the tile, the name and the status are drawn
    /// here. Otherwise the row is the chip's own and only the details under its band are.
    whole: Cell<bool>,
    /// The details under each line: where it stands and what it asks.
    details: Cell<bool>,
    /// The ⌘ key that reaches each line, while Select Tab's modifiers are held.
    hints: RefCell<Vec<Option<String>>>,
    /// The tab is the one on screen: its focused pane's line is selected.
    selected: Cell<bool>,
    palette: Cell<Option<Palette>>,
    style: Cell<Option<IconStyle>>,
    /// The line under the pointer: its title in full ink and, among several, its own face.
    hot: Cell<Option<usize>>,
}

define_class!(
    /// A tab's agents in the side column's Activity, over its chip: a line each, [`tabs::AGENT_ROW`]
    /// high — the program's tile, what it calls its work and, under it, what it is doing and for
    /// how long — on one face when there are two or more, the focused pane's line selected while
    /// the tab is on screen. It draws, the chip takes the press ([`AgentRows::pane_at`]).
    // SAFETY: NSView is designed for subclassing; AgentRows implements no `Drop` and is born with
    // `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriAgentRows"]
    #[ivars = AgentIvars]
    pub(crate) struct AgentRows;

    unsafe impl NSObjectProtocol for AgentRows {}

    impl AgentRows {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// The chip under it takes every press.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, event: &objc2_app_kit::NSEvent) {
            self.hover_at(event);
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &objc2_app_kit::NSEvent) {
            self.hover_at(event);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &objc2_app_kit::NSEvent) {
            if self.ivars().hot.replace(None).is_some() {
                self.setNeedsDisplay(true);
            }
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            self.draw();
        }
    }
);

impl AgentRows {
    pub(super) fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(AgentIvars {
            lines: RefCell::new(Vec::new()),
            heights: RefCell::new(Vec::new()),
            whole: Cell::new(true),
            details: Cell::new(false),
            hints: RefCell::new(Vec::new()),
            selected: Cell::new(false),
            palette: Cell::new(None),
            style: Cell::new(None),
            hot: Cell::new(None),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setHidden(true);
        // The line under the pointer is its own: entering, moving within and leaving.
        let options = objc2_app_kit::NSTrackingAreaOptions::MouseEnteredAndExited
            | objc2_app_kit::NSTrackingAreaOptions::MouseMoved
            | objc2_app_kit::NSTrackingAreaOptions::ActiveInActiveApp
            | objc2_app_kit::NSTrackingAreaOptions::InVisibleRect;
        // SAFETY: `owner` is the view, which owns the area and outlives it; no user info.
        // `InVisibleRect` follows the view's bounds, so the rect is ignored.
        let area = unsafe {
            objc2_app_kit::NSTrackingArea::initWithRect_options_owner_userInfo(
                objc2_app_kit::NSTrackingArea::alloc(),
                NSRect::ZERO,
                options,
                Some(&this),
                None,
            )
        };
        this.addTrackingArea(&area);
        this
    }

    /// Takes the lines away: the chip is no row of the column (the title row's chip, the hidden
    /// column's title), whatever the last layout of the column gave it.
    pub(super) fn hide(&self) {
        self.setHidden(true);
        self.ivars().hot.set(None);
    }

    /// The pointer moved over the lines: the one under it is the hot one.
    fn hover_at(&self, event: &objc2_app_kit::NSEvent) {
        let at = self.convertPoint_fromView(event.locationInWindow(), None);
        let hot = self.line_at(at.y);
        if self.ivars().hot.replace(hot) != hot {
            self.setNeedsDisplay(true);
        }
    }

    /// The index of the line at `y` (the view's own, top-down); the last one below the end.
    fn line_at(&self, y: f64) -> Option<usize> {
        let iv = self.ivars();
        let count = iv.lines.borrow().len();
        let heights = iv.heights.borrow();
        let mut top = 0.0;
        for (index, height) in heights.iter().take(count).enumerate() {
            top += height;
            if y < top {
                return Some(index);
            }
        }
        count.checked_sub(1)
    }

    /// Shows the tab's `lines` (`None` hides them: the chip is a row of its own again), each
    /// of its `heights`; `whole` while they are the row (Activity), else only their details are
    /// drawn, under the chip's band.
    pub(super) fn set(&self, lines: Option<&[AgentLine]>, look: AgentLook) {
        let iv = self.ivars();
        let Some(lines) = lines else {
            self.setHidden(true);
            return;
        };
        let AgentLook {
            hints,
            heights,
            whole,
            details,
            selected,
            palette,
            style,
        } = look;
        // Every field is written whatever came before it: `|`, not `||` — a first set with new
        // lines left the palette unset and drew nothing until the next layout.
        let changed = (*iv.lines.borrow() != lines)
            | (*iv.hints.borrow() != hints)
            | (*iv.heights.borrow() != heights)
            | (iv.whole.replace(whole) != whole)
            | (iv.details.replace(details) != details)
            | (iv.selected.replace(selected) != selected)
            | (iv.palette.replace(Some(palette)) != Some(palette))
            | (iv.style.replace(Some(style)) != Some(style));
        iv.lines.replace(lines.to_vec());
        iv.hints.replace(hints);
        iv.heights.replace(heights);
        self.setHidden(false);
        if changed {
            self.setNeedsDisplay(true);
        }
    }

    /// Whether the lines are the row and take its press ([`Self::pane_at`]).
    pub(super) fn takes_press(&self) -> bool {
        !self.isHidden() && self.ivars().whole.get()
    }

    /// The pane whose line is at `y` (the view's own, top-down); the last one below the end.
    pub(super) fn pane_at(&self, y: f64) -> Option<u64> {
        let index = self.line_at(y)?;
        self.ivars().lines.borrow().get(index).map(|line| line.pane)
    }

    /// How wide `text` is in the type a question is drawn in ([`tabs::ask_lines`] measures by it).
    pub(super) fn ask_width(text: &str) -> f64 {
        let (_, attributes) = line_type(DETAIL_TEXT, false, 0, NSLineBreakMode::ByWordWrapping);
        // SAFETY: the dictionary holds a font, a colour and a paragraph style under their keys.
        unsafe { NSString::from_str(text).sizeWithAttributes(Some(&attributes)) }.width
    }

    fn draw(&self) {
        let iv = self.ivars();
        let (Some(palette), Some(style)) = (iv.palette.get(), iv.style.get()) else {
            return;
        };
        let lines = iv.lines.borrow();
        let hints = iv.hints.borrow();
        let heights = iv.heights.borrow();
        let bounds = self.bounds();
        let width = bounds.size.width;
        let x = tabs::ROW_LEAD + tabs::TILE_WIDTH + tabs::ICON_GAP + 2.0;
        if !iv.whole.get() {
            // A plain row: its band is the chip's, the details stand under it.
            if let Some(line) = lines.first() {
                let room = (width - x - tabs::ROW_END).max(0.0);
                self.draw_details(line, x, tabs::TAB_HEIGHT - 5.0, room, palette);
            }
            return;
        }
        let bundled = lines.len() > 1;
        // A tab's agents share a face; the picked line and a hovered one lie on it, together the
        // selected face (10 %) and a hover of 7 %. One agent's row is the chip's: its face, its
        // hover, its `×` and its key are the chip's.
        if bundled {
            rounded(bounds, TAB_RADIUS, Tint::of(palette.title, 0.04), None);
        }
        let hot = iv.hot.get();
        let mut next = 0.0;
        for (index, line) in lines.iter().enumerate() {
            let height = heights.get(index).copied().unwrap_or(tabs::AGENT_ROW);
            let top = next;
            next += height;
            let row = NSRect::new(NSPoint::new(0.0, top), NSSize::new(width, height));
            let strong = iv.selected.get() && (line.focused || !bundled);
            let lit = hot == Some(index) && !strong;
            if bundled && strong {
                rounded(
                    row,
                    TAB_RADIUS,
                    Tint::of(palette.title, 0.0625),
                    Some(palette.selected_line),
                );
            } else if bundled && lit {
                rounded(row, TAB_RADIUS, Tint::of(palette.title, 0.03125), None);
            }
            // The program's tile, where a row's stands, centred on the line.
            let slot = glyph_slot();
            NSGraphicsContext::saveGraphicsState_class();
            let shift = NSAffineTransform::transform();
            shift.translateXBy_yBy(
                tabs::ROW_LEAD - slot.origin.x,
                top + (tabs::AGENT_ROW - tabs::TILE_HEIGHT) / 2.0 - slot.origin.y,
            );
            shift.concat();
            draw_tile(
                Emblem {
                    kind: line.kind,
                    edges: 0,
                    face: None,
                },
                style,
                palette,
            );
            NSGraphicsContext::restoreGraphicsState_class();
            // A line of several draws its own key; one agent's is the chip's, which also shows
            // its `×` under the pointer: the title gives way to either.
            let hint = hints.get(index).cloned().flatten().filter(|_| bundled);
            let crowded = if bundled {
                hint.is_some()
            } else {
                hot.is_some() || hints.first().is_some_and(Option::is_some)
            };
            let end = if crowded {
                tabs::ROW_CROWDED
            } else {
                tabs::ROW_END
            };
            let room = (width - x - end).max(0.0);
            draw_line(
                &line.title,
                NSRect::new(NSPoint::new(x, top + 4.5), NSSize::new(room, 18.0)),
                super::CHIP_TEXT,
                strong,
                if strong || hot == Some(index) {
                    palette.title
                } else {
                    palette.dim
                },
            );
            let tone = match line.tone {
                AgentTone::Quiet => palette.dim,
                AgentTone::Asking => palette.warning,
                AgentTone::Failed => palette.error,
            };
            draw_line(
                &line.status,
                NSRect::new(NSPoint::new(x, top + 23.0), NSSize::new(room, 16.0)),
                DETAIL_TEXT,
                false,
                tone,
            );
            self.draw_details(line, x, top + tabs::AGENT_ROW - 5.0, room, palette);
            if let Some(hint) = hint {
                draw_line(
                    &hint,
                    NSRect::new(
                        NSPoint::new(width - tabs::CLOSE_INSET - tabs::CHIP_PAD, top + 12.0),
                        NSSize::new(tabs::CHIP_PAD, 20.0),
                    ),
                    11.0,
                    false,
                    palette.dim,
                );
            }
        }
    }
}

impl AgentRows {
    /// A line's details from `base` down: where it stands, in the dim ink, and what it asks, in
    /// the title's, cut at [`tabs::ASK_LINES`] lines.
    fn draw_details(&self, line: &AgentLine, x: f64, base: f64, room: f64, palette: Palette) {
        if !self.ivars().details.get() {
            return;
        }
        let mut at = base;
        if let Some(place) = &line.place {
            draw_line(
                place,
                NSRect::new(NSPoint::new(x, at), NSSize::new(room, tabs::PLACE_ROW)),
                DETAIL_TEXT,
                false,
                palette.dim,
            );
            at += tabs::PLACE_ROW;
        }
        if let Some(asks) = &line.asks {
            let lines = tabs::ask_lines(Some(Self::ask_width(asks)), room);
            let (_, attributes) = line_type(
                DETAIL_TEXT,
                false,
                palette.title,
                NSLineBreakMode::ByWordWrapping,
            );
            let rect = NSRect::new(
                NSPoint::new(x, at),
                NSSize::new(room, lines as f64 * tabs::ASK_ROW),
            );
            // SAFETY: the dictionary holds a font, a colour and a paragraph style under their
            // keys; the text wraps in the box and what is past its foot is not drawn.
            unsafe { NSString::from_str(asks).drawInRect_withAttributes(rect, Some(&attributes)) };
        }
    }
}

/// The type of a detail's line and of the status, points.
const DETAIL_TEXT: f64 = 11.0;

/// The font and the attributes of a line of `size` points, semibold when `strong`, else medium,
/// in `color`, broken as `breaking` says.
fn line_type(
    size: f64,
    strong: bool,
    color: u32,
    breaking: NSLineBreakMode,
) -> (
    Retained<NSFont>,
    Retained<NSDictionary<NSAttributedStringKey, AnyObject>>,
) {
    // SAFETY: AppKit's constant font weights.
    let weight = unsafe {
        if strong {
            NSFontWeightSemibold
        } else {
            NSFontWeightMedium
        }
    };
    let font = NSFont::systemFontOfSize_weight(size, weight);
    let paragraph = NSMutableParagraphStyle::new();
    paragraph.setLineBreakMode(breaking);
    let ink = Tint::of(color, 1.0).color();
    // SAFETY: AppKit's constant attribute keys, alive for the process.
    let keys: [&NSAttributedStringKey; 3] = unsafe {
        [
            NSFontAttributeName,
            NSForegroundColorAttributeName,
            NSParagraphStyleAttributeName,
        ]
    };
    let values: [&AnyObject; 3] = [&font, &ink, &paragraph];
    let attributes = NSDictionary::from_slices(&keys, &values);
    (font, attributes)
}

/// The look [`AgentRows::set`] is given: what the column decided for the tab's lines.
pub(super) struct AgentLook {
    pub(super) hints: Vec<Option<String>>,
    pub(super) heights: Vec<f64>,
    pub(super) whole: bool,
    pub(super) details: bool,
    pub(super) selected: bool,
    pub(super) palette: Palette,
    pub(super) style: IconStyle,
}

/// [`draw_line`] for the bar's other children: a pane's header writes its title with it.
pub(super) fn draw_title(text: &str, rect: NSRect, size: f64, strong: bool, color: u32) {
    draw_line(text, rect, size, strong, color);
}

/// One line of text in `rect` (top-down), cut with an ellipsis where it does not fit: `size`
/// points, semibold when `strong`, else medium.
fn draw_line(text: &str, rect: NSRect, size: f64, strong: bool, color: u32) {
    let (font, attributes) = line_type(size, strong, color, NSLineBreakMode::ByTruncatingTail);
    // Centred up and down in its box, as a label's single line is.
    let height = font.ascender() - font.descender();
    let at = NSRect::new(
        NSPoint::new(
            rect.origin.x,
            rect.origin.y + (rect.size.height - height) / 2.0,
        ),
        NSSize::new(rect.size.width, height),
    );
    // SAFETY: the dictionary holds a font, a colour and a paragraph style under their keys.
    unsafe { NSString::from_str(text).drawInRect_withAttributes(at, Some(&attributes)) };
}

/// What stands in the column's list, top to bottom, for `count` tabs listed under `headings`:
/// each header just above the row it stands over, and `room` more rows at the end. A tab carried
/// in from another window opens its room as a row **at** place `gap` — below any header that
/// stands over that place — and none when `gap` is `None`.
fn side_entries(headings: &[Heading], count: usize, gap: Option<usize>, room: usize) -> Vec<Entry> {
    let mut entries = Vec::with_capacity(count + headings.len() + room + 1);
    let mut headers = headings.iter().peekable();
    for index in 0..=count {
        while headers
            .next_if(|heading| heading.first_row <= index)
            .is_some()
        {
            entries.push(Entry::Header);
        }
        if gap == Some(index) {
            entries.push(Entry::Tab);
        }
        if index < count {
            entries.push(Entry::Tab);
        }
    }
    entries.extend(std::iter::repeat_n(Entry::Tab, room));
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::groups::{Groups, Target};
    use crate::tabs::Tabs;

    /// A heading over the row at `first_row`, for a group of its own.
    fn heading(first_row: usize) -> Heading {
        let mut tabs = Tabs::new(1u64);
        let mut groups = Groups::new();
        groups.place(&mut tabs, &[1], Target::New("g".to_owned()));
        Heading {
            group: groups.groups()[0].id,
            name: "g".to_owned(),
            collapsed: false,
            tabs: 1,
            indicator: None,
            hint: None,
            first_row,
        }
    }

    const TAB: Entry = Entry::Tab;
    const HEAD: Entry = Entry::Header;

    #[test]
    fn a_column_without_groups_is_rows_alone() {
        assert_eq!(side_entries(&[], 3, None, 0), [TAB, TAB, TAB]);
        assert_eq!(side_entries(&[], 3, None, 1), [TAB, TAB, TAB, TAB]);
    }

    #[test]
    fn a_header_stands_just_above_the_row_it_names() {
        let headings = [heading(2), heading(3)];
        assert_eq!(
            side_entries(&headings, 4, None, 0),
            [TAB, TAB, HEAD, TAB, HEAD, TAB]
        );
        // A closed group with nothing listed under it names the row past the last: its header
        // ends the list.
        assert_eq!(
            side_entries(&[heading(0), heading(2)], 2, None, 0),
            [HEAD, TAB, TAB, HEAD]
        );
    }

    #[test]
    fn a_carried_tabs_room_opens_below_the_header_that_stands_there() {
        let headings = [heading(2)];
        assert_eq!(
            side_entries(&headings, 3, Some(2), 0),
            [TAB, TAB, HEAD, TAB, TAB],
            "before the first row of the group: at the group's head"
        );
        assert_eq!(
            side_entries(&headings, 3, Some(1), 0),
            [TAB, TAB, TAB, HEAD, TAB],
            "among the ungrouped rows"
        );
        assert_eq!(
            side_entries(&headings, 3, Some(3), 0),
            [TAB, TAB, HEAD, TAB, TAB],
            "past the last row"
        );
    }
}
