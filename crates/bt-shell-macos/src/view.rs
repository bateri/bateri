//! The window's content: the view that carries the `CAMetalLayer` and streams the keyboard to the PTY.
//!
//! There is **no drawing** here - `bt-gpu` fills the layer's content. This
//! class's job is to be first responder, hand a keystroke to the right arm,
//! turn the mouse (press, drag, release and wheel) into a cell and pass it to
//! the session, drop the path of a file dropped from Finder onto the input
//! line, and answer the Edit menu's Copy/Paste actions. The terminal
//! decisions (the selection range, the page length, the wheel's path by
//! mode, the arrow's bytes) are in `bt-core`; what lives here is the side
//! facing AppKit - the pixel → cell arithmetic, the wheel's line remainder,
//! whether a drag is in progress.
//!
//! **The keyboard's text path now goes through AppKit's stack** and
//! `keyDown:` is not a single gate but an **arbitration**: a Cmd event is
//! swallowed except for one key of a closed allow list (⌘⌫), Shift+PgUp/PgDn
//! is the terminal's scrolling, a Control event goes straight to
//! [`crate::keys::encode_key`] and **the rest** is handed to the text stack
//! with `interpretKeyEvents:`. The stack keeps the dead-key state itself and
//! when the composition completes it hands the text back with `insertText:` -
//! we do not read the layout data. If the stack did not take the event
//! ([`ViewIvars::consumed`]) the event falls to `encode_key` anyway:
//! function keys, Enter/Tab/Esc/Backspace and everything unrecognised go
//! through it.
//!
//! **The view is also a drag destination** (`NSDraggingDestination`): the
//! path of a file dropped from Finder is escaped
//! ([`crate::quote::shell_quote`]) and lands on the input line through
//! `Session::paste`. Registration is with `NSPasteboardTypeFileURL` and
//! **only** it - a plain-text drop would make the escape rule conditional on
//! the type, and since Finder puts two types in a single drop the order of
//! the arms would become a decision too.

use std::cell::{Cell, OnceCell, RefCell};
use std::sync::Arc;

use bt_core::{
    CellHalf, Click, MouseButton, MouseModifiers, ScrollIntent, SearchCover, SelectionPoint,
    Session, Wheel,
};
use bt_gpu::{CellMetrics, Origin};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, ProtocolObject, Sel};
use objc2::{
    ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel,
};
use objc2_app_kit::{
    NSCursor, NSDragOperation, NSDraggingContext, NSDraggingDestination, NSDraggingInfo,
    NSDraggingSession, NSDraggingSource, NSEvent, NSEventModifierFlags, NSEventPhase, NSMenuItem,
    NSPasteboard, NSPasteboardTypeFileURL, NSTextInputClient, NSView,
};
use objc2_foundation::{
    NSArray, NSAttributedString, NSAttributedStringKey, NSNotFound, NSObjectProtocol, NSPoint,
    NSRange, NSRangePointer, NSRect, NSSize, NSString, NSUInteger, NSURL,
};

use crate::clipboard;
use crate::gesture::{Drag, Gesture, Press, Release};
use crate::hyperlink::LinkState;
use crate::keys::{
    ARROW_LEFT, ARROW_RIGHT, BACKSPACE, KeyInput, KeyPress, dock_key, encode_key, only_char,
    page_scroll,
};
use crate::pane::TerminalPane;
use crate::quote::{paste_quote, shell_quote};

/// What happens to a point that falls outside the grid - [`point_to_cell`]'s
/// single decision axis.
///
/// The rule is one sentence: **an event that starts a gesture is rejected,
/// the continuation of a running gesture is clamped.** A press and a
/// buttonless motion *state* a place, so a coordinate coming from the
/// window's title bar, left padding or the dock band would report a **wrong**
/// cell to the application; the coordinate of a drag and of a release is the
/// continuation of an already started gesture and there sticking to the edge
/// is both xterm's behaviour and a requirement (a dropped release leaves a
/// button stuck in the application).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OutOfGrid {
    /// Snap to the nearest cell. `fill_rows` is the fill band's length: above
    /// the origin, if it is **full**, still `None`, because there is drawn text there.
    Clamp { fill_rows: u16 },
    /// `None` if the point is outside `[0, cols) × [0, rows)`.
    Reject,
}

/// Mouse point → selection end. **Pure and AppKit-free**, so testable.
///
/// `view_px` is in view coordinates (points), `metrics` and `origin_px` are
/// physical pixels, `scale` is the backing scale: since the measure came
/// from `bt-gpu` in physical terms the mouse first goes up to physical
/// pixels, **subtracts the left padding and the vertical origin**, and then
/// divides. The padding comes from the same `CellMetrics` as the `cols`
/// computation (`split_into_grid`) and the drawing origin
/// (`Frame::pos_at`); had the three diverged the symptom would be "the mouse
/// is a column off".
///
/// `origin_px` is the vertical half of the same sentence and its source is
/// also single ([`bt_gpu::Origin`]): the **drawn** frame's origin, the value
/// the frame path wrote. Were there a second computation the symptom would
/// be "the mouse is a row off" and during the slide animation it
/// would be off by a different row every frame. A parameter, not a field: the
/// function stays pure and tests that do not care about the origin pass `0.0`.
///
/// `fill_rows` comes from the same body ([`bt_gpu::Origin`]) and for the same
/// reason: the two halves of the question "what is above the origin" - how
/// many pixels and is it blank - are the same frame's geometry. No second
/// synchronisation was built; the frame path writes, the mouse path reads,
/// both on the main thread.
///
/// A click that falls **inside** the padding is clamped to the first column,
/// so a selection does not start in the padding: after the subtraction x stays
/// negative and the two language rules below stick it to the left half of
/// cell 0 - the same path as a point left of the grid, there is no separate clamping arm.
///
/// **The name stayed "cell", what comes back is cell + half**: the half is
/// the second half of the place inside the cell, not a separate question - it
/// comes out of the same division as `col`. Its caller (`window_point_cell`:
/// mouse events and the mouse position during scroll) already says "the cell
/// under the mouse"; a second name (`point_to_selection_point`) would only be churn.
///
/// Every point outside the edge **sticks to the nearest cell**: whichever
/// side of the grid a drag leaves, it holds on to that edge. A point
/// overflowing to the right is the **right** half of the last column: when
/// the mouse dragged to the end of the line passes onto the unused strip at
/// the grid's right (`split_into_grid` rounds the column count down), the
/// last letter must stay in the selection.
///
/// There are **two** reasons for `None`: a grid with zero columns/rows (a
/// minimised window - there is no cell to stick to) and a point falling
/// **above** the origin while `fill_rows > 0`. The second is this function's
/// only **rejection**: when the fill band is drawn that area is not
/// blank, the scrollback's rows stand there and those rows cannot be
/// represented by the boundary's row numbers. The rejection does not replace
/// the clamping, it goes **beside** it - with `fill_rows == 0` a point
/// overflowing upward sticks to row 0 as today and must: that area really is
/// blank, and besides the real protection against `u16` overflow is in that
/// clamping (`the_origin_shifts_the_grid_down_and_the_blank_area_clamps`).
///
/// Floor rounding (the `as u16` truncation): the question is **which** cell
/// the mouse is in and the arithmetic is the same as `split_into_grid`. The
/// left/top sticking is not a separate clamp, it is two rules of the
/// language: `f64 as u16` **saturates** a negative to 0 (does not wrap), and
/// `f64`'s `%` keeps the dividend's sign - the remainder of a negative x is
/// negative, so always less than half a cell and in the **left** half. A drag
/// starting at the grid's left thus includes cell 0 in the selection; a
/// "fix" moving to `rem_euclid` would turn the remainder positive and leave
/// it out (the `dragging_left_of_the_grid_clamps_to_the_left_half` guard).
pub(crate) fn point_to_cell(
    view_px: (f64, f64),
    metrics: CellMetrics,
    origin_px: f64,
    outside: OutOfGrid,
    scale: f64,
    cols: u16,
    rows: u16,
) -> Option<SelectionPoint> {
    if cols == 0 || rows == 0 {
        return None;
    }
    let (cell_px_w, cell_px_h) = metrics.cell_px();
    let (cell_w, cell_h) = (f64::from(cell_px_w), f64::from(cell_px_h));
    // The view is `isFlipped`, so y arrives in the grid's direction (from the
    // top): no flipping back. The grid's size is told by `cols`/`rows`, not
    // the view - a point in the window margin sticks to the last cell.
    let x = view_px.0 * scale - f64::from(metrics.gutter_px());
    // The vertical origin is subtracted the same way as the padding and **in
    // `f64`**: with bottom-sticking the blank area is **at the top** and on a
    // click there the difference goes negative. Done in `u16` it would
    // overflow and a click on the window's upper half would select the last
    // row; in `f64` it stays negative and `as u16` **saturates** it to zero -
    // the same path the padding uses horizontally, no separate clamping arm.
    let y = view_px.1 * scale - origin_px;
    match outside {
        // **Above the origin, if full, rejection, not clamping.** Clamping is
        // right only when that area is *blank*: when the fill band is drawn the
        // user sees text there and an anchor sticking to row 0 would put the
        // highlight somewhere other than where the eye sees it. The filled rows
        // cannot be represented by the boundary's row numbers (all in the
        // scrollback, so negative) - between "selected wrongly" and "cannot be
        // selected" the second is the honest one.
        OutOfGrid::Clamp { fill_rows } if fill_rows > 0 && y < 0.0 => return None,
        OutOfGrid::Clamp { .. } => {}
        OutOfGrid::Reject
            if x < 0.0
                || y < 0.0
                || x >= cell_w * f64::from(cols)
                || y >= cell_h * f64::from(rows) =>
        {
            return None;
        }
        OutOfGrid::Reject => {}
    }
    let row = ((y / cell_h) as u16).min(rows - 1);
    let col = (x / cell_w) as u16;
    let (col, half) = if col < cols {
        (col, cell_half(x, cell_w))
    } else {
        (cols - 1, CellHalf::Right)
    };
    Some(SelectionPoint { col, row, half })
}

/// The cells the search panel covers - **pure**, with [`point_to_cell`]'s
/// arithmetic: the panel's bottom edge and left edge in physical pixels (from
/// the view's top and left), `origin_px` the drawn frame's vertical origin
/// ([`bt_gpu::Origin`]).
///
/// The first fully visible row is rounded **up** (ceiling) below the panel: a
/// row whose half stays under the panel counts as covered. It can be
/// negative - the fill band's rows above the origin. The column is **floor**:
/// the cell in which the panel's left edge falls is covered.
pub(crate) fn cover_of(
    bottom_px: f64,
    left_px: f64,
    origin_px: f64,
    metrics: CellMetrics,
) -> SearchCover {
    let (cell_w, cell_h) = metrics.cell_px();
    let (cell_w, cell_h) = (f64::from(cell_w.max(1)), f64::from(cell_h.max(1)));
    let first_row = ((bottom_px - origin_px) / cell_h).ceil();
    let from_col = ((left_px - f64::from(metrics.gutter_px())) / cell_w).floor();
    SearchCover {
        // `as` saturates: no overflow in a giant window either.
        first_row: first_row as i32,
        from_col: from_col.max(0.0) as u16,
    }
}

/// The top of the dock's input line, in the view's physical pixels (from the top).
///
/// The dock band is at the window's bottom and its size is `bt-gpu`'s formula
/// ([`bt_gpu::dock_px`]; the **single** copy `split_into_grid` and the second
/// viewport use), the input line is below the band's breathing padding - the
/// padding's source is the left padding ([`CellMetrics::gutter_px`],
/// `Frame::dock_pos`). The line is given to [`point_to_cell`] with this value
/// as **a one-row grid**: the column and half arithmetic comes from the same
/// body as the grid's, and so does the press's rejection ("the context line
/// and the band's padding do nothing") and the drag's clamping ("into the line").
/// Pure, testable.
pub(crate) fn dock_input_top_px(height_px: f64, metrics: CellMetrics, dock_rows: u16) -> f64 {
    height_px - f64::from(bt_gpu::dock_px(dock_rows, metrics)) + f64::from(metrics.gutter_px())
}

/// The half of x within a cell - the single input that draws the selection boundary.
///
/// The half **cannot be derived from `col`**: `col` truncates to an integer
/// and discards the truncation remainder, so the information about where in
/// the cell we are is not there. The source is the **remainder** before the
/// division (x relative to `cell_w`). For a negative x the remainder is also
/// negative and falls to the left half - the left edge rule is in [`point_to_cell`].
///
/// **The midpoint is written to the right half** (`>=`): the two halves
/// partition exactly as `[0, w/2)` and `[w/2, w)` - no x is left without a
/// half, none falls into both halves and the rule becomes a single
/// comparison. Pressing exactly on the middle (when the mouse pixel falls
/// exactly on the boundary) leaves the cell **outside** at the start end and
/// **inside** at the end end - that is the meaning of the right half at the two ends
/// ([`CellHalf`]).
fn cell_half(x_px: f64, cell_w: f64) -> CellHalf {
    if x_px % cell_w >= cell_w / 2.0 {
        CellHalf::Right
    } else {
        CellHalf::Left
    }
}

/// Wheel delta → whole lines and the **carried remainder**. Pure, testable.
///
/// `unit` is the size of one line in delta terms: on a trackpad
/// (`hasPreciseScrollingDeltas`) the delta arrives in points and the unit is
/// the cell height (points); on a classic wheel the delta is already lines
/// and the unit is 1. The sign is kept - AppKit's `scrollingDeltaY` has the
/// "natural scrolling" preference applied and its positive is toward the
/// start of the document, i.e. the same as `Session::scroll_wheel`'s
/// "positive is backward" direction.
///
/// **Why the remainder is carried:** a trackpad showers deltas smaller than a
/// cell height; had each event been truncated to zero alone, a slow scroll
/// would never produce a line. The truncation is toward zero (`trunc`), the
/// remainder keeps its sign: when the direction reverses the accumulated
/// remainder melts first.
///
/// A non-finite total (zero unit, NaN delta) gives `(0, 0.0)` - had NaN
/// entered the remainder every later total would be NaN and the wheel would
/// silently die. A giant delta saturates with `as i32`; clamping to the
/// scrollback's length is in `bt-core`.
pub(crate) fn wheel_lines(delta: f64, unit: f64, carry: f64) -> (i32, f64) {
    let total = carry + delta / unit;
    if !total.is_finite() {
        return (0, 0.0);
    }
    let whole = total.trunc();
    (whole as i32, total - whole)
}

/// What a wheel event on the smooth path carries to `Session::scroll_wheel`:
/// the fractional amount, the same event's whole lines and the intent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SmoothWheel {
    /// The scroll arm's amount, in lines (positive is backward).
    pub(crate) rows: f64,
    /// The arrow and report arms' amount ([`wheel_lines`]'s whole lines).
    pub(crate) lines: i32,
    pub(crate) intent: ScrollIntent,
}

/// A wheel event's **intent** - pure, `NSEvent`-free, testable (the arm of
/// `smooth_scroll = "on"`; `"off"` never visits this function).
///
/// The separator is the **gesture phase**, not the delta's precision: an
/// event with a phase (trackpad, Magic Mouse) tracks the finger, an event
/// without one (classic wheel) is a notch. A phaseless but precise event
/// (synthetic events of external scrollers) also counts as a notch: since it
/// carries no phase that says when it ends, had it been tracked directly the
/// window would rest at half a line.
///
/// - **Gesture start** (`phase` `Began`/`MayBegin`, `momentum` `Began`):
///   [`ScrollIntent::GestureBegan`] - the finger touched again or momentum
///   began, an in-flight settling must end.
/// - **Gesture end** (`Ended`/`Cancelled`, in either phase):
///   [`ScrollIntent::Settle`] - settling to the nearest line. If momentum is
///   coming, its `Began` ends the settling; since the model is relative there
///   is no jump and no timer or threshold is needed.
/// - **In between** (`Changed`/`Stationary`): [`ScrollIntent::Direct`].
/// - **Notch**: [`ScrollIntent::Glide`] and its amount is **whole lines** - a
///   fractional notch target would leave the window at half a line and no
///   gesture end comes to settle it. The distance is thus the same as the
///   `"off"` arm's; only the gliding differs.
///
/// `lines` is from [`wheel_lines`] in every arm, because the route is chosen
/// in `bt-core` and the arrow/report arm reads it. The return is `(step, new
/// remainder)`; if the step is `None` there is nothing to send (a notch with
/// no whole line, a motionless in-between event).
pub(crate) fn smooth_wheel(
    delta: f64,
    unit: f64,
    carry: f64,
    phase: NSEventPhase,
    momentum: NSEventPhase,
) -> (Option<SmoothWheel>, f64) {
    let (lines, rest) = wheel_lines(delta, unit, carry);
    if phase.is_empty() && momentum.is_empty() {
        let step = (lines != 0).then_some(SmoothWheel {
            rows: f64::from(lines),
            lines,
            intent: ScrollIntent::Glide,
        });
        return (step, rest);
    }
    let rows = delta / unit;
    let rows = if rows.is_finite() { rows } else { 0.0 };
    let intent = if momentum.contains(NSEventPhase::Began)
        || phase.intersects(NSEventPhase::Began | NSEventPhase::MayBegin)
    {
        ScrollIntent::GestureBegan
    } else if momentum.intersects(NSEventPhase::Ended | NSEventPhase::Cancelled)
        || phase.intersects(NSEventPhase::Ended | NSEventPhase::Cancelled)
    {
        ScrollIntent::Settle
    } else {
        ScrollIntent::Direct
    };
    // A motionless in-between event (`Stationary`, zero delta) changes
    // nothing; it must not go to the `Term` lock.
    if intent == ScrollIntent::Direct && rows == 0.0 && lines == 0 {
        return (None, rest);
    }
    (
        Some(SmoothWheel {
            rows,
            lines,
            intent,
        }),
        rest,
    )
}

