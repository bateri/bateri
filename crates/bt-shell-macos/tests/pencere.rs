//! A program's view in a pane, end to end, in a process that is not bateri: a real pane with
//! a real shell, a program that speaks the pencere protocol through two files, and the
//! `WKWebView` the pane puts over its terminal for the view the program asks for.
//!
//! The program is this test. The shell runs a script that puts the tty in raw mode and is a
//! cable: what the test appends to `in` is written to the pane as the program's output, and
//! what the pane writes to the program's input arrives in `out`. The test says `hello`, opens
//! a view of a bundle whose page speaks the bridge of a page (a script of the bundle), and
//! from there the only thing that can make the program receive the next message is the whole
//! chain working — the shell page served by the application's scheme handler, the page's
//! script served from the bundle to the view's own origin under the policy the crate built,
//! the host's first `context` delivered once the frame has loaded, the page's `postMessage`
//! carried through the script-message handler into the session and written to the program as
//! a `msg`, and the program's `msg` (a response, a push) carried back by `evaluateJavaScript`.
//! The page calls the program once it has been told its `context`, so the call that proves it
//! is the first thing the program hears of a started page; the program answers it, and the
//! page then subscribes, hears a push, and calls the program once more with what it heard.
//!
//! Then ⌃C typed in the view arrives at the program as the byte 0x03, a key that is the
//! application's shortcut (⌘T) goes to the main menu and not to the view, the view covers the
//! whole pane and gives its owner the words for the header over it, and the program's `bye`
//! takes the view down. A page can
//! ask to close itself (`close`, and the program is told `closed` with reason `view`).
//!
//! A bundle that asks for an address makes the pane ask the person, with a sheet on the
//! window: the test reads the sheet's words and presses its buttons the way a person's
//! click would (`performClick` on the button, in this process; the one button it never
//! presses is the one that opens a link in the user's browser). The view does not start
//! before the answer, the page is told what was allowed in its `context`, an answer is not
//! asked for again by the same program and set, and another set asks. A link the page asks to
//! open is asked about and, refused, nothing is said to the page.
//!
//! The engine's own channels are closed under a page whatever the person allowed: a page that
//! opens a peer connection (STUN over UDP and TURN over TCP to loopback) and links a preconnect and
//! a prefetch reaches no listener of this Mac, while the one origin it was allowed answers a fetch
//! (the listeners are known to be reachable). A pane whose page asks for the keyboard does not take
//! it from a pane that has it, and takes it from the pane that is the person's.
//!
//! Its own `main` (`harness = false`): AppKit, the main queue and `WKWebView` want the main
//! thread. Without a window server session (an ssh login) it says so and passes, as the embed
//! test does. It is a heavy test — a real shell, a real web view, tens of seconds — and runs
//! only when `BT_HEAVY` is set (`make test-view`); `make check` passes it
//! by, as `#[ignore]` does a heavy test that has the harness.

use std::cell::{Cell, RefCell};
use std::fs;
use std::io::{Read as _, Write as _};
use std::net::{TcpListener, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use bt_core::{Chord, Key, Mods, PaneUuid, Settings, Theme};
use bt_shell_common::agent::message::{Format, Reply};
use bt_shell_common::agent::view::{Action, Step, View, Way};
use bt_shell_common::kablo::SessionId;
use bt_shell_common::kablo::service::{LocalAsk, PaneLink, Shown};
use bt_shell_macos::embed::{self, Cover, Host, Identity, Source, TerminalPane};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel,
};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSApplication, NSApplicationActivationPolicy,
    NSBackingStoreType, NSBitmapImageFileType, NSBitmapImageRep, NSButton, NSEvent,
    NSEventModifierFlags, NSEventType, NSGraphicsContext, NSImage, NSMenu, NSMenuItem,
    NSPasteboard, NSPasteboardItem, NSPasteboardTypeString, NSPasteboardWriting, NSTextField,
    NSTextView, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_core_graphics::CGImage;
use objc2_foundation::{
    NSArray, NSData, NSDate, NSDefaultRunLoopMode, NSDictionary, NSObject, NSObjectProtocol,
    NSPoint, NSRect, NSRunLoop, NSSize, NSString,
};

/// What a host heard: nothing this test is about, but the trait wants an answer to each. The
/// second is the terminal of a server the pane shows, as the section about the connection says it.
#[derive(Default)]
struct Quiet(RefCell<Vec<String>>, RefCell<Option<Shown>>);

impl Host for Quiet {
    /// ⌘T is this host's own shortcut (a new tab, in an application that has tabs).
    fn keeps_key(&self, chord: &Chord) -> bool {
        *chord == Chord::plain(Mods::CMD, Key::Char('t'))
    }
    fn title_changed(&self, _: u64) {}
    fn shell_exited(&self, _: u64) {
        self.0.borrow_mut().push("exit".to_owned());
    }
    fn focused(&self, _: u64) {}
    fn uploads_changed(&self, _: u64) {}
    fn activity_changed(&self, _: u64) {}
    fn notify(&self, _: u64, _: &str, _: &str) {}
    fn post_notices(&self, _: u64, _: Source, _: Vec<String>) {}
    fn questions_changed(&self, _: u64) {}
    fn program_notification(&self, _: u64, _: Option<&str>, _: &str) {}
    fn cover(&self, _: &TerminalPane) -> Option<Rc<dyn Cover>> {
        None
    }
    fn kept_terminal(&self, _: &PaneUuid) -> Option<Shown> {
        self.1.borrow().clone()
    }
}

define_class!(
    // SAFETY: `NSObject` has no subclassing condition; no `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "PencereTestMenuTarget"]
    #[ivars = Cell<u32>]
    struct MenuTarget;

    unsafe impl NSObjectProtocol for MenuTarget {}

    impl MenuTarget {
        /// The action of the menu item with ⌘T: counts how many times the menu ran it.
        #[unsafe(method(newTab:))]
        fn new_tab(&self, _sender: Option<&AnyObject>) {
            self.ivars().set(self.ivars().get() + 1);
        }
    }
);

/// The pane's id: any number the host picks, unique in the process.
const PANE: u64 = 77;

/// How long any one step may take. A web view's first load spawns WebKit's processes.
const STEP: Duration = Duration::from_secs(60);

/// ESC.
const ESC: &str = "\u{1b}";

/// Runs the main run loop for a moment.
fn spin(seconds: f64) {
    NSRunLoop::currentRunLoop().runMode_beforeDate(
        // SAFETY: a Foundation constant, alive for the process.
        unsafe { NSDefaultRunLoopMode },
        &NSDate::dateWithTimeIntervalSinceNow(seconds),
    );
}

/// Runs the main run loop for `seconds` whatever comes in it (one `spin` returns at the first
/// thing it handled).
fn spin_for(seconds: f64) {
    let until = Instant::now() + Duration::from_secs_f64(seconds);
    while Instant::now() < until {
        spin(0.05);
    }
}

/// Runs the loop until `done` or the step's time is out.
fn until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + STEP;
    while !done() {
        assert!(Instant::now() < deadline, "timed out: {what}");
        spin(0.05);
    }
}

/// A pencere frame: `ESC _ pencere;<kind>;<control>;<payload> ESC \`.
fn frame(kind: &str, control: &str, payload: &str) -> String {
    format!("{ESC}_pencere;{kind};{control};{payload}{ESC}\\")
}

/// The program's side of the files.
struct Program {
    input: PathBuf,
    output: PathBuf,
    /// The id of the next message the program writes.
    next: u32,
    /// How much of `out` the test has read.
    read: usize,
}

