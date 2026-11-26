//! The hotkey window from the application's side: the one entrance every trigger goes through
//! ([`AppDelegate::toggle_hotkey`]), where the window opens, how the focus leaving it is read,
//! and the choices among windows that must know one of them may be out of sight.
//!
//! **What is decided here is only wiring.** Which screen, what frame, what a tap does to a window
//! in each state, whether the focus leaving hides it, which window a new pane or a merge goes to:
//! all of it is [`bt_shell_common::hotkey`]'s, tried without a window. This file reads the
//! settings and AppKit, hands them to those rules, and does what they say. The window's own part
//! (the panel, the slide, what it gives back) is [`crate::window`]'s.

use bt_core::{HOTKEY_DEFAULT_NAME, HotkeySpaces, HotkeyWindowSettings, SettingsEdit};
use bt_shell_common::agent::message::HotkeyAction;
use bt_shell_common::hotkey::{
    self as rules, Candidate, GlobalKeys as _, KeyTaker, Lost, Presence, Step, Verb,
};
use objc2_app_kit::NSScreen;

use super::*;
use crate::window::{frame_of, frontmost_other, rect_of};

/// The bundle the agent tries bateri with: it never holds the system's key.
const AGENT_CHECK_BUNDLE: &str = "dev.bateri.agent-check";

/// System Settings ▸ Privacy & Security ▸ Input Monitoring: where a double tap is allowed.
const INPUT_MONITORING_PANE: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent";

impl AppDelegate {
    // -----------------------------------------------------------------------------------------
    // The windows.

    /// The hotkey window, if there is one (it exists from the first tap until its last tab
    /// closes).
    pub(crate) fn hotkey_window(&self) -> Option<Retained<TerminalWindow>> {
        self.windows().into_iter().find(|window| window.is_hotkey())
    }

    /// The windows in the order a choice reads them — the key window, the main one, then the
    /// others front to back as AppKit lists them, then those out of sight — and each as the
    /// rules see it.
    fn candidates(&self) -> (Vec<Retained<TerminalWindow>>, Vec<Candidate>) {
        let app = NSApplication::sharedApplication(self.mtm());
        let mut rest = self.windows();
        let mut ordered = Vec::with_capacity(rest.len());
        let mut take = |rest: &mut Vec<Retained<TerminalWindow>>,
                        wanted: &dyn Fn(&TerminalWindow) -> bool| {
            if let Some(at) = rest.iter().position(|window| wanted(window)) {
                ordered.push(rest.remove(at));
            }
        };
        let key = app.keyWindow().map(|key| key.sheetParent().unwrap_or(key));
        if let Some(key) = &key {
            take(&mut rest, &|window| window.owns(key));
        }
        if let Some(main) = app.mainWindow() {
            take(&mut rest, &|window| window.owns(&main));
        }
        if let Some(numbers) =
            NSWindow::windowNumbersWithOptions(NSWindowNumberListOptions(0), self.mtm())
        {
            for number in numbers.iter() {
                let number = number.integerValue();
                take(&mut rest, &|window| {
                    window.ns_window().windowNumber() == number
                });
            }
        }
        ordered.extend(rest);
        let candidates = ordered
            .iter()
            .map(|window| Candidate {
                hotkey: window.is_hotkey(),
                shown: !window.is_hotkey() || window.hotkey_shown(),
                key: key.as_deref().is_some_and(|key| window.owns(key)),
                main: app.mainWindow().is_some_and(|main| window.owns(&main)),
            })
            .collect();
        (ordered, candidates)
    }

    /// Whether the Dock's click, or the launch, has to open a normal window
    /// ([`rules::needs_normal_window`]).
    pub(crate) fn needs_normal_window(&self) -> bool {
        rules::needs_normal_window(&self.candidates().1)
    }

    /// Whether Merge All Windows has anything to do ([`rules::can_merge`]).
    pub(crate) fn can_merge_windows(&self) -> bool {
        rules::can_merge(&self.candidates().1)
    }

    /// The window a new pane goes to ([`rules::pane_window`]).
    pub(crate) fn pane_window(&self) -> Option<Retained<TerminalWindow>> {
        let (windows, candidates) = self.candidates();
        rules::pane_window(&candidates).and_then(|at| windows.get(at).cloned())
    }

    /// The front normal window ([`rules::front_normal`]): the one the smoke and a measurement
    /// start in.
    pub(crate) fn first_normal_window(&self) -> Option<Retained<TerminalWindow>> {
        let (windows, candidates) = self.candidates();
        rules::front_normal(&candidates).and_then(|at| windows.get(at).cloned())
    }

    // -----------------------------------------------------------------------------------------
    // Screens and frames.