/// A mouse event's modifiers. Shift does not enter the report, it does the
/// arbitration - the rationale is in [`MouseModifiers`]'s doc.
fn modifiers(event: &NSEvent) -> MouseModifiers {
    let flags = event.modifierFlags();
    MouseModifiers {
        shift: flags.contains(NSEventModifierFlags::Shift),
        // macOS's Option is xterm's Meta - the same key as the keyboard's
        // Meta encoding (`Option+←` → `\eb`).
        meta: flags.contains(NSEventModifierFlags::Option),
        control: flags.contains(NSEventModifierFlags::Control),
    }
}

/// Whether a keystroke goes to the terminal - a **pure decision**, tested: a
/// Command key, **except for one exception**, does not go.
///
/// The menu's shortcuts (Cmd-C, Cmd-V, Cmd-Q, Cmd-,, Cmd +/−/0) never reach
/// this question: AppKit gives a Command key to the main menu with
/// `performKeyEquivalent:` **before** `keyDown:` (`menu`). A Command key that
/// reaches here has no counterpart in the menu (Cmd-T) or its item is
/// disabled at that moment; had it fallen to the terminal it would type a
/// plain letter into the shell. The rest of the modifiers are not asked:
/// Cmd-Shift-T is also a shortcut attempt, not input.
///
/// **The exceptions are three keys and the list is closed:** ⌘⌫
/// ([`BACKSPACE`]), ⌘← ([`ARROW_LEFT`]) and ⌘→ ([`ARROW_RIGHT`]) pass; their
/// bytes are in [`encode_key`] (`\x15` = `^U` `kill-whole-line`, `\x01` =
/// `^A` `beginning-of-line`, `\x05` = `^E` `end-of-line`). All three are
/// macOS's line gestures and all three bytes are really bound in zsh. The
/// list staying **closed** is a design decision, not its length: a passing
/// key is written by name, otherwise an open rule would one day pass Cmd-T
/// too and type `t` into the shell (the entry of ⌘←/⌘→ is
/// justified in `encode_key`'s arm).
///
/// The exception asks **only the character**, not the modifiers beside it:
/// with CapsLock on ⌘⌫ must still delete the line and Shift or Control give
/// ⌫ no second meaning. The same as `page_scroll`'s rule "modifiers other
/// than Shift are not asked"; the opposite decision would silently swallow
/// the key for a user whose flag happens to be on.
///
/// If `chars` is missing (a pure modifier key) a Command event is swallowed:
/// the allow list's criterion is a character and there is no character.
fn reaches_terminal(flags: NSEventModifierFlags, chars: Option<&str>) -> bool {
    if !flags.contains(NSEventModifierFlags::Command) {
        return true;
    }
    // A single-character match, the precedent of `page_scroll` and **from the
    // same owner** ([`only_char`]): a multi-character `characters` that
    // **starts** with a key in the list does not enter the allow list.
    matches!(
        only_char(chars.unwrap_or_default()),
        Some(BACKSPACE | ARROW_LEFT | ARROW_RIGHT)
    )
}

pub(crate) struct ViewIvars {
    /// The view must be born **before** the session: the grid size is derived
    /// from the contentView's bounds and `Session::spawn` asks for that size.
    /// A keystroke cannot pass in between, but the reason is not that the
    /// window is not yet key (`makeKeyAndOrderFront` runs earlier): the gap
    /// closes inside `applicationDidFinishLaunching`, **before the run loop
    /// turns**, so no event can fall in between.
    session: OnceCell<Arc<Session>>,
    /// The mouse's gesture ledger: the selection drag, the presses reported to
    /// the application and the motion report's notch - the rules and
    /// rationales are in [`Gesture`], a tested struct. `Cell` + `Copy`: every
    /// event is take-modify-put, no borrow in the middle of a `Session` call.
    gesture: Cell<Gesture>,
    /// **Whether the text stack took this event** - `keyDown:`'s re-entry
    /// flag. It is pulled to `false` before `interpretKeyEvents:` is called;
    /// `insertText:` **and** `setMarkedText:` make it `true`, `keyDown:` reads
    /// it on return and if `false` drops the event to [`crate::keys::encode_key`].
    ///
    /// The invariant is **"the stack took the event"**, not "text arrived" -
    /// hence the name `consumed`. On a dead key's first stroke (`Option+ü`)
    /// `characters` is empty, so the fallback is accidentally harmless today;
    /// had only `insertText:` set the flag the invariant would be written on
    /// that accident and a non-empty composition start would send the key twice.
    ///
    /// `Cell`, an ivar: `interpretKeyEvents:` **calls us again**, so the value
    /// cannot be carried in `keyDown:`'s stack frame. Since there is a single
    /// thread (the main thread) it is not shared state - the precedent beside
    /// it is [`ViewIvars::gesture`].
    ///
    /// **There is a state it cannot see and it was not measured:** a key that
    /// cancels a pending composition only with `unmarkText` (Backspace or Esc
    /// after a dead key) does not set the flag, so the event falls to
    /// `encode_key` and `0x7f` goes to the PTY - a letter the user **really**
    /// typed is deleted. The opposite state was not measured either: counting
    /// `unmarkText` as consumption would also swallow the first arrow after a
    /// composition (the stack gives it as `unmarkText` + `moveLeft:`). The two
    /// directions separate with one key round and the defence is built
    /// **after** that measurement - an arm written today could pick the wrong
    /// half without knowing which is real.
    consumed: Cell<bool>,
    /// The **minimal** state of the composition (marked text): the stack's
    /// not-yet-completed input. There is **no drawing** - `bt-gpu`'s
    /// underlined preedit surface is not born in this set; here there is only
    /// the **state** that `hasMarkedText`/`markedRange`/`selectedRange` can answer with.
    ///
    /// An empty string means "no composition": `unmarkText` and `insertText:`
    /// empty it. Leaving a stub (saying "no composition" to everything) was an
    /// **unmeasured** claim; alacritty and ghostty both keep a marked-text field.
    marked_text: RefCell<String>,
    /// The wheel's remainder not yet turned into lines ([`wheel_lines`]). It
    /// is reset in three places, and in all three the remaining remainder does
    /// not belong to the next scroll: at the start of a new gesture (the
    /// previous gesture's crumb must not trigger the new one early or late),
    /// when the wheel is ignored (`Wheel::Ignored`: one mode's remainder must
    /// not carry to the next mode) and when it hits the end of the scrollback
    /// (momentum accumulated toward the end must not delay the first row in
    /// the opposite direction). When the wheel goes to the application
    /// (`Wheel::Sent`) it is **kept**: in a slow trackpad scroll, had each
    /// event's fraction been dropped, `less` would scroll jerkily. The smooth
    /// arm's two exceptions are in [`BateriView::smooth_scroll_wheel`].
    scroll_carry: Cell<f64>,
    /// Whether scrolling is smooth ([`smooth_wheel`]) or by line steps: the
    /// **resolved** state of `[motion] smooth_scroll`, Reduce Motion and
    /// `cursor_motion = "snap"` (`app::resolve_smooth_scroll`). It is born
    /// `true` because the setting's default is `"on"` and the hermetic timed
    /// run reads no settings; the window's `start` still writes the setting's value.
    smooth_scroll: Cell<bool>,
    /// The live inputs of the mouse translation: the metrics from `bt-gpu`,
    /// the grid the number `bt-core` knows. `Cell<Option<…>>` not `OnceCell`,
    /// because it is refreshed when the window size changes (`set_metrics`).
    /// It looks like a separate copy but is not: the very values that go to
    /// `start_session` and `DisplayLink::resize`, written at the same call site.
    ///
    /// **Vertically it does not come from the same frame as `origin`** and
    /// this is a known transition: this triple is refreshed on the window
    /// event (`set_metrics`), the offset on the next **frame** path. In the
    /// single frame in between `rows` is new and the offset old - but the frame
    /// standing on screen is also old, so `origin`'s staleness is the right
    /// one; the only thing that diverges is the clamp limit of a click on the
    /// bottom edge. Since geometry snaps the offset anyway the window closes
    /// in one frame. A degenerate size never causes this transition: the
    /// session rejects it (`Session::resize`) and `point_to_cell` returns
    /// `None` at zero rows/columns, so both sides go silent at the same place.
    metrics: Cell<Option<(CellMetrics, (u16, u16))>>,
    /// The dock's row count; `0` → the window has no dock (an integration-less
    /// shell, the alternate screen). Written in the **same** call as `metrics`
    /// (`set_metrics`): when the dock goes away on the alternate screen the
    /// grid is resized too, so the two are two halves of the same geometry.
    dock_rows: Cell<u16>,
    /// The hand-cursor rectangles the last `resetCursorRects` set up (what
    /// [`BateriView::sync_cursor_rects`] compares): the upload buttons' and the
    /// ⌘-hovered link's, one list.
    cursor_rects: RefCell<Vec<NSRect>>,
    /// The ⌘-hover and ⌘-click state ([`crate::hyperlink`]).
    link: RefCell<LinkState>,
    /// The drawn frame's vertical origin - the read end of the body the frame
    /// path writes ([`bt_gpu::Origin`]).
    ///
    /// Beside `metrics` but **not inside it**: that triple is refreshed on
    /// window events (`set_metrics`), while the origin changes per frame. Had
    /// it been put inside, the mouse would not have seen the bottom-sticking
    /// until the next resize.
    ///
    /// `OnceCell`: the link is born once with the session and its body never
    /// changes after that - what changes is the body's **content** and the
    /// frame path writes it. While absent (the single window before the link
    /// is set up) the origin is zero and the drawing is stuck to the ceiling,
    /// so the two are consistent.
    origin: OnceCell<Origin>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; BateriView does not
    // implement `Drop` and offers no initializer other than `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriView"]
    #[ivars = ViewIvars]
    pub(crate) struct BateriView;

    unsafe impl NSObjectProtocol for BateriView {}

    impl BateriView {
        /// The condition for keystrokes to arrive here. `NSView`'s default is
        /// `false`; `makeFirstResponder` is silently refused without this.
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        /// The keyboard came to the terminal: the return from the
        /// search panel's field - Esc, close or a click on the terminal. The
        /// caret's focus is "window key **and** keyboard in the terminal" and
        /// the second bit comes from a single source, from here
        /// (`TerminalPane::keyboard_moved`).
        #[unsafe(method(becomeFirstResponder))]
        fn become_first_responder(&self) -> bool {
            // SAFETY: `NSResponder`'s argumentless method returning `BOOL`.
            let accepted: bool = unsafe { msg_send![super(self), becomeFirstResponder] };
            if accepted {
                self.keyboard_moved(true);
            }
            accepted
        }

        /// The keyboard left the terminal (the search field became first responder).
        #[unsafe(method(resignFirstResponder))]
        fn resign_first_responder(&self) -> bool {
            // SAFETY: `NSResponder`'s argumentless method returning `BOOL`.
            let resigned: bool = unsafe { msg_send![super(self), resignFirstResponder] };
            if resigned {
                self.keyboard_moved(false);
                // ⌘F or another pane took the keyboard: `flagsChanged:` and
                // `mouseMoved:` now go elsewhere and could never clear this
                // view's link hover (underline, hand, target label).
                self.clear_link();
            }
            resigned
        }

        /// Edit ▸ Copy (Cmd-C): writes the selected text to the general
        /// pasteboard. If there is no selection or it is empty the pasteboard
        /// is left untouched (`clipboard::copy`). The window has a single
        /// selection - the grid's or the dock's - and its text is given by its
        /// owner through `Session::selection_text`.
        ///
        /// The menu item has no target: the action reaches the first
        /// responder through the responder chain, i.e. here (`menu`). The text
        /// is from `selection_text()` - the selection's single text path.
        #[unsafe(method(copy:))]
        fn copy_selection(&self, _sender: Option<&AnyObject>) {
            if let Some(session) = self.ivars().session.get() {
                clipboard::copy(&NSPasteboard::generalPasteboard(), session.selection_text());
            }
        }

        /// Edit ▸ Cut (⌘X): writes the dock selection's text to the
        /// pasteboard and deletes the selection. It does
        /// something only while the editing gate is open (`Session::dock_cut`);
        /// the menu item is enabled then ([`BateriView::validate_menu_item`]),
        /// so the shortcut does not reach here with the gate closed either.
        /// There is nothing to cut in the grid.
        #[unsafe(method(cut:))]
        fn cut_selection(&self, _sender: Option<&AnyObject>) {
            if let Some(session) = self.ivars().session.get()
                && let Some(text) = session.dock_cut()
            {
                clipboard::copy(&NSPasteboard::generalPasteboard(), Some(text));
            }
        }

        /// A menu item's enablement: once `validateMenuItem:` is defined
        /// AppKit asks about **every** item, so the default answer is `true` -
        /// Copy, Paste and Select All are always enabled as today. Two
        /// exceptions: Cut is grey if the dock has no selection or the editing
        /// gate is closed (`vicmd`, stale mirror, a command running); Paste
        /// Escaped Text is grey if there is no text on the pasteboard -
        /// Paste itself is always enabled as today and silent on an empty pasteboard.
        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: &NSMenuItem) -> bool {
            // No `return`: `define_class!` converts the `bool` to `Bool` at the
            // **end** of the body, an early return does not compile.
            let action = item.action();
            if action == Some(sel!(cut:)) {
                self.ivars()
                    .session
                    .get()
                    .is_some_and(|session| session.can_cut())
            } else if action == Some(sel!(pasteEscaped:)) {
                clipboard::read(&NSPasteboard::generalPasteboard()).is_some()
            } else {
                true
            }
        }

        /// Edit ▸ Paste (Cmd-V): pastes the pasteboard's text into the session.
        ///
        /// It enters through the `paste()` path: if mode 2004 is set it is
        /// wrapped in bracketed paste, otherwise written raw. Raw bytes do not
        /// touch `session.write`. Silent if there is no text on the
        /// pasteboard. If there is a selection in the dock the payload
        /// replaces it - the deletion is inside `paste()` too.
        #[unsafe(method(paste:))]
        fn paste_clipboard(&self, _sender: Option<&AnyObject>) {
            let Some(session) = self.ivars().session.get() else {
                return;
            };
            if let Some(text) = clipboard::read(&NSPasteboard::generalPasteboard()) {
                session.paste(text.into_bytes());
            }
        }

        /// Edit ▸ Paste Escaped Text (⌃⌘V): makes the
        /// pasteboard's text writable to the shell **as a single argument** and
        /// pastes it - with the Finder drop's backslash if there is no line
        /// break, wholly in single quotes if there is
        /// ([`crate::quote::paste_quote`]). What follows is Paste's path
        /// (`Session::paste`: bracketed wrapping, replacing the dock selection).
        ///
        /// Here, not in `TerminalPane` (the `paste:` precedent):
        /// while the search field is focused the responder chain does not pass
        /// through this view and the item is grey - pasting escaped text into
        /// the field has no meaning.
        #[unsafe(method(pasteEscaped:))]
        fn paste_escaped(&self, _sender: Option<&AnyObject>) {
            let Some(session) = self.ivars().session.get() else {
                return;
            };
            if let Some(text) = clipboard::read(&NSPasteboard::generalPasteboard()) {
                session.paste(paste_quote(&text).into_bytes());
            }
        }

        /// Edit ▸ Select All (⌘A): while the dock owns the caret and the line
        /// has text it selects the dock's whole `BUFFER`, otherwise the grid's
        /// whole scrollback (`Session::select_all`; Terminal.app's norm).
        ///
        /// The menu item is caught **before** `keyDown:` by
        /// `performKeyEquivalent:`, so ⌘A never reaches the shell and
        /// `keyDown:`'s Cmd allow list does not change (the tab shortcuts' path).
        #[unsafe(method(selectAll:))]
        fn select_all(&self, _sender: Option<&AnyObject>) {
            if let Some(session) = self.ivars().session.get() {
                session.select_all();
            }
        }