impl Program {
    /// Writes bytes as the program's output: they reach the pane's parser.
    fn say(&self, text: &str) {
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&self.input)
            .expect("the input file exists");
        file.write_all(text.as_bytes()).expect("written");
    }

    /// The id of the next message the program writes.
    fn take_id(&mut self) -> u32 {
        let id = self.next;
        self.next += 1;
        id
    }

    /// One frame with the next id.
    fn send(&mut self, kind: &str, payload: &str) {
        let id = self.take_id();
        self.say(&frame(kind, &format!("id={id}"), payload));
    }

    /// The query, with the fence a program writes behind it, and the token of the answer.
    fn hello(&mut self) -> String {
        let id = self.take_id();
        self.say(&format!(
            "{ESC}_pencere;hello;id={id};{{\"versions\":[1],\"views\":[{{\"kind\":\"web\",\"versions\":[\"0.1\"]}}]}}{ESC}\\{ESC}[c"
        ));
        let welcome = self.wait_for("pencere;welcome");
        assert!(
            welcome.contains("pencere-test"),
            "the host is the application that opened the pane: {welcome}"
        );
        assert!(
            welcome.contains("\"dismiss\""),
            "the host closes a view the program dismisses, and says so: {welcome}"
        );
        assert!(
            welcome.contains("\"views\":[{\"kind\":\"web\",\"versions\":[\"0.1\"]}]"),
            "the host opens the page at the version of its bridge: {welcome}"
        );
        welcome
            .split("\"token\":\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .expect("a token in the welcome")
            .to_owned()
    }

    /// The query of a program that offers a view of `kind` (`lua`), and the token of the answer.
    fn hello_kind(&mut self, kind: &str) -> String {
        let id = self.take_id();
        self.say(&format!(
            "{ESC}_pencere;hello;id={id};{{\"versions\":[1],\"views\":[{{\"kind\":\"{kind}\",\"versions\":[\"0.1\"]}}]}}{ESC}\\{ESC}[c"
        ));
        let welcome = self.wait_for("pencere;welcome");
        welcome
            .split("\"token\":\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .expect("a token in the welcome")
            .to_owned()
    }

    /// The open of a view of the kind `lua` whose bundle is the one script `source`.
    fn open_script(&mut self, token: &str, source: &str) {
        let bytes = source.as_bytes();
        self.send(
            "open",
            &format!(
                "{{\"token\":\"{token}\",\"view\":\"lua\",\"version\":\"0.1\",\"program\":\"Pencere Test\",\
\"title\":\"Keys\",\"bundle\":{{\"entry\":\"page.lua\",\"files\":[{{\"path\":\"page.lua\",\"sha256\":\"{}\",\"size\":{},\"type\":\"text/x-lua\"}}]}}}}",
                bt_core::pencere::Digest::of(bytes),
                bytes.len()
            ),
        );
    }

    /// The open of a view of `files` (path, type, bytes), asking for the address `domain` if it
    /// names one. The program calls itself “Pencere Test”, the view “Show things”, and gives
    /// its icon.
    fn open(&mut self, token: &str, files: &[(&str, &str, &[u8])], domain: Option<&str>) {
        self.open_with_access(
            token,
            files,
            domain.map(|domain| format!("{{\"connect\":[\"{domain}\"]}}")),
        );
    }

    /// The same, asking for the `access` given as JSON.
    fn open_with_access(
        &mut self,
        token: &str,
        files: &[(&str, &str, &[u8])],
        access: Option<String>,
    ) {
        let list: Vec<String> = files
            .iter()
            .map(|(path, kind, bytes)| {
                format!(
                    "{{\"path\":\"{path}\",\"sha256\":\"{}\",\"size\":{},\"type\":\"{kind}\"}}",
                    bt_core::pencere::Digest::of(bytes),
                    bytes.len()
                )
            })
            .collect();
        let access = access.map_or_else(String::new, |access| format!(",\"access\":{access}"));
        self.send(
            "open",
            &format!(
                "{{\"token\":\"{token}\",\"view\":\"web\",\"version\":\"0.1\",\"program\":\"Pencere Test\",\
\"title\":\"Show things\",\"icons\":[{{\"src\":\"{ICON}\"}}]{access},\
\"bundle\":{{\"entry\":\"index.html\",\"files\":[{}]}}}}",
                list.join(",")
            ),
        );
    }

    /// The host asks for the files it does not hold (`need`): sends those of `files`, and returns
    /// what it asked for.
    fn supply(&mut self, files: &[(&str, &str, &[u8])]) -> String {
        let missing = self.wait_for("pencere;need");
        for (_, _, bytes) in files {
            let digest = bt_core::pencere::Digest::of(bytes).to_string();
            if missing.contains(&digest) {
                let id = self.take_id();
                self.say(&frame(
                    "file",
                    &format!("id={id},d={digest}"),
                    &base64(bytes),
                ));
            }
        }
        missing
    }

    /// The program ends its view.
    fn bye(&mut self, token: &str) {
        self.send(
            "bye",
            &format!("{{\"token\":\"{token}\",\"reason\":\"exit\"}}"),
        );
    }

    /// What the pane has written to the program's input so far, as text.
    fn heard(&self) -> String {
        String::from_utf8_lossy(&fs::read(&self.output).unwrap_or_default()).into_owned()
    }

    /// Waits for `needle` in what the pane writes after what was already waited
    /// for, and returns the text from that point up to the end of the
    /// frame that holds it.
    fn wait_for(&mut self, needle: &str) -> String {
        let mut found = None;
        until(&format!("the program to hear {needle}"), || {
            let all = self.heard();
            let fresh = all.get(self.read..).unwrap_or("");
            match fresh.find(needle) {
                Some(at) => {
                    let end = fresh[at..]
                        .find(&format!("{ESC}\\"))
                        .map_or(fresh.len(), |end| at + end);
                    found = Some(fresh[..end].to_owned());
                    self.read += end;
                    true
                }
                None => false,
            }
        });
        found.expect("found")
    }
}

/// The bundle's page: it loads the script of its bundle, which only runs if the script was
/// served to the view's own origin (a `<script src>` the policy's `'self'` allows).
const PAGE: &str = "<!doctype html><html><head><meta charset=utf-8><title>t</title></head>\
<body><p id=p>view</p><script src=app.js></script></body></html>";

/// The page's script. The page speaks the bridge: when it is told its `context` it calls the
/// program once and says what it was told; the program's response makes it subscribe; a push
/// makes it call again with what it heard. (Once: the host may tell the context again.)
const SCRIPT: &str = "var send=function(m){window.parent.postMessage(m,'*')};\
var started=false;\
window.addEventListener('message',function(e){\
if(e.source!==window.parent)return;var m=e.data;\
if(m.context){if(started)return;started=true;\
send({call:{id:1,method:'hello',params:{mode:m.context.mode,\
width:m.context.size.width,height:m.context.size.height,\
background:m.context.colors.background,access:m.context.access||null}}})}\
else if(m.response&&m.response.id===1){send({subscribe:{id:2,topic:'ticks'}})}\
else if(m.push&&m.push.id===2){send({call:{id:3,method:'pushed',params:m.push.value}});\
setTimeout(escape,0)}});\
var escape=function(){\
try{var h=document.createElement('a');h.href='#section';document.body.appendChild(h);h.click()}catch(e){}\
try{location.hash='routed'}catch(e){}\
try{location.href='other.html'}catch(e){}\
try{var f=document.createElement('form');f.action='other.html';f.method='post';\
document.body.appendChild(f);f.submit()}catch(e){}\
try{var a=document.createElement('a');a.href='other.html';document.body.appendChild(a);a.click()}catch(e){}\
try{window.open('other.html')}catch(e){}\
setTimeout(function(){send({call:{id:4,method:'checked',params:{path:location.pathname,hash:location.hash}}})},700)};";

/// The bundle's other file, which a page that navigated would load: it says so to the program.
const OTHER: &str = "<!doctype html><script>\
window.parent.postMessage({call:{id:77,method:'moved',params:{}}},'*')</script>";

/// A script that, once told its `context`, asks to open a link, and calls the program with
/// everything else the host says to it (a page's one way to reach the program, and so the test's
/// way to see what the page was told).
const LINK_SCRIPT: &str = "var send=function(m){window.parent.postMessage(m,'*')};\
var started=false;\
window.addEventListener('message',function(e){\
if(e.source!==window.parent)return;var m=e.data;\
if(m.context){if(started)return;started=true;\
send({call:{id:1,method:'hello',params:{}}});\
send({link:{url:'https://example.com/docs'}})}\
else{send({call:{id:9,method:'told',params:m}})}});";

/// A script that asks to be closed as soon as it is told its `context`.
const CLOSE_SCRIPT: &str = "var send=function(m){window.parent.postMessage(m,'*')};\
var started=false;\
window.addEventListener('message',function(e){\
if(e.source!==window.parent)return;var m=e.data;\
if(m.context&&!started){started=true;send({close:{}})}});";

/// A page that nests two frames once it is told its `context`: the origin the person allowed
/// (`ALLOWED`) and one nobody did (`BLOCKED`). What the allowed one says to its parent is what the
/// program hears (`framed`); the page then tries every way there is to go to another document,
/// as `SCRIPT` does, and goes for the allowed origin itself in its own frame, and says where it
/// is (`checked`).
const FRAMES_SCRIPT: &str = "var send=function(m){window.parent.postMessage(m,'*')};\
var started=false;\
var embed=function(src){var f=document.createElement('iframe');f.src=src;document.body.appendChild(f)};\
var escape=function(){\
try{var h=document.createElement('a');h.href='#section';document.body.appendChild(h);h.click()}catch(e){}\
try{location.hash='routed'}catch(e){}\
try{location.href='ALLOWED/third'}catch(e){}\
try{location.href='other.html'}catch(e){}\
try{var f=document.createElement('form');f.action='other.html';f.method='post';\
document.body.appendChild(f);f.submit()}catch(e){}\
try{var a=document.createElement('a');a.href='other.html';document.body.appendChild(a);a.click()}catch(e){}\
try{window.open('other.html')}catch(e){}\
setTimeout(function(){send({call:{id:4,method:'checked',params:{path:location.pathname,hash:location.hash}}})},700)};\
window.addEventListener('message',function(e){\
if(e.source===window.parent){var m=e.data;\
if(m.context&&!started){started=true;\
send({call:{id:1,method:'hello',params:{access:m.context.access||null}}});\
embed('ALLOWED/');embed('BLOCKED/')}return}\
send({call:{id:20,method:'framed',params:{said:e.data}}});\
if(e.data==='second')escape()});";

/// What the server on this Mac gives a nested frame: a page that says `first` to the page that
/// nests it and then goes on to `/second` in its own frame (a navigation of a frame the page
/// nested), which says `second`.
const FRAME_FIRST: &str = "<!doctype html><script>parent.postMessage('first','*');\
setTimeout(function(){location.href='/second'},100)</script>";
const FRAME_SECOND: &str = "<!doctype html><script>parent.postMessage('second','*')</script>";

/// A server on this Mac, for a view to nest a frame of. It counts the connections it gets: one
/// that is never asked is an origin the web view never reached.
struct Local {
    origin: String,
    hits: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Local {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a port on this Mac");
        listener
            .set_nonblocking(true)
            .expect("a listener that does not wait");
        let port = listener.local_addr().expect("an address").port();
        let hits = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = thread::spawn({
            let (hits, stop) = (Arc::clone(&hits), Arc::clone(&stop));
            move || {
                while !stop.load(Ordering::Acquire) {
                    let Ok((mut stream, _)) = listener.accept() else {
                        thread::sleep(Duration::from_millis(10));
                        continue;
                    };
                    hits.fetch_add(1, Ordering::AcqRel);
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                    let mut request = [0u8; 2048];
                    let read = stream.read(&mut request).unwrap_or(0);
                    let head = String::from_utf8_lossy(&request[..read]).into_owned();
                    let body = match head.split_whitespace().nth(1) {
                        Some("/second") => FRAME_SECOND,
                        Some("/") => FRAME_FIRST,
                        _ => "",
                    };
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                }
            }
        });
        Self {
            origin: format!("http://127.0.0.1:{port}"),
            hits,
            stop,
            thread: Some(thread),
        }
    }

    fn hits(&self) -> usize {
        self.hits.load(Ordering::Acquire)
    }
}

impl Drop for Local {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Whether a view covers the pane, as a host can see it: a subview of the view's class (the
/// name may carry KVO's prefix, which AppKit adds once something observes the instance).
fn web_view(pane: &TerminalPane) -> Option<objc2::rc::Retained<objc2_app_kit::NSView>> {
    pane.subviews().iter().find(|view| {
        view.class()
            .name()
            .to_string_lossy()
            .contains("BateriPencereView")
    })
}

fn covered(pane: &TerminalPane) -> bool {
    web_view(pane).is_some()
}

/// The pane's terminal, the view the shell is drawn in: a subview of the pane of its own class,
/// under the program's view when there is one.
fn terminal_view(pane: &TerminalPane) -> Option<Retained<NSView>> {
    pane.subviews().iter().find(|view| {
        let name = view.class().name().to_string_lossy().into_owned();
        name.contains("BateriView") && !name.contains("BateriPencere")
    })
}

/// Whether the window's keyboard is in the program's view of `pane`, or something inside it.
fn holds_keyboard(window: &NSWindow, pane: &TerminalPane) -> bool {
    let Some(web) = web_view(pane) else {
        return false;
    };
    window
        .firstResponder()
        .and_then(|responder| responder.downcast::<NSView>().ok())
        .is_some_and(|view| view.isDescendantOf(&web))
}

/// The test program's icon (the `icons` of its `open`): a 64-pixel PNG of three stacked boxes, as a
/// `data:` URI, the only kind a terminal takes.
const ICON: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAYAAACqaXHeAAAAy0lEQVR42u3YsQ3CMBBG4QzowhJTsIMrClgD0dEyAYUHSEXrUYwcKR1VSOP4e9JJVqr4j+zcvWkCAAD4QUg5hpRLSLl2Wu3d4z+brwepuCWAcqAAypYA6pFqtwBOt3c9P19LtfVQAbQNX+ZHvX7uS7V1LyHsEkD76uvm12rPBOAIuATHCWCk3+DwjdDYrfDwwxDAB/ABfAAfoBU2DBmHBeAIuAT5AD6AD+ADwAfwAXwAH8AH8AF8AB/AB/ABfAAfwAfwAXwAAAAAOucLnVucWIVm/igAAAAASUVORK5CYII=";

/// Every view under `view`, `view` first.
fn descendants(view: Retained<NSView>) -> Vec<Retained<NSView>> {
    let mut found = vec![view];
    let mut at = 0;
    while at < found.len() {
        let subviews = found[at].subviews();
        found.extend(subviews.iter());
        at += 1;
    }
    found
}

/// The sheet the window shows, once it shows one: the pane's question.
fn sheet_of(window: &NSWindow) -> Retained<NSWindow> {
    let mut sheet = None;
    until("the pane's question to open as a sheet", || {
        sheet = window.attachedSheet();
        sheet.is_some()
    });
    sheet.expect("a sheet")
}

/// What the sheet says: the title and the text, one after the other.
fn sheet_text(sheet: &NSWindow) -> String {
    let content = sheet.contentView().expect("a sheet has a content view");
    descendants(content)
        .iter()
        .filter_map(|view| {
            view.downcast_ref::<NSTextField>()
                .map(|field| field.stringValue().to_string())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The titles of the sheet's buttons, sorted (the views stand in the order AppKit made them,
/// which says nothing of which button Return presses).
fn sheet_buttons(sheet: &NSWindow) -> Vec<String> {
    let content = sheet.contentView().expect("a sheet has a content view");
    let mut titles: Vec<String> = descendants(content)
        .iter()
        .filter_map(|view| {
            view.downcast_ref::<NSButton>()
                .map(|button| button.title().to_string())
        })
        .collect();
    titles.sort();
    titles
}

/// Presses the sheet's button titled `title`, as a click on it does, and waits for the sheet
/// to go. Never the button that opens a link.
fn press(window: &NSWindow, sheet: &NSWindow, title: &str) {
    assert_ne!(
        title, "Open",
        "a test does not open a page in the user's browser"
    );
    let content = sheet.contentView().expect("a sheet has a content view");
    let button = descendants(content)
        .into_iter()
        .find_map(|view| {
            let button = view.downcast_ref::<NSButton>()?;
            (button.title().to_string() == title).then(|| button.retain())
        })
        .unwrap_or_else(|| panic!("the sheet has no {title:?} button"));
    // SAFETY: a button of the sheet, clicked on the main thread; no sender is named.
    unsafe { button.performClick(None) };
    // Ended: the window has no sheet, or another one, which a question that waits behind this
    // one opens the moment it is gone.
    until("the sheet to end", || {
        window
            .attachedSheet()
            .is_none_or(|current| !std::ptr::eq(&*current, sheet))
    });
}

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bateri-pencere-test-{}", std::process::id()));
    fs::create_dir_all(dir.join("zdot")).expect("the scratch folder");
    dir
}

fn main() {
    if std::env::var_os("BT_HEAVY").is_none() {
        println!("pencere: SKIPPED (heavy; make test-heavy)");
        return;
    }
    let aqua = Command::new("launchctl")
        .arg("managername")
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).trim() == "Aqua");
    if !aqua {
        println!("pencere: SKIPPED (no window server session)");
        return;
    }
    let dir = scratch();
    let result = std::panic::catch_unwind(|| run(&dir));
    // Only the folder this test made, by its own name.
    let _ = fs::remove_dir_all(&dir);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

/// A pane in `window` whose shell is a cable between its tty and two files in `dir`, and the
/// program's side of the files. The pane is `id` to its host.
fn open_pane(
    mtm: MainThreadMarker,
    window: &NSWindow,
    frame_rect: NSRect,
    dir: &Path,
    id: u64,
) -> (Retained<TerminalPane>, Program) {
    open_hosted_pane(
        mtm,
        window,
        frame_rect,
        dir,
        id,
        Rc::new(Quiet::default()),
        Settings::default(),
    )
}

/// [`open_pane`], for a host the test keeps and with `settings` (its font, say).
fn open_hosted_pane(
    mtm: MainThreadMarker,
    window: &NSWindow,
    frame_rect: NSRect,
    dir: &Path,
    id: u64,
    host: Rc<Quiet>,
    settings: Settings,
) -> (Retained<TerminalPane>, Program) {
    fs::create_dir_all(dir.join("zdot")).expect("the pane's folder");
    let identity = Rc::new(Identity {
        app_name: "pencere-test".to_owned(),
        helper: None,
        zsh_wrapper_dir: None,
        fish_wrapper_dir: None,
        bash_wrapper_dir: None,
    });
    let mut config = embed::Config::new(mtm, id, host, identity, settings, Theme::BATERI);
    // No user dotfiles: the shell reads what it is typed and nothing of the account's.
    config.env = vec![
        ("ZDOTDIR".to_owned(), dir.join("zdot").display().to_string()),
        ("PENCERE_TEST_DIR".to_owned(), dir.display().to_string()),
    ];
    let pane = embed::open(mtm, frame_rect, config).expect("the pane opens");
    window
        .contentView()
        .expect("a window has a content view")
        .addSubview(&pane);
    pane.observe_frame();
    pane.start(mtm).expect("the shell starts");

    let input = dir.join("in");
    fs::write(&input, b"").expect("the input file");
    let program = Program {
        output: dir.join("out"),
        input,
        next: 1,
        read: 0,
    };

    // The program: raw mode, then a cable between the tty and the two files. The `tail`
    // that follows the input file goes with the script, whatever ends it (the hang-up of the
    // pane's close, or a normal exit): a background job outlives the hang-up otherwise.
    pane.session().expect("a started pane has a session").write(
        concat!(
            "sh -c 'stty raw -echo; : > \"$PENCERE_TEST_DIR/ready\"; ",
            "tail -f -c +1 \"$PENCERE_TEST_DIR/in\" & follower=$!; ",
            "trap \"kill $follower\" EXIT HUP TERM; cat > \"$PENCERE_TEST_DIR/out\"'\n"
        )
        .as_bytes(),
    );
    until("the program to be ready", || dir.join("ready").exists());
    // A moment for `stty` to have been applied before the first frame is heard.
    spin(0.3);
    (pane, program)
}

fn run(dir: &Path) {
    let mtm = MainThreadMarker::new().expect("a harness-less test runs on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let frame_rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(640.0, 400.0));
    // SAFETY: with `defer` false the window exists at once; the `Retained` is its owner.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            frame_rect,
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: only changes who owns the window; the `Retained` above does.
    unsafe { window.setReleasedWhenClosed(false) };

    let (pane, mut program) = open_pane(mtm, &window, frame_rect, dir, PANE);

    // The view is for the alternate screen (§6.5).
    program.say(&format!("{ESC}[?1049h"));

    // A view of a bundle that asks for nothing: no question, the files go to the host once, and
    // the page is told its context, calls the program, hears the answer and a push.
    let files = [
        ("index.html", "text/html", PAGE.as_bytes()),
        ("app.js", "text/javascript", SCRIPT.as_bytes()),
        ("other.html", "text/html", OTHER.as_bytes()),
    ];
    let token = program.hello();
    program.open(&token, &files, None);
    let missing = program.supply(&files);
    for (_, _, bytes) in &files {
        let digest = bt_core::pencere::Digest::of(bytes).to_string();
        assert!(
            missing.contains(&digest),
            "the host holds none of them yet: {missing}"
        );
    }
    let hello = watch_it_start(&pane, &mut program);
    // The context the page was told: the theme's own roles, the whole pane (the header over it
    // is the owner's), and nothing allowed (no `access`).
    for needle in [
        "\"mode\":\"dark\"",
        "\"background\":\"#",
        "\"width\":640,\"height\":400",
        "\"access\":null",
    ] {
        assert!(hello.contains(needle), "{needle:?} is not in: {hello}");
    }
    assert!(
        window.attachedSheet().is_none(),
        "the page asked for nothing"
    );
    // The program's answer reaches the page, which subscribes; a push reaches it, and it calls
    // the program with what it heard: the six messages cross the web view both ways.
    program.send("msg", "{\"response\":{\"id\":1,\"result\":{\"ok\":true}}}");
    program.wait_for("\"subscribe\":{\"id\":2,\"topic\":\"ticks\"}");
    program.send("msg", "{\"push\":{\"id\":2,\"value\":{\"n\":7}}}");
    let pushed = program.wait_for("\"method\":\"pushed\"");
    assert!(pushed.contains("\"params\":{\"n\":7}"), "{pushed}");

    // After its entry has loaded the page follows a link to `#section` and sets `location.hash`:
    // both stay on the page and take effect (a page may route by its fragment). Then it tries
    // every way there is to go to another document: a change of `location`, a form posted, a
    // link followed, a window opened. All are cancelled (specification §12.3): the page that
    // asks is the page still, and what it would have run there never said a word.
    let checked = program.wait_for("\"method\":\"checked\"");
    assert!(
        checked.contains("\"path\":\"/index.html\""),
        "the page stays where it loaded: {checked}"
    );
    assert!(
        checked.contains("\"hash\":\"#routed\""),
        "a change of the fragment is the page's own: {checked}"
    );
    assert!(
        !program.heard().contains("\"method\":\"moved\""),
        "a navigation was let through: {}",
        program.heard()
    );

    // The tty's signal characters in the view are the program's: ⌃C, ⌃\ and ⌃Z, each the byte
    // that was set for it, written to the tty (specification §11.3).
    let web = web_view(&pane).expect("the view is a subview of the pane");
    for (character, plain, code, byte) in [
        ("\u{3}", "c", 8, 0x03u8),
        ("\u{1c}", "\\", 42, 0x1c),
        ("\u{1a}", "z", 6, 0x1a),
    ] {
        let before = program.heard().len();
        let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            NSEventType::KeyDown,
            NSPoint::new(0.0, 0.0),
            NSEventModifierFlags::Control,
            0.0,
            window.windowNumber(),
            None,
            &NSString::from_str(character),
            &NSString::from_str(plain),
            false,
            code,
        )
        .expect("a key event");
        web.keyDown(&event);
        until(&format!("{byte:#04x} to reach the program"), || {
            program
                .heard()
                .as_bytes()
                .get(before..)
                .is_some_and(|new| new.contains(&byte))
        });
    }

    // The view covers the whole pane; what it is, the header over the pane says — the owner's,
    // outside the view, where the view cannot draw.
    let (area, bounds) = (web.frame(), pane.bounds());
    assert_eq!(area, bounds);
    // The page's own events are the page's: a pinch, a double tap and a swipe reach its DOM as
    // they are (the web view makes nothing of its own of them: no magnification), and nothing of
    // the pane lies over it to take an event first — a hit in the middle of the pane is the view
    // or something inside it. (Over a floating card the 5 point grips of its edge are the one
    // thing that is above the view, and they are not in the middle.)
    let kit = web
        .clone()
        .downcast::<objc2_web_kit::WKWebView>()
        .expect("the view is a web view");
    // SAFETY: reading a flag of a web view on the main thread.
    assert!(
        !unsafe { kit.allowsMagnification() },
        "the web view magnifies itself"
    );
    let middle = NSPoint::new(bounds.size.width / 2.0, bounds.size.height / 2.0);
    // SAFETY: reading the superview on the main thread; the `Retained` keeps it alive.
    let above = unsafe { pane.superview() };
    let in_super = above.map_or(middle, |above| {
        pane.convertPoint_toView(middle, Some(&above))
    });
    let hit = pane
        .hitTest(in_super)
        .expect("the middle of the pane is something");
    assert!(
        hit.isDescendantOf(&web),
        "a view over the program's takes the events first: {:?}",
        hit.class().name()
    );
    // The words the program gave for the view and itself (the `title` and `program` of its
    // `open`), not the process's; on this Mac no place. Its icon is drawn by its name, and the
    // way out is ⌃C to the program.
    let head = pane
        .view_head()
        .expect("the header's words while the view is up");
    assert_eq!(head.words.place, None);
    assert_eq!(head.words.program.as_deref(), Some("Pencere Test"));
    assert_eq!(head.words.title.as_deref(), Some("Show things"));
    assert!(head.has_icon(), "the program's icon is the header's");
    assert_eq!(head.exit, embed::ViewExit::Quit);

    // The picture a tab's card shows of this pane is the view's own (not the terminal's under
    // it). A card that opens on a view has one at once: the view took its first when it
    // loaded, the page's own shape (the whole pane) and no wider than a tile needs
    // — not the tile's, and not the terminal's (whose shape is the tile's), which is what a
    // card would get if the view had none. The call asked for the tile's own, which a turn of
    // the run loop later is what the card is given: the page fills the tile and what spills
    // over is cut.
    let tile = (200.0, 120.0);
    let shape_of = |image: &CGImage| {
        let (width, height) = (CGImage::width(Some(image)), CGImage::height(Some(image)));
        assert!(width > 0 && height > 0, "a picture with something in it");
        width as f64 / height as f64
    };
    let first = pane
        .snapshot(tile, 2.0)
        .expect("a card that opens on a view has a picture to show");
    let page = pane.bounds().size;
    assert!(
        (shape_of(&first) - page.width / page.height).abs() < 0.02,
        "the view's first picture is the page's shape, not the tile's"
    );
    let wanted = tile.0 / tile.1;
    until("the web view's picture cut to the tile", || {
        pane.snapshot(tile, 2.0)
            .is_some_and(|card| (shape_of(&card) - wanted).abs() < 0.05)
    });
    // The card lets go of its pictures; the next one that opens is not empty either.
    pane.release_snapshot();
    assert!(
        pane.snapshot(tile, 2.0).is_some(),
        "a card is never given an empty tile: the pane's terminal stands in until the view's \
         picture comes"
    );
    pane.release_snapshot();

    // A key that is the application's own — ⌘T here — goes to the main menu while the view
    // has the keyboard, and the menu runs its item once: the view is asked about it and
    // leaves it alone. The menu is this test's: one item with ⌘T.
    let target = MenuTarget::alloc(mtm).set_ivars(Cell::new(0));
    // SAFETY: `NSObject`'s `init`.
    let target: Retained<MenuTarget> = unsafe { msg_send![super(target), init] };
    let bar = NSMenu::new(mtm);
    let holder = NSMenuItem::new(mtm);
    let submenu = NSMenu::new(mtm);
    // SAFETY: `newTab:` is the target's, one optional argument.
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("New Tab"),
            Some(sel!(newTab:)),
            &NSString::from_str("t"),
        )
    };
    // SAFETY: the target outlives the menu; the item holds it weakly.
    unsafe { item.setTarget(Some(&target)) };
    submenu.addItem(&item);
    holder.setSubmenu(Some(&submenu));
    bar.addItem(&holder);
    app.setMainMenu(Some(&bar));
    assert!(
        window.makeFirstResponder(Some(&web)),
        "the view takes the keyboard"
    );
    let command_t = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
        NSEventType::KeyDown,
        NSPoint::new(0.0, 0.0),
        NSEventModifierFlags::Command,
        0.0,
        window.windowNumber(),
        None,
        &NSString::from_str("t"),
        &NSString::from_str("t"),
        false,
        17,
    )
    .expect("a key event");
    assert!(
        window.performKeyEquivalent(&command_t),
        "the key is handled: it is the application's"
    );
    assert_eq!(target.ivars().get(), 1, "the menu ran ⌘T's item once");
    app.setMainMenu(None);

    // The program ends its view: `bye` with the token. The pane is the terminal's again.
    program.bye(&token);
    until("the view to go", || !covered(&pane));
    assert!(
        pane.view_head().is_none(),
        "the header says the terminal's words again"
    );

    // A view whose page asks for an address: the person is asked before the view starts, and
    // the page is told what was allowed.
    let token = program.hello();
    program.open(&token, &files, Some("https://api.example.com"));
    let missing = program.supply(&files);
    assert!(
        missing.ends_with("{\"missing\":[]}"),
        "the host holds the files of the first view still: {missing}"
    );
    // The manifest declares an address, so the person is asked before the view starts: the
    // question says who asks, what and where the program runs, and no view stands while it is up.
    let sheet = sheet_of(&window);
    let text = sheet_text(&sheet);
    for needle in [
        "Allow this view to reach other sites?",
        "Connect to\n• https://api.example.com",
        "opened the view “Show things”",
        "“Pencere Test” on this Mac opened",
        "pencere-test keeps your answer until it quits.",
    ] {
        assert!(text.contains(needle), "{needle:?} is not in:\n{text}");
    }
    assert_eq!(sheet_buttons(&sheet), ["Allow", "Don't Allow"]);
    assert!(!covered(&pane), "the view waits for the answer");
    press(&window, &sheet, "Allow");
    let hello = watch_it_start(&pane, &mut program);
    assert!(
        hello.contains("\"access\":{\"connect\":[\"https://api.example.com\"]}"),
        "the page is told what was allowed: {hello}"
    );
    program.bye(&token);
    until("the bundle's view to go", || !covered(&pane));

    // The same bundle again: the host has every file, and says so; the person has answered
    // this program for this set, and is not asked again.
    let token = program.hello();
    program.open(&token, &files, Some("https://api.example.com"));
    let missing = program.supply(&files);
    assert!(
        missing.ends_with("{\"missing\":[]}"),
        "the second open sends no file: {missing}"
    );
    watch_it_start(&pane, &mut program);
    assert!(
        window.attachedSheet().is_none(),
        "the same answer is not asked for twice"
    );
    program.bye(&token);
    until("the second view to go", || !covered(&pane));

    // Another set is another question, and a place on this Mac is marked as one. "Don't Allow"
    // is an answer: the view starts all the same, with nothing allowed, and the question is
    // not asked again for that set either.
    let token = program.hello();
    program.open(&token, &files, Some("http://localhost:8080"));
    program.supply(&files);
    let sheet = sheet_of(&window);
    let text = sheet_text(&sheet);
    assert!(
        text.contains("• http://localhost:8080 (this Mac)"),
        "{text}"
    );
    assert!(
        text.contains("A place marked above is on this Mac"),
        "{text}"
    );
    press(&window, &sheet, "Don't Allow");
    let hello = watch_it_start(&pane, &mut program);
    assert!(
        hello.contains("\"access\":null"),
        "what the person refused is not allowed: {hello}"
    );
    program.bye(&token);
    until("the refused view to go", || !covered(&pane));
    let token = program.hello();
    program.open(&token, &files, Some("http://localhost:8080"));
    program.supply(&files);
    watch_it_start(&pane, &mut program);
    assert!(
        window.attachedSheet().is_none(),
        "a refusal is remembered as well"
    );
    program.bye(&token);
    until("the remembered view to go", || !covered(&pane));

    // A program that gives up while the person is being asked takes the question with it, and
    // since nobody answered it, the same set is asked again the next time.
    let token = program.hello();
    program.open(&token, &files, Some("https://late.example.com"));
    program.supply(&files);
    let sheet = sheet_of(&window);
    assert!(sheet_text(&sheet).contains("https://late.example.com"));
    program.bye(&token);
    until("the question of a program that left to go", || {
        window.attachedSheet().is_none()
    });
    let token = program.hello();
    program.open(&token, &files, Some("https://late.example.com"));
    program.supply(&files);
    let sheet = sheet_of(&window);
    press(&window, &sheet, "Allow");
    watch_it_start(&pane, &mut program);
    program.bye(&token);
    until("the view of the second try to go", || !covered(&pane));

    // A link the page asks to open: asked about, with where the program runs. A refusal is
    // nothing the page is told: the next thing it hears is the program's.
    let link_files = [
        ("index.html", "text/html", PAGE.as_bytes()),
        ("app.js", "text/javascript", LINK_SCRIPT.as_bytes()),
    ];
    let token = program.hello();
    program.open(&token, &link_files, None);
    program.supply(&link_files);
    watch_it_start(&pane, &mut program);
    let sheet = sheet_of(&window);
    let text = sheet_text(&sheet);
    for needle in [
        "Open this link?",
        "on this Mac opened a view that asks to open this link in your browser on this Mac:",
        "https://example.com/docs",
    ] {
        assert!(text.contains(needle), "{needle:?} is not in:\n{text}");
    }
    assert_eq!(sheet_buttons(&sheet), ["Cancel", "Open"]);
    press(&window, &sheet, "Cancel");
    program.send("msg", "{\"push\":{\"id\":5,\"value\":\"probe\"}}");
    let told = program.wait_for("\"method\":\"told\"");
    assert!(
        told.contains("\"probe\""),
        "the page was told nothing of the person's answer before the program's push: {told}"
    );
    program.bye(&token);
    until("the link view to go", || !covered(&pane));

    // The question about a link that is still up when the view ends goes with the view: its
    // Open would open a link for a view that is not there, and its being up would refuse the next
    // view's link. The next view's link is asked about.
    let token = program.hello();
    program.open(&token, &link_files, None);
    program.supply(&link_files);
    watch_it_start(&pane, &mut program);
    let _left_up = sheet_of(&window);
    program.bye(&token);
    until("the link question of a view that ended to go", || {
        window.attachedSheet().is_none()
    });
    until("the view that ended to go", || !covered(&pane));
    let token = program.hello();
    program.open(&token, &link_files, None);
    program.supply(&link_files);
    watch_it_start(&pane, &mut program);
    let sheet = sheet_of(&window);
    assert!(
        sheet_text(&sheet).contains("Open this link?"),
        "the next view's link is asked about"
    );
    press(&window, &sheet, "Cancel");
    program.bye(&token);
    until("the next link view to go", || !covered(&pane));

    // A view the person allowed to embed an origin (`frames`, the policy's `frame-src`): the frame
    // loads and may go on to other pages of its own, and the page that nests it is still held to
    // the page it opened. An origin the person did not allow is not reached at all: the policy
    // stops it before the web view asks the network.
    let (allowed, blocked) = (Local::start(), Local::start());
    let nested_script = FRAMES_SCRIPT
        .replace("ALLOWED", &allowed.origin)
        .replace("BLOCKED", &blocked.origin);
    let nested_files = [
        ("index.html", "text/html", PAGE.as_bytes()),
        ("app.js", "text/javascript", nested_script.as_bytes()),
        ("other.html", "text/html", OTHER.as_bytes()),
    ];
    let token = program.hello();
    program.open_with_access(
        &token,
        &nested_files,
        Some(format!("{{\"frames\":[\"{}\"]}}", allowed.origin)),
    );
    program.supply(&nested_files);
    let sheet = sheet_of(&window);
    let text = sheet_text(&sheet);
    assert!(text.contains(&allowed.origin), "{text}");
    press(&window, &sheet, "Allow");
    watch_it_start(&pane, &mut program);
    let first = program.wait_for("\"said\":\"first\"");
    assert!(first.contains("\"method\":\"framed\""), "{first}");
    program.wait_for("\"said\":\"second\"");
    let checked = program.wait_for("\"method\":\"checked\"");
    assert!(
        checked.contains("\"path\":\"/index.html\""),
        "the page that nested a frame stays where it loaded: {checked}"
    );
    assert!(
        checked.contains("\"hash\":\"#routed\""),
        "and still routes by its fragment: {checked}"
    );
    assert!(
        !program.heard().contains("\"method\":\"moved\""),
        "a navigation of the view's own frame was let through: {}",
        program.heard()
    );
    assert_eq!(
        allowed.hits(),
        2,
        "the nested frame loaded its two pages, and the view's own frame, sent to the same origin, \
         was not let ask for a third"
    );
    assert_eq!(
        blocked.hits(),
        0,
        "an origin that is not in the policy's frame-src was reached"
    );
    program.bye(&token);
    until("the view that nested a frame to go", || !covered(&pane));

    // A page that asks to be closed: the program is told the view closed (reason `view`) and the
    // pane is the terminal's again, at once.
    let close_files = [
        ("index.html", "text/html", PAGE.as_bytes()),
        ("app.js", "text/javascript", CLOSE_SCRIPT.as_bytes()),
    ];
    let token = program.hello();
    program.open(&token, &close_files, None);
    program.supply(&close_files);
    let closed = program.wait_for("pencere;closed");
    assert!(closed.contains("\"reason\":\"view\""), "{closed}");
    until("the page that closed itself to go", || !covered(&pane));

    // The program closes its view and keeps running (`dismiss`), and opens the tool again: the
    // pane is the terminal's between, the host still has every file of the bundle, so the second
    // open sends none, and the person is not asked again for what they have answered.
    let token = program.hello();
    program.open(&token, &files, None);
    program.supply(&files);
    watch_it_start(&pane, &mut program);
    program.send("dismiss", &format!("{{\"token\":\"{token}\"}}"));
    let closed = program.wait_for("pencere;closed");
    assert!(closed.contains("\"reason\":\"program\""), "{closed}");
    until("the dismissed view to go", || !covered(&pane));
    let token = program.hello();
    program.open(&token, &files, None);
    let missing = program.supply(&files);
    assert!(
        missing.ends_with("{\"missing\":[]}"),
        "the second open after a dismiss sends no file: {missing}"
    );
    watch_it_start(&pane, &mut program);
    assert!(window.attachedSheet().is_none(), "nothing is asked again");
    program.bye(&token);
    until("the reopened view to go", || !covered(&pane));

    // The pane's view started again where the screen came back without its messages (a
    // new bateri showing the terminal, a reconnection): the pane that is the user's has the keyboard in the new view
    // as it had in the first, and a keyboard that finds its way into the terminal under the
    // view goes back to the view when the pane is the user's again.
    assert!(pane.focus(), "the keyboard is in the pane's terminal");
    assert!(!holds_keyboard(&window, &pane));
    pane.session().expect("a session").restart_view();
    let offer = program.wait_for("pencere;welcome");
    assert!(offer.contains("\"restart\":true"), "{offer}");
    let token = offer
        .split("\"token\":\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("a token in the restart")
        .to_owned();
    program.open(&token, &files, None);
    program.supply(&files);
    watch_it_start(&pane, &mut program);
    assert!(
        holds_keyboard(&window, &pane),
        "the restarted view has the keyboard of the pane that had it"
    );
    let terminal = terminal_view(&pane).expect("the terminal is a subview of the pane");
    assert!(window.makeFirstResponder(Some(&terminal)));
    assert!(!holds_keyboard(&window, &pane));
    pane.window_became_key();
    assert!(
        holds_keyboard(&window, &pane),
        "a pane that is the user's does not keep its keyboard in the terminal under its view"
    );
    program.bye(&token);
    until("the restarted view to go", || !covered(&pane));

    // Two panes that open the same tool at once (a restored session) ask the same question, and
    // the person answers it once: the answer to the first is the answer to the other, whose
    // sheet goes with it. A pane that asks for something else is still asked.
    let (twin, mut twin_program) = open_pane(mtm, &window, frame_rect, &dir.join("twin"), PANE + 1);
    twin_program.say(&format!("{ESC}[?1049h"));
    for (first_domain, second_domain) in [
        ("https://twin.example.com", "https://twin.example.com"),
        ("https://alpha.example.com", "https://beta.example.com"),
    ] {
        let same = first_domain == second_domain;
        let first = program.hello();
        program.open(&first, &files, Some(first_domain));
        program.supply(&files);
        let sheet = sheet_of(&window);
        assert!(sheet_text(&sheet).contains(first_domain));
        // The first pane's question is up; the second pane asks while it is.
        let second = twin_program.hello();
        twin_program.open(&second, &files, Some(second_domain));
        twin_program.supply(&files);
        spin(0.5);
        assert!(
            !covered(&twin),
            "the second pane's view waits for its answer"
        );
        press(&window, &sheet, "Allow");
        watch_it_start(&pane, &mut program);
        if same {
            watch_it_start(&twin, &mut twin_program);
            spin(0.5);
            assert!(
                window.attachedSheet().is_none(),
                "the same question is not asked twice"
            );
        } else {
            let sheet = sheet_of(&window);
            assert!(sheet_text(&sheet).contains(second_domain));
            press(&window, &sheet, "Allow");
            watch_it_start(&twin, &mut twin_program);
        }
        program.bye(&first);
        twin_program.bye(&second);
        until("both views to go", || !covered(&pane) && !covered(&twin));
    }
    twin.close();

    // What the engine does beside the policy, and what a page does with the keyboard: sections of
    // their own, so that a run says what failed of each when more than one does.
    let mut failures = Vec::new();
    section("the engine's own channels", &mut failures, || {
        engine_channels(mtm, &window, frame_rect, dir);
    });
    section("a page's keyboard", &mut failures, || {
        page_keyboard(mtm, &window, frame_rect, dir);
    });
    section(
        "a connection that goes and comes back",
        &mut failures,
        || {
            connection_goes_and_comes_back(mtm, &window, frame_rect, dir);
        },
    );
    section("an agent in a web view", &mut failures, || {
        agent_in_a_web_view(mtm, &window, frame_rect, dir);
    });
    section("an agent reads a web page as a tree", &mut failures, || {
        agent_reads_a_web_page_as_a_tree(mtm, &window, frame_rect, dir);
    });
    section(
        "an agent reads a dialog behind a large page",
        &mut failures,
        || {
            agent_reads_a_dialog_behind_a_large_page(mtm, &window, frame_rect, dir);
        },
    );
    section("a pane born without its terminal", &mut failures, || {
        born_without_its_terminal(mtm, &window, frame_rect, dir);
    });
    // Last: it registers the app's fonts for this process, and with them the default font of
    // every pane opened after it.
    section("the app's fonts in the page", &mut failures, || {
        app_fonts_in_the_page(mtm, &window, frame_rect, dir);
    });
    // After the fonts: its fields are in the app's faces, which that section registered.
    section("a field's text on its baseline", &mut failures, || {
        field_text_on_its_baseline(mtm, &window, frame_rect, dir);
    });
    section("a field of rows stands in its box", &mut failures, || {
        field_rows_stand_in_their_boxes(mtm, &window, frame_rect, dir);
    });
    section(
        "a screen reader reads a page as the pane's group",
        &mut failures,
        || {
            a_screen_reader_reads_a_page_as_the_panes_group(mtm, &window, frame_rect, dir);
        },
    );

    pane.close();
    assert!(failures.is_empty(), "failed: {failures:?}");
    println!("pencere: ok");
}

/// The owner of a pane that is born over a stream whose terminal does not come (or is refused at
/// once, `refuse`): what the service does for a pane of this Mac, as far as the pane can tell.
struct Born {
    quiet: Quiet,
    refuse: Option<&'static str>,
    found: bool,
    /// The person's Reduce Motion, as the pane is opened with it.
    reduce_motion: bool,
    asks: RefCell<Vec<LocalAsk>>,
    pids: RefCell<Vec<bt_core::ChildPid>>,
    /// How many times the pane asked its stream's owner for an ordinary shell instead.
    chosen: Arc<AtomicUsize>,
}

/// The ops of a stream as its owner gave them, counting what the pane asks of them.
struct Counting {
    inner: Arc<dyn bt_core::PtyOps>,
    chosen: Arc<AtomicUsize>,
}

impl bt_core::PtyOps for Counting {
    fn resize(&self, master: std::os::fd::BorrowedFd<'_>, size: bt_core::PtySize) {
        self.inner.resize(master, size);
    }
    fn hangup(&self, pid: Option<u32>, exit: std::os::fd::BorrowedFd<'_>) {
        self.inner.hangup(pid, exit);
    }
    fn still_waiting(&self) {
        self.inner.still_waiting();
    }
    fn ordinary_instead(&self) {
        self.chosen.fetch_add(1, Ordering::SeqCst);
        self.inner.ordinary_instead();
    }
}

impl Host for Born {
    fn keeps_key(&self, chord: &Chord) -> bool {
        self.quiet.keeps_key(chord)
    }
    fn title_changed(&self, _: u64) {}
    fn shell_exited(&self, _: u64) {}
    fn focused(&self, _: u64) {}
    fn uploads_changed(&self, _: u64) {}
    fn activity_changed(&self, _: u64) {}
    fn notify(&self, _: u64, _: &str, _: &str) {}
    fn post_notices(&self, _: u64, _: Source, _: Vec<String>) {}
    fn questions_changed(&self, _: u64) {}
    fn program_notification(&self, _: u64, _: Option<&str>, _: &str) {}
    fn cover(&self, _: &TerminalPane) -> Option<Rc<dyn Cover>> {
        None
    }
    fn kept_terminal(&self, _: &PaneUuid) -> Option<Shown> {
        None
    }
    fn open_local_terminal(
        &self,
        pane: &PaneUuid,
        launch: &bt_core::ChildLaunch,
        history: u32,
        _program: bool,
    ) -> Option<bt_core::Stream> {
        let (spec, size) = bt_shell_common::kablo::local::new_session(launch, history);
        let (stream, ask) = LocalAsk::new(pane, spec, size, launch.clone(), self.found).ok()?;
        self.pids.borrow_mut().push(stream.pid.clone());
        match self.refuse {
            Some(why) => ask.refuse(why),
            None => self.asks.borrow_mut().push(ask),
        }
        Some(bt_core::Stream {
            ops: Arc::new(Counting {
                inner: Arc::clone(&stream.ops),
                chosen: Arc::clone(&self.chosen),
            }),
            ..stream
        })
    }
}

/// What `pane` draws, with the labels over it, as a PNG: the terminal is the pane's still, and the
/// labels (AppKit's views, drawn beside the frame path) are drawn over it where they stand.
fn picture_of(pane: &TerminalPane, scale: f64) -> Option<Vec<u8>> {
    let size = pane.bounds().size;
    let still = pane.snapshot((size.width, size.height), scale)?;
    let pixels = |points: f64| (points * scale).round() as isize;
    // SAFETY: a bitmap the rep allocates for itself; the colour space is the device's RGB.
    let rep = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            pixels(size.width),
            pixels(size.height),
            8,
            4,
            true,
            false,
            objc2_app_kit::NSDeviceRGBColorSpace,
            0,
            0,
        )
    }?;
    rep.setSize(size);
    let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)?;
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&context));
    let whole = NSRect::new(NSPoint::new(0.0, 0.0), size);
    let terminal = NSImage::initWithCGImage_size(NSImage::alloc(), &still, size);
    terminal.drawInRect(whole);
    let view: &NSView = pane;
    for label in descendants(view.retain()) {
        if label.isHidden()
            || !matches!(
                label.class().name().to_str(),
                Ok("BateriWaitRow" | "BateriJumpButton")
            )
        {
            continue;
        }
        let bounds = label.bounds();
        if let Some(label_rep) = label.bitmapImageRepForCachingDisplayInRect(bounds) {
            label.cacheDisplayInRect_toBitmapImageRep(bounds, &label_rep);
            let image = NSImage::initWithSize(NSImage::alloc(), bounds.size);
            image.addRepresentation(&label_rep);
            image.drawInRect(label.frame());
        }
    }
    NSGraphicsContext::restoreGraphicsState_class();
    // SAFETY: the properties are an empty dictionary, which the type takes for no property.
    let png = unsafe {
        rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    };
    png.map(|png| png.to_vec())
}

