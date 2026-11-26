//! The real [`GlobalKeys`]: the system's hold on the hotkey window's key, Carbon's
//! `RegisterEventHotKey`. It asks for **no permission** (the event is the system's own, delivered
//! to the application that registered it), and it takes the key away from every other application
//! while it is held — which is why a test, the gate, the smoke and the agent's copy never build
//! this ([`bt_shell_common::hotkey::owns_global_keys`], and `AppDelegate::apply_global_keys` is
//! the one place that does).
//!
//! **A chord only here.** One modifier tapped twice is a listen-only event tap, which asks for
//! Input Monitoring; it is not this file's yet and [`CarbonKeys::bind`] answers it with
//! [`Refusal::NoListening`].
//!
//! **The key code** is looked for in the keyboard layout the person has now (`UCKeyTranslate`, so
//! the key that types the character the shortcut was recorded with is the one that is held), and
//! on the US layout when that layout has no such character (a Cyrillic or Arabic one); a named
//! key has its code in [`bt_shell_common::hotkey::named_key_code`].

use std::ffi::c_void;

use bt_core::Key;
use bt_shell_common::hotkey::{
    Access, Binding, GlobalKeys, Refusal, carbon_modifiers, named_key_code,
};
use bt_shell_common::keys::us_code_of;
use dispatch2::DispatchQueue;
use objc2::MainThreadMarker;

use crate::app;

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

/// The system's hold on the key, one at a time. The handler is installed with the first chord and
/// stays; the key is let go and held again at each change.
pub(crate) struct CarbonKeys {
    handler: Ref,
    hot_key: Ref,
}

impl CarbonKeys {
    pub(crate) fn new() -> Self {
        Self {
            handler: std::ptr::null_mut(),
            hot_key: std::ptr::null_mut(),
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

impl GlobalKeys for CarbonKeys {
    fn bind(&mut self, binding: Option<Binding>) -> Result<(), Refusal> {
        self.release();
        let Some(binding) = binding else {
            return Ok(());
        };
        let chord = match binding {
            Binding::Chord(chord) => chord,
            // The listen-only tap is the next piece of this file.
            Binding::Twice(_) => return Err(Refusal::NoListening),
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
        // A chord listens to nothing; the tap's permission is the next piece's.
        Access::NotGranted
    }

    fn ask_listen_access(&mut self) {}
}

impl Drop for CarbonKeys {
    fn drop(&mut self) {
        self.release();
        if !self.handler.is_null() {
            // SAFETY: the reference came from `InstallEventHandler` and is removed once.
            unsafe { RemoveEventHandler(self.handler) };
        }
    }
}
