//! A window's strip of tabs behind the C interface: the order and the selected tab, and bateri's
//! rules for them — where a new tab goes, which one comes up when one closes, where Show Next Tab
//! and ⌘1–9 land. The rules are a strip's whatever it looks like: a row of chips in bateri, a
//! sidebar list in another host. Like the engine, it touches no AppKit and may be used from any
//! thread, never from two at once.

use std::ptr::null_mut;

use bt_shell_common::tabs::Tabs;

use crate::guarded;

/// A strip: tabs in order, and the selected one — there is one exactly when there are tabs. Owned
/// ones are the caller's to free; a plan's lent one lives as long as the plan.
#[repr(transparent)]
pub struct BtStrip(Tabs<u64>);

impl BtStrip {
    pub(crate) fn lend(order: &Tabs<u64>) -> *const BtStrip {
        std::ptr::from_ref(order).cast()
    }
}

/// Runs `body` on the strip `strip` points at; `failed` if it is NULL or `body` panics.
///
/// # Safety
/// `strip` is NULL or a live strip.
unsafe fn with_strip<T: Copy>(
    strip: *const BtStrip,
    failed: T,
    body: impl FnOnce(&Tabs<u64>) -> T,
) -> T {
    guarded(failed, || {
        // SAFETY: the caller's promise.
        unsafe { strip.as_ref() }.map_or(failed, |strip| body(&strip.0))
    })
}

/// [`with_strip`] for the calls that change it.
///
/// # Safety
/// `strip` is NULL or an owned strip, not in use elsewhere.
unsafe fn with_strip_mut<T: Copy>(
    strip: *mut BtStrip,
    failed: T,
    body: impl FnOnce(&mut Tabs<u64>) -> T,
) -> T {
    guarded(failed, || {
        // SAFETY: the caller's promise.
        unsafe { strip.as_mut() }.map_or(failed, |strip| body(&mut strip.0))
    })
}

/// Writes `value` where `out` points, if it points anywhere; `true` if there was a value.
///
/// # Safety
/// `out` is NULL or writable.
unsafe fn answer<T>(out: *mut T, value: Option<T>) -> bool {
    let Some(value) = value else {
        return false;
    };
    if !out.is_null() {
        // SAFETY: the caller's promise.
        unsafe { out.write(value) };
    }
    true
}

/// An empty strip: no tabs, nothing selected.
#[unsafe(no_mangle)]
pub extern "C" fn bt_strip_new() -> *mut BtStrip {
    guarded(null_mut(), || {
        Box::into_raw(Box::new(BtStrip(Tabs::empty())))
    })
}

/// A copy of `strip`, the caller's — to keep a lent one past its lender.
///
/// # Safety
/// `strip` is NULL or a live strip — for every `bt_strip_*` query.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_strip_copy(strip: *const BtStrip) -> *mut BtStrip {
    // SAFETY: the caller's promise.
    unsafe {
        with_strip(strip, null_mut(), |strip| {
            Box::into_raw(Box::new(BtStrip(strip.clone())))
        })
    }
}

/// Frees an owned strip. NULL is ignored; a lent one is never freed.
///
/// # Safety
/// `strip` is NULL or an owned strip, not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_strip_free(strip: *mut BtStrip) {
    guarded((), || {
        if !strip.is_null() {
            // SAFETY: the caller's promise.
            drop(unsafe { Box::from_raw(strip) });
        }
    });
}

/// How many tabs the strip holds.
///
/// # Safety
/// As [`bt_strip_copy`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_strip_count(strip: *const BtStrip) -> usize {
    // SAFETY: the caller's promise.
    unsafe { with_strip(strip, 0, Tabs::len) }
}

/// The tab at `index`, left to right (or top to bottom); 0 past the end — check the count.
///
/// # Safety
/// As [`bt_strip_copy`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_strip_tab_at(strip: *const BtStrip, index: usize) -> u64 {
    // SAFETY: the caller's promise.
    unsafe {
        with_strip(strip, 0, |strip| {
            strip.ids().get(index).copied().unwrap_or(0)
        })
    }
}

/// The selected tab: `true` and written to `tab`; `false` for an empty strip.
///
/// # Safety
/// As [`bt_strip_copy`]; `tab` NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_strip_selected(strip: *const BtStrip, tab: *mut u64) -> bool {
    // SAFETY: the caller's promises.
    unsafe { with_strip(strip, false, |strip| answer(tab, strip.selected())) }
}

