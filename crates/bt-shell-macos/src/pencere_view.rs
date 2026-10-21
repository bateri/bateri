//! The web view of a program's view: what a pane shows when the program in it
//! asks for a view with the pencere protocol, and what that web view may do.
//!
//! The conversation with the program is `bt-core`'s (`bt_core::pencere`, in the
//! pane's stream); this is the other end of it, the part that has a window. A
//! `WKWebView` is put over the pane, loads the application's own shell page, and
//! that page frames the view's document in a sandboxed `<iframe>` of another
//! origin (`serve`). Both documents are served by this module's
//! `WKURLSchemeHandler` from the crate's file service and nothing else: the web
//! view has no network, no file, no cookie that outlives it and nowhere to go. A file of the
//! bundle is read off the main thread (`files`): the handler holds the web view's task, and
//! answers it on the main queue when the file is ready.
//!
//! What crosses between the view and the application is JSON-RPC text, both ways:
//! the view's `postMessage` reaches the shell page, which passes it to the
//! script-message handler, which gives it to the session (`view_message`); the
//! session's messages for the view come back as one `evaluateJavaScript` call
//! each (`deliver`). Nothing else is a message, and a script message that does
//! not come from the top frame — a view can reach the handler's name too — is
//! dropped.
//!
//! **What the web view is refused**, in one place (the navigation and UI
//! delegates, [`Bridge`]): navigating anywhere but the one shell load and the
//! view's own origin in its frame (`serve::Page::allows`); a new window;
//! a file chooser; the media capture the permission policy would not have
//! stopped; the context menu (it has Reload, which would load the shell again).
//! A view's web content process that dies closes the view: nothing is left to
//! answer its teardown.
//!
//! **The keyboard is the view's**, with two things the application keeps (the
//! protocol's §11.3). ⌃C goes to the program as the byte 0x03, wherever the focus
//! is in the view — the person's one way out of a view that has stopped answering.
//! And the application's own shortcuts — a tab, a window, a split, the palette,
//! settings, quitting — never reach the view: `performKeyEquivalent:` asks the pane's
//! owner whether the key is one ([`bt_shell_common::shortcuts::Resolved::kept_from_view`],
//! read from the shortcut table and the person's `[keys]`, so there is no second list of
//! them here) and hands it to the main menu. Every other key is the view's, ⌘F included.
//! The commands that work on the pane's **content** (Edit, scrolling, text size) are
//! the view's while it is shown: their keys go to it and their menu items are grey
//! for the pane ([`is_content_item`]).
//!
//! **A strip of the application's own** stands at the top of the pane, outside the
//! view's area ([`strip`]): that a view is shown, where its program runs and how to
//! leave. The view's area is the pane's less the strip ([`view_area`]).
//!
//! **What the view asks of the person** is asked in the pane's own sheet ([`ask`]): the
//! addresses and browser features it declared are allowed or refused before it starts, and
//! the answer is remembered until the application quits; a link it asks to open opens in the
//! person's browser on this Mac only after a yes. The words are
//! `bt_shell_common::view_ask`'s.
//!
//! Pane module: it names no application module (`make audit`); the pane it sits in
//! is found from the view tree, the way [`crate::view::BateriView`] finds it, and
//! what the pane does with a view (the actions, the frames that stop, the
//! deadlines) is `pane`'s.

pub(crate) mod ask;
mod files;
mod serve;
mod still;
mod strip;
mod wait;

#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use block2::DynBlock;
use bt_core::pencere::{Shelf, Size as ViewSize, ViewSource, ViewStart, system_random};
use bt_core::{Session, Theme};
use bt_shell_common::palette::{Modifiers, menu_item_id};
use bt_shell_common::shortcuts::{by_id, chord_of};
use bt_shell_common::view_store::DiskStore;
use bt_shell_common::view_strip::{self, Strip};
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{
    AllocAnyThread, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, Message,
    define_class, msg_send,
};
use objc2_app_kit::{
    NSApplication, NSAutoresizingMaskOptions, NSColor, NSEvent, NSEventModifierFlags, NSEventType,
    NSMenu, NSMenuItem, NSView, NSWindow,
};
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::CGImage;
use objc2_foundation::{
    NSBundle, NSData, NSDictionary, NSError, NSHTTPURLResponse, NSLocale, NSObject,
    NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSTimeZone, NSURL, NSURLRequest,
};
use objc2_web_kit::{
    WKFrameInfo, WKNavigation, WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate,
    WKScriptMessage, WKScriptMessageHandler, WKSecurityOrigin, WKUIDelegate, WKURLSchemeHandler,
    WKURLSchemeTask, WKUserContentController, WKWebView, WKWebViewConfiguration,
    WKWebsiteDataStore,
};
use objc2_web_kit::{WKMediaCaptureType, WKPermissionDecision};

