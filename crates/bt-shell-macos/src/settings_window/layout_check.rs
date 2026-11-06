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

use objc2::rc::Retained;

use bt_core::{CursorBlink, Settings, Theme};
use bt_gpu::ScrollbarMode;
use objc2::{DefinedClass, Message};
use objc2_app_kit::{NSBitmapImageFileType, NSView};
use objc2_foundation::{NSDate, NSDictionary, NSIndexSet, NSObjectProtocol, NSRect, NSRunLoop};

use super::{Category, Key, Resolved, SettingsWindow, SideItem, ThemeLists, pages, specimen};
use crate::settings::{self, FileState};

/// How far two frames may run into each other before it is an overlap: the hairline rounding of
/// a backing scale.
const SLACK: f64 = 0.5;

impl SettingsWindow {
    /// Lays out every page and lists what overlaps; with `shots`, writes `{page}.png` (the whole
    /// page), `{page}-window.png` (the window) and `{page}-sidebar.png` (the sidebar) there, and
    /// the same with `-dark` in a dark theme.
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
            user_themes: &[],
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

            // The page's example: as wide as its groups, its cards filled by their pictures, and
            // still where it was with the rows scrolled to their end.
            if let Some(kind) = specimen::Kind::of(category)
                && let Some(specimens) = ivars.specimens.get()
                && let Some(example) = specimens.view(kind)
                && !example.isHidden()
            {
                let boxed = controls
                    .groups
                    .iter()
                    .filter(|group| specs.iter().any(|spec| std::ptr::eq(spec, group.spec)))
                    .find(|group| !group.view.isHidden())
                    .and_then(|group| group.view.arrangedSubviews().lastObject());
                let shown = in_window(example);
                if let Some(boxed) = boxed {
                    let groups = in_window(&boxed);
                    let left = (shown.origin.x - groups.origin.x).abs();
                    let right =
                        (shown.origin.x + shown.size.width - groups.origin.x - groups.size.width)
                            .abs();
                    if left > SLACK || right > SLACK {
                        say(format!(
                            "the example ({:.1}–{:.1}) is not as wide as the groups ({:.1}–{:.1})",
                            shown.origin.x,
                            shown.origin.x + shown.size.width,
                            groups.origin.x,
                            groups.origin.x + groups.size.width
                        ));
                    }
                }
                for (picture, place) in specimens.card_widths(kind) {
                    if (picture - place).abs() > SLACK {
                        say(format!(
                            "a card's picture is {picture:.1} wide in a place {place:.1} wide"
                        ));
                    }
                }
                let clip = scroll.contentView();
                let end = (document.frame().size.height - clip.bounds().size.height).max(0.0);
                clip.scrollToPoint(objc2_foundation::NSPoint::new(0.0, end));
                scroll.reflectScrolledClipView(&clip);
                content.layoutSubtreeIfNeeded();
                let scrolled = in_window(example);
                if let Some(dir) = shots {
                    let slug = page.to_lowercase().replace(' ', "-");
                    picture(&content, &dir.join(format!("{slug}-scrolled-window.png")));
                }
                if (scrolled.origin.y - shown.origin.y).abs() > SLACK {
                    say(format!(
                        "the example moved from {:.1} to {:.1} with the rows scrolled",
                        shown.origin.y, scrolled.origin.y
                    ));
                }
                if end > SLACK
                    && let Some(sticky) = ivars.sticky.get()
                    && sticky.rule.isHidden()
                {
                    say("no line under the example with rows scrolled under it".to_owned());
                }
                // A setting changed down the page refreshes the window from the file: the page
                // stays where it was scrolled.
                let before = clip.bounds().origin.y;
                self.refresh(&settings, resolved, &FileState::Missing, &[], &themes);
                content.layoutSubtreeIfNeeded();
                let after = clip.bounds().origin.y;
                if (after - before).abs() > SLACK {
                    say(format!(
                        "a refresh scrolled the page from {before:.1} to {after:.1}"
                    ));
                }
                clip.scrollToPoint(objc2_foundation::NSPoint::new(0.0, 0.0));
                scroll.reflectScrolledClipView(&clip);
            }

