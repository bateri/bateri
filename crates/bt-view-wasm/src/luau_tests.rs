//! The interpreter (`luau.wasm`, built from `guest/luau`) through the sandbox, with scripts. It
//! is part of the repository, so these run wherever the tests do (`make check` needs no
//! toolchain for WebAssembly), and they are what notices an interpreter built from another
//! interface than the one the sandbox speaks.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use bt_view::{Button, Element, Event, Id, Input, Key, Mods, Point};
use wasmtime::component::Component;

use crate::guests_tests::{Open, Rough, SIZE};
use crate::{Fault, Limits, Module, Program, Unbounded, Update};

pub(crate) fn open_script(script: &str, limits: Limits) -> Open {
    open_script_for(script, limits, Arc::new(Unbounded))
}

pub(crate) fn open_script_for(script: &str, limits: Limits, program: Arc<dyn Program>) -> Open {
    let (told, woken) = channel();
    let told = std::sync::Mutex::new(told);
    let module = Module::from_script(script.as_bytes().to_vec(), limits, program, move || {
        let _ = told.lock().unwrap().send(());
    });
    Open {
        module,
        woken,
        seen: Update::default(),
    }
}

/// The first thing a script's page is: the page, or why it is closed.
fn first(script: &str, limits: Limits) -> Update {
    let mut page = open_script(script, limits);
    page.module.render(SIZE, 0.0, false);
    page.until(Duration::from_secs(20), |u| {
        u.frame.is_some() || u.closed.is_some()
    })
}

/// The words of the text elements of a page, in order.
fn words(root: &Element) -> Vec<String> {
    root.fold()
        .into_iter()
        .filter_map(|node| match node.kind {
            bt_view::wire::Kind::Text(text) => Some(text),
            _ => None,
        })
        .collect()
}

/// Takes until what has been seen so far (not only the last taking) says `done`, or `within`.
fn until_seen(page: &mut Open, within: Duration, done: impl Fn(&Update) -> bool) {
    let deadline = Instant::now() + within;
    while !done(&page.seen) && Instant::now() < deadline {
        page.take(Duration::from_millis(100));
    }
}

fn failed(update: &Update) -> String {
    match &update.closed {
        Some(Fault::Failed(why)) => why.clone(),
        other => panic!("expected a script's failure, got {other:?}"),
    }
}

#[test]
fn the_interpreter_asks_for_nothing_the_page_interface_does_not_give() {
    let engine = crate::sandbox::engine().expect("an engine");
    let component = Component::new(&engine, crate::LUAU_COMPONENT).expect("a component");
    let imports: Vec<String> = component
        .component_type()
        .imports(&engine)
        .map(|(name, _)| name.to_owned())
        .collect();
    assert!(!imports.is_empty());
    assert!(
        imports.iter().all(|name| name.starts_with("bateri:view/")),
        "an interpreter that imports more than the interface: {imports:?}"
    );
    // And it is the interface of this sandbox: it starts, and runs a page.
    let update = first(
        "return { render = function() return bateri.div() end }",
        Limits::default(),
    );
    assert_eq!(update.closed, None);
    assert!(update.frame.is_some());
}

#[test]
fn a_script_that_returns_a_page_is_drawn_with_what_it_made_of_the_viewport() {
    let update = first(
        r#"
        return {
            render = function(size, now, reduce)
                return bateri.div():gap(4):child(bateri.text("w " .. size.w .. " h " .. size.h)):child(
                    bateri.text(string.format("now %.1f %s", now, tostring(reduce)))), false
            end,
        }
        "#,
        Limits::default(),
    );
    assert_eq!(update.closed, None);
    let frame = update.frame.expect("a page");
    assert!(!frame.animating);
    assert_eq!(words(&frame.root), ["w 1000 h 700", "now 0.0 false"]);
}

#[test]
fn a_page_that_asks_to_animate_is_asked_for_again() {
    let update = first(
        "return { render = function() return bateri.div(), true end }",
        Limits::default(),
    );
    assert!(update.frame.expect("a page").animating);
}