use crate::child;
use crate::pane::TerminalPane;

pub(crate) use wait::Waits;

/// The name the shell page's script posts to ([`serve`]'s `host`): the one
/// script-message handler.
const HANDLER: &str = "pencere";

/// The byte a terminal writes for ⌃C: what the tty turns into the interrupt.
pub(crate) const INTERRUPT: u8 = 0x03;

/// The shelf of the files programs' bundles are made of, for every pane of the
/// process: on disk under the bundle's own cache folder — so a development build
/// and the installed application keep separate ones — and in memory when the
/// process has no bundle (`cargo run`). One store for all the panes, because a
/// program that opens a view in two of them sends its files once.
pub(crate) fn shelf() -> Shelf {
    static SHELF: OnceLock<Shelf> = OnceLock::new();
    SHELF
        .get_or_init(|| match cache_directory() {
            Some(directory) => DiskStore::shelf(directory),
            None => Shelf::in_memory(),
        })
        .clone()
}

/// `~/Library/Caches/<bundle identifier>/pencere`; `None` without a bundle or a
/// home to put it in.
fn cache_directory() -> Option<PathBuf> {
    let bundle = NSBundle::mainBundle().bundleIdentifier()?.to_string();
    let home = child::home()?;
    Some(home.join("Library/Caches").join(bundle).join("pencere"))
}

/// The person's language and region as a view is told them (BCP 47): the first
/// of the system's preferred languages.
pub(crate) fn locale_tag() -> Option<String> {
    NSLocale::preferredLanguages()
        .firstObject()
        .map(|tag| tag.to_string())
}

/// The person's time zone (IANA).
pub(crate) fn time_zone_name() -> Option<String> {
    Some(NSTimeZone::localTimeZone().name().to_string())
}

/// Whether a key is ⌃C and only ⌃C: Control held and no other modifier, and the
/// character it makes is the interrupt (`characters` gives it with the modifiers
/// applied, as [`crate::view::BateriView`] reads it).
fn is_interrupt(flags: NSEventModifierFlags, characters: Option<&str>) -> bool {
    let held = flags
        & (NSEventModifierFlags::Control
            | NSEventModifierFlags::Command
            | NSEventModifierFlags::Option
            | NSEventModifierFlags::Shift);
    held == NSEventModifierFlags::Control && characters == Some("\u{3}")
}

/// The modifiers of `flags` among the four a shortcut uses.
fn modifiers_of(flags: NSEventModifierFlags) -> Modifiers {
    Modifiers {
        control: flags.contains(NSEventModifierFlags::Control),
        option: flags.contains(NSEventModifierFlags::Option),
        shift: flags.contains(NSEventModifierFlags::Shift),
        command: flags.contains(NSEventModifierFlags::Command),
    }
}

/// Whether a menu item is a content command's ([`bt_shell_common::shortcuts::Command::content`]):
/// the table finds the command by the item's id, its selector without the colon and `:tag`
/// when the tag is not 0 — the palette's own `menu_item_id`, so the item, the palette's row and
/// the table speak about one command.
pub(crate) fn is_content_item(item: &NSMenuItem) -> bool {
    let Some(action) = item.action() else {
        return false;
    };
    let id = menu_item_id(&action.name().to_string_lossy(), item.tag());
    by_id(&id).is_some_and(|command| command.content)
}

/// What the program calls itself, as its view's start says (`serverInfo`'s title or name);
/// `None` when it gave none, or nothing is left of it. These are the words for the program in
/// the strip and in the questions, before what the pane read (`TerminalPane::view_strip`).
pub(crate) fn said_program(start: &ViewStart) -> Option<&str> {
    start
        .program
        .as_deref()
        .filter(|name| !name.trim().is_empty())
}

/// The view's area in a pane of `bounds` (the pane's unflipped points): all of it under the
/// strip. A pane shorter than the strip has none.
pub(crate) fn view_area(bounds: NSRect) -> NSRect {
    let height = (bounds.size.height - view_strip::HEIGHT).max(0.0);
    NSRect::new(bounds.origin, NSSize::new(bounds.size.width, height))
}

