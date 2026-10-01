//! The settings model: the pure path from `settings.toml`'s text to
//! [`Settings`].
//!
//! It **doesn't see** the file system: `bt-shell` reads the text, watches it
//! and shows the error in the window. Only the decision is here — the "pure
//! decision + thin system wrapper" pattern of `child.rs` — and this is the
//! defaults' only owner. The decision's record is
//! `.tasks/007-ayarlar-ve-tema/discussion.md` → Karar 1.
//!
//! **There is a single error rule:** if the text can't be parsed as TOML the
//! result is a separate value (the `Err` of [`Settings::parse`]) and no field is
//! invented — what to do is the caller's decision (defaults at startup, nothing
//! on a live reload). If it parses, every key takes either its valid value or
//! its default; a value that isn't accepted leaves a [`Diagnostic`].
//! **The one exception is `clipboard.osc52`:** for a value that isn't accepted it
//! takes not the default (on) but off (the doc of [`Settings::parse_keeping`]).
//! **An unknown key and section are silently ignored:** a later set's key
//! (`[motion] intensity`) mustn't produce a diagnostic in today's version.
//!
//! The parser sees the previous settings only as the value that stands in for a
//! value that isn't accepted ([`Settings::parse_keeping`], save time); taking the
//! difference is the caller's job ([`Settings::changes`]).
//!
//! The diagnostic type and the TOML helpers are shared with the theme file's
//! parser (`theme`): so that the two files speak the same error language.

use std::fmt;

use toml_edit::{Document, Item, TableLike};

use crate::session::{Osc52, TerminalOptions};

/// The scrollback's ceiling: the limit of **the alacritty application**.
///
/// The source is `alacritty/src/config/scrolling.rs` → `MAX_SCROLLBACK_LINES =
/// 100_000`; it rejects a value above it when reading the config. The limit is
/// in the application, **not** in `alacritty_terminal` — `Term` takes
/// `scrolling_history` without clamping, so setting the ceiling is our job. It
/// can't be imported (that crate isn't our dependency), the number was copied
/// here with its source; it is not a measured memory budget.
///
/// The ceiling is a rule for user input, not an invariant of `Session` — there
/// is no other gate that clamps `SessionOptions.scrollback` and none is needed,
/// the only value going there passes through this parser. `pub`, because the
/// settings window's field asks for the same ceiling: a second copy could have
/// made the window write a number the parser rejects (029).
pub const SCROLLBACK_MAX: usize = 100_000;

/// The reserved value of `[appearance] theme`: the system's light/dark
/// appearance picks the theme ([`Settings::theme_for`]).
///
/// **Not** a theme name — so `themes/system.toml` can't be selected and
/// `light_theme`/`dark_theme` don't accept this value (it would be a choice
/// that loops back on itself).
///
/// `pub`: View ▸ Theme ▸ Match System writes this value
/// ([`Settings::with_theme`]); the reading side looks at
/// [`Settings::follows_system`], it doesn't compare the value.
pub const SYSTEM_THEME: &str = "system";

/// `[font]`: the two values that determine the cell size and the glyphs.
///
/// A separate type, because the renderer holds it **as a whole** and takes its
/// startup value from here: the default point size's only owner is this type's
/// `Default`. Had it been the renderer's own constant, a timed run's font (the
/// settings are never read) and a file-less user's would be tied to two separate
/// numbers.
///
/// No `Eq`: the point size is `f64`. The parser leaves only finite and positive
/// values, so NaN doesn't enter the comparison.
#[derive(Clone, Debug, PartialEq)]
pub struct FontOptions {
    /// Family name; `None` → the chain (SF Mono, else Menlo). Whether it exists
    /// on the machine is `bt-atlas`'s question — only text here.
    pub family: Option<String>,
    /// Logical point size. Clamping its scale-multiplied form is in `bt-atlas`
    /// and is **silent**; the rule here is only "finite and greater than zero".
    pub size: f64,
    /// Line-height multiplier: the cell becomes this multiple of the font's own
    /// `ascent + descent + leading`, and the excess is distributed **equally
    /// below and above** the glyph (the baseline drops by that much too).
    ///
    /// The floor is `1.0` and it **isn't gone below**: a cell shorter than the
    /// font wants would clip the coverage under `g j p q y` and that has its own
    /// guard (`descender_fits_in_the_cell` in `bt-atlas`). A setting punching
    /// through a guard matters more than the setting itself.
    pub line_height: f64,
}

impl Default for FontOptions {
    /// 13 points was `bt-gpu`'s `POINT_SIZE` constant until 006: a chosen
    /// default, not a measured number.
    fn default() -> Self {
        Self {
            family: None,
            size: 13.0,
            line_height: 1.0,
        }
    }
}

/// `[motion] cursor_motion`: how the cursor travels between cells.
///
/// It lives in the settings model (the precedent of [`FontOptions`] and `Osc52`)
/// but its consumer is `bt-gpu`: `bt-shell` hands the resolved value to the
/// renderer's rhythm. The only information here is **which style**; the owner of
/// the durations and spring coefficients is `bt_gpu::motion` — keeping the
/// numbers in the settings model would make them changeable from two places.
///
/// `Default` is **`Spring`** (008 Karar 6): the set's product rationale is "one
/// of the three things that introduce the reference on screen" and making the
/// default `Snap` would have meant shipping the feature off. Because the default
/// has a single owner here, the hermetic timed run (it doesn't read a settings
/// file) gets this value too — `make smoke`'s `motion > 0` requirement leans
/// exactly on this.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorMotion {
    /// Instant: the cursor is born in the target cell, the animation never
    /// starts.
    Snap,
    /// Fixed duration, no overshoot — independent of distance.
    Ease,
    /// Critically damped spring; the duration grows with distance.
    #[default]
    Spring,
}

impl CursorMotion {
    /// The **single list** of spellings in the settings file (the rationale of
    /// [`UnfocusedCaret::NAMES`]).
    pub const NAMES: &'static [(&'static str, Self)] = &[
        ("snap", Self::Snap),
        ("ease", Self::Ease),
        ("spring", Self::Spring),
    ];

    /// The spelling in the settings file.
    pub fn name(self) -> &'static str {
        name_in(Self::NAMES, self)
    }
}

/// `[terminal] cursor_blink`: whether the cursor blinks.
///
/// **Three-valued, because there are two questions:** should the application's
/// request be heeded ([`Self::Auto`]) or should what the user said override
/// everything ([`Self::On`]/[`Self::Off`]). Had it been two-valued, whether
/// `false` means "the application can't turn it off either" or "default off"
/// would have stayed ambiguous — and a zsh setup that sends `\e[5 q` in vi mode
/// would have lit the cursor up while the user had written off.
///
/// **The default is [`Self::Off`]** and that is a product decision: a blinking
/// cursor makes the window **permanently non-idle** (two frames per second) and
/// this repo defended "zero frames when idle" across thirteen sets. Its cost
/// should be something the user **chose**, not a default that arrives silently.
/// The hermetic timed run (it doesn't read a settings file) gets this value too,
/// so `make smoke`'s quiet gate is structurally immune.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorBlink {
    /// What the application says: DECSCUSR's odd numbers and DECSET 12 turn it on.
    Auto,
    /// Always blinks; even the application's `\e[2 q` can't stop it.
    On,
    /// Never blinks; even the application's `\e[5 q` can't start it.
    #[default]
    Off,
}

impl CursorBlink {
    /// The single list of spellings in the settings file.
    pub const NAMES: &'static [(&'static str, Self)] =
        &[("auto", Self::Auto), ("on", Self::On), ("off", Self::Off)];

    /// The spelling in the settings file.
    pub fn name(self) -> &'static str {
        name_in(Self::NAMES, self)
    }

    /// Combines what the application says with what the user said — the **single
    /// place**, `Session::frame` asks it from here.
    pub(crate) fn resolve(self, requested: bool) -> bool {
        match self {
            Self::Auto => requested,
            Self::On => true,
            Self::Off => false,
        }
    }
}

/// `[terminal] cursor`: the cursor's **shape** — DECSCUSR's three forms.
///
/// The section is **not** `[motion]`: the shape isn't motion, it's a part of the
/// terminal's state machine. It sits in `[terminal]` and in the same section the
/// value lands in `Session` with [`TerminalOptions`], where it becomes
/// alacritty's `default_cursor_style`.
///
/// **That mirror is descriptive, not prescriptive** (016): `[clipboard] osc52`
/// enters `TerminalOptions` too, while `[terminal] cursor_radius` doesn't. The
/// section names **what the user is setting**, not which struct carries it. The
/// reference keeps the key in its own `[typography]` (`docs/ARASTIRMA.md` →
/// İmleç); we took the name, not the place.
///
/// **This setting only says the default.** An application can change the shape
/// with DECSCUSR (`\e[5 q`) or OSC 50 and that word is heeded: if vim wants a bar
/// in insert mode it becomes a bar. What the user writes here is the state when
/// nobody asked for anything.
///
/// `Hidden` and `HollowBlock` are **not represented**: the first isn't a shape
/// but visibility (`\e[?25l`) and `Cursor::visible` already carries it; the
/// second is the state of lost focus and focus doesn't cross the boundary today
/// (`.tasks/014-imlec-stilleri/plan.md` → Kapsam Dışı).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CaretShape {
    /// A block filling the cell — alacritty's default too.
    #[default]
    Block,
    /// A thin line under the cell.
    Underline,
    /// A thin vertical bar at the cell's left.
    Beam,
}

/// The cursor's **corner radius** default, a ratio of the cell height.
///
/// **This is the defaults' only owner** and that is a plumbing decision:
/// `bt-gpu` **imports** the same constant (`Frame::default()` and the pixel
/// guards). Had there been two literals the guards would test their own
/// consistency and pass green even if the shipped cursor had a different size —
/// the recipe for a silent breakage (`/plan-review`, 016). The precedent is
/// [`FontOptions`]'s doc: *"had it been the renderer's own constant, a timed
/// run's font and a file-less user's would be tied to two separate numbers"*.
///
/// The value is **chosen, not measured** and went down in two rounds of eyeballing
/// in 015 (0.18 → 0.10): the block caret is shorter than the cell width and a
/// larger radius turned it from a rectangle into a pill.
pub const CURSOR_RADIUS: f64 = 0.10;

/// The cursor's **glow strength** default; `1.0` = the design's own measure.
///
/// **One number, not two** (`/plan-review`, 016): the halo margin and its alpha
/// went down in the same two eyeballing rounds in 015 **in the same direction**
/// (margin 1.0 → 0.5 → 0.4, alpha 0.35 → 0.14 → 0.10), i.e. the user moved along
/// a single feel, not two axes. Separate keys would also produce meaningless
/// states: `margin = 2, alpha = 0` means a quad that paints the halo of nothing.
///
/// The two constants in `bt-gpu` **stay in place as the base**; this is only
/// their multiplier, so the "no second design constant" rule is kept.
pub const CURSOR_GLOW: f64 = 1.0;

/// The blink's **half period** default, in seconds.
///
/// **The default's only owner is here** (the same reasoning as
/// [`CURSOR_RADIUS`]): `bt-gpu` imports this. The value is **chosen, not
/// measured** — its goal is "noticeable that it blinks but not tiring to the
/// eye" and its cost is linear: 250 ms makes four frames per second.
pub const CURSOR_BLINK_INTERVAL: f64 = 0.5;

/// The accepted range of the blink period, in seconds — **chosen, not measured**.
///
/// The lower end stops the ceiling: a 50 ms half period makes 20 frames per
/// second and going below it would turn the terminal into a strobe. **The gate
/// can't see this** and let that be written down: the timed run never reads the
/// settings file and the blink default is off too, so a broken period will
/// **under no condition** turn `make smoke`'s `quiet=` tier red (014
/// `teslim.md`: "the protection is not a token but the default itself"). The
/// only protection is this range.
pub const CURSOR_BLINK_RANGE: std::ops::RangeInclusive<f64> = 0.05..=5.0;

/// The accepted range of the radius; half = half the cell, beyond is
/// meaningless.
///
/// The ranges are `pub`: the settings window's controls are built with these
/// ends too, so what the parser accepts and what the window offers come from a
/// single place (029).
pub const CURSOR_RADIUS_RANGE: std::ops::RangeInclusive<f64> = 0.0..=0.5;

/// The accepted range of the glow multiplier — **chosen, not measured**.
///
/// The multiplier scales **both axes at once** and the ceiling's rationale must
/// count both (`/code-review`): at 3.0 the alpha is 0.30 (below the 0.35
/// rejected in 015) but the spread is `1.2 × gutter_px`, i.e. **above** the
/// "the whole left margin" that `CARET_GLOW_RATIO`'s doc records as "neon, not a
/// glow".
///
/// The ceiling is still there, because **the default's taste and the ceiling's
/// job are separate**: what was rejected was that image being the *default*. The
/// ceiling leaves room for the end the user picks explicitly and its only job is
/// to cut off unboundedness.
pub const CURSOR_GLOW_RANGE: std::ops::RangeInclusive<f64> = 0.0..=3.0;

/// `[terminal] cursor_unfocused`: what the cursor is in an unfocused window.
///
/// 015 hollows out the cursor when focus is lost; this key turns that off. **It
/// doesn't touch blink** — blink stopping when unfocused is a separate signal
/// and a separate decision (015 R7.4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UnfocusedCaret {
    /// Hollows out: the frame stays, the fill goes. Today's behavior.
    #[default]
    Hollow,
    /// Never changes; the only sign of being unfocused is blink stopping.
    Solid,
}

impl UnfocusedCaret {
    /// The **single list** of spellings in the settings file: the parser,
    /// [`Self::name`] and the settings window's options all read from here.
    ///
    /// Had it been written in two places, changing one variant's spelling would
    /// produce a diagnostic suggesting the user a value **the parser rejects**.
    /// The order is the diagnostic text's order (`"hollow" or "solid"`).
    pub const NAMES: &'static [(&'static str, Self)] =
        &[("hollow", Self::Hollow), ("solid", Self::Solid)];

    /// The spelling in the settings file.
    pub fn name(self) -> &'static str {
        name_in(Self::NAMES, self)
    }
}

/// `[terminal] confirm_close`: when to ask as a window, tab or the application
/// closes (`.tasks/028-kapatma-onayi/discussion.md` → Karar 6).
///
/// "Running" means a program in the foreground **outside** the shell (vim,
/// `ssh`, Claude Code); a background job and the shell's own loop aren't
/// counted. Detection is in `bt-shell`, from the process table — only the user's
/// choice here.
///
/// It **doesn't enter** `TerminalOptions` and [`Changes`] (precedent
/// [`CaretStyle`]): the value is read from the current settings at close time,
/// so being in effect at save time is free and no path to the sessions is needed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConfirmClose {
    /// Never ask.
    Never,
    /// Ask if a program outside the shell is running in the foreground.
    #[default]
    Running,
    /// Ask even when the shell is idle.
    Always,
}

impl ConfirmClose {
    /// The single list of spellings in the settings file (the same rationale as
    /// [`UnfocusedCaret::NAMES`]).
    pub const NAMES: &'static [(&'static str, Self)] = &[
        ("never", Self::Never),
        ("running", Self::Running),
        ("always", Self::Always),
    ];

    /// The spelling in the settings file.
    pub fn name(self) -> &'static str {
        name_in(Self::NAMES, self)
    }
}

/// `[terminal] cursor_radius` and `cursor_glow`: the cursor's **drawing**
/// numbers.
///
/// They **don't enter** `TerminalOptions` and `Session` doesn't see them: both
/// are pure painting, not the terminal's state. The path is the precedent of
/// `cursor_motion` — `Settings::changes` finds the difference, `bt-shell` passes
/// it to `bt_gpu::DisplayLink`, it is applied at save time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaretStyle {
    /// Corner radius, a ratio of the cell **height**.
    ///
    /// `f64`, not `f32`, and the reason is the **diagnostic text**: `ranged_float`
    /// prints the fallen-back value into the message and `f64::from(0.10f32)`
    /// comes to `0.10000000149011612` — the user would see float noise rather
    /// than the number they wrote (`/code-review`). The narrowing is at the
    /// `bt-gpu` boundary, once rather than per frame.
    pub radius_ratio: f64,
    /// The glow's strength; `0.0` is off, `1.0` is the design's own measure.
    pub glow: f64,
    /// The cursor's state in an unfocused window.
    pub unfocused: UnfocusedCaret,
}

impl Default for CaretStyle {
    fn default() -> Self {
        Self {
            radius_ratio: CURSOR_RADIUS,
            glow: CURSOR_GLOW,
            unfocused: UnfocusedCaret::default(),
        }
    }
}

impl CaretShape {
    /// The single list of spellings in the settings file.
    pub const NAMES: &'static [(&'static str, Self)] = &[
        ("block", Self::Block),
        ("underline", Self::Underline),
        ("beam", Self::Beam),
    ];

    /// The spelling in the settings file.
    pub fn name(self) -> &'static str {
        name_in(Self::NAMES, self)
    }
}

/// `[motion] reduce_motion`: whether animations are reduced.
///
/// **Not a `bool`** and the reason is this file's own rule: the most likely
/// choice is "follow the system" and in a `bool` the only way to express it
/// would be to **delete** the key — keys aren't deleted here, even an unknown key
/// is preserved. A three-valued string keeps all three written down.
///
/// Reading the system's answer is `bt-shell`'s job (`NSWorkspace`); the only
/// information here is **which one** the user wants. The place where the three
/// collapse to a single `bool` is also there, because `bt-gpu` doesn't see
/// AppKit (`CLAUDE.md` → the layer table).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReduceMotion {
    /// Follow macOS's Reduce Motion setting.
    #[default]
    System,
    /// Reduce whatever the system says.
    On,
    /// Don't reduce whatever the system says.
    Off,
}

impl ReduceMotion {
    /// The single list of spellings in the settings file.
    pub const NAMES: &'static [(&'static str, Self)] = &[
        ("system", Self::System),
        ("on", Self::On),
        ("off", Self::Off),
    ];

    /// The spelling in the settings file.
    pub fn name(self) -> &'static str {
        name_in(Self::NAMES, self)
    }
}

/// `[motion] smooth_scroll`: whether scrolling in scrollback is smooth or by
/// line steps.
///
/// **Not a `bool`**, the file's string-enum convention ([`ReduceMotion`],
/// `Osc52`): the value is the meaning of the reference's key (`scroll.smooth`),
/// the type is ours.
///
/// Its consumer is `bt-shell` (the precedent of [`CursorMotion`]) and there it
/// collapses **to a single `bool`** with Reduce Motion and `cursor_motion =
/// "snap"`: if one of the three turns motion off, the wheel goes by today's line
/// step (`.tasks/027-yumusak-kaydirma/discussion.md` → Karar 5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SmoothScroll {
    /// The trackpad follows the finger, the notch glides, at the gesture's end it
    /// settles on a line.
    #[default]
    On,
    /// Line step — the very behavior before `"on"`.
    Off,
}

impl SmoothScroll {
    /// The single list of spellings in the settings file.
    pub const NAMES: &'static [(&'static str, Self)] = &[("on", Self::On), ("off", Self::Off)];

    /// The spelling in the settings file.
    pub fn name(self) -> &'static str {
        name_in(Self::NAMES, self)
    }
}

/// `[motion] keypress`: how a glyph typed in the dock arrives (030).
///
/// **Only drawable names** ([`Self::NAMES`]) and today the reference list in
/// full: names go in as they become drawable — had a name that isn't drawn been
/// accepted in the popup or the file, selecting it would do nothing
/// (`.tasks/030-dock-yazim-animasyonlari/discussion.md` → Karar 7). The
/// definition of the looks is Karar 6's table; the amplitudes are in `bt-gpu`'s
/// `shaders/glyph_fx.metal`.
///
/// Its consumer is `bt-gpu` (the precedent of [`CursorMotion`]) and the value
/// goes **raw**: the reduction by `cursor_motion = "snap"` and Reduce Motion is
/// there, in the same place as the cursor's mode. The owner of the durations and
/// the curve is there too.
///
/// The default is **[`Self::Fade`]**: the user explicitly asked for animation
/// and should see it out of the box; the effect that displaces least in the list
/// — the glyph doesn't move from its place, it only appears.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Keypress {
    /// The glyph appears instantly.
    Off,
    /// The glyph fades in place from transparent to full color.
    #[default]
    Fade,
    /// The glyph slides up from slightly below the cell and settles into place,
    /// appearing as it slides.
    Rise,
    /// The glyph is born small, for a moment grows a bit past its size and
    /// settles into place.
    Pop,
    /// The glyph emerges extending from its left edge to the right.
    Extrude,
    /// The glyph is born in the theme's `cursor` color and cools to its own color.
    Heat,
    /// The glyph appears in place and a faint copy of it, growing and fading,
    /// disperses over it.
    Echo,
    /// The glyph drops from above the cell, bounces slightly and settles into
    /// place.
    Drop,
    /// First the strokes' core is visible, then the ink spreads to the edges.
    Ink,
    /// The glyph is born squeezed horizontally and stretched vertically, and
    /// opens to its own proportions.
    Squeeze,
}

impl Keypress {
    /// The single list of spellings in the settings file.
    pub const NAMES: &'static [(&'static str, Self)] = &[
        ("off", Self::Off),
        ("fade", Self::Fade),
        ("rise", Self::Rise),
        ("pop", Self::Pop),
        ("extrude", Self::Extrude),
        ("heat", Self::Heat),
        ("echo", Self::Echo),
        ("drop", Self::Drop),
        ("ink", Self::Ink),
        ("squeeze", Self::Squeeze),
    ];

    /// The spelling in the settings file.
    pub fn name(self) -> &'static str {
        name_in(Self::NAMES, self)
    }
}

/// `[motion] erase`: how a glyph erased in the dock goes away (030).
///
/// [`Keypress`]'s sibling, with the same rules: only drawable names, the raw
/// value to `bt-gpu`. The default is **[`Self::Recede`]** — the departure that
/// displaces least, the glyph shrinks in place and fades.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Erase {
    /// The glyph disappears instantly.
    Off,
    /// A circular diaphragm over the glyph closes toward the center.
    Iris,
    /// The glyph is pulled down and toward the caret and fades.
    Undertow,
    /// The glyph grows, disperses outward like a ring and fades.
    Echo,
    /// The glyph's ink disperses: it fades as the edges spread and thin out.
    Bleed,
    /// The glyph splits into horizontal strips, the strips slide sideways in turn
    /// and dissolve.
    Unravel,
    /// The glyph shrinks toward its center, recedes and fades.
    #[default]
    Recede,
    /// The glyph drifts upward as if evaporating, spreading out and fading.
    Sublime,
    /// The glyph breaks into pieces, the pieces rotate slightly, scatter and fall,
    /// and fade.
    Shatter,
}

impl Erase {
    /// The single list of spellings in the settings file.
    pub const NAMES: &'static [(&'static str, Self)] = &[
        ("off", Self::Off),
        ("iris", Self::Iris),
        ("undertow", Self::Undertow),
        ("echo", Self::Echo),
        ("bleed", Self::Bleed),
        ("unravel", Self::Unravel),
        ("recede", Self::Recede),
        ("sublime", Self::Sublime),
        ("shatter", Self::Shatter),
    ];

    /// The spelling in the settings file.
    pub fn name(self) -> &'static str {
        name_in(Self::NAMES, self)
    }
}

/// `[shell] integration`: whether our wrapper is installed into the shell.
///
/// The key's meaning is narrow and deliberately so: **"don't install the
/// wrapper"**. Parsing the marks stays free in every case — seeing a real OSC 133
/// emitted by another tool (or an installation on the far side of SSH) is a gain,
/// not a harm, and that isn't the reason for turning it off either.
///
/// **The only setting not applied at save time** and this is the first exception
/// to the "settings apply at save time" contract: the wrapper is installed at the
/// shell's **birth**, by the time the file is saved the shell is already born.
/// That's why no arm is attached to [`Changes`] and `docs/AYARLAR.md` says in
/// the key's own line that it takes effect **in the next session** (009 Karar 5).
///
/// Its consumer is `bt-shell` (the precedent of [`CursorMotion`]): it gives the
/// decision, because it is the side that sees which shell is running and where
/// the script is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShellIntegration {
    /// If it is a shell we recognize the wrapper is installed; otherwise nothing
    /// happens.
    ///
    /// In a shell that can mirror (zsh today) the dock opens too and the prompt
    /// becomes the terminal's.
    #[default]
    Auto,
    /// The wrapper is installed but the **input line is not taken**: command
    /// blocks and marks work, the dock doesn't open, the prompt is the user's.
    ///
    /// **Not an invented tier, the structure itself.** The dock is tied to ZLE's
    /// mirror; when the bash (`--rcfile`) and fish (`vendor_conf.d`) scripts are
    /// born, those shells will have marks but no dock. This value only gives the
    /// zsh user the right to **choose** the same state.
    ///
    /// It replaced `[shell] prompt` in 012 phase-10: a separate key produced
    /// **two prompts** on screen (the user's in the grid, the dock's below) and
    /// the caret jumped between the two. Saying "the prompt is the user's"
    /// already means "the line is in the grid".
    Blocks,
    /// The wrapper is never installed.
    Off,
}