/// The bars' heights (parts of the tallest) are these.
fn assert_heights(heights: &[f64], expected: [f64; 3], what: &str) {
    assert!(
        heights.len() == 3
            && heights
                .iter()
                .zip(expected)
                .all(|(a, b)| (a - b).abs() < 1e-3),
        "{what}: {heights:?} is not {expected:?}"
    );
}

/// The heights the bars rest at under Reduce Motion, all different.
const AT_REST: [f64; 3] = [0.5, 1.0, 0.7];

/// Two colours (red, green, blue, alpha) that are the same one.
fn assert_close(left: [f64; 4], right: [f64; 4], what: &str) {
    for (a, b) in left.iter().zip(right) {
        assert!((a - b).abs() < 1e-3, "{what}: {left:?} and {right:?}");
    }
}

/// A window of the test's own kind: borderless, nothing in it.
fn make_window(mtm: MainThreadMarker, frame_rect: NSRect) -> Retained<NSWindow> {
    // SAFETY: with `defer` false the window exists at once; the `Retained` is its owner.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            frame_rect,
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: only changes who owns the window; the `Retained` above does.
    unsafe { window.setReleasedWhenClosed(false) };
    window
}

/// What the bars did in a second and a fifth, as the animation has them: how many different
/// heights the middle one was drawn at, its lowest and highest, the fastest it changed between two
/// looks (parts of the capital a second: a jump is a change faster than the motion goes, however
/// far apart a busy machine puts the looks), and how many looks had the three bars at different
/// heights (they are behind one another). Where the animation plays and the window draws it is
/// smooth and fills the range; where it does not, one height (the test window is never visible,
/// and Core Animation could then not advance it).
struct Swing {
    distinct: usize,
    low: f64,
    high: f64,
    fastest: f64,
    apart: usize,
    looks: usize,
}

