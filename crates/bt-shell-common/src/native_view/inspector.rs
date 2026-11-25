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
//! messages as the program library does). What it reads of the pane it looks at comes from a
//! [`Subject`], which the platform's pane gives, and what it says is:
//!
//! - `target` (topic): `{ pane, page, gone, stamp }`, when the page subscribes and whenever what
//!   it says changes. The `stamp` says which page of the target's it is: a new one each time
//!   the target draws a frame that did something (made a page, laid it out again, ran its
//!   script), and a new generation for each page the pane shows after another. **The page asks
//!   again what it asked under another stamp**, and the stamp changes at most four times a
//!   second ([`INTERVAL`]), however often the target draws, and once more where it comes to rest;
//! - `costs` (topic): what the target's last frames cost, pushed after the target drew a frame
//!   and never otherwise;
//! - `children(path)`, `node(path)` (calls): an element's children, which are asked one level at a
//!   time (a large page's whole tree is more than a message may carry), and one element's box,
//!   room, border, style and what a screen reader says of it;
//! - `select(path)` (call): the element whose box the target shows, which is followed by its
//!   name where it has one and by its place where it has none;
//! - `close` (call): the page asks to be closed ([`Responder::closing`]).
//!
//! **The inspector is the one reader of the target's page**, on the main thread where the target's
//! frames are built too: it never takes the page where something else has it ([`PageHandle::try_page`])
//! and what it was asked meanwhile waits for the next turn ([`Responder::poll`]). The target's page
//! measures while the inspector is open and is let go of when it closes ([`Responder::end`]).
//! What it says is no program's: it never reaches a terminal or a wire.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use pencere_drive::headless::{Calls, Failure, Peer, Value};
use pencere_drive::{Cost, Inspection, Item};
use pencere_sandbox::{ModuleLimits, Script};
use pencere_view::Placing;

use super::{Opening, PageHandle, Rect, script_page};

/// The page's script.
pub const SCRIPT: &str = include_str!("inspector.lua");

/// The topic the page hears what it looks at by.
const TARGET: &str = "target";

/// The topic the page hears what the target's frames cost by.
const COSTS: &str = "costs";

/// The call that asks the pane to close.
const CLOSE: &str = "close";

/// The least time between two things the page is told of the target's frames, in seconds: a
/// target that draws on every frame (a drag, a live chart) is told of at most four times a second,
/// since each telling has the page ask for what it shows again, in the same thread the target is
/// drawn in.
pub const INTERVAL: f64 = 0.25;

/// How many of the target's last frames the page is told the cost of.
const FRAMES_TOLD: usize = 4;

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
    /// The pane's page, if it shows one.
    fn page(&self) -> Option<PageHandle>;
    /// The box to show over the pane, in its points (`None`: none), where the element chosen is.
    fn highlight(&self, rect: Option<Rect>);
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

/// How many calls of the page the table keeps before the answered ones are let go of.
const KEPT: usize = 256;

/// The table the page's calls are answered by: `close`, which the responder takes itself.
fn table(closing: &Rc<Cell<bool>>) -> Calls {
    let asked = Rc::clone(closing);
    Calls::new().on(CLOSE, move |_| {
        asked.set(true);
        Ok(Value::Object(Default::default()))
    })
}

/// What the page was last told of its target: the pane, and the stamp of its page.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Told {
    target: Target,
    stamp: u64,
}

/// The element the page chose: the name it has, where it has one, and the place it stood in.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Chosen {
    id: Option<u64>,
    path: Vec<usize>,
}

/// What reading the target's page came to.
enum Read {
    /// The pane has no page.
    Absent,
    /// The page is in use: a frame is being built or a call is in it.
    Busy,
    /// The page has not drawn a frame yet.
    Unseen,
    /// The page as its last frame left it, and the stamp it has.
    Model(u64, Rc<Inspection>),
}

/// Why a call is not answered now.
enum Held {
    /// The page is in use: asked again at the next turn.
    Busy,
    /// Refused, as the protocol has it.
    Refused(Failure),
}

fn refused(code: &str, message: &str) -> Held {
    Held::Refused(Failure {
        code: code.to_owned(),
        message: message.to_owned(),
    })
}