/// The size of the view's area in a pane of `bounds`, in CSS pixels — which are points — as the
/// conversation is told its container ([`bt_core::Session::set_view_size`]): the one place it
/// is worked out, for the size a session is opened with and for the one told afterwards.
pub(crate) fn content_size(bounds: NSRect) -> ViewSize {
    let area = view_area(bounds);
    ViewSize {
        width: area.size.width.max(0.0).floor() as u32,
        height: area.size.height.max(0.0).floor() as u32,
    }
}

/// Whether the window's keyboard is in a program's view or something inside one: the first
/// responder is a view's web view, or a view of its own page. Where the keyboard is, not which
/// pane has the focus: a pane that shows a view and has no keyboard in it (it is in the tab
/// bar, in a field of the window) is not "in the view".
pub(crate) fn keyboard_in_view(window: &NSWindow) -> bool {
    let mut at = window
        .firstResponder()
        .and_then(|responder| responder.downcast::<NSView>().ok());
    while let Some(view) = at {
        if view.isKindOfClass(PencereView::class()) {
            return true;
        }
        // SAFETY: reading the superview of a view on the main thread.
        at = unsafe { view.superview() };
    }
    false
}

/// The strip's place in a pane of `bounds`: the top [`view_strip::HEIGHT`] points, or all of a
/// pane that is shorter. With [`view_area`] it is the pane exactly.
fn strip_area(bounds: NSRect) -> NSRect {
    let height = bounds.size.height.clamp(0.0, view_strip::HEIGHT);
    NSRect::new(
        NSPoint::new(
            bounds.origin.x,
            bounds.origin.y + bounds.size.height - height,
        ),
        NSSize::new(bounds.size.width, height),
    )
}

define_class!(
    // SAFETY: `WKWebView` is designed for subclassing; this class has no ivar,
    // implements no `Drop` and is born with `initWithFrame:configuration:`.
    #[unsafe(super(WKWebView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriPencereView"]
    pub(crate) struct PencereView;

    unsafe impl NSObjectProtocol for PencereView {}

    impl PencereView {
        /// ⌃C goes to the program; every other key is the view's. Before the
        /// web view's own handling, which would give the keystroke to the page.
        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            let characters = event.characters().map(|text| text.to_string());
            if is_interrupt(event.modifierFlags(), characters.as_deref())
                && let Some(pane) = self.pane()
            {
                pane.interrupt_program();
                return;
            }
            // SAFETY: `NSResponder`'s `keyDown:`, the web view's own.
            let _: () = unsafe { msg_send![super(self), keyDown: event] };
        }

        /// Before the web view's own handling and before the menu: a key that is the
        /// application's goes to its main menu and the view never sees it; every other
        /// key is the web view's. Asked of every view in the window, so a view the
        /// keyboard is not in answers nothing.
        #[unsafe(method(performKeyEquivalent:))]
        fn perform_key_equivalent(&self, event: &NSEvent) -> bool {
            // No `return`: `define_class!` converts the `bool` at the end of the body.
            if !self.holds_keyboard() {
                false
            } else if event.r#type() == NSEventType::KeyDown && self.application_keeps(event) {
                // What the menu does with the key is the menu's (a grey item does nothing);
                // the key is not the view's either way, as the terminal's view swallows
                // what the menu did not catch.
                //
                // The menu may close the pane this view is in (⌘W), which lets go of the
                // view while this method is still running: hold it until it returns.
                let _held = self.retain();
                if let Some(menu) = NSApplication::sharedApplication(self.mtm()).mainMenu() {
                    menu.performKeyEquivalent(event);
                }
                true
            } else {
                // SAFETY: `NSView`'s `performKeyEquivalent:`, the web view's own.
                let handled: bool = unsafe { msg_send![super(self), performKeyEquivalent: event] };
                handled
            }
        }

        /// The keyboard came to the view: the pane's owner hears it as it does
        /// for the terminal (`TerminalPane::keyboard_moved`).
        #[unsafe(method(becomeFirstResponder))]
        fn become_first_responder(&self) -> bool {
            // SAFETY: `NSResponder`'s argumentless method returning `BOOL`.
            let accepted: bool = unsafe { msg_send![super(self), becomeFirstResponder] };
            if accepted && let Some(pane) = self.pane() {
                pane.keyboard_moved(true);
            }
            accepted
        }

        /// The keyboard left the view.
        #[unsafe(method(resignFirstResponder))]
        fn resign_first_responder(&self) -> bool {
            // SAFETY: `NSResponder`'s argumentless method returning `BOOL`.
            let resigned: bool = unsafe { msg_send![super(self), resignFirstResponder] };
            if resigned && let Some(pane) = self.pane() {
                pane.keyboard_moved(false);
            }
            resigned
        }

        /// No context menu: its Reload would load the shell page again, and
        /// nothing in it is the application's.
        #[unsafe(method_id(menuForEvent:))]
        fn menu_for_event(&self, _event: &NSEvent) -> Option<Retained<NSMenu>> {
            None
        }
    }
);

