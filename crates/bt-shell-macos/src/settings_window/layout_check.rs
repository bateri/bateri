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
use std::ptr::NonNull;

use objc2::rc::Retained;

use bt_core::{CursorBlink, Settings, TabDetails, TabPosition, Theme};
use bt_gpu::ScrollbarMode;
use objc2::{AllocAnyThread, ClassType, DefinedClass, MainThreadOnly, Message, sel};
use objc2_app_kit::{
    NSAppearanceCustomization, NSBitmapImageFileType, NSBitmapImageRep, NSBox, NSView, NSWindow,
};
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::CGImage;
use objc2_foundation::{
    NSDate, NSDictionary, NSIndexSet, NSObjectProtocol, NSPoint, NSRect, NSRunLoop, ns_string,
};

use super::pictures::Palette;
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
        self.agents_problems(&themes, resolved, shots, &mut problems);
        self.apps_problems(&themes, resolved, &dark, shots, &mut problems);
        if let Some(dir) = shots {
            // The Keyboard page is a list of its own, not a grid: its picture only.
            sidebar.selectRowIndexes_byExtendingSelection(
                &NSIndexSet::indexSetWithIndex(SideItem::row_of(Category::Keyboard)),
                false,
            );
            self.show_selected();
            if let Some(content) = window.contentView() {
                content.layoutSubtreeIfNeeded();
                picture(&content, &dir.join("keyboard-window.png"));
            }
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
            // The Keyboard page too: a list of its own, with no grid to check, but a picture.
            for category in Category::ALL {
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
                if category.grid().is_some()
                    && let Some(document) = scroll.documentView()
                {
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
        if let Some(dir) = shots {
            self.search_pictures(&themes, resolved, dir);
        }
        problems
    }

    /// The Apps page with a row of every state, with long names, with nothing, with the Add field
    /// typed in, refused and looking; and the three sheets. Each row stands inside its box with
    /// nothing running into anything, each sheet is as wide as it was made and holds its text.
    fn apps_problems(
        &self,
        themes: &ThemeLists<'_>,
        resolved: Resolved,
        dark: &Theme,
        shots: Option<&Path>,
        problems: &mut Vec<String>,
    ) {
        use bt_core::pencere::apps::Version;
        use bt_shell_common::apps::page::Tracked;
        use bt_shell_common::apps::{Failure, State, Status};

        let ivars = self.ivars();
        let (Some(window), Some(sidebar), Some(apps)) =
            (ivars.window.get(), ivars.sidebar.get(), ivars.apps.get())
        else {
            return;
        };
        let version = |text: &str| Version::parse(text);
        let status = |source: &str, name: Option<&str>, state: State| Status {
            source: source.to_owned(),
            name: name.map(str::to_owned),
            state,
            problem: None,
        };
        let key = "ed25519:Zk3mP9wLq1xD7sVb0nJ4tHc8RaE2yUf6OgIiKd5Yh+A=".to_owned();
        let (Some(v1), Some(v03), Some(v09), Some(v14), Some(v21), Some(v30), Some(v124)) = (
            version("1.0.0"),
            version("0.3.1"),
            version("0.9.0"),
            version("1.4.2"),
            version("2.1.0"),
            version("3.0.0"),
            version("12.4.10"),
        ) else {
            problems.push("Apps: a version of the check does not parse".to_owned());
            return;
        };
        let long_source =
            "https://downloads.internal.example-company.org/releases/latest/lab/app.json";
        let all: Vec<Status> = vec![
            status(
                "bateri/textpen",
                Some("textpen"),
                State::Ready {
                    version: v1,
                    newer: None,
                },
            ),
            status(
                "acme/notes",
                Some("notes"),
                State::Ready {
                    version: v03,
                    newer: None,
                },
            ),
            status(
                "mina/sketch",
                Some("sketch"),
                State::Ready {
                    version: v21,
                    newer: Some(v30),
                },
            ),
            status(
                "https://lab.example.org/app.json",
                Some("lab"),
                State::Awaiting {
                    version: v09,
                    key: key.clone(),
                    changed: false,
                },
            ),
            status(
                "kerem/draw",
                Some("draw"),
                State::Awaiting {
                    version: v14,
                    key: key.clone(),
                    changed: true,
                },
            ),
            status("ozge/mono", Some("mono"), State::NotHere { version: v1 }),
            status(
                "dev/flaky",
                None,
                State::Error(Failure::Fetch(
                    "Could not resolve host: github.com".to_owned(),
                )),
            ),
        ];
        let long: Vec<Status> = vec![
            status(
                "bateri/textpen",
                Some("textpen"),
                State::Ready {
                    version: v1,
                    newer: None,
                },
            ),
            status(
                "platform-engineering-tools/kubernetes-cluster-resource-inspector",
                Some("kubernetes-cluster-resource-inspector-and-log-follower"),
                State::Ready {
                    version: v124,
                    newer: None,
                },
            ),
            status(
                long_source,
                Some("lab"),
                State::Awaiting {
                    version: v09,
                    key,
                    changed: false,
                },
            ),
        ];
        let sources_of = |statuses: &[Status]| -> Vec<String> {
            statuses
                .iter()
                .map(|status| status.source.clone())
                .collect()
        };
        // A source nobody has checked yet is a row that is still looking.
        let mut waiting = all.clone();
        waiting.truncate(1);
        let mut waiting_sources = sources_of(&waiting);
        waiting_sources.push("acme/fresh".to_owned());
        // A list no check could start for: every row says so instead of looking for ever.
        let unchecked = sources_of(&all);
        // A scene's name, the list, and what the tracker says (`None`: no check could start).
        type Scene = (&'static str, Vec<String>, Option<Tracked>);
        let tracked = |statuses: Vec<Status>, running: bool| Tracked {
            statuses,
            running,
            checked: Some(std::time::SystemTime::now()),
        };
        let scenes: [Scene; 6] = [
            ("all", sources_of(&all), Some(tracked(all.clone(), false))),
            // Check Now pressed: the ring in the title, a row a try may mend turning.
            ("checking", sources_of(&all), Some(tracked(all, true))),
            ("long", sources_of(&long), Some(tracked(long, false))),
            ("empty", Vec::new(), Some(tracked(Vec::new(), false))),
            ("looking", waiting_sources, Some(tracked(waiting, false))),
            ("unchecked", unchecked, None),
        ];
        apps.sample_updates(true);
        sidebar.selectRowIndexes_byExtendingSelection(
            &NSIndexSet::indexSetWithIndex(SideItem::row_of(Category::Apps)),
            false,
        );
        self.show_selected();
        for (suffix, dark_pass) in [("", false), ("-dark", true)] {
            if dark_pass {
                let settings = Settings {
                    theme: "bateri".to_owned(),
                    ..Settings::default()
                };
                let themes = ThemeLists {
                    on_screen: dark,
                    dark_appearance: true,
                    ..*themes
                };
                self.refresh(&settings, resolved, &FileState::Missing, &[], &themes);
                sidebar.selectRowIndexes_byExtendingSelection(
                    &NSIndexSet::indexSetWithIndex(SideItem::row_of(Category::Apps)),
                    false,
                );
                self.show_selected();
            }
            for (name, sources, statuses) in &scenes {
                apps.show(self, sources, statuses.as_ref(), false);
                apps.sample("", None, false);
                let Some(content) = window.contentView() else {
                    continue;
                };
                content.layoutSubtreeIfNeeded();
                let list = apps.rows_view();
                let box_frame = in_window(list);
                for (at, row) in list.arrangedSubviews().iter().enumerate() {
                    let frame = in_window(&row);
                    if frame.size.height < 1.0 {
                        problems.push(format!("Apps ({name}): row {at} has no height"));
                    }
                    if !contains(box_frame, frame) {
                        problems.push(format!("Apps ({name}): row {at} leaves its box"));
                    }
                    let bounds = row.bounds();
                    let parts: Vec<NSRect> = row
                        .subviews()
                        .iter()
                        .filter(|view| !view.isHidden() && !view.isKindOfClass(NSBox::class()))
                        .map(|view| view.alignmentRectForFrame(view.frame()))
                        .collect();
                    for (i, part) in parts.iter().enumerate() {
                        if !contains(bounds, *part) {
                            problems.push(format!("Apps ({name}): row {at} part {i} spills out"));
                        }
                        for (j, other) in parts.iter().enumerate().skip(i + 1) {
                            if overlap(*part, *other) {
                                problems.push(format!(
                                    "Apps ({name}): row {at}: part {i} runs into part {j}"
                                ));
                            }
                        }
                    }
                }
                if let Some(dir) = shots {
                    picture(
                        &content,
                        &dir.join(format!("apps-{name}{suffix}-window.png")),
                    );
                }
                // A sign's tip, as the pointer resting on it shows it: inside the window, and
                // pictured for the first rows that have something to say.
                let tips: &[usize] = match *name {
                    "all" => &[0, 2, 3, 6],
                    "unchecked" => &[0],
                    _ => &[],
                };
                for &at in tips {
                    if !apps.sample_tip(at) {
                        problems.push(format!("Apps ({name}): row {at} has no sign to show"));
                        continue;
                    }
                    let room = content.bounds();
                    let tip = content.subviews().iter().last().map(|tip| tip.frame());
                    match tip {
                        Some(tip) if contains(room, tip) => {}
                        _ => problems
                            .push(format!("Apps ({name}): row {at}'s tip leaves the window")),
                    }
                    if let Some(dir) = shots {
                        picture(
                            &content,
                            &dir.join(format!("apps-{name}-tip-{at}{suffix}-window.png")),
                        );
                    }
                }
                super::apps::hide_tip();
            }
            // The Files row in each of its three states: the program is in the list, Nothing, a
            // program the list lacks (the warning, which is the longest note). The row stands in
            // its box, its text clear of its control, and says what the state calls for.
            let followed = ["textpen".to_owned(), "notes".to_owned()];
            for (name, value, says) in [
                ("open-in-list", "textpen", "In a floating pane"),
                ("open-nothing", "", "A path is not a link."),
                (
                    "open-gone",
                    "sketch",
                    "sketch is not a followed program that opens files.",
                ),
            ] {
                self.apply_open_with(value, &followed);
                self.set_open_with_note(value);
                let (Some(content), Some(controls)) = (window.contentView(), ivars.controls.get())
                else {
                    continue;
                };
                content.layoutSubtreeIfNeeded();
                let Some(row) = controls
                    .rows
                    .iter()
                    .find(|row| row.key == Some(Key::OpenWith))
                else {
                    problems.push(format!("Apps ({name}): the Open with row is not built"));
                    continue;
                };
                let note = row.note.stringValue().to_string();
                if row.note.isHidden() || !note.starts_with(says) {
                    problems.push(format!(
                        "Apps ({name}): the note says {note:?}, not {says:?}"
                    ));
                }
                let boxed = controls
                    .groups
                    .iter()
                    .find(|group| group.spec.title == Some("Files"))
                    .and_then(|group| group.view.arrangedSubviews().lastObject());
                let frame = in_window(&row.view);
                match boxed.map(|boxed| in_window(&boxed)) {
                    Some(boxed) if contains(boxed, frame) => {}
                    _ => problems.push(format!("Apps ({name}): the Open with row leaves its box")),
                }
                let inner: Vec<NSRect> = row
                    .view
                    .subviews()
                    .iter()
                    .filter(|view| !view.isHidden())
                    .map(|view| view.alignmentRectForFrame(view.frame()))
                    .collect();
                for (i, part) in inner.iter().enumerate() {
                    if !contains(row.view.bounds(), *part) {
                        problems.push(format!("Apps ({name}): part {i} of the row spills out"));
                    }
                }
                if let [_, text, control] = inner[..]
                    && overlap(text, control)
                {
                    problems.push(format!("Apps ({name}): the text runs under the control"));
                }
                if let Some(dir) = shots {
                    picture(
                        &content,
                        &dir.join(format!("apps-{name}{suffix}-window.png")),
                    );
                }
            }
            // The Add field typed in, refused, and looking.
            let first = scenes[0].2.as_ref().map(|all| Tracked {
                statuses: all.statuses[..1].to_vec(),
                ..all.clone()
            });
            apps.show(self, &scenes[0].1[..1], first.as_ref(), false);
            for (name, typed, error, looking) in [
                ("typing", "acme/notes", None, false),
                ("looking-up", "acme/notes", None, true),
                (
                    "error",
                    "acme/missing",
                    Some("No release of acme/missing has an app.json."),
                    false,
                ),
            ] {
                apps.sample(typed, error, looking);
                if let Some(content) = window.contentView() {
                    content.layoutSubtreeIfNeeded();
                    if let Some(dir) = shots {
                        picture(
                            &content,
                            &dir.join(format!("apps-add-{name}{suffix}-window.png")),
                        );
                    }
                }
            }
            apps.sample("", None, false);
            for (name, view) in super::apps::sample_sheets(self.mtm(), self) {
                let size = view.frame().size;
                if (size.width - 460.0).abs() > SLACK || size.height < 150.0 {
                    problems.push(format!(
                        "Apps: the {name} sheet is {:.0} × {:.0}",
                        size.width, size.height
                    ));
                }
                if let Some(dir) = shots {
                    picture(&view, &dir.join(format!("apps-sheet-{name}{suffix}.png")));
                }
            }
        }
        // The page as the person leaves it: the list the file has.
        self.refresh(
            &Settings::default(),
            resolved,
            &FileState::Missing,
            &[],
            themes,
        );
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

    /// The Keyboard page draws nothing off screen, so its search field is pictured on its own: one
    /// made the way the page makes it, for a moment in the window, in a dark and a light theme.
    fn search_pictures(&self, themes: &ThemeLists<'_>, resolved: Resolved, dir: &Path) {
        let Some(content) = self
            .ivars()
            .window
            .get()
            .and_then(|window| window.contentView())
        else {
            return;
        };
        for (name, theme, dark) in [
            ("ultraviolet", Theme::ULTRAVIOLET, true),
            ("linen", Theme::LINEN, false),
        ] {
            let settings = Settings {
                theme: name.to_owned(),
                ..Settings::default()
            };
            let themes = ThemeLists {
                on_screen: &theme,
                dark_appearance: dark,
                ..*themes
            };
            self.refresh(&settings, resolved, &FileState::Missing, &[], &themes);
            let (search, frame) = super::themed_search(self.mtm(), "Search commands");
            content.addSubview(&frame);
            for constraint in [
                frame.widthAnchor().constraintEqualToConstant(320.0),
                frame
                    .leadingAnchor()
                    .constraintEqualToAnchor_constant(&content.leadingAnchor(), 260.0),
                frame
                    .topAnchor()
                    .constraintEqualToAnchor_constant(&content.topAnchor(), 60.0),
            ] {
                constraint.setActive(true);
            }
            content.layoutSubtreeIfNeeded();
            picture(&frame, &dir.join(format!("search-{name}.png")));
            // Typed into, with the keyboard in it: the words stand clear of the magnifier.
            search.setStringValue(ns_string!("command"));
            if let Some(window) = self.ivars().window.get() {
                window.makeFirstResponder(Some(&search));
            }
            content.layoutSubtreeIfNeeded();
            picture(&frame, &dir.join(format!("search-{name}-typed.png")));
            if let Some(window) = self.ivars().window.get() {
                window.makeFirstResponder(None);
            }
            frame.removeFromSuperview();
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

    /// The Agents page keeps still when its switch turns — the explanation and the example stand
    /// where they stood — and its line on where the reply shows follows Details under a tab and
    /// the tabs' position, with the button hidden where all is well and a click that opens Tabs
    /// and Panes where those are.
    fn agents_problems(
        &self,
        themes: &ThemeLists<'_>,
        resolved: Resolved,
        shots: Option<&Path>,
        problems: &mut Vec<String>,
    ) {
        let ivars = self.ivars();
        let (Some(window), Some(sidebar), Some(controls), Some(scroll)) = (
            ivars.window.get(),
            ivars.sidebar.get(),
            ivars.controls.get(),
            ivars.pane_scroll.get(),
        ) else {
            return;
        };
        let mut say = |what: String| problems.push(format!("Agents: {what}"));
        let Some(line) = controls.agent_where.as_ref() else {
            say("the page has no line on where the reply shows".to_owned());
            return;
        };
        let spec = &pages::groups(Category::Agents)[0];
        let Some(group) = controls
            .groups
            .iter()
            .find(|group| std::ptr::eq(group.spec, spec))
        else {
            say("the page has no group".to_owned());
            return;
        };
        let rows = &controls.rows[group.rows.clone()];
        let cases = [
            (TabPosition::Top, TabDetails::Smart),
            (TabPosition::Side, TabDetails::Smart),
            (TabPosition::Side, TabDetails::Always),
            (TabPosition::Side, TabDetails::Off),
        ];
        for (position, details) in cases {
            let mut frames = Vec::new();
            for on in [false, true] {
                let settings = Settings {
                    tab_position: position,
                    tab_details: details,
                    agents_claude_integration: on,
                    ..Settings::default()
                };
                self.refresh(&settings, resolved, &FileState::Missing, &[], themes);
                sidebar.selectRowIndexes_byExtendingSelection(
                    &NSIndexSet::indexSetWithIndex(SideItem::row_of(Category::Agents)),
                    false,
                );
                self.show_selected();
                let Some(content) = window.contentView() else {
                    return;
                };
                content.layoutSubtreeIfNeeded();
                let Some(document) = scroll.documentView() else {
                    return;
                };
                let case = format!("{position:?} and {details:?}, switch {on}");
                let said = pages::agent_where(&settings);
                let shown = line.open.isHidden();
                if controls.claude_integration.is_on() != on {
                    say(format!(
                        "the switch does not show the file's value ({case})"
                    ));
                }
                if line.open.isHidden() == said.button {
                    say(format!(
                        "the button is {} where it should be {} ({case})",
                        if shown { "hidden" } else { "shown" },
                        if said.button { "shown" } else { "hidden" },
                    ));
                }
                if controls.rows[line.row].note.stringValue().to_string() != said.text {
                    say(format!("the line does not say what it should ({case})"));
                }
                frames.push(
                    rows.iter()
                        .map(|row| in_document(&row.view, &document))
                        .collect::<Vec<_>>(),
                );
                if on && let Some(dir) = shots {
                    let name = format!("agents-{position:?}-{details:?}-window.png").to_lowercase();
                    picture(&content, &dir.join(name));
                }
            }
            if frames[0] != frames[1] {
                say(format!(
                    "a row moved when the switch turned ({position:?} and {details:?})"
                ));
            }
        }
        // The button is a way to Tabs and Panes: it is wired, and the way leads there.
        if line.open.action() != Some(sel!(showTabsPage:)) || line.open.target().is_none() {
            say("the button is not wired to the Tabs and Panes page".to_owned());
        }
        self.open_row(Category::Tabs, Some(Key::TabDetails));
        if sidebar.selectedRow() != SideItem::row_of(Category::Tabs) as isize {
            say("the way to Tabs and Panes does not open it".to_owned());
        }
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

    /// Shows the window before it has a theme, then refreshes it in a light theme and in a second
    /// light theme whose text is another colour, and lists each time the "Open settings.toml"
    /// button at the sidebar's foot is not titled in that theme's text colour on the screen. A
    /// button reads its title's colour when it is first drawn on the screen and keeps it: drawing
    /// it again does not read it again, nor does a title in the same colour object. A title left
    /// from before the theme came is a light grey on a light button, which reads as a button that
    /// is off. The window is shown off the screen and the window server's copy of it is read:
    /// drawing the sidebar into a bitmap draws its button afresh, in the right colour whatever the
    /// screen shows. The light themes keep the window's light appearance, so only the theme
    /// changes. Runs before anything else in the process sets the window's palette.
    pub(crate) fn title_problems(&self) -> Vec<String> {
        let ivars = self.ivars();
        let (Some(window), Some(open)) = (ivars.window.get(), ivars.side_open.get()) else {
            return vec!["the window was not built".to_owned()];
        };
        let settings = Settings::default();
        let embedded = Theme::embedded_groups();
        let (first, _) = settings::load_theme(None, &settings.light_theme).or_embedded(false);
        let text = |theme: &Theme| luma(Palette::of(theme).foreground);
        // The light theme whose text is farthest from the first one's, so the two are told apart.
        let Some(second) = embedded[2]
            .iter()
            .filter_map(|name| Theme::embedded(name))
            .max_by(|a, b| {
                let away = |theme: &Theme| (text(theme) - text(&first)).abs();
                away(a).total_cmp(&away(b))
            })
        else {
            return vec!["there is no second light theme".to_owned()];
        };

        window.setAppearance(super::look::appearance(&first).as_deref());
        window.setFrameTopLeftPoint(NSPoint::new(-4000.0, -4000.0));
        window.orderFront(None);
        let settle = || {
            NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.3));
        };
        settle();
        let mut problems = Vec::new();
        // The text colours the window had before: first its own, before it has a theme.
        let mut before = vec![luma(super::look::palette().foreground)];
        for (which, theme) in [
            ("the light theme", &first),
            ("a second light theme", &second),
        ] {
            let themes = ThemeLists {
                embedded: &embedded,
                user: &[],
                user_themes: &[],
                on_screen: theme,
                light: theme,
                dark: theme,
                dark_appearance: false,
            };
            let resolved = Resolved {
                reduce: false,
                scrollbar: ScrollbarMode::Auto,
            };
            self.refresh(&settings, resolved, &FileState::Missing, &[], &themes);
            settle();
            let want = text(theme);
            match darkest_on_screen(window, open) {
                None => {
                    problems.push("the window could not be read from the screen".to_owned());
                    break;
                }
                Some(got) => {
                    let off = |earlier: &f64| (got - earlier).abs();
                    if let Some(stale) = before
                        .iter()
                        .find(|earlier| off(earlier) <= (got - want).abs())
                    {
                        problems.push(format!(
                            "{which}: \"Open settings.toml\" is titled at {got:.2}, nearer a text \
                             colour the window had before ({stale:.2}) than the theme's ({want:.2})"
                        ));
                    }
                }
            }
            before.push(want);
        }
        window.orderOut(None);
        problems
    }
}

/// An sRGB `0xRRGGBB`'s luma (Rec. 709 weights on the encoded channels), 0 to 1.
fn luma(rgb: u32) -> f64 {
    let channel = |shift: u32| f64::from(rgb >> shift & 0xff) / 255.0;
    0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
}

/// The darkest luma in the middle of `button` as the screen shows it, where its title's strokes
/// are: read from the window server's copy of `window`. `None` when it cannot be read.
fn darkest_on_screen(window: &NSWindow, button: &NSView) -> Option<f64> {
    type Capture = unsafe extern "C" fn(CGRect, u32, u32, u32) -> *mut CGImage;
    const INCLUDING_WINDOW: u32 = 1 << 3;
    const IGNORE_FRAMING: u32 = 1 << 0;
    const BEST_RESOLUTION: u32 = 1 << 3;
    // Looked up when the check runs, not linked: the system marks the call for removal, and a
    // symbol gone from a later system must not keep the application from starting.
    // SAFETY: a NUL-terminated name; `RTLD_DEFAULT` searches every image the process loaded.
    let symbol = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"CGWindowListCreateImage".as_ptr()) };
    if symbol.is_null() {
        return None;
    }
    // SAFETY: the symbol is CoreGraphics' `CGWindowListCreateImage`, which has this signature.
    let capture: Capture = unsafe { std::mem::transmute::<*mut libc::c_void, Capture>(symbol) };
    let null = CGRect::new(
        CGPoint::new(f64::INFINITY, f64::INFINITY),
        CGSize::new(0.0, 0.0),
    );
    let number = u32::try_from(window.windowNumber()).ok()?;
    // SAFETY: the window is this process's and on the screen; the image follows the Create rule.
    let image = unsafe {
        capture(
            null,
            INCLUDING_WINDOW,
            number,
            IGNORE_FRAMING | BEST_RESOLUTION,
        )
    };
    // SAFETY: a +1 reference the call returned, or null.
    let image = unsafe { CFRetained::from_raw(NonNull::new(image)?) };
    let rep = NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), &image);
    let frame = window.frame();
    let scale = rep.pixelsWide() as f64 / frame.size.width;
    let place = button.convertRect_toView(button.bounds(), None);
    // The image's rows run down from the window's top; the window's coordinates run up.
    let top = (frame.size.height - place.origin.y - place.size.height) * scale;
    let left = place.origin.x * scale;
    let (wide, high) = (place.size.width * scale, place.size.height * scale);
    let mut darkest: Option<f64> = None;
    for y in (top + high * 0.25) as isize..(top + high * 0.75) as isize {
        for x in (left + wide * 0.2) as isize..(left + wide * 0.8) as isize {
            let Some(color) = rep.colorAtX_y(x, y) else {
                continue;
            };
            let value = 0.2126 * color.redComponent()
                + 0.7152 * color.greenComponent()
                + 0.0722 * color.blueComponent();
            darkest = Some(darkest.map_or(value, |d| d.min(value)));
        }
    }
    darkest
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
