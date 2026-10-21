//! What AppKit does to the windows when the application quits: once the delegate's
//! `applicationWillTerminate:` has returned, it closes every window still open, and each
//! window's delegate hears `windowWillClose:` — the very notification the user's red button
//! sends.
//!
//! bateri leans on both halves. The quit's own work is in `applicationWillTerminate:`
//! (`AppDelegate::shutdown`): it decides by `keep_running` what becomes of the programs and of
//! the terminals servers keep, and after a handover it leaves every window open with its panes.
//! Then AppKit closes those windows, and `TerminalWindow`'s `windowWillClose:` — which ends the
//! terminals servers keep when the *user* closes a window — must know the close is the quit's.
//! It asks `AppDelegate::is_terminating`, which `shutdown` sets first; that only works because
//! the close comes **after** `applicationWillTerminate:`. Had nobody known that AppKit closes
//! the windows at all, every update and every ⌘Q under `keep_running = "quit"` ended the
//! programs on the servers. This test holds AppKit to the order; the source guard in `app.rs`
//! (`the_paths_that_close_a_pane_for_the_user_end_the_terminal_a_server_keeps`) holds bateri
//! to the gate.
//!
//! Its own `main` (`harness = false`): AppKit wants the main thread, and `terminate:` ends the
//! process — so the application runs in a child of this test (the same executable, told by an
//! environment variable), and the parent reads what it printed. Without a window server session
//! (an ssh login) it says so and passes, as `embed` does.

use std::io::Write as _;
use std::process::Command;
use std::time::Duration;

use dispatch2::{DispatchQueue, DispatchTime};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate,
    NSApplicationTerminateReply, NSBackingStoreType, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize};

/// The environment variable that makes the executable the child that quits.
const CHILD: &str = "BT_TERMINATE_TEST_CHILD";

/// What the child says, a line each, in the order AppKit makes it happen.
const LAUNCHED: &str = "launched";
const WILL_TERMINATE: &str = "will terminate";
const WILL_CLOSE: &str = "window will close";

/// The child gives up after this, so a quit that never comes cannot hang the test.
const CHILD_LIMIT: Duration = Duration::from_secs(20);

fn say(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

define_class!(
    // SAFETY: NSObject subclassing has no requirements; `Probe` does not implement `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTerminateProbe"]
    #[ivars = std::cell::RefCell<Option<Retained<NSWindow>>>]
    struct Probe;

    unsafe impl NSObjectProtocol for Probe {}

    unsafe impl NSApplicationDelegate for Probe {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _n: &NSNotification) {
            let mtm = self.mtm();
            // Off every screen and without a frame: a window AppKit counts as open, that
            // nobody sees.
            let frame = NSRect::new(NSPoint::new(-4000.0, -4000.0), NSSize::new(200.0, 100.0));
            // SAFETY: with `defer` false the window exists at once; it is not released on
            // close (below), the `Retained` in the ivars is its owner.
            let window = unsafe {
                NSWindow::initWithContentRect_styleMask_backing_defer(
                    NSWindow::alloc(mtm),
                    frame,
                    NSWindowStyleMask::Borderless,
                    NSBackingStoreType::Buffered,
                    false,
                )
            };
            // SAFETY: only changes who owns the window; the ivars do.
            unsafe { window.setReleasedWhenClosed(false) };
            window.setDelegate(Some(ProtocolObject::from_ref(self)));
            window.orderFront(None);
            self.ivars().replace(Some(window));
            say(LAUNCHED);
            // The quit comes from the run loop, as a menu item's does.
            let when = DispatchTime::try_from(Duration::from_millis(200)).expect("a short delay");
            let _ = DispatchQueue::main().after(when, || {
                let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                NSApplication::sharedApplication(mtm).terminate(None);
            });
        }

        #[unsafe(method(applicationShouldTerminate:))]
        fn should_terminate(&self, _app: &NSApplication) -> NSApplicationTerminateReply {
            NSApplicationTerminateReply::TerminateNow
        }

        #[unsafe(method(applicationWillTerminate:))]
        fn will_terminate(&self, _n: &NSNotification) {
            say(WILL_TERMINATE);
        }
    }

    unsafe impl NSWindowDelegate for Probe {
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _n: &NSNotification) {
            say(WILL_CLOSE);
        }
    }
);

impl Probe {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(std::cell::RefCell::new(None));
        // SAFETY: NSObject's `init` takes no arguments and the ivars are set.
        unsafe { msg_send![super(this), init] }
    }
}

/// The child: an application with one open window, which quits.
fn quit_with_a_window_open() {
    std::thread::spawn(|| {
        std::thread::sleep(CHILD_LIMIT);
        std::process::exit(3);
    });
    let mtm = MainThreadMarker::new().expect("a harness-less test runs on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let probe = Probe::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*probe)));
    app.run();
}

fn main() {
    if std::env::var_os(CHILD).is_some() {
        quit_with_a_window_open();
        return;
    }
    let aqua = Command::new("launchctl")
        .arg("managername")
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).trim() == "Aqua");
    if !aqua {
        println!("terminate: SKIPPED (no window server session)");
        return;
    }
    let output = Command::new(std::env::current_exe().expect("the test's own path"))
        .env(CHILD, "1")
        .output()
        .expect("the child starts");
    let said = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = said.lines().collect();
    assert!(
        output.status.success(),
        "the child quit by AppKit's own exit: {:?}, said {lines:?}, stderr {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        lines,
        [LAUNCHED, WILL_TERMINATE, WILL_CLOSE],
        "AppKit closes the windows a quit leaves open, after applicationWillTerminate: — \
         the order `windowWillClose:` reads the quit's flag by"
    );
    println!("terminate: ok");
}
