//! `[hotkey_window]`: the bateri window a shortcut slides in from the edge of the screen.
//!
//! This file owns the **keys** and their reading. What the keys do — which screen, the frame of
//! each position, when the window hides — is `bt-shell-common`'s `hotkey`, which sees no
//! AppKit and is tried without a window.
//!
//! `position`, `depth` and `span` are written for the edge the window comes from: `depth` is
//! how far in from the edge, `span` how far along it, both shares of the screen's usable area
//! (the menu bar and the Dock are not in it). `width` and `height` would mean different things
//! on `left` and `right`, and a key that is written is never taken out, so the name is
//! chosen once.
//!
//! The shortcut is the one rule of [`Shortcut::for_global`]: a chord with a modifier (not Shift
//! alone) or a function key, or one modifier tapped twice. `""` is none.

use toml_edit::TableLike;

use super::{Diagnostic, Shortcut, boolean, kind, line_of, name_in, named_enum, ranged_integer};

/// `[hotkey_window] position`: the edge the window comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HotkeyPosition {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
    /// In the middle of the screen; it slides in from the top.
    Center,
}

impl HotkeyPosition {
    /// The single list of spellings in the settings file.
    pub const NAMES: &'static [(&'static str, Self)] = &[
        ("top", Self::Top),
        ("bottom", Self::Bottom),
        ("left", Self::Left),
        ("right", Self::Right),
        ("center", Self::Center),
    ];

    /// The spelling in the settings file.
    pub fn name(self) -> &'static str {
        name_in(Self::NAMES, self)
    }
}

/// `[hotkey_window] screen`: which screen the window opens on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HotkeyScreen {
    /// The one the pointer is on.
    #[default]
    Pointer,
    /// The one with the menu bar.
    Primary,
}

impl HotkeyScreen {
    /// The single list of spellings in the settings file.
    pub const NAMES: &'static [(&'static str, Self)] =
        &[("pointer", Self::Pointer), ("primary", Self::Primary)];

    /// The spelling in the settings file.
    pub fn name(self) -> &'static str {
        name_in(Self::NAMES, self)
    }
}

/// `[hotkey_window] spaces`: which Space the window opens in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HotkeySpaces {
    /// The one in front now.
    #[default]
    Follow,
    /// The one it was opened in.
    Stay,
}

impl HotkeySpaces {
    /// The single list of spellings in the settings file.
    pub const NAMES: &'static [(&'static str, Self)] =
        &[("follow", Self::Follow), ("stay", Self::Stay)];

    /// The spelling in the settings file.
    pub fn name(self) -> &'static str {
        name_in(Self::NAMES, self)
    }
}

/// `depth` and `span`'s accepted range, in percent of the screen. **A design constant**: below a
/// tenth the window is a sliver nobody can use (a tab row alone is that tall), and the drag that
/// sets the size ends there too.
pub const HOTKEY_SIZE_RANGE: std::ops::RangeInclusive<u8> = 10..=100;

/// The name of the one hotkey window there is. A name is in the session file and in the
/// settings' shape (`[hotkey_window.<name>]` later), so more than one can be added without
/// changing either.
pub const HOTKEY_DEFAULT_NAME: &str = "default";

/// `[hotkey_window]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotkeyWindowSettings {
    /// `shortcut`: what opens and hides the window from any application; `None` is no shortcut
    /// (the window is opened from the menu, the palette and `bateri hotkey`). Always the
    /// result of [`Shortcut::for_global`]: a chord, or [`Shortcut::Twice`].
    pub shortcut: Option<Shortcut>,
    /// `position`: the edge it comes from.
    pub position: HotkeyPosition,
    /// `depth`: how far in from the edge, in percent of the screen.
    pub depth: u8,
    /// `span`: how far along the edge, in percent of the screen.
    pub span: u8,
    /// `screen`: which screen it opens on.
    pub screen: HotkeyScreen,
    /// `spaces`: which Space it opens in.
    pub spaces: HotkeySpaces,
    /// `hide_on_click_away`: whether it hides when the focus leaves bateri.
    pub hide_on_click_away: bool,
    /// `animate`: whether it slides.
    pub animate: bool,
}