    /// The screens as the rules read them: the first is the primary one.
    fn hotkey_screens(&self) -> Vec<rules::Screen> {
        NSScreen::screens(self.mtm())
            .iter()
            .map(|screen| rules::Screen {
                frame: frame_of(screen.frame()),
                visible: frame_of(screen.visibleFrame()),
            })
            .collect()
    }

    /// The slide of a window that is shown now, on the screen `screen` names and the pointer's
    /// place decides: its frame at the end and from where it comes.
    fn hotkey_slide(&self, settings: &HotkeyWindowSettings) -> Option<rules::Slide> {
        let screens = self.hotkey_screens();
        let pointer = NSEvent::mouseLocation();
        let index = rules::screen_index(&screens, settings.screen, Some((pointer.x, pointer.y)))?;
        let screen = screens.get(index)?;
        let shown = rules::shown_frame(
            screen.visible,
            settings.position,
            settings.depth,
            settings.span,
        );
        Some(rules::slide(screen, shown, settings.position))
    }

    /// The frame the hotkey window has when it is shown, for a window that is placed out of sight
    /// first ([`TerminalWindow::place_hidden`]).
    pub(crate) fn hotkey_frame_now(&self) -> Option<NSRect> {
        let settings = self.settings().hotkey_window;
        self.hotkey_slide(&settings)
            .map(|slide| rect_of(slide.shown))
    }

    /// The slide a window that is in sight goes away on: the screen it is on, its frame as it
    /// is. It goes out through the edge it came from.
    fn hotkey_slide_away(&self, window: &TerminalWindow) -> Option<rules::Slide> {
        let screens = self.hotkey_screens();
        let frame = window.frame_now();
        let on = window.screen_now().map(|screen| frame_of(screen.frame()));
        let screen = on
            .and_then(|on| screens.iter().find(|screen| screen.frame == on))
            .or_else(|| screens.first())?;
        Some(rules::slide(
            screen,
            frame,
            self.settings().hotkey_window.position,
        ))
    }

    /// Whether a slide happens: not with `animate = false`, nor under Reduce Motion
    /// ([`AppDelegate::reduce_motion`]: the setting and the system).
    fn hotkey_animates(&self, settings: &HotkeyWindowSettings) -> bool {
        settings.animate && !self.reduce_motion()
    }

    // -----------------------------------------------------------------------------------------
    // The triggers.

    /// **The one entrance of the hotkey window**: the shortcut, the double tap, the menu, the
    /// palette and `bateri hotkey` all come here. What happens to the window is
    /// [`rules::decide`]'s; the step taken is returned. What could not be done is said on the
    /// error output: the person who pressed a key has nobody to ask ([`Self::try_toggle_hotkey`]
    /// is for the one who does, `bateri hotkey`).
    pub(crate) fn toggle_hotkey(&self, verb: Verb) -> Step {
        self.try_toggle_hotkey(verb).unwrap_or_else(|e| {
            eprintln!("bateri: could not show the hotkey window: {e}");
            Step::Nothing
        })
    }

    /// [`Self::toggle_hotkey`], with the reason the window could not be shown (no screen, a
    /// window that could not be built) for a caller that answers someone.
    pub(crate) fn try_toggle_hotkey(&self, verb: Verb) -> Result<Step, String> {
        if self.is_terminating() {
            return Ok(Step::Nothing);
        }
        let window = self.hotkey_window();
        let presence = match &window {
            None => Presence::Absent,
            Some(window) if !window.hotkey_shown() => Presence::Hidden,
            Some(window) => Presence::Shown {
                key: window.is_key_window(),
            },
        };
        let step = rules::decide(verb, presence);
        match (step, window) {
            (Step::Open, _) => self.open_hotkey_window()?,
            (Step::Show, Some(window)) => self.slide_in_hotkey(&window)?,
            (Step::Raise, Some(window)) => window.raise_hotkey(),
            (Step::Hide, Some(window)) => self.slide_out_hotkey(&window, true),
            _ => {}
        }
        Ok(step)
    }

    /// A window that something asks to bring forward — ⌘P and the palette, `bateri focus`,
    /// `bateri://`, the agent — comes the hotkey way ([`TerminalWindow::order_front`]).
    pub(crate) fn show_hotkey_window(&self, window: &TerminalWindow) {
        let presence = if window.hotkey_shown() {
            Presence::Shown {
                key: window.is_key_window(),
            }
        } else {
            Presence::Hidden
        };
        match rules::decide(Verb::Show, presence) {
            Step::Show => {
                if let Err(e) = self.slide_in_hotkey(window) {
                    eprintln!("bateri: could not show the hotkey window: {e}");
                }
            }
            Step::Raise => window.raise_hotkey(),
            _ => {}
        }
    }

