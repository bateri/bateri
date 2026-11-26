//! The hotkey window's rules: the **pure** part of a bateri window that a shortcut slides in
//! from the edge of the screen. Everything that is decided without a window lives here and is
//! tried without one: which screen, the frame of each position, where a slide starts and ends,
//! what a tap of the shortcut does, when the focus leaving hides the window, which window a
//! thing goes to now that one may be out of sight, and who may register a key for the whole
//! system. The AppKit part (`bt-shell-macos`) only applies what this module answers.
//!
//! **Coordinates are AppKit's:** points, the origin at the bottom left of the primary screen and
//! `y` growing **upward**, so a window's top edge is `y + height`. The usable area of a screen is
//! its `visibleFrame` (the menu bar and the Dock are not in it); the whole screen is only for
//! where a slide starts, because a window comes from beyond the glass and not from the menu bar.
//!
//! **One hotkey window, with a name** ([`DEFAULT_NAME`]): the name is in the session file and the
//! settings' shape, so more than one can be added without changing either. This module never
//! assumes there is only one.
//!
//! **The global key is behind a seam** ([`GlobalKeys`]): the real one is Carbon's
//! `RegisterEventHotKey` and, for a modifier tapped twice, the system's event monitors
//! (`bt-shell-macos`); **neither asks the person for a permission**. No test, gate, smoke or
//! agent try registers either — a real registration steals the keystroke from the Mac that runs
//! the gate. A test's is [`fake::Keys`]. Only an instance that [`owns_global_keys`] builds the
//! real one.

use bt_core::{Chord, Modifier, Settings, Shortcut};

pub use bt_core::{
    HOTKEY_DEFAULT_NAME as DEFAULT_NAME, HOTKEY_SIZE_RANGE, HotkeyPosition, HotkeyScreen,
    HotkeySpaces, HotkeyWindowSettings,
};

use crate::restore::Frame;

/// How long a slide lasts, in milliseconds. A **design constant**: `.claude/rules/tasarim.md`'s
/// floor for a motion that is the point of the gesture, and the feel is the person's to judge.
pub const SLIDE_MILLIS: u64 = 240;

// ---------------------------------------------------------------------------------------------
// Screens and frames.

/// A screen: its whole frame and its usable area, both in AppKit's coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    pub frame: Frame,
    pub visible: Frame,
}

/// Which of `screens` the window opens on (the index), or `None` for no screen at all. `pointer`
/// is the mouse's place in the same coordinates; `screens[0]` is the primary one (the one with
/// the menu bar, as `NSScreen.screens` lists it).
///
/// A pointer on no screen, or none given, is the first screen: the screen the pointer was on may
/// have been unplugged, and the window must still come ([`crate::restore`] clamps a saved frame
/// the same way). A pointer between two screens is on exactly one: the edges are half open, and on
/// the vertical axis the top edge is the one that belongs to the screen (the pointer's `y` counts
/// up from the bottom and is a screen's `maxY` on its top row, as `NSMouseInRect` has it); a
/// pointer on an outer edge that no screen has by that rule (the bottom row) is on the screen
/// whose closed frame it is in.
pub fn screen_index(
    screens: &[Screen],
    policy: HotkeyScreen,
    pointer: Option<(f64, f64)>,
) -> Option<usize> {
    if screens.is_empty() {
        return None;
    }
    if policy == HotkeyScreen::Primary {
        return Some(0);
    }
    let Some((x, y)) = pointer else {
        return Some(0);
    };
    let within = |frame: &Frame| {
        x >= frame.x && x < frame.x + frame.width && y > frame.y && y <= frame.y + frame.height
    };
    let touching = |frame: &Frame| {
        x >= frame.x && x <= frame.x + frame.width && y >= frame.y && y <= frame.y + frame.height
    };
    Some(
        screens
            .iter()
            .position(|screen| within(&screen.frame))
            .or_else(|| screens.iter().position(|screen| touching(&screen.frame)))
            .unwrap_or(0),
    )
}

/// The frame the window has when it is shown, on `visible` (a screen's usable area): `depth`
/// percent of it from the edge `position` names, `span` percent of it along the edge, centered
/// along it. `center` is centered both ways and `depth` is its height.
///
/// A share outside [`HOTKEY_SIZE_RANGE`] is brought into it: a hand-made call, or a settings
/// value that was never read, cannot make a window with no size.
pub fn shown_frame(visible: Frame, position: HotkeyPosition, depth: u8, span: u8) -> Frame {
    // Multiplied before it is divided: a tenth of 1440 is 144, not 144.00000000000003.
    let part = |whole: f64, percent: u8| {
        whole * f64::from(percent.clamp(*HOTKEY_SIZE_RANGE.start(), 100)) / 100.0
    };
    let (width, height) = match position {
        HotkeyPosition::Top | HotkeyPosition::Bottom | HotkeyPosition::Center => {
            (part(visible.width, span), part(visible.height, depth))
        }
        HotkeyPosition::Left | HotkeyPosition::Right => {
            (part(visible.width, depth), part(visible.height, span))
        }
    };
    let across = visible.x + (visible.width - width) / 2.0;
    let along = visible.y + (visible.height - height) / 2.0;
    let (x, y) = match position {
        HotkeyPosition::Top => (across, visible.y + visible.height - height),
        HotkeyPosition::Bottom => (across, visible.y),
        HotkeyPosition::Left => (visible.x, along),
        HotkeyPosition::Right => (visible.x + visible.width - width, along),
        HotkeyPosition::Center => (across, along),
    };
    Frame {
        x,
        y,
        width,
        height,
    }
}

/// The `depth` and `span` (percent, rounded, inside [`HOTKEY_SIZE_RANGE`]) of `frame` on
/// `visible`: what a frame the person dragged the window to writes to the settings. The inverse
/// of [`shown_frame`] for its sizes.
pub fn shares(visible: Frame, position: HotkeyPosition, frame: Frame) -> (u8, u8) {
    let percent = |part: f64, whole: f64| {
        let share = if whole > 0.0 {
            part / whole * 100.0
        } else {
            100.0
        };
        // Inside the range first, so the cast cannot meet a number it does not hold.
        let (low, high) = (f64::from(*HOTKEY_SIZE_RANGE.start()), 100.0);
        share.round().clamp(low, high) as u8
    };
    match position {
        HotkeyPosition::Top | HotkeyPosition::Bottom | HotkeyPosition::Center => (
            percent(frame.height, visible.height),
            percent(frame.width, visible.width),
        ),
        HotkeyPosition::Left | HotkeyPosition::Right => (
            percent(frame.width, visible.width),
            percent(frame.height, visible.height),
        ),
    }
}

/// A slide: where the window is when it is away and where it is when it is shown. Showing goes
/// from `away` to `shown`, hiding back; both have **the same size**, only the place moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slide {
    pub away: Frame,
    pub shown: Frame,
    /// The window does not travel: where it would come from touches another screen (a screen
    /// above, or beside, the one it is on), and a window sliding in over that one would be seen
    /// there. It stays at `shown` and fades in and out in the same time ([`slide_among`]).
    pub fade: bool,
}

/// The slide of a window that is `shown` on `screen`: it comes from just beyond the screen's own
/// edge (not the menu bar's, and not the Dock's) so that all of it is out of sight at the
/// start. `center` comes from the top, like `top`.
pub fn slide(screen: &Screen, shown: Frame, position: HotkeyPosition) -> Slide {
    let whole = screen.frame;
    let mut away = shown;
    match position {
        HotkeyPosition::Top | HotkeyPosition::Center => away.y = whole.y + whole.height,
        HotkeyPosition::Bottom => away.y = whole.y - shown.height,
        HotkeyPosition::Left => away.x = whole.x - shown.width,
        HotkeyPosition::Right => away.x = whole.x + whole.width,
    }
    Slide {
        away,
        shown,
        fade: false,
    }
}

/// [`slide`] on the screen `on` of `screens`, which also knows whether the way in is clear: when
/// the place the window comes from overlaps another screen it fades where it is instead of
/// travelling (as iTerm2 does). `None` for a screen that is not there.
pub fn slide_among(
    screens: &[Screen],
    on: usize,
    shown: Frame,
    position: HotkeyPosition,
) -> Option<Slide> {
    let mut moving = slide(screens.get(on)?, shown, position);
    moving.fade = screens
        .iter()
        .enumerate()
        .any(|(at, other)| at != on && overlap_area(&other.frame, &moving.away) > 0.0);
    Some(moving)
}

/// The area two frames have in common (none when they only touch).
fn overlap_area(a: &Frame, b: &Frame) -> f64 {
    let width = (a.x + a.width).min(b.x + b.width) - a.x.max(b.x);
    let height = (a.y + a.height).min(b.y + b.height) - a.y.max(b.y);
    width.max(0.0) * height.max(0.0)
}

/// The screen a window is on: the one its frame overlaps the most (the first when two are
/// equal), `None` when it overlaps none. **A window that is shown stays on its own screen**: the
/// pointer and the primary screen choose where it comes ([`screen_index`]), not where it is
/// laid out again (a setting that changed, an edge that was dragged) or taken away from.
pub fn screen_of(screens: &[Screen], frame: Frame) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (at, screen) in screens.iter().enumerate() {
        let area = overlap_area(&screen.frame, &frame);
        if area > 0.0 && best.is_none_or(|(_, most)| area > most) {
            best = Some((at, area));
        }
    }
    best.map(|(at, _)| at)
}

// ---------------------------------------------------------------------------------------------
// The tap of the shortcut.

/// What a window is, to someone who wants to show or hide it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    /// There is no window: none was opened yet, or its last tab closed.
    Absent,
    /// It is there and out of sight.
    Hidden,
    /// It is in sight; `key` says whether it has the keyboard. In sight with no keyboard is a
    /// pinned window that something else took the focus from.
    Shown { key: bool },
}

/// What was asked: the shortcut and `bateri hotkey toggle` are [`Verb::Toggle`], the other two
/// words of the command are their own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
    Toggle,
    Show,
    Hide,
}

/// What to do about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Open a fresh window (one tab, the home folder) and show it.
    Open,
    /// Slide the hidden window in and give it the keyboard.
    Show,
    /// Slide the window away and give the keyboard back.
    Hide,
    /// The window is in sight and has no keyboard: bring it to the front and give it the
    /// keyboard, without sliding.
    Raise,
    /// Nothing to do: it is as it was asked to be.
    Nothing,
}

