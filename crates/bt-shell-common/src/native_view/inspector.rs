//! The page inspector's answerer: what stands where a program would for the inspector's own page.
//!
//! The inspector is a page of the kit (`inspector.lua`) in a pane of its own, and it asks what it
//! shows as a page asks its program: `pencere.call` and `pencere.subscribe`, the six messages of
//! the protocol. No program is on the other end: the pane gives what the page says to the
//! [`Responder`] (what `Host::send` is handed) and tells the page what the responder answers. So
//! the inspector is a page like any other to the pane and to the kit, and the kit is tried by the
//! tool that is meant to look at it.
//!
//! The responder is a table of calls of the drive's own ([`Calls`]: it reads and answers the
//! messages as the program library does) with the topics and methods the inspector has. What it
//! reads of the pane it looks at comes from a [`Subject`], which the platform's pane gives.
//!
//! - `target` (topic): `{ pane, page, gone }`, pushed when the page subscribes and whenever
//!   what it says changes ([`Responder::refresh`]);
//! - `close` (call): the page asks to be closed ([`Responder::closing`]).
//!
//! What it says is no program's: it never reaches a terminal or a wire, and what it says of the
//! pane looked at is a name and two booleans.

use std::cell::Cell;
use std::rc::Rc;

use pencere_drive::headless::{Calls, Failure, Peer, Value};

use super::{Opening, PageHandle, script_page};
use pencere_sandbox::{ModuleLimits, Script};

/// The page's script.
pub const SCRIPT: &str = include_str!("inspector.lua");

/// The topic the page hears what it looks at by.
const TARGET: &str = "target";

/// The call that asks the pane to close.
const CLOSE: &str = "close";

/// What the inspector reads of the pane it looks at, as it is now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// The pane's name, as the person knows it.
    pub name: String,
    /// The pane shows a page of the kit.
    pub page: bool,
    /// The pane is gone: it is closed, or closing.
    pub gone: bool,
}

/// The pane the inspector looks at, read when the responder is asked and no other time.
pub trait Subject {
    /// What the pane is now.
    fn target(&self) -> Target;
}

impl<F: Fn() -> Target> Subject for F {
    fn target(&self) -> Target {
        self()
    }
}

/// The inspector's page, for a pane to draw (the kit's page of the script) and drive by its
/// handle: a page like a program's, with no program to hold it to a flow.
pub fn inspector_page(
    opening: Opening,
    wake: impl Fn() + Send + Sync + 'static,
) -> (Box<dyn bt_gpu::UiPage>, PageHandle) {
    script_page(
        Script {
            path: "inspector.lua".to_owned(),
            source: SCRIPT.as_bytes().to_vec(),
            bundle: None,
        },
        ModuleLimits::default(),
        opening,
        wake,
    )
}

/// What the page says is answered here, and what the page is to be told is said here.
pub struct Responder {
    calls: Calls,
    subject: Rc<dyn Subject>,
    /// The page asked to be closed.
    closing: Rc<Cell<bool>>,
    /// What the live subscriptions of the target were last told.
    told: Option<Target>,
    /// How many of the page's subscriptions have been looked at.
    seen: usize,
}

impl Responder {
    /// A responder for the page of the inspector of `subject`.
    pub fn new(subject: Rc<dyn Subject>) -> Self {
        let closing = Rc::new(Cell::new(false));
        let asked = Rc::clone(&closing);
        let calls = Calls::new().on(CLOSE, move |_| {
            asked.set(true);
            Ok(Value::Object(Default::default()))
        });
        Self {
            calls,
            subject,
            closing,
            told: None,
            seen: 0,
        }
    }

    /// The page said `message` to its program: what the responder says back, in order.
    pub fn hear(&mut self, message: &str) -> Vec<String> {
        self.calls.heard(message);
        self.refuse_unknown_calls();
        self.answer_new_subscriptions();
        self.calls.said()
    }