    /// The window slides in on the screen the settings and the pointer choose.
    fn slide_in_hotkey(&self, window: &TerminalWindow) -> Result<(), String> {
        let settings = self.settings().hotkey_window;
        let slide = self
            .hotkey_slide(&settings)
            .ok_or_else(|| "there is no screen".to_owned())?;
        window.slide_in(
            &slide,
            settings.spaces == HotkeySpaces::Follow,
            self.hotkey_animates(&settings),
            frontmost_other(self.mtm()),
        );
        Ok(())
    }

    /// The window slides away. `give_back`: the application the person was in comes to the
    /// front (the tap and the command); without it the focus went elsewhere by itself.
    fn slide_out_hotkey(&self, window: &TerminalWindow, give_back: bool) {
        let settings = self.settings().hotkey_window;
        let Some(slide) = self.hotkey_slide_away(window) else {
            return;
        };
        window.slide_out(&slide, self.hotkey_animates(&settings), give_back);
    }

    /// The first tap with no window: a new one, one tab in the home folder, at its final size and
    /// out of sight, then slid in.
    fn open_hotkey_window(&self) -> Result<(), String> {
        let settings = self.settings().hotkey_window;
        let slide = self
            .hotkey_slide(&settings)
            .ok_or_else(|| "there is no screen".to_owned())?;
        // Said before the window is there: in front of it is the application the person is in.
        let previous = frontmost_other(self.mtm());
        let window = self.open_window_as(
            None,
            None,
            Opening::Window,
            None,
            false,
            Some((HOTKEY_DEFAULT_NAME, rect_of(slide.shown))),
        )?;
        window.slide_in(
            &slide,
            settings.spaces == HotkeySpaces::Follow,
            self.hotkey_animates(&settings),
            previous,
        );
        Ok(())
    }

    // -----------------------------------------------------------------------------------------
    // The system's key.

    /// What this instance is, to the rule that says whether it holds the key
    /// ([`rules::owns_global_keys`]). Asked **after** the session lock is taken
    /// (`restore_or_open_windows`).
    fn hotkey_instance(&self) -> rules::Instance {
        let bundle = NSBundle::mainBundle()
            .bundleIdentifier()
            .map(|id| id.to_string());
        rules::Instance {
            holds_session_lock: self.ivars().restore_lock.borrow().is_some(),
            bundled: bundle.is_some(),
            timed_run: self.ivars().run.is_some(),
            agent_check: bundle.as_deref() == Some(AGENT_CHECK_BUNDLE),
        }
    }

    /// The system's keys, built on first use. **The one place they are built** — and its callers
    /// ask first whether this instance owns them ([`rules::owns_global_keys`]).
    fn with_system_keys<R>(
        &self,
        work: impl FnOnce(&mut crate::global_keys::SystemKeys) -> R,
    ) -> R {
        let mut keys = self.ivars().global_keys.borrow_mut();
        work(keys.get_or_insert_with(crate::global_keys::SystemKeys::new))
    }

    /// Brings the system's hold on the key to what `settings` ask: at launch and when
    /// `[hotkey_window]` changes (`new`, the slot not written yet). **The one place the real
    /// keys are built**, and only for an instance that owns them: nothing else ever registers a
    /// key for the whole system. What could not be held is told with the settings' diagnostics.
    pub(crate) fn apply_global_keys(&self, settings: &Settings) {
        let instance = self.hotkey_instance();
        if !rules::owns_global_keys(&instance) {
            return;
        }
        let told = self.with_system_keys(|keys| {
            let mut registration = self.ivars().hotkey_registration.borrow_mut();
            registration.sync(keys, settings, &instance);
            registration.notices(settings.hotkey_window.shortcut)
        });
        self.post_notices(Source::Hotkey, told);
        // The page that shows why a key is not held shows it now.
        self.refresh_settings_window();
    }

    /// The person wants the double tap heard: asks the system for the permission (the system asks
    /// the person, once), asks for the key again, and — when the button sent this and the
    /// permission is still not there — opens the System Settings pane that gives it. **Only the
    /// Settings page calls this** (a double tap just recorded, or its button): a launch that asked
    /// would put a question on every run of every build ([`Self::hotkey_access_may_have_changed`]
    /// only asks the system whether it was given).
    pub(crate) fn allow_listening(&self, open_pane: bool) {
        if !self.retry_global_keys(true) {
            return;
        }
        if open_pane {
            crate::hyperlink::open_url(INPUT_MONITORING_PANE);
        }
    }

    /// Whether the key asked for waits for the permission to hear a double tap (the Settings
    /// row shows its button then).
    pub(crate) fn hotkey_waits_for_access(&self) -> bool {
        self.ivars().hotkey_registration.borrow().waits_for_access()
    }

