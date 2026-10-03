//! Diagnostics shown in the window subtitle: one slot per source.
//!
//! There is no modal alert: the settings file is edited live and a window opening on every save
//! would stop the user. In a window without a toolbar the subtitle
//! is drawn **on the same line** as the title ("bateri – …"), so the text must stay short.
//!
//! **A slot empties only when its own source is fixed:** a successful read of an unrelated
//! source must not erase another source's diagnostic. The subtitle's only writer is
//! `app::AppDelegate::post_notices`; this module only builds the text.

use std::collections::BTreeMap;

use bt_gpu::FontNotice;

/// Where the diagnostic came from. The order is which slot shows first in the subtitle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    /// View ▸ Theme ▸ writing to the file: the file could not be read, parsed or written.
    /// **First in order**, because it answers something the user just did; the settings file's
    /// diagnostic must not push it into "(+1 more)".
    Write,
    /// `settings.toml`: could not be read or parsed, or a key was not
    /// accepted.
    Settings,
    /// The selected theme: not found, its file could not be read or parsed, or
    /// one of its colors was not accepted.
    Theme,
    /// The opened font: the requested family was not found or is not monospaced. Its source is
    /// the atlas, not a file; it is rewritten after every build of the atlas.
    Font,
}

/// The messages of the font slot.
///
/// The text lives here, not in `bt-gpu`: the subtitle's other strings are also built in this
/// crate and the language rule (UI strings in English) is applied in one place.
pub fn font_messages(notice: Option<FontNotice>) -> Vec<String> {
    match notice {
        None => Vec::new(),
        Some(FontNotice::FamilyNotFound { requested, using }) => {
            vec![format!("font \"{requested}\" not found; using {using}")]
        }
        Some(FontNotice::NotMonospaced { family }) => {
            vec![format!(
                "font \"{family}\" is not monospaced; text may not line up"
            )]
        }
    }
}

/// The filled slots; an empty slot does not stay in the map.
#[derive(Debug, Default)]
pub struct Notices {
    slots: BTreeMap<Source, Vec<String>>,
}

impl Notices {
    /// The messages in the source's slot; an empty slot is an empty slice.
    pub fn get(&self, source: Source) -> &[String] {
        self.slots.get(&source).map_or(&[], Vec::as_slice)
    }

    /// Rewrites the source's slot **entirely**; an empty list empties the slot.
    /// Does not touch another source's slot.
    pub fn replace(&mut self, source: Source, messages: Vec<String>) {
        if messages.is_empty() {
            self.slots.remove(&source);
        } else {
            self.slots.insert(source, messages);
        }
    }

    /// The subtitle's text: `""` if empty, otherwise the first diagnostic and the count of the
    /// rest.
    ///
    /// They are not all written side by side: it would be cut off on a single line. The full
    /// set is on stderr.
    pub fn subtitle(&self) -> String {
        let mut messages = self.slots.values().flatten();
        let Some(first) = messages.next() else {
            return String::new();
        };
        match messages.count() {
            0 => first.clone(),
            rest => format!("{first} (+{rest} more)"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_notices_clear_the_subtitle() {
        let mut notices = Notices::default();
        assert_eq!(notices.subtitle(), "");
        notices.replace(Source::Settings, vec!["bozuk".to_owned()]);
        assert_eq!(notices.subtitle(), "bozuk");
        // When the source is fixed the slot empties, and so does the subtitle.
        notices.replace(Source::Settings, Vec::new());
        assert_eq!(notices.subtitle(), "");
    }

    #[test]
    fn several_notices_show_the_first_and_the_rest_count() {
        let mut notices = Notices::default();
        notices.replace(
            Source::Settings,
            vec!["ilk".to_owned(), "ikinci".to_owned(), "üçüncü".to_owned()],
        );
        assert_eq!(notices.subtitle(), "ilk (+2 more)");
        // Rewriting is not appending: the old three go.
        notices.replace(Source::Settings, vec!["tek".to_owned()]);
        assert_eq!(notices.subtitle(), "tek");
    }

    #[test]
    fn a_source_clears_only_its_own_slot() {
        let mut notices = Notices::default();
        notices.replace(Source::Theme, vec!["tema".to_owned()]);
        notices.replace(Source::Settings, vec!["ayar".to_owned()]);
        // The settings slot shows first; the theme slot is in the count.
        assert_eq!(notices.subtitle(), "ayar (+1 more)");
        // The settings file was fixed: the theme's diagnostic stays in place.
        notices.replace(Source::Settings, Vec::new());
        assert_eq!(notices.subtitle(), "tema");
        assert_eq!(notices.get(Source::Theme), ["tema"]);
        assert!(notices.get(Source::Settings).is_empty());
    }

    #[test]
    fn write_notice_comes_first() {
        let mut notices = Notices::default();
        notices.replace(Source::Settings, vec!["ayar".to_owned()]);
        notices.replace(Source::Write, vec!["yazma".to_owned()]);
        assert_eq!(notices.subtitle(), "yazma (+1 more)");
    }

    #[test]
    fn font_notices_come_after_the_files() {
        let mut notices = Notices::default();
        notices.replace(
            Source::Font,
            font_messages(Some(FontNotice::FamilyNotFound {
                requested: "Fira".to_owned(),
                using: "Menlo".to_owned(),
            })),
        );
        assert_eq!(notices.subtitle(), "font \"Fira\" not found; using Menlo");
        notices.replace(Source::Theme, vec!["tema".to_owned()]);
        assert_eq!(notices.subtitle(), "tema (+1 more)");
        assert_eq!(
            font_messages(Some(FontNotice::NotMonospaced {
                family: "Helvetica".to_owned()
            })),
            ["font \"Helvetica\" is not monospaced; text may not line up"]
        );
        assert!(font_messages(None).is_empty());
    }
}
