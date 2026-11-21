//! What an agent reads and does in a program's web view (`bateri view` on a pane whose view is a
//! web view).
//!
//! **The script is in a world of its own.** A named content world, [`WORLD`], holds [`script`]: the
//! page's scripts see nothing of it (its variables, its functions, the handler it speaks to the
//! app through), so what a page does cannot make the agent's reading say something else, and the
//! page cannot call the app by it. It is put in every frame of the web view and acts only in the
//! program's (`agent.js` says how it knows which). When the document of that frame can be read it
//! sends one message; the app keeps the frame it came from (`WKFrameInfo`) and calls the script
//! in that frame, in that world, by `callAsyncJavaScript`: nothing else of the web view runs a
//! script in the agent's world, and nothing of the agent's is run in the page's.
//!
//! **The actions are the page's own events.** A click is the sequence of pointer, mouse and click
//! events dispatched to the element a person's click would hit, a key is a keyboard event, a fill
//! sets the field's value and tells the page it was typed. They are not events of the system: a
//! system event reaches a web view only through its window, and needs it to be the window in
//! front with the keyboard in the view — the user's window, the user's keyboard — and a pane on a
//! tab that is not shown has no window to give one to. This is why the actions work on a pane
//! that is not seen, and why nothing here moves the keyboard or brings a window or a tab forward.
//!
//! **What the page does to the person's things is turned back.** The web view runs the agent's
//! call as a person's gesture, so a page's Copy button writes the pasteboard and its Upload button
//! asks for the Open panel. A request that drives the page keeps what the pasteboard held and puts
//! it back if the page wrote it ([`Kept`]), and while a request is carried out the panel is
//! turned away as a Cancel ([`WebRun`] marks the pane's session active, as a native page's
//! request does).
//!
//! **A request is a short chain of calls, asked again until the page is at rest** ([`WebRun`]):
//! look (to name the element), act, look until the document has stood still ([`QUIET_MS`], in the
//! script) or the time is up, answer. Nothing blocks: the answers of the calls come on the main
//! queue into a slot, and the app's clock for the requests that wait ([`WebRun::step`]) looks at
//! the slot. The reading is a [`Reading`] like a native page's and is written by the same writer.
//!
//! **The picture** is the web view's own ([`still::shoot`], a new one asked for and waited for), brought
//! to the size [`picture_size`] says and written as PNG.
//!
//! [`QUIET_MS`]: bt_shell_common::agent::view::web::QUIET_MS

use std::cell::RefCell;
use std::ffi::c_void;
use std::rc::Rc;
use std::time::{Duration, Instant};

use block2::RcBlock;
use bt_shell_common::agent::message::{Failure, Format, Kind, Reply};
use bt_shell_common::agent::view::web::{self, Act};
use bt_shell_common::agent::view::{
    Action, Pane, REST_LIMIT, Shared, Step, View, WAIT_DEFAULT, Way, drives, picture_size, written,
};
use bt_shell_common::native_view::Reading;
use objc2::MainThreadOnly;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardWriting};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo,
    CGInterpolationQuality, kCGColorSpaceSRGB,
};
use objc2_foundation::{NSArray, NSData, NSError, NSString};
use objc2_web_kit::{WKContentWorld, WKFrameInfo};

use super::{ViewHost, serve::js_string, still};

/// The name of the agent's content world: the page's scripts are in another, and so is every
/// other named one.
pub(super) const WORLD: &str = "bateri-agent";

/// The handler the script tells the app it is there by. It is registered in [`WORLD`] alone, so a
/// page's script has no `webkit.messageHandlers.bateriAgent`.
pub(super) const HANDLER: &str = "bateriAgent";

/// The script of the agent's world.
const SCRIPT: &str = include_str!("agent.js");

/// The script as it is put in the frames of the web view.
pub(super) fn script() -> &'static str {
    SCRIPT
}

/// How long the program's frame may take to say it is there, seconds: a view whose page has not
/// loaded (or loaded and does nothing with its document) is not a view to read.
const READY_LIMIT: f64 = 8.0;

/// How long one call into the page may stay unanswered, seconds: a page whose script runs for
/// longer than this is not answering.
const CALL_LIMIT: f64 = 6.0;

/// How long the picture of the view may be waited for, seconds.
const PICTURE_LIMIT: f64 = 6.0;

/// How often the page is looked at while it is waited on, seconds: each look walks its document.
const LOOK_EVERY: f64 = 0.1;

