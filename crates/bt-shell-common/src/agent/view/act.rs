//! What an agent does to a page: each action of `bateri view`, done as the person would do it, but
//! through the page's own inputs ([`PageHandle`], [`pencere_drive::Page`]) and never through the
//! window: no responder is made first, no pane is brought forward, no shortcut of the app's is
//! pressed, no byte is written to a terminal.
//!
//! The page is read first, by what it is now, so that an element that goes with the action (a
//! palette row that closes the palette) is named by what it was.

use pencere_drive::reading::{CommandReading, Found, reference};
use pencere_drive::{Reading, Unreachable};
use pencere_view::edit::EditCommand;

use super::held::Shared;
use super::words;
use super::{Ctx, Env};
use crate::agent::message::{Failure, Kind};
use crate::native_view::{
    Button, Id, Input, Key, KeyConvention, Mods, Phase, Point, Route, Shortcut, ShownMenu,
};
use kablo_bateri::view::{Action, Way};

/// What an action did.
#[derive(Debug, Default)]
pub struct Done {
    /// The line that says what was done, with the element's role and words.
    pub said: Option<String>,
    /// The whole answer, for an action that is one (`text`).
    pub text: Option<String>,
    /// Whether a menu was asked for: where none came, the answer says so.
    pub asked_for_menu: bool,
}

impl Done {
    fn said(line: impl Into<String>) -> Result<Done, Failure> {
        Ok(Done {
            said: Some(line.into()),
            ..Done::default()
        })
    }
}

fn mistake(text: impl Into<String>) -> Failure {
    Failure::new(Kind::Failed, text)
}

/// Does `action` to the page of `ctx`, which `reading` is the reading of now.
pub fn perform(
    ctx: &Ctx<'_>,
    env: &Env<'_>,
    reading: &Reading,
    action: &Action,
) -> Result<Done, Failure> {
    match action {
        // A picture is a reading that ends in pixels: nothing is done to the page for it.
        Action::Read | Action::Wait { .. } | Action::Shot { .. } => Ok(Done::default()),
        Action::Command(title) => command(ctx, reading, title),
        Action::Click(word) => click(ctx, env, reading, word, 1),
        Action::DoubleClick(word) => click(ctx, env, reading, word, 2),
        Action::Fill { target, text } => fill(ctx, env, reading, target, text),
        Action::Type(text) => typed(ctx, env, text),
        Action::Press(keys) => press(ctx, env, keys),
        Action::Scroll { target, way, count } => scroll(ctx, reading, target, *way, *count),
        Action::Menu(word) => menu(ctx, reading, word),
        Action::Pick(entry) => pick(ctx, env, entry),
        Action::Text(word) => text(ctx, env, reading, word),
    }
}

// ─── Naming ─────────────────────────────────────────────────────────────────────────────────────

/// The element `word` names, or why none.
fn target(reading: &Reading, word: &str) -> Result<Id, Failure> {
    match reading.find(word) {
        Found::One(id) => Ok(id),
        Found::Nothing => Err(mistake(format!(
            "no element of the page is named {} now: read the view again for the names it has",
            words(word, 40)
        ))),
        Found::Clash => Err(mistake(format!(
            "{} names more than one element: give the whole id (`bateri view --json` has it)",
            words(word, 40)
        ))),
    }
}

/// The element as the answer names it: what it is, its words and its name.
fn describe(reading: &Reading, id: Id) -> String {
    let name = reference(id);
    let called = |kind: &str, label: &str| {
        if label.trim().is_empty() {
            format!("{kind} ({name})")
        } else {
            format!("{kind} \"{}\" ({name})", words(label, 60))
        }
    };
    if let Some(item) = reading.items.iter().find(|item| item.id == Some(id)) {
        return called(&item.role, &item.words);
    }
    if let Some(field) = reading.fields.iter().find(|field| field.id == id) {
        let label = field
            .label
            .as_deref()
            .or(field.hint.as_deref())
            .unwrap_or("");
        let kind = if field.kind == crate::native_view::FieldKind::Secret {
            "secret field"
        } else {
            "field"
        };
        return called(kind, label);
    }
    if let Some(editor) = reading.editors.iter().find(|editor| editor.id == id) {
        return called("editor", editor.label.as_deref().unwrap_or(""));
    }
    if reading.scrolls.iter().any(|scroll| scroll.id == id) {
        return format!("box that scrolls ({name})");
    }
    format!("element {name}")
}

