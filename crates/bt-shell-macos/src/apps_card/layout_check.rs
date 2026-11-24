//! A check of the card's layout that needs no eyes: the card is built for each of its states, stood
//! over the corner of a pane-sized stand-in the way the pane stands it, in a light and in a dark
//! appearance, and what must not run into anything is measured — the card inside the pane, what
//! it holds inside it, no two of its parts over one another, a button as wide as its title, a
//! name on one line. With a folder, each state is also written there as a picture.
//!
//! It runs from `tests/settings_window.rs`, which AppKit's main thread and the window server
//! session it wants are already arranged for.

use std::path::Path;

use bt_shell_common::apps::card::{Card, Coming, Row, Stage, Why};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSAccessibility, NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua,
    NSAppearanceNameDarkAqua, NSBackingStoreType, NSBitmapImageFileType, NSBox, NSBoxType,
    NSButton, NSColor, NSFont, NSTextField, NSTitlePosition, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSDictionary, NSPoint, NSRect, NSSize, NSString};

use super::view::{self, Target};
use crate::pane::{CORNER_INSET, card_top_inset, stand_card_over};
use crate::split_view::put_in_order;

/// How far two frames may run into each other before it is an overlap: the hairline rounding of a
/// backing scale.
const SLACK: f64 = 0.5;

/// The size of the pane the card is stood over: a small one, the smallest a person has a card in.
const PANE: (f64, f64) = (520.0, 300.0);

/// The width of a narrow pane (a scene whose name ends in `-narrow`), and of the narrowest one the
/// card is made whole for (`-narrowest`): the card is as wide as the pane less the room it keeps
/// at its sides, down to its least width.
const NARROW: f64 = 200.0;
const NARROWEST: f64 = 164.0;

