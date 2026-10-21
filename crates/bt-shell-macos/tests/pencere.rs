//! A program's view in a pane, end to end, in a process that is not bateri: a real pane with
//! a real shell, a program that speaks the pencere protocol through two files, and the
//! `WKWebView` the pane puts over its terminal for the view the program asks for.
//!
//! The program is this test. The shell runs a script that puts the tty in raw mode and is a
//! cable: what the test appends to `in` is written to the pane as the program's output, and
//! what the pane writes to the program's input arrives in `out`. The test says `hello`, opens
//! a view whose document is one HTML page, and from there the only thing that can make the
//! program receive the next message is the whole chain working — the shell page served by the
//! application's scheme handler, the view's document inside its sandboxed frame under the
//! policy the crate built, the view's `postMessage` carried through the script-message
//! handler into the session, and the session's answer carried back by `evaluateJavaScript`.
//! The page then asks nothing of the host beyond the handshake, so the message that proves it
//! is the host calling the tool the view is for (`tools/call`), which the host does when the
//! view says it is initialised.
//!
//! Then ⌃C typed in the view arrives at the program as the byte 0x03, a key that is the
//! application's shortcut (⌘T) goes to the main menu and not to the view, the application's
//! strip stands over the view's area, and the program's `bye` takes the view down.
//!
//! Its own `main` (`harness = false`): AppKit, the main queue and `WKWebView` want the main
//! thread. Without a window server session (an ssh login) it says so and passes, as the embed
//! test does.

use std::cell::{Cell, RefCell};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::time::{Duration, Instant};

use bt_core::{Chord, Key, Mods, Settings, Theme};
use bt_shell_common::view_strip;
use bt_shell_macos::embed::{self, Cover, Host, Identity, Source, TerminalPane};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSEvent,
    NSEventModifierFlags, NSEventType, NSMenu, NSMenuItem, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{
    NSDate, NSDefaultRunLoopMode, NSObject, NSObjectProtocol, NSPoint, NSRect, NSRunLoop, NSSize,
    NSString,
};