/// What a call came back with: the script's text, or what the web view said when it could not
/// run it. Filled on the main queue, read by [`WebRun::step`].
type Slot = Rc<RefCell<Option<Result<String, String>>>>;

/// What the web view's picture came to: the PNG, or why there is none.
type PictureSlot = Rc<RefCell<Option<Result<Vec<u8>, &'static str>>>>;

impl ViewHost {
    /// The world of the agent's script, made (or found: a named world is one for the process)
    /// for the web view's configuration.
    pub(super) fn agent_world(mtm: objc2::MainThreadMarker) -> Retained<WKContentWorld> {
        // SAFETY: main thread; a name is the whole of the call.
        unsafe { WKContentWorld::worldWithName(&NSString::from_str(WORLD), mtm) }
    }

    /// Whether the program's frame has said it is there.
    fn agent_there(&self) -> bool {
        self.bridge.agent_frame().is_some()
    }

    /// Runs `body`, the body of a function, in the agent's world in the program's frame; the
    /// answer is put in the slot that comes back. `None` when the frame has not said it is there.
    fn agent_call(&self, body: &str) -> Option<Slot> {
        let frame: Retained<WKFrameInfo> = self.bridge.agent_frame()?;
        let slot: Slot = Rc::default();
        let into = Rc::clone(&slot);
        let answer = RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
            // SAFETY: the web view's own value and error, alive through this call.
            let reply = match unsafe { error.as_ref() } {
                Some(error) => Err(error.localizedDescription().to_string()),
                None => Ok(unsafe { value.as_ref() }
                    .and_then(|value| value.downcast_ref::<NSString>())
                    .map(ToString::to_string)
                    .unwrap_or_default()),
            };
            into.replace(Some(reply));
        });
        // SAFETY: main thread; the web view copies the block and holds the frame and the world
        // through the call.
        unsafe {
            self.web
                .callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(
                    &NSString::from_str(body),
                    None,
                    Some(&frame),
                    &self.world,
                    Some(&answer),
                );
        }
        Some(slot)
    }

    /// The picture of the view, asked for: a new one, which the web view makes now and answers on
    /// the main queue, brought to the size [`picture_size`] says and written as a PNG.
    fn agent_picture(&self, full: bool) -> PictureSlot {
        let slot: PictureSlot = Rc::default();
        let bounds = self.web.bounds().size;
        if !(bounds.width >= 1.0 && bounds.height >= 1.0) {
            slot.replace(Some(Err("the view has no room yet")));
            return slot;
        }
        let into = Rc::clone(&slot);
        still::shoot(
            &self.web,
            self.web.mtm(),
            still::Capture {
                rect: [0.0, 0.0, bounds.width, bounds.height],
                width: bounds.width,
            },
            move |image| {
                let png = image
                    .ok_or("the web view gave no picture")
                    .and_then(|image| png_of(&image, full).ok_or("the picture could not be made"));
                into.replace(Some(png));
            },
        );
        slot
    }
}

/// What the web view said, on one line and not long: it is an error's description.
fn short(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(120)
        .collect()
}

/// `image` as a PNG: its pixels (sRGB, opaque) brought to the size [`picture_size`] says.
fn png_of(image: &CGImage, full: bool) -> Option<Vec<u8>> {
    let source = (
        u32::try_from(CGImage::width(Some(image))).ok()?,
        u32::try_from(CGImage::height(Some(image))).ok()?,
    );
    if source.0 == 0 || source.1 == 0 {
        return None;
    }
    let (width, height) = picture_size(source, full);
    let (w, h) = (width as usize, height as usize);
    let mut data = vec![0u8; w * h * 4];
    // SAFETY: Core Graphics' constant name for the sRGB space, alive for the process.
    let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))?;
    // SAFETY: `data` is `w * h * 4` bytes, four to a pixel and rows tight, and is alive while the
    // context is (it is dropped before `data` is read).
    let context = unsafe {
        CGBitmapContextCreate(
            data.as_mut_ptr().cast::<c_void>(),
            w,
            h,
            8,
            w * 4,
            Some(&space),
            CGImageAlphaInfo::NoneSkipLast.0,
        )
    }?;
    CGContext::set_interpolation_quality(Some(&context), CGInterpolationQuality::High);
    CGContext::draw_image(
        Some(&context),
        CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w as f64, h as f64)),
        Some(image),
    );
    drop(context);
    // The fourth byte of a pixel is skipped by the context: the picture is opaque.
    for pixel in data.as_chunks_mut::<4>().0 {
        pixel[3] = 255;
    }
    bt_core::encode_png(width, height, &data)
}

