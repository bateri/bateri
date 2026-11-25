//! The private framework's functions, looked up when they are needed, and the choice of what plays
//! the vibration from what was found.
//!
//! Nothing is linked: the framework is opened with `dlopen` and every function is looked up by
//! name and is optional, so that a system that renames or removes one is a different choice (the
//! fallback, or nothing), not an application that does not start. The signatures are the ones
//! the program this was taken from calls them with; a function that is found with another
//! signature is the one risk this cannot see, and it is the reason the order of fallbacks is
//! written in [`Backend`] and not hidden in the calls.

use std::ffi::{CStr, c_void};
use std::ptr::NonNull;

/// Where the framework is.
const FRAMEWORK: &CStr =
    c"/System/Library/PrivateFrameworks/MultitouchSupport.framework/MultitouchSupport";

/// The callback the framework calls for each frame of a device (`contact::on_frame` is the one).
pub(super) type FrameCallback = extern "C" fn(*const c_void, *const u8, i32, f64, i32) -> i32;

pub(super) type CreateList = unsafe extern "C" fn() -> *const c_void;
pub(super) type DeviceId = unsafe extern "C" fn(*const c_void, *mut u64) -> i32;
pub(super) type ActuatorCreate = unsafe extern "C" fn(u64) -> *const c_void;
pub(super) type ActuatorOpen = unsafe extern "C" fn(*const c_void) -> i32;
pub(super) type ActuatorClose = unsafe extern "C" fn(*const c_void) -> i32;
pub(super) type ActuationCreate = unsafe extern "C" fn(*const c_void, i32) -> *const c_void;
pub(super) type ActuationPlay =
    unsafe extern "C" fn(*const c_void, *const c_void, u32, f32, f32) -> i32;
pub(super) type Register = unsafe extern "C" fn(*const c_void, FrameCallback);
pub(super) type Start = unsafe extern "C" fn(*const c_void, i32);
pub(super) type Stop = unsafe extern "C" fn(*const c_void);

/// The functions found, each as the framework gives it or not at all.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Symbols {
    pub create_list: Option<CreateList>,
    pub device_id: Option<DeviceId>,
    pub actuator_create: Option<ActuatorCreate>,
    pub actuator_open: Option<ActuatorOpen>,
    pub actuator_close: Option<ActuatorClose>,
    pub actuation_create: Option<ActuationCreate>,
    pub actuation_play: Option<ActuationPlay>,
    pub register: Option<Register>,
    pub unregister: Option<Register>,
    pub start: Option<Start>,
    pub stop: Option<Stop>,
}

/// `name` as a function of type `T`, from whatever `lookup` finds for it.
///
/// # Safety
/// `T` must be a function pointer type, the signature of the function the name is of.
unsafe fn function<T: Copy>(
    lookup: &dyn Fn(&CStr) -> Option<NonNull<c_void>>,
    name: &CStr,
) -> Option<T> {
    debug_assert_eq!(size_of::<T>(), size_of::<NonNull<c_void>>());
    let found = lookup(name)?;
    // SAFETY: a function pointer is as big as a data pointer on every system this runs on (checked
    // above in debug), and the caller says `T` is the type of this function.
    Some(unsafe { std::mem::transmute_copy::<NonNull<c_void>, T>(&found) })
}

impl Symbols {
    /// What `lookup` finds of the framework's functions.
    pub(super) fn resolve(lookup: &dyn Fn(&CStr) -> Option<NonNull<c_void>>) -> Self {
        // SAFETY: each type is the signature of the function it is looked up by (`type` aliases
        // above, from the program the calls were taken from).
        unsafe {
            Self {
                create_list: function(lookup, c"MTDeviceCreateList"),
                device_id: function(lookup, c"MTDeviceGetDeviceID"),
                actuator_create: function(lookup, c"MTActuatorCreateFromDeviceID"),
                actuator_open: function(lookup, c"MTActuatorOpen"),
                actuator_close: function(lookup, c"MTActuatorClose"),
                actuation_create: function(lookup, c"MTActuationCreateFromDictionary"),
                actuation_play: function(lookup, c"MTActuationActuate"),
                register: function(lookup, c"MTRegisterContactFrameCallback"),
                unregister: function(lookup, c"MTUnregisterContactFrameCallback"),
                start: function(lookup, c"MTDeviceStart"),
                stop: function(lookup, c"MTDeviceStop"),
            }
        }
    }

