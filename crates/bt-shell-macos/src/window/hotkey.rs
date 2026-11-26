//! The hotkey window as a [`TerminalWindow`]: the panel it is built on, the slide that brings it
//! in and takes it away, and what it gives back when it goes.
//!
//! **It is the same window.** The tab strip, ⌘T, ⌘D, the side column and the moves all run
//! through the one `TerminalWindow` ([`TerminalWindow::with_tab`] builds it through
//! [`make_window`]); what the hotkey window adds is here. Where it goes, and when, is the
//! application's call (`app::hotkey`, from the rules in [`bt_shell_common::hotkey`]); this file
//! is what one window does when it is told.
//!
//! **The panel.** An `NSPanel` with `NonactivatingPanel` from its birth (a mask added later may
//! leave the activation as it was), so that showing it does not bring bateri's other windows in
//! front of the application the person was in. It becomes key all the same, and bateri is
//! activated after it ([`TerminalWindow::slide_in`]): the menu bar, ⌘T, ⌘D and ⇧⌘P are the
//! person's the moment it is there. `hidesOnDeactivate` is off (the panel's default is on), or a
//! pin could not keep it when bateri goes behind. `fullScreenAuxiliary` and `ignoresCycle` are
//! always set; `moveToActiveSpace` is the `follow` policy.
//!
//! **The slide** moves only the window's place; its size is the same at both ends, so the panes
//! never see a resize in it. The window is above the menu bar while it moves and floats after.
//! Hiding is `orderOut`: the occlusion path stops the panes' links, so a hidden window draws no
//! frame.
//!
//! **Not movable by hand.** [`is_panel`] is the one question the places that would move a
//! window ask (`tab_bar::move_window`); `movable = NO` is not used, because it would also take
//! away Move & Resize (`rules/sekmeler.md`).

use bt_shell_common::hotkey::{SLIDE_MILLIS, Slide};
use objc2::runtime::Bool;
use objc2::{ClassType, sel};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSApplicationActivationOptions, NSMainMenuWindowLevel,
    NSMenuItem, NSMenuItemValidation, NSPanel, NSRunningApplication, NSScreen,
    NSWindowAnimationBehavior, NSWindowButton, NSWindowCollectionBehavior, NSWorkspace,
};
use objc2_foundation::ns_string;

use super::*;
use crate::card;

define_class!(
    /// The panel a hotkey window is on. AppKit keeps a titled window's frame on the screen when
    /// it is set or ordered in (`constrainFrameRect:toScreen:`); a window that slides in from
    /// beyond the edge must start there, so the frame is taken as it is given.
    // SAFETY: `NSPanel` has no subclassing requirement beyond the override below, which is the
    // documented way to take the frame as given; no ivars and no `Drop`.
    #[unsafe(super(NSPanel))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriHotkeyPanel"]
    pub(crate) struct HotkeyPanel;

    impl HotkeyPanel {
        #[unsafe(method(constrainFrameRect:toScreen:))]
        fn constrain_frame_rect(&self, frame: NSRect, _screen: Option<&NSScreen>) -> NSRect {
            frame
        }
    }
);

/// What a hotkey window keeps beside a normal one's state.
pub(super) struct State {
    /// The window's name (`default`: the only one today, `[hotkey_window]`'s).
    name: String,
    /// In sight: set when a slide in begins, cleared when a slide out does. The window's own
    /// word, not AppKit's `isVisible`, which a slide in the middle answers differently.
    shown: Cell<bool>,
    /// The number of the latest slide: the end of one that was overtaken does nothing.
    turn: Cell<u64>,
    /// The application the person was in before the window came, to give the front back to.
    previous: RefCell<Option<Retained<NSRunningApplication>>>,
}

impl State {
    pub(super) fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            shown: Cell::new(false),
            turn: Cell::new(0),
            previous: RefCell::new(None),
        }
    }
}

