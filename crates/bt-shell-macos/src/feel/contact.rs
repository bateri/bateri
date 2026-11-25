//! Which trackpad has a finger on it, read from the frames the framework delivers.
//!
//! The framework calls [`on_frame`] on a thread of its own for every frame of every device that is
//! started, so everything here is a few atomic stores: no lock, no allocation, no call back into
//! the framework. The player thread reads the same atomics.
//!
//! **The layout of a touch record is the one thing here that is not the framework's contract but
//! this code's reading of it** ([`RECORD_BYTES`], [`STATE_OFFSET`], [`DOWN`]): the structure is
//! private and its layout was taken from a program that runs on current Macs. It is read in one
//! function ([`any_down`]), so that a system that changes it is a change there, and a frame that is
//! not a whole number of records is not read at all (no finger).

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

/// The most trackpads that are watched at once: the built-in one and the ones plugged or paired.
pub(super) const MOST_DEVICES: usize = 8;
/// Bytes of one touch record in a frame.
pub(super) const RECORD_BYTES: usize = 96;
/// Where the state of a touch is in its record, as an `i32`.
pub(super) const STATE_OFFSET: usize = 20;
/// The states a touch is on the glass in: made, touching, breaking (the finger leaves with the
/// next frame). The others are in range above it or gone.
pub(super) const DOWN: std::ops::RangeInclusive<i32> = 3..=5;

/// Whether any touch of `records` (the bytes of a frame) is on the glass.
pub(super) fn any_down(records: &[u8]) -> bool {
    let (whole, _) = records.as_chunks::<RECORD_BYTES>();
    whole.iter().any(|record| {
        record
            .get(STATE_OFFSET..STATE_OFFSET + 4)
            .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
            .is_some_and(|bytes| DOWN.contains(&i32::from_ne_bytes(bytes)))
    })
}

#[derive(Debug)]
struct Slot {
    /// The device's address, or 0 where the slot is free.
    device: AtomicUsize,
    down: AtomicBool,
    /// Which finger came to the glass later: a number that grows with each, so that the clock the
    /// framework stamps its frames with does not matter.
    since: AtomicU64,
}

impl Slot {
    const fn new() -> Self {
        Self {
            device: AtomicUsize::new(0),
            down: AtomicBool::new(false),
            since: AtomicU64::new(0),
        }
    }
}

/// The trackpads watched and whether a finger is on each.
#[derive(Debug)]
pub(super) struct Contacts {
    slots: [Slot; MOST_DEVICES],
    arrivals: AtomicU64,
}

impl Contacts {
    pub(super) const fn new() -> Self {
        Self {
            slots: [const { Slot::new() }; MOST_DEVICES],
            arrivals: AtomicU64::new(0),
        }
    }

    /// Watches `device` (an address, never 0): the slot it has, or `None` where all are taken.
    pub(super) fn register(&self, device: usize) -> Option<usize> {
        if device == 0 {
            return None;
        }
        self.slots.iter().position(|slot| {
            slot.device
                .compare_exchange(0, device, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        })
    }

    /// Stops watching the device in `slot`.
    pub(super) fn release(&self, slot: usize) {
        if let Some(slot) = self.slots.get(slot) {
            slot.down.store(false, Ordering::Release);
            slot.device.store(0, Ordering::Release);
        }
    }

    /// A frame of `device`: whether a finger is on it now. A device that is not watched is passed
    /// over.
    pub(super) fn touch(&self, device: usize, down: bool) {
        let Some(slot) = self
            .slots
            .iter()
            .find(|slot| slot.device.load(Ordering::Acquire) == device)
        else {
            return;
        };
        if down && !slot.down.load(Ordering::Acquire) {
            let order = self.arrivals.fetch_add(1, Ordering::AcqRel) + 1;
            slot.since.store(order, Ordering::Release);
        }
        slot.down.store(down, Ordering::Release);
    }

    /// Whether a finger is on the device in `slot`.
    pub(super) fn is_down(&self, slot: usize) -> bool {
        self.slots
            .get(slot)
            .is_some_and(|slot| slot.down.load(Ordering::Acquire))
    }

    /// The slot whose finger came last, among those with one on the glass.
    pub(super) fn latest_down(&self) -> Option<usize> {
        self.slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.down.load(Ordering::Acquire))
            .max_by_key(|(_, slot)| slot.since.load(Ordering::Acquire))
            .map(|(index, _)| index)
    }
}

