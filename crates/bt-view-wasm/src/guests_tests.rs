//! The real pages (`guest/`, built by `make guest`) through the sandbox: what the tests in
//! `sandbox` and `module` take on faith. They need the toolchain that builds a module, which
//! `make check` does not; where the components are not built they say so and stand down.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use bt_view::{Event, Font, Input, Key, Measure, Metrics, Mods, Point, Size};
use wasmtime::component::Component;

use crate::{Fault, Limits, Module, Program, Unbounded, Update};

fn component(name: &str) -> Option<Vec<u8>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../guest/dist")
        .join(format!("{name}.wasm"));
    match std::fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(_) => {
            eprintln!(
                "skipped: {} is not built (`make guest` builds the page modules)",
                path.display()
            );
            None
        }
    }
}

/// Every line is as wide as it has characters: enough to lay a page out.
pub(crate) struct Rough;

impl Measure for Rough {
    fn measure(&mut self, font: &Font, text: &str) -> Metrics {
        Metrics {
            width: text.chars().count() as f32 * font.size * 0.6,
            height: font.size * 1.3,
        }
    }
}

pub(crate) struct Open {
    pub(crate) module: Module,
    pub(crate) woken: Receiver<()>,
    pub(crate) seen: Update,
}

fn open(name: &str, limits: Limits) -> Option<Open> {
    open_for(name, limits, Arc::new(Unbounded))
}

/// The module `name`, for `program`.
fn open_for(name: &str, limits: Limits, program: Arc<dyn Program>) -> Option<Open> {
    let bytes = component(name)?;
    let (told, woken) = channel();
    let told = std::sync::Mutex::new(told);
    let module = Module::from_component(bytes, limits, program, move || {
        let _ = told.lock().unwrap().send(());
    });
    Some(Open {
        module,
        woken,
        seen: Update::default(),
    })
}

impl Open {
    /// Takes what is waiting, waiting up to `within` for there to be something.
    pub(crate) fn take(&mut self, within: Duration) -> Update {
        let _ = self.woken.recv_timeout(within);
        let update = self.module.poll();
        if update.frame.is_some() {
            self.seen.frame = update.frame.clone();
        }
        self.seen.replies.extend(update.replies.iter().cloned());
        self.seen.sent.extend(update.sent.iter().cloned());
        update
    }

    /// Takes until `done` says so of what came, or `within` has passed.
    pub(crate) fn until(&mut self, within: Duration, done: impl Fn(&Update) -> bool) -> Update {
        let began = Instant::now();
        loop {
            let left = within.saturating_sub(began.elapsed());
            let update = self.take(left.max(Duration::from_millis(1)));
            if done(&update) || left.is_zero() {
                return update;
            }
        }
    }
}

pub(crate) const SIZE: Size = Size {
    w: 1000.0,
    h: 700.0,
};

#[test]
fn the_container_list_asks_for_nothing_the_page_interface_does_not_give() {
    let Some(bytes) = component("containers") else {
        return;
    };
    let engine = crate::sandbox::engine().expect("an engine");
    let component = Component::new(&engine, bytes).expect("a component");
    let imports: Vec<String> = component
        .component_type()
        .imports(&engine)
        .map(|(name, _)| name.to_owned())
        .collect();
    assert!(!imports.is_empty());
    assert!(
        imports.iter().all(|name| name.starts_with("bateri:view/")),
        "a page that imports more than its interface: {imports:?}"
    );
}