    /// bateri came to the front: the person may have given the permission a double tap waits for
    /// in System Settings. Asks the system whether it was, and the key again if so; asks no one.
    pub(crate) fn hotkey_access_may_have_changed(&self) {
        if self.ivars().hotkey_registration.borrow().waits_for_access() {
            self.retry_global_keys(false);
        }
    }

    /// Asks for the key again where a double tap waits for the permission (`ask`: the person
    /// is asked for it first); tells the settings what came of it. Whether it still waits.
    fn retry_global_keys(&self, ask: bool) -> bool {
        let instance = self.hotkey_instance();
        if !rules::owns_global_keys(&instance) {
            return false;
        }
        let settings = self.settings().clone();
        let (waiting, told) = self.with_system_keys(|keys| {
            if ask {
                keys.ask_listen_access();
            }
            let mut registration = self.ivars().hotkey_registration.borrow_mut();
            registration.retry(keys, &settings, &instance);
            (
                registration.waits_for_access(),
                registration.notices(settings.hotkey_window.shortcut),
            )
        });
        self.post_notices(Source::Hotkey, told);
        self.refresh_settings_window();
        waiting
    }

    /// The person dragged an edge of the hotkey window and let go: the size it has now is a share
    /// of the screen it is on, and **that** is what is written — `depth` and `span`, nothing else
    /// (the size has one owner, the settings file; kept in memory it would jump back at the next
    /// read). The window then goes to the frame the settings make of the shares, so the edge that
    /// holds it to its place did not move away.
    pub(crate) fn hotkey_resized(&self, window: &TerminalWindow) {
        if !window.hotkey_shown() {
            return;
        }
        let settings = self.settings().hotkey_window;
        let frame = window.frame_now();
        let screens = self.hotkey_screens();
        let on = window.screen_now().map(|screen| frame_of(screen.frame()));
        if let Some(screen) = on.and_then(|on| screens.iter().find(|screen| screen.frame == on)) {
            let (depth, span) = rules::shares(screen.visible, settings.position, frame);
            if (depth, span) != (settings.depth, settings.span) {
                let edits = [
                    SettingsEdit::HotkeyDepth(depth),
                    SettingsEdit::HotkeySpan(span),
                ];
                // A file that cannot be written leaves the shares as they were: the window
                // goes back to them below.
                let _ = self.save_edits(&edits);
            }
        }
        let now = self.settings().clone();
        self.hotkey_settings_changed(&now);
    }

    /// Why the system does not hold the key the settings ask for, if it does not (the Settings
    /// row says it).
    pub(crate) fn hotkey_problem(&self) -> Option<rules::Refusal> {
        self.ivars().hotkey_registration.borrow().problem()
    }

    /// The system's key was pressed. Silent while a field of Settings listens for a shortcut: the
    /// key is the person's to give, not one to run.
    pub(crate) fn hotkey_pressed(&self) {
        if crate::settings_window::is_recording(self.mtm()) {
            return;
        }
        self.toggle_hotkey(Verb::Toggle);
    }

    // -----------------------------------------------------------------------------------------
    // The focus leaving.

    /// Whether the focus leaving hides the window now ([`rules::Hiding`]).
    fn hotkey_hiding(&self) -> rules::Hiding {
        rules::Hiding {
            hide_on_click_away: self.settings().hotkey_window.hide_on_click_away,
            pinned: self.ivars().hotkey_pinned.get(),
        }
    }