// ─── A request carried out ──────────────────────────────────────────────────────────────────────

/// Where a request stands.
enum Stage {
    /// Waiting for the program's frame to say it is there.
    Ready,
    /// The first look is asked for: what the action names is found in it.
    Before(Slot),
    /// The action is asked for.
    Acting(Slot),
    /// Looking until the page is at rest: the look that is out, if one is, and when the next may be.
    Settling { out: Option<Slot>, next: Instant },
    /// The picture is asked for.
    Shooting(PictureSlot),
}

/// What the person's pasteboard held when a request that drives the page began: every type of every
/// item, to be put back if the page wrote the pasteboard meanwhile (a Copy button does, as the
/// web view runs the agent's call as a person's gesture).
struct Kept {
    board: Retained<NSPasteboard>,
    count: isize,
    items: Vec<Vec<(Retained<NSString>, Retained<NSData>)>>,
}

impl Kept {
    fn take() -> Self {
        let board = NSPasteboard::generalPasteboard();
        let count = board.changeCount();
        let mut items = Vec::new();
        if let Some(found) = board.pasteboardItems() {
            for item in &found {
                let mut kept = Vec::new();
                for kind in &item.types() {
                    if let Some(data) = item.dataForType(&kind) {
                        kept.push((kind.clone(), data));
                    }
                }
                items.push(kept);
            }
        }
        Self {
            board,
            count,
            items,
        }
    }

    /// Whether the pasteboard was written since, and so put back as it was.
    fn put_back(&self) -> bool {
        if self.board.changeCount() == self.count {
            return false;
        }
        self.board.clearContents();
        let items: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = self
            .items
            .iter()
            .map(|kept| {
                let item = NSPasteboardItem::new();
                for (kind, data) in kept {
                    item.setData_forType(data, kind);
                }
                ProtocolObject::from_retained(item)
            })
            .collect();
        if !items.is_empty() {
            self.board
                .writeObjects(&NSArray::from_retained_slice(&items));
        }
        true
    }
}

/// One `bateri view` on a web view, carried out.
pub struct WebRun {
    host: ViewHost,
    /// The pane's session, marked active while the request is: the pane turns a file panel the
    /// page asks for away meanwhile, as it does for a native page.
    session: Shared,
    /// What the pasteboard held before the page was acted on, where the action drives it.
    kept: Option<Kept>,
    /// Whether the page wrote the pasteboard and it was put back.
    copied: bool,
    view: View,
    name: String,
    program: Option<String>,
    began: Instant,
    /// When the page began to be waited on for rest or for the words of a `wait`: after the
    /// frame was there and the action was done, so that a frame that came late does not use up
    /// the time the page has to settle in.
    settling: Instant,
    /// When the call or the look that is out was asked, for the time it may take.
    asked: Instant,
    stage: Stage,
    /// What the first look read: the element the action names is told by it.
    before: Option<Reading>,
    /// What was done, once it was.
    said: Option<String>,
}

impl WebRun {
    /// A request on the view `host` is: the verbs a web view does not have are refused at once.
    ///
    /// # Errors
    ///
    /// The action is not one a web view has.
    pub(crate) fn begin(
        host: ViewHost,
        session: Shared,
        view: &View,
        name: &str,
        program: Option<&str>,
    ) -> Result<Self, Failure> {
        if let Some(failure) = web::unsupported(&view.action) {
            return Err(failure);
        }
        let now = Instant::now();
        session.borrow_mut().active = drives(&view.action);
        Ok(Self {
            host,
            session,
            kept: None,
            copied: false,
            view: view.clone(),
            name: name.to_owned(),
            program: program.map(str::to_owned),
            began: now,
            settling: now,
            asked: now,
            stage: Stage::Ready,
            before: None,
            said: None,
        })
    }

    /// What the page did to the pasteboard is undone: the pasteboard is the person's.
    fn put_back(&mut self) {
        if let Some(kept) = self.kept.take()
            && kept.put_back()
        {
            self.copied = true;
        }
    }

    fn failed(&self, text: impl Into<String>) -> Step {
        Step::Answer(Reply::Failed(Failure::new(Kind::Failed, text)))
    }

    /// The time a request has run, in seconds.
    fn elapsed(&self) -> f64 {
        self.began.elapsed().as_secs_f64()
    }

