//! A page of the kit as a pane uses it, end to end through what the pane calls: the keys and where
//! they go, an input method, the scroll bars and the wheel, a reply done in the pane's order (the
//! pointer's shape, the fields, the keyboard, the pasteboard, a link, a menu, what the page said to
//! its program, its end), the clock of a pane that is not seen, and the outline the pane's fields and
//! its screen reader are made from. Each scenario is written once against [`PaneRig`] and checked as
//! a transcript of what the pane would do and what the page heard, so that what a pane sees of a
//! page does not depend on who drives it.
//!
//! The rig is the pane in all but AppKit: it does what the pane does around the drive (a press ends
//! what an input method composes, a key goes to the editor with the keyboard first, a key that goes
//! up while an editor has the keyboard is nobody's) and writes down, in the order the pane does it,
//! everything the pane would do. The page is a script, measured by the tests' even measure, so no
//! GPU is needed.

use std::fmt::Debug;

use super::{
    Accessible, Bars, Button, Command, Cursor, Fill, Id, Input, Key, Keyboard, Menu, MenuEntry,
    Mods, Phase, Point, Scrolling,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::agent::message::{Format, Reply};
use crate::agent::view::{Action, Env, Fields, Pane, Run, Session, Shared, Shown, Step, View};

pub(super) use super::page::tests::PaneRig;
use super::page::{MenuUp, ShownMenu};

/// What the pane did and what the page heard, a line each, in order.
#[derive(Default)]
pub(super) struct Transcript {
    lines: Vec<String>,
    /// The menu the pane has up.
    pub(super) up: MenuUp,
}

impl Transcript {
    pub(super) fn say(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
    }

    /// Takes the lines written so far.
    pub(super) fn take(&mut self) -> Vec<String> {
        std::mem::take(&mut self.lines)
    }

    // What the pane does with what a page said, in the pane's order: what the page said to its
    // program, then each reply (its wish to close, its frame, the pointer's shape for the pointer's
    // answer alone, the fields' texts, the keyboard, the pasteboard, the link, the menu last), the
    // images' words, the end, and the frame or the clock it wants.

    pub(super) fn send(&mut self, message: &str) {
        self.say(format!("send {message}"));
    }
    pub(super) fn close(&mut self) {
        self.say("close");
    }
    pub(super) fn redraw(&mut self) {
        self.say("redraw");
    }
    pub(super) fn cursor(&mut self, cursor: Cursor) {
        self.say(format!("cursor {cursor:?}"));
    }
    pub(super) fn fill(&mut self, field: Id, fill: &Fill) {
        let mut line = match &fill.text {
            Some(text) => format!("fill {field:?} {text:?}"),
            None => format!("fill {field:?} (text kept)"),
        };
        if let Some(select) = &fill.select {
            line.push_str(&format!(" select {}..{}", select.start, select.end));
        }
        self.say(line);
    }
    pub(super) fn keyboard(&mut self, to: Keyboard) {
        self.say(format!("keyboard {to:?}"));
    }
    pub(super) fn copy(&mut self, text: &str, by_person: bool) {
        if by_person {
            self.say(format!("copy {text:?}"));
        } else {
            self.say(format!("copy {text:?} (not the person's act)"));
        }
    }
    pub(super) fn link(&mut self, url: &str) {
        self.say(format!("link {url}"));
    }
    pub(super) fn menu(&mut self, menu: &Menu) {
        if self.up.raise(menu) {
            self.say(format!("menu {menu:?}"));
        } else {
            self.say("menu not shown: one is up");
        }
    }
    pub(super) fn diagnostic(&mut self, words: &str) {
        self.say(format!("diagnostic {words}"));
    }
    pub(super) fn ended(&mut self, why: &str) {
        self.say(format!("ended {why}"));
    }
    pub(super) fn rearm(&mut self) {
        self.say("rearm");
    }
    /// The outline the pane's fields and its screen reader are made from, as told: what is not
    /// the line of what the page heard, which changes with every scenario's step.
    pub(super) fn outline(
        &mut self,
        fields: &[impl Debug],
        accessible: &[Accessible],
        tips: &[impl Debug],
        commands: &[Command],
    ) {
        let accessible: Vec<_> = accessible
            .iter()
            .filter(|item| !item.words.starts_with("said: "))
            .collect();
        self.say(format!(
            "outline fields {fields:?} accessible {accessible:?} tips {tips:?} commands {commands:?}"
        ));
    }
}

/// The page every scenario uses: a command (⌘⏎), a text field and a button with a tooltip in a row
/// 40 points high, an editor 100 high below it, a list of 30 rows in a box 100 high that scrolls,
/// and a line of what the page heard. Its keys answer with each part of a reply.
const PAGE: &str = r#"local said = {}
local function note(words) said[#said + 1] = words end
local function at(event) return string.format("%g,%g", event.x, event.y) end
return {
    render = function()
        local list = pencere.div():id("list"):h(100):w(200):scroll()
        for n = 1, 30 do list:child(pencere.div():h(20):child(pencere.text("row " .. n))) end
        return pencere.div():size_full()
            :command("run", "Run", "cmd+enter")
            :child(pencere.div():h(40):row()
                :child(pencere.div():text_field("q"):w(200):h(20):child(pencere.text("Filter")))
                :child(pencere.div():id("help"):w(40):h(20):tip("Help", "?"):label("Help"):role("button")))
            :child(pencere.editor("e", { gutter = false }):h(100):w(400))
            :child(list)
            :child(pencere.text("said: " .. table.concat(said, "|")))
    end,
    handle = function(event)
        local t = event.type
        if t == "key" then
            local k = event.key
            note("key " .. k)
            if k == "l" then
                return { redraw = true, keyboard = { field = "e" }, editor = { e = { load = "hello\nworld" } } }
            elseif k == "c" then
                return { redraw = true, copy = "copied words", cursor = "grab" }
            elseif k == "o" then
                return { redraw = true, link = "https://example.com/a?b=c" }
            elseif k == "m" then
                return { redraw = true, menu = { at = { x = 10, y = 20 }, entries = {
                    { title = "One", keys = "1", enabled = true }, "-", { title = "Two", enabled = false } } } }
            elseif k == "f" then
                return { redraw = true, keyboard = { field = "q" }, fill = { q = "filled" } }
            elseif k == "p" then
                return { redraw = true, keyboard = "page" }
            elseif k == "w" then
                return { wake = 0.5 }
            elseif k == "s" then
                pencere.send({ said = "hi" })
                return true
            elseif k == "x" then
                return { redraw = true, close = true }
            elseif k == "z" then
                error("the page gave up")
            end
            return true
        elseif t == "key_up" then
            note("up " .. event.key)
        elseif t == "move" then
            return { cursor = "pointer" }
        elseif t == "press" then
            note("press " .. (event.place and (event.place.editor .. "@" .. event.place.at) or at(event)))
            return { redraw = true, cursor = "text" }
        elseif t == "release" then
            note("release " .. at(event))
        elseif t == "wheel" then
            note("wheel " .. event.dy)
        elseif t == "scrolled" then
            note("scrolled " .. event.boxes[1].id .. " " .. event.boxes[1].y)
        elseif t == "text" then
            note("text " .. event.field .. " " .. event.text)
        elseif t == "focus" then
            note("focus " .. tostring(event.field))
        elseif t == "menu" then
            note("menu " .. event.index)
        elseif t == "command" then
            note("command " .. event.id)
        elseif t == "files" then
            note("files " .. at(event) .. " " .. table.concat(event.paths, ","))
        elseif t == "wake" then
            note("wake")
        elseif t == "edit" then
            note("edit " .. event.origin .. " " .. pencere.layout.editor_text("e", 1, pencere.layout.editor_length("e") + 1))
        end
        return true
    end,
}"#;

fn plain(key: char) -> (Key, Mods) {
    (Key::Character(key), Mods::default())
}

fn command(key: Key) -> (Key, Mods) {
    (
        key,
        Mods {
            command: true,
            ..Mods::default()
        },
    )
}

/// How the pane has the page's boxes scroll: bars always drawn (so that no bar is on its way out
/// when a scenario looks), a track that takes the box where it is clicked.
const SCROLLING: Scrolling = Scrolling {
    bars: Bars::Always,
    elastic: false,
    track_jumps: true,
};

/// A page laid out at rest, scrolling as the pane has it scroll.
fn opened() -> PaneRig {
    let mut rig = PaneRig::open(PAGE);
    rig.set_scrolling(SCROLLING);
    rig.frame(0.0);
    rig.settle(0.0);
    rig
}

/// The transcript is the one expected; where it is not, the one there is, written as the list of
/// lines it is checked against.
#[track_caller]
fn expect(rig: &mut PaneRig, expected: &[&str]) {
    let lines = rig.lines();
    if lines != expected {
        let written: Vec<String> = lines
            .iter()
            .map(|line| format!("        {line:?},"))
            .collect();
        panic!("the transcript is\n{}", written.join("\n"));
    }
}

/// A key goes to the page's command for it, to the editor with the keyboard (the page's command
/// first, the editor's own, the text to the system), or to the page; a key that goes up is the
/// page's only where no editor has the keyboard.
#[test]
fn a_key_goes_to_the_command_the_editor_with_the_keyboard_or_the_page() {
    let mut rig = opened();
    let (key, mods) = command(Key::Enter);
    rig.key(key, mods);
    let (key, mods) = plain('g');
    rig.key(key, mods);
    rig.key_up(key, mods);
    rig.settle(0.1);
    // The editor takes the keyboard.
    let (key, mods) = plain('l');
    rig.key(key, mods);
    rig.settle(0.2);
    let focused = rig.editors().and_then(|e| e.focused());
    rig.note(format!("focused {focused:?}"));
    for (key, mods) in [
        plain('a'),
        command(Key::Enter),
        (Key::Right, Mods::default()),
        command(Key::Character('a')),
        command(Key::Character('c')),
        command(Key::Character('v')),
        (Key::Escape, Mods::default()),
    ] {
        rig.key(key, mods);
        rig.key_up(key, mods);
    }
    rig.settle(0.3);
    let said = rig.said();
    rig.note(format!("said {said}"));
    expect(
        &mut rig,
        &[
            "outline fields [] accessible [] tips [] commands []",
            "outline fields [] accessible [Accessible { role: Text, id: None, rect: Rect { x: 344.0, y: 308.0, w: 112.0, h: 16.0 }, words: \"Starting the page…\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [] commands []",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, text: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "redraw",
            "redraw",
            "redraw",
            "route Enter NotEditor",
            "route Character('g') NotEditor",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, text: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "route Character('l') NotEditor",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, text: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "keyboard Field(Id(12638182802509129152))",
            "redraw",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, text: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "focused Some(Id(12638182802509129152))",
            "route Character('a') Type",
            "route Enter Command(Id(9922177756001819210))",
            "route Right Done",
            "route Character('a') Done",
            "route Character('c') Copy(\"hello\\nworld\")",
            "route Character('v') Paste",
            "route Escape Page",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, text: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "said command run|key g|up g|key l|edit load hello\nworld|command run|key escape",
        ],
    );
}

/// What an input method composes is in the editor with the keyboard, counted in UTF-16; a press
/// ends it (what it composed stays, and the pane has the system forget it), and text the system
/// types in place of a range replaces it.
#[test]
fn an_input_method_composes_in_the_editor_and_a_press_ends_it() {
    let mut rig = opened();
    let (key, mods) = plain('l');
    rig.key(key, mods);
    rig.settle(0.1);
    let state = |rig: &mut PaneRig| {
        let editors = rig.editors().expect("the page has editors");
        let state = format!(
            "marked {:?} selected {:?} length {:?} text {:?} rect {:?} contents {:?}",
            editors.marked(),
            editors.selected(),
            editors.length(),
            editors.text(0, 7),
            editors.rect(0),
            editors.contents(Id::named("e")),
        );
        rig.note(state);
    };
    rig.editors()
        .expect("the page has editors")
        .compose("\u{304b}\u{3099}", (2, 0), None);
    state(&mut rig);
    // A press in the editor's room, past the end of its first line.
    let at = Point::new(300.0, 32.0 + 40.0 + 5.0);
    rig.input(Input::Press {
        at,
        button: Button::Left,
        mods: Mods::default(),
        count: 1,
    });
    rig.input(Input::Release {
        at,
        button: Button::Left,
        mods: Mods::default(),
    });
    rig.settle(0.2);
    state(&mut rig);
    // The system types in place of a range of what is there: composed, then done.
    let editors = rig.editors().expect("the page has editors");
    editors.compose("HE", (2, 0), Some((0, 2)));
    editors.unmark();
    editors.type_text("!");
    let index = editors.index_at(Point::new(4.0, 32.0 + 40.0 + 5.0));
    rig.note(format!("index {index:?}"));
    rig.settle(0.3);
    state(&mut rig);
    let said = rig.said();
    rig.note(format!("said {said}"));
    expect(
        &mut rig,
        &[
            "outline fields [] accessible [] tips [] commands []",
            "outline fields [] accessible [Accessible { role: Text, id: None, rect: Rect { x: 344.0, y: 308.0, w: 112.0, h: 16.0 }, words: \"Starting the page…\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [] commands []",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, text: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "redraw",
            "redraw",
            "redraw",
            "route Character('l') NotEditor",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, text: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "keyboard Field(Id(12638182802509129152))",
            "redraw",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, text: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "marked Some((0, 2)) selected Some((2, 0)) length Some(13) text Some(\"か\\u{3099}hello\") rect Some(Rect { x: 4.0, y: 76.0, w: 2.0, h: 20.0 }) contents Some((\"か\\u{3099}hello\\nworld\", (2, 0)))",
            "discard marked text",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "cursor Text",
            "redraw",
            "cursor Text",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, text: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "marked None selected Some((7, 0)) length Some(13) text Some(\"か\\u{3099}hello\") rect Some(Rect { x: 4.0, y: 76.0, w: 2.0, h: 20.0 }) contents Some((\"か\\u{3099}hello\\nworld\", (7, 0)))",
            "index Some(0)",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, text: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "marked None selected Some((3, 0)) length Some(14) text Some(\"HE!hell\") rect Some(Rect { x: 4.0, y: 76.0, w: 2.0, h: 20.0 }) contents Some((\"HE!hello\\nworld\", (3, 0)))",
            "said key l|edit load hello\nworld|edit compose か\u{3099}hello\nworld|press e@12|release 300,45|edit compose HEhello\nworld|edit person HE!hello\nworld",
        ],
    );
}

/// The bars of a box that scrolls are drawn as the pane's own are, and answer the pointer before
/// the page; the wheel scrolls the box under it, the page is told where the box went, and a turn no
/// box takes is the page's.
#[test]
fn the_bars_answer_the_pointer_before_the_page_and_the_wheel_scrolls_the_box_under_it() {
    let mut rig = opened();
    // The list is 140 points down the page, below the strip's 32.
    let list = |x: f32, y: f32| Point::new(x, 32.0 + 140.0 + y);
    rig.wheel(list(50.0, 50.0), 0.0, 60.0, Phase::Wheel);
    rig.settle(0.2);
    rig.wheel(list(50.0, 300.0), 0.0, 30.0, Phase::Wheel);
    rig.settle(0.3);
    // A press in the bar's track, at the list's right edge: the bar's, and the box goes there.
    rig.input(Input::Press {
        at: list(198.0, 90.0),
        button: Button::Left,
        mods: Mods::default(),
        count: 1,
    });
    rig.input(Input::Release {
        at: list(198.0, 90.0),
        button: Button::Left,
        mods: Mods::default(),
    });
    rig.settle(0.4);
    // A press beside it is the page's.
    rig.input(Input::Press {
        at: list(100.0, 50.0),
        button: Button::Left,
        mods: Mods::default(),
        count: 1,
    });
    rig.settle(0.5);
    let said = rig.said();
    rig.note(format!("said {said}"));
    expect(
        &mut rig,
        &[
            "outline fields [] accessible [] tips [] commands []",
            "outline fields [] accessible [Accessible { role: Text, id: None, rect: Rect { x: 344.0, y: 308.0, w: 112.0, h: 16.0 }, words: \"Starting the page…\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [] commands []",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, text: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, text: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 159.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 175.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 190.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 206.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 221.0, w: 200.0, h: 16.0 }, words: \"row 8\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 237.0, w: 200.0, h: 16.0 }, words: \"row 9\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 253.0, w: 200.0, h: 15.0 }, words: \"row 10\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 268.0, w: 200.0, h: 16.0 }, words: \"row 11\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "redraw",
            "cursor Arrow",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, text: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 159.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 175.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 190.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 206.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 221.0, w: 200.0, h: 16.0 }, words: \"row 8\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 237.0, w: 200.0, h: 16.0 }, words: \"row 9\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 253.0, w: 200.0, h: 15.0 }, words: \"row 10\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 268.0, w: 200.0, h: 16.0 }, words: \"row 11\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, text: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 162.0, w: 200.0, h: 16.0 }, words: \"row 24\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 178.0, w: 200.0, h: 16.0 }, words: \"row 25\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 194.0, w: 200.0, h: 15.0 }, words: \"row 26\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 209.0, w: 200.0, h: 16.0 }, words: \"row 27\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 225.0, w: 200.0, h: 16.0 }, words: \"row 28\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 241.0, w: 200.0, h: 15.0 }, words: \"row 29\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 256.0, w: 200.0, h: 16.0 }, words: \"row 30\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "redraw",
            "cursor Text",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, text: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 162.0, w: 200.0, h: 16.0 }, words: \"row 24\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 178.0, w: 200.0, h: 16.0 }, words: \"row 25\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 194.0, w: 200.0, h: 15.0 }, words: \"row 26\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 209.0, w: 200.0, h: 16.0 }, words: \"row 27\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 225.0, w: 200.0, h: 16.0 }, words: \"row 28\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 241.0, w: 200.0, h: 15.0 }, words: \"row 29\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 256.0, w: 200.0, h: 16.0 }, words: \"row 30\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "said scrolled list 60|wheel 30|scrolled list 369|press 100,190",
        ],
    );
}