/// Where a press reaches the element, or why it does not.
fn reach(ctx: &Ctx<'_>, id: Id, what: &str) -> Result<Point, Failure> {
    ctx.handle.page().reach(id).map_err(|why| match why {
        Unreachable::Absent => mistake(format!("{what} is not on the page now")),
        Unreachable::Covered => mistake(format!(
            "{what} is covered, or scrolled out of view: scroll the box it is in first"
        )),
    })
}

// ─── The pointer ────────────────────────────────────────────────────────────────────────────────

fn press_and_release(ctx: &Ctx<'_>, at: Point, button: Button, count: u32) {
    let mods = Mods::default();
    ctx.handle.input(
        Input::Press {
            at,
            button,
            mods,
            count,
        },
        mods,
    );
    ctx.handle.input(Input::Release { at, button, mods }, mods);
}

fn click(
    ctx: &Ctx<'_>,
    env: &Env<'_>,
    reading: &Reading,
    word: &str,
    times: u32,
) -> Result<Done, Failure> {
    let id = target(reading, word)?;
    let what = describe(reading, id);
    // A field is the host's: the person's click lands on its own control, not on the page, and
    // what it does is put the keyboard there.
    if reading.fields.iter().any(|field| field.id == id) {
        focus_field(ctx, env.session, id);
        return Done::said(format!("put the keyboard in {what}"));
    }
    let at = reach(ctx, id, &what)?;
    for count in 1..=times {
        press_and_release(ctx, at, Button::Left, count);
    }
    Done::said(format!(
        "{} {what}",
        if times == 2 {
            "double-clicked"
        } else {
            "clicked"
        }
    ))
}

fn menu(ctx: &Ctx<'_>, reading: &Reading, word: &str) -> Result<Done, Failure> {
    let id = target(reading, word)?;
    let what = describe(reading, id);
    let at = reach(ctx, id, &what)?;
    press_and_release(ctx, at, Button::Right, 1);
    Ok(Done {
        said: Some(format!("asked for the menu of {what}")),
        asked_for_menu: true,
        ..Done::default()
    })
}

fn scroll(
    ctx: &Ctx<'_>,
    reading: &Reading,
    word: &str,
    way: Way,
    count: Option<u32>,
) -> Result<Done, Failure> {
    let id = target(reading, word)?;
    let what = describe(reading, id);
    let Some(scrolling) = reading.scrolls.iter().find(|scroll| scroll.id == id) else {
        return Err(mistake(format!(
            "{what} does not scroll: name one of the boxes the reading lists under scrolls"
        )));
    };
    let at = reach(ctx, id, &what)?;
    let screens = count.unwrap_or(1).max(1) as f32;
    // The wheel's points: positive moves the content up, which is going down the page. A box that
    // is gone to its end takes what is left of a very long turn as no more.
    let turn = match way {
        Way::Down => scrolling.rect.h * screens,
        Way::Up => -scrolling.rect.h * screens,
        Way::Bottom => 1.0e7,
        Way::Top => -1.0e7,
    };
    ctx.handle
        .wheel(at, 0.0, turn, Phase::Wheel, false, Mods::default());
    let words = match way {
        Way::Down => format!("scrolled {what} down {screens} screens"),
        Way::Up => format!("scrolled {what} up {screens} screens"),
        Way::Bottom => format!("scrolled {what} to the bottom"),
        Way::Top => format!("scrolled {what} to the top"),
    };
    Done::said(words)
}

// ─── The page's commands and its menu ───────────────────────────────────────────────────────────

fn command(ctx: &Ctx<'_>, reading: &Reading, title: &str) -> Result<Done, Failure> {
    let wanted = title.trim().to_lowercase();
    let named: Vec<&CommandReading> = reading
        .commands
        .iter()
        .filter(|command| command.title.trim().to_lowercase() == wanted)
        .collect();
    match named.as_slice() {
        [] => {
            let titles: Vec<String> = reading
                .commands
                .iter()
                .take(12)
                .map(|command| format!("\"{}\"", words(&command.title, 40)))
                .collect();
            Err(mistake(format!(
                "the page has no command titled \"{}\"; it has: {}",
                words(title, 40),
                if titles.is_empty() {
                    "none".to_owned()
                } else {
                    titles.join(", ")
                }
            )))
        }
        [one] if !one.enabled => Err(mistake(format!(
            "the command \"{}\" is turned off now",
            words(&one.title, 40)
        ))),
        [one] => {
            ctx.handle.page().command(one.id);
            Done::said(format!("ran the command \"{}\"", words(&one.title, 40)))
        }
        _ => Err(mistake(format!(
            "more than one command is titled \"{}\": none is meant",
            words(title, 40)
        ))),
    }
}

