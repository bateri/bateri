//! The real [`GlobalKeys`]: the system's hold on the hotkey window's key. Two ways, one at a time:
//!
//! - **A chord** is Carbon's `RegisterEventHotKey`. It asks for **no permission** (the event is
//!   the system's own, delivered to the application that registered it), and it takes the key away
//!   from every other application while it is held.
//! - **One modifier tapped twice** is heard by the system's **event monitors**, and asks for **no
//!   permission**. Measured on macOS 26.4 only, by a process with no Input Monitoring and no
//!   Accessibility: the oldest system bateri runs on (14) is not measured, and Apple's
//!   documentation says a global monitor hears key events only with the Accessibility
//!   permission, which a modifier change is not. A global monitor
//!   (`addGlobalMonitorForEventsMatchingMask:`, the other
//!   applications') and a local one (bateri's own windows') both get the modifier changes, and
//!   feed the count the whole application uses ([`bt_shell_common::hotkey::DoubleTap`] over
//!   `arrange::taps()`), with the thresholds of the live trigger and of Settings' recorder. A
//!   global monitor never hears an event bateri itself is sent, so the two never count one press
//!   twice (and a press told twice would change nothing: the count compares the modifiers held).
//!   What spoils a count — a key, a click, a scroll — a monitor does **not** hear without the
//!   permission, so the system is **asked** at each modifier change how long ago the last of them
//!   was (`CGEventSourceSecondsSinceLastEventType`, which needs no permission either) and the
//!   count takes it from there ([`bt_shell_common::hotkey::spoils_count`]). No key's meaning is
//!   read, no event changed. A system that gives no global monitor says so
//!   ([`Refusal::Refused`]); one that gives it and never delivers an event cannot be told from
//!   code, only by trying the shortcut.
//!
//! This takes keys from the person's Mac, which is why a test, the gate, the smoke and the
//! agent's copy never build it ([`bt_shell_common::hotkey::owns_global_keys`], and
//! `AppDelegate::apply_global_keys` is the one place that does).
//!
//! **The key code** is looked for in the keyboard layout the person has now (`UCKeyTranslate`, so
//! the key that types the character the shortcut was recorded with is the one that is held), and
//! on the US layout when that layout has no such character (a Cyrillic or Arabic one); a named
//! key has its code in [`bt_shell_common::hotkey::named_key_code`].

use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::rc::Rc;

use block2::RcBlock;
use bt_core::Key;
use bt_shell_common::hotkey::{
    Binding, DoubleTap, GlobalKeys, Refusal, carbon_modifiers, input_age_at_event, named_key_code,
};
use bt_shell_common::keys::us_code_of;
use bt_shell_common::shortcuts::Held;
use dispatch2::DispatchQueue;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSEvent, NSEventMask};
use objc2_core_graphics::{CGEventSource, CGEventSourceStateID, CGEventType};
use objc2_foundation::NSProcessInfo;

use crate::{app, arrange};

type OsStatus = i32;
type Ref = *mut c_void;

/// `EventTypeSpec`.
#[repr(C)]
struct EventTypeSpec {
    class: u32,
    kind: u32,
}

/// `EventHotKeyID`.
#[repr(C)]
#[derive(Clone, Copy)]
struct EventHotKeyId {
    signature: u32,
    id: u32,
}

/// `'keyb'`: the keyboard's event class.
const EVENT_CLASS_KEYBOARD: u32 = 0x6B65_7962;
/// `kEventHotKeyPressed`.
const EVENT_HOT_KEY_PRESSED: u32 = 5;
/// `'bthk'`: the signature of bateri's hot key.
const SIGNATURE: u32 = 0x6274_686B;
/// `paramErr`: what the key that has no code is refused with.
const PARAM_ERR: OsStatus = -50;
/// `kUCKeyTranslateNoDeadKeysBit` as a mask: a dead key is not waited on.
const NO_DEAD_KEYS: u32 = 1;
/// The codes of the keys that type characters (letters, digits, punctuation), the keypad left out
/// so that a digit is the main row's.
const TYPING_CODES: std::ops::RangeInclusive<u16> = 0..=50;