/// What a reply asks is done in the pane's order, the pointer's shape only for an answer to the
/// pointer, and what the person does back (a menu's entry, a field's text, the keyboard put in a
/// field, a command from the pane's list, files dropped) is told the page; a page that goes wrong
/// says why, once.
#[test]
fn a_reply_is_done_in_the_panes_order_and_what_the_person_does_back_is_told() {
    let mut rig = opened();
    for key in ['c', 'o', 'm', 'f', 'p', 's'] {
        let (key, mods) = plain(key);
        rig.key(key, mods);
        rig.settle(0.1);
    }
    rig.input(Input::Move {
        at: Point::new(300.0, 32.0 + 300.0),
        mods: Mods::default(),
    });
    rig.settle(0.2);
    // The first line: the second is a rule, and a pick of a rule is not told the page.
    rig.pick(0);
    rig.text(Id::named("q"), "abc");
    rig.focus(Some(Id::named("q")));
    rig.focus(None);
    rig.command(Id::named("run"));
    rig.files(
        Point::new(20.0, 32.0 + 10.0),
        vec!["/tmp/a b".to_owned(), "/tmp/c".to_owned()],
    );
    rig.settle(0.3);
    let said = rig.said();
    rig.note(format!("said {said}"));
    let (key, mods) = plain('x');
    rig.key(key, mods);
    rig.settle(0.4);
    let (key, mods) = plain('z');
    rig.key(key, mods);
    rig.settle(0.5);
    expect(
        &mut rig,
        &[
            "outline fields [] accessible [] tips [] commands []",
            "outline fields [] accessible [Accessible { role: Text, id: None, rect: Rect { x: 344.0, y: 308.0, w: 112.0, h: 16.0 }, words: \"Starting the page…\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [] commands []",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, text: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "redraw",
            "redraw",
            "redraw",
            "route Character('c') NotEditor",
            "redraw",
            // A plain key is the page's word for a copy, not the person's: only ⌘C and ⌘X are.
            "copy \"copied words\" (not the person's act)",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, text: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "route Character('o') NotEditor",
            "redraw",
            "link https://example.com/a?b=c",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, text: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "route Character('m') NotEditor",
            "redraw",
            "menu Menu { at: Point { x: 10.0, y: 52.0 }, entries: [Item { title: \"One\", keys: Some(Shortcut { key: Character('1'), mods: Mods { shift: false, control: false, alt: false, command: false } }), enabled: true }, Separator, Item { title: \"Two\", keys: None, enabled: false }], serial: 1 }",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, text: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "route Character('f') NotEditor",
            "redraw",
            "fill Id(12638204792741693372) \"filled\"",
            "keyboard Field(Id(12638204792741693372))",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, text: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "route Character('p') NotEditor",
            "redraw",
            "keyboard Page",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, text: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "route Character('s') NotEditor",
            "send {\"said\":\"hi\"}",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, text: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "cursor Pointer",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, text: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "said key c|key o|key m|key f|key p|key s|menu 1|text q abc|focus q|focus nil|command run|files 20,10 /tmp/a b,/tmp/c",
            "route Character('x') NotEditor",
            "close",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, text: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "route Character('z') NotEditor",
            "ended page.lua:43: the page gave up",
        ],
    );
}

/// A wake a reply asks for arms the pane's clock with no frame. While the pane is seen the frame at
/// its time wakes the page — the kit's first, which places what was asked outside a wake, then the
/// page's own — and while it is not the pane's own clock does, a second apart, losing none.
#[test]
fn a_wake_is_given_by_the_frames_while_the_pane_is_seen_and_by_its_clock_while_it_is_not() {
    let round = |at: Option<f64>| at.map(|at| (at * 100.0).round() / 100.0);
    let mut rig = opened();
    let (key, mods) = plain('w');
    rig.key(key, mods);
    rig.settle(0.0);
    for now in [0.1, 0.4, 0.7] {
        // The time the kit asks to place what was asked is the module's reading of the clock:
        // due by the next frame, not a time to the hundredth.
        let asked = match rig.wake_at() {
            Some(at) if at <= now => "due".to_owned(),
            at => format!("{:?}", round(at)),
        };
        rig.frame(now);
        rig.settle(now);
        rig.note(format!("frame at {now}: asked {asked}"));
    }
    let said = rig.said();
    rig.note(format!("said {said}"));
    // Not seen: the pane's clock, before the time and at it, and a second after the last wake.
    rig.key(key, mods);
    rig.settle_unseen();
    for now in [0.8, 1.7, 1.8, 2.2, 2.7, 2.8] {
        let next = round(rig.wake_unseen(now));
        rig.settle_unseen();
        rig.note(format!("unseen at {now}: next {next:?}"));
    }
    rig.frame(2.9);
    rig.settle(2.9);
    let said = rig.said();
    rig.note(format!("said {said}"));
    expect(
        &mut rig,
        &[
            "outline fields [] accessible [] tips [] commands []",
            "outline fields [] accessible [Accessible { role: Text, id: None, rect: Rect { x: 344.0, y: 308.0, w: 112.0, h: 16.0 }, words: \"Starting the page…\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [] commands []",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, text: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "redraw",
            "redraw",
            "redraw",
            "route Character('w') NotEditor",
            "rearm",
            "rearm",
            "frame at 0.1: asked due",
            "frame at 0.4: asked Some(0.6)",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, text: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "frame at 0.7: asked due",
            "said key w|wake",
            "route Character('w') NotEditor",
            "rearm",
            "unseen at 0.8: next Some(1.7)",
            "rearm",
            "unseen at 1.7: next None",
            "unseen at 1.8: next Some(2.7)",
            "unseen at 2.2: next Some(2.7)",
            "redraw",
            "unseen at 2.7: next None",
            "unseen at 2.8: next None",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, text: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "said key w|wake|key w|wake",
        ],
    );
}