/// The width of the pane a scene is stood in.
fn width_of(name: &str) -> f64 {
    if name.ends_with("-narrowest") {
        NARROWEST
    } else if name.ends_with("-narrow") {
        NARROW
    } else {
        PANE.0
    }
}

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
    let waiting = |program: &str, why: Why| Card::for_waiting_view(Some(program), why);
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
    let link = |host: &str| Card::for_link(host, row("browserpen"), "http://localhost:8790/");
    let mut link_installing = link("rasp");
    link_installing.begin();
    let mut link_failed = link("rasp");
    link_failed.begin();
    link_failed.settle(0, Err("No space left on this machine.".to_owned()));
    let mut link_done = link("rasp");
    link_done.begin();
    link_done.settle(0, Ok(()));
    // The first click's question, over a pane of this Mac or of a server: before anything is
    // decided, when it is answered yes, and in what comes of that.
    let here = |program_here: bool, engine_here: bool| {
        Card::for_here(
            "http://localhost:8790/",
            program_here,
            engine_here,
            Some(181_000_000),
        )
    };
    let on = |program_here: bool, engine_here: bool, program: Coming, engine: Coming| {
        let mut card = here(program_here, engine_here);
        card.say_yes();
        card.sync_here(&program, &engine);
        card
    };
    let key = "browserpen is signed with a key you have not approved yet. Approve it in Settings\u{a0}▸\u{a0}Apps.";
    let here_starting = on(false, false, Coming::Busy(None), Coming::Busy(None));
    let here_downloading = on(false, false, Coming::Busy(None), Coming::Busy(Some(42)));
    let here_program_in = on(false, false, Coming::Done, Coming::Busy(Some(42)));
    let here_finishing = on(false, false, Coming::Done, Coming::Busy(Some(100)));
    let here_engine_only = on(true, false, Coming::Done, Coming::Busy(Some(7)));
    let here_failed = on(
        false,
        false,
        Coming::Done,
        Coming::Failed("The server answered 503.".to_owned()),
    );
    let here_offline = on(
        false,
        false,
        Coming::Failed("No connection to the internet.".to_owned()),
        Coming::Failed("No connection to the internet.".to_owned()),
    );
    let here_key = on(
        false,
        false,
        Coming::Failed(key.to_owned()),
        Coming::Busy(Some(42)),
    );
    let here_key_settled = on(false, false, Coming::Failed(key.to_owned()), Coming::Done);
    let here_done = on(false, false, Coming::Done, Coming::Done);
    let mut here_unwritten = here(false, false);
    here_unwritten.say_yes();
    here_unwritten.fail("An entry of [apps] sources could not be read, so bateri does not rewrite the list. Fix it, or add bateri/browserpen to it, and choose browserpen again.");
    let mut here_unwritten_all_here = here(true, true);
    here_unwritten_all_here.say_yes();
    here_unwritten_all_here.fail("settings.toml is not available.");
    vec![
        ("link-local-ask", here(false, false), PANE.1),
        ("link-local-ask-engine", here(true, false), PANE.1),
        ("link-local-ask-all-here", here(true, true), PANE.1),
        ("link-local-ask-narrow", here(false, false), PANE.1),
        ("link-local-ask-narrowest", here(false, false), 420.0),
        ("link-local-ask-over-floating", here(false, false), PANE.1),
        ("link-local-ask-under-find", here(false, false), PANE.1),
        ("link-local-starting", here_starting, PANE.1),
        ("link-local-downloading", here_downloading, PANE.1),
        (
            "link-local-downloading-program-in",
            here_program_in.clone(),
            PANE.1,
        ),
        ("link-local-downloading-finishing", here_finishing, PANE.1),
        (
            "link-local-downloading-engine-only",
            here_engine_only,
            PANE.1,
        ),
        ("link-local-downloading-narrow", here_program_in, PANE.1),
        ("link-local-failed", here_failed.clone(), PANE.1),
        ("link-local-failed-narrow", here_failed, PANE.1),
        ("link-local-failed-offline", here_offline, PANE.1),
        ("link-local-failed-long", here_key, PANE.1),
        (
            "link-local-failed-long-narrow",
            here_key_settled.clone(),
            PANE.1,
        ),
        (
            "link-local-failed-long-narrowest",
            here_key_settled.clone(),
            420.0,
        ),
        ("link-local-failed-long-low", here_key_settled, LOW),
        ("link-local-failed-unwritten", here_unwritten, PANE.1),
        (
            "link-local-failed-unwritten-all-here",
            here_unwritten_all_here,
            PANE.1,
        ),
        ("link-local-done", here_done, PANE.1),
        ("link-ask", link("rasp"), PANE.1),
        ("link-ask-narrow", link("rasp"), PANE.1),
        ("link-ask-over-floating", link("rasp"), PANE.1),
        ("link-ask-under-find", link("rasp"), PANE.1),
        (
            "link-long-host",
            link("a-very-long-server-name.production.example.internal"),
            PANE.1,
        ),
        ("link-installing", link_installing, PANE.1),
        ("link-failed", link_failed.clone(), PANE.1),
        ("link-failed-narrow", link_failed, PANE.1),
        ("link-done", link_done, PANE.1),
        ("ask", made("dev-1", &["textpen", "notes"]), PANE.1),
        ("ask-narrow", made("dev-1", &["textpen", "notes"]), PANE.1),
        (
            "ask-over-floating",
            made("dev-1", &["textpen", "notes"]),
            PANE.1,
        ),
        (
            "ask-under-find",
            made("dev-1", &["textpen", "notes"]),
            PANE.1,
        ),
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
        ("failed", failed.clone(), PANE.1),
        ("failed-narrow", failed, PANE.1),
        ("failed-long", long_why, PANE.1),
        ("done", done, PANE.1),
        // The notices: a program's view that waits (after a quit, after a page failed, with a long
        // name and with none) and the windows a launch gave up.
        ("notice-view-quit", waiting("linearpen", Why::Quit), PANE.1),
        (
            "notice-view-quit-narrow",
            waiting("linearpen", Why::Quit),
            PANE.1,
        ),
        (
            "notice-view-quit-narrowest",
            waiting("linearpen", Why::Quit),
            420.0,
        ),
        ("notice-view-quit-low", waiting("linearpen", Why::Quit), LOW),
        (
            "notice-view-quit-over-floating",
            waiting("linearpen", Why::Quit),
            PANE.1,
        ),
        (
            "notice-view-quit-under-find",
            waiting("linearpen", Why::Quit),
            PANE.1,
        ),
        (
            "notice-view-quit-long-name",
            waiting(
                "a-very-long-program-name-that-a-person-gave-their-own-view",
                Why::Quit,
            ),
            PANE.1,
        ),
        (
            "notice-view-quit-unnamed",
            Card::for_waiting_view(None, Why::Quit),
            PANE.1,
        ),
        (
            "notice-view-stopped",
            waiting("linearpen", Why::Stopped),
            PANE.1,
        ),
        (
            "notice-view-stopped-narrow",
            waiting("linearpen", Why::Stopped),
            PANE.1,
        ),
        ("notice-windows", Card::for_given_up_windows(), PANE.1),
        (
            "notice-windows-narrow",
            Card::for_given_up_windows(),
            PANE.1,
        ),
        (
            "notice-windows-narrowest",
            Card::for_given_up_windows(),
            420.0,
        ),
        ("notice-windows-low", Card::for_given_up_windows(), LOW),
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
            let width = width_of(name);
            let (pane, window) = stand_in(mtm, dark, width, height);
            // The container the panes of a tab stand in, and the pane in it that the card is over:
            // the card is stood there by the pane's own code ([`stand_card_over`]).
            let container = plain_view(mtm, width, height);
            pane.addSubview(&container);
            let terminal = plain_view(mtm, width, height);
            container.addSubview(&terminal);
            // The find panel, open in the same corner, and the card under it.
            let panel = name
                .ends_with("-under-find")
                .then(|| find_panel(mtm, width, height));
            let top = card_top_inset(height, panel.as_ref().map(|panel| panel.frame()));
            let built = view::build(mtm, &card, &target, NSSize::new(width, height), top);
            if let Some(panel) = &panel {
                container.addSubview(panel);
            }
            stand_card_over(&built, &terminal, &container, top);
            let mut found = Vec::new();
            if name.ends_with("-over-floating") {
                found.extend(over_floating(mtm, dark, &container, &built));
            }
            if let Some(panel) = &panel {
                found.extend(under_find(&container, &terminal, &built, panel, height));
            }
            pane.layoutSubtreeIfNeeded();
            for problem in found
                .into_iter()
                .chain(measure(&pane, &built))
                .chain(speech(&card, &built))
            {
                problems.push(format!("apps card {name} ({look}): {problem}"));
            }
            if let Some(folder) = shots {
                picture(&pane, &folder.join(format!("card-{name}-{look}.png")));
            }
            drop(window);
        }
    }
    problems.extend(in_place_problems(mtm));
    problems
}