impl ShellIntegration {
    /// The single list of spellings in the settings file.
    pub const NAMES: &'static [(&'static str, Self)] = &[
        ("auto", Self::Auto),
        ("blocks", Self::Blocks),
        ("off", Self::Off),
    ];

    /// The spelling in the settings file.
    pub fn name(self) -> &'static str {
        name_in(Self::NAMES, self)
    }

    /// Whether the wrapper will be installed — yes for `auto` and `blocks`.
    pub fn installs_wrapper(self) -> bool {
        !matches!(self, Self::Off)
    }

    /// Whether the dock will open. Whether the shell can mirror is a **separate**
    /// question and `bt-shell` answers it; this is only the user's choice.
    pub fn wants_dock(self) -> bool {
        matches!(self, Self::Auto)
    }
}

/// A remote host's mark (037 Karar 2, 3): meaning, not color — its color comes
/// from the theme's role ([`crate::Theme::mark_linear`]), so the light/dark
/// switch carries the mark along by itself.
///
/// `None` means "unmarked" (the remote session's `info` today) and **ends** the
/// match in the pattern list ([`host_mark`]): it is the only way to leave a
/// single host caught by a glob unmarked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HostMark {
    /// The theme's `error`.
    Production,
    /// The theme's `warning`.
    Staging,
    /// The theme's `success`.
    Development,
    /// Unmarked: the theme's `info`.
    #[default]
    None,
    /// A direct color, `0xRRGGBB` (`"#rrggbb"`). It doesn't change with the theme
    /// and nobody checks that it will be legible on the light theme — the cost is
    /// named in Karar 2; the menu never writes it.
    Rgb(u32),
}

impl HostMark {
    /// The spellings of the named marks in the settings file; `Rgb` isn't a name,
    /// it is the `"#rrggbb"` format.
    pub const NAMES: &'static [(&'static str, Self)] = &[
        ("production", Self::Production),
        ("staging", Self::Staging),
        ("development", Self::Development),
        ("none", Self::None),
    ];

    /// The spelling in the settings file: the named mark's name ([`Self::NAMES`]),
    /// the direct color's `"#rrggbb"` — the inverse of what the parser reads.
    pub fn written(self) -> String {
        match self {
            Self::Rgb(hex) => format!("#{hex:06x}"),
            named => Self::NAMES
                .iter()
                .find(|(_, mark)| *mark == named)
                .map_or_else(String::new, |(name, _)| (*name).to_owned()),
        }
    }
}

/// One entry of the `[remote] hosts` array: a pattern and its mark (037 Karar 2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostRule {
    /// `*` (any string, empty included) and `?` (a single character), case
    /// insensitive; if it carries `@` it matches the whole host, if not the part
    /// after the last `@` ([`host_mark`]).
    pub pattern: String,
    pub mark: HostMark,
}

/// The host's mark: that of `rules`'s **first** matching entry, or
/// [`HostMark::None`] if none matches (037 Karar 2).
///
/// The input is the host the remote session shows (036: as the user typed it,
/// scheme and port dropped). If the pattern has no `@`, the part of the input
/// after the last `@` is matched — `deploy@prod` and `prod` are the same machine;
/// if the pattern has `@`, the whole input, so `root@*` can be written. The
/// order is in the array, because a TOML table's keys are semantically
/// unordered. A `None` entry ends the match there and the result is `None` too.
///
/// Pure and not on the frame path: `Session` calls it only at the two edges
/// where the remote state and the list change.
pub fn host_mark(rules: &[HostRule], host: &str) -> HostMark {
    let bare = bare_host(host);
    rules
        .iter()
        .find(|rule| {
            let subject = if rule.pattern.contains('@') {
                host
            } else {
                bare
            };
            glob_matches(&rule.pattern, subject)
        })
        .map_or(HostMark::None, |rule| rule.mark)
}

/// A pattern with `*` and `?`, case insensitive; no class (`[a-z]`) or set
/// (`{a,b}`) — those would mean a separate glob library (Karar 2).
///
/// Both sides are lowered to lowercase **once** and the comparison is over
/// character sequences: case folding can expand one character into several
/// (`İ`), so folding character by character would shift what `?` counts.
/// Backtracking goes only to the last `*` — the classic linear matcher.
fn glob_matches(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.to_lowercase().chars().collect();
    let text: Vec<char> = text.to_lowercase().chars().collect();
    let (mut p, mut t) = (0, 0);
    // The last `*`'s position in the pattern and how far into the text it has
    // swallowed at that moment.
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        match pattern.get(p) {
            Some('*') => {
                star = Some((p, t));
                p += 1;
            }
            Some(&c) if c == '?' || c == text[t] => {
                p += 1;
                t += 1;
            }
            _ => match star {
                Some((at, swallowed)) => {
                    p = at + 1;
                    t = swallowed + 1;
                    star = Some((at, swallowed + 1));
                }
                None => return false,
            },
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

/// `[remote] preview_keep`: how long a remote file's preview copy stays in the
/// preview folder after it was last opened (045 Karar 9). Checked at launch and
/// once a day; nothing is removed at quit, so [`Self::UntilLaunch`] removes this
/// session's previews at the **next** launch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PreviewKeep {
    /// Until bateri starts again.
    UntilLaunch,
    Day,
    #[default]
    Week,
    Month,
}

impl PreviewKeep {
    /// The single list of spellings in the settings file.
    pub const NAMES: &'static [(&'static str, Self)] = &[
        ("launch", Self::UntilLaunch),
        ("1d", Self::Day),
        ("7d", Self::Week),
        ("30d", Self::Month),
    ];

    /// The spelling in the settings file.
    pub fn name(self) -> &'static str {
        name_in(Self::NAMES, self)
    }

    /// How long a preview stays after its last opening; `None` for
    /// [`Self::UntilLaunch`] — its age does not matter, the launch does.
    pub fn max_age(self) -> Option<std::time::Duration> {
        let days = match self {
            Self::UntilLaunch => return None,
            Self::Day => 1,
            Self::Week => 7,
            Self::Month => 30,
        };
        Some(std::time::Duration::from_secs(days * 24 * 60 * 60))
    }
}

/// `[remote] download_conflict`: what a download does when its name already
/// exists in the target folder (045 Karar 5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DownloadConflict {
    /// The sheet asks: Keep both / Replace.
    #[default]
    Ask,
    /// The new item takes a free name (`report 2.pdf`).
    KeepBoth,
    /// The new item replaces the old one.
    Replace,
}

impl DownloadConflict {
    /// The single list of spellings in the settings file.
    pub const NAMES: &'static [(&'static str, Self)] = &[
        ("ask", Self::Ask),
        ("keep_both", Self::KeepBoth),
        ("replace", Self::Replace),
    ];

    /// The spelling in the settings file.
    pub fn name(self) -> &'static str {
        name_in(Self::NAMES, self)
    }
}

/// The size units of the settings file — **decimal**, Finder's units (the
/// transfer line's `format_bytes` speaks the same ones). The single table:
/// [`parse_size`] reads it and [`format_size`] writes it.
pub const SIZE_UNITS: &[(&str, u64)] = &[
    ("B", 1),
    ("KB", 1_000),
    ("MB", 1_000_000),
    ("GB", 1_000_000_000),
    ("TB", 1_000_000_000_000),
];

/// `"100MB"` → bytes: a whole number and one of [`SIZE_UNITS`], an optional
/// space between them; case-sensitive like every value of the file (`"100mb"`
/// is a typo, not a size). `None` for anything else and for an overflow.
pub fn parse_size(text: &str) -> Option<u64> {
    let digits = text.find(|c: char| !c.is_ascii_digit())?;
    let (number, unit) = text.split_at(digits);
    let unit = unit.strip_prefix(' ').unwrap_or(unit);
    let (_, scale) = SIZE_UNITS.iter().find(|(name, _)| *name == unit)?;
    number.parse::<u64>().ok()?.checked_mul(*scale)
}

/// Bytes → the file's spelling, in the largest unit that divides them exactly
/// (`100_000_000` → `"100MB"`, `1_500_000` → `"1500KB"`) — so the value read
/// back is the very value written.
pub fn format_size(bytes: u64) -> String {
    let (unit, scale) = SIZE_UNITS
        .iter()
        .rev()
        .find(|(_, scale)| bytes % scale == 0 && (bytes > 0 || *scale == 1))
        .copied()
        .unwrap_or(("B", 1));
    format!("{}{unit}", bytes / scale)
}

/// A folder key's text (`"~/Downloads"`) → the path, `~` expanded to `home`.
/// `None` if the text names no absolute folder (the parser already rejected it)
/// or it starts with `~` and there is no home directory.
pub fn expand_home(text: &str, home: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
    if text == "~" {
        return home.map(std::path::Path::to_path_buf);
    }
    if let Some(rest) = text.strip_prefix("~/") {
        return home.map(|home| home.join(rest));
    }
    text.starts_with('/')
        .then(|| std::path::PathBuf::from(text))
}

/// `[remote]`'s remote file keys (045 R8): previewing a remote file (⌘-click),
/// cleaning the preview folder and downloading. The numbers are design
/// constants' starting values, not measured (045 Karar 8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteFiles {
    /// `preview_max_size`, bytes: a larger file asks before its preview downloads.
    pub preview_max_size: u64,
    /// `preview_read_only`: a preview copy is made read-only (`0444`) — a hint,
    /// an application can unlock it.
    pub preview_read_only: bool,
    /// `preview_dir`: the preview folder, as written (`~` unexpanded,
    /// [`expand_home`]).
    pub preview_dir: String,
    /// `preview_keep`: how long a preview stays.
    pub preview_keep: PreviewKeep,
    /// `preview_limit`, bytes: the preview folder's size limit, applied at launch
    /// only, oldest first.
    pub preview_limit: u64,
    /// `download_dir`: where "Download to Downloads" puts an item, as written.
    pub download_dir: String,
    /// `download_conflict`: an existing name in the target folder.
    pub download_conflict: DownloadConflict,
    /// `download_notify`: a transfer that ends while bateri is in the background
    /// sends a notification.
    pub download_notify: bool,
}

impl Default for RemoteFiles {
    fn default() -> Self {
        Self {
            preview_max_size: 100_000_000,
            preview_read_only: true,
            preview_dir: "~/Library/Caches/bateri/Previews".to_owned(),
            preview_keep: PreviewKeep::default(),
            preview_limit: 2_000_000_000,
            download_dir: "~/Downloads".to_owned(),
            download_conflict: DownloadConflict::default(),
            download_notify: true,
        }
    }
}

/// Retired keys: they stay in the file, are **not read**, and leave a
/// diagnostic when seen.
///
/// The repo's rule is "an unknown key is preserved, a key is not deleted";
/// retirement is that rule's third state. Silently ignoring would be wrong — the
/// user would think the line they wrote does something; deleting would be wrong
/// too, because we don't touch the file. The diagnostic is between the two: the
/// line stays in place and the subtitle says where to look.
const RETIRED: &[(&str, &str)] = &[(
    "prompt",
    // 012 phase-10: `[shell] prompt` as a separate key produced two prompts on
    // screen; the choice moved to `integration`'s third value.
    "`shell.prompt` is no longer read; use `shell.integration = \"blocks\"` \
     to keep your own prompt",
)];

/// Everything the user can change — parsed and validated.
///
/// The fields are `pub`: the type is a record, it carries no behavior. The path
/// that establishes the value's validity is [`Settings::parse`]; a `Settings`
/// built by hand can skip these rules and that is deliberately allowed (the
/// tests build them that way).
///
/// No `Eq`: [`FontOptions::size`] is `f64`.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// `[terminal] scrollback`: the rows kept in history, `0..=SCROLLBACK_MAX`.
    pub scrollback: usize,
    /// `[terminal] cursor`: the cursor's **default** shape; an application can
    /// override it with DECSCUSR ([`CaretShape`]).
    pub cursor: CaretShape,
    /// `[terminal] cursor_blink`: whether the cursor blinks ([`CursorBlink`]).
    pub cursor_blink: CursorBlink,
    /// `[terminal] cursor_radius` + `cursor_glow` + `cursor_unfocused`:
    /// the cursor's drawing numbers ([`CaretStyle`]). Doesn't enter
    /// `TerminalOptions`.
    pub caret: CaretStyle,
    /// `[terminal] cursor_blink_interval`: the blink's **half period**, in seconds.
    ///
    /// A field separate from [`Self::caret`], because the destination is
    /// separate: the drawing numbers go to `Frame`, this goes to `bt_gpu::blink`.
    /// `Changes::caret` carries both — the precedent of `Changes::motion`'s two
    /// keys.
    pub blink_interval: f64,
    /// `[appearance] theme`: [`SYSTEM_THEME`] or a theme **name** —
    /// `themes/{name}.toml` or an embedded theme. The name is valid in form (not
    /// empty, no `/`); whether it exists requires the file system and is in
    /// `bt-shell`'s name resolution.
    pub theme: String,
    /// `[appearance] light_theme`: the light appearance's theme when
    /// `theme = "system"`. A **separate** key from `theme`: choosing a fixed
    /// theme from the menu writes only `theme` ([`Settings::with_theme`]) and the
    /// user's light/dark pair stays in place.
    pub light_theme: String,
    /// `[appearance] dark_theme`: the dark appearance's theme when
    /// `theme = "system"`.
    pub dark_theme: String,
    /// `[font] family` and `size`.
    pub font: FontOptions,
    /// `[clipboard] osc52`: `"copy"` or `"off"`.
    pub osc52: Osc52,
    /// `[motion] cursor_motion`: the cursor's glide style.
    pub cursor_motion: CursorMotion,
    /// `[motion] reduce_motion`: whether animations are reduced.
    pub reduce_motion: ReduceMotion,
    /// `[motion] smooth_scroll`: whether scrolling in scrollback is smooth.
    pub smooth_scroll: SmoothScroll,
    /// `[motion] keypress`: the effect of a glyph typed in the dock.
    pub keypress: Keypress,
    /// `[motion] erase`: the effect of a glyph erased in the dock.
    pub erase: Erase,
    /// `[shell] integration`: whether the shell wrapper is installed. Takes effect
    /// **in the next session** ([`ShellIntegration`]).
    pub shell_integration: ShellIntegration,
    /// `[terminal] confirm_close`: when to ask on close ([`ConfirmClose`]).
    /// Doesn't enter `TerminalOptions`.
    pub confirm_close: ConfirmClose,
    /// `[remote] hosts`: the remote hosts' mark patterns, in the file's order
    /// (037 Karar 2; matching is [`host_mark`]). Empty by default.
    pub remote_hosts: Vec<HostRule>,
    /// `[remote]`'s preview and download keys (045 R8, [`RemoteFiles`]).
    pub remote_files: RemoteFiles,
}

impl Default for Settings {
    /// The values in effect when there is no file and when a key is missing.
    ///
    /// `scrollback` was `bt-shell`'s `SCROLLBACK` constant until 006; the value
    /// stayed the same, its owner moved here. The theme follows the system's
    /// appearance: the embedded `bateri-light` on light, the embedded `bateri` on
    /// dark.
    ///
    /// OSC 52 is **on** (`copy`): vim over ssh having its copy arrive in the
    /// local clipboard is expected behavior from a terminal and alacritty's
    /// default too. The cost: a remote program running in the background can
    /// write to the clipboard too; it can't read. It is not the startup value in
    /// an unusable file ([`Settings::for_unusable_file`]).
    fn default() -> Self {
        Self {
            scrollback: 10_000,
            cursor: CaretShape::default(),
            cursor_blink: CursorBlink::default(),
            caret: CaretStyle::default(),
            blink_interval: CURSOR_BLINK_INTERVAL,
            theme: SYSTEM_THEME.to_owned(),
            light_theme: "bateri-light".to_owned(),
            dark_theme: "bateri".to_owned(),
            font: FontOptions::default(),
            osc52: Osc52::Copy,
            cursor_motion: CursorMotion::default(),
            reduce_motion: ReduceMotion::default(),
            smooth_scroll: SmoothScroll::default(),
            keypress: Keypress::default(),
            erase: Erase::default(),
            shell_integration: ShellIntegration::default(),
            confirm_close: ConfirmClose::default(),
            remote_hosts: Vec::new(),
            remote_files: RemoteFiles::default(),
        }
    }
}

/// The result of a file that could be parsed: the values and the ones not
/// accepted.
#[derive(Clone, Debug, PartialEq)]
pub struct Parsed {
    pub settings: Settings,
    /// Not in the file's order but **in key reading order**; if empty the file
    /// is clean.
    pub diagnostics: Vec<Diagnostic>,
}

/// Why a setting wasn't accepted.
///
/// The text is **English**: it appears in the window's subtitle, so it is a UI
/// string (`CLAUDE.md` → Dil); stderr prints a copy of the same text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// Dotted key path (`terminal.scrollback`); `None` on a syntax error.
    pub key: Option<&'static str>,
    /// The 1-based line; `None` if the parser gave no position.
    pub line: Option<usize>,
    pub message: String,
}

impl fmt::Display for Diagnostic {
    /// A single line: the window's subtitle is drawn **on the same line** as the
    /// title (a window without a toolbar, 007 phase-1 eyeball check), a long and
    /// multi-line text would be cut off there.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(line) = self.line {
            write!(f, "line {line}: ")?;
        }
        f.write_str(&self.message)
    }
}

/// The new value of a single key — what the settings window writes to the file
/// ([`Settings::with_edit`]).
///
/// Typed, not a string: the section, key and TOML type derive from the variant,
/// so the window can't write to the wrong section or with the wrong type. The
/// value's **range** isn't checked — the window's controls take the ranges from
/// here ([`CURSOR_RADIUS_RANGE`] …); even if they didn't, the parser rejects
/// when reading.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingsEdit {
    Scrollback(usize),
    Cursor(CaretShape),
    CursorBlink(CursorBlink),
    CursorRadius(f64),
    CursorGlow(f64),
    CursorUnfocused(UnfocusedCaret),
    BlinkInterval(f64),
    ConfirmClose(ConfirmClose),
    Theme(String),
    LightTheme(String),
    DarkTheme(String),
    /// An empty string is the default family (the chain): the parser reads
    /// `family = ""` that way and the key isn't deleted.
    FontFamily(String),
    FontSize(f64),
    LineHeight(f64),
    Osc52(Osc52),
    CursorMotion(CursorMotion),
    ReduceMotion(ReduceMotion),
    SmoothScroll(SmoothScroll),
    Keypress(Keypress),
    Erase(Erase),
    ShellIntegration(ShellIntegration),
    /// Shell ▸ Mark … as ▸ (037 Karar 5): the `[remote] hosts` edit that makes
    /// `host`'s mark `mark`. Not a single key's value but the array's entries —
    /// the rule is in this arm of [`Settings::with_edit`]. `host` is the form the
    /// remote session shows (`user@` included, the match's input); the pattern
    /// written is its part without `user@`. [`HostMark::None`] is the menu's
    /// "None".
    RemoteHostMark {
        host: String,
        mark: HostMark,
    },
    /// Bytes; written in the largest exact unit ([`format_size`]).
    PreviewMaxSize(u64),
    PreviewReadOnly(bool),
    /// As written in the file (`~` unexpanded).
    PreviewDir(String),
    PreviewKeep(PreviewKeep),
    /// Bytes; written in the largest exact unit ([`format_size`]).
    PreviewLimit(u64),
    /// As written in the file (`~` unexpanded).
    DownloadDir(String),
    DownloadConflict(DownloadConflict),
    DownloadNotify(bool),
}

impl SettingsEdit {
    /// The edited key's dotted path (`terminal.cursor`) — the same as
    /// [`Diagnostic::key`] that appears in the parser's diagnostic; the settings
    /// window finds its row with this.
    pub fn path(&self) -> &'static str {
        self.place().2
    }

    /// The section, the key and the dotted path the diagnostic carries
    /// (`Diagnostic::key` wants `'static`, so all three are constants).
    fn place(&self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Scrollback(_) => ("terminal", "scrollback", "terminal.scrollback"),
            Self::Cursor(_) => ("terminal", "cursor", "terminal.cursor"),
            Self::CursorBlink(_) => ("terminal", "cursor_blink", "terminal.cursor_blink"),
            Self::CursorRadius(_) => ("terminal", "cursor_radius", "terminal.cursor_radius"),
            Self::CursorGlow(_) => ("terminal", "cursor_glow", "terminal.cursor_glow"),
            Self::CursorUnfocused(_) => {
                ("terminal", "cursor_unfocused", "terminal.cursor_unfocused")
            }
            Self::BlinkInterval(_) => (
                "terminal",
                "cursor_blink_interval",
                "terminal.cursor_blink_interval",
            ),
            Self::ConfirmClose(_) => ("terminal", "confirm_close", "terminal.confirm_close"),
            Self::Theme(_) => ("appearance", "theme", "appearance.theme"),
            Self::LightTheme(_) => ("appearance", "light_theme", "appearance.light_theme"),
            Self::DarkTheme(_) => ("appearance", "dark_theme", "appearance.dark_theme"),
            Self::FontFamily(_) => ("font", "family", "font.family"),
            Self::FontSize(_) => ("font", "size", "font.size"),
            Self::LineHeight(_) => ("font", "line_height", "font.line_height"),
            Self::Osc52(_) => ("clipboard", "osc52", "clipboard.osc52"),
            Self::CursorMotion(_) => ("motion", "cursor_motion", "motion.cursor_motion"),
            Self::ReduceMotion(_) => ("motion", "reduce_motion", "motion.reduce_motion"),
            Self::SmoothScroll(_) => ("motion", "smooth_scroll", "motion.smooth_scroll"),
            Self::Keypress(_) => ("motion", "keypress", "motion.keypress"),
            Self::Erase(_) => ("motion", "erase", "motion.erase"),
            Self::ShellIntegration(_) => ("shell", "integration", "shell.integration"),
            Self::RemoteHostMark { .. } => ("remote", "hosts", "remote.hosts"),
            Self::PreviewMaxSize(_) => ("remote", "preview_max_size", "remote.preview_max_size"),
            Self::PreviewReadOnly(_) => ("remote", "preview_read_only", "remote.preview_read_only"),
            Self::PreviewDir(_) => ("remote", "preview_dir", "remote.preview_dir"),
            Self::PreviewKeep(_) => ("remote", "preview_keep", "remote.preview_keep"),
            Self::PreviewLimit(_) => ("remote", "preview_limit", "remote.preview_limit"),
            Self::DownloadDir(_) => ("remote", "download_dir", "remote.download_dir"),
            Self::DownloadConflict(_) => {
                ("remote", "download_conflict", "remote.download_conflict")
            }
            Self::DownloadNotify(_) => ("remote", "download_notify", "remote.download_notify"),
        }
    }

    /// The value to be written to the file. Rounded to two decimals: so the
    /// window's slider doesn't write `0.30000000000000004`, and so the value read
    /// is the very value written.
    fn value(&self) -> toml_edit::Value {
        let decimal = |value: f64| toml_edit::Value::from((value * 100.0).round() / 100.0);
        match self {
            // A row count that doesn't fit `i64` is far beyond the ceiling anyway.
            Self::Scrollback(lines) => i64::try_from(*lines).unwrap_or(i64::MAX).into(),
            Self::Cursor(shape) => shape.name().into(),
            Self::CursorBlink(blink) => blink.name().into(),
            Self::CursorUnfocused(unfocused) => unfocused.name().into(),
            Self::ConfirmClose(confirm) => confirm.name().into(),
            Self::Osc52(mode) => mode.name().into(),
            Self::CursorMotion(motion) => motion.name().into(),
            Self::ReduceMotion(reduce) => reduce.name().into(),
            Self::SmoothScroll(smooth) => smooth.name().into(),
            Self::Keypress(keypress) => keypress.name().into(),
            Self::Erase(erase) => erase.name().into(),
            Self::ShellIntegration(integration) => integration.name().into(),
            // Not the array itself, but the written entry's `mark`.
            Self::RemoteHostMark { mark, .. } => mark.written().into(),
            Self::PreviewKeep(keep) => keep.name().into(),
            Self::DownloadConflict(conflict) => conflict.name().into(),
            Self::PreviewMaxSize(bytes) | Self::PreviewLimit(bytes) => format_size(*bytes).into(),
            Self::PreviewReadOnly(on) | Self::DownloadNotify(on) => (*on).into(),
            Self::CursorRadius(value)
            | Self::CursorGlow(value)
            | Self::BlinkInterval(value)
            | Self::FontSize(value)
            | Self::LineHeight(value) => decimal(*value),
            Self::Theme(name)
            | Self::LightTheme(name)
            | Self::DarkTheme(name)
            | Self::FontFamily(name)
            | Self::PreviewDir(name)
            | Self::DownloadDir(name) => name.as_str().into(),
        }
    }
}