    /// Opens the framework and looks the functions up in it; none where it cannot be opened.
    pub(super) fn load() -> Self {
        // SAFETY: a NUL-terminated path; the handle is kept for the life of the process (the
        // functions are called until it ends), so it is never closed.
        let handle = unsafe { libc::dlopen(FRAMEWORK.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        let Some(handle) = NonNull::new(handle) else {
            return Self::default();
        };
        Self::resolve(&|name| {
            // SAFETY: `handle` is a handle `dlopen` gave and a NUL-terminated name.
            NonNull::new(unsafe { libc::dlsym(handle.as_ptr(), name.as_ptr()) })
        })
    }

    /// What it takes to make the motor play and to tell which device does: to list the devices,
    /// to open one's motor and to play on it.
    pub(super) fn can_actuate(&self) -> bool {
        self.create_list.is_some()
            && self.device_id.is_some()
            && self.actuator_create.is_some()
            && self.actuator_open.is_some()
            && self.actuation_create.is_some()
            && self.actuation_play.is_some()
    }

    /// What it takes to know a finger is on a device: to list them and to be told of their frames.
    pub(super) fn can_feel_fingers(&self) -> bool {
        self.create_list.is_some()
            && self.device_id.is_some()
            && self.register.is_some()
            && self.start.is_some()
    }
}

/// What plays a vibration, from the most it can do to the least.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Backend {
    /// The framework's motor, patterns of any shape and strength, while a finger is on the glass
    /// of the device that plays.
    Motor,
    /// AppKit's three patterns, which the system plays only while a finger is on the glass: the
    /// shape and strength of a pattern are lost, and a held event is not played.
    Fallback,
    /// There is no trackpad that vibrates: nothing plays.
    Nothing,
}

/// What was found, as far as the choice goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Facts {
    /// The framework gives what it takes to play on the motor.
    pub actuate: bool,
    /// The framework gives what it takes to know that a finger is on a device.
    pub fingers: bool,
    /// How many trackpads there are that vibrate: the number whose motor opened where it was tried,
    /// else the number the framework lists, else `None` where it was not asked.
    pub devices: Option<usize>,
}

/// The backend for what was found. The motor needs both halves: without the finger it would play
/// with none on the glass, which is the rule it is there to keep, so a framework that lost one
/// of them is the fallback, which keeps the rule by itself.
pub(super) fn choose(facts: Facts) -> Backend {
    match facts {
        Facts {
            devices: Some(0), ..
        } => Backend::Nothing,
        Facts {
            actuate: true,
            fingers: true,
            devices: Some(_),
        } => Backend::Motor,
        _ => Backend::Fallback,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A table that has every name but the ones in `missing`: the addresses are never called.
    fn table(missing: &[&CStr]) -> Symbols {
        Symbols::resolve(&|name| (!missing.contains(&name)).then(NonNull::<c_void>::dangling))
    }

    #[test]
    fn a_framework_with_every_function_can_both_play_and_feel_a_finger() {
        let symbols = table(&[]);
        assert!(symbols.can_actuate() && symbols.can_feel_fingers());
        assert!(symbols.stop.is_some() && symbols.unregister.is_some());
    }

    #[test]
    fn a_framework_that_does_not_open_has_nothing() {
        let symbols = Symbols::resolve(&|_| None);
        assert!(!symbols.can_actuate() && !symbols.can_feel_fingers());
    }

    #[test]
    fn each_function_the_motor_takes_is_one_the_table_asks_for_by_name() {
        for needed in [
            c"MTDeviceCreateList",
            c"MTDeviceGetDeviceID",
            c"MTActuatorCreateFromDeviceID",
            c"MTActuatorOpen",
            c"MTActuationCreateFromDictionary",
            c"MTActuationActuate",
        ] {
            assert!(!table(&[needed]).can_actuate(), "{needed:?}");
        }
        for needed in [
            c"MTDeviceCreateList",
            c"MTDeviceGetDeviceID",
            c"MTRegisterContactFrameCallback",
            c"MTDeviceStart",
        ] {
            assert!(!table(&[needed]).can_feel_fingers(), "{needed:?}");
        }
        // What is only for tidying up or for a name does not decide.
        for optional in [
            c"MTActuatorClose",
            c"MTUnregisterContactFrameCallback",
            c"MTDeviceStop",
        ] {
            let symbols = table(&[optional]);
            assert!(
                symbols.can_actuate() && symbols.can_feel_fingers(),
                "{optional:?}"
            );
        }
    }

    fn facts(actuate: bool, fingers: bool, devices: Option<usize>) -> Facts {
        Facts {
            actuate,
            fingers,
            devices,
        }
    }

    #[test]
    fn the_order_is_the_motor_then_the_system_then_nothing() {
        // Both halves and a motor that opened.
        assert_eq!(choose(facts(true, true, Some(2))), Backend::Motor);
        // The finger half is gone: the motor would play with none on the glass.
        assert_eq!(choose(facts(true, false, Some(2))), Backend::Fallback);
        // The motor half is gone, the fingers are not enough to play.
        assert_eq!(choose(facts(false, true, Some(1))), Backend::Fallback);
        // The framework is not there at all: AppKit decides for itself.
        assert_eq!(choose(facts(false, false, None)), Backend::Fallback);
        // The framework is there and no trackpad vibrates (a Mac with none, or whose motor did not open).
        assert_eq!(choose(facts(true, true, Some(0))), Backend::Nothing);
        assert_eq!(choose(facts(false, false, Some(0))), Backend::Nothing);
        assert_eq!(choose(facts(true, true, None)), Backend::Fallback);
    }
}