    /// The time the page has been waited on to settle, in seconds.
    fn waited(&self) -> f64 {
        self.settling.elapsed().as_secs_f64()
    }

    /// What a failed call says: the view's script could not run, with what the web view said.
    fn script_failed(&self, why: &str) -> Step {
        self.failed(format!(
            "the view of {} could not be read just now ({}): ask again in a moment",
            self.name,
            short(why)
        ))
    }

    /// The text a call of the script answered with, once it has, and the failure if the call
    /// failed or ran too long.
    fn answered(&self, slot: &Slot) -> Option<Result<String, Step>> {
        match slot.borrow_mut().take() {
            Some(Ok(text)) => Some(Ok(text)),
            Some(Err(why)) => Some(Err(self.script_failed(&why))),
            None if self.asked.elapsed().as_secs_f64() > CALL_LIMIT => {
                Some(Err(self.failed(format!(
                    "the view of {} did not answer in {CALL_LIMIT} seconds: its page may be busy",
                    self.name
                ))))
            }
            None => None,
        }
    }

    fn look(&mut self) -> Option<Slot> {
        self.asked = Instant::now();
        self.host
            .agent_call("return window.__bateriAgent.snapshot();")
    }

    /// The call of the script for what the plan does.
    fn call_of(act: &Act) -> Option<String> {
        let agent = "window.__bateriAgent";
        Some(match act {
            Act::Nothing => return None,
            Act::Click { number, times } => format!("return {agent}.click({number}, {times});"),
            Act::Fill { number, text } => {
                format!("return {agent}.fill({number}, {});", js_string(text))
            }
            Act::Type { text } => format!("return {agent}.type({});", js_string(text)),
            Act::Press(keys) => {
                let list: Vec<String> = keys
                    .iter()
                    .map(|key| {
                        format!(
                            "[{}, {}, {}, {}, {}, {}]",
                            js_string(&key.key),
                            js_string(&key.code),
                            key.shift,
                            key.control,
                            key.alt,
                            key.meta
                        )
                    })
                    .collect();
                format!("return {agent}.press([{}]);", list.join(", "))
            }
            Act::Scroll {
                number,
                way,
                screens,
            } => {
                let way = match way {
                    Way::Up => "up",
                    Way::Down => "down",
                    Way::Top => "top",
                    Way::Bottom => "bottom",
                };
                format!("return {agent}.scroll({number}, \"{way}\", {screens});")
            }
        })
    }

    /// Looks at the request once more: [`Step::Pending`] while it waits, the answer when it is over.
    /// The caller has seen to it that the pane is open and still shows the view.
    pub fn step(&mut self) -> Step {
        match std::mem::replace(&mut self.stage, Stage::Ready) {
            Stage::Ready => {
                if !self.host.agent_there() {
                    if self.elapsed() > READY_LIMIT {
                        return self.failed(format!(
                            "the view of {} has not shown its page yet: ask again in a moment",
                            self.name
                        ));
                    }
                    self.stage = Stage::Ready;
                    return Step::Pending;
                }
                match self.look() {
                    Some(slot) => {
                        self.stage = Stage::Before(slot);
                        Step::Pending
                    }
                    None => self.failed(format!("the view of {} is not there to read", self.name)),
                }
            }
            Stage::Before(slot) => {
                let text = match self.answered(&slot) {
                    None => {
                        self.stage = Stage::Before(slot);
                        return Step::Pending;
                    }
                    Some(Err(step)) => return step,
                    Some(Ok(text)) => text,
                };
                let reading = match web::parse(&text) {
                    Ok(reading) => reading,
                    Err(why) => return self.failed(why),
                };
                let planned = match web::plan(&self.view.action, &reading) {
                    Ok(planned) => planned,
                    Err(failure) => return Step::Answer(Reply::Failed(failure)),
                };
                self.before = Some(reading);
                self.said = planned.said;
                match Self::call_of(&planned.act) {
                    Some(body) => {
                        self.kept = Some(Kept::take());
                        self.asked = Instant::now();
                        match self.host.agent_call(&body) {
                            Some(slot) => {
                                self.stage = Stage::Acting(slot);
                                Step::Pending
                            }
                            None => self.failed(format!(
                                "the view of {} is not there to act on",
                                self.name
                            )),
                        }
                    }
                    None => {
                        self.settling = Instant::now();
                        self.stage = Stage::Settling {
                            out: None,
                            next: Instant::now(),
                        };
                        Step::Pending
                    }
                }
            }
            Stage::Acting(slot) => {
                let text = match self.answered(&slot) {
                    None => {
                        self.stage = Stage::Acting(slot);
                        return Step::Pending;
                    }
                    Some(Err(step)) => return step,
                    Some(Ok(text)) => text,
                };
                if let Some(rest) = text.strip_prefix("err\t") {
                    let (code, detail) = rest.split_once('\t').unwrap_or((rest, ""));
                    let what = self
                        .before
                        .as_ref()
                        .and_then(|before| web::named(&self.view.action, before))
                        .unwrap_or_else(|| "the element".to_owned());
                    return Step::Answer(Reply::Failed(web::failure(code, detail, &what)));
                }
                self.settling = Instant::now();
                self.stage = Stage::Settling {
                    out: None,
                    next: Instant::now() + Duration::from_secs_f64(LOOK_EVERY),
                };
                Step::Pending
            }
            Stage::Settling { out, next } => self.settle(out, next),
            Stage::Shooting(slot) => {
                let done = slot.borrow_mut().take();
                match done {
                    Some(Ok(png)) => Step::Answer(Reply::Picture(png)),
                    Some(Err(why)) => self.failed(format!("no picture of {}: {why}", self.name)),
                    None if self.asked.elapsed().as_secs_f64() > PICTURE_LIMIT => self.failed(
                        format!("no picture of {}: the web view did not give one", self.name),
                    ),
                    None => {
                        self.stage = Stage::Shooting(slot);
                        Step::Pending
                    }
                }
            }
        }
    }