    /// What the pane looked at is may have changed: the page is told it if it is not what it was
    /// told last, as the pane's page comes and goes. Nothing where it is the same, or where the
    /// page hears no `target`.
    pub fn refresh(&mut self) -> Vec<String> {
        let now = self.subject.target();
        if self.told.as_ref() != Some(&now) {
            let value = value_of(&now);
            let ids: Vec<u64> = self
                .calls
                .subscriptions()
                .iter()
                .filter(|subscription| subscription.live && subscription.topic == TARGET)
                .map(|subscription| subscription.id)
                .collect();
            for id in ids {
                self.calls.say(&push(id, &value));
            }
            self.told = Some(now);
        }
        self.calls.said()
    }

    /// The page asked for its pane to be closed.
    pub fn closing(&self) -> bool {
        self.closing.get()
    }

    /// A call the responder has no method for is refused as the protocol has it (`not-found`),
    /// not left waiting as a slow program's would be.
    fn refuse_unknown_calls(&mut self) {
        while let Some(method) = self.calls.waiting().first().map(|call| call.method.clone()) {
            let refused = self.calls.answer(
                &method,
                Err(Failure {
                    code: "not-found".to_owned(),
                    message: "the inspector has no such method".to_owned(),
                }),
            );
            if !refused {
                break;
            }
        }
    }

    /// A subscription the page made since the last time: the target is told what it is now; any
    /// other topic is refused as the protocol has it.
    fn answer_new_subscriptions(&mut self) {
        let fresh: Vec<(u64, String, bool)> = self
            .calls
            .subscriptions()
            .iter()
            .skip(self.seen)
            .map(|subscription| {
                (
                    subscription.id,
                    subscription.topic.clone(),
                    subscription.live,
                )
            })
            .collect();
        self.seen += fresh.len();
        for (id, topic, live) in fresh {
            if topic != TARGET {
                self.calls.say(&refusal(id));
            } else if live {
                let now = self.subject.target();
                self.calls.say(&push(id, &value_of(&now)));
                self.told = Some(now);
            }
        }
    }
}

/// What the page is told of the pane it looks at.
fn value_of(target: &Target) -> Value {
    [
        ("pane", Value::from(target.name.as_str())),
        ("page", Value::from(target.page)),
        ("gone", Value::from(target.gone)),
    ]
    .into_iter()
    .collect()
}