impl PencereView {
    /// Whether the window's keyboard is in this view or something inside it.
    fn holds_keyboard(&self) -> bool {
        let Some(window) = self.window() else {
            return false;
        };
        window
            .firstResponder()
            .and_then(|responder| responder.downcast::<NSView>().ok())
            .is_some_and(|view| view.isDescendantOf(self))
    }

    /// Whether the key of `event` is one of the application's shortcuts, which the view does
    /// not get: the pane's owner reads it from the shortcut table. A key with none of ⌘, ⌃ and
    /// ⌥ is nobody's shortcut. Read as the menu reads a key, the character it makes without
    /// the modifiers (`shortcuts::chord_of`, the recorder's rule too).
    fn application_keeps(&self, event: &NSEvent) -> bool {
        let modifiers = modifiers_of(event.modifierFlags());
        if !(modifiers.command || modifiers.control || modifiers.option) {
            return false;
        }
        let characters = event
            .charactersIgnoringModifiers()
            .map(|text| text.to_string())
            .unwrap_or_default();
        let Some(chord) = chord_of(&characters, modifiers) else {
            return false;
        };
        self.pane()
            .is_some_and(|pane| pane.host().keeps_key(&chord))
    }

    /// The pane this view covers: its superview, as for the terminal's view.
    fn pane(&self) -> Option<Retained<TerminalPane>> {
        // SAFETY: reading the superview; the `Retained` keeps it alive for the
        // caller and this is the main thread (`MainThreadOnly`).
        let parent = unsafe { self.superview() }?;
        parent.downcast::<TerminalPane>().ok()
    }
}

/// What the bridge of one view holds.
struct BridgeIvars {
    /// The conversation with the program: where the view's messages go, and
    /// where the files it is made of come from.
    session: Arc<Session>,
    /// What is served and what may be loaded ([`serve::Page`]); shared with the file queue,
    /// which makes the answer for a file.
    page: Arc<serve::Page>,
    /// The shell page was asked for as the top document: it is loaded once.
    shell_loaded: std::cell::Cell<bool>,
    /// The tab card's picture of the view, shared with the host: the bridge takes the first
    /// one when the shell has loaded ([`still::first`]).
    shot: still::Shared,
}