/// A view `width` by `height` at the origin.
fn plain_view(mtm: MainThreadMarker, width: f64, height: f64) -> Retained<NSView> {
    NSView::initWithFrame(
        mtm.alloc::<NSView>(),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, height)),
    )
}

/// The size of the find panel as the pane's own makes it: its field, its count, its toggles and
/// its buttons in one row, and the padding round them.
const FIND_PANEL: (f64, f64) = (440.0, 38.0);

/// A stand-in for the find panel, at the pane's top right where it rests.
fn find_panel(mtm: MainThreadMarker, width: f64, height: f64) -> Retained<NSBox> {
    let panel = NSBox::new(mtm);
    panel.setBoxType(NSBoxType::Custom);
    panel.setTitlePosition(NSTitlePosition::NoTitle);
    panel.setCornerRadius(11.0);
    panel.setBorderWidth(1.0);
    panel.setBorderColor(&NSColor::separatorColor());
    panel.setFillColor(&NSColor::controlBackgroundColor());
    panel.setContentViewMargins(NSSize::new(0.0, 0.0));
    panel.setFrame(NSRect::new(
        NSPoint::new(width - 10.0 - FIND_PANEL.0, height - 10.0 - FIND_PANEL.1),
        NSSize::new(FIND_PANEL.0, FIND_PANEL.1),
    ));
    let text =
        NSTextField::labelWithString(&NSString::from_str("Find   deploy        3 of 12"), mtm);
    text.setFont(Some(&NSFont::systemFontOfSize(13.0)));
    text.setTextColor(Some(&NSColor::labelColor()));
    text.setFrameOrigin(NSPoint::new(14.0, 10.0));
    panel.addSubview(&text);
    panel
}