        /// The view's y axis is from the top: the mouse point arrives in the grid's direction.
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            // Let the mouse's y arrive in the grid's direction (from the top):
            // no flipping back in the translation, no `bounds.height` cut-off -
            // not a constant that drifts when the window size changes but the type's promise.
            true
        }

        /// Left button pressed: whether the gesture is the application's or
        /// the terminal's, the decision is given by `bt-core`
        /// ([`BateriView::button_event`]).
        ///
        /// The `buttonNumber()` gate stays: AppKit reserves this selector for
        /// the left button and another button falling here would be reported
        /// with the **wrong** button - right and middle have their own
        /// selectors. It does not pass to `super`: the default `NSView`
        /// behaviour knows nothing of selection and would swallow the event.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            self.note_interaction();
            if event.buttonNumber() != 0 {
                return;
            }
            self.button_event(event, MouseButton::Left, true);
        }

        /// A drag with the left button held - **the single selector of two
        /// gestures**. If the press was reported ([`Gesture::dragged`]) the
        /// motion goes as a report too; otherwise the active end is moved to
        /// the mouse's current place, the anchor is in `bt-core`
        /// (`Session::update_selection` only moves the end). Events that do not
        /// change the drawn range (staying in the same half, crossing a cell
        /// boundary) are filtered out at the session's range gate - no frame is requested.
        ///
        /// A drag without a press is swallowed: there is no `mouseDragged:`
        /// without `mouseDown:` but AppKit's word is not trusted - were there
        /// one it would move the previous selection's end.
        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            self.drag_event(event, MouseButton::Left);
        }

        #[unsafe(method(rightMouseDragged:))]
        fn right_mouse_dragged(&self, event: &NSEvent) {
            self.drag_event(event, MouseButton::Right);
        }

        #[unsafe(method(otherMouseDragged:))]
        fn other_mouse_dragged(&self, event: &NSEvent) {
            if event.buttonNumber() != 2 {
                return;
            }
            self.drag_event(event, MouseButton::Middle);
        }

        /// AppKit's call to rebuild the cursor rects: automatically when the
        /// frame changes, with `invalidateCursorRectsForView:` when the
        /// buttons' place changes ([`BateriView::sync_cursor_rects`]).
        #[unsafe(method(resetCursorRects))]
        fn reset_cursor_rects(&self) {
            self.hand_cursor_rects();
        }

        /// A modifier key went down or up: ⌘ shows or clears the link
        /// under the pointer without the pointer moving
        /// ([`BateriView::link_flags`]). Then `NSResponder`'s default, which
        /// passes the event along the chain.
        #[unsafe(method(flagsChanged:))]
        fn flags_changed(&self, event: &NSEvent) {
            self.link_flags(event);
            // SAFETY: `NSResponder`'s `flagsChanged:` takes an `NSEvent`, returns nothing.
            let _: () = unsafe { msg_send![super(self), flagsChanged: event] };
        }

        /// A buttonless motion. Since the window is opened with
        /// `setAcceptsMouseMovedEvents:` it arrives in **every window**, even
        /// if the mode is not on: the event's cost is a coordinate arithmetic
        /// and if the cell did not change `bt-core` is not called at all
        /// ([`BateriView::motion_event`]). Turning it on by mode would need
        /// publishing the mode to `bt-shell-macos`, i.e. a new piece of shared
        /// state; if a symptom is seen we return to that arm.
        ///
        /// The upload line's button ([`BateriView::upload_hover`]) is asked
        /// **before** the motion report and independently of it: the context
        /// line is outside the grid and the report path rejects that area. The
        /// ⌘-hovered link ([`BateriView::link_motion`]) likewise: its hit
        /// test also covers the fill band, which the report keeps rejecting.
        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            self.note_interaction();
            self.upload_hover(event);
            self.link_motion(event);
            self.motion_event(event, None);
        }

        /// Left button released: if the press was reported the release is
        /// reported too, otherwise the drag ends and the selection stays
        /// on screen (Cmd-C copies it).
        ///
        /// There is **no** `buttonNumber()` gate here and the asymmetry is
        /// deliberate: with a gate, an unexpected button number would leave
        /// `dragging` stale `true` and every later scroll would silently
        /// extend the old selection (the very state [`BateriView::follow_pointer`] closes).
        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            self.button_event(event, MouseButton::Left, false);
        }

        /// Right button: the report path, and where the terminal owns the press
        /// (mouse mode off, the fill band, the dock) the link's context menu
        /// ([`BateriView::link_menu`]). Off a link a right click does
        /// nothing - no general context menu, and no selection either: it would
        /// produce an unexpected highlight.
        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, event: &NSEvent) {
            self.note_interaction();
            self.button_event(event, MouseButton::Right, true);
        }

        #[unsafe(method(rightMouseUp:))]
        fn right_mouse_up(&self, event: &NSEvent) {
            self.button_event(event, MouseButton::Right, false);
        }

        /// The link context menu's items ([`BateriView::link_menu`]): the menu's
        /// link is parked in the link state and taken by the item.
        #[unsafe(method(openLinkFromMenu:))]
        fn open_link_from_menu(&self, _sender: Option<&AnyObject>) {
            self.menu_open_link();
        }

        #[unsafe(method(revealLinkFromMenu:))]
        fn reveal_link_from_menu(&self, _sender: Option<&AnyObject>) {
            self.menu_reveal_link();
        }

        #[unsafe(method(copyLinkFromMenu:))]
        fn copy_link_from_menu(&self, _sender: Option<&AnyObject>) {
            self.menu_copy_link();
        }

        /// A remote link's items: download to the download folder, to
        /// a chosen folder, and its scp path.
        #[unsafe(method(downloadLinkFromMenu:))]
        fn download_link_from_menu(&self, _sender: Option<&AnyObject>) {
            self.menu_download(false);
        }

        #[unsafe(method(downloadLinkToFromMenu:))]
        fn download_link_to_from_menu(&self, _sender: Option<&AnyObject>) {
            self.menu_download(true);
        }

        #[unsafe(method(copyScpPathFromMenu:))]
        fn copy_scp_path_from_menu(&self, _sender: Option<&AnyObject>) {
            self.menu_copy_scp_path();
        }

        /// Middle button and **beyond**: AppKit sends everything past the
        /// fourth button to this selector too, while X10's two bits carry only
        /// three buttons and `3` is reserved for release. If the number is not
        /// 2 the event is dropped - reporting it as the middle button would
        /// tell the application a **wrong** button.
        #[unsafe(method(otherMouseDown:))]
        fn other_mouse_down(&self, event: &NSEvent) {
            self.note_interaction();
            if event.buttonNumber() != 2 {
                return;
            }
            self.button_event(event, MouseButton::Middle, true);
        }

        #[unsafe(method(otherMouseUp:))]
        fn other_mouse_up(&self, event: &NSEvent) {
            if event.buttonNumber() != 2 {
                return;
            }
            self.button_event(event, MouseButton::Middle, false);
        }

        /// Wheel and trackpad: if the application asked for mouse reports -
        /// whichever screen - the wheel goes to the application as a wheel
        /// report; if not, on the alternate screen it goes as arrows, on the
        /// primary screen it scrolls the visible window into the scrollback.
        /// There is **no** scrollbar - AppKit's chronic (thumb, proportion,
        /// drag), not needed for the threshold.
        ///
        /// The decision by mode is in `bt-core` (`Session::scroll_wheel`); this
        /// supplies the line, the pointer's cell and Shift. The horizontal
        /// delta is ignored - there is nothing to scroll horizontally
        /// (the horizontal wheel report, 66/67, is out of scope). macOS turns
        /// Shift+wheel on a classic mouse into a horizontal delta, so Shift's
        /// arm on this path mostly comes from the trackpad.
        ///
        /// If scrolling happens in the middle of a held drag the selection's
        /// end moves to the cell **now** under the mouse
        /// ([`BateriView::follow_pointer`]).
        ///
        /// **There are two arms** and the chooser is `ViewIvars::smooth_scroll`:
        /// the smooth arm ([`BateriView::smooth_scroll_wheel`]) sends the
        /// fractional amount and the intent, the line arm (the body below) is
        /// the very path from before `"on"`, byte for byte - the fallback of
        /// `smooth_scroll = "off"`, Reduce Motion and `cursor_motion = "snap"`.
        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            self.note_interaction();
            let Some(session) = self.ivars().session.get() else {
                return;
            };
            let Some((metrics, _)) = self.ivars().metrics.get() else {
                return;
            };
            let Some(window) = self.window() else {
                return;
            };
            // A trackpad is in points: the unit is the cell height, lowered
            // from physical pixels to points (the measure comes physical from
            // `bt-gpu`). A classic wheel already gives lines.
            let unit = if event.hasPreciseScrollingDeltas() {
                f64::from(metrics.cell_px().1) / window.backingScaleFactor()
            } else {
                1.0
            };
            let carry = &self.ivars().scroll_carry;
            if event.phase().contains(NSEventPhase::Began) {
                carry.set(0.0);
            }
            if self.ivars().smooth_scroll.get() {
                self.smooth_scroll_wheel(event, session, unit);
                return;
            }
            let (lines, rest) = wheel_lines(event.scrollingDeltaY(), unit, carry.get());
            carry.set(rest);
            if lines == 0 {
                return;
            }
            if self.dock_wheel(event, session, lines) {
                return;
            }
            // The pointer's cell enters the report in mouse mode; its half does
            // not (`bt-core` does not read it). A point beyond the edge sticks,
            // `None` only on a zero-size grid.
            //
            // **The fill rejection does not apply here** and zero is passed
            // deliberately: the point here is not a selection end but a
            // coordinate going to the report, and had it been rejected this
            // `else` would have dropped **all** of the scrolling - a user who
            // took the pointer onto the band while it is on screen could not
            // scroll at all. A point above the band enters the report as row 0
            // as today: the application does not know about the fill anyway, it
            // is a terminal drawing.
            let Some(pointer) =
                self.window_point_cell(event.locationInWindow(), OutOfGrid::Clamp { fill_rows: 0 })
            else {
                return;
            };
            let shift = event.modifierFlags().contains(NSEventModifierFlags::Shift);
            // Line path: the fractional amount is the line itself and the intent
            // is whole lines - the scroll arm goes through today's `scroll_locked` too.
            match session.scroll_wheel(f64::from(lines), lines, ScrollIntent::Lines, pointer, shift) {
                Wheel::Scrolled(0) | Wheel::Ignored => carry.set(0.0),
                Wheel::Scrolled(_) => self.follow_pointer(session),
                // The window did not scroll, the application draws its own
                // screen: the selection end does not move, the remainder is kept
                // (`ViewIvars::scroll_carry`).
                Wheel::Sent => {}
            }
        }

        /// The **arbitration** of a keystroke - four arms, and the order is the contract.
        ///
        /// The first three arms do **not enter** AppKit's text stack
        /// (`interpretKeyEvents:`) and each has its own rationale for not entering:
        ///
        /// 1. **A Cmd event** is swallowed (`reaches_terminal`); **the
        ///    exceptions** are in the closed allow list (⌘⌫ → `\x15`, ⌘← →
        ///    `\x01`, ⌘→ → `\x05`) and they also do **not enter** the stack,
        ///    going straight to [`encode_key`]. Had they entered the stack,
        ///    ⌘⌫ would become `deleteToBeginningOfLine:` there and ⌘←/⌘→
        ///    `moveToBeginningOfLine:`/`moveToEndOfLine:`, and
        ///    `doCommandBySelector:` would silently swallow all three; ⌘T
        ///    would reach `insertText:` and type `t` into the shell.
        /// 2. **Shift+PgUp/PgDn** is the terminal's scrolling
        ///    ([`page_scroll`]). The arm is **before** Control's, because
        ///    `page_scroll` does not ask about modifiers other than Shift -
        ///    Ctrl+Shift+PgUp scrolls today too and had the order been
        ///    reversed that key would have fallen to `\e[5~`.
        /// 3. **A Control event** goes straight to [`encode_key`]. It cannot
        ///    be left to AppKit to choose its arm: numpad Enter's `characters`
        ///    is U+0003 (Ctrl-C's byte) and Ctrl-Y's shares U+0019 with
        ///    Shift+Tab - if the stack chose the wrong arm every command would
        ///    be interrupted. A side gain: the Ctrl+Shift+Tab and Ctrl+numpad
        ///    Enter debts stay in their present state. **The cost, by name:** a
        ///    pending composition is not torn down by this arm - typing `^C`
        ///    after Option+ü and then `a` may produce `ã`, because the stack is
        ///    still waiting for the dead key. It was not measured and no
        ///    defence was built: putting the arm into the stack would leave
        ///    numpad Enter's U+0003 to AppKit's choice, so the trade is "every
        ///    command may be interrupted" against "a rare accent".
        /// 4. **The rest** is given to the stack; if the stack did not take the
        ///    event ([`ViewIvars::consumed`]) it falls to `encode_key` anyway.
        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            self.note_interaction();
            let flags = event.modifierFlags();
            let Some(session) = self.ivars().session.get() else {
                return;
            };
            // `characters` gives the state with modifiers applied (Option-held
            // "ø", Ctrl-C → U+0003); the raw key code would be
            // `charactersIgnoringModifiers` and would require us to reimplement
            // the keyboard layout. Its absence (a pure modifier key) silences
            // the two arms below but **does not silence the stack**: on a
            // composition's first stroke `characters` is empty and a dead key
            // starts exactly there.
            //
            // It is read **before** the Cmd arm: the allow list's criterion is
            // now the key's identity, not its flags.
            let chars = event.characters().map(|c| c.to_string());
            // While Command is held a key is a shortcut, not input. The menu
            // catches it first with `performKeyEquivalent:` (Cmd-C/V/Q/,,
            // Cmd +/−/0); what it does not catch arrives here and is
            // **swallowed** (`reaches_terminal`) - except the three keys in the
            // allow list (⌘⌫, ⌘←, ⌘→).
            if !reaches_terminal(flags, chars.as_deref()) {
                return;
            }
            let command = flags.contains(NSEventModifierFlags::Command);
            // Shift+PgUp/PgDn is the terminal's scrolling, not the
            // application's key - but only if the session accepts. On the
            // alternate screen scrolling is refused (`None`) and the key goes
            // to the application by the path below as a plain PgUp: in less/vim
            // Shift+PgUp turns the page too, it is not swallowed. How many
            // lines a page is is `bt-core`'s decision (`scroll_page`).
            let shift = flags.contains(NSEventModifierFlags::Shift);
            if let Some(chars) = chars.as_deref()
                && let Some(pages) = page_scroll(chars, shift)
                && let Some(moved) = session.scroll_page(pages)
            {
                if moved != 0 {
                    self.follow_pointer(session);
                }
                return;
            }
            let ctrl = flags.contains(NSEventModifierFlags::Control);
            let option = flags.contains(NSEventModifierFlags::Option);
            // **The dock selection's keys**, BEFORE the stack: ⌫
            // and the arrows would fall to `doCommandBySelector:` in the stack
            // and take their bytes from `encode_key`, i.e. instead of deleting
            // the selection they would delete one character. A key that is not
            // consumed (gate closed, no selection, another key) follows its
            // present path below and input removes the selection.
            // **Not asked while a composition is pending**: ⌫ must cancel it
            // (had the ⌫ after Option+e not gone to the stack the next letter
            // would come out accented; found in code review).
            if self.ivars().marked_text.borrow().is_empty()
                && let Some(chars) = chars.as_deref()
                && let Some(key) = dock_key(
                    KeyPress {
                        chars,
                        ctrl,
                        option,
                        command,
                    },
                    shift,
                )
                && session.dock_key(key)
            {
                return;
            }
            // `!command` is where the allow list is **applied**: the three keys that pass
            // the allow list do not enter the stack either. Had they, the stack
            // would turn them into
            // `deleteToBeginningOfLine:`/`moveToBeginningOfLine:`/`moveToEndOfLine:`,
            // `doCommandBySelector:` would silently swallow them and the arms
            // below would never see their bytes.
            if !ctrl && !command {
                // The text stack: it holds the dead-key state and when the
                // composition completes it gives the text back with
                // `insertText:`. The flag is lowered **before** the call; since
                // the stack calls us again, the ivar carries the answer, not
                // `keyDown:`'s stack frame.
                self.ivars().consumed.set(false);
                // A one-event array: the stack consumes it synchronously and
                // the read below is valid after the return.
                self.interpretKeyEvents(&NSArray::from_slice(&[event]));
                if self.ivars().consumed.get() {
                    return;
                }
            }
            // An event the stack did not take (or never visited): function
            // keys, Enter/Tab/Esc/Backspace, Control letters, Option
            // navigation/deletion (the stack gives them to `doCommandBySelector:`
            // and that method is a silent no-op) and ⌘⌫/⌘←/⌘→ that passed the allow list.
            //
            // We do not pass to `super`: `NSResponder::keyDown:` beeps on a key
            // it does not recognise and every arrow key in the terminal would beep.
            let Some(chars) = chars else {
                return;
            };
            let key = KeyPress {
                chars: &chars,
                ctrl,
                option,
                command,
            };
            match encode_key(key) {
                Some(KeyInput::Bytes(bytes)) => session.write(&bytes),
                // The arrow's bytes depend on DECCKM, the mode is in `bt-core`.
                Some(KeyInput::Arrow(arrow)) => session.write_arrow(arrow),
                None => {}
            }
        }
    }

    /// The side of AppKit's text stack facing this view. All **11 required**
    /// methods of the protocol are here: `objc2-app-kit` marks none of them
    /// `#[optional]` and `define_class!` panics in a debug assertion on any
    /// that is missing - partial conformance is not an option.
    ///
    /// Three **write** the composition state (`insertText:`,
    /// `setMarkedText:`, `unmarkText`), three **read** it (`selectedRange`,
    /// `markedRange`, `hasMarkedText`); `doCommandBySelector:` is deliberately
    /// empty and the remaining four give fixed answers - each with its own "why".
    unsafe impl NSTextInputClient for BateriView {
        /// The composition completed (or a plain letter arrived): the text goes to the PTY.
        ///
        /// The argument is `&AnyObject` - the stack can send an `NSString`
        /// **or** an `NSAttributedString`. The **single** decoding rule:
        /// downcast to `NSString`, failing that
        /// `NSAttributedString::string()`; if it is neither, the event is
        /// **not counted as consumed** and `keyDown:` drops it to
        /// `encode_key` - silently swallowing a type we do not recognise would
        /// lose the key altogether.
        ///
        /// `replacement_range` is ignored: we have no document the stack could
        /// edit, what is typed flows straight to the PTY and the owner of the
        /// line is the shell. **It has a known consequence**: when the accent
        /// popover has a letter chosen the call becomes
        /// `insertText:"é" replacementRange:{n-1,1}`, i.e. "replace the last
        /// letter with this"; since we skip the range, `eé` goes to the shell.
        /// The popover is off (`app::disable_press_and_hold`) and so the path is
        /// dead today; if that suppression does not hold this is the **silent
        /// half** of the symptom (the noisy half being that a held key does
        /// not repeat).
        #[unsafe(method(insertText:replacementRange:))]
        fn insert_text(&self, string: &AnyObject, _replacement_range: NSRange) {
            // The composition is cleared **before the decoding rule**: even if
            // a type we do not recognise arrives the stack has finished that
            // composition, and had the state stayed there `hasMarkedText` would
            // say `true` forever.
            self.ivars().marked_text.borrow_mut().clear();
            let Some(text) = resolve_text(string) else {
                return;
            };
            // The flag **before the session**: the invariant is "the stack took
            // this event", not "bytes were written". If the session is not yet
            // attached the key is lost but `encode_key` does not send it a second time.
            self.ivars().consumed.set(true);
            // `type_text`, not `write`: if there is a selection in the dock the
            // letter is typed in its place.
            if let Some(session) = self.ivars().session.get() {
                session.type_text(&text);
            }
        }

        /// An editing command the stack recognises (Enter → `insertNewline:`,
        /// Tab → `insertTab:`, Esc → `cancelOperation:`, `^A` →
        /// `moveToBeginningOfParagraph:`…): a **silent no-op**.
        ///
        /// The method cannot be left without a body, even an empty one:
        /// otherwise `NSResponder`'s default runs and **beeps** on a selector it
        /// does not recognise - the very rationale for not passing to `super`
        /// in `keyDown:`, through a new door. The flag is not set, so the event
        /// falls to `encode_key` and its bytes come from where they do today.
        #[unsafe(method(doCommandBySelector:))]
        fn do_command_by_selector(&self, _selector: Sel) {}

        /// The composition continues (a dead key was pressed, not yet
        /// completed): the state is updated. **No drawing** - the underlined
        /// preedit surface is not born in this set.
        ///
        /// This method sets the flag too ([`ViewIvars::consumed`]): the
        /// invariant is "the stack took the event", not "text arrived".
        #[unsafe(method(setMarkedText:selectedRange:replacementRange:))]
        fn set_marked_text(
            &self,
            string: &AnyObject,
            _selected_range: NSRange,
            _replacement_range: NSRange,
        ) {
            let Some(text) = resolve_text(string) else {
                return;
            };
            self.ivars().consumed.set(true);
            *self.ivars().marked_text.borrow_mut() = text;
        }

        /// The composition was cancelled or completed. The flag is **not
        /// set**: the stack also calls this from outside `keyDown:` (focus
        /// loss, mouse) and such a call does not count as consuming a key event.
        ///
        /// **A deliberate deviation from the contract:** Apple says "accept the
        /// marked text as if it were typed normally", we **discard** it. The
        /// reason is that we have no document to undo: `insertText:` flows the
        /// bytes straight to the PTY and the shell takes them into its line, so
        /// "accepting" would mean writing the pending accent somewhere the user
        /// never wanted. The cost stays, by name - clicking the window in the
        /// middle of a composition silently drops the pending `~`; alacritty and
        /// ghostty do the same thing in the same place.
        #[unsafe(method(unmarkText))]
        fn unmark_text(&self) {
            self.ivars().marked_text.borrow_mut().clear();
        }

        /// The selection range. Our model is one sentence: **document =
        /// composition text, caret at its end**. The selection in the
        /// terminal's grid (made with the mouse) does not enter this question -
        /// it is `bt-core`'s selection and not a text the stack could edit.
        #[unsafe(method(selectedRange))]
        fn selected_range(&self) -> NSRange {
            NSRange::new(self.marked_utf16_len(), 0)
        }

        /// The marked range; `NSNotFound` if there is no composition - the
        /// contract's counterpart of "nothing is marked", not a zero-length range.
        #[unsafe(method(markedRange))]
        fn marked_range(&self) -> NSRange {
            match self.marked_utf16_len() {
                0 => EMPTY_RANGE,
                len => NSRange::new(0, len),
            }
        }

        #[unsafe(method(hasMarkedText))]
        fn has_marked_text(&self) -> bool {
            !self.ivars().marked_text.borrow().is_empty()
        }

        /// There is **no** document the stack could read back: everything
        /// typed flows to the PTY and the grid's content is `bt-core`'s, not
        /// the text stack's. `None` = "I have no text in this range".
        ///
        /// `actual_range` is not written: since we return no range there is no
        /// real range to fill in (Apple's contract).
        #[unsafe(method_id(attributedSubstringForProposedRange:actualRange:))]
        fn attributed_substring(
            &self,
            _range: NSRange,
            _actual_range: NSRangePointer,
        ) -> Option<Retained<NSAttributedString>> {
            None
        }

        /// The attributes marked text can carry: **none**. The empty array
        /// says "I cannot apply underline, colouring, ruby - none of them" and
        /// since we do not draw the preedit this is the right answer.
        #[unsafe(method_id(validAttributesForMarkedText))]
        fn valid_attributes_for_marked_text(&self) -> Retained<NSArray<NSAttributedStringKey>> {
            NSArray::new()
        }

        /// The rectangle where the composition surface (the accent popover,
        /// the candidate window) will be positioned on screen - **in screen
        /// coordinates**.
        ///
        /// The answer is the view's own rectangle, not cell-precise, and this is
        /// **deliberate**: there is no consumer within scope (the dead-key
        /// preview is marked text, not a popover, and the candidate window
        /// belongs to CJK, i.e. the full IME debt). Carrying the caret cell
        /// across the crate boundary (the `bt_gpu::Origin` precedent) would be
        /// the first step when that work comes. The approach's direction is
        /// right anyway: the content is stuck to the window's **bottom**, so
        /// the caret is near the view rectangle's bottom left corner and the
        /// surface opens from there.
        ///
        /// The reason for not returning a zero rectangle stands: the surface
        /// would then appear at the screen's corner. For a view without a
        /// window (not yet attached) there is no space to convert to, the
        /// answer is zero.
        #[unsafe(method(firstRectForCharacterRange:actualRange:))]
        fn first_rect_for_character_range(
            &self,
            range: NSRange,
            actual_range: NSRangePointer,
        ) -> NSRect {
            // We say we satisfy the whole asked range: we return a single
            // rectangle and that rectangle belongs to the whole range.
            // SAFETY: the pointer is either null or a valid `NSRange` on the
            // caller's stack; that is AppKit's contract.
            unsafe {
                if let Some(actual) = actual_range.as_mut() {
                    *actual = range;
                }
            }
            let Some(window) = self.window() else {
                return NSRect::ZERO;
            };
            window.convertRectToScreen(self.convertRect_toView(self.bounds(), None))
        }

        /// Which character a point on screen corresponds to: **we have no
        /// answer**. The stack asks this to select text by dragging, and the
        /// grid's selection is our own path (`mouseDragged:`), not the
        /// stack's. `NSNotFound` is the contract's "I have no character at this point".
        #[unsafe(method(characterIndexForPoint:))]
        fn character_index_for_point(&self, _point: NSPoint) -> NSUInteger {
            NOT_FOUND
        }
    }

    /// The side of the drop from Finder facing this view. **All** of the
    /// protocol's methods are `#[optional]` - the very opposite of
    /// `NSTextInputClient` - so two suffice: the one that says the drop is
    /// accepted and the one that writes it.
    ///
    /// `prepareForDragOperation:` is deliberately absent: on an unimplemented
    /// method AppKit assumes "yes" and moves straight to
    /// `performDragOperation:`, so the body to write would be a constant `true`.
    unsafe impl NSDraggingDestination for BateriView {
        /// The pointer entered the window with the drop: the answer is **copy**
        /// - unless an upload sheet (probe included) is in progress in this tab.
        ///
        /// The type filtering was done at registration
        /// (`registerForDraggedTypes` in [`BateriView::new`]): this method is
        /// called only if there is a file URL on the pasteboard. The only thing
        /// asked is the sheet of the upload to the remote directory: two
        /// sheets cannot open on top of each other and a drop arriving
        /// meanwhile would be rejected in `performDragOperation:` - showing "+"
        /// would be a lie. While an upload is **flowing** the drop is accepted:
        /// it enters the queue.
        ///
        /// **Copy**, not move: the file in Finder must stay in place, we only
        /// write its path. `draggingUpdated:` is not implemented either -
        /// on a target that does not implement it AppKit keeps the answer here,
        /// so the second method would repeat the same constant.
        ///
        /// **The session is not asked and the asymmetry stands deliberately:**
        /// `performDragOperation:` below returns `false` while no session is
        /// attached, so the cursor may show "+" and the drop go back with a
        /// "poof". Asking here too would equalise the two answers but the
        /// criterion would be wrong - this method runs at the **start** of the
        /// drag and if there is no session at that moment it may have been born
        /// by the time of release. The absence of a session in the window is
        /// already unreachable ([`ViewIvars::session`]: since the run loop does
        /// not turn between the view and the session no event can fall in
        /// between), so there is no frame in which the asymmetry could be seen;
        /// let its name stay here anyway.
        #[unsafe(method(draggingEntered:))]
        fn dragging_entered(
            &self,
            _sender: &ProtocolObject<dyn NSDraggingInfo>,
        ) -> NSDragOperation {
            if self.pane().is_none_or(|pane| pane.accepts_drop()) {
                NSDragOperation::Copy
            } else {
                NSDragOperation::None
            }
        }

        /// The drop was released: in a local session the paths are escaped and
        /// written to the input line; **in a remote session** a local path is
        /// not written to the remote shell - the drop is uploaded to the remote
        /// directory (the confirmation sheet and the queue are in
        /// `crate::uploader`) and no path is pasted on its own.
        ///
        /// The output is [`Session::paste`] - **not** `session.write`: the
        /// bracketed paste wrapping and the dock exception come free from there.
        /// While the dock owns the line a single-file drop enters
        /// the dock "as if typed" (`Session::can_be_typed`; a backslash is not a
        /// control character, it passes the raw branch without trouble) and this
        /// is the **right** behaviour: the user sees the drop as the
        /// continuation of the line being typed.
        ///
        /// There are two reasons for `false` and both are "there is nothing to
        /// write": the session is not yet attached, or no readable path came out
        /// of the drop. AppKit shows this as the drop's rejection - silently
        /// saying `true` would show the user that something happened when
        /// nothing did.
        ///
        /// There is **no** early `return` in the body and there cannot be:
        /// `define_class!` converts the answer to ObjC's `BOOL` and the
        /// conversion is applied only to the **tail expression**, so a `return
        /// false` would conflict with the outer signature and break the compilation.
        #[unsafe(method(performDragOperation:))]
        fn perform_drag_operation(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            let paths = dropped_paths(&sender.draggingPasteboard());
            match self.ivars().session.get() {
                Some(session) if !paths.is_empty() && session.remote_target().is_some() => self
                    .pane()
                    .is_some_and(|pane| pane.upload_drop(paths)),
                Some(session) if !paths.is_empty() => {
                    session.paste(shell_quote(&paths).into_bytes());
                    true
                }
                _ => false,
            }
        }
    }

    /// **The view is also a drag source**: a ⌘-drag of a remote link
    /// is a file promise to Finder ([`crate::promise::begin_drag`]).
    unsafe impl NSDraggingSource for BateriView {
        /// Copy only, inside and outside the application: the remote item
        /// stays where it is, a copy lands where it is dropped.
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn source_operation_mask(
            &self,
            _session: &NSDraggingSession,
            _context: NSDraggingContext,
        ) -> NSDragOperation {
            NSDragOperation::Copy
        }

        /// The drag ended: with no drop its promise's delegate is let go.
        #[unsafe(method(draggingSession:endedAtPoint:operation:))]
        fn session_ended(
            &self,
            session: &NSDraggingSession,
            _at: NSPoint,
            operation: NSDragOperation,
        ) {
            if let Some(pane) = self.pane() {
                pane.finder_drag_ended(session, operation);
            }
        }
    }
);