/// The fastest a bar's height changes, parts of the capital a second: from a third of it to all
/// of it in half a second, eased in and out (`0.42, 0, 0.58, 1`, whose steepest slope is 1.72 times
/// the even one): 0.7 × 2 × 1.72 = 2.41. A jump of a fifth of the capital between two looks 25 ms
/// apart is eight a second.
const FASTEST_SWING: f64 = 2.42;

fn swing(pane: &TerminalPane) -> Swing {
    let mut seen: Vec<i64> = Vec::new();
    let (mut low, mut high, mut fastest, mut apart, mut looks) =
        (f64::MAX, f64::MIN, 0.0_f64, 0, 0);
    let mut last: Option<(f64, Instant)> = None;
    let until = Instant::now() + Duration::from_millis(1200);
    while Instant::now() < until {
        let bars = pane.wait_bars_drawn();
        let at = Instant::now();
        let middle = bars[1];
        let key = (middle * 1000.0).round() as i64;
        if !seen.contains(&key) {
            seen.push(key);
        }
        low = low.min(middle);
        high = high.max(middle);
        if let Some((before, then)) = last {
            let gap = at.duration_since(then).as_secs_f64();
            if gap > 0.0 {
                fastest = fastest.max((middle - before).abs() / gap);
            }
        }
        last = Some((middle, at));
        looks += 1;
        if (bars[0] - bars[2]).abs() > 0.02 {
            apart += 1;
        }
        spin(0.025);
    }
    Swing {
        distinct: seen.len(),
        low,
        high,
        fastest,
        apart,
        looks,
    }
}

/// Whether two lists of the moments the bars' animations begin are the same: each bar's animation
/// put again begins where the one before it did, and none is missing.
fn same_begins(before: &[Option<f64>], after: &[Option<f64>]) -> bool {
    before.len() == after.len()
        && before
            .iter()
            .zip(after)
            .all(|(before, after)| match (before, after) {
                (Some(before), Some(after)) => (before - after).abs() < 1e-6,
                _ => false,
            })
}

/// Whether every bar's animation goes from its lowest to its highest in the height the bars are
/// now (a third of the capital to all of it).
fn reaches_the_capital(reach: &[Option<(f64, f64)>]) -> bool {
    reach.len() == 3
        && reach.iter().all(|reach| {
            reach.is_some_and(|(low, high)| (low - 0.3).abs() < 0.01 && (high - 1.0).abs() < 0.01)
        })
}

/// Where a click at `point` (in the pane's coordinates) lands, as a view: the name of its class.
fn hit_by_a_click(pane: &TerminalPane, point: NSPoint) -> (String, bool) {
    // SAFETY: the pane is in a window's view tree on the main thread.
    let container = unsafe { pane.superview() }.expect("the pane is in a window");
    let hit = container
        .hitTest(pane.convertPoint_toView(point, Some(&container)))
        .expect("a click on the pane lands on a view");
    let name = hit.class().name().to_string_lossy().into_owned();
    (name, hit.isDescendantOf(pane))
}

/// The middle of a rectangle.
fn centre_of(rect: NSRect) -> NSPoint {
    NSPoint::new(
        rect.origin.x + rect.size.width / 2.0,
        rect.origin.y + rect.size.height / 2.0,
    )
}

/// A pane that is born without its terminal: nothing waits for it; two seconds in it says so with
/// a dim line of plain words in the terminal's font and three bars that rise and fall smoothly, which go
/// when the terminal comes; the bars still move when the pane has been out of its window and back
/// (a tab put in the background, a split, a tab moved to another window), take the colour of the
/// words when the theme changes under them, and stand at different heights under Reduce Motion; ten
/// seconds in it offers an ordinary shell instead, which only a click takes. A pane that is
/// refused its terminal has the line and the bars no more and no mark, and one born where no
/// kablo is expected has neither. Pictures of each, light and dark, go where `BT_PANE_SHOTS`
/// says (the test pane has no title bar and no foot: the pictures that are looked at are of the
/// application's own window).
fn born_without_its_terminal(
    mtm: MainThreadMarker,
    window: &NSWindow,
    frame_rect: NSRect,
    dir: &Path,
) {
    let shots = std::env::var_os("BT_PANE_SHOTS").map(PathBuf::from);
    if let Some(shots) = &shots {
        fs::create_dir_all(shots).expect("the pictures' folder");
    }
    let host = |refuse, found, reduce_motion| {
        Rc::new(Born {
            quiet: Quiet::default(),
            refuse,
            found,
            reduce_motion,
            asks: RefCell::new(Vec::new()),
            pids: RefCell::new(Vec::new()),
            chosen: Arc::new(AtomicUsize::new(0)),
        })
    };
    let open = |id: u64, host: Rc<Born>, theme: Theme| -> Retained<TerminalPane> {
        let folder = dir.join(format!("born-{id}"));
        fs::create_dir_all(folder.join("zdot")).expect("the pane's folder");
        let identity = Rc::new(Identity {
            app_name: "pencere-test".to_owned(),
            helper: None,
            zsh_wrapper_dir: None,
            fish_wrapper_dir: None,
            bash_wrapper_dir: None,
        });
        let reduce_motion = host.reduce_motion;
        let mut config = embed::Config::new(mtm, id, host, identity, Settings::default(), theme);
        config.reduce_motion = reduce_motion;
        config.env = vec![
            (
                "ZDOTDIR".to_owned(),
                folder.join("zdot").display().to_string(),
            ),
            ("PS1".to_owned(), "% ".to_owned()),
        ];
        let pane = embed::open(mtm, frame_rect, config).expect("the pane opens");
        window
            .contentView()
            .expect("a window has a content view")
            .addSubview(&pane);
        pane.observe_frame();
        pane.start(mtm).expect("the pane starts");
        pane
    };
    // The line and its bars are no control: a click on them reaches the pane's own view.
    let is_no_control = |pane: &TerminalPane| {
        let (line, bars, _) = pane.wait_frames();
        for (what, rect) in [("the line", line), ("the bars", bars)] {
            let (name, is_the_panes) = hit_by_a_click(pane, centre_of(rect.expect(what)));
            println!("pencere: a click on {what} reaches {name}");
            assert!(is_the_panes, "{what}: the click is the pane's, not {name}");
            assert!(
                !matches!(
                    name.as_str(),
                    "BateriWaitRow" | "BateriWaitBars" | "NSTextField" | "BateriJumpButton"
                ),
                "{what}: a click on it reached {name}"
            );
        }
    };
    for (look, theme, appearance) in [
        ("dark", Theme::BATERI, "NSAppearanceNameDarkAqua"),
        ("light", Theme::BATERI_LIGHT, "NSAppearanceNameAqua"),
    ] {
        let named = NSString::from_str(appearance);
        window.setAppearance(NSAppearance::appearanceNamed(&named).as_deref());
        let base = 900 + if look == "dark" { 0 } else { 10 };
        // Born without its terminal, a new one: nothing yet, then the line, then nothing again.
        let born = host(None, false, false);
        let started = Instant::now();
        let pane = open(base, born.clone(), theme);
        assert!(started.elapsed() < Duration::from_secs(1), "nothing waited");
        assert_eq!(pane.wait_line_text(), None, "not before two seconds");
        assert!(!pane.wait_bars_turn());
        assert_eq!(
            born.asks.borrow().len(),
            1,
            "the pane asked its owner for a terminal"
        );
        spin_for(2.4);

        assert_eq!(
            pane.wait_line_text().as_deref(),
            Some("Starting the terminal"),
            "plain words, no dots"
        );
        assert!(pane.wait_bars_turn(), "the bars move");
        assert!(pane.wait_bars_play(), "the bars have their animation");
        assert_eq!(pane.ordinary_link_text(), None, "no way out yet");
        is_no_control(&pane);
        // The bars stand in the width of one cell of the font the words are in, and that font has
        // one width for every character.
        let (family, fixed, cell) = pane.wait_font();
        println!("pencere: the words of the wait are in {family}, a cell {cell:.2} points");
        assert!(fixed, "{family} is not a font of one width");
        let (_, bars, _) = pane.wait_frames();
        assert!(
            (bars.expect("the bars").size.width - cell).abs() < 0.01,
            "the bars are a cell wide"
        );
        if let Some(shots) = &shots {
            let png = picture_of(&pane, 2.0).expect("a picture");
            fs::write(shots.join(format!("waiting-new-{look}.png")), png).expect("written");
        }
        // The bars are the words' colour, and follow a theme that changes while the line shows
        // (the window's appearance goes with the theme, as in the application).
        let (bars_colour, words_colour) = pane.wait_colors().expect("colours while it shows");
        assert_close(
            bars_colour,
            words_colour,
            &format!("{look}: bars and words"),
        );
        let other = if theme.is_dark() {
            Theme::BATERI_LIGHT
        } else {
            Theme::BATERI
        };
        let other_appearance = if theme.is_dark() {
            "NSAppearanceNameAqua"
        } else {
            "NSAppearanceNameDarkAqua"
        };
        window.setAppearance(
            NSAppearance::appearanceNamed(&NSString::from_str(other_appearance)).as_deref(),
        );
        pane.set_theme(other);
        spin_for(0.3);
        let (switched, words_switched) = pane.wait_colors().expect("colours while it shows");
        assert_ne!(
            switched, bars_colour,
            "{look}: the bars kept the old colour"
        );
        assert_close(switched, words_switched, &format!("{look}: switched"));
        window.setAppearance(NSAppearance::appearanceNamed(&named).as_deref());
        pane.set_theme(theme);
        spin_for(0.3);
        let (back, words_back) = pane.wait_colors().expect("colours while it shows");
        assert_close(back, bars_colour, &format!("{look}: back"));
        assert_close(back, words_back, &format!("{look}: back, the words"));
        // The pane goes out of its window and comes back, as a tab does that is put in the
        // background, a split, a tab moved to another window: the animation is on every bar
        // where the pane stands in a window, and not where it does not.
        let content = window.contentView().expect("a window has a content view");
        let other_window = make_window(mtm, frame_rect);
        let is_back = |what: &str, began: &[Option<f64>]| {
            spin_for(0.3);
            assert!(pane.wait_bars_turn(), "{what}: the bars are to move");
            assert!(
                pane.wait_bars_play(),
                "{what}: the bars lost their animation"
            );
            // Put again where they would be: the same beginning, so no bar jumps.
            assert!(
                same_begins(began, &pane.wait_bars_begins()),
                "{what}: the bars began again: {began:?} then {:?}",
                pane.wait_bars_begins()
            );
            let swing = swing(&pane);
            println!(
                "pencere: {what}: {} heights, {:.2} to {:.2}, at most {:.2} of the capital a second, apart in {} of {} looks",
                swing.distinct, swing.low, swing.high, swing.fastest, swing.apart, swing.looks
            );
            assert!(
                swing.distinct >= 20,
                "{what}: the bars are not moving smoothly"
            );
            assert!(
                swing.high - swing.low >= 0.5,
                "{what}: the bars do not fall and rise"
            );
            assert!(
                swing.fastest < 2.0 * FASTEST_SWING,
                "{what}: the bars jump: {:.2} of the capital a second",
                swing.fastest
            );
            assert!(
                swing.apart * 10 >= swing.looks * 8,
                "{what}: the bars are not behind one another"
            );
        };
        // Each time the animation is taken off the bars while the pane is out of its place (as a
        // layer may lose it when its window changes), and the pane must put it again.
        let began = pane.wait_bars_begins();
        assert!(began.iter().all(Option::is_some), "{began:?}");
        pane.removeFromSuperview();
        pane.wait_bars_drop_animation();
        spin_for(0.3);
        assert!(!pane.wait_bars_play(), "out of every window nothing plays");
        content.addSubview(&pane);
        is_back("a tab put in the background and chosen again", &began);
        pane.setHidden(true);
        pane.wait_bars_drop_animation();
        spin_for(0.3);
        pane.setHidden(false);
        is_back("a pane hidden and shown", &began);
        let split = NSView::initWithFrame(NSView::alloc(mtm), frame_rect);
        content.addSubview(&split);
        pane.removeFromSuperview();
        pane.wait_bars_drop_animation();
        split.addSubview(&pane);
        is_back("a split", &began);
        pane.removeFromSuperview();
        pane.wait_bars_drop_animation();
        other_window
            .contentView()
            .expect("a window has a content view")
            .addSubview(&pane);
        is_back("a tab moved to another window", &began);
        pane.removeFromSuperview();
        pane.wait_bars_drop_animation();
        split.removeFromSuperview();
        content.addSubview(&pane);
        is_back("and back", &began);
        other_window.close();
        // The font grows while the bars move (View ▸ Bigger): they move in the new height, in
        // their phase; and back.
        assert!(reaches_the_capital(&pane.wait_bars_reach()));
        let (_, small, _) = pane.wait_frames();
        // SAFETY: the pane's own action of View ▸ Bigger, with no sender.
        let _: () = unsafe { msg_send![&*pane, makeFontBigger: None::<&AnyObject>] };
        spin_for(0.2);
        let (_, big, _) = pane.wait_frames();
        assert!(
            big.expect("the bars").size.height > small.expect("the bars").size.height,
            "the font did not grow"
        );
        assert!(
            reaches_the_capital(&pane.wait_bars_reach()),
            "the bars move in the height of the font before: {:?}",
            pane.wait_bars_reach()
        );
        assert!(same_begins(&began, &pane.wait_bars_begins()));
        // SAFETY: the pane's own action of View ▸ Actual Size, with no sender.
        let _: () = unsafe { msg_send![&*pane, resetFontSize: None::<&AnyObject>] };
        spin_for(0.2);
        assert!(reaches_the_capital(&pane.wait_bars_reach()));
        // Reduce Motion, while the line shows: the bars stand at different heights, and move
        // again when the person lets them.
        pane.set_reduce_motion(true);
        assert!(!pane.wait_bars_turn() && !pane.wait_bars_play());
        assert_heights(&pane.wait_bars_rest(), AT_REST, "at rest");
        spin_for(0.5);
        assert_heights(&pane.wait_bars_drawn(), AT_REST, "and they stand");
        pane.set_reduce_motion(false);
        assert!(pane.wait_bars_turn() && pane.wait_bars_play());
        assert!(swing(&pane).distinct >= 20, "and they move again");
        // The bars are Core Animation's and the pane's renderer asks for no frame for them: the
        // application's own CPU while they move is measured in the application (`k8-*`), as the
        // test window is never visible here and a frame count would not move for anything.
        // The terminal comes: the line goes, and the bars stop with it.
        born.pids.borrow()[0].set(4242);
        until("the line to go", || pane.wait_line_text().is_none());
        assert!(!pane.wait_bars_turn(), "the bars stopped with the line");
        assert!(!pane.wait_bars_play());
        assert_eq!(pane.ordinary_link_text(), None);
        pane.close();

        // A pane that finds its terminal again says so in its words.
        let found = host(None, true, false);
        let pane = open(base + 1, found, theme);
        spin_for(2.4);
        assert_eq!(
            pane.wait_line_text().as_deref(),
            Some("Bringing the terminal back")
        );
        if let Some(shots) = &shots {
            let png = picture_of(&pane, 2.0).expect("a picture");
            fs::write(shots.join(format!("waiting-found-{look}.png")), png).expect("written");
        }
        pane.close();

        // A terminal that came before the two seconds are over: the line never shows.
        let quick = host(None, false, false);
        let pane = open(base + 2, quick.clone(), theme);
        quick.pids.borrow()[0].set(4243);
        spin_for(2.4);
        assert_eq!(pane.wait_line_text(), None, "the one-shot found it over");
        assert!(!pane.wait_bars_turn());
        pane.close();

        // Reduce Motion from the start: the line shows and the bars stand at different heights;
        // they move when the person lets them, and stand again, without the pane being made
        // again.
        let reduced = host(None, false, true);
        let pane = open(base + 3, reduced, theme);
        spin_for(2.4);
        assert!(pane.wait_line_text().is_some());
        assert!(!pane.wait_bars_turn(), "Reduce Motion: the bars stand");
        assert!(!pane.wait_bars_play());
        assert_heights(&pane.wait_bars_drawn(), AT_REST, "bars at rest");
        pane.set_reduce_motion(false);
        assert!(
            pane.wait_bars_turn() && pane.wait_bars_play(),
            "Reduce Motion went off"
        );
        pane.set_reduce_motion(true);
        assert!(!pane.wait_bars_turn(), "Reduce Motion came on: they stand");
        assert_heights(&pane.wait_bars_drawn(), AT_REST, "bars at rest");
        pane.close();

        // Refused: an ordinary shell, told in the pane in a dim line, and nothing of the wait
        // is left over it — no line, no bars, no mark.
        let refused = host(
            Some("the service that makes the terminals of this Mac did not start"),
            false,
            false,
        );
        let pane = open(base + 4, refused, theme);
        spin_for(2.4);
        assert_eq!(pane.wait_line_text(), None);
        assert!(!pane.wait_bars_turn());
        assert_eq!(pane.ordinary_link_text(), None);
        if let Some(shots) = &shots {
            let png = picture_of(&pane, 2.0).expect("a picture");
            fs::write(shots.join(format!("refused-{look}.png")), png).expect("written");
        }
        pane.close();
    }

    // Ten seconds in, once: a link beside the line, which only a click takes. Once, in the dark.
    window.setAppearance(
        NSAppearance::appearanceNamed(&NSString::from_str("NSAppearanceNameDarkAqua")).as_deref(),
    );
    let waits = host(None, false, false);
    let pane = open(920, waits.clone(), Theme::BATERI);
    spin_for(9.0);
    assert!(pane.wait_line_text().is_some());
    assert_eq!(pane.ordinary_link_text(), None, "not before ten seconds");
    spin_for(1.6);
    assert_eq!(
        pane.ordinary_link_text().as_deref(),
        Some("Open an ordinary shell instead")
    );
    assert!(pane.wait_line_text().is_some(), "the line is still there");
    // The link is the one hit; the line and the bars are still no control.
    is_no_control(&pane);
    let (_, _, link) = pane.wait_frames();
    let (name, is_the_panes) = hit_by_a_click(&pane, centre_of(link.expect("the link's frame")));
    println!("pencere: a click on the link reaches {name}");
    assert_eq!(name, "BateriJumpButton");
    assert!(is_the_panes);
    // The pane is made narrow, as a split makes it: the line and the link stand in it again — the
    // link under the line, shortened — and nothing of them is past the pane's edge, over the pane
    // beside it. Made wide again, the link is beside the words again.
    let wide = pane.frame().size;
    let narrow = 220.0;
    pane.setFrameSize(NSSize::new(narrow, wide.height));
    spin_for(0.2);
    let (line, _, link) = pane.wait_frames();
    let (line, link) = (line.expect("the line"), link.expect("the link"));
    println!("pencere: in a pane {narrow} wide the line is {line:?} and the link {link:?}");
    assert!(
        line.origin.x + line.size.width <= narrow - 8.0 + 0.5,
        "the line runs past the pane: {line:?}"
    );
    assert!(
        link.origin.x + link.size.width <= narrow - 8.0 + 0.5,
        "the link runs past the pane: {link:?}"
    );
    assert!(
        link.origin.y + link.size.height <= line.origin.y + 0.5,
        "the link is not under the line: {line:?} {link:?}"
    );
    // Too narrow for the whole link: its short words, whole, not the long ones cut mid-action.
    assert_eq!(
        pane.ordinary_link_text().as_deref(),
        Some("Open an ordinary shell"),
        "the narrow pane's link"
    );
    if let Some(shots) = &shots {
        let png = picture_of(&pane, 2.0).expect("a picture");
        fs::write(shots.join("link-narrow-dark.png"), png).expect("written");
    }
    pane.setFrameSize(wide);
    spin_for(0.2);
    let (line, _, link) = pane.wait_frames();
    let (line, link) = (line.expect("the line"), link.expect("the link"));
    assert!(
        link.origin.x >= line.origin.x + line.size.width,
        "the link is not beside the words again: {line:?} {link:?}"
    );
    assert_eq!(
        pane.ordinary_link_text().as_deref(),
        Some("Open an ordinary shell instead"),
        "the wide pane's link has its whole words again"
    );
    // Nothing fell to a shell by itself, by time or by a key.
    assert_eq!(waits.chosen.load(Ordering::SeqCst), 0);
    // The bars move beside the link as they moved without it.
    assert!(pane.wait_bars_play());
    spin_for(1.5);
    // The click: the owner is asked once, and the link goes.
    pane.click_ordinary_link();
    assert_eq!(
        waits.chosen.load(Ordering::SeqCst),
        1,
        "the owner was asked"
    );
    assert_eq!(
        pane.ordinary_link_text(),
        None,
        "the link goes at the click"
    );
    pane.click_ordinary_link();
    assert_eq!(
        waits.chosen.load(Ordering::SeqCst),
        1,
        "a second click asks nothing"
    );
    // The owner did not take the click, as the terminal had come: the pane shows it, its wait is
    // over on its screen before the pid is known, and the line goes.
    let pid = waits.pids.borrow()[0].clone();
    pid.shown();
    until("the line to go as the terminal is shown", || {
        pane.wait_line_text().is_none()
    });
    assert!(!pane.wait_bars_turn());
    // The terminal was not there after all, and another is on its way: the pane waits again and
    // says so as it did when it was born, both clocks from the start — the line two seconds in,
    // the link at ten — and a click on the link is told to the owner again.
    pid.waits_again();
    spin_for(1.0);
    assert_eq!(
        pane.wait_line_text(),
        None,
        "the line not before two seconds"
    );
    spin_for(1.4);
    assert_eq!(
        pane.wait_line_text().as_deref(),
        Some("Starting the terminal"),
        "the line of the wait that began again"
    );
    assert!(pane.wait_bars_play());
    assert_eq!(
        pane.ordinary_link_text(),
        None,
        "the link not before ten seconds"
    );
    spin_for(8.2);
    assert_eq!(
        pane.ordinary_link_text().as_deref(),
        Some("Open an ordinary shell instead"),
        "the link of the wait that began again"
    );
    pane.click_ordinary_link();
    assert_eq!(
        waits.chosen.load(Ordering::SeqCst),
        2,
        "the owner was asked again"
    );
    assert_eq!(pane.ordinary_link_text(), None);
    // The shell is on the stream: the owner settles the wait and the line goes with it.
    waits.pids.borrow()[0].set(4244);
    until("the line to go", || pane.wait_line_text().is_none());
    assert!(!pane.wait_bars_turn());
    pane.close();

    // A terminal that comes at the sixth second: the link is not offered at the tenth.
    let comes = host(None, false, false);
    let pane = open(921, comes.clone(), Theme::BATERI);
    spin_for(6.0);
    assert!(pane.wait_line_text().is_some());
    comes.pids.borrow()[0].set(4245);
    spin_for(5.0);
    assert_eq!(pane.wait_line_text(), None);
    assert_eq!(pane.ordinary_link_text(), None, "the terminal came first");
    pane.close();
    window.setAppearance(None);
}