/// Builds the `NSWindow` of a terminal window: the plain one, or the hotkey window's panel
/// ([`configure`] applied). **The one setup** the shell and the panel's check share
/// (`tests/hotkey_panel.rs`).
pub(crate) fn make_window(mtm: MainThreadMarker, rect: NSRect, hotkey: bool) -> Retained<NSWindow> {
    // The content reaches under the title row: the tab bar is drawn in it.
    let style = NSWindowStyleMask::Titled
        | NSWindowStyleMask::Closable
        | NSWindowStyleMask::Resizable
        | NSWindowStyleMask::FullSizeContentView;
    if !hotkey {
        let style = style | NSWindowStyleMask::Miniaturizable;
        // SAFETY: with defer=false the window is created immediately. The constructor is unsafe
        // because of `releasedWhenClosed`: without a window controller AppKit releases the window
        // on close and the Retained in `WindowIvars.window` would dangle; the caller turns it off.
        return unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect,
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
    }
    // No Miniaturizable: Window ▸ Minimize is grey of itself.
    let style = style | NSWindowStyleMask::NonactivatingPanel;
    // SAFETY: `initWithContentRect:styleMask:backing:defer:` is `NSPanel`'s (inherited
    // `NSWindow`'s) designated initialiser, the arguments as the plain window's.
    let panel: Retained<HotkeyPanel> = unsafe {
        msg_send![
            HotkeyPanel::alloc(mtm),
            initWithContentRect: rect,
            styleMask: style,
            backing: NSBackingStoreType::Buffered,
            defer: Bool::NO,
        ]
    };
    let window = panel.into_super().into_super();
    configure(&window);
    window
}

/// The hotkey panel's look and standing, before it is ever shown.
fn configure(window: &NSWindow) {
    // The panel's default is to go with the application; a pin must outlive bateri's being
    // behind, and the focus rules are the window's own ([`TerminalWindow::focus_lost`]).
    window.setHidesOnDeactivate(false);
    window.setLevel(NSFloatingWindowLevel);
    window.setCollectionBehavior(behavior(true));
    // The slide is the only animation: no fade or zoom of AppKit's own on ordering in.
    window.setAnimationBehavior(NSWindowAnimationBehavior::None);
    // No lights: it is not closed, minimized or zoomed by them. The row they stood in is the
    // tab bar's ([`crate::tab_bar`] starts the strip at the edge when the zoom button is hidden).
    for button in [
        NSWindowButton::CloseButton,
        NSWindowButton::MiniaturizeButton,
        NSWindowButton::ZoomButton,
    ] {
        if let Some(button) = window.standardWindowButton(button) {
            button.setHidden(true);
        }
    }
    // Window ▸ Zoom is grey with its button.
    if let Some(zoom) = window.standardWindowButton(NSWindowButton::ZoomButton) {
        zoom.setEnabled(false);
    }
}

/// The panel's `collectionBehavior`: over a full-screen application's Space (`fullScreenAuxiliary`),
/// out of the window cycle (`ignoresCycle`), and with the person's Space when `follow`
/// (`[hotkey_window] spaces`).
pub(crate) fn behavior(follow: bool) -> NSWindowCollectionBehavior {
    let always =
        NSWindowCollectionBehavior::FullScreenAuxiliary | NSWindowCollectionBehavior::IgnoresCycle;
    if follow {
        always | NSWindowCollectionBehavior::MoveToActiveSpace
    } else {
        always
    }
}

/// Whether `window` is a hotkey window's panel. The mask is the mark: it is given at birth and
/// nothing else of bateri's is a non-activating panel.
pub(crate) fn is_panel(window: &NSWindow) -> bool {
    window
        .styleMask()
        .contains(NSWindowStyleMask::NonactivatingPanel)
}

/// The application in front now, unless it is bateri itself: what the hotkey window gives the
/// front back to when it is taken away.
pub(crate) fn frontmost_other(mtm: MainThreadMarker) -> Option<Retained<NSRunningApplication>> {
    // The question is whether bateri is the one in front; its own `isActive` says so without the
    // process id (`NSRunningApplication`'s is behind a feature that adds a dependency edge).
    if NSApplication::sharedApplication(mtm).isActive() {
        return None;
    }
    NSWorkspace::sharedWorkspace().frontmostApplication()
}

/// A rule frame as AppKit's rectangle.
pub(crate) fn rect_of(frame: crate::restore::Frame) -> NSRect {
    NSRect::new(
        NSPoint::new(frame.x, frame.y),
        NSSize::new(frame.width, frame.height),
    )
}

/// An AppKit rectangle as a rule frame.
pub(crate) fn frame_of(rect: NSRect) -> crate::restore::Frame {
    crate::restore::Frame {
        x: rect.origin.x,
        y: rect.origin.y,
        width: rect.size.width,
        height: rect.size.height,
    }
}