/// A value of the subscription `id`.
fn push(id: u64, value: &Value) -> String {
    format!(r#"{{"push":{{"id":{id},"value":{value}}}}}"#)
}

/// The end of the subscription `id` that the responder has no topic for.
fn refusal(id: u64) -> String {
    format!(
        r#"{{"response":{{"id":{id},"error":{{"code":"not-found","message":"the inspector has no such topic"}}}}}}"#
    )
}

/// What the responder said, told to the page, in order.
pub fn tell(handle: &PageHandle, replies: &[String]) {
    for reply in replies {
        handle.page().message(reply);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_view::pane_tests::PaneRig;
    use pencere_view::{Key, Mods};
    use std::cell::RefCell;

    fn target(page: bool) -> Target {
        Target {
            name: "p3".to_owned(),
            page,
            gone: false,
        }
    }

    /// The inspector's page in a rig, the responder that answers it, and the pane it looks at,
    /// which a scenario changes.
    struct Bench {
        rig: PaneRig,
        responder: Responder,
        subject: Rc<RefCell<Target>>,
    }

    impl Bench {
        fn open() -> Self {
            let subject = Rc::new(RefCell::new(target(true)));
            let read = Rc::clone(&subject);
            let responder = Responder::new(Rc::new(move || read.borrow().clone()));
            let mut bench = Self {
                rig: PaneRig::open(SCRIPT),
                responder,
                subject,
            };
            bench.rig.frame(0.0);
            bench.settle();
            bench
        }

        /// The page said what it said and the responder answered, until neither has more.
        fn settle(&mut self) {
            for _ in 0..20 {
                self.rig.settle(0.0);
                let said: Vec<String> = self
                    .rig
                    .lines()
                    .into_iter()
                    .filter_map(|line| line.strip_prefix("send ").map(str::to_owned))
                    .collect();
                if said.is_empty() {
                    return;
                }
                for message in said {
                    let replies = self.responder.hear(&message);
                    tell(self.rig.handle(), &replies);
                }
            }
            panic!("the page and the responder never came to rest");
        }

        /// The pane looked at is as `target` now; the page is told it if it changed.
        fn becomes(&mut self, target: Target) {
            *self.subject.borrow_mut() = target;
            let replies = self.responder.refresh();
            tell(self.rig.handle(), &replies);
            self.settle();
        }

        fn says(&self, words: &str) -> bool {
            self.rig.words().iter().any(|line| line == words)
        }
    }

    /// The page is the kit's own: it requires `@pencere/ui` and `@pencere/query`, which a script of
    /// no bundle can reach, and it is drawn by a host with no screen.
    #[test]
    fn the_page_requires_the_kits_library_and_is_drawn_headless() {
        let bench = Bench::open();
        assert!(bench.says("Page inspector"), "{:?}", bench.rig.words());
    }

    /// The page hears the pane it looks at the moment it subscribes, and again whenever the
    /// pane's page goes and comes back.
    #[test]
    fn the_page_is_told_the_target_first_and_again_when_its_page_goes_and_comes_back() {
        let mut bench = Bench::open();
        assert!(
            bench.says("Reading the page of p3."),
            "{:?}",
            bench.rig.words()
        );
        bench.becomes(target(false));
        assert!(bench.says("No page in p3."), "{:?}", bench.rig.words());
        bench.becomes(target(true));
        assert!(
            bench.says("Reading the page of p3."),
            "{:?}",
            bench.rig.words()
        );
        bench.becomes(Target {
            name: "p3".to_owned(),
            page: false,
            gone: true,
        });
        assert!(bench.says("The pane is gone."), "{:?}", bench.rig.words());
    }

    /// What has not changed is not told again: the page asks no frame for it.
    #[test]
    fn what_is_the_same_is_not_told_again() {
        let mut bench = Bench::open();
        assert!(bench.responder.refresh().is_empty());
        bench.becomes(target(false));
        assert!(bench.responder.refresh().is_empty());
    }

    /// Esc is the page's key to close itself: a call the responder takes as the pane's close.
    #[test]
    fn escape_asks_the_pane_to_close() {
        let mut bench = Bench::open();
        assert!(!bench.responder.closing());
        bench.rig.key(Key::Escape, Mods::default());
        bench.settle();
        assert!(bench.responder.closing());
    }

    /// A method and a topic the responder has none of are refused as the protocol has it, not
    /// left waiting.
    #[test]
    fn a_method_and_a_topic_it_has_none_of_are_refused() {
        let subject = Rc::new(|| target(true));
        let mut responder = Responder::new(subject);
        let replies = responder.hear(r#"{"call":{"id":4,"method":"nothing"}}"#);
        assert_eq!(
            replies,
            [
                r#"{"response":{"id":4,"error":{"code":"not-found","message":"the inspector has no such method"}}}"#
            ]
        );
        let replies = responder.hear(r#"{"subscribe":{"id":5,"topic":"nothing"}}"#);
        assert_eq!(
            replies,
            [
                r#"{"response":{"id":5,"error":{"code":"not-found","message":"the inspector has no such topic"}}}"#
            ]
        );
        assert!(!responder.closing());
    }

    /// A page that stops hearing the target is told nothing more; one that hears it twice is told
    /// twice.
    #[test]
    fn a_subscription_that_was_cancelled_is_told_nothing() {
        let current = Rc::new(RefCell::new(target(true)));
        let read = Rc::clone(&current);
        let mut responder = Responder::new(Rc::new(move || read.borrow().clone()));
        let first = responder.hear(r#"{"subscribe":{"id":1,"topic":"target"}}"#);
        assert_eq!(
            first,
            [r#"{"push":{"id":1,"value":{"gone":false,"page":true,"pane":"p3"}}}"#]
        );
        let second = responder.hear(r#"{"subscribe":{"id":2,"topic":"target"}}"#);
        assert_eq!(second.len(), 1);
        responder.hear(r#"{"cancel":{"id":1}}"#);
        *current.borrow_mut() = target(false);
        let changed = responder.refresh();
        assert_eq!(changed.len(), 1);
        assert!(changed[0].contains(r#""id":2"#), "{changed:?}");
    }
}
