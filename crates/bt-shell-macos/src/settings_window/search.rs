//! The sidebar's search: every row of every page, found by its name, by what it says about the
//! value it has now, by the names of its choices, and, when the query looks like one, by its key in
//! the file (`font.size`, `tab_icons`).
//!
//! The matcher is pure over a list of [`Entry`]s the window builds at each refresh from the page
//! table ([`entries`]); the results are rows of one box, each a click away from its page
//! ([`ResultRow`]).

use std::cell::Cell;

use bt_core::Settings;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSColor, NSControl, NSEvent, NSFont, NSFontAttributeName, NSForegroundColorAttributeName,
    NSStringDrawing, NSView,
};
use objc2_foundation::{
    NSAttributedStringKey, NSDictionary, NSInteger, NSObjectProtocol, NSPoint, NSRect, NSSize,
    NSString,
};

use super::{Category, Key, Resolved, choice_labels, pages, row_description};

/// A row of the window as the search sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Entry {
    pub(super) page: Category,
    /// The row's key; `None` for the Keyboard page's one entry.
    pub(super) key: Option<Key>,
    pub(super) name: String,
    /// What the row says about the value it has now.
    pub(super) description: String,
    /// The names of the row's choices, space apart.
    options: String,
    /// The key as the file spells it (`terminal.cursor`).
    file_key: &'static str,
}

/// The entries of every page, in the pages' order, for the file's `settings`: a row that has no
/// meaning now (Light and Dark under a theme of its own) is not offered, because opening it would
/// show nothing.
pub(super) fn entries(settings: &Settings, resolved: Resolved) -> Vec<Entry> {
    let mut entries = Vec::new();
    for page in Category::ALL {
        if page == Category::Keyboard {
            entries.push(Entry {
                page,
                key: None,
                name: "Keyboard shortcuts".to_owned(),
                description: String::new(),
                options: "shortcuts keys".to_owned(),
                file_key: "keys",
            });
            continue;
        }
        for group in pages::groups(page) {
            for item in group.items {
                let pages::Item::Key(key) = *item else {
                    continue;
                };
                if pages::dependency(key, settings).hidden {
                    continue;
                }
                let (name, options) = if key == Key::FontFeatures {
                    let offered = crate::font_options::options(settings.font.family.as_deref());
                    if offered.is_empty() {
                        continue;
                    }
                    let titles: Vec<&str> = offered.iter().map(|option| option.title()).collect();
                    ("Font options".to_owned(), titles.join(" "))
                } else {
                    (pages::label(key).to_owned(), choice_labels(key).join(" "))
                };
                entries.push(Entry {
                    page,
                    key: Some(key),
                    name,
                    description: row_description(key, settings, resolved)
                        .unwrap_or_default()
                        .to_owned(),
                    options,
                    file_key: key.path(),
                });
            }
        }
    }
    entries
}

/// Whether `query` finds `entry`: every word of the query is in the name, the description, the
/// choices or — only when the query has a `.` or `_` in it — the file's key; case does not
/// matter. An empty query finds nothing.
pub(super) fn matches(query: &str, entry: &Entry) -> bool {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return false;
    }
    let keyed = needle.contains(['.', '_']);
    let haystack = format!(
        "{} {} {} {}",
        entry.name,
        entry.description,
        entry.options,
        if keyed { entry.file_key } else { "" }
    )
    .to_lowercase();
    haystack.contains(&needle)
}

/// The entries `query` finds, in their order.
pub(super) fn hits<'a>(query: &str, entries: &'a [Entry]) -> Vec<&'a Entry> {
    entries
        .iter()
        .filter(|entry| matches(query, entry))
        .collect()
}

// ─── The results ───────────────────────────────────────────────────────────────────────────────

/// A result row's height without and with a description, and its text's margins.
const HEIGHT_PLAIN: f64 = 38.0;
const HEIGHT_DESCRIBED: f64 = 62.0;
const PAD_X: f64 = 12.0;
const PAD_Y: f64 = 10.0;