impl Default for HotkeyWindowSettings {
    fn default() -> Self {
        Self {
            shortcut: None,
            position: HotkeyPosition::default(),
            depth: 50,
            span: 100,
            screen: HotkeyScreen::default(),
            spaces: HotkeySpaces::default(),
            hide_on_click_away: true,
            animate: true,
        }
    }
}

/// The section's keys. A value that is not accepted keeps `fallback`'s for its own key alone
/// ([`Settings::parse_keeping`](super::Settings::parse_keeping)).
pub(super) fn read(
    text: &str,
    table: &dyn TableLike,
    fallback: &HotkeyWindowSettings,
    diagnostics: &mut Vec<Diagnostic>,
) -> HotkeyWindowSettings {
    let mut window = HotkeyWindowSettings::default();
    if let Some(item) = table.get("shortcut") {
        window.shortcut = shortcut(text, item, fallback.shortcut, diagnostics);
    }
    if let Some(item) = table.get("position") {
        window.position = named_enum(
            text,
            item,
            "hotkey_window.position",
            HotkeyPosition::NAMES,
            fallback.position,
            diagnostics,
        );
    }
    if let Some(item) = table.get("depth") {
        window.depth = ranged_integer(
            text,
            item,
            "hotkey_window.depth",
            HOTKEY_SIZE_RANGE,
            fallback.depth,
            diagnostics,
        );
    }
    if let Some(item) = table.get("span") {
        window.span = ranged_integer(
            text,
            item,
            "hotkey_window.span",
            HOTKEY_SIZE_RANGE,
            fallback.span,
            diagnostics,
        );
    }
    if let Some(item) = table.get("screen") {
        window.screen = named_enum(
            text,
            item,
            "hotkey_window.screen",
            HotkeyScreen::NAMES,
            fallback.screen,
            diagnostics,
        );
    }
    if let Some(item) = table.get("spaces") {
        window.spaces = named_enum(
            text,
            item,
            "hotkey_window.spaces",
            HotkeySpaces::NAMES,
            fallback.spaces,
            diagnostics,
        );
    }
    if let Some(item) = table.get("hide_on_click_away") {
        window.hide_on_click_away = boolean(
            text,
            item,
            "hotkey_window.hide_on_click_away",
            fallback.hide_on_click_away,
            diagnostics,
        );
    }
    if let Some(item) = table.get("animate") {
        window.animate = boolean(
            text,
            item,
            "hotkey_window.animate",
            fallback.animate,
            diagnostics,
        );
    }
    window
}