/// What a host heard: nothing this test is about, but the trait wants an answer to each.
#[derive(Default)]
struct Quiet(RefCell<Vec<String>>);

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
    fn files_dragged(&self, _: u64, _: bool) {}
    fn carry_press(&self, _: u64, _: (f64, f64)) {}
    fn questions_changed(&self, _: u64) {}
    fn ports_changed(&self, _: u64) {}
    fn bell(&self, _: u64) {}
    fn program_notification(&self, _: u64, _: Option<&str>, _: &str) {}
    fn cover(&self, _: &TerminalPane) -> Option<Rc<dyn Cover>> {
        None
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
            "{ESC}_pencere;hello;id={id};{{\"versions\":[1]}}{ESC}\\{ESC}[c"
        ));
        let welcome = self.wait_for("pencere;welcome");
        assert!(
            welcome.contains("pencere-test"),
            "the host is the application that opened the pane: {welcome}"
        );
        welcome
            .split("\"token\":\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .expect("a token in the welcome")
            .to_owned()
    }

    /// The open of a view of the tool `show`: with a bundle of `files` (path, type, bytes),
    /// or with none.
    fn open(&mut self, token: &str, files: Option<&[(&str, &str, &[u8])]>) {
        let bundle = files.map_or_else(String::new, |files| {
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
            format!(
                ",\"bundle\":{{\"entry\":\"index.html\",\"files\":[{}],\"meta\":{{\"csp\":\
{{\"connectDomains\":[\"https://api.example.com\"]}}}}}}",
                list.join(",")
            )
        });
        self.send(
            "open",
            &format!(
                "{{\"token\":\"{token}\",\"tool\":{{\"name\":\"show\",\"arguments\":{{}}}}{bundle}}}"
            ),
        );
    }

    /// The MCP start-up the host runs as a client of the program: `initialize`, the tool list
    /// and, for a view with no bundle, the document (`html`).
    fn start_up(&mut self, html: Option<&str>) {
        let initialize = self.wait_for("\"method\":\"initialize\"");
        let id = rpc_id(&initialize);
        self.send(
            "msg",
            &format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{{\"protocolVersion\":\"2025-11-25\",\
\"capabilities\":{{\"tools\":{{}}}},\"serverInfo\":{{\"name\":\"show\",\"version\":\"0\"}}}}}}"
            ),
        );
        let tools = self.wait_for("\"method\":\"tools/list\"");
        let id = rpc_id(&tools);
        self.send(
            "msg",
            &format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{{\"tools\":[{{\"name\":\"show\",\
\"inputSchema\":{{\"type\":\"object\"}},\"_meta\":{{\"ui\":{{\"resourceUri\":\"ui://show/view\"}}}}}}]}}}}"
            ),
        );
        let Some(html) = html else {
            return;
        };
        let read = self.wait_for("\"method\":\"resources/read\"");
        let id = rpc_id(&read);
        self.send(
            "msg",
            &format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{{\"contents\":[{{\"uri\":\"ui://show/view\",\
\"mimeType\":\"text/html;profile=mcp-app\",\"text\":{},\"_meta\":{{\"ui\":{{}}}}}}]}}}}",
                json_string(html)
            ),
        );
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

/// The JSON-RPC id of the request the last waited-for frame holds.
fn rpc_id(frame: &str) -> u32 {
    let at = frame.rfind("\"id\":").expect("a request has an id") + "\"id\":".len();
    frame[at..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .expect("a number")
}

/// The view's document: one page that says it is initialised when the host has answered
/// `ui/initialize`. Printable ASCII and no quote of the kind JSON would escape twice.
const VIEW: &str = "<!doctype html><html><head><meta charset=utf-8><title>t</title></head>\
<body><p id=p>view</p><script>\
var send=function(m){window.parent.postMessage(Object.assign({jsonrpc:'2.0'},m),'*')};\
window.addEventListener('message',function(e){\
if(e.source!==window.parent)return;var m=e.data;\
if(m&&m.id===1&&m.result){send({method:'ui/notifications/initialized'})}});\
send({id:1,method:'ui/initialize',params:{protocolVersion:'2026-01-26',\
appInfo:{name:'t',version:'0'},appCapabilities:{experimental:{'pencere/pane':{}}}}});\
</script></body></html>";

/// The same handshake as [`VIEW`]'s, as a bundle's two files: a page that loads a script.
const BUNDLE_PAGE: &str = "<!doctype html><html><head><meta charset=utf-8></head>\
<body><p>bundle</p><script src=app.js></script></body></html>";

/// The bundle's script: the handshake of [`VIEW`], which only runs if the script was served
/// from the bundle to the view's own origin.
const BUNDLE_SCRIPT: &str = "var send=function(m){window.parent.postMessage(\
Object.assign({jsonrpc:'2.0'},m),'*')};\
window.addEventListener('message',function(e){\
if(e.source!==window.parent)return;var m=e.data;\
if(m&&m.id===1&&m.result){send({method:'ui/notifications/initialized'})}});\
send({id:1,method:'ui/initialize',params:{protocolVersion:'2026-01-26',\
appInfo:{name:'t',version:'0'},appCapabilities:{experimental:{'pencere/pane':{}}}}});";

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

/// The strip the application keeps over a view: a subview of the pane of its own class.
fn strip_view(pane: &TerminalPane) -> Option<Retained<NSView>> {
    pane.subviews().iter().find(|view| {
        view.class()
            .name()
            .to_string_lossy()
            .contains("BateriPencereStrip")
    })
}

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bateri-pencere-test-{}", std::process::id()));
    fs::create_dir_all(dir.join("zdot")).expect("the scratch folder");
    dir
}

fn main() {
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

    let identity = Rc::new(Identity {
        app_name: "pencere-test".to_owned(),
        helper: None,
        zsh_wrapper_dir: None,
        fish_wrapper_dir: None,
        bash_wrapper_dir: None,
    });
    let mut config = embed::Config::new(
        mtm,
        PANE,
        Rc::new(Quiet::default()),
        identity,
        Settings::default(),
        Theme::BATERI,
    );
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
    let mut program = Program {
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

    // The view is for the alternate screen (§6.5).
    program.say(&format!("{ESC}[?1049h"));

    // A view whose document is the one page the program's `resources/read` gives.
    let token = program.hello();
    program.open(&token, None);
    program.start_up(Some(VIEW));
    watch_it_start(&pane, &mut program);

    // ⌃C in the view is the program's interrupt: the byte, written to the tty.
    let before = program.heard().len();
    let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
        NSEventType::KeyDown,
        NSPoint::new(0.0, 0.0),
        NSEventModifierFlags::Control,
        0.0,
        window.windowNumber(),
        None,
        &NSString::from_str("\u{3}"),
        &NSString::from_str("c"),
        false,
        8,
    )
    .expect("a key event");
    let web = web_view(&pane).expect("the view is a subview of the pane");
    web.keyDown(&event);
    until("⌃C to reach the program as 0x03", || {
        program
            .heard()
            .as_bytes()
            .get(before..)
            .is_some_and(|new| new.contains(&3))
    });

    // The strip is the application's own and stands over the view's area: the view is the
    // pane less the strip's height, below it.
    let strip = strip_view(&pane).expect("the strip is a subview of the pane");
    let (area, bounds) = (web.frame(), pane.bounds());
    assert_eq!(strip.frame().size.height, view_strip::HEIGHT);
    assert_eq!(
        strip.frame().origin.y + strip.frame().size.height,
        bounds.size.height
    );
    assert_eq!(area.origin.y, 0.0);
    assert_eq!(area.size.height, bounds.size.height - view_strip::HEIGHT);
    assert_eq!(area.size.width, bounds.size.width);

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

    // A view of a bundle: its files go to the host once, the page loads its script from the
    // bundle's own origin (a `<script src>` the policy's `'self'` allows), and a manifest that
    // declares a domain makes a question the pane refuses for now — the view still starts.
    let files = [
        ("index.html", "text/html", BUNDLE_PAGE.as_bytes()),
        ("app.js", "text/javascript", BUNDLE_SCRIPT.as_bytes()),
    ];
    let token = program.hello();
    program.open(&token, Some(&files));
    let missing = program.wait_for("pencere;need");
    for (_, _, bytes) in &files {
        let digest = bt_core::pencere::Digest::of(bytes).to_string();
        assert!(
            missing.contains(&digest),
            "the host holds none of them yet: {missing}"
        );
    }
    for (_, _, bytes) in &files {
        let digest = bt_core::pencere::Digest::of(bytes).to_string();
        let id = program.take_id();
        program.say(&frame(
            "file",
            &format!("id={id},d={digest}"),
            &base64(bytes),
        ));
    }
    program.start_up(None);
    watch_it_start(&pane, &mut program);
    program.bye(&token);
    until("the bundle's view to go", || !covered(&pane));

    // The same bundle again: the host has every file, and says so.
    let token = program.hello();
    program.open(&token, Some(&files));
    let missing = program.wait_for("pencere;need");
    assert!(
        missing.ends_with("{\"missing\":[]}"),
        "the second open sends no file: {missing}"
    );
    program.start_up(None);
    watch_it_start(&pane, &mut program);
    program.bye(&token);
    until("the second view to go", || !covered(&pane));

    pane.close();
    println!("pencere: ok");
}

/// The view covers the pane, and everything that took was the chain: the shell page and the
/// frame loaded, the view said `ui/initialize`, the host answered it, the view said it was
/// initialised, and the host called the tool the view is for.
fn watch_it_start(pane: &TerminalPane, program: &mut Program) {
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
    let call = program.wait_for("\"method\":\"tools/call\"");
    assert!(call.contains("\"name\":\"show\""), "{call}");
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

/// `text` as a JSON string of printable ASCII, the way the protocol writes one.
fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (' '..='~').contains(&c) => out.push(c),
            c => out.push_str(&format!("\\u{:04x}", u32::from(c))),
        }
    }
    out.push('"');
    out
}