/// The file-system paths of the file URLs on the pasteboard.
///
/// The reading API is **chosen**: `readObjectsForClasses:options:` + the
/// `NSURL` class. `pasteboardItems()` would do the same job but would need
/// the `NSPasteboardItem` feature and the work left to us would still be
/// decoding the item into a URL.
///
/// The path is taken from `NSURL.path`: **percent-decoding is not written a
/// second time.** `bt-core`'s own decoder exists for OSC 7 and stays there
/// (the layering); here Foundation's own answer is read.
///
/// An item that cannot be decoded is **silently dropped**: not understanding
/// one part of a drop is no reason to drop all of it. The filtering has three
/// steps and the middle condition - the one that cannot be decoded into an
/// `NSURL`, the one that **does not say `isFileURL`** and the one that gives no path.
///
/// The middle step was added later: the `NSURL` class reads
/// `http://` too and `NSURL.path` answers it with `/foo`, so a user dropping a
/// web address would find a root-anchored path on the input line. The rule
/// is "only file URLs" and a text/URL drop is out of scope; the
/// registration was right, the code was missing.
fn dropped_paths(board: &NSPasteboard) -> Vec<String> {
    let classes: Retained<NSArray<AnyClass>> = NSArray::from_slice(&[NSURL::class()]);
    // SAFETY: both conditions of the signature are met - the class array
    // carries a real class (`NSURL`) and no options dictionary is given (`None`).
    let Some(objects) = (unsafe { board.readObjectsForClasses_options(&classes, None) }) else {
        return Vec::new();
    };
    objects
        .iter()
        .filter_map(|object| {
            object
                .downcast_ref::<NSURL>()
                .filter(|url| url.isFileURL())
                .and_then(NSURL::path)
                .map(|path| path.to_string())
        })
        .collect()
}

/// `NSNotFound`'s type in `NSRange` fields. The constant comes as `NSInteger`
/// while the ranges' two fields are `NSUInteger`; let the conversion be in one place.
const NOT_FOUND: NSUInteger = NSNotFound as NSUInteger;

/// "Nothing is marked" - `markedRange`'s answer with no composition.
const EMPTY_RANGE: NSRange = NSRange::new(NOT_FOUND, 0);

/// Reduces the text object the stack gave to a string - the **single**
/// decoding rule ([`NSTextInputClient::insertText_replacementRange`] and
/// `setMarkedText:` ask the same question).
///
/// The argument's type is documented as "must be of the right type" and in
/// practice two types come: a plain `NSString` (most paths) and an
/// `NSAttributedString` (marked text, the candidate window). `None` = neither;
/// the caller does not count that event as consumed and drops it to `encode_key`.
fn resolve_text(string: &AnyObject) -> Option<String> {
    if let Some(text) = string.downcast_ref::<NSString>() {
        return Some(text.to_string());
    }
    string
        .downcast_ref::<NSAttributedString>()
        .map(|text| text.string().to_string())
}

impl BateriView {
    pub(crate) fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ViewIvars {
            session: OnceCell::new(),
            gesture: Cell::new(Gesture::default()),
            consumed: Cell::new(false),
            marked_text: RefCell::new(String::new()),
            scroll_carry: Cell::new(0.0),
            smooth_scroll: Cell::new(true),
            metrics: Cell::new(None),
            dock_rows: Cell::new(0),
            cursor_rects: RefCell::new(Vec::new()),
            link: RefCell::new(LinkState::default()),
            origin: OnceCell::new(),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        // The single condition for being a drag destination: the view must say
        // **in advance** which types it accepts, otherwise `draggingEntered:` is
        // never called. The list has a single type - a plain-text drop is out of
        // scope and the escape rule is thus not conditional on the type.
        //
        // SAFETY: the `unsafe` block is only for the **static** access of
        // `NSPasteboardTypeFileURL` (the `clipboard` precedent); it is a real
        // pasteboard type registration and does not resolve to `None`.
        this.registerForDraggedTypes(&NSArray::from_slice(&[unsafe { NSPasteboardTypeFileURL }]));
        this
    }

    /// The number of **UTF-16 code units** of the composition text - that is `NSRange`'s unit.
    ///
    /// Not bytes: `ü` is one code unit but two bytes, and a dead-key
    /// composition lives exactly on those letters. Had `String::len()` been
    /// written the stack would see the composition's length as longer than it is.
    fn marked_utf16_len(&self) -> usize {
        self.ivars().marked_text.borrow().encode_utf16().count()
    }

    /// Attaches the session; from this moment the keys go to the PTY.
    pub(crate) fn attach(&self, session: Arc<Session>) {
        // Had a second call been silently dropped the keys would go to the old
        // session and the window would look like it does not type - without
        // leaving even a line of trace.
        assert!(
            self.ivars().session.set(session).is_ok(),
            "session bound a second time"
        );
    }

