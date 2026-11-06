//! The settings the palette can change: their names, their values, and the edit each value is.
//!
//! The palette lists the settings people reach for (34 of them) and changes them without the
//! settings window: a row shows the value in effect, and choosing a value writes it through the
//! same typed edit the menus and the window use ([`SettingsEdit`], `Settings::with_edit`). So the
//! catalog owns no value of its own:
//!
//! - the **choices** of a named setting are the settings model's own spelling table
//!   (`CaretShape::NAMES` …), so a new value there shows here;
//! - the **default** is read off `Settings::default()`, so it cannot drift from the model;
//! - what a number's page offers are **ready values** (point size, line spacing, scrollback,
//!   contrast) — a free number stays in the settings window. A value in the file that the list
//!   lacks is put in its place in the list rather than hidden.
//!
//! A choice is a **token**, a string the catalog reads back: the spelling of a name (`beam`), a
//! number (`13`, `1.1`), `on`/`off` for a switch, a theme's name. The default choice is marked
//! (it writes its value; the key is never deleted, as the file's writer never deletes one). The
//! themes themselves are the shell's to list (the embedded ones and `themes/*.toml`); the catalog
//! knows only that a setting takes one.
//!
//! Each setting says whether it is **live**: whether showing it while it is highlighted is cheap
//! and safe (the theme, the font, the line spacing, the cursor's shape, the split style, the
//! veil, the tab bar's place and icons). Typing, deleting and arrival effects, and everything
//! that decides what a session does, are chosen and written, never previewed.

use bt_core::{
    CaretShape, ConfirmClose, ContentEdge, CursorBlink, CursorMotion, DockArrival, Erase,
    IconColors, KeepRunning, Keypress, Osc52, ReduceMotion, RemoteStatsMode, RestoreWindows,
    SYSTEM_THEME, ScrollBounce, Scrollbar, Settings, SettingsEdit, ShellIntegration, SmoothScroll,
    SplitStyle, TabDetails, TabPosition, TabRunning,
};

use super::row::{Kind, Preview, Row, Scope, Slot, Tone};
use crate::font_options::{self, FontOption};

/// The settings window's grouping, kept as the order the settings are listed in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Appearance,
    Font,
    Terminal,
    Motion,
    Shell,
    Remote,
}

impl Section {
    pub fn title(self) -> &'static str {
        match self {
            Section::Appearance => "Appearance",
            Section::Font => "Font",
            Section::Terminal => "Terminal",
            Section::Motion => "Motion",
            Section::Shell => "Shell",
            Section::Remote => "Remote",
        }
    }
}

/// What kind of value a setting takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    /// A theme, for one slot ([`Slot::Active`] also takes "Match System").
    Theme(Slot),
    /// On or off.
    Toggle,
    /// One of a list: a name from the model's table, or a ready number.
    Choices,
}

/// One choice on a setting's page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    /// What is written (see the module header).
    pub token: String,
    pub label: String,
    /// The value in effect.
    pub current: bool,
    /// The value the model starts with: marked "default", and writes its value when chosen.
    pub default: bool,
}

/// A setting the palette offers.
pub struct Setting {
    key: &'static str,
    section: Section,
    title: &'static str,
    keywords: &'static str,
    control: Control,
    live: bool,
    note: Option<&'static str>,
    read: fn(&Settings) -> String,
    write: fn(&str) -> Option<SettingsEdit>,
    tokens: fn() -> Vec<String>,
    label: fn(&str) -> String,
}