/// The find panel is open in the corner the card is in: the card stands under it, the same
/// distance from its lower edge as it stands from the pane's top, and runs over nothing of it; where
/// the panel is not open it stands where it did, at the top. (At the top it would be over the
/// panel, which is checked first: else the panel stands where the card is not and the check
/// says nothing.)
fn under_find(
    container: &NSView,
    terminal: &NSView,
    card: &NSView,
    panel: &NSBox,
    height: f64,
) -> Vec<String> {
    let mut problems = Vec::new();
    let panel_at = panel.frame();
    stand_card_over(card, terminal, container, card_top_inset(height, None));
    if !overlap(card.frame(), panel_at) {
        problems
            .push("the card is not over the find panel at the top, so nothing is checked".into());
    }
    stand_card_over(
        card,
        terminal,
        container,
        card_top_inset(height, Some(panel_at)),
    );
    let at = card.frame();
    if overlap(at, panel_at) {
        problems.push(format!(
            "the card {} runs over the find panel {}",
            show(at),
            show(panel_at)
        ));
    }
    let gap = panel_at.origin.y - (at.origin.y + at.size.height);
    if (gap - CORNER_INSET.0).abs() > SLACK {
        problems.push(format!(
            "the card is {gap} under the find panel, not {}",
            CORNER_INSET.0
        ));
    }
    // The panel closes: the card goes back to the top.
    stand_card_over(card, terminal, container, card_top_inset(height, None));
    let up = card.frame();
    let top = height - (up.origin.y + up.size.height);
    if (top - CORNER_INSET.0).abs() > SLACK {
        problems.push(format!(
            "the card is {top} from the top when the panel is closed, not {}",
            CORNER_INSET.0
        ));
    }
    stand_card_over(
        card,
        terminal,
        container,
        card_top_inset(height, Some(panel_at)),
    );
    problems
}

/// A floating pane of the tab comes up over the corner where the card stands, after the card was
/// there, and the container puts its views in order the way it does at every layout
/// ([`put_in_order`]: the floating pane, then the card). The card is then over the pane and seen
/// whole: its buttons are what a click on them reaches, and the pane is not. The pane is stood so
/// that before the order is put every button is under it (else the check says nothing).
fn over_floating(
    mtm: MainThreadMarker,
    dark: bool,
    container: &NSView,
    card: &NSView,
) -> Vec<String> {
    let mut problems = Vec::new();
    let at = card.frame();
    let floating = NSBox::new(mtm);
    floating.setBoxType(NSBoxType::Custom);
    floating.setTitlePosition(NSTitlePosition::NoTitle);
    floating.setBorderWidth(1.0);
    floating.setBorderColor(&NSColor::separatorColor());
    floating.setCornerRadius(10.0);
    let tint = if dark {
        NSColor::colorWithSRGBRed_green_blue_alpha(0.16, 0.20, 0.30, 1.0)
    } else {
        NSColor::colorWithSRGBRed_green_blue_alpha(0.86, 0.92, 1.0, 1.0)
    };
    floating.setFillColor(&tint);
    // Over the card's left and lower part, and past it to the left.
    floating.setFrame(NSRect::new(
        NSPoint::new(at.origin.x - 120.0, at.origin.y - 80.0),
        NSSize::new(at.size.width * 0.9 + 120.0, at.size.height * 0.6 + 80.0),
    ));
    for (i, line) in ["browserpen", "http://localhost:8790/", "Loading\u{2026}"]
        .iter()
        .enumerate()
    {
        let text = NSTextField::labelWithString(&NSString::from_str(line), mtm);
        text.setFont(Some(&NSFont::systemFontOfSize(12.0)));
        text.setTextColor(Some(&NSColor::labelColor()));
        text.setFrameOrigin(NSPoint::new(
            12.0,
            floating.frame().size.height - 30.0 - i as f64 * 20.0,
        ));
        floating.addSubview(&text);
    }
    // It is added after the card: it is over it, as a pane opened while a card is up is.
    container.addSubview(&floating);
    let ring = plain_view(mtm, 0.0, 0.0);
    container.addSubview(&ring);
    let reached = |button: &NSButton| {
        let frame = button.frame();
        let centre = NSPoint::new(
            frame.origin.x + frame.size.width / 2.0,
            frame.origin.y + frame.size.height / 2.0,
        );
        let there = card.convertPoint_toView(centre, Some(container));
        // A view's `hitTest:` is given the point in its superview's coordinates, which the
        // container shares (it stands at the origin of the stand-in).
        container.hitTest(there).is_some_and(|found| {
            std::ptr::eq(
                Retained::as_ptr(&found).cast::<()>(),
                std::ptr::from_ref::<NSButton>(button).cast::<()>(),
            )
        })
    };
    let buttons = buttons(card);
    if buttons.is_empty() {
        problems.push("the card has no buttons to be reached".to_owned());
    }
    for button in &buttons {
        if reached(button) {
            problems.push(format!(
                "the floating pane does not cover the button {:?} before the order is put",
                button.title()
            ));
        }
    }
    let floating_view: Retained<NSView> = Retained::into_super(floating);
    put_in_order(
        container,
        &[floating_view.clone(), objc2::Message::retain(card)],
        &ring,
        None,
    );
    let order: Vec<_> = container.subviews().iter().collect();
    let place = |view: &NSView| order.iter().position(|v| std::ptr::eq(&**v, view));
    if place(card) <= place(&floating_view) {
        problems.push("the card is not in front of the floating pane".to_owned());
    }
    for button in &buttons {
        if !reached(button) {
            problems.push(format!(
                "a click on the button {:?} does not reach it, with the floating pane over its place",
                button.title()
            ));
        }
    }
    problems
}