/// What View ▸ Inspect Page (⌥⌘I) is asked of, by the pane that has the keyboard.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Asked {
    /// The pane shows a page of the kit (a program's view, or the inspector's own page).
    pub page: bool,
    /// The key was pressed: the item was not clicked in the menu.
    pub by_key: bool,
    /// A browser of the page is the one the host's tools key means (the one the keyboard is in,
    /// or the one that had it last, or the only one shown).
    pub browser: bool,
    /// The pane is an inspector.
    pub tool: bool,
    /// An inspector of the pane is open.
    pub open: bool,
}

/// What the command does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Inspect {
    /// The item is grey: the pane shows no page of the kit (a terminal, a web view).
    Grey,
    /// The browser's tools open, as ⌥⌘I did before the page had an inspector: the key stays
    /// the browser's wherever it means one.
    Tools,
    /// The inspector of the pane opens, beside it.
    Open,
    /// The inspector of the pane closes: the command is asked a second time.
    Close,
    /// The pane is the inspector and closes: it inspects no inspector.
    End,
}

/// What View ▸ Inspect Page does for the pane that has the keyboard. The key keeps the browser's
/// tools where the pane has a browser they mean, as it was before (the menu's key is caught before
/// the pane's own `keyDown:`, so the command has to do what the pane's would have done); from the
/// menu, where nothing says a browser is meant, it is always the inspector.
pub fn inspect(asked: Asked) -> Inspect {
    if asked.tool {
        Inspect::End
    } else if !asked.page {
        Inspect::Grey
    } else if asked.by_key && asked.browser {
        Inspect::Tools
    } else if asked.open {
        Inspect::Close
    } else {
        Inspect::Open
    }
}

/// What the page says is answered here, and what the page is to be told is said here.
pub struct Responder {
    calls: Calls,
    subject: Rc<dyn Subject>,
    /// The page asked to be closed.
    closing: Rc<Cell<bool>>,
    /// What the live subscriptions of the target were last told.
    told: Option<Told>,
    /// How many of the page's subscriptions have been looked at.
    seen: usize,
    /// The element the page chose.
    chosen: Option<Chosen>,
    /// The box shown over the pane now.
    shown: Option<Rect>,
    /// The target drew frames that the page has not been told of.
    pending: bool,
    /// When the page was last told of the target's frames.
    published_at: Option<f64>,
    /// The target's page as its last frame left it, kept until the next frame.
    cache: Option<Rc<Inspection>>,
    /// How many pages the pane has shown after another since the inspector looked at it: the
    /// stamp's high part, so that a page that came where another went (a reload) is not taken for
    /// the one before, whose first frames the new one's would otherwise match.
    generation: Cell<u64>,
    /// The page the stamp is of.
    page: RefCell<Option<PageHandle>>,
    /// How many frames of the page that did something the inspector has seen: the stamp's low part.
    epoch: Cell<u64>,
    /// The last frame that did something that was seen, which says whether another came.
    last_frame: RefCell<Option<Cost>>,
    /// The inspector is over: the target is not measured any more.
    ended: bool,
}

impl Responder {
    /// A responder for the page of the inspector of `subject`, which starts measuring the page
    /// the pane has.
    pub fn new(subject: Rc<dyn Subject>) -> Self {
        let closing = Rc::new(Cell::new(false));
        let calls = table(&closing);
        let responder = Self {
            calls,
            subject,
            closing,
            told: None,
            seen: 0,
            chosen: None,
            shown: None,
            pending: false,
            published_at: None,
            cache: None,
            generation: Cell::new(0),
            page: RefCell::new(None),
            epoch: Cell::new(0),
            last_frame: RefCell::new(None),
            ended: false,
        };
        responder.measure();
        responder
    }

    /// The page said `message` to its program: what the responder says back, in order.
    pub fn hear(&mut self, message: &str) -> Vec<String> {
        self.calls.heard(message);
        self.refuse_unknown_calls();
        self.answer_new_subscriptions();
        self.serve();
        self.out()
    }

    /// What the pane looked at shows changed (its page came up or went down): the page is told it
    /// if it is not what it was told last, and the target's page is measured if it is a new one.
    /// Nothing where it is the same, or where the page hears no `target`.
    pub fn refresh(&mut self) -> Vec<String> {
        self.measure();
        self.cache = None;
        self.tell_target();
        // What was chosen is kept while the page is away: where one comes back, the element is
        // found again by its name, or its place where it has none.
        self.place_highlight();
        self.serve();
        self.out()
    }