/// The step for `verb` on a window that is `presence`.
///
/// A tap shows a window that is away, hides one that is in front, and brings one that is in
/// sight but not in front. The last is the pinned window: it stays in sight when the focus
/// leaves, so a tap there means *come here*, not *go*.
pub fn decide(verb: Verb, presence: Presence) -> Step {
    match (verb, presence) {
        (Verb::Toggle | Verb::Show, Presence::Absent) => Step::Open,
        (Verb::Toggle | Verb::Show, Presence::Hidden) => Step::Show,
        (Verb::Toggle, Presence::Shown { key: true }) => Step::Hide,
        (Verb::Toggle | Verb::Show, Presence::Shown { key: false }) => Step::Raise,
        (Verb::Show, Presence::Shown { key: true }) => Step::Nothing,
        (Verb::Hide, Presence::Shown { .. }) => Step::Hide,
        (Verb::Hide, Presence::Absent | Presence::Hidden) => Step::Nothing,
    }
}

// ---------------------------------------------------------------------------------------------
// The focus leaving.

/// Who took the keyboard from the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyTaker {
    /// A surface of bateri's own: a sheet, the palette, the Settings window, a menu. The
    /// hotkey window stays: Settings is where the hotkey is tried live, and a menu never moves
    /// the keyboard at all.
    OwnSurface,
    /// Another window of the terminal, a normal one: the person went there, so the window does
    /// not float over it.
    OtherTerminal,
    /// Nobody, or nobody that can be told. The keyboard may have gone to another application,
    /// and then [`Lost::Active`] says so; if it did not, nothing happened that hides.
    Unknown,
}

/// What happened to the window's focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lost {
    /// The window stopped being key, and `to` took it.
    Key { to: KeyTaker },
    /// bateri stopped being the active application (another one was clicked or came forward).
    Active,
}

/// What decides whether the focus leaving hides the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hiding {
    /// `[hotkey_window] hide_on_click_away`.
    pub hide_on_click_away: bool,
    /// Window ▸ Pin Hotkey Window: in memory only, gone with the application.
    pub pinned: bool,
    /// `[hotkey_window] spaces = "follow"`: the window comes along when the person goes to
    /// another desktop.
    pub follow: bool,
}

/// Whether `lost` hides the window, **on the desktop it is on**. A pin, or
/// `hide_on_click_away = false`, hides on nothing.
pub fn hides_on(lost: Lost, hiding: Hiding) -> bool {
    if hiding.pinned || !hiding.hide_on_click_away {
        return false;
    }
    match lost {
        Lost::Active => true,
        Lost::Key { to } => to == KeyTaker::OtherTerminal,
    }
}

/// Where the window in sight is, to the desktop (Space) the person is on now. The system's own
/// answer (`isOnActiveSpace`), read when the question is asked: the window is on the old desktop
/// after the person went to another one, until it is brought along.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Desktop {
    /// On the desktop in front.
    Here,
    /// On another one: the person went away from it.
    Elsewhere,
}

/// What the focus leaving, with the desktop it happened on, does to the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Leaving {
    /// Nothing: the window stays where it is.
    Stays,
    /// The window goes ([`HideCause::FocusLeft`]).
    Hides,
    /// The window comes to the desktop the person is on, with the keyboard ([`follows`]).
    Follows,
}

/// Which desktops (Spaces) the window belongs to: the Space part of its `collectionBehavior`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Desktops {
    /// The one it was opened on (`spaces = "stay"`).
    Its,
    /// The one in front whenever it is shown or brought along (`follow`): `moveToActiveSpace`.
    Front,
    /// Every one (`follow`, with a pin or `hide_on_click_away = false` keeping it in sight):
    /// `canJoinAllSpaces`. **It is with the person on each desktop without taking the keyboard**
    /// from the application there — bringing a pinned window along by making it key would send
    /// the next keys typed in that application to the terminal (bateri's own decision; Ghostty
    /// follows only where it would have hidden).
    Every,
}

/// Which desktops the window belongs to, for these settings and pin.
pub fn desktops(hiding: Hiding) -> Desktops {
    if !hiding.follow {
        Desktops::Its
    } else if hiding.pinned || !hiding.hide_on_click_away {
        Desktops::Every
    } else {
        Desktops::Front
    }
}

/// Whether the window comes to the desktop in front, with the keyboard: it belongs to the one in
/// front ([`Desktops::Front`]) and is on another one — the person went there from it. A window
/// that belongs to every desktop is already there ([`Desktops::Every`]).
pub fn follows(hiding: Hiding, desktop: Desktop) -> bool {
    desktops(hiding) == Desktops::Front && desktop == Desktop::Elsewhere
}

/// Where bateri's Settings window stands against the hotkey window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsLevel {
    /// An ordinary window: the hotkey window is out of sight, or there is none.
    Normal,
    /// Over the hotkey window, which floats: the person who changes the window's settings sees
    /// the result at once, with the Settings window beside it and not under it (the panel is
    /// half the screen). The Settings window floats with it only while the panel is in sight
    /// and bateri is in front: a pinned panel stays over another application, the Settings
    /// window does not.
    AbovePanel,
}

/// The level of the Settings window when a hotkey window is (not) in sight and bateri is (not)
/// the application in front.
pub fn settings_level(panel_in_sight: bool, in_front: bool) -> SettingsLevel {
    if panel_in_sight && in_front {
        SettingsLevel::AbovePanel
    } else {
        SettingsLevel::Normal
    }
}

/// What `lost` does to the window on `desktop`. **The one question both ways the window loses
/// the focus ask** — the key window changing and the application stopping to be active — so a
/// desktop change, which brings both, is read the same way by each: the window follows, or (with
/// `spaces = "stay"`) is hidden or kept as [`hides_on`] says.
pub fn leaving(lost: Lost, hiding: Hiding, desktop: Desktop) -> Leaving {
    if follows(hiding, desktop) {
        Leaving::Follows
    } else if hides_on(lost, hiding) {
        Leaving::Hides
    } else {
        Leaving::Stays
    }
}

/// What took the window away.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HideCause {
    /// The person asked: the shortcut, or `bateri hotkey` with `toggle` or `hide`.
    Asked,
    /// The window ended: its last tab went. The person was typing in it, so they go back to
    /// where they were.
    Closed,
    /// The focus left it: a click on something else, another application coming forward.
    FocusLeft,
}

/// Whether the application the person was in comes back to the front as the window goes. Only
/// when the person asked **and the window had the keyboard**: that is the person coming back
/// from where they were typing. A window that did not have it (a pin, with the person in
/// another application), and every hide the focus caused, bring nobody forward: the person is
/// already where they went, and the record of where they were is let go either way. Decided by
/// the cause and not by whether bateri is the active application: the keyboard leaving a window
/// and the application stopping to be active come in turns of the run loop of their own.
pub fn restores_previous(cause: HideCause, had_key: bool) -> bool {
    matches!(cause, HideCause::Asked | HideCause::Closed) && had_key
}

// ---------------------------------------------------------------------------------------------
// Which window, now that one may be out of sight.

/// One of the windows of the application, to the rules that choose among them. A list of them is
/// in the order the application keeps (the front first), which is the order a choice reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Candidate {
    /// A hotkey window.
    pub hotkey: bool,
    /// In sight: not out of the screen. A normal window is always counted in sight, whether or
    /// not it is minimized; that is not this module's to decide.
    pub shown: bool,
    /// The key window.
    pub key: bool,
    /// The main window.
    pub main: bool,
}

impl Candidate {
    /// A normal window.
    pub fn normal() -> Self {
        Self {
            shown: true,
            ..Self::default()
        }
    }

    /// A hotkey window, in sight or not.
    pub fn hotkey(shown: bool) -> Self {
        Self {
            hotkey: true,
            shown,
            ..Self::default()
        }
    }

    /// Whether something that needs a window **in sight** can use it: a hidden hotkey window
    /// cannot.
    fn usable(&self) -> bool {
        !self.hotkey || self.shown
    }
}

/// Whether the Dock's icon, or the launch, has to open a normal window: there is none among
/// `windows`. A hotkey window, hidden or in sight, is not a normal window — a click on the icon
/// with only a hidden one must not do nothing, and the launch of a session that held only the
/// hotkey window still has to show something.
pub fn needs_normal_window(windows: &[Candidate]) -> bool {
    windows.iter().all(|window| window.hotkey)
}

/// The window a new pane goes to (the index): the key one, else the main one, else the front
/// one, else the first, among the windows that can be seen. A hidden hotkey window is not one:
/// a pane that fell into it would be somewhere nobody can look.
pub fn pane_window(windows: &[Candidate]) -> Option<usize> {
    let usable = || {
        windows
            .iter()
            .enumerate()
            .filter(|(_, window)| window.usable())
    };
    usable()
        .find(|(_, window)| window.key)
        .or_else(|| usable().find(|(_, window)| window.main))
        .or_else(|| usable().next())
        .map(|(at, _)| at)
}

/// The front **normal** window (the index): what a thing that means "the window the person is
/// working in" reads — Merge All Windows' target, the session's front-window mark, the first window
/// of the smoke and of a measurement. A hotkey window is never it, in sight or not: it is the
/// person's to call, not their workplace, and a session must not come back with it in front.
pub fn front_normal(windows: &[Candidate]) -> Option<usize> {
    windows.iter().position(|window| !window.hotkey)
}

/// Whether Merge All Windows has anything to do: more than one **normal** window.
pub fn can_merge(windows: &[Candidate]) -> bool {
    windows.iter().filter(|window| !window.hotkey).count() > 1
}

// ---------------------------------------------------------------------------------------------
// The key for the whole system.

/// What the system is asked to hold for the hotkey window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Binding {
    /// A chord that works in every application (`RegisterEventHotKey`): asks for no permission.
    Chord(Chord),
    /// One modifier tapped twice (the system's event monitors, [`DoubleTap`]): asks for no
    /// permission.
    Twice(Modifier),
}

impl Binding {
    /// The binding a shortcut of `[hotkey_window]` makes; `None` for a kind the section never
    /// holds (its reading brings every shortcut to a chord or a double tap).
    pub fn of(shortcut: Shortcut) -> Option<Self> {
        match shortcut.for_global().ok()? {
            Shortcut::Chord(chord) => Some(Self::Chord(chord)),
            Shortcut::Twice(modifier) => Some(Self::Twice(modifier)),
            Shortcut::Mods(_) => None,
        }
    }
}

/// What the settings ask the system to hold, if anything.
pub fn binding_of(settings: &Settings) -> Option<Binding> {
    settings.hotkey_window.shortcut.and_then(Binding::of)
}

/// Why the system did not hold a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The registration was refused; the number is the system's status. Another application
    /// holding the same keys is **not** told by this: the system accepts a key that another one
    /// also holds, and says nothing.
    Refused(i32),
}

