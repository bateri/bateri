//! The real [`GlobalKeys`]: the system's hold on the hotkey window's key. Two ways, one at a time:
//!
//! - **A chord** is Carbon's `RegisterEventHotKey`. It asks for **no permission** (the event is
//!   the system's own, delivered to the application that registered it), and it takes the key away
//!   from every other application while it is held.
//! - **One modifier tapped twice** is a **listen-only** event tap (`CGEventTapCreate` with
//!   `kCGEventTapOptionListenOnly`), which the system builds only for an application that was
//!   allowed to listen (Input Monitoring). It sees the modifier changes and the presses that spoil
//!   a count — a key, a click, a scroll — and passes them to the count the whole application
//!   uses ([`bt_shell_common::hotkey::DoubleTap`] over `arrange::taps()`), with the thresholds
//!   of the live trigger and of Settings' recorder. It changes no event and reads no key's
//!   meaning. The permission is **asked about** here ([`CGPreflightListenEventAccess`], which asks
//!   no one) and **asked for** only when `AppDelegate::allow_listening` says: the Settings page,
//!   when a double tap is recorded or its button pressed — a launch that asked would put a
//!   question on every run of every build. The system disables a tap that is slow or that the
//!   person's input suspended; it is enabled again at once.
//!
//! This takes keys from the person's Mac, which is why a test, the gate, the smoke and the
//! agent's copy never build it ([`bt_shell_common::hotkey::owns_global_keys`], and
//! `AppDelegate::apply_global_keys` is the one place that does).
//!
//! **The key code** is looked for in the keyboard layout the person has now (`UCKeyTranslate`, so
//! the key that types the character the shortcut was recorded with is the one that is held), and
//! on the US layout when that layout has no such character (a Cyrillic or Arabic one); a named
//! key has its code in [`bt_shell_common::hotkey::named_key_code`].

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::ptr::NonNull;

use bt_core::Key;
use bt_shell_common::hotkey::{
    Access, Binding, DoubleTap, GlobalKeys, Refusal, carbon_modifiers, named_key_code,
};
use bt_shell_common::keys::us_code_of;
use dispatch2::DispatchQueue;
use objc2::MainThreadMarker;
use objc2_app_kit::NSEventModifierFlags;
use objc2_core_foundation::{
    CFMachPort, CFRetained, CFRunLoop, CFRunLoopSource, kCFRunLoopCommonModes,
};
use objc2_core_graphics::{
    CGEvent, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventTapProxy,
    CGEventType, CGPreflightListenEventAccess, CGRequestListenEventAccess,
};
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

/// What the tap's callback keeps: the count, and the port to enable again when the system
/// disables it. Boxed, so its address (the callback's `user_info`) holds still.
struct TapState {
    count: RefCell<DoubleTap>,
    /// The tap's port, set once it exists (the callback runs only after).
    port: Cell<*const CFMachPort>,
}

/// The listening tap while it is held: its port, the source that brings its events to the main
/// run loop, and the state its callback reads.
struct Tap {
    port: CFRetained<CFMachPort>,
    source: CFRetained<CFRunLoopSource>,
    // Kept for the callback's pointer: dropped after the port is invalidated.
    _state: Box<TapState>,
}

impl Tap {
    /// The events the tap hears: the modifiers' changes, and what spoils a count of them (a key,
    /// a click, a scroll).
    fn mask() -> u64 {
        [
            CGEventType::FlagsChanged,
            CGEventType::KeyDown,
            CGEventType::LeftMouseDown,
            CGEventType::RightMouseDown,
            CGEventType::OtherMouseDown,
            CGEventType::ScrollWheel,
        ]
        .into_iter()
        .fold(0, |mask, kind| mask | (1u64 << kind.0))
    }

    /// Builds the tap for `count` and starts it, or `None` when the system will not (the
    /// permission is not there).
    fn start(count: DoubleTap) -> Option<Tap> {
        let state = Box::new(TapState {
            count: RefCell::new(count),
            port: Cell::new(std::ptr::null()),
        });
        let user = std::ptr::from_ref::<TapState>(&state)
            .cast_mut()
            .cast::<c_void>();
        // SAFETY: the callback is `tapped` below, which implements the callback's contract; `user`
        // is the state's address, which lives until the tap is stopped (`Drop`: the port is
        // invalidated before the state goes).
        let port = unsafe {
            CGEvent::tap_create(
                CGEventTapLocation::SessionEventTap,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::ListenOnly,
                Self::mask(),
                Some(tapped),
                user,
            )
        }?;
        state.port.set(CFRetained::as_ptr(&port).as_ptr());
        let source = CFMachPort::new_run_loop_source(None, Some(&port), 0)?;
        // The main thread's loop: the callback runs where the application's windows are.
        CFRunLoop::main()?.add_source(Some(&source), unsafe { kCFRunLoopCommonModes });
        CGEvent::tap_enable(&port, true);
        Some(Tap {
            port,
            source,
            _state: state,
        })
    }
}

impl Drop for Tap {
    fn drop(&mut self) {
        CGEvent::tap_enable(&self.port, false);
        if let Some(main) = CFRunLoop::main() {
            main.remove_source(Some(&self.source), unsafe { kCFRunLoopCommonModes });
        }
        self.port.invalidate();
    }
}