    /// The hotkey window stopped being key. **Who has the keyboard now is not known yet** — it
    /// is settled after this turn — so the question is asked in the next one
    /// ([`AppDelegate::hotkey_focus_settled`]): a click on a normal window of bateri must not be
    /// read as nobody's.
    pub(crate) fn hotkey_key_resigned(&self, window: &TerminalWindow) {
        let id = window.id();
        DispatchQueue::main().exec_async(move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(app) = delegate(mtm) {
                app.hotkey_focus_settled(id);
            }
        });
    }

    fn hotkey_focus_settled(&self, id: u64) {
        let Some(window) = self.window(id) else {
            return;
        };
        if !window.hotkey_shown() || window.is_key_window() {
            return;
        }
        let taker = self.key_taker(&window);
        // Nobody has the key and bateri is not the active application: the focus went to another
        // application, and a non-activating panel is not always told ([`Lost::Active`]).
        let lost = if taker == KeyTaker::Unknown
            && !NSApplication::sharedApplication(self.mtm()).isActive()
        {
            Lost::Active
        } else {
            Lost::Key { to: taker }
        };
        if rules::hides_on(lost, self.hotkey_hiding()) {
            self.slide_out_hotkey(&window, false);
        }
    }

    /// Who has the keyboard in `window`'s place: a surface of its own, another terminal window,
    /// or nobody that can be told ([`KeyTaker`]).
    fn key_taker(&self, window: &TerminalWindow) -> KeyTaker {
        let Some(key) = NSApplication::sharedApplication(self.mtm()).keyWindow() else {
            return KeyTaker::Unknown;
        };
        let key = key.sheetParent().unwrap_or(key);
        match self.window_owning(&key) {
            Some(owner) if owner.id() != window.id() => KeyTaker::OtherTerminal,
            // The window itself again, a sheet on it, the Settings window, the palette's:
            // bateri's own surfaces.
            _ => KeyTaker::OwnSurface,
        }
    }

    /// Another application came to the front ([`Lost::Active`]): every hotkey window in sight
    /// goes, unless the pin or the setting keeps it.
    pub(crate) fn hotkey_active_lost(&self) {
        if !rules::hides_on(Lost::Active, self.hotkey_hiding()) {
            return;
        }
        for window in self.windows() {
            if window.hotkey_shown() {
                self.slide_out_hotkey(&window, false);
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // The pin and the settings.

    /// `bateri hotkey ACTION`, from a terminal or from none: the window as the action says, through
    /// the one entrance ([`Self::toggle_hotkey`]). `pin` shows the window too — a pin on a window
    /// that is not there would be nothing to see — and keeps it until `unpin` or the quit. The
    /// answer is the window that could not be shown, when it could not: the one who asked is a
    /// program that would take the window for there.
    pub(crate) fn hotkey_asked(&self, action: HotkeyAction) -> Result<(), String> {
        match action {
            HotkeyAction::Toggle => {
                self.try_toggle_hotkey(Verb::Toggle)?;
            }
            HotkeyAction::Show => {
                self.try_toggle_hotkey(Verb::Show)?;
            }
            HotkeyAction::Hide => {
                self.try_toggle_hotkey(Verb::Hide)?;
            }
            HotkeyAction::Pin => {
                self.try_toggle_hotkey(Verb::Show)?;
                self.ivars().hotkey_pinned.set(true);
            }
            HotkeyAction::Unpin => self.ivars().hotkey_pinned.set(false),
        }
        Ok(())
    }

    /// Window ▸ Pin Hotkey Window: toggled, in memory.
    pub(crate) fn toggle_hotkey_pin(&self) {
        let pinned = &self.ivars().hotkey_pinned;
        pinned.set(!pinned.get());
    }

    /// Whether the hotkey window is pinned now.
    pub(crate) fn hotkey_pinned(&self) -> bool {
        self.ivars().hotkey_pinned.get()
    }

    /// The Pin item: checked while pinned, and checked and grey when `hide_on_click_away = false`
    /// keeps the window for good (the item stays in its place, saying so).
    pub(crate) fn validate_pin_item(&self, item: &NSMenuItem) -> bool {
        let lasting = !self.settings().hotkey_window.hide_on_click_away;
        item.setState(if self.hotkey_pinned() || lasting {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
        !lasting
    }

    /// `[hotkey_window]` changed (`new` is the file that was read; the slot still holds the old
    /// one while this runs): a window in sight takes its new place and size at once.
    pub(crate) fn hotkey_settings_changed(&self, new: &Settings) {
        let Some(window) = self.hotkey_window().filter(|window| window.hotkey_shown()) else {
            return;
        };
        let settings = new.hotkey_window;
        if let Some(slide) = self.hotkey_slide(&settings) {
            window.replace(&slide, settings.spaces == HotkeySpaces::Follow);
        }
    }
}

#[cfg(test)]
mod tests {
    /// The body of the function that begins with `name`, up to its closing brace at `indent`.
    fn body_of<'a>(source: &'a str, name: &str, indent: &str) -> &'a str {
        let at = source.find(name).unwrap_or_else(|| panic!("{name}"));
        let end = source[at..]
            .find(&format!("\n{indent}}}\n"))
            .unwrap_or_else(|| panic!("the end of {name}"));
        &source[at..at + end]
    }

    /// The choices among windows go through the rules' selection functions
    /// ([`bt_shell_common::hotkey`]), each at the place that chooses: a source scan, because the
    /// application needs a window server. A new place that picks "the first window" or counts the
    /// windows would count a hidden hotkey window in, and this is where it is found.
    #[test]
    fn the_choices_among_windows_ask_the_rules_which_one_may_be_out_of_sight() {
        let app = include_str!("../app.rs");
        // This file's own code, not these tests' (which name the calls they look for).
        let hotkey = include_str!("hotkey.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("the code");
        let window = include_str!("../window.rs");
        let body = |name: &str| body_of(app, name, "    ");
        let inner = |name: &str| body_of(app, name, "        ");

        // The Dock's click and the launch: a normal window is opened when there is none.
        assert!(inner("        fn should_handle_reopen(").contains("self.needs_normal_window()"));
        assert!(body("    fn restore_or_open_windows(").contains("self.needs_normal_window()"));
        // A new pane's place, and the terminals shown again.
        assert!(body("    fn show_unshown_terminals(").contains("self.pane_window()"));
        assert!(
            !body("    fn show_unshown_terminals(").contains("self.windows().into_iter().next()")
        );
        // The windows that mean "where the person works": a normal one.
        assert!(body("    fn front_terminal_window(").contains("!window.is_hotkey()"));
        assert!(body("    fn first_pane(").contains("self.first_normal_window()"));
        assert!(body("    fn open_measured_tab(").contains("self.first_normal_window()"));
        // The session writes no hotkey window as the front one and writes its name.
        let saved = body("    fn saved_session(");
        assert!(saved.contains("!owner.is_hotkey()"));
        assert!(saved.contains("window.hotkey_name()"));
        assert!(saved.contains("!window.is_hotkey() && key.as_deref()"));
        // Merge All Windows: the item's greying, its target, and the windows it takes tabs from.
        assert!(
            inner("            } else if item.action() == Some(sel!(mergeWindows:)) {")
                .contains("self.can_merge_windows()")
        );
        assert!(body("    pub(crate) fn merge_all_windows(").contains("!window.is_hotkey()"));
        assert!(body("    pub(crate) fn make_move_raising(").contains("MergeAllWindows"));
        // A tab is not dropped on it, unless it is the window the carry started in.
        let drop = body("    pub(crate) fn drop_window_at(");
        assert!(drop.contains("!window.is_hotkey() || window.id() == from"));
        for (source, name) in [
            (include_str!("../pane_drag.rs"), "app.drop_window_at("),
            (include_str!("../tab_merge.rs"), "self.drop_window_at("),
        ] {
            assert!(source.contains(name), "{name}");
        }
        // A restored hotkey window is out of sight, at the settings' frame.
        let restore = body("    fn restore_window(");
        assert!(restore.contains("this.place_hidden(self.hotkey_frame_now()"));
        // What brings a window forward goes the hotkey way for this one.
        for name in [
            "    pub(crate) fn bring_to_front(",
            "    pub(crate) fn raise(",
            "    pub(crate) fn select(",
        ] {
            assert!(
                body_of(window, name, "    ").contains("self.order_front("),
                "{name}"
            );
        }

        // The rules read their windows **front to back**, which the application's list is not:
        // the key window, the main one, then AppKit's order, then those out of sight.
        let candidates = body_of(hotkey, "    fn candidates(", "    ");
        let key = candidates.find(".keyWindow()").expect("the key window");
        let main = candidates.find(".mainWindow()").expect("the main window");
        let listed = candidates
            .find("windowNumbersWithOptions")
            .expect("AppKit's order");
        assert!(key < main && main < listed);
        // And no rule is asked of a list that did not come from there.
        for call in [
            "rules::needs_normal_window(",
            "rules::can_merge(",
            "rules::pane_window(",
            "rules::front_normal(",
        ] {
            assert_eq!(hotkey.matches(call).count(), 1, "{call}");
        }
        assert!(hotkey.matches("self.candidates()").count() >= 4);
    }

    /// **No test, gate, smoke or agent's copy builds a tap, and nothing asks the person for the
    /// permission but the Settings page.** A source scan (the real tap needs the person's
    /// permission and a window server): the one `tap_create` is behind the question whether the
    /// permission is there, the question to the person is in one place that only Settings
    /// calls, and the thresholds of the count are the application's (`arrange::taps()`), not
    /// numbers of this file's.
    #[test]
    fn the_tap_is_built_with_the_permission_and_the_person_is_asked_only_from_settings() {
        let keys = include_str!("../global_keys.rs");
        let code = keys.split("#[cfg(test)]").next().expect("the code");
        assert_eq!(code.matches("CGEvent::tap_create(").count(), 1);
        assert_eq!(code.matches("CGRequestListenEventAccess()").count(), 1);
        let bind = body_of(code, "    fn bind(", "    ");
        let asked = bind
            .find("self.listen_access() != Access::Granted")
            .expect("the permission is looked at");
        let built = bind.find("Tap::start(").expect("the tap is built here");
        assert!(asked < built, "the permission before the tap");
        assert!(code.contains("CGEventTapOptions::ListenOnly"));
        assert!(code.contains("arrange::taps()"));
        assert!(
            !code.contains("Taps::new("),
            "the thresholds are `arrange::taps()`'s"
        );
        // Nobody else names the system's calls.
        for (name, other) in [
            ("app.rs", include_str!("../app.rs")),
            ("window.rs", include_str!("../window.rs")),
            ("lib.rs", include_str!("../lib.rs")),
            ("menu.rs", include_str!("../menu.rs")),
            ("settings_window.rs", include_str!("../settings_window.rs")),
            ("hotkey.rs", include_str!("hotkey.rs")),
        ] {
            let other = other.split("#[cfg(test)]").next().expect("the code");
            assert!(
                !other.contains("tap_create(") && !other.contains("CGRequestListenEventAccess"),
                "{name}"
            );
        }
        // The person is asked from `allow_listening` only, and only Settings calls it.
        let code_here = include_str!("hotkey.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("the code");
        assert_eq!(code_here.matches("retry_global_keys(true)").count(), 1);
        assert_eq!(code_here.matches("retry_global_keys(false)").count(), 1);
        assert!(
            body_of(code_here, "    pub(crate) fn allow_listening(", "    ")
                .contains("retry_global_keys(true)")
        );
        assert!(
            body_of(
                code_here,
                "    pub(crate) fn hotkey_access_may_have_changed(",
                "    "
            )
            .contains("retry_global_keys(false)")
        );
        for (name, other) in [
            ("app.rs", include_str!("../app.rs")),
            ("window.rs", include_str!("../window.rs")),
            ("menu.rs", include_str!("../menu.rs")),
            ("lib.rs", include_str!("../lib.rs")),
        ] {
            let other = other.split("#[cfg(test)]").next().expect("the code");
            assert!(!other.contains("allow_listening("), "{name}");
        }
        let page = include_str!("../settings_window/hotkey.rs");
        assert!(page.contains("delegate.allow_listening(false)"));
        assert!(include_str!("../settings_window.rs").contains("delegate.allow_listening(true)"));
        // Coming back to the front only asks the system whether it was given.
        assert!(include_str!("../app.rs").contains("self.hotkey_access_may_have_changed();"));
    }

    /// The size the person dragged to is written as `depth` and `span` and **nothing else**: the
    /// shares of a frame are read back from the file as the frame they came from, the shortcut and
    /// the other keys stay, and so does the person's comment. The write goes through the one
    /// place that writes settings (`save_edits`).
    #[test]
    fn a_dragged_size_writes_depth_and_span_and_nothing_else() {
        use bt_core::{HotkeyPosition, Settings, SettingsEdit};
        use bt_shell_common::hotkey as rules;
        use bt_shell_common::restore::Frame;

        let visible = Frame {
            x: 0.0,
            y: 0.0,
            width: 1440.0,
            height: 875.0,
        };
        let text = "# my notes\n[terminal]\nscrollback = 1234\n\n[hotkey_window]\n\
                    shortcut = \"option+space\"\nposition = \"left\"\n";
        let before = Settings::parse_keeping(text, &Settings::default())
            .expect("it parses")
            .settings;
        for position in HotkeyPosition::NAMES.iter().map(|&(_, position)| position) {
            let dragged = rules::shown_frame(visible, position, 35, 80);
            let (depth, span) = rules::shares(visible, position, dragged);
            assert_eq!((depth, span), (35, 80), "{position:?}");
            let written = [
                SettingsEdit::HotkeyDepth(depth),
                SettingsEdit::HotkeySpan(span),
            ]
            .iter()
            .try_fold(text.to_owned(), |text, edit| {
                Settings::with_edit(&text, edit)
            })
            .expect("it writes");
            let after = Settings::parse_keeping(&written, &Settings::default())
                .expect("it parses")
                .settings;
            assert_eq!(after.hotkey_window.depth, 35);
            assert_eq!(after.hotkey_window.span, 80);
            // Everything the drag did not name stays: the shortcut, the position, the others.
            let mut same = after.clone();
            same.hotkey_window = before.hotkey_window;
            assert_eq!(same, before, "{written}");
            assert!(written.contains("# my notes") && written.contains("scrollback = 1234"));
        }
        // And the code that writes it names those two edits only, after the rule that reads the
        // frame as shares.
        let code = include_str!("hotkey.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("the code");
        let drag = body_of(code, "    pub(crate) fn hotkey_resized(", "    ");
        assert!(drag.contains("rules::shares("));
        assert_eq!(drag.matches("SettingsEdit::").count(), 2);
        assert!(
            drag.contains("SettingsEdit::HotkeyDepth(")
                && drag.contains("SettingsEdit::HotkeySpan(")
        );
        // The edge's end is told by the window's own delegate, for the hotkey window only.
        let window = include_str!("../window.rs");
        let hook = body_of(window, "        fn window_did_end_live_resize(", "        ");
        assert!(hook.contains("self.is_hotkey()") && hook.contains("hotkey_resized("));
    }

    /// **No test, gate, smoke or agent's copy registers a key for the whole system**: the real
    /// keys are built in one place, after the rule that says who owns them, and the
    /// application's other modules never name them. A source scan: building them would steal the
    /// keystroke of the Mac the gate runs on, and a copy of the application beside the person's
    /// would hold the same keys.
    #[test]
    fn the_real_keys_are_built_in_one_place_and_only_by_the_owner() {
        let hotkey = include_str!("hotkey.rs");
        let code = hotkey.split("#[cfg(test)]").next().expect("the code");
        let building = "SystemKeys::new";
        assert_eq!(code.matches(building).count(), 1);
        for (name, other) in [
            ("app.rs", include_str!("../app.rs")),
            ("window.rs", include_str!("../window.rs")),
            ("lib.rs", include_str!("../lib.rs")),
            ("menu.rs", include_str!("../menu.rs")),
            ("settings_window.rs", include_str!("../settings_window.rs")),
        ] {
            assert!(!other.contains(building), "{name}");
        }
        // Both ways in ask who owns the keys before they reach the place that builds them.
        for caller in [
            "    pub(crate) fn apply_global_keys(",
            "    fn retry_global_keys(",
        ] {
            let body = body_of(code, caller, "    ");
            let owner = body.find("rules::owns_global_keys(").expect("the rule");
            let built = body.find("self.with_system_keys(").expect("the way in");
            assert!(
                owner < built,
                "{caller}: asked who owns them before building them"
            );
        }
        assert_eq!(code.matches("self.with_system_keys(").count(), 2);
        // The instance is read from the lock, the bundle, the timed run and the agent's copy.
        let instance = body_of(code, "    fn hotkey_instance(", "    ");
        for fact in [
            "restore_lock",
            "bundleIdentifier",
            "self.ivars().run",
            "AGENT_CHECK_BUNDLE",
        ] {
            assert!(instance.contains(fact), "{fact}");
        }
        // And the key press is silent while a field listens for a shortcut.
        assert!(
            body_of(code, "    pub(crate) fn hotkey_pressed(", "    ")
                .contains("settings_window::is_recording(")
        );
        // The settings' change asks for the keys again, from the file that was read.
        let app = include_str!("../app.rs");
        assert!(app.contains("self.apply_global_keys(&new);"));
        assert!(app.contains("self.apply_global_keys(&self.settings());"));
        // A launch that gave the session up takes the lock later, and asks again from there.
        assert!(
            body_of(app, "    pub(crate) fn open_given_up_windows(", "    ")
                .contains("self.apply_global_keys(&self.settings());")
        );
    }

    /// A command whose double press is the hotkey window's own is not run by it: the tap that
    /// shows the window hears the two taps first, and `held_by_hotkey` says the
    /// menu never gets them.
    #[test]
    fn a_double_press_the_hotkey_window_holds_runs_no_command() {
        let app = include_str!("../app.rs");
        let body = body_of(app, "    pub(crate) fn double_pressed(", "    ");
        assert!(body.contains("held_by_hotkey()"));
        assert!(body.contains("owner_of_twice("));
    }

    /// One entrance for every trigger: the menu's item, the shortcut and the command call the
    /// same function, and the rules decide what it does.
    #[test]
    fn every_trigger_goes_through_the_one_entrance() {
        let app = include_str!("../app.rs");
        assert!(
            body_of(app, "        fn show_hotkey_window_action(", "        ")
                .contains("self.toggle_hotkey(")
        );
        let hotkey = include_str!("hotkey.rs");
        let toggle = body_of(hotkey, "    pub(crate) fn try_toggle_hotkey(", "    ");
        assert!(toggle.contains("rules::decide("));
        assert!(
            body_of(hotkey, "    pub(crate) fn toggle_hotkey(", "    ")
                .contains("self.try_toggle_hotkey(")
        );
        // `bateri hotkey` is one more of them, whatever the action: shown, hidden or toggled
        // by the entrance; pinned or let go in the one place the pin is kept.
        let asked = body_of(hotkey, "    pub(crate) fn hotkey_asked(", "    ");
        assert_eq!(asked.matches("self.try_toggle_hotkey(").count(), 4);
        assert_eq!(asked.matches("hotkey_pinned.set(").count(), 2);
        assert!(
            body_of(
                include_str!("../agent.rs"),
                "        Answer::Hotkey(action) => {",
                "        "
            )
            .contains("app.hotkey_asked(action)")
        );
        // The focus leaving asks the rules too, and what took the key is settled a turn later.
        assert!(
            body_of(hotkey, "    fn hotkey_focus_settled(", "    ").contains("rules::hides_on(")
        );
        assert!(
            body_of(hotkey, "    pub(crate) fn hotkey_key_resigned(", "    ")
                .contains("exec_async(")
        );
    }
}