impl Settings {
    /// The `settings.toml` that "Settings…" creates when there is no file: every
    /// key with its description and default value.
    ///
    /// Its owner is the owner of the defaults, i.e. here: a test ties it so that,
    /// once parsed, it gives [`Settings::default`] with no diagnostics. The keys
    /// are **written**, not in a comment — the user changes the value in place,
    /// and choosing a theme from the menu writes the line in place too
    /// ([`Settings::with_theme`]). The cost: if a default changes one day, a user
    /// who has opened the template stays on the old one. Only `family`, which has
    /// no default, is an example in a comment.
    ///
    /// The section headers aren't in a comment: if a key whose comment was
    /// removed were left without a header it would become a root key and would be
    /// **silently** ignored as an unrecognized key.
    ///
    /// The text is English: the file the user opens is a UI string
    /// (`CLAUDE.md` → Dil).
    pub const TEMPLATE: &str = r##"# bateri settings. Changes apply as soon as you save this file.
# A key you delete goes back to its default. Values are case-sensitive; one that
# is not understood leaves its key alone and says so under the title — except
# clipboard.osc52, which turns off instead.

[terminal]
# 0 to 100000. Lines of history kept above the screen.
scrollback = 10000
# "block" | "underline" | "beam". The cursor's default shape: block fills the
# cell, underline sits below it, beam stands at its left edge. Programs such as
# vim may ask for a different shape while they run; this is the shape when none
# is asked for.
cursor = "block"
# "auto" | "on" | "off". Whether the cursor blinks: auto blinks until a program
# asks it to stop (vim in normal mode does), on blinks whatever the program
# says, off never blinks. Blinking asks for two frames a second, so it is off
# unless you choose it; with it on, it stops on its own 15 seconds after the
# window last drew anything and comes back with the next output or keystroke.
cursor_blink = "off"
# 0.0 to 0.5. How round the cursor's corners are, as a fraction of the cell's
# height: 0 is a sharp rectangle, 0.5 rounds a block into a stadium. It scales
# with the font size, so a larger point size keeps the same look.
cursor_radius = 0.10
# 0.0 to 3.0. How strong the soft shadow around the cursor is: 0 turns it off,
# 1 is the designed amount. It scales both how far the shadow reaches and how
# dark it is, because those two are one feeling, not two.
cursor_glow = 1.0
# "hollow" | "solid". What the cursor does while the window is not focused:
# hollow empties it to an outline, solid leaves it as it is. Either way a
# blinking cursor stops blinking until the window is focused again.
cursor_unfocused = "hollow"
# 0.05 to 5.0. Half the blink period in seconds: the cursor stays lit this
# long, then dark this long. Shorter costs more frames — 0.25 asks for four a
# second — and 0.5 is a blink you notice without it tiring the eye.
cursor_blink_interval = 0.5
# "never" | "running" | "always". When closing a tab or window, or quitting,
# asks first: running asks only while a program other than the shell is in
# the foreground (vim, ssh, a build) and names it, always asks even at an idle
# prompt, never closes without asking. Typing exit never asks, and neither do
# programs left running in the background.
confirm_close = "running"

[appearance]
# "system" or a theme name. "system" follows the macOS light/dark appearance;
# any other value is a theme used in both — a file themes/NAME.toml next to
# this one, or a built-in theme, "bateri" (dark) or "bateri-light" (light).
theme = "system"
# Theme names, used while theme = "system".
light_theme = "bateri-light"
dark_theme = "bateri"

[font]
# A family name as shown in Font Book. Without it bateri uses SF Mono, or
# Menlo when SF Mono is not installed — SF Mono ships with Xcode, so it is not
# on every machine. A character the family lacks is drawn from the system font
# chain when it fits one cell; emoji, CJK and other wide glyphs stay as boxes.
# family = "Menlo"
# Greater than 0. Size in points.
size = 13
# 1 to 2. Line spacing as a multiple of the font's own: 1 is the font's own
# spacing, 1.4 is airy. Below 1 is refused — it would clip the tails of g and y.
line_height = 1.0

[clipboard]
# "copy" | "off". Lets programs in the terminal, also over ssh, copy text to
# the clipboard (OSC 52): copy allows it, off does not. They can never read it.
osc52 = "copy"

[motion]
# "snap" | "ease" | "spring". How the cursor travels between cells: spring
# glides and eases into place, ease glides for a fixed time, snap jumps there
# at once.
cursor_motion = "spring"
# "system" | "on" | "off". Whether to tone animations down to a short fade:
# system follows the macOS Reduce Motion setting, on and off decide it here.
reduce_motion = "system"
# "on" | "off". How scrolling back through history moves: on follows your
# fingers on a trackpad pixel by pixel, lets a flick coast to a stop, glides a
# mouse wheel notch and settles on a whole line when you let go; off moves
# line by line. Reduce Motion and cursor_motion = "snap" also move line by
# line.
smooth_scroll = "on"
# "off" | "fade" | "rise" | "pop" | "extrude" | "heat" | "echo" | "drop" |
# "ink" | "squeeze". How a letter you type in the dock at the bottom of the
# window appears: fade brings it in from clear, rise slides it up into place,
# pop springs it out from small, extrude stretches it out from its left edge,
# heat starts it in the cursor color and cools it to its own, echo sends a
# faint copy of it rippling outward, drop lets it fall into place with a small
# bounce, ink fills it from the middle of its strokes outward, squeeze starts
# it narrow and tall and lets it spring into shape. off shows it at once.
keypress = "fade"
# "off" | "iris" | "undertow" | "echo" | "bleed" | "unravel" | "recede" |
# "sublime" | "shatter". How a letter you delete in the dock goes: iris closes
# a round shutter over it, undertow pulls it down toward the cursor, echo
# swells it outward like a ripple, bleed lets its ink spread thin, unravel
# slides it apart in strips, recede shrinks it away, sublime lets it drift up
# like vapor, shatter breaks it into falling pieces. off removes it at once.
# Pasting, history and deleting a whole word or line are instant. cursor_motion = "snap" turns both off; Reduce Motion
# keeps only a fade for typing.
erase = "recede"

[shell]
# "auto" | "blocks" | "off". Whether bateri sets up the shell so it can report
# where prompts and commands begin and end. auto does it for shells bateri
# knows, and on those shells it also moves the line you type into the dock at
# the bottom of the window and draws the prompt itself. blocks keeps command
# blocks and marks but leaves the line and the prompt to your shell, the way a
# terminal normally works. off never sets anything up.
# Unlike every other key here, this one only takes effect in shells started
# after the change; shells already open keep what they were started with.
integration = "auto"

[remote]
# Colors the dock of an ssh or mosh session by the host it is on, so a
# production machine is never mistaken for another. Each entry names a host
# pattern and a mark: "production" (red), "staging" (yellow), "development"
# (green), "none" (no mark), or a color like "#c678dd". In a pattern * stands
# for any run of characters and ? for one, ignoring case; a pattern without @
# matches the host after any user@. The first entry that matches wins, so put
# exact names before wide patterns; "none" stops the search. Shell > Mark
# "host" as writes the entry for the host of the ssh tab you are in.
# hosts = [
#   { host = "prod-*", mark = "production" },
#   { host = "*.staging.example.com", mark = "staging" },
# ]
hosts = []
# Sizes are written like "100MB" or "2GB" (B, KB, MB, GB, TB); folders start
# with / or ~/.
# A file larger than this asks before its preview downloads (cmd-click on a
# remote file name).
preview_max_size = "100MB"
# true | false. Previews open read-only. It is a hint: an app can unlock one,
# and a preview you changed is moved to the download folder, never deleted.
preview_read_only = true
# Where previews are kept.
preview_dir = "~/Library/Caches/bateri/Previews"
# "launch" | "1d" | "7d" | "30d". How long a preview stays after you last
# opened it; checked when bateri starts and once a day. launch keeps previews
# until bateri starts again.
preview_keep = "7d"
# The preview folder's size limit, applied when bateri starts, oldest first.
preview_limit = "2GB"
# Where "Download to Downloads" puts a remote file or folder.
download_dir = "~/Downloads"
# "ask" | "keep_both" | "replace". What a download does when the name already
# exists: ask, keep both (the new one gets a number), or replace the old one.
download_conflict = "ask"
# true | false. Notify when a transfer ends while bateri is in the background.
download_notify = true
"##;

    /// The startup settings when the file **exists but is unusable** (can't be
    /// read or invalid TOML): the defaults, with only OSC 52 **off**.
    ///
    /// The file may hold the user's `osc52 = "off"` and an unreadable file can't
    /// say so: the clipboard falls to off for a remote program, not to on
    /// (`discussion.md` → Karar 5). A wrong guess on every other key is harmless
    /// and visible (theme, point size); OSC 52's is silent. When the file is
    /// fixed and saved, the live reload applies the value in the file.
    ///
    /// When the file **doesn't exist** it isn't this but [`Settings::default`]:
    /// the user said nothing. The one that decides (`bt-shell`'s loader)
    /// separates the two; the value's owner is here, because it is the defaults'
    /// owner.
    pub fn for_unusable_file() -> Self {
        Self {
            osc52: Osc52::Off,
            ..Self::default()
        }
    }

    /// `settings.toml`'s text → values + diagnostics, or it couldn't be parsed.
    ///
    /// `Err` **only** on invalid TOML; every key-level problem falls into `Ok`'s
    /// diagnostic list and that key takes its default (`osc52` takes off,
    /// [`Settings::parse_keeping`]).
    ///
    /// Wider than invalid TOML syntax: a duplicate key and a number exceeding
    /// TOML's integer limit (`i64`) also drop the whole document —
    /// `scrollback = 99999999999999999999` can't be clamped to the ceiling,
    /// because the value can't be read at all. `docs/AYARLAR.md` says this.
    pub fn parse(text: &str) -> Result<Parsed, Diagnostic> {
        Self::parse_keeping(text, &Settings::default())
    }

    /// [`Settings::parse`], but a value that is **not accepted** takes
    /// `fallback`'s rather than the default — the save-time rule: the caller
    /// supplies the current settings (`bt-shell`'s live reload).
    ///
    /// The reason is the application that can't be undone: if a mistakenly saved
    /// `scrollback = "100000"` fell to the default (ten thousand) while it was
    /// `scrollback = 100000`, ninety thousand rows of history would be deleted at
    /// that moment and fixing the file wouldn't bring them back. The diagnostic
    /// also says the value fallen back to ("using 100000").
    ///
    /// Only a value that isn't accepted: a key that is **not in** the file takes
    /// its default (the file says nothing, a user who deleted the key wants the
    /// default) and a value above the ceiling is clamped to the ceiling (the
    /// intent is clear). If a section has the wrong type (`terminal = 5`) all of
    /// the section's keys count as not accepted.
    ///
    /// **The one exception is `clipboard.osc52`:** a value that isn't accepted
    /// takes `"off"`, not `fallback`'s, even when the section has the wrong type.
    /// The rule's reason was the application that can't be undone and turning
    /// OSC 52 off is reversible; the opposite, a turn-off mistyped as `"of"`
    /// silently keeping the clipboard on, is not (`discussion.md` → Karar 5).
    pub fn parse_keeping(text: &str, fallback: &Settings) -> Result<Parsed, Diagnostic> {
        let doc = document(text)?;
        let mut parsed = Parsed {
            settings: Settings::default(),
            diagnostics: Vec::new(),
        };
        let root = doc.as_table();
        match section(text, root, "terminal", &mut parsed.diagnostics) {
            Some(terminal) => {
                if let Some(item) = terminal.get("scrollback") {
                    parsed.settings.scrollback =
                        scrollback(text, item, fallback.scrollback, &mut parsed.diagnostics);
                }
                if let Some(item) = terminal.get("cursor") {
                    parsed.settings.cursor = named_enum(
                        text,
                        item,
                        "terminal.cursor",
                        CaretShape::NAMES,
                        fallback.cursor,
                        &mut parsed.diagnostics,
                    );
                }
                if let Some(item) = terminal.get("cursor_blink") {
                    parsed.settings.cursor_blink = named_enum(
                        text,
                        item,
                        "terminal.cursor_blink",
                        CursorBlink::NAMES,
                        fallback.cursor_blink,
                        &mut parsed.diagnostics,
                    );
                }
                if let Some(item) = terminal.get("cursor_radius") {
                    parsed.settings.caret.radius_ratio = ranged_float(
                        text,
                        item,
                        "terminal.cursor_radius",
                        CURSOR_RADIUS_RANGE,
                        fallback.caret.radius_ratio,
                        &mut parsed.diagnostics,
                    );
                }
                if let Some(item) = terminal.get("cursor_blink_interval") {
                    parsed.settings.blink_interval = ranged_float(
                        text,
                        item,
                        "terminal.cursor_blink_interval",
                        CURSOR_BLINK_RANGE,
                        fallback.blink_interval,
                        &mut parsed.diagnostics,
                    );
                }
                if let Some(item) = terminal.get("cursor_unfocused") {
                    parsed.settings.caret.unfocused = named_enum(
                        text,
                        item,
                        "terminal.cursor_unfocused",
                        UnfocusedCaret::NAMES,
                        fallback.caret.unfocused,
                        &mut parsed.diagnostics,
                    );
                }
                if let Some(item) = terminal.get("cursor_glow") {
                    parsed.settings.caret.glow = ranged_float(
                        text,
                        item,
                        "terminal.cursor_glow",
                        CURSOR_GLOW_RANGE,
                        fallback.caret.glow,
                        &mut parsed.diagnostics,
                    );
                }
                if let Some(item) = terminal.get("confirm_close") {
                    parsed.settings.confirm_close = named_enum(
                        text,
                        item,
                        "terminal.confirm_close",
                        ConfirmClose::NAMES,
                        fallback.confirm_close,
                        &mut parsed.diagnostics,
                    );
                }
            }
            None if root.contains_key("terminal") => {
                parsed.settings.scrollback = fallback.scrollback;
                parsed.settings.cursor = fallback.cursor;
                parsed.settings.cursor_blink = fallback.cursor_blink;
                parsed.settings.caret = fallback.caret;
                parsed.settings.blink_interval = fallback.blink_interval;
                parsed.settings.confirm_close = fallback.confirm_close;
            }
            None => {}
        }
        // The second is the dotted path in the diagnostic (`Diagnostic::key` wants
        // `'static`), the same idiom as `theme.rs`'s `ANSI_KEYS`; the last is the
        // one that stands in for a value that isn't accepted.
        let names = [
            (
                "theme",
                "appearance.theme",
                &mut parsed.settings.theme,
                &fallback.theme,
            ),
            (
                "light_theme",
                "appearance.light_theme",
                &mut parsed.settings.light_theme,
                &fallback.light_theme,
            ),
            (
                "dark_theme",
                "appearance.dark_theme",
                &mut parsed.settings.dark_theme,
                &fallback.dark_theme,
            ),
        ];
        match section(text, root, "appearance", &mut parsed.diagnostics) {
            Some(appearance) => {
                for (key, path, slot, kept) in names {
                    if let Some(item) = appearance.get(key) {
                        let accepts_system = key == "theme";
                        let diagnostics = &mut parsed.diagnostics;
                        *slot = theme_name(text, item, path, kept, accepts_system, diagnostics)
                            .unwrap_or_else(|| kept.clone());
                    }
                }
            }
            None if root.contains_key("appearance") => {
                for (_, _, slot, kept) in names {
                    slot.clone_from(kept);
                }
            }
            None => {}
        }
        match section(text, root, "font", &mut parsed.diagnostics) {
            Some(font) => {
                let diagnostics = &mut parsed.diagnostics;
                if let Some(item) = font.get("family") {
                    parsed.settings.font.family =
                        font_family(text, item, &fallback.font.family, diagnostics);
                }
                if let Some(item) = font.get("size") {
                    parsed.settings.font.size =
                        font_size(text, item, fallback.font.size, diagnostics);
                }
                if let Some(item) = font.get("line_height") {
                    parsed.settings.font.line_height =
                        line_height(text, item, fallback.font.line_height, diagnostics);
                }
            }
            None if root.contains_key("font") => {
                parsed.settings.font.clone_from(&fallback.font);
            }
            None => {}
        }
        // `fallback` is deliberately not read: a value that isn't accepted falls
        // to off (the exception in the doc above).
        match section(text, root, "clipboard", &mut parsed.diagnostics) {
            Some(clipboard) => {
                if let Some(item) = clipboard.get("osc52") {
                    parsed.settings.osc52 = named_enum(
                        text,
                        item,
                        "clipboard.osc52",
                        Osc52::NAMES,
                        Osc52::Off,
                        &mut parsed.diagnostics,
                    );
                }
            }
            None if root.contains_key("clipboard") => {
                parsed.settings.osc52 = Osc52::Off;
            }
            None => {}
        }
        match section(text, root, "motion", &mut parsed.diagnostics) {
            Some(motion) => {
                if let Some(item) = motion.get("cursor_motion") {
                    parsed.settings.cursor_motion = named_enum(
                        text,
                        item,
                        "motion.cursor_motion",
                        CursorMotion::NAMES,
                        fallback.cursor_motion,
                        &mut parsed.diagnostics,
                    );
                }
                if let Some(item) = motion.get("reduce_motion") {
                    parsed.settings.reduce_motion = named_enum(
                        text,
                        item,
                        "motion.reduce_motion",
                        ReduceMotion::NAMES,
                        fallback.reduce_motion,
                        &mut parsed.diagnostics,
                    );
                }
                if let Some(item) = motion.get("smooth_scroll") {
                    parsed.settings.smooth_scroll = named_enum(
                        text,
                        item,
                        "motion.smooth_scroll",
                        SmoothScroll::NAMES,
                        fallback.smooth_scroll,
                        &mut parsed.diagnostics,
                    );
                }
                if let Some(item) = motion.get("keypress") {
                    parsed.settings.keypress = named_enum(
                        text,
                        item,
                        "motion.keypress",
                        Keypress::NAMES,
                        fallback.keypress,
                        &mut parsed.diagnostics,
                    );
                }
                if let Some(item) = motion.get("erase") {
                    parsed.settings.erase = named_enum(
                        text,
                        item,
                        "motion.erase",
                        Erase::NAMES,
                        fallback.erase,
                        &mut parsed.diagnostics,
                    );
                }
            }
            None if root.contains_key("motion") => {
                parsed.settings.cursor_motion = fallback.cursor_motion;
                parsed.settings.reduce_motion = fallback.reduce_motion;
                parsed.settings.smooth_scroll = fallback.smooth_scroll;
                parsed.settings.keypress = fallback.keypress;
                parsed.settings.erase = fallback.erase;
            }
            None => {}
        }
        match section(text, root, "shell", &mut parsed.diagnostics) {
            Some(shell) => {
                if let Some(item) = shell.get("integration") {
                    parsed.settings.shell_integration = named_enum(
                        text,
                        item,
                        "shell.integration",
                        ShellIntegration::NAMES,
                        fallback.shell_integration,
                        &mut parsed.diagnostics,
                    );
                }
                // Retired keys: the value isn't read, its presence is reported.
                for (key, message) in RETIRED {
                    if let Some(item) = shell.get(key) {
                        parsed.diagnostics.push(Diagnostic {
                            key: None,
                            line: item.span().and_then(|span| line_of(text, span.start)),
                            message: (*message).to_owned(),
                        });
                    }
                }
            }
            None if root.contains_key("shell") => {
                parsed.settings.shell_integration = fallback.shell_integration;
            }
            None => {}
        }
        match section(text, root, "remote", &mut parsed.diagnostics) {
            Some(remote) => {
                if let Some(item) = remote.get("hosts") {
                    parsed.settings.remote_hosts =
                        host_rules(text, item, &fallback.remote_hosts, &mut parsed.diagnostics);
                }
                parsed.settings.remote_files = remote_files(
                    text,
                    remote,
                    &fallback.remote_files,
                    &mut parsed.diagnostics,
                );
            }
            None if root.contains_key("remote") => {
                parsed
                    .settings
                    .remote_hosts
                    .clone_from(&fallback.remote_hosts);
                parsed
                    .settings
                    .remote_files
                    .clone_from(&fallback.remote_files);
            }
            None => {}
        }
        Ok(parsed)
    }

    /// The **name** of the theme to use: `light_theme` or `dark_theme` by
    /// appearance if `theme = "system"`, otherwise `theme` itself — independent
    /// of the appearance.
    ///
    /// Pure: `bt-shell` reads the appearance, and name resolution is there too.
    pub fn theme_for(&self, dark: bool) -> &str {
        match (self.follows_system(), dark) {
            (false, _) => &self.theme,
            (true, true) => &self.dark_theme,
            (true, false) => &self.light_theme,
        }
    }

    /// Whether the theme is tied to the system's appearance. If not, an
    /// appearance change doesn't touch the theme and the caller needn't re-read
    /// the file.
    pub fn follows_system(&self) -> bool {
        self.theme == SYSTEM_THEME
    }

    /// The session's terminal options — **all of them** go to `Session` with this,
    /// both at startup and on a live change ([`TerminalOptions`]'s doc).
    pub fn terminal(&self) -> TerminalOptions {
        TerminalOptions {
            scrollback: self.scrollback,
            osc52: self.osc52,
            cursor: self.cursor,
            blink: self.cursor_blink,
        }
    }

    /// What changed from `self` (the previous) to `new` — the live reload's gate:
    /// a part that didn't change isn't applied.
    ///
    /// Pure; the caller (`bt-shell`) holds the previous value. There is no
    /// separate merging mechanism: if one save causes several events the second
    /// gives an empty difference.
    pub fn changes(&self, new: &Settings) -> Changes {
        Changes {
            terminal: self.terminal() != new.terminal(),
            font: self.font != new.font,
            motion: self.cursor_motion != new.cursor_motion
                || self.reduce_motion != new.reduce_motion
                || self.smooth_scroll != new.smooth_scroll
                || self.keypress != new.keypress
                || self.erase != new.erase,
            caret: self.caret != new.caret || self.blink_interval != new.blink_interval,
            remote: self.remote_hosts != new.remote_hosts || self.remote_files != new.remote_files,
        }
    }

    /// The menu's theme choice (View ▸ Theme ▸): makes `[appearance] theme`
    /// `name` — the theme form of [`Settings::with_edit`].
    ///
    /// It doesn't touch `light_theme` and `dark_theme`: a user who chose a fixed
    /// theme finds their pair again when they return to `"system"`. The name's
    /// format isn't checked: the menu gives only the names of the embedded
    /// themes and of the files in `themes/`; even if it didn't, the parser
    /// rejects the name when reading.
    pub fn with_theme(text: &str, name: &str) -> Result<String, Diagnostic> {
        Self::with_edit(text, &SettingsEdit::Theme(name.to_owned()))
    }

    /// Makes a single key in `settings.toml`'s text `edit`'s value and leaves
    /// **every other byte** in place — comments, blank lines, key order, keys we
    /// don't recognize, the comment next to the value. `bt-shell` reads and
    /// writes the file; the writers are the menu and the settings window.
    ///
    /// - If the section is missing it is appended at the end, if the key is
    ///   missing it is added inside the section; the section's spelling (header,
    ///   inline table, dotted key) is preserved.
    /// - **Text that can't be parsed is `Err`**, no new text is produced: the
    ///   file is the user's half-finished work and writing over it would erase
    ///   it. For the same reason a section that isn't a section (`appearance = 1`,
    ///   `[[appearance]]`) and a key that is a section (`[appearance.theme]`,
    ///   `theme = { … }`) are `Err` too: writing in their place would erase
    ///   their contents. A value of a type that isn't accepted (`theme = 3`)
    ///   does change — the user chose a value.
    ///
    /// [`SettingsEdit::RemoteHostMark`] writes the array's entries, not a single
    /// value; its rule is in [`with_host_mark`], with the same contract (text
    /// that can't be parsed and a broken array are `Err`, every other byte stays
    /// in place).
    pub fn with_edit(text: &str, edit: &SettingsEdit) -> Result<String, Diagnostic> {
        if let SettingsEdit::RemoteHostMark { host, mark } = edit {
            return with_host_mark(text, host, *mark);
        }
        let (section_name, key, path) = edit.place();
        let value = edit.value();
        let parsed = document(text)?;
        // On the document with positions for the rejection: `into_mut` drops the
        // positions, the diagnostic's line comes from them.
        let mut refused = Vec::new();
        // An inline table (`theme = { … }`) is a section too: `is_value` would let
        // it through and writing in its place would in this spelling silently
        // delete the content that `[appearance.theme]` is rejected for
        // (a `/code-review` finding).
        if let Some(table) = section(text, parsed.as_table(), section_name, &mut refused)
            && let Some(item) = table
                .get(key)
                .filter(|item| !item.is_value() || item.is_inline_table())
        {
            let expected = match value {
                toml_edit::Value::String(_) => "a string",
                toml_edit::Value::Integer(_) => "an integer",
                _ => "a number",
            };
            refused.push(Diagnostic {
                key: Some(path),
                line: item.span().and_then(|span| line_of(text, span.start)),
                message: format!("`{path}` must be {expected}, found {}", kind(item)),
            });
        }
        if let Some(diagnostic) = refused.pop() {
            return Err(diagnostic);
        }
        let mut doc = parsed.into_mut();
        ensure_section(&mut doc, section_name);
        // There is no `else` arm: a section that isn't a section was rejected
        // above, and a missing one was just added as a table.
        if let Some(table) = doc.get_mut(section_name).and_then(Item::as_table_like_mut) {
            match table.get_mut(key).and_then(Item::as_value_mut) {
                // The decor (the space after `=`, the comment at line end) sits on
                // the value; if the new value doesn't inherit it the comment would
                // be dropped.
                Some(old) => {
                    let decor = old.decor().clone();
                    *old = value;
                    *old.decor_mut() = decor;
                }
                None => {
                    table.insert(key, Item::Value(value));
                }
            }
        }
        Ok(rendered(text, &doc))
    }
}

