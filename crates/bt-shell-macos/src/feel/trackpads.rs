//! When the trackpads there are change: one is plugged in or taken out, one reconnects, the Mac
//! wakes. What was found of them is then no longer what is there, so the service lists them again.
//!
//! A trackpad that is plugged back in is a new device to the framework, and the frames of the old
//! one never come again: what was listed once is a list of the devices of that time. The system
//! says when one comes or goes (IOKit's matching notifications for the class the framework's
//! devices are, which is a public interface), and the Mac that wakes may have changed them in its
//! sleep. A burst of notices (a trackpad that comes brings several, a Bluetooth one comes and goes
//! while it connects) is one look, a second after the last: [`SETTLE`].
//!
//! Nothing here polls, and nothing is listed on each vibration: the notices are what says it is
//! time, and the list is made where the vibration is played ([`Feel::trackpads_changed`]).
//!
//! [`Feel::trackpads_changed`]: super::Feel::trackpads_changed

use std::ffi::{CStr, c_char, c_void};
use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use dispatch2::{DispatchQueue, DispatchTime, MainThreadBound};
use objc2::MainThreadMarker;

/// How long after the last notice the trackpads are looked at again: what comes of a trackpad being
/// plugged in is several notices over a moment, and it is not ready at the first.
pub(super) const SETTLE: Duration = Duration::from_secs(1);

/// The class of the devices the framework lists as trackpads.
const DEVICES: &CStr = c"AppleMultitouchDevice";
/// The notice that a device of the class came, and that one went.
const CAME: &CStr = c"IOServiceFirstMatch";
const WENT: &CStr = c"IOServiceTerminate";

/// Runs the work after the time, on the main thread.
type Later = Box<dyn Fn(Duration, Box<dyn FnOnce() + Send>) + Send + Sync>;
/// What is done when the trackpads are to be listed again.
type Then = Box<dyn Fn() + Send + Sync>;

struct Inner {
    /// The number of the latest notice: the work a notice leaves for later runs only if no notice
    /// came after it.
    latest: AtomicU64,
    /// A look was asked for because the motor refused a burst, and has not been answered by a
    /// burst that played or by a notice of the system's: a motor that refuses after the look is
    /// not looked for again, or a trackpad that cannot play would be listed again for each
    /// vibration.
    failed: AtomicBool,
    later: Later,
    then: Then,
}

/// The way to say that the trackpads changed, from any thread. A clone says it to the same place.
#[derive(Clone)]
pub struct Trackpads {
    inner: Arc<Inner>,
}

impl Trackpads {
    /// A watcher that calls `then`, on the main thread, a moment after the trackpads changed.
    #[must_use]
    pub fn new(mtm: MainThreadMarker, then: impl Fn() + 'static) -> Self {
        let then: MainThreadBound<Box<dyn Fn()>> = MainThreadBound::new(Box::new(then), mtm);
        Self::with(
            Box::new(|wait, work| match DispatchTime::try_from(wait) {
                Ok(when) => {
                    // The error arm is the time not being representable, which a second is not.
                    let _ = DispatchQueue::main().after(when, work);
                }
                Err(_) => DispatchQueue::main().exec_async(work),
            }),
            Box::new(move || then.get_on_main(|then| then())),
        )
    }

    /// A watcher with its own way to run the work later and to do what is asked (the tests').
    fn with(later: Later, then: Then) -> Self {
        Self {
            inner: Arc::new(Inner {
                latest: AtomicU64::new(0),
                failed: AtomicBool::new(false),
                later,
                then,
            }),
        }
    }

    /// The trackpads changed: a device came or went, or the Mac woke.
    pub fn changed(&self) {
        self.inner.failed.store(false, Ordering::Release);
        self.settle();
    }

    /// A motor refused a burst: its device may be gone or not be the one it was. Asks for a look
    /// once, until a burst plays or the system says something changed.
    fn refused(&self) {
        if !self.inner.failed.swap(true, Ordering::AcqRel) {
            self.settle();
        }
    }

    /// A burst played.
    fn played(&self) {
        self.inner.failed.store(false, Ordering::Release);
    }

    /// The look for this notice, after [`SETTLE`], unless another notice came by then.
    fn settle(&self) {
        let ticket = self.inner.latest.fetch_add(1, Ordering::AcqRel) + 1;
        let inner = Arc::clone(&self.inner);
        (self.inner.later)(
            SETTLE,
            Box::new(move || {
                if inner.latest.load(Ordering::Acquire) == ticket {
                    (inner.then)();
                }
            }),
        );
    }