#[test]
fn a_syntax_error_closes_the_page_and_says_the_line() {
    let update = first(
        "local ok = 1\nlocal oops = = 3\nreturn {}",
        Limits::default(),
    );
    let why = failed(&update);
    assert!(why.starts_with("script:2:"), "{why}");
    assert!(update.frame.is_none());
}

#[test]
fn an_error_while_the_script_starts_says_the_line() {
    let update = first("local t = nil\n\nreturn t.page", Limits::default());
    let why = failed(&update);
    assert!(why.starts_with("script:3:"), "{why}");
}

#[test]
fn an_error_in_render_says_the_line_and_the_frame_is_not_read() {
    let update = first(
        "return { render = function()\n  local x = nil\n  return bateri.div():child(x.y)\nend }",
        Limits::default(),
    );
    let why = failed(&update);
    assert!(why.starts_with("script:3:"), "{why}");
    assert!(
        update.frame.is_none(),
        "a page from a call that failed is not read"
    );
}

#[test]
fn an_error_in_a_handler_closes_the_page_with_its_line() {
    let mut page = open_script(
        "return {\n render = function() return bateri.div() end,\n handle = function(e)\n  error('no ' .. e.type)\n end,\n}",
        Limits::default(),
    );
    page.module.render(SIZE, 0.0, false);
    page.until(Duration::from_secs(20), |u| u.frame.is_some());
    page.module
        .event(Event::Input(Input::Move(Point::new(1.0, 1.0))));
    let update = page.until(Duration::from_secs(5), |u| u.closed.is_some());
    let why = failed(&update);
    assert!(why.starts_with("script:4: no move"), "{why}");
}

#[test]
fn an_error_the_script_catches_is_not_an_error() {
    let update = first(
        r#"
        local ok, why = pcall(function() error("inner", 0) end)
        local ok2, why2 = pcall(function() local t = nil; return t.x end)
        return { render = function()
            return bateri.div():child(bateri.text(tostring(ok) .. " " .. why .. " | " .. why2))
        end }
        "#,
        Limits::default(),
    );
    assert_eq!(update.closed, None);
    let text = words(&update.frame.expect("a page").root).join("");
    assert!(text.starts_with("false inner | script:3:"), "{text}");
}

#[test]
fn a_script_that_does_not_return_a_page_is_closed_and_told_so() {
    for script in ["return 3", "return {}", "return { render = 4 }", ""] {
        let update = first(script, Limits::default());
        let why = failed(&update);
        assert!(why.contains("must return a page"), "{script:?}: {why}");
    }
    let update = first(
        "return { render = function() return 5 end }",
        Limits::default(),
    );
    assert!(failed(&update).contains("render must return an element"));
}

#[test]
fn a_tree_that_is_not_a_page_is_closed_with_what_is_wrong_in_it() {
    let update = first(
        "return { render = function() return bateri.div():bg('puce') end }",
        Limits::default(),
    );
    assert!(failed(&update).contains("\"puce\" is not the name of a tone"));
    let update = first(
        "return { render = function() local d = bateri.div(); d:child(d); return d end }",
        Limits::default(),
    );
    assert!(failed(&update).contains("nested more than"));
    let update = first(
        "return { render = function() return { kind = 'box', style = { width = 'wide' } } end }",
        Limits::default(),
    );
    assert!(failed(&update).contains("width"));
}

#[test]
fn a_script_that_never_returns_is_cut_off() {
    let limits = Limits {
        call: Duration::from_millis(200),
        ..Limits::default()
    };
    let began = Instant::now();
    let update = first(
        "return { render = function() while true do end end }",
        limits.clone(),
    );
    assert_eq!(update.closed, Some(Fault::Timeout));
    assert!(began.elapsed() < Duration::from_secs(10));
    // Also while it starts, and while it handles.
    let update = first("while true do end", limits.clone());
    assert_eq!(update.closed, Some(Fault::Timeout));
    let mut page = open_script(
        "return { render = function() return bateri.div() end, handle = function() local n = 0 repeat n += 1 until false end }",
        limits,
    );
    page.module.render(SIZE, 0.0, false);
    page.until(Duration::from_secs(20), |u| u.frame.is_some());
    page.module.event(Event::Input(Input::Key {
        key: Key::Char('x'),
        mods: Mods::default(),
    }));
    let update = page.until(Duration::from_secs(10), |u| u.closed.is_some());
    assert_eq!(update.closed, Some(Fault::Timeout));
}

