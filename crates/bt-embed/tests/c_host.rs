//! A pane opened through the C interface alone, as a host written in another language opens one:
//! no bateri delegate, a window of the host's own, and a handler that hears every event. The
//! shell is given a variable, a directory and a command that writes both to a file and exits; its
//! exit must reach the handler, which closes the pane from inside that event — a host's natural
//! answer, with the pane's own code still on the stack — and the file must hold what was given.
//!
//! Its own `main` (`harness = false`): AppKit wants the main thread. Without a window server
//! session (an ssh login) it says so and passes, as bateri's own launch does.

use std::cell::RefCell;
use std::ffi::{CString, c_void};
use std::process::Command;
use std::ptr::null_mut;
use std::time::{Duration, Instant};

use bt_embed::*;
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSPoint, NSRect, NSRunLoop, NSSize};

/// What the host keeps: the pane's handle until it closes it, and the kinds it heard.
struct Host {
    pane: *mut BtPane,
    heard: Vec<u32>,
}

/// The pane's id: any number the host picks.
const PANE: u64 = 7;

unsafe extern "C" fn heard(context: *mut c_void, event: *const BtEvent) {
    // SAFETY: the context is the `RefCell<Host>` below, alive for the run.
    let host = unsafe { &*context.cast::<RefCell<Host>>() };
    // SAFETY: the event is lent for this call.
    let (kind, pane) = unsafe { (bt_event_kind(event), bt_event_pane(event)) };
    assert_eq!(pane, PANE, "every event names the pane");
    // Held across the close: an event arriving from inside it would find the host borrowed and
    // fail here — a host's handle must not be reached once it is closing.
    let mut host = host.borrow_mut();
    host.heard.push(kind);
    if kind == kind::SHELL_EXITED {
        let handle = std::mem::replace(&mut host.pane, null_mut());
        // SAFETY: the handle is the open pane's, not used again.
        unsafe { bt_pane_close(handle) };
    }
}

fn c(text: &str) -> CString {
    CString::new(text).expect("no NUL")
}

fn main() {
    let aqua = Command::new("launchctl")
        .arg("managername")
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).trim() == "Aqua");
    if !aqua {
        println!("c_host: SKIPPED (no window server session)");
        return;
    }
    let mtm = MainThreadMarker::new().expect("a harness-less test runs on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(640.0, 400.0));
    // SAFETY: with `defer` false the window exists at once; it is not released on close (below),
    // the `Retained` here is its owner.
    let window: Retained<NSWindow> = unsafe {
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
    let content = window.contentView().expect("a window has a content view");

    let dir = std::env::temp_dir()
        .join(format!("bt-embed-c-host-{}", std::process::id()))
        .canonicalize()
        .unwrap_or_else(|_| {
            let dir = std::env::temp_dir().join(format!("bt-embed-c-host-{}", std::process::id()));
            std::fs::create_dir_all(&dir).expect("a scratch directory");
            dir.canonicalize().expect("the scratch directory resolves")
        });
    let out = dir.join("out");
    let probe = format!("probe-{}", std::process::id());

    let host = RefCell::new(Host {
        pane: null_mut(),
        heard: Vec::new(),
    });
    // SAFETY: every string is alive for its call; the configuration is consumed by the open; the
    // context outlives the pane.
    unsafe {
        let config = bt_pane_config_new(PANE, c("c-host-test").as_ptr());
        assert!(!config.is_null());
        assert!(bt_pane_config_set_event_handler(
            config,
            Some(heard),
            std::ptr::from_ref(&host).cast_mut().cast(),
        ));
        assert!(bt_pane_config_set_working_directory(
            config,
            c(dir.to_str().expect("a UTF-8 path")).as_ptr()
        ));
        assert!(bt_pane_config_add_env(
            config,
            c("BT_EMBED_PROBE").as_ptr(),
            c(&probe).as_ptr()
        ));
        assert!(bt_pane_config_add_env(
            config,
            c("BT_EMBED_OUT").as_ptr(),
            c(out.to_str().expect("a UTF-8 path")).as_ptr()
        ));
        assert!(bt_pane_config_set_command(
            config,
            c(r#"printf '%s|%s' "$BT_EMBED_PROBE" "$PWD" > "$BT_EMBED_OUT"; exit"#).as_ptr()
        ));
        let pane = bt_pane_open(Retained::as_ptr(&content).cast_mut().cast(), config);
        assert!(!pane.is_null(), "the pane opens");
        host.borrow_mut().pane = pane;
        assert_eq!(bt_pane_id(pane), PANE);
        assert!(bt_pane_start(pane), "the shell starts");
    }

    let deadline = Instant::now() + Duration::from_secs(15);
    while !host.borrow().heard.contains(&kind::SHELL_EXITED) && Instant::now() < deadline {
        NSRunLoop::currentRunLoop().runMode_beforeDate(
            // SAFETY: a Foundation constant, alive for the process.
            unsafe { NSDefaultRunLoopMode },
            &NSDate::dateWithTimeIntervalSinceNow(0.05),
        );
    }
    let heard = host.borrow().heard.clone();
    assert!(
        heard.contains(&kind::SHELL_EXITED),
        "the shell's exit reached the handler; heard {heard:?}"
    );
    assert!(host.borrow().pane.is_null(), "the handler closed the pane");
    assert!(
        content.subviews().is_empty(),
        "a closed pane leaves its parent"
    );
    let written = std::fs::read_to_string(&out).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        written,
        format!("{probe}|{}", dir.display()),
        "the shell had the host's variable and directory"
    );
    println!("c_host: ok ({} events)", heard.len());
}
