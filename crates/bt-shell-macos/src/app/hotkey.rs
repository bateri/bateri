//! The hotkey window from the application's side: the one entrance every trigger goes through
//! ([`AppDelegate::toggle_hotkey`]), where the window opens, how the focus leaving it is read,
//! and the choices among windows that must know one of them may be out of sight.
//!
//! **What is decided here is only wiring.** Which screen, what frame, what a tap does to a window
//! in each state, whether the focus leaving hides it, which window a new pane or a merge goes to:
//! all of it is [`bt_shell_common::hotkey`]'s, tried without a window. This file reads the
//! settings and AppKit, hands them to those rules, and does what they say. The window's own part
//! (the panel, the slide, what it gives back) is [`crate::window`]'s.

use bt_core::{HOTKEY_DEFAULT_NAME, HotkeySpaces, HotkeyWindowSettings};
use bt_shell_common::hotkey::{self as rules, Candidate, KeyTaker, Lost, Presence, Step, Verb};
use objc2_app_kit::NSScreen;

use super::*;
use crate::window::{frame_of, frontmost_other, rect_of};

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
    /// [`rules::decide`]'s; the step taken is returned.
    pub(crate) fn toggle_hotkey(&self, verb: Verb) -> Step {
        if self.is_terminating() {
            return Step::Nothing;
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
            (Step::Open, _) => {
                if let Err(e) = self.open_hotkey_window() {
                    eprintln!("bateri: could not open the hotkey window: {e}");
                }
            }
            (Step::Show, Some(window)) => self.slide_in_hotkey(&window),
            (Step::Raise, Some(window)) => window.raise_hotkey(),
            (Step::Hide, Some(window)) => self.slide_out_hotkey(&window, true),
            _ => {}
        }
        step
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
            Step::Show => self.slide_in_hotkey(window),
            Step::Raise => window.raise_hotkey(),
            _ => {}
        }
    }

    /// The window slides in on the screen the settings and the pointer choose.
    fn slide_in_hotkey(&self, window: &TerminalWindow) {
        let settings = self.settings().hotkey_window;
        let Some(slide) = self.hotkey_slide(&settings) else {
            return;
        };
        window.slide_in(
            &slide,
            settings.spaces == HotkeySpaces::Follow,
            self.hotkey_animates(&settings),
            frontmost_other(self.mtm()),
        );
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
        let toggle = body_of(hotkey, "    pub(crate) fn toggle_hotkey(", "    ");
        assert!(toggle.contains("rules::decide("));
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