define_class!(
    // SAFETY: `NSObject` has no subclassing condition; no `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriPencereBridge"]
    #[ivars = BridgeIvars]
    struct Bridge;

    unsafe impl NSObjectProtocol for Bridge {}

    unsafe impl WKURLSchemeHandler for Bridge {
        /// A request for the shell page or a file of the view: the shell page is answered
        /// at once, from memory; a file of the bundle is held and answered on the main
        /// queue when it has been read off it.
        #[unsafe(method(webView:startURLSchemeTask:))]
        fn start_task(&self, _web_view: &WKWebView, task: &ProtocolObject<dyn WKURLSchemeTask>) {
            self.answer(task);
        }

        /// The web view gave a request up: a task that waits for its file is not answered when
        /// the file is ready (AppKit raises when a stopped task is).
        #[unsafe(method(webView:stopURLSchemeTask:))]
        fn stop_task(&self, _web_view: &WKWebView, task: &ProtocolObject<dyn WKURLSchemeTask>) {
            let stopped = std::ptr::from_ref(task);
            WAITING.with(|waiting| {
                waiting
                    .borrow_mut()
                    .stop(|held| Retained::as_ptr(held) == stopped);
            });
        }
    }

    unsafe impl WKScriptMessageHandler for Bridge {
        /// A message of the view, as the text the shell page serialised it to.
        /// Only the top frame's: the handler's name is reachable from the view's
        /// own frame too, and what it posts is not the shell's.
        #[unsafe(method(userContentController:didReceiveScriptMessage:))]
        fn receive(&self, _controller: &WKUserContentController, message: &WKScriptMessage) {
            // SAFETY: plain reads of a message the web view just delivered.
            let top = unsafe { message.frameInfo().isMainFrame() };
            if !top {
                return;
            }
            // SAFETY: as above.
            let body = unsafe { message.body() };
            if let Some(text) = body.downcast_ref::<NSString>()
                && self.ivars().session.view_message(&text.to_string())
            {
                // A request was passed on to the program: the pane's clock runs until it
                // answers.
                // SAFETY: a plain read of the message the web view just delivered.
                let web = unsafe { message.webView() };
                if let Some(pane) = web
                    .and_then(|web| web.downcast::<PencereView>().ok())
                    .and_then(|view| view.pane())
                {
                    pane.view_requests_out();
                }
            }
        }
    }

    unsafe impl WKNavigationDelegate for Bridge {
        /// Every navigation of the web view is asked here: the shell once, the
        /// view's own origin in its frame, and nothing else.
        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        fn decide(
            &self,
            _web_view: &WKWebView,
            action: &WKNavigationAction,
            handler: &DynBlock<dyn Fn(WKNavigationActionPolicy)>,
        ) {
            let allowed = self.allows(action);
            handler.call((if allowed {
                WKNavigationActionPolicy::Allow
            } else {
                WKNavigationActionPolicy::Cancel
            },));
        }

        /// The shell page has loaded: the view's first picture is taken now, so a card that
        /// opens before it has asked for one has something of the view to show
        /// ([`still::first`]).
        #[unsafe(method(webView:didFinishNavigation:))]
        fn did_finish(&self, web_view: &WKWebView, _navigation: Option<&WKNavigation>) {
            still::first(web_view, self.mtm(), &self.ivars().shot);
        }

        /// The web content process died: a view that cannot answer its teardown
        /// is closed now and not after the wait for an answer.
        #[unsafe(method(webViewWebContentProcessDidTerminate:))]
        fn process_terminated(&self, _web_view: &WKWebView) {
            let session = &self.ivars().session;
            session.close_view();
            session.view_wait_over();
        }
    }

    unsafe impl WKUIDelegate for Bridge {
        /// No new window, whatever the view asks.
        #[unsafe(method_id(webView:createWebViewWithConfiguration:forNavigationAction:windowFeatures:))]
        fn create_web_view(
            &self,
            _web_view: &WKWebView,
            _configuration: &WKWebViewConfiguration,
            _action: &WKNavigationAction,
            _features: &objc2_web_kit::WKWindowFeatures,
        ) -> Option<Retained<WKWebView>> {
            None
        }

        /// No camera or microphone: what the person allows is the application's
        /// question, and it is not asked here.
        #[unsafe(method(webView:requestMediaCapturePermissionForOrigin:initiatedByFrame:type:decisionHandler:))]
        fn capture(
            &self,
            _web_view: &WKWebView,
            _origin: &WKSecurityOrigin,
            _frame: &WKFrameInfo,
            _kind: WKMediaCaptureType,
            handler: &DynBlock<dyn Fn(WKPermissionDecision)>,
        ) {
            handler.call((WKPermissionDecision::Deny,));
        }
    }
);

impl Bridge {
    fn new(
        mtm: MainThreadMarker,
        session: Arc<Session>,
        page: serve::Page,
        shot: still::Shared,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(BridgeIvars {
            session,
            page: Arc::new(page),
            shell_loaded: std::cell::Cell::new(false),
            shot,
        });
        // SAFETY: `NSObject`'s `init`.
        unsafe { msg_send![super(this), init] }
    }

    /// The URL of a request, as text.
    fn url_of(request: &NSURLRequest) -> Option<(Retained<NSURL>, String)> {
        let url = request.URL()?;
        let text = url.absoluteString()?.to_string();
        Some((url, text))
    }