// SAFETY: Carbon is a public framework of the system, linked by name; these are its own functions
// with the signatures below (`CarbonEvents.h`, `TextInputSources.h`, `UnicodeUtilities.h`):
// opaque references are pointers, an `OSStatus` is an `i32` and is 0 where it worked.
#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn GetApplicationEventTarget() -> Ref;
    fn InstallEventHandler(
        target: Ref,
        handler: extern "C" fn(Ref, Ref, Ref) -> OsStatus,
        count: u32,
        list: *const EventTypeSpec,
        user: Ref,
        out: *mut Ref,
    ) -> OsStatus;
    fn RemoveEventHandler(handler: Ref) -> OsStatus;
    fn RegisterEventHotKey(
        code: u32,
        modifiers: u32,
        id: EventHotKeyId,
        target: Ref,
        options: u32,
        out: *mut Ref,
    ) -> OsStatus;
    fn UnregisterEventHotKey(hot_key: Ref) -> OsStatus;
    fn TISCopyCurrentKeyboardLayoutInputSource() -> Ref;
    fn TISGetInputSourceProperty(source: Ref, key: Ref) -> Ref;
    static kTISPropertyUnicodeKeyLayoutData: Ref;
    fn LMGetKbdType() -> u8;
    fn UCKeyTranslate(
        layout: *const c_void,
        code: u16,
        action: u16,
        modifier_state: u32,
        keyboard_type: u32,
        options: u32,
        dead_state: *mut u32,
        capacity: usize,
        length: *mut usize,
        text: *mut u16,
    ) -> OsStatus;
}

// SAFETY: CoreFoundation is linked through `objc2-core-foundation`; these are its own C functions
// with the signatures below.
unsafe extern "C" {
    fn CFDataGetBytePtr(data: *const c_void) -> *const u8;
    fn CFRelease(object: *const c_void);
}

/// The hot key was pressed: told to the application a turn later, from the main queue, so that
/// the window it shows is not shown from inside Carbon's dispatch.
extern "C" fn pressed(_call: Ref, _event: Ref, _user: Ref) -> OsStatus {
    DispatchQueue::main().exec_async(|| {
        // audit: a block running on the main queue is on the main thread by definition.
        let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
        if let Some(app) = app::delegate(mtm) {
            app.hotkey_pressed();
        }
    });
    0
}

/// The code of the key that types `character` on the layout in use, if that layout has one.
fn layout_code(character: char) -> Option<u16> {
    let wanted = u16::try_from(u32::from(character)).ok()?;
    // SAFETY: the calls are Carbon's, with a source this function owns and releases; the layout
    // data is read while the source lives and not kept; `UCKeyTranslate` writes at most
    // `capacity` units into `text` and says how many.
    unsafe {
        let source = TISCopyCurrentKeyboardLayoutInputSource();
        if source.is_null() {
            return None;
        }
        let data = TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData);
        let found = if data.is_null() {
            None
        } else {
            let layout = CFDataGetBytePtr(data);
            let keyboard = u32::from(LMGetKbdType());
            TYPING_CODES.into_iter().find(|&code| {
                let mut dead = 0u32;
                let mut length = 0usize;
                let mut text = [0u16; 4];
                let status = UCKeyTranslate(
                    layout.cast(),
                    code,
                    0,
                    0,
                    keyboard,
                    NO_DEAD_KEYS,
                    &mut dead,
                    text.len(),
                    &mut length,
                    text.as_mut_ptr(),
                );
                status == 0 && length == 1 && text[0] == wanted
            })
        };
        CFRelease(source);
        found
    }
}

/// The virtual key code of `key`: a named key's from the table, a character's from the layout in
/// use and from the US layout when that has none.
fn code_of(key: Key) -> Option<u16> {
    match key {
        Key::Named(named) => named_key_code(named),
        Key::Char(character) => layout_code(character).or_else(|| us_code_of(character)),
    }
}

/// What spoils a count of modifier taps: a key, a click, a scroll. A monitor does not hear them
/// without the permission; the system tells how long ago the last was.
const SPOILERS: [CGEventType; 5] = [
    CGEventType::KeyDown,
    CGEventType::LeftMouseDown,
    CGEventType::RightMouseDown,
    CGEventType::OtherMouseDown,
    CGEventType::ScrollWheel,
];