/// The table the framework's callback writes: one for the process, as the callback has no place
/// to carry its own.
pub(super) static CONTACTS: Contacts = Contacts::new();

/// The callback given to the framework for each device it watches: `data` is `count` records of
/// [`RECORD_BYTES`], valid for the call.
pub(super) extern "C" fn on_frame(
    device: *const c_void,
    data: *const u8,
    count: i32,
    _timestamp: f64,
    _frame: i32,
) -> i32 {
    let records = match usize::try_from(count) {
        // SAFETY: the framework hands `count` records of `RECORD_BYTES` bytes at `data` for the
        // length of the call and nothing is kept; a count or a pointer that is none reads none.
        Ok(count) if count > 0 && !data.is_null() && count <= MAX_RECORDS => unsafe {
            std::slice::from_raw_parts(data, count * RECORD_BYTES)
        },
        _ => &[],
    };
    CONTACTS.touch(device as usize, any_down(records));
    0
}

/// More touches than hands have fingers: a count past it is not a frame.
const MAX_RECORDS: usize = 32;

#[cfg(test)]
mod tests {
    use super::*;

    fn record(state: i32) -> [u8; RECORD_BYTES] {
        let mut bytes = [0_u8; RECORD_BYTES];
        bytes[STATE_OFFSET..STATE_OFFSET + 4].copy_from_slice(&state.to_ne_bytes());
        bytes
    }

    #[test]
    fn a_touch_on_the_glass_is_down_and_one_in_range_or_gone_is_not() {
        for state in [3, 4, 5] {
            assert!(any_down(&record(state)), "state {state}");
        }
        for state in [0, 1, 2, 6, 7, -1] {
            assert!(!any_down(&record(state)), "state {state}");
        }
    }

    #[test]
    fn one_finger_among_the_touches_of_a_frame_is_enough() {
        let mut frame = Vec::new();
        frame.extend_from_slice(&record(2));
        frame.extend_from_slice(&record(7));
        assert!(!any_down(&frame));
        frame.extend_from_slice(&record(4));
        assert!(any_down(&frame));
        assert!(!any_down(&[]));
    }

    #[test]
    fn a_frame_that_is_not_whole_records_is_read_no_further() {
        let mut frame = record(4).to_vec();
        frame.truncate(RECORD_BYTES - 1);
        assert!(!any_down(&frame));
    }

    #[test]
    fn the_finger_that_came_last_is_the_one_the_vibration_goes_to() {
        let contacts = Contacts::new();
        let built_in = contacts.register(0x10).expect("a slot");
        let external = contacts.register(0x20).expect("a slot");
        assert_eq!(contacts.latest_down(), None);

        contacts.touch(0x10, true);
        assert_eq!(contacts.latest_down(), Some(built_in));
        contacts.touch(0x20, true);
        assert_eq!(contacts.latest_down(), Some(external));
        // The frames that follow of a finger that stays do not make it the later one.
        contacts.touch(0x10, true);
        assert_eq!(contacts.latest_down(), Some(external));
        contacts.touch(0x20, false);
        assert_eq!(contacts.latest_down(), Some(built_in));
        assert!(contacts.is_down(built_in) && !contacts.is_down(external));
        contacts.touch(0x10, false);
        assert_eq!(contacts.latest_down(), None);
    }

    #[test]
    fn a_device_that_is_not_watched_is_passed_over_and_a_slot_is_given_back() {
        let contacts = Contacts::new();
        contacts.touch(0x99, true);
        assert_eq!(contacts.latest_down(), None);
        assert_eq!(contacts.register(0), None);
        let slots: Vec<usize> = (1..=MOST_DEVICES)
            .map(|device| contacts.register(device).expect("a slot"))
            .collect();
        assert_eq!(slots.len(), MOST_DEVICES);
        assert_eq!(contacts.register(0x1000), None, "all slots are taken");
        contacts.touch(3, true);
        contacts.release(slots[2]);
        assert!(!contacts.is_down(slots[2]), "a released slot has no finger");
        assert_eq!(contacts.register(0x1000), Some(slots[2]));
    }
}
