//! The trackpad's motor, driven through the framework: the devices, their motors, the frames that
//! say where a finger is, and a burst played on the motor of the device the finger is on.

use std::ffi::c_void;
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, ns_string};

use super::contact::{CONTACTS, on_frame};
use super::player::Driver;
use super::steps::Step;
use super::symbols::Symbols;

// SAFETY: CoreFoundation is linked through `objc2-core-foundation`; these are its own C
// functions with the signatures below.
unsafe extern "C" {
    fn CFArrayGetCount(array: *const c_void) -> isize;
    fn CFArrayGetValueAtIndex(array: *const c_void, index: isize) -> *const c_void;
    fn CFRelease(object: *const c_void);
}

/// How long after the devices were started the frames of a finger that is already on the glass are
/// waited for: a device sends none before it is started, and a pattern asked for in that time
/// would find no finger and be dropped, though one rests there. A trackpad sends dozens of frames
/// a second, so this is some frames.
pub(super) const FIRST_FRAMES: Duration = Duration::from_millis(40);

/// What is left of that wait, `started` being when the devices were started.
fn first_frames_left(started: Instant, now: Instant) -> Duration {
    FIRST_FRAMES.saturating_sub(now.saturating_duration_since(started))
}

/// Why there is no motor to drive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Unavailable {
    /// The framework lists no device whose motor opens.
    NoDevice,
    /// The framework gave no list.
    Unlisted,
}

/// A trackpad whose motor is open and whose frames are watched.
struct Device {
    /// Its place in [`CONTACTS`].
    slot: usize,
    /// The framework's reference to it (`MTDeviceRef`), valid while [`Motor::list`] is held.
    device: *const c_void,
    /// Its motor, a reference this owns.
    actuator: *const c_void,
}

/// The devices whose motors are open: the framework's list is kept, as its devices live as long as it.
pub(super) struct Motor {
    symbols: Symbols,
    list: *const c_void,
    devices: Vec<Device>,
    /// When the devices were started.
    started: Instant,
}

// SAFETY: the framework's references are plain handles that its functions take from any thread
// (the program the calls were taken from drives the motor from a thread of its own), and the
// motor is used from the one player thread after it is made.
unsafe impl Send for Motor {}

/// The devices of the framework's list, each as its reference and its id; `None` where the ids
/// cannot be read (there is no function for them, or it fails for every device of a list that
/// is not empty): a list that was not read is not a list of no trackpad, and the system's own
/// patterns are what is left.
fn listed(symbols: &Symbols, list: *const c_void) -> Option<Vec<(*const c_void, u64)>> {
    let device_id = symbols.device_id?;
    // SAFETY: `list` is a `CFArray` the framework gave and is held by the caller.
    let count = unsafe { CFArrayGetCount(list) };
    let read: Vec<_> = (0..count)
        .filter_map(|index| {
            // SAFETY: an index below the count of that array.
            let device = unsafe { CFArrayGetValueAtIndex(list, index) };
            let mut id = 0_u64;
            // SAFETY: a device of the list and a place for its id.
            let done = !device.is_null() && unsafe { device_id(device, &mut id) } == 0;
            done.then_some((device, id))
        })
        .collect();
    (count == 0 || !read.is_empty()).then_some(read)
}

/// How many devices the framework lists, or `None` where it cannot be asked.
pub(super) fn count(symbols: &Symbols) -> Option<usize> {
    let create_list = symbols.create_list?;
    // SAFETY: the framework's own list call, which takes nothing.
    let list = unsafe { create_list() };
    if list.is_null() {
        return None;
    }
    let found = listed(symbols, list).map(|read| read.len());
    // SAFETY: the list was made by a `Create` function and is released once.
    unsafe { CFRelease(list) };
    found
}

