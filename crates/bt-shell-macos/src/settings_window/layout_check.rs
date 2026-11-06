//! A check of the window's layout that needs no eyes: every page is laid out with the default
//! settings and every pair of things that must not overlap is measured — the parts of a page
//! (its example and its groups) one under another, a group's rows inside its box and one under
//! another, a row's text beside its control. With a folder, each page is also written there as a
//! picture, the whole page and the window as it shows it.
//!
//! A constraint that leaves a height open does not fail to build: AppKit picks a size, often
//! none, and the rows draw over one another. Only a measurement of the laid-out frames sees it.
//! It runs from `tests/settings_window.rs`: AppKit wants the main thread, which libtest does not
//! give.

use std::path::Path;

use bt_core::{Settings, Theme};
use bt_gpu::ScrollbarMode;
use objc2::DefinedClass;
use objc2_app_kit::{NSBitmapImageFileType, NSView};
use objc2_foundation::{NSDictionary, NSIndexSet, NSRect};

use super::{Category, Resolved, SettingsWindow, SideItem, ThemeLists, pages};
use crate::settings::{self, FileState};

/// How far two frames may run into each other before it is an overlap: the hairline rounding of
/// a backing scale.
const SLACK: f64 = 0.5;

impl SettingsWindow {
    /// Lays out every page and lists what overlaps; with `shots`, writes `{page}.png` (the whole
    /// page) and `{page}-window.png` (the window) there.
    pub(crate) fn layout_problems(&self, shots: Option<&Path>) -> Vec<String> {
        let ivars = self.ivars();
        let (Some(window), Some(sidebar), Some(controls), Some(scroll), Some(panes)) = (
            ivars.window.get(),
            ivars.sidebar.get(),
            ivars.controls.get(),
            ivars.pane_scroll.get(),
            ivars.panes.get(),
        ) else {
            return vec!["the window was not built".to_owned()];
        };
        let settings = Settings::default();
        let embedded = Theme::embedded_groups();
        let (light, _) = settings::load_theme(None, &settings.light_theme).or_embedded(false);
        let (dark, _) = settings::load_theme(None, &settings.dark_theme).or_embedded(true);
        let themes = ThemeLists {
            embedded: &embedded,
            user: &[],
            on_screen: &light,
            light: &light,
            dark: &dark,
            dark_appearance: false,
        };
        let resolved = Resolved {
            reduce: false,
            scrollbar: ScrollbarMode::Auto,
        };
        self.refresh(&settings, resolved, &FileState::Missing, &[], &themes);

        let mut problems = Vec::new();
        for category in Category::ALL {
            let Some(grid) = category.grid() else {
                continue;
            };
            sidebar.selectRowIndexes_byExtendingSelection(
                &NSIndexSet::indexSetWithIndex(SideItem::row_of(category)),
                false,
            );
            self.show_selected();
            let Some(content) = window.contentView() else {
                continue;
            };
            content.layoutSubtreeIfNeeded();
            let Some(document) = scroll.documentView() else {
                problems.push("the pages have no document".to_owned());
                continue;
            };
            let page = category.title();
            let mut say = |what: String| problems.push(format!("{page}: {what}"));

            // The page's parts, top to bottom.
            if let Some(pane) = panes.get(grid) {
                let mut above: Option<NSRect> = None;
                for (i, part) in pane.arrangedSubviews().iter().enumerate() {
                    if part.isHidden() {
                        continue;
                    }
                    let frame = in_document(&part, &document);
                    if frame.size.height < 1.0 {
                        say(format!("part {i} has no height"));
                    }
                    if let Some(above) = above
                        && top(frame) < bottom(above) - SLACK
                    {
                        say(format!(
                            "part {i} starts at {:.1}, inside the one above (ends at {:.1})",
                            top(frame),
                            bottom(above)
                        ));
                    }
                    above = Some(frame);
                }
            }

            // Each group's rows: inside the box, one under another, text beside the control.
            let specs = pages::groups(category);
            for group in controls
                .groups
                .iter()
                .filter(|group| specs.iter().any(|spec| std::ptr::eq(spec, group.spec)))
            {
                if group.view.isHidden() {
                    continue;
                }
                let name = group.spec.title.unwrap_or("(untitled)");
                let Some(boxed) = group.view.arrangedSubviews().lastObject() else {
                    continue;
                };
                let boxed = in_document(&boxed, &document);
                let mut above: Option<NSRect> = None;
                for row in &controls.rows[group.rows.clone()] {
                    if row.view.isHidden() {
                        continue;
                    }
                    let label = row.label.stringValue().to_string();
                    let frame = in_document(&row.view, &document);
                    if frame.size.height < 1.0 {
                        say(format!("{name} › {label}: the row has no height"));
                    }
                    if top(frame) < top(boxed) - SLACK || bottom(frame) > bottom(boxed) + SLACK {
                        say(format!(
                            "{name} › {label}: the row ({:.1}–{:.1}) leaves its box ({:.1}–{:.1})",
                            top(frame),
                            bottom(frame),
                            top(boxed),
                            bottom(boxed)
                        ));
                    }
                    if let Some(above) = above
                        && top(frame) < bottom(above) - SLACK
                    {
                        say(format!("{name} › {label}: the row runs into the one above"));
                    }
                    above = Some(frame);
                    let bounds = row.view.bounds();
                    let inner: Vec<NSRect> = row
                        .view
                        .subviews()
                        .iter()
                        .filter(|view| !view.isHidden())
                        // The alignment rectangle: a separator box's frame reaches past the line it draws.
                        .map(|view| view.alignmentRectForFrame(view.frame()))
                        .collect();
                    for (i, part) in inner.iter().enumerate() {
                        if !contains(bounds, *part) {
                            say(format!("{name} › {label}: part {i} spills out of the row"));
                        }
                    }
                    // The separator, the text, the control: the last two side by side must not meet.
                    if let [_, text, control] = inner[..]
                        && overlap(text, control)
                    {
                        say(format!("{name} › {label}: the text runs under the control"));
                    }
                }
            }

            if let Some(dir) = shots {
                let slug = page.to_lowercase().replace(' ', "-");
                picture(&document, &dir.join(format!("{slug}.png")));
                picture(&content, &dir.join(format!("{slug}-window.png")));
            }
        }
        problems
    }
}