pub(super) struct RowIvars {
    name: String,
    description: String,
    page: &'static str,
    /// Whether a hairline goes over the row: every row but the first.
    separated: bool,
    tag: Cell<NSInteger>,
}

define_class!(
    /// A found row: its name, what it says and the page it is on; a click opens the page.
    // SAFETY: NSControl has no subclassing requirement beyond `tag`, which the base class does
    // not keep for a control without a cell (it is answered here); Drop is not implemented.
    #[unsafe(super(NSControl))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriSettingsResultRow"]
    #[ivars = RowIvars]
    pub(super) struct ResultRow;

    unsafe impl NSObjectProtocol for ResultRow {}

    impl ResultRow {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(tag))]
        fn tag(&self) -> NSInteger {
            self.ivars().tag.get()
        }

        #[unsafe(method(setTag:))]
        fn set_tag(&self, tag: NSInteger) {
            self.ivars().tag.set(tag);
        }

        #[unsafe(method(intrinsicContentSize))]
        fn intrinsic_content_size(&self) -> NSSize {
            // No width of its own: the box gives it its width.
            NSSize::new(
                -1.0,
                if self.ivars().description.is_empty() {
                    HEIGHT_PLAIN
                } else {
                    HEIGHT_DESCRIBED
                },
            )
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            // SAFETY: the action and target are the ones the window gave this control; the target
            // is a weak reference to an object that lives for the whole process.
            unsafe {
                let _ = self.sendAction_to(self.action(), self.target().as_deref());
            }
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            self.paint();
        }
    }
);

impl ResultRow {
    pub(super) fn new(mtm: MainThreadMarker, entry: &Entry, separated: bool) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(RowIvars {
            name: entry.name.clone(),
            description: entry.description.clone(),
            page: entry.page.title(),
            separated,
            tag: Cell::new(0),
        });
        // SAFETY: `init` is NSControl's designated initializer (zero frame); the size comes from
        // the box and the intrinsic height.
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        this
    }

    fn paint(&self) {
        let ivars = self.ivars();
        let bounds = self.bounds();
        if ivars.separated {
            NSColor::separatorColor().setFill();
            objc2_app_kit::NSBezierPath::fillRect(NSRect::new(
                NSPoint::new(PAD_X, 0.0),
                NSSize::new(bounds.size.width - 2.0 * PAD_X, 1.0),
            ));
        }
        let page_attributes = attributes(
            &NSFont::systemFontOfSize(12.0),
            &NSColor::secondaryLabelColor(),
        );
        let name_attributes = attributes(
            &NSFont::systemFontOfSize(NSFont::systemFontSize()),
            &NSColor::labelColor(),
        );
        let note_attributes = attributes(
            &NSFont::systemFontOfSize(NSFont::smallSystemFontSize()),
            &NSColor::secondaryLabelColor(),
        );
        let page = NSString::from_str(ivars.page);
        // SAFETY: the attribute dictionaries hold the keys AppKit reads, with the value types it
        // documents; drawing happens inside `drawRect:`.
        unsafe {
            let page_size = page.sizeWithAttributes(Some(&page_attributes));
            page.drawAtPoint_withAttributes(
                NSPoint::new(
                    bounds.size.width - PAD_X - page_size.width,
                    (bounds.size.height - page_size.height) / 2.0,
                ),
                Some(&page_attributes),
            );
            let text_width = bounds.size.width - 3.0 * PAD_X - page_size.width;
            let name = NSString::from_str(&ivars.name);
            name.drawAtPoint_withAttributes(NSPoint::new(PAD_X, PAD_Y), Some(&name_attributes));
            if !ivars.description.is_empty() {
                // Two lines of the description at most; what does not fit in the rectangle is
                // not drawn.
                NSString::from_str(&ivars.description).drawInRect_withAttributes(
                    NSRect::new(
                        NSPoint::new(PAD_X, PAD_Y + 19.0),
                        NSSize::new(text_width.max(120.0), 30.0),
                    ),
                    Some(&note_attributes),
                );
            }
        }
    }
}