    /// Answers one request of the web view. The shell page and what is refused come from memory,
    /// at once. **A file of the bundle is read off the main thread**: the file service waits for
    /// the conversation (busy while a file of 8 MiB is checked) and reads and checks one, so the
    /// task is held ([`WAITING`]), the reply is made on the file queue, and the task is answered
    /// on the main queue if it is still held then.
    fn answer(&self, task: &ProtocolObject<dyn WKURLSchemeTask>) {
        // SAFETY: the task is the web view's own and has not been stopped: it is being started.
        let request = unsafe { task.request() };
        let Some((_, text)) = Self::url_of(&request) else {
            fail(task);
            return;
        };
        let ivars = self.ivars();
        if ivars.page.file_of(&text).is_none() {
            Self::respond(task, &ivars.page.reply(&text, &|_| None));
            return;
        }
        let serial = WAITING.with(|waiting| {
            waiting
                .borrow_mut()
                .hold(std::ptr::from_ref(self) as usize, task.retain())
        });
        let (page, session) = (Arc::clone(&ivars.page), Arc::clone(&ivars.session));
        files_queue().exec_async(move || {
            let reply = read_file_reply(&page, &session, &text);
            DispatchQueue::main().exec_async(move || {
                let held = WAITING.with(|waiting| waiting.borrow_mut().finish(serial));
                // Stopped by the web view, or its view closed: nobody to answer.
                if let Some(task) = held {
                    Self::respond(&task, &reply);
                }
            });
        });
    }

    /// Gives the task `reply` and ends it. The task is live: it was started and not stopped.
    fn respond(task: &ProtocolObject<dyn WKURLSchemeTask>, reply: &serve::Reply) {
        // SAFETY: as above.
        let request = unsafe { task.request() };
        let Some((url, _)) = Self::url_of(&request) else {
            fail(task);
            return;
        };
        let keys: Vec<Retained<NSString>> = reply
            .headers
            .iter()
            .map(|(name, _)| NSString::from_str(name))
            .collect();
        let values: Vec<Retained<NSString>> = reply
            .headers
            .iter()
            .map(|(_, value)| NSString::from_str(value))
            .collect();
        let key_refs: Vec<&NSString> = keys.iter().map(|key| &**key).collect();
        let fields = NSDictionary::from_retained_objects(&key_refs, &values);
        let response = NSHTTPURLResponse::initWithURL_statusCode_HTTPVersion_headerFields(
            NSHTTPURLResponse::alloc(),
            &url,
            isize::from(reply.status),
            Some(&NSString::from_str("HTTP/1.1")),
            Some(&fields),
        );
        let Some(response) = response else {
            fail(task);
            return;
        };
        // SAFETY: the task is live (above); the three calls are its protocol, in
        // its order.
        unsafe {
            task.didReceiveResponse(&response);
            if !reply.body.is_empty() {
                task.didReceiveData(&NSData::with_bytes(&reply.body));
            }
            task.didFinish();
        }
    }

    /// The view closed: the tasks it was holding for files are not answered.
    fn forget_tasks(&self) {
        WAITING.with(|waiting| {
            waiting
                .borrow_mut()
                .forget(std::ptr::from_ref(self) as usize);
        });
    }

    /// Whether the navigation the web view asks about is one it may make.
    fn allows(&self, action: &WKNavigationAction) -> bool {
        // SAFETY: plain reads of the action the web view just made.
        let (request, target) = unsafe { (action.request(), action.targetFrame()) };
        // A navigation with no target frame is a new window's.
        let Some(target) = target else {
            return false;
        };
        let Some((_, url)) = Self::url_of(&request) else {
            return false;
        };
        // SAFETY: a plain read.
        let top = unsafe { target.isMainFrame() };
        let ivars = self.ivars();
        let allowed = ivars.page.allows(&url, top, ivars.shell_loaded.get());
        if allowed && top {
            ivars.shell_loaded.set(true);
        }
        allowed
    }
}

/// Fails a task whose request or response could not be made. The error names
/// nothing: what the view asked for is not the view's to learn.
fn fail(task: &ProtocolObject<dyn WKURLSchemeTask>) {
    // SAFETY: a new error with a domain, a code and no user info.
    let error = unsafe {
        NSError::errorWithDomain_code_userInfo(&NSString::from_str("BateriPencere"), 1, None)
    };
    // SAFETY: the task is live; a failure ends it.
    unsafe { task.didFailWithError(&error) };
}

thread_local! {
    /// The web view's tasks that wait for a file of their bundle, on the main thread.
    static WAITING: RefCell<files::Pending<Retained<ProtocolObject<dyn WKURLSchemeTask>>>> =
        const { RefCell::new(files::Pending::new()) };
}