/// How long ago the last key, click and scroll was, in seconds, each told by itself as the
/// system tells it (any application's: the combined session's). Needs no permission.
fn last_input_ages() -> [f64; SPOILERS.len()] {
    SPOILERS.map(|kind| {
        CGEventSource::seconds_since_last_event_type(
            CGEventSourceStateID::CombinedSessionState,
            kind,
        )
    })
}

/// What a modifier change is to the count: whether it was the second quick tap. `at` is the
/// event's own time, in seconds since boot, as the live trigger's `NSEvent.timestamp` says it
/// (and not the time this runs: a main thread that was busy would run two taps one after the
/// other and the count would see them quick), `now` the time the system was asked, and
/// `ages` its answers ([`last_input_ages`]). Told twice, a change is no second tap: the count
/// compares the modifiers held. A count that is being changed already (never, on one thread) says
/// no.
fn counted(count: &RefCell<DoubleTap>, at: f64, now: f64, ages: &[f64], held: Held) -> bool {
    count
        .try_borrow_mut()
        .is_ok_and(|mut count| count.flags(at, held, input_age_at_event(ages, now - at)))
}

/// A modifier change heard by a monitor: told to the count, and the main queue is told when it
/// was the double tap (the application's own work does not run inside an event's delivery).
fn heard(count: &RefCell<DoubleTap>, event: &NSEvent) {
    let at = event.timestamp();
    let now = NSProcessInfo::processInfo().systemUptime();
    let held = crate::arrange::held_keys(event.modifierFlags());
    if counted(count, at, now, &last_input_ages(), held) {
        DispatchQueue::main().exec_async(|| {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(app) = app::delegate(mtm) {
                app.hotkey_pressed();
            }
        });
    }
}

/// The monitors while a double tap is held: the other applications' and bateri's own. Both are
/// removed when this goes.
struct Monitors {
    global: Option<Retained<AnyObject>>,
    local: Option<Retained<AnyObject>>,
}

impl Monitors {
    /// Starts the monitors of `count`; `None` when the system gives no monitor (none is left
    /// behind). No permission is asked for: a monitor of modifier changes is given to any
    /// application. The monitors call back on the main thread, where the marker says this runs.
    fn start(_mtm: MainThreadMarker, count: DoubleTap) -> Option<Monitors> {
        let count = Rc::new(RefCell::new(count));
        let others = {
            let count = Rc::clone(&count);
            RcBlock::new(move |event: NonNull<NSEvent>| {
                // SAFETY: AppKit gives the monitor a valid event.
                heard(&count, unsafe { event.as_ref() });
            })
        };
        let own = {
            let count = Rc::clone(&count);
            RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
                // SAFETY: AppKit gives the monitor a valid event.
                heard(&count, unsafe { event.as_ref() });
                // The event goes on as it came: this only listens.
                event.as_ptr()
            })
        };
        // SAFETY: the local block returns the valid event it was given.
        let (global, local) = unsafe {
            (
                NSEvent::addGlobalMonitorForEventsMatchingMask_handler(
                    NSEventMask::FlagsChanged,
                    &others,
                ),
                NSEvent::addLocalMonitorForEventsMatchingMask_handler(
                    NSEventMask::FlagsChanged,
                    &own,
                ),
            )
        };
        // One without the other would hear half the taps: both or none (a monitor that was made
        // goes with the value dropped here).
        let (global, local) = (global?, local?);
        Some(Monitors {
            global: Some(global),
            local: Some(local),
        })
    }
}

impl Drop for Monitors {
    fn drop(&mut self) {
        for monitor in [self.global.take(), self.local.take()]
            .into_iter()
            .flatten()
        {
            // SAFETY: the object is the monitor an `add…Monitor…` call returned, removed once.
            unsafe { NSEvent::removeMonitor(&monitor) };
        }
    }
}

/// The system's hold on the key, one at a time. The handler is installed with the first chord and
/// stays; the key is let go and held again at each change.
pub(crate) struct SystemKeys {
    /// Where this is built and used: the monitors call back on the main thread.
    mtm: MainThreadMarker,
    handler: Ref,
    hot_key: Ref,
    monitors: Option<Monitors>,
}

