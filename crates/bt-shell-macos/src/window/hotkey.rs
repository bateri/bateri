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

use std::time::Instant;

use bt_shell_common::hotkey::{
    Desktop, Desktops, HideCause, SLIDE_MILLIS, Slide, restores_previous,
};
use dispatch2::DispatchTime;
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
    /// When the window began to hand the front to that application, until it is out of sight:
    /// the window is ordered out only once bateri has stopped being the active application
    /// ([`HANDOVER_WAIT`]).
    handed: Cell<Option<Instant>>,
}

/// How long ordering the window out waits for the application it gave the front to to have it.
/// On macOS 26 that application is in front a few turns of the run loop after it was asked, and
/// a window ordered out while bateri is still the active one brings another window of bateri in
/// front of it for a moment (iTerm2's note: it waits for the application to stop being active, and
/// goes on after 0.3 s whatever happened).
const HANDOVER_WAIT: std::time::Duration = std::time::Duration::from_millis(300);

impl State {
    pub(super) fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            shown: Cell::new(false),
            turn: Cell::new(0),
            previous: RefCell::new(None),
            handed: Cell::new(None),
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
    window.setCollectionBehavior(behavior(Desktops::Front));
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
/// out of the window cycle (`ignoresCycle`), and on the desktops [`Desktops`] says
/// (`[hotkey_window] spaces` and what keeps the window in sight).
pub(crate) fn behavior(desktops: Desktops) -> NSWindowCollectionBehavior {
    let always =
        NSWindowCollectionBehavior::FullScreenAuxiliary | NSWindowCollectionBehavior::IgnoresCycle;
    match desktops {
        Desktops::Its => always,
        Desktops::Front => always | NSWindowCollectionBehavior::MoveToActiveSpace,
        Desktops::Every => always | NSWindowCollectionBehavior::CanJoinAllSpaces,
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

    /// Where the window is, to the desktop in front: the system's own answer, read now
    /// (`isOnActiveSpace`). Meant for a window in sight: one out of sight is on none.
    pub(crate) fn desktop(&self) -> Desktop {
        if self.ivars().window.isOnActiveSpace() {
            Desktop::Here
        } else {
            Desktop::Elsewhere
        }
    }

    /// The window's frame as a rule frame.
    pub(crate) fn frame_now(&self) -> crate::restore::Frame {
        frame_of(self.ivars().window.frame())
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
        desktops: Desktops,
        animate: bool,
        previous: Option<Retained<NSRunningApplication>>,
    ) {
        let Some(state) = self.ivars().hotkey.as_ref() else {
            return;
        };
        let turn = state.turn.get() + 1;
        state.turn.set(turn);
        state.shown.set(true);
        state.handed.set(None);
        if previous.is_some() {
            state.previous.replace(previous);
        }
        let window = self.ivars().window.clone();
        window.setCollectionBehavior(behavior(desktops));
        let (away, shown) = (rect_of(slide.away), rect_of(slide.shown));
        let fades = animate && slide.fade;
        if fades {
            // It comes where it stays, out of sight only because it is clear.
            window.setLevel(NSFloatingWindowLevel);
            window.setAlphaValue(0.0);
            window.setFrame_display(shown, false);
        } else if animate {
            window.setLevel(sliding_level());
            self.visible_at_once();
            window.setFrame_display(away, false);
        } else {
            window.setLevel(NSFloatingWindowLevel);
            self.visible_at_once();
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
            move || {
                if fades {
                    moving.animator().setAlphaValue(1.0);
                } else {
                    moving.animator().setFrame_display(shown, true);
                }
            },
            move || this.slid_in(turn),
        );
    }

    /// The window is opaque now, whatever fade was under way: a fade out that is cut short by a
    /// slide in would otherwise go on to its end (alpha 0) over the window that came back. Set
    /// through a group of no duration, which replaces an animation in flight.
    fn visible_at_once(&self) {
        let window = self.ivars().window.clone();
        if window.alphaValue() >= 1.0 {
            return;
        }
        card::animate(0.0, move || window.animator().setAlphaValue(1.0), || {});
    }

    /// The end of a slide in: floats from here on, unless another slide began meanwhile.
    fn slid_in(&self, turn: u64) {
        let Some(state) = self.ivars().hotkey.as_ref() else {
            return;
        };
        if state.turn.get() == turn && state.shown.get() {
            self.ivars().window.setLevel(NSFloatingWindowLevel);
            self.ivars().window.setAlphaValue(1.0);
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
        // The record of where the person was is let go whatever the hide was; the application
        // comes forward only when `give_back` ([`bt_shell_common::hotkey::restores_previous`]).
        let previous = state.previous.take().filter(|_| give_back);
        let window = self.ivars().window.clone();
        let away = rect_of(slide.away);
        // Handed at once, not at the slide's end: the keyboard is the application's while the
        // window leaves, and before it is ordered out.
        if let Some(previous) = previous {
            state.handed.set(Some(Instant::now()));
            previous.activateWithOptions(NSApplicationActivationOptions::empty());
        }
        if animate {
            let fades = slide.fade;
            if !fades {
                window.setLevel(sliding_level());
            }
            let moving = window.clone();
            let this = self.retain();
            card::animate(
                SLIDE_MILLIS as f64 / 1000.0,
                move || {
                    if fades {
                        moving.animator().setAlphaValue(0.0);
                    } else {
                        moving.animator().setFrame_display(away, true);
                    }
                },
                move || this.slid_out(turn),
            );
        } else {
            self.slid_out(turn);
        }
    }

    /// Whether ordering this window out would bring another window of bateri forward: one that
    /// can be seen, is not minimized or transparent, and can take the keyboard
    /// (iTerm2's `hasPromotableBackgroundWindow`). With none the window goes at once.
    fn another_window_would_come_forward(&self) -> bool {
        let this = &self.ivars().window;
        NSApplication::sharedApplication(self.mtm())
            .windows()
            .iter()
            .any(|other| {
                !std::ptr::eq(&*other, &**this)
                    && other.isVisible()
                    && !other.isMiniaturized()
                    && other.alphaValue() > 0.0
                    && other.canBecomeKeyWindow()
                    && other.isOnActiveSpace()
            })
    }

    /// The end of a slide out: out of sight, unless another slide began meanwhile.
    fn slid_out(&self, turn: u64) {
        let Some(state) = self.ivars().hotkey.as_ref() else {
            return;
        };
        if state.turn.get() != turn || state.shown.get() {
            return;
        }
        // Another application was given the front: ordering this window out while bateri is the
        // active application would bring another window of bateri in front of it for a moment.
        // The wait ends when bateri is no longer active, or after [`HANDOVER_WAIT`].
        if let Some(since) = state.handed.get()
            && since.elapsed() < HANDOVER_WAIT
            && NSApplication::sharedApplication(self.mtm()).isActive()
            && self.another_window_would_come_forward()
            && let Ok(when) = DispatchTime::try_from(std::time::Duration::from_millis(30))
        {
            let id = self.id();
            let waiting = DispatchQueue::main().after(when, move || {
                // audit: a block running on the main queue is on the main thread by definition.
                let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                if let Some(window) = app::delegate(mtm).and_then(|app| app.window(id)) {
                    window.slid_out(turn);
                }
            });
            // A wait that could not be set goes on to take the window out of sight.
            if waiting.is_ok() {
                return;
            }
        }
        state.handed.set(None);
        let window = &self.ivars().window;
        window.orderOut(None);
        window.setLevel(NSFloatingWindowLevel);
        // The next slide in begins from a window that can be seen.
        window.setAlphaValue(1.0);
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

    /// The desktops the window belongs to changed (the pin came or went): only its Space policy.
    pub(crate) fn set_desktops(&self, desktops: Desktops) {
        self.ivars()
            .window
            .setCollectionBehavior(behavior(desktops));
    }

    /// The settings changed while the window is in sight: its new frame and Space policy at once,
    /// with no slide.
    pub(crate) fn replace(&self, slide: &Slide, desktops: Desktops) {
        let window = &self.ivars().window;
        window.setCollectionBehavior(behavior(desktops));
        window.setFrame_display(rect_of(slide.shown), true);
        self.ivars().root.lay_out();
    }

    /// The window is in sight with no keyboard (a pin that something else took the focus from):
    /// in front and key, without a slide.
    ///
    /// `previous` is the application the person is in now (`None` when it is bateri itself): the
    /// one the window gives the front back to when it is taken away from here, in place of the
    /// one from when it first came. The person is back from there, not from where they were an
    /// hour ago.
    pub(crate) fn raise_hotkey(&self, previous: Option<Retained<NSRunningApplication>>) {
        if let Some(state) = self.ivars().hotkey.as_ref() {
            state.previous.replace(previous);
        }
        let window = &self.ivars().window;
        window.makeKeyAndOrderFront(None);
        NSApplication::sharedApplication(self.mtm()).activate();
    }

    /// The window lets go of the application it was to give the front back to (its only tab left
    /// for another window of bateri: the person goes there).
    pub(crate) fn hotkey_forget_previous(&self) {
        if let Some(state) = self.ivars().hotkey.as_ref() {
            state.previous.take();
        }
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
        // The pin was this window's: the next one, fresh, is not pinned.
        if let Some(app) = app::delegate(self.mtm()) {
            app.hotkey_window_ended();
        }
        // Only the window the person was typing in gives the front back: a pinned one that
        // something else has the keyboard from ends without taking it from there.
        let in_front =
            was_shown && restores_previous(HideCause::Closed, self.ivars().window.isKeyWindow());
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
    // What a tab that leaves the hotkey window is given is this window (`with_tab` with no
    // hotkey name): the traffic lights shown, movable, at the ordinary level, in the window
    // cycle and in one Space — none of the panel's marks.
    for button in [
        NSWindowButton::CloseButton,
        NSWindowButton::MiniaturizeButton,
        NSWindowButton::ZoomButton,
    ] {
        check(
            plain
                .standardWindowButton(button)
                .is_some_and(|button| !button.isHidden()),
            "a plain window's traffic light is not in sight",
        );
    }
    check(plain.isMovable(), "a plain window cannot be moved");
    check(
        plain.level() != NSFloatingWindowLevel,
        "a plain window floats like the panel",
    );
    check(
        !plain.collectionBehavior().intersects(
            NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::IgnoresCycle
                | NSWindowCollectionBehavior::MoveToActiveSpace,
        ),
        "a plain window carries the panel's collection behavior",
    );
    check(
        !plain
            .styleMask()
            .contains(NSWindowStyleMask::NonactivatingPanel),
        "a plain window is a non-activating panel",
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
    // AppKit puts a window on whole points: half a point of the rules' frame (a depth of 50% of
    // a screen whose visible height is odd) is no move.
    let near = |got: crate::restore::Frame, wanted: crate::restore::Frame| {
        (got.x - wanted.x).abs() <= 1.0
            && (got.y - wanted.y).abs() <= 1.0
            && (got.width - wanted.width).abs() <= 1.0
            && (got.height - wanted.height).abs() <= 1.0
    };
    panel.setFrame_display(rect_of(slide.away), false);
    check(
        near(frame_of(panel.frame()), slide.away),
        &format!(
            "the frame beyond the edge was moved onto the screen (wanted {:?}, got {:?}; visible {:?})",
            slide.away,
            frame_of(panel.frame()),
            screen.visible
        ),
    );
    panel.setFrame_display(rect_of(slide.shown), false);
    check(
        near(frame_of(panel.frame()), slide.shown),
        &format!(
            "the shown frame is not the rules' frame (wanted {:?}, got {:?})",
            slide.shown,
            frame_of(panel.frame())
        ),
    );
    check(
        (panel.frame().size.width - rect_of(slide.away).size.width).abs() <= 1.0
            && (panel.frame().size.height - rect_of(slide.away).size.height).abs() <= 1.0,
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