    /// Refreshes the mouse translation's inputs: from the `start_session` and
    /// `resize` path, with the very grid that goes to the session and the
    /// link. The three are written at the same call site; one cannot change
    /// while another stays stale.
    pub(crate) fn set_metrics(&self, grid: crate::app::Grid, dock_rows: u16) {
        self.ivars()
            .metrics
            .set(Some((grid.cell, (grid.cols, grid.rows))));
        self.ivars().dock_rows.set(dock_rows);
    }

    /// Whether scrolling goes smooth or by line steps - the window gives the
    /// resolved `bool` at launch and on every save/system notification
    /// (`TerminalPane::set_smooth_scroll`).
    ///
    /// The switch to `false` does not touch a gliding in flight: Reduce Motion
    /// and `snap` already end it in the link, and in `"off"` itself the next
    /// line step drops the remaining fraction and increments the generation
    /// (`ScrollIntent::Lines`) and the gliding in flight settles in its own time.
    pub(crate) fn set_smooth_scroll(&self, smooth: bool) {
        self.ivars().smooth_scroll.set(smooth);
    }

    /// `scrollWheel:`'s smooth arm ([`smooth_wheel`]). Its only difference
    /// from the line arm is the amount and the intent; the pointer, Shift and
    /// the route are the same.
    ///
    /// **The remainder's rule** is the same sentence as the line arm's, with
    /// two exceptions: in the scroll arm (`Wheel::Scrolled`) the remainder is
    /// **reset** - the owner of the fractional position there is `Session`,
    /// the remainder has no consumer and if it stayed it would leak into a
    /// later mode (arrow, report) - but it is **kept on a notch**, because the
    /// notch's whole line is born of the remainder and `Scrolled(0)` there
    /// means not "the end" but "a gliding request". The `Ignored` of an event
    /// with no whole line does not erase the remainder either: the arrow and
    /// report arms reject zero lines and in a slow trackpad scroll had every
    /// small event reset the remainder `less` would never move.
    ///
    /// The **settling** of a trackpad gesture glides during a drag too and the
    /// end does not follow it until the next `mouseDragged:`: the amount is
    /// under half a line and this limit is known and accepted.
    fn smooth_scroll_wheel(&self, event: &NSEvent, session: &Session, unit: f64) {
        let carry = &self.ivars().scroll_carry;
        let (step, rest) = smooth_wheel(
            event.scrollingDeltaY(),
            unit,
            carry.get(),
            event.phase(),
            event.momentumPhase(),
        );
        carry.set(rest);
        let Some(mut step) = step else {
            return;
        };
        // The wheel above the dock is the dock's (with whole lines; there is no
        // gliding since there is no dock window resting at half a line). **The
        // gesture's start and end stay the grid's** (found in code review): had the
        // `Settle` of a scroll that began in the grid and ended with momentum
        // above the dock been swallowed, the grid would hang at half a line.
        if !matches!(
            step.intent,
            ScrollIntent::GestureBegan | ScrollIntent::Settle
        ) && self.dock_wheel(event, session, step.lines)
        {
            return;
        }
        // **A notch does not glide during a held drag**, it goes by line
        // steps: the gliding's amount scrolls the window in the frame path and
        // there nobody moves the selection's end to the mouse - while the mouse
        // is still the end would stay on the old row (found in code review). The line
        // step returns `Scrolled(n)` and `follow_pointer` runs as today; when
        // selecting, precision in scrolling comes before ornament.
        if step.intent == ScrollIntent::Glide && self.ivars().gesture.get().dragging() {
            step.intent = ScrollIntent::Lines;
        }
        // The pointer's cell and the fill rejection's zero, with the
        // rationale in the line arm (`scrollWheel:`).
        let Some(pointer) =
            self.window_point_cell(event.locationInWindow(), OutOfGrid::Clamp { fill_rows: 0 })
        else {
            return;
        };
        let shift = event.modifierFlags().contains(NSEventModifierFlags::Shift);
        match session.scroll_wheel(step.rows, step.lines, step.intent, pointer, shift) {
            Wheel::Ignored if step.lines == 0 => {}
            Wheel::Scrolled(0) if step.intent == ScrollIntent::Glide => {}
            Wheel::Scrolled(0) | Wheel::Ignored => carry.set(0.0),
            Wheel::Scrolled(_) => {
                // In the whole-line arms (notch, the line step in a drag) the
                // remainder's source is the notch, kept as in the line arm.
                if !matches!(step.intent, ScrollIntent::Glide | ScrollIntent::Lines) {
                    carry.set(0.0);
                }
                self.follow_pointer(session);
            }
            Wheel::Sent => {}
        }
    }

    /// Reports the keyboard's place to the owner pane; silent if the view is
    /// not yet attached to a pane. The owner is from `superview()`:
    /// the pane is this view's direct parent.
    fn keyboard_moved(&self, here: bool) {
        if let Some(pane) = self.pane() {
            pane.keyboard_moved(here);
        }
    }

    /// The cells the search panel covers ([`SearchCover`]) - `panel` is in the
    /// view's own coordinates (points). If there is no metrics or scale
    /// nothing is covered.
    pub(crate) fn search_cover(&self, panel: NSRect) -> SearchCover {
        let (Some((metrics, _)), Some(window)) = (self.ivars().metrics.get(), self.window()) else {
            return SearchCover::default();
        };
        let scale = window.backingScaleFactor();
        let origin = self.ivars().origin.get().map_or(0.0, Origin::px);
        // The view is flipped: the panel's bottom edge is `maxY`.
        cover_of(
            (panel.origin.y + panel.size.height) * scale,
            panel.origin.x * scale,
            f64::from(origin),
            metrics,
        )
    }

    /// The mouse translation's measure and grid ([`ViewIvars::metrics`]).
    pub(crate) fn metrics(&self) -> Option<(CellMetrics, (u16, u16))> {
        self.ivars().metrics.get()
    }

    /// The drawn frame's origin body ([`ViewIvars::origin`]); `None` before the link.
    pub(crate) fn origin(&self) -> Option<&Origin> {
        self.ivars().origin.get()
    }

    /// The session; `None` before it is born.
    pub(crate) fn session(&self) -> Option<&Arc<Session>> {
        self.ivars().session.get()
    }

    /// The ⌘-hover and ⌘-click state ([`crate::hyperlink`]).
    pub(crate) fn link_state(&self) -> &RefCell<LinkState> {
        &self.ivars().link
    }

    /// Binds the mouse translation's vertical origin; once, right after the
    /// link is born.
    ///
    /// A separate call from `set_metrics`, because its source is separate: that
    /// triple comes from the window geometry, this one from the link, and the
    /// link is set up after `set_metrics`
    /// (`pane::TerminalPane::start_session`). Had a second call been silently
    /// dropped the mouse would read the old body, i.e. an origin that is zero forever.
    pub(crate) fn attach_origin(&self, origin: Origin) {
        assert!(
            self.ivars().origin.set(origin).is_ok(),
            "origin bound a second time"
        );
    }

    /// The common body of the mouse button's **six** selectors: press or
    /// release, three buttons.
    ///
    /// The decision is given by `bt-core` ([`Session::mouse_button`]) - the
    /// mode is neither kept nor asked here. This is only the AppKit
    /// translation: the cell, the modifiers and the answer's three arms.
    ///
    /// **A press and a release go through different cell gates** and this is
    /// not an inconsistency, it is the two faces of [`OutOfGrid`]'s single
    /// rule. A press *starts* a gesture: a point falling outside the grid is
    /// rejected, so a press on the title bar, the left padding, the dock band
    /// and the fill band produces neither a report nor a selection. A release *ends* a started gesture: the point is
    /// clamped, because a dropped release would leave a **button stuck** in
    /// the application.
    ///
    /// On release `Clamp`'s `fill_rows` is passed as zero: the area above the
    /// band must give a cell too, `bt-core` does the clamping.
    fn button_event(&self, event: &NSEvent, button: MouseButton, pressed: bool) {
        let Some(session) = self.ivars().session.get() else {
            return;
        };
        if !pressed {
            match self.with_gesture(|g| g.released(button)) {
                Release::Report => {
                    let clamp = OutOfGrid::Clamp { fill_rows: 0 };
                    if let Some(cell) = self.window_point_cell(event.locationInWindow(), clamp) {
                        self.report_button(session, button, false, cell, event);
                    }
                }
                // The gesture in the dock ended: if it was a click without a drag the caret goes there.
                Release::Dock => session.dock_click(),
                // A ⌘-click on a link: opened if the pointer is still over the
                // range locked at the press and this is the first click.
                Release::Link => self.link_release(event),
                Release::Done => {}
            }
            return;
        }
        // The upload line's buttons and the load indicator: on the
        // context line, without entering the gesture ledger -
        // a click is a button, it starts no drag.
        if button == MouseButton::Left && self.context_control(event) {
            return;
        }
        self.forget_link_menu();
        self.with_gesture(|g| g.begin_press(button));
        // **A ⌘-press on the shown link is the link's in every mode**:
        // neither a report (vim, htop, Claude Code see nothing) nor a selection,
        // Shift or not. Only a **verified** hover counts — the press before the
        // path's `stat` returned takes today's route.
        //
        // A **remote** link can also be dragged out to Finder: the
        // ledger keeps the press point and the first motion past the threshold
        // starts the file promise drag ([`Drag::Link`]).
        if button == MouseButton::Left
            && let Some(draggable) = self.link_press(event)
        {
            let at = event.locationInWindow();
            let from = draggable.then_some((at.x, at.y));
            self.with_gesture(|g| g.pressed_link(from));
            return;
        }
        // **The dock's input line before the grid** and without asking the
        // mouse mode at all: the band is not the application's screen but the
        // terminal's own surface. Only the left button; the
        // context line and the band's padding are rejected, so a press there does nothing.
        if button == MouseButton::Left
            && let Some(point) = self.window_point_dock(event.locationInWindow(), OutOfGrid::Reject)
        {
            let shift = modifiers(event).shift;
            let clicks = event.clickCount();
            match self.with_gesture(|g| g.pressed_dock(clicks, shift)) {
                Press::Select(kind) => session.dock_select(kind, point),
                Press::Extend => session.dock_extend(point),
            }
            return;
        }
        let Some(cell) = self.window_point_cell(event.locationInWindow(), OutOfGrid::Reject) else {
            // The fill band and the dock are never the application's screen: a
            // right click there is the terminal's, the link menu's.
            if button == MouseButton::Right {
                self.link_menu(event);
            }
            return;
        };
        let answer = self.report_button(session, button, true, cell, event);
        // Mouse mode off (or Shift, the mode's escape): the right click is the
        // terminal's — the link menu. `pressed` sets no bit for a right
        // `Select`, so the menu swallowing `rightMouseUp:` leaves nothing stale.
        if button == MouseButton::Right && answer == Click::Select {
            self.link_menu(event);
        }
        let shift = modifiers(event).shift;
        let clicks = event.clickCount();
        match self.with_gesture(|g| g.pressed(button, answer, clicks, shift)) {
            // The anchor goes **with its half**: whichever half of the cell the
            // press is in, the boundary passes there and stays there throughout
            // the drag. On a single click the two ends are the same and the
            // selection is empty - a click without a drag selects nothing, Cmd-C
            // does not touch the pasteboard; the first motion in the opposite
            // direction does not empty the selection, it grows from the mouse's
            // end. On a double/triple click it takes the whole word/line under
            // the same point.
            Some(Press::Select(kind)) => session.set_selection(kind, cell, cell),
            Some(Press::Extend) => session.extend_selection(cell),
            None => {}
        }
    }

    /// Take-modify-put on the gesture ledger ([`ViewIvars::gesture`]). No
    /// `Session` call inside the closure: the ledger stays pure.
    fn with_gesture<R>(&self, change: impl FnOnce(&mut Gesture) -> R) -> R {
        let cell = &self.ivars().gesture;
        let mut gesture = cell.get();
        let answer = change(&mut gesture);
        cell.set(gesture);
        answer
    }

    /// The common body of a held drag: if the gesture is the application's a
    /// motion report, if the terminal's the selection's end. The route was
    /// locked at the press and is not asked again here
    /// ([`Gesture::dragged`]): releasing Shift or the application turning the
    /// mode off in the middle of the same gesture must not change the path.
    fn drag_event(&self, event: &NSEvent, button: MouseButton) {
        let at = event.locationInWindow();
        match self.with_gesture(|g| g.dragged(button, (at.x, at.y))) {
            // A ⌘-press on a remote link moved past the threshold: the file
            // promise drag to Finder. AppKit owns the mouse from here.
            Drag::Link => self.link_drag(event),
            Drag::Report => self.motion_event(event, Some(button)),
            Drag::Select => {
                if let Some((session, cell)) = self.session_cell(event) {
                    session.update_selection(cell);
                }
            }
            // A drag that began in the dock stays in the dock: the point is
            // clamped into the input block, it does not overflow onto the grid.
            // **A drag past the block's edge scrolls the vertical window**:
            // so the selection can extend to invisible rows in an
            // input past the ceiling. One row per event, i.e. as the mouse moves
            // beyond the edge - there is no periodic timer.
            Drag::SelectDock => {
                let at = event.locationInWindow();
                let clamp = OutOfGrid::Clamp { fill_rows: 0 };
                if let Some(session) = self.ivars().session.get() {
                    let edge = self.dock_edge(at);
                    if edge != 0 {
                        session.dock_scroll(edge);
                    }
                    if let Some(point) = self.window_point_dock(at, clamp) {
                        session.dock_drag(point);
                    }
                }
            }
            Drag::Ignore => {}
        }
    }

    /// Sends the button report and, **if it was reported**, stamps the
    /// throttling's notch to that cell ([`Gesture::stamp`]). The criterion is
    /// the answer itself, because in the `Select` and `Ignored` arms nothing
    /// went to the application and stamping would silently swallow the first
    /// hover report there.
    fn report_button(
        &self,
        session: &Session,
        button: MouseButton,
        pressed: bool,
        cell: SelectionPoint,
        event: &NSEvent,
    ) -> Click {
        let answer = session.mouse_button(button, pressed, cell, modifiers(event));
        if answer == Click::Sent {
            self.with_gesture(|g| g.stamp(cell));
        }
        answer
    }

    /// Releases the buttons a lost `mouseUp:` left pressed in the application
    /// ([`Gesture::take_lost_releases`]). Silently dropping the bit is not
    /// enough: the application still thinks the button is **held** and grows its
    /// own selection on every motion report, so the release itself must be sent.
    ///
    /// The stale-bit cleanup at a press ([`Gesture::begin_press`]) does not
    /// replace this: that fixes the terminal's own ledger and runs only when the
    /// user **presses the same button again**.
    fn flush_lost_releases(&self, session: &Session, event: &NSEvent) {
        let lost: Vec<MouseButton> = self.with_gesture(|g| g.take_lost_releases().collect());
        if lost.is_empty() {
            return;
        }
        // The continuation of a gesture, not its start: the coordinate is clamped.
        let clamp = OutOfGrid::Clamp { fill_rows: 0 };
        let Some(cell) = self.window_point_cell(event.locationInWindow(), clamp) else {
            return;
        };
        for button in lost {
            self.report_button(session, button, false, cell, event);
        }
    }

    /// The single path of the motion report: buttonless (`mouseMoved:`) and
    /// held (`*MouseDragged:`).
    ///
    /// **The throttling is before the `bt-core` call** ([`Gesture::moved_to`]):
    /// if the cell did not change the `Term` lock is never taken. The notch is
    /// written even if no report goes - so that with the mode off too there
    /// remains a single resultless call per cell change, not per pixel.
    ///
    /// The cell's gate depends on the button ([`OutOfGrid`]): a held drag is
    /// the continuation of a started gesture and is clamped, while a buttonless
    /// motion *states* a place and is rejected outside the grid - a pointer
    /// roaming over the title bar must not report row 0 to the application.
    fn motion_event(&self, event: &NSEvent, button: Option<MouseButton>) {
        let Some(session) = self.ivars().session.get() else {
            return;
        };
        let outside = if button.is_some() {
            OutOfGrid::Clamp { fill_rows: 0 }
        } else {
            self.flush_lost_releases(session, event);
            OutOfGrid::Reject
        };
        let Some(cell) = self.window_point_cell(event.locationInWindow(), outside) else {
            return;
        };
        if !self.with_gesture(|g| g.moved_to(cell)) {
            return;
        }
        session.mouse_motion(button, cell, modifiers(event));
    }

    /// The session + the end under the event (cell and its half). `None` if
    /// the three (`session`, metrics, grid) are not all present: the
    /// selection's end cannot be moved with half the information. A point
    /// falling above the fill band is also `None` ([`point_to_cell`]).
    ///
    /// Today its only consumer is the drag; the button events want the session
    /// and the cell separately ([`BateriView::button_event`]), because the
    /// release takes the cell through another gate (`fill_rows = 0`).
    fn session_cell(&self, event: &NSEvent) -> Option<(Arc<Session>, SelectionPoint)> {
        let session = Arc::clone(self.ivars().session.get()?);
        let cell = self.event_cell(event)?;
        Some((session, cell))
    }

    /// Lowers the event point to a selection end. `None` while the metrics or
    /// the window do not exist yet, while the grid is zero-sized and above the
    /// fill band - a point beyond the edge sticks.
    fn event_cell(&self, event: &NSEvent) -> Option<SelectionPoint> {
        let fill_rows = self.fill_rows();
        self.window_point_cell(event.locationInWindow(), OutOfGrid::Clamp { fill_rows })
    }