/// `shortcut`: `""` is none, otherwise a chord or one modifier twice. Not a shortcut the rule of
/// [`Shortcut::for_global`] accepts → the key keeps `fallback` and says why.
fn shortcut(
    text: &str,
    item: &toml_edit::Item,
    fallback: Option<Shortcut>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Shortcut> {
    let line = item.span().and_then(|span| line_of(text, span.start));
    let message = match item.as_str() {
        Some("") => return None,
        Some(written) => match Shortcut::parse_global(written) {
            Ok(shortcut) => return Some(shortcut),
            Err(error) => format!("`hotkey_window.shortcut`: {error}"),
        },
        None => format!(
            "`hotkey_window.shortcut` must be a string, found {}",
            kind(item)
        ),
    };
    diagnostics.push(Diagnostic {
        key: Some("hotkey_window.shortcut"),
        line,
        message,
    });
    fallback
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{Chord, Key, Modifier, Mods, NamedKey, Settings, SettingsEdit};

    fn read_text(text: &str) -> (HotkeyWindowSettings, Vec<Diagnostic>) {
        let parsed = Settings::parse(text).expect("the text is TOML");
        (parsed.settings.hotkey_window, parsed.diagnostics)
    }

    fn chord(mods: Mods, key: Key) -> Shortcut {
        Shortcut::Chord(Chord::new(mods, key))
    }

    #[test]
    fn what_is_not_written_is_the_default() {
        let window = HotkeyWindowSettings::default();
        assert_eq!(window.shortcut, None, "no shortcut until one is recorded");
        assert_eq!(window.position, HotkeyPosition::Top);
        assert_eq!((window.depth, window.span), (50, 100));
        assert_eq!(window.screen, HotkeyScreen::Pointer);
        assert_eq!(window.spaces, HotkeySpaces::Follow);
        assert!(window.hide_on_click_away && window.animate);
        let (read, diagnostics) = read_text("");
        assert_eq!((read, diagnostics.len()), (window, 0));
    }

    #[test]
    fn the_template_says_the_defaults() {
        let (read, diagnostics) = read_text(Settings::TEMPLATE);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(read, HotkeyWindowSettings::default());
    }

    #[test]
    fn every_key_is_read() {
        let (read, diagnostics) = read_text(
            "[hotkey_window]\nshortcut = \"option+space\"\nposition = \"left\"\ndepth = 30\n\
             span = 80\nscreen = \"primary\"\nspaces = \"stay\"\nhide_on_click_away = false\n\
             animate = false\n",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(
            read,
            HotkeyWindowSettings {
                shortcut: Some(chord(
                    Mods::NONE.with(Modifier::Option),
                    Key::Named(NamedKey::Space)
                )),
                position: HotkeyPosition::Left,
                depth: 30,
                span: 80,
                screen: HotkeyScreen::Primary,
                spaces: HotkeySpaces::Stay,
                hide_on_click_away: false,
                animate: false,
            }
        );
        // An inline table is the same section.
        let (inline, _) = read_text("hotkey_window = { position = \"bottom\" }");
        assert_eq!(inline.position, HotkeyPosition::Bottom);
    }

    #[test]
    fn a_value_that_is_not_accepted_spoils_its_own_key_alone() {
        let (read, diagnostics) = read_text(
            "[hotkey_window]\nposition = \"diagonal\"\ndepth = 5\nspan = 101\nscreen = 3\n\
             spaces = \"both\"\nhide_on_click_away = \"yes\"\nanimate = false\n\
             shortcut = \"a\"\n",
        );
        let keys: Vec<_> = diagnostics.iter().filter_map(|d| d.key).collect();
        assert_eq!(
            keys,
            [
                "hotkey_window.shortcut",
                "hotkey_window.position",
                "hotkey_window.depth",
                "hotkey_window.span",
                "hotkey_window.screen",
                "hotkey_window.spaces",
                "hotkey_window.hide_on_click_away",
            ]
        );
        // The others are read, and the spoiled ones are the defaults.
        assert!(!read.animate);
        assert_eq!(
            HotkeyWindowSettings {
                animate: true,
                ..read
            },
            HotkeyWindowSettings::default()
        );
    }

    #[test]
    fn a_value_that_is_not_accepted_keeps_the_previous_one() {
        let before = Settings::parse(
            "[hotkey_window]\nshortcut = \"f12\"\nposition = \"bottom\"\ndepth = 40\n",
        )
        .unwrap()
        .settings;
        let parsed = Settings::parse_keeping(
            "[hotkey_window]\nshortcut = \"x\"\nposition = \"up\"\ndepth = 400\nspan = 60\n",
            &before,
        )
        .unwrap();
        let kept = parsed.settings.hotkey_window;
        assert_eq!(kept.shortcut, before.hotkey_window.shortcut);
        assert_eq!(kept.position, HotkeyPosition::Bottom);
        assert_eq!((kept.depth, kept.span), (40, 60));
        assert_eq!(parsed.diagnostics.len(), 3);
        // A section of the wrong kind reads nothing: the whole of it stays.
        let parsed = Settings::parse_keeping("hotkey_window = 5\n", &before).unwrap();
        assert_eq!(parsed.settings.hotkey_window, before.hotkey_window);
    }

    #[test]
    fn a_shortcut_for_every_application_is_a_modifier_and_a_key_or_a_function_key() {
        let option = Mods::NONE.with(Modifier::Option);
        let accepted = [
            ("option+space", chord(option, Key::Named(NamedKey::Space))),
            (
                "ctrl+`",
                chord(Mods::NONE.with(Modifier::Ctrl), Key::Char('`')),
            ),
            (
                "shift+cmd+k",
                chord(
                    Mods::NONE.with(Modifier::Shift).with(Modifier::Cmd),
                    Key::Char('k'),
                ),
            ),
            ("f12", chord(Mods::NONE, Key::Named(NamedKey::F(12)))),
            ("f20", chord(Mods::NONE, Key::Named(NamedKey::F(20)))),
            (
                "shift+f13",
                chord(
                    Mods::NONE.with(Modifier::Shift),
                    Key::Named(NamedKey::F(13)),
                ),
            ),
            ("option+option", Shortcut::Twice(Modifier::Option)),
            // One modifier's name alone is the tap twice.
            ("option", Shortcut::Twice(Modifier::Option)),
            ("shift", Shortcut::Twice(Modifier::Shift)),
            ("ctrl+ctrl", Shortcut::Twice(Modifier::Ctrl)),
        ];
        for (text, shortcut) in accepted {
            assert_eq!(Shortcut::parse_global(text), Ok(shortcut), "{text}");
        }
        for refused in [
            "a",
            "space",
            "shift+a",
            "shift+space",
            "shift+tab",
            "tab",
            "option+cmd",
            "ctrl+option+shift",
            "f21",
            "",
        ] {
            assert!(Shortcut::parse_global(refused).is_err(), "{refused:?}");
        }
    }

    #[test]
    fn the_file_keeps_its_comments_and_unknown_keys_when_the_size_is_written() {
        let text = "# mine\n[future]\nkey = 1\n\n[hotkey_window]\n# where it comes from\n\
                    position = \"left\"\nnotes = \"kept\"\ndepth = 40\n";
        let written = Settings::with_edit(text, &SettingsEdit::HotkeyDepth(65)).unwrap();
        assert_eq!(written, text.replace("depth = 40", "depth = 65"));
        let written = Settings::with_edit(&written, &SettingsEdit::HotkeySpan(70)).unwrap();
        assert_eq!(
            written,
            format!("{}span = 70\n", text.replace("depth = 40", "depth = 65"))
        );
        assert_eq!(read_text(&written).0.span, 70);
    }

    #[test]
    fn the_size_is_written_where_there_is_no_section_and_no_key() {
        // Most files written before this section have none.
        let none = Settings::with_edit(
            "[terminal]\nscrollback = 5\n",
            &SettingsEdit::HotkeyDepth(30),
        )
        .unwrap();
        assert!(none.starts_with("[terminal]\nscrollback = 5\n"), "{none}");
        assert_eq!(read_text(&none).0.depth, 30);
        assert_eq!(
            Settings::with_edit("", &SettingsEdit::HotkeySpan(90)).map(|t| read_text(&t).0.span),
            Ok(90)
        );
        // The section is there and the key is a comment (the template's example form).
        let commented = Settings::with_edit(
            "[hotkey_window]\n# depth = 50\nposition = \"top\"\n",
            &SettingsEdit::HotkeyDepth(20),
        )
        .unwrap();
        let (read, diagnostics) = read_text(&commented);
        assert!(diagnostics.is_empty(), "{commented}");
        assert_eq!((read.depth, read.position), (20, HotkeyPosition::Top));
        assert!(commented.contains("# depth = 50"), "{commented}");
    }

    #[test]
    fn a_change_of_the_section_is_a_change_of_its_own() {
        let before = Settings::default();
        let mut after = before.clone();
        assert!(!before.changes(&after).hotkey);
        after.hotkey_window.depth = 70;
        let changes = before.changes(&after);
        assert!(changes.hotkey);
        assert_eq!(
            changes,
            crate::settings::Changes {
                hotkey: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn the_shortcut_is_a_plain_key_the_command_may_read() {
        let key = crate::settings::plain_key("hotkey_window.shortcut").expect("a plain key");
        assert_eq!(
            key.default,
            Some(crate::settings::PlainValue::Text(String::new()))
        );
        // Writing it is the window's; the page writes `""` back for none.
        let written = Settings::with_plain(
            "",
            key,
            Some(&crate::settings::PlainValue::Text("f12".into())),
        )
        .unwrap();
        assert!(written.contains("shortcut = \"f12\""), "{written}");
        assert!(
            Settings::with_plain(
                "",
                key,
                Some(&crate::settings::PlainValue::Text("a".into()))
            )
            .is_err()
        );
    }
}