/// Adds an empty `[name]` to the document if the section is missing; if present
/// leaves it alone.
///
/// A comment at the end of the document is the document's tail in `toml_edit`
/// and the new section would be written in front of it: the `# family = "Menlo"`
/// under the last section would pass to `[appearance]`, and the line of a user
/// who uncommented it would be silently ignored. The tail is taken in front of
/// the new header, i.e. it stays in the section it was written in.
fn ensure_section(doc: &mut toml_edit::DocumentMut, name: &str) {
    if doc.contains_key(name) {
        return;
    }
    let mut table = toml_edit::Table::new();
    let trailing = doc.trailing().as_str().unwrap_or_default().to_owned();
    if !trailing.trim().is_empty() {
        table.decor_mut().set_prefix(format!("{trailing}\n"));
        doc.set_trailing("");
    }
    doc.insert(name, Item::Table(table));
}

/// The edited document's text, with `text`'s line endings.
///
/// `toml_edit` writes line endings as LF. A file whose first line is CRLF stays
/// CRLF; otherwise a single selection would show the whole file as changed in a
/// dotfile repository. A file with mixed line endings takes the first line's.
///
/// It is first lowered to LF and then converted (a `/code-review` finding):
/// `toml_edit` leaves a `\r\n` **inside** a multi-line string as it is and a
/// direct conversion would turn it into `\r\r\n` — invalid TOML, so the file
/// couldn't be written again and no save would be applied.
fn rendered(text: &str, doc: &toml_edit::DocumentMut) -> String {
    let crlf = text
        .find('\n')
        .is_some_and(|end| text.as_bytes()[..end].ends_with(b"\r"));
    let written = doc.to_string();
    if crlf {
        written.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        written
    }
}

/// What [`with_host_mark`] will do to the array ([`host_mark_plan`]).
#[derive(Debug, PartialEq, Eq)]
struct MarkPlan {
    /// The entry at this index has its `mark` changed in place.
    in_place: Option<usize>,
    /// The deleted entries, in ascending order.
    remove: Vec<usize>,
    /// `{ host = <host without user@>, mark }` goes at the start of the array.
    prepend: bool,
}

/// The menu's writing rule (037 Karar 5), pure: the edit that disturbs `rules`
/// the least so that `host`'s resolution becomes `mark`; `None` (a no-op) if the
/// resolution is already `mark`.
///
/// - The first entry that writes exactly this host (the pattern equals, case
///   insensitive, the host without `user@` or the full host) has its mark changed
///   **in place** — the order the user set isn't disturbed.
/// - If the in-place change doesn't give the result (there is no entry, or a
///   glob that gives another mark stands in front of it) the exact entries are
///   deleted and the new one is written **at the start**: the user said "this
///   machine is prod", that sentence mustn't stay behind a glob and look
///   ineffective. The decision says "at the start if there is none"; an exact
///   entry with a glob in front of it is moved to the start for the same reason.
/// - **None** deletes the exact entries; if a glob still gives a mark afterwards
///   `mark = "none"` is written at the start.
fn host_mark_plan(rules: &[HostRule], host: &str, mark: HostMark) -> Option<MarkPlan> {
    if host_mark(rules, host) == mark {
        return None;
    }
    let bare = bare_host(host).to_lowercase();
    let full = host.to_lowercase();
    let exact: Vec<usize> = rules
        .iter()
        .enumerate()
        .filter(|(_, rule)| {
            let pattern = rule.pattern.to_lowercase();
            pattern == bare || pattern == full
        })
        .map(|(index, _)| index)
        .collect();
    if mark != HostMark::None
        && let Some(&first) = exact.first()
    {
        let mut edited = rules.to_vec();
        edited[first].mark = mark;
        if host_mark(&edited, host) == mark {
            return Some(MarkPlan {
                in_place: Some(first),
                remove: Vec::new(),
                prepend: false,
            });
        }
    }
    let kept: Vec<HostRule> = rules
        .iter()
        .enumerate()
        .filter(|(index, _)| !exact.contains(index))
        .map(|(_, rule)| rule.clone())
        .collect();
    Some(MarkPlan {
        in_place: None,
        prepend: host_mark(&kept, host) != mark,
        remove: exact,
    })
}

/// The host's part without `user@`: the match's input when the pattern has no
/// `@` ([`host_mark`]) and the name in the menu's title.
pub fn bare_host(host: &str) -> &str {
    host.rsplit('@').next().unwrap_or(host)
}

/// The writing of [`SettingsEdit::RemoteHostMark`]: applies [`host_mark_plan`]
/// to `[remote] hosts`. Both spellings (the inline array and `[[remote.hosts]]`)
/// stay in their own form; if the section or key is missing it is born as an
/// inline array. Text that can't be parsed and a broken array are `Err` —
/// leaving a broken entry in place and writing in front of it would be guessing
/// the list's meaning.
fn with_host_mark(text: &str, host: &str, mark: HostMark) -> Result<String, Diagnostic> {
    let parsed = document(text)?;
    let mut refused = Vec::new();
    let rules = match section(text, parsed.as_table(), "remote", &mut refused)
        .and_then(|remote| remote.get("hosts"))
    {
        Some(item) => host_rules(text, item, &[], &mut refused),
        None => Vec::new(),
    };
    if let Some(diagnostic) = refused.pop() {
        return Err(diagnostic);
    }
    let Some(plan) = host_mark_plan(&rules, host, mark) else {
        return Ok(text.to_owned());
    };
    let pattern = bare_host(host);
    let written = mark.written();
    let mut doc = parsed.into_mut();
    ensure_section(&mut doc, "remote");
    // There is no `else` arm: a section that isn't a section was rejected above,
    // and a missing one was just added.
    if let Some(remote) = doc.get_mut("remote").and_then(Item::as_table_like_mut) {
        match remote.get_mut("hosts") {
            Some(Item::ArrayOfTables(tables)) => {
                if let Some(index) = plan.in_place
                    && let Some(table) = tables.get_mut(index)
                {
                    set_keeping_decor(table.get_mut("mark"), &written);
                }
                for &index in plan.remove.iter().rev() {
                    tables.remove(index);
                }
                if plan.prepend {
                    let mut table = toml_edit::Table::new();
                    table.insert("host", toml_edit::value(pattern));
                    table.insert("mark", toml_edit::value(written.as_str()));
                    // The old first section's place and the comment above it pass
                    // to the new one (writing order is by position; at equal
                    // position the array's order), the old one is separated by a
                    // blank line.
                    if let Some(first) = tables.get_mut(0) {
                        table.set_position(first.position());
                        *table.decor_mut() = first.decor().clone();
                        first.decor_mut().set_prefix("\n");
                    }
                    tables.insert(0, table);
                }
            }
            Some(Item::Value(toml_edit::Value::Array(array))) => {
                if let Some(index) = plan.in_place
                    && let Some(entry) = array
                        .get_mut(index)
                        .and_then(toml_edit::Value::as_inline_table_mut)
                    && let Some(old) = entry.get_mut("mark")
                {
                    let decor = old.decor().clone();
                    *old = written.as_str().into();
                    *old.decor_mut() = decor;
                }
                for &index in plan.remove.iter().rev() {
                    array.remove(index);
                }
                if plan.prepend {
                    prepend_entry(array, pattern, &written);
                }
            }
            // No key (as if the array were empty): only writing at the start is
            // possible.
            _ => {
                let mut array = toml_edit::Array::new();
                if plan.prepend {
                    prepend_entry(&mut array, pattern, &written);
                }
                remote.insert("hosts", Item::Value(array.into()));
            }
        }
    }
    Ok(rendered(text, &doc))
}

/// If `item` is a value, makes it `written`, preserving its decor (the comment
/// next to it) — the `mark = "…"` line of a `[[remote.hosts]]` entry.
fn set_keeping_decor(item: Option<&mut Item>, written: &str) {
    if let Some(old) = item.and_then(Item::as_value_mut) {
        let decor = old.decor().clone();
        *old = written.into();
        *old.decor_mut() = decor;
    }
}

/// Writes `{ host, mark }` at the start of an inline array and keeps the array's
/// spelling: the new entry takes the old first entry's decor (the `\n  `
/// indentation in a multi-line array); the old first entry gains a space after
/// the comma in a single-line array, otherwise `{…},{…}` would stick together.
fn prepend_entry(array: &mut toml_edit::Array, pattern: &str, written: &str) {
    let mut entry = toml_edit::InlineTable::new();
    entry.insert("host", pattern.into());
    entry.insert("mark", written.into());
    entry.fmt();
    let mut value = toml_edit::Value::InlineTable(entry);
    if let Some(first) = array.get_mut(0) {
        *value.decor_mut() = first.decor().clone();
        let multiline = first
            .decor()
            .prefix()
            .and_then(|prefix| prefix.as_str())
            .is_some_and(|prefix| prefix.contains('\n'));
        if !multiline {
            first.decor_mut().set_prefix(" ");
        }
    }
    array.insert_formatted(0, value);
}

/// The difference between two [`Settings`] ([`Settings::changes`]).
///
/// **The theme isn't here**, deliberately: on a live reload the theme is
/// re-resolved on every event, because the active theme file is itself a source
/// and its change isn't visible in the settings text's difference. A field for
/// the theme name would be a second, half gate; swapping the same theme is a
/// no-op anyway (`Session::set_theme`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    /// [`Settings::terminal`] changed: the options go to `Session` **in full**.
    pub terminal: bool,
    /// [`Settings::font`] changed: goes to the renderer, the cell size and grid
    /// are recomputed.
    pub font: bool,
    /// The `[motion]` section changed: goes to the rhythm that drives the frame
    /// (`bt_gpu::DisplayLink::set_cursor_motion`,
    /// `bt_gpu::DisplayLink::set_reduce_motion`). A field separate from terminal
    /// and font, because motion concerns neither the session nor the cell size —
    /// had it been tied to either, a style change would have caused the grid to
    /// be rebuilt.
    ///
    /// Three keys in **one** field: all three are resolved at the same call site
    /// and separate fields would make the caller write three `if`s instead of
    /// one. Since [`Settings::reduce_motion`] is three-valued `bt-shell` has to
    /// resolve it anyway, and [`Settings::smooth_scroll`] collapses with the
    /// other two to a single `bool` (for the view's wheel); the difference only
    /// says "something changed".
    pub motion: bool,
    /// [`Settings::caret`] changed: the cursor's drawing numbers go to `bt-gpu`
    /// (`bt_gpu::DisplayLink::set_caret_style`).
    ///
    /// **A field separate from `terminal`** and this is a must: `changes.terminal`
    /// today is exactly `self.terminal() != new.terminal()`, i.e. the difference
    /// of `TerminalOptions`. These two keys **don't enter** there; had they
    /// shared the field, a radius change would send `TerminalOptions` to
    /// `Session` anew.
    pub caret: bool,
    /// [`Settings::remote_hosts`] or [`Settings::remote_files`] changed: the
    /// pattern list goes to every session (`Session::set_host_marks`) and the
    /// active remote host's mark is resolved again (037 Karar 2); the remote file
    /// keys are read where they are used (045). One field for the section: a
    /// remote file key's change re-sends the same marks, which is a no-op.
    pub remote: bool,
}

/// Parses the text into a TOML document; a one-line diagnostic if it can't be
/// parsed.
///
/// `Document` (the immutable document), not `DocumentMut`: positions live only
/// in the parsed document and the diagnostic's line comes from them.
pub(crate) fn document(text: &str) -> Result<Document<&str>, Diagnostic> {
    Document::parse(text).map_err(|err| Diagnostic {
        key: None,
        line: err.span().and_then(|span| line_of(text, span.start)),
        message: format!("invalid TOML: {}", parser_reason(err.message())),
    })
}

/// Reads a section; if it isn't a section (`terminal = 5`) leaves a diagnostic
/// and `None`.
///
/// `TableLike`: the `[terminal]` header and the `terminal = { scrollback = 1 }`
/// inline table are the same section.
pub(crate) fn section<'a>(
    text: &str,
    root: &'a toml_edit::Table,
    name: &'static str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<&'a dyn TableLike> {
    let item = root.get(name)?;
    let table = item.as_table_like();
    if table.is_none() {
        diagnostics.push(Diagnostic {
            key: Some(name),
            line: item.span().and_then(|span| line_of(text, span.start)),
            message: format!("`{name}` must be a section, found {}", kind(item)),
        });
    }
    table
}