/// What a download's percent moving does to the card of the first click: the number under the
/// engine's name is written in its label and the view is not made again, so the ring of the row is
/// the same object and goes on turning; a number that comes (or a row that changes) is another
/// matter, the card is made again.
fn in_place_problems(mtm: MainThreadMarker) -> Vec<String> {
    let mut problems = Vec::new();
    let target = Target::new(mtm, "layout-check");
    let at = |engine: Coming| {
        let mut card = Card::for_here("http://localhost:8790/", false, false, Some(181_000_000));
        card.say_yes();
        card.sync_here(&Coming::Done, &engine);
        card
    };
    let (before, after) = (at(Coming::Busy(Some(42))), at(Coming::Busy(Some(43))));
    let room = NSSize::new(PANE.0, PANE.1);
    let built = view::build(mtm, &before, &target, room, CORNER_INSET.0);
    let ring_of = |card: &NSView| {
        let mut found = Vec::new();
        collect_class(card, "NSProgressIndicator", &mut found);
        found
    };
    let rings = ring_of(&built);
    if rings.len() != 1 {
        problems.push(format!("the card has {} rings, not one", rings.len()));
        return problems;
    }
    let size = built.frame().size;
    if !view::refresh_notes(&built, &before, &after) {
        problems.push("a percent that moved was not written in place".to_owned());
    }
    let again = ring_of(&built);
    if again.len() != 1 || !std::ptr::eq(Retained::as_ptr(&again[0]), Retained::as_ptr(&rings[0])) {
        problems
            .push("the ring of the row is not the same view after the percent moved".to_owned());
    }
    if built.frame().size != size {
        problems.push("the card changed its size when a percent moved".to_owned());
    }
    let mut said = Vec::new();
    collect_labels(&built, &mut said);
    for want in ["43% of 181 MB", "Browser engine, on its way, 43% of 181 MB"] {
        if !said.iter().any(|label| label == want) {
            problems.push(format!(
                "after the percent moved the card does not say {want:?}"
            ));
        }
    }
    if said.iter().any(|label| label.contains("42%")) {
        problems.push("the old percent is still said after it moved".to_owned());
    }
    // The first figure makes the row taller, and a row that changes is not a number: both are the
    // card made again.
    let none = at(Coming::Busy(None));
    let bare = view::build(mtm, &none, &target, room, CORNER_INSET.0);
    if view::refresh_notes(&bare, &none, &before) {
        problems
            .push("a percent that came was written in place, where the row is taller".to_owned());
    }
    // A pane that was resized is another width: the card is made again though nothing else changed.
    let top = CORNER_INSET.0;
    let narrow = NSSize::new(NARROW, PANE.1);
    if view::made_for(mtm, &built, &after, &target, narrow, top) {
        problems.push("a card was kept at its width in a pane of another width".to_owned());
    }
    if !view::made_for(mtm, &built, &after, &target, room, top) {
        problems.push("a card that nothing changed the room of was made again".to_owned());
    }
    if !view::refresh_notes(&built, &after, &after) {
        problems.push("a card that nothing changed in was made again".to_owned());
    }
    let failed = at(Coming::Failed("The server answered 503.".to_owned()));
    if view::refresh_notes(&built, &after, &failed) {
        problems.push("a row that failed was written in place".to_owned());
    }
    problems.extend(height_problems(mtm));
    problems
}

