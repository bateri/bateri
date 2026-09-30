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
//! It is never started in a timed run (`BT_RUN_SECONDS`): the hermetic run
//! does not go to the network and an update prompt must not cover the window.
//! The decision is the caller's (`app`).

use objc2::msg_send;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2_foundation::{NSBundle, NSString};

/// The framework's name in the bundle; its path is under `Contents/Frameworks/`.
const FRAMEWORK: &str = "Sparkle.framework";

/// Loads the bundle's Sparkle and builds the updater **starting it**
/// (`initWithStartingUpdater:YES`). `None` when the framework is missing,
/// cannot be loaded or the class is not found — a bateri without updates is
/// still a complete terminal, so no branch is fatal.
///
/// The returned object must be kept for the life of the process: it is the
/// menu item's target and `NSMenuItem` holds its target weakly.
pub(crate) fn start() -> Option<Retained<AnyObject>> {
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
    // SAFETY: Sparkle 2's documented initializer:
    // `-initWithStartingUpdater:(BOOL) updaterDelegate:(id) userDriverDelegate:(id)`,
    // both delegates are nullable. We are on the main thread
    // (`applicationDidFinishLaunching:`).
    unsafe {
        let allocated: Allocated<AnyObject> = msg_send![class, alloc];
        msg_send![
            allocated,
            initWithStartingUpdater: Bool::YES,
            updaterDelegate: none,
            userDriverDelegate: none,
        ]
    }
}