/// `terminal.scrollback`: an integer, non-negative, the ceiling if it exceeds it.
///
/// The two not-accepted states give two separate results and both leave a
/// diagnostic:
///
/// - **Above the ceiling → the ceiling.** The intent of a user who asks for
///   "lots of history" is clear; dropping to `fallback` (ten thousand at
///   startup) would give the opposite of what they want. The diagnostic isn't
///   silent: the number they asked for wasn't applied and they should know.
///   (Point-size clamping is silent — there the limit is the usual end of
///   Cmd +/−, not an error.)
/// - **Negative or not an integer → `fallback`.** The intent can't be read.
fn scrollback(
    text: &str,
    item: &Item,
    fallback: usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> usize {
    const KEY: &str = "terminal.scrollback";
    let line = item.span().and_then(|span| line_of(text, span.start));
    let reject = |message: String| Diagnostic {
        key: Some(KEY),
        line,
        message,
    };
    let Some(value) = item.as_integer() else {
        diagnostics.push(reject(format!(
            "`{KEY}` must be an integer, found {}; using {fallback}",
            kind(item)
        )));
        return fallback;
    };
    let Ok(value) = usize::try_from(value) else {
        diagnostics.push(reject(format!(
            "`{KEY}` cannot be negative; using {fallback}"
        )));
        return fallback;
    };
    if value > SCROLLBACK_MAX {
        diagnostics.push(reject(format!(
            "`{KEY}` is at most {SCROLLBACK_MAX}; using {SCROLLBACK_MAX}"
        )));
        return SCROLLBACK_MAX;
    }
    value
}

/// `appearance.theme`, `.light_theme`, `.dark_theme`: a theme name (for `theme`
/// [`SYSTEM_THEME`] too).
///
/// Only the name's **format** is checked: an empty name and a name containing
/// `/` return to the default. `/` would carry the name outside the `themes/`
/// directory — `"../settings"` would make the settings file itself be read as a
/// theme. NUL can't be a file path either. Whether the name resolves to a theme
/// is `bt-shell`'s job.
fn theme_name(
    text: &str,
    item: &Item,
    path: &'static str,
    default: &str,
    accepts_system: bool,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<String> {
    let line = item.span().and_then(|span| line_of(text, span.start));
    let reject = |message: String| Diagnostic {
        key: Some(path),
        line,
        message,
    };
    let Some(name) = item.as_str() else {
        diagnostics.push(reject(format!(
            "`{path}` must be a string, found {}; using \"{default}\"",
            kind(item)
        )));
        return None;
    };
    if name.is_empty() || name.contains(['/', '\0']) {
        diagnostics.push(reject(format!(
            "`{path}` must be a theme name without `/`, found {name:?}; using \"{default}\""
        )));
        return None;
    }
    if name == SYSTEM_THEME && !accepts_system {
        diagnostics.push(reject(format!(
            "`{path}` must name a theme, not \"{SYSTEM_THEME}\"; using \"{default}\""
        )));
        return None;
    }
    Some(name.to_owned())
}

/// `font.family`: text; `None` (the chain) if its trimmed form is empty.
///
/// An empty name isn't an error: it is the writable way of saying "you pick the
/// family", without deleting the key. Whether the name exists on the machine
/// isn't asked here — it needs CoreText, not the file system, and `bt-atlas`
/// says it.
fn font_family(
    text: &str,
    item: &Item,
    fallback: &Option<String>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<String> {
    const KEY: &str = "font.family";
    let Some(name) = item.as_str() else {
        let using = match fallback {
            Some(name) => format!("\"{name}\""),
            None => "the default font".to_owned(),
        };
        diagnostics.push(Diagnostic {
            key: Some(KEY),
            line: item.span().and_then(|span| line_of(text, span.start)),
            message: format!(
                "`{KEY}` must be a string, found {}; using {using}",
                kind(item)
            ),
        });
        return fallback.clone();
    };
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// `font.size`: an integer or a decimal, finite and greater than zero.
///
/// There is **no** upper bound: clamping depends on `point size × scale` and is
/// silent in `bt-atlas` (`discussion.md` → Karar 4). Had there been a ceiling
/// here it would have had two owners and the diagnostic would come and go as the
/// window changes screens.
/// `font.line_height`: a multiplier between `1.0` and [`MAX_LINE_HEIGHT`].
///
/// **Two-ended**, unlike [`font_size`]. The lower end forbids going below the
/// font's own metrics (see [`FontOptions::line_height`]); the upper end isn't
/// arbitrary but a budget: every slot is `cell_w × cell_h` bytes and the atlas is
/// fixed-size, so as the multiplier grows the number of glyphs fitting the atlas
/// falls. Leaving it unbounded would have meant a terminal that falls to tofu
/// and the symptom would show only after a long session.
fn line_height(text: &str, item: &Item, fallback: f64, diagnostics: &mut Vec<Diagnostic>) -> f64 {
    ranged_float(
        text,
        item,
        "font.line_height",
        LINE_HEIGHT_RANGE,
        fallback,
        diagnostics,
    )
}

/// The **shared body** of a ranged decimal key: if it isn't a number or is out
/// of range the key stays at its own value and a diagnostic is left.
///
/// A separate function, because the same ~35 lines had been written **twice by
/// hand** in the repo (`line_height`, `font_size`) and 016 was bringing three
/// more keys — had the helper not been named, three more copies would have been
/// born (`/plan-review`).
///
/// **No clamping, rejection:** had an out-of-range value been silently pulled
/// to the end, the user would never see they wrote it wrong. That is the repo's
/// rule ([`Settings::parse_keeping`]).
///
/// `font_size` was **not moved**: it is one-ended (`> 0`) and its diagnostic text
/// doesn't fit this mold; forcing it would have spoiled the message.
fn ranged_float(
    text: &str,
    item: &Item,
    key: &'static str,
    range: std::ops::RangeInclusive<f64>,
    fallback: f64,
    diagnostics: &mut Vec<Diagnostic>,
) -> f64 {
    let line = item.span().and_then(|span| line_of(text, span.start));
    let reject = |message: String| Diagnostic {
        key: Some(key),
        line,
        message,
    };
    let value = match (item.as_float(), item.as_integer()) {
        (Some(value), _) => value,
        (None, Some(value)) => value as f64,
        (None, None) => {
            diagnostics.push(reject(format!(
                "`{key}` must be a number, found {}; using {fallback}",
                kind(item)
            )));
            return fallback;
        }
    };
    if !(value.is_finite() && range.contains(&value)) {
        diagnostics.push(reject(format!(
            "`{key}` must be a number between {} and {}, found {value}; using {fallback}",
            range.start(),
            range.end()
        )));
        return fallback;
    }
    value
}

/// The line-height multiplier's ceiling — the atlas budget (see [`line_height`]).
pub const MAX_LINE_HEIGHT: f64 = 2.0;

/// The line-height multiplier's accepted range: its lower end is the font's own
/// metrics ([`FontOptions::line_height`]), its upper end [`MAX_LINE_HEIGHT`].
pub const LINE_HEIGHT_RANGE: std::ops::RangeInclusive<f64> = 1.0..=MAX_LINE_HEIGHT;

fn font_size(text: &str, item: &Item, fallback: f64, diagnostics: &mut Vec<Diagnostic>) -> f64 {
    const KEY: &str = "font.size";
    let line = item.span().and_then(|span| line_of(text, span.start));
    let reject = |message: String| Diagnostic {
        key: Some(KEY),
        line,
        message,
    };
    // `as f64` isn't lossless for every integer but is lossless at every
    // meaningful point size; the magnitude where loss begins is far beyond
    // clamping anyway.
    let value = match (item.as_float(), item.as_integer()) {
        (Some(value), _) => value,
        (None, Some(value)) => value as f64,
        (None, None) => {
            diagnostics.push(reject(format!(
                "`{KEY}` must be a number, found {}; using {fallback}",
                kind(item)
            )));
            return fallback;
        }
    };
    if !(value.is_finite() && value > 0.0) {
        diagnostics.push(reject(format!(
            "`{KEY}` must be a number greater than 0, found {value}; using {fallback}"
        )));
        return fallback;
    }
    value
}

/// The **single body** of a named-option key: if it isn't one of the names in the
/// list the key stays at `fallback` and a diagnostic is left. Every string enum
/// (`clipboard.osc52` included) is read from here, and the diagnostic's "must be
/// …" list and "using …" value also come from the type's `NAMES` table — the
/// spelling is in one place, so the diagnostic can't suggest a value the parser
/// rejects.
///
/// A value that isn't accepted falling to `fallback`
/// ([`Settings::parse_keeping`]) is right for all of these keys, because the
/// symptom of a wrong guess is visible: the cursor's shape, its glide, the
/// scroll step. The one exception is `osc52` and the caller sets it up by
/// passing `Osc52::Off` — there a wrong guess is silent.
///
/// **Case-sensitive**: `"Hollow"` is a typo and silently accepting it would
/// mislead the user.
fn named_enum<T: Copy + PartialEq>(
    text: &str,
    item: &Item,
    key: &'static str,
    names: &'static [(&'static str, T)],
    fallback: T,
    diagnostics: &mut Vec<Diagnostic>,
) -> T {
    let found = match item.as_str() {
        Some(value) => {
            if let Some((_, picked)) = names.iter().find(|(name, _)| *name == value) {
                return *picked;
            }
            format!("{value:?}")
        }
        None => kind(item).to_owned(),
    };
    let expected = match names {
        [] => String::new(),
        [(one, _)] => format!("{one:?}"),
        [rest @ .., (last, _)] => format!(
            "{} or {last:?}",
            rest.iter()
                .map(|(name, _)| format!("{name:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    diagnostics.push(Diagnostic {
        key: Some(key),
        line: item.span().and_then(|span| line_of(text, span.start)),
        message: format!(
            "`{key}` must be {expected}, found {found}; using \"{}\"",
            name_in(names, fallback)
        ),
    });
    fallback
}

/// `[remote]`'s remote file keys (045 R8): every key present is read, a value
/// that isn't accepted takes `fallback`'s and leaves a diagnostic.
fn remote_files(
    text: &str,
    remote: &dyn TableLike,
    fallback: &RemoteFiles,
    diagnostics: &mut Vec<Diagnostic>,
) -> RemoteFiles {
    let mut files = RemoteFiles::default();
    if let Some(item) = remote.get("preview_max_size") {
        files.preview_max_size = size(
            text,
            item,
            "remote.preview_max_size",
            fallback.preview_max_size,
            diagnostics,
        );
    }
    if let Some(item) = remote.get("preview_read_only") {
        files.preview_read_only = boolean(
            text,
            item,
            "remote.preview_read_only",
            fallback.preview_read_only,
            diagnostics,
        );
    }
    if let Some(item) = remote.get("preview_dir") {
        files.preview_dir = folder(
            text,
            item,
            "remote.preview_dir",
            &fallback.preview_dir,
            diagnostics,
        );
    }
    if let Some(item) = remote.get("preview_keep") {
        files.preview_keep = named_enum(
            text,
            item,
            "remote.preview_keep",
            PreviewKeep::NAMES,
            fallback.preview_keep,
            diagnostics,
        );
    }
    if let Some(item) = remote.get("preview_limit") {
        files.preview_limit = size(
            text,
            item,
            "remote.preview_limit",
            fallback.preview_limit,
            diagnostics,
        );
    }
    if let Some(item) = remote.get("download_dir") {
        files.download_dir = folder(
            text,
            item,
            "remote.download_dir",
            &fallback.download_dir,
            diagnostics,
        );
    }
    if let Some(item) = remote.get("download_conflict") {
        files.download_conflict = named_enum(
            text,
            item,
            "remote.download_conflict",
            DownloadConflict::NAMES,
            fallback.download_conflict,
            diagnostics,
        );
    }
    if let Some(item) = remote.get("download_notify") {
        files.download_notify = boolean(
            text,
            item,
            "remote.download_notify",
            fallback.download_notify,
            diagnostics,
        );
    }
    files
}

/// A size key: a string like `"100MB"` ([`parse_size`]). An integer is not
/// accepted either — a bare number would leave the unit to guesswork.
fn size(
    text: &str,
    item: &Item,
    key: &'static str,
    fallback: u64,
    diagnostics: &mut Vec<Diagnostic>,
) -> u64 {
    if let Some(bytes) = item.as_str().and_then(parse_size) {
        return bytes;
    }
    let found = item
        .as_str()
        .map_or_else(|| kind(item).to_owned(), |value| format!("{value:?}"));
    diagnostics.push(Diagnostic {
        key: Some(key),
        line: item.span().and_then(|span| line_of(text, span.start)),
        message: format!(
            "`{key}` must be a size like \"100MB\" (B, KB, MB, GB or TB), found {found}; \
             using \"{}\"",
            format_size(fallback)
        ),
    });
    fallback
}

/// A switch key: `true` or `false` — a TOML boolean, not the string `"true"`.
fn boolean(
    text: &str,
    item: &Item,
    key: &'static str,
    fallback: bool,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    if let Some(value) = item.as_bool() {
        return value;
    }
    diagnostics.push(Diagnostic {
        key: Some(key),
        line: item.span().and_then(|span| line_of(text, span.start)),
        message: format!(
            "`{key}` must be true or false, found {}; using {fallback}",
            kind(item)
        ),
    });
    fallback
}

/// A folder key: an absolute path or one under the home directory (`~`,
/// `~/…`), kept as written ([`expand_home`] resolves it where it is used). A
/// relative path would depend on bateri's own directory (`/` from the Dock), and
/// `~user` would need a passwd lookup — both rejected, like NUL.
fn folder(
    text: &str,
    item: &Item,
    key: &'static str,
    fallback: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> String {
    let accepted = item.as_str().filter(|path| {
        !path.contains('\0') && (path.starts_with('/') || *path == "~" || path.starts_with("~/"))
    });
    if let Some(path) = accepted {
        return path.to_owned();
    }
    let found = item
        .as_str()
        .map_or_else(|| kind(item).to_owned(), |value| format!("{value:?}"));
    diagnostics.push(Diagnostic {
        key: Some(key),
        line: item.span().and_then(|span| line_of(text, span.start)),
        message: format!(
            "`{key}` must be a folder starting with / or ~/, found {found}; using \"{fallback}\""
        ),
    });
    fallback.to_owned()
}

/// `remote.hosts`: the array of `{ host, mark }` entries (037 Karar 2).
///
/// **A single broken entry rejects the whole key** and `fallback`'s list stays —
/// `parse_keeping`'s rule, without exception: dropping only the broken entry from
/// the list would change the order and could silently change a mark by bringing
/// an exact name behind a glob to the front. The diagnostic names the first
/// broken entry's line.
///
/// Both an inline array (`hosts = [{ … }]`) and an array of tables
/// (`[[remote.hosts]]`) are accepted: both write the same list and a user
/// writing TOML by hand can choose the second too.
fn host_rules(
    text: &str,
    item: &Item,
    fallback: &[HostRule],
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<HostRule> {
    const KEY: &str = "remote.hosts";
    let reject = |span: Option<std::ops::Range<usize>>, found: String| Diagnostic {
        key: Some(KEY),
        line: span.and_then(|span| line_of(text, span.start)),
        message: format!(
            "`{KEY}` must be a list of {{ host = \"pattern\", mark = \"production\", \
             \"staging\", \"development\", \"none\" or \"#rrggbb\" }}, found {found}; \
             keeping the previous list"
        ),
    };
    let mut entries: Vec<(&dyn TableLike, Option<std::ops::Range<usize>>)> = Vec::new();
    if let Some(array) = item.as_array() {
        for value in array {
            let Some(table) = value.as_inline_table() else {
                let found = "an entry that is not a { … } table".to_owned();
                diagnostics.push(reject(value.span(), found));
                return fallback.to_vec();
            };
            entries.push((table, value.span()));
        }
    } else if let Some(tables) = item.as_array_of_tables() {
        entries.extend(
            tables
                .iter()
                .map(|table| (table as &dyn TableLike, table.span())),
        );
    } else {
        diagnostics.push(reject(item.span(), kind(item).to_owned()));
        return fallback.to_vec();
    }
    let mut rules = Vec::with_capacity(entries.len());
    for (table, span) in entries {
        let pattern = table
            .get("host")
            .and_then(Item::as_str)
            .filter(|pattern| !pattern.is_empty());
        let mark = table.get("mark").and_then(Item::as_str).and_then(|mark| {
            HostMark::NAMES
                .iter()
                .find(|(name, _)| *name == mark)
                .map(|(_, named)| *named)
                .or_else(|| crate::theme::hex_color(mark).map(HostMark::Rgb))
        });
        match (pattern, mark) {
            (Some(pattern), Some(mark)) => rules.push(HostRule {
                pattern: pattern.to_owned(),
                mark,
            }),
            (None, _) => {
                diagnostics.push(reject(span, "an entry without a host".to_owned()));
                return fallback.to_vec();
            }
            (Some(pattern), None) => {
                let found = match table.get("mark").and_then(Item::as_str) {
                    Some(mark) => format!("mark {mark:?} for {pattern:?}"),
                    None => format!("no mark for {pattern:?}"),
                };
                diagnostics.push(reject(span, found));
                return fallback.to_vec();
            }
        }
    }
    rules
}

/// The value's spelling is in the `names` table. The tables carry every variant
/// (guard `every_name_reads_back_as_its_value`), an empty string would only be
/// the symptom of a missing table.
pub(crate) fn name_in<T: PartialEq>(names: &'static [(&'static str, T)], value: T) -> &'static str {
    names
        .iter()
        .find(|(_, named)| *named == value)
        .map_or("", |(name, _)| *name)
}

/// The 1-based line of a byte position.
///
/// `toml_edit`'s own translation (`translate_position`) is private to the crate;
/// the position the parser gives is always inside the text but `get` still turns
/// out-of-bounds into `None`, so there is no slicing panic.
pub(crate) fn line_of(text: &str, offset: usize) -> Option<usize> {
    let before = text.as_bytes().get(..offset)?;
    Some(before.iter().filter(|&&byte| byte == b'\n').count() + 1)
}

/// The part of the parser's message that fits the subtitle: the reason, without
/// the "expected" list.
///
/// `toml_edit` builds the message as "reason, expected a, b, …" and the list can
/// reach ten items (`a = "\q"`); it would be cut off in a one-line subtitle and
/// showing the user the line is enough anyway.
fn parser_reason(message: &str) -> &str {
    message
        .split_once(", expected")
        .map_or(message, |(reason, _)| reason)
}

/// The type of the value found, for the diagnostic text.
pub(crate) fn kind(item: &Item) -> &'static str {
    match item {
        Item::None => "nothing",
        Item::Table(_) => "a section",
        // `[[terminal]]`: an **array** of sections. Saying "must be a section,
        // found a section" wouldn't tell the user to turn `[[…]]` into `[…]`.
        Item::ArrayOfTables(_) => "an array of sections (`[[…]]`)",
        Item::Value(value) => match value {
            toml_edit::Value::String(_) => "a string",
            toml_edit::Value::Integer(_) => "an integer",
            toml_edit::Value::Float(_) => "a float",
            toml_edit::Value::Boolean(_) => "a boolean",
            toml_edit::Value::Datetime(_) => "a date",
            toml_edit::Value::Array(_) => "an array",
            toml_edit::Value::InlineTable(_) => "a section",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(text: &str) -> Settings {
        let parsed = Settings::parse(text).expect("parseable text");
        assert_eq!(
            parsed.diagnostics,
            Vec::new(),
            "no diagnostic expected: {text}"
        );
        parsed.settings
    }

    fn rejected(text: &str) -> (Settings, Diagnostic) {
        let parsed = Settings::parse(text).expect("parseable text");
        let [diagnostic] = <[Diagnostic; 1]>::try_from(parsed.diagnostics)
            .unwrap_or_else(|got| panic!("a single diagnostic expected: {got:?}"));
        (parsed.settings, diagnostic)
    }

    #[test]
    fn empty_file_is_default() {
        assert_eq!(clean(""), Settings::default());
        assert_eq!(clean("# yalnız yorum\n\n"), Settings::default());
    }

    #[test]
    fn template_is_the_defaults() {
        // The file "Settings…" creates mustn't change today's behavior: no
        // diagnostic and the defaults themselves. If a default changes and the
        // template doesn't, this fails here.
        assert_eq!(clean(Settings::TEMPLATE), Settings::default());

        // Every key that has a default is **written**, not in a comment: the user
        // changes the value in place and choosing a theme from the menu writes the
        // line in place. An empty template would pass the equality above too.
        let doc = document(Settings::TEMPLATE).expect("template TOML");
        for (section, key) in [
            ("terminal", "scrollback"),
            ("terminal", "cursor"),
            ("terminal", "cursor_blink"),
            ("terminal", "cursor_radius"),
            ("terminal", "cursor_glow"),
            ("terminal", "cursor_unfocused"),
            ("terminal", "cursor_blink_interval"),
            ("terminal", "confirm_close"),
            ("appearance", "theme"),
            ("appearance", "light_theme"),
            ("appearance", "dark_theme"),
            ("font", "size"),
            ("font", "line_height"),
            ("clipboard", "osc52"),
            ("motion", "cursor_motion"),
            ("motion", "reduce_motion"),
            ("motion", "smooth_scroll"),
            ("motion", "keypress"),
            ("motion", "erase"),
            ("shell", "integration"),
            ("remote", "hosts"),
            ("remote", "preview_max_size"),
            ("remote", "preview_read_only"),
            ("remote", "preview_dir"),
            ("remote", "preview_keep"),
            ("remote", "preview_limit"),
            ("remote", "download_dir"),
            ("remote", "download_conflict"),
            ("remote", "download_notify"),
        ] {
            assert!(
                doc.get(section).and_then(|s| s.get(key)).is_some(),
                "{section}.{key} is missing from the template"
            );
        }

        // `family`, which has no default, is an example in a comment; a user who
        // uncomments it must find a valid value.
        let uncommented = Settings::TEMPLATE.replace("# family = ", "family = ");
        assert_ne!(
            uncommented,
            Settings::TEMPLATE,
            "no family example in the template"
        );
        assert!(clean(&uncommented).font.family.is_some());
    }

    #[test]
    fn documented_template_is_the_template() {
        // `docs/AYARLAR.md` shows the template as it is; a copy would drift.
        let doc = include_str!("../../../docs/AYARLAR.md");
        let (_, after) = doc
            .split_once("### Şablon\n")
            .expect("no template heading in AYARLAR.md");
        let (_, block) = after
            .split_once("```toml\n")
            .expect("no toml block under the heading");
        let (block, _) = block
            .split_once("```")
            .expect("the toml block doesn't close");
        assert_eq!(block, Settings::TEMPLATE);
    }

    #[test]
    fn scrollback_is_read() {
        assert_eq!(clean("[terminal]\nscrollback = 500\n").scrollback, 500);
        // An inline table is the same section.
        assert_eq!(clean("terminal = { scrollback = 0 }").scrollback, 0);
        assert_eq!(
            clean(&format!("[terminal]\nscrollback = {SCROLLBACK_MAX}")).scrollback,
            SCROLLBACK_MAX
        );
    }

    #[test]
    fn wrong_type_falls_back_to_default_with_diagnostic() {
        let (settings, diagnostic) = rejected("[terminal]\n\nscrollback = \"çok\"\n");
        assert_eq!(settings, Settings::default());
        assert_eq!(diagnostic.key, Some("terminal.scrollback"));
        assert_eq!(diagnostic.line, Some(3));
        assert_eq!(
            diagnostic.to_string(),
            "line 3: `terminal.scrollback` must be an integer, found a string; using 10000"
        );

        let (settings, diagnostic) = rejected("[terminal]\nscrollback = 1.5\n");
        assert_eq!(settings, Settings::default());
        assert!(diagnostic.message.contains("a float"), "{diagnostic}");

        let (settings, diagnostic) = rejected("[terminal]\nscrollback = -1\n");
        assert_eq!(settings, Settings::default());
        assert!(diagnostic.message.contains("negative"), "{diagnostic}");
    }

    #[test]
    fn scrollback_over_the_ceiling_is_clamped_with_diagnostic() {
        let (settings, diagnostic) = rejected("[terminal]\nscrollback = 1000000\n");
        assert_eq!(settings.scrollback, SCROLLBACK_MAX);
        assert_eq!(diagnostic.key, Some("terminal.scrollback"));
        assert_eq!(diagnostic.line, Some(2));
    }

    #[test]
    fn theme_name_is_read() {
        assert_eq!(clean("[appearance]\ntheme = \"paper\"\n").theme, "paper");
        // The default is to follow the system, the pair is the embedded themes.
        let defaults = clean("");
        assert_eq!(
            (
                defaults.theme.as_str(),
                defaults.light_theme.as_str(),
                defaults.dark_theme.as_str()
            ),
            ("system", "bateri-light", "bateri")
        );
        // The section can be written inline too; a neighboring section doesn't
        // break reading.
        let settings = clean("appearance = { theme = \"a b.c\" }\n[terminal]\nscrollback = 3\n");
        assert_eq!((settings.theme.as_str(), settings.scrollback), ("a b.c", 3));

        let settings = clean("[appearance]\nlight_theme = \"paper\"\ndark_theme = \"ink\"\n");
        assert_eq!(
            (
                settings.theme.as_str(),
                settings.light_theme.as_str(),
                settings.dark_theme.as_str()
            ),
            ("system", "paper", "ink")
        );
    }

    #[test]
    fn theme_follows_the_appearance_only_when_system() {
        let pair = Settings {
            light_theme: "paper".to_owned(),
            dark_theme: "ink".to_owned(),
            ..Settings::default()
        };
        assert_eq!(pair.theme_for(true), "ink");
        assert_eq!(pair.theme_for(false), "paper");
        assert!(pair.follows_system());
        // A fixed name is independent of the appearance; even if the pair stays in
        // place it isn't read.
        let fixed = Settings {
            theme: "bateri".to_owned(),
            ..pair
        };
        assert_eq!(fixed.theme_for(true), "bateri");
        assert_eq!(fixed.theme_for(false), "bateri");
        assert!(!fixed.follows_system());
    }

    #[test]
    fn unchanged_settings_have_no_changes() {
        // On every save the whole file is re-read; the same text must give an
        // empty difference, otherwise every save would rebuild the history and
        // request a frame.
        let text = "[terminal]\nscrollback = 500\n[appearance]\ntheme = \"paper\"\n";
        assert_eq!(clean(text).changes(&clean(text)), Changes::default());
        assert_eq!(
            Settings::default().changes(&Settings::default()),
            Changes::default()
        );
    }

    #[test]
    fn scrollback_change_is_a_terminal_change() {
        let before = clean("[terminal]\nscrollback = 500\n");
        let after = clean("[terminal]\nscrollback = 20\n");
        assert_eq!(
            before.changes(&after),
            Changes {
                terminal: true,
                font: false,
                motion: false,
                caret: false,
                remote: false,
            }
        );
        assert_eq!(
            after.terminal(),
            TerminalOptions {
                scrollback: 20,
                osc52: Osc52::Copy,
                cursor: CaretShape::default(),
                blink: CursorBlink::default(),
            }
        );
        // Theme names aren't a terminal option: the theme is re-resolved on every
        // save (`bt-shell`), the difference can't gate it.
        let themed = clean("[terminal]\nscrollback = 500\n[appearance]\ntheme = \"paper\"\n");
        assert_eq!(before.changes(&themed), Changes::default());
    }

    #[test]
    fn rejected_values_keep_the_given_settings() {
        // The save-time rule (a `/code-review` finding): had a wrong-typed save of
        // `scrollback` fallen to the default (ten thousand), a hundred-thousand
        // history would have been irreversibly truncated at that moment. A value
        // that isn't accepted takes the given settings' and the diagnostic says
        // **that**.
        let current = Settings {
            scrollback: 100_000,
            cursor: CaretShape::default(),
            cursor_blink: CursorBlink::default(),
            caret: CaretStyle::default(),
            blink_interval: CURSOR_BLINK_INTERVAL,
            theme: "paper".to_owned(),
            light_theme: "chalk".to_owned(),
            dark_theme: "ink".to_owned(),
            font: FontOptions::default(),
            osc52: Osc52::Copy,
            cursor_motion: CursorMotion::Spring,
            reduce_motion: ReduceMotion::System,
            smooth_scroll: SmoothScroll::On,
            keypress: Keypress::Fade,
            erase: Erase::Recede,
            shell_integration: ShellIntegration::Auto,
            confirm_close: ConfirmClose::Always,
            remote_hosts: Vec::new(),
            remote_files: RemoteFiles::default(),
        };
        let parsed = Settings::parse_keeping(
            "[terminal]\nscrollback = \"100000\"\n[appearance]\ntheme = 3\n",
            &current,
        )
        .expect("parseable text");
        assert_eq!(parsed.settings.scrollback, 100_000);
        assert_eq!(parsed.settings.theme, "paper");
        assert_eq!(
            parsed
                .diagnostics
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>(),
            [
                "`terminal.scrollback` must be an integer, found a string; using 100000",
                "`appearance.theme` must be a string, found an integer; using \"paper\"",
            ]
        );
        // A key **not in** the file is still the default: the file says nothing,
        // and there is no value that isn't accepted either.
        assert_eq!(parsed.settings.light_theme, "bateri-light");
        assert_eq!(parsed.settings.dark_theme, "bateri");

        // A value above the ceiling is clamped to the ceiling, not to the given
        // one: the intent is clear.
        let parsed = Settings::parse_keeping("[terminal]\nscrollback = 1000000\n", &current)
            .expect("parseable text");
        assert_eq!(parsed.settings.scrollback, SCROLLBACK_MAX);

        // The section has the wrong type: all of the section's keys count as not
        // accepted.
        let parsed = Settings::parse_keeping("terminal = 5\nappearance = 1\n", &current)
            .expect("parseable text");
        assert_eq!(parsed.settings, current);
        assert_eq!(parsed.diagnostics.len(), 2);

        // `parse` is the startup rule: the same text falls to the default.
        assert_eq!(
            Settings::parse("terminal = 5\nappearance = 1\n")
                .expect("parseable text")
                .settings,
            Settings::default()
        );
    }

    #[test]
    fn font_is_read() {
        // If not in the file, the chain and 13 points.
        assert_eq!(
            clean("").font,
            FontOptions {
                family: None,
                size: 13.0,
                line_height: 1.0
            }
        );
        assert_eq!(
            clean("[font]\nfamily = \"Monaco\"\nsize = 14.5\n").font,
            FontOptions {
                family: Some("Monaco".to_owned()),
                size: 14.5,
                line_height: 1.0
            }
        );
        // An integer is a point size too: the first thing the user will write is
        // `size = 14`.
        assert_eq!(clean("[font]\nsize = 14\n").font.size, 14.0);
        // An empty family (whitespace alone too) means the chain, not an error;
        // the name is trimmed.
        assert_eq!(clean("[font]\nfamily = \"\"\n").font.family, None);
        assert_eq!(clean("[font]\nfamily = \"  \"\n").font.family, None);
        assert_eq!(
            clean("font = { family = \" Menlo \" }\n").font.family,
            Some("Menlo".to_owned())
        );
    }

    #[test]
    fn broken_font_size_falls_back_with_diagnostic() {
        // TOML accepts `nan` and `inf` as numbers; the type check lets them
        // through, the rule mustn't. Negative and zero point sizes too.
        for (value, found) in [
            ("-1", "-1"),
            ("0", "0"),
            ("0.0", "0"),
            ("-2.5", "-2.5"),
            ("nan", "NaN"),
            ("inf", "inf"),
        ] {
            let (settings, diagnostic) = rejected(&format!("[font]\nsize = {value}\n"));
            assert_eq!(settings, Settings::default(), "{value}");
            assert_eq!(diagnostic.key, Some("font.size"), "{value}");
            assert_eq!(diagnostic.line, Some(2), "{value}");
            assert_eq!(
                diagnostic.message,
                format!("`font.size` must be a number greater than 0, found {found}; using 13")
            );
        }
        let (settings, diagnostic) = rejected("[font]\nsize = \"14\"\n");
        assert_eq!(settings, Settings::default());
        assert_eq!(
            diagnostic.message,
            "`font.size` must be a number, found a string; using 13"
        );

        let (settings, diagnostic) = rejected("[font]\nfamily = 3\n");
        assert_eq!(settings, Settings::default());
        assert_eq!(diagnostic.key, Some("font.family"));
        assert_eq!(
            diagnostic.message,
            "`font.family` must be a string, found an integer; using the default font"
        );
    }

    #[test]
    fn rejected_font_values_keep_the_given_settings() {
        // The save-time rule applies to the font too: a half-saved point size
        // mustn't slam the window to the default and bring it back.
        let current = Settings {
            font: FontOptions {
                family: Some("Monaco".to_owned()),
                size: 18.0,
                line_height: 1.0,
            },
            ..Settings::default()
        };
        let parsed = Settings::parse_keeping("[font]\nfamily = false\nsize = 0\n", &current)
            .expect("parseable text");
        assert_eq!(parsed.settings.font, current.font);
        assert_eq!(
            parsed
                .diagnostics
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>(),
            [
                "`font.family` must be a string, found a boolean; using \"Monaco\"",
                "`font.size` must be a number greater than 0, found 0; using 18",
            ]
        );
        let parsed = Settings::parse_keeping("font = 5\n", &current).expect("parseable text");
        assert_eq!(parsed.settings, current);
        assert_eq!(parsed.diagnostics.len(), 1);
    }

    #[test]
    fn font_change_is_a_font_change() {
        let before = clean("[font]\nsize = 13\n");
        let font_only = Changes {
            terminal: false,
            font: true,
            motion: false,
            caret: false,
            remote: false,
        };
        assert_eq!(before.changes(&clean("[font]\nsize = 14\n")), font_only);
        assert_eq!(
            before.changes(&clean("[font]\nfamily = \"Monaco\"\nsize = 13\n")),
            font_only
        );
        // A default written explicitly isn't a difference: saving doesn't make the
        // atlas be rebuilt.
        assert_eq!(Settings::default().changes(&before), Changes::default());
    }

    #[test]
    fn osc52_is_read() {
        // On if not in the file: vim over ssh having its copy work out of the box.
        assert_eq!(clean("").osc52, Osc52::Copy);
        assert_eq!(clean("[clipboard]\nosc52 = \"copy\"\n").osc52, Osc52::Copy);
        assert_eq!(clean("[clipboard]\nosc52 = \"off\"\n").osc52, Osc52::Off);
        assert_eq!(clean("clipboard = { osc52 = \"off\" }\n").osc52, Osc52::Off);
    }

    #[test]
    fn unrecognized_osc52_is_off_with_diagnostic() {
        // Falls to off: the read direction's name, an uppercase spelling, `false`,
        // a number and a section — none returns to the default (on).
        for (value, found) in [
            ("\"paste\"", "\"paste\""),
            ("\"Copy\"", "\"Copy\""),
            ("false", "a boolean"),
            ("1", "an integer"),
            ("{ mode = \"copy\" }", "a section"),
        ] {
            let (settings, diagnostic) = rejected(&format!("[clipboard]\nosc52 = {value}\n"));
            assert_eq!(
                settings,
                Settings {
                    osc52: Osc52::Off,
                    ..Settings::default()
                },
                "{value}"
            );
            assert_eq!(diagnostic.key, Some("clipboard.osc52"), "{value}");
            assert_eq!(diagnostic.line, Some(2), "{value}");
            assert_eq!(
                diagnostic.message,
                format!(
                    "`clipboard.osc52` must be \"copy\" or \"off\", found {found}; using \"off\""
                )
            );
        }
        // The section has the wrong type: its key counts as not accepted, off
        // again.
        let (settings, diagnostic) = rejected("clipboard = \"copy\"\n");
        assert_eq!(settings.osc52, Osc52::Off);
        assert_eq!(diagnostic.key, Some("clipboard"));
    }

    #[test]
    fn rejected_osc52_is_off_even_when_the_given_settings_copy() {
        // The only exception to the save-time "keep the current value" rule: a
        // turn-off mistyped as `"of"` mustn't keep the clipboard on.
        let current = Settings::default();
        assert_eq!(current.osc52, Osc52::Copy);
        for text in ["[clipboard]\nosc52 = \"of\"\n", "clipboard = 5\n"] {
            let parsed = Settings::parse_keeping(text, &current).expect("parseable text");
            assert_eq!(parsed.settings.osc52, Osc52::Off, "{text}");
            assert_eq!(parsed.diagnostics.len(), 1, "{text}");
        }
        // A key deleted from the file returns to the default: there is no value
        // that isn't accepted, the user wants the default.
        let off = Settings {
            osc52: Osc52::Off,
            ..Settings::default()
        };
        let parsed = Settings::parse_keeping("", &off).expect("parseable text");
        assert_eq!(parsed.settings.osc52, Osc52::Copy);
    }

    #[test]
    fn osc52_change_is_a_terminal_change() {
        // A live change goes with **all** of the terminal options: `scrollback`
        // is carried alongside, it doesn't pull it to the default.
        let before = clean("[terminal]\nscrollback = 500\n");
        let after = clean("[terminal]\nscrollback = 500\n[clipboard]\nosc52 = \"off\"\n");
        assert_eq!(
            before.changes(&after),
            Changes {
                terminal: true,
                font: false,
                motion: false,
                caret: false,
                remote: false,
            }
        );
        assert_eq!(
            after.terminal(),
            TerminalOptions {
                scrollback: 500,
                osc52: Osc52::Off,
                cursor: CaretShape::default(),
                blink: CursorBlink::default(),
            }
        );
    }

    #[test]
    fn cursor_shape_is_read() {
        // If not in the file, `block`: the same as alacritty's default, so the
        // behavior before the setting arrived is preserved exactly.
        assert_eq!(clean("").cursor, CaretShape::Block);
        assert_eq!(
            clean("[terminal]\ncursor = \"underline\"\n").cursor,
            CaretShape::Underline
        );
        // An inline table is the same section.
        assert_eq!(
            clean("terminal = { cursor = \"beam\" }").cursor,
            CaretShape::Beam
        );
        // An unrecognized value **doesn't change** the key and leaves a
        // diagnostic; case-sensitive.
        for text in [
            "[terminal]\ncursor = \"bar\"\n",
            "[terminal]\ncursor = \"Block\"\n",
        ] {
            let (settings, diagnostic) = rejected(text);
            assert_eq!(settings.cursor, CaretShape::Block, "{text}");
            assert_eq!(diagnostic.key, Some("terminal.cursor"), "{text}");
        }
        // If the section **isn't a section** (e.g. `terminal = 1`) the key falls
        // to fallback — the same as `scrollback`'s second arm. The diagnostic
        // belongs to the section itself, not the key.
        let (settings, diagnostic) = rejected("terminal = 1");
        assert_eq!(settings.cursor, CaretShape::Block);
        assert_eq!(diagnostic.key, Some("terminal"));
    }

    #[test]
    fn cursor_blink_is_read() {
        // `off` if not in the file: a blinking cursor keeps the window
        // permanently busy and that must be something the user **chose**.
        assert_eq!(clean("").cursor_blink, CursorBlink::Off);
        assert_eq!(
            clean("[terminal]\ncursor_blink = \"auto\"\n").cursor_blink,
            CursorBlink::Auto
        );
        assert_eq!(
            clean("terminal = { cursor_blink = \"on\" }").cursor_blink,
            CursorBlink::On
        );
        let (settings, diagnostic) = rejected("[terminal]\ncursor_blink = \"yes\"\n");
        assert_eq!(settings.cursor_blink, CursorBlink::Off);
        assert_eq!(diagnostic.key, Some("terminal.cursor_blink"));
    }

    #[test]
    fn the_blink_setting_overrides_what_the_program_asks() {
        // `auto` follows the application; the others **override** and both
        // directions of overriding are a must: vim sending `\e[5 q` mustn't break
        // through `off`, and a program sending `\e[2 q` mustn't silence `on`.
        assert!(CursorBlink::Auto.resolve(true));
        assert!(!CursorBlink::Auto.resolve(false));
        assert!(CursorBlink::On.resolve(false), "on ezmedi");
        assert!(!CursorBlink::Off.resolve(true), "off ezmedi");
    }

    #[test]
    fn cursor_motion_is_read() {
        // `spring` if not in the file: not shipping the feature off is the
        // decision itself (008 Karar 6).
        assert_eq!(clean("").cursor_motion, CursorMotion::Spring);
        assert_eq!(
            clean("[motion]\ncursor_motion = \"snap\"\n").cursor_motion,
            CursorMotion::Snap
        );
        assert_eq!(
            clean("[motion]\ncursor_motion = \"ease\"\n").cursor_motion,
            CursorMotion::Ease
        );
        assert_eq!(
            clean("motion = { cursor_motion = \"spring\" }\n").cursor_motion,
            CursorMotion::Spring
        );
    }

    #[test]
    fn unrecognized_cursor_motion_keeps_its_own_key() {
        // `osc52`'s "a value that isn't accepted falls to off" exception does
        // **not** pass here: the cost of a wrong guess is a visible animation, not
        // a silent clipboard leak (008 Karar 6). So the rule is the other keys':
        // the key stays at its own value, with a diagnostic beside it.
        for (value, found) in [
            ("\"sprong\"", "\"sprong\""),
            ("\"Spring\"", "\"Spring\""),
            ("true", "a boolean"),
            ("{ style = \"snap\" }", "a section"),
        ] {
            let (settings, diagnostic) = rejected(&format!("[motion]\ncursor_motion = {value}\n"));
            assert_eq!(settings, Settings::default(), "{value}");
            assert_eq!(diagnostic.key, Some("motion.cursor_motion"), "{value}");
            assert_eq!(diagnostic.line, Some(2), "{value}");
            assert_eq!(
                diagnostic.message,
                format!(
                    "`motion.cursor_motion` must be \"snap\", \"ease\" or \"spring\", \
found {found}; using \"spring\""
                )
            );
        }
        // The value that stands in at save time is the **current** setting, not
        // the default: the style on screen mustn't change because of a typo.
        let current = Settings {
            cursor_motion: CursorMotion::Ease,
            ..Settings::default()
        };
        let parsed = Settings::parse_keeping("[motion]\ncursor_motion = \"sprong\"\n", &current)
            .expect("parseable text");
        assert_eq!(parsed.settings.cursor_motion, CursorMotion::Ease);
        assert!(parsed.diagnostics[0].message.ends_with("using \"ease\""));
        // The section has the wrong type: its key counts as not accepted.
        let parsed = Settings::parse_keeping("motion = 5\n", &current).expect("parseable text");
        assert_eq!(parsed.settings.cursor_motion, CursorMotion::Ease);
        assert_eq!(parsed.diagnostics[0].key, Some("motion"));
    }

    #[test]
    fn cursor_motion_change_is_a_motion_change() {
        // Its own difference: the style goes to the renderer's rhythm, not to the
        // session or the font — it mustn't move either.
        let before = clean("");
        let after = clean("[motion]\ncursor_motion = \"snap\"\n");
        assert_eq!(
            before.changes(&after),
            Changes {
                terminal: false,
                font: false,
                motion: true,
                caret: false,
                remote: false,
            }
        );
        assert_eq!(after.changes(&after), Changes::default());
    }

    #[test]
    fn reduce_motion_is_read() {
        // `system` if not in the file: the most likely choice is "follow the
        // system" and that is the reason the key is three-valued (`ReduceMotion`).
        assert_eq!(clean("").reduce_motion, ReduceMotion::System);
        assert_eq!(
            clean("[motion]\nreduce_motion = \"on\"\n").reduce_motion,
            ReduceMotion::On
        );
        assert_eq!(
            clean("[motion]\nreduce_motion = \"off\"\n").reduce_motion,
            ReduceMotion::Off
        );
        assert_eq!(
            clean("motion = { reduce_motion = \"system\" }\n").reduce_motion,
            ReduceMotion::System
        );
        // The two keys don't override each other: both are read in the same
        // section.
        let both = clean("[motion]\ncursor_motion = \"ease\"\nreduce_motion = \"on\"\n");
        assert_eq!(both.cursor_motion, CursorMotion::Ease);
        assert_eq!(both.reduce_motion, ReduceMotion::On);
    }

    #[test]
    fn unrecognized_reduce_motion_keeps_its_own_key() {
        // The same rule as `cursor_motion`: the key stays at its own value, with a
        // diagnostic beside it — and **only its own** key is affected.
        for (value, found) in [
            ("\"yes\"", "\"yes\""),
            ("\"System\"", "\"System\""),
            ("true", "a boolean"),
        ] {
            let text = format!("[motion]\ncursor_motion = \"snap\"\nreduce_motion = {value}\n");
            let (settings, diagnostic) = rejected(&text);
            assert_eq!(
                settings,
                Settings {
                    cursor_motion: CursorMotion::Snap,
                    ..Settings::default()
                },
                "{value}"
            );
            assert_eq!(diagnostic.key, Some("motion.reduce_motion"), "{value}");
            assert_eq!(diagnostic.line, Some(3), "{value}");
            assert_eq!(
                diagnostic.message,
                format!(
                    "`motion.reduce_motion` must be \"system\", \"on\" or \"off\", \
found {found}; using \"system\""
                )
            );
        }
        // The value that stands in at save time is the **current** setting, not
        // the default.
        let current = Settings {
            reduce_motion: ReduceMotion::Off,
            ..Settings::default()
        };
        let parsed = Settings::parse_keeping("[motion]\nreduce_motion = \"yes\"\n", &current)
            .expect("parseable text");
        assert_eq!(parsed.settings.reduce_motion, ReduceMotion::Off);
        assert!(parsed.diagnostics[0].message.ends_with("using \"off\""));
        // The section has the wrong type: both keys count as not accepted.
        let parsed = Settings::parse_keeping("motion = 5\n", &current).expect("parseable text");
        assert_eq!(parsed.settings.reduce_motion, ReduceMotion::Off);
    }

    #[test]
    fn reduce_motion_change_is_a_motion_change() {
        // It lands in the **same** difference as `cursor_motion`: both go to the
        // same place, at the same call site (`Changes::motion`).
        let before = clean("");
        let after = clean("[motion]\nreduce_motion = \"on\"\n");
        assert_eq!(
            before.changes(&after),
            Changes {
                terminal: false,
                font: false,
                motion: true,
                caret: false,
                remote: false,
            }
        );
        assert_eq!(after.changes(&after), Changes::default());
    }

    #[test]
    fn shell_integration_is_read() {
        assert_eq!(clean("").shell_integration, ShellIntegration::Auto);
        assert_eq!(
            clean("[shell]\nintegration = \"auto\"\n").shell_integration,
            ShellIntegration::Auto
        );
        assert_eq!(
            clean("[shell]\nintegration = \"off\"\n").shell_integration,
            ShellIntegration::Off
        );
        assert_eq!(
            clean("shell = { integration = \"off\" }\n").shell_integration,
            ShellIntegration::Off
        );
    }

    #[test]
    fn unrecognized_shell_integration_keeps_its_own_key() {
        // The same rule as `cursor_motion`: the key stays at its own value, with a
        // diagnostic beside it. `osc52`'s "fall to off" exception doesn't pass
        // here.
        for (value, found) in [
            ("\"on\"", "\"on\""),
            ("\"Auto\"", "\"Auto\""),
            ("false", "a boolean"),
        ] {
            let (settings, diagnostic) = rejected(&format!("[shell]\nintegration = {value}\n"));
            assert_eq!(settings, Settings::default(), "{value}");
            assert_eq!(diagnostic.key, Some("shell.integration"), "{value}");
            assert_eq!(diagnostic.line, Some(2), "{value}");
            assert_eq!(
                diagnostic.message,
                format!(
                    "`shell.integration` must be \"auto\", \"blocks\" or \"off\", \
found {found}; using \"auto\""
                )
            );
        }
        // The value that stands in at save time is the **current** setting, not
        // the default.
        let current = Settings {
            shell_integration: ShellIntegration::Off,
            ..Settings::default()
        };
        let parsed = Settings::parse_keeping("[shell]\nintegration = \"on\"\n", &current)
            .expect("parseable text");
        assert_eq!(parsed.settings.shell_integration, ShellIntegration::Off);
        assert!(parsed.diagnostics[0].message.ends_with("using \"off\""));
        // The section has the wrong type: the key counts as not accepted too.
        let parsed = Settings::parse_keeping("shell = 5\n", &current).expect("parseable text");
        assert_eq!(parsed.settings.shell_integration, ShellIntegration::Off);
    }

    #[test]
    fn shell_integration_is_not_a_live_change() {
        // The contract's only exception and its test is here: even if the key
        // changes `Changes` stays empty, because the shell is already born and
        // there is nothing to apply. If an arm is ever attached to `Changes` this
        // will go red and `docs/AYARLAR.md`'s "takes effect in the next session"
        // sentence will have to be corrected too.
        let before = clean("");
        let after = clean("[shell]\nintegration = \"off\"\n");
        assert_ne!(before.shell_integration, after.shell_integration);
        assert_eq!(before.changes(&after), Changes::default());
    }

    #[test]
    fn caret_style_is_read_and_bounded() {
        // **The default is today's look:** a user with no file sees the cursor 015
        // shipped and `bt-gpu` imports the same constants (016 R2) — had there been
        // two literals the pixel guards would have stayed blind.
        assert_eq!(clean("").caret, CaretStyle::default());
        assert_eq!(
            (
                CaretStyle::default().radius_ratio,
                CaretStyle::default().glow
            ),
            (CURSOR_RADIUS, CURSOR_GLOW)
        );

        let read = clean("[terminal]\ncursor_radius = 0.3\ncursor_glow = 0\n").caret;
        assert_eq!((read.radius_ratio, read.glow), (0.3, 0.0));
        // An integer is valid too: the `cursor_glow = 2` the user will write.
        assert_eq!(clean("[terminal]\ncursor_glow = 2\n").caret.glow, 2.0);
    }

    #[test]
    fn a_rejected_caret_number_keeps_its_own_key() {
        // **No clamping, rejection** (016 R1.3): had an out-of-range value been
        // silently pulled to the end, the user would never see they wrote it
        // wrong. The rejected key stays at its own default, **its neighbor is
        // read**.
        for value in ["1.5", "-0.1", "\"big\""] {
            let (settings, diagnostic) = rejected(&format!(
                "[terminal]\ncursor_radius = {value}\ncursor_glow = 2.0\n"
            ));
            assert_eq!(
                settings.caret.radius_ratio, CURSOR_RADIUS,
                "{value} changed its own key"
            );
            assert_eq!(
                settings.caret.glow, 2.0,
                "{value} dropped the neighboring key too"
            );
            assert_eq!(diagnostic.key, Some("terminal.cursor_radius"), "{value}");
        }
    }

    #[test]
    fn the_unfocused_caret_key_is_read_and_diagnosed() {
        assert_eq!(clean("").caret.unfocused, UnfocusedCaret::Hollow);
        assert_eq!(
            clean("[terminal]\ncursor_unfocused = \"solid\"\n")
                .caret
                .unfocused,
            UnfocusedCaret::Solid
        );

        // Case-sensitive, and a value that isn't accepted doesn't change its own
        // key; the expected list must be written **in the diagnostic**, otherwise
        // the user has to look up the right spelling in the file.
        let (settings, diagnostic) = rejected("[terminal]\ncursor_unfocused = \"Solid\"\n");
        assert_eq!(settings.caret.unfocused, UnfocusedCaret::Hollow);
        assert_eq!(diagnostic.key, Some("terminal.cursor_unfocused"));
        assert!(
            diagnostic.message.contains("\"hollow\" or \"solid\""),
            "the diagnostic doesn't list the expected values: {}",
            diagnostic.message
        );
    }

    #[test]
    fn a_rejected_caret_number_names_the_value_the_user_kept() {
        // **The diagnostic text must show the number the user wrote**, not float
        // noise (`/code-review`): when `CaretStyle` was `f32`, `f64::from(0.10f32)`
        // came to `0.10000000149011612` and the message said
        // "using 0.10000000149011612". The sibling tests (font, theme) check the
        // whole message; this key looked only at `key` and the defect leaked from
        // there.
        let (_, diagnostic) = rejected("[terminal]\ncursor_radius = 1.5\n");
        assert_eq!(
            diagnostic.message,
            "`terminal.cursor_radius` must be a number between 0 and 0.5, \
found 1.5; using 0.1"
        );
        let (_, diagnostic) = rejected("[terminal]\ncursor_glow = 9\n");
        assert_eq!(
            diagnostic.message,
            "`terminal.cursor_glow` must be a number between 0 and 3, found 9; using 1"
        );
    }

    #[test]
    fn a_caret_change_is_its_own_field() {
        // `changes.terminal` today is exactly `terminal() != terminal()` and these
        // two keys **don't enter** `TerminalOptions`; had they shared the field, a
        // radius change would have made the session be rebuilt from scratch.
        let before = clean("");
        let after = clean("[terminal]\ncursor_radius = 0.2\n");
        let changes = before.changes(&after);
        assert!(changes.caret, "the cursor difference wasn't seen");
        assert!(!changes.terminal, "the radius makes the session rebuild");
        assert!(!changes.font && !changes.motion);

        // The reverse direction: when a terminal key changes the cursor field
        // doesn't move.
        let scrolled = clean("[terminal]\nscrollback = 50\n");
        let changes = before.changes(&scrolled);
        assert!(changes.terminal && !changes.caret, "{changes:?}");
    }

    #[test]
    fn line_height_is_read_and_bounded() {
        assert_eq!(
            clean("").font.line_height,
            1.0,
            "the default is the font's own"
        );
        assert_eq!(clean("[font]\nline_height = 1.4\n").font.line_height, 1.4);
        // An integer is a multiplier too: the `line_height = 2` the user will write.
        assert_eq!(clean("[font]\nline_height = 2\n").font.line_height, 2.0);

        // **Two-ended, and the two ends have separate reasons.** The lower end
        // protects a guard: a cell shorter than the font wants would clip the tails
        // of `g` and `y` (`descender_fits_in_the_cell` in `bt-atlas`). The upper
        // end is a budget: a slot is `cell_w × cell_h` bytes and the atlas is
        // fixed-size, so as the multiplier grows the number of fitting glyphs
        // falls.
        for value in ["0.9", "0", "-1", "2.5", "1e9"] {
            let (settings, diagnostic) = rejected(&format!("[font]\nline_height = {value}\n"));
            assert_eq!(settings, Settings::default(), "{value}");
            assert_eq!(diagnostic.key, Some("font.line_height"), "{value}");
            assert!(
                diagnostic.message.contains("between 1 and 2"),
                "{value}: {}",
                diagnostic.message
            );
        }
        // A non-numeric value stays at its own key too.
        let (_, diagnostic) = rejected("[font]\nline_height = \"big\"\n");
        assert!(diagnostic.message.contains("must be a number"));

        // The neighboring key isn't dropped: a rejected multiplier doesn't affect
        // the point size.
        let parsed = Settings::parse("[font]\nline_height = 9\nsize = 18\n").expect("parses");
        assert_eq!(parsed.settings.font.size, 18.0);
        assert_eq!(parsed.settings.font.line_height, 1.0);
    }

    #[test]
    fn the_retired_prompt_key_is_kept_but_not_read() {
        // **012 phase-10: `shell.prompt` is retired.** A separate key produced two
        // prompts on screen (the user's in the grid, the dock's below) and the
        // caret jumped between the two; the choice moved to `integration`'s third
        // value.
        //
        // Retirement is the third state of the rule "an unknown key is preserved,
        // a key is not deleted": the line stays in the file, never interferes with
        // behavior, but **isn't silent either**. Had it been silent the user would
        // think the line they wrote does something.
        for text in [
            "[shell]\nprompt = \"shell\"\n",
            "[shell]\nprompt = \"terminal\"\n",
            // Because its value is never read, an unrecognized value falls the same
            // way: there is no longer such a thing as "not accepted", the key
            // itself doesn't exist.
            "[shell]\nprompt = false\n",
            "shell = { prompt = \"shell\" }\n",
        ] {
            let parsed = Settings::parse(text).expect("parses");
            assert_eq!(parsed.settings, Settings::default(), "{text:?}");
            assert_eq!(parsed.diagnostics.len(), 1, "{text:?}");
            assert_eq!(
                parsed.diagnostics[0].message,
                "`shell.prompt` is no longer read; use `shell.integration = \"blocks\"` \
                 to keep your own prompt",
                "{text:?}"
            );
        }
    }

    #[test]
    fn the_retired_key_leaves_integration_alone() {
        // Two keys in the same section: the retired one's presence mustn't drop the
        // other.
        let parsed = Settings::parse("[shell]\nprompt = \"zsh\"\nintegration = \"blocks\"\n")
            .expect("parses");
        assert_eq!(parsed.settings.shell_integration, ShellIntegration::Blocks);
        assert_eq!(parsed.diagnostics.len(), 1);
    }

    #[test]
    fn integration_has_three_rungs() {
        assert_eq!(clean("").shell_integration, ShellIntegration::Auto);
        for (value, expected) in [
            ("auto", ShellIntegration::Auto),
            ("blocks", ShellIntegration::Blocks),
            ("off", ShellIntegration::Off),
        ] {
            assert_eq!(
                clean(&format!("[shell]\nintegration = \"{value}\"\n")).shell_integration,
                expected,
                "{value}"
            );
        }
        // Two derived questions and **separate** answers: `blocks` installs the
        // wrapper (for blocks and marks) but doesn't want the dock. Deriving one
        // from the other would bring back the bug phase-10 closed.
        assert!(ShellIntegration::Auto.installs_wrapper());
        assert!(ShellIntegration::Blocks.installs_wrapper());
        assert!(!ShellIntegration::Off.installs_wrapper());
        assert!(ShellIntegration::Auto.wants_dock());
        assert!(!ShellIntegration::Blocks.wants_dock());
        assert!(!ShellIntegration::Off.wants_dock());
    }

    #[test]
    fn unusable_file_closes_osc52_only() {
        assert_eq!(
            Settings::for_unusable_file(),
            Settings {
                osc52: Osc52::Off,
                ..Settings::default()
            }
        );
    }

    #[test]
    fn theme_name_outside_themes_dir_falls_back() {
        let (settings, diagnostic) = rejected("[appearance]\ntheme = \"../settings\"\n");
        assert_eq!(settings.theme, "system");
        assert_eq!(diagnostic.key, Some("appearance.theme"));
        assert_eq!(diagnostic.line, Some(2));
        assert_eq!(
            diagnostic.message,
            "`appearance.theme` must be a theme name without `/`, found \"../settings\"; using \"system\""
        );
        assert_eq!(rejected("[appearance]\ntheme = \"\"\n").0.theme, "system");
        assert_eq!(
            rejected("[appearance]\ntheme = \"a\\u0000\"\n").0.theme,
            "system"
        );

        let (settings, diagnostic) = rejected("[appearance]\ntheme = 3\n");
        assert_eq!(settings.theme, "system");
        assert_eq!(
            diagnostic.message,
            "`appearance.theme` must be a string, found an integer; using \"system\""
        );

        // The pair's keys too by the same rule, to their own defaults.
        let (settings, diagnostic) = rejected("[appearance]\ndark_theme = \"a/b\"\n");
        assert_eq!(settings.dark_theme, "bateri");
        assert_eq!(diagnostic.key, Some("appearance.dark_theme"));
        let (settings, diagnostic) = rejected("[appearance]\nlight_theme = false\n");
        assert_eq!(settings.light_theme, "bateri-light");
        assert_eq!(
            diagnostic.message,
            "`appearance.light_theme` must be a string, found a boolean; using \"bateri-light\""
        );
    }

    #[test]
    fn system_is_not_a_name_for_the_pair() {
        // `light_theme = "system"` would be a choice that loops back on itself.
        let (settings, diagnostic) = rejected("[appearance]\nlight_theme = \"system\"\n");
        assert_eq!(settings, Settings::default());
        assert_eq!(diagnostic.key, Some("appearance.light_theme"));
        assert_eq!(
            diagnostic.message,
            "`appearance.light_theme` must name a theme, not \"system\"; using \"bateri-light\""
        );
        assert_eq!(
            rejected("[appearance]\ndark_theme = \"system\"\n")
                .0
                .dark_theme,
            "bateri"
        );
        // The reserved value is valid for `theme`.
        assert!(clean("[appearance]\ntheme = \"system\"\n").follows_system());
    }

    #[test]
    fn section_of_wrong_type_is_diagnosed() {
        let (settings, diagnostic) = rejected("terminal = 5\n");
        assert_eq!(settings, Settings::default());
        assert_eq!(diagnostic.key, Some("terminal"));
        assert_eq!(diagnostic.line, Some(1));

        // An array of sections is called by its own name: "section … found a
        // section" would contradict itself.
        let (_, diagnostic) = rejected("[[terminal]]\nscrollback = 5\n");
        assert_eq!(
            diagnostic.message,
            "`terminal` must be a section, found an array of sections (`[[…]]`)"
        );
    }

    #[test]
    fn smooth_scroll_is_read() {
        // `on` if not in the file: the feature isn't shipped off (027 Karar 4).
        assert_eq!(clean("").smooth_scroll, SmoothScroll::On);
        assert_eq!(
            clean("[motion]\nsmooth_scroll = \"off\"\n").smooth_scroll,
            SmoothScroll::Off
        );
        assert_eq!(
            clean("motion = { smooth_scroll = \"on\" }\n").smooth_scroll,
            SmoothScroll::On
        );
        // The three keys don't override each other.
        let all = clean(
            "[motion]\ncursor_motion = \"ease\"\nreduce_motion = \"on\"\nsmooth_scroll = \"off\"\n",
        );
        assert_eq!(all.cursor_motion, CursorMotion::Ease);
        assert_eq!(all.reduce_motion, ReduceMotion::On);
        assert_eq!(all.smooth_scroll, SmoothScroll::Off);
    }

    #[test]
    fn keypress_and_erase_are_read() {
        // `fade` / `recede` if not in the file: the animation must be visible out
        // of the box (030 Karar 7).
        let empty = clean("");
        assert_eq!(
            (empty.keypress, empty.erase),
            (Keypress::Fade, Erase::Recede)
        );
        for &(name, keypress) in Keypress::NAMES {
            let settings = clean(&format!("[motion]\nkeypress = \"{name}\"\n"));
            assert_eq!(settings.keypress, keypress, "{name}");
            assert_eq!(keypress.name(), name);
        }
        for &(name, erase) in Erase::NAMES {
            let settings = clean(&format!("motion = {{ erase = \"{name}\" }}\n"));
            assert_eq!(settings.erase, erase, "{name}");
            assert_eq!(erase.name(), name);
        }
        // Neighbors don't override each other.
        let all = clean(
            "[motion]\ncursor_motion = \"ease\"\nkeypress = \"off\"\nerase = \"off\"\n\
             smooth_scroll = \"off\"\n",
        );
        assert_eq!(all.cursor_motion, CursorMotion::Ease);
        assert_eq!(all.smooth_scroll, SmoothScroll::Off);
        assert_eq!((all.keypress, all.erase), (Keypress::Off, Erase::Off));
    }

    #[test]
    fn unrecognized_keypress_and_erase_keep_their_own_keys() {
        // `bounce` is in no list: it is unrecognized, i.e. a name that does nothing
        // when selected isn't accepted (030 Karar 7).
        for (value, found) in [
            ("\"bounce\"", "\"bounce\""),
            ("\"Fade\"", "\"Fade\""),
            ("1", "an integer"),
        ] {
            let text = format!("[motion]\ncursor_motion = \"snap\"\nkeypress = {value}\n");
            let (settings, diagnostic) = rejected(&text);
            assert_eq!(
                settings,
                Settings {
                    cursor_motion: CursorMotion::Snap,
                    ..Settings::default()
                },
                "{value}"
            );
            assert_eq!(diagnostic.key, Some("motion.keypress"), "{value}");
            assert_eq!(diagnostic.line, Some(3), "{value}");
            assert_eq!(
                diagnostic.message,
                format!(
                    "`motion.keypress` must be \"off\", \"fade\", \"rise\", \"pop\", \
                     \"extrude\", \"heat\", \"echo\", \"drop\", \"ink\" or \"squeeze\", \
                     found {found}; using \"fade\""
                )
            );
        }
        let (settings, diagnostic) =
            rejected("[motion]\nkeypress = \"off\"\nerase = \"dissolve\"\n");
        assert_eq!(settings.keypress, Keypress::Off);
        assert_eq!(settings.erase, Erase::Recede);
        assert_eq!(diagnostic.key, Some("motion.erase"));
        assert_eq!(
            diagnostic.message,
            "`motion.erase` must be \"off\", \"iris\", \"undertow\", \"echo\", \"bleed\", \
             \"unravel\", \"recede\", \"sublime\" or \"shatter\", found \"dissolve\"; \
             using \"recede\""
        );
        // The value that stands in at save time is the **current** setting, not
        // the default.
        let current = Settings {
            keypress: Keypress::Off,
            erase: Erase::Off,
            ..Settings::default()
        };
        let parsed =
            Settings::parse_keeping("[motion]\nkeypress = \"x\"\nerase = \"y\"\n", &current)
                .expect("parseable text");
        assert_eq!(
            (parsed.settings.keypress, parsed.settings.erase),
            (Keypress::Off, Erase::Off)
        );
        assert_eq!(parsed.diagnostics.len(), 2);
        let parsed = Settings::parse_keeping("motion = 5\n", &current).expect("parseable text");
        assert_eq!(
            (parsed.settings.keypress, parsed.settings.erase),
            (Keypress::Off, Erase::Off)
        );
    }

    #[test]
    fn keypress_and_erase_changes_are_motion_changes() {
        // Both go to the link (`bt_gpu::DisplayLink::set_glyph_fx`), so in the
        // `motion` arm; they mustn't move the session or the font.
        let before = clean("");
        for text in [
            "[motion]\nkeypress = \"off\"\n",
            "[motion]\nerase = \"off\"\n",
        ] {
            assert_eq!(
                before.changes(&clean(text)),
                Changes {
                    terminal: false,
                    font: false,
                    motion: true,
                    caret: false,
                    remote: false,
                },
                "{text}"
            );
        }
    }

    #[test]
    fn unrecognized_smooth_scroll_keeps_its_own_key() {
        // The same rule as `cursor_motion`: only its own key is affected, with a
        // diagnostic beside it.
        for (value, found) in [
            ("\"yes\"", "\"yes\""),
            ("\"On\"", "\"On\""),
            ("true", "a boolean"),
        ] {
            let text = format!("[motion]\ncursor_motion = \"snap\"\nsmooth_scroll = {value}\n");
            let (settings, diagnostic) = rejected(&text);
            assert_eq!(
                settings,
                Settings {
                    cursor_motion: CursorMotion::Snap,
                    ..Settings::default()
                },
                "{value}"
            );
            assert_eq!(diagnostic.key, Some("motion.smooth_scroll"), "{value}");
            assert_eq!(diagnostic.line, Some(3), "{value}");
            assert_eq!(
                diagnostic.message,
                format!(
                    "`motion.smooth_scroll` must be \"on\" or \"off\", found {found}; using \"on\""
                )
            );
        }
        // The value that stands in at save time is the **current** setting, not
        // the default.
        let current = Settings {
            smooth_scroll: SmoothScroll::Off,
            ..Settings::default()
        };
        let parsed = Settings::parse_keeping("[motion]\nsmooth_scroll = \"yes\"\n", &current)
            .expect("parseable text");
        assert_eq!(parsed.settings.smooth_scroll, SmoothScroll::Off);
        assert!(parsed.diagnostics[0].message.ends_with("using \"off\""));
        // The section has the wrong type: the key counts as not accepted.
        let parsed = Settings::parse_keeping("motion = 5\n", &current).expect("parseable text");
        assert_eq!(parsed.settings.smooth_scroll, SmoothScroll::Off);
    }

    #[test]
    fn smooth_scroll_change_is_a_motion_change() {
        // `bt-shell` resolves it on Reduce Motion's path, so the difference is in
        // `motion`; it mustn't move the session or the font.
        let before = clean("");
        let after = clean("[motion]\nsmooth_scroll = \"off\"\n");
        assert_eq!(
            before.changes(&after),
            Changes {
                terminal: false,
                font: false,
                motion: true,
                caret: false,
                remote: false,
            }
        );
        assert_eq!(after.changes(&after), Changes::default());
    }

    #[test]
    fn theme_write_keeps_smooth_scroll_and_unknown_keys() {
        // The menu's write path leaves the new key and its unrecognized neighbor
        // in place; the written text reads back the same value.
        let text = "[motion]\nsmooth_scroll = \"off\" # satır satır\nglide = 3\n";
        let written = Settings::with_theme(text, "paper").expect("writable text");
        assert!(written.starts_with(text), "{written}");
        let settings = clean(&written);
        assert_eq!(settings.smooth_scroll, SmoothScroll::Off);
        assert_eq!(settings.theme, "paper");
    }

    #[test]
    fn confirm_close_is_read() {
        // `running` if not in the file: the question only when a job is running.
        assert_eq!(clean("").confirm_close, ConfirmClose::Running);
        for (value, expected) in [
            ("never", ConfirmClose::Never),
            ("running", ConfirmClose::Running),
            ("always", ConfirmClose::Always),
        ] {
            let text = format!("[terminal]\nconfirm_close = \"{value}\"\n");
            assert_eq!(clean(&text).confirm_close, expected, "{value}");
        }
        // Neighboring keys don't override each other.
        let both = clean("[terminal]\nscrollback = 42\nconfirm_close = \"never\"\n");
        assert_eq!(both.scrollback, 42);
        assert_eq!(both.confirm_close, ConfirmClose::Never);
    }

    #[test]
    fn unrecognized_confirm_close_keeps_its_own_key() {
        for (value, found) in [
            ("\"Always\"", "\"Always\""),
            ("\"ask\"", "\"ask\""),
            ("true", "a boolean"),
        ] {
            let text = format!("[terminal]\nscrollback = 42\nconfirm_close = {value}\n");
            let (settings, diagnostic) = rejected(&text);
            assert_eq!(
                settings,
                Settings {
                    scrollback: 42,
                    ..Settings::default()
                },
                "{value}"
            );
            assert_eq!(diagnostic.key, Some("terminal.confirm_close"), "{value}");
            assert_eq!(diagnostic.line, Some(3), "{value}");
            assert_eq!(
                diagnostic.message,
                format!(
                    "`terminal.confirm_close` must be \"never\", \"running\" or \"always\", \
                     found {found}; using \"running\""
                )
            );
        }
        // The value that stands in at save time is the current setting; also when
        // the section has the wrong type.
        let current = Settings {
            confirm_close: ConfirmClose::Always,
            ..Settings::default()
        };
        let parsed = Settings::parse_keeping("[terminal]\nconfirm_close = \"no\"\n", &current)
            .expect("parseable text");
        assert_eq!(parsed.settings.confirm_close, ConfirmClose::Always);
        assert!(parsed.diagnostics[0].message.ends_with("using \"always\""));
        let parsed = Settings::parse_keeping("terminal = 5\n", &current).expect("parseable text");
        assert_eq!(parsed.settings.confirm_close, ConfirmClose::Always);
    }

    #[test]
    fn confirm_close_change_reaches_no_session() {
        // Read from the current settings at close time: the difference mustn't send
        // anything to the sessions, the font or the cursor (028 → Karar 6,
        // precedent `caret`).
        let before = clean("");
        let after = clean("[terminal]\nconfirm_close = \"always\"\n");
        assert_eq!(before.terminal(), after.terminal());
        assert_eq!(before.changes(&after), Changes::default());
    }

    #[test]
    fn theme_write_keeps_confirm_close_and_unknown_keys() {
        let text = "[terminal]\nconfirm_close = \"never\" # sormadan\nask_twice = true\n";
        let written = Settings::with_theme(text, "paper").expect("writable text");
        assert!(written.starts_with(text), "{written}");
        let settings = clean(&written);
        assert_eq!(settings.confirm_close, ConfirmClose::Never);
        assert_eq!(settings.theme, "paper");
    }

    fn host_rule(pattern: &str, mark: HostMark) -> HostRule {
        HostRule {
            pattern: pattern.to_owned(),
            mark,
        }
    }

    #[test]
    fn a_host_pattern_matches_with_star_and_question_ignoring_case() {
        // 037 Karar 2: `*` is any string, empty included, `?` a single character.
        let prod = [host_rule("prod-*", HostMark::Production)];
        assert_eq!(host_mark(&prod, "prod-web-1"), HostMark::Production);
        assert_eq!(host_mark(&prod, "PROD-WEB-1"), HostMark::Production);
        assert_eq!(host_mark(&prod, "prod-"), HostMark::Production);
        assert_eq!(host_mark(&prod, "preprod-web"), HostMark::None);
        let one = [host_rule("prod-?", HostMark::Staging)];
        assert_eq!(host_mark(&one, "prod-1"), HostMark::Staging);
        assert_eq!(host_mark(&one, "prod-10"), HostMark::None);
        assert_eq!(host_mark(&one, "prod-"), HostMark::None);
        // The dot is an ordinary character; `*` swallows it too.
        let domain = [host_rule("*.staging.example.com", HostMark::Staging)];
        assert_eq!(
            host_mark(&domain, "a.b.staging.example.com"),
            HostMark::Staging
        );
        assert_eq!(host_mark(&domain, "staging.example.com"), HostMark::None);
        // Backtracking: `*`'s first attempt ends in the wrong place.
        let tricky = [host_rule("*a*b?", HostMark::Development)];
        assert_eq!(host_mark(&tricky, "xaxbxbz"), HostMark::Development);
        assert_eq!(host_mark(&tricky, "xaxb"), HostMark::None);
    }

    #[test]
    fn a_pattern_without_at_matches_the_host_after_the_user() {
        // `deploy@prod` and `prod` are the same machine; a pattern with `@` asks
        // about the user too.
        let bare = [host_rule("prod", HostMark::Production)];
        assert_eq!(host_mark(&bare, "deploy@prod"), HostMark::Production);
        assert_eq!(host_mark(&bare, "prod"), HostMark::Production);
        let root = [host_rule("root@*", HostMark::Staging)];
        assert_eq!(host_mark(&root, "root@db"), HostMark::Staging);
        assert_eq!(host_mark(&root, "deploy@db"), HostMark::None);
        assert_eq!(host_mark(&root, "db"), HostMark::None);
    }

    #[test]
    fn the_first_matching_host_rule_wins_and_none_stops_the_search() {
        let rules = [
            host_rule("prod-canary", HostMark::None),
            host_rule("prod-db", HostMark::Rgb(0xc678dd)),
            host_rule("prod-*", HostMark::Production),
            host_rule("*", HostMark::Development),
        ];
        assert_eq!(host_mark(&rules, "prod-canary"), HostMark::None);
        assert_eq!(host_mark(&rules, "prod-db"), HostMark::Rgb(0xc678dd));
        assert_eq!(host_mark(&rules, "prod-web"), HostMark::Production);
        assert_eq!(host_mark(&rules, "vm"), HostMark::Development);
        assert_eq!(host_mark(&[], "vm"), HostMark::None);
    }

    #[test]
    fn remote_hosts_are_read_in_order() {
        let settings = clean(
            "[remote]\nhosts = [\n  { host = \"prod-*\", mark = \"production\" },\n  \
             { host = \"stage\", mark = \"staging\" },\n  { host = \"dev\", mark = \"development\" },\n  \
             { host = \"vm\", mark = \"#C678dd\" },\n  { host = \"x\", mark = \"none\" },\n]\n",
        );
        assert_eq!(
            settings.remote_hosts,
            [
                host_rule("prod-*", HostMark::Production),
                host_rule("stage", HostMark::Staging),
                host_rule("dev", HostMark::Development),
                host_rule("vm", HostMark::Rgb(0xc678dd)),
                host_rule("x", HostMark::None),
            ]
        );
        // An array of sections writes the same list.
        let tables = clean(
            "[[remote.hosts]]\nhost = \"prod-*\"\nmark = \"production\"\n\
             [[remote.hosts]]\nhost = \"vm\"\nmark = \"#c678dd\"\n",
        );
        assert_eq!(
            tables.remote_hosts,
            [
                host_rule("prod-*", HostMark::Production),
                host_rule("vm", HostMark::Rgb(0xc678dd)),
            ]
        );
        assert_eq!(clean("[remote]\nhosts = []\n").remote_hosts, []);
    }

    #[test]
    fn a_broken_host_entry_rejects_the_whole_list() {
        // The save-time rule: a single broken entry rejects the whole key and the
        // given list stays; the diagnostic names the first broken entry's line.
        let current = Settings {
            remote_hosts: vec![host_rule("old", HostMark::Staging)],
            ..Settings::default()
        };
        for (text, found) in [
            (
                "[remote]\nhosts = [\n  { host = \"a\", mark = \"production\" },\n  \
                 { host = \"b\", mark = \"prod\" },\n]\n",
                "found mark \"prod\" for \"b\"",
            ),
            (
                "[remote]\nhosts = [\n  { host = \"a\", mark = \"production\" },\n  \
                 { mark = \"staging\" },\n]\n",
                "found an entry without a host",
            ),
            (
                "[remote]\nhosts = [\n  { host = \"a\", mark = \"production\" },\n  \
                 { host = \"b\" },\n]\n",
                "found no mark for \"b\"",
            ),
            (
                "[remote]\nhosts = [\n  { host = \"a\", mark = \"production\" },\n  \
                 { host = \"b\", mark = \"#12345\" },\n]\n",
                "found mark \"#12345\" for \"b\"",
            ),
            (
                "[remote]\nhosts = [\n  { host = \"a\", mark = \"production\" },\n  \
                 \"b\",\n]\n",
                "found an entry that is not a { … } table",
            ),
            ("[remote]\n\nhosts = \"prod\"\n", "found a string"),
        ] {
            let parsed = Settings::parse_keeping(text, &current).expect("parseable text");
            assert_eq!(parsed.settings.remote_hosts, current.remote_hosts, "{text}");
            let [diagnostic] = <[Diagnostic; 1]>::try_from(parsed.diagnostics)
                .unwrap_or_else(|got| panic!("a single diagnostic expected: {got:?}"));
            assert_eq!(diagnostic.key, Some("remote.hosts"));
            assert!(diagnostic.message.contains(found), "{diagnostic}");
            assert!(diagnostic.message.ends_with("keeping the previous list"));
            // The broken entry is on the fourth line; `hosts` itself on the third.
            let line = if found == "found a string" { 3 } else { 4 };
            assert_eq!(diagnostic.line, Some(line), "{text}");
        }
        // At startup the same text falls to the empty list; a section of the wrong
        // type keeps the given list too.
        let (settings, _) = rejected("[remote]\nhosts = [{ host = \"b\", mark = \"x\" }]\n");
        assert_eq!(settings.remote_hosts, []);
        let parsed = Settings::parse_keeping("remote = 1\n", &current).expect("parseable");
        assert_eq!(parsed.settings.remote_hosts, current.remote_hosts);
    }

    #[test]
    fn a_remote_hosts_change_is_its_own_field() {
        let before = clean("");
        let after = clean("[remote]\nhosts = [{ host = \"prod\", mark = \"production\" }]\n");
        assert_eq!(
            before.changes(&after),
            Changes {
                remote: true,
                ..Changes::default()
            }
        );
        assert_eq!(after.changes(&after), Changes::default());
    }

    #[test]
    fn theme_write_keeps_remote_hosts_comments_and_unknown_keys() {
        // R2.4: every path that writes to the file leaves `[remote]`, its comment
        // and a key we don't recognize in place.
        let text = "[remote]\n# prod kırmızı\nhosts = [\n  { host = \"prod\", mark = \"production\" }, # canlı\n]\nfuture = 1\n";
        let written = Settings::with_theme(text, "paper").expect("writable text");
        assert!(written.starts_with(text), "{written}");
        let settings = clean(&written);
        assert_eq!(
            settings.remote_hosts,
            [host_rule("prod", HostMark::Production)]
        );
        assert_eq!(settings.theme, "paper");
    }

    #[test]
    fn unknown_keys_and_sections_are_silent() {
        // A later set's keys mustn't produce a diagnostic today: `intensity` and
        // `speed` are in the reference's `[motion]` section, not ours (008 → Kapsam
        // dışı; `keypress` was recognized in 030 and served as the witness).
        let text = "\
future = true
[terminal]
scrollback = 42
shape = \"block\"
[motion]
speed = \"brisk\"
intensity = 0.5
[font]
line_height = 1.2
";
        let settings = clean(text);
        assert_eq!(settings.scrollback, 42);
        assert_eq!(settings.cursor_motion, CursorMotion::Spring);
    }

    #[test]
    fn unparseable_text_is_a_separate_result() {
        let err = Settings::parse("[terminal]\nscrollback = \n").expect_err("invalid TOML");
        assert_eq!(err.key, None);
        assert_eq!(err.line, Some(2));
        assert!(err.message.starts_with("invalid TOML: "), "{err}");
        // The diagnostic is a single line with no "expected" list: the subtitle is
        // drawn on the same line as the title.
        assert!(!err.to_string().contains('\n'), "{err}");
        assert!(!err.message.contains("expected"), "{err}");

        assert!(Settings::parse("[terminal").is_err());
        // A number exceeding TOML's integer limit can't be clamped to the ceiling:
        // the value can't be read at all and the document drops
        // (`docs/AYARLAR.md`).
        assert!(Settings::parse("[terminal]\nscrollback = 99999999999999999999\n").is_err());
    }

    #[test]
    fn theme_write_keeps_every_other_byte() {
        // The user's file: comments, blank lines, key order, a key and section we
        // don't recognize, the comment next to a value. Choosing a theme from the
        // menu changes only the value.
        let text = "\
# my settings

[terminal]
scrollback = 500  # plenty
shape = \"block\"

[appearance]
# picked by hand
theme = \"system\"   # follows macOS
light_theme = \"paper\"
dark_theme = \"ink\"
future = true

[motion]
cursor = \"spring\"
";
        let written = Settings::with_theme(text, "bateri").expect("writable text");
        assert_eq!(
            written,
            text.replace("theme = \"system\"", "theme = \"bateri\"")
        );
        // The pair is in place: returning to `"system"` brings it back.
        let settings = clean(&written);
        assert_eq!(
            (
                settings.theme.as_str(),
                settings.light_theme.as_str(),
                settings.dark_theme.as_str(),
                settings.scrollback
            ),
            ("bateri", "paper", "ink", 500)
        );
        assert_eq!(
            Settings::with_theme(&written, SYSTEM_THEME).expect("writable text"),
            text
        );
    }

    #[test]
    fn theme_write_adds_what_is_missing() {
        // No section: appended at the end, what comes before stays the same.
        let text = "[terminal]\nscrollback = 5\n";
        let written = Settings::with_theme(text, "paper").expect("writable text");
        assert_eq!(
            written,
            format!("{text}\n[appearance]\ntheme = \"paper\"\n")
        );
        assert_eq!(clean(&written).theme, "paper");

        // Empty text.
        assert_eq!(
            Settings::with_theme("", "paper").expect("writable text"),
            "[appearance]\ntheme = \"paper\"\n"
        );

        // Section present, key missing: inside the section, before the next section.
        let text = "[appearance]\ndark_theme = \"ink\"\n\n[font]\nsize = 14\n";
        let written = Settings::with_theme(text, "paper").expect("writable text");
        assert_eq!(
            written,
            "[appearance]\ndark_theme = \"ink\"\ntheme = \"paper\"\n\n[font]\nsize = 14\n"
        );
    }

    #[test]
    fn theme_write_leaves_trailing_comments_where_they_were() {
        // A `/code-review` finding: a comment at the end of the document is the
        // document's tail in `toml_edit`, the new section was being added **in
        // front of** it. The line of a user who uncommented it would become
        // `appearance.family` and be silently ignored.
        let text = "[font]\nsize = 14\n# family = \"Menlo\"\n";
        let written = Settings::with_theme(text, "paper").expect("writable text");
        assert_eq!(
            written,
            format!("{text}\n[appearance]\ntheme = \"paper\"\n")
        );
        let uncommented = written.replace("# family", "family");
        assert_eq!(clean(&uncommented).font.family.as_deref(), Some("Menlo"));
    }

    #[test]
    fn theme_write_keeps_crlf_line_endings() {
        // A `/code-review` finding: `toml_edit` writes line endings as LF; a single
        // theme selection would show the whole file as changed in a dotfile
        // repository.
        let text = "[appearance]\r\ntheme = \"a\"  # c\r\n\r\n[font]\r\nsize = 14\r\n";
        assert_eq!(
            Settings::with_theme(text, "paper").expect("writable text"),
            text.replace("\"a\"", "\"paper\"")
        );
        // The added section too with the file's line ending.
        assert_eq!(
            Settings::with_theme("[font]\r\nsize = 14\r\n", "paper").expect("writable text"),
            "[font]\r\nsize = 14\r\n\r\n[appearance]\r\ntheme = \"paper\"\r\n"
        );
        // A `/code-review` finding: `toml_edit` leaves the `\r\n` inside a
        // multi-line string raw; the conversion turned it into `\r\r\n` and left the
        // file invalid. The result is parseable and the text is the same.
        let text = "[appearance]\r\ntheme = \"a\"\r\n[notes]\r\nnote = \"\"\"x\r\ny\"\"\"\r\n";
        let written = Settings::with_theme(text, "paper").expect("writable text");
        assert_eq!(written, text.replace("\"a\"", "\"paper\""));
        assert_eq!(clean(&written).theme, "paper");
    }

    #[test]
    fn theme_write_keeps_the_way_the_section_is_written() {
        for (text, expected) in [
            // An inline table stays inline.
            (
                "appearance = { theme = \"a\", dark_theme = \"ink\" }\n",
                "appearance = { theme = \"paper\", dark_theme = \"ink\" }\n",
            ),
            // A dotted key stays dotted.
            (
                "appearance.theme = \"a\"\n",
                "appearance.theme = \"paper\"\n",
            ),
            // In place of a value of a type that isn't accepted: the user chose a
            // theme.
            (
                "[appearance]\ntheme = 3 # oops\n",
                "[appearance]\ntheme = \"paper\" # oops\n",
            ),
        ] {
            let written = Settings::with_theme(text, "paper").expect("writable text");
            assert_eq!(written, expected);
            assert_eq!(clean(&written).theme, "paper", "{text}");
        }
        // An `[appearance]` with only a subsection written: the result is still
        // read.
        let written =
            Settings::with_theme("[appearance.extra]\nx = 1\n", "paper").expect("writable text");
        assert_eq!(clean(&written).theme, "paper", "{written}");
    }

    #[test]
    fn theme_write_refuses_what_it_would_destroy() {
        // Text that can't be parsed: no text is **produced**, the user's
        // half-finished work isn't overwritten.
        let err =
            Settings::with_theme("[appearance]\ntheme = \"a\n", "paper").expect_err("invalid");
        assert_eq!((err.key, err.line), (None, Some(2)));
        assert!(err.message.starts_with("invalid TOML: "), "{err}");

        // An `appearance` that isn't a section: writing a table in its place would
        // delete the value.
        for text in ["appearance = 1\n", "[[appearance]]\ntheme = \"a\"\n"] {
            let err = Settings::with_theme(text, "paper").expect_err("not a section");
            assert_eq!(err.key, Some("appearance"), "{text}");
            assert!(err.message.contains("must be a section"), "{err}");
        }
        // A `theme` that is a table: writing a value in its place would delete the
        // subtable — both spellings (a `/code-review` finding: the inline form was
        // getting through).
        for text in [
            "[appearance.theme]\nx = 1\n",
            "[appearance]\ntheme = { light = \"paper\" }\n",
        ] {
            let err = Settings::with_theme(text, "paper").expect_err("theme is a section");
            assert_eq!(err.key, Some("appearance.theme"), "{text}");
            assert!(err.message.contains("found a section"), "{err}");
        }
    }

    /// The test's oracle: applies the edit to `Settings` **by hand**, independent
    /// of the write path. Decimals at two digits (the write path's promise).
    fn applied(mut settings: Settings, edit: &SettingsEdit) -> Settings {
        let two = |value: f64| (value * 100.0).round() / 100.0;
        match edit.clone() {
            SettingsEdit::Scrollback(lines) => settings.scrollback = lines,
            SettingsEdit::Cursor(shape) => settings.cursor = shape,
            SettingsEdit::CursorBlink(blink) => settings.cursor_blink = blink,
            SettingsEdit::CursorRadius(ratio) => settings.caret.radius_ratio = two(ratio),
            SettingsEdit::CursorGlow(glow) => settings.caret.glow = two(glow),
            SettingsEdit::CursorUnfocused(unfocused) => settings.caret.unfocused = unfocused,
            SettingsEdit::BlinkInterval(seconds) => settings.blink_interval = two(seconds),
            SettingsEdit::ConfirmClose(confirm) => settings.confirm_close = confirm,
            SettingsEdit::Theme(name) => settings.theme = name,
            SettingsEdit::LightTheme(name) => settings.light_theme = name,
            SettingsEdit::DarkTheme(name) => settings.dark_theme = name,
            SettingsEdit::FontFamily(name) => {
                settings.font.family = (!name.is_empty()).then_some(name);
            }
            SettingsEdit::FontSize(size) => settings.font.size = two(size),
            SettingsEdit::LineHeight(height) => settings.font.line_height = two(height),
            SettingsEdit::Osc52(mode) => settings.osc52 = mode,
            SettingsEdit::CursorMotion(motion) => settings.cursor_motion = motion,
            SettingsEdit::ReduceMotion(reduce) => settings.reduce_motion = reduce,
            SettingsEdit::SmoothScroll(smooth) => settings.smooth_scroll = smooth,
            SettingsEdit::Keypress(keypress) => settings.keypress = keypress,
            SettingsEdit::Erase(erase) => settings.erase = erase,
            SettingsEdit::ShellIntegration(integration) => {
                settings.shell_integration = integration;
            }
            // The oracle is right only from an empty list: both of `every_edit`'s
            // texts carry `hosts = []`. The filled list's rule is in its own test.
            SettingsEdit::RemoteHostMark { host, mark } => settings.remote_hosts.insert(
                0,
                HostRule {
                    pattern: bare_host(&host).to_owned(),
                    mark,
                },
            ),
            SettingsEdit::PreviewMaxSize(bytes) => settings.remote_files.preview_max_size = bytes,
            SettingsEdit::PreviewReadOnly(on) => settings.remote_files.preview_read_only = on,
            SettingsEdit::PreviewDir(path) => settings.remote_files.preview_dir = path,
            SettingsEdit::PreviewKeep(keep) => settings.remote_files.preview_keep = keep,
            SettingsEdit::PreviewLimit(bytes) => settings.remote_files.preview_limit = bytes,
            SettingsEdit::DownloadDir(path) => settings.remote_files.download_dir = path,
            SettingsEdit::DownloadConflict(conflict) => {
                settings.remote_files.download_conflict = conflict;
            }
            SettingsEdit::DownloadNotify(on) => settings.remote_files.download_notify = on,
        }
        settings
    }

    /// A non-default value from every key — every variant at least once;
    /// `FontFamily` twice, because an empty string means "the default family".
    fn every_edit() -> Vec<SettingsEdit> {
        vec![
            SettingsEdit::Scrollback(2500),
            SettingsEdit::Cursor(CaretShape::Beam),
            SettingsEdit::CursorBlink(CursorBlink::Auto),
            // Rounded to two digits: 0.123 → 0.12.
            SettingsEdit::CursorRadius(0.123),
            SettingsEdit::CursorGlow(2.5),
            SettingsEdit::CursorUnfocused(UnfocusedCaret::Solid),
            SettingsEdit::BlinkInterval(0.75),
            SettingsEdit::ConfirmClose(ConfirmClose::Always),
            SettingsEdit::Theme("paper".to_owned()),
            SettingsEdit::LightTheme("paper".to_owned()),
            SettingsEdit::DarkTheme("ink".to_owned()),
            SettingsEdit::FontFamily("Menlo".to_owned()),
            SettingsEdit::FontFamily(String::new()),
            SettingsEdit::FontSize(14.5),
            SettingsEdit::LineHeight(1.25),
            SettingsEdit::Osc52(Osc52::Off),
            SettingsEdit::CursorMotion(CursorMotion::Ease),
            SettingsEdit::ReduceMotion(ReduceMotion::On),
            SettingsEdit::SmoothScroll(SmoothScroll::Off),
            SettingsEdit::Keypress(Keypress::Off),
            SettingsEdit::Erase(Erase::Off),
            SettingsEdit::ShellIntegration(ShellIntegration::Blocks),
            SettingsEdit::RemoteHostMark {
                host: "deploy@prod".to_owned(),
                mark: HostMark::Production,
            },
            // Written as "250MB" and "1500KB": the largest exact unit.
            SettingsEdit::PreviewMaxSize(250_000_000),
            SettingsEdit::PreviewReadOnly(false),
            SettingsEdit::PreviewDir("/Volumes/Scratch/previews".to_owned()),
            SettingsEdit::PreviewKeep(PreviewKeep::UntilLaunch),
            SettingsEdit::PreviewLimit(1_500_000),
            SettingsEdit::DownloadDir("~/Desktop".to_owned()),
            SettingsEdit::DownloadConflict(DownloadConflict::KeepBoth),
            SettingsEdit::DownloadNotify(false),
        ]
    }

    fn marked(text: &str, host: &str, mark: HostMark) -> String {
        let edit = SettingsEdit::RemoteHostMark {
            host: host.to_owned(),
            mark,
        };
        Settings::with_edit(text, &edit).expect("writable text")
    }

    #[test]
    fn marking_a_host_creates_the_list() {
        // Empty file: the section and key are born; the pattern is without `user@`.
        assert_eq!(
            marked("", "deploy@prod", HostMark::Production),
            "[remote]\nhosts = [{ host = \"prod\", mark = \"production\" }]\n"
        );
        // Section present, key missing.
        assert_eq!(
            marked("[remote]\nfuture = 1\n", "vm", HostMark::Staging),
            "[remote]\nfuture = 1\nhosts = [{ host = \"vm\", mark = \"staging\" }]\n"
        );
        // At the start of a single-line array, with a space after the comma.
        assert_eq!(
            marked(
                "[remote]\nhosts = [{ host = \"a\", mark = \"staging\" }]\n",
                "b",
                HostMark::Development
            ),
            "[remote]\nhosts = [{ host = \"b\", mark = \"development\" }, \
             { host = \"a\", mark = \"staging\" }]\n"
        );
    }

    #[test]
    fn marking_a_host_keeps_the_list_as_written() {
        // Comments, the unknown key and the user's order are in place.
        let text = "# üst\n[remote]\n# prod kırmızı\nhosts = [\n  \
                    { host = \"db\", mark = \"staging\" }, # veri\n  \
                    { host = \"prod-*\", mark = \"production\" },\n]\nfuture = 1\n";
        // An equal pattern (case insensitive) changes in place, the order is kept.
        assert_eq!(
            marked(text, "root@DB", HostMark::Development),
            text.replace("mark = \"staging\"", "mark = \"development\"")
        );
        // The new host at the start, with the array's indentation.
        assert_eq!(
            marked(text, "cache", HostMark::Production),
            text.replace(
                "hosts = [\n",
                "hosts = [\n  { host = \"cache\", mark = \"production\" },\n"
            )
        );
        // None deletes the exact entry; if no glob matches nothing else is written.
        let removed = marked(text, "db", HostMark::None);
        assert_eq!(clean(&removed).remote_hosts.len(), 1, "{removed}");
        assert!(removed.contains("# prod kırmızı") && removed.contains("future = 1"));
        assert!(!removed.contains("\"db\""), "{removed}");
        // The chosen one is already the current resolution (even if it comes from a
        // glob): the text is unchanged.
        assert_eq!(marked(text, "db", HostMark::Staging), text);
        assert_eq!(marked(text, "prod-web", HostMark::Production), text);
        assert_eq!(marked(text, "vm", HostMark::None), text);
    }

    #[test]
    fn marking_a_host_beats_the_globs_in_front() {
        let resolved = |text: &str, host: &str| host_mark(&clean(text).remote_hosts, host);
        let globs = "[remote]\nhosts = [\n  { host = \"prod-*\", mark = \"production\" },\n]\n";
        // None leaves a host caught by a glob unmarked: `none` at the start.
        let none = marked(globs, "prod-canary", HostMark::None);
        assert_eq!(
            none,
            "[remote]\nhosts = [\n  { host = \"prod-canary\", mark = \"none\" },\n  \
             { host = \"prod-*\", mark = \"production\" },\n]\n"
        );
        assert_eq!(resolved(&none, "prod-canary"), HostMark::None);
        assert_eq!(resolved(&none, "prod-web"), HostMark::Production);
        // An exact entry with a glob in front of it would be ineffective if changed
        // in place: it is moved to the start.
        let behind = "[remote]\nhosts = [\n  { host = \"*\", mark = \"development\" },\n  \
                      { host = \"db\", mark = \"production\" },\n]\n";
        let moved = marked(behind, "db", HostMark::Staging);
        assert_eq!(
            moved,
            "[remote]\nhosts = [\n  { host = \"db\", mark = \"staging\" },\n  \
             { host = \"*\", mark = \"development\" },\n]\n"
        );
        // A glob with `user@`: the pattern without `user@` that None writes gets
        // past it.
        let user = "[remote]\nhosts = [{ host = \"root@*\", mark = \"staging\" }]\n";
        let none = marked(user, "root@db", HostMark::None);
        assert_eq!(resolved(&none, "root@db"), HostMark::None);
        assert_eq!(resolved(&none, "root@web"), HostMark::Staging);
    }

    #[test]
    fn marking_a_host_in_an_array_of_sections() {
        let text = "[[remote.hosts]]\nhost = \"db\"\nmark = \"staging\" # veri\n\n\
                    [[remote.hosts]]\nhost = \"prod-*\"\nmark = \"production\"\n";
        let in_place = marked(text, "db", HostMark::Development);
        assert_eq!(
            in_place,
            text.replace("mark = \"staging\"", "mark = \"development\"")
        );
        let prepended = marked(text, "cache", HostMark::Production);
        assert_eq!(
            clean(&prepended).remote_hosts,
            [
                HostRule {
                    pattern: "cache".to_owned(),
                    mark: HostMark::Production
                },
                HostRule {
                    pattern: "db".to_owned(),
                    mark: HostMark::Staging
                },
                HostRule {
                    pattern: "prod-*".to_owned(),
                    mark: HostMark::Production
                },
            ],
            "{prepended}"
        );
        assert!(prepended.contains("# veri"), "{prepended}");
        // Other sections before and after: the new entry is born in the array's
        // place.
        let around = format!("[font]\nsize = 13\n\n# liste\n{text}\n[notes]\nx = 1\n");
        assert_eq!(
            marked(&around, "cache", HostMark::Production),
            around.replace(
                "# liste\n",
                "# liste\n[[remote.hosts]]\nhost = \"cache\"\nmark = \"production\"\n\n"
            )
        );
        let removed = marked(text, "db", HostMark::None);
        assert_eq!(clean(&removed).remote_hosts.len(), 1, "{removed}");
    }

    #[test]
    fn marking_a_host_refuses_a_broken_list() {
        // Text that can't be parsed and a broken array aren't written: the user's
        // half-finished work.
        for text in [
            "[remote\n",
            "[remote]\nhosts = [{ host = \"a\", mark = \"prod\" }]\n",
            "[remote]\nhosts = \"a\"\n",
            "remote = 1\n",
        ] {
            let edit = SettingsEdit::RemoteHostMark {
                host: "b".to_owned(),
                mark: HostMark::Production,
            };
            assert!(Settings::with_edit(text, &edit).is_err(), "{text}");
        }
    }

    #[test]
    fn a_mark_is_written_the_way_it_is_read() {
        for (_, mark) in HostMark::NAMES {
            let text = marked("", "a", HostMark::Production);
            let text = marked(&text, "a", *mark);
            assert_eq!(host_mark(&clean(&text).remote_hosts, "a"), *mark, "{text}");
        }
        assert_eq!(HostMark::Rgb(0x0a0b0c).written(), "#0a0b0c");
    }

    /// Whether `written` was born from `text` by a single line changing or a
    /// single line being added — the line ending is `eol`.
    fn one_line_apart(text: &str, written: &str, eol: &str) -> bool {
        let before: Vec<&str> = text.split(eol).collect();
        let after: Vec<&str> = written.split(eol).collect();
        if before.len() == after.len() {
            return before.iter().zip(&after).filter(|(a, b)| a != b).count() == 1;
        }
        after.len() == before.len() + 1
            && (0..after.len()).any(|skip| {
                let mut rest = after.clone();
                rest.remove(skip);
                rest == before
            })
    }

    #[test]
    fn every_edit_reads_back_and_touches_one_line() {
        // The user's file: the template's comments and order, plus a key and
        // section we don't recognize, a comment next to a value.
        let rich = format!(
            "{}future = true\n\n[notes]\nx = 1\n",
            Settings::TEMPLATE.replace("scrollback = 10000", "scrollback = 10000  # plenty")
        );
        let crlf = rich.replace('\n', "\r\n");
        for edit in every_edit() {
            for (text, eol) in [(rich.as_str(), "\n"), (crlf.as_str(), "\r\n")] {
                let written = Settings::with_edit(text, &edit).expect("writable text");
                assert!(one_line_apart(text, &written, eol), "{edit:?}\n{written}");
                assert_eq!(
                    clean(&written),
                    applied(clean(text), &edit),
                    "{edit:?}\n{written}"
                );
            }
            // Empty text: the section and key are added.
            let written = Settings::with_edit("", &edit).expect("writable text");
            assert_eq!(
                clean(&written),
                applied(Settings::default(), &edit),
                "{edit:?}"
            );
        }
    }

    #[test]
    fn edits_keep_the_way_the_section_is_written() {
        // `with_theme`'s test for a key of every type: preserves the inline table
        // and dotted key spellings, writes in place of a value that isn't accepted
        // (the user chose a value in the window).
        for (text, edit, expected) in [
            (
                "font = { size = 13, family = \"Menlo\" }\n",
                SettingsEdit::FontSize(14.5),
                "font = { size = 14.5, family = \"Menlo\" }\n",
            ),
            (
                "terminal.scrollback = 5\n",
                SettingsEdit::Scrollback(2500),
                "terminal.scrollback = 2500\n",
            ),
            (
                "[motion]\ncursor_motion = 3 # oops\n",
                SettingsEdit::CursorMotion(CursorMotion::Snap),
                "[motion]\ncursor_motion = \"snap\" # oops\n",
            ),
        ] {
            assert_eq!(
                Settings::with_edit(text, &edit).expect("writable text"),
                expected
            );
        }
        // A key that is a section: writing a value in its place would delete the
        // subtable.
        for text in ["[font.size]\nx = 1\n", "[font]\nsize = { x = 1 }\n"] {
            let err =
                Settings::with_edit(text, &SettingsEdit::FontSize(14.0)).expect_err("section");
            assert_eq!(err.key, Some("font.size"), "{text}");
            assert_eq!(err.message, "`font.size` must be a number, found a section");
        }
        let err = Settings::with_edit(
            "motion = 1\n",
            &SettingsEdit::SmoothScroll(SmoothScroll::Off),
        )
        .expect_err("not a section");
        assert_eq!(err.key, Some("motion"));
    }

    #[test]
    fn every_name_reads_back_as_its_value() {
        // Table ↔ parser guard: every spelling gives its own variant from the
        // parser and `name()` writes it back.
        fn check<T: Copy + PartialEq + std::fmt::Debug>(
            names: &[(&str, T)],
            name: fn(T) -> &'static str,
            section: &str,
            key: &str,
            read: fn(&Settings) -> T,
        ) {
            for &(written, value) in names {
                assert_eq!(name(value), written);
                let text = format!("[{section}]\n{key} = {written:?}\n");
                assert_eq!(read(&clean(&text)), value, "{text}");
            }
        }
        check(
            CaretShape::NAMES,
            CaretShape::name,
            "terminal",
            "cursor",
            |s| s.cursor,
        );
        check(
            CursorBlink::NAMES,
            CursorBlink::name,
            "terminal",
            "cursor_blink",
            |s| s.cursor_blink,
        );
        check(
            UnfocusedCaret::NAMES,
            UnfocusedCaret::name,
            "terminal",
            "cursor_unfocused",
            |s| s.caret.unfocused,
        );
        check(
            ConfirmClose::NAMES,
            ConfirmClose::name,
            "terminal",
            "confirm_close",
            |s| s.confirm_close,
        );
        check(Osc52::NAMES, Osc52::name, "clipboard", "osc52", |s| s.osc52);
        check(
            CursorMotion::NAMES,
            CursorMotion::name,
            "motion",
            "cursor_motion",
            |s| s.cursor_motion,
        );
        check(
            ReduceMotion::NAMES,
            ReduceMotion::name,
            "motion",
            "reduce_motion",
            |s| s.reduce_motion,
        );
        check(
            SmoothScroll::NAMES,
            SmoothScroll::name,
            "motion",
            "smooth_scroll",
            |s| s.smooth_scroll,
        );
        check(
            ShellIntegration::NAMES,
            ShellIntegration::name,
            "shell",
            "integration",
            |s| s.shell_integration,
        );
        check(
            PreviewKeep::NAMES,
            PreviewKeep::name,
            "remote",
            "preview_keep",
            |s| s.remote_files.preview_keep,
        );
        check(
            DownloadConflict::NAMES,
            DownloadConflict::name,
            "remote",
            "download_conflict",
            |s| s.remote_files.download_conflict,
        );
    }

    #[test]
    fn sizes_read_and_write_in_the_unit_table() {
        assert_eq!(parse_size("100MB"), Some(100_000_000));
        assert_eq!(parse_size("2 GB"), Some(2_000_000_000));
        assert_eq!(parse_size("0B"), Some(0));
        assert_eq!(parse_size("1500KB"), Some(1_500_000));
        assert_eq!(parse_size("3TB"), Some(3_000_000_000_000));
        for bad in [
            "", "MB", "100", "100mb", "1.5GB", "-1MB", "100  MB", " 100MB", "100MiB",
        ] {
            assert_eq!(parse_size(bad), None, "{bad:?}");
        }
        assert_eq!(parse_size("99999999999TB"), None, "an overflow is no size");
        assert_eq!(format_size(100_000_000), "100MB");
        assert_eq!(format_size(2_000_000_000), "2GB");
        assert_eq!(format_size(1_500_000), "1500KB");
        assert_eq!(format_size(1), "1B");
        assert_eq!(format_size(0), "0B");
        for bytes in [0, 1, 999, 1_000, 1_500_000, 100_000_000, 7_000_000_000_000] {
            assert_eq!(parse_size(&format_size(bytes)), Some(bytes), "{bytes}");
        }
    }

    #[test]
    fn the_remote_file_keys_are_read() {
        let settings = clean(
            "[remote]\npreview_max_size = \"5MB\"\npreview_read_only = false\n\
             preview_dir = \"/tmp/p\"\npreview_keep = \"30d\"\npreview_limit = \"10 GB\"\n\
             download_dir = \"~\"\ndownload_conflict = \"replace\"\ndownload_notify = false\n",
        );
        assert_eq!(
            settings.remote_files,
            RemoteFiles {
                preview_max_size: 5_000_000,
                preview_read_only: false,
                preview_dir: "/tmp/p".to_owned(),
                preview_keep: PreviewKeep::Month,
                preview_limit: 10_000_000_000,
                download_dir: "~".to_owned(),
                download_conflict: DownloadConflict::Replace,
                download_notify: false,
            }
        );
        // A key left out keeps its default; the hosts array is untouched.
        let one = clean("[remote]\npreview_keep = \"launch\"\n");
        assert_eq!(one.remote_files.preview_keep, PreviewKeep::UntilLaunch);
        assert_eq!(one.remote_files.preview_max_size, 100_000_000);
        assert_eq!(one.remote_files.download_dir, "~/Downloads");
        assert_eq!(PreviewKeep::UntilLaunch.max_age(), None);
        assert_eq!(
            PreviewKeep::Week.max_age(),
            Some(std::time::Duration::from_secs(7 * 86_400))
        );
    }

    #[test]
    fn a_rejected_remote_file_value_keeps_the_previous_one() {
        let previous = Settings {
            remote_files: RemoteFiles {
                preview_max_size: 1_000,
                preview_read_only: false,
                preview_dir: "/old".to_owned(),
                ..RemoteFiles::default()
            },
            ..Settings::default()
        };
        // The rejected key takes the previous value; every other key, absent from
        // the text, its default.
        let field = |files: &RemoteFiles, key: &str| match key {
            "remote.preview_max_size" => files.preview_max_size.to_string(),
            "remote.preview_read_only" => files.preview_read_only.to_string(),
            "remote.preview_dir" => files.preview_dir.clone(),
            "remote.preview_keep" => files.preview_keep.name().to_owned(),
            "remote.download_conflict" => files.download_conflict.name().to_owned(),
            _ => String::new(),
        };
        for (text, key, message) in [
            (
                "[remote]\npreview_max_size = 100\n",
                "remote.preview_max_size",
                "`remote.preview_max_size` must be a size like \"100MB\" (B, KB, MB, GB or TB), \
                 found an integer; using \"1KB\"",
            ),
            (
                "[remote]\npreview_max_size = \"100mb\"\n",
                "remote.preview_max_size",
                "`remote.preview_max_size` must be a size like \"100MB\" (B, KB, MB, GB or TB), \
                 found \"100mb\"; using \"1KB\"",
            ),
            (
                "[remote]\npreview_read_only = \"true\"\n",
                "remote.preview_read_only",
                "`remote.preview_read_only` must be true or false, found a string; using false",
            ),
            (
                "[remote]\npreview_dir = \"Previews\"\n",
                "remote.preview_dir",
                "`remote.preview_dir` must be a folder starting with / or ~/, \
                 found \"Previews\"; using \"/old\"",
            ),
            (
                "[remote]\npreview_dir = \"~other/x\"\n",
                "remote.preview_dir",
                "`remote.preview_dir` must be a folder starting with / or ~/, \
                 found \"~other/x\"; using \"/old\"",
            ),
            (
                "[remote]\npreview_keep = \"2d\"\n",
                "remote.preview_keep",
                "`remote.preview_keep` must be \"launch\", \"1d\", \"7d\" or \"30d\", \
                 found \"2d\"; using \"7d\"",
            ),
            (
                "[remote]\ndownload_conflict = \"Ask\"\n",
                "remote.download_conflict",
                "`remote.download_conflict` must be \"ask\", \"keep_both\" or \"replace\", \
                 found \"Ask\"; using \"ask\"",
            ),
        ] {
            let parsed = Settings::parse_keeping(text, &previous).expect("parseable");
            let [diagnostic] = <[Diagnostic; 1]>::try_from(parsed.diagnostics)
                .unwrap_or_else(|got| panic!("a single diagnostic expected: {got:?}"));
            assert_eq!(diagnostic.key, Some(key), "{text}");
            assert_eq!(diagnostic.line, Some(2), "{text}");
            assert_eq!(diagnostic.message, message, "{text}");
            assert_eq!(
                field(&parsed.settings.remote_files, key),
                field(&previous.remote_files, key),
                "{text}"
            );
        }
        // A `[remote]` of the wrong type keeps every remote key.
        let parsed = Settings::parse_keeping("remote = 5\n", &previous).expect("parseable");
        assert_eq!(parsed.settings.remote_files, previous.remote_files);
    }

    #[test]
    fn a_remote_file_key_change_is_a_remote_change() {
        let old = Settings::default();
        let mut new = old.clone();
        new.remote_files.download_notify = false;
        assert!(old.changes(&new).remote);
        assert!(!old.changes(&old).remote);
    }

    #[test]
    fn folders_expand_under_the_home_directory() {
        let home = std::path::Path::new("/Users/u");
        assert_eq!(
            expand_home("~/Downloads", Some(home)),
            Some(home.join("Downloads"))
        );
        assert_eq!(expand_home("~", Some(home)), Some(home.to_path_buf()));
        assert_eq!(
            expand_home("/Volumes/x", None),
            Some(std::path::PathBuf::from("/Volumes/x"))
        );
        assert_eq!(expand_home("~/Downloads", None), None);
        assert_eq!(expand_home("Downloads", Some(home)), None);
    }

    #[test]
    fn parser_reason_drops_the_expected_list() {
        assert_eq!(
            parser_reason("invalid escape, expected `b`, `e`"),
            "invalid escape"
        );
        assert_eq!(parser_reason("duplicate key"), "duplicate key");
    }

    #[test]
    fn line_of_counts_from_one_and_rejects_out_of_range() {
        assert_eq!(line_of("a\nb\nc", 0), Some(1));
        assert_eq!(line_of("a\nb\nc", 2), Some(2));
        assert_eq!(line_of("a\nb\nc", 4), Some(3));
        assert_eq!(line_of("a", 99), None);
    }
}