fn pick(ctx: &Ctx<'_>, env: &Env<'_>, entry: &str) -> Result<Done, Failure> {
    let Some(menu) = env.session.borrow().menu.clone() else {
        return Err(mistake(
            "no menu is held: ask the view for one first (`bateri view menu REF`); it is let go \
             of at the next action and after half a minute",
        ));
    };
    // The lines an agent sees, numbered from 1; a separator is no line and is counted by the drive.
    let lines: Vec<(usize, String, bool)> = super::menu_lines(&menu);
    let wanted = entry.trim();
    let chosen: Vec<&(usize, String, bool)> = match wanted.parse::<usize>() {
        Ok(number) => number
            .checked_sub(1)
            .and_then(|at| lines.get(at))
            .into_iter()
            .collect(),
        Err(_) => lines
            .iter()
            .filter(|(_, title, _)| title.trim().to_lowercase() == wanted.to_lowercase())
            .collect(),
    };
    let (index, title, enabled) = match chosen.as_slice() {
        [] => {
            return Err(mistake(format!(
                "the menu has no line {}: it has {} lines",
                words(wanted, 40),
                lines.len()
            )));
        }
        [one] => (*one).clone(),
        _ => {
            return Err(mistake(format!(
                "more than one line of the menu is titled {}: pick by number",
                words(wanted, 40)
            )));
        }
    };
    if !enabled {
        return Err(mistake(format!(
            "the line \"{}\" of the menu is turned off",
            words(&title, 40)
        )));
    }
    // The pasteboard is the person's: a Paste line pastes nothing.
    ctx.handle.pick(&ShownMenu::of(&menu), index, &mut || None);
    env.session.borrow_mut().let_go_of_menu();
    Done::said(format!("picked \"{}\" from the menu", words(&title, 40)))
}

// ─── Text ───────────────────────────────────────────────────────────────────────────────────────

/// The page's keyboard goes to the host's field `id`, as a click into it does: the page is told,
/// the system's keyboard is not moved.
fn focus_field(ctx: &Ctx<'_>, session: &Shared, id: Id) {
    ctx.handle.page().focus(Some(id));
    session.borrow_mut().field = Some(id);
}

fn fill(
    ctx: &Ctx<'_>,
    env: &Env<'_>,
    reading: &Reading,
    word: &str,
    text: &str,
) -> Result<Done, Failure> {
    let id = target(reading, word)?;
    let what = describe(reading, id);
    let characters = text.chars().count();
    if reading.fields.iter().any(|field| field.id == id) {
        focus_field(ctx, env.session, id);
        env.fields.put(id, text);
        ctx.handle.page().text(id, text);
        // What a secret field holds is not said back: only how much was put in.
        return Done::said(format!("filled {what} with {characters} characters"));
    }
    if reading.editors.iter().any(|editor| editor.id == id) {
        {
            let mut page = ctx.handle.page();
            let mut editors = page.editors();
            editors.focus(Some(id));
            editors.command(EditCommand::SelectAll);
            if text.is_empty() {
                editors.command(EditCommand::DeleteBackward);
            } else {
                editors.type_text(text);
            }
        }
        env.session.borrow_mut().field = None;
        return Done::said(format!("filled {what} with {characters} characters"));
    }
    Err(mistake(format!(
        "{what} is neither a field nor an editor: fill takes one of them"
    )))
}

/// Where the keyboard is in the page: an editor, a field of the host, or neither.
enum Where {
    Editor,
    Field(Id),
    Nowhere,
}

fn keyboard_in(ctx: &Ctx<'_>, env: &Env<'_>) -> Where {
    if ctx.handle.page().focused_editor().is_some() {
        return Where::Editor;
    }
    match env.fields.keyboard_in().or(env.session.borrow().field) {
        Some(field) => Where::Field(field),
        None => Where::Nowhere,
    }
}