    /// Starts listening for trackpads coming and going; none where the system does not take it.
    /// What it says is what [`Trackpads::changed`] does, and the player thread's refusals
    /// ([`actuation_refused`]) go to the same place.
    #[must_use]
    pub fn watch(&self, _mtm: MainThreadMarker) -> Option<Watch> {
        // SAFETY: the default main port is the null one; the port is kept by the `Watch` that is
        // made of it and destroyed with it.
        let port = unsafe { IONotificationPortCreate(0) };
        if port.is_null() {
            return None;
        }
        // The port's notices are delivered on the main queue, which is valid forever (and is
        // retained by the port).
        let queue = std::ptr::from_ref(DispatchQueue::main())
            .cast_mut()
            .cast::<c_void>();
        // SAFETY: a port just made and the main queue.
        unsafe { IONotificationPortSetDispatchQueue(port, queue) };
        let refcon = Arc::into_raw(Arc::clone(&self.inner));
        let mut watch = Watch {
            port,
            iterators: Vec::new(),
            refcon,
        };
        for kind in [CAME, WENT] {
            let mut iterator = 0;
            // SAFETY: a class name; the call takes the dictionary it is given, so each notice has
            // its own.
            let matching = unsafe { IOServiceMatching(DEVICES.as_ptr()) };
            // SAFETY: the port above, the dictionary just made (taken here), a callback of this
            // file whose `refcon` is the `Arc` the `Watch` keeps until it destroys the port.
            let done = unsafe {
                IOServiceAddMatchingNotification(
                    port,
                    kind.as_ptr(),
                    matching,
                    on_devices,
                    refcon.cast_mut().cast(),
                    &mut iterator,
                )
            };
            if done != 0 {
                continue;
            }
            watch.iterators.push(iterator);
            // What is there now is not news; emptying the iterator is what arms the notice for the
            // next ones.
            drain(iterator);
        }
        if watch.iterators.is_empty() {
            return None;
        }
        let _ = WATCHED.set(self.clone());
        Some(watch)
    }
}

/// The watcher the application started, which the player thread tells when the motor refuses a
/// burst: it has no place to carry its own.
static WATCHED: OnceLock<Trackpads> = OnceLock::new();

/// The motor refused a burst (from the player thread): the trackpads are looked at again once.
pub(super) fn actuation_refused() {
    if let Some(watched) = WATCHED.get() {
        watched.refused();
    }
}

/// The motor played a burst.
pub(super) fn actuation_played() {
    if let Some(watched) = WATCHED.get() {
        watched.played();
    }
}

/// The system's notices of trackpads coming and going, for as long as this is kept.
pub struct Watch {
    /// The notification port (`IONotificationPortRef`).
    port: *mut c_void,
    /// One iterator for each notice that was asked for.
    iterators: Vec<u32>,
    /// The `Arc` the callback is given as its `refcon`, which this owns one count of.
    refcon: *const Inner,
}

impl Drop for Watch {
    fn drop(&mut self) {
        // SAFETY: the port is destroyed first, so that no callback runs after; the iterators are
        // released once each; the `Arc` was made by `into_raw` in `watch` and is given back once.
        unsafe {
            IONotificationPortDestroy(self.port);
            for iterator in &self.iterators {
                IOObjectRelease(*iterator);
            }
            drop(Arc::from_raw(self.refcon));
        }
    }
}

/// The notice: something of the class came or went. Called on the main queue.
extern "C" fn on_devices(refcon: *mut c_void, iterator: u32) {
    drain(iterator);
    if refcon.is_null() {
        return;
    }
    // SAFETY: the `refcon` is the pointer `watch` made of an `Arc`, which is kept until the port
    // that calls this is destroyed; the count this borrows is not given back.
    let inner = ManuallyDrop::new(unsafe { Arc::from_raw(refcon.cast_const().cast::<Inner>()) });
    Trackpads {
        inner: Arc::clone(&inner),
    }
    .changed();
}

/// Takes what the iterator holds and gives it back: the objects are not looked at, and the
/// notice is only armed again once the iterator is empty.
fn drain(iterator: u32) {
    loop {
        // SAFETY: an iterator the system made for the notice.
        let object = unsafe { IOIteratorNext(iterator) };
        if object == 0 {
            return;
        }
        // SAFETY: an object the iterator gave, which this owns one count of.
        unsafe { IOObjectRelease(object) };
    }
}