/// The fields, what a screen reader says, the tooltips and the commands are told in the pane's
/// points when they change: below the strip, and as large as the terminal's text makes the page; an
/// editor's caret answers in the pane's points too, and is drawn only while the pane's keyboard is
/// in the page.
#[test]
fn the_outline_is_told_in_the_panes_points_as_the_page_is_drawn_larger() {
    let mut rig = opened();
    let (key, mods) = plain('l');
    rig.key(key, mods);
    rig.settle(0.1);
    let caret = rig.editors().and_then(|e| e.rect(0));
    rig.note(format!("caret {caret:?} boxes {}", rig.quads()));
    rig.set_zoom(2.0);
    rig.settle(0.2);
    let caret = rig.editors().and_then(|e| e.rect(0));
    rig.note(format!("caret {caret:?} boxes {}", rig.quads()));
    rig.set_active(false);
    rig.settle(0.3);
    rig.note(format!("away: boxes {}", rig.quads()));
    rig.set_active(true);
    rig.settle(0.4);
    rig.note(format!("back: boxes {}", rig.quads()));
    // Drawn twice as large: a press beside the editor (the keyboard back to the page), a menu, the
    // wheel over the list and below it, each in the pane's points and told the page in its own.
    rig.input(Input::Press {
        at: Point::new(500.0, 550.0),
        button: Button::Left,
        mods: Mods::default(),
        count: 1,
    });
    rig.settle(0.5);
    let (key, mods) = plain('m');
    rig.key(key, mods);
    rig.settle(0.6);
    rig.wheel(Point::new(100.0, 400.0), 0.0, 60.0, Phase::Wheel);
    rig.settle(0.7);
    rig.wheel(Point::new(100.0, 560.0), 0.0, 20.0, Phase::Wheel);
    rig.settle(0.8);
    let said = rig.said();
    rig.note(format!("said {said}"));
    expect(
        &mut rig,
        &[
            "outline fields [] accessible [] tips [] commands []",
            "outline fields [] accessible [Accessible { role: Text, id: None, rect: Rect { x: 344.0, y: 308.0, w: 112.0, h: 16.0 }, words: \"Starting the page…\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [] commands []",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, text: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "redraw",
            "redraw",
            "redraw",
            "route Character('l') NotEditor",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, text: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "keyboard Field(Id(12638182802509129152))",
            "redraw",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, text: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "caret Some(Rect { x: 4.0, y: 76.0, w: 2.0, h: 20.0 }) boxes 4",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, text: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 400.0, y: 32.0, w: 80.0, h: 40.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 112.0, w: 800.0, h: 200.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 312.0, w: 400.0, h: 31.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 343.0, w: 400.0, h: 32.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 375.0, w: 400.0, h: 31.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 406.0, w: 400.0, h: 31.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 437.0, w: 400.0, h: 31.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 468.0, w: 400.0, h: 32.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 500.0, w: 400.0, h: 31.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 400.0, y: 32.0, w: 80.0, h: 40.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "caret Some(Rect { x: 8.0, y: 120.0, w: 4.0, h: 40.0 }) boxes 4",
            "redraw",
            "away: boxes 3",
            "redraw",
            "back: boxes 4",
            "redraw",
            "redraw",
            "cursor Text",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, text: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 400.0, y: 32.0, w: 80.0, h: 40.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 112.0, w: 800.0, h: 200.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 312.0, w: 400.0, h: 31.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 343.0, w: 400.0, h: 32.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 375.0, w: 400.0, h: 31.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 406.0, w: 400.0, h: 31.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 437.0, w: 400.0, h: 31.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 468.0, w: 400.0, h: 32.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 500.0, w: 400.0, h: 31.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 400.0, y: 32.0, w: 80.0, h: 40.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "route Character('m') NotEditor",
            "redraw",
            "menu Menu { at: Point { x: 20.0, y: 72.0 }, entries: [Item { title: \"One\", keys: Some(Shortcut { key: Character('1'), mods: Mods { shift: false, control: false, alt: false, command: false } }), enabled: true }, Separator, Item { title: \"Two\", keys: None, enabled: false }], serial: 1 }",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, text: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 400.0, y: 32.0, w: 80.0, h: 40.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 112.0, w: 800.0, h: 200.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 312.0, w: 400.0, h: 31.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 343.0, w: 400.0, h: 32.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 375.0, w: 400.0, h: 31.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 406.0, w: 400.0, h: 31.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 437.0, w: 400.0, h: 31.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 468.0, w: 400.0, h: 32.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 500.0, w: 400.0, h: 31.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 400.0, y: 32.0, w: 80.0, h: 40.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, text: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 400.0, y: 32.0, w: 80.0, h: 40.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 112.0, w: 800.0, h: 200.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 283.0, w: 400.0, h: 32.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 315.0, w: 400.0, h: 31.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 346.0, w: 400.0, h: 31.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 377.0, w: 400.0, h: 31.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 408.0, w: 400.0, h: 32.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 440.0, w: 400.0, h: 31.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 471.0, w: 400.0, h: 31.0 }, words: \"row 8\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 502.0, w: 400.0, h: 31.0 }, words: \"row 9\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 400.0, y: 32.0, w: 80.0, h: 40.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "redraw",
            "cursor Arrow",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, padding_box: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, text: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, hint: Some(\"Filter\"), label: None, covered: false }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 400.0, y: 32.0, w: 80.0, h: 40.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: Some(\"?\"), role_description: None, actions: [] } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 112.0, w: 800.0, h: 200.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 283.0, w: 400.0, h: 32.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 315.0, w: 400.0, h: 31.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 346.0, w: 400.0, h: 31.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 377.0, w: 400.0, h: 31.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 408.0, w: 400.0, h: 32.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 440.0, w: 400.0, h: 31.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 471.0, w: 400.0, h: 31.0 }, words: \"row 8\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 502.0, w: 400.0, h: 31.0 }, words: \"row 9\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None, mixed: false, level: None, setsize: None, posinset: None, rowcount: None, colcount: None, rowindex: None, colindex: None, sort: None }, pressable: false, parent: None, detail: Detail { description: None, keys: None, role_description: None, actions: [] } }] tips [Tip { rect: Rect { x: 400.0, y: 32.0, w: 80.0, h: 40.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "said key l|edit load hello\nworld|focus nil|press 250,259|key m|scrolled list 30|wheel 10",
        ],
    );
}

/// A page that opens a find bar when ⌥F… is pressed: the reply lays the bar out, fills it with a
/// text and selects part of it, and gives it the keyboard, all at once.
const FIND: &str = r#"local open = false
return {
    render = function()
        local root = pencere.div():size_full():child(pencere.text("page"))
        if open then
            root:child(pencere.div():text_field("find"):w(200):h(20):child(pencere.text("Find")))
        end
        return root
    end,
    handle = function(event)
        if event.type == "key" and event.key == "f" then
            open = true
            return {
                redraw = true,
                keyboard = { field = "find" },
                fill = { find = { text = "needle", select = { from = 2, to = 5 } } },
            }
        end
        return true
    end,
}"#;

/// A field a reply opens has no host's field until the frame that lays it out has been drawn and its
/// outline told, so what the reply asks of it (its text and what is selected in it, the keyboard) is
/// done after that frame's outline, in the order asked: the find bar of a page opens with its text
/// and the keyboard, as the pane's host gets them, and not before the field is there (the reply's
/// `fill` and `keyboard` came before the outline, and the field was empty and without focus).
#[test]
fn a_field_opened_by_a_reply_gets_its_text_and_the_keyboard_once_the_frame_that_lays_it_out_is_applied()
 {
    let mut rig = PaneRig::open(FIND);
    rig.frame(0.0);
    rig.settle(0.0);
    rig.lines();
    let (key, mods) = plain('f');
    rig.key(key, mods);
    rig.settle(0.1);
    let lines = rig.lines();
    let field = format!("{:?}", Id::named("find"));
    let at = |start: &str| {
        lines
            .iter()
            .position(|line| line.starts_with(start))
            .unwrap_or_else(|| panic!("no `{start}` in {lines:#?}"))
    };
    let outline = at(&format!("outline fields [Field {{ id: {field}"));
    let fill = at("fill ");
    let keyboard = at("keyboard ");
    assert_eq!(
        lines[fill],
        format!("fill {field} \"needle\" select 1..4"),
        "{lines:#?}"
    );
    assert_eq!(lines[keyboard], format!("keyboard Field({field})"));
    assert!(
        outline < fill && fill < keyboard,
        "the field is there before it is filled, and filled before it has the keyboard: {lines:#?}"
    );
    // And once, not again with every frame.
    rig.frame(0.2);
    rig.settle(0.2);
    let later = rig.lines();
    assert!(
        later
            .iter()
            .all(|line| !line.starts_with("fill ") && !line.starts_with("keyboard ")),
        "{later:#?}"
    );
}