/// A pane whose terminal a server keeps, with a program's view over it, as its connection goes and
/// comes back: the view stays under the veil, the program is offered it again only when the server
/// has said a program is there, an offer that goes nowhere is made again, and a program that ended
/// while the connection was gone loses its view with nothing written.
fn connection_goes_and_comes_back(
    mtm: MainThreadMarker,
    window: &NSWindow,
    frame_rect: NSRect,
    dir: &Path,
) {
    let host = Rc::new(Quiet::default());
    let (pane, mut program) = open_hosted_pane(
        mtm,
        window,
        frame_rect,
        &dir.join("connection"),
        PANE + 40,
        Rc::clone(&host),
        Settings::default(),
    );
    let say = |front: &str, link: PaneLink, program_fresh: bool| {
        host.1.replace(Some(Shown {
            account: "u@rasp:22".to_owned(),
            host: "rasp".to_owned(),
            argv: vec!["ssh".to_owned(), "rasp".to_owned()],
            terminal: SessionId::new("t1").expect("an id"),
            program: Some(front.to_owned()),
            program_fresh,
            link,
            taken_by: None,
        }));
    };
    let goes = |front: &str| {
        say(front, PaneLink::Reconnecting, false);
        pane.set_connection(PaneLink::Reconnecting);
    };
    let token_of = |offer: &str| {
        assert!(offer.contains("\"restart\":true"), "{offer}");
        offer
            .split("\"token\":\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .expect("a token in the offer")
            .to_owned()
    };
    say("pen", PaneLink::Live, true);
    program.say(&format!("{ESC}[?1049h"));
    let files = [
        ("index.html", "text/html", PAGE.as_bytes()),
        ("app.js", "text/javascript", SCRIPT.as_bytes()),
        ("other.html", "text/html", OTHER.as_bytes()),
    ];
    let token = program.hello();
    program.open(&token, &files, None);
    program.supply(&files);
    watch_it_start(&pane, &mut program);

    // The connection goes: nothing is written and the view stays, under the veil.
    goes("pen");
    spin(0.3);
    assert!(covered(&pane), "the view stays while the terminal is away");

    // It is back and the server has not said what is in front: a program may have ended, and an
    // offer now would be typed at the shell.
    say("pen", PaneLink::Live, false);
    pane.set_connection(PaneLink::Live);
    spin(0.3);
    assert!(covered(&pane), "the view waits for the server's word");
    assert!(!program.heard()[program.read..].contains("pencere;welcome"));

    // The server says a program is there: the offer goes, and it is the one way back.
    say("pen", PaneLink::Live, true);
    pane.server_program_changed();
    let first = token_of(&program.wait_for("pencere;welcome"));
    until("the view to go for the restart", || !covered(&pane));

    // The connection goes before the program answers: the offer may have gone nowhere, and it is
    // made again when the terminal is back.
    goes("pen");
    say("pen", PaneLink::Live, true);
    pane.set_connection(PaneLink::Live);
    let second = token_of(&program.wait_for("pencere;welcome"));
    assert_ne!(first, second, "a new offer has a new token");
    program.open(&second, &files, None);
    program.supply(&files);
    watch_it_start(&pane, &mut program);

    // The program ends while the connection is gone: its view goes and the shell is told nothing.
    goes("pen");
    say("bash", PaneLink::Live, true);
    pane.set_connection(PaneLink::Live);
    until("the view of the program that went", || !covered(&pane));
    spin(0.3);
    let after = &program.heard()[program.read..];
    assert!(!after.contains("pencere;welcome"), "{after}");
    assert!(!after.contains("pencere;closed"), "{after}");
    pane.close();
}

/// Runs `body`; a panic is recorded under `name` (its message was printed as it happened) and the
/// run goes on to the next section.
fn section(name: &str, failures: &mut Vec<String>, body: impl FnOnce()) {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)).is_err() {
        failures.push(name.to_owned());
    }
}

/// A UDP socket on this Mac that counts the datagrams it gets: where a peer connection's STUN
/// request would arrive. It tries itself once (a datagram from the test) and counts only what comes
/// after.
struct Datagrams {
    port: u16,
    seen: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Datagrams {
    fn start() -> Self {
        let socket = UdpSocket::bind("127.0.0.1:0").expect("a UDP port on this Mac");
        socket
            .set_read_timeout(Some(Duration::from_millis(20)))
            .expect("a socket that does not wait for ever");
        let port = socket.local_addr().expect("an address").port();
        let seen = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = thread::spawn({
            let (seen, stop) = (Arc::clone(&seen), Arc::clone(&stop));
            move || {
                let mut buffer = [0u8; 2048];
                while !stop.load(Ordering::Acquire) {
                    if socket.recv_from(&mut buffer).is_ok() {
                        seen.fetch_add(1, Ordering::AcqRel);
                    }
                }
            }
        });
        let this = Self {
            port,
            seen,
            stop,
            thread: Some(thread),
        };
        // The listener hears: a datagram from here is counted, and then forgotten.
        UdpSocket::bind("127.0.0.1:0")
            .and_then(|sender| sender.send_to(b"self-test", ("127.0.0.1", port)))
            .expect("a datagram to the listener");
        let deadline = Instant::now() + Duration::from_secs(5);
        while this.seen.load(Ordering::Acquire) == 0 {
            assert!(Instant::now() < deadline, "the UDP listener hears nothing");
            thread::sleep(Duration::from_millis(10));
        }
        this.seen.store(0, Ordering::Release);
        this
    }

    fn count(&self) -> usize {
        self.seen.load(Ordering::Acquire)
    }
}

impl Drop for Datagrams {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A page that tries what the engine opens beside a policy once it is told its `context`: a peer
/// connection with a STUN server (UDP) and a TURN server (TCP) on this Mac, a preconnect and a
/// prefetch of two origins on this Mac, and a fetch of a third, the only one a person may allow.
/// It tells the program what it could make and what the engine said.
const CHANNELS_SCRIPT: &str = "var send=function(m){window.parent.postMessage(m,'*')};\
var started=false;\
window.addEventListener('message',function(e){\
if(e.source!==window.parent)return;var m=e.data;\
if(!m.context||started)return;started=true;\
var report={rtc:typeof RTCPeerConnection,prefixed:typeof webkitRTCPeerConnection};\
try{var pc=new RTCPeerConnection({iceServers:[{urls:'stun:127.0.0.1:__STUN__'},\
{urls:'turn:127.0.0.1:__TURN__?transport=tcp',username:'u',credential:'c'}]});\
pc.createDataChannel('x');\
pc.createOffer().then(function(o){return pc.setLocalDescription(o)}).catch(function(x){report.offer=String(x)});\
report.made=true}catch(x){report.threw=String(x)}\
var link=function(rel,href){var l=document.createElement('link');l.rel=rel;l.href=href;document.head.appendChild(l)};\
link('preconnect','http://127.0.0.1:__PRECONNECT__/');\
link('prefetch','http://127.0.0.1:__PREFETCH__/x');\
fetch('http://127.0.0.1:__CONTROL__/control').catch(function(){});\
send({call:{id:1,method:'hello',params:report}})});";

/// The engine's channels under a page: it reaches no listener of this Mac by a peer connection, a
/// preconnect or a prefetch, whether the person allowed it nothing or one origin, and the origin it
/// was allowed (the control) does answer its fetch, so that the listeners are known to be reachable.
fn engine_channels(mtm: MainThreadMarker, window: &NSWindow, frame_rect: NSRect, dir: &Path) {
    let (pane, mut program) = open_pane(mtm, window, frame_rect, &dir.join("channels"), PANE + 2);
    program.say(&format!("{ESC}[?1049h"));
    let stun = Datagrams::start();
    let turn = Local::start();
    let preconnect = Local::start();
    let prefetch = Local::start();
    let control = Local::start();
    let port = |local: &Local| {
        local
            .origin
            .rsplit(':')
            .next()
            .unwrap_or_default()
            .to_owned()
    };
    let script = CHANNELS_SCRIPT
        .replace("__STUN__", &stun.port.to_string())
        .replace("__TURN__", &port(&turn))
        .replace("__PRECONNECT__", &port(&preconnect))
        .replace("__PREFETCH__", &port(&prefetch))
        .replace("__CONTROL__", &port(&control));
    let files = [
        ("index.html", "text/html", PAGE.as_bytes()),
        ("app.js", "text/javascript", script.as_bytes()),
    ];
    let reached = || {
        format!(
            "stun udp {}, turn tcp {}, preconnect {}, prefetch {}, control {}",
            stun.count(),
            turn.hits(),
            preconnect.hits(),
            prefetch.hits(),
            control.hits()
        )
    };

    for allowed in [None, Some(control.origin.clone())] {
        let token = program.hello();
        program.open(&token, &files, allowed.as_deref());
        program.supply(&files);
        if allowed.is_some() {
            // The question of the address: the person says yes.
            let sheet = sheet_of(window);
            press(window, &sheet, "Allow");
        }
        let said = watch_it_start(&pane, &mut program);
        // ICE gathering and the engine's speculation take a few seconds; the allowed fetch is
        // the sign that the page's script ran to its end and that the listeners are reachable.
        if allowed.is_some() {
            until("the allowed origin to be asked", || control.hits() > 0);
        }
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            spin(0.1);
        }
        eprintln!(
            "pencere: channels, {} allowed: the page said {said}; reached: {}",
            if allowed.is_some() {
                "one origin"
            } else {
                "nothing"
            },
            reached()
        );
        program.bye(&token);
        until("the view to go", || !covered(&pane));
    }
    assert!(control.hits() > 0, "the control origin was never asked");
    assert_eq!(
        (
            stun.count(),
            turn.hits(),
            preconnect.hits(),
            prefetch.hits()
        ),
        (0, 0, 0, 0),
        "the engine opened a channel beside the policy: {}",
        reached()
    );
    pane.close();
}

/// A script that, once told its `context`, asks the page's font set to load each family it was
/// given as the page would ask for it (regular, italic and bold) and tells the program how the
/// set stands: how many faces it holds, how many loads came back with every face loaded, and which
/// did not. One short string (the host's frame is read as it arrives, and a long one is read in
/// pieces), so that how the host writes the call back does not matter either.
const FONTS_SCRIPT: &str = "var send=function(m){window.parent.postMessage(m,'*')};\
var started=false;\
window.addEventListener('message',function(e){\
if(e.source!==window.parent)return;var m=e.data;\
if(!m.context||started)return;started=true;\
var families=__FAMILIES__;var styles=['16px','italic 16px','bold 16px'];\
var ok=0;var bad=[];var jobs=[];\
families.forEach(function(f){styles.forEach(function(s){\
jobs.push(document.fonts.load(s+' \"'+f+'\"').then(function(l){\
if(l.length&&l.every(function(x){return x.status==='loaded'}))ok++;\
else bad.push(f+'|'+s+'='+l.map(function(x){return x.status}).join('/'))},\
function(x){bad.push(f+'|'+s+'=error')}))})});\
Promise.all(jobs).then(function(){\
send({call:{id:1,method:'hello',params:{r:'size='+document.fonts.size+';ok='+ok+';bad='+bad.join(',')}}})})});";

/// The page of a program's view is given the app's own fonts (the web engine is another process and
/// never sees the ones this process registered): a family the machine does not have loads in the
/// page, from the view's own origin, in each style a page asks for, and the page's font set holds
/// exactly the faces of the files that were registered — none for a family the machine had.
/// Nothing is asked of the grid: every family the pass registered is, by definition, one this Mac
/// does not have, so a face that loads here came through the route and nowhere else.
fn app_fonts_in_the_page(mtm: MainThreadMarker, window: &NSWindow, frame_rect: NSRect, dir: &Path) {
    // What a launch does, in this process: the pane's page is given what it registered.
    bt_shell_common::font_options::register_app_fonts();
    let app = bt_gpu::app_fonts().expect("the pass ran");
    let mut families: Vec<&str> = app.faces.iter().map(|face| face.family.as_str()).collect();
    families.dedup();
    if families.is_empty() {
        println!(
            "pencere: app fonts SKIPPED (this Mac has every family the app ships: kept {:?})",
            app.kept
        );
        return;
    }
    let list = format!(
        "[{}]",
        families
            .iter()
            .map(|family| format!("\"{family}\""))
            .collect::<Vec<_>>()
            .join(",")
    );
    let script = FONTS_SCRIPT.replace("__FAMILIES__", &list);
    let files = [
        ("index.html", "text/html", PAGE.as_bytes()),
        ("app.js", "text/javascript", script.as_bytes()),
    ];
    let (pane, mut program) = open_pane(mtm, window, frame_rect, &dir.join("fonts"), PANE + 5);
    program.say(&format!("{ESC}[?1049h"));
    let token = program.hello();
    program.open(&token, &files, None);
    program.supply(&files);
    let said = watch_it_start(&pane, &mut program);
    let report = said
        .split("\"r\":\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap_or_else(|| panic!("the page's report: {said}"));
    eprintln!("pencere: app fonts: {report} ({} families)", families.len());
    // One face for each file that was registered and no other (none for a family the machine
    // had), and every family loaded in each of the three styles a page asks for.
    assert_eq!(
        report,
        format!("size={};ok={};bad=", app.faces.len(), families.len() * 3),
        "{said}"
    );
    program.bye(&token);
    until("the view to go", || !covered(&pane));
    pane.close();
}

/// The page an agent reads and drives: a heading, a button that counts, a text field that is
/// echoed, a password field, a box that scrolls and a line for the last key the page heard. Its
/// script is the page's own, with what it can see of the agent's world (nothing) in its first call.
const AGENT_PAGE: &str = "<!doctype html><html><head><meta charset=utf-8><title>agent</title></head>\
<body><h1>Agent page</h1><p id=count>count 0</p><button id=inc>Increment</button>\
<label>Name <input id=name></label><label>Secret <input id=pw type=password></label>\
<p id=echo>echo -</p><p id=keyed>keyed -</p><p id=copied>copied -</p>\
<button id=cp>Copy it</button><button id=up>Upload it</button><input id=file type=file hidden>\
<div id=list><div id=tall>tall box</div></div><div id=many></div><script src=app.js></script></body></html>";

const AGENT_SCRIPT: &str = "var send=function(m){window.parent.postMessage(m,'*')};\
var n=0,started=false,$=function(id){return document.getElementById(id)};\
$('list').style.height='60px';$('list').style.overflow='auto';$('tall').style.height='600px';\
var many=$('many');many.style.display='flex';many.style.flexWrap='wrap';many.style.width='400px';\
many.style.height='20px';many.style.overflow='hidden';\
for(var i=0;i<1200;i++){var f=document.createElement('input');f.style.width='2px';f.style.height='2px';\
f.style.padding='0';f.style.border='0';f.style.flex='none';many.appendChild(f)}\
$('inc').addEventListener('click',function(){n++;$('count').textContent='count '+n});\
$('name').addEventListener('input',function(e){$('echo').textContent='echo '+e.target.value});\
document.addEventListener('keydown',function(e){$('keyed').textContent='keyed '+e.key});\
$('cp').addEventListener('click',function(){var r=document.createRange();r.selectNodeContents($('echo'));\
var g=getSelection();g.removeAllRanges();g.addRange(r);var a=false;try{a=document.execCommand('copy')}catch(e){}\
$('copied').textContent='copied exec '+a;\
try{navigator.clipboard.writeText('agent-copy-marker').then(function(){$('copied').textContent+=' clip yes'},\
function(e){$('copied').textContent+=' clip '+e.name})}catch(e){$('copied').textContent+=' clip none'}});\
$('up').addEventListener('click',function(){$('file').click()});\
window.addEventListener('message',function(e){\
if(e.source!==window.parent)return;var m=e.data;\
if(m.context){if(started)return;started=true;\
var h=window.webkit&&window.webkit.messageHandlers;\
send({call:{id:1,method:'hello',params:{agent:typeof window.__bateriAgent,\
handler:typeof(h&&h.bateriAgent),handlers:h?Object.keys(h).join(','):'none'}}})}});";

/// An agent's request on the web view of `pane`, carried out as the app carries it out: begun, and
/// asked again between turns of the main loop until it is answered.
fn view_of(pane: &TerminalPane, action: Action, format: Format) -> Reply {
    let view = View {
        terminal: None,
        format,
        action,
    };
    let mut run = match pane.web_run(&view, "p9", Some("agent-test")) {
        Some(Ok(run)) => run,
        Some(Err(failure)) => return Reply::Failed(failure),
        None => panic!("the pane shows no web view"),
    };
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match run.step() {
            Step::Answer(reply) => return reply,
            Step::Shot { .. } => panic!("a web view's picture is made by its own run"),
            Step::Pending => {
                assert!(Instant::now() < deadline, "the request was never answered");
                spin(0.03);
            }
        }
    }
}

/// Requests on several panes at once, as the app carries them out when they overlap: all begun, and
/// each asked again between turns of the main loop until it is answered.
fn together(requests: Vec<(&TerminalPane, Action)>) -> Vec<Reply> {
    let mut runs: Vec<_> = requests
        .into_iter()
        .map(|(pane, action)| {
            let view = View {
                terminal: None,
                format: Format::Text,
                action,
            };
            match pane.web_run(&view, "p9", Some("agent-test")) {
                Some(Ok(run)) => run,
                _ => panic!("no web view to begin on"),
            }
        })
        .collect();
    let mut replies: Vec<Option<Reply>> = runs.iter().map(|_| None).collect();
    let deadline = Instant::now() + Duration::from_secs(30);
    while replies.iter().any(Option::is_none) {
        for (run, reply) in runs.iter_mut().zip(replies.iter_mut()) {
            if reply.is_none() {
                match run.step() {
                    Step::Answer(answer) => *reply = Some(answer),
                    Step::Pending => {}
                    Step::Shot { .. } => panic!("a web view's picture is made by its own run"),
                }
            }
        }
        assert!(
            Instant::now() < deadline,
            "the requests were never answered"
        );
        spin(0.03);
    }
    replies.into_iter().flatten().collect()
}

fn said(pane: &TerminalPane, action: Action) -> String {
    match view_of(pane, action, Format::Text) {
        Reply::Text(text) => text,
        other => panic!("not an answer: {other:?}"),
    }
}

fn refused(pane: &TerminalPane, action: Action) -> String {
    match view_of(pane, action, Format::Text) {
        Reply::Failed(failure) => failure.text,
        other => panic!("not a refusal: {other:?}"),
    }
}

/// The word that names an element in an answer: the first of the line that has `words`.
fn ref_of(answer: &str, words: &str) -> String {
    answer
        .lines()
        .find(|line| line.contains(words))
        .and_then(|line| line.split_whitespace().next())
        .unwrap_or_else(|| panic!("no line with {words} in\n{answer}"))
        .trim_end_matches('!')
        .to_owned()
}

/// The size a PNG says it is.
fn png_size(bytes: &[u8]) -> (u32, u32) {
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "not a PNG");
    let at = |from: usize| {
        u32::from_be_bytes([
            bytes[from],
            bytes[from + 1],
            bytes[from + 2],
            bytes[from + 3],
        ])
    };
    (at(16), at(20))
}

