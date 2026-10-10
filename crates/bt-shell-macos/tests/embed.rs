//! A pane opened through `embed` in a process that is not bateri: an application without
//! bateri's delegate, a borderless window, and a host that records every event it hears. The
//! shell is started, told to exit, and its exit must reach the host — through the pane's own
//! main-queue return, which finds the pane by its id wherever it is hosted. Nothing on that path
//! may quietly need bateri's application object.
//!
//! Its own `main` (`harness = false`): AppKit and the main queue want the main thread. Without a
//! window server session (an ssh login) it says so and passes, as bateri's own launch does.

use std::cell::RefCell;
use std::process::Command;
use std::rc::Rc;
use std::time::{Duration, Instant};

use bt_core::{Settings, Theme};
use bt_shell_macos::embed::{self, Cover, Host, Identity, Source, TerminalPane};
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSPoint, NSRect, NSRunLoop, NSSize};

/// What a host heard, in order: the event's name and the pane's id.
#[derive(Default)]
struct Recorder(RefCell<Vec<(String, u64)>>);

impl Recorder {
    fn heard(&self, name: &str, pane: u64) {
        self.0.borrow_mut().push((name.to_owned(), pane));
    }

    fn saw(&self, name: &str) -> bool {
        self.0.borrow().iter().any(|(heard, _)| heard == name)
    }
}

impl Host for Recorder {
    fn title_changed(&self, pane: u64) {
        self.heard("title", pane);
    }
    fn shell_exited(&self, pane: u64) {
        self.heard("exit", pane);
    }
    fn focused(&self, pane: u64) {
        self.heard("focused", pane);
    }
    fn uploads_changed(&self, pane: u64) {
        self.heard("uploads", pane);
    }
    fn activity_changed(&self, pane: u64) {
        self.heard("activity", pane);
    }
    fn notify(&self, pane: u64, _title: &str, _body: &str) {
        self.heard("notify", pane);
    }
    fn post_notices(&self, pane: u64, _source: Source, _messages: Vec<String>) {
        self.heard("notices", pane);
    }
    fn files_dragged(&self, pane: u64, _over: bool) {
        self.heard("files", pane);
    }
    fn carry_press(&self, pane: u64, _at: (f64, f64)) {
        self.heard("carry", pane);
    }
    fn questions_changed(&self, pane: u64) {
        self.heard("questions", pane);
    }
    fn cover(&self, _pane: &TerminalPane) -> Option<Rc<dyn Cover>> {
        None
    }
}

/// The pane's id: any number the host picks, unique in the process.
const PANE: u64 = 41;

fn main() {
    let aqua = Command::new("launchctl")
        .arg("managername")
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).trim() == "Aqua");
    if !aqua {
        println!("embed: SKIPPED (no window server session)");
        return;
    }
    let mtm = MainThreadMarker::new().expect("a harness-less test runs on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    assert!(
        app.delegate().is_none(),
        "the application has no delegate: nothing of bateri's"
    );

    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(640.0, 400.0));
    // SAFETY: with `defer` false the window exists at once; it is not released on close (below),
    // the `Retained` here is its owner.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            frame,
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: only changes who owns the window; the `Retained` above does.
    unsafe { window.setReleasedWhenClosed(false) };

    let host = Rc::new(Recorder::default());
    let identity = Rc::new(Identity {
        app_name: "embed-test".to_owned(),
        helper: None,
        zsh_wrapper_dir: None,
    });
    let config = embed::Config::new(
        mtm,
        PANE,
        host.clone(),
        identity,
        Settings::default(),
        Theme::BATERI,
    );
    let pane = embed::open(mtm, frame, config).expect("the pane opens");
    window
        .contentView()
        .expect("a window has a content view")
        .addSubview(&pane);
    pane.observe_frame();
    pane.start(mtm).expect("the shell starts");
    assert!(
        embed::pane(mtm, PANE).is_some(),
        "an open pane is found by its id"
    );

    // Typeahead: the shell reads it when it is ready.
    pane.session()
        .expect("a started pane has a session")
        .write(b"exit\n");
    let deadline = Instant::now() + Duration::from_secs(15);
    while !host.saw("exit") && Instant::now() < deadline {
        NSRunLoop::currentRunLoop().runMode_beforeDate(
            // SAFETY: a Foundation constant, alive for the process.
            unsafe { NSDefaultRunLoopMode },
            &NSDate::dateWithTimeIntervalSinceNow(0.05),
        );
    }
    let heard = host.0.borrow().clone();
    assert!(
        heard.contains(&("exit".to_owned(), PANE)),
        "the shell's exit reached the host with the pane's id; heard {heard:?}"
    );

    pane.close();
    assert!(
        embed::pane(mtm, PANE).is_none(),
        "a closing pane is found no more"
    );
    println!("embed: ok ({} events)", heard.len());
}