    /// Lowers a point in window coordinates to a selection end - [`Self::event_cell`]'s
    /// eventless form: in key-driven scrolling there is no mouse event carrying the mouse's place.
    ///
    /// `outside` is a **parameter**, not a field: the same point becomes a
    /// selection end or a coordinate going to a report depending on its caller,
    /// and when it falls outside the grid the two want different things
    /// ([`OutOfGrid`]).
    fn window_point_cell(&self, in_window: NSPoint, outside: OutOfGrid) -> Option<SelectionPoint> {
        let (metrics, (cols, rows)) = self.ivars().metrics.get()?;
        let point = self.convertPoint_fromView(in_window, None);
        let scale = self.window()?.backingScaleFactor();
        // The origin is the **drawn** frame's value: without a link (the first
        // window) it is zero and the drawing is stuck to the ceiling, so the two are consistent.
        let origin_px = self.ivars().origin.get().map_or(0.0, Origin::px);
        point_to_cell(
            (point.x, point.y),
            metrics,
            f64::from(origin_px),
            outside,
            scale,
            cols,
            rows,
        )
    }

    /// Window point → row + column + half in the dock's input block. `None`
    /// if there is no dock or the point is outside the input block (under `Reject`).
    ///
    /// The geometry is from the **drawn frame** ([`bt_gpu::Origin::dock`]):
    /// the block's top and its row count are published in the same write
    /// as the grid's origin, so while the band grows the mouse reads neither the
    /// grid nor the block a frame behind. If no frame has been drawn yet, the
    /// PTY pad's single-row block ([`dock_input_top_px`]); the height in that
    /// arm is from the view's bounds - the drawable's size is set in the same
    /// call as that (`TerminalPane::sync_geometry`).
    ///
    /// A drawn frame with **zero** input rows (a remote session's status bar,
    /// or no band at all while a program reads the keyboard itself) is left
    /// to [`point_to_cell`]'s rejection of a zero-row grid: no point is the
    /// dock's, so a click on the lowered grid's bottom row reaches the grid.
    pub(crate) fn window_point_dock(
        &self,
        in_window: NSPoint,
        outside: OutOfGrid,
    ) -> Option<SelectionPoint> {
        let (metrics, (cols, _)) = self.ivars().metrics.get()?;
        let dock_rows = self.ivars().dock_rows.get();
        if dock_rows == 0 {
            return None;
        }
        let point = self.convertPoint_fromView(in_window, None);
        let scale = self.window()?.backingScaleFactor();
        let (top, rows) = match self.ivars().origin.get().and_then(Origin::dock) {
            Some((top, rows)) => (f64::from(top), rows),
            None => (
                dock_input_top_px(self.bounds().size.height * scale, metrics, dock_rows),
                1,
            ),
        };
        point_to_cell((point.x, point.y), metrics, top, outside, scale, cols, rows)
    }

    /// A key, a press, the wheel or a mouse move: the remote load indicator
    /// keeps sampling while the user is around. A stamp, no
    /// `Term` lock — it runs at mouse-move rate.
    fn note_interaction(&self) {
        if let Some(pane) = self.pane() {
            pane.note_interaction();
        }
    }

    /// The owner pane of this view - its direct superview;
    /// `None` if the view is not yet attached to a pane. There is no linear
    /// search in a window list or reaching for the application delegate: the
    /// owner is in the view tree.
    pub(crate) fn pane(&self) -> Option<Retained<TerminalPane>> {
        // SAFETY: reading the superview; the returned `Retained` keeps it alive
        // for the caller and we are on the main thread (`MainThreadOnly`).
        let parent = unsafe { self.superview() }?;
        parent.downcast::<TerminalPane>().ok()
    }

    /// Window point → dock-local column on the context line and the context
    /// line's budget: the line is **below** the input
    /// block, the column pitch is the small class's advance. `None` if there is
    /// no dock, no frame yet or the point is not on the context line.
    ///
    /// The **single** geometry of click and hover; the button's column range
    /// comes from the same layout as the drawing (`bt_core::transfer_button_at`),
    /// so the column the two see and the drawn fill cannot diverge.
    fn context_column(&self, in_window: NSPoint) -> Option<(u16, u16)> {
        let (metrics, (cols, _)) = self.ivars().metrics.get()?;
        let (top, rows) = self.ivars().origin.get().and_then(Origin::dock)?;
        let scale = self.window()?.backingScaleFactor();
        let at = self.convertPoint_fromView(in_window, None);
        let col = context_col_at(metrics, top, rows, (at.x * scale, at.y * scale))?;
        Some((col, bt_gpu::context_cols(cols, metrics)))
    }

    /// The rectangle, at view points, of the dock-local `[start, end)` column
    /// range on the context line - the inverse of [`Self::context_column`],
    /// from the same geometry ([`context_span_px`]): the anchor of the "Show
    /// files (N)" popover and the buttons' hand cursor
    /// ([`Self::hand_cursor_rects`]).
    pub(crate) fn context_span_rect(&self, start: u16, end: u16) -> Option<NSRect> {
        let (metrics, _) = self.ivars().metrics.get()?;
        let (top, rows) = self.ivars().origin.get().and_then(Origin::dock)?;
        let scale = self.window()?.backingScaleFactor();
        let (x, y, width, height) = context_span_px(metrics, top, rows, start, end);
        Some(NSRect::new(
            NSPoint::new(x / scale, y / scale),
            NSSize::new(width / scale, height / scale),
        ))
    }

    /// The context line's budget ([`bt_gpu::context_cols`]); `None` if there is no metrics.
    pub(crate) fn context_budget(&self) -> Option<u16> {
        let (metrics, (cols, _)) = self.ivars().metrics.get()?;
        Some(bt_gpu::context_cols(cols, metrics))
    }

    /// The upload buttons' hand cursor: AppKit's **cursor
    /// rect**, the button's whole fill. Not `set()`, because the window's
    /// re-evaluation of the cursor (every `↑ N%` write of the title, becoming
    /// key, the frame) sends the view `cursorUpdate:` and `NSView`'s default
    /// sets the arrow - measured; a hand set by hand turned back into an arrow
    /// at every percentage change and came back at the next refresh. The cursor
    /// rect is that evaluation's **input**: inside the rectangle AppKit sets the
    /// hand itself, outside the arrow, in a non-key window none at all.
    ///
    /// The ⌘-hovered link's cells join the **same** list: one
    /// `resetCursorRects`, one comparison in [`Self::sync_cursor_rects`].
    fn hand_cursor_rects(&self) {
        let rects = self.hand_rects();
        let hand = NSCursor::pointingHandCursor();
        for rect in &rects {
            self.addCursorRect_cursor(*rect, &hand);
        }
        self.ivars().cursor_rects.replace(rects);
    }

    /// Every hand-cursor rectangle: the upload buttons, the load indicator
    /// and the shown link.
    fn hand_rects(&self) -> Vec<NSRect> {
        let mut rects = self.upload_button_rects();
        rects.extend(self.stats_rect());
        rects.extend(self.sign_in_rect());
        rects.extend(self.link_rects());
        rects
    }

    /// The load indicator's rectangle, in view points — the same range as its
    /// click and its popover's anchor (`Session::stats_span`); `None` if it is
    /// not drawn.
    fn stats_rect(&self) -> Option<NSRect> {
        let budget = self.context_budget()?;
        let (start, end) = self.pane()?.session()?.stats_span(budget)?;
        self.context_span_rect(start, end)
    }

    /// The Sign In… button's rectangle, in view points — its click's range
    /// (`Session::sign_in_span`); `None` if it is not drawn.
    fn sign_in_rect(&self) -> Option<NSRect> {
        let budget = self.context_budget()?;
        let (start, end) = self.pane()?.session()?.sign_in_span(budget)?;
        self.context_span_rect(start, end)
    }

    /// The buttons' current rectangles, in view points - from click and hover's
    /// geometry ([`Self::context_span_rect`]); empty if there is no upload.
    fn upload_button_rects(&self) -> Vec<NSRect> {
        let (Some(pane), Some(context)) = (self.pane(), self.context_budget()) else {
            return Vec::new();
        };
        pane.upload_button_spans(context)
            .into_iter()
            .filter_map(|(start, end)| self.context_span_rect(start, end))
            .collect()
    }

    /// Makes the installed cursor rects refresh if they are stale: buttons
    /// appeared or went away (so the hand does not hang), or the dock's
    /// **drawn** place moved - point size, window size, the band's gliding, the
    /// alternate screen. The rectangle is read from the last drawn frame and
    /// AppKit's own triggers (the frame) can run before that frame, so the
    /// criterion is the geometry itself. The callers are every motion and every
    /// refresh (`TerminalPane::upload_hover`, `show_transfer`); on the same
    /// rectangle it is a no-op, i.e. the cursor is not re-evaluated.
    pub(crate) fn sync_cursor_rects(&self) {
        let fresh = self.hand_rects();
        if *self.ivars().cursor_rects.borrow() == fresh {
            return;
        }
        if let Some(window) = self.window() {
            window.invalidateCursorRectsForView(self);
        }
    }

    /// Whether the click landed on one of the upload line's buttons
    /// or, failing that, on the load indicator (never both —
    /// the indicator is not drawn while an upload row is); `true` → the click
    /// was consumed. The geometry is [`Self::context_column`]'s.
    fn context_control(&self, event: &NSEvent) -> bool {
        self.context_column(event.locationInWindow())
            .zip(self.pane())
            .is_some_and(|((col, context), pane)| {
                pane.upload_click(col, context)
                    || pane.stats_click(col, context)
                    || pane.sign_in_click(col, context)
            })
    }

    /// The mouse's **current** place on the context line ([`Self::context_column`]):
    /// an eventless question - so the hover is recomputed when the line changes
    /// under the mouse (refresh, the list closed, the window became key).
    pub(crate) fn pointer_context_column(&self) -> Option<(u16, u16)> {
        self.context_column(self.window()?.mouseLocationOutsideOfEventStream())
    }

    /// The upload button under the mouse: a window change asks
    /// for a frame only when the button changes and turns the cursor
    /// (`TerminalPane::upload_hover`).
    fn upload_hover(&self, event: &NSEvent) {
        if let Some(pane) = self.pane() {
            pane.upload_hover(self.context_column(event.locationInWindow()));
        }
    }

    /// If the wheel is above the dock's input block it gives it to the dock's
    /// vertical window; `true` → the event was consumed. If the
    /// dock does not overflow (`Session::dock_scroll` `false`) the event is the
    /// grid's, as today.
    fn dock_wheel(&self, event: &NSEvent, session: &Session, lines: i32) -> bool {
        self.window_point_dock(event.locationInWindow(), OutOfGrid::Reject)
            .is_some()
            && session.dock_scroll(lines)
    }

    /// Where the point is relative to the dock's input block: `1` if above (the
    /// window should scroll backward), `-1` if below, `0` if inside or there is
    /// no dock - `Session::dock_scroll`'s direction. The geometry is
    /// [`Self::window_point_dock`]'s.
    fn dock_edge(&self, in_window: NSPoint) -> i32 {
        let Some((metrics, _)) = self.ivars().metrics.get() else {
            return 0;
        };
        let Some((top, rows)) = self.ivars().origin.get().and_then(Origin::dock) else {
            return 0;
        };
        let Some(window) = self.window() else {
            return 0;
        };
        let y = self.convertPoint_fromView(in_window, None).y * window.backingScaleFactor()
            - f64::from(top);
        let height = f64::from(metrics.cell_px().1) * f64::from(rows);
        if y < 0.0 {
            1
        } else if y >= height {
            -1
        } else {
            0
        }
    }

    /// The drawn frame's fill band length - from the **same body** as the
    /// origin ([`bt_gpu::Origin`]), so the two belong to the same frame. Zero if
    /// there is no link: no band and no drawing.
    fn fill_rows(&self) -> u16 {
        self.ivars().origin.get().map_or(0, Origin::fill_rows)
    }

    /// A window scroll; if there is a held drag it moves the selection's end to
    /// the cell **now** under the mouse - the mouse did not move but the content
    /// under it changed. Going down into the scrollback with the button held
    /// (wheel or Shift+PgUp) extends the selection there; the anchor is in
    /// `bt-core` at the grid's absolute position, it does not slide. The two
    /// triggers pass through a **single** path so that the same gesture does not
    /// show two different behaviours.
    ///
    /// The mouse position is read from the window, not the event
    /// (`mouseLocationOutsideOfEventStream`): a key event has no position.
    ///
    /// `dragging` alone is not enough: if `mouseUp:` never reaches this view (a
    /// modal in the middle of a drag, a system gesture) the flag stays stale
    /// `true` and every buttonless scroll would silently extend the old
    /// selection - the next Cmd-C copies it. Whether the button is **really**
    /// held is asked of the system; if not, the stale flag is lowered here.
    fn follow_pointer(&self, session: &Session) {
        if !self.ivars().gesture.get().dragging() {
            return;
        }
        if NSEvent::pressedMouseButtons() & 1 == 0 {
            self.with_gesture(Gesture::lost_drag);
            return;
        }
        let Some(window) = self.window() else {
            return;
        };
        // If `None` comes the end is **not moved**: if the mouse went above the
        // fill band the selection stays at its last valid cell, it does not jump to row 0.
        let fill_rows = self.fill_rows();
        if let Some(cell) = self.window_point_cell(
            window.mouseLocationOutsideOfEventStream(),
            OutOfGrid::Clamp { fill_rows },
        ) {
            session.update_selection(cell);
        }
    }
}

/// The context line's cell band, in physical pixels and in the view's
/// (flipped) space: `[top, bottom)`. `top`/`rows` are the drawn frame's dock
/// (`Origin::dock`). The band is the fill itself (`Frame::dock_button_draws`);
/// the gap above it and the breathing padding below it are outside the fill.
fn context_band_px(metrics: CellMetrics, top: f32, rows: u16) -> (f64, f64) {
    let band_top = f64::from(top)
        + f64::from(metrics.cell_px().1) * f64::from(rows)
        + f64::from(bt_gpu::context_row_offset(rows, metrics));
    (band_top, band_top + f64::from(metrics.cell_px().1))
}

/// Physical-pixel point → dock-local column on the context line; `None` if
/// outside the band or the left padding. The column pitch is the small
/// class's advance. The inverse of [`context_span_px`]: click, hover and the
/// hand cursor read these two, so the column and the rectangle cannot diverge.
fn context_col_at(metrics: CellMetrics, top: f32, rows: u16, (x, y): (f64, f64)) -> Option<u16> {
    let (band_top, band_bottom) = context_band_px(metrics, top, rows);
    let x = x - f64::from(metrics.gutter_px());
    if y < band_top || y >= band_bottom || x < 0.0 {
        return None;
    }
    // audit: `x ≥ 0` and the window width fits a `u16` column; an overflowing
    // value only becomes a column that falls on no button.
    Some((x / f64::from(metrics.context_cell_px())).floor() as u16)
}