    /// The target drew a frame, `now` on the clock the pane keeps. The page is told of it, if the
    /// last telling was [`INTERVAL`] ago or more; otherwise at the end of the interval
    /// ([`Responder::due`]), once, however many frames come before.
    pub fn drew(&mut self, now: f64) -> Vec<String> {
        self.cache = None;
        self.place_highlight();
        if self.published_at.is_none_or(|at| now - at >= INTERVAL) {
            self.publish(now);
        } else {
            self.pending = true;
        }
        self.serve();
        self.out()
    }

    /// When the frames the page has not been told of are due to be, on the pane's clock, if
    /// there are.
    pub fn due(&self) -> Option<f64> {
        let at = self.published_at?;
        self.pending.then_some(at + INTERVAL)
    }

    /// `now` has come: the frames that were waiting are told if their time is.
    pub fn tick(&mut self, now: f64) -> Vec<String> {
        if self.due().is_some_and(|due| now >= due) {
            self.publish(now);
        }
        self.serve();
        self.out()
    }

    /// What the page asked while the target's page was in use is answered now if the page is not.
    pub fn poll(&mut self) -> Vec<String> {
        self.serve();
        self.out()
    }

    /// Whether what the page asked is still waiting for the target's page to be free.
    pub fn waiting(&self) -> bool {
        self.calls
            .waiting()
            .iter()
            .any(|call| matches!(call.method.as_str(), "children" | "node" | "select"))
    }

    /// The page asked for its pane to be closed.
    pub fn closing(&self) -> bool {
        self.closing.get()
    }

    /// The inspector is over: the target's page is let go of, and no box is shown over it. Once.
    pub fn end(&mut self) {
        if self.ended {
            return;
        }
        self.ended = true;
        self.chosen = None;
        self.place_highlight();
        if let Some(handle) = self.subject.page()
            && let Some(mut page) = handle.try_page()
        {
            page.inspect(false);
        }
    }

    /// The target's page, and whether it is another than the one the stamp was of: a page that
    /// came where another went is a new generation of stamps, however few pages it has made.
    fn handle(&self) -> Option<PageHandle> {
        let handle = self.subject.page()?;
        let mut known = self.page.borrow_mut();
        match known.as_ref() {
            Some(old) if old.same(&handle) => {}
            Some(_) => {
                self.generation.set(self.generation.get() + 1);
                self.epoch.set(0);
                self.last_frame.replace(None);
                *known = Some(handle.clone());
            }
            None => *known = Some(handle.clone()),
        }
        Some(handle)
    }

    /// What the responder said since it was last asked. The table of calls keeps what the page
    /// asked for ever, and a page that is asked again at every stamp of a target that moves asks a
    /// great many: where all of it is answered, what is kept of it is let go of.
    fn out(&mut self) -> Vec<String> {
        let said = self.calls.said();
        if self.calls.calls().len() > KEPT && self.calls.waiting().is_empty() {
            let live: Vec<(u64, &str)> = self
                .calls
                .subscriptions()
                .iter()
                .filter(|subscription| subscription.live)
                .filter_map(|subscription| {
                    [TARGET, COSTS]
                        .into_iter()
                        .find(|topic| *topic == subscription.topic)
                        .map(|topic| (subscription.id, topic))
                })
                .collect();
            let mut fresh = table(&self.closing);
            for (id, topic) in &live {
                fresh.heard(&format!(
                    r#"{{"subscribe":{{"id":{id},"topic":"{topic}"}}}}"#
                ));
            }
            self.seen = live.len();
            self.calls = fresh;
        }
        said
    }

    /// The target's page measures from now on, where it does not.
    fn measure(&self) {
        if self.ended {
            return;
        }
        if let Some(handle) = self.handle()
            && let Some(mut page) = handle.try_page()
            && !page.inspecting()
        {
            page.inspect(true);
        }
    }

    /// The target's page as its last frame left it.
    fn read(&mut self) -> Read {
        let Some(handle) = self.handle() else {
            return Read::Absent;
        };
        let Some(mut page) = handle.try_page() else {
            return Read::Busy;
        };
        if !self.ended && !page.inspecting() {
            page.inspect(true);
        }
        if let Some(model) = &self.cache {
            return Read::Model(self.stamp(), Rc::clone(model));
        }
        match page.inspection() {
            Some(inspection) => {
                self.note(&inspection);
                let model = Rc::new(inspection);
                self.cache = Some(Rc::clone(&model));
                Read::Model(self.stamp(), model)
            }
            None => Read::Unseen,
        }
    }