/// A pane that becomes lower or higher, or a find panel that takes the corner: the room of the
/// card's list is the pane's height under the card's top, so a card the new room cuts differently
/// is made again (kept, its buttons would be below the pane), and one it does not touch is left as
/// it is (made again at every step of a download or of a drag of the divider, its rings would start
/// over). In a pane too low for the card's foot, the list gives up its room down to nothing, under
/// the panel too.
fn height_problems(mtm: MainThreadMarker) -> Vec<String> {
    let mut problems = Vec::new();
    let target = Target::new(mtm, "layout-check");
    let scene = |name: &str| {
        scenes()
            .into_iter()
            .find(|(found, ..)| *found == name)
            .map(|(_, card, _)| card)
    };
    let (Some(ask), Some(cut), Some(scrolling)) =
        (scene("ask"), scene("failed-low"), scene("scrolling"))
    else {
        return vec!["a scene the height checks stand on is gone".to_owned()];
    };
    let top = CORNER_INSET.0;
    let room = |height: f64| NSSize::new(PANE.0, height);
    let made = view::build(mtm, &ask, &target, room(PANE.1), top);
    // A card the room does not cut is left as it is, in a pane made higher or a little lower.
    for height in [PANE.1 + 120.0, PANE.1 - 30.0] {
        if !view::made_for(mtm, &made, &ask, &target, room(height), top) {
            problems.push(format!(
                "a card the room does not cut was made again in a pane {height} high"
            ));
        }
    }
    // One the room cuts is made again, lower or higher.
    let tall = view::build(mtm, &cut, &target, room(PANE.1), top);
    let low = view::build(mtm, &cut, &target, room(LOW), top);
    if low.frame().size.height >= tall.frame().size.height - SLACK {
        problems.push(
            "the card made for a low pane is no lower than the one made for a tall pane: the \
             scene does not show what the height of the pane changes"
                .to_owned(),
        );
    }
    if view::made_for(mtm, &tall, &cut, &target, room(LOW), top) {
        problems.push("a card was kept at its height in a pane that became lower".to_owned());
    }
    if view::made_for(mtm, &low, &cut, &target, room(PANE.1), top) {
        problems.push("a card was kept at its height in a pane that became higher".to_owned());
    }
    // A pane too low for the card's foot: the foot stands in it, the list has given up its room.
    for height in [110.0, 120.0, 140.0] {
        let card = view::build(mtm, &ask, &target, room(height), top);
        let foot = top + card.frame().size.height;
        if foot > height - CORNER_INSET.0 + SLACK {
            problems.push(format!(
                "the card made for a pane {height} high ends {foot} from its top"
            ));
        }
    }
    // The find panel in the corner: the card stands under it, and the room is what is left.
    for height in [200.0, 220.0, 260.0] {
        let panel = find_panel(mtm, PANE.0, height);
        let top = card_top_inset(height, Some(panel.frame()));
        let card = view::build(mtm, &scrolling, &target, room(height), top);
        let foot = top + card.frame().size.height;
        if foot > height - CORNER_INSET.0 + SLACK {
            problems.push(format!(
                "under the find panel the card made for a pane {height} high ends {foot} from its top"
            ));
        }
        // The panel opening changes what the card shows in a pane this low: the card is made again.
        let before = view::build(mtm, &scrolling, &target, room(height), CORNER_INSET.0);
        if view::made_for(mtm, &before, &scrolling, &target, room(height), top) {
            problems.push(format!(
                "a card was kept when the find panel took the corner of a pane {height} high"
            ));
        }
    }
    // In a pane that has room, the card keeps to its share of what is under the panel, not of the
    // pane: the same share of a lower corner.
    let panel = find_panel(mtm, PANE.0, PANE.1);
    let top = card_top_inset(PANE.1, Some(panel.frame()));
    let card = view::build(mtm, &scrolling, &target, room(PANE.1), top);
    let share = view::ROOM_SHARE * (PANE.1 - (top - CORNER_INSET.0));
    if card.frame().size.height > share + SLACK {
        problems.push(format!(
            "under the find panel the card is {} high, more than its share, {share}, of what is under it",
            card.frame().size.height
        ));
    }
    problems
}