/// Where `tab` stands: `true` and its place written to `index`; `false` if it is not here.
///
/// # Safety
/// As [`bt_strip_copy`]; `index` NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_strip_index_of(
    strip: *const BtStrip,
    tab: u64,
    index: *mut usize,
) -> bool {
    // SAFETY: the caller's promises.
    unsafe { with_strip(strip, false, |strip| answer(index, strip.index_of(tab))) }
}

/// A new tab `tab` goes right of the selected one and is selected — where New Tab opens it, next
/// to what the user was looking at. A tab already here is selected instead.
///
/// # Safety
/// `strip` is NULL or an owned strip — for every call that changes it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_strip_insert(strip: *mut BtStrip, tab: u64) -> bool {
    // SAFETY: the caller's promise.
    unsafe {
        with_strip_mut(strip, false, |strip| {
            strip.insert(tab);
            true
        })
    }
}

/// Tab `tab` joins at the end, the selection where it was (into an empty strip it is selected):
/// a tab that came with others. `false` if it is here already.
///
/// # Safety
/// As [`bt_strip_insert`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_strip_append(strip: *mut BtStrip, tab: u64) -> bool {
    // SAFETY: the caller's promise.
    unsafe { with_strip_mut(strip, false, |strip| strip.append(tab)) }
}

/// Tab `tab` goes to `index` (past the end is the end) and is selected — a tab let go on the
/// strip; one already here moves there.
///
/// # Safety
/// As [`bt_strip_insert`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_strip_insert_at(strip: *mut BtStrip, tab: u64, index: usize) -> bool {
    // SAFETY: the caller's promise.
    unsafe {
        with_strip_mut(strip, false, |strip| {
            strip.insert_at(tab, index);
            true
        })
    }
}

/// A new tab `tab` goes before the tab at `gap` (the length is the end), the selection where it
/// was — a pane made a tab of its own between others. `false` if it is here already.
///
/// # Safety
/// As [`bt_strip_insert`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_strip_place_new(strip: *mut BtStrip, tab: u64, gap: usize) -> bool {
    // SAFETY: the caller's promise.
    unsafe { with_strip_mut(strip, false, |strip| strip.place_new(tab, gap)) }
}

/// Tab `tab` closes. Closing the selected one selects its right neighbour, or the left one when it
/// was the last — the tab that slides under the pointer that closed it; closing another keeps the
/// selection. `false` if it was not here.
///
/// # Safety
/// As [`bt_strip_insert`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_strip_close(strip: *mut BtStrip, tab: u64) -> bool {
    // SAFETY: the caller's promise.
    unsafe { with_strip_mut(strip, false, |strip| strip.close(tab)) }
}

/// Selects `tab`; `true` if the selection changed.
///
/// # Safety
/// As [`bt_strip_insert`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_strip_select(strip: *mut BtStrip, tab: u64) -> bool {
    // SAFETY: the caller's promise.
    unsafe { with_strip_mut(strip, false, |strip| strip.select(tab)) }
}

/// Moves `tab` to `index` (clamped to the last place), the selection staying on its tab; `true`
/// if the order changed.
///
/// # Safety
/// As [`bt_strip_insert`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_strip_move_to(strip: *mut BtStrip, tab: u64, index: usize) -> bool {
    // SAFETY: the caller's promise.
    unsafe { with_strip_mut(strip, false, |strip| strip.move_to(tab, index)) }
}

/// The tab Show Next Tab (`forward`) or Show Previous Tab selects — the neighbour, wrapping at
/// both ends: `true` and written to `tab`; `false` for an empty strip.
///
/// # Safety
/// As [`bt_strip_copy`]; `tab` NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_strip_adjacent(
    strip: *const BtStrip,
    forward: bool,
    tab: *mut u64,
) -> bool {
    // SAFETY: the caller's promises.
    unsafe { with_strip(strip, false, |strip| answer(tab, strip.adjacent(forward))) }
}

/// The tab ⌘`digit` reaches: 1–8 the nth tab, 9 the last one, whatever the count: `true` and
/// written to `tab`; `false` when there is none.
///
/// # Safety
/// As [`bt_strip_copy`]; `tab` NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_strip_by_shortcut(
    strip: *const BtStrip,
    digit: u8,
    tab: *mut u64,
) -> bool {
    // SAFETY: the caller's promises.
    unsafe { with_strip(strip, false, |strip| answer(tab, strip.by_shortcut(digit))) }
}