    /// A model that has a frame of the page that did something that the last did not is another
    /// page of the target's: the stamp moves on. It is the page as it was **drawn** that is read,
    /// and what the drive says of it is the frames it kept: the count of pages the script made
    /// runs ahead of the one drawn, and a stamp that followed it would be the same for the page
    /// before and for the one that came after.
    fn note(&self, model: &Inspection) {
        let last = model.frames.last();
        if self.last_frame.borrow().as_ref() != last {
            self.epoch.set(self.epoch.get() + 1);
            self.last_frame.replace(last.cloned());
        }
    }

    /// Which page of the target's the inspector knows: the pages the pane has shown one after the
    /// other, and the frames of the page that did something.
    pub fn stamp(&self) -> u64 {
        (self.generation.get() << 32) | self.epoch.get().min(u64::from(u32::MAX))
    }

    /// The target's frames were drawn since the page was last told: it is told now.
    fn publish(&mut self, now: f64) {
        self.pending = false;
        self.published_at = Some(now);
        // The page as it was drawn first: what it says of it is the stamp that is told.
        let read = self.read();
        self.tell_target();
        let Read::Model(stamp, model) = read else {
            return;
        };
        let value = costs_of(stamp, &model);
        let ids = self.subscribed(COSTS);
        for id in ids {
            self.calls.say(&push(id, &value));
        }
    }

    /// The page is told what it looks at if that is not what it was told.
    fn tell_target(&mut self) {
        let now = Told {
            target: self.subject.target(),
            stamp: self.stamp(),
        };
        if self.told.as_ref() == Some(&now) {
            return;
        }
        let value = value_of(&now);
        for id in self.subscribed(TARGET) {
            self.calls.say(&push(id, &value));
        }
        self.told = Some(now);
    }

    /// The live subscriptions of `topic`.
    fn subscribed(&self, topic: &str) -> Vec<u64> {
        self.calls
            .subscriptions()
            .iter()
            .filter(|subscription| subscription.live && subscription.topic == topic)
            .map(|subscription| subscription.id)
            .collect()
    }

    /// A call the responder has no method for is refused as the protocol has it (`not-found`),
    /// not left waiting as a slow program's would be.
    fn refuse_unknown_calls(&mut self) {
        let unknown: Vec<String> = self
            .calls
            .waiting()
            .iter()
            .map(|call| call.method.as_str())
            .filter(|method| !matches!(*method, "children" | "node" | "select"))
            .map(str::to_owned)
            .collect();
        for method in unknown {
            let _ = self.calls.answer(
                &method,
                Err(Failure {
                    code: "not-found".to_owned(),
                    message: "the inspector has no such method".to_owned(),
                }),
            );
        }
    }