impl SystemKeys {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        Self {
            mtm,
            handler: std::ptr::null_mut(),
            hot_key: std::ptr::null_mut(),
            monitors: None,
        }
    }

    /// Lets go of the key held, if one is.
    fn release(&mut self) {
        if !self.hot_key.is_null() {
            // SAFETY: the reference came from `RegisterEventHotKey` and is released once.
            unsafe { UnregisterEventHotKey(self.hot_key) };
            self.hot_key = std::ptr::null_mut();
        }
    }

    /// Installs the handler of the hot key's event, once.
    fn install(&mut self) -> Result<(), Refusal> {
        if !self.handler.is_null() {
            return Ok(());
        }
        let spec = EventTypeSpec {
            class: EVENT_CLASS_KEYBOARD,
            kind: EVENT_HOT_KEY_PRESSED,
        };
        // SAFETY: the target is the application's own, `spec` lives through the call, the
        // handler is a plain function with the callback's signature and takes no user data.
        let status = unsafe {
            InstallEventHandler(
                GetApplicationEventTarget(),
                pressed,
                1,
                &spec,
                std::ptr::null_mut(),
                &mut self.handler,
            )
        };
        if status == 0 {
            Ok(())
        } else {
            self.handler = std::ptr::null_mut();
            Err(Refusal::Refused(status))
        }
    }
}

impl GlobalKeys for SystemKeys {
    fn bind(&mut self, binding: Option<Binding>) -> Result<(), Refusal> {
        self.release();
        // What was held is let go before the new one is held: the monitors with the chord.
        self.monitors = None;
        let Some(binding) = binding else {
            return Ok(());
        };
        let chord = match binding {
            Binding::Chord(chord) => chord,
            Binding::Twice(modifier) => {
                let count = DoubleTap::new(modifier, arrange::taps());
                // The system gave no monitor: the shortcut is not heard, and the row says so.
                self.monitors =
                    Some(Monitors::start(self.mtm, count).ok_or(Refusal::Refused(PARAM_ERR))?);
                return Ok(());
            }
        };
        let code = code_of(chord.key).ok_or(Refusal::Refused(PARAM_ERR))?;
        self.install()?;
        let id = EventHotKeyId {
            signature: SIGNATURE,
            id: 1,
        };
        // SAFETY: the target is the application's own and `out` is a place for one reference.
        let status = unsafe {
            RegisterEventHotKey(
                u32::from(code),
                carbon_modifiers(chord.mods),
                id,
                GetApplicationEventTarget(),
                0,
                &mut self.hot_key,
            )
        };
        if status == 0 {
            Ok(())
        } else {
            self.hot_key = std::ptr::null_mut();
            Err(Refusal::Refused(status))
        }
    }
}

impl Drop for SystemKeys {
    fn drop(&mut self) {
        self.release();
        self.monitors = None;
        if !self.handler.is_null() {
            // SAFETY: the reference came from `InstallEventHandler` and is removed once.
            unsafe { RemoveEventHandler(self.handler) };
        }
    }
}

#[cfg(test)]
mod tests {
    use bt_core::{Modifier, Mods};
    use bt_shell_common::shortcuts::Held;
    use objc2_app_kit::NSEventModifierFlags;

    use super::*;

    /// The count a monitor feeds, with the application's own thresholds.
    fn count() -> RefCell<DoubleTap> {
        RefCell::new(DoubleTap::new(Modifier::Option, arrange::taps()))
    }

    fn option() -> Held {
        arrange::held_keys(NSEventModifierFlags::Option)
    }

    /// What a double tap of ⌥ is to the monitors: every change is told as the system tells it
    /// (`at` the event's time, asked at `now = at`), and `since_input` is how long ago the last
    /// key, click or scroll was at each. The hits.
    fn tapped(count: &RefCell<DoubleTap>, events: &[(f64, bool, f64)]) -> usize {
        events
            .iter()
            .filter(|&&(at, down, since_input)| {
                let held = if down { option() } else { Held::default() };
                counted(count, at, at, &[since_input], held)
            })
            .count()
    }