            if let Some(dir) = shots {
                let slug = page.to_lowercase().replace(' ', "-");
                picture(&document, &dir.join(format!("{slug}.png")));
                picture(&content, &dir.join(format!("{slug}-window.png")));
                // The window's picture leaves the sidebar out (its material is the window
                // server's); its rows draw themselves.
                picture(&column(sidebar), &dir.join(format!("{slug}-sidebar.png")));
            }
        }
        self.blink_problems(&themes, resolved, shots, &mut problems);
        self.drag_problems(shots, &mut problems);
        if let Some(dir) = shots {
            // The same pages in a dark theme of the terminal's: the window is drawn in it.
            let settings = Settings {
                theme: "bateri".to_owned(),
                ..Settings::default()
            };
            let themes = ThemeLists {
                on_screen: &dark,
                dark_appearance: true,
                ..themes
            };
            self.refresh(&settings, resolved, &FileState::Missing, &[], &themes);
            // The Keyboard page is left out here as above: drawn off screen, its window comes back
            // empty (before this palette as after it).
            for category in Category::ALL.into_iter().filter(|c| c.grid().is_some()) {
                sidebar.selectRowIndexes_byExtendingSelection(
                    &NSIndexSet::indexSetWithIndex(SideItem::row_of(category)),
                    false,
                );
                self.show_selected();
                let Some(content) = window.contentView() else {
                    continue;
                };
                content.layoutSubtreeIfNeeded();
                let slug = category.title().to_lowercase().replace(' ', "-");
                if let Some(document) = scroll.documentView() {
                    picture(&document, &dir.join(format!("{slug}-dark.png")));
                }
                picture(&content, &dir.join(format!("{slug}-dark-window.png")));
                picture(
                    &column(sidebar),
                    &dir.join(format!("{slug}-dark-sidebar.png")),
                );
            }
        }
        self.gallery_problems(&themes, resolved, shots, &mut problems);
        self.theme_change_problems(&themes, resolved, &dark, shots, &mut problems);
        self.window_ground_problems(&themes, resolved, &mut problems);
        problems
    }

    /// A theme that changes and keeps the appearance (one dark theme for another) draws the
    /// window in the new one: its colours are read when drawn, not kept. The General page's first
    /// group is pictured in each and must differ.
    fn theme_change_problems(
        &self,
        themes: &ThemeLists<'_>,
        resolved: Resolved,
        dark: &Theme,
        shots: Option<&Path>,
        problems: &mut Vec<String>,
    ) {
        let ivars = self.ivars();
        let (Some(window), Some(sidebar), Some(controls)) = (
            ivars.window.get(),
            ivars.sidebar.get(),
            ivars.controls.get(),
        ) else {
            return;
        };
        let other = Theme::ULTRAVIOLET;
        if crate::window::is_dark_background(&other) != crate::window::is_dark_background(dark) {
            problems.push("the theme change check needs two dark themes".to_owned());
            return;
        }
        let mut seen = Vec::new();
        for (name, theme) in [("bateri", dark), ("ultraviolet", &other)] {
            let settings = Settings {
                theme: name.to_owned(),
                ..Settings::default()
            };
            let themes = ThemeLists {
                on_screen: theme,
                dark_appearance: true,
                ..*themes
            };
            self.refresh(&settings, resolved, &FileState::Missing, &[], &themes);
            sidebar.selectRowIndexes_byExtendingSelection(
                &NSIndexSet::indexSetWithIndex(SideItem::row_of(Category::General)),
                false,
            );
            self.show_selected();
            if let Some(content) = window.contentView() {
                content.layoutSubtreeIfNeeded();
                if let Some(dir) = shots {
                    picture(&content, &dir.join(format!("general-{name}-window.png")));
                }
            }
            let group = controls
                .groups
                .iter()
                .find(|group| !group.view.isHidden())
                .and_then(|group| group.view.arrangedSubviews().lastObject());
            seen.push(group.and_then(|group| png_of(&group)));
        }
        if seen.iter().any(Option::is_none) || seen[0] == seen[1] {
            problems.push(
                "General: another dark theme left the window in the colours of the one before"
                    .to_owned(),
            );
        }
    }

    /// The theme galleries ring the file's choice: the pair's while the theme follows the system
    /// (the one-theme gallery hidden), the one theme's while it does not (Light and Dark hidden),
    /// and a theme of the user's folder like an embedded one.
    fn gallery_problems(
        &self,
        themes: &ThemeLists<'_>,
        resolved: Resolved,
        shots: Option<&Path>,
        problems: &mut Vec<String>,
    ) {
        let ivars = self.ivars();
        let (Some(window), Some(sidebar), Some(controls)) = (
            ivars.window.get(),
            ivars.sidebar.get(),
            ivars.controls.get(),
        ) else {
            return;
        };
        let row_shown = |key: Key| {
            controls
                .rows
                .iter()
                .find(|row| row.key == Some(key))
                .is_some_and(|row| !row.view.isHidden())
        };
        let user = ["paper".to_owned()];
        let user_themes = [Theme::LINEN];
        let cases = [
            (Settings::default(), false),
            (
                Settings {
                    theme: "linen".to_owned(),
                    ..Settings::default()
                },
                false,
            ),
            (
                Settings {
                    theme: "paper".to_owned(),
                    ..Settings::default()
                },
                true,
            ),
        ];
        for (settings, with_user) in cases {
            let themes = ThemeLists {
                user: if with_user { &user } else { &[] },
                user_themes: if with_user { &user_themes } else { &[] },
                ..*themes
            };
            self.refresh(&settings, resolved, &FileState::Missing, &[], &themes);
            let follows = settings.theme == bt_core::SYSTEM_THEME;
            let mut say =
                |what: String| problems.push(format!("Appearance ({}): {what}", settings.theme));
            if controls.theme.isHidden() != follows {
                say("the one-theme gallery is shown with the wrong way".to_owned());
            }
            if follows {
                for (gallery, name) in [
                    (&controls.light_theme, &settings.light_theme),
                    (&controls.dark_theme, &settings.dark_theme),
                ] {
                    if gallery.chosen().as_ref() != Some(name) {
                        say(format!("{:?} is ringed, not {name}", gallery.chosen()));
                    }
                }
            } else {
                if controls.theme.chosen().as_ref() != Some(&settings.theme) {
                    say(format!("{:?} is ringed", controls.theme.chosen()));
                }
                if row_shown(Key::LightTheme) || row_shown(Key::DarkTheme) {
                    say("Light and Dark show under one theme".to_owned());
                }
            }
            if with_user && let Some(dir) = shots {
                sidebar.selectRowIndexes_byExtendingSelection(
                    &NSIndexSet::indexSetWithIndex(SideItem::row_of(Category::Appearance)),
                    false,
                );
                self.show_selected();
                if let (Some(content), Some(document)) = (
                    window.contentView(),
                    ivars
                        .pane_scroll
                        .get()
                        .and_then(|scroll| scroll.documentView()),
                ) {
                    content.layoutSubtreeIfNeeded();
                    picture(&document, &dir.join("appearance-user.png"));
                }
            }
        }
    }

    /// One light theme for another keeps the appearance, and the window's own ground (the strip
    /// round the sidebar, the title row) is drawn again only when its colour is another: the
    /// window must be given the new theme's ground, not the same colour as before.
    fn window_ground_problems(
        &self,
        themes: &ThemeLists<'_>,
        resolved: Resolved,
        problems: &mut Vec<String>,
    ) {
        let Some(window) = self.ivars().window.get() else {
            return;
        };
        let mut grounds = Vec::new();
        for (name, theme) in [
            ("bateri-light", Theme::BATERI_LIGHT),
            ("linen", Theme::LINEN),
        ] {
            let settings = Settings {
                theme: name.to_owned(),
                ..Settings::default()
            };
            let themes = ThemeLists {
                on_screen: &theme,
                dark_appearance: false,
                ..*themes
            };
            self.refresh(&settings, resolved, &FileState::Missing, &[], &themes);
            grounds.push(window.backgroundColor());
        }
        if grounds[0].isEqual(Some(&grounds[1])) {
            problems.push(
                "the window's ground is the same colour after one light theme for another, so \
                 the strip round the sidebar keeps the old one"
                    .to_owned(),
            );
        }
    }

    /// While a slider is dragged the Cursor example follows the value under the hand, and what the
    /// window holds as the file's stays the file's. Runs on the Cursor page [`Self::blink_problems`]
    /// left open; a machine with no GPU has no example to follow.
    fn drag_problems(&self, shots: Option<&Path>, problems: &mut Vec<String>) {
        let ivars = self.ivars();
        let Some(specimens) = ivars.specimens.get() else {
            return;
        };
        // The window, not the pages' document: the example stands above it.
        let Some(document) = ivars.window.get().and_then(|window| window.contentView()) else {
            return;
        };
        if specimens
            .view(specimen::Kind::Cursor)
            .is_none_or(NSView::isHidden)
        {
            return;
        }
        let file = |read: fn(&Settings) -> f64| {
            ivars
                .specimen_inputs
                .borrow()
                .as_ref()
                .map(|inputs| read(&inputs.settings))
        };
        let radius = file(|settings| settings.caret.radius_ratio);
        let before = png_of(&document);
        self.preview_specimen(Key::Radius, 0.5);
        self.preview_specimen(Key::Glow, 3.0);
        let after = png_of(&document);
        if before.is_none() || before == after {
            problems.push(
                "Cursor: dragging Corner radius and Glow left the example as it was".to_owned(),
            );
        }
        if file(|settings| settings.caret.radius_ratio) != radius {
            problems.push("Cursor: a dragged value was kept as the file's".to_owned());
        }
        if let (Some(dir), Some(png)) = (shots, after) {
            let _ = std::fs::write(dir.join("cursor-drag.png"), png);
        }
        self.show_specimen();
        self.cancel_blink();
        specimens.stop_blink();
    }

    /// With the file's blink on, the Cursor example's caret is turned by the window's clock: half
    /// a period after the page opens it is dark. A machine with no GPU has no example to blink.
    fn blink_problems(
        &self,
        themes: &ThemeLists<'_>,
        resolved: Resolved,
        shots: Option<&Path>,
        problems: &mut Vec<String>,
    ) {
        let ivars = self.ivars();
        let (Some(sidebar), Some(specimens)) = (ivars.sidebar.get(), ivars.specimens.get()) else {
            return;
        };
        let settings = Settings {
            cursor_blink: CursorBlink::On,
            ..Settings::default()
        };
        self.refresh(&settings, resolved, &FileState::Missing, &[], themes);
        sidebar.selectRowIndexes_byExtendingSelection(
            &NSIndexSet::indexSetWithIndex(SideItem::row_of(Category::Cursor)),
            false,
        );
        self.show_selected();
        if specimens
            .view(specimen::Kind::Cursor)
            .is_none_or(NSView::isHidden)
        {
            return;
        }
        if specimens.blink_shown() != Some(true) {
            problems.push("Cursor: the example does not blink".to_owned());
            return;
        }
        let half = settings.blink_interval;
        NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(half * 1.5));
        if specimens.blink_shown() != Some(false) {
            problems.push(format!(
                "Cursor: {:.2} s on, the blinking caret has not gone dark",
                half * 1.5
            ));
        }
        if let (Some(dir), Some(content)) = (
            shots,
            ivars.window.get().and_then(|window| window.contentView()),
        ) {
            picture(&content, &dir.join("cursor-blink-dark.png"));
        }
        self.cancel_blink();
        specimens.stop_blink();
    }
}

/// The sidebar's column: the table's scroll view's parent, which holds its ground, the search
/// field and the button too; the table itself when it stands alone.
fn column(table: &NSView) -> Retained<NSView> {
    table
        .enclosingScrollView()
        // SAFETY: the window is built and open for the check; its views are alive.
        .and_then(|scroll| unsafe { scroll.superview() })
        .unwrap_or_else(|| table.retain())
}

/// `view`'s frame in the window.
fn in_window(view: &NSView) -> NSRect {
    view.convertRect_toView(view.bounds(), None)
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
    if let Some(png) = png_of(view) {
        let _ = std::fs::write(path, png);
    }
}

/// What `view` draws, as a PNG; `None` for a view that cannot be drawn.
fn png_of(view: &NSView) -> Option<Vec<u8>> {
    let bounds = view.bounds();
    let rep = view.bitmapImageRepForCachingDisplayInRect(bounds)?;
    view.cacheDisplayInRect_toBitmapImageRep(bounds, &rep);
    // SAFETY: the properties are an empty dictionary, which the type takes for no property.
    let png = unsafe {
        rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    };
    png.map(|png| png.to_vec())
}