    /// A subscription the page made since the last time: what it hears is told it as it is now,
    /// and any other topic is refused as the protocol has it.
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
            match topic.as_str() {
                TARGET if live => {
                    let now = Told {
                        target: self.subject.target(),
                        stamp: self.stamp(),
                    };
                    self.calls.say(&push(id, &value_of(&now)));
                    self.told = Some(now);
                }
                // What the frames cost is told after a frame; what the frames already drawn
                // cost is told to the page that comes after them.
                COSTS if live => {
                    if let Read::Model(stamp, model) = self.read()
                        && !model.frames.is_empty()
                    {
                        self.calls.say(&push(id, &costs_of(stamp, &model)));
                    }
                }
                TARGET | COSTS => {}
                _ => self.calls.say(&refusal(id)),
            }
        }
    }

    /// What the page asked of the target's page, answered where the page is free; each method's
    /// calls in the order they were asked.
    fn serve(&mut self) {
        let mut blocked: Vec<String> = Vec::new();
        let asked: Vec<(String, Value)> = self
            .calls
            .waiting()
            .iter()
            .filter(|call| matches!(call.method.as_str(), "children" | "node" | "select"))
            .map(|call| (call.method.clone(), call.params.clone()))
            .collect();
        for (method, params) in asked {
            if blocked.contains(&method) {
                continue;
            }
            match self.answer(&method, &params) {
                Err(Held::Busy) => blocked.push(method),
                Err(Held::Refused(failure)) => {
                    let _ = self.calls.answer(&method, Err(failure));
                }
                Ok(result) => {
                    let _ = self.calls.answer(&method, Ok(result));
                }
            }
        }
    }

    /// The answer to the call `method` with `params`.
    fn answer(&mut self, method: &str, params: &Value) -> Result<Value, Held> {
        let (stamp, model) = self.model()?;
        match method {
            "children" => {
                let path = path_of(params)?.unwrap_or_default();
                children_of(stamp, &model, &path)
            }
            "node" => {
                let path =
                    path_of(params)?.ok_or_else(|| refused("invalid", "node asks for a path"))?;
                node_of(stamp, &model, &path)
            }
            _ => {
                match path_of(params)? {
                    None => self.chosen = None,
                    Some(path) => {
                        let item = item_at(&model, &path)
                            .ok_or_else(|| refused("not-found", "no element at that place"))?;
                        self.chosen = Some(Chosen {
                            id: item.element.id.map(|id| id.number()),
                            path,
                        });
                    }
                }
                self.place_highlight_in(&model);
                Ok(Value::Object(Default::default()))
            }
        }
    }

    /// The target's page, or why it is not to be had.
    fn model(&mut self) -> Result<(u64, Rc<Inspection>), Held> {
        match self.read() {
            Read::Model(stamp, model) => Ok((stamp, model)),
            Read::Busy => Err(Held::Busy),
            Read::Absent => Err(refused("unavailable", "the pane shows no page")),
            Read::Unseen => Err(refused("unavailable", "the page has not been drawn yet")),
        }
    }

    /// The box over the pane is where the element the page chose is now, after the page the
    /// target draws changed: it follows the element's name where it has one, its place where it
    /// has none.
    fn place_highlight(&mut self) {
        if self.chosen.is_none() {
            self.show(None);
            return;
        }
        match self.read() {
            Read::Model(_, model) => self.place_highlight_in(&model),
            // No page: no box; what was chosen is kept to be found where a page comes back.
            Read::Absent => self.show(None),
            Read::Busy | Read::Unseen => {}
        }
    }

    fn place_highlight_in(&mut self, model: &Inspection) {
        let found = self.chosen.as_ref().and_then(|chosen| {
            let by_name = chosen.id.and_then(|id| {
                model
                    .items
                    .iter()
                    .find(|item| item.element.id.map(|name| name.number()) == Some(id))
            });
            by_name.or_else(|| item_at(model, &chosen.path))
        });
        if let (Some(chosen), Some(item)) = (self.chosen.as_mut(), found)
            && let Some(path) = &item.element.path
        {
            chosen.path.clone_from(path);
        }
        self.show(found.and_then(|item| shown_box(&item.element)));
    }

    /// The box is where `rect` is: told to the pane each time a frame was drawn while there is one,
    /// since the pane may have moved or resized it meanwhile (a box is kept from the top, the
    /// pane's frames from the bottom), and told once when there is none.
    fn show(&mut self, rect: Option<Rect>) {
        if rect.is_some() || self.shown != rect {
            self.shown = rect;
            self.subject.highlight(rect);
        }
    }
}

impl Drop for Responder {
    fn drop(&mut self) {
        self.end();
    }
}

/// Where the part of `element`'s box that shows is, in the host's points: its box within what the
/// elements around it let be drawn. `None` for what nothing of is drawn.
pub fn shown_box(element: &pencere_view::Inspected) -> Option<Rect> {
    if element.hidden || element.exiting {
        return None;
    }
    let (rect, clip) = (element.rect, element.clip);
    let left = rect.x.max(clip.x);
    let top = rect.y.max(clip.y);
    let right = (rect.x + rect.w).min(clip.x + clip.w);
    let bottom = (rect.y + rect.h).min(clip.y + clip.h);
    (right > left && bottom > top).then(|| Rect::new(left, top, right - left, bottom - top))
}

/// The element at the place `path` in the page's tree.
fn item_at<'a>(model: &'a Inspection, path: &[usize]) -> Option<&'a Item> {
    model
        .items
        .iter()
        .find(|item| item.element.path.as_deref() == Some(path))
}

