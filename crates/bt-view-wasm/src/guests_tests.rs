//! The real pages (`guest/`, built by `make guest`) through the sandbox: what the tests in
//! `sandbox` and `module` take on faith. They need the toolchain that builds a module, which
//! `make check` does not; where the components are not built they say so and stand down.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use bt_view::{Event, Font, Input, Key, Measure, Metrics, Mods, Point, Size};
use wasmtime::component::Component;

use crate::{Fault, Limits, Module, Update};

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
struct Rough;

impl Measure for Rough {
    fn measure(&mut self, font: &Font, text: &str) -> Metrics {
        Metrics {
            width: text.chars().count() as f32 * font.size * 0.6,
            height: font.size * 1.3,
        }
    }
}

struct Open {
    module: Module,
    woken: Receiver<()>,
    seen: Update,
}

fn open(name: &str, limits: Limits) -> Option<Open> {
    let bytes = component(name)?;
    let (told, woken) = channel();
    let told = std::sync::Mutex::new(told);
    let module = Module::from_component(bytes, limits, move || {
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
    fn take(&mut self, within: Duration) -> Update {
        let _ = self.woken.recv_timeout(within);
        let update = self.module.poll();
        if update.frame.is_some() {
            self.seen.frame = update.frame.clone();
        }
        self.seen.replies.extend(update.replies.iter().cloned());
        update
    }

    /// Takes until `done` says so of what came, or `within` has passed.
    fn until(&mut self, within: Duration, done: impl Fn(&Update) -> bool) -> Update {
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

const SIZE: Size = Size {
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
fn a_page_that_traps_or_runs_out_of_stack_is_closed_with_the_trap() {
    for letter in ['t', 's'] {
        let Some((fault, _)) = broken(letter, Limits::default()) else {
            return;
        };
        assert!(matches!(fault, Fault::Trap(_)), "{letter}: {fault:?}");
    }
}