    /// Looking until the page is at rest, or the wait has run out.
    fn settle(&mut self, out: Option<Slot>, next: Instant) -> Step {
        let Some(slot) = out else {
            if Instant::now() < next {
                self.stage = Stage::Settling { out: None, next };
                return Step::Pending;
            }
            return match self.look() {
                Some(slot) => {
                    self.stage = Stage::Settling {
                        out: Some(slot),
                        next,
                    };
                    Step::Pending
                }
                None => self.failed(format!("the view of {} is not there to read", self.name)),
            };
        };
        let text = match self.answered(&slot) {
            None => {
                self.stage = Stage::Settling {
                    out: Some(slot),
                    next,
                };
                return Step::Pending;
            }
            Some(Err(step)) => return step,
            Some(Ok(text)) => text,
        };
        let reading = match web::parse(&text) {
            Ok(reading) => reading,
            Err(why) => return self.failed(why),
        };
        let elapsed = self.waited();
        let again = |this: &mut Self| {
            this.stage = Stage::Settling {
                out: None,
                next: Instant::now() + Duration::from_secs_f64(LOOK_EVERY),
            };
            Step::Pending
        };
        match self.view.action.clone() {
            Action::Wait {
                text,
                gone,
                seconds,
            } => {
                let shows = reading.text().contains(text.as_str());
                if shows != gone {
                    return self.answer(&reading);
                }
                let limit = f64::from(seconds.unwrap_or(WAIT_DEFAULT));
                if elapsed >= limit {
                    let what = if gone { "to go" } else { "to show" };
                    return self.failed(format!(
                        "timed out after {limit} seconds waiting for \"{}\" {what} in the view of {}",
                        text.chars().take(60).collect::<String>(),
                        self.name
                    ));
                }
                again(self)
            }
            Action::Shot { full } => {
                if reading.rest.resting() || elapsed >= REST_LIMIT {
                    self.asked = Instant::now();
                    self.stage = Stage::Shooting(self.host.agent_picture(full));
                    return Step::Pending;
                }
                again(self)
            }
            _ => {
                if reading.rest.resting() || elapsed >= REST_LIMIT {
                    return self.answer(&reading);
                }
                again(self)
            }
        }
    }

    fn answer(&mut self, reading: &Reading) -> Step {
        self.put_back();
        let notes = if self.copied {
            vec![
                "the page copied text: the pasteboard is the person's and was put back as it was"
                    .to_owned(),
            ]
        } else {
            Vec::new()
        };
        let format: Format = self.view.format;
        Step::Answer(Reply::Text(written(
            Pane {
                name: &self.name,
                program: self.program.as_deref(),
            },
            reading,
            format,
            self.said.as_deref(),
            &notes,
        )))
    }
}

impl Drop for WebRun {
    /// A request that ends, answered or not (the pane closed, the view gone), lets the pane's
    /// panel through again and puts back what the page wrote.
    fn drop(&mut self) {
        self.put_back();
        self.session.borrow_mut().active = false;
    }
}