/// The level a window has while it slides: above the menu bar, which it comes over and leaves
/// under.
fn sliding_level() -> isize {
    NSMainMenuWindowLevel + 1
}

impl TerminalWindow {
    /// The name of the hotkey window this is; `None` for a normal window.
    pub(crate) fn hotkey_name(&self) -> Option<&str> {
        self.ivars()
            .hotkey
            .as_ref()
            .map(|state| state.name.as_str())
    }

    /// Whether this is a hotkey window.
    pub(crate) fn is_hotkey(&self) -> bool {
        self.ivars().hotkey.is_some()
    }

    /// Whether this hotkey window is in sight (its own word: [`State::shown`]); a normal window
    /// is not a hotkey window and answers no.
    pub(crate) fn hotkey_shown(&self) -> bool {
        self.ivars()
            .hotkey
            .as_ref()
            .is_some_and(|state| state.shown.get())
    }

    /// Whether the window has the keyboard.
    pub(crate) fn is_key_window(&self) -> bool {
        self.ivars().window.isKeyWindow()
    }

    /// The window's frame as a rule frame.
    pub(crate) fn frame_now(&self) -> crate::restore::Frame {
        frame_of(self.ivars().window.frame())
    }

    /// The screen the window is on, if any.
    pub(crate) fn screen_now(&self) -> Option<Retained<NSScreen>> {
        self.ivars().window.screen()
    }

    /// A hotkey window that is born or comes back hidden: placed at `frame` (its shells see their
    /// final size first) and left out of sight, its panes told so.
    pub(crate) fn place_hidden(&self, frame: NSRect) {
        self.ivars().window.setFrame_display(frame, false);
        self.ivars().root.lay_out();
        let visible = self.window_visible();
        for tab in self.tabs() {
            tab.apply_visibility(visible);
        }
    }

    /// Slides the window in and gives it the keyboard. `previous` is the application the person
    /// was in (`None`: bateri itself, or when the window was in sight already and keeps what it
    /// had). `animate` is false for Reduce Motion and `animate = false`: it comes at once.
    pub(crate) fn slide_in(
        &self,
        slide: &Slide,
        follow: bool,
        animate: bool,
        previous: Option<Retained<NSRunningApplication>>,
    ) {
        let Some(state) = self.ivars().hotkey.as_ref() else {
            return;
        };
        let turn = state.turn.get() + 1;
        state.turn.set(turn);
        state.shown.set(true);
        if previous.is_some() {
            state.previous.replace(previous);
        }
        let window = self.ivars().window.clone();
        window.setCollectionBehavior(behavior(follow));
        let (away, shown) = (rect_of(slide.away), rect_of(slide.shown));
        if animate {
            window.setLevel(sliding_level());
            window.setFrame_display(away, false);
        } else {
            window.setLevel(NSFloatingWindowLevel);
            window.setFrame_display(shown, false);
        }
        // Key first, then bateri is activated: only this window comes to the front of the
        // application, not the others (`NSApplicationActivateAllWindows` is not given).
        window.makeKeyAndOrderFront(None);
        self.ivars().root.lay_out();
        NSApplication::sharedApplication(self.mtm()).activate();
        if !animate {
            return;
        }
        let moving = window.clone();
        let this = self.retain();
        card::animate(
            SLIDE_MILLIS as f64 / 1000.0,
            move || moving.animator().setFrame_display(shown, true),
            move || this.slid_in(turn),
        );
    }

    /// The end of a slide in: floats from here on, unless another slide began meanwhile.
    fn slid_in(&self, turn: u64) {
        let Some(state) = self.ivars().hotkey.as_ref() else {
            return;
        };
        if state.turn.get() == turn && state.shown.get() {
            self.ivars().window.setLevel(NSFloatingWindowLevel);
        }
    }