fn attributes(
    font: &NSFont,
    color: &NSColor,
) -> Retained<NSDictionary<NSAttributedStringKey, AnyObject>> {
    let values: [&AnyObject; 2] = [color.as_ref(), font.as_ref()];
    // SAFETY: AppKit's two attribute keys (extern statics), each with the value type it
    // documents — an `NSColor` and an `NSFont`.
    unsafe {
        NSDictionary::<NSAttributedStringKey, AnyObject>::from_slices(
            &[NSForegroundColorAttributeName, NSFontAttributeName],
            &values,
        )
    }
}

/// The view as a plain view, for adding to a stack.
pub(super) fn as_view(row: &ResultRow) -> &NSView {
    row.as_super().as_super()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, description: &str, options: &str, file_key: &'static str) -> Entry {
        Entry {
            page: Category::General,
            key: None,
            name: name.to_owned(),
            description: description.to_owned(),
            options: options.to_owned(),
            file_key,
        }
    }

    /// A query finds a row by its name, its description or its choices, in any case.
    #[test]
    fn a_query_finds_the_name_the_description_and_the_choices() {
        let row = entry(
            "Where text meets the tab bar",
            "The first line fades under the bar.",
            "Fade Line Cut",
            "appearance.content_edge",
        );
        assert!(matches("TAB BAR", &row));
        assert!(matches("fades under", &row));
        assert!(matches("cut", &row));
        assert!(!matches("cursor", &row));
        assert!(!matches("   ", &row));
        assert!(!matches("", &row));
    }

    /// The key in the file is searched only when the query looks like a key.
    #[test]
    fn the_key_in_the_file_is_found_only_by_a_query_that_looks_like_one() {
        let row = entry("Size", "", "", "font.size");
        assert!(matches("font.size", &row));
        assert!(!matches("font_", &row));
        assert!(!matches("font", &row));
        let row = entry("Program icons", "", "", "appearance.tab_icons");
        assert!(matches("tab_icons", &row));
        assert!(!matches("tab", &row));
    }

    /// Every page is searchable: a row of the page table is an entry, in the table's order, and the
    /// Keyboard page is found by "keyboard".
    #[test]
    fn every_row_of_every_page_is_an_entry() {
        let settings = Settings::default();
        let resolved = Resolved {
            reduce: false,
            scrollbar: bt_gpu::ScrollbarMode::Auto,
        };
        let all = entries(&settings, resolved);
        let mut seen = Vec::new();
        for entry in &all {
            if let Some(key) = entry.key {
                assert!(!seen.contains(&key), "{key:?} twice");
                seen.push(key);
                assert!(!entry.name.is_empty(), "{key:?} has no name");
            }
        }
        // Every key but the font's options (a test run has no font that offers any) is there; a
        // theme of its own chosen takes Light and Dark away, they have no meaning then.
        assert!(
            seen.len() >= Key::ALL.len() - 1,
            "{} of {}",
            seen.len(),
            Key::ALL.len()
        );
        assert!(seen.contains(&Key::LightTheme));
        let own = Settings {
            theme: "bateri".to_owned(),
            ..Settings::default()
        };
        let own = entries(&own, resolved);
        assert!(
            !own.iter()
                .any(|entry| matches!(entry.key, Some(Key::LightTheme | Key::DarkTheme)))
        );
        assert_eq!(
            hits("keyboard", &all).first().map(|entry| entry.page),
            Some(Category::Keyboard)
        );
        // "Blink" finds the two rows about it, Blink and Blink speed, with their page.
        let blink: Vec<_> = hits("blink", &all)
            .into_iter()
            .map(|entry| entry.key)
            .collect();
        assert!(blink.contains(&Some(Key::Blink)));
        assert!(blink.contains(&Some(Key::BlinkSpeed)));
    }
}