/// The place a call names (`{ path = { 1, 2 } }`): `None` where it names none (no `path`, or
/// `null`).
fn path_of(params: &Value) -> Result<Option<Vec<usize>>, Held> {
    let Some(path) = params.get("path").filter(|path| !path.is_null()) else {
        return Ok(None);
    };
    // A page that writes an empty list as `{}` names the root: an empty table is both.
    let places = match path {
        Value::Array(places) => places.as_slice(),
        Value::Object(members) if members.is_empty() => &[],
        _ => return Err(refused("invalid", "a path is a list of places")),
    };
    places
        .iter()
        .map(|place| {
            place
                .as_u64()
                .and_then(|place| usize::try_from(place).ok())
                .ok_or_else(|| refused("invalid", "a place of a path is a whole number"))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

/// What the page is told of an element among the others: where it stands, what it is, its name
/// and how many children it has.
fn child_of(item: &Item, children: usize) -> Value {
    let element = &item.element;
    object([
        ("path", places(element.path.as_deref().unwrap_or_default())),
        ("shape", Value::from(element.shape.name())),
        ("id", id_of(element)),
        ("name", name_of(item)),
        ("count", Value::from(children)),
        ("hidden", Value::from(element.hidden)),
        ("overlay", Value::from(element.overlay)),
    ])
}

/// The elements of the page one step below `path`, each with how many are below it.
fn children_of(stamp: u64, model: &Inspection, path: &[usize]) -> Result<Value, Held> {
    if item_at(model, path).is_none() {
        return Err(refused("not-found", "no element at that place"));
    }
    let mut counts: HashMap<&[usize], usize> = HashMap::new();
    for item in &model.items {
        if let Some(own) = item.element.path.as_deref()
            && let Some((_, parent)) = own.split_last()
        {
            *counts.entry(parent).or_insert(0) += 1;
        }
    }
    let below: Vec<Value> = model
        .items
        .iter()
        .filter(|item| {
            item.element
                .path
                .as_deref()
                .is_some_and(|own| own.len() == path.len() + 1 && own.starts_with(path))
        })
        .map(|item| {
            let own = item.element.path.as_deref().unwrap_or_default();
            child_of(item, counts.get(own).copied().unwrap_or(0))
        })
        .collect();
    Ok(object([
        ("stamp", Value::from(stamp)),
        ("path", places(path)),
        ("children", Value::Array(below)),
    ]))
}

/// One element: its box, its room inside, its border, how it is placed and what a screen reader says
/// of it.
fn node_of(stamp: u64, model: &Inspection, path: &[usize]) -> Result<Value, Held> {
    let item =
        item_at(model, path).ok_or_else(|| refused("not-found", "no element at that place"))?;
    let element = &item.element;
    let side = |room: [f32; 4]| Value::Array(room.iter().map(|side| number(*side)).collect());
    let said: Vec<Value> = item
        .said
        .iter()
        .map(|said| {
            object([
                ("role", Value::from(said.role.as_str())),
                ("words", Value::from(said.words.as_str())),
                ("state", Value::from(said.state.as_str())),
                ("pressable", Value::from(said.pressable)),
            ])
        })
        .collect();
    Ok(object([
        ("stamp", Value::from(stamp)),
        ("path", places(path)),
        ("shape", Value::from(element.shape.name())),
        ("id", id_of(element)),
        ("name", name_of(item)),
        (
            "field",
            Value::from(element.field.map(|field| format!("{field:?}"))),
        ),
        ("rect", rect_of(element.rect)),
        ("clip", rect_of(element.clip)),
        ("padding", side(element.padding)),
        (
            "border",
            element.border.map_or(Value::Null, |border| {
                object([("width", number(border.width))])
            }),
        ),
        (
            "scroll",
            if element.content.is_some() {
                object([
                    ("x", number(element.scroll.0)),
                    ("y", number(element.scroll.1)),
                ])
            } else {
                Value::Null
            },
        ),
        (
            "content",
            element.content.map_or(Value::Null, |content| {
                object([("w", number(content.w)), ("h", number(content.h))])
            }),
        ),
        ("hidden", Value::from(element.hidden)),
        ("overlay", Value::from(element.overlay)),
        ("exiting", Value::from(element.exiting)),
        (
            "placing",
            element.placing.as_ref().map_or(Value::Null, placing_of),
        ),
        ("said", Value::Array(said)),
    ]))
}

/// How an element is placed, as the page wrote it, each part as a short word or a few numbers.
fn placing_of(placing: &Placing) -> Value {
    let free = |inset: Option<f32>| inset.map_or(Value::Null, number);
    let length = |length: &pencere_view::Length| match length {
        pencere_view::Length::Auto => "auto".to_owned(),
        pencere_view::Length::Px(points) => format!("{}px", trim(*points)),
        pencere_view::Length::Percent(share) => format!("{}%", trim(*share)),
    };
    let side = |room: [f32; 4]| Value::Array(room.iter().map(|side| number(*side)).collect());
    object([
        ("display", Value::from(format!("{:?}", placing.display))),
        ("direction", Value::from(format!("{:?}", placing.direction))),
        ("wrap", Value::from(placing.wrap)),
        ("absolute", Value::from(placing.absolute)),
        ("overlay", Value::from(placing.overlay)),
        ("inset", Value::Array(placing.inset.map(free).to_vec())),
        ("width", Value::from(length(&placing.width))),
        ("height", Value::from(length(&placing.height))),
        ("min_width", Value::from(length(&placing.min_width))),
        ("min_height", Value::from(length(&placing.min_height))),
        ("max_width", Value::from(length(&placing.max_width))),
        ("max_height", Value::from(length(&placing.max_height))),
        ("padding", side(placing.padding)),
        ("margin", side(placing.margin)),
        (
            "gap",
            Value::Array(vec![number(placing.gap.0), number(placing.gap.1)]),
        ),
        ("grow", number(placing.grow)),
        ("shrink", placing.shrink.map_or(Value::Null, number)),
        (
            "items",
            Value::from(placing.items.map(|items| format!("{items:?}"))),
        ),
        (
            "justify",
            Value::from(placing.justify.map(|justify| format!("{justify:?}"))),
        ),
        ("columns", Value::from(format!("{:?}", placing.columns))),
        ("rows", Value::from(format!("{:?}", placing.rows))),
        ("column_span", Value::from(placing.column_span)),
        ("overflow", Value::from(format!("{:?}", placing.overflow))),
    ])
}

/// What the target's last frames cost.
fn costs_of(stamp: u64, model: &Inspection) -> Value {
    let kept = model.frames.len().saturating_sub(FRAMES_TOLD);
    let frames: Vec<Value> = model.frames[kept..].iter().map(frame_of).collect();
    object([
        ("stamp", Value::from(stamp)),
        ("elements", Value::from(model.elements)),
        ("frames", Value::Array(frames)),
    ])
}

fn frame_of(cost: &Cost) -> Value {
    let script: Duration = cost.script.iter().map(|call| call.took).sum();
    object([
        ("elements", Value::from(cost.elements)),
        ("renders", Value::from(cost.renders)),
        ("calls", Value::from(cost.script.len())),
        ("layouts", Value::from(cost.layouts)),
        ("script_ms", millis(script)),
        ("layout_ms", millis(cost.layout)),
        ("paint_ms", millis(cost.paint)),
    ])
}

/// What the page is told of its target.
fn value_of(told: &Told) -> Value {
    object([
        ("pane", Value::from(told.target.name.as_str())),
        ("page", Value::from(told.target.page)),
        ("gone", Value::from(told.target.gone)),
        ("stamp", Value::from(told.stamp)),
    ])
}

/// An object of `members`, in the order given.
fn object<const N: usize>(members: [(&str, Value); N]) -> Value {
    members.into_iter().collect()
}

/// A place in the tree, as a list.
fn places(path: &[usize]) -> Value {
    Value::Array(path.iter().map(|place| Value::from(*place)).collect())
}

/// The name an element was given, in hex: a number of 64 bits is more than a page's number holds.
fn id_of(element: &pencere_view::Inspected) -> Value {
    element
        .id
        .map_or(Value::Null, |id| Value::from(format!("{:x}", id.number())))
}

/// What a screen reader first says of the element, where it says anything.
fn name_of(item: &Item) -> Value {
    item.said
        .iter()
        .map(|said| said.words.as_str())
        .find(|words| !words.is_empty())
        .map_or(Value::Null, Value::from)
}

fn rect_of(rect: Rect) -> Value {
    object([
        ("x", number(rect.x)),
        ("y", number(rect.y)),
        ("w", number(rect.w)),
        ("h", number(rect.h)),
    ])
}

/// A length in points, to hundredths: what the page was given is not what a reader needs the
/// float's digits of.
fn number(points: f32) -> Value {
    Value::from((f64::from(points) * 100.0).round() / 100.0)
}

fn trim(points: f32) -> String {
    format!("{}", (f64::from(points) * 100.0).round() / 100.0)
}

fn millis(time: Duration) -> Value {
    Value::from((time.as_secs_f64() * 100_000.0).round() / 100.0)
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
mod tests;