/// The person's general pasteboard as this process found it, written back when it is dropped if it
/// changed meanwhile (a failed assertion included): a web view writes only the general one, so the check that an agent's
/// click leaves it alone has to look at that one, and must leave the developer's clipboard as it was
/// whatever came of the check.
struct PasteboardAsFound {
    board: Retained<NSPasteboard>,
    /// Its change count then: it is written back only if it changed since.
    count: isize,
    items: Vec<Vec<(Retained<NSString>, Retained<NSData>)>>,
}

impl PasteboardAsFound {
    fn take() -> Self {
        let board = NSPasteboard::generalPasteboard();
        let mut items = Vec::new();
        if let Some(found) = board.pasteboardItems() {
            for item in &found {
                let mut kept = Vec::new();
                for kind in &item.types() {
                    if let Some(data) = item.dataForType(&kind) {
                        kept.push((kind.clone(), data));
                    }
                }
                items.push(kept);
            }
        }
        let count = board.changeCount();
        Self {
            board,
            count,
            items,
        }
    }

    /// The text a paste would give.
    fn text(&self) -> Option<String> {
        // SAFETY: a static of AppKit's, read on the main thread.
        let kind = unsafe { NSPasteboardTypeString };
        self.board.stringForType(kind).map(|text| text.to_string())
    }
}

impl Drop for PasteboardAsFound {
    fn drop(&mut self) {
        // A pasteboard that was not written since is left as it is: writing it back would drop what
        // could not be read and bump its count for nothing.
        if self.board.changeCount() == self.count {
            return;
        }
        self.board.clearContents();
        let items: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = self
            .items
            .iter()
            .map(|kept| {
                let item = NSPasteboardItem::new();
                for (kind, data) in kept {
                    item.setData_forType(data, kind);
                }
                ProtocolObject::from_retained(item)
            })
            .collect();
        if !items.is_empty() {
            self.board
                .writeObjects(&NSArray::from_retained_slice(&items));
        }
    }
}

/// An agent in a program's web view, in a pane on a window nobody sees (the window of this test is
/// never put in front, so the pane is one that is not on the screen): the page is read with a ref
/// for each element it can be acted on by, and clicked, filled, typed into, pressed keys at, scrolled
/// and waited on, and a picture of it is a PNG, with nothing of the window's keyboard or the
/// pasteboard moved. The script lives in a world of its own: the page's own script sees neither
/// it nor its handler.
fn agent_in_a_web_view(mtm: MainThreadMarker, window: &NSWindow, frame_rect: NSRect, dir: &Path) {
    let files = [
        ("index.html", "text/html", AGENT_PAGE.as_bytes()),
        ("app.js", "text/javascript", AGENT_SCRIPT.as_bytes()),
    ];
    let (pane, mut program) = open_pane(mtm, window, frame_rect, &dir.join("agent"), PANE + 6);
    program.say(&format!("{ESC}[?1049h"));
    let token = program.hello();
    program.open(&token, &files, None);
    program.supply(&files);
    let hello = watch_it_start(&pane, &mut program);
    // **The world of its own.** The page's script looks for the agent's object and its handler and
    // finds neither (a page's `webkit.messageHandlers` has the bridge's two and not the agent's).
    assert!(hello.contains("\"agent\":\"undefined\""), "{hello}");
    assert!(hello.contains("\"handler\":\"undefined\""), "{hello}");
    assert!(
        !hello.contains("bateriAgent"),
        "the page can see the agent's handler: {hello}"
    );

    // The pasteboard and the window's keyboard are where they were.
    let keyboard = window.firstResponder().map(|responder| {
        let pointer: *const AnyObject = &*responder as *const _ as *const AnyObject;
        pointer
    });

    // **The reading.** A ref for each element that can be acted on, the field's text and no
    // password.
    let read = said(&pane, Action::Read);
    assert!(
        read.starts_with("pane p9 (agent-test)\nfocus: page\n"),
        "{read}"
    );
    for needle in [
        "heading1 \"Agent page\"",
        "button \"Increment\"",
        "text \"count 0\"",
        "field \"Name\"",
        "field (secret) \"Secret\"",
        "scroll 0% (at the top)",
        "note: this is a web view",
    ] {
        assert!(read.contains(needle), "{needle:?} is not in:\n{read}");
    }
    let button = ref_of(&read, "button \"Increment\"");
    let name = ref_of(&read, "field \"Name\"");
    let secret = ref_of(&read, "field (secret)");
    let list = ref_of(&read, "scroll 0%");
    assert_eq!(button.len(), 6, "{button}");
    // The same ref names the same element once the page has changed.
    let json = match view_of(&pane, Action::Read, Format::Json) {
        Reply::Text(text) => text,
        other => panic!("{other:?}"),
    };
    assert!(
        json.starts_with("{\"pane\":\"p9\",\"program\":\"agent-test\","),
        "{json}"
    );
    assert!(json.contains(&button), "{json}");

    // **Click**, the page's own events: the page counts it.
    let clicked = said(&pane, Action::Click(button.clone()));
    assert!(
        clicked.starts_with(&format!("clicked button \"Increment\" ({button})\n")),
        "{clicked}"
    );
    assert!(clicked.contains("text \"count 1\""), "{clicked}");
    let again = said(&pane, Action::DoubleClick(button.to_uppercase()));
    assert!(
        again.contains("text \"count 3\""),
        "a double click is two presses: {again}"
    );

    // **Fill**: the field's value and the page's own handler, and the keyboard is in it.
    let filled = said(
        &pane,
        Action::Fill {
            target: name.clone(),
            text: "docker".into(),
        },
    );
    assert!(filled.contains("filled field \"Name\""), "{filled}");
    assert!(filled.contains("with 6 characters"), "{filled}");
    assert!(filled.contains("text \"echo docker\""), "{filled}");
    assert!(filled.contains("= \"docker\""), "{filled}");
    assert!(filled.contains(&format!("focus: field {name}")), "{filled}");
    // **Type** goes where the keyboard is; **press** a key the page takes.
    let typed = said(&pane, Action::Type("er".into()));
    assert!(
        typed.contains("typed 2 characters into the field"),
        "{typed}"
    );
    assert!(typed.contains("text \"echo dockerer\""), "{typed}");
    let pressed = said(&pane, Action::Press(vec!["f5".into()]));
    assert!(pressed.contains("pressed f5"), "{pressed}");
    assert!(pressed.contains("text \"keyed F5\""), "{pressed}");
    // Keys that are none are refused before any is pressed.
    let bad = refused(
        &pane,
        Action::Press(vec!["a".into(), "cmd+nonsense+".into()]),
    );
    assert!(bad.contains("is not a key"), "{bad}");

    // **The password**: filled and never said.
    let hidden = said(
        &pane,
        Action::Fill {
            target: secret.clone(),
            text: "s3cret!".into(),
        },
    );
    assert!(hidden.contains("with 7 characters"), "{hidden}");
    assert!(!hidden.contains("s3cret"), "{hidden}");
    let after = said(&pane, Action::Read);
    assert!(!after.contains("s3cret"), "{after}");

    // **Scroll** a box by screens and to its ends; what does not scroll says so.
    let down = said(
        &pane,
        Action::Scroll {
            target: list.clone(),
            way: Way::Down,
            count: Some(1),
        },
    );
    assert!(down.contains("down 1 screens"), "{down}");
    assert!(
        !down.contains("scroll 0% (at the top)"),
        "it did not move: {down}"
    );
    let bottom = said(
        &pane,
        Action::Scroll {
            target: list.clone(),
            way: Way::Bottom,
            count: None,
        },
    );
    assert!(bottom.contains("scroll 100% (at the bottom)"), "{bottom}");
    let top = said(
        &pane,
        Action::Scroll {
            target: list,
            way: Way::Top,
            count: None,
        },
    );
    assert!(top.contains("scroll 0% (at the top)"), "{top}");
    let not = refused(
        &pane,
        Action::Scroll {
            target: button.clone(),
            way: Way::Down,
            count: None,
        },
    );
    assert!(not.contains("does not scroll"), "{not}");

    // **What a page does to the person's things.** A Copy button and an Upload button that opens the
    // file input: an agent's click leaves the pasteboard where it was and no Open sheet on the window.
    let found = PasteboardAsFound::take();
    let text_before = found.text();
    let copy = ref_of(&said(&pane, Action::Read), "button \"Copy it\"");
    let upload = ref_of(&said(&pane, Action::Read), "button \"Upload it\"");
    let copied = said(&pane, Action::Click(copy.clone()));
    assert!(
        copied.contains("text \"copied exec true"),
        "the page's Copy did not run, so nothing was tried: {copied}"
    );
    assert!(
        copied.contains("the page wrote text to the pasteboard: it was put back as it was"),
        "{copied}"
    );
    spin(0.3);
    let upped = said(&pane, Action::Click(upload));
    spin(0.5);
    assert!(upped.contains("clicked button \"Upload it\""), "{upped}");
    eprintln!(
        "pencere: agent in a web view: the file panel was {}",
        if upped.contains("a file panel was asked for and turned away") {
            "asked for and turned away"
        } else {
            "never asked for"
        }
    );
    assert_eq!(
        found.text(),
        text_before,
        "an agent's click left the person's pasteboard changed"
    );
    assert!(
        window.attachedSheet().is_none(),
        "an agent's click opened a file panel on the person's window"
    );

    // **Two requests at once**, on two panes whose pages both copy: the pasteboard is kept once, and
    // what the first page wrote is not kept as the person's by the second.
    let files_two = [
        ("index.html", "text/html", AGENT_PAGE.as_bytes()),
        ("app.js", "text/javascript", AGENT_SCRIPT.as_bytes()),
    ];
    let (pane_two, mut program_two) =
        open_pane(mtm, window, frame_rect, &dir.join("agent-two"), PANE + 7);
    program_two.say(&format!("{ESC}[?1049h"));
    let token_two = program_two.hello();
    program_two.open(&token_two, &files_two, None);
    program_two.supply(&files_two);
    watch_it_start(&pane_two, &mut program_two);
    let copy_two = ref_of(&said(&pane_two, Action::Read), "button \"Copy it\"");
    let both = together(vec![
        (&pane, Action::Click(copy)),
        (&pane_two, Action::Click(copy_two)),
    ]);
    let both: Vec<String> = both
        .into_iter()
        .map(|reply| match reply {
            Reply::Text(text) => text,
            other => panic!("not an answer: {other:?}"),
        })
        .collect();
    assert_eq!(
        both.iter()
            .filter(|text| text.contains("it was put back as it was"))
            .count(),
        1,
        "the last to end puts it back, once: {both:?}"
    );
    assert!(
        both.iter().all(|text| !text.contains("was not put back")),
        "{both:?}"
    );
    assert_eq!(
        found.text(),
        text_before,
        "two requests that overlapped left the person's pasteboard changed"
    );
    program_two.bye(&token_two);
    until("the second view to go", || !covered(&pane_two));
    pane_two.close();
    drop(found);

    // **A page larger than a reading lists** (a table of a thousand inputs): the fields are told of
    // to a most, and the reading says it is cut.
    let big = said(&pane, Action::Read);
    let fields = big
        .lines()
        .filter(|line| line.split_whitespace().nth(1) == Some("field"))
        .count();
    assert!(fields <= 200, "{fields} fields in\n{big}");
    assert!(fields >= 150, "the fields of the page came to {fields}");
    assert!(
        big.contains("note: the page is larger than a reading lists"),
        "{big}"
    );
    assert!(
        big.contains("field \"Name\""),
        "the first fields stay: {big}"
    );

    // **Wait** for words that show, words that go, and the time that runs out.
    let shows = said(
        &pane,
        Action::Wait {
            text: "count 3".into(),
            gone: false,
            seconds: None,
        },
    );
    assert!(shows.starts_with("pane p9"), "{shows}");
    let late = refused(
        &pane,
        Action::Wait {
            text: "count 9".into(),
            gone: false,
            seconds: Some(1),
        },
    );
    assert!(
        late.contains("timed out after 1 seconds waiting for \"count 9\" to show"),
        "{late}"
    );

    // What a web view does not have is told.
    for (action, word) in [
        (Action::Command("x".into()), "commands"),
        (
            Action::CommandOn {
                target: button.clone(),
                title: "x".into(),
            },
            "commands",
        ),
        (Action::Menu(button.clone()), "menu"),
        (Action::Text(name.clone()), "`text`"),
    ] {
        let told = refused(&pane, action);
        assert!(
            told.contains("a web view has no") && told.contains(word),
            "{told}"
        );
    }
    let nothing = refused(&pane, Action::Click("zzzzzz".into()));
    assert!(nothing.contains("no element"), "{nothing}");

    // **The picture**: a PNG of the view, brought down to 1280 pixels on its long edge unless the
    // pane's own size is asked for.
    let picture = match view_of(&pane, Action::Shot { full: false }, Format::Text) {
        Reply::Picture(bytes) => bytes,
        other => panic!("not a picture: {other:?}"),
    };
    let (width, height) = png_size(&picture);
    assert!(
        width > 0 && height > 0 && width.max(height) <= 1280,
        "{width}x{height}"
    );
    let whole = match view_of(&pane, Action::Shot { full: true }, Format::Text) {
        Reply::Picture(bytes) => bytes,
        other => panic!("not a picture: {other:?}"),
    };
    let (full_width, full_height) = png_size(&whole);
    assert!(
        full_width >= width && full_height >= height,
        "{full_width}x{full_height}"
    );

    eprintln!(
        "pencere: agent in a web view: read {} lines, picture {width}x{height} ({} bytes), whole {full_width}x{full_height}",
        read.lines().count(),
        picture.len()
    );
    // None of it reached the window: the keyboard is where it was.
    let now = window.firstResponder().map(|responder| {
        let pointer: *const AnyObject = &*responder as *const _ as *const AnyObject;
        pointer
    });
    assert_eq!(keyboard, now, "an agent moved the window's keyboard");

    program.bye(&token);
    until("the view to go", || !covered(&pane));
    pane.close();
}