/// A page with an editor that makes a link of the word under the pointer while ⌘ is held (`target`,
/// a look that is `held` and `over`, the hand then) and hears of the pointer's rest on a word
/// (`hover`, after half a second). What it heard is the line of `said: `.
const WORDS: &str = r#"local text = "alpha beta\ngamma delta"
local told = {}
local version = 0
return {
    render = function()
        return pencere.div():size_full()
            :child(pencere.editor("code", {
                gutter = false,
                events = { "target", "hover" },
                hover = { delay = 0.5 },
                gestures = { ["mod+press"] = "target" },
            }):h(100):w(400))
            :child(pencere.text("said: " .. table.concat(told, " ")))
    end,
    handle = function(event)
        if event.type == "editor_ready" then
            return { editor = { code = { { load = text } } } }
        elseif event.type == "edit" then
            version = event.version
        elseif event.type == "target" then
            told[#told + 1] = event.kind
            local marks = {}
            if event.kind == "enter" then
                marks[1] = { from = event.from, to = event.to, color = "accent", underline = "accent",
                    held = "mod", over = true }
            end
            return { redraw = true, editor = { code = { { decorate = "links", version = version, from = 1,
                to = #text + 1, decorations = marks } } } }
        elseif event.type == "hover" then
            told[#told + 1] = "hover:" .. event.kind
            return { redraw = true }
        end
        return true
    end,
}"#;

fn words() -> PaneRig {
    let mut rig = PaneRig::open(WORDS);
    rig.frame(0.0);
    rig.settle(0.0);
    rig.frame(0.1);
    rig.settle(0.1);
    rig.lines();
    rig
}

/// The shapes the pane was told since it last looked, in order.
fn shapes(rig: &mut PaneRig) -> Vec<String> {
    rig.lines()
        .into_iter()
        .filter(|line| line.starts_with("cursor "))
        .collect()
}

/// A frame at `now`, and what the page said to it.
fn frame(rig: &mut PaneRig, now: f64) {
    rig.frame(now);
    rig.settle(now);
}

const COMMAND: Mods = Mods {
    shift: false,
    control: false,
    alt: false,
    command: true,
};

/// The point of the first word of the editor of [`WORDS`], below the strip.
fn on_a_word() -> Point {
    Point::new(12.0, 32.0 + 12.0)
}

/// ⌘ pressed while another window had the keyboard is not heard by the page (`flagsChanged` is the
/// key window's alone): before the pointer is told anything the pane tells the page the modifiers the
/// system holds, when they are not what it last heard, so the word under the pointer is a link and
/// the pointer the hand at once. The pointer's leaving and the keyboard's leaving make the drive
/// forget the modifiers, and the next thing the pointer does tells them again; a modifier that
/// changes alone while the pointer stands still is told by the pane's `flagsChanged` and the shape
/// it makes (`Drained::cursor`) is the pane's to take.
#[test]
fn a_modifier_pressed_elsewhere_is_told_before_the_pointer_comes_over_the_editor() {
    // Without it, the control: the pointer over the word is the I-beam and no link was offered.
    let mut rig = words();
    rig.input(Input::Move {
        at: on_a_word(),
        mods: Mods::default(),
    });
    frame(&mut rig, 0.3);
    assert_eq!(
        shapes(&mut rig).last().map(String::as_str),
        Some("cursor Text")
    );
    assert_eq!(rig.said(), "");

    let mut rig = words();
    rig.hold(COMMAND);
    rig.input(Input::Move {
        at: on_a_word(),
        mods: Mods::default(),
    });
    frame(&mut rig, 0.3);
    assert_eq!(
        shapes(&mut rig).last().map(String::as_str),
        Some("cursor Pointer")
    );
    assert_eq!(rig.said(), "enter");
    // The pointer standing still, ⌘ let go with the keyboard in the page: the page is told, the
    // link is taken back and the shape comes with no move of the pointer.
    rig.flags_changed(Mods::default());
    frame(&mut rig, 0.6);
    assert_eq!(shapes(&mut rig), ["cursor Text"]);
    assert_eq!(rig.said(), "enter leave");
    // The pointer leaves the page and ⌘ is pressed outside it; it comes back: the drive forgot the
    // modifiers with the pointer's leaving, and is told them again with its first move.
    rig.input(Input::Leave);
    rig.hold(COMMAND);
    rig.input(Input::Move {
        at: on_a_word(),
        mods: Mods::default(),
    });
    frame(&mut rig, 0.9);
    assert_eq!(
        shapes(&mut rig).last().map(String::as_str),
        Some("cursor Pointer")
    );
    assert_eq!(rig.said(), "enter leave enter");
    // The keyboard leaves the page (another window in front) with ⌘ held: the drive forgets them,
    // the link goes, and the pointer's next move tells them again.
    rig.set_active(false);
    frame(&mut rig, 1.2);
    assert_eq!(rig.said(), "enter leave enter leave");
    rig.input(Input::Move {
        at: Point::new(14.0, 32.0 + 12.0),
        mods: Mods::default(),
    });
    frame(&mut rig, 1.5);
    assert_eq!(rig.said(), "enter leave enter leave enter");
    // And the wheel: the page that does not know ⌘ is told it before the wheel.
    rig.set_active(false);
    frame(&mut rig, 1.8);
    rig.wheel(on_a_word(), 0.0, 10.0, Phase::Wheel);
    frame(&mut rig, 2.1);
    assert_eq!(
        rig.said(),
        "enter leave enter leave enter leave enter",
        "the wheel tells the modifiers too"
    );
}

/// The pointer's rest on a word is a time the page wants a frame at (`Page::wake_at` counts it as the
/// caret's blink), and the pane that is seen takes it as it takes the page's own wakes: the frame at
/// that time tells the page the pointer rested, once.
#[test]
fn the_pointers_rest_on_a_word_is_a_time_the_seen_pane_wakes_the_page_at() {
    let round = |at: Option<f64>| at.map(|at| (at * 10.0).round() / 10.0);
    let mut rig = words();
    assert_eq!(
        round(rig.wake_at()),
        None,
        "a page at rest waits for nothing"
    );
    rig.input(Input::Move {
        at: on_a_word(),
        mods: Mods::default(),
    });
    rig.settle(0.2);
    assert_eq!(
        round(rig.wake_at()),
        Some(0.6),
        "half a second after the move"
    );
    frame(&mut rig, 0.3);
    assert_eq!(rig.said(), "");
    frame(&mut rig, 0.7);
    assert_eq!(rig.said(), "hover:dwell");
    frame(&mut rig, 1.2);
    assert_eq!(rig.said(), "hover:dwell", "told once");
    assert_eq!(round(rig.wake_at()), None);
    rig.input(Input::Leave);
    frame(&mut rig, 1.5);
    assert_eq!(rig.said(), "hover:dwell hover:leave");
}

/// A page whose editor opens a menu of the page's on a right press (`gestures`): Cut and Copy are its
/// asks, Paste a line the host performs. Two keys, pressed while no editor has the keyboard, set what
/// the page does while the menu is up: `w` has it ask for a menu in its wake half a second after the
/// press (which the kit does not let open) and answer the person's next right press with a menu whose
/// every line is a Paste, `k` has the keyboard go to the page when it is woken. What the page heard
/// is the line of `said: `.
const MENU: &str = r#"local told = {}
local mode = ""
local asked = 0
return {
    render = function()
        return pencere.div():size_full()
            :child(pencere.editor("e", { gutter = false, gestures = { ["right+press"] = "context" } }):h(100):w(400))
            :child(pencere.text("said: " .. table.concat(told, " ")))
    end,
    handle = function(event)
        local t = event.type
        if t == "editor_ready" then
            return { editor = { e = { { load = "alpha beta\ngamma delta" } } } }
        elseif t == "key" then
            if event.key == "w" then mode = "menu" elseif event.key == "k" then mode = "keyboard" end
            return true
        elseif t == "press" and event.place then
            told[#told + 1] = "press:" .. event.button .. ":ctrl=" .. tostring(event.mods.control)
                .. ":inside=" .. tostring(event.place.inside) .. ":v" .. tostring(event.place.version)
            if event.button == "right" then
                asked = asked + 1
                local entries = { { title = "Cut" }, { title = "Copy" }, "-", { title = "Whatever", paste = "e" } }
                if mode == "menu" and asked == 2 then
                    entries = { { paste = "e" }, { paste = "e" }, { paste = "e" }, { paste = "e" } }
                end
                return { redraw = true, wake = mode ~= "" and 0.5 or nil, menu = { at = { x = event.x, y = event.y }, entries = entries } }
            end
        elseif t == "wake" then
            told[#told + 1] = "wake:" .. mode
            if mode == "menu" then
                return { redraw = true, menu = { at = { x = 5, y = 5 }, entries = {
                    { paste = "e" }, { paste = "e" }, { paste = "e" }, { paste = "e" } } } }
            elseif mode == "keyboard" then
                return { redraw = true, keyboard = "page" }
            end
        elseif t == "menu" then
            told[#told + 1] = "pick:" .. event.index
            if event.index == 2 then
                return { redraw = true, editor = { e = { { command = "copy" } } } }
            end
        elseif t == "text" then
            told[#told + 1] = "text:" .. event.text
        elseif t == "edit" then
            if event.origin ~= "load" then told[#told + 1] = "edit:" .. event.origin end
        end
        return true
    end,
}"#;

fn menu_page() -> PaneRig {
    let mut rig = PaneRig::open(MENU);
    rig.frame(0.0);
    rig.settle(0.0);
    rig.lines();
    rig
}

/// The text of the editor with the keyboard.
fn typed(rig: &PaneRig) -> String {
    let editors = rig.editors().expect("editors");
    let length = editors.length().expect("an editor has the keyboard");
    editors.text(0, length).expect("its text")
}

/// A press of `button` at `at` in the editor of [`MENU`], below the strip.
fn press_in_editor(rig: &mut PaneRig, button: Button, mods: Mods) {
    rig.input(Input::Press {
        at: Point::new(12.0, 32.0 + 12.0),
        button,
        mods,
        count: 1,
    });
    rig.input(Input::Release {
        at: Point::new(12.0, 32.0 + 12.0),
        button,
        mods: Mods::default(),
    });
    rig.settle(0.0);
}

const WAKE_AT: f64 = 1.0;

/// The pasteboard is read for the pick of a Paste line of the menu that is up, with the keyboard in
/// the editor the line was shown for, and for no other: the page hears the pick, never the text.
#[test]
fn a_paste_line_reads_the_pasteboard_when_it_is_picked_and_the_page_hears_the_pick_not_the_text() {
    let mut rig = menu_page();
    rig.set_pasteboard("SECRET");
    press_in_editor(&mut rig, Button::Right, Mods::default());
    // The menu is up and nothing has read the pasteboard.
    let lines = rig.lines();
    assert!(lines.iter().any(|line| line.starts_with("menu Menu")));
    assert!(
        lines.iter().all(|line| line != "pasteboard read"),
        "{lines:#?}"
    );
    rig.pick(3);
    rig.settle(0.0);
    let lines = rig.lines();
    assert_eq!(
        lines
            .iter()
            .filter(|line| *line == "pasteboard read")
            .count(),
        1
    );
    assert!(typed(&rig).contains("SECRET"), "{}", typed(&rig));
    // The menu is the kit's words, whatever the page wrote; the page is told the pick and an edit,
    // and never the text.
    let said = rig.said();
    assert!(said.contains("pick:4") && said.contains("edit:"), "{said}");
    assert!(
        !said.contains("SECRET") && !said.contains("text:"),
        "{said}"
    );
}

/// The other lines of a menu never read the pasteboard: Copy is the page's ask, which the pane
/// answers by writing the pasteboard (what the page has an editor copy, `Drained::copied`).
#[test]
fn a_line_that_is_not_a_paste_never_reads_the_pasteboard_and_a_copy_is_written_to_it() {
    let mut rig = menu_page();
    rig.set_pasteboard("SECRET");
    press_in_editor(&mut rig, Button::Right, Mods::default());
    rig.lines();
    rig.pick(1);
    rig.settle(0.0);
    let lines = rig.lines();
    assert!(
        lines.iter().all(|line| line != "pasteboard read"),
        "{lines:#?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("copy \"") && line.contains("alpha beta")),
        "{lines:#?}"
    );
    assert!(!typed(&rig).contains("SECRET"));
}

/// While a menu is up another reply of the page may ask for a menu (the main queue runs while a
/// pop-up menu is tracked, and a press of the person's that was queued is answered): the drive keeps
/// the entries of the last menu the page asked for, so a pick of a line of the one on the screen that
/// is no Paste would paste with the new menu's. That menu is not shown, and the pick is read against
/// the one the person saw. A menu the page asks for in a wake is never one of them: it does not open.
#[test]
fn a_menu_the_page_asks_for_while_another_is_up_is_not_shown_and_a_pick_is_read_against_the_shown()
{
    let mut rig = menu_page();
    rig.set_pasteboard("SECRET");
    let (key, mods) = plain('w');
    rig.key(key, mods);
    press_in_editor(&mut rig, Button::Right, Mods::default());
    // The first frame after it places the wake the menu's reply asked for, and the next wakes the page.
    rig.frame(0.1);
    rig.settle(0.1);
    rig.lines();
    // The page is woken while its menu is up and asks for a menu, whose every line is a Paste: the
    // wake is no act of the person's, so no menu comes of it, shown or not.
    rig.frame(WAKE_AT);
    rig.settle(WAKE_AT);
    let lines = rig.lines();
    assert!(rig.said().contains("wake:menu"), "{}", rig.said());
    assert!(
        lines.iter().all(|line| !line.starts_with("menu")),
        "{lines:#?}"
    );
    // The person's next press is answered with that menu while the first is still up.
    press_in_editor(&mut rig, Button::Right, Mods::default());
    let lines = rig.lines();
    assert!(
        lines.iter().any(|line| line == "menu not shown: one is up"),
        "{lines:#?}"
    );
    let before = typed(&rig);
    // The person picks Copy, a line of the menu they were shown: nothing is pasted.
    rig.pick(1);
    rig.settle(WAKE_AT);
    let lines = rig.lines();
    assert!(
        lines.iter().all(|line| line != "pasteboard read"),
        "{lines:#?}"
    );
    assert_eq!(typed(&rig), before);
    assert!(!rig.said().contains("edit:"), "{}", rig.said());
    // And a Paste of the menu that was shown still pastes (a fresh menu: the last was picked from).
    press_in_editor(&mut rig, Button::Right, Mods::default());
    rig.pick(3);
    rig.settle(WAKE_AT);
    assert!(typed(&rig).contains("SECRET"));
}

/// A Paste pastes into the editor it was shown for, and only while it has the keyboard: a reply that
/// takes the keyboard from it while the menu is up leaves the pick with nothing to paste into.
#[test]
fn a_paste_line_does_not_read_the_pasteboard_once_its_editor_has_lost_the_keyboard() {
    let mut rig = menu_page();
    rig.set_pasteboard("SECRET");
    let (key, mods) = plain('k');
    rig.key(key, mods);
    press_in_editor(&mut rig, Button::Right, Mods::default());
    rig.frame(0.1);
    rig.settle(0.1);
    rig.frame(WAKE_AT);
    rig.settle(WAKE_AT);
    assert!(rig.said().contains("wake:keyboard"), "{}", rig.said());
    rig.lines();
    let shown = rig.log_menu();
    assert!(shown.is_some());
    rig.pick(3);
    rig.settle(WAKE_AT);
    let lines = rig.lines();
    assert!(
        lines.iter().all(|line| line != "pasteboard read"),
        "{lines:#?}"
    );
    assert!(rig.said().contains("pick:4"), "{}", rig.said());
}

/// A menu dismissed lets the next the page asks for be shown (`MenuUp`).
#[test]
fn a_menu_that_was_dismissed_lets_the_next_one_be_shown() {
    let up = MenuUp::default();
    let menu = Menu {
        at: Point::new(0.0, 0.0),
        entries: vec![MenuEntry::Paste {
            title: "Paste".to_owned(),
            enabled: true,
            editor: Id::named("e"),
        }],
        serial: 1,
    };
    assert!(up.shown().is_none());
    assert!(up.raise(&menu));
    assert!(!up.raise(&menu), "one at a time");
    assert!(up.shown().is_some());
    up.lower();
    // Known until the next is shown, for an action the system sends after the pop-up returned.
    assert!(up.shown().is_some());
    assert!(up.raise(&menu));
}

/// A line is a Paste for the pick only where the menu shown says so: a Paste that is off, a rule and
/// an ordinary line are not.
#[test]
fn only_a_paste_that_is_on_is_a_paste_in_the_menu_that_was_shown() {
    let paste = |enabled| MenuEntry::Paste {
        title: "Paste".to_owned(),
        enabled,
        editor: Id::named("e"),
    };
    let menu = Menu {
        at: Point::new(0.0, 0.0),
        entries: vec![
            MenuEntry::Item {
                title: "Copy".to_owned(),
                keys: None,
                enabled: true,
            },
            MenuEntry::Separator,
            paste(false),
            paste(true),
        ],
        serial: 1,
    };
    let shown = ShownMenu::of(&menu);
    let pastes: Vec<bool> = (0..5).map(|index| shown.pastes_at(index)).collect();
    assert_eq!(pastes, [false, false, false, true, false]);
}

/// A Mac's ⌃ with the main button reaches the page as it is sent (the left button and ⌃), and the
/// drive turns it into the right press of an editor whose `gestures` say a right press opens a menu:
/// the page is told the right button's press, without the ⌃, and the menu opens.
#[test]
fn a_control_click_is_sent_as_the_left_button_and_is_a_right_press_to_the_page() {
    let control = Mods {
        control: true,
        ..Mods::default()
    };
    let mut rig = menu_page();
    press_in_editor(&mut rig, Button::Left, control);
    assert!(
        rig.said().contains("press:right:ctrl=false"),
        "{}",
        rig.said()
    );
    assert!(rig.lines().iter().any(|line| line.starts_with("menu Menu")));
    // Without the ⌃ it is the editor's own press: the page is told a left press, and no menu.
    let mut rig = menu_page();
    press_in_editor(&mut rig, Button::Left, Mods::default());
    assert!(rig.said().contains("press:left"), "{}", rig.said());
    assert!(
        rig.lines()
            .iter()
            .all(|line| !line.starts_with("menu Menu"))
    );
}

/// The press that opens a menu tells the page where it was (`place`): `inside` says it was in a
/// selection or at a caret before the press moved the caret, and `version` is the editor's.
#[test]
fn the_press_that_opens_a_menu_says_whether_it_was_inside_a_selection_or_at_a_caret() {
    let mut rig = menu_page();
    press_in_editor(&mut rig, Button::Right, Mods::default());
    assert_eq!(rig.said(), "press:right:ctrl=false:inside=false:v1");
    rig.dismiss();
    // The caret the first press put is where the second is.
    press_in_editor(&mut rig, Button::Right, Mods::default());
    assert_eq!(
        rig.said(),
        "press:right:ctrl=false:inside=false:v1 press:right:ctrl=false:inside=true:v1"
    );
}

// ─── `bateri view`: a pane's page read and driven ───────────────────────────────────────────────

/// The host's fields as a pane has them: the texts the system's controls hold.
#[derive(Default)]
struct HostFields {
    texts: RefCell<HashMap<Id, String>>,
    keyboard: Cell<Option<Id>>,
    /// What an agent holds of the pane, which is the pane's and outlives a request.
    session: Shared,
}

impl HostFields {
    fn holding(texts: &[(&str, &str)]) -> Self {
        let held = Self::default();
        for (name, text) in texts {
            held.texts
                .borrow_mut()
                .insert(Id::named(name), (*text).to_owned());
        }
        held
    }

    fn text_of(&self, name: &str) -> String {
        self.texts
            .borrow()
            .get(&Id::named(name))
            .cloned()
            .unwrap_or_default()
    }
}

impl Fields for HostFields {
    fn text(&self, id: Id) -> Option<String> {
        self.texts.borrow().get(&id).cloned()
    }
    fn put(&self, id: Id, text: &str) {
        self.texts.borrow_mut().insert(id, text.to_owned());
    }
    fn append(&self, id: Id, text: &str) -> Option<String> {
        let mut texts = self.texts.borrow_mut();
        let held = texts.entry(id).or_default();
        held.push_str(text);
        Some(held.clone())
    }
    fn keyboard_in(&self) -> Option<Id> {
        self.keyboard.get()
    }
}

fn session() -> Shared {
    Rc::new(RefCell::new(Session::default()))
}

fn request(action: Action) -> View {
    View {
        terminal: None,
        format: Format::Text,
        action,
    }
}

fn page_of(rig: &PaneRig, seen: bool, now: f64) -> Shown {
    Shown::Page {
        handle: rig.handle().clone(),
        seen,
        size: Some((800, 600)),
        scale: 1.0,
        reduce_motion: false,
        now,
    }
}

const PANE: Pane<'static> = Pane {
    name: "p9",
    program: Some("readpen"),
};

/// A request carried out as the app does: begun, and then looked at again at each drain and each
/// frame until it is answered. The clock is the test's: a tenth of a second at each look.
fn ask_in(
    rig: &mut PaneRig,
    seen: bool,
    action: Action,
    fields: &HostFields,
    session: &Shared,
    format: Format,
) -> Reply {
    let view = View {
        terminal: None,
        format,
        action,
    };
    let mut now = 1.0;
    let mut run = {
        let rig = &*rig;
        let drain = || {
            rig.drain_for_agent(session);
        };
        let env = Env {
            fields,
            session,
            drain: &drain,
        };
        match Run::begin(PANE, &page_of(rig, seen, now), &env, &view) {
            Ok(run) => run,
            Err(failure) => return Reply::Failed(failure),
        }
    };
    for _ in 0..400 {
        rig.flush_module();
        let step = {
            let rig = &*rig;
            let drain = || {
                rig.drain_for_agent(session);
            };
            let env = Env {
                fields,
                session,
                drain: &drain,
            };
            run.step(PANE, &page_of(rig, seen, now), &env)
        };
        match step {
            Step::Answer(reply) => return reply,
            // The app draws the picture; the test says what it was asked for.
            Step::Shot { full, seen } => {
                return Reply::Text(format!("shot full={full} seen={seen}"));
            }
            Step::Pending => {
                // A sound the decoder's thread is at is given the time it takes, and the test's
                // clock does not run meanwhile (the page gives up on it at its own deadline).
                let decoding = rig.handle().page().sounds_deadline().is_some();
                if decoding {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                if seen {
                    rig.frame(now);
                }
                if !decoding {
                    now += 0.1;
                }
            }
        }
    }
    panic!("the request was never answered");
}

fn ask(rig: &mut PaneRig, seen: bool, action: Action, fields: &HostFields) -> Reply {
    ask_in(rig, seen, action, fields, &fields.session, Format::Text)
}

fn answered(reply: Reply) -> String {
    match reply {
        Reply::Text(text) => text,
        other => panic!("not an answer: {other:?}"),
    }
}

fn refused(reply: Reply) -> String {
    match reply {
        Reply::Failed(failure) => failure.text,
        other => panic!("not a failure: {other:?}"),
    }
}

/// The name of the element of `words` in an answer: the first word of its line.
fn name_of(answer: &str, words: &str) -> String {
    answer
        .lines()
        .find(|line| line.contains(words))
        .and_then(|line| line.split_whitespace().next())
        .unwrap_or_else(|| panic!("no line with {words} in\n{answer}"))
        .to_owned()
}

/// A page of a text field, a secret field, a button and an editor of twenty lines, loaded when the
/// page is first laid out.
const READABLE: &str = r#"local loaded = false
local said = {}
return {
    render = function()
        return pencere.div():size_full()
            :command("go", "Go", {})
            :child(pencere.div():id("q"):text_field("q"):h(24):w(200):child(pencere.text("Search")))
            :child(pencere.div():id("pw"):text_field("pw", "secret"):h(24):w(200):child(pencere.text("Password")))
            :child(pencere.div():id("save"):press("save"):size(80, 24):child(pencere.text("Save")))
            :child(pencere.editor("sql", {}):label("Query"):h(60):w(300))
            :child(pencere.text("said: " .. table.concat(said, "|")))
    end,
    handle = function(event)
        if event.type == "viewport" and not loaded then
            loaded = true
            return { editor = { sql = { load = "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\nline 9\nline 10\nline 11\nline 12\nline 13\nline 14\nline 15\nline 16\nline 17\nline 18\nline 19\nline 20" } } }
        elseif event.type == "key" then
            said[#said + 1] = event.key
            return { redraw = true }
        end
        return true
    end,
}"#;

fn host_texts() -> HostFields {
    HostFields::holding(&[("q", "api"), ("pw", "hunter2")])
}

#[test]
fn the_reading_of_a_page_the_pane_shows_has_refs_the_fields_texts_the_editors_lines_and_no_secret()
{
    let mut rig = PaneRig::open(READABLE);
    rig.frame(0.0);
    rig.settle(0.1);
    let fields = host_texts();
    let text = answered(ask(&mut rig, true, Action::Read, &fields));
    assert!(
        text.starts_with("pane p9 (readpen)\nfocus: page\n"),
        "{text}"
    );
    assert!(text.contains("button \"Save\""), "{text}");
    assert!(text.contains("field \"Search\" = \"api\""), "{text}");
    assert!(text.contains("field (secret) \"Password\""), "{text}");
    assert!(
        text.contains(
            "editor \"Query\" lines 1-3 of 20, caret 1:1\n  1| line 1\n  2| line 2\n  3| line 3\n"
        ),
        "{text}"
    );
    assert!(text.contains("\"Go\""), "{text}");
    // Every element that can be acted on has a ref of six characters, the first word of its line.
    for line in text
        .lines()
        .filter(|line| line.contains("button \"Save\"") || line.contains("field "))
    {
        let name = line.split_whitespace().next().unwrap_or_default();
        assert!(
            name.len() == 6
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
            "{line}"
        );
    }
    let json = answered(ask_in(
        &mut rig,
        true,
        Action::Read,
        &fields,
        &session(),
        Format::Json,
    ));
    assert!(
        json.starts_with("{\"pane\":\"p9\",\"program\":\"readpen\","),
        "{json}"
    );
    assert!(json.contains("\"value\":\"api\""), "{json}");
    assert!(json.contains("\"kind\":\"secret\""), "{json}");
    for written in [&text, &json] {
        assert!(!written.contains("hunter2"), "{written}");
    }
    // Read again, the page says the same.
    assert_eq!(answered(ask(&mut rig, true, Action::Read, &fields)), text);
}

/// A pane that is not seen draws no frame, so the page it shows stays as its last frame left it:
/// what the page said since is laid out once, for the reading, and nothing is drawn, listed or
/// counted as a frame of the pane's.
#[test]
fn a_page_that_is_not_seen_is_laid_out_once_for_the_reading_and_draws_nothing() {
    let mut rig = PaneRig::open(READABLE);
    rig.frame(0.0);
    rig.settle(0.1);
    assert_eq!(rig.said(), "");
    let quads = rig.quads();
    // The pane is covered; the page hears a key and says something new, and no frame comes.
    rig.key(Key::Character('a'), Mods::default());
    rig.settle_unseen();
    assert_eq!(rig.said(), "", "no frame was drawn");
    let fields = host_texts();
    let read = answered(ask(&mut rig, false, Action::Read, &fields));
    assert!(read.contains("said: a"), "{read}");
    assert!(read.contains("button \"Save\""), "{read}");
    assert!(read.contains("field \"Search\" = \"api\""), "{read}");
    // What the layout would have drawn is let go: the rig, which keeps the list of the last frame
    // it was given, has the one it had.
    assert_eq!(rig.said(), "");
    assert_eq!(rig.quads(), quads);
}

/// A page that was never laid out has had no room told it and the module has made nothing: the
/// request lays it out once, and again for what the module makes of it, and then answers with the
/// page in it (not with the line the kit shows while a page starts).
#[test]
fn a_page_that_has_never_been_laid_out_is_read_once_the_module_has_made_it() {
    let mut rig = PaneRig::open(READABLE);
    let fields = host_texts();
    let read = answered(ask(&mut rig, false, Action::Read, &fields));
    assert!(read.contains("button \"Save\""), "{read}");
    assert!(!read.contains("Starting the page"), "{read}");
    assert_eq!(rig.quads(), 0, "nothing was drawn");
}

/// The layout for a reading makes no list and counts no frame.
#[test]
fn the_layout_for_a_reading_makes_no_list_and_counts_no_frame() {
    let source = include_str!("page.rs");
    let start = source
        .find("pub fn lay_out_unseen")
        .expect("the layout for a reading is in the page's handle");
    let body = &source[start
        ..source[start..]
            .find("\n    }\n")
            .map_or(source.len(), |end| start + end)];
    for drawn in [
        "to_list",
        "present",
        "request_frame",
        "damage",
        "frames +=",
        "Quad",
        "set_clock",
    ] {
        assert!(!body.contains(drawn), "{drawn} in {body}");
    }
    assert!(
        body.contains("frame(at)") && body.contains("tell_frame"),
        "{body}"
    );
}

#[test]
fn what_is_not_a_page_is_said_by_what_it_is_and_a_page_with_no_room_says_it_is_not_placed() {
    let rig = PaneRig::open(READABLE);
    let held = session();
    let fields = HostFields::default();
    let ask_of = |shown: &Shown| {
        let drain = || {};
        let env = Env {
            fields: &fields,
            session: &held,
            drain: &drain,
        };
        match Run::begin(PANE, shown, &env, &request(Action::Read)) {
            Ok(_) => panic!("a request was begun"),
            Err(failure) => failure.text,
        }
    };
    let terminal = ask_of(&Shown::Terminal);
    assert!(
        terminal.contains("terminal") && terminal.contains("bateri capture p9"),
        "{terminal}"
    );
    let not_up = ask_of(&Shown::NotUp);
    assert!(not_up.contains("not up yet"), "{not_up}");
    let web = ask_of(&Shown::Web);
    assert!(web.contains("web view"), "{web}");
    for size in [None, Some((0, 600)), Some((800, 0))] {
        let shown = Shown::Page {
            handle: rig.handle().clone(),
            seen: true,
            size,
            scale: 1.0,
            reduce_motion: false,
            now: 1.0,
        };
        let none = ask_of(&shown);
        assert!(none.contains("not placed"), "{none}");
    }
}

/// A page to act on: two commands of one title and one that is off, a text field, a secret field, a
/// button, an editor with a menu, a box of thirty rows that scrolls, and a line of what the page
/// heard. Its keys ask for what a host does for a page: `c` a copy, `o` a link, `u` a file panel,
/// `f` the keyboard in the field `q` with a text for it, `k` a call to the program.
const ACT: &str = r#"local said = {}
local function note(words) said[#said + 1] = words end
local function at(event) return string.format("%g,%g", event.x, event.y) end
return {
    render = function()
        local list = pencere.div():id("list"):h(100):w(200):scroll()
        for n = 1, 30 do list:child(pencere.div():h(20):child(pencere.text("row " .. n))) end
        return pencere.div():size_full()
            :command("run", "Run", "cmd+enter")
            :command("off", "Off", {}, false)
            :command("t1", "Twice", {})
            :command("t2", "twice", {})
            :child(pencere.div():h(24):row()
                :child(pencere.div():id("q"):text_field("q"):w(200):h(20):child(pencere.text("Filter")))
                :child(pencere.div():id("pw"):text_field("pw", "secret"):w(200):h(20):child(pencere.text("Password")))
                :child(pencere.div():id("save"):w(60):h(20):label("Save"):role("button")))
            :child(pencere.editor("e", { gutter = false, gestures = { ["right+press"] = "context" } }):h(60):w(400))
            :child(list)
            :child(pencere.text("said: " .. table.concat(said, "|")))
    end,
    handle = function(event)
        local t = event.type
        if t == "editor_ready" then
            return { editor = { e = { { load = "hello\nworld" } } } }
        elseif t == "key" then
            local k = event.key
            note("key " .. k)
            if k == "c" then
                return { redraw = true, copy = "copied words" }
            elseif k == "o" then
                return { redraw = true, link = "https://example.com/a" }
            elseif k == "u" then
                return { redraw = true, pick = { multiple = false } }
            elseif k == "h" then
                return { redraw = true, haptic = "success", sound = { "tap", { name = "snap", pitch = 0.8 } } }
            elseif k == "H" then
                return { redraw = true, haptic = { events = { { time = 0, kind = "tap", intensity = 1, sharpness = 0.5 } } }, sound = false }
            elseif k == "x" then
                return { redraw = true, haptic = false }
            elseif k == "f" then
                return { redraw = true, keyboard = { field = "q" }, fill = { q = "filled" } }
            elseif k == "k" then
                pencere.call("save", nil, function(result, err) note("saved") return { redraw = true } end)
                return true
            end
            return { redraw = true }
        elseif t == "press" then
            if event.place then
                if event.button == "right" then
                    return { redraw = true, menu = { at = { x = event.x, y = event.y }, entries = {
                        { title = "Cut" }, { title = "Copy", enabled = false }, "-", { title = "Whatever" } } } }
                end
                return true
            end
            note("press " .. at(event))
            return { redraw = true }
        elseif t == "release" then
            note("release")
            return { redraw = true }
        elseif t == "menu" then
            note("pick " .. event.index)
            return { redraw = true }
        elseif t == "command" then
            note("command " .. event.id)
            return { redraw = true }
        elseif t == "text" then
            if event.field ~= "pw" then note("text " .. event.field .. " " .. event.text) end
            return { redraw = true }
        elseif t == "scrolled" then
            note("scrolled " .. event.boxes[1].id)
            return { redraw = true }
        end
        return true
    end,
}"#;

fn acting() -> (PaneRig, HostFields) {
    let mut rig = PaneRig::open(ACT);
    rig.frame(0.0);
    rig.settle(0.1);
    (rig, HostFields::holding(&[("q", "api"), ("pw", "hunter2")]))
}

fn names(rig: &mut PaneRig, fields: &HostFields) -> String {
    answered(ask(rig, true, Action::Read, fields))
}

#[test]
fn a_click_and_a_double_click_press_the_middle_of_the_element_and_the_answer_names_it() {
    let (mut rig, fields) = acting();
    let read = names(&mut rig, &fields);
    let save = name_of(&read, "button \"Save\"");
    let said = answered(ask(&mut rig, true, Action::Click(save.clone()), &fields));
    assert!(
        said.starts_with(&format!("clicked button \"Save\" ({save})\n")),
        "{said}"
    );
    assert!(
        said.contains("press ") && said.contains("release"),
        "{said}"
    );
    // The name is a person's to type in any case.
    let again = answered(ask(
        &mut rig,
        true,
        Action::DoubleClick(save.to_uppercase()),
        &fields,
    ));
    assert!(
        again.starts_with(&format!("double-clicked button \"Save\" ({save})")),
        "{again}"
    );
    assert_eq!(again.matches("press ").count(), 3, "{again}");
    // A name no element has, and one that is not a name, are told apart from a mistake in the page.
    let unknown = refused(ask(&mut rig, true, Action::Click("zzzzzz".into()), &fields));
    assert!(
        unknown.contains("no element") && unknown.contains("read the view again"),
        "{unknown}"
    );
}

#[test]
fn a_command_is_found_by_its_title_in_any_case_and_a_closed_a_missing_and_a_doubled_one_say_so() {
    let (mut rig, fields) = acting();
    let done = answered(ask(&mut rig, true, Action::Command("rUN".into()), &fields));
    assert!(done.starts_with("ran the command \"Run\"\n"), "{done}");
    assert!(done.contains("command run"), "{done}");
    let off = refused(ask(&mut rig, true, Action::Command("off".into()), &fields));
    assert!(off.contains("turned off"), "{off}");
    let twice = refused(ask(
        &mut rig,
        true,
        Action::Command("TWICE".into()),
        &fields,
    ));
    assert!(twice.contains("more than one"), "{twice}");
    let none = refused(ask(&mut rig, true, Action::Command("Nope".into()), &fields));
    assert!(
        none.contains("no command titled") && none.contains("\"Run\""),
        "{none}"
    );
}

#[test]
fn fill_puts_the_text_in_the_page_and_the_field_and_the_keyboard_stays_where_it_is() {
    let (mut rig, fields) = acting();
    let read = names(&mut rig, &fields);
    let q = name_of(&read, "field \"Filter\"");
    let said = answered(ask(
        &mut rig,
        true,
        Action::Fill {
            target: q.clone(),
            text: "docker".into(),
        },
        &fields,
    ));
    assert!(
        said.starts_with(&format!(
            "filled field \"Filter\" ({q}) with 6 characters\n"
        )),
        "{said}"
    );
    assert_eq!(fields.text_of("q"), "docker");
    // The page heard the person's text, and its keyboard is in the field: the reading says so.
    assert!(said.contains("text q docker"), "{said}");
    assert!(said.contains(&format!("focus: field {q}")), "{said}");
    // The system's keyboard was told nothing: no line of what the pane does about it.
    assert!(
        rig.lines().iter().all(|line| !line.starts_with("keyboard")),
        "the system's keyboard moved"
    );
    // The editor's whole text is replaced.
    let read = names(&mut rig, &fields);
    let e = name_of(&read, "editor \"");
    answered(ask(
        &mut rig,
        true,
        Action::Fill {
            target: e.clone(),
            text: "one\ntwo".into(),
        },
        &fields,
    ));
    let held = answered(ask(&mut rig, true, Action::Text(e), &fields));
    assert_eq!(held, "one\ntwo\n");
    // And what is not a field is told.
    let save = name_of(&read, "button \"Save\"");
    let not = refused(ask(
        &mut rig,
        true,
        Action::Fill {
            target: save,
            text: "x".into(),
        },
        &fields,
    ));
    assert!(not.contains("neither a field nor an editor"), "{not}");
}

#[test]
fn typing_goes_where_the_keyboard_is_and_a_secret_is_never_said_back() {
    let (mut rig, fields) = acting();
    let read = names(&mut rig, &fields);
    let q = name_of(&read, "field \"Filter\"");
    let pw = name_of(&read, "field (secret)");
    let e = name_of(&read, "editor \"");
    // No keyboard in the page: nothing to type into.
    let nowhere = refused(ask(&mut rig, true, Action::Type("x".into()), &fields));
    assert!(nowhere.contains("no editor or field"), "{nowhere}");
    // A click on a field puts the page's keyboard there.
    let clicked = answered(ask(&mut rig, true, Action::Click(q.clone()), &fields));
    assert!(
        clicked.starts_with("put the keyboard in field"),
        "{clicked}"
    );
    let typed = answered(ask(&mut rig, true, Action::Type("er".into()), &fields));
    assert!(
        typed.starts_with(&format!("typed 2 characters into the field {q}")),
        "{typed}"
    );
    assert_eq!(fields.text_of("q"), "apier");
    // The secret: filled and typed into, never read and never said.
    let filled = answered(ask(
        &mut rig,
        true,
        Action::Fill {
            target: pw.clone(),
            text: "s3cret".into(),
        },
        &fields,
    ));
    assert!(filled.contains("with 6 characters"), "{filled}");
    let more = answered(ask(&mut rig, true, Action::Type("!".into()), &fields));
    for answer in [&filled, &more] {
        assert!(
            !answer.contains("s3cret") && !answer.contains("hunter2"),
            "{answer}"
        );
    }
    assert_eq!(fields.text_of("pw"), "s3cret!");
    let read_it = refused(ask(&mut rig, true, Action::Text(pw), &fields));
    assert!(
        read_it.contains("secret field") && !read_it.contains("s3cret"),
        "{read_it}"
    );
    // Into the editor that has the keyboard.
    answered(ask(&mut rig, true, Action::Click(e.clone()), &fields));
    answered(ask(&mut rig, true, Action::Type("Z".into()), &fields));
    let held = answered(ask(&mut rig, true, Action::Text(e), &fields));
    assert!(held.contains('Z'), "{held}");
}

/// A backspace with the keyboard in a field of the host takes the last letter out of what the
/// host's field holds (the key that is no letter was told "pressed" and did nothing), and a request
/// that puts nothing in the page holds nothing of what the person does meanwhile.
#[test]
fn a_backspace_in_a_field_of_the_host_takes_a_letter_out_and_a_read_holds_nothing() {
    let (mut rig, fields) = acting();
    let read = names(&mut rig, &fields);
    let q = name_of(&read, "field \"Filter\"");
    answered(ask(
        &mut rig,
        true,
        Action::Fill {
            target: q.clone(),
            text: "docker".into(),
        },
        &fields,
    ));
    let said = answered(ask(
        &mut rig,
        true,
        Action::Press(vec!["backspace".into(), "backspace".into()]),
        &fields,
    ));
    assert!(said.contains("pressed backspace backspace"), "{said}");
    assert_eq!(fields.text_of("q"), "dock");
    // There is nothing after the caret to delete.
    answered(ask(
        &mut rig,
        true,
        Action::Press(vec!["delete".into()]),
        &fields,
    ));
    assert_eq!(fields.text_of("q"), "dock");
    // A character a person sees as one goes whole; a ⌫ with a modifier is the page's key.
    answered(ask(
        &mut rig,
        true,
        Action::Fill {
            target: q.clone(),
            text: "a\u{1F44D}\u{1F3FD}".into(),
        },
        &fields,
    ));
    answered(ask(
        &mut rig,
        true,
        Action::Press(vec!["alt+backspace".into()]),
        &fields,
    ));
    assert_eq!(fields.text_of("q"), "a\u{1F44D}\u{1F3FD}");
    answered(ask(
        &mut rig,
        true,
        Action::Press(vec!["backspace".into()]),
        &fields,
    ));
    assert_eq!(fields.text_of("q"), "a");
    // Only a request that drives the page holds the page's replies.
    let begun = |rig: &PaneRig, action: Action| {
        let session = Rc::clone(&fields.session);
        let drain = || {
            rig.drain_for_agent(&session);
        };
        let env = Env {
            fields: &fields,
            session: &session,
            drain: &drain,
        };
        let view = View {
            terminal: None,
            format: Format::Text,
            action,
        };
        let run = Run::begin(PANE, &page_of(rig, true, 1.0), &env, &view);
        (run.is_ok(), session.borrow().active)
    };
    assert_eq!(begun(&rig, Action::Read), (true, false));
    assert_eq!(
        begun(
            &rig,
            Action::Wait {
                text: "x".into(),
                gone: false,
                seconds: Some(1)
            }
        ),
        (true, false)
    );
    assert_eq!(begun(&rig, Action::Click(q)), (true, true));
}

#[test]
fn keys_go_to_the_page_and_what_it_asks_of_the_host_is_held_back_and_told() {
    let (mut rig, fields) = acting();
    let _ = rig.lines();
    // A copy is not written to the pasteboard: it comes in the answer.
    let copied = answered(ask(
        &mut rig,
        true,
        Action::Press(vec!["c".into()]),
        &fields,
    ));
    assert!(
        copied.contains("copied, not written to the pasteboard: \"copied words\""),
        "{copied}"
    );
    // A link is not opened.
    let linked = answered(ask(
        &mut rig,
        true,
        Action::Press(vec!["o".into()]),
        &fields,
    ));
    assert!(
        linked.contains("link not opened: https://example.com/a"),
        "{linked}"
    );
    // A panel is turned away: the page hears no files, and none is shown.
    let panel = answered(ask(
        &mut rig,
        true,
        Action::Press(vec!["u".into()]),
        &fields,
    ));
    assert!(
        panel.contains("a file panel was asked for and turned away"),
        "{panel}"
    );
    // The paste key pastes nothing: the pasteboard is the person's.
    rig.set_pasteboard("the person's words");
    let pasted = answered(ask(
        &mut rig,
        true,
        Action::Press(vec!["cmd+v".into()]),
        &fields,
    ));
    assert!(!pasted.contains("the person's words"), "{pasted}");
    // None of that reached the pane: it did not copy, open or show anything.
    let done = rig.lines();
    for line in &done {
        assert!(
            !line.starts_with("copy ") && !line.starts_with("link ") && !line.starts_with("panel"),
            "the pane did {line}"
        );
    }
    // The keyboard the page sends to a field is the page's, and the system's stays.
    let sent = answered(ask(
        &mut rig,
        true,
        Action::Press(vec!["f".into()]),
        &fields,
    ));
    assert!(sent.contains("focus: field "), "{sent}");
    assert!(
        rig.lines().iter().all(|line| !line.starts_with("keyboard")),
        "the system's keyboard moved"
    );
    // A key that is none, and all of them read before any is pressed.
    let none = refused(ask(
        &mut rig,
        true,
        Action::Press(vec!["a".into(), "cmd+nonsense+".into()]),
        &fields,
    ));
    assert!(none.contains("is not a key"), "{none}");
    let heard = names(&mut rig, &fields);
    assert!(
        !heard.contains("key a"),
        "the first key was pressed: {heard}"
    );
}

/// The person's trackpad and speaker are not the agent's: a page's vibration and sounds are not
/// played for a request, and the agent is told what the page asked for instead, at the moment the
/// page asked.
#[test]
fn a_vibration_and_a_sound_the_page_asks_for_are_told_to_the_agent_and_not_played() {
    let (mut rig, fields) = acting();
    let _ = rig.lines();
    let named = answered(ask(
        &mut rig,
        true,
        Action::Press(vec!["h".into()]),
        &fields,
    ));
    assert!(
        named.contains("the page asked to vibrate: success"),
        "{named}"
    );
    assert!(named.contains("the page played the sound tap"), "{named}");
    assert!(named.contains("the page played the sound snap"), "{named}");
    let own = answered(ask(
        &mut rig,
        true,
        Action::Press(vec!["H".into()]),
        &fields,
    ));
    assert!(
        own.contains("the page asked to vibrate: a pattern of its own"),
        "{own}"
    );
    assert!(own.contains("the page ended its sounds"), "{own}");
    let stopped = answered(ask(
        &mut rig,
        true,
        Action::Press(vec!["x".into()]),
        &fields,
    ));
    assert!(
        stopped.contains("the page ended its vibration"),
        "{stopped}"
    );
    // The pane's own host vibrated and played nothing; it was told the ends, which it ends what the
    // person's page began with.
    let heard = rig.lines();
    for line in &heard {
        assert!(
            !((line.starts_with("haptic") || line.starts_with("sound")) && line.contains("Play")),
            "the pane did {line}"
        );
    }
    assert!(heard.iter().any(|line| line == "haptic Stop"), "{heard:?}");
    assert!(heard.iter().any(|line| line == "sound Stop"), "{heard:?}");
}

/// A bundle of one file, a sound the page names by path.
struct OneSound(std::sync::Arc<[u8]>);

impl pencere_sandbox::Assets for OneSound {
    fn read(&self, path: &str) -> Option<std::sync::Arc<[u8]>> {
        // The decoder's thread is slower than the page's coming to rest, as a long file is.
        std::thread::sleep(std::time::Duration::from_millis(150));
        (path == "sounds/ding.wav").then(|| std::sync::Arc::clone(&self.0))
    }

    fn media_type(&self, _path: &str) -> Option<String> {
        None
    }
}

/// A sound of the page's own file is decoded on a thread of its own and told when it is ready:
/// the request waits for it, so that it is told to the agent (as the sound of a kit's name is)
/// and is not played for the person once the request is over.
#[test]
fn a_sound_from_a_file_the_page_asks_for_is_waited_for_and_told_to_the_agent() {
    use crate::native_view::sounds;
    let tap = sounds::play_of("tap", 1.0, 0).expect("the kit has a tap");
    let wav = sounds::wav(&tap).expect("a tap is a file");
    let source = ACT.replace(
        r#"elseif k == "x" then"#,
        r#"elseif k == "d" then
                return { redraw = true, sound = { file = "sounds/ding.wav" } }
            elseif k == "x" then"#,
    );
    let mut rig = PaneRig::open_bundled(
        &source,
        crate::native_view::LookSlot::default(),
        Some(std::sync::Arc::new(OneSound(wav.into()))),
    );
    rig.frame(0.0);
    rig.settle(0.1);
    let fields = HostFields::holding(&[("q", "api"), ("pw", "hunter2")]);
    let _ = rig.lines();
    let told = answered(ask(
        &mut rig,
        true,
        Action::Press(vec!["d".into()]),
        &fields,
    ));
    assert!(
        told.contains("the page played a sound from sounds/ding.wav"),
        "{told}"
    );
    // Nothing is left to be told afterwards: the pane's own drain has no sound to play.
    assert!(!rig.handle().page().sounds_waiting());
    for line in rig.lines() {
        assert!(!line.starts_with("sound"), "the pane did {line}");
    }
    rig.drain_for_agent(&fields.session);
    for line in rig.lines() {
        assert!(!line.starts_with("sound"), "the pane did {line}");
    }
}

#[test]
fn a_box_scrolls_by_screens_to_its_ends_and_what_does_not_scroll_says_so() {
    let (mut rig, fields) = acting();
    let read = names(&mut rig, &fields);
    let list = name_of(&read, "scroll ");
    let down = answered(ask(
        &mut rig,
        true,
        Action::Scroll {
            target: list.clone(),
            way: kablo_bateri::view::Way::Down,
            count: Some(2),
        },
        &fields,
    ));
    assert!(
        down.contains("scrolled box that scrolls") && down.contains("down 2 screens"),
        "{down}"
    );
    assert!(
        down.contains("scrolled list"),
        "the page heard no scroll: {down}"
    );
    let bottom = answered(ask(
        &mut rig,
        true,
        Action::Scroll {
            target: list.clone(),
            way: kablo_bateri::view::Way::Bottom,
            count: None,
        },
        &fields,
    ));
    assert!(bottom.contains("to the bottom"), "{bottom}");
    let top = answered(ask(
        &mut rig,
        true,
        Action::Scroll {
            target: list,
            way: kablo_bateri::view::Way::Top,
            count: None,
        },
        &fields,
    ));
    assert!(top.contains("to the top"), "{top}");
    let save = name_of(&read, "button \"Save\"");
    let not = refused(ask(
        &mut rig,
        true,
        Action::Scroll {
            target: save,
            way: kablo_bateri::view::Way::Down,
            count: None,
        },
        &fields,
    ));
    assert!(not.contains("does not scroll"), "{not}");
}

#[test]
fn a_menu_is_asked_for_held_read_and_picked_from_and_let_go_of_by_the_next_action() {
    let (mut rig, fields) = acting();
    let read = names(&mut rig, &fields);
    let e = name_of(&read, "editor \"");
    let _ = rig.lines();
    let session = Rc::clone(&fields.session);
    let asked = answered(ask_in(
        &mut rig,
        true,
        Action::Menu(e.clone()),
        &fields,
        &session,
        Format::Text,
    ));
    assert!(asked.starts_with("asked for the menu of editor"), "{asked}");
    // The lines are in the reading, numbered by the agent's count, the one that is off marked.
    assert!(
        asked.contains("menu:")
            && asked.contains("Cut")
            && asked.contains("Copy")
            && asked.contains("Whatever"),
        "{asked}"
    );
    // It was not popped up.
    assert!(
        rig.lines().iter().all(|line| !line.starts_with("menu")),
        "the pane showed the menu"
    );
    // A line is picked by its title in any case, and the page hears its place among the entries, from 1.
    let picked = answered(ask_in(
        &mut rig,
        true,
        Action::Pick("cut".into()),
        &fields,
        &session,
        Format::Text,
    ));
    assert!(
        picked.starts_with("picked \"Cut\" from the menu"),
        "{picked}"
    );
    assert!(picked.contains("pick 1"), "{picked}");
    assert!(session.borrow().menu.is_none());
    // By number: the third line is the entry after the separator, the fourth of the entries.
    ask_in(
        &mut rig,
        true,
        Action::Menu(e.clone()),
        &fields,
        &session,
        Format::Text,
    );
    let by_number = answered(ask_in(
        &mut rig,
        true,
        Action::Pick("3".into()),
        &fields,
        &session,
        Format::Text,
    ));
    assert!(
        by_number.contains("picked \"Whatever\"") && by_number.contains("pick 4"),
        "{by_number}"
    );
    // A line that is off is not picked, and one that is not there is told.
    ask_in(
        &mut rig,
        true,
        Action::Menu(e.clone()),
        &fields,
        &session,
        Format::Text,
    );
    let off = refused(ask_in(
        &mut rig,
        true,
        Action::Pick("Copy".into()),
        &fields,
        &session,
        Format::Text,
    ));
    assert!(off.contains("turned off"), "{off}");
    let none = refused(ask_in(
        &mut rig,
        true,
        Action::Pick("9".into()),
        &fields,
        &session,
        Format::Text,
    ));
    assert!(none.contains("no line 9"), "{none}");
    // The next action lets the menu go.
    answered(ask_in(
        &mut rig,
        true,
        Action::Read,
        &fields,
        &session,
        Format::Text,
    ));
    let gone = refused(ask_in(
        &mut rig,
        true,
        Action::Pick("Cut".into()),
        &fields,
        &session,
        Format::Text,
    ));
    assert!(gone.contains("no menu is held"), "{gone}");
}

#[test]
fn a_call_the_program_has_not_answered_is_waited_for_and_the_answer_says_so_at_the_limit() {
    let (mut rig, fields) = acting();
    // The page calls its program on `k`; nobody answers.
    let waited = answered(ask(
        &mut rig,
        true,
        Action::Press(vec!["k".into()]),
        &fields,
    ));
    assert!(
        waited.contains("1 call is waiting for an answer"),
        "{waited}"
    );
    // The answer comes later: the page is at rest, and what it did with the answer is read.
    rig.handle()
        .page()
        .message(r#"{"response":{"id":1,"result":{}}}"#);
    let read = names(&mut rig, &fields);
    assert!(read.contains("saved"), "{read}");
    assert!(!read.contains("call is waiting"), "{read}");
}

#[test]
fn a_seen_page_that_no_frame_comes_for_is_answered_at_the_limit_with_what_is_going_on() {
    let (mut rig, fields) = acting();
    // The key makes the page say something: a frame is owed, and nobody draws it.
    rig.key(Key::Character('x'), Mods::default());
    rig.flush_module();
    let session = session();
    let view = request(Action::Read);
    let mut now = 1.0;
    let mut run = {
        let rig = &rig;
        let drain = || {
            rig.drain_for_agent(&session);
        };
        let env = Env {
            fields: &fields,
            session: &session,
            drain: &drain,
        };
        Run::begin(PANE, &page_of(rig, true, now), &env, &view).expect("begun")
    };
    let mut pending = 0;
    let reply = loop {
        rig.flush_module();
        let step = {
            let rig = &rig;
            let drain = || {
                rig.drain_for_agent(&session);
            };
            let env = Env {
                fields: &fields,
                session: &session,
                drain: &drain,
            };
            run.step(PANE, &page_of(rig, true, now), &env)
        };
        match step {
            Step::Pending => {
                pending += 1;
                now += 0.1;
            }
            Step::Answer(reply) => break reply,
            Step::Shot { .. } => panic!("a reading is answered with text"),
        }
        assert!(pending < 100, "no answer at the limit");
    };
    let text = answered(reply);
    assert!(text.contains("the page is still settling"), "{text}");
    // The limit is the named one: two seconds of the clock it is given.
    assert!((15..=25).contains(&pending), "{pending}");
}

#[test]
fn wait_answers_when_the_words_show_or_go_and_fails_at_its_time() {
    let (mut rig, fields) = acting();
    let shows = answered(ask(
        &mut rig,
        true,
        Action::Wait {
            text: "row 1".into(),
            gone: false,
            seconds: None,
        },
        &fields,
    ));
    assert!(shows.contains("row 1"), "{shows}");
    let gone = answered(ask(
        &mut rig,
        true,
        Action::Wait {
            text: "no such words".into(),
            gone: true,
            seconds: None,
        },
        &fields,
    ));
    assert!(gone.starts_with("pane p9"), "{gone}");
    let late = refused(ask(
        &mut rig,
        true,
        Action::Wait {
            text: "no such words".into(),
            gone: false,
            seconds: Some(1),
        },
        &fields,
    ));
    assert!(
        late.contains("timed out after 1 seconds waiting for \"no such words\" to show"),
        "{late}"
    );
    let stays = refused(ask(
        &mut rig,
        true,
        Action::Wait {
            text: "row 1".into(),
            gone: true,
            seconds: Some(1),
        },
        &fields,
    ));
    assert!(stays.contains("to go"), "{stays}");
    // A wait is not a change: it is a reading that goes on.
    assert!(!crate::agent::view::drives(&Action::Wait {
        text: String::new(),
        gone: false,
        seconds: None
    }));
}

#[test]
fn an_element_is_named_by_what_it_was_before_the_action_took_it_away() {
    let (mut rig, fields) = acting();
    let read = names(&mut rig, &fields);
    let save = name_of(&read, "button \"Save\"");
    // The page re-renders with the press; the answer's first line is of the element as it was read.
    let said = answered(ask(&mut rig, true, Action::Click(save.clone()), &fields));
    assert!(
        said.starts_with(&format!("clicked button \"Save\" ({save})")),
        "{said}"
    );
}

#[test]
fn a_shot_waits_for_the_page_to_rest_does_nothing_to_it_and_is_the_apps_to_draw() {
    let (mut rig, fields) = acting();
    let session = Rc::clone(&fields.session);
    let asked = answered(ask_in(
        &mut rig,
        true,
        Action::Shot { full: false },
        &fields,
        &session,
        Format::Text,
    ));
    assert_eq!(asked, "shot full=false seen=true");
    let full = answered(ask(&mut rig, true, Action::Shot { full: true }, &fields));
    assert_eq!(full, "shot full=true seen=true");
    assert!(!session.borrow().active, "the request is over");
    // Nothing was done to the page: it heard no key and no press.
    let heard = names(&mut rig, &fields);
    assert!(heard.contains("text \"said: \""), "{heard}");
}

/// A pane that is not seen is pictured as the page is laid out now: the request says so, and the
/// page was laid out for it (it has a layout and the reading of it has the page's things), with
/// nothing drawn.
#[test]
fn a_shot_of_a_pane_that_is_not_seen_says_so_and_has_the_page_laid_out() {
    let mut rig = PaneRig::open(ACT);
    let fields = host_texts();
    let asked = answered(ask(&mut rig, false, Action::Shot { full: false }, &fields));
    assert_eq!(asked, "shot full=false seen=false");
    assert!(
        rig.handle().page().laid().is_some(),
        "the page was not laid out for the picture"
    );
    assert_eq!(rig.quads(), 0, "nothing was drawn for it");
}

/// A page that never comes to rest is pictured as it is at the limit, as a reading is answered.
#[test]
fn a_shot_of_a_page_that_does_not_come_to_rest_is_asked_for_at_the_limit() {
    let (mut rig, fields) = acting();
    // The page's program is waited on: no call is answered.
    let held = answered(ask(
        &mut rig,
        true,
        Action::Press(vec!["k".into()]),
        &fields,
    ));
    assert!(held.contains("call is waiting"), "{held}");
    let asked = answered(ask(&mut rig, true, Action::Shot { full: false }, &fields));
    assert_eq!(asked, "shot full=false seen=true");
}

/// A page of a table, an outline and, when asked, a dialog in front: what a screen reader is given
/// of it ([`super::ax`]).
const TREE_PAGE: &str = r#"
local modal = false
return {
    render = function()
        local head = pencere.div():id("head"):role("row"):row()
            :child(pencere.div():id("cpu"):role("columnheader"):cell_at(nil, 1):sorted("ascending"):press("cpu"):h(20):w(80):child(pencere.text("CPU")))
            :child(pencere.div():id("name"):role("columnheader"):cell_at(nil, 2):press("name"):h(20):w(80):child(pencere.text("Name")))
        local row = pencere.div():id("r5"):role("row"):cell_at(5):position(5, 200):press("r5"):h(20):w(200):child(pencere.text("node"))
        local grid = pencere.div():id("grid"):role("table"):label("Processes"):table_size(200, 2):child(head):child(row)
        local src = pencere.div():id("src"):role("treeitem"):label("src"):level(1):expanded(true):press("src"):h(20):w(100)
        local main = pencere.div():id("main"):role("treeitem"):label("main.rs"):level(2):press("main"):h(20):w(100)
        local tree = pencere.div():id("tree"):role("tree"):label("Files"):child(src):child(main)
        local page = pencere.div():size_full():child(grid):child(tree)
        if modal then
            page:child(pencere.div():id("dialog"):overlay():inset(0, 0, 0, 0):layer(true)
                :child(pencere.div():id("card"):role("dialog"):label("Settings"):col():w(200):h(80)
                    :child(pencere.div():id("ok"):press("ok"):h(24):w(60):child(pencere.text("OK")))))
        end
        return page
    end,
    handle = function(event)
        if event.type == "key" and event.key == "d" then
            modal = true
            return { redraw = true }
        end
    end,
}
"#;

/// The tree of what the page says: a table with its head, its headers and its rows, an outline with
/// its levels; and with a dialog in front, nothing but the dialog and what is in it, the page behind
/// it neither read nor in the tree.
#[test]
fn a_screen_reader_is_given_the_table_and_the_outline_and_with_a_dialog_in_front_the_dialog_alone()
{
    use super::ax;

    let mut rig = PaneRig::open(TREE_PAGE);
    rig.frame(0.0);
    rig.settle(0.0);
    let items = rig.handle().page().outline().accessible.clone();
    let at = |words: &str| {
        items
            .iter()
            .position(|item| item.words == words)
            .unwrap_or_else(|| panic!("{words}: {items:?}"))
    };
    let nodes = ax::plan(&items);
    let (grid, tree) = (at("Processes"), at("Files"));
    assert_eq!(nodes[grid].row_count, Some(200));
    assert_eq!(nodes[grid].column_count, Some(2));
    assert_eq!(nodes[grid].column_headers.len(), 2);
    assert_eq!(nodes[grid].rows.len(), 1);
    let head = nodes[grid].header.expect("a head");
    assert_eq!(nodes[head].children.len(), 2);
    let cpu = at("CPU");
    assert_eq!(nodes[cpu].sort, Some(super::Sort::Ascending));
    assert_eq!(nodes[cpu].index, Some(0));
    assert_eq!(nodes[nodes[grid].rows[0]].index, Some(4));
    assert_eq!(nodes[tree].rows.len(), 2);
    let (src, main) = (at("src"), at("main.rs"));
    assert_eq!(nodes[src].disclosure_level, Some(0));
    assert_eq!(nodes[main].disclosure_level, Some(1));
    assert_eq!(nodes[main].disclosed_by, Some(src));
    assert_eq!(nodes[src].disclosed_rows, [main]);
    assert!(ax::top_level(&nodes).len() >= 2);

    rig.key(plain('d').0, plain('d').1);
    rig.frame(0.1);
    rig.settle(0.2);
    let items = rig.handle().page().outline().accessible.clone();
    let nodes = ax::plan(&items);
    let top = ax::top_level(&nodes);
    assert_eq!(top.len(), 1, "{items:?}");
    assert_eq!(items[top[0]].words, "Settings");
    assert_eq!(items[top[0]].role, super::AccessibleRole::Dialog);
    assert!(
        nodes[top[0]]
            .children
            .iter()
            .any(|child| items[*child].words == "OK"),
        "{items:?}"
    );
    assert!(
        items.iter().all(|item| item.words != "Processes"),
        "the page behind is not in the tree: {items:?}"
    );
}

/// A list of processes: each row lists the commands it can be given (`Stop`, `Restart` that is off now),
/// and the page says which row a command was done on.
const PROCESSES: &str = r#"
local said = {}
return {
    render = function()
        local list = pencere.div():id("procs"):role("list"):label("Processes"):h(100):w(300):scroll():table_size(3, 1)
        for n = 1, 3 do
            list:child(pencere.div():id("p" .. n):role("listitem"):label("proc " .. n):press("p" .. n):h(20)
                :command("stop", "Stop", {})
                :command("restart", "Restart", {}, n ~= 2)
                :child(pencere.text("proc " .. n)))
        end
        return pencere.div():size_full():child(list):child(pencere.text("said: " .. table.concat(said, "|")))
    end,
    handle = function(event)
        if event.type == "command" then
            said[#said + 1] = event.id .. " on " .. tostring(event.target)
            return { redraw = true }
        end
        return true
    end,
}
"#;

/// `command REF TITLE` does the command an element lists, on that element, found by its title in any
/// case; one that is off, one the element does not list and an element that lists none say so; `reveal`
/// goes to the page as it was asked.
#[test]
fn a_command_an_element_lists_is_done_on_it_by_its_title_and_the_ones_it_does_not_have_say_so() {
    let mut rig = PaneRig::open(PROCESSES);
    rig.frame(0.0);
    rig.settle(0.1);
    let fields = HostFields::holding(&[]);
    let read = answered(ask(&mut rig, true, Action::Read, &fields));
    assert!(
        read.contains("action \"Stop\"") && read.contains("action \"Restart\""),
        "{read}"
    );
    let second = name_of(&read, "listitem \"proc 2\"");
    let done = answered(ask(
        &mut rig,
        true,
        Action::CommandOn {
            target: second.clone(),
            title: "sTOP".into(),
        },
        &fields,
    ));
    assert!(
        done.starts_with(&format!(
            "ran the command \"Stop\" on listitem \"proc 2\" ({second})\n"
        )),
        "{done}"
    );
    assert!(done.contains("said: stop on"), "{done}");
    let off = refused(ask(
        &mut rig,
        true,
        Action::CommandOn {
            target: second.clone(),
            title: "Restart".into(),
        },
        &fields,
    ));
    assert!(off.contains("turned off"), "{off}");
    let none = refused(ask(
        &mut rig,
        true,
        Action::CommandOn {
            target: second.clone(),
            title: "Kill".into(),
        },
        &fields,
    ));
    assert!(
        none.contains("has no command titled \"Kill\"")
            && none.contains("\"Stop\"")
            && none.contains("\"Restart\""),
        "{none}"
    );
    let list = name_of(&read, "list \"Processes\"");
    let plain = refused(ask(
        &mut rig,
        true,
        Action::CommandOn {
            target: list,
            title: "Stop".into(),
        },
        &fields,
    ));
    assert!(plain.contains("has no command titled"), "{plain}");
    // An element that is gone, or never was, is not found.
    let unknown = refused(ask(
        &mut rig,
        true,
        Action::CommandOn {
            target: "zzzzzz".into(),
            title: "Stop".into(),
        },
        &fields,
    ));
    assert!(unknown.contains("no element"), "{unknown}");

    let revealed = answered(ask(
        &mut rig,
        true,
        Action::Reveal {
            target: second.clone(),
            index: None,
        },
        &fields,
    ));
    assert!(
        revealed.starts_with(&format!(
            "brought listitem \"proc 2\" ({second}) into view\n"
        )),
        "{revealed}"
    );
    // A row is of a list, a table or an outline, and a first one is row 1: a row of a row is none.
    let list = name_of(&read, "list \"Processes\"");
    let row_two = answered(ask(
        &mut rig,
        true,
        Action::Reveal {
            target: list.clone(),
            index: Some(2),
        },
        &fields,
    ));
    assert!(
        row_two.starts_with(&format!(
            "brought row 2 of list \"Processes\" ({list}) into view\n"
        )),
        "{row_two}"
    );
    let not_a_list = refused(ask(
        &mut rig,
        true,
        Action::Reveal {
            target: second.clone(),
            index: Some(2),
        },
        &fields,
    ));
    assert!(
        not_a_list.contains("is not a list, a table or an outline"),
        "{not_a_list}"
    );
    let zero = refused(ask(
        &mut rig,
        true,
        Action::Reveal {
            target: list,
            index: Some(0),
        },
        &fields,
    ));
    assert!(zero.contains("no such row 0"), "{zero}");
    let past = refused(ask(
        &mut rig,
        true,
        Action::Reveal {
            target: name_of(&read, "list \"Processes\""),
            index: Some(4),
        },
        &fields,
    ));
    assert!(
        past.contains("no such row 4") && past.contains("1 to 3"),
        "{past}"
    );
}

/// A page of two boxes: a ring of 2 points a point off a box, and a box with a border that scrolls.
const RINGED: &str = r#"return {
    render = function()
        local list = pencere.div():id("list"):w(120):h(80):border(2, "border"):scroll()
        for n = 1, 20 do list:child(pencere.div():h(20):no_shrink()) end
        return pencere.div():size_full():p(10):gap(12)
            :child(pencere.div():id("ring"):w(40):h(20):outline(2, "accent", 1))
            :child(list)
    end,
    handle = function(event) end,
}"#;

/// What the page draws as a ring and as the bars of a box that scrolls are boxes like any other: the
/// ring a border and no fill a few points off the box, the bars (track and knob) in the padding box,
/// 2 points in from the border. The renderer takes them as they are (`quad`), with no mapping of its
/// own for either.
#[test]
fn a_ring_and_the_bars_of_a_box_with_a_border_reach_the_renderer_as_boxes() {
    use bt_core::Theme;
    use bt_gpu::{UiQuad, UiRect};
    use pencere_view::{Paint, Rect, Role};

    use super::{clip_at, quad, resolve};

    let theme = Theme::BATERI;
    let origin = (3.0, 32.0);
    let mut rig = PaneRig::open(RINGED);
    rig.set_scrolling(SCROLLING);
    rig.frame(0.0);
    rig.settle(0.0);
    let quads: Vec<_> = rig
        .list()
        .items
        .iter()
        .filter_map(|item| match item {
            Paint::Quad(quad) => Some(quad),
            _ => None,
        })
        .collect();
    // What the renderer is handed for a box of the page, as the page drew it.
    let as_drawn = |drawn: &pencere_view::Quad| {
        let mut out = UiQuad::new(UiRect::new(
            drawn.rect.x + origin.0,
            drawn.rect.y + origin.1,
            drawn.rect.w,
            drawn.rect.h,
        ))
        .radii(drawn.radii)
        .clipped(clip_at(drawn.clip, origin));
        if let Some(fill) = drawn.fill {
            out = out.fill(resolve(&theme, fill));
        }
        if let Some(border) = drawn.border {
            out = out.border(border.width, resolve(&theme, border.color));
        }
        out
    };

    // The ring: the box is at (10, 10), 40 by 20; the ring stands 3 points out of it (a point of
    // offset and 2 of width) and has no fill.
    let ring = quads
        .iter()
        .find(|quad| quad.rect == Rect::new(7.0, 7.0, 46.0, 26.0))
        .expect("a ring");
    assert!(ring.fill.is_none() && ring.border.is_some_and(|border| border.width == 2.0));
    assert_eq!(quad(&theme, ring, origin), as_drawn(ring));
    assert_eq!(
        quad(&theme, ring, origin),
        UiQuad::new(UiRect::new(10.0, 39.0, 46.0, 26.0))
            .radii(ring.radii)
            .clipped(clip_at(ring.clip, origin))
            .border(2.0, resolve(&theme, Role::Accent.solid())),
    );

    // The bars of the box that scrolls, a box of (10, 42), 120 by 80, with a border of 2: they stand
    // in its padding box, which ends at 128, 2 points in from there; the border is not under them.
    let bars: Vec<_> = quads
        .iter()
        .filter(|quad| quad.fill.is_some() && quad.rect.x >= 100.0)
        .collect();
    assert_eq!(bars.len(), 2, "the track and the knob");
    for bar in bars {
        assert_eq!(bar.rect.right(), 126.0, "{:?}", bar.rect);
        assert!(
            bar.rect.y >= 46.0 && bar.rect.bottom() <= 118.0,
            "{:?}",
            bar.rect
        );
        assert_eq!(quad(&theme, bar, origin), as_drawn(bar));
    }
}
