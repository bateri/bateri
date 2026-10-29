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
    pub(super) fn copy(&mut self, text: &str) {
        self.say(format!("copy {text:?}"));
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
                    { title = "One", key = "1", enabled = true }, "-", { title = "Two", enabled = false } } } }
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
        elseif t == "scroll" then
            note("scroll " .. event.dy)
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
            note("edit " .. event.origin .. " " .. pencere.layout.editor_text("e", 0, pencere.layout.editor_length("e")))
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
            "outline fields [] accessible [Accessible { role: Text, id: None, rect: Rect { x: 344.0, y: 308.0, w: 112.0, h: 16.0 }, words: \"Starting the page…\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [] commands []",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "redraw",
            "redraw",
            "redraw",
            "route Enter NotEditor",
            "route Character('g') NotEditor",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "route Character('l') NotEditor",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "keyboard Field(Id(12638182802509129152))",
            "redraw",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
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
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
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
            "outline fields [] accessible [Accessible { role: Text, id: None, rect: Rect { x: 344.0, y: 308.0, w: 112.0, h: 16.0 }, words: \"Starting the page…\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [] commands []",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "redraw",
            "redraw",
            "redraw",
            "route Character('l') NotEditor",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "keyboard Field(Id(12638182802509129152))",
            "redraw",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
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
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "marked None selected Some((7, 0)) length Some(13) text Some(\"か\\u{3099}hello\") rect Some(Rect { x: 4.0, y: 76.0, w: 2.0, h: 20.0 }) contents Some((\"か\\u{3099}hello\\nworld\", (7, 0)))",
            "index Some(0)",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "marked None selected Some((3, 0)) length Some(14) text Some(\"HE!hell\") rect Some(Rect { x: 4.0, y: 76.0, w: 2.0, h: 20.0 }) contents Some((\"HE!hello\\nworld\", (3, 0)))",
            "said key l|edit load hello\nworld|edit compose か\u{3099}hello\nworld|press e@7|release 300,45|edit compose HEhello\nworld|edit person HE!hello\nworld",
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
            "outline fields [] accessible [Accessible { role: Text, id: None, rect: Rect { x: 344.0, y: 308.0, w: 112.0, h: 16.0 }, words: \"Starting the page…\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [] commands []",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 159.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 175.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 190.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 206.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 221.0, w: 200.0, h: 16.0 }, words: \"row 8\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 237.0, w: 200.0, h: 16.0 }, words: \"row 9\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 253.0, w: 200.0, h: 15.0 }, words: \"row 10\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 268.0, w: 200.0, h: 16.0 }, words: \"row 11\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "redraw",
            "cursor Arrow",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 159.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 175.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 190.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 206.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 221.0, w: 200.0, h: 16.0 }, words: \"row 8\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 237.0, w: 200.0, h: 16.0 }, words: \"row 9\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 253.0, w: 200.0, h: 15.0 }, words: \"row 10\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 268.0, w: 200.0, h: 16.0 }, words: \"row 11\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 162.0, w: 200.0, h: 16.0 }, words: \"row 24\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 178.0, w: 200.0, h: 16.0 }, words: \"row 25\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 194.0, w: 200.0, h: 15.0 }, words: \"row 26\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 209.0, w: 200.0, h: 16.0 }, words: \"row 27\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 225.0, w: 200.0, h: 16.0 }, words: \"row 28\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 241.0, w: 200.0, h: 15.0 }, words: \"row 29\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 256.0, w: 200.0, h: 16.0 }, words: \"row 30\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "redraw",
            "cursor Text",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 162.0, w: 200.0, h: 16.0 }, words: \"row 24\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 178.0, w: 200.0, h: 16.0 }, words: \"row 25\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 194.0, w: 200.0, h: 15.0 }, words: \"row 26\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 209.0, w: 200.0, h: 16.0 }, words: \"row 27\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 225.0, w: 200.0, h: 16.0 }, words: \"row 28\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 241.0, w: 200.0, h: 15.0 }, words: \"row 29\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 256.0, w: 200.0, h: 16.0 }, words: \"row 30\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "said scrolled list 60|scroll 30|scrolled list 369|press 100,190",
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
    rig.input(Input::Move(Point::new(300.0, 32.0 + 300.0)));
    rig.settle(0.2);
    rig.pick(1);
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
            "outline fields [] accessible [Accessible { role: Text, id: None, rect: Rect { x: 344.0, y: 308.0, w: 112.0, h: 16.0 }, words: \"Starting the page…\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [] commands []",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "redraw",
            "redraw",
            "redraw",
            "route Character('c') NotEditor",
            "redraw",
            "copy \"copied words\"",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "route Character('o') NotEditor",
            "redraw",
            "link https://example.com/a?b=c",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "route Character('m') NotEditor",
            "redraw",
            "menu Menu { at: Point { x: 10.0, y: 52.0 }, entries: [Item { title: \"One\", keys: Some(Shortcut { key: Character('1'), mods: Mods { shift: false, control: false, alt: false, command: false } }), enabled: true }, Separator, Item { title: \"Two\", keys: None, enabled: false }] }",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "route Character('f') NotEditor",
            "redraw",
            "fill Id(12638204792741693372) \"filled\"",
            "keyboard Field(Id(12638204792741693372))",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "route Character('p') NotEditor",
            "redraw",
            "keyboard Page",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "route Character('s') NotEditor",
            "send {\"said\":\"hi\"}",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "cursor Pointer",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "said key c|key o|key m|key f|key p|key s|menu 2|text q abc|focus q|focus nil|command run|files 20,10 /tmp/a b,/tmp/c",
            "route Character('x') NotEditor",
            "close",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
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
            "outline fields [] accessible [Accessible { role: Text, id: None, rect: Rect { x: 344.0, y: 308.0, w: 112.0, h: 16.0 }, words: \"Starting the page…\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [] commands []",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "redraw",
            "redraw",
            "redraw",
            "route Character('w') NotEditor",
            "rearm",
            "rearm",
            "frame at 0.1: asked due",
            "frame at 0.4: asked Some(0.6)",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
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
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
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
            "outline fields [] accessible [Accessible { role: Text, id: None, rect: Rect { x: 344.0, y: 308.0, w: 112.0, h: 16.0 }, words: \"Starting the page…\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [] commands []",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "redraw",
            "redraw",
            "redraw",
            "route Character('l') NotEditor",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "keyboard Field(Id(12638182802509129152))",
            "redraw",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "caret Some(Rect { x: 4.0, y: 76.0, w: 2.0, h: 20.0 }) boxes 4",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 400.0, y: 32.0, w: 80.0, h: 40.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 112.0, w: 800.0, h: 200.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 312.0, w: 400.0, h: 31.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 343.0, w: 400.0, h: 32.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 375.0, w: 400.0, h: 31.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 406.0, w: 400.0, h: 31.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 437.0, w: 400.0, h: 31.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 468.0, w: 400.0, h: 32.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 500.0, w: 400.0, h: 31.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 400.0, y: 32.0, w: 80.0, h: 40.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "caret Some(Rect { x: 8.0, y: 120.0, w: 4.0, h: 40.0 }) boxes 4",
            "redraw",
            "away: boxes 3",
            "redraw",
            "back: boxes 4",
            "redraw",
            "redraw",
            "cursor Text",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 400.0, y: 32.0, w: 80.0, h: 40.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 112.0, w: 800.0, h: 200.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 312.0, w: 400.0, h: 31.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 343.0, w: 400.0, h: 32.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 375.0, w: 400.0, h: 31.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 406.0, w: 400.0, h: 31.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 437.0, w: 400.0, h: 31.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 468.0, w: 400.0, h: 32.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 500.0, w: 400.0, h: 31.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 400.0, y: 32.0, w: 80.0, h: 40.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "route Character('m') NotEditor",
            "redraw",
            "menu Menu { at: Point { x: 20.0, y: 72.0 }, entries: [Item { title: \"One\", keys: Some(Shortcut { key: Character('1'), mods: Mods { shift: false, control: false, alt: false, command: false } }), enabled: true }, Separator, Item { title: \"Two\", keys: None, enabled: false }] }",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 400.0, y: 32.0, w: 80.0, h: 40.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 112.0, w: 800.0, h: 200.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 312.0, w: 400.0, h: 31.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 343.0, w: 400.0, h: 32.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 375.0, w: 400.0, h: 31.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 406.0, w: 400.0, h: 31.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 437.0, w: 400.0, h: 31.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 468.0, w: 400.0, h: 32.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 500.0, w: 400.0, h: 31.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 400.0, y: 32.0, w: 80.0, h: 40.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 400.0, y: 32.0, w: 80.0, h: 40.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 112.0, w: 800.0, h: 200.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 283.0, w: 400.0, h: 32.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 315.0, w: 400.0, h: 31.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 346.0, w: 400.0, h: 31.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 377.0, w: 400.0, h: 31.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 408.0, w: 400.0, h: 32.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 440.0, w: 400.0, h: 31.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 471.0, w: 400.0, h: 31.0 }, words: \"row 8\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 502.0, w: 400.0, h: 31.0 }, words: \"row 9\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 400.0, y: 32.0, w: 80.0, h: 40.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "redraw",
            "cursor Arrow",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 400.0, y: 32.0, w: 80.0, h: 40.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 112.0, w: 800.0, h: 200.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 283.0, w: 400.0, h: 32.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 315.0, w: 400.0, h: 31.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 346.0, w: 400.0, h: 31.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 377.0, w: 400.0, h: 31.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 408.0, w: 400.0, h: 32.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 440.0, w: 400.0, h: 31.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 471.0, w: 400.0, h: 31.0 }, words: \"row 8\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 502.0, w: 400.0, h: 31.0 }, words: \"row 9\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 400.0, y: 32.0, w: 80.0, h: 40.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: [Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }], enabled: true }]",
            "said key l|edit load hello\nworld|focus nil|press 250,259|key m|scrolled list 30|scroll 10",
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
                fill = { find = { text = "needle", select = { from = 1, to = 4 } } },
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
            return { redraw = true, editor = { code = { { decorate = "links", version = version, from = 0,
                to = #text, decorations = marks } } } }
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
    rig.input(Input::Move(on_a_word()));
    frame(&mut rig, 0.3);
    assert_eq!(
        shapes(&mut rig).last().map(String::as_str),
        Some("cursor Text")
    );
    assert_eq!(rig.said(), "");

    let mut rig = words();
    rig.hold(COMMAND);
    rig.input(Input::Move(on_a_word()));
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
    rig.input(Input::Move(on_a_word()));
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
    rig.input(Input::Move(Point::new(14.0, 32.0 + 12.0)));
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
    rig.input(Input::Move(on_a_word()));
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
/// the page does when it is woken half a second after the menu opened, which is another reply while
/// the menu is up:
/// `w` asks for another menu whose every line is a Paste, `k` has the keyboard go to the page. What
/// the page heard is the line of `said: `.
const MENU: &str = r#"local told = {}
local mode = ""
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
                return { redraw = true, wake = mode ~= "" and 0.5 or nil, menu = { at = { x = event.x, y = event.y }, entries = {
                    { title = "Cut" }, { title = "Copy" }, "-", { title = "Whatever", paste = "e" } } } }
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

/// While a menu is up the page may reply with another (the main queue runs while a pop-up menu is
/// tracked): the drive keeps the entries of the last menu the page asked for, so a pick of a line
/// of the one on the screen that is no Paste would paste with the new menu's. That menu is not
/// shown, and the pick is read against the one the person saw.
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
    // The page is woken while its menu is up and asks for another, whose every line is a Paste.
    rig.frame(WAKE_AT);
    rig.settle(WAKE_AT);
    let lines = rig.lines();
    assert!(
        lines.iter().any(|line| line == "menu not shown: one is up"),
        "{lines:#?}"
    );
    assert!(rig.said().contains("wake:menu"), "{}", rig.said());
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
            keys: None,
            enabled: true,
            editor: Id::named("e"),
        }],
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
        keys: None,
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