impl Motor {
    /// Opens the motor of each device that has one and starts watching it for fingers.
    pub(super) fn open(symbols: Symbols) -> Result<Self, Unavailable> {
        let (Some(create_list), Some(actuator_create), Some(actuator_open)) = (
            symbols.create_list,
            symbols.actuator_create,
            symbols.actuator_open,
        ) else {
            return Err(Unavailable::Unlisted);
        };
        let (Some(register), Some(start)) = (symbols.register, symbols.start) else {
            return Err(Unavailable::Unlisted);
        };
        // SAFETY: the framework's own list call, which takes nothing.
        let list = unsafe { create_list() };
        if list.is_null() {
            return Err(Unavailable::Unlisted);
        }
        let Some(found) = listed(&symbols, list) else {
            // SAFETY: the list was made by a `Create` function and is released once.
            unsafe { CFRelease(list) };
            return Err(Unavailable::Unlisted);
        };
        let mut devices = Vec::new();
        for (device, id) in found {
            // SAFETY: a device id the framework gave; the actuator is a reference this owns.
            let actuator = unsafe { actuator_create(id) };
            if actuator.is_null() {
                continue;
            }
            // SAFETY: the actuator just made.
            if unsafe { actuator_open(actuator) } != 0 {
                // SAFETY: made by a `Create` function and not kept.
                unsafe { CFRelease(actuator) };
                continue;
            }
            let Some(slot) = CONTACTS.register(device as usize) else {
                // SAFETY: opened above, not kept.
                unsafe {
                    if let Some(close) = symbols.actuator_close {
                        close(actuator);
                    }
                    CFRelease(actuator);
                }
                continue;
            };
            // SAFETY: a device of the list, the callback is a function of this crate that reads
            // the frame it is given for the length of the call and writes atomics.
            unsafe {
                register(device, on_frame);
                start(device, 0);
            }
            devices.push(Device {
                slot,
                device,
                actuator,
            });
        }
        if devices.is_empty() {
            // SAFETY: the list was made by a `Create` function and is released once.
            unsafe { CFRelease(list) };
            return Err(Unavailable::NoDevice);
        }
        Ok(Self {
            symbols,
            list,
            devices,
            started: Instant::now(),
        })
    }

    /// How many devices have an open motor.
    pub(super) fn len(&self) -> usize {
        self.devices.len()
    }

    /// Plays `step` on `device`'s motor.
    fn actuate(&self, device: &Device, step: &Step) {
        let (Some(create), Some(play)) =
            (self.symbols.actuation_create, self.symbols.actuation_play)
        else {
            return;
        };
        let shape = shape(step);
        let shape: *const c_void = Retained::as_ptr(&shape).cast();
        // SAFETY: a dictionary of the shape the framework reads (`CFDictionary`, which
        // `NSDictionary` is toll-free bridged to), held for the call.
        let actuation = unsafe { create(shape, 0) };
        if actuation.is_null() {
            return;
        }
        // SAFETY: the actuation just made and the open motor of a device of the list; the
        // actuation is a reference this owns and is released once.
        unsafe {
            play(actuation, device.actuator, 0, step.level, 0.0);
            CFRelease(actuation);
        }
    }
}

/// One burst as the framework's description of a vibration: a silent base wave and a sine of the
/// step's frequency, as long as the step.
fn shape(step: &Step) -> Retained<NSDictionary<NSString, AnyObject>> {
    let milliseconds = NSNumber::new_f64(f64::from(step.ms));
    let silent = NSNumber::new_f64(0.0);
    let whole = NSNumber::new_f64(1.0);
    let kilohertz = NSNumber::new_f64(f64::from(step.hz) / 1000.0);
    let base_keys = [
        ns_string!("Type"),
        ns_string!("Amplitude"),
        ns_string!("DurationMS"),
    ];
    let base_values: [&AnyObject; 3] = [
        ns_string!("Gaussian").as_ref(),
        silent.as_ref(),
        milliseconds.as_ref(),
    ];
    let base = NSDictionary::<NSString, AnyObject>::from_slices(&base_keys, &base_values);
    let tone_keys = [
        ns_string!("Type"),
        ns_string!("Amplitude"),
        ns_string!("DurationMS"),
        ns_string!("DelayMS"),
        ns_string!("FrequencykHz"),
    ];
    let tone_values: [&AnyObject; 5] = [
        ns_string!("Sine").as_ref(),
        whole.as_ref(),
        milliseconds.as_ref(),
        silent.as_ref(),
        kilohertz.as_ref(),
    ];
    let tone = NSDictionary::<NSString, AnyObject>::from_slices(&tone_keys, &tone_values);
    let tones = NSArray::from_retained_slice(&[tone]);
    let keys = [ns_string!("BaseWaveform"), ns_string!("Tones")];
    let values: [&AnyObject; 2] = [base.as_ref(), tones.as_ref()];
    NSDictionary::<NSString, AnyObject>::from_slices(&keys, &values)
}