/// The seam in front of the system's global keys: the one place that registers a key for every
/// application and the one that listens for a modifier tapped twice.
pub trait GlobalKeys {
    /// Holds `binding` in place of whatever was held before; `None` lets go of it. Said again
    /// with the same binding it is held once.
    fn bind(&mut self, binding: Option<Binding>) -> Result<(), Refusal>;
}

/// What an instance of the application is, to the rule that says whether it holds the key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Instance {
    /// It holds the session directory's lock (`restore::Lock`): the one instance that owns the
    /// saved layout. A second one of the same bundle does not, and a second one that took the
    /// key too would steal the person's keystroke.
    pub holds_session_lock: bool,
    /// It runs from an application bundle, not as a bare binary a test or `cargo run` starts.
    pub bundled: bool,
    /// A timed run (`BT_RUN_SECONDS`): the smoke, the gate's measurements.
    pub timed_run: bool,
    /// The copy the agent tries bateri with (`dev.bateri.agent-check`).
    pub agent_check: bool,
}

/// Whether this instance builds the real [`GlobalKeys`]: the one that holds the session lock, is
/// bundled, and is neither a timed run nor the agent's copy. `make dev`'s copy is bundled and has
/// its own session, so it holds the key by its own settings; when the installed bateri holds the
/// same keys one of the two is silent, and the demonstration says so.
pub fn owns_global_keys(instance: &Instance) -> bool {
    instance.holds_session_lock && instance.bundled && !instance.timed_run && !instance.agent_check
}

/// What an instance asks the system to hold: the settings' binding when it owns the keys,
/// nothing when it does not.
pub fn binding_for(settings: &Settings, instance: &Instance) -> Option<Binding> {
    owns_global_keys(instance)
        .then(|| binding_of(settings))
        .flatten()
}

// ---------------------------------------------------------------------------------------------
// The registration: what is held, and what is told when it could not be.

/// The system's hold on the hotkey window's key, kept in step with the settings. It remembers
/// what it last **asked** for (so the same binding is not asked twice, whatever the answer was),
/// what the system holds, and why it did not when it did not.
#[derive(Debug, Default)]
pub struct Registration {
    attempted: Option<Binding>,
    held: Option<Binding>,
    problem: Option<Refusal>,
    /// A field of Settings is listening for a shortcut: the system holds nothing, so the field
    /// hears the keys that were the window's too ([`Registration::set_quiet`]).
    quiet: bool,
}

impl Registration {
    pub fn new() -> Self {
        Self::default()
    }

    /// Brings what the system holds to what `settings` ask of this `instance`: the old binding
    /// is let go and the new one held, in that order, and only when the binding changed. An
    /// instance that does not [`owns_global_keys`] asks for nothing, so a timed run, the agent's
    /// copy and a second instance of the bundle never touch the system's keys.
    pub fn sync(&mut self, keys: &mut dyn GlobalKeys, settings: &Settings, instance: &Instance) {
        // While the field listens nothing is held, whatever the settings say: they are asked
        // again when it stops.
        if self.quiet {
            return;
        }
        let wanted = binding_for(settings, instance);
        if wanted == self.attempted {
            return;
        }
        self.attempted = wanted;
        match keys.bind(wanted) {
            Ok(()) => {
                self.held = wanted;
                self.problem = None;
            }
            Err(refusal) => {
                self.held = None;
                self.problem = Some(refusal);
            }
        }
    }

    /// A field of Settings starts (`true`) or stops listening for a shortcut. While it listens
    /// the system holds **nothing** (a chord's registration and a double tap's monitors are let go),
    /// so the key the window has now is heard by the field like any other, and pressing it does
    /// not show or hide the window. When it stops, what the settings ask is held again. Why a
    /// key was not held stays as it was, so the page does not change under the field.
    ///
    /// `true` when the field has just stopped listening and what the settings ask was held
    /// again: what the system said about it is news to tell, which no other call is.
    pub fn set_quiet(
        &mut self,
        keys: &mut dyn GlobalKeys,
        settings: &Settings,
        instance: &Instance,
        quiet: bool,
    ) -> bool {
        if self.quiet == quiet {
            return false;
        }
        self.quiet = quiet;
        if quiet {
            if self.held.take().is_some() {
                // Letting go cannot be refused.
                let _ = keys.bind(None);
            }
        } else {
            // Asked again from nothing: a refusal from before is not kept for a key that is no
            // longer asked for (it was taken away while the field listened).
            self.attempted = None;
            self.problem = None;
            self.sync(keys, settings, instance);
        }
        !quiet
    }

    /// What the system holds now.
    pub fn held(&self) -> Option<Binding> {
        self.held
    }

    /// Why the key asked for is not held, if it is not.
    pub fn problem(&self) -> Option<Refusal> {
        self.problem
    }

    /// The line the settings' diagnostics carry about it (empty when all is well, and while a
    /// field of Settings listens: what was refused then was the shortcut that is being replaced,
    /// and it would be told with the name of the new one).
    pub fn notices(&self, shortcut: Option<Shortcut>) -> Vec<String> {
        if self.quiet {
            return Vec::new();
        }
        self.problem
            .map(|refusal| vec![refusal_note(refusal, shortcut)])
            .unwrap_or_default()
    }
}

/// What to tell the person about a key the system did not hold, **in the file's language**: the key
/// as the file writes it, between code quotes, as a line of the settings' diagnostics
/// ([`refusal_note`]) and the notices say it. The Settings row says it to a person
/// ([`refusal_words`]). A registration that succeeds says nothing about whether another
/// application also wants the key: only a refusal is told.
pub fn refusal_text(refusal: Refusal, shortcut: Option<Shortcut>) -> String {
    let key = shortcut.map_or_else(
        || "The shortcut".to_owned(),
        |shortcut| shortcut.to_string(),
    );
    match refusal {
        Refusal::Refused(status) => format!(
            "The system did not give `{key}` (status {status}); another application may hold it."
        ),
    }
}

/// [`refusal_text`] as the Settings row says it, to a person: the key in its glyphs (⌥ ⌥, not
/// `option+option`), no code quotes.
pub fn refusal_words(refusal: Refusal, shortcut: Option<Shortcut>) -> String {
    let named = shortcut.map(|shortcut| {
        crate::shortcuts::glyphs(&shortcut).unwrap_or_else(|| shortcut.to_string())
    });
    match refusal {
        Refusal::Refused(status) => format!(
            "The system did not give {} (status {status}); another application may hold it.",
            named.as_deref().unwrap_or("the shortcut")
        ),
    }
}

/// [`refusal_text`] as a line of the settings' diagnostics, which name the key they are about.
pub fn refusal_note(refusal: Refusal, shortcut: Option<Shortcut>) -> String {
    format!(
        "hotkey_window.shortcut: {}",
        refusal_text(refusal, shortcut)
    )
}

// ---------------------------------------------------------------------------------------------
// The double tap: the count the system's event monitors feed.

/// Whether a key, a click or a scroll **came after the press that began a count**, which spoils
/// the count (⌥e is not half of ⌥⌥): the press went down `since_press_ms` ago and the last such
/// input was `since_input_ms` ago, both as of the same moment, so the input is the later one when
/// it is the nearer. An input at the very moment of the press is the press's own company, not
/// something that came after it.
///
/// This is how a double tap is heard **without a permission**: the modifier changes come from
/// the system's event monitors, which an application with no permission still gets for a
/// modifier, but not the keys, clicks or scrolls that would spoil the count. Those the system
/// tells on request, as the time since the last of them
/// (`CGEventSourceSecondsSinceLastEventType`), and this is the question put to that answer.
pub fn spoils_count(since_press_ms: u64, since_input_ms: u64) -> bool {
    since_input_ms < since_press_ms
}

/// How long before an event the last key, click or scroll was, when the system was asked
/// `lag` seconds **after** the event (a main thread that was busy delivers it late): `ages_now`
/// are the system's answers, one for each kind of input (it tells the last of each kind). A kind
/// whose last input came after the event says nothing of before it and is left out, so a click
/// that came in the lag does not hide a key that came before the event. The youngest of the
/// rest is the answer, as old as can be when there is none. (What stays hidden is an input of the
/// same kind that came in the lag over an earlier one: the system keeps only the last.)
pub fn input_age_at_event(ages_now: &[f64], lag: f64) -> f64 {
    let lag = lag.max(0.0);
    ages_now
        .iter()
        .map(|age| age - lag)
        .filter(|age| *age >= 0.0)
        .fold(f64::INFINITY, f64::min)
}

/// Seconds as whole milliseconds, never below nothing and never past what a `u64` holds.
fn millis(seconds: f64) -> u64 {
    if seconds.is_nan() || seconds <= 0.0 {
        0
    } else if seconds * 1000.0 >= u64::MAX as f64 {
        u64::MAX
    } else {
        (seconds * 1000.0).round() as u64
    }
}

/// The count of the one modifier the hotkey window waits for, fed by the system's event monitors
/// ([`crate::shortcuts::Taps`] is the one machine behind every double press: the live trigger
/// and the recorder use it too, so the three cannot disagree on what a double press is). The
/// thresholds are the application's (`arrange::taps()`): none is written here. What spoils a
/// count is asked of the system at each modifier change ([`spoils_count`]).
#[derive(Clone, Debug)]
pub struct DoubleTap {
    wanted: Modifier,
    taps: crate::shortcuts::Taps,
}

impl DoubleTap {
    /// A count that answers the second tap of `wanted`.
    pub fn new(wanted: Modifier, taps: crate::shortcuts::Taps) -> Self {
        Self { wanted, taps }
    }

    /// The modifiers held after a change, at `at` (seconds, the event's own time): whether this
    /// change was the second quick tap of the one modifier waited for. Another modifier's double
    /// tap, fn's, and a first tap are not. `input_age` is how long before `at` the last key,
    /// click or scroll was, in seconds ([`input_age_at_event`]): one that came after the press
    /// that began the count spoils it first, as if it had been told when it happened.
    pub fn flags(&mut self, at: f64, held: crate::shortcuts::Held, input_age: f64) -> bool {
        use crate::shortcuts::{Step, Tapped};
        if self
            .taps
            .began()
            .is_some_and(|began| spoils_count(millis(at - began), millis(input_age)))
        {
            self.taps.interrupt();
        }
        matches!(
            self.taps.flags(at, held),
            Step::Twice(Tapped::Modifier(modifier)) if modifier == self.wanted
        )
    }

    /// A key, a click or a scroll told as it happened: whatever was being counted is lost.
    pub fn interrupt(&mut self) {
        self.taps.interrupt();
    }
}

// ---------------------------------------------------------------------------------------------
// What the system's registration is told.

