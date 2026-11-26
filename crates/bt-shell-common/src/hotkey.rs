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
//! `RegisterEventHotKey` and a listen-only event tap (`bt-shell-macos`), and no test, gate, smoke
//! or agent try registers either — a real registration steals the keystroke from the Mac that
//! runs the gate and a tap raises a permission question. A test's is [`fake::Keys`]. Only an
//! instance that [`owns_global_keys`] builds the real one.

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
/// the same way). The edges are half open, so a pointer between two screens is on exactly one.
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
        x >= frame.x && x < frame.x + frame.width && y >= frame.y && y < frame.y + frame.height
    };
    Some(
        screens
            .iter()
            .position(|screen| within(&screen.frame))
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
    Slide { away, shown }
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
}

/// Whether `lost` hides the window. A pin, or `hide_on_click_away = false`, hides on nothing.
pub fn hides_on(lost: Lost, hiding: Hiding) -> bool {
    if hiding.pinned || !hiding.hide_on_click_away {
        return false;
    }
    match lost {
        Lost::Active => true,
        Lost::Key { to } => to == KeyTaker::OtherTerminal,
    }
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
    /// One modifier tapped twice (a listen-only event tap): asks for Input Monitoring.
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
    /// A double tap is listened to through the event tap, and the permission it needs is not
    /// given ([`Access::NotGranted`]), or the tap could not be made.
    NoListening,
}

/// The state of the permission a double tap needs (Input Monitoring).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Granted,
    NotGranted,
}

/// The seam in front of the system's global keys: the one place that registers a key for every
/// application and the one that listens for a modifier tapped twice.
pub trait GlobalKeys {
    /// Holds `binding` in place of whatever was held before; `None` lets go of it. Said again
    /// with the same binding it is held once.
    fn bind(&mut self, binding: Option<Binding>) -> Result<(), Refusal>;

    /// Whether the permission a double tap needs is given. Asks the system and does **not** ask
    /// the person: a launch that asked would put a question on every run of every build.
    fn listen_access(&self) -> Access;

    /// Asks the person for the permission (once; the system remembers the answer). Only the
    /// Settings page calls it, when a double tap is recorded or the button is pressed.
    fn ask_listen_access(&mut self);
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

/// The stand-in for the system's keys that tests use: it keeps what it was asked to hold and
/// holds nothing.
#[cfg(any(test, feature = "test-support"))]
pub mod fake {
    use super::{Access, Binding, GlobalKeys, Refusal};

    /// A [`GlobalKeys`] that registers nothing anywhere.
    #[derive(Debug)]
    pub struct Keys {
        /// What it holds now.
        pub held: Option<Binding>,
        /// Every call of [`GlobalKeys::bind`], in order.
        pub calls: Vec<Option<Binding>>,
        /// What the next [`GlobalKeys::bind`] answers (kept until changed).
        pub refuse: Option<Refusal>,
        /// What the permission is.
        pub access: Access,
        /// How many times the person was asked.
        pub asked: usize,
    }

    impl Default for Keys {
        fn default() -> Self {
            Self {
                held: None,
                calls: Vec::new(),
                refuse: None,
                access: Access::Granted,
                asked: 0,
            }
        }
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

        fn listen_access(&self) -> Access {
            self.access
        }

        fn ask_listen_access(&mut self) {
            self.asked += 1;
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
        assert_eq!(keys.listen_access(), Access::Granted);
        keys.ask_listen_access();
        assert_eq!(keys.asked, 1);
    }
}