impl Driver for Motor {
    fn begin(&mut self) -> Option<usize> {
        // Only the first pattern asked for after the devices were started waits, and only where
        // no finger is known to be on the glass.
        loop {
            if let Some(target) = CONTACTS.latest_down() {
                return Some(target);
            }
            let left = first_frames_left(self.started, Instant::now());
            if left.is_zero() {
                return None;
            }
            std::thread::sleep(left.min(Duration::from_millis(4)));
        }
    }

    fn fire(&mut self, target: usize, step: &Step) -> bool {
        if !CONTACTS.is_down(target) {
            return false;
        }
        let Some(device) = self.devices.iter().find(|device| device.slot == target) else {
            return false;
        };
        self.actuate(device, step);
        true
    }
}

impl Drop for Motor {
    fn drop(&mut self) {
        for device in &self.devices {
            // SAFETY: the devices are of the list, which is held until the end of this; each
            // call is the framework's own and each reference is released once.
            unsafe {
                if let Some(unregister) = self.symbols.unregister {
                    unregister(device.device, on_frame);
                }
                if let Some(stop) = self.symbols.stop {
                    stop(device.device);
                }
                if let Some(close) = self.symbols.actuator_close {
                    close(device.actuator);
                }
                CFRelease(device.actuator);
            }
            CONTACTS.release(device.slot);
        }
        // SAFETY: the list was made by a `Create` function and is released once, after its devices.
        unsafe { CFRelease(self.list) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_foundation::NSNumber;

    unsafe extern "C" fn id_of_any(_device: *const c_void, id: *mut u64) -> i32 {
        // SAFETY: the caller gives a place for the id.
        unsafe { *id = 7 };
        0
    }

    unsafe extern "C" fn id_of_none(_device: *const c_void, _id: *mut u64) -> i32 {
        -1
    }

    #[test]
    fn the_wait_for_the_first_frames_is_what_is_left_of_it_after_the_devices_started() {
        let started = Instant::now();
        assert_eq!(first_frames_left(started, started), FIRST_FRAMES);
        let half = started + FIRST_FRAMES / 2;
        assert_eq!(first_frames_left(started, half), FIRST_FRAMES / 2);
        assert!(first_frames_left(started, started + FIRST_FRAMES).is_zero());
        assert!(first_frames_left(started, started + FIRST_FRAMES * 3).is_zero());
        // A clock that reads before the start is the whole of it, not more.
        assert_eq!(first_frames_left(half, started), FIRST_FRAMES);
    }

    /// A list of `count` devices that are only handles the fake functions never look through.
    fn list_of(count: usize) -> Retained<NSArray<NSNumber>> {
        let numbers: Vec<_> = (0..count).map(|n| NSNumber::new_usize(n + 1)).collect();
        NSArray::from_retained_slice(&numbers)
    }

    fn read(device_id: Option<super::super::symbols::DeviceId>, count: usize) -> Option<usize> {
        let symbols = Symbols {
            device_id,
            ..Symbols::default()
        };
        let list = list_of(count);
        listed(&symbols, Retained::as_ptr(&list).cast()).map(|read| read.len())
    }

    #[test]
    fn a_list_whose_ids_cannot_be_read_is_not_a_list_of_no_trackpad() {
        // The ids are read: as many devices as the list has, and none where it has none.
        assert_eq!(read(Some(id_of_any), 2), Some(2));
        assert_eq!(read(Some(id_of_any), 0), Some(0));
        // No function for them, or one that fails for every device of a list that is not empty:
        // the question was not answered, and the system's own patterns are what is left.
        assert_eq!(read(None, 2), None);
        assert_eq!(read(Some(id_of_none), 2), None);
        // A list that is empty has no device whatever the function does.
        assert_eq!(read(Some(id_of_none), 0), Some(0));
    }
}