/// The serial queue the files of bundles are read on: one at a time, in the order the web view
/// asked, and never on the main thread.
fn files_queue() -> &'static DispatchRetained<DispatchQueue> {
    static QUEUE: OnceLock<DispatchRetained<DispatchQueue>> = OnceLock::new();
    QUEUE.get_or_init(|| DispatchQueue::new("dev.bateri.pencere.files", None))
}

/// The reply for a request of a file of the bundle: the file service's bytes and media type, or
/// the refusal. **Blocks** — on the conversation's lock and on the disk: the file queue's.
fn read_file_reply(page: &serve::Page, session: &Session, url: &str) -> serve::Reply {
    page.reply(url, &|path| session.serve_view_file(path))
}

/// A view's web view and the bridge it talks to: what a pane holds while a program
/// shows a view in it. A copy is another hold on the same two objects, so the pane
/// takes one out of its slot before it calls into AppKit, which may call back.
#[derive(Clone)]
pub(crate) struct ViewHost {
    web: Retained<PencereView>,
    /// The application's strip over the view's area ([`strip`]).
    strip: Retained<strip::StripView>,
    /// The delegate and the handlers (the web view holds its delegates weakly).
    bridge: Retained<Bridge>,
    /// The picture the tab's card has of the view ([`still`]).
    shot: still::Shared,
    /// What the program calls itself ([`said_program`]), for the questions the view asks
    /// after it started.
    program: Option<String>,
}

impl ViewHost {
    /// Starts the web view of `start` in the pane's area `frame` below a strip that says
    /// `text`, and has it load the shell. The pane's background (`theme`) shows before the
    /// view paints. `None` if the view cannot be started: a bundle with no entry, or no
    /// unpredictable bytes for the view's origin — the caller then closes the view.
    pub(crate) fn start(
        mtm: MainThreadMarker,
        session: &Arc<Session>,
        start: &ViewStart,
        frame: NSRect,
        theme: &Theme,
        text: &Strip,
    ) -> Option<Self> {
        let ground = theme.background_srgb();
        let document = match &start.source {
            ViewSource::Bundle => serve::Document::Bundle(session.view_entry()?),
            ViewSource::Document(html) => {
                serve::Document::Html(Arc::from(html.as_bytes().to_vec()))
            }
        };
        // The same source the token of an `open` comes from, and the same refusal when it fails.
        let random = system_random()?;
        let page = serve::Page::new(
            serve::instance(random),
            document,
            start.policy.clone(),
            &start.allow,
            &serve::css_color(ground),
        );
        let shot = still::Shared::default();
        let bridge = Bridge::new(mtm, Arc::clone(session), page, Rc::clone(&shot));

        // SAFETY: configuring a fresh configuration on the main thread; the
        // handlers are the bridge, which the host keeps alive.
        let configuration = unsafe {
            let configuration = WKWebViewConfiguration::new(mtm);
            // Nothing outlives the web view: no cookie, no storage, no cache.
            configuration.setWebsiteDataStore(&WKWebsiteDataStore::nonPersistentDataStore(mtm));
            configuration.setURLSchemeHandler_forURLScheme(
                Some(ProtocolObject::from_ref(&*bridge)),
                &NSString::from_str(serve::SCHEME),
            );
            configuration
                .userContentController()
                .addScriptMessageHandler_name(
                    ProtocolObject::from_ref(&*bridge),
                    &NSString::from_str(HANDLER),
                );
            configuration
        };

        let this = PencereView::alloc(mtm).set_ivars(());
        // SAFETY: `WKWebView`'s designated initializer.
        let web: Retained<PencereView> = unsafe {
            msg_send![super(this), initWithFrame: view_area(frame), configuration: &*configuration]
        };
        web.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        // SAFETY: setting delegates and flags of a web view this thread owns; the
        // delegates are weak and the bridge outlives the web view (the host
        // removes the web view before it drops the bridge).
        unsafe {
            web.setNavigationDelegate(Some(ProtocolObject::from_ref(&*bridge)));
            web.setUIDelegate(Some(ProtocolObject::from_ref(&*bridge)));
            web.setAllowsBackForwardNavigationGestures(false);
            web.setAllowsLinkPreview(false);
            web.setAllowsMagnification(false);
            web.setInspectable(false);
        }
        let strip = strip::StripView::new(mtm, strip_area(frame), text, theme);
        let host = Self {
            web,
            strip,
            bridge,
            shot,
            program: said_program(start).map(str::to_owned),
        };
        host.paint(theme);
        // SAFETY: a request for the shell's own URL.
        let loaded = unsafe {
            NSURL::URLWithString(&NSString::from_str(&serve::Page::shell_url()))
                .map(|url| host.web.loadRequest(&NSURLRequest::requestWithURL(&url)))
        };
        loaded.map(|_| host)
    }

