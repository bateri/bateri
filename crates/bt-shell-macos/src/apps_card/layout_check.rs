//! A check of the card's layout that needs no eyes: the card is built for each of its states, stood
//! over the corner of a pane-sized stand-in the way the pane stands it, in a light and in a dark
//! appearance, and what must not run into anything is measured — the card inside the pane, what
//! it holds inside it, no two of its parts over one another, a button as wide as its title, a
//! name on one line. With a folder, each state is also written there as a picture.
//!
//! It runs from `tests/settings_window.rs`, which AppKit's main thread and the window server
//! session it wants are already arranged for.

use std::path::Path;

use bt_shell_common::apps::card::{Card, Row};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
    NSBackingStoreType, NSBitmapImageFileType, NSBox, NSBoxType, NSButton, NSColor, NSFont,
    NSTextField, NSTitlePosition, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSDictionary, NSPoint, NSRect, NSSize, NSString};

use super::view::{self, Target};
use crate::pane::CORNER_INSET;

/// How far two frames may run into each other before it is an overlap: the hairline rounding of a
/// backing scale.
const SLACK: f64 = 0.5;

/// The size of the pane the card is stood over: a small one, the smallest a person has a card in.
const PANE: (f64, f64) = (520.0, 300.0);

/// The height of the pane in the states that stand in a pane too low for a long list.
const LOW: f64 = 180.0;

fn row(name: &str) -> Row {
    Row {
        name: name.to_owned(),
        words: Vec::new(),
    }
}

/// The states: a name, the card in it, and the height of the pane it hangs over.
fn scenes() -> Vec<(&'static str, Card, f64)> {
    let made = |host: &str, names: &[&str]| {
        Card::new(host, names.iter().map(|name| row(name)).collect()).expect("rows make a card")
    };
    let mut none = made("dev-1", &["textpen", "notes"]);
    none.toggle(0);
    none.toggle(1);
    let mut installing = made("dev-1", &["textpen", "notes"]);
    installing.begin();
    installing.settle(0, Ok(()));
    let mut failed = made("dev-1", &["textpen", "notes"]);
    failed.begin();
    failed.settle(0, Ok(()));
    failed.settle(1, Err("No space left on this machine.".to_owned()));
    let mut long_why = made("dev-1", &["textpen", "notes", "sketch"]);
    long_why.begin();
    long_why.settle(0, Err(
        "kablo on this machine said: the release is signed with a key it does not know, and nobody can be asked here".to_owned(),
    ));
    long_why.settle(1, Ok(()));
    long_why.settle(2, Ok(()));
    let mut done = made("dev-1", &["textpen", "notes"]);
    done.begin();
    done.settle(0, Ok(()));
    done.settle(1, Ok(()));
    let twelve = || {
        made(
            "dev-1",
            &[
                "textpen",
                "notes",
                "dockerpen",
                "sqlpen",
                "pickpen",
                "sketch",
                "lab",
                "draw",
                "mono",
                "flaky",
                "twin",
                "gauge",
            ],
        )
    };
    let mut low_failed = twelve();
    low_failed.begin();
    low_failed.settle(0, Err("No space left on this machine.".to_owned()));
    for line in 1..12 {
        low_failed.settle(line, Ok(()));
    }
    vec![
        ("ask", made("dev-1", &["textpen", "notes"]), PANE.1),
        (
            "many",
            made(
                "dev-1",
                &[
                    "textpen",
                    "notes",
                    "dockerpen",
                    "sqlpen",
                    "pickpen",
                    "kubernetes-cluster-resource-inspector",
                ],
            ),
            PANE.1,
        ),
        ("scrolling", twelve(), PANE.1),
        ("scrolling-low", twelve(), LOW),
        ("failed-low", low_failed, LOW),
        ("none", none, PANE.1),
        ("installing", installing, PANE.1),
        ("failed", failed, PANE.1),
        ("failed-long", long_why, PANE.1),
        ("done", done, PANE.1),
        (
            "long-host",
            made(
                "a-very-long-server-name.production.example.internal",
                &["textpen"],
            ),
            PANE.1,
        ),
    ]
}

/// Lists what overlaps or leaves its place; with `shots`, writes `card-{state}-light.png` and
/// `card-{state}-dark.png` there.
pub(crate) fn layout_problems(mtm: MainThreadMarker, shots: Option<&Path>) -> Vec<String> {
    let mut problems = Vec::new();
    let target = Target::new(mtm, "layout-check");
    for dark in [false, true] {
        let look = if dark { "dark" } else { "light" };
        for (name, card, height) in scenes() {
            let (pane, window) = stand_in(mtm, dark, height);
            let built = view::build(mtm, &card, &target, height);
            let size = built.frame().size;
            built.setFrameOrigin(NSPoint::new(
                PANE.0 - CORNER_INSET.1 - size.width,
                height - CORNER_INSET.0 - size.height,
            ));
            pane.addSubview(&built);
            pane.layoutSubtreeIfNeeded();
            for problem in measure(&pane, &built) {
                problems.push(format!("apps card {name} ({look}): {problem}"));
            }
            if let Some(folder) = shots {
                picture(&pane, &folder.join(format!("card-{name}-{look}.png")));
            }
            drop(window);
        }
    }
    problems
}