#[test]
fn the_container_list_is_drawn_laid_out_and_used_through_the_sandbox() {
    let Some(mut page) = open("containers", Limits::default()) else {
        return;
    };
    page.module.render(SIZE, 0.0, false);
    let update = page.until(Duration::from_secs(20), |u| {
        u.frame.is_some() || u.closed.is_some()
    });
    assert_eq!(update.closed, None);
    let frame = update.frame.expect("the first page");
    assert!(!frame.animating);
    // The page that crossed the border is, element for element and field for field, the page
    // the same code makes without the sandbox.
    let mut native = bt_view::demo::Containers::fixture();
    native.advance(0.0, false);
    assert!(
        frame.root == native.view(SIZE),
        "the module's page differs from the engine's own"
    );
    let laid = Arc::new(bt_view::layout(frame.root.clone(), SIZE, 1.0, &mut Rough));
    assert!(laid.len() > 100, "{} elements", laid.len());
    native.settle(&*laid);

    // The module asks of the layout it was given: the filter's box is where the page put it.
    page.module.lay_out(Arc::clone(&laid));
    let field = laid
        .text_field()
        .expect("the page leaves room for a text field");

    // Down, down: the cursor moves, so the page changes and comes back by itself.
    for _ in 0..2 {
        page.module.event(Event::Input(Input::Key {
            key: Key::Down,
            mods: Mods::default(),
        }));
    }
    let update = page.until(Duration::from_secs(5), |u| {
        u.replies.len() >= 2 || u.closed.is_some()
    });
    assert_eq!(update.closed, None);
    // The highlight moves with time: the page says it is moving, and the application asks for
    // the next frame the way it does for any page that is (the clock advances by a few frames).
    let moving = page.seen.frame.clone().expect("the page after the keys");
    assert!(moving.animating, "a highlight on its way is a moving page");
    let mut steps = 0;
    for step in 1..=20 {
        steps = step;
        page.module.render(SIZE, f64::from(step) * 0.05, false);
        page.until(Duration::from_secs(5), |u| {
            u.frame.is_some() || u.closed.is_some()
        });
        if page.seen.frame.as_ref().is_some_and(|f| !f.animating) {
            break;
        }
    }
    let moved = page.seen.frame.clone().expect("the page after the keys");
    assert!(!moved.animating, "the highlight came to rest");
    assert!(moved.root != frame.root, "two keys down changed nothing");
    // The same two keys and the same steps of time on the page that is not in a sandbox end in
    // the same page.
    let down = Input::Key {
        key: Key::Down,
        mods: Mods::default(),
    };
    // (The module makes the page again for the time it knew, right after an event that changed it.)
    for _ in 0..2 {
        native.input(down, &*laid);
        native.advance(0.0, false);
    }
    for step in 1..=steps {
        native.advance(f64::from(step) * 0.05, false);
    }
    assert!(
        moved.root == native.view(SIZE),
        "after two keys the module's page differs from the engine's own"
    );
    assert!(page.seen.replies.iter().all(|a| a.reply.redraw));

    // A click on the filter's room asks for the keyboard to go to the host's text field.
    let inside = Point::new(field.x + field.w / 2.0, field.y + field.h / 2.0);
    for input in [
        Input::Press {
            at: inside,
            button: bt_view::Button::Left,
            mods: Mods::default(),
        },
        Input::Release {
            at: inside,
            button: bt_view::Button::Left,
        },
    ] {
        page.module.event(Event::Input(input));
    }
    page.until(Duration::from_secs(5), |u| {
        u.replies.iter().any(|a| a.reply.keyboard.is_some()) || u.closed.is_some()
    });
    assert!(
        page.seen
            .replies
            .iter()
            .any(|a| a.reply.keyboard == Some(bt_view::Keyboard::Filter)),
        "{:?}",
        page.seen.replies
    );

    // The text typed there narrows the list.
    let before = page.seen.frame.clone().expect("a page").root.fold().len();
    page.module.event(Event::Text("api".into()));
    page.until(Duration::from_secs(5), |u| u.frame.is_some());
    let after = page.seen.frame.clone().expect("a page").root.fold().len();
    assert!(
        after < before,
        "{after} elements after the filter, {before} before"
    );
}

/// Opens the page that goes wrong, presses `letter` and returns why it was closed.
fn broken(letter: char, limits: Limits) -> Option<(Fault, Duration)> {
    let mut page = open("broken", limits)?;
    page.module.render(SIZE, 0.0, false);
    page.until(Duration::from_secs(20), |u| {
        u.frame.is_some() || u.closed.is_some()
    });
    assert_eq!(page.module.poll().closed, None);
    let began = Instant::now();
    page.module.event(Event::Input(Input::Key {
        key: Key::Char(letter),
        mods: Mods::default(),
    }));
    let update = page.until(Duration::from_secs(20), |u| u.closed.is_some());
    Some((update.closed.expect("the page was closed"), began.elapsed()))
}

#[test]
fn a_page_that_never_returns_is_cut_off_and_the_application_hears_why() {
    let limits = Limits {
        call: Duration::from_millis(200),
        ..Limits::default()
    };
    let Some((fault, took)) = broken('l', limits) else {
        return;
    };
    assert_eq!(fault, Fault::Timeout);
    assert!(took < Duration::from_secs(5), "{took:?}");
}

#[test]
fn a_page_that_takes_memory_without_end_is_refused_it() {
    let limits = Limits {
        memory: 16 << 20,
        ..Limits::default()
    };
    let Some((fault, _)) = broken('m', limits) else {
        return;
    };
    assert_eq!(fault, Fault::Memory);
}

