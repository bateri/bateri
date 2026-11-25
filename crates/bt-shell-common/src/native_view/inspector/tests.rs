//! The inspector's page and its responder, the target's page beside them, each in a rig with no
//! window: what the page says is answered by the responder as the pane does, and the pane's
//! part (the box shown over the target, the target's frames) is a stand-in that writes down what
//! it was told.

use super::*;
use crate::native_view::pane_tests::PaneRig;
use crate::native_view::{Button, Input};
use pencere_view::{Id, Key, Mods};
use std::cell::RefCell;

/// A page of three named boxes (`a`, `b`, `c`), a box with no name after them, and a key (`r`)
/// that turns the order of the named ones round: the page is made again and the boxes change places.
const TARGET_PAGE: &str = r#"local order = { "a", "b", "c" }
return {
    render = function()
        local root = pencere.div():size_full()
        for _, name in order do
            root:child(pencere.div():id(name):h(20):child(pencere.text(name)))
        end
        root:child(pencere.div():h(10))
        return root
    end,
    handle = function(event)
        if event.type == "key" and event.key == "r" then
            order = { order[3], order[2], order[1] }
            return true
        end
    end,
}"#;

/// The pane the inspector looks at, as the pane's own side tells it: the name, the page it
/// shows if it shows one, and the boxes it was asked to show over it.
struct Stand {
    name: RefCell<String>,
    gone: Cell<bool>,
    page: RefCell<Option<PageHandle>>,
    lit: RefCell<Vec<Option<Rect>>>,
}

impl Stand {
    fn new(page: Option<PageHandle>) -> Rc<Self> {
        Rc::new(Self {
            name: RefCell::new("p3".to_owned()),
            gone: Cell::new(false),
            page: RefCell::new(page),
            lit: RefCell::new(Vec::new()),
        })
    }

    /// The last box shown over the pane.
    fn box_now(&self) -> Option<Rect> {
        self.lit.borrow().last().copied().flatten()
    }
}

impl Subject for Stand {
    fn target(&self) -> Target {
        Target {
            name: self.name.borrow().clone(),
            page: self.page.borrow().is_some(),
            gone: self.gone.get(),
        }
    }

    fn page(&self) -> Option<PageHandle> {
        self.page.borrow().clone()
    }

    fn highlight(&self, rect: Option<Rect>) {
        self.lit.borrow_mut().push(rect);
    }
}

/// What a response of the responder carries, or the value of a push.
fn value(message: &str) -> Value {
    message.parse().expect("the responder's words are JSON")
}

/// The result of the response to the call `id`, among `replies`.
fn result_of(replies: &[String], id: u64) -> Value {
    replies
        .iter()
        .map(|reply| value(reply))
        .find_map(|reply| {
            let response = reply.get("response")?;
            (response.get("id")?.as_u64() == Some(id))
                .then(|| response.get("result").cloned().unwrap_or(Value::Null))
        })
        .unwrap_or_else(|| panic!("no response to {id} in {replies:?}"))
}

