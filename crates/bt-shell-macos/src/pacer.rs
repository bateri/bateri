//! The macOS [`Pacer`]: the vsync rhythm `bt-gpu`'s frame loop runs on (040
//! phase-5, Karar 7's (b) path).
//!
//! Four jobs, all AppKit/GCD, none in `bt-gpu`:
//!
//! 1. **The tick** — `NSView.displayLink(target:selector:)` (macOS 14, the
//!    product's floor) as a **timer only**: it follows the view's display and
//!    calls [`Ticker::tick`] with its `targetTimestamp`. It hands over no
//!    drawable — the frame takes its texture from the wgpu surface, and only
//!    when it draws.
//! 2. **`set_running` from any thread** — the link's `paused` bit behind a
//!    `MainThreadBound`; a start is always queued on the main queue and
//!    coalesced while one is pending (the reader thread wakes thousands of
//!    times a second under flowing output, and each dispatch would wake the
//!    main thread for the same idempotent unpause).
//! 3. **The delayed wakeup** — `dispatch2`'s `after` on the main queue.
//! 4. **The time base** — `CACurrentMediaTime`, the base of
//!    `targetTimestamp`.
//!
//! The file moves to `bt-shell-macos` as it is when the shell is split (Karar
//! 7); Linux's pacer comes with the winit set.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use bt_gpu::{Pacer, TickTarget, Ticker};
use dispatch2::{DispatchQueue, DispatchTime, MainThreadBound};
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::NSView;
use objc2_foundation::{NSObject, NSObjectProtocol, NSRunLoop, NSRunLoopCommonModes};
use objc2_quartz_core::{CACurrentMediaTime, CADisplayLink};

define_class!(
    // SAFETY: NSObject subclassing has no requirements; `TickTarget` does not
    // implement `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTickTarget"]
    #[ivars = RefCell<Option<Ticker>>]
    struct TickReceiver;

    unsafe impl NSObjectProtocol for TickReceiver {}

    impl TickReceiver {
        /// The display link's callback; on the main run loop, so on the main
        /// thread — the thread the `DisplayLink` was built on, which the
        /// `Ticker` (not `Send`) requires.
        #[unsafe(method(tick:))]
        fn tick(&self, link: &CADisplayLink) {
            // Cloned out of the slot before ticking: the tick may run for a
            // while and nothing should hold this borrow meanwhile.
            let ticker = self.ivars().borrow().clone();
            if let Some(ticker) = ticker {
                ticker.tick(link.targetTimestamp(), TickTarget::Surface);
            }
        }
    }
);

impl TickReceiver {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(RefCell::new(None));
        // SAFETY: NSObject's `init` takes no arguments and the ivars are set.
        unsafe { msg_send![super(this), init] }
    }
}

/// The macOS pacer; `bt-gpu` holds it as `Arc<dyn Pacer>`.
pub(crate) struct MacPacer {
    inner: Arc<Inner>,
}

struct Inner {
    /// `Retained<CADisplayLink>` is not `Send`; `MainThreadBound` ties access
    /// to a `MainThreadMarker`, which makes carrying it across threads safe —
    /// and writes in the type who may touch it.
    ///
    /// **Its drop blocks off the main thread:** it hops to the main queue
    /// with `exec_sync`. The `Waker` holding this pacer is also held by the
    /// reader thread, so the last reference must not drop there while the
    /// main thread waits on shutdown; `bt-shell-macos` detaches that copy on close
    /// (`TerminalPane::begin_close`) — `bt_gpu::Waker`'s doc.
    link: MainThreadBound<Retained<CADisplayLink>>,
    /// The link's target; the ticker is put in it once the `DisplayLink`
    /// exists ([`MacPacer::attach`]). The link retains it; kept here only to
    /// reach its slot.
    target: MainThreadBound<Retained<TickReceiver>>,
    /// A start is queued on the main queue and not yet run: further starts
    /// are dropped. Lossless — the damage flag is planted before the start
    /// is asked for, and the queued start opens a tick that reads it.
    pending: AtomicBool,
    /// Permanent stop latch ([`Pacer::stop`]): a start queued before the stop
    /// lands on an invalidated link and must not unpause it.
    stopped: AtomicBool,
}