/// Text typed into the host's field `id`: put after what it holds, and the page told what it holds
/// now. Never said back: the field may be a password's.
fn type_into_field(ctx: &Ctx<'_>, env: &Env<'_>, id: Id, text: &str) -> bool {
    match env.fields.append(id, text) {
        Some(now) => {
            ctx.handle.page().text(id, &now);
            true
        }
        None => false,
    }
}

fn typed(ctx: &Ctx<'_>, env: &Env<'_>, text: &str) -> Result<Done, Failure> {
    let characters = text.chars().count();
    match keyboard_in(ctx, env) {
        Where::Editor => {
            ctx.handle.page().editors().type_text(text);
            Done::said(format!("typed {characters} characters into the editor"))
        }
        Where::Field(id) => {
            if type_into_field(ctx, env, id, text) {
                Done::said(format!(
                    "typed {characters} characters into the field {}",
                    reference(id)
                ))
            } else {
                Err(mistake(
                    "the field the keyboard is in is not on the page now: click or fill one first",
                ))
            }
        }
        Where::Nowhere => Err(mistake(
            "the keyboard is in no editor or field of the page: click or fill one first",
        )),
    }
}

fn text(ctx: &Ctx<'_>, env: &Env<'_>, reading: &Reading, word: &str) -> Result<Done, Failure> {
    let id = target(reading, word)?;
    let what = describe(reading, id);
    if let Some(field) = reading.fields.iter().find(|field| field.id == id) {
        if field.kind == crate::native_view::FieldKind::Secret {
            return Err(mistake(format!(
                "{what} is a secret field: its text is not read"
            )));
        }
        let held = env
            .fields
            .text(id)
            .ok_or_else(|| mistake(format!("the text of {what} is not known yet: ask again")))?;
        return Ok(Done {
            text: Some(held),
            ..Done::default()
        });
    }
    if reading.editors.iter().any(|editor| editor.id == id) {
        let held = ctx
            .handle
            .editors()
            .contents(id)
            .map(|(text, _)| text)
            .ok_or_else(|| mistake(format!("{what} holds no document yet")))?;
        return Ok(Done {
            text: Some(held),
            ..Done::default()
        });
    }
    Err(mistake(format!(
        "{what} is neither a field nor an editor: it has no text to read"
    )))
}

// ─── Keys ───────────────────────────────────────────────────────────────────────────────────────

/// The convention the keys are written for: the system's.
fn convention() -> KeyConvention {
    if cfg!(target_os = "macos") {
        KeyConvention::Mac
    } else {
        KeyConvention::Linux
    }
}

fn press(ctx: &Ctx<'_>, env: &Env<'_>, keys: &[String]) -> Result<Done, Failure> {
    // All of them are read before any is pressed: a mistake in the last is not found out after
    // the first has done what it does.
    let mut shortcuts = Vec::new();
    for keys in keys {
        shortcuts.push(Shortcut::parse_for(keys, convention()).ok_or_else(|| {
            mistake(format!(
                "{} is not a key: write them as cmd+enter, f5, escape or a letter",
                words(keys, 40)
            ))
        })?);
    }
    for Shortcut { key, mods } in shortcuts {
        press_one(ctx, env, key, mods);
    }
    Done::said(format!(
        "pressed {}",
        keys.iter()
            .map(|key| words(key, 30))
            .collect::<Vec<_>>()
            .join(" ")
    ))
}

/// A key, as the page is told it: to the editor that has the keyboard first (the page's command for
/// the key before the editor's own), or to the page. A letter for a field of the host is its text:
/// the host's own control would have taken it.
fn press_one(ctx: &Ctx<'_>, env: &Env<'_>, key: Key, mods: Mods) {
    if let (Key::Character(letter), false) = (key, mods.any_command())
        && let Where::Field(id) = keyboard_in(ctx, env)
    {
        type_into_field(ctx, env, id, &letter.to_string());
        return;
    }
    let route = ctx.handle.page().key(key, mods);
    match route {
        Route::Type => {
            if let Key::Character(letter) = key {
                ctx.handle.page().editors().type_text(&letter.to_string());
            }
        }
        Route::Copy(text) | Route::Cut(text) => {
            env.session.borrow_mut().effects.copies.push(text);
        }
        Route::Paste => {
            env.session
                .borrow_mut()
                .effects
                .notes
                .push("paste is not done for an agent: the pasteboard is the person's".to_owned());
        }
        Route::NotEditor | Route::Command(_) | Route::Page | Route::Done => {}
    }
}