    /// A double tap of ⌥ with nothing typed or clicked is one hit; ⌥e ⌥e is none; ⌥ held with a
    /// key typed in the middle (or a click, or a scroll — the system answers for all of them
    /// alike) is none. The thresholds are the application's: the second tap comes within the
    /// double-click interval of the first.
    #[test]
    fn a_double_tap_is_a_hit_and_a_key_between_is_none() {
        let never = f64::INFINITY;
        let clean = [
            (1.00, true, never),
            (1.07, false, never),
            (1.20, true, never),
            (1.27, false, never),
        ];
        assert_eq!(tapped(&count(), &clean), 1);
        // ⌥F18 ⌥F18, the measured case: a key 70 ms before each release.
        let typed = [
            (1.00, true, never),
            (1.07, false, 0.07 - 0.0),
            (1.20, true, never),
            (1.27, false, 0.07),
        ];
        assert_eq!(tapped(&count(), &typed), 0);
        // A click, or a scroll, while the second ⌥ is held.
        let clicked = [
            (1.00, true, never),
            (1.07, false, never),
            (1.20, true, never),
            (1.27, false, 0.04),
        ];
        assert_eq!(tapped(&count(), &clicked), 0);
        // Typing before the first ⌥ is not between the taps.
        let before = [
            (1.00, true, 0.30),
            (1.07, false, 0.37),
            (1.20, true, 0.50),
            (1.27, false, 0.57),
        ];
        assert_eq!(tapped(&count(), &before), 1);
    }

    /// A change told twice — the global monitor and the local one, were both to hear it — is one
    /// change: the count compares the modifiers held, and counts the second tap once.
    #[test]
    fn a_change_told_by_both_monitors_counts_once() {
        let never = f64::INFINITY;
        let count = count();
        let told_twice = [
            (1.00, true, never),
            (1.00, true, never),
            (1.07, false, never),
            (1.07, false, never),
            (1.20, true, never),
            (1.20, true, never),
            (1.27, false, never),
            (1.27, false, never),
        ];
        assert_eq!(tapped(&count, &told_twice), 1);
    }

    /// The system is asked a moment after the event, and the count is told as of the event: an
    /// input that came in that moment is no input before the event.
    #[test]
    fn the_age_of_the_last_input_is_told_as_of_the_event() {
        let count = count();
        // Taps at 1.00-1.07 and 1.20-1.27; the monitors ran 0.2 s late each time, and a key was
        // typed at 1.30 (after the double tap, before the system was asked).
        let hit = |at: f64, down: bool| {
            let held = if down { option() } else { Held::default() };
            let now = at + 0.2;
            counted(&count, at, now, &[now - 1.30], held)
        };
        assert!(!hit(1.00, true));
        assert!(!hit(1.07, false));
        assert!(!hit(1.20, true));
        assert!(hit(1.27, false), "a key after the event does not spoil it");
    }

    /// The system's answer for the keys, clicks and scrolls needs no permission to read: the
    /// five kinds are the ones a count is spoiled by, and the answer is a time (never negative).
    #[test]
    fn the_system_tells_how_long_ago_the_last_input_was() {
        assert_eq!(
            SPOILERS,
            [
                CGEventType::KeyDown,
                CGEventType::LeftMouseDown,
                CGEventType::RightMouseDown,
                CGEventType::OtherMouseDown,
                CGEventType::ScrollWheel,
            ]
        );
        assert!(last_input_ages().iter().all(|age| *age >= 0.0));
    }

    /// The modifier changes a monitor hears are read as the application's own modifier flags
    /// (`arrange::held_keys`, the one reading of them): a count made of the monitors' events is
    /// the count the live trigger makes. Caps lock is no modifier of a shortcut: ⌥ with it on is
    /// ⌥ alone.
    #[test]
    fn the_monitors_flags_are_the_applications_modifier_flags() {
        let held =
            arrange::held_keys(NSEventModifierFlags::Option | NSEventModifierFlags::CapsLock);
        assert_eq!(held.mods, Mods::NONE.with(Modifier::Option));
        assert!(!held.function);
    }
}
