//! Updates: Sparkle 2's standard updater (`SPUStandardUpdaterController`) and
//! its "Check for Updates…" item.
//!
//! **The framework is not linked, it is loaded at run time.** Sparkle is in
//! the bundle's `Contents/Frameworks/` only in the bundle that `make bundle`
//! builds; `cargo run`, `make smoke` and the tests do not see it. Had it been
//! linked, all of those paths would fail in dyld, or each would have to
//! download the framework. Loading goes through `NSBundle`, by class name
//! (`AnyClass::get`, `None` when missing — `class!` panics): without the
//! framework the updater is never born and the menu item is never added.
//!
//! All behavior belongs to Sparkle and `Info.plist` (`SUFeedURL`,
//! `SUPublicEDKey`, `SUEnableAutomaticChecks`): a background check once a day,
//! a prompt on a new version, no silent install. There are no settings here —
//! Sparkle's own preferences live in `NSUserDefaults` and its documentation
//! asks not to build a second layer on top of them.
//!
//! **The delegate** ([`UpdaterDelegate`]) tells one thing: the
//! coming quit is Sparkle's relaunch (`updaterWillRelaunchApplication:`), so
//! the running programs are handed over instead of hung up
//! ([`take_relaunch`]). An aborted install (`updater:didAbortWithError:`)
//! clears it, and the quit that reads it consumes it: a ⌘Q after a failed
//! install is today's quit. While a transfer streams or a password sheet is
//! open it postpones the relaunch until they end
//! (`updater:shouldPostponeRelaunchForUpdate:untilInvokingBlock:`,
//! [`crate::app::AppDelegate::postpone_update`]).
//!
//! It is never started in a timed run (`BT_RUN_SECONDS`): the hermetic run
//! does not go to the network and an update prompt must not cover the window.
//! The decision is the caller's (`app`).

use std::sync::atomic::{AtomicBool, Ordering};

use block2::DynBlock;
use objc2::MainThreadMarker;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject, Bool, NSObject, NSObjectProtocol};
use objc2::{AllocAnyThread, define_class, msg_send};
use objc2_foundation::{NSBundle, NSString};

/// The framework's name in the bundle; its path is under `Contents/Frameworks/`.
const FRAMEWORK: &str = "Sparkle.framework";

/// "The coming quit is a relaunch": set by Sparkle's delegate (or the
/// handover test item, `AppDelegate`), cleared by an aborted install,
/// consumed by the quit that reads it. Process-wide and atomic: Sparkle's
/// callbacks are not promised a thread.
static RELAUNCH: AtomicBool = AtomicBool::new(false);

/// Marks the coming quit as a relaunch.
pub(crate) fn request_relaunch() {
    RELAUNCH.store(true, Ordering::SeqCst);
}

/// Whether the coming quit is a relaunch — **consumed**: asked once per quit.
pub(crate) fn take_relaunch() -> bool {
    RELAUNCH.swap(false, Ordering::SeqCst)
}

define_class!(
    // SAFETY: NSObject has no subclassing requirement; no `Drop`.
    #[unsafe(super(NSObject))]
    #[name = "BateriUpdaterDelegate"]
    /// Sparkle's `SPUUpdaterDelegate` (an informal match: Sparkle asks
    /// `respondsToSelector:` per method, the protocol itself is the
    /// framework's and not linked here). Stateless; the flag is
    /// [`RELAUNCH`].
    pub(crate) struct UpdaterDelegate;

    unsafe impl NSObjectProtocol for UpdaterDelegate {}

    impl UpdaterDelegate {
        /// Sparkle is about to quit bateri to relaunch the new version.
        #[unsafe(method(updaterWillRelaunchApplication:))]
        fn will_relaunch(&self, _updater: &AnyObject) {
            request_relaunch();
        }

        /// The update ended without installing (a refused administrator
        /// password, a failed install): the next quit is not a relaunch, and
        /// a postponed relaunch is not waited for any more.
        #[unsafe(method(updater:didAbortWithError:))]
        fn did_abort(&self, _updater: &AnyObject, _error: &AnyObject) {
            RELAUNCH.store(false, Ordering::SeqCst);
            if let Some(mtm) = MainThreadMarker::new()
                && let Some(app) = crate::app::delegate(mtm)
            {
                app.drop_postponed_update();
            }
        }

        /// Sparkle is about to quit for the install:
        /// while a transfer streams or a password sheet is open — bytes and
        /// answers that pass through bateri and cannot be handed over — the
        /// relaunch waits; `install` is called when the last one ends (⌘.
        /// cancels and so moves it on). Off the main thread (not promised by
        /// Sparkle): no wait, the safe direction is today's.
        #[unsafe(method(updater:shouldPostponeRelaunchForUpdate:untilInvokingBlock:))]
        fn should_postpone(
            &self,
            _updater: &AnyObject,
            _item: &AnyObject,
            install: &DynBlock<dyn Fn()>,
        ) -> Bool {
            let postponed = MainThreadMarker::new()
                .and_then(crate::app::delegate)
                .is_some_and(|app| app.postpone_update(install.copy()));
            Bool::new(postponed)
        }
    }
);

impl UpdaterDelegate {
    fn new() -> Retained<Self> {
        // SAFETY: NSObject's `init` takes no arguments.
        unsafe { msg_send![Self::alloc(), init] }
    }
}

/// Sparkle's updater and its delegate — both kept for the life of the
/// process: the menu item holds the updater weakly and Sparkle holds the
/// delegate weakly.
pub(crate) struct Updater {
    pub(crate) controller: Retained<AnyObject>,
    _delegate: Retained<UpdaterDelegate>,
}

/// Loads the bundle's Sparkle and builds the updater **starting it**
/// (`initWithStartingUpdater:YES`). `None` when the framework is missing,
/// cannot be loaded or the class is not found — a bateri without updates is
/// still a complete terminal, so no branch is fatal.
///
/// The returned object must be kept for the life of the process: it is the
/// menu item's target and `NSMenuItem` holds its target weakly.
pub(crate) fn start() -> Option<Updater> {
    let frameworks = NSBundle::mainBundle().privateFrameworksPath()?;
    let path = NSString::from_str(&format!("{frameworks}/{FRAMEWORK}"));
    let sparkle = NSBundle::bundleWithPath(&path)?;
    // SAFETY: `load` binds the framework's code into the process; the copy in
    // the bundle is signed with our identity (the hardened runtime's library
    // validation would reject another identity) and its only side effect is
    // class registration.
    if !unsafe { sparkle.load() } {
        eprintln!("bateri: {FRAMEWORK} could not be loaded; updates are off");
        return None;
    }
    let class = AnyClass::get(c"SPUStandardUpdaterController")?;
    let none: Option<&AnyObject> = None;
    let delegate = UpdaterDelegate::new();
    // SAFETY: Sparkle 2's documented initializer:
    // `-initWithStartingUpdater:(BOOL) updaterDelegate:(id) userDriverDelegate:(id)`,
    // both delegates are nullable and the updater's is our
    // `UpdaterDelegate` (an `NSObject` answering two of the protocol's
    // optional methods). We are on the main thread
    // (`applicationDidFinishLaunching:`).
    let controller: Option<Retained<AnyObject>> = unsafe {
        let allocated: Allocated<AnyObject> = msg_send![class, alloc];
        msg_send![
            allocated,
            initWithStartingUpdater: Bool::YES,
            updaterDelegate: &*delegate,
            userDriverDelegate: none,
        ]
    };
    Some(Updater {
        controller: controller?,
        _delegate: delegate,
    })
}