/// `view`'s frame in the pages' document (flipped: y grows downwards).
fn in_document(view: &NSView, document: &NSView) -> NSRect {
    view.convertRect_toView(view.bounds(), Some(document))
}

fn top(rect: NSRect) -> f64 {
    rect.origin.y
}

fn bottom(rect: NSRect) -> f64 {
    rect.origin.y + rect.size.height
}

fn contains(outer: NSRect, inner: NSRect) -> bool {
    inner.origin.x >= outer.origin.x - SLACK
        && inner.origin.y >= outer.origin.y - SLACK
        && inner.origin.x + inner.size.width <= outer.origin.x + outer.size.width + SLACK
        && inner.origin.y + inner.size.height <= outer.origin.y + outer.size.height + SLACK
}

fn overlap(a: NSRect, b: NSRect) -> bool {
    let x = a.origin.x.max(b.origin.x)
        < (a.origin.x + a.size.width).min(b.origin.x + b.size.width) - SLACK;
    let y = a.origin.y.max(b.origin.y)
        < (a.origin.y + a.size.height).min(b.origin.y + b.size.height) - SLACK;
    x && y
}

/// Writes what `view` draws as a PNG at `path`; a view that cannot be drawn writes nothing.
fn picture(view: &NSView, path: &Path) {
    let bounds = view.bounds();
    let Some(rep) = view.bitmapImageRepForCachingDisplayInRect(bounds) else {
        return;
    };
    view.cacheDisplayInRect_toBitmapImageRep(bounds, &rep);
    // SAFETY: the properties are an empty dictionary, which the type takes for no property.
    let png = unsafe {
        rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    };
    if let Some(png) = png {
        let _ = std::fs::write(path, png.to_vec());
    }
}