/// The views of the class named `class` under `view`.
fn collect_class(view: &NSView, class: &str, found: &mut Vec<Retained<NSView>>) {
    for sub in view.subviews() {
        if sub.class().name().to_string_lossy() == class {
            found.push(sub.clone());
        }
        collect_class(&sub, class, found);
    }
}

/// A window off the screen that holds a pane-sized view, in the appearance asked for, with a few
/// lines of terminal in it so that the corner is seen over something.
fn stand_in(
    mtm: MainThreadMarker,
    dark: bool,
    width: f64,
    height: f64,
) -> (Retained<NSView>, Retained<NSWindow>) {
    let frame = NSRect::new(
        NSPoint::new(-10_000.0, -10_000.0),
        NSSize::new(width, height),
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
    // A box keeps its content a few points in from its edge: the stand-in is the pane, whose
    // content reaches its edge.
    ground.setContentViewMargins(NSSize::new(0.0, 0.0));
    let fill = if dark {
        NSColor::colorWithSRGBRed_green_blue_alpha(0.07, 0.07, 0.08, 1.0)
    } else {
        NSColor::colorWithSRGBRed_green_blue_alpha(0.99, 0.99, 0.99, 1.0)
    };
    ground.setFillColor(&fill);
    ground.setFrame(NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(width, height),
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

/// What a screen reader is told by the card: the card itself by its title, each row that is shown
/// by its name, how it stands and what is under the name, and each button by its title (and key).
/// What is missing is a row or a button a person who cannot see the card would not find.
fn speech(card: &Card, built: &NSView) -> Vec<String> {
    let mut said = Vec::new();
    collect_labels(built, &mut said);
    let mut wanted = vec![card.title()];
    let rows_shown = !(card.stage() == Stage::Ask && (card.is_here() || card.link().is_some()))
        && !card.not_saved();
    if rows_shown && card.stage() != Stage::Ask {
        wanted.extend(card.lines().iter().map(view::spoken));
    }
    for button in buttons(built) {
        // The title with its key after two spaces is said "title, key".
        wanted.push(button.title().to_string().replacen("  ", ", ", 1));
    }
    let mut problems = Vec::new();
    for want in wanted {
        if !said
            .iter()
            .any(|label| label == &want || label.starts_with(&format!("{want}, ")))
        {
            problems.push(format!("a screen reader is not told {want:?}"));
        }
    }
    // The lines under the card's title are text a screen reader reads as they are.
    for note in card.notes() {
        if !said.iter().any(|label| label == &note) {
            problems.push(format!("a screen reader is not told {note:?}"));
        }
    }
    problems
}

/// The accessibility label of `view` and of everything under it, and the text of its labels.
fn collect_labels(view: &NSView, found: &mut Vec<String>) {
    if let Some(label) = view.accessibilityLabel() {
        found.push(label.to_string());
    }
    if let Some(field) = view.downcast_ref::<NSTextField>() {
        found.push(field.stringValue().to_string());
    }
    for sub in view.subviews() {
        collect_labels(&sub, found);
    }
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
        if !view::holds_title(&button) {
            problems.push(format!(
                "button {:?} is {} wide and {} high, which does not hold its title ({} wanted in a line)",
                button.title(),
                button.frame().size.width,
                button.frame().size.height,
                view::wanted_width(&button),
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