fn call(id: u64, method: &str, path: Option<&[usize]>) -> String {
    match path {
        Some(path) => {
            let path: Vec<String> = path.iter().map(usize::to_string).collect();
            format!(
                r#"{{"call":{{"id":{id},"method":"{method}","params":{{"path":[{}]}}}}}}"#,
                path.join(",")
            )
        }
        None => format!(r#"{{"call":{{"id":{id},"method":"{method}"}}}}"#),
    }
}

/// A target page drawn once, as the pane draws it.
fn target_rig() -> PaneRig {
    let mut rig = PaneRig::open(TARGET_PAGE);
    rig.frame(0.0);
    rig.settle(0.0);
    rig
}

fn names(children: &Value) -> Vec<Option<String>> {
    children["children"]
        .as_array()
        .expect("a list of children")
        .iter()
        .map(|child| child["id"].as_str().map(str::to_owned))
        .collect()
}

fn hex(name: &str) -> String {
    format!("{:x}", Id::named(name).number())
}

/// The inspector's page in a rig, the responder that answers it, and the stand-in for the pane it
/// looks at, which a scenario changes.
struct Bench {
    rig: PaneRig,
    responder: Responder,
    stand: Rc<Stand>,
    /// How many `children` calls the page made.
    asked: usize,
    /// What the page said, in order.
    heard: Vec<String>,
    /// The time of the frames: it goes on where a scenario lets it.
    now: f64,
}

impl Bench {
    fn open() -> Self {
        Self::over(Stand::new(None))
    }

    fn over(stand: Rc<Stand>) -> Self {
        let responder = Responder::new(Rc::clone(&stand) as Rc<dyn Subject>);
        let mut bench = Self {
            rig: PaneRig::open(SCRIPT),
            responder,
            stand,
            asked: 0,
            heard: Vec::new(),
            now: 0.0,
        };
        bench.rig.frame(0.0);
        bench.settle();
        bench
    }

    /// The page said what it said and the responder answered, until neither has more.
    fn settle(&mut self) {
        for _ in 0..20 {
            self.rig.settle(self.now);
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
                if message.contains(r#""method":"children""#) {
                    self.asked += 1;
                }
                self.heard.push(message.clone());
                let replies = self.responder.hear(&message);
                tell(self.rig.handle(), &replies);
            }
        }
        panic!("the page and the responder never came to rest");
    }

    /// What the responder said is told to the page, and the page settles.
    fn told(&mut self, replies: &[String]) {
        tell(self.rig.handle(), replies);
        self.settle();
    }

    /// The pane looked at is as `target` now; the page is told it if it changed.
    fn becomes(&mut self, page: bool, gone: bool) {
        self.stand.gone.set(gone);
        if !page {
            self.stand.page.replace(None);
        }
        let replies = self.responder.refresh();
        self.told(&replies);
    }

    fn says(&self, words: &str) -> bool {
        self.rig.words().iter().any(|line| line == words)
    }

    /// The person presses the element named `name`, and lets go.
    fn click(&mut self, name: &str) {
        let at = self
            .rig
            .handle()
            .page()
            .reach(Id::named(name))
            .unwrap_or_else(|why| panic!("{name} is not reached: {why:?}"));
        for input in [
            Input::Press {
                at,
                button: Button::Left,
                mods: Mods::default(),
                count: 1,
            },
            Input::Release {
                at,
                button: Button::Left,
                mods: Mods::default(),
            },
        ] {
            self.rig.input(input);
        }
        self.settle();
    }

    /// The calls of `method` the page made, as their params say.
    fn calls_of(&self, method: &str) -> Vec<Value> {
        self.heard
            .iter()
            .map(|message| value(message))
            .filter_map(|message| {
                let call = message.get("call")?;
                (call["method"] == method).then(|| call["params"].clone())
            })
            .collect()
    }
}

/// The page is the kit's own: it requires `@pencere/ui` and `@pencere/query`, which a script of
/// no bundle can reach, and it is drawn by a host with no screen.
#[test]
fn the_page_requires_the_kits_library_and_is_drawn_headless() {
    let bench = Bench::open();
    assert!(bench.says("Page inspector"), "{:?}", bench.rig.words());
}

/// The page hears the pane it looks at the moment it subscribes, and again whenever the pane's
/// page goes and comes back.
#[test]
fn the_page_is_told_the_target_first_and_again_when_its_page_goes_and_comes_back() {
    let target = target_rig();
    let mut bench = Bench::over(Stand::new(Some(target.handle().clone())));
    assert!(bench.says("p3"), "{:?}", bench.rig.words());
    bench.becomes(false, false);
    assert!(bench.says("No page in p3."), "{:?}", bench.rig.words());
    bench.stand.page.replace(Some(target.handle().clone()));
    let replies = bench.responder.refresh();
    bench.told(&replies);
    assert!(bench.says("p3"), "{:?}", bench.rig.words());
    bench.becomes(false, true);
    assert!(bench.says("The pane is gone."), "{:?}", bench.rig.words());
}