/// A page of what a screen reader reads as a tree: a list with a link, a table with a sorted header and
/// two rows of a long one, tabs, a box that is half checked, and a dialog that says it is modal, held
/// by a scrim that answers a click, shown by a button. Its script is the page's own.
const TREE_PAGE: &str = "<!doctype html><html><head><meta charset=utf-8><title>tree</title></head>\
<body><h1>Tree page</h1><button id=open>Open settings</button>\
<ul aria-label=\"Files\"><li><a href=\"#a\">alpha</a></li><li>beta</li></ul>\
<table aria-label=\"Processes\" aria-rowcount=\"200\"><thead><tr><th aria-sort=\"ascending\">CPU</th><th>Name</th></tr></thead>\
<tbody><tr aria-rowindex=\"5\"><td>12</td><td>node</td></tr><tr><td>3</td><td>bash</td></tr></tbody></table>\
<div role=tablist aria-label=\"Views\"><button role=tab aria-selected=true>One</button><button role=tab>Two</button></div>\
<div role=checkbox aria-checked=mixed aria-label=\"All\" tabindex=0></div>\
<div role=grid aria-label=\"Board\"><div role=row><span role=columnheader>Lane</span><span role=columnheader>Cards</span></div>\
<div role=row aria-rowindex=7><span role=gridcell>Todo</span><span role=gridcell>nine</span></div></div>\
<div role=listbox aria-label=\"Library\"><div role=option aria-posinset=55 aria-setsize=1000>fifty-five</div>\
<div role=option aria-posinset=56 aria-setsize=1000>fifty-six</div></div>\
<div role=tree aria-label=\"Outline\"><div role=treeitem aria-expanded=true>top<div role=group>\
<div role=treeitem>leaf</div></div></div><div role=treeitem aria-expanded=false>closed<div role=group style=\"display:none\">\
<div role=treeitem>secret</div></div></div><div role=treeitem>last</div></div>\
<div id=scrim style=\"display:none;position:fixed;left:0;top:0;right:0;bottom:0\">\
<div id=dlg role=dialog aria-modal=true aria-label=\"Settings\" style=\"margin:40px;padding:10px;background:#eee\">\
<h2>Settings</h2><button id=ok>OK</button></div></div><script src=app.js></script></body></html>";

const TREE_SCRIPT: &str = "var $=function(id){return document.getElementById(id)},started=false;\
$('open').addEventListener('click',function(){$('scrim').style.display='block'});\
$('scrim').addEventListener('click',function(){});\
$('ok').addEventListener('click',function(){$('scrim').style.display='none'});\
window.addEventListener('message',function(e){if(e.source!==window.parent)return;var m=e.data;\
if(m.context){if(started)return;started=true;\
window.parent.postMessage({call:{id:1,method:'hello',params:{}}},'*')}});";

/// How deep the line that has `words` is in a reading: its name and two spaces, then two for each group
/// it is under.
fn depth_of(reading: &str, words: &str) -> usize {
    let line = reading
        .lines()
        .find(|line| line.contains(words))
        .unwrap_or_else(|| panic!("no line with {words} in\n{reading}"));
    let rest = &line[6..];
    (rest.len() - rest.trim_start().len() - 2) / 2
}

/// A web page is read by the rules of a native one: what holds others is one item with them under it,
/// a table is a table (its rows counted for the whole, its sorted header, its places), and with a
/// dialog that says it is modal the page behind is not read, though a scrim that answers a click holds
/// the dialog. Bringing a row into view goes to the page; a command of an element is a native page's.
fn agent_reads_a_web_page_as_a_tree(
    mtm: MainThreadMarker,
    window: &NSWindow,
    frame_rect: NSRect,
    dir: &Path,
) {
    let files = [
        ("index.html", "text/html", TREE_PAGE.as_bytes()),
        ("app.js", "text/javascript", TREE_SCRIPT.as_bytes()),
    ];
    let (pane, mut program) = open_pane(mtm, window, frame_rect, &dir.join("tree"), PANE + 8);
    program.say(&format!("{ESC}[?1049h"));
    let token = program.hello();
    program.open(&token, &files, None);
    program.supply(&files);
    watch_it_start(&pane, &mut program);

    // **The tree.** A list is a list with its items under it and a link under its item; a table has
    // its whole count, its header under it, its sorted column and the place of its row.
    let read = said(&pane, Action::Read);
    for needle in [
        "list \"Files\"",
        "listitem \"beta\"",
        "link \"alpha\"",
        "table \"Processes\"",
        "rows 200, columns 2",
        "columnheader \"CPU\"",
        "sorted ascending",
        "cell \"node\"",
        "row 5",
        "tablist \"Views\"",
        "tab \"One\"",
        "1 of 2",
        "mixed",
        // ARIA's own grid, row and cells (no `<table>`): a table, with a row at the place it says and
        // cells at their places in it.
        "table \"Board\"",
        "columnheader \"Lane\"",
        "cell \"nine\"",
        "row 7",
        "column 2",
    ] {
        assert!(read.contains(needle), "{needle:?} is not in:\n{read}");
    }
    assert!(!read.contains("behind the modal"), "{read}");
    assert_eq!(depth_of(&read, "list \"Files\""), 0, "{read}");
    assert_eq!(depth_of(&read, "listitem \"beta\""), 1, "{read}");
    assert_eq!(depth_of(&read, "link \"alpha\""), 2, "{read}");
    assert_eq!(depth_of(&read, "table \"Processes\""), 0, "{read}");
    assert!(
        depth_of(&read, "columnheader \"CPU\"") > depth_of(&read, "table \"Processes\""),
        "{read}"
    );
    // The JSON has the shape's number and where each item is held.
    let json = match view_of(&pane, Action::Read, Format::Json) {
        Reply::Text(text) => text,
        other => panic!("{other:?}"),
    };
    assert!(json.contains("\"format\":2"), "{json}");
    // The listitems are held by the list, which is the third item of the reading.
    assert!(json.contains("\"parent\":2"), "{json}");

    // **Bringing a row into view** goes to the page: the second row of the table, and a row it has
    // not built is no row.
    let table = ref_of(&read, "table \"Processes\"");
    // The row is the one the reading calls row 5 (the page's own `aria-rowindex`), not the second.
    let brought = said(
        &pane,
        Action::Reveal {
            target: table.clone(),
            index: Some(5),
        },
    );
    assert!(
        brought.starts_with(&format!(
            "brought row 5 of table \"Processes\" ({table}) into view\n"
        )),
        "{brought}"
    );
    let board = ref_of(&read, "table \"Board\"");
    let seven = said(
        &pane,
        Action::Reveal {
            target: board.clone(),
            index: Some(7),
        },
    );
    assert!(
        seven.starts_with("brought row 7 of table \"Board\""),
        "{seven}"
    );
    let second = refused(
        &pane,
        Action::Reveal {
            target: table.clone(),
            index: Some(2),
        },
    );
    assert!(
        second.contains("no such row"),
        "row 2 is not a row the reading has: {second}"
    );
    let far = refused(
        &pane,
        Action::Reveal {
            target: table.clone(),
            index: Some(99),
        },
    );
    assert!(far.contains("no such row"), "{far}");

    // A list says its items' places as `N of M`, and `reveal` takes that number: item 55 of a list
    // that has built only 55 and 56 is the first of them, not the "55th built".
    let library = ref_of(&read, "listbox \"Library\"");
    let fifty_five = said(
        &pane,
        Action::Reveal {
            target: library.clone(),
            index: Some(55),
        },
    );
    assert!(
        fifty_five.starts_with("brought row 55 of listbox \"Library\""),
        "{fifty_five}"
    );
    let first = refused(
        &pane,
        Action::Reveal {
            target: library,
            index: Some(1),
        },
    );
    assert!(
        first.contains("no such row"),
        "1 of 1000 is not built: {first}"
    );
    // An outline's rows are numbered as they are shown, top to bottom: a closed branch's rows are not.
    let outline = ref_of(&read, "tree \"Outline\"");
    let fourth = said(
        &pane,
        Action::Reveal {
            target: outline.clone(),
            index: Some(4),
        },
    );
    assert!(
        fourth.starts_with("brought row 4 of tree \"Outline\""),
        "{fourth}"
    );
    let fifth = refused(
        &pane,
        Action::Reveal {
            target: outline,
            index: Some(5),
        },
    );
    assert!(
        fifth.contains("no such row"),
        "the closed branch's row is not a row: {fifth}"
    );

    // **A dialog that says it is modal** shuts the page behind it out; a scrim that answers a click and
    // holds the dialog does not swallow what is in it.
    let open = ref_of(&read, "button \"Open settings\"");
    let opened = said(&pane, Action::Click(open));
    assert!(opened.contains("dialog \"Settings\""), "{opened}");
    assert!(
        opened.contains("button \"OK\""),
        "the scrim swallowed the dialog: {opened}"
    );
    assert!(
        depth_of(&opened, "button \"OK\"") > depth_of(&opened, "dialog \"Settings\""),
        "{opened}"
    );
    assert!(
        opened.contains("note: behind the modal in front,")
            && opened.contains("of the page, not read"),
        "{opened}"
    );
    // (The line that says what was clicked names the button, which is not an item of the reading.)
    let items = &opened[opened.find("items:").expect("items")..];
    for gone in [
        "table \"Processes\"",
        "list \"Files\"",
        "Open settings",
        "tab \"One\"",
    ] {
        assert!(
            !items.contains(gone),
            "{gone} is read behind the modal:\n{opened}"
        );
    }
    // Closed, the page is read again, whole.
    let ok = ref_of(&opened, "button \"OK\"");
    let closed = said(&pane, Action::Click(ok));
    assert!(closed.contains("table \"Processes\""), "{closed}");
    assert!(!closed.contains("behind the modal"), "{closed}");

    program.bye(&token);
    until("the view to go", || !covered(&pane));
    pane.close();
}

/// A page whose script builds thousands of elements before its dialog (which is open from the start
/// and comes last in the document).
const BIG_PAGE: &str = "<!doctype html><html><head><meta charset=utf-8><title>big</title></head><body>\
<div id=dlg role=dialog aria-modal=true aria-label=\"Last words\"><h2>Last words</h2><button>Fine</button></div>\
<script src=app.js></script></body></html>";

/// The script of the large page: 6000 paragraphs, which it puts in front of the dialog, and a page's
/// hello to the program.
const BIG_SCRIPT: &str = "var d=document.getElementById('dlg'),started=false;\
for(var i=0;i<6000;i++){var p=document.createElement('p');p.textContent='x';document.body.insertBefore(p,d)}\
window.addEventListener('message',function(e){if(e.source!==window.parent)return;var m=e.data;\
if(m.context){if(started)return;started=true;\
window.parent.postMessage({call:{id:1,method:'hello',params:{}}},'*')}});";

/// A page so large that its walk would end before its dialog (which comes last in the document) is
/// reached still reads the dialog: what is behind the modal has a budget of its own.
fn agent_reads_a_dialog_behind_a_large_page(
    mtm: MainThreadMarker,
    window: &NSWindow,
    frame_rect: NSRect,
    dir: &Path,
) {
    let files = [
        ("index.html", "text/html", BIG_PAGE.as_bytes()),
        ("app.js", "text/javascript", BIG_SCRIPT.as_bytes()),
    ];
    let (pane, mut program) = open_pane(mtm, window, frame_rect, &dir.join("big"), PANE + 9);
    program.say(&format!("{ESC}[?1049h"));
    let token = program.hello();
    program.open(&token, &files, None);
    program.supply(&files);
    watch_it_start(&pane, &mut program);
    let read = said(&pane, Action::Read);
    assert!(
        read.contains("dialog \"Last words\""),
        "{}",
        &read[..read.len().min(600)]
    );
    assert!(
        read.contains("button \"Fine\""),
        "{}",
        &read[..read.len().min(600)]
    );
    assert!(
        read.contains("note: behind the modal in front,"),
        "{}",
        &read[..read.len().min(600)]
    );
    program.bye(&token);
    until("the view to go", || !covered(&pane));
    pane.close();
}

/// The page of a pane that is not the person's, which asks for the keyboard when its program says
/// something (a message that no call of its waits for): into its field or into the page, and says
/// it asked.
const KEYS_PAGE: &str = r#"return {
    render = function()
        return pencere.div():w(300):h(200)
            :child(pencere.div():text_field("q"):w(200):h(20):child(pencere.text("Filter")))
    end,
    message = function(m)
        pencere.call("asked", { to = m.to })
        if m.to == "field" then
            return { redraw = true, keyboard = { field = "q" } }
        end
        return { redraw = true, keyboard = "page" }
    end,
}"#;

/// Whether the window's keyboard is in `pane`: its terminal, or a field of its page.
fn keyboard_in(window: &NSWindow, pane: &TerminalPane) -> bool {
    let me: &NSView = pane;
    window
        .firstResponder()
        .and_then(|responder| responder.downcast::<NSView>().ok())
        .is_some_and(|view| view.isDescendantOf(me))
}

/// Whether the window's keyboard is in a field of the page of `pane` (the field's editor), and not
/// in its terminal.
fn typing_in(window: &NSWindow, pane: &TerminalPane) -> bool {
    let terminal = terminal_view(pane);
    let in_pane = keyboard_in(window, pane);
    let in_terminal = window
        .firstResponder()
        .and_then(|responder| responder.downcast::<NSView>().ok())
        .zip(terminal)
        .is_some_and(|(view, terminal)| std::ptr::eq(&*view, &*terminal));
    in_pane && !in_terminal
}

/// A page asks for the keyboard (a reply's `keyboard`) in two panes of one window. In the pane that
/// does not have it, nothing is moved; once the person has clicked into that pane (`focus`) it is
/// theirs and the page takes it into its field and gives it back to the page as it asks. The page
/// goes first for the pane that has it, and the other pane keeps its own while this one asks.
fn page_keyboard(mtm: MainThreadMarker, window: &NSWindow, frame_rect: NSRect, dir: &Path) {
    let (holder, _holder_program) =
        open_pane(mtm, window, frame_rect, &dir.join("holder"), PANE + 3);
    let (asker, mut program) = open_pane(mtm, window, frame_rect, &dir.join("asker"), PANE + 4);
    // A page is drawn, and its fields made, only while its pane is seen: the window is on screen
    // for this section (it takes no key and no focus from the person's applications) and the host
    // says the panes are seen.
    window.setLevel(objc2_app_kit::NSFloatingWindowLevel);
    window.setCollectionBehavior(
        objc2_app_kit::NSWindowCollectionBehavior::CanJoinAllSpaces
            | objc2_app_kit::NSWindowCollectionBehavior::FullScreenAuxiliary
            | objc2_app_kit::NSWindowCollectionBehavior::Stationary,
    );
    window.setFrameOrigin(NSPoint::new(40.0, 120.0));
    window.orderFrontRegardless();
    holder.set_visibility(true);
    asker.set_visibility(true);
    program.say(&format!("{ESC}[?1049h"));
    let token = program.hello_kind("lua");
    program.open_script(&token, KEYS_PAGE);
    program.supply(&[("page.lua", "text/x-lua", KEYS_PAGE.as_bytes())]);
    // The page is up when its field is a view of the pane.
    let has_field = || {
        asker.subviews().iter().any(|sub| {
            descendants(sub).iter().any(|view| {
                view.downcast_ref::<NSTextField>()
                    .is_some_and(|f| f.isEditable())
            })
        })
    };
    // The page's field is made by the first frame the page is drawn in, and a window that is
    // covered, or on a display that is asleep, draws none: then the keyboard is asked for the page
    // alone, which needs no field, and the field's half says so and is left.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !has_field() && Instant::now() < deadline {
        spin(0.05);
    }
    let fields = has_field();
    if !fields {
        eprintln!(
            "pencere: the page was not drawn (window occlusion {:?}): its field is not tried",
            window.occlusionState()
        );
    }

    // The person is in the other pane; the page asks in every way it can: nothing moves.
    assert!(holder.focus(), "the keyboard is in the first pane");
    assert!(keyboard_in(window, &holder));
    let asks: &[&str] = if fields {
        &["field", "page"]
    } else {
        &["page"]
    };
    for to in asks {
        program.send("msg", &format!("{{\"to\":\"{to}\"}}"));
        program.wait_for("\"method\":\"asked\"");
        spin(0.7);
        assert!(
            keyboard_in(window, &holder) && !keyboard_in(window, &asker),
            "the page took the keyboard from the pane that has it (asking for {to})"
        );
    }

    // The person clicks into the page's pane: the keyboard is there and stays; and the page sends
    // it into its field, and back to the page.
    assert!(asker.focus(), "the keyboard is in the second pane");
    assert!(keyboard_in(window, &asker) && !keyboard_in(window, &holder));
    program.send("msg", "{\"to\":\"page\"}");
    program.wait_for("\"method\":\"asked\"");
    spin(0.7);
    assert!(
        keyboard_in(window, &asker),
        "the pane that has the keyboard keeps it"
    );
    if fields {
        program.send("msg", "{\"to\":\"field\"}");
        program.wait_for("\"method\":\"asked\"");
        until("the page's field to have the keyboard", || {
            typing_in(window, &asker)
        });
        program.send("msg", "{\"to\":\"page\"}");
        program.wait_for("\"method\":\"asked\"");
        until("the keyboard to be back in the page", || {
            keyboard_in(window, &asker) && !typing_in(window, &asker)
        });
    }

    program.bye(&token);
    holder.close();
    asker.close();
    window.orderOut(None);
}

/// A page with a field of one line and a secret, each with a hint in capitals: the ink of a hint is
/// its capitals, top to baseline.
const FIELDS_PAGE: &str = r#"return {
    render = function()
        return pencere.div():col():gap(16):p(20):w(420):h(200)
            :child(pencere.div():text_field("line"):w(300):h(20):child(pencere.text("FILTER BY NAME")))
            :child(pencere.div():text_field("secret", "secret"):w(300):h(20):child(pencere.text("SECRET")))
    end,
}"#;

