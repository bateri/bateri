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
    Accessible, Bars, Button, Command, Cursor, Id, Input, Key, Keyboard, Menu, Mods, Phase, Point,
    Scrolling,
};

pub(super) use super::page::tests::PaneRig;

/// What the pane did and what the page heard, a line each, in order.
#[derive(Default)]
pub(super) struct Transcript(Vec<String>);

impl Transcript {
    pub(super) fn say(&mut self, line: impl Into<String>) {
        self.0.push(line.into());
    }

    /// Takes the lines written so far.
    pub(super) fn take(&mut self) -> Vec<String> {
        std::mem::take(&mut self.0)
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
    pub(super) fn fill(&mut self, field: Id, text: &str) {
        self.say(format!("fill {field:?} {text:?}"));
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
        self.say(format!("menu {menu:?}"));
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
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
            "redraw",
            "redraw",
            "route Enter NotEditor",
            "route Character('g') NotEditor",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
            "route Character('l') NotEditor",
            "redraw",
            "keyboard Field(Id(12638182802509129152))",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
            "redraw",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
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
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
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
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
            "redraw",
            "redraw",
            "route Character('l') NotEditor",
            "redraw",
            "keyboard Field(Id(12638182802509129152))",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
            "redraw",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
            "marked Some((0, 2)) selected Some((2, 0)) length Some(13) text Some(\"か\\u{3099}hello\") rect Some(Rect { x: 4.0, y: 76.0, w: 2.0, h: 20.0 }) contents Some((\"か\\u{3099}hello\\nworld\", (2, 0)))",
            "discard marked text",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "cursor Text",
            "redraw",
            "cursor Arrow",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
            "marked None selected Some((7, 0)) length Some(13) text Some(\"か\\u{3099}hello\") rect Some(Rect { x: 4.0, y: 76.0, w: 2.0, h: 20.0 }) contents Some((\"か\\u{3099}hello\\nworld\", (7, 0)))",
            "index Some(0)",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
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
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 159.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 175.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 190.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 206.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 221.0, w: 200.0, h: 16.0 }, words: \"row 8\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 237.0, w: 200.0, h: 16.0 }, words: \"row 9\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 253.0, w: 200.0, h: 15.0 }, words: \"row 10\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 268.0, w: 200.0, h: 16.0 }, words: \"row 11\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
            "redraw",
            "cursor Arrow",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 159.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 175.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 190.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 206.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 221.0, w: 200.0, h: 16.0 }, words: \"row 8\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 237.0, w: 200.0, h: 16.0 }, words: \"row 9\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 253.0, w: 200.0, h: 15.0 }, words: \"row 10\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 268.0, w: 200.0, h: 16.0 }, words: \"row 11\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 162.0, w: 200.0, h: 16.0 }, words: \"row 24\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 178.0, w: 200.0, h: 16.0 }, words: \"row 25\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 194.0, w: 200.0, h: 15.0 }, words: \"row 26\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 209.0, w: 200.0, h: 16.0 }, words: \"row 27\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 225.0, w: 200.0, h: 16.0 }, words: \"row 28\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 241.0, w: 200.0, h: 15.0 }, words: \"row 29\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 256.0, w: 200.0, h: 16.0 }, words: \"row 30\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
            "redraw",
            "cursor Text",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 162.0, w: 200.0, h: 16.0 }, words: \"row 24\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 178.0, w: 200.0, h: 16.0 }, words: \"row 25\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 194.0, w: 200.0, h: 15.0 }, words: \"row 26\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 209.0, w: 200.0, h: 16.0 }, words: \"row 27\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 225.0, w: 200.0, h: 16.0 }, words: \"row 28\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 241.0, w: 200.0, h: 15.0 }, words: \"row 29\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 256.0, w: 200.0, h: 16.0 }, words: \"row 30\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
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
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
            "redraw",
            "redraw",
            "route Character('c') NotEditor",
            "redraw",
            "copy \"copied words\"",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
            "route Character('o') NotEditor",
            "redraw",
            "link https://example.com/a?b=c",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
            "route Character('m') NotEditor",
            "redraw",
            "menu Menu { at: Point { x: 10.0, y: 52.0 }, entries: [Item { title: \"One\", key: Some('1'), enabled: true }, Separator, Item { title: \"Two\", key: None, enabled: false }] }",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
            "route Character('f') NotEditor",
            "redraw",
            "fill Id(12638204792741693372) \"filled\"",
            "keyboard Field(Id(12638204792741693372))",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
            "route Character('p') NotEditor",
            "redraw",
            "keyboard Page",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
            "route Character('s') NotEditor",
            "send {\"said\":\"hi\"}",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
            "cursor Pointer",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
            "said key c|key o|key m|key f|key p|key s|menu 2|text q abc|focus q|focus nil|command run|files 20,10 /tmp/a b,/tmp/c",
            "route Character('x') NotEditor",
            "close",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
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
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
            "redraw",
            "redraw",
            "route Character('w') NotEditor",
            "rearm",
            "rearm",
            "frame at 0.1: asked due",
            "frame at 0.4: asked Some(0.6)",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
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
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
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
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
            "redraw",
            "redraw",
            "route Character('l') NotEditor",
            "redraw",
            "keyboard Field(Id(12638182802509129152))",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
            "redraw",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 200.0, h: 20.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 72.0, w: 400.0, h: 100.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 172.0, w: 200.0, h: 16.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 188.0, w: 200.0, h: 15.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 203.0, w: 200.0, h: 16.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 219.0, w: 200.0, h: 16.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 235.0, w: 200.0, h: 15.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 250.0, w: 200.0, h: 16.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 266.0, w: 200.0, h: 15.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 200.0, y: 32.0, w: 40.0, h: 20.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
            "caret Some(Rect { x: 4.0, y: 76.0, w: 2.0, h: 20.0 }) boxes 4",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 400.0, y: 32.0, w: 80.0, h: 40.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 112.0, w: 800.0, h: 200.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 312.0, w: 400.0, h: 31.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 343.0, w: 400.0, h: 32.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 375.0, w: 400.0, h: 31.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 406.0, w: 400.0, h: 31.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 437.0, w: 400.0, h: 31.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 468.0, w: 400.0, h: 32.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 500.0, w: 400.0, h: 31.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 400.0, y: 32.0, w: 80.0, h: 40.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
            "caret Some(Rect { x: 8.0, y: 120.0, w: 4.0, h: 40.0 }) boxes 4",
            "redraw",
            "away: boxes 3",
            "redraw",
            "back: boxes 4",
            "redraw",
            "redraw",
            "cursor Text",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 400.0, y: 32.0, w: 80.0, h: 40.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 112.0, w: 800.0, h: 200.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 312.0, w: 400.0, h: 31.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 343.0, w: 400.0, h: 32.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 375.0, w: 400.0, h: 31.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 406.0, w: 400.0, h: 31.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 437.0, w: 400.0, h: 31.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 468.0, w: 400.0, h: 32.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 500.0, w: 400.0, h: 31.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 400.0, y: 32.0, w: 80.0, h: 40.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
            "route Character('m') NotEditor",
            "redraw",
            "menu Menu { at: Point { x: 20.0, y: 72.0 }, entries: [Item { title: \"One\", key: Some('1'), enabled: true }, Separator, Item { title: \"Two\", key: None, enabled: false }] }",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 400.0, y: 32.0, w: 80.0, h: 40.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 112.0, w: 800.0, h: 200.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 312.0, w: 400.0, h: 31.0 }, words: \"row 1\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 343.0, w: 400.0, h: 32.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 375.0, w: 400.0, h: 31.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 406.0, w: 400.0, h: 31.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 437.0, w: 400.0, h: 31.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 468.0, w: 400.0, h: 32.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 500.0, w: 400.0, h: 31.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 400.0, y: 32.0, w: 80.0, h: 40.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
            "redraw",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 400.0, y: 32.0, w: 80.0, h: 40.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 112.0, w: 800.0, h: 200.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 283.0, w: 400.0, h: 32.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 315.0, w: 400.0, h: 31.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 346.0, w: 400.0, h: 31.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 377.0, w: 400.0, h: 31.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 408.0, w: 400.0, h: 32.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 440.0, w: 400.0, h: 31.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 471.0, w: 400.0, h: 31.0 }, words: \"row 8\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 502.0, w: 400.0, h: 31.0 }, words: \"row 9\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 400.0, y: 32.0, w: 80.0, h: 40.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
            "redraw",
            "cursor Arrow",
            "redraw",
            "outline fields [Field { id: Id(12638204792741693372), kind: Line, rect: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, hint: Some(\"Filter\"), label: None }] accessible [Accessible { role: Field(Line), id: Some(Id(12638204792741693372)), rect: Rect { x: 0.0, y: 32.0, w: 400.0, h: 40.0 }, words: \"Filter\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Button, id: Some(Id(763668877872884410)), rect: Rect { x: 400.0, y: 32.0, w: 80.0, h: 40.0 }, words: \"Help\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Editor, id: Some(Id(12638182802509129152)), rect: Rect { x: 0.0, y: 112.0, w: 800.0, h: 200.0 }, words: \"\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 283.0, w: 400.0, h: 32.0 }, words: \"row 2\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 315.0, w: 400.0, h: 31.0 }, words: \"row 3\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 346.0, w: 400.0, h: 31.0 }, words: \"row 4\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 377.0, w: 400.0, h: 31.0 }, words: \"row 5\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 408.0, w: 400.0, h: 32.0 }, words: \"row 6\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 440.0, w: 400.0, h: 31.0 }, words: \"row 7\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 471.0, w: 400.0, h: 31.0 }, words: \"row 8\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }, Accessible { role: Text, id: None, rect: Rect { x: 0.0, y: 502.0, w: 400.0, h: 31.0 }, words: \"row 9\", state: State { checked: None, selected: false, expanded: None, disabled: false, value: None } }] tips [Tip { rect: Rect { x: 400.0, y: 32.0, w: 80.0, h: 40.0 }, words: \"Help (?)\" }] commands [Command { id: Id(9922177756001819210), title: \"Run\", keys: Some(Shortcut { key: Enter, mods: Mods { shift: false, control: false, alt: false, command: true } }), enabled: true }]",
            "said key l|edit load hello\nworld|focus nil|press 250,259|key m|scrolled list 30|scroll 10",
        ],
    );
}