impl Setting {
    /// The dotted key in the settings file (`terminal.cursor`): also the key of the row's id.
    pub fn key(&self) -> &'static str {
        self.key
    }

    /// The row's id (`set.terminal.cursor`).
    pub fn id(&self) -> String {
        format!("{}{}", Kind::Setting.prefix(), self.key)
    }

    pub fn section(&self) -> Section {
        self.section
    }

    pub fn title(&self) -> &'static str {
        self.title
    }

    pub fn control(&self) -> Control {
        self.control
    }

    /// Whether highlighting a value may show it before it is chosen.
    pub fn live(&self) -> bool {
        self.live
    }

    /// A line under the title: when the setting takes effect, or what it depends on.
    pub fn note(&self) -> Option<&'static str> {
        self.note
    }

    /// The token of the value `settings` holds.
    pub fn current(&self, settings: &Settings) -> String {
        (self.read)(settings)
    }

    /// The token of the model's own default.
    pub fn default_token(&self) -> String {
        self.current(&Settings::default())
    }

    /// What a token reads as to the user ("Beam", "13 pt", "Match System").
    pub fn label(&self, token: &str) -> String {
        (self.label)(token)
    }

    /// The label of the value `settings` holds.
    pub fn current_label(&self, settings: &Settings) -> String {
        self.label(&self.current(settings))
    }

    /// The edit that makes `token` the value; `None` for a token this setting cannot take.
    pub fn edit(&self, token: &str) -> Option<SettingsEdit> {
        (self.write)(token)
    }

    /// Showing `token` without writing it, if the setting is live.
    pub fn preview(&self, token: &str) -> Option<Preview> {
        self.live.then(|| Preview {
            key: self.key.to_owned(),
            token: token.to_owned(),
        })
    }

    /// The choices of the setting's page, in order, with the value in effect marked. A value
    /// outside the ready list (a point size of 13.5 written in the file) is put in its place.
    /// A theme setting has none here: the themes are the shell's.
    pub fn choices(&self, settings: &Settings) -> Vec<Choice> {
        if matches!(self.control, Control::Theme(_)) {
            return Vec::new();
        }
        let current = self.current(settings);
        let default = self.default_token();
        let mut tokens = (self.tokens)();
        if !tokens.contains(&current) {
            let at = if let Ok(value) = current.parse::<f64>()
                && tokens.iter().all(|t| t.parse::<f64>().is_ok())
            {
                tokens.partition_point(|t| t.parse::<f64>().is_ok_and(|n| n < value))
            } else {
                // Next to the default, which is the list's first in the lists that have one.
                tokens
                    .iter()
                    .position(|t| *t == default)
                    .map_or(0, |i| i + 1)
            };
            tokens.insert(at, current.clone());
        }
        tokens
            .into_iter()
            .map(|token| Choice {
                label: self.label(&token),
                current: token == current,
                default: token == default,
                token,
            })
            .collect()
    }

    /// The rows of the setting's page: one per choice, the value in effect checked, the default
    /// marked, each showing itself when highlighted if the setting is live.
    pub fn choice_rows(&self, settings: &Settings) -> Vec<Row> {
        self.choices(settings)
            .into_iter()
            .map(|choice| {
                let mut row =
                    Row::new(Kind::Choice, &choice.token, choice.label).checked(choice.current);
                if choice.default {
                    row = row.status("default", Tone::Dim);
                }
                row.preview = self.preview(&choice.token);
                row
            })
            .collect()
    }

    /// The setting's row in the list: the value at the right (a switch shows its state), and the
    /// page of values behind ⏎.
    pub fn row(&self, settings: &Settings) -> Row {
        let current = self.current(settings);
        let row = Row::new(Kind::Setting, self.key, self.title)
            .keywords(format!("{} {}", self.keywords, self.key))
            .subtitle(self.note.unwrap_or_default());
        match self.control {
            Control::Toggle => row.toggle(current == "on"),
            Control::Theme(slot) => row
                .value(self.label(&current))
                .opens(Scope::Theme(slot), true),
            Control::Choices => row
                .value(self.label(&current))
                .opens(Scope::Choices(self.key.to_owned()), true),
        }
    }
}

/// Every setting, in the order the palette lists them under their sections.
pub fn all() -> &'static [Setting] {
    &CATALOG
}

/// The setting with this dotted key.
pub fn find(key: &str) -> Option<&'static Setting> {
    CATALOG.iter().find(|setting| setting.key == key)
}

/// The setting that holds the theme of `slot`.
pub(super) fn theme_setting(slot: Slot) -> &'static Setting {
    CATALOG
        .iter()
        .find(|setting| setting.control == Control::Theme(slot))
        .unwrap_or(&CATALOG[0])
}

/// A row per setting, and after the last of the Font section a switch per option the
/// selected font offers ([`font_rows`]).
pub fn rows(settings: &Settings) -> Vec<Row> {
    let last_font = CATALOG
        .iter()
        .rposition(|setting| setting.section == Section::Font);
    let mut rows = Vec::with_capacity(CATALOG.len());
    for (at, setting) in CATALOG.iter().enumerate() {
        rows.push(setting.row(settings));
        if last_font == Some(at) {
            rows.extend(font_rows(settings));
        }
    }
    rows
}

/// The id of a font option's row: the options are the selected font's own and change with it,
/// so they are not in the static table; a row and its edit are resolved from this prefix
/// ([`font_edits`]).
pub(super) const FONT_OPTION: &str = "font.features.";

/// A switch per option of the font in effect: Ligatures, Hide Secrets, the font's sets and
/// Slashed Zero, whichever it offers; a font that offers none (Menlo) adds no row.
pub fn font_rows(settings: &Settings) -> Vec<Row> {
    font_rows_of(
        &font_options::options(settings.font.family.as_deref()),
        settings,
    )
}

fn font_rows_of(options: &[FontOption], settings: &Settings) -> Vec<Row> {
    options
        .iter()
        .map(|option| {
            Row::new(
                Kind::Setting,
                &format!("{FONT_OPTION}{}", option.id()),
                option.title(),
            )
            .keywords(format!(
                "font feature opentype {} {}",
                option.tags().join(" "),
                option.id()
            ))
            .toggle(option.is_on(&settings.font))
        })
        .collect()
}

/// The edits that flip the font option `id` of the font in effect; `None` when the key is not
/// a font option or the font does not offer it.
pub fn font_edits(key: &str, settings: &Settings) -> Option<Vec<SettingsEdit>> {
    let id = key.strip_prefix(FONT_OPTION)?;
    let option = font_options::find(&settings.font, id)?;
    Some(flip(&option, settings))
}