/// A window off the screen that holds a pane-sized view, in the appearance asked for, with a few
/// lines of terminal in it so that the corner is seen over something.
fn stand_in(
    mtm: MainThreadMarker,
    dark: bool,
    height: f64,
) -> (Retained<NSView>, Retained<NSWindow>) {
    let frame = NSRect::new(
        NSPoint::new(-10_000.0, -10_000.0),
        NSSize::new(PANE.0, height),
    );
    // SAFETY: a window made and kept by this function, never shown; AppKit's designated
    // initializer with a borderless style.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            mtm.alloc::<NSWindow>(),
            frame,
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: the window is this function's, and the next line gives it the content.
    unsafe { window.setReleasedWhenClosed(false) };
    let name = if dark {
        // SAFETY: AppKit's constant names of the appearances.
        unsafe { NSAppearanceNameDarkAqua }
    } else {
        // SAFETY: as above.
        unsafe { NSAppearanceNameAqua }
    };
    window.setAppearance(NSAppearance::appearanceNamed(name).as_deref());
    let ground = NSBox::new(mtm);
    ground.setBoxType(NSBoxType::Custom);
    ground.setTitlePosition(NSTitlePosition::NoTitle);
    ground.setBorderWidth(0.0);
    let fill = if dark {
        NSColor::colorWithSRGBRed_green_blue_alpha(0.07, 0.07, 0.08, 1.0)
    } else {
        NSColor::colorWithSRGBRed_green_blue_alpha(0.99, 0.99, 0.99, 1.0)
    };
    ground.setFillColor(&fill);
    ground.setFrame(NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(PANE.0, height),
    ));
    for (i, line) in [
        "omer@dev-1:~/app$ ls",
        "Cargo.toml  src  target  README.md",
        "omer@dev-1:~/app$ git status",
        "On branch main",
        "omer@dev-1:~/app$",
    ]
    .iter()
    .enumerate()
    {
        let text = NSTextField::labelWithString(&NSString::from_str(line), mtm);
        text.setFont(Some(&NSFont::monospacedSystemFontOfSize_weight(12.0, 0.0)));
        text.setTextColor(Some(&NSColor::labelColor()));
        text.setFrameOrigin(NSPoint::new(10.0, height - 22.0 - i as f64 * 16.0));
        ground.addSubview(&text);
    }
    window.setContentView(Some(&ground));
    let content = window
        .contentView()
        .expect("the window holds the stand-in it was given");
    (content, window)
}

/// What is wrong with `card` over `pane`.
fn measure(pane: &NSView, card: &NSView) -> Vec<String> {
    let mut problems = Vec::new();
    let place = card.frame();
    let room = NSRect::new(NSPoint::new(0.0, 0.0), pane.bounds().size);
    if !contains(room, place) {
        problems.push(format!("the card {} is not inside the pane", show(place)));
    }
    if place.size.height > room.size.height * 0.8 {
        problems.push(format!(
            "the card is {} high in a pane {}",
            place.size.height, room.size.height
        ));
    }
    let own = NSRect::new(NSPoint::new(0.0, 0.0), place.size);
    let mut parts = Vec::new();
    gather(card, &mut parts);
    for (what, frame) in &parts {
        if !contains(own, *frame) {
            problems.push(format!("{what} {} leaves the card", show(*frame)));
        }
    }
    for (i, (what, frame)) in parts.iter().enumerate() {
        for (other, there) in &parts[i + 1..] {
            if overlap(*frame, *there) {
                problems.push(format!(
                    "{what} {} runs over {other} {}",
                    show(*frame),
                    show(*there)
                ));
            }
        }
    }
    for button in buttons(card) {
        let wanted = view::wanted_width(&button);
        if wanted > button.frame().size.width + SLACK {
            problems.push(format!(
                "button {:?} is {} wide and its title wants {wanted}",
                button.title(),
                button.frame().size.width
            ));
        }
    }
    problems
}

/// The parts of the card in the card's own coordinates: what it holds directly and, in a list
/// that does not scroll, the rows' marks, names and reasons. A scrolled list is one part.
fn gather(card: &NSView, parts: &mut Vec<(String, NSRect)>) {
    for sub in card.subviews() {
        let name = sub.class().name().to_string_lossy().into_owned();
        if name == "BateriAppsCardList" {
            let origin = sub.frame().origin;
            for part in sub.subviews() {
                let frame = part.frame();
                let (width, height) = (frame.size.width, frame.size.height);
                if height > 0.0 && width > 0.0 {
                    parts.push((
                        part.class().name().to_string_lossy().into_owned(),
                        NSRect::new(
                            NSPoint::new(frame.origin.x + origin.x, frame.origin.y + origin.y),
                            frame.size,
                        ),
                    ));
                }
            }
        } else {
            parts.push((name, sub.frame()));
        }
    }
}

fn buttons(card: &NSView) -> Vec<Retained<NSButton>> {
    view::controls(card)
        .into_iter()
        .filter_map(|control| control.downcast::<NSButton>().ok())
        .filter(|button| button.title().length() > 0)
        .collect()
}

fn show(rect: NSRect) -> String {
    format!(
        "({:.1}, {:.1}, {:.1} x {:.1})",
        rect.origin.x, rect.origin.y, rect.size.width, rect.size.height
    )
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