// SAFETY: IOKit is a public framework of the system, linked by name; these are its own functions
// with the signatures below (`IOKitLib.h`): `io_object_t` and `io_iterator_t` are `mach_port_t`
// (`u32`), the dictionary and the port are pointers, and a `kern_return_t` is 0 where it worked.
#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IONotificationPortCreate(main_port: u32) -> *mut c_void;
    fn IONotificationPortDestroy(port: *mut c_void);
    fn IONotificationPortSetDispatchQueue(port: *mut c_void, queue: *mut c_void);
    fn IOServiceMatching(name: *const c_char) -> *mut c_void;
    fn IOServiceAddMatchingNotification(
        port: *mut c_void,
        kind: *const c_char,
        matching: *mut c_void,
        callback: extern "C" fn(*mut c_void, u32),
        refcon: *mut c_void,
        iterator: *mut u32,
    ) -> i32;
    fn IOIteratorNext(iterator: u32) -> u32;
    fn IOObjectRelease(object: u32) -> i32;
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;

    use super::*;

    /// What waits for its time: the time and the work.
    type Waiting = Vec<(Duration, Box<dyn FnOnce() + Send>)>;

    /// A watcher whose waiting is kept for the test to run, and what it did.
    struct Rig {
        trackpads: Trackpads,
        waiting: Arc<Mutex<Waiting>>,
        looks: Arc<AtomicUsize>,
    }

    impl Rig {
        fn new() -> Self {
            let waiting = Arc::new(Mutex::new(Vec::new()));
            let looks = Arc::new(AtomicUsize::new(0));
            let (kept, counted) = (Arc::clone(&waiting), Arc::clone(&looks));
            let trackpads = Trackpads::with(
                Box::new(move |wait, work| kept.lock().expect("a lock").push((wait, work))),
                Box::new(move || {
                    counted.fetch_add(1, Ordering::SeqCst);
                }),
            );
            Self {
                trackpads,
                waiting,
                looks,
            }
        }

        /// Lets the time pass for everything that waits, in the order it was asked.
        fn pass(&self) {
            let due: Vec<_> = self.waiting.lock().expect("a lock").drain(..).collect();
            for (_, work) in due {
                work();
            }
        }

        fn looks(&self) -> usize {
            self.looks.load(Ordering::SeqCst)
        }
    }

    #[test]
    fn three_notices_inside_the_wait_are_one_look() {
        let rig = Rig::new();
        rig.trackpads.changed();
        rig.trackpads.changed();
        rig.trackpads.changed();
        assert_eq!(rig.waiting.lock().expect("a lock").len(), 3);
        assert!(
            rig.waiting
                .lock()
                .expect("a lock")
                .iter()
                .all(|(wait, _)| *wait == SETTLE)
        );
        rig.pass();
        assert_eq!(rig.looks(), 1);
    }

    #[test]
    fn a_notice_after_the_look_is_a_look_of_its_own() {
        let rig = Rig::new();
        rig.trackpads.changed();
        rig.pass();
        assert_eq!(rig.looks(), 1);
        rig.trackpads.changed();
        assert_eq!(rig.looks(), 1, "not before the wait");
        rig.pass();
        assert_eq!(rig.looks(), 2);
    }

    #[test]
    fn a_motor_that_refuses_is_looked_for_once_until_something_changes() {
        let rig = Rig::new();
        rig.trackpads.refused();
        rig.trackpads.refused();
        rig.pass();
        assert_eq!(rig.looks(), 1, "the second refusal is the same one");
        // The look did not help: the motor refuses again and is not looked for again.
        rig.trackpads.refused();
        rig.pass();
        assert_eq!(rig.looks(), 1);
        // A burst that plays ends it: the next refusal is a new one.
        rig.trackpads.played();
        rig.trackpads.refused();
        rig.pass();
        assert_eq!(rig.looks(), 2);
        // So does the system saying that the trackpads changed.
        rig.trackpads.refused();
        rig.pass();
        assert_eq!(rig.looks(), 2);
        rig.trackpads.changed();
        rig.pass();
        assert_eq!(rig.looks(), 3);
        rig.trackpads.refused();
        rig.pass();
        assert_eq!(rig.looks(), 4);
    }

    #[test]
    fn a_refusal_and_a_notice_that_come_together_are_one_look() {
        let rig = Rig::new();
        rig.trackpads.refused();
        rig.trackpads.changed();
        rig.pass();
        assert_eq!(rig.looks(), 1);
    }
}