impl MacPacer {
    /// Builds the link for `view` — paused, on the main run loop in the
    /// common modes (live resizing puts the run loop in the tracking mode, and
    /// a link added in the default mode would be silent there).
    pub(crate) fn new(mtm: MainThreadMarker, view: &NSView) -> Arc<Self> {
        let target = TickReceiver::new(mtm);
        // SAFETY: the target is a `TickReceiver`, which implements `tick:`
        // with the `(CADisplayLink)` signature the display link calls.
        let link = unsafe { view.displayLinkWithTarget_selector(&target, sel!(tick:)) };
        // Paused before it is added: a fresh link fires as soon as it is on a
        // run loop, and the frame loop is born paused.
        link.setPaused(true);
        // SAFETY: added to the main run loop from the main thread (`mtm`).
        unsafe { link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes) };
        Arc::new(Self {
            inner: Arc::new(Inner {
                link: MainThreadBound::new(link, mtm),
                target: MainThreadBound::new(target, mtm),
                pending: AtomicBool::new(false),
                stopped: AtomicBool::new(false),
            }),
        })
    }

    /// Hands the pacer the frame loop to tick (`DisplayLink::ticker`).
    pub(crate) fn attach(&self, mtm: MainThreadMarker, ticker: Ticker) {
        *self.inner.target.get(mtm).ivars().borrow_mut() = Some(ticker);
    }
}

impl Inner {
    fn pause(&self, mtm: MainThreadMarker) {
        if !self.stopped.load(Ordering::Acquire) {
            self.link.get(mtm).setPaused(true);
        }
    }
}

impl Pacer for MacPacer {
    fn set_running(&self, running: bool) {
        if !running {
            // Immediate on the main thread (the tick's own pause), queued
            // otherwise.
            match MainThreadMarker::new() {
                Some(mtm) => self.inner.pause(mtm),
                None => {
                    let inner = Arc::clone(&self.inner);
                    DispatchQueue::main().exec_async(move || {
                        // audit: a block on the main queue runs on the main
                        // thread by definition.
                        let mtm = MainThreadMarker::new().expect("main queue is the main thread");
                        inner.pause(mtm);
                    });
                }
            }
            return;
        }
        // A start is **always** queued, even from the main thread: the
        // `Pacer` contract says a start lands after the current tick, so a
        // tick that pauses on its way out cannot swallow it.
        if self.inner.stopped.load(Ordering::Acquire) {
            return;
        }
        if self.inner.pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let inner = Arc::clone(&self.inner);
        DispatchQueue::main().exec_async(move || {
            // audit: a block on the main queue runs on the main thread by
            // definition.
            let mtm = MainThreadMarker::new().expect("main queue is the main thread");
            inner.pending.store(false, Ordering::Release);
            // The latch is read **here** too: this block may have been queued
            // before `stop`, and unpausing an invalidated link is a state the
            // rest of the code does not rely on.
            if inner.stopped.load(Ordering::Acquire) {
                return;
            }
            inner.link.get(mtm).setPaused(false);
        });
    }

    fn after(&self, delay: Duration, wake: Box<dyn FnOnce() + Send>) {
        // A delay that cannot be represented arms nothing — not a panic path:
        // the window wakes on the next damage anyway, and the clock is armed
        // again at the next sleep point.
        let Ok(when) = DispatchTime::try_from(delay) else {
            return;
        };
        // The error arm is not represented today (`dispatch2` returns `Ok`
        // unconditionally) but the signature is fallible; a dropped tick only
        // stops the counter or the blink until the next damage frame.
        let _ = DispatchQueue::main().after(when, wake);
    }

    fn now(&self) -> f64 {
        CACurrentMediaTime()
    }

    fn stop(&self) {
        if self.inner.stopped.swap(true, Ordering::AcqRel) {
            return;
        }
        let teardown = |inner: &Inner, mtm: MainThreadMarker| {
            let link = inner.link.get(mtm);
            link.setPaused(true);
            // Off the run loop and releases its target: without it the run
            // loop would keep the link (and the target) alive for good.
            link.invalidate();
        };
        match MainThreadMarker::new() {
            Some(mtm) => teardown(&self.inner, mtm),
            None => {
                let inner = Arc::clone(&self.inner);
                DispatchQueue::main().exec_async(move || {
                    // audit: a block on the main queue runs on the main
                    // thread by definition.
                    let mtm = MainThreadMarker::new().expect("main queue is the main thread");
                    teardown(&inner, mtm);
                });
            }
        }
    }
}