/// The rectangle of the dock-local `[start, end)` column range, physical
/// pixels: `(x, y, width, height)` - the inverse of [`context_col_at`].
fn context_span_px(
    metrics: CellMetrics,
    top: f32,
    rows: u16,
    start: u16,
    end: u16,
) -> (f64, f64, f64, f64) {
    let (band_top, band_bottom) = context_band_px(metrics, top, rows);
    let cell = f64::from(metrics.context_cell_px());
    let x = f64::from(metrics.gutter_px()) + f64::from(start) * cell;
    let width = f64::from(end.saturating_sub(start)) * cell;
    (x, band_top, width, band_bottom - band_top)
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_foundation::ns_string;

    /// The scenes' grid measure; the padding is an **argument**, because two
    /// separate things are asked: the cell arithmetic (padding zero) and the padding itself.
    fn grid(gutter: u16) -> CellMetrics {
        CellMetrics::new(9, 18, 9, gutter, 1).expect("non-zero cell")
    }

    #[test]
    fn a_click_on_the_lowered_grids_bottom_row_is_the_grids() {
        // No band (a program reading the keyboard itself): the grid is drawn
        // the whole PTY share lower, so its bottom row sits where the dock's
        // input row was. The drawn frame's dock is a zero-row block below the
        // window (`Frame::dock_hit`), so the dock rejects the point and the
        // grid takes it — the bottom row, not a phantom dock row.
        let metrics = grid(8);
        let rows: u16 = 10;
        let share = f64::from(bt_gpu::dock_px(bt_gpu::DOCK_ROWS, metrics));
        let height = f64::from(rows) * 18.0 + share;
        let click = (40.0, height - 9.0);
        let dock_top = height + f64::from(metrics.gutter_px());
        assert_eq!(
            point_to_cell(click, metrics, dock_top, OutOfGrid::Reject, 1.0, 40, 0),
            None,
            "a zero-row dock takes no point"
        );
        let cell = point_to_cell(click, metrics, share, OutOfGrid::Reject, 1.0, 40, rows)
            .expect("the grid takes it");
        assert_eq!(cell.row, rows - 1);
    }

    #[test]
    fn the_button_rect_and_the_pointer_column_read_one_geometry() {
        // The button's hand cursor (cursor rect) and the click/hover's column
        // must read the same band and the same pitch: were they to diverge the
        // hand would appear beside the button. Two dock shapes: with an input
        // line (a gap between lines) and a remote session (input line zero).
        let metrics = CellMetrics::new(16, 33, 13, 8, 2).expect("cell");
        for (top, rows) in [(500.0_f32, 2_u16), (620.0, 0)] {
            let (start, end) = (40_u16, 52_u16);
            let (x, y, width, height) = context_span_px(metrics, top, rows, start, end);
            assert_eq!(height, f64::from(metrics.cell_px().1));
            let at = |px: f64, py: f64| context_col_at(metrics, top, rows, (px, py));
            let mid = y + height / 2.0;
            assert_eq!(
                at(x + 0.01, mid),
                Some(start),
                "left edge is the first column"
            );
            assert_eq!(
                at(x + width - 0.01, mid),
                Some(end - 1),
                "inside the right edge is the last column"
            );
            assert_eq!(
                at(x + width, mid),
                Some(end),
                "right edge is outside the range"
            );
            assert_eq!(at(x + 0.01, y), Some(start), "the band's top is inside");
            assert_eq!(at(x + 0.01, y - 0.01), None, "above the band is outside");
            assert_eq!(at(x + 0.01, y + height), None, "below the band is outside");
            assert_eq!(
                at(f64::from(metrics.gutter_px()) - 0.01, mid),
                None,
                "left padding is outside"
            );
        }
    }

    #[test]
    fn the_search_panel_covers_whole_rows_and_the_columns_under_it() {
        // 9×18 cell, 4 px padding. The panel's bottom is 40 px: rows 0 and 1
        // (0…36) and half of row 2 are covered, the first fully visible row is 3.
        let cover = cover_of(40.0, 4.0 + 9.0 * 30.5, 0.0, grid(4));
        assert_eq!(
            cover,
            SearchCover {
                first_row: 3,
                from_col: 30
            }
        );
        // A panel ending at a row boundary does not cover that row.
        assert_eq!(cover_of(36.0, 4.0, 0.0, grid(4)).first_row, 2);
        // The origin is below (bottom-stuck content): the panel does not touch
        // the grid at all, the band's rows are in the open - negative.
        assert_eq!(cover_of(40.0, 4.0, 76.0, grid(4)).first_row, -2);
        // The left edge inside the padding is clamped to column 0.
        assert_eq!(cover_of(40.0, 0.0, 0.0, grid(4)).from_col, 0);
    }

    /// The tests' common scene: 100×33 grid, 9×18 cell, @2x.
    /// The view is 450×297 points.
    ///
    /// **The left padding is zero in this scene** and that is deliberate: what
    /// the tests below ask is the arithmetic of the cell and its half, and
    /// shifting the expected x values by the padding would make their
    /// rationales unreadable. The padding's own test is `the_gutter_shifts_the_grid_origin`.
    fn scene_point(view_px: (f64, f64)) -> Option<SelectionPoint> {
        point_to_cell(
            view_px,
            grid(0),
            0.0,
            OutOfGrid::Clamp { fill_rows: 0 },
            2.0,
            100,
            33,
        )
    }

    /// The scene's cell and half are read separately: let the cell tests look
    /// at the cell, the half tests at the half.
    fn scene(view_px: (f64, f64)) -> Option<(u16, u16)> {
        scene_point(view_px).map(|point| (point.col, point.row))
    }

    fn scene_half(view_px: (f64, f64)) -> Option<CellHalf> {
        scene_point(view_px).map(|point| point.half)
    }

    #[test]
    fn view_origin_maps_to_top_left_cell() {
        // The view is `isFlipped`: the top-left corner is cell (0,0). Had y come
        // from the bottom it would have landed on row 32.
        assert_eq!(scene((0.0, 0.0)), Some((0, 0)));
    }

    #[test]
    fn cell_middle_stays_in_same_cell() {
        // The middle of a cell gives the same cell - floor rounding, not edge
        // rounding. The cell is 4.5×9 points in the view; the middle of cell
        // (2,1) is x = 2.5, y = 1.5 cells.
        assert_eq!(scene((2.5 * 4.5, 1.5 * 9.0)), Some((2, 1)));
        // The cell and the half come out of the **same** translation, they are not asked separately.
        assert_eq!(
            scene_point((11.0, 13.5)),
            Some(SelectionPoint {
                col: 2,
                row: 1,
                half: CellHalf::Left,
            })
        );
    }

    #[test]
    fn halves_split_the_cell_at_its_middle() {
        // Cell (2,1) is x ∈ [9.0, 13.5), y ∈ [9.0, 18.0) points in the view;
        // its half is cell_w/2 = 4.5 pixels in physical x, i.e. 2.25 points in
        // the view. Left half 9.0-11.25, right half 11.25-13.5.
        assert_eq!(scene_half((9.0, 9.0)), Some(CellHalf::Left));
        assert_eq!(scene_half((11.0, 9.0)), Some(CellHalf::Left));
        assert_eq!(scene_half((11.5, 9.0)), Some(CellHalf::Right));
        assert_eq!(scene_half((13.4, 9.0)), Some(CellHalf::Right));
        // The half does not shift the cell: all four are in cell (2,1).
        for x in [9.0, 11.0, 11.5, 13.4] {
            assert_eq!(scene((x, 9.0)), Some((2, 1)), "x = {x}");
        }
    }

    #[test]
    fn the_exact_middle_belongs_to_the_right_half() {
        // The midpoint is a **written** decision: the halves partition as
        // `[0, w/2)` and `[w/2, w)`, so the exact boundary falls in the right
        // half (9.0 + 2.25 = 11.25 points in the view); a tick to its left is
        // still the left half. The right half leaves the cell outside at the
        // start end and inside at the end end.
        assert_eq!(scene_half((11.25, 9.0)), Some(CellHalf::Right));
        assert_eq!(scene_half((11.25 - 0.25, 9.0)), Some(CellHalf::Left));
    }

    #[test]
    fn reject_keeps_the_report_inside_the_grid() {
        // The scene is 100×33 cells, 9×18 pixels @2x → the view is 450×297 points.
        // `Clamp` sticks what is beyond the edge (the selection's rule),
        // `Reject` rejects it (the rule of the event that **starts** a report):
        // a coordinate coming from the title bar, the left padding or the dock
        // band would report the grid's edge cell to the application and the
        // pointer is not there.
        let reject =
            |view_px| point_to_cell(view_px, grid(0), 0.0, OutOfGrid::Reject, 2.0, 100, 33);
        // Inside: both gates give the same cell.
        assert_eq!(reject((5.0, 9.0)), scene_point((5.0, 9.0)));
        // The last cell's interior is still valid (449.5 points < 450).
        assert!(reject((449.0, 296.0)).is_some());
        // On the top (the title bar side) and left (padding) rejection; `Clamp` sticks.
        assert_eq!(reject((5.0, -1.0)), None);
        assert_eq!(reject((-1.0, 9.0)), None);
        assert_eq!(scene((5.0, -1.0)), Some((1, 0)));
        assert_eq!(scene((-1.0, 9.0)), Some((0, 1)));
        // At the bottom (the dock band) and right rejection; `Clamp` sticks to the last row/column.
        assert_eq!(reject((5.0, 297.0)), None);
        assert_eq!(reject((450.0, 9.0)), None);
        assert_eq!(scene((5.0, 297.0)), Some((1, 32)));
        assert_eq!(scene((450.0, 9.0)), Some((99, 1)));
    }

    #[test]
    fn reject_measures_from_the_origin_like_clamp_does() {
        // The offset pushes the grid down: the blank left above is **outside**
        // the grid, so the event that starts a report is rejected there too. The
        // gate reads the offset from the same place as `Clamp` (`origin_px`),
        // otherwise in a bottom-stuck window the whole upper half would count as valid.
        let origin_px = 100.0;
        let at =
            |view_px, outside| point_to_cell(view_px, grid(0), origin_px, outside, 2.0, 100, 33);
        // 49 points × 2 = 98 pixels < 100: above the origin.
        assert_eq!(at((5.0, 49.0), OutOfGrid::Reject), None);
        // Without fill `Clamp` keeps sticking that area to row 0.
        assert_eq!(
            at((5.0, 49.0), OutOfGrid::Clamp { fill_rows: 0 }).map(|p| p.row),
            Some(0)
        );
        // Right below the origin is valid.
        assert_eq!(at((51.0, 51.0), OutOfGrid::Reject).map(|p| p.row), Some(0));
    }

    #[test]
    fn dragging_left_of_the_grid_clamps_to_the_left_half() {
        // An x left of the grid sticks to the **left** half of cell 0: `as u16`
        // saturates, `%` keeps the dividend's sign (negative remainder < w/2).
        // Had the remainder been turned positive (`rem_euclid`) it would land in
        // the right half and a drag starting at the left edge would leave cell 0
        // outside - the first letter would come out missing when the user
        // wanted to select from the line start.
        //
        // The point is **chosen**: -1 point in the view, -2 pixels @2x; `-2 % 9
        // = -2` (left), `(-2).rem_euclid(9) = 7` (right). The two rules diverge
        // on every `[-(k+½)w, -kw)` interval and give the same half in the
        // rest - -3 points (-6 pixels, remainder 3) falls to the left half in
        // both and would stop this test being a guard.
        assert_eq!(scene((-1.0, 9.0)), Some((0, 1)));
        assert_eq!(scene_half((-1.0, 9.0)), Some(CellHalf::Left));
    }

    #[test]
    fn points_past_the_grid_stick_to_its_edge() {
        // The right and bottom beyond-the-edge is **not swallowed**, it sticks
        // to the last column/row. Since the half now decides the selection,
        // swallowing produced a loss: if the window width is not an exact
        // multiple of the cell an unused strip remains at the grid's right
        // (`split_into_grid` rounds the column count down) and when the mouse
        // dragged toward the line end passed onto it the event would drop and the
        // selection would stay at the last event in the grid. If that event was
        // in the left half of the last column the last letter would be missing
        // from the copy. A point overflowing to the right is the **right** half
        // of the last column: it includes the cell.
        let last = |col, row| {
            Some(SelectionPoint {
                col,
                row,
                half: CellHalf::Right,
            })
        };
        assert_eq!(scene_point((900.0, 100.0)), last(99, 11));
        // The window can be larger than the grid (margin): the view is 500×400
        // but the grid 450×297.
        assert_eq!(scene_point((470.0, 100.0)), last(99, 11));
        // A bottom overflow only clips the row; the column and half come from x.
        assert_eq!(scene((100.0, 600.0)), Some((22, 32)));
        assert_eq!(scene((100.0, 350.0)), Some((22, 32)));
    }

    #[test]
    fn the_gutter_shifts_the_grid_origin() {
        // The scene: 9×18 cell, @2x, **8 physical pixels** of padding. In the
        // view the padding is 4 points, the cell 4.5 points.
        let at = |x: f64| {
            point_to_cell(
                (x, 9.0),
                grid(8),
                0.0,
                OutOfGrid::Clamp { fill_rows: 0 },
                2.0,
                100,
                33,
            )
        };
        let cell = |point: Option<SelectionPoint>| point.map(|p| (p.col, p.half));

        // The **inside** of the padding is clamped to the first column and
        // stays in the left half: the selection does not start in the padding.
        // There is no separate clamping arm - after the subtraction x is
        // negative and `as u16` saturates it to zero, and `%` writes the
        // negative remainder to the left half (the same path as a point left of the grid).
        assert_eq!(
            cell(at(0.0)),
            Some((0, CellHalf::Left)),
            "padding's left end"
        );
        assert_eq!(cell(at(2.0)), Some((0, CellHalf::Left)), "padding's middle");

        // A point left of the padding sticks to the same place: a drag starting
        // at the grid's left must include the first letter in the selection.
        assert_eq!(
            cell(at(-1.0)),
            Some((0, CellHalf::Left)),
            "left of the padding"
        );

        // Where the padding ends is the **start** of column 0: clicking the
        // text's first character gives the first column.
        assert_eq!(cell(at(4.0)), Some((0, CellHalf::Left)), "padding's end");
        assert_eq!(cell(at(8.5)), Some((1, CellHalf::Left)), "one cell later");

        // **Two points that see the shift.** Since the padding is narrower than
        // a cell width most x values fall in the same column with or without
        // the padding and only the half changes; these are the places where the
        // column really moves. The paddingless scene asks the same question and
        // gives a different answer - that is what makes the test discriminating,
        // otherwise it would pass even if the padding were never applied.
        assert_eq!(cell(at(5.0)), Some((0, CellHalf::Left)), "with padding");
        assert_eq!(
            point_to_cell(
                (5.0, 9.0),
                grid(0),
                0.0,
                OutOfGrid::Clamp { fill_rows: 0 },
                2.0,
                100,
                33
            )
            .map(|p| (p.col, p.half)),
            Some((1, CellHalf::Left)),
            "without padding the same point is the next column"
        );

        // Right edge: since the padding pushes the columns right, the grid's
        // right end finishes later by the padding too. In the paddingless scene
        // the same point **overflows** the grid and is clamped to the last
        // column's right half; in the padded scene it is still inside column 99.
        // This is also the proof that the padding comes from the same source as
        // the `cols` computation: had they diverged the last column would
        // either end early or overflow.
        assert_eq!(
            cell(at(451.5)),
            Some((99, CellHalf::Left)),
            "padded right end"
        );
        assert_eq!(
            point_to_cell(
                (451.5, 9.0),
                grid(0),
                0.0,
                OutOfGrid::Clamp { fill_rows: 0 },
                2.0,
                100,
                33
            )
            .map(|p| (p.col, p.half)),
            Some((99, CellHalf::Right)),
            "without padding the same point overflows the grid"
        );
    }

    /// The scene's measure above the origin: 9×18 cell, @2x, **180 physical
    /// pixels** of origin - i.e. a ten-row area, then the content. In the view
    /// the origin is 90 points, the cell 9 points.
    ///
    /// Two tests ask the same scene with two `fill`s: at zero the area is
    /// **blank** and the click is clamped, above zero the area has
    /// **scrollback** and the click is rejected.
    const ORIGIN_PX: f64 = 180.0;

    #[test]
    fn the_origin_shifts_the_grid_down_and_the_blank_area_clamps() {
        // The padding's vertical twin and **the real home of the `u16` trap**:
        // with bottom-sticking the blank area is at the top, so on a click on
        // the window's upper half the difference goes negative. Done in `u16` it
        // would overflow and that click would select the last row - the drag's
        // start would leap to the screen's bottom. In `f64` it stays negative
        // and `as u16` saturates it to zero.
        let at = |y: f64| {
            point_to_cell(
                (0.0, y),
                grid(0),
                ORIGIN_PX,
                OutOfGrid::Clamp { fill_rows: 0 },
                2.0,
                100,
                33,
            )
        };
        let row = |point: Option<SelectionPoint>| point.map(|p| p.row);

        // The whole blank area sticks to row 0: the top edge, its middle and one
        // before where the origin ends. There is no separate clamping arm - and
        // the clamping **cannot be removed**: without fill that area is really
        // blank and a drag starting from above must include the first row in the selection.
        assert_eq!(row(at(0.0)), Some(0), "top edge");
        assert_eq!(row(at(45.0)), Some(0), "middle of the blank area");
        assert_eq!(row(at(89.0)), Some(0), "one before the content");

        // Where the origin ends is the **start** of row 0: clicking the
        // content's first row gives the first row, the next cell the next row.
        assert_eq!(row(at(90.0)), Some(0), "start of the content");
        assert_eq!(row(at(99.0)), Some(1), "one row later");

        // **The point that sees the shift:** the origin-less scene asks the same
        // question and gives a different answer. Without this line the test
        // would pass even if the origin were never applied - the very
        // distinction of the padding's own test.
        assert_eq!(
            row(point_to_cell(
                (0.0, 99.0),
                grid(0),
                0.0,
                OutOfGrid::Clamp { fill_rows: 0 },
                2.0,
                100,
                33
            )),
            Some(11),
            "without the origin the same point is eleven rows lower"
        );

        // A bottom overflow is still clamped to the last row: the origin does
        // not change the bottom edge's rule, it only pushes the start.
        assert_eq!(row(at(600.0)), Some(32), "bottom overflow");

        // **The middle of a slide is a legitimate origin too**: the mouse
        // reads the drawn value and that value does not stop at a row boundary
        // during the slide. Here half a cell (9 physical pixels) is added: the
        // content's first row is now half a cell lower and the old boundary
        // falls one row up. The function has no whole-row assumption - had it
        // had one the symptom would be "a click is a row off while sliding".
        let mid = |y: f64| {
            point_to_cell(
                (0.0, y),
                grid(0),
                ORIGIN_PX + 9.0,
                OutOfGrid::Clamp { fill_rows: 0 },
                2.0,
                100,
                33,
            )
        };
        assert_eq!(
            row(mid(99.0)),
            Some(0),
            "the content's first row in the middle of a slide"
        );
        assert_eq!(row(mid(103.5)), Some(1), "half a cell later, one row down");
    }

    #[test]
    fn a_click_over_the_filled_area_is_rejected_instead_of_clamped() {
        // When the fill arrives the area above the origin is **not blank**: the
        // user sees text there. Had the clamping continued the anchor would
        // land not on the row the eye sees but on the content's top and the
        // highlight would appear somewhere else entirely - what the selection
        // contract ("what the eye sees and what the pasteboard gives do not
        // diverge") forbids by name. Since the filled rows are **not
        // selectable** (they cannot be represented without opening the row
        // numbers to negative) the only right answer is to reject.
        let at = |y: f64| {
            point_to_cell(
                (0.0, y),
                grid(0),
                ORIGIN_PX,
                OutOfGrid::Clamp { fill_rows: 10 },
                2.0,
                100,
                33,
            )
        };
        let row = |point: Option<SelectionPoint>| point.map(|p| p.row);

        // The **same three points** at which the blank area was clamped, this
        // time `None`: the only input separating the two tests is `fill`.
        assert_eq!(at(0.0), None, "top edge");
        assert_eq!(at(45.0), None, "middle of the band");
        assert_eq!(at(89.0), None, "one before the content");

        // The drag stops exactly here: both call sites (`mouseDragged:` and
        // `follow_pointer`) enter with `if let Some`, so on an event that comes
        // as `None` the selection's end stays at its **last valid cell**.

        // The content itself passes untouched - the rejection only for above the origin.
        assert_eq!(row(at(90.0)), Some(0), "start of the content");
        assert_eq!(row(at(99.0)), Some(1), "one row later");
        assert_eq!(
            row(at(600.0)),
            Some(32),
            "bottom overflow is still the last row"
        );

        // **Even if the band does not cover the whole gap** the rejection is for
        // all of above the origin: `fill = min(gap, fresh rows)` and a gap may
        // still remain above. Separating the two regions would need the mouse to
        // turn `fill` into pixels too; the rejection's direction is safe, the
        // clamping's is not.
        let thin = |y: f64| {
            point_to_cell(
                (0.0, y),
                grid(0),
                ORIGIN_PX,
                OutOfGrid::Clamp { fill_rows: 1 },
                2.0,
                100,
                33,
            )
        };
        assert_eq!(thin(0.0), None, "the gap left above the band");
        assert_eq!(thin(89.0), None, "inside the band");
    }

    /// **The full grid is a band higher up**: when the dock grows to
    /// three input rows the drawn origin goes negative (two rows, `-36` px) and
    /// the grid's top is outside the window. The first visible pixel is row 2
    /// and a click must select that - a mapping that ignored the origin would
    /// select row 0, i.e. a row that is not on screen. @1x, paddingless.
    #[test]
    fn a_negative_origin_maps_the_clipped_grid_to_the_visible_row() {
        let press = |y: f64| {
            point_to_cell((20.0, y), grid(0), -36.0, OutOfGrid::Reject, 1.0, 40, 29)
                .map(|point| point.row)
        };
        assert_eq!(press(0.0), Some(2));
        assert_eq!(press(17.0), Some(2));
        assert_eq!(press(18.0), Some(3));
        // The grid's last row is also two rows up: row 28 is 468..486.
        assert_eq!(press(470.0), Some(28));
    }

    /// The dock's input line is where `bt-gpu` draws it: below the bottom-stuck
    /// band's top by the breathing padding. A press yields a point only in that
    /// row - the hairline, the padding and the context line are rejected - while
    /// a drag is clamped into the row. @1x, 9×18 cell, padding 7: the band is
    /// `2·18 + 2·7 + 14 = 64` px, so in a 400 px view the input line is 343..361.
    #[test]
    fn the_dock_input_row_is_where_the_dock_draws_it() {
        let metrics = grid(7);
        let top = dock_input_top_px(400.0, metrics, 2);
        assert_eq!(top, 343.0);
        let press =
            |x: f64, y: f64| point_to_cell((x, y), metrics, top, OutOfGrid::Reject, 1.0, 40, 1);
        // Inside the row: the column by the grid's arithmetic (the left padding is subtracted).
        let point = press(7.0 + 3.0 * 9.0 + 6.0, 350.0).expect("no point on the line");
        assert_eq!((point.col, point.row, point.half), (3, 0, CellHalf::Right));
        // The band's padding, the context line and the grid's area are nothing.
        for y in [337.0, 342.0, 362.0, 390.0, 100.0] {
            assert_eq!(press(20.0, y), None, "y = {y}");
        }
        // The drag is clamped into the row.
        let drag = point_to_cell(
            (1000.0, 390.0),
            metrics,
            top,
            OutOfGrid::Clamp { fill_rows: 0 },
            1.0,
            40,
            1,
        )
        .expect("no clamp");
        assert_eq!((drag.col, drag.row, drag.half), (39, 0, CellHalf::Right));
    }

    #[test]
    fn empty_grid_has_no_cell() {
        // A minimised window can give zero columns/rows: there is no last cell to stick to.
        assert_eq!(
            point_to_cell(
                (1.0, 1.0),
                grid(0),
                0.0,
                OutOfGrid::Clamp { fill_rows: 0 },
                2.0,
                0,
                33
            ),
            None
        );
        assert_eq!(
            point_to_cell(
                (1.0, 1.0),
                grid(0),
                0.0,
                OutOfGrid::Clamp { fill_rows: 0 },
                2.0,
                100,
                0
            ),
            None
        );
    }

    #[test]
    fn command_keys_never_reach_the_terminal() {
        let extras = [
            NSEventModifierFlags::empty(),
            NSEventModifierFlags::Shift,
            NSEventModifierFlags::Option,
            NSEventModifierFlags::Control,
            NSEventModifierFlags::Function,
            NSEventModifierFlags::CapsLock,
        ];
        // A Command letter with no counterpart in the menu (Cmd-T) does not
        // type "t" into the shell; whatever modifier is beside it. The list is
        // **closed**: the criterion became "is it in the allow list", not "is it
        // Command", and an open rule would one day let this key through too.
        for extra in extras {
            assert!(
                !reaches_terminal(NSEventModifierFlags::Command | extra, Some("t")),
                "Command + {extra:?}"
            );
        }
        // A pure modifier key: there is no `characters` and no key whose
        // identity could be asked - swallowed.
        assert!(!reaches_terminal(NSEventModifierFlags::Command, None));
        // **The exceptions**: ⌘⌫ (`\x15`), ⌘← (`\x01`) and ⌘→ (`\x05`); their
        // bytes are in `encode_key`. The modifier beside them is not asked -
        // with CapsLock on it must still delete the line, and ⌘⇧← must go to the line start too.
        for allowed in [BACKSPACE, ARROW_LEFT, ARROW_RIGHT] {
            for extra in extras {
                assert!(
                    reaches_terminal(
                        NSEventModifierFlags::Command | extra,
                        Some(&allowed.to_string())
                    ),
                    "Command + {allowed:?} + {extra:?}"
                );
            }
            // A single-character match: a multi-character `characters` that
            // starts with a key in the list does not enter the list.
            assert!(
                !reaches_terminal(NSEventModifierFlags::Command, Some(&format!("{allowed}x"))),
                "{allowed:?} + x"
            );
        }
        // The list is **closed**: ⌘↑/⌘↓, whose direction is the same, are not in the list, swallowed.
        for swallowed in ['\u{f700}', '\u{f701}'] {
            assert!(
                !reaches_terminal(NSEventModifierFlags::Command, Some(&swallowed.to_string())),
                "{swallowed:?}"
            );
        }
        // A key without Command is the terminal's: a Control letter is a byte,
        // an Option navigation key a Meta sequence, an Option letter a character.
        for flags in extras {
            assert!(reaches_terminal(flags, Some("t")), "{flags:?}");
        }
    }

    /// The smooth arm's step; the unit is 9 points (trackpad), remainder zero.
    fn smooth(delta: f64, phase: NSEventPhase, momentum: NSEventPhase) -> Option<SmoothWheel> {
        smooth_wheel(delta, 9.0, 0.0, phase, momentum).0
    }

    #[test]
    fn a_trackpad_gesture_is_classified_phase_by_phase() {
        // Began → Changed → Ended → momentum Began → Changed → momentum Ended.
        let none = NSEventPhase::None;
        let intent = |delta, phase, momentum| smooth(delta, phase, momentum).map(|s| s.intent);
        assert_eq!(
            intent(0.0, NSEventPhase::MayBegin, none),
            Some(ScrollIntent::GestureBegan),
            "a settling in flight must end when the finger touches"
        );
        assert_eq!(
            intent(2.0, NSEventPhase::Began, none),
            Some(ScrollIntent::GestureBegan)
        );
        assert_eq!(
            intent(3.0, NSEventPhase::Changed, none),
            Some(ScrollIntent::Direct)
        );
        assert_eq!(
            intent(0.0, NSEventPhase::Ended, none),
            Some(ScrollIntent::Settle)
        );
        assert_eq!(
            intent(8.0, none, NSEventPhase::Began),
            Some(ScrollIntent::GestureBegan),
            "the momentum start must end the settling"
        );
        assert_eq!(
            intent(5.0, none, NSEventPhase::Changed),
            Some(ScrollIntent::Direct)
        );
        assert_eq!(
            intent(0.0, none, NSEventPhase::Ended),
            Some(ScrollIntent::Settle)
        );
        // A cancelled gesture settles too: the window must not rest at half a line.
        assert_eq!(
            intent(0.0, NSEventPhase::Cancelled, none),
            Some(ScrollIntent::Settle)
        );
        // A motionless in-between event is not sent.
        assert_eq!(intent(0.0, NSEventPhase::Stationary, none), None);
        assert_eq!(intent(0.0, NSEventPhase::Changed, none), None);
    }

    #[test]
    fn a_trackpad_delta_is_sent_as_a_fraction_of_a_row() {
        // The source of tracking the finger pixel by pixel: the amount is the
        // delta divided by the cell height, not truncated. The whole line is separate, for the arrow/report arm.
        let step = smooth(4.5, NSEventPhase::Changed, NSEventPhase::None).expect("step");
        assert_eq!(step.rows, 0.5);
        assert_eq!(step.lines, 0);
        let (step, rest) = smooth_wheel(-12.0, 9.0, 0.0, NSEventPhase::Changed, NSEventPhase::None);
        let step = step.expect("step");
        assert_eq!((step.rows, step.lines), (-12.0 / 9.0, -1));
        assert_eq!(rest, -12.0 / 9.0 + 1.0);
        // A non-finite amount does not leak into the scroll arm.
        let step = smooth_wheel(9.0, 0.0, 0.0, NSEventPhase::Ended, NSEventPhase::None)
            .0
            .expect("step");
        assert_eq!((step.rows, step.lines), (0.0, 0));
    }

    #[test]
    fn a_notch_glides_in_whole_rows() {
        // A phaseless event is a notch: the amount is **whole lines**, i.e.
        // "off"'s distance, and the fractional part stays in the remainder.
        let none = NSEventPhase::None;
        let (step, rest) = smooth_wheel(2.5, 1.0, 0.0, none, none);
        assert_eq!(
            step,
            Some(SmoothWheel {
                rows: 2.0,
                lines: 2,
                intent: ScrollIntent::Glide
            })
        );
        assert_eq!(rest, 0.5);
        // A notch that does not reach a line sends nothing, the remainder accumulates.
        let (step, rest) = smooth_wheel(0.3, 1.0, 0.5, none, none);
        assert_eq!(step, None);
        assert_eq!(rest, 0.8);
        // A precise but phaseless event is a notch too: there is no phase to say its end.
        let (step, _) = smooth_wheel(18.0, 9.0, 0.0, none, none);
        assert_eq!(
            step.map(|s| (s.rows, s.intent)),
            Some((2.0, ScrollIntent::Glide))
        );
    }

    #[test]
    fn the_line_amount_is_the_off_arms_amount() {
        // The `"off"` arm is `wheel_lines` itself and the smooth arm's `lines`
        // (the arrow and report arms' amount) is from the same function, with
        // the same remainder - vim/less and the wheel in mouse mode send the
        // same line in both modes.
        let phases = [
            (NSEventPhase::None, NSEventPhase::None),
            (NSEventPhase::Began, NSEventPhase::None),
            (NSEventPhase::Changed, NSEventPhase::None),
            (NSEventPhase::Ended, NSEventPhase::None),
            (NSEventPhase::None, NSEventPhase::Changed),
        ];
        for (phase, momentum) in phases {
            for (delta, unit, carry) in [(4.0, 9.0, 6.0), (-27.0, 9.0, 0.0), (2.0, 1.0, 0.25)] {
                let (step, rest) = smooth_wheel(delta, unit, carry, phase, momentum);
                let (lines, off_rest) = wheel_lines(delta, unit, carry);
                assert_eq!(rest, off_rest, "{phase:?}/{momentum:?}");
                if let Some(step) = step {
                    assert_eq!(step.lines, lines, "{phase:?}/{momentum:?}");
                }
            }
        }
    }

    #[test]
    fn wheel_whole_lines_pass_through() {
        // Trackpad: the unit is the cell height (points). One whole cell = one
        // line, the sign is kept - positive is backward, the same direction as `Session::scroll_wheel`.
        assert_eq!(wheel_lines(9.0, 9.0, 0.0), (1, 0.0));
        assert_eq!(wheel_lines(-27.0, 9.0, 0.0), (-3, 0.0));
        // Classic wheel: `scrollingDeltaY` is already lines, the unit is 1.
        assert_eq!(wheel_lines(2.0, 1.0, 0.0), (2, 0.0));
    }

    #[test]
    fn wheel_sub_line_deltas_accumulate() {
        // A trackpad showers deltas smaller than a cell height. Had the
        // remainder not been carried a slow scroll would produce **no** line at
        // all: each event is truncated to zero by itself.
        let (lines, carry) = wheel_lines(4.0, 9.0, 0.0);
        assert_eq!(lines, 0);
        let (lines, carry) = wheel_lines(4.0, 9.0, carry);
        assert_eq!(lines, 0);
        let (lines, carry) = wheel_lines(4.0, 9.0, carry);
        assert_eq!(lines, 1);
        assert!((carry - 3.0 / 9.0).abs() < 1e-9, "{carry}");
        // When the direction reverses the remainder melts first: a third
        // accumulated backward, two thirds of a cell forward → a total of a
        // third forward, no line.
        let (lines, carry) = wheel_lines(-6.0, 9.0, carry);
        assert_eq!(lines, 0);
        assert!((carry + 3.0 / 9.0).abs() < 1e-9, "{carry}");
    }

    #[test]
    fn wheel_degenerate_inputs_do_not_poison_the_carry() {
        // A zero unit (an unmeasured cell) produces infinity, 0/0 NaN; if NaN
        // entered the remainder every later total would be NaN and the wheel would silently die.
        assert_eq!(wheel_lines(9.0, 0.0, 0.0), (0, 0.0));
        assert_eq!(wheel_lines(0.0, 0.0, 0.0), (0, 0.0));
        assert_eq!(wheel_lines(f64::NAN, 9.0, 0.5), (0, 0.0));
        // A giant delta saturates; the clamping is in `bt-core` (to the scrollback's length).
        assert_eq!(wheel_lines(1e300, 1.0, 0.0).0, i32::MAX);
    }

    #[test]
    fn scale_changes_the_cell() {
        // The same view point is two different cells at two scales: the measure
        // comes from physical pixels and if the scale factor is skipped the
        // selection is off by half on a retina machine.
        let at1x = point_to_cell(
            (90.0, 150.0),
            grid(0),
            0.0,
            OutOfGrid::Clamp { fill_rows: 0 },
            1.0,
            100,
            33,
        );
        let at2x = point_to_cell(
            (90.0, 150.0),
            grid(0),
            0.0,
            OutOfGrid::Clamp { fill_rows: 0 },
            2.0,
            100,
            33,
        );
        assert_eq!(
            (at1x.map(|p| (p.col, p.row)), at2x.map(|p| (p.col, p.row))),
            (Some((10, 8)), Some((20, 16)))
        );
    }

    /// The drop's filtering: **only a file URL** yields a path.
    ///
    /// The guard was added **after** the set's gate, because the fix of the
    /// doc↔code contradiction the gate found (the `isFileURL` step) had not
    /// been seen by the gate. What it pins is `NSURL`'s generosity: the class
    /// reads `http://` too and `NSURL.path` answers it with `/foo`, so without
    /// the step a link dragged from a browser would write a root-anchored path
    /// to the input line (the rule: only a file URL).
    ///
    /// The pasteboard is **unique and local**: had `generalPasteboard` been
    /// used the test would have erased what the user copied.
    #[test]
    fn only_file_urls_become_dropped_paths() {
        let _pasteboard = crate::clipboard::tests::pasteboard_lock();
        let board = NSPasteboard::pasteboardWithUniqueName();
        let file = NSURL::fileURLWithPath(ns_string!("/tmp/bir dosya.txt"));
        let web = NSURL::URLWithString(ns_string!("http://example.com/foo")).expect("valid URL");
        board.clearContents();
        let written = board.writeObjects(&NSArray::from_retained_slice(&[
            ProtocolObject::from_retained(file),
            ProtocolObject::from_retained(web),
        ]));
        assert!(written, "the pasteboard accepted both URLs");

        // The web address is dropped, the file path arrives **percent-decoded**:
        // the observable half of the decision not to write percent-decoding a
        // second time (Foundation's own answer).
        assert_eq!(
            dropped_paths(&board),
            vec!["/tmp/bir dosya.txt".to_string()]
        );

        // The pasteboard is unique and process-local: it goes when the test
        // process ends, a manual release (`releaseGlobally`) does not exist in this binding.
        board.clearContents();
    }
}