/// What has not changed is not told again: the page asks no frame for it.
#[test]
fn what_is_the_same_is_not_told_again() {
    let mut bench = Bench::open();
    assert!(bench.responder.refresh().is_empty());
    bench.becomes(false, false);
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

/// A method and a topic the responder has none of are refused as the protocol has it, not left
/// waiting.
#[test]
fn a_method_and_a_topic_it_has_none_of_are_refused() {
    let mut responder = Responder::new(Stand::new(None));
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
    let stand = Stand::new(None);
    let mut responder = Responder::new(Rc::clone(&stand) as Rc<dyn Subject>);
    let first = responder.hear(r#"{"subscribe":{"id":1,"topic":"target"}}"#);
    assert_eq!(
        first,
        [r#"{"push":{"id":1,"value":{"gone":false,"page":false,"pane":"p3","stamp":0}}}"#]
    );
    let second = responder.hear(r#"{"subscribe":{"id":2,"topic":"target"}}"#);
    assert_eq!(second.len(), 1);
    responder.hear(r#"{"cancel":{"id":1}}"#);
    stand.name.replace("p4".to_owned());
    let changed = responder.refresh();
    assert_eq!(changed.len(), 1);
    assert!(changed[0].contains(r#""id":2"#), "{changed:?}");
}

/// The elements of a page are asked a level at a time, and the ones the page never named are
/// among them.
#[test]
fn the_children_of_an_element_are_given_with_the_unnamed_ones() {
    let target = target_rig();
    let stand = Stand::new(Some(target.handle().clone()));
    let mut responder = Responder::new(stand);
    let replies = responder.hear(&call(1, "children", Some(&[])));
    let top = result_of(&replies, 1);
    assert_eq!(
        names(&top),
        [Some(hex("a")), Some(hex("b")), Some(hex("c")), None]
    );
    let first = &top["children"][0];
    assert_eq!(first["shape"], "box");
    // The text inside the first box is its one child, which is no box.
    assert_eq!(first["count"], 1);
    assert_eq!(first["path"], Value::Array(vec![Value::from(0)]));
    let below = result_of(&responder.hear(&call(2, "children", Some(&[0]))), 2);
    assert_eq!(below["children"][0]["shape"], "text");
    // No element is at that place: the page asked about a tree that has changed.
    let replies = responder.hear(&call(3, "children", Some(&[9])));
    assert!(replies[0].contains(r#""code":"not-found""#), "{replies:?}");
}

/// An element's box, its room inside, its border, how it is placed and what is said of it.
#[test]
fn a_node_gives_its_box_its_room_and_its_style() {
    let target = target_rig();
    let mut responder = Responder::new(Stand::new(Some(target.handle().clone())));
    let node = result_of(&responder.hear(&call(1, "node", Some(&[1]))), 1);
    assert_eq!(node["id"], hex("b"));
    assert_eq!(node["shape"], "box");
    // The rig puts the page under a strip of 32 points: the points are the pane's.
    assert_eq!(node["rect"]["h"], 20.0);
    assert_eq!(node["rect"]["y"], 52.0);
    assert_eq!(node["padding"], Value::Array(vec![Value::from(0.0); 4]));
    assert_eq!(node["border"], Value::Null);
    assert_eq!(node["placing"]["height"], "20px");
    assert_eq!(node["placing"]["display"], "Flex");
    assert_eq!(node["placing"]["direction"], "Column");
    assert_eq!(node["hidden"], false);
    assert!(node["stamp"].as_u64().is_some());
}

/// The target's page is measured while the inspector is open and let go of when it closes.
#[test]
fn the_target_is_measured_while_the_inspector_is_open() {
    let target = target_rig();
    assert!(!target.handle().page().inspecting());
    let mut responder = Responder::new(Stand::new(Some(target.handle().clone())));
    assert!(target.handle().page().inspecting());
    responder.end();
    assert!(!target.handle().page().inspecting());
    // And where it is dropped without being ended.
    let responder = Responder::new(Stand::new(Some(target.handle().clone())));
    assert!(target.handle().page().inspecting());
    drop(responder);
    assert!(!target.handle().page().inspecting());
}

/// A new page in the pane is measured too: the reading is of the one that is there.
#[test]
fn a_new_page_in_the_target_is_measured_when_the_pane_says_it_changed() {
    let old = target_rig();
    let stand = Stand::new(Some(old.handle().clone()));
    let mut responder = Responder::new(Rc::clone(&stand) as Rc<dyn Subject>);
    let new = target_rig();
    assert!(!new.handle().page().inspecting());
    stand.page.replace(Some(new.handle().clone()));
    responder.refresh();
    assert!(new.handle().page().inspecting());
}

/// What the page cost is told after the target drew a frame, never while it is idle.
#[test]
fn the_costs_come_after_a_frame_and_never_while_the_target_is_idle() {
    let mut target = target_rig();
    let mut responder = Responder::new(Stand::new(Some(target.handle().clone())));
    // Subscribing: what the target already cost is told with the subscription, if it has frames.
    responder.hear(r#"{"subscribe":{"id":7,"topic":"costs"}}"#);
    // The target is idle: nothing is told, however long it takes.
    assert!(responder.tick(10.0).is_empty());
    assert!(responder.poll().is_empty());
    assert!(responder.due().is_none());
    // The target made a page and drew it.
    target.key(Key::Character('r'), Mods::default());
    target.settle(1.0);
    let told = responder.drew(1.0);
    let costs: Vec<Value> = told
        .iter()
        .map(|reply| value(reply))
        .filter(|reply| reply["push"]["id"] == 7)
        .collect();
    assert_eq!(costs.len(), 1, "{told:?}");
    let costs = &costs[0]["push"]["value"];
    assert_eq!(costs["elements"], 8);
    assert!(
        !costs["frames"].as_array().expect("frames").is_empty(),
        "{costs}"
    );
    assert!(costs["frames"][0]["layout_ms"].is_number());
    // And not again until it draws.
    assert!(responder.tick(20.0).is_empty());
}

/// The stamps `told` carries to the subscription 1 of the target.
fn stamped(stamps: &mut Vec<u64>, told: Vec<String>) {
    for reply in told {
        let reply = value(&reply);
        if reply["push"]["id"] == 1
            && let Some(stamp) = reply["push"]["value"]["stamp"].as_u64()
        {
            stamps.push(stamp);
        }
    }
}

/// A target that makes a page on every frame is told of four times a second at most, and where it
/// comes to rest the last of what it did is told.
#[test]
fn a_target_that_draws_all_the_time_is_told_of_four_times_a_second_at_most() {
    let mut target = target_rig();
    let mut responder = Responder::new(Stand::new(Some(target.handle().clone())));
    responder.hear(r#"{"subscribe":{"id":1,"topic":"target"}}"#);
    let mut stamps = Vec::new();
    // One second at 60 frames a second, each making a page.
    for step in 0..60_u32 {
        let now = 1.0 + f64::from(step) / 60.0;
        target.key(Key::Character('r'), Mods::default());
        target.settle(now);
        stamped(&mut stamps, responder.drew(now));
        stamped(&mut stamps, responder.tick(now));
    }
    assert!(stamps.len() <= 4, "{stamps:?}");
    assert!(stamps.len() >= 3, "{stamps:?}");
    // It rests: the last frames are told when their time comes.
    let due = responder.due().expect("frames wait to be told");
    stamped(&mut stamps, responder.tick(due));
    assert_eq!(
        stamps.last().copied(),
        Some(responder.stamp()),
        "{stamps:?}"
    );
    // Each was another page of the target's.
    assert!(
        stamps.windows(2).all(|pair| pair[0] < pair[1]),
        "{stamps:?}"
    );
    assert!(responder.due().is_none());
}

/// The page asks for the root's children when the stamp changes and no other time: as many as
/// the stamps it was told of, not as many as the frames the target drew.
#[test]
fn the_page_asks_again_for_a_new_stamp_and_for_no_frame() {
    let mut target = target_rig();
    let stand = Stand::new(Some(target.handle().clone()));
    let mut bench = Bench::over(stand);
    let first = bench.asked;
    assert_eq!(first, 1, "the page asks once for the stamp it is told");
    let mut told = 0;
    for step in 0..60_u32 {
        let now = 1.0 + f64::from(step) / 60.0;
        target.key(Key::Character('r'), Mods::default());
        target.settle(now);
        let mut replies = bench.responder.drew(now);
        replies.extend(bench.responder.tick(now));
        told += replies
            .iter()
            .filter(|reply| reply.contains(r#""stamp""#) && reply.contains(r#""push""#))
            .count();
        bench.told(&replies);
    }
    assert!(bench.asked - first <= 4, "asked {}", bench.asked);
    assert!(
        bench.asked - first <= told,
        "{told} told, {} asked",
        bench.asked
    );
    // The page was told of a page made, so what it asks is of the target's page it has now.
    assert!(bench.says("p3"), "{:?}", bench.rig.words());
}

/// The element chosen has its box shown over the pane, and the box follows it when the page
/// is made again: by its name where it has one and by its place where it has none.
#[test]
fn the_chosen_element_is_followed_by_its_name_or_its_place() {
    let mut target = target_rig();
    let stand = Stand::new(Some(target.handle().clone()));
    let mut responder = Responder::new(Rc::clone(&stand) as Rc<dyn Subject>);
    // `a`, the first of three: at the top.
    let chosen = responder.hear(&call(1, "select", Some(&[0])));
    assert!(result_of(&chosen, 1).as_object().is_some());
    let top = stand.box_now().expect("a box over the pane");
    assert_eq!((top.y, top.h), (32.0, 20.0));
    // The order turns round: `a` is now the last of the three, and the box is where it is.
    target.key(Key::Character('r'), Mods::default());
    target.settle(1.0);
    responder.drew(1.0);
    let moved = stand.box_now().expect("a box over the pane");
    assert_eq!((moved.y, moved.h), (72.0, 20.0));
    let node = result_of(&responder.hear(&call(2, "node", Some(&[2]))), 2);
    assert_eq!(node["id"], hex("a"));
    // The unnamed box is the fourth, and follows its place: the page is made again and it is there.
    responder.hear(&call(3, "select", Some(&[3])));
    let unnamed = stand.box_now().expect("a box over the pane");
    assert_eq!((unnamed.y, unnamed.h), (92.0, 10.0));
    target.key(Key::Character('r'), Mods::default());
    target.settle(2.0);
    responder.drew(2.0);
    assert_eq!(stand.box_now(), Some(unnamed));
    // Nothing chosen, nothing shown; and nowhere to choose, no box.
    responder.hear(&call(4, "select", None));
    assert_eq!(stand.box_now(), None);
    let refused = responder.hear(&call(5, "select", Some(&[9, 9])));
    assert!(refused[0].contains(r#""code":"not-found""#), "{refused:?}");
}

/// The box is in the host's points: a page drawn at twice its size has a box twice as large.
#[test]
fn the_box_is_in_the_hosts_points_where_the_page_is_zoomed() {
    let mut target = target_rig();
    target.set_zoom(2.0);
    target.frame(1.0);
    target.settle(1.0);
    let stand = Stand::new(Some(target.handle().clone()));
    let mut responder = Responder::new(Rc::clone(&stand) as Rc<dyn Subject>);
    responder.hear(&call(1, "select", Some(&[1])));
    let shown = stand.box_now().expect("a box over the pane");
    assert_eq!((shown.y, shown.h), (72.0, 40.0));
    // What the element's box says is the same box.
    let node = result_of(&responder.hear(&call(2, "node", Some(&[1]))), 2);
    assert_eq!(node["rect"]["h"], 40.0);
    assert_eq!(node["rect"]["y"], 72.0);
}

/// What the page cut off is not shown: the box is where the element shows.
#[test]
fn only_what_shows_of_an_element_is_boxed() {
    let page = |rect: (f32, f32, f32, f32), clip: (f32, f32, f32, f32), hidden: bool| {
        let element = pencere_view::Inspected {
            path: Some(vec![0]),
            id: None,
            shape: pencere_view::Shape::Box,
            field: None,
            rect: pencere_view::Rect::new(rect.0, rect.1, rect.2, rect.3),
            clip: pencere_view::Rect::new(clip.0, clip.1, clip.2, clip.3),
            padding: [0.0; 4],
            border: None,
            scroll: (0.0, 0.0),
            content: None,
            hidden,
            overlay: false,
            exiting: false,
            parent: None,
            placing: None,
            accessible: Vec::new(),
        };
        shown_box(&element)
    };
    let whole = (0.0, 0.0, 100.0, 100.0);
    assert_eq!(
        page((10.0, 10.0, 20.0, 20.0), whole, false),
        Some(Rect::new(10.0, 10.0, 20.0, 20.0))
    );
    // Cut by a box that holds it.
    assert_eq!(
        page((10.0, 10.0, 200.0, 20.0), whole, false),
        Some(Rect::new(10.0, 10.0, 90.0, 20.0))
    );
    // Outside it, or not drawn: nothing to show.
    assert_eq!(page((150.0, 10.0, 20.0, 20.0), whole, false), None);
    assert_eq!(page((10.0, 10.0, 20.0, 20.0), whole, true), None);
}

/// A page that is in use is not borrowed again: what was asked waits and is answered at the next
/// turn.
#[test]
fn a_target_that_is_in_use_is_asked_again_and_never_borrowed() {
    let target = target_rig();
    let mut responder = Responder::new(Stand::new(Some(target.handle().clone())));
    let held = target.handle().page();
    let replies = responder.hear(&call(1, "children", Some(&[])));
    assert!(replies.is_empty(), "{replies:?}");
    assert!(responder.waiting());
    // Nothing else is let by it: the answer to a later call of the same method comes after.
    let replies = responder.hear(&call(2, "children", Some(&[0])));
    assert!(replies.is_empty());
    drop(held);
    let replies = responder.poll();
    assert_eq!(replies.len(), 2, "{replies:?}");
    assert!(replies[0].contains(r#""id":1"#) && replies[1].contains(r#""id":2"#));
    assert!(!responder.waiting());
}

/// The target's frames are told to who watches them, and to nobody otherwise.
#[test]
fn a_frame_is_told_to_the_one_who_watches_the_frames() {
    let mut target = target_rig();
    let drawn = Rc::new(Cell::new(0_u32));
    target.frame(1.0);
    assert_eq!(drawn.get(), 0);
    let counted = Rc::clone(&drawn);
    target
        .handle()
        .watch_frames(Some(Box::new(move || counted.set(counted.get() + 1))));
    target.frame(2.0);
    target.frame(3.0);
    assert_eq!(drawn.get(), 2);
    target.handle().watch_frames(None);
    target.frame(4.0);
    assert_eq!(drawn.get(), 2);
}

/// View ▸ Inspect Page and ⌥⌘I, for each way the pane that has the keyboard can be.
#[test]
fn the_command_goes_the_way_the_pane_and_the_key_say() {
    let page = Asked {
        page: true,
        ..Asked::default()
    };
    // A pane with no page of the kit (a terminal, a web view): the item is grey, from the key too.
    assert_eq!(inspect(Asked::default()), Inspect::Grey);
    assert_eq!(
        inspect(Asked {
            by_key: true,
            browser: true,
            ..Asked::default()
        }),
        Inspect::Grey
    );
    // A page: the inspector opens, and a second time it closes.
    assert_eq!(inspect(page), Inspect::Open);
    assert_eq!(inspect(Asked { open: true, ..page }), Inspect::Close);
    // The key, where a browser is the one it means: the browser's tools, as it always was.
    let keyed = Asked {
        by_key: true,
        ..page
    };
    assert_eq!(
        inspect(Asked {
            browser: true,
            ..keyed
        }),
        Inspect::Tools
    );
    // The key where there is no browser, or the menu where there is: the inspector.
    assert_eq!(inspect(keyed), Inspect::Open);
    assert_eq!(
        inspect(Asked {
            browser: true,
            ..page
        }),
        Inspect::Open
    );
    assert_eq!(
        inspect(Asked {
            browser: true,
            open: true,
            ..page
        }),
        Inspect::Close
    );
    // The inspector's own pane closes, whatever else is true of it.
    assert_eq!(inspect(Asked { tool: true, ..page }), Inspect::End);
    assert_eq!(
        inspect(Asked {
            tool: true,
            by_key: true,
            browser: true,
            ..page
        }),
        Inspect::End
    );
}

/// The place a call named, as a list (a page writes the root's as an empty list or as an empty
/// table: the responder takes either, and so does this).
fn place_of(params: &Value) -> Vec<u64> {
    params["path"]
        .as_array()
        .map(|places| places.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default()
}

/// The key the page gives the element named `name`.
fn key(name: &str) -> String {
    format!("#{}", hex(name))
}

fn over_target() -> (PaneRig, Bench) {
    let target = target_rig();
    let mut bench = Bench::over(Stand::new(Some(target.handle().clone())));
    // The timer the page set to take the keyboard for the tree runs at the frame after it.
    bench.now = 0.5;
    bench.settle();
    bench.rig.frame(0.5);
    bench.settle();
    (target, bench)
}

/// The first thing the page asks is the first level of the page it looks at, and it shows it.
#[test]
fn the_page_reads_the_first_level_and_shows_it() {
    let (_target, bench) = over_target();
    let asked = bench.calls_of("children");
    assert_eq!(asked.len(), 1, "{:?}", bench.heard);
    assert!(place_of(&asked[0]).is_empty());
    // The root, and the four boxes the page has at its top, named or not.
    assert!(bench.says("p3"), "{:?}", bench.rig.words());
    let boxes = bench
        .rig
        .words()
        .iter()
        .filter(|line| *line == "box")
        .count();
    assert_eq!(boxes, 4, "{:?}", bench.rig.words());
    assert!(
        bench.says("Choose an element of the page."),
        "{:?}",
        bench.rig.words()
    );
}

/// A folder is read when it is opened: its level is asked for then and no sooner.
#[test]
fn a_level_is_read_when_it_is_opened() {
    let (_target, mut bench) = over_target();
    assert!(!bench.says("text"));
    bench.click(&format!("tree/chevron/{}", key("a")));
    let asked: Vec<Vec<u64>> = bench.calls_of("children").iter().map(place_of).collect();
    assert_eq!(asked, [vec![], vec![0]], "{:?}", bench.heard);
    // What is in the first box is its word, which is a text.
    assert!(bench.says("text"), "{:?}", bench.rig.words());
}

/// An element chosen is chosen in the page it is of, and its box and style are in the details.
#[test]
fn an_element_chosen_is_chosen_in_the_target_and_described() {
    let (_target, mut bench) = over_target();
    bench.click(&format!("tree/list/row/{}", key("b")));
    let chosen = bench.calls_of("select");
    assert_eq!(chosen.len(), 1, "{:?}", bench.heard);
    assert_eq!(place_of(&chosen[0]), [1]);
    let lit = bench.stand.box_now().expect("a box over the pane");
    assert_eq!((lit.y, lit.h), (52.0, 20.0));
    // Its box and how it was placed are said in the details.
    assert!(bench.says("box"), "{:?}", bench.rig.words());
    assert!(bench.says("0, 52   800 × 20"), "{:?}", bench.rig.words());
    assert!(bench.says("auto × 20px"), "{:?}", bench.rig.words());
}

/// The tree is walked with the keys the moment the inspector opens, and what the keys land on is
/// chosen as a click would choose it.
#[test]
fn the_keyboard_is_the_trees_from_the_start() {
    let (_target, mut bench) = over_target();
    bench.rig.key(Key::Down, Mods::default());
    bench.settle();
    bench.rig.key(Key::Down, Mods::default());
    bench.settle();
    let chosen = bench.calls_of("select");
    assert!(!chosen.is_empty(), "{:?}", bench.heard);
}

/// The button closes it as Esc does.
#[test]
fn the_close_button_asks_the_pane_to_close() {
    let (_target, mut bench) = over_target();
    assert!(!bench.responder.closing());
    bench.click("close");
    assert!(bench.responder.closing());
}

/// A page that came where another went is another page whatever it has drawn, so that what the
/// page of the inspector kept of the one before is asked again; the same page is not.
#[test]
fn a_page_that_comes_where_another_went_is_another_stamp() {
    let old = target_rig();
    let stand = Stand::new(Some(old.handle().clone()));
    let mut responder = Responder::new(Rc::clone(&stand) as Rc<dyn Subject>);
    responder.hear(r#"{"subscribe":{"id":1,"topic":"target"}}"#);
    responder.hear(&call(2, "children", Some(&[])));
    let before = responder.stamp();
    // The same page, asked again: the stamp stays.
    responder.hear(&call(3, "children", Some(&[])));
    assert_eq!(responder.stamp(), before);
    // A new page that has drawn as much as the old one: the stamp is not the old one's.
    let new = target_rig();
    stand.page.replace(Some(new.handle().clone()));
    let told = responder.refresh();
    responder.hear(&call(4, "children", Some(&[])));
    assert_ne!(responder.stamp(), before);
    assert!(
        told.iter()
            .any(|reply| reply.contains(r#""push""#) && reply.contains(r#""stamp""#)),
        "{told:?}"
    );
}

/// What was chosen is kept while the page is away, and found again where one comes back.
#[test]
fn the_chosen_element_is_found_again_when_the_page_comes_back() {
    let old = target_rig();
    let stand = Stand::new(Some(old.handle().clone()));
    let mut responder = Responder::new(Rc::clone(&stand) as Rc<dyn Subject>);
    responder.hear(&call(1, "select", Some(&[1])));
    assert!(stand.box_now().is_some());
    stand.page.replace(None);
    responder.refresh();
    assert_eq!(stand.box_now(), None, "no page, no box");
    let new = target_rig();
    stand.page.replace(Some(new.handle().clone()));
    responder.refresh();
    let shown = stand.box_now().expect("the element is boxed again");
    assert_eq!((shown.y, shown.h), (52.0, 20.0));
}

/// The box is told to the pane at each frame while there is one: the pane may have been resized
/// meanwhile, and the box is placed from the pane's own edge.
#[test]
fn the_box_is_told_at_each_frame_even_where_it_has_not_moved() {
    let mut target = target_rig();
    let stand = Stand::new(Some(target.handle().clone()));
    let mut responder = Responder::new(Rc::clone(&stand) as Rc<dyn Subject>);
    responder.hear(&call(1, "select", Some(&[0])));
    let told = stand.lit.borrow().len();
    target.frame(1.0);
    responder.drew(1.0);
    target.frame(2.0);
    responder.drew(2.0);
    assert_eq!(stand.lit.borrow().len(), told + 2);
}

/// What the page asked for ever is not kept for ever: a page that is asked again at every stamp of
/// a target that moves asks a great many, and still hears what it subscribed to.
#[test]
fn the_calls_of_a_long_session_are_let_go_of_and_the_subscriptions_are_kept() {
    let mut target = target_rig();
    let mut responder = Responder::new(Stand::new(Some(target.handle().clone())));
    responder.hear(r#"{"subscribe":{"id":1,"topic":"target"}}"#);
    let mut stamps = Vec::new();
    for id in 2..2000_u64 {
        responder.hear(&call(id, "children", Some(&[])));
    }
    assert!(
        responder.calls.calls().len() <= KEPT + 1,
        "{} calls kept",
        responder.calls.calls().len()
    );
    // The subscription is still there: a frame that did something is told.
    target.key(Key::Character('r'), Mods::default());
    target.settle(10.0);
    stamped(&mut stamps, responder.drew(10.0));
    assert_eq!(stamps.len(), 1, "{stamps:?}");
    // And a page that was cancelled stays cancelled.
    responder.hear(r#"{"cancel":{"id":1}}"#);
    target.key(Key::Character('r'), Mods::default());
    target.settle(20.0);
    stamped(&mut stamps, responder.drew(20.0));
    assert_eq!(stamps.len(), 1, "{stamps:?}");
}

/// A layout made for a reader who asked for it draws no frame: the one who watches the frames is
/// not told.
#[test]
fn a_layout_made_for_a_reader_is_no_frame() {
    let target = target_rig();
    let drawn = Rc::new(Cell::new(0_u32));
    let counted = Rc::clone(&drawn);
    target
        .handle()
        .watch_frames(Some(Box::new(move || counted.set(counted.get() + 1))));
    target.handle().lay_out_unseen(pencere_drive::FrameAt {
        size: (800, 600),
        scale: 1.0,
        now: 3.0,
        reduce_motion: false,
    });
    assert_eq!(drawn.get(), 0);
}