/// The edits that make `option` the opposite of what the file has.
fn flip(option: &FontOption, settings: &Settings) -> Vec<SettingsEdit> {
    option.edits(&settings.font, !option.is_on(&settings.font))
}

fn names<T>(table: &'static [(&'static str, T)]) -> Vec<String> {
    table.iter().map(|(name, _)| (*name).to_owned()).collect()
}

fn pick<T: Copy>(table: &'static [(&'static str, T)], token: &str) -> Option<T> {
    table
        .iter()
        .find(|(name, _)| *name == token)
        .map(|(_, value)| *value)
}

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

fn parse_on_off(token: &str) -> Option<bool> {
    match token {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    }
}

/// A number's token: the shortest text that reads back as the same number (`13`, `1.1`).
fn number_token(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

fn capitalized(token: &str) -> String {
    let mut chars = token.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

fn with_number(token: &str, show: fn(f64) -> String) -> String {
    token.parse::<f64>().map_or_else(|_| token.to_owned(), show)
}

fn points(token: &str) -> String {
    with_number(token, |n| format!("{} pt", number_token(n)))
}

fn multiple(token: &str) -> String {
    with_number(token, |n| format!("{n:.1}×"))
}

fn thousands(token: &str) -> String {
    with_number(token, |n| {
        let digits = number_token(n);
        let (sign, digits) = digits
            .strip_prefix('-')
            .map_or(("", digits.as_str()), |rest| ("-", rest));
        let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
        let mut grouped = String::new();
        for (i, c) in whole.chars().enumerate() {
            if i > 0 && (whole.len() - i) % 3 == 0 {
                grouped.push(',');
            }
            grouped.push(c);
        }
        let dot = if fraction.is_empty() { "" } else { "." };
        format!("{sign}{grouped}{dot}{fraction}")
    })
}

fn contrast(token: &str) -> String {
    with_number(token, |n| {
        if n <= 1.0 {
            "Off".to_owned()
        } else {
            format!("{} : 1", number_token(n))
        }
    })
}

fn theme_label(token: &str) -> String {
    if token == SYSTEM_THEME {
        "Match System".to_owned()
    } else {
        token.to_owned()
    }
}

/// The Font page's choices: the default chain (`""`), then the monospaced
/// families installed on this machine that the font chain opens without a
/// warning — the list the settings window shows, so no row names a font that
/// is not here and answers `font "X" not found` when chosen. Taken **once**
/// per process: `choices` asks for it every time a page opens and the list
/// opens every installed family to answer ([`font_options::families`], which the
/// settings window shares). The value in effect is added when it is not in the
/// list ([`Setting::choices`]).
fn font_families() -> Vec<String> {
    std::iter::once(String::new())
        .chain(font_options::families())
        .collect()
}

fn family_label(token: &str) -> String {
    if token.is_empty() {
        // The default font, named: the app's own default family where its fonts are there,
        // the system's chain otherwise. Not the bare family name — the list has a row of
        // that name too, and two rows alike would not say which one is "no choice made".
        bt_gpu::app_default_family().map_or_else(
            || "System Mono".to_owned(),
            |family| format!("Default ({family})"),
        )
    } else {
        token.to_owned()
    }
}

/// A setting whose values are a model enum's spelling table.
macro_rules! named {
    ($key:literal, $section:ident, $title:literal, $words:literal, $live:literal, $note:expr,
     $ty:ty, $get:expr, $edit:expr) => {
        Setting {
            key: $key,
            section: Section::$section,
            title: $title,
            keywords: $words,
            control: Control::Choices,
            live: $live,
            note: $note,
            read: |s| {
                let get: fn(&Settings) -> $ty = $get;
                get(s).name().to_owned()
            },
            write: |t| {
                let edit: fn($ty) -> SettingsEdit = $edit;
                pick(<$ty>::NAMES, t).map(edit)
            },
            tokens: || names(<$ty>::NAMES),
            label: capitalized,
        }
    };
}

/// An on/off setting.
macro_rules! toggle {
    ($key:literal, $section:ident, $title:literal, $words:literal, $live:literal, $note:expr,
     $get:expr, $edit:expr) => {
        Setting {
            key: $key,
            section: Section::$section,
            title: $title,
            keywords: $words,
            control: Control::Toggle,
            live: $live,
            note: $note,
            read: |s| {
                let get: fn(&Settings) -> bool = $get;
                on_off(get(s)).to_owned()
            },
            write: |t| {
                let edit: fn(bool) -> SettingsEdit = $edit;
                parse_on_off(t).map(edit)
            },
            tokens: || vec!["on".to_owned(), "off".to_owned()],
            label: capitalized,
        }
    };
}

/// A number chosen from a ready list.
macro_rules! number {
    ($key:literal, $section:ident, $title:literal, $words:literal, $live:literal,
     $presets:expr, $get:expr, $edit:expr, $label:expr) => {
        Setting {
            key: $key,
            section: Section::$section,
            title: $title,
            keywords: $words,
            control: Control::Choices,
            live: $live,
            note: None,
            read: |s| {
                let get: fn(&Settings) -> f64 = $get;
                number_token(get(s))
            },
            write: |t| {
                let edit: fn(f64) -> SettingsEdit = $edit;
                t.parse::<f64>().ok().filter(|n| n.is_finite()).map(edit)
            },
            tokens: || {
                let presets: &[f64] = $presets;
                presets.iter().map(|n| number_token(*n)).collect()
            },
            label: $label,
        }
    };
}

const WHILE_MATCHING_SYSTEM: Option<&str> = Some("while Theme is Match System");
const NEW_SHELLS: Option<&str> = Some("new shells only");

static CATALOG: [Setting; 39] = [
    Setting {
        key: "appearance.theme",
        section: Section::Appearance,
        title: "Theme",
        keywords: "color scheme colors dark light appearance palette",
        control: Control::Theme(Slot::Active),
        live: true,
        note: None,
        read: |s| s.theme.clone(),
        write: |t| (!t.is_empty()).then(|| SettingsEdit::Theme(t.to_owned())),
        tokens: Vec::new,
        label: theme_label,
    },
    Setting {
        key: "appearance.dark_theme",
        section: Section::Appearance,
        title: "Theme for Dark Appearance",
        keywords: "system macos night",
        control: Control::Theme(Slot::Dark),
        live: true,
        note: WHILE_MATCHING_SYSTEM,
        read: |s| s.dark_theme.clone(),
        write: |t| (!t.is_empty()).then(|| SettingsEdit::DarkTheme(t.to_owned())),
        tokens: Vec::new,
        label: theme_label,
    },
    Setting {
        key: "appearance.light_theme",
        section: Section::Appearance,
        title: "Theme for Light Appearance",
        keywords: "system macos day",
        control: Control::Theme(Slot::Light),
        live: true,
        note: WHILE_MATCHING_SYSTEM,
        read: |s| s.light_theme.clone(),
        write: |t| (!t.is_empty()).then(|| SettingsEdit::LightTheme(t.to_owned())),
        tokens: Vec::new,
        label: theme_label,
    },
    number!(
        "font.size",
        Font,
        "Font Size",
        "zoom bigger smaller text size points",
        true,
        &[10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 18.0, 20.0],
        |s| s.font.size,
        SettingsEdit::FontSize,
        points
    ),
    Setting {
        key: "font.family",
        section: Section::Font,
        title: "Font",
        keywords: "typeface family mono",
        control: Control::Choices,
        live: true,
        note: None,
        // The empty family is the default chain; the model writes it as `family = ""`.
        read: |s| s.font.family.clone().unwrap_or_default(),
        write: |t| Some(SettingsEdit::FontFamily(t.to_owned())),
        tokens: font_families,
        label: family_label,
    },
    number!(
        "font.line_height",
        Font,
        "Line Height",
        "line height leading rows",
        true,
        &[0.9, 1.0, 1.1, 1.2, 1.4],
        |s| s.font.line_height,
        SettingsEdit::LineHeight,
        multiple
    ),
    toggle!(
        "font.smoothing",
        Font,
        "Bolder Text",
        "smoothing thicken heavier strokes weight terminal",
        false,
        None,
        |s| s.font.smoothing,
        SettingsEdit::FontSmoothing
    ),
    named!(
        "terminal.cursor",
        Terminal,
        "Cursor Shape",
        "caret beam underline block",
        true,
        None,
        CaretShape,
        |s| s.cursor,
        SettingsEdit::Cursor
    ),
    named!(
        "terminal.cursor_blink",
        Terminal,
        "Cursor Blink",
        "caret flash",
        false,
        None,
        CursorBlink,
        |s| s.cursor_blink,
        SettingsEdit::CursorBlink
    ),
    named!(
        "motion.cursor_motion",
        Motion,
        "Cursor Motion",
        "animation glide caret",
        false,
        None,
        CursorMotion,
        |s| s.cursor_motion,
        SettingsEdit::CursorMotion
    ),
    named!(
        "motion.keypress",
        Motion,
        "Typing Effect",
        "animation keypress letters dock",
        false,
        None,
        Keypress,
        |s| s.keypress,
        SettingsEdit::Keypress
    ),
    named!(
        "motion.erase",
        Motion,
        "Delete Effect",
        "animation backspace dock",
        false,
        None,
        Erase,
        |s| s.erase,
        SettingsEdit::Erase
    ),
    named!(
        "motion.dock_arrival",
        Motion,
        "Dock Arrival",
        "animation new tab start",
        false,
        None,
        DockArrival,
        |s| s.dock_arrival,
        SettingsEdit::DockArrival
    ),
    named!(
        "motion.reduce_motion",
        Motion,
        "Reduce Motion",
        "accessibility animation calm",
        false,
        None,
        ReduceMotion,
        |s| s.reduce_motion,
        SettingsEdit::ReduceMotion
    ),
    toggle!(
        "motion.smooth_scroll",
        Motion,
        "Smooth Scrolling",
        "trackpad wheel",
        false,
        None,
        |s| s.smooth_scroll == SmoothScroll::On,
        |on| SettingsEdit::SmoothScroll(if on {
            SmoothScroll::On
        } else {
            SmoothScroll::Off
        })
    ),
    toggle!(
        "motion.scroll_bounce",
        Motion,
        "Bounce at the Ends",
        "elastic rubber band overscroll scroll bar squeeze program view",
        false,
        None,
        |s| s.scroll_bounce == ScrollBounce::On,
        |on| SettingsEdit::ScrollBounce(if on {
            ScrollBounce::On
        } else {
            ScrollBounce::Off
        })
    ),
    named!(
        "appearance.split_style",
        Appearance,
        "Split Style",
        "pane panes cards lines divider",
        true,
        None,
        SplitStyle,
        |s| s.split_style,
        SettingsEdit::SplitStyle
    ),
    named!(
        "appearance.tab_details",
        Appearance,
        "Details Under a Tab",
        "row folder branch question details activity",
        true,
        None,
        TabDetails,
        |s| s.tab_details,
        SettingsEdit::TabDetails
    ),
    toggle!(
        "appearance.dim_unfocused_splits",
        Appearance,
        "Dim the Other Panes",
        "pane veil focus",
        true,
        None,
        |s| s.dim_unfocused_splits,
        SettingsEdit::DimUnfocusedSplits
    ),
    toggle!(
        "appearance.focus_frame",
        Appearance,
        "Frame the Focused Pane",
        "pane focus line border accent outline",
        true,
        None,
        |s| s.focus_frame,
        SettingsEdit::FocusFrame
    ),
    named!(
        "appearance.tab_position",
        Appearance,
        "Tab Position",
        "sidebar vertical tabs side top column",
        true,
        None,
        TabPosition,
        |s| s.tab_position,
        SettingsEdit::TabPosition
    ),
    toggle!(
        "appearance.tab_icons",
        Appearance,
        "Program Icons",
        "program icon tile",
        true,
        None,
        |s| s.tab_icons,
        SettingsEdit::TabIcons
    ),
    named!(
        "appearance.tab_icon_colors",
        Appearance,
        "Tab Icon Colors",
        "brand logo color",
        true,
        None,
        IconColors,
        |s| s.tab_icon_colors,
        SettingsEdit::IconColors
    ),
    named!(
        "appearance.tab_running",
        Appearance,
        "While a Command Runs",
        "tab running command line tail breathe animation",
        true,
        None,
        TabRunning,
        |s| s.tab_running,
        SettingsEdit::TabRunning
    ),
    named!(
        "terminal.scrollbar",
        Terminal,
        "Scroll Bar",
        "scroll bar track",
        false,
        None,
        Scrollbar,
        |s| s.scrollbar,
        SettingsEdit::Scrollbar
    ),
    named!(
        "appearance.content_edge",
        Appearance,
        "Where Text Meets the Tab Bar",
        "top edge fade",
        false,
        None,
        ContentEdge,
        |s| s.content_edge,
        SettingsEdit::ContentEdge
    ),
    number!(
        "appearance.minimum_contrast",
        Appearance,
        "Minimum Contrast",
        "readable legible accessibility",
        false,
        &[1.0, 3.0, 4.5, 7.0],
        |s| s.minimum_contrast,
        SettingsEdit::MinimumContrast,
        contrast
    ),
    toggle!(
        "appearance.resize_in_steps",
        Appearance,
        "Resize a Whole Cell at a Time",
        "window cell resize",
        false,
        None,
        |s| s.resize_in_steps,
        SettingsEdit::ResizeInSteps
    ),
    number!(
        "terminal.scrollback",
        Terminal,
        "Scrollback",
        "history buffer lines",
        false,
        &[1000.0, 10000.0, 50000.0, 100000.0],
        |s| s.scrollback as f64,
        |n| SettingsEdit::Scrollback(n as usize),
        thousands
    ),
    named!(
        "terminal.confirm_close",
        Terminal,
        "Ask Before Closing",
        "close quit warn confirm",
        false,
        None,
        ConfirmClose,
        |s| s.confirm_close,
        SettingsEdit::ConfirmClose
    ),
    named!(
        "terminal.restore_windows",
        Terminal,
        "Reopen After Quitting",
        "session reopen relaunch",
        false,
        None,
        RestoreWindows,
        |s| s.restore_windows,
        SettingsEdit::RestoreWindows
    ),
    named!(
        "terminal.keep_running",
        Terminal,
        "Keep Programs Running",
        "background survive",
        false,
        None,
        KeepRunning,
        |s| s.keep_running,
        SettingsEdit::KeepRunning
    ),
    toggle!(
        "clipboard.osc52",
        Terminal,
        "Let Programs Copy to the Clipboard",
        "clipboard osc52 copy pasteboard",
        false,
        None,
        |s| s.osc52 == Osc52::Copy,
        |on| SettingsEdit::Osc52(if on { Osc52::Copy } else { Osc52::Off })
    ),
    named!(
        "shell.integration",
        Shell,
        "Shell Integration",
        "zsh fish bash dock prompt blocks",
        false,
        NEW_SHELLS,
        ShellIntegration,
        |s| s.shell_integration,
        SettingsEdit::ShellIntegration
    ),
    toggle!(
        "shell.ports",
        Shell,
        "Listening Ports",
        "localhost server listening",
        false,
        None,
        |s| s.shell_ports,
        SettingsEdit::ShellPorts
    ),
    toggle!(
        "shell.python_env",
        Shell,
        "Python Environment",
        "venv conda virtualenv",
        false,
        None,
        |s| s.shell_python_env,
        SettingsEdit::ShellPythonEnv
    ),
    toggle!(
        "shell.node_version",
        Shell,
        "Node Version",
        "nvm nodejs",
        false,
        None,
        |s| s.shell_node_version,
        SettingsEdit::ShellNodeVersion
    ),
    toggle!(
        "remote.integration",
        Remote,
        "Set Up Shell Integration on Servers",
        "ssh server remote integration",
        false,
        None,
        |s| s.remote_integration,
        SettingsEdit::RemoteIntegration
    ),
    named!(
        "remote.stats",
        Remote,
        "Server Load",
        "cpu memory load ssh status",
        false,
        None,
        RemoteStatsMode,
        |s| s.remote_stats.mode,
        SettingsEdit::RemoteStats
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    use bt_core::FontOptions;

    /// What the file says after `edit`, read back by the settings parser — the whole path a
    /// chosen value takes.
    fn written(edit: &SettingsEdit) -> Settings {
        let text = Settings::with_edit("", edit).expect("the edit writes");
        let parsed = Settings::parse(&text).expect("the file reads");
        assert!(
            parsed.diagnostics.is_empty(),
            "{text}: {:?}",
            parsed.diagnostics
        );
        parsed.settings
    }

    fn themes_to_try(setting: &Setting) -> Vec<String> {
        match setting.control() {
            Control::Theme(Slot::Active) => vec!["system".into(), "nord".into()],
            Control::Theme(_) => vec!["nord".into(), "rose-pine".into()],
            _ => Vec::new(),
        }
    }

    #[test]
    fn the_catalog_lists_the_thirty_nine_settings_once_each() {
        assert_eq!(all().len(), 39);
        for (n, setting) in all().iter().enumerate() {
            assert!(
                all()[n + 1..]
                    .iter()
                    .all(|other| other.key() != setting.key()),
                "{} twice",
                setting.key()
            );
            assert_eq!(setting.id(), format!("set.{}", setting.key()));
            assert_eq!(find(setting.key()).map(Setting::key), Some(setting.key()));
        }
        assert!(find("terminal.nonsense").is_none());
    }

    #[test]
    fn every_choice_writes_and_reads_back_as_itself() {
        let defaults = Settings::default();
        for setting in all() {
            let mut tokens: Vec<String> = setting
                .choices(&defaults)
                .into_iter()
                .map(|c| c.token)
                .collect();
            tokens.extend(themes_to_try(setting));
            assert!(!tokens.is_empty(), "{} offers nothing", setting.key());
            for token in tokens {
                let edit = setting
                    .edit(&token)
                    .unwrap_or_else(|| panic!("{} takes {token:?}", setting.key()));
                // The edit lands on the key the catalog names.
                assert_eq!(edit.path(), setting.key(), "{token:?}");
                let read = written(&edit);
                assert_eq!(setting.current(&read), token, "{} {token:?}", setting.key());
            }
        }
    }

    #[test]
    fn a_token_a_setting_cannot_take_is_refused() {
        let cursor = find("terminal.cursor").unwrap();
        assert_eq!(cursor.edit("Beam"), None, "spellings are case sensitive");
        assert_eq!(cursor.edit("wiggle"), None);
        assert_eq!(find("shell.ports").unwrap().edit("maybe"), None);
        assert_eq!(find("font.size").unwrap().edit("big"), None);
        assert_eq!(find("font.size").unwrap().edit("NaN"), None);
        assert_eq!(find("appearance.theme").unwrap().edit(""), None);
    }

    #[test]
    fn the_default_is_the_models_and_is_among_the_choices() {
        for setting in all() {
            let default = setting.default_token();
            if matches!(setting.control(), Control::Theme(_)) {
                continue;
            }
            let choices = setting.choices(&Settings::default());
            let marked: Vec<&Choice> = choices.iter().filter(|c| c.default).collect();
            assert_eq!(marked.len(), 1, "{}", setting.key());
            assert_eq!(marked[0].token, default);
            // With nothing written, the value in effect is the default.
            assert!(marked[0].current, "{}", setting.key());
        }
        assert_eq!(find("terminal.cursor").unwrap().default_token(), "block");
        assert_eq!(find("font.size").unwrap().default_token(), "13");
        assert_eq!(find("appearance.theme").unwrap().default_token(), "system");
    }

    #[test]
    fn a_value_outside_the_ready_list_is_put_in_its_place() {
        let mut settings = Settings::default();
        settings.font.size = 13.5;
        let sizes: Vec<String> = find("font.size")
            .unwrap()
            .choices(&settings)
            .into_iter()
            .map(|c| c.token)
            .collect();
        assert_eq!(sizes[3..6], ["13", "13.5", "14"]);
        settings.scrollback = 2500;
        let lines = find("terminal.scrollback").unwrap().choices(&settings);
        let tokens: Vec<&str> = lines.iter().map(|c| c.token.as_str()).collect();
        assert_eq!(tokens, ["1000", "2500", "10000", "50000", "100000"]);
        assert!(lines[1].current);
        assert!(!lines[2].current);
        // The list is the machine's installed monospaced families, so the test
        // names only the first row and the value it adds: a family no machine has.
        settings.font.family = Some("Zz Not Installed 7".into());
        let families = find("font.family").unwrap().choices(&settings);
        // Next to the default, which opens the list.
        assert_eq!(families[0].token, "");
        assert_eq!(families[1].token, "Zz Not Installed 7");
        assert!(families[1].current);
        // The default chain is chosen when no family is written.
        settings.font.family = None;
        let families = find("font.family").unwrap().choices(&settings);
        assert_eq!(families[0].token, "");
        assert!(families[0].current);
        assert_eq!(families.iter().filter(|c| c.token.is_empty()).count(), 1);
    }

    #[test]
    fn labels_read_as_the_user_says_them() {
        let s = |key: &str, token: &str| find(key).unwrap().label(token);
        assert_eq!(s("terminal.cursor", "beam"), "Beam");
        assert_eq!(s("font.size", "13"), "13 pt");
        assert_eq!(s("font.size", "13.5"), "13.5 pt");
        assert_eq!(s("font.line_height", "1"), "1.0×");
        assert_eq!(s("font.line_height", "1.1"), "1.1×");
        assert_eq!(s("terminal.scrollback", "10000"), "10,000");
        assert_eq!(s("terminal.scrollback", "100000"), "100,000");
        assert_eq!(s("terminal.scrollback", "999"), "999");
        assert_eq!(s("appearance.minimum_contrast", "1"), "Off");
        assert_eq!(s("appearance.minimum_contrast", "4.5"), "4.5 : 1");
        assert_eq!(s("appearance.theme", "system"), "Match System");
        assert_eq!(s("appearance.dark_theme", "nord"), "nord");
        assert_eq!(s("font.family", ""), "System Mono");
        assert_eq!(s("shell.ports", "off"), "Off");
    }

    #[test]
    fn a_row_shows_the_value_in_effect_and_opens_its_page() {
        let settings = Settings {
            cursor: CaretShape::Beam,
            shell_ports: false,
            ..Settings::default()
        };
        let cursor = find("terminal.cursor").unwrap().row(&settings);
        assert_eq!(cursor.id, "set.terminal.cursor");
        assert_eq!(cursor.value.as_deref(), Some("Beam"));
        assert_eq!(cursor.opens, Some(Scope::Choices("terminal.cursor".into())));
        assert!(cursor.enters && cursor.toggle.is_none());
        let ports = find("shell.ports").unwrap().row(&settings);
        assert_eq!(ports.toggle, Some(false));
        assert_eq!((ports.value, ports.opens), (None, None));
        let dark = find("appearance.dark_theme").unwrap().row(&settings);
        assert_eq!(dark.opens, Some(Scope::Theme(Slot::Dark)));
        assert_eq!(dark.subtitle, "while Theme is Match System");
        assert_eq!(dark.value.as_deref(), Some("bateri"));
        // The font's own options are the machine's, so only the table is counted.
        assert_eq!(all().len(), 39);
        assert_eq!(rows(&settings).len(), 39 + font_rows(&settings).len());
    }

    #[test]
    fn bolder_text_is_a_switch_in_the_font_section_that_shows_the_file_and_writes_it() {
        let bolder = find("font.smoothing").expect("the switch is in the catalog");
        assert_eq!(bolder.section(), Section::Font);
        assert_eq!(bolder.control(), Control::Toggle);
        let mut settings = Settings::default();
        assert_eq!(bolder.row(&settings).toggle, Some(false), "off by default");
        assert_eq!(bolder.edit("on"), Some(SettingsEdit::FontSmoothing(true)));
        assert_eq!(bolder.edit("off"), Some(SettingsEdit::FontSmoothing(false)));
        assert_eq!(bolder.edit("maybe"), None);
        settings.font.smoothing = true;
        assert_eq!(bolder.row(&settings).toggle, Some(true));
        // It follows the line spacing, the section's setting before it, and the
        // font's own options come after it.
        let ids: Vec<String> = rows(&settings).iter().map(|r| r.id.clone()).collect();
        let at = ids.iter().position(|id| id == "set.font.smoothing");
        let spacing = ids.iter().position(|id| id == "set.font.line_height");
        assert_eq!(at, spacing.map(|spacing| spacing + 1), "{ids:?}");
        // Passing over the row shows nothing: it would rebuild every pane's atlas.
        assert!(!bolder.live());
    }

    #[test]
    fn a_page_checks_the_value_marks_the_default_and_previews_only_live_settings() {
        let settings = Settings {
            cursor: CaretShape::Beam,
            ..Settings::default()
        };
        let page = find("terminal.cursor").unwrap().choice_rows(&settings);
        let titles: Vec<&str> = page.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles, ["Block", "Underline", "Beam"]);
        assert_eq!(
            page.iter().map(|r| r.checked).collect::<Vec<_>>(),
            [false, false, true]
        );
        assert_eq!(
            page[0].status.as_ref().map(|s| s.text.as_str()),
            Some("default")
        );
        assert!(page[1].status.is_none());
        assert_eq!(
            page[2].preview,
            Some(Preview {
                key: "terminal.cursor".into(),
                token: "beam".into()
            })
        );
        // The typing effect is chosen and written, never shown while passing over.
        let effects = find("motion.keypress").unwrap().choice_rows(&settings);
        assert!(effects.iter().all(|r| r.preview.is_none()));
    }

    #[test]
    fn what_is_live_is_what_the_design_previews() {
        let live: Vec<&str> = all().iter().filter(|s| s.live()).map(|s| s.key()).collect();
        assert_eq!(
            live,
            [
                "appearance.theme",
                "appearance.dark_theme",
                "appearance.light_theme",
                "font.size",
                "font.family",
                "font.line_height",
                "terminal.cursor",
                "appearance.split_style",
                "appearance.tab_details",
                "appearance.dim_unfocused_splits",
                "appearance.focus_frame",
                "appearance.tab_position",
                "appearance.tab_icons",
                "appearance.tab_icon_colors",
                "appearance.tab_running",
            ]
        );
    }

    fn obel_options() -> Vec<FontOption> {
        let feature = |tag: &str, name: Option<&str>| bt_gpu::FontFeature {
            tag: tag.to_owned(),
            name: name.map(str::to_owned),
        };
        font_options::options_of(&[
            feature("calt", None),
            feature("ss01", Some("Mask secrets")),
            feature("ss02", Some("Alternate zero")),
            feature("zero", None),
        ])
    }

    #[test]
    fn a_font_option_is_a_switch_that_shows_the_file_and_flips_it() {
        let options = obel_options();
        let settings = Settings {
            font: FontOptions {
                family: Some("Obel Mono".to_owned()),
                features: [(
                    "obel mono".to_owned(),
                    [("ss01".to_owned(), true), ("calt".to_owned(), false)].into(),
                )]
                .into(),
                ..FontOptions::default()
            },
            ..Settings::default()
        };
        let rows = font_rows_of(&options, &settings);
        let shown: Vec<(&str, &str, Option<bool>)> = rows
            .iter()
            .map(|row| (row.id.as_str(), row.title.as_str(), row.toggle))
            .collect();
        assert_eq!(
            shown,
            [
                ("set.font.features.ligatures", "Ligatures", Some(false)),
                ("set.font.features.secrets", "Hide Secrets", Some(true)),
                ("set.font.features.ss02", "Alternate zero", Some(false)),
                ("set.font.features.zero", "Slashed Zero", Some(false)),
            ]
        );
        // A search by what the option does or by its tag finds it.
        assert!(rows[1].keywords.contains("ss01") && rows[1].keywords.contains("feature"));
        // Flipping reads the file: Hide Secrets is on, so the edit removes the line;
        // Ligatures is off, so the edit removes both lines.
        let tag = |tag: &str, on| SettingsEdit::FontFeature {
            family: "Obel Mono".to_owned(),
            tag: tag.to_owned(),
            on,
        };
        assert_eq!(flip(&options[1], &settings), [tag("ss01", None)]);
        assert_eq!(flip(&options[0], &settings), [tag("calt", None)]);
        assert_eq!(flip(&options[3], &settings), [tag("zero", Some(true))]);
    }

    #[test]
    fn the_font_rows_follow_the_section_and_a_font_without_options_adds_none() {
        // Menlo lists no feature (and a backend that cannot read tables lists none): the
        // table's rows and nothing else.
        let menlo = Settings {
            font: FontOptions {
                family: Some("Menlo".to_owned()),
                ..FontOptions::default()
            },
            ..Settings::default()
        };
        assert!(font_rows(&menlo).is_empty());
        assert_eq!(rows(&menlo).len(), all().len());
        // The options sit right after the Font section, whichever the machine's default is.
        let rows = rows(&Settings::default());
        let at = rows.iter().position(|row| row.id == "set.font.line_height");
        let last = rows
            .iter()
            .rposition(|row| row.id.starts_with("set.font.features."));
        if let (Some(at), Some(last)) = (at, last) {
            assert!(last > at);
            assert!(
                rows[at + 1..=last]
                    .iter()
                    .all(|row| row.id.starts_with("set.font.features."))
            );
        }
        // An id the font does not offer is not a write.
        assert_eq!(font_edits("font.features.ss99", &menlo), None);
        assert_eq!(font_edits("font.size", &menlo), None);
    }
}