#[test]
fn a_script_that_takes_memory_without_end_is_refused_it() {
    let limits = Limits {
        memory: 24 << 20,
        ..Limits::default()
    };
    let update = first(
        "local held = {}\nreturn { render = function() while true do held[#held + 1] = string.rep('y', 100000) .. #held end end }",
        limits,
    );
    assert_eq!(update.closed, Some(Fault::Memory));
}

#[test]
fn a_script_can_reach_nothing_of_the_system() {
    let update = first(
        r#"
        local names = { "io", "os", "debug", "require", "loadstring", "load", "dofile", "loadfile",
            "print", "getfenv", "setfenv", "newproxy", "collectgarbage", "gcinfo", "coroutine",
            "package", "bit32", "buffer", "vector", "utf8", "string", "table", "math" }
        local found = {}
        for _, name in ipairs(names) do
            if rawget(_G, name) ~= nil or _G[name] ~= nil then found[#found + 1] = name end
        end
        local more = {}
        for _, name in ipairs({ "string.dump", "math.randomseed" }) do
            local lib, fn = name:match("(%w+)%.(%w+)")
            if _G[lib][fn] ~= nil then more[#more + 1] = name end
        end
        return { render = function()
            return bateri.div():child(bateri.text(table.concat(found, " "))):child(bateri.text(table.concat(more, " ")))
        end }
        "#,
        Limits::default(),
    );
    assert_eq!(update.closed, None, "{:?}", update.closed);
    let frame = update.frame.expect("a page");
    // Only the pure libraries are there, and none of their ways to reach out of the sandbox.
    assert_eq!(
        words(&frame.root),
        ["utf8 string table math", "math.randomseed"]
    );
}

#[test]
fn a_script_cannot_change_what_every_script_shares() {
    let update = first(
        r#"
        local ok1 = pcall(function() string.sub = nil end)
        local ok2 = pcall(function() bateri.div = nil end)
        local ok3 = pcall(function() _G.owned = 1 end)
        owned_by_me = 2
        return { render = function()
            return bateri.div():child(bateri.text(tostring(ok1) .. tostring(ok2) .. tostring(ok3) .. tostring(owned_by_me)))
        end }
        "#,
        Limits::default(),
    );
    assert_eq!(update.closed, None);
    // The libraries are read only; the script's own globals are its own.
    let text = words(&update.frame.expect("a page").root).join("");
    assert!(text.starts_with("falsefalse"), "{text}");
    assert!(text.ends_with('2'), "{text}");
}

#[test]
fn json_is_written_and_read_the_same_way_in_both_directions() {
    let update = first(
        r#"
        local json = bateri.json
        local text = json.encode({ b = { 1, 2.5, "x\n\"y\"" }, a = true, c = setmetatable({}, json.array), d = { z = 1 } })
        local back = json.decode(text)
        local bad, why = json.decode('{"a": [1, 2,')
        local uni = json.decode('"\\u00e9\\ud83d\\ude00"')
        return { render = function()
            return bateri.div():child(bateri.text(text)):child(bateri.text(tostring(back.b[3])))
                :child(bateri.text(tostring(bad) .. " " .. tostring(why))):child(bateri.text(uni))
        end }
        "#,
        Limits::default(),
    );
    assert_eq!(update.closed, None, "{:?}", update.closed);
    let said = words(&update.frame.expect("a page").root);
    assert_eq!(
        said[0],
        r#"{"a":true,"b":[1,2.5,"x\n\"y\""],"c":[],"d":{"z":1}}"#
    );
    assert_eq!(said[1], "x\n\"y\"");
    assert!(said[2].starts_with("nil "), "{}", said[2]);
    assert_eq!(said[3], "é😀");
}

/// A program that has room for so many of what the page says.
struct Room {
    left: AtomicUsize,
    nothing_left: AtomicBool,
}

impl Program for Room {
    fn may_say(&self, _bytes: usize) -> bool {
        let had = self.left.load(Ordering::SeqCst);
        if had == 0 {
            self.nothing_left.store(true, Ordering::SeqCst);
            return false;
        }
        self.left.store(had - 1, Ordering::SeqCst);
        true
    }

    fn taken(&self, _bytes: usize) {}
}

#[test]
fn a_page_that_asks_its_program_things_is_answered_and_pushed_to() {
    let mut page = open_script(
        r#"
        local seen = "nothing yet"
        local answered = "no answer"
        bateri.subscribe("stats", nil, function(value) seen = "push " .. value.n; return true end)
        bateri.call("sum", { a = 1, b = 2 }, function(result, err)
            answered = err and ("error " .. err.code) or ("sum " .. result.total)
            return true
        end)
        return { render = function()
            return bateri.div():child(bateri.text(seen)):child(bateri.text(answered))
        end }
        "#,
        Limits::default(),
    );
    page.module.render(SIZE, 0.0, false);
    page.until(Duration::from_secs(20), |u| {
        u.frame.is_some() || u.closed.is_some()
    });
    // What the script asked at its start was said, in order, as the six messages are written.
    assert_eq!(
        page.seen.sent,
        [
            r#"{"subscribe":{"id":1,"topic":"stats"}}"#,
            r#"{"call":{"id":2,"method":"sum","params":{"a":1,"b":2}}}"#,
        ]
    );
    page.module
        .message(r#"{"push":{"id":1,"value":{"n":7}}}"#.into());
    page.module
        .message(r#"{"response":{"id":2,"result":{"total":3}}}"#.into());
    // A message that changed the page makes the page again, by itself.
    until_seen(&mut page, Duration::from_secs(5), |seen| {
        seen.frame
            .as_ref()
            .is_some_and(|frame| words(&frame.root) == ["push 7", "sum 3"])
    });
    let said = words(&page.seen.frame.clone().expect("a page").root);
    assert_eq!(said, ["push 7", "sum 3"]);
    // What nobody asked for, and what is not understood, is let go.
    page.module
        .message(r#"{"push":{"id":9,"value":{"n":1}}}"#.into());
    page.module.message("not json at all".into());
    let update = page.until(Duration::from_millis(300), |u| u.closed.is_some());
    assert_eq!(update.closed, None);
}

#[test]
fn a_page_told_full_keeps_what_it_meant_to_say_until_it_is_called_with_room() {
    let program = Arc::new(Room {
        left: AtomicUsize::new(1),
        nothing_left: AtomicBool::new(false),
    });
    let mut page = open_script_for(
        r#"
        for n = 1, 3 do bateri.call("part", { n = n }) end
        return { render = function() return bateri.div() end,
            room = function() return true end }
        "#,
        Limits::default(),
        program.clone(),
    );
    page.module.render(SIZE, 0.0, false);
    page.until(Duration::from_secs(20), |u| u.frame.is_some());
    // The program took one; the page was told full for the second and keeps it and the third.
    assert_eq!(
        page.seen.sent,
        [r#"{"call":{"id":1,"method":"part","params":{"n":1}}}"#]
    );
    assert!(program.nothing_left.load(Ordering::SeqCst));
    program.left.store(10, Ordering::SeqCst);
    page.module.room();
    let update = page.until(Duration::from_secs(5), |u| !u.sent.is_empty());
    assert_eq!(update.closed, None);
    assert_eq!(
        page.seen.sent[1..],
        [
            r#"{"call":{"id":2,"method":"part","params":{"n":2}}}"#,
            r#"{"call":{"id":3,"method":"part","params":{"n":3}}}"#,
        ]
    );
}

#[test]
fn a_page_that_says_a_great_deal_is_not_closed_for_it() {
    // More than a call may say is said over several calls; the page asks to be drawn again
    // until it is all said.
    let mut page = open_script(
        "for n = 1, 100 do bateri.call('part', { n = n }) end\nreturn { render = function() return bateri.div() end }",
        Limits::default(),
    );
    let mut rounds = 0;
    while page.seen.sent.len() < 100 && rounds < 50 {
        rounds += 1;
        page.module.render(SIZE, 0.0, false);
        let update = page.until(Duration::from_secs(10), |u| {
            u.frame.is_some() || u.closed.is_some()
        });
        assert_eq!(update.closed, None);
    }
    assert_eq!(page.seen.sent.len(), 100);
    assert!(
        page.seen.frame.is_some_and(|f| !f.animating),
        "all said, so at rest"
    );
}

#[test]
fn what_a_script_hands_out_that_is_not_text_is_made_into_text() {
    // A string in Lua is any bytes; the application is only handed text.
    let update = first(
        "return { render = function() return bateri.div():child(bateri.text('a\\xffb\\xc3')) end }",
        Limits::default(),
    );
    assert_eq!(update.closed, None);
    assert_eq!(
        words(&update.frame.expect("a page").root),
        ["a\u{fffd}b\u{fffd}"]
    );
}

#[test]
fn the_compiled_interpreter_is_kept_and_the_next_page_starts_from_it() {
    // The first page of a process compiles the interpreter; the next reads what was left.
    let began = Instant::now();
    let _ = first(
        "return { render = function() return bateri.div() end }",
        Limits::default(),
    );
    let compiled = began.elapsed();
    let began = Instant::now();
    let update = first(
        "return { render = function() return bateri.div() end }",
        Limits::default(),
    );
    let kept = began.elapsed();
    eprintln!(
        "a page after the interpreter was compiled: {kept:?} (a page that may have compiled it: {compiled:?})"
    );
    assert_eq!(update.closed, None);
    assert!(kept < Duration::from_secs(5), "{kept:?}");
}

#[test]
fn errors_a_script_catches_are_not_kept() {
    // An error is an exception in the interpreter, an object on the runtime's collected heap:
    // a script that catches them by the hundred thousand does not run the heap out.
    // (What is under test is the heap and not the speed: a call may take as long as a loaded
    // machine makes it.)
    let limits = Limits {
        call: Duration::from_secs(10),
        ..Limits::default()
    };
    let mut page = open_script(
        "return { render = function() for n = 1, 5000 do pcall(error, 'again') end return bateri.div() end }",
        limits,
    );
    for step in 0..60 {
        page.module.render(SIZE, f64::from(step), false);
        let update = page.until(Duration::from_secs(20), |u| {
            u.frame.is_some() || u.closed.is_some()
        });
        assert_eq!(update.closed, None, "after {} errors", step * 5000);
    }
}

#[test]
fn the_example_page_counts_clicks() {
    let mut page = open_script(
        include_str!("../../../guest/luau/examples/hello.lua"),
        Limits::default(),
    );
    page.module.render(SIZE, 0.0, false);
    let update = page.until(Duration::from_secs(20), |u| {
        u.frame.is_some() || u.closed.is_some()
    });
    assert_eq!(update.closed, None);
    let frame = update.frame.expect("a page");
    assert_eq!(words(&frame.root), ["Clicked 0 times", "Click me"]);
    // Laid out, the button is where the page put it: a press and a release on it is a click.
    let laid = Arc::new(bt_view::layout(frame.root.clone(), SIZE, 1.0, &mut Rough));
    page.module.lay_out(Arc::clone(&laid));
    let button = laid
        .bounds(Id::named("counter button"))
        .expect("the button is laid out");
    let at = Point::new(button.x + button.w / 2.0, button.y + button.h / 2.0);
    for input in [
        Input::Move(at),
        Input::Press {
            at,
            button: Button::Left,
            mods: Mods::default(),
        },
        Input::Release {
            at,
            button: Button::Left,
        },
    ] {
        page.module.event(Event::Input(input));
    }
    until_seen(&mut page, Duration::from_secs(10), |seen| {
        seen.replies.len() >= 3
    });
    assert_eq!(page.seen.replies.len(), 3);
    // The pointer moving onto the button, and the click, change the page; the press does not.
    let redraws: Vec<bool> = page.seen.replies.iter().map(|a| a.reply.redraw).collect();
    assert_eq!(redraws, [true, false, true]);
    let frame = page.seen.frame.clone().expect("the page after the click");
    assert_eq!(words(&frame.root), ["Clicked 1 times", "Click me"]);
}