    /// Slides the window away and takes it out of sight. With `give_back` the application the
    /// person was in comes to the front (the shortcut and the command ended the window); without,
    /// the focus went elsewhere by itself and is left where it went.
    pub(crate) fn slide_out(&self, slide: &Slide, animate: bool, give_back: bool) {
        let Some(state) = self.ivars().hotkey.as_ref() else {
            return;
        };
        if !state.shown.replace(false) {
            return;
        }
        let turn = state.turn.get() + 1;
        state.turn.set(turn);
        let previous = state.previous.take().filter(|_| give_back);
        let window = self.ivars().window.clone();
        let away = rect_of(slide.away);
        if animate {
            window.setLevel(sliding_level());
            let moving = window.clone();
            let this = self.retain();
            card::animate(
                SLIDE_MILLIS as f64 / 1000.0,
                move || moving.animator().setFrame_display(away, true),
                move || this.slid_out(turn),
            );
        } else {
            self.slid_out(turn);
        }
        // Handed at once, not at the slide's end: the keyboard is the application's while the
        // window leaves.
        if let Some(previous) = previous {
            previous.activateWithOptions(NSApplicationActivationOptions::empty());
        }
    }

    /// The end of a slide out: out of sight, unless another slide began meanwhile.
    fn slid_out(&self, turn: u64) {
        let Some(state) = self.ivars().hotkey.as_ref() else {
            return;
        };
        if state.turn.get() == turn && !state.shown.get() {
            let window = &self.ivars().window;
            window.orderOut(None);
            window.setLevel(NSFloatingWindowLevel);
        }
    }

    /// Brings the window in front, with the keyboard if `keys`. **A hotkey window comes the way
    /// it comes** (its slide, on the pointer's screen, from the rule's decision), not by AppKit's
    /// order: it may be out of sight, and `orderFront` would show it where it was left.
    pub(crate) fn order_front(&self, keys: bool) {
        if self.is_hotkey() {
            if let Some(app) = app::delegate(self.mtm()) {
                app.show_hotkey_window(self);
            }
        } else if keys {
            self.ivars().window.makeKeyAndOrderFront(None);
        } else {
            self.ivars().window.orderFront(None);
        }
    }

    /// The settings changed while the window is in sight: its new frame and Space policy at once,
    /// with no slide.
    pub(crate) fn replace(&self, slide: &Slide, follow: bool) {
        let window = &self.ivars().window;
        window.setCollectionBehavior(behavior(follow));
        window.setFrame_display(rect_of(slide.shown), true);
        self.ivars().root.lay_out();
    }

    /// The window is in sight with no keyboard (a pin that something else took the focus from):
    /// in front and key, without a slide.
    pub(crate) fn raise_hotkey(&self) {
        let window = &self.ivars().window;
        window.makeKeyAndOrderFront(None);
        NSApplication::sharedApplication(self.mtm()).activate();
    }

    /// The window closes (its last tab went, or the application quits): what it owed the person
    /// is paid — the application they were in comes to the front if the window was the one in
    /// sight. The quit gives nothing back: bateri is going.
    pub(crate) fn hotkey_closing(&self, quitting: bool) {
        let Some(state) = self.ivars().hotkey.as_ref() else {
            return;
        };
        state.turn.set(state.turn.get() + 1);
        let was_shown = state.shown.replace(false);
        let previous = state.previous.take();
        // Only the window the person was typing in gives the front back: a pinned one that
        // something else has the keyboard from ends without taking it from there.
        let in_front = was_shown && self.ivars().window.isKeyWindow();
        if let (true, false, Some(previous)) = (in_front, quitting, previous) {
            previous.activateWithOptions(NSApplicationActivationOptions::empty());
        }
    }
}