#[test]
fn a_page_that_panics_says_why_and_one_that_runs_out_of_stack_is_closed_with_the_trap() {
    let Some((fault, _)) = broken('t', Limits::default()) else {
        return;
    };
    assert!(
        matches!(&fault, Fault::Failed(why) if why.contains("this page trapped on purpose")),
        "{fault:?}"
    );
    let Some((fault, _)) = broken('s', Limits::default()) else {
        return;
    };
    assert!(matches!(fault, Fault::Trap(_)), "{fault:?}");
}

#[test]
fn the_container_list_asks_its_program_for_readings_and_shows_what_it_pushes() {
    let Some(mut page) = open("containers", Limits::default()) else {
        return;
    };
    page.module.render(SIZE, 0.0, false);
    let update = page.until(Duration::from_secs(20), |u| {
        u.frame.is_some() || u.closed.is_some()
    });
    assert_eq!(update.closed, None);
    // At its start the page asked the program (if there is one) to be told the numbers.
    assert_eq!(
        page.seen.sent,
        [r#"{"subscribe":{"id":1,"topic":"stats"}}"#.to_owned()]
    );

    // The program pushes a reading; the page that comes is the engine's own page with it.
    let push = r#"{"push":{"id":1,"value":{"readings":[{"name":"api","cpu":42.5,"memory":"130MiB","share":0.3}]}}}"#;
    page.module.message(push.to_owned());
    let update = page.until(Duration::from_secs(20), |u| {
        u.frame.is_some() || u.closed.is_some()
    });
    assert_eq!(update.closed, None);
    let frame = update.frame.expect("a page for the push");
    let mut native = bt_view::demo::Containers::fixture();
    native.advance(0.0, false);
    assert!(native.live(&[bt_view::demo::Reading {
        name: "api".into(),
        cpu: 42.5,
        memory: "130MiB".into(),
        share: 0.3,
    }]));
    assert!(
        frame.root == native.view(SIZE),
        "the module's page differs from the engine's with the same reading"
    );

    // A push for another subscription, a response, and what is not a message change nothing and
    // make no page.
    for noise in [
        r#"{"push":{"id":7,"value":{"readings":[]}}}"#,
        r#"{"response":{"id":1,"result":{}}}"#,
        "[]",
    ] {
        page.module.message(noise.to_owned());
    }
    let update = page.take(Duration::from_millis(500));
    assert_eq!(update.closed, None);
    assert!(update.frame.is_none(), "noise made a page");
    assert_eq!(page.seen.sent.len(), 1, "and it said nothing back");
}

#[test]
fn a_component_over_the_size_the_application_runs_is_not_compiled() {
    let Some(bytes) = component("containers") else {
        return;
    };
    let limits = Limits {
        component: bytes.len() - 1,
        ..Limits::default()
    };
    let (told, woken) = channel();
    let told = std::sync::Mutex::new(told);
    let module = Module::from_component(bytes, limits, Arc::new(Unbounded), move || {
        let _ = told.lock().unwrap().send(());
    });
    assert!(woken.recv_timeout(Duration::from_secs(20)).is_ok());
    match module.poll().closed {
        Some(Fault::Load(why)) => assert!(why.contains("bytes"), "{why}"),
        other => panic!("expected a refusal to load, got {other:?}"),
    }
}

/// A program that has room for so many messages, and counts the bytes the module has taken of
/// what it said.
struct Room {
    left: std::sync::atomic::AtomicUsize,
    taken: std::sync::atomic::AtomicUsize,
}

impl Room {
    fn with(messages: usize) -> Arc<Self> {
        Arc::new(Self {
            left: std::sync::atomic::AtomicUsize::new(messages),
            taken: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    fn add(&self, messages: usize) {
        self.left
            .fetch_add(messages, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Program for Room {
    fn may_say(&self, _bytes: usize) -> bool {
        self.left
            .fetch_update(
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
                |left| left.checked_sub(1),
            )
            .is_ok()
    }

    fn taken(&self, bytes: usize) {
        self.taken
            .fetch_add(bytes, std::sync::atomic::Ordering::SeqCst);
    }
}

/// The numbers of the messages `chatty` has said, in the order it said them.
fn numbers(sent: &[String]) -> Vec<u64> {
    sent.iter()
        .map(|text| {
            text.split("\"n\":")
                .nth(1)
                .and_then(|rest| rest.split('}').next())
                .and_then(|n| n.parse().ok())
                .unwrap_or(0)
        })
        .collect()
}

/// A module that is told its program has no room is not closed: it keeps the message it was told
/// *full* to, says nothing more until it is called, and says that one first when it is. It is
/// called once for each time, and nothing is lost or said twice.
#[test]
fn a_module_told_full_keeps_what_it_meant_to_say_and_goes_on_when_it_is_called_for_room() {
    let program = Room::with(10);
    let Some(mut page) = open_for("chatty", Limits::default(), program.clone()) else {
        return;
    };
    // Called for room before it has said anything: it has nothing to go on with.
    page.module.room();
    page.until(Duration::from_millis(300), |_| false);
    assert!(page.seen.sent.is_empty());
    assert!(page.seen.closed.is_none());

    page.module.message("go".to_owned());
    // (The first message waits for the component to be compiled.)
    page.until(Duration::from_secs(30), |update| {
        !update.sent.is_empty() || update.closed.is_some()
    });
    page.until(Duration::from_millis(300), |_| false);
    assert_eq!(numbers(&page.seen.sent), (1..=10).collect::<Vec<_>>());
    assert_eq!(
        program.taken.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "the module took the two bytes of what the program said"
    );

    // Room for five: it goes on from the message it was told full to, not from the next.
    program.add(5);
    page.module.room();
    page.until(Duration::from_millis(500), |_| false);
    assert_eq!(numbers(&page.seen.sent), (1..=15).collect::<Vec<_>>());
    assert!(page.seen.closed.is_none());

    // Room for everything: one call says as many as a call of the page says, and no more until
    // it is called again — and it is not closed for it.
    program.add(1_000);
    page.module.room();
    page.until(Duration::from_millis(500), |_| false);
    assert_eq!(numbers(&page.seen.sent), (1..=45).collect::<Vec<_>>());
    page.module.room();
    page.until(Duration::from_millis(500), |_| false);
    assert_eq!(numbers(&page.seen.sent), (1..=75).collect::<Vec<_>>());
    assert!(page.seen.closed.is_none());
    assert!(page.module.poll().closed.is_none());
}

/// The data and the number a chunk of the container list's payload carries, and whether it is the
/// last.
fn chunk_of(text: &str) -> (u64, usize, bool) {
    let after = |key: &str| text.split(key).nth(1).expect("a member");
    let seq = after(r#""seq":"#)
        .split([',', '}'])
        .next()
        .and_then(|n| n.parse().ok())
        .expect("a number");
    let data = after(r#""data":""#).split('"').next().expect("data").len();
    let last = after(r#""last":"#).starts_with("true");
    (seq, data, last)
}

/// Shift-U on the container list sends its program 4 MiB in chunks, as far as the program has the
/// room: it is told *full*, keeps the chunk, goes on when called for room, and in the end every
/// byte is said once, in order, the last chunk saying so.
#[test]
fn the_container_list_sends_its_payload_as_fast_as_its_program_has_room_and_loses_none() {
    let program = Room::with(1 /* the subscription */ + 3);
    let Some(mut page) = open_for("containers", Limits::default(), program.clone()) else {
        return;
    };
    page.module.render(SIZE, 0.0, false);
    page.until(Duration::from_secs(30), |u| {
        u.frame.is_some() || u.closed.is_some()
    });
    // A page nobody opened has no one to send the payload to; this one is told its program is
    // there by its first word.
    page.module
        .message(r#"{"push":{"id":1,"value":{"readings":[]}}}"#.to_owned());
    page.module.event(Event::Input(Input::Key {
        key: Key::Char('U'),
        mods: Mods::default(),
    }));
    page.until(Duration::from_millis(500), |_| false);
    let chunks = |sent: &[String]| -> Vec<(u64, usize, bool)> {
        sent.iter()
            .filter(|text| text.contains(r#""method":"upload""#))
            .map(|text| chunk_of(text))
            .collect()
    };
    assert_eq!(
        chunks(&page.seen.sent).len(),
        3,
        "three chunks, then told full"
    );

    // The program reads; the page is called for room and goes on, a few chunks in a call and the
    // next call being its next frame. Nothing is closed.
    let mut calls = 0;
    while chunks(&page.seen.sent).last().is_none_or(|last| !last.2) {
        calls += 1;
        assert!(calls < 200, "the payload never ended");
        program.add(1_000);
        page.module.room();
        page.module.render(SIZE, f64::from(calls) * 0.016, false);
        page.until(Duration::from_millis(100), |_| false);
        assert!(page.seen.closed.is_none(), "{:?}", page.seen.closed);
    }
    let all = chunks(&page.seen.sent);
    assert_eq!(
        all.iter().map(|c| c.0).collect::<Vec<_>>(),
        (1..=all.len() as u64).collect::<Vec<_>>()
    );
    assert_eq!(all.iter().map(|c| c.1).sum::<usize>(), 4 << 20);
    assert_eq!(all.iter().filter(|c| c.2).count(), 1);
}