/// Carbon's modifier mask of `mods` (`cmdKey`, `shiftKey`, `optionKey`, `controlKey`).
pub fn carbon_modifiers(mods: bt_core::Mods) -> u32 {
    let mut mask = 0;
    if mods.cmd {
        mask |= 0x0100;
    }
    if mods.shift {
        mask |= 0x0200;
    }
    if mods.option {
        mask |= 0x0800;
    }
    if mods.ctrl {
        mask |= 0x1000;
    }
    mask
}

/// The virtual key code of a named key. **Function keys are not in a row:** F13–F20 are
/// 105, 107, 113, 106, 64, 79, 80, 90 (F1–F12 follow the keyboard's own scatter), so the table is
/// the answer and no arithmetic is.
pub fn named_key_code(key: bt_core::NamedKey) -> Option<u16> {
    use bt_core::NamedKey;
    const FUNCTION: [u16; 20] = [
        122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111, 105, 107, 113, 106, 64, 79, 80, 90,
    ];
    Some(match key {
        NamedKey::Left => 123,
        NamedKey::Right => 124,
        NamedKey::Down => 125,
        NamedKey::Up => 126,
        NamedKey::Tab => 48,
        NamedKey::Return => 36,
        NamedKey::Space => 49,
        NamedKey::Escape => 53,
        NamedKey::Backspace => 51,
        NamedKey::Delete => 117,
        NamedKey::Home => 115,
        NamedKey::End => 119,
        NamedKey::PageUp => 116,
        NamedKey::PageDown => 121,
        NamedKey::F(number) => *FUNCTION.get(usize::from(number.checked_sub(1)?))?,
    })
}

/// The stand-in for the system's keys that tests use: it keeps what it was asked to hold and
/// holds nothing.
#[cfg(any(test, feature = "test-support"))]
pub mod fake {
    use super::{Binding, GlobalKeys, Refusal};

    /// A [`GlobalKeys`] that registers nothing anywhere.
    #[derive(Debug, Default)]
    pub struct Keys {
        /// What it holds now.
        pub held: Option<Binding>,
        /// Every call of [`GlobalKeys::bind`], in order.
        pub calls: Vec<Option<Binding>>,
        /// What the next [`GlobalKeys::bind`] answers (kept until changed).
        pub refuse: Option<Refusal>,
    }