    /// What the program that opened the view calls itself, when it said.
    pub(crate) fn program(&self) -> Option<&str> {
        self.program.as_deref()
    }

    /// The web view, to put in the pane.
    pub(crate) fn web(&self) -> &NSView {
        &self.web
    }

    /// The strip over it, to put in the pane beside it.
    pub(crate) fn strip(&self) -> &NSView {
        &self.strip
    }

    /// Points the strip's button at the pane (`quitProgram:`).
    pub(crate) fn aim(&self, pane: &TerminalPane) {
        self.strip.aim(pane);
    }

    /// Runs a call of the shell page's script.
    fn run(&self, script: &str) {
        // SAFETY: main thread; no completion handler, so nothing comes back.
        unsafe {
            self.web
                .evaluateJavaScript_completionHandler(&NSString::from_str(script), None);
        }
    }

    /// Hands the view one message of the session's (JSON text).
    pub(crate) fn deliver(&self, message: &str) {
        self.run(&serve::deliver_script(message));
    }

    /// Gives the keyboard to the view's frame: the page inside the web view
    /// decides which element has it, and after the web view became first
    /// responder that is still the shell page's body.
    pub(crate) fn focus_page(&self) {
        self.run(serve::FOCUS_SCRIPT);
    }

    /// The ground behind the view is the pane's background (`theme`): the web view's own
    /// under-page colour and the shell page's. The strip is painted with the theme too.
    pub(crate) fn paint(&self, theme: &Theme) {
        self.strip.paint(theme);
        let ground = theme.background_srgb();
        let [r, g, b] = ground.map(|byte| f64::from(byte) / 255.0);
        // SAFETY: setting a colour on a web view this thread owns.
        unsafe {
            self.web.setUnderPageBackgroundColor(Some(
                &NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 1.0),
            ));
        }
        self.run(&serve::ground_script(&serve::css_color(ground)));
    }

    /// Whether the keyboard is in the view: the window's first responder is the
    /// web view or something inside it.
    pub(crate) fn holds_keyboard(&self) -> bool {
        self.web.holds_keyboard()
    }

    /// The picture of the view for a card's tile of `tile` pixels at `scale` pixels to the
    /// point: the last one the web view gave, and a request for the next
    /// ([`still`]). `None` until the first has come.
    pub(crate) fn still(&self, tile: (u32, u32), scale: f64) -> Option<CFRetained<CGImage>> {
        let (image, ask) = still::Still::call(&self.shot, Instant::now());
        if let Some(generation) = ask {
            self.shoot(tile, scale, generation);
        }
        image
    }

    /// The card is done with the pictures: the last one is let go.
    pub(crate) fn release_still(&self) {
        still::Still::release(&self.shot);
    }

    /// Asks the web view for its page as a picture of the tile's size, cropped to the
    /// tile's shape ([`still::capture`]). The answer comes later, on the main queue.
    fn shoot(&self, tile: (u32, u32), scale: f64, generation: u64) {
        let bounds = self.web.bounds().size;
        let Some(capture) = still::capture((bounds.width, bounds.height), scale, tile) else {
            still::Still::answered(&self.shot, generation, None);
            return;
        };
        let shared = Rc::clone(&self.shot);
        still::shoot(&self.web, self.web.mtm(), capture, move |image| {
            still::Still::answered(&shared, generation, image);
        });
    }

    /// Takes the web view down: stops it loading, takes it out of the pane and
    /// lets go of what it held. The bridge goes with the host.
    pub(crate) fn close(&self) {
        // SAFETY: main thread; the web view and its configuration are ours.
        unsafe {
            self.web.stopLoading();
            self.web.setNavigationDelegate(None);
            self.web.setUIDelegate(None);
            self.web
                .configuration()
                .userContentController()
                .removeScriptMessageHandlerForName(&NSString::from_str(HANDLER));
        }
        self.web.removeFromSuperview();
        self.strip.removeFromSuperview();
        // The files that were being read for it are for nobody now.
        self.bridge.forget_tasks();
        // The bridge is held until here on purpose: the web view held its
        // delegates weakly.
        let _ = &self.bridge;
    }
}