/// How far the middle of the capitals `field` draws stands from the middle bateri centred them on
/// (its frame's top, AppKit's baseline in it, half a capital up: where the room's middle is), in
/// points, down positive; `None` where it drew nothing. Read from the field's own drawing, which
/// has no ground: what it draws is its text.
fn caps_off(field: &NSTextField) -> Option<f64> {
    // SAFETY: reading the superview, on the main thread.
    let host = unsafe { field.superview() }?;
    let frame = field.frame();
    let font = field.font()?;
    // SAFETY: NSView's `firstBaselineOffsetFromTop`, which takes nothing and gives a CGFloat.
    let baseline: f64 = unsafe { msg_send![field, firstBaselineOffsetFromTop] };
    let aimed = frame.origin.y + baseline - font.capHeight() / 2.0;
    let picture = host.bitmapImageRepForCachingDisplayInRect(frame)?;
    host.cacheDisplayInRect_toBitmapImageRep(frame, &picture);
    let (wide, high) = (picture.pixelsWide(), picture.pixelsHigh());
    // The most opaque pixel of each row: a hint is drawn in a lighter ink than a text, so a row is
    // inked where it holds half of the darkest row's.
    let rows: Vec<f64> = (0..high)
        .map(|y| {
            (0..wide)
                .filter_map(|x| picture.colorAtX_y(x, y))
                .map(|colour| colour.alphaComponent())
                .fold(0.0, f64::max)
        })
        .collect();
    let most = rows.iter().copied().fold(0.0, f64::max);
    let inked: Vec<usize> = (0..rows.len()).filter(|&y| rows[y] > most / 2.0).collect();
    let (first, last) = (*inked.first()?, *inked.last()?);
    #[allow(clippy::cast_precision_loss)]
    let scale = high as f64 / frame.size.height;
    #[allow(clippy::cast_precision_loss)]
    let middle = frame.origin.y + (first + last + 1) as f64 / 2.0 / scale;
    Some(middle - aimed)
}

/// A page's fields draw their text where bateri centres it on the room, in the app's faces: a
/// hint at rest and with the keyboard in its field, a text while it is typed and after. (AppKit's
/// single-line cell drew the text at rest at a baseline of its own, set by the control's size: a
/// point or more above in most of the terminal's faces, four at 17 points, and lower once typed
/// into.) A line takes a line break put in it as a space, and a secret takes the keyboard.
fn field_text_on_its_baseline(
    mtm: MainThreadMarker,
    window: &NSWindow,
    frame_rect: NSRect,
    dir: &Path,
) {
    // Drawn only while it is seen: on screen, as for the page's keyboard.
    window.setLevel(objc2_app_kit::NSFloatingWindowLevel);
    window.setCollectionBehavior(
        objc2_app_kit::NSWindowCollectionBehavior::CanJoinAllSpaces
            | objc2_app_kit::NSWindowCollectionBehavior::FullScreenAuxiliary
            | objc2_app_kit::NSWindowCollectionBehavior::Stationary,
    );
    window.setFrameOrigin(NSPoint::new(40.0, 120.0));
    window.orderFrontRegardless();
    // The face a terminal is most often in, at a size where the single-line cell stood four points
    // high; and one whose line that cell drew lower, at the kit's own size.
    for (family, size, id) in [("JetBrains Mono", 17.0, 50), ("Cascadia Code", 12.5, 51)] {
        let mut settings = Settings::default();
        settings.font.family = Some(family.to_owned());
        settings.font.size = size;
        let (pane, mut program) = open_hosted_pane(
            mtm,
            window,
            frame_rect,
            &dir.join(format!("field-{id}")),
            PANE + id,
            Rc::new(Quiet::default()),
            settings,
        );
        pane.set_visibility(true);
        program.say(&format!("{ESC}[?1049h"));
        let token = program.hello_kind("lua");
        program.open_script(&token, FIELDS_PAGE);
        program.supply(&[("page.lua", "text/x-lua", FIELDS_PAGE.as_bytes())]);
        let fields = || {
            let mut found: Vec<Retained<NSTextField>> = pane
                .subviews()
                .iter()
                .flat_map(descendants)
                .filter_map(|view| view.downcast::<NSTextField>().ok())
                .filter(|field| field.isEditable())
                .collect();
            found.sort_by(|a, b| a.frame().origin.y.total_cmp(&b.frame().origin.y));
            found
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while fields().len() < 2 && Instant::now() < deadline {
            spin(0.05);
        }
        let found = fields();
        if found.len() < 2 {
            // As for the page's keyboard: a covered window, or a display asleep, draws no page.
            eprintln!(
                "pencere: the page was not drawn (window occlusion {:?}): its fields are not measured",
                window.occlusionState()
            );
            program.bye(&token);
            pane.close();
            continue;
        }
        let (line, secret) = (&found[0], &found[1]);
        let mut offs = Vec::new();
        let mut look = |what: &str, field: &NSTextField| {
            spin(0.2);
            let off = caps_off(field)
                .unwrap_or_else(|| panic!("{family} at {size}: the {what} drew nothing"));
            eprintln!("pencere: {family} at {size}: the {what} is {off:+.2} pt off its middle");
            offs.push(format!("{what} {off:+.2}"));
            off
        };
        let mut worst: f64 = 0.0;
        worst = worst.max(look("line's hint", line).abs());
        window.makeFirstResponder(Some(line));
        worst = worst.max(look("line's hint with the keyboard", line).abs());
        let editor = line.currentEditor().expect("the line has an editor");
        // What a paste puts in: through the editor, as the person's.
        // SAFETY: a field's editor is an NSTextView, which takes `insertText:` with a string.
        let _: () = unsafe { msg_send![&*editor, insertText: &*NSString::from_str("FX\nZ")] };
        assert_eq!(
            line.stringValue().to_string(),
            "FX Z",
            "a line takes no break"
        );
        worst = worst.max(look("line's text while typed", line).abs());
        window.makeFirstResponder(None);
        worst = worst.max(look("line's text", line).abs());
        worst = worst.max(look("secret's hint", secret).abs());
        // A secret takes the keyboard and an editor (a plain field with a secure cell made AppKit
        // throw here, and the throw ended the application).
        assert!(
            window.makeFirstResponder(Some(secret)),
            "the secret takes the keyboard"
        );
        assert!(secret.currentEditor().is_some(), "the secret has an editor");
        worst = worst.max(look("secret's hint with the keyboard", secret).abs());
        window.makeFirstResponder(None);
        assert!(
            worst <= 0.5,
            "{family} at {size}: a field's capitals stand off their middle: {offs:?}"
        );
        program.bye(&token);
        until("the view to go", || !covered(&pane));
        pane.close();
    }
    window.orderOut(None);
}

/// A page with a box of three rows and one of one row, each in a room with six points of padding
/// above and below, at a width of 300.
const ROWS_PAGE: &str = r#"return {
    render = function()
        return pencere.div():col():gap(16):p(20):w(420):h(300)
            :child(pencere.div():text_field("notes", "lines", { rows = 3 }):py(6):w(300):child(pencere.text("NOTES")))
            :child(pencere.div():text_field("one", "lines", { rows = 1 }):py(6):w(300):child(pencere.text("ONE")))
    end,
}"#;

/// A field of `rows` rows is as tall as that many lines of the face AppKit draws it in, and no
/// more, at the terminal's size and at twice it: AppKit's own line, told to the page's measure
/// (`FieldLine`), laid out by the kit and put in the field's box by the pane, so that the last row
/// is not cut and a row of its text is not half a point off. The room is the box and the padding;
/// the second field stands where the first's room ends and the gap goes on.
fn field_rows_stand_in_their_boxes(
    mtm: MainThreadMarker,
    window: &NSWindow,
    frame_rect: NSRect,
    dir: &Path,
) {
    window.setLevel(objc2_app_kit::NSFloatingWindowLevel);
    window.setFrameOrigin(NSPoint::new(40.0, 120.0));
    window.orderFrontRegardless();
    // Monaco has 16 points to a line at 12.5 (the kit's text, zoom 1) and 31 at 25 (zoom 2).
    for (size, line, id) in [(12.5, 16.0, 70), (25.0, 31.0, 71)] {
        let mut settings = Settings::default();
        settings.font.family = Some("Monaco".to_owned());
        settings.font.size = size;
        let (pane, mut program) = open_hosted_pane(
            mtm,
            window,
            frame_rect,
            &dir.join(format!("rows-{id}")),
            PANE + id,
            Rc::new(Quiet::default()),
            settings,
        );
        pane.set_visibility(true);
        program.say(&format!("{ESC}[?1049h"));
        let token = program.hello_kind("lua");
        program.open_script(&token, ROWS_PAGE);
        program.supply(&[("page.lua", "text/x-lua", ROWS_PAGE.as_bytes())]);
        // A field of lines is a text view in a scroll view, which is what stands in its box (a search
        // for text fields finds none of them, and a test that looks for none passes by saying the
        // page was not drawn).
        let fields = || {
            let mut found: Vec<(Retained<NSTextView>, NSRect)> = pane
                .subviews()
                .iter()
                .flat_map(descendants)
                .filter_map(|view| view.downcast::<NSTextView>().ok())
                .filter(|text| text.isEditable() && !text.isFieldEditor())
                .filter_map(|text| {
                    let frame = text.enclosingScrollView()?.frame();
                    Some((text, frame))
                })
                .collect();
            found.sort_by(|a, b| a.1.origin.y.total_cmp(&b.1.origin.y));
            found
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while fields().len() < 2 && Instant::now() < deadline {
            spin(0.05);
        }
        let found = fields();
        if found.len() < 2 {
            eprintln!(
                "pencere: the page was not drawn (window occlusion {:?}): its rows are not measured",
                window.occlusionState()
            );
            program.bye(&token);
            pane.close();
            continue;
        }
        spin(0.2);
        let found = fields();
        let (notes, one) = (found[0].1, found[1].1);
        let zoom = size / 12.5;
        let (pad, gap, margin) = (6.0 * zoom, 16.0 * zoom, 20.0 * zoom);
        eprintln!("pencere: Monaco at {size}: three rows {notes:?}, one row {one:?}");
        assert!(
            (notes.size.height - 3.0 * line).abs() < 0.01 && (one.size.height - line).abs() < 0.01,
            "Monaco at {size}: three rows are {} points and one row {}, not {} and {}",
            notes.size.height,
            one.size.height,
            3.0 * line,
            line
        );
        assert!(
            (notes.origin.y - (margin + pad)).abs() < 0.51,
            "Monaco at {size}: the box starts {} points down, past the padding at {}",
            notes.origin.y,
            margin + pad
        );
        assert!(
            ((one.origin.y - notes.origin.y) - (3.0 * line + 2.0 * pad + gap)).abs() < 0.51,
            "Monaco at {size}: the second field is {} points below the first, not a room and a gap ({})",
            one.origin.y - notes.origin.y,
            3.0 * line + 2.0 * pad + gap
        );
        // The text is not cut: three lines of it fit the box AppKit made.
        let text = &found[0].0;
        text.setString(&NSString::from_str("one\ntwo\nthree"));
        // SAFETY: the text view was made with its layout manager and container.
        let (manager, container) = unsafe { (text.layoutManager(), text.textContainer()) };
        let (manager, container) = (
            manager.expect("a layout manager"),
            container.expect("a container"),
        );
        manager.ensureLayoutForTextContainer(&container);
        let needs = manager.usedRectForTextContainer(&container).size.height;
        assert!(
            needs <= notes.size.height + 0.01,
            "Monaco at {size}: three lines need {needs} points of the box's {}",
            notes.size.height
        );
        // The text is as wide as the box: the page broke the lines for it.
        let across = text
            .enclosingScrollView()
            .map(|scroll| scroll.contentSize().width);
        assert!(
            across.is_some_and(|across| (across - notes.size.width).abs() < 0.01),
            "Monaco at {size}: the text is {across:?} points across, the box {}",
            notes.size.width
        );
        program.bye(&token);
        until("the view to go", || !covered(&pane));
        pane.close();
    }
    window.orderOut(None);
}

/// A page with a button, and a dialog that holds a button and a field.
const GROUPS_PAGE: &str = r#"return {
    render = function()
        return pencere.div():col():gap(12):p(20):w(420):h(260)
            :child(pencere.div():press("outside"):role("button"):label("Outside"):w(120):h(24))
            :child(pencere.div():role("dialog"):label("Settings"):col():gap(12):p(10):w(320):h(140)
                :child(pencere.div():press("ok"):role("button"):label("OK"):w(80):h(24))
                :child(pencere.div():text_field("name"):w(240):h(20):child(pencere.text("NAME"))))
    end,
}"#;

/// What AppKit's `name` of `object` is, as a string, where it has one (`accessibilityRole`, …).
fn ax_words(object: &AnyObject, name: Sel) -> Option<String> {
    // SAFETY: an NSAccessibility getter that takes nothing and gives a string or nil.
    let words: Option<Retained<NSString>> = unsafe { msg_send![object, performSelector: name] };
    words.map(|words| words.to_string())
}

/// What `object` holds, to a screen reader.
fn held(object: &AnyObject) -> Vec<Retained<AnyObject>> {
    // SAFETY: NSAccessibility's `accessibilityChildren`, which takes nothing.
    let children: Option<Retained<NSArray>> = unsafe { msg_send![object, accessibilityChildren] };
    children
        .map(|children| children.to_vec())
        .unwrap_or_default()
}

/// What `object` says holds it, to a screen reader.
fn holder(object: &AnyObject) -> Option<Retained<AnyObject>> {
    // SAFETY: NSAccessibility's `accessibilityParent`, which takes nothing.
    unsafe { msg_send![object, accessibilityParent] }
}

/// The middle of `object`'s frame on the screen.
fn middle(object: &AnyObject) -> NSPoint {
    // SAFETY: NSAccessibility's `accessibilityFrame`, which takes nothing.
    let frame: NSRect = unsafe { msg_send![object, accessibilityFrame] };
    NSPoint::new(
        frame.origin.x + frame.size.width / 2.0,
        frame.origin.y + frame.size.height / 2.0,
    )
}

/// While a page of the kit is up, VoiceOver is given the pane as a group named for the program,
/// and each element of the page says the element that lists it is its parent: the pane for what
/// stands on the page, a dialog for what is in it. (A pane left out of the tree, as a view is,
/// was named as the parent of elements the window listed as its own, and VoiceOver, inside a
/// dialog or a table, could not move from the first thing in it.) A field is read by its cell, a
/// text field; a hit test finds the element at a point, asked of the pane or of the terminal's view
/// under it; and with the page down the pane is no element again.
fn a_screen_reader_reads_a_page_as_the_panes_group(
    mtm: MainThreadMarker,
    window: &NSWindow,
    frame_rect: NSRect,
    dir: &Path,
) {
    window.orderFrontRegardless();
    let (pane, mut program) = open_hosted_pane(
        mtm,
        window,
        frame_rect,
        &dir.join("groups"),
        PANE + 60,
        Rc::new(Quiet::default()),
        Settings::default(),
    );
    pane.set_visibility(true);
    program.say(&format!("{ESC}[?1049h"));
    let token = program.hello_kind("lua");
    program.open_script(&token, GROUPS_PAGE);
    program.supply(&[("page.lua", "text/x-lua", GROUPS_PAGE.as_bytes())]);
    let pane_object: &AnyObject = &pane;
    let dialog = || {
        held(pane_object).into_iter().find(|child| {
            ax_words(child, sel!(accessibilitySubrole)).as_deref() == Some("AXApplicationDialog")
        })
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    while dialog().is_none_or(|dialog| held(&dialog).len() < 2) && Instant::now() < deadline {
        spin(0.05);
    }
    let Some(dialog) = dialog() else {
        // As for the fields' baseline: a covered window, or a display asleep, draws no page.
        eprintln!(
            "pencere: the page was not drawn (window occlusion {:?}): its tree is not read",
            window.occlusionState()
        );
        program.bye(&token);
        pane.close();
        window.orderOut(None);
        return;
    };
    // SAFETY: NSAccessibility's `isAccessibilityElement`, which takes nothing.
    let element: bool = unsafe { msg_send![pane_object, isAccessibilityElement] };
    assert!(element, "the pane is an element while a page is up");
    assert_eq!(
        ax_words(pane_object, sel!(accessibilityRole)).as_deref(),
        Some("AXGroup")
    );
    assert_eq!(
        ax_words(&dialog, sel!(accessibilityLabel)).as_deref(),
        Some("Settings")
    );
    assert_eq!(
        ax_words(&dialog, sel!(accessibilityRoleDescription)).as_deref(),
        Some("dialog")
    );
    // Each holder lists what names it as its parent.
    for (outer, inner) in [(pane_object, held(pane_object)), (&*dialog, held(&dialog))] {
        for child in inner {
            let up = holder(&child).expect("an element has a parent");
            assert!(
                std::ptr::eq::<AnyObject>(&*up, outer),
                "{:?} names another parent than what lists it",
                ax_words(&child, sel!(accessibilityLabel))
            );
        }
    }
    let inside = held(&dialog);
    let ok = inside
        .iter()
        .find(|child| ax_words(child, sel!(accessibilityLabel)).as_deref() == Some("OK"))
        .expect("the dialog holds its button");
    let field = inside
        .iter()
        .find(|child| ax_words(child, sel!(accessibilityRole)).as_deref() == Some("AXTextField"))
        .expect("the dialog holds its field, read as a text field");
    assert!(
        field.downcast_ref::<objc2_app_kit::NSCell>().is_some(),
        "a field is read by its cell"
    );
    // The element at the button's middle, asked of the pane and of the terminal's view.
    let point = middle(ok);
    // SAFETY: NSAccessibility's `accessibilityHitTest:`, with a point on the screen.
    let found: Option<Retained<AnyObject>> =
        unsafe { msg_send![pane_object, accessibilityHitTest: point] };
    assert!(
        found.is_some_and(|found| std::ptr::eq::<AnyObject>(&*found, &**ok)),
        "the pane's hit test finds the button"
    );
    let terminal = terminal_view(&pane).expect("the pane has its terminal's view");
    // SAFETY: as above.
    let found: Option<Retained<AnyObject>> =
        unsafe { msg_send![&*terminal, accessibilityHitTest: point] };
    assert!(
        found.is_some_and(|found| std::ptr::eq::<AnyObject>(&*found, &**ok)),
        "the terminal's view's hit test finds the button"
    );
    program.bye(&token);
    until("the page to go", || {
        // SAFETY: as above.
        let element: bool = unsafe { msg_send![pane_object, isAccessibilityElement] };
        !element
    });
    pane.close();
    window.orderOut(None);
}

/// The view covers the pane, and everything that took was the chain: the shell page and the
/// frame loaded, the page's script ran, the host told the page its `context`, and the page called
/// the program. Returns that call (`hello`), which says what the page was told.
fn watch_it_start(pane: &TerminalPane, program: &mut Program) -> String {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !covered(pane) && Instant::now() < deadline {
        spin(0.05);
    }
    if !covered(pane) {
        let names: Vec<String> = pane
            .subviews()
            .iter()
            .map(|view| view.class().name().to_string_lossy().into_owned())
            .collect();
        eprintln!(
            "subviews {names:?}\nstate {:?}\nheard {:?}",
            pane.session().and_then(|session| session.view_state()),
            program.heard()
        );
        panic!("the view never covered the pane");
    }
    let call = program.wait_for("\"method\":\"hello\"");
    assert!(call.contains("\"call\":{\"id\":1,"), "{call}");
    call
}

/// Standard base64 with padding.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let word = chunk.iter().enumerate().fold(0u32, |word, (at, byte)| {
            word | (u32::from(*byte) << (16 - 8 * at))
        });
        for at in 0..4 {
            if at <= chunk.len() {
                out.push(char::from(
                    ALPHABET[((word >> (18 - 6 * at)) & 63) as usize],
                ));
            } else {
                out.push('=');
            }
        }
    }
    out
}