    impl GlobalKeys for Keys {
        fn bind(&mut self, binding: Option<Binding>) -> Result<(), Refusal> {
            self.calls.push(binding);
            if let Some(refusal) = self.refuse
                && binding.is_some()
            {
                self.held = None;
                return Err(refusal);
            }
            self.held = binding;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_core::{Key, Mods, NamedKey};

    fn frame(x: f64, y: f64, width: f64, height: f64) -> Frame {
        Frame {
            x,
            y,
            width,
            height,
        }
    }

    /// The built-in display (the menu bar above, the Dock below) and a larger one to its right
    /// that sits lower: the second one's origin is not the first's.
    fn two_screens() -> [Screen; 2] {
        [
            Screen {
                frame: frame(0.0, 0.0, 1440.0, 900.0),
                visible: frame(0.0, 40.0, 1440.0, 835.0),
            },
            Screen {
                frame: frame(1440.0, -180.0, 1920.0, 1080.0),
                visible: frame(1440.0, -180.0, 1920.0, 1055.0),
            },
        ]
    }

    const POSITIONS: [HotkeyPosition; 5] = [
        HotkeyPosition::Top,
        HotkeyPosition::Bottom,
        HotkeyPosition::Left,
        HotkeyPosition::Right,
        HotkeyPosition::Center,
    ];

    fn right(frame: &Frame) -> f64 {
        frame.x + frame.width
    }

    fn top(frame: &Frame) -> f64 {
        frame.y + frame.height
    }

    fn overlap(a: &Frame, b: &Frame) -> f64 {
        let width = right(a).min(right(b)) - a.x.max(b.x);
        let height = top(a).min(top(b)) - a.y.max(b.y);
        width.max(0.0) * height.max(0.0)
    }

    #[test]
    fn the_pointers_screen_is_the_one_it_is_on_and_the_first_when_it_is_on_none() {
        let screens = two_screens();
        let pick = |policy, pointer| screen_index(&screens, policy, pointer);
        assert_eq!(pick(HotkeyScreen::Pointer, Some((100.0, 100.0))), Some(0));
        assert_eq!(pick(HotkeyScreen::Pointer, Some((2000.0, -100.0))), Some(1));
        // The seam is on exactly one screen, and the menu bar is still its screen's.
        assert_eq!(pick(HotkeyScreen::Pointer, Some((1440.0, 100.0))), Some(1));
        assert_eq!(pick(HotkeyScreen::Pointer, Some((1439.9, 899.9))), Some(0));
        // The top row of a screen reads `maxY` (the pointer's `y` counts up from the bottom), the
        // bottom row of the one at the edge reads `minY`: both are on their screen.
        assert_eq!(pick(HotkeyScreen::Pointer, Some((2000.0, 900.0))), Some(1));
        assert_eq!(pick(HotkeyScreen::Pointer, Some((2000.0, -180.0))), Some(1));
        assert_eq!(pick(HotkeyScreen::Pointer, Some((300.0, 900.0))), Some(0));
        assert_eq!(pick(HotkeyScreen::Pointer, Some((3360.0, 300.0))), Some(1));
        // A screen that was unplugged, a pointer nobody knows.
        assert_eq!(pick(HotkeyScreen::Pointer, Some((9000.0, 9000.0))), Some(0));
        assert_eq!(pick(HotkeyScreen::Pointer, None), Some(0));
        // The primary one, wherever the pointer is.
        assert_eq!(pick(HotkeyScreen::Primary, Some((2000.0, -100.0))), Some(0));
        assert_eq!(
            screen_index(&[], HotkeyScreen::Pointer, Some((0.0, 0.0))),
            None
        );
        assert_eq!(screen_index(&[], HotkeyScreen::Primary, None), None);
    }

    /// The frame of every position on both screens, said by the relation to the usable area
    /// and not by a number worked out by hand: a flipped axis is wrong in every row alike.
    #[test]
    fn every_position_hugs_its_edge_and_is_centered_along_it() {
        for screen in two_screens() {
            let area = screen.visible;
            for (depth, span) in [(50, 100), (30, 60), (100, 100), (10, 10)] {
                for position in POSITIONS {
                    let shown = shown_frame(area, position, depth, span);
                    let say = format!("{position:?} {depth}/{span} on {area:?}: {shown:?}");
                    let (d, s) = (f64::from(depth) / 100.0, f64::from(span) / 100.0);
                    let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
                    match position {
                        HotkeyPosition::Top => {
                            assert!(near(top(&shown), top(&area)), "{say}");
                            assert!(near(shown.height, area.height * d), "{say}");
                            assert!(near(shown.width, area.width * s), "{say}");
                        }
                        HotkeyPosition::Bottom => {
                            assert!(near(shown.y, area.y), "{say}");
                            assert!(near(shown.height, area.height * d), "{say}");
                            assert!(near(shown.width, area.width * s), "{say}");
                        }
                        HotkeyPosition::Left => {
                            assert!(near(shown.x, area.x), "{say}");
                            assert!(near(shown.width, area.width * d), "{say}");
                            assert!(near(shown.height, area.height * s), "{say}");
                        }
                        HotkeyPosition::Right => {
                            assert!(near(right(&shown), right(&area)), "{say}");
                            assert!(near(shown.width, area.width * d), "{say}");
                            assert!(near(shown.height, area.height * s), "{say}");
                        }
                        HotkeyPosition::Center => {
                            assert!(near(shown.height, area.height * d), "{say}");
                            assert!(near(shown.width, area.width * s), "{say}");
                        }
                    }
                    // Centered along the edge it hugs (both ways for the center).
                    let across = matches!(
                        position,
                        HotkeyPosition::Top | HotkeyPosition::Bottom | HotkeyPosition::Center
                    );
                    if across {
                        let (left, rest) = (shown.x - area.x, right(&area) - right(&shown));
                        assert!(near(left, rest), "{say}");
                    }
                    if matches!(
                        position,
                        HotkeyPosition::Left | HotkeyPosition::Right | HotkeyPosition::Center
                    ) {
                        let (below, above) = (shown.y - area.y, top(&area) - top(&shown));
                        assert!(near(below, above), "{say}");
                    }
                    // Always inside what the person can use.
                    assert!(
                        overlap(&shown, &area) > shown.width * shown.height - 1e-6,
                        "{say}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_defaults_are_a_half_height_band_across_the_top_and_a_half_width_column() {
        let area = two_screens()[0].visible;
        let band = shown_frame(area, HotkeyPosition::Top, 50, 100);
        assert_eq!((band.width, band.height), (1440.0, 417.5));
        let column = shown_frame(area, HotkeyPosition::Left, 50, 100);
        assert_eq!((column.width, column.height), (720.0, 835.0));
        let middle = shown_frame(area, HotkeyPosition::Center, 50, 100);
        assert_eq!((middle.width, middle.height), (1440.0, 417.5));
    }

    #[test]
    fn a_share_outside_the_range_is_brought_into_it() {
        let area = two_screens()[0].visible;
        let small = shown_frame(area, HotkeyPosition::Top, 0, 0);
        assert_eq!((small.width, small.height), (144.0, 83.5));
        let large = shown_frame(area, HotkeyPosition::Top, 200, 255);
        assert_eq!((large.width, large.height), (1440.0, 835.0));
    }

    #[test]
    fn a_dragged_frame_comes_back_as_the_shares_that_made_it() {
        for screen in two_screens() {
            for position in POSITIONS {
                for (depth, span) in [(50, 100), (33, 61), (10, 10), (100, 100)] {
                    let shown = shown_frame(screen.visible, position, depth, span);
                    assert_eq!(
                        shares(screen.visible, position, shown),
                        (depth, span),
                        "{position:?}"
                    );
                }
            }
        }
        // A drag past the edge or to nothing stays inside the range.
        let area = two_screens()[0].visible;
        let past = frame(0.0, 0.0, 5000.0, 5000.0);
        let none = frame(0.0, 0.0, 0.0, 0.0);
        assert_eq!(shares(area, HotkeyPosition::Top, past), (100, 100));
        assert_eq!(shares(area, HotkeyPosition::Right, none), (10, 10));
        assert_eq!(
            shares(frame(0.0, 0.0, 0.0, 0.0), HotkeyPosition::Top, past),
            (100, 100)
        );
    }

    #[test]
    fn a_slide_moves_the_place_and_never_the_size_and_starts_out_of_sight() {
        for screen in two_screens() {
            for position in POSITIONS {
                let shown = shown_frame(screen.visible, position, 40, 80);
                let moving = slide(&screen, shown, position);
                let say = format!("{position:?} on {screen:?}");
                assert_eq!(moving.shown, shown, "{say}");
                assert_eq!(
                    (moving.away.width, moving.away.height),
                    (shown.width, shown.height),
                    "{say}"
                );
                // Out of sight: not one point of it is on the screen.
                assert_eq!(overlap(&moving.away, &screen.frame), 0.0, "{say}");
                // It comes in on the edge it is named for, along the axis of that edge only.
                match position {
                    HotkeyPosition::Top | HotkeyPosition::Center => {
                        assert_eq!(moving.away.y, top(&screen.frame), "{say}");
                        assert_eq!(moving.away.x, shown.x, "{say}");
                    }
                    HotkeyPosition::Bottom => {
                        assert_eq!(top(&moving.away), screen.frame.y, "{say}");
                        assert_eq!(moving.away.x, shown.x, "{say}");
                    }
                    HotkeyPosition::Left => {
                        assert_eq!(right(&moving.away), screen.frame.x, "{say}");
                        assert_eq!(moving.away.y, shown.y, "{say}");
                    }
                    HotkeyPosition::Right => {
                        assert_eq!(moving.away.x, right(&screen.frame), "{say}");
                        assert_eq!(moving.away.y, shown.y, "{say}");
                    }
                }
            }
        }
        assert_eq!(SLIDE_MILLIS, 240);
    }

    /// A window whose way in is clear slides; one whose way in is another screen fades where it is.
    #[test]
    fn a_window_fades_where_the_place_it_comes_from_is_another_screen() {
        let screens = two_screens();
        let shown = |screen: &Screen, position| shown_frame(screen.visible, position, 40, 80);
        let among = |on: usize, position| {
            slide_among(&screens, on, shown(&screens[on], position), position).expect("a screen")
        };
        // Nothing above the first screen: the top is clear, and so is the left. Its right edge is
        // the second screen's, which a window from the right would be seen on.
        assert!(!among(0, HotkeyPosition::Top).fade);
        assert!(!among(0, HotkeyPosition::Center).fade);
        assert!(!among(0, HotkeyPosition::Left).fade);
        assert!(among(0, HotkeyPosition::Right).fade);
        // The second screen has the first beside it on its left, and nothing at the others.
        assert!(among(1, HotkeyPosition::Left).fade);
        assert!(!among(1, HotkeyPosition::Right).fade);
        assert!(!among(1, HotkeyPosition::Top).fade);
        // The faded window does not travel, and has the frame of the slide all the same.
        let right = among(0, HotkeyPosition::Right);
        assert_eq!(right.shown, shown(&screens[0], HotkeyPosition::Right));
        // One screen: nothing to be seen on.
        for position in POSITIONS {
            let alone = slide_among(&screens[..1], 0, shown(&screens[0], position), position);
            assert_eq!(alone.map(|slide| slide.fade), Some(false), "{position:?}");
        }
        // A screen above: the top fades; a screen only beside the corner (touching, no overlap
        // of the window's own width) does not.
        let above = [
            screens[0],
            Screen {
                frame: frame(0.0, 900.0, 1440.0, 900.0),
                visible: frame(0.0, 900.0, 1440.0, 875.0),
            },
        ];
        let top = slide_among(
            &above,
            0,
            shown(&above[0], HotkeyPosition::Top),
            HotkeyPosition::Top,
        );
        assert_eq!(top.map(|slide| slide.fade), Some(true));
        let bottom = slide_among(
            &above,
            0,
            shown(&above[0], HotkeyPosition::Bottom),
            HotkeyPosition::Bottom,
        );
        assert_eq!(bottom.map(|slide| slide.fade), Some(false));
        // No such screen.
        assert!(
            slide_among(
                &screens,
                5,
                shown(&screens[0], HotkeyPosition::Top),
                HotkeyPosition::Top
            )
            .is_none()
        );
    }

    /// A window that is in sight stays on the screen it is on: the one its frame overlaps the
    /// most, whatever the pointer or the primary screen say.
    #[test]
    fn a_window_in_sight_is_on_the_screen_it_overlaps_the_most() {
        let screens = two_screens();
        // Wholly on the second, partly across the seam on either side, off every screen.
        assert_eq!(
            screen_of(&screens, frame(1500.0, 0.0, 800.0, 400.0)),
            Some(1)
        );
        assert_eq!(
            screen_of(&screens, frame(1000.0, 100.0, 600.0, 300.0)),
            Some(0)
        );
        assert_eq!(
            screen_of(&screens, frame(1200.0, 100.0, 600.0, 300.0)),
            Some(1)
        );
        assert_eq!(
            screen_of(&screens, frame(5000.0, 5000.0, 100.0, 100.0)),
            None
        );
        // Only touching the edge of a screen is not on it.
        assert_eq!(screen_of(&screens, frame(-100.0, 0.0, 100.0, 100.0)), None);
        // Equal: the first.
        assert_eq!(
            screen_of(&screens, frame(1340.0, 100.0, 200.0, 100.0)),
            Some(0)
        );
        assert_eq!(screen_of(&[], frame(0.0, 0.0, 10.0, 10.0)), None);
    }

    /// The application the person was in comes back only when they asked for the window to go
    /// and the window had the keyboard; every other way out lets the record go and brings nobody.
    #[test]
    fn the_application_before_comes_back_only_to_a_person_who_asked_from_the_window() {
        for (cause, had_key, back) in [
            (HideCause::Asked, true, true),
            (HideCause::Asked, false, false),
            (HideCause::Closed, true, true),
            (HideCause::Closed, false, false),
            (HideCause::FocusLeft, true, false),
            (HideCause::FocusLeft, false, false),
        ] {
            assert_eq!(
                restores_previous(cause, had_key),
                back,
                "{cause:?} {had_key}"
            );
        }
    }

    #[test]
    fn a_tap_shows_what_is_away_and_hides_what_is_in_front_and_brings_what_is_not() {
        use Presence::{Absent, Hidden, Shown};
        let (toggle, show, hide) = (Verb::Toggle, Verb::Show, Verb::Hide);
        let table = [
            (toggle, Absent, Step::Open),
            (toggle, Hidden, Step::Show),
            (toggle, Shown { key: true }, Step::Hide),
            // Pinned, with the keyboard somewhere else: *come here*.
            (toggle, Shown { key: false }, Step::Raise),
            (show, Absent, Step::Open),
            (show, Hidden, Step::Show),
            (show, Shown { key: true }, Step::Nothing),
            (show, Shown { key: false }, Step::Raise),
            (hide, Absent, Step::Nothing),
            (hide, Hidden, Step::Nothing),
            (hide, Shown { key: true }, Step::Hide),
            (hide, Shown { key: false }, Step::Hide),
        ];
        for (verb, presence, step) in table {
            assert_eq!(decide(verb, presence), step, "{verb:?} on {presence:?}");
        }
    }

    #[test]
    fn the_focus_leaving_hides_unless_it_went_to_a_surface_of_the_applications_own() {
        let on = Hiding {
            hide_on_click_away: true,
            pinned: false,
            follow: false,
        };
        let key = |to| Lost::Key { to };
        // The table, for a window that hides.
        assert!(
            !hides_on(key(KeyTaker::OwnSurface), on),
            "a sheet, the palette, Settings"
        );
        assert!(
            hides_on(key(KeyTaker::OtherTerminal), on),
            "a normal window"
        );
        // The right-click menu changes no key window: nothing is known, so nothing hides; the
        // application leaving ([`Lost::Active`]) is the one that does when it really left.
        assert!(!hides_on(key(KeyTaker::Unknown), on));
        assert!(hides_on(Lost::Active, on));
        // A pin and the setting each silence all of it.
        for off in [
            Hiding { pinned: true, ..on },
            Hiding {
                hide_on_click_away: false,
                ..on
            },
            Hiding {
                hide_on_click_away: false,
                pinned: true,
                ..on
            },
        ] {
            for lost in [
                key(KeyTaker::OwnSurface),
                key(KeyTaker::OtherTerminal),
                key(KeyTaker::Unknown),
                Lost::Active,
            ] {
                assert!(!hides_on(lost, off), "{lost:?} with {off:?}");
            }
        }
    }

    /// The settings and the pin as a window in sight reads them: `Hiding` for a window that
    /// hides on a click away (`hide_on_click_away`, not pinned), with the Spaces policy.
    fn hiding(follow: bool, pinned: bool, hide_on_click_away: bool) -> Hiding {
        Hiding {
            hide_on_click_away,
            pinned,
            follow,
        }
    }

    #[test]
    fn a_desktop_change_is_followed_in_follow_and_is_what_it_was_in_stay() {
        use Desktop::{Elsewhere, Here};
        use KeyTaker::{OtherTerminal, OwnSurface, Unknown};
        let losses = [
            Lost::Key { to: OwnSurface },
            Lost::Key { to: OtherTerminal },
            Lost::Key { to: Unknown },
            Lost::Active,
        ];
        for lost in losses {
            // `follow`: the window on another desktop comes, whatever took the focus.
            assert_eq!(
                leaving(lost, hiding(true, false, true), Elsewhere),
                Leaving::Follows,
                "{lost:?}"
            );
            // A window something keeps in sight (a pin, `hide_on_click_away = false`) belongs to
            // every desktop: it is never brought, so it never takes the keyboard.
            for (pinned, click_away) in [(true, true), (false, false), (true, false)] {
                assert_eq!(
                    leaving(lost, hiding(true, pinned, click_away), Elsewhere),
                    Leaving::Stays,
                    "{lost:?}, pinned {pinned}, click away {click_away}"
                );
            }
            // On the desktop it is on, `follow` is no different from `stay`: nothing to follow.
            for (pinned, click_away) in [(false, true), (true, true), (false, false)] {
                let hiding = hiding(true, pinned, click_away);
                let expected = if hides_on(lost, hiding) {
                    Leaving::Hides
                } else {
                    Leaving::Stays
                };
                assert_eq!(leaving(lost, hiding, Here), expected, "{lost:?} {hiding:?}");
            }
            // `stay`: the desktop is not asked; the answer is `hides_on`'s, as before.
            for (pinned, click_away) in [(false, true), (true, true), (false, false)] {
                let hiding = hiding(false, pinned, click_away);
                let expected = if hides_on(lost, hiding) {
                    Leaving::Hides
                } else {
                    Leaving::Stays
                };
                for desktop in [Here, Elsewhere] {
                    assert_eq!(leaving(lost, hiding, desktop), expected);
                }
            }
        }
        assert!(follows(hiding(true, false, true), Elsewhere));
        assert!(!follows(hiding(true, false, true), Here));
        assert!(!follows(hiding(false, false, true), Elsewhere));
    }

    /// A window in sight and the person's desktops, to the two ways the application hears of a
    /// desktop change (the window losing the key, and the system's notification), in the order
    /// each test gives. Each handler is what the application's own does: it asks the rule, and
    /// does what the rule says.
    struct Stage {
        hiding: Hiding,
        /// The window is in sight.
        shown: bool,
        /// The window is on the desktop in front.
        here: bool,
        /// The window has the keyboard.
        key: bool,
        /// The application is the active one.
        active: bool,
        /// How many times the window was brought along.
        brought: u32,
    }

    impl Stage {
        fn new(hiding: Hiding) -> Self {
            Stage {
                hiding,
                shown: true,
                here: true,
                key: true,
                active: true,
                brought: 0,
            }
        }

        fn desktop(&self) -> Desktop {
            // A window of every desktop is on the one in front, whichever it is.
            if self.here || desktops(self.hiding) == Desktops::Every {
                Desktop::Here
            } else {
                Desktop::Elsewhere
            }
        }

        /// The person goes to another desktop: the window is not on it and is not key, and the
        /// application is not the active one (another one is, there).
        fn the_person_goes_elsewhere(&mut self) {
            self.here = false;
            self.key = false;
            self.active = false;
        }

        /// The window lost the key (read a turn later, with the window not key):
        /// `AppDelegate::hotkey_focus_settled`.
        fn key_resigned(&mut self, lost: Lost) {
            if !self.shown || self.key {
                return;
            }
            self.act(leaving(lost, self.hiding, self.desktop()));
        }

        /// The application stopped being active: `AppDelegate::hotkey_active_lost`. It is
        /// told a turn after the fact: an application that is the active one again by then
        /// (the window was brought along, which activates it) lost nothing.
        fn active_lost(&mut self) {
            if self.shown && !self.active {
                self.act(leaving(Lost::Active, self.hiding, self.desktop()));
            }
        }

        /// The system said the desktop in front changed: `AppDelegate::hotkey_desktop_changed`.
        fn notified(&mut self) {
            if self.shown && follows(self.hiding, self.desktop()) {
                self.act(Leaving::Follows);
            }
        }

        fn act(&mut self, leaving: Leaving) {
            match leaving {
                Leaving::Hides => self.shown = false,
                Leaving::Follows => {
                    self.here = true;
                    self.key = true;
                    self.active = true;
                    self.brought += 1;
                }
                Leaving::Stays => {}
            }
        }
    }

    #[test]
    fn the_settings_window_is_over_the_panel_while_the_panel_is_in_sight_and_ordinary_after() {
        assert_eq!(settings_level(true, true), SettingsLevel::AbovePanel);
        assert_eq!(settings_level(false, true), SettingsLevel::Normal);
        // A pinned panel over another application: the Settings window does not float with it.
        assert_eq!(settings_level(true, false), SettingsLevel::Normal);
        assert_eq!(settings_level(false, false), SettingsLevel::Normal);
    }

    #[test]
    fn the_window_follows_whichever_comes_first_the_resign_or_the_notification() {
        // Every pair of what the application can hear, each with the application becoming
        // inactive too (another application on the new desktop) or not.
        let sequences: [&[&str]; 6] = [
            &["resign", "notify"],
            &["notify", "resign"],
            &["inactive", "notify"],
            &["notify", "inactive"],
            &["resign", "inactive", "notify"],
            &["notify", "resign", "inactive"],
        ];
        for pinned in [false, true] {
            for sequence in sequences {
                let mut stage = Stage::new(hiding(true, pinned, true));
                stage.the_person_goes_elsewhere();
                for step in sequence {
                    match *step {
                        "resign" => stage.key_resigned(Lost::Key {
                            to: KeyTaker::Unknown,
                        }),
                        "inactive" => stage.active_lost(),
                        _ => stage.notified(),
                    }
                }
                if pinned {
                    // A pinned window belongs to every desktop: in sight there, never brought,
                    // and the keyboard stays with the application the person went to.
                    assert!(
                        stage.shown && stage.desktop() == Desktop::Here && !stage.key,
                        "{sequence:?}, pinned: in sight, on the desktop, without the keyboard"
                    );
                    assert_eq!(stage.brought, 0, "{sequence:?}");
                } else {
                    assert!(
                        stage.shown && stage.here && stage.key,
                        "{sequence:?}: in sight, on the desktop, with the keyboard"
                    );
                    assert!(stage.brought >= 1, "{sequence:?}");
                }
            }
        }
    }

    #[test]
    fn a_window_kept_in_sight_belongs_to_every_desktop_in_follow_and_to_its_own_in_stay() {
        assert_eq!(desktops(hiding(true, false, true)), Desktops::Front);
        assert_eq!(desktops(hiding(true, true, true)), Desktops::Every);
        assert_eq!(desktops(hiding(true, false, false)), Desktops::Every);
        for (pinned, click_away) in [(false, true), (true, true), (false, false)] {
            assert_eq!(desktops(hiding(false, pinned, click_away)), Desktops::Its);
        }
    }

    #[test]
    fn an_application_switch_to_a_window_on_another_desktop_hides_the_window_and_it_stays_hidden() {
        // Measured on a Mac: activating an application whose window is on another desktop
        // deactivates this one while the desktop in front is still the old one; the desktop
        // changes about 300 ms later. The window is where it was at the deactivation, so the
        // focus leaving hides it, and the notice that follows finds nothing in sight to bring.
        for pinned_or_kept in [false] {
            for first in ["resign", "inactive"] {
                let mut stage = Stage::new(hiding(true, pinned_or_kept, true));
                // The application is switched, the desktop is not changed yet.
                stage.key = false;
                stage.active = false;
                match first {
                    "resign" => stage.key_resigned(Lost::Active),
                    _ => stage.active_lost(),
                }
                assert!(
                    !stage.shown,
                    "{first}: hidden, as for any other application"
                );
                // The desktop changes.
                stage.here = false;
                stage.notified();
                assert!(!stage.shown, "{first}: still hidden after the notice");
                assert_eq!(stage.brought, 0, "{first}: nothing was brought");
            }
        }
        // A pinned window, or one that is kept for good, is in sight on every desktop and is
        // not hidden by the switch at all.
        for (pinned, click_away) in [(true, true), (false, false)] {
            let mut stage = Stage::new(hiding(true, pinned, click_away));
            stage.key = false;
            stage.active = false;
            stage.active_lost();
            stage.here = false;
            stage.notified();
            assert!(stage.shown && stage.brought == 0);
        }
        // `stay` hides it the same, and nothing comes along.
        let mut stage = Stage::new(hiding(false, false, true));
        stage.key = false;
        stage.active = false;
        stage.active_lost();
        stage.here = false;
        stage.notified();
        assert!(!stage.shown && stage.brought == 0);
    }

    #[test]
    fn in_stay_the_application_leaving_hides_in_either_order_and_nothing_comes_along() {
        for sequence in [["resign", "inactive"], ["inactive", "resign"]] {
            let mut stage = Stage::new(hiding(false, false, true));
            stage.the_person_goes_elsewhere();
            for step in sequence {
                match step {
                    "resign" => stage.key_resigned(Lost::Key {
                        to: KeyTaker::Unknown,
                    }),
                    _ => stage.active_lost(),
                }
                // The notification between each: `stay` does not read it.
                stage.notified();
            }
            assert!(!stage.shown, "{sequence:?}: hidden, as before");
            assert_eq!(stage.brought, 0, "{sequence:?}");
        }
        // With no other application to become active, the window loses the key and nothing hides
        // it: it stays on the desktop it was on.
        let mut stage = Stage::new(hiding(false, false, true));
        stage.the_person_goes_elsewhere();
        stage.key_resigned(Lost::Key {
            to: KeyTaker::Unknown,
        });
        stage.notified();
        assert!(stage.shown && !stage.here && stage.brought == 0);
    }

    #[test]
    fn a_hidden_hotkey_window_is_not_a_window_for_the_dock_or_for_a_new_pane() {
        let normal = Candidate::normal();
        let away = Candidate::hotkey(false);
        let seen = Candidate::hotkey(true);
        // The Dock and the launch.
        assert!(needs_normal_window(&[]));
        assert!(
            needs_normal_window(&[away]),
            "only a hidden hotkey window: open one"
        );
        assert!(
            needs_normal_window(&[seen]),
            "a hotkey window is no normal window"
        );
        assert!(!needs_normal_window(&[away, normal]));
        assert!(!needs_normal_window(&[normal]));
        // A new pane: the key window, else the main one, else the front one.
        assert_eq!(pane_window(&[]), None);
        assert_eq!(pane_window(&[away]), None, "nowhere anyone can look");
        assert_eq!(pane_window(&[away, normal]), Some(1));
        assert_eq!(
            pane_window(&[seen, normal]),
            Some(0),
            "the front one that is in sight"
        );
        let key = Candidate {
            key: true,
            ..Candidate::normal()
        };
        let main = Candidate {
            main: true,
            ..Candidate::normal()
        };
        assert_eq!(pane_window(&[normal, main, key]), Some(2));
        assert_eq!(pane_window(&[normal, main]), Some(1));
        let key_hotkey = Candidate {
            key: true,
            ..Candidate::hotkey(true)
        };
        assert_eq!(
            pane_window(&[normal, key_hotkey]),
            Some(1),
            "the person is in it"
        );
    }

    #[test]
    fn the_front_window_and_the_merge_read_normal_windows_only() {
        let normal = Candidate::normal();
        let seen = Candidate::hotkey(true);
        let away = Candidate::hotkey(false);
        assert_eq!(front_normal(&[]), None);
        assert_eq!(front_normal(&[away]), None);
        assert_eq!(
            front_normal(&[seen, normal]),
            Some(1),
            "never the hotkey window"
        );
        assert_eq!(front_normal(&[away, normal, normal]), Some(1));
        assert!(!can_merge(&[normal]));
        assert!(
            !can_merge(&[normal, away]),
            "one normal window and the hotkey one"
        );
        assert!(!can_merge(&[normal, seen]));
        assert!(can_merge(&[normal, away, normal]));
    }

    #[test]
    fn what_the_system_is_asked_to_hold_follows_the_settings_and_the_owner() {
        let option_space = Shortcut::parse_global("option+space").unwrap();
        let mut settings = Settings::default();
        assert_eq!(
            binding_of(&settings),
            None,
            "no shortcut until one is recorded"
        );
        settings.hotkey_window.shortcut = Some(option_space);
        let chord = Chord::new(
            Mods::NONE.with(Modifier::Option),
            Key::Named(NamedKey::Space),
        );
        assert_eq!(binding_of(&settings), Some(Binding::Chord(chord)));
        settings.hotkey_window.shortcut = Some(Shortcut::Twice(Modifier::Cmd));
        assert_eq!(binding_of(&settings), Some(Binding::Twice(Modifier::Cmd)));
        // A kind the section never holds is no binding.
        assert_eq!(
            Binding::of(Shortcut::Mods(
                Mods::NONE.with(Modifier::Cmd).with(Modifier::Ctrl)
            )),
            None
        );
        // A single modifier's name is the double tap.
        assert_eq!(
            Binding::of(Shortcut::Mods(Mods::NONE.with(Modifier::Option))),
            Some(Binding::Twice(Modifier::Option))
        );

        // The owner: the session lock, a bundle, no timed run, not the agent's copy.
        let owner = Instance {
            holds_session_lock: true,
            bundled: true,
            ..Instance::default()
        };
        assert!(owns_global_keys(&owner));
        for not in [
            Instance {
                holds_session_lock: false,
                ..owner
            },
            Instance {
                bundled: false,
                ..owner
            },
            Instance {
                timed_run: true,
                ..owner
            },
            Instance {
                agent_check: true,
                ..owner
            },
            Instance::default(),
        ] {
            assert!(!owns_global_keys(&not), "{not:?}");
            assert_eq!(binding_for(&settings, &not), None, "{not:?}");
        }
        assert_eq!(
            binding_for(&settings, &owner),
            Some(Binding::Twice(Modifier::Cmd))
        );
    }

    fn settings_with(shortcut: &str) -> Settings {
        let mut settings = Settings::default();
        settings.hotkey_window.shortcut =
            Some(Shortcut::parse_global(shortcut).expect("a shortcut"));
        settings
    }

    fn owner() -> Instance {
        Instance {
            holds_session_lock: true,
            bundled: true,
            ..Instance::default()
        }
    }

    /// The registration follows the settings: a changed shortcut lets the old one go and holds
    /// the new one (one call each, in that order), a shortcut that did not change is not asked
    /// again, and an emptied one lets go.
    #[test]
    fn the_registration_follows_the_settings_and_asks_once_for_each_binding() {
        let (first, second) = (settings_with("cmd+f1"), settings_with("option+f2"));
        let mut keys = fake::Keys::default();
        let mut registration = Registration::new();
        registration.sync(&mut keys, &first, &owner());
        registration.sync(&mut keys, &first, &owner());
        assert_eq!(keys.calls.len(), 1, "the same binding is held once");
        assert_eq!(registration.held(), binding_of(&first));
        registration.sync(&mut keys, &second, &owner());
        assert_eq!(keys.calls, [binding_of(&first), binding_of(&second)]);
        assert_eq!(keys.held, binding_of(&second));
        registration.sync(&mut keys, &Settings::default(), &owner());
        assert_eq!(keys.calls.last(), Some(&None));
        assert_eq!(registration.held(), None);
        assert_eq!(registration.problem(), None);
    }

    /// While a field of Settings listens the system holds nothing (the chord's registration, the
    /// double tap's monitors), the key the window has is the field's to hear, and what the settings
    /// ask is held again when it stops — the shortcut that was recorded, if one was.
    #[test]
    fn nothing_is_held_while_a_field_listens_and_the_settings_are_held_again_after() {
        let (old, new) = (settings_with("option+shift+h"), settings_with("cmd+f1"));
        let mut keys = fake::Keys::default();
        let mut registration = Registration::new();
        registration.sync(&mut keys, &old, &owner());
        assert_eq!(keys.held, binding_of(&old));
        // The field starts listening: the key is let go, and said so once.
        assert!(!registration.set_quiet(&mut keys, &old, &owner(), true));
        assert_eq!(keys.held, None);
        assert_eq!(registration.held(), None);
        assert_eq!(keys.calls, [binding_of(&old), None]);
        assert!(!registration.set_quiet(&mut keys, &old, &owner(), true));
        assert_eq!(keys.calls.len(), 2, "said once");
        // Nothing is held for a change of the settings made while it listens (the key that was
        // just recorded is written before the field is let go) ...
        registration.sync(&mut keys, &new, &owner());
        assert_eq!(keys.calls.len(), 2);
        assert_eq!(keys.held, None);
        // ... and it is held when the field stops: the key of the file as it is then.
        assert!(
            registration.set_quiet(&mut keys, &new, &owner(), false),
            "it left quiet: what was held is news"
        );
        assert!(
            !registration.set_quiet(&mut keys, &new, &owner(), false),
            "and asked again it is not"
        );
        assert_eq!(keys.held, binding_of(&new));
        assert_eq!(registration.held(), binding_of(&new));
        assert_eq!(keys.calls, [binding_of(&old), None, binding_of(&new)]);
        // Listening and stopping with nothing held asks the system for nothing it did not have:
        // the shortcut was taken away (Remove, or ⌫).
        registration.set_quiet(&mut keys, &new, &owner(), true);
        registration.sync(&mut keys, &Settings::default(), &owner());
        registration.set_quiet(&mut keys, &Settings::default(), &owner(), false);
        assert_eq!(keys.held, None);
        assert_eq!(registration.held(), None);
        assert_eq!(registration.problem(), None);
    }

    /// A key the system could not be given and then taken away while the field listens is not
    /// told after: the refusal goes with the key.
    #[test]
    fn a_refusal_does_not_outlive_the_key_that_was_taken_away_while_the_field_listened() {
        let chord = settings_with("cmd+f1");
        let mut refusing = fake::Keys {
            refuse: Some(Refusal::Refused(-9878)),
            ..fake::Keys::default()
        };
        let mut registration = Registration::new();
        registration.sync(&mut refusing, &chord, &owner());
        assert_eq!(registration.problem(), Some(Refusal::Refused(-9878)));
        registration.set_quiet(&mut refusing, &chord, &owner(), true);
        assert_eq!(
            registration.problem(),
            Some(Refusal::Refused(-9878)),
            "kept while it listens"
        );
        // Remove while it listens: the settings ask for nothing when it stops.
        registration.sync(&mut refusing, &Settings::default(), &owner());
        registration.set_quiet(&mut refusing, &Settings::default(), &owner(), false);
        assert_eq!(registration.problem(), None);
        assert!(registration.notices(None).is_empty());
        // A key that is refused when the field stops is told then.
        let mut registration = Registration::new();
        registration.set_quiet(&mut refusing, &chord, &owner(), true);
        registration.set_quiet(&mut refusing, &chord, &owner(), false);
        assert_eq!(registration.problem(), Some(Refusal::Refused(-9878)));
    }

    /// What was refused when the old shortcut was asked is not told again while the field
    /// listens (it would carry the name of the shortcut being recorded); it is told when the
    /// field stops and the new one is asked.
    #[test]
    fn a_refusal_is_not_told_with_the_name_of_the_key_that_replaced_it() {
        let (first, second) = (settings_with("cmd+f1"), settings_with("cmd+f2"));
        let mut keys = fake::Keys {
            refuse: Some(Refusal::Refused(-9878)),
            ..fake::Keys::default()
        };
        let mut registration = Registration::new();
        registration.sync(&mut keys, &first, &owner());
        assert_eq!(registration.notices(first_shortcut(&first)).len(), 1);
        registration.set_quiet(&mut keys, &first, &owner(), true);
        // The new shortcut was written while the field listened: nothing is told about the old.
        registration.sync(&mut keys, &second, &owner());
        assert!(registration.notices(first_shortcut(&second)).is_empty());
        // The field stops: the new key is asked, and refused too: told, with its own name.
        assert!(registration.set_quiet(&mut keys, &second, &owner(), false));
        let told = registration.notices(first_shortcut(&second));
        assert_eq!(told.len(), 1);
        assert!(told[0].contains("cmd+f2"), "{told:?}");
    }

    fn first_shortcut(settings: &Settings) -> Option<Shortcut> {
        settings.hotkey_window.shortcut
    }

    /// A double tap goes the same way: its monitors are let go while the field listens, and are made
    /// again after; taken away while the field listens, it is not built again.
    #[test]
    fn a_double_tap_comes_down_with_the_field_and_with_remove() {
        let twice = settings_with("option");
        let mut keys = fake::Keys::default();
        let mut registration = Registration::new();
        registration.sync(&mut keys, &twice, &owner());
        assert_eq!(registration.held(), Some(Binding::Twice(Modifier::Option)));
        registration.set_quiet(&mut keys, &twice, &owner(), true);
        assert_eq!(keys.held, None, "the tap is down while the field listens");
        registration.set_quiet(&mut keys, &twice, &owner(), false);
        assert_eq!(keys.held, Some(Binding::Twice(Modifier::Option)));
        // Remove, with the field not listening: the settings lose the shortcut and the tap goes.
        registration.sync(&mut keys, &Settings::default(), &owner());
        assert_eq!(keys.held, None);
        assert_eq!(registration.held(), None);
        // Remove, with the field listening: the tap is already down and stays down.
        registration.sync(&mut keys, &twice, &owner());
        registration.set_quiet(&mut keys, &twice, &owner(), true);
        registration.sync(&mut keys, &Settings::default(), &owner());
        registration.set_quiet(&mut keys, &Settings::default(), &owner(), false);
        assert_eq!(keys.held, None);
        assert_eq!(registration.held(), None);
    }

    /// An instance that is not the owner never touches the system's keys: not a timed run, nor the
    /// agent's copy, nor one without the session lock (whatever the settings say).
    #[test]
    fn an_instance_that_does_not_own_the_keys_registers_nothing_at_all() {
        let settings = settings_with("cmd+f1");
        for not in [
            Instance::default(),
            Instance {
                timed_run: true,
                ..owner()
            },
            Instance {
                agent_check: true,
                ..owner()
            },
            Instance {
                holds_session_lock: false,
                ..owner()
            },
        ] {
            let mut keys = fake::Keys::default();
            let mut registration = Registration::new();
            registration.sync(&mut keys, &settings, &not);
            assert!(keys.calls.is_empty(), "{not:?}");
            assert_eq!(registration.held(), None);
        }
    }

    /// The Settings row says a refusal to a person: the key in its glyphs and no code quotes.
    #[test]
    fn the_row_says_a_refusal_in_glyphs() {
        let twice = Shortcut::Twice(Modifier::Option);
        let refused = refusal_words(Refusal::Refused(-9878), Some(twice));
        assert!(
            refused.starts_with("The system did not give \u{2325} \u{2325} "),
            "{refused}"
        );
        assert!(refused.contains("(status -9878)"), "{refused}");
        assert!(!refused.contains('`'), "{refused}");
        assert!(!refused.contains("option"), "{refused}");
        // The file's own language is as it was: the key as the file writes it, in code quotes.
        let file = refusal_note(Refusal::Refused(-9878), Some(twice));
        assert!(file.contains("`option+option`"), "{file}");
        // No key to name: the sentence says "the shortcut".
        assert!(refusal_words(Refusal::Refused(1), None).contains("give the shortcut ("));
    }

    /// A refusal is told, with the key and the system's number, and goes when a later binding is
    /// held.
    #[test]
    fn a_key_the_system_refused_is_told_and_forgotten_when_another_is_held() {
        let (first, second) = (settings_with("cmd+f1"), settings_with("cmd+f2"));
        let mut keys = fake::Keys {
            refuse: Some(Refusal::Refused(-9878)),
            ..fake::Keys::default()
        };
        let mut registration = Registration::new();
        registration.sync(&mut keys, &first, &owner());
        assert_eq!(registration.problem(), Some(Refusal::Refused(-9878)));
        assert_eq!(registration.held(), None);
        let told = registration.notices(first.hotkey_window.shortcut);
        assert_eq!(told.len(), 1);
        assert!(told[0].contains("-9878") && told[0].contains("hotkey_window.shortcut"));
        keys.refuse = None;
        registration.sync(&mut keys, &second, &owner());
        assert_eq!(registration.problem(), None);
        assert!(
            registration
                .notices(second.hotkey_window.shortcut)
                .is_empty()
        );
    }

    /// The codes Carbon is given: the function keys are not in a row, and the modifiers are the
    /// system's own mask.
    #[test]
    fn carbon_is_given_the_codes_of_the_keyboard() {
        assert_eq!(named_key_code(NamedKey::F(1)), Some(122));
        assert_eq!(named_key_code(NamedKey::F(13)), Some(105));
        assert_eq!(named_key_code(NamedKey::F(17)), Some(64));
        assert_eq!(named_key_code(NamedKey::F(20)), Some(90));
        assert_eq!(named_key_code(NamedKey::F(0)), None);
        assert_eq!(named_key_code(NamedKey::F(21)), None);
        assert_eq!(named_key_code(NamedKey::Space), Some(49));
        let mods = Mods::NONE.with(Modifier::Cmd).with(Modifier::Option);
        assert_eq!(carbon_modifiers(mods), 0x0100 | 0x0800);
        assert_eq!(carbon_modifiers(Mods::NONE), 0);
        assert_eq!(crate::keys::us_code_of('a'), Some(0));
        assert_eq!(crate::keys::us_code_of(';'), Some(41));
    }

    fn twice_settings() -> Settings {
        settings_with("option")
    }

    /// A double tap is asked of the event monitors and a chord of the key registration: the setting
    /// decides which, and a change from one to the other lets the old one go first.
    #[test]
    fn a_single_modifier_asks_for_the_tap_and_a_chord_does_not() {
        let mut keys = fake::Keys::default();
        let mut registration = Registration::new();
        registration.sync(&mut keys, &twice_settings(), &owner());
        assert_eq!(keys.calls, [Some(Binding::Twice(Modifier::Option))]);
        registration.sync(&mut keys, &settings_with("cmd+f1"), &owner());
        assert_eq!(keys.calls.len(), 2);
        assert!(matches!(keys.calls[1], Some(Binding::Chord(_))));
        // Taking the shortcut away lets the tap go.
        registration.sync(&mut keys, &twice_settings(), &owner());
        registration.sync(&mut keys, &Settings::default(), &owner());
        assert_eq!(keys.calls.last(), Some(&None));
        assert_eq!(keys.held, None);
    }

    /// The count answers the second quick tap of the modifier waited for, and of no other.
    #[test]
    fn the_count_answers_the_second_tap_of_its_own_modifier_only() {
        use crate::shortcuts::{Held, Taps};
        let held = |modifier| Held {
            mods: Mods::NONE.with(modifier),
            function: false,
        };
        let none = Held::default();
        let count = |wanted| DoubleTap::new(wanted, Taps::new(0.3, 0.5));
        // No key, click or scroll ever: as old as can be.
        let tap = |count: &mut DoubleTap, key, at: f64| {
            let first = count.flags(at, key, f64::INFINITY);
            first | count.flags(at + 0.1, none, f64::INFINITY)
        };
        // Option twice, quickly: the second release is the hit.
        let mut option = count(Modifier::Option);
        assert!(!tap(&mut option, held(Modifier::Option), 1.0));
        assert!(tap(&mut option, held(Modifier::Option), 1.3));
        // The same count does not answer for shift's double tap, nor for a tap too late.
        let mut other = count(Modifier::Option);
        assert!(!tap(&mut other, held(Modifier::Shift), 1.0));
        assert!(!tap(&mut other, held(Modifier::Shift), 1.3));
        let mut late = count(Modifier::Option);
        assert!(!tap(&mut late, held(Modifier::Option), 1.0));
        assert!(!tap(&mut late, held(Modifier::Option), 3.0));
        // A key told as it happens spoils the count (⌥e is not half of ⌥⌥).
        let mut keyed = count(Modifier::Option);
        assert!(!tap(&mut keyed, held(Modifier::Option), 1.0));
        keyed.interrupt();
        assert!(!tap(&mut keyed, held(Modifier::Option), 1.3));
        // fn is never the shortcut's.
        let mut function = count(Modifier::Option);
        let fn_held = Held {
            mods: Mods::NONE,
            function: true,
        };
        assert!(!tap(&mut function, fn_held, 1.0));
        assert!(!tap(&mut function, fn_held, 1.3));
    }

    /// What the system tells of the keys, clicks and scrolls is its answer to "how long ago was
    /// the last one": a count is spoiled when one came after the press that began it, wherever it
    /// came — between the taps or during the second.
    #[test]
    fn a_key_a_click_or_a_scroll_after_the_first_press_spoils_the_count() {
        use crate::shortcuts::{Held, Taps};
        let option = Held {
            mods: Mods::NONE.with(Modifier::Option),
            function: false,
        };
        let none = Held::default();
        // The events of a double tap at 1.00-1.08 and 1.25-1.33, with `input` (a time, or none) as
        // the last key, click or scroll the system knew of at each: told as the age it had then.
        let run = |input: Option<f64>, at_each: &[(f64, bool)]| {
            let mut count = DoubleTap::new(Modifier::Option, Taps::new(0.3, 0.5));
            let mut hit = false;
            for &(at, down) in at_each {
                let age = match input {
                    Some(time) if time <= at => at - time,
                    _ => f64::INFINITY,
                };
                hit |= count.flags(at, if down { option } else { none }, age);
            }
            hit
        };
        let clean = [(1.00, true), (1.08, false), (1.25, true), (1.33, false)];
        assert!(run(None, &clean));
        // An input before the first press is not one that came after it (it may be by a
        // millisecond), nor one after the whole double tap.
        assert!(run(Some(0.999), &clean));
        assert!(run(Some(0.50), &clean));
        assert!(run(Some(1.34), &clean), "after the second release");
        // Between the press and its release (⌥ held, a key typed: ⌥e), between the taps, during
        // the second tap: the count is spoiled whichever of them is the last the system knows.
        for time in [1.04, 1.15, 1.30] {
            assert!(!run(Some(time), &clean), "{time}");
        }
        // The input at the moment of the press is the press's own company.
        assert!(run(Some(1.00), &clean));
        // A count that began after the input is a count of its own: typing, then ⌥⌥ at once.
        let after = [(2.00, true), (2.08, false), (2.25, true), (2.33, false)];
        assert!(run(Some(1.50), &after));
        // The key between two double taps spoils the first one and not the next: the second
        // tap of the first pair is already a first tap of the next count.
        let two = [(1.00, true), (1.08, false), (1.25, true), (1.33, false)];
        assert!(!run(Some(1.20), &two));
    }

    /// The rule on its own: the input is the later one when it is the nearer.
    #[test]
    fn an_input_nearer_than_the_press_is_the_later_one() {
        assert!(spoils_count(300, 200));
        assert!(spoils_count(300, 0));
        assert!(!spoils_count(300, 300), "the very moment of the press");
        assert!(!spoils_count(300, 301));
        assert!(!spoils_count(300, u64::MAX), "no input ever");
        assert!(!spoils_count(0, 0));
    }

    /// The system is asked a moment after the event: what came in that moment is no input before
    /// the event, and what came before is as old as it was then.
    #[test]
    fn the_age_of_an_input_is_told_as_of_the_event() {
        assert_eq!(input_age_at_event(&[0.50], 0.0), 0.50);
        assert!((input_age_at_event(&[0.50], 0.125) - 0.375).abs() < 1e-9);
        // The last input came after the event (age less than the lag): none before it.
        assert!(input_age_at_event(&[0.05], 0.10).is_infinite());
        // A negative lag (a clock that went back a hair) is none.
        assert_eq!(input_age_at_event(&[0.50], -0.01), 0.50);
        // Each kind is told by itself: a click that came after the event (0.05 s ago, the event
        // 0.10 s ago) does not hide the key that came before it (0.30 s ago); the youngest of
        // what came before is the answer.
        let both = input_age_at_event(&[0.30, 0.05, f64::INFINITY], 0.10);
        assert!((both - 0.20).abs() < 1e-9, "{both}");
        assert!((input_age_at_event(&[0.40, 0.30], 0.10) - 0.20).abs() < 1e-9);
        assert!(input_age_at_event(&[], 0.0).is_infinite());
        assert_eq!(millis(0.0), 0);
        assert_eq!(millis(-1.0), 0);
        assert_eq!(millis(f64::NAN), 0);
        assert_eq!(millis(0.0004), 0);
        assert_eq!(millis(0.0016), 2);
        assert_eq!(millis(f64::INFINITY), u64::MAX);
    }

    #[test]
    fn the_stand_in_keys_register_nothing_and_remember_what_they_were_asked() {
        let mut keys = fake::Keys::default();
        let binding = Binding::Twice(Modifier::Option);
        assert_eq!(keys.bind(Some(binding)), Ok(()));
        assert_eq!(keys.held, Some(binding));
        // A refusal holds nothing and says why; letting go is never refused.
        keys.refuse = Some(Refusal::Refused(-9878));
        assert_eq!(keys.bind(Some(binding)), Err(Refusal::Refused(-9878)));
        assert_eq!(keys.held, None);
        assert_eq!(keys.bind(None), Ok(()));
        assert_eq!(keys.calls, [Some(binding), Some(binding), None]);
    }
}