/// The listening tap's callback: what the event is to the count. Returns the event as it came
/// (a listen-only tap's return is ignored). **Nothing here may unwind or take long**: the system
/// disables a tap that is slow, so the application's own work is told to the main queue.
unsafe extern "C-unwind" fn tapped(
    _proxy: CGEventTapProxy,
    kind: CGEventType,
    event: NonNull<CGEvent>,
    user: *mut c_void,
) -> *mut CGEvent {
    // SAFETY: `user` is the `TapState` this tap was made with (`Tap::start`), alive while the
    // tap is; the event is valid for the call.
    let (state, event_ref) = unsafe { (&*user.cast::<TapState>(), event.as_ref()) };
    if kind == CGEventType::TapDisabledByTimeout || kind == CGEventType::TapDisabledByUserInput {
        // The system switched it off: switched on again.
        // SAFETY: the port is set before the first event and lives with the tap.
        if let Some(port) = unsafe { state.port.get().as_ref() } {
            CGEvent::tap_enable(port, true);
        }
    } else if let Ok(mut count) = state.count.try_borrow_mut() {
        let hit = if kind == CGEventType::FlagsChanged {
            let flags = NSEventModifierFlags(CGEvent::flags(Some(event_ref)).0 as usize);
            // The events' own clock: seconds since boot, as the live trigger's `NSEvent.timestamp`.
            let at = NSProcessInfo::processInfo().systemUptime();
            count.flags(at, arrange::held_keys(flags))
        } else {
            count.interrupt();
            false
        };
        if hit {
            DispatchQueue::main().exec_async(|| {
                // audit: a block running on the main queue is on the main thread by definition.
                let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                if let Some(app) = app::delegate(mtm) {
                    app.hotkey_pressed();
                }
            });
        }
    }
    event.as_ptr()
}

/// The system's hold on the key, one at a time. The handler is installed with the first chord and
/// stays; the key is let go and held again at each change.
pub(crate) struct SystemKeys {
    handler: Ref,
    hot_key: Ref,
    tap: Option<Tap>,
}

impl SystemKeys {
    pub(crate) fn new() -> Self {
        Self {
            handler: std::ptr::null_mut(),
            hot_key: std::ptr::null_mut(),
            tap: None,
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
        // What was held is let go before the new one is held: the tap with the chord.
        self.tap = None;
        let Some(binding) = binding else {
            return Ok(());
        };
        let chord = match binding {
            Binding::Chord(chord) => chord,
            Binding::Twice(modifier) => {
                // Without the permission the system would ask the person to give it: only the
                // Settings page may cause that, so nothing is built until it is there.
                if self.listen_access() != Access::Granted {
                    return Err(Refusal::NoListening);
                }
                let count = DoubleTap::new(modifier, arrange::taps());
                self.tap = Some(Tap::start(count).ok_or(Refusal::NoListening)?);
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

    fn listen_access(&self) -> Access {
        // Asks the system, not the person.
        if CGPreflightListenEventAccess() {
            Access::Granted
        } else {
            Access::NotGranted
        }
    }

    fn ask_listen_access(&mut self) {
        // The system's own question, once: it remembers the answer.
        CGRequestListenEventAccess();
    }
}

impl Drop for SystemKeys {
    fn drop(&mut self) {
        self.release();
        self.tap = None;
        if !self.handler.is_null() {
            // SAFETY: the reference came from `InstallEventHandler` and is removed once.
            unsafe { RemoveEventHandler(self.handler) };
        }
    }
}

#[cfg(test)]
mod tests {
    use objc2_core_graphics::CGEventFlags;

    use super::*;

    /// The tap's flags are read as the application's own modifier flags (`arrange::held_keys`,
    /// the one reading of them): the bits are the same ones, so a count made of the tap's events is
    /// the count the live trigger makes. Constants only — **no test builds a tap**.
    #[test]
    fn the_tap_flags_are_the_applications_modifier_flags() {
        for (cg, ns) in [
            (CGEventFlags::MaskShift, NSEventModifierFlags::Shift),
            (CGEventFlags::MaskControl, NSEventModifierFlags::Control),
            (CGEventFlags::MaskAlternate, NSEventModifierFlags::Option),
            (CGEventFlags::MaskCommand, NSEventModifierFlags::Command),
            (
                CGEventFlags::MaskSecondaryFn,
                NSEventModifierFlags::Function,
            ),
        ] {
            assert_eq!(cg.0, ns.0 as u64, "{cg:?}");
        }
        // Caps lock is no modifier of a shortcut: ⌥ with it on is ⌥ alone.
        let flags = NSEventModifierFlags(
            (CGEventFlags::MaskAlternate | CGEventFlags::MaskAlphaShift).0 as usize,
        );
        let held = arrange::held_keys(flags);
        assert_eq!(
            held.mods,
            bt_core::Mods::NONE.with(bt_core::Modifier::Option)
        );
        assert!(!held.function);
    }

    /// The tap hears the modifiers and what spoils their count, and nothing else: no key's
    /// meaning is asked of an event.
    #[test]
    fn the_tap_hears_the_modifiers_and_what_spoils_their_count() {
        let mask = Tap::mask();
        for kind in [
            CGEventType::FlagsChanged,
            CGEventType::KeyDown,
            CGEventType::LeftMouseDown,
            CGEventType::RightMouseDown,
            CGEventType::OtherMouseDown,
            CGEventType::ScrollWheel,
        ] {
            assert_ne!(mask & (1u64 << kind.0), 0, "{kind:?}");
        }
        assert_eq!(mask.count_ones(), 6);
    }
}