/// What is wrong with the hotkey panel as [`make_window`] builds it, listed — `tests/hotkey_panel.rs`
/// opens it through **the same setup** the shell uses and reads it back: the class, the style, the
/// standing flags, the frame the rules compute (an off-screen one too: the slide starts there),
/// and the window going out of sight when the second tap hides it. Nothing here puts a window in
/// front of the person: the panel is ordered in beyond the screen's edge and out again.
pub(crate) fn panel_problems(mtm: MainThreadMarker) -> Vec<String> {
    use bt_shell_common::hotkey as rules;

    let mut problems = Vec::new();
    let mut check = |ok: bool, what: &str| {
        if !ok {
            problems.push(what.to_owned());
        }
    };
    let plain = make_window(
        mtm,
        NSRect::new(NSPoint::ZERO, NSSize::new(200.0, 100.0)),
        false,
    );
    check(!is_panel(&plain), "a plain window is taken for a panel");
    check(
        plain
            .styleMask()
            .contains(NSWindowStyleMask::Miniaturizable),
        "a plain window lost Minimize",
    );

    let first = NSScreen::screens(mtm).firstObject();
    let Some(screen) = first else {
        problems.push("there is no screen to place the panel on".to_owned());
        return problems;
    };
    let screen = rules::Screen {
        frame: frame_of(screen.frame()),
        visible: frame_of(screen.visibleFrame()),
    };
    let shown = rules::shown_frame(screen.visible, bt_core::HotkeyPosition::Top, 50, 100);
    let slide = rules::slide(&screen, shown, bt_core::HotkeyPosition::Top);
    let panel = make_window(mtm, rect_of(slide.away), true);
    check(
        is_panel(&panel),
        "the hotkey window is not a non-activating panel",
    );
    check(
        panel.isKindOfClass(NSPanel::class()),
        "the hotkey window is not an NSPanel",
    );
    check(
        !panel
            .styleMask()
            .contains(NSWindowStyleMask::Miniaturizable),
        "the hotkey window can be minimized",
    );
    check(
        !panel.hidesOnDeactivate(),
        "the panel hides when bateri goes behind",
    );
    let behavior = panel.collectionBehavior();
    check(
        behavior.contains(NSWindowCollectionBehavior::FullScreenAuxiliary),
        "the panel cannot join a full-screen Space",
    );
    check(
        behavior.contains(NSWindowCollectionBehavior::IgnoresCycle),
        "the panel is in the window cycle",
    );
    check(
        panel.level() == NSFloatingWindowLevel,
        "the panel does not float",
    );
    for button in [
        NSWindowButton::CloseButton,
        NSWindowButton::MiniaturizeButton,
        NSWindowButton::ZoomButton,
    ] {
        check(
            panel
                .standardWindowButton(button)
                .is_none_or(|button| button.isHidden()),
            "a traffic light is in sight",
        );
    }
    // The frames: the slide starts beyond the screen's edge and AppKit must leave it there.
    panel.setFrame_display(rect_of(slide.away), false);
    check(
        frame_of(panel.frame()) == slide.away,
        "the frame beyond the edge was moved onto the screen",
    );
    panel.setFrame_display(rect_of(slide.shown), false);
    check(
        frame_of(panel.frame()) == slide.shown,
        "the shown frame is not the rules' frame",
    );
    check(
        panel.frame().size == rect_of(slide.away).size,
        "the slide changes the size, not only the place",
    );
    // Window ▸ Minimize and Zoom, and Enter Full Screen, are grey for it: asked of the panel as
    // the menu bar asks.
    for action in [
        sel!(performMiniaturize:),
        sel!(performZoom:),
        sel!(toggleFullScreen:),
    ] {
        // SAFETY: a title, a selector and an empty key equivalent are what the initialiser asks.
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                ns_string!("check"),
                Some(action),
                ns_string!(""),
            )
        };
        check(
            !panel.validateMenuItem(&item),
            &format!("{action:?} is enabled for the hotkey window"),
        );
        // The control: the same question of a plain window says yes, so a "no" above is the
        // panel's and not the question's. (Full screen is asked of a window that is key and
        // `fullScreenPrimary`, which a bare window is not: its reason is checked below.)
        if action != sel!(toggleFullScreen:) {
            check(
                plain.validateMenuItem(&item),
                &format!("{action:?} is not enabled for a plain window either"),
            );
        }
    }
    check(
        !behavior.contains(NSWindowCollectionBehavior::FullScreenPrimary),
        "the panel can enter full screen",
    );
    // The second tap: a shown window with the keyboard is hidden by the rules; and a hidden
    // window is out of sight to AppKit. (The window's own `slide_out`, with the panes' occlusion
    // path behind it, is the person's to look at: it needs a window with a pane.)
    check(
        rules::decide(rules::Verb::Toggle, rules::Presence::Shown { key: true })
            == rules::Step::Hide,
        "the second tap does not hide the window",
    );
    // In and out beyond the screen's edge, where the slide starts: the check never puts a window
    // in front of the person.
    panel.setFrame_display(rect_of(slide.away), false);
    panel.orderFront(None);
    check(panel.isVisible(), "the panel did not come into sight");
    panel.orderOut(None);
    check(
        !panel.isVisible(),
        "the panel is still in sight after it was hidden",
    );
    problems
}
