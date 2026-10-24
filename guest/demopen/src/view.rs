//! The conversation of `demopen` with its terminal (specification: the capability query, the
//! open with a bundle whose entry is a WebAssembly component or a Lua script, the messages of
//! the view, the end), and with the view once it runs.
//!
//! Everything on the wire is `pencere`'s own code: the frames are written by the crate that
//! reads them, and the digest and the base64 of the module are its too.
//!
//! **Debt**: this is the first program to speak the protocol from Rust, and there is no program
//! library yet (terminal modes, the handshake and the clean exit are written here by hand). It
//! is the shape such a library would take, not the library.

use std::time::{Duration, Instant};

use bateri_view_messages::{Message, Refused};
use pencere::base64;
use pencere::bundle::Digest;
use pencere::frame::{
    Direction, Event, Ids, Message as Frame, Outgoing, Split, StreamDecoder, encode,
};
use pencere::json;
use serde::Serialize;
use serde_json::{Value, json};

use crate::terminal::{self, Tty};

/// How the conversation ended.
pub enum Outcome {
    /// The view ran and is over: the program closed it, the person did, or the terminal ended.
    Ended,
    /// ⌃C (or another signal): the view was ended with a `bye` and the terminal is given back.
    Interrupted,
    /// There is no view to show: the terminal does not speak the protocol, does not show this
    /// kind of view, or refused. The terminal is as it was found.
    NoView,
}

/// How long a terminal that speaks the protocol has to answer a `hello` (specification §5.3).
const ANSWER_WITHIN: Duration = Duration::from_secs(2);

/// How long a program that said `bye` reads on for the answer before it gives up (specification
/// §10.3).
const ANSWER_TO_BYE: Duration = Duration::from_millis(500);

/// How often the made-up host reports.
const EVERY: Duration = Duration::from_secs(1);

/// A kind of view this program can show (specification §5.5), the version of its interface the
/// program is written against, and the file it is the bundle of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kind {
    /// What the `hello` lists and the `open` names.
    pub name: &'static str,
    /// The version of the kind's interface: what the `hello` lists under the name and the `open`
    /// names.
    version: &'static str,
    /// The name of the file in the bundle.
    entry: &'static str,
    /// Its media type.
    media_type: &'static str,
}

impl Kind {
    /// The view is a WebAssembly component.
    pub const WASM: Self = Self {
        name: "wasm",
        version: "0.5",
        entry: "containers.wasm",
        media_type: pencere::bundle::WASM_MEDIA_TYPE,
    };

    /// The view is a script in the Lua language.
    pub const LUA: Self = Self {
        name: "lua",
        version: "1.0",
        entry: "containers.lua",
        media_type: pencere::bundle::LUA_MEDIA_TYPE,
    };
}

#[derive(Serialize)]
struct Open<'a> {
    token: &'a str,
    view: &'a str,
    version: &'a str,
    program: &'a str,
    title: &'a str,
    bundle: Bundle<'a>,
}

#[derive(Serialize)]
struct Bundle<'a> {
    entry: &'a str,
    files: [File<'a>; 1],
}

#[derive(Serialize)]
struct File<'a> {
    path: &'a str,
    sha256: String,
    size: usize,
    #[serde(rename = "type")]
    media_type: &'a str,
}

#[derive(Serialize)]
struct Bye<'a> {
    token: &'a str,
    reason: &'a str,
}

struct Conversation<'a> {
    tty: Tty,
    /// The kind of view shown, and its file.
    kind: Kind,
    module: &'a [u8],
    ids: Ids,
    decoder: StreamDecoder,
    /// The id of the `open` that is out, for the `re` of what answers it.
    opened: Option<u32>,
    /// The token the view was opened with (a `bye` carries it).
    token: String,
    /// The subscription of the view, once it has asked, and the reading it hears next.
    subscription: Option<u64>,
    tick: u32,
    next_push: Instant,
    /// Push as fast as the terminal takes it, not once a second.
    flood: bool,
    /// Stop pushing at this time, as a Docker that hangs would.
    stall: Option<Instant>,
    /// Bytes of data the view's `upload` calls have carried so far.
    uploaded: u64,
    /// Why the view failed, in the words the terminal gave when it told the program.
    failure: Option<String>,
}

/// Shows the view (`module` is the file of a view of `kind`) and serves it until it is over. With
/// `flood` the readings come as fast as the terminal takes them, with `stall` they stop after that
/// long. The bytes the view sent with its `upload` calls come back with how it ended, and the
/// words the terminal gave if the view failed: they are for the person once the terminal is given
/// back.
pub fn run(
    kind: Kind,
    module: &[u8],
    flood: bool,
    stall: Option<Duration>,
) -> (Outcome, u64, Option<String>) {
    let Ok(tty) = Tty::take() else {
        return (Outcome::NoView, 0, None);
    };
    let mut talk = Conversation {
        tty,
        kind,
        module,
        ids: Ids::new(),
        decoder: StreamDecoder::new(Direction::ToProgram),
        opened: None,
        token: String::new(),
        subscription: None,
        tick: 0,
        next_push: Instant::now(),
        flood,
        stall: stall.map(|after| Instant::now() + after),
        uploaded: 0,
        failure: None,
    };
    let outcome = talk.go();
    talk.tty.give_back();
    (outcome, talk.uploaded, talk.failure)
}

impl Conversation<'_> {
    fn go(&mut self) -> Outcome {
        let Some(token) = self.ask() else {
            return Outcome::NoView;
        };
        if self.tty.enter_alternate().is_err() {
            return Outcome::NoView;
        }
        if self.open(&token).is_err() {
            return Outcome::NoView;
        }
        self.serve()
    }

    /// The capability query: a `hello`, then `CSI c` as the fence. The token of a host that
    /// shows this kind of view, or `None` — there is no host, it shares no version, it does not
    /// show components, or it said nothing in time.
    fn ask(&mut self) -> Option<String> {
        let hello = self.ids.next_id();
        let mut bytes = Vec::new();
        encode(
            Direction::ToHost,
            &Outgoing {
                kind: "hello",
                id: hello,
                re: None,
                digest: None,
                payload: &hello_payload(self.kind),
            },
            Split::Fill,
            &mut bytes,
        )
        .ok()?;
        bytes.extend_from_slice(b"\x1b[c");
        terminal::write_all(&bytes).ok()?;

        let deadline = Instant::now() + ANSWER_WITHIN;
        let mut seen = Vec::new();
        let mut buffer = [0u8; 4096];
        let mut answer = None;
        while answer.is_none() && !terminal::signalled() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            let wait = i32::try_from(left.as_millis().min(100)).unwrap_or(100);
            let Ok(got) = terminal::read(&mut buffer, wait) else {
                break;
            };
            let Some(got) = got else { continue };
            if got == 0 {
                break;
            }
            let mut events = Vec::new();
            self.decoder.feed(&buffer[..got], &mut events);
            for event in events {
                if let Event::Message(message) = event
                    && message.kind == "welcome"
                    && message.re() == Some(hello)
                {
                    answer = Some(offer_of(&message, self.kind));
                    break;
                }
            }
            if answer.is_some() {
                break;
            }
            // The terminal's own answer to `CSI c` (`ESC [ ? … c`) says nobody else will.
            seen.extend_from_slice(&buffer[..got]);
            if replied_to_the_fence(&seen) {
                break;
            }
            let keep = seen.len().saturating_sub(64);
            seen.drain(..keep);
        }
        match answer {
            Some(Some(token)) => Some(token),
            // A welcome that cannot be used, or no welcome: no view. A late answer to the
            // fence is not left for the shell to read as typed text (specification §5.3).
            _ => {
                self.discard_the_reply_to_the_fence(&seen);
                None
            }
        }
    }

    fn discard_the_reply_to_the_fence(&mut self, seen: &[u8]) {
        if replied_to_the_fence(seen) {
            return;
        }
        let until = Instant::now() + Duration::from_millis(400);
        let mut buffer = [0u8; 256];
        let mut tail = seen.to_vec();
        while Instant::now() < until {
            match terminal::read(&mut buffer, 50) {
                Ok(Some(0)) | Err(_) => return,
                Ok(Some(got)) => {
                    tail.extend_from_slice(&buffer[..got]);
                    if replied_to_the_fence(&tail) {
                        return;
                    }
                }
                Ok(None) => {}
            }
        }
    }

    /// Sends the `open` of the view with its module as the bundle.
    fn open(&mut self, token: &str) -> Result<(), ()> {
        self.token = token.to_owned();
        self.subscription = None;
        let open = Open {
            token,
            view: self.kind.name,
            version: self.kind.version,
            program: "demopen",
            title: "Containers",
            bundle: Bundle {
                entry: self.kind.entry,
                files: [File {
                    path: self.kind.entry,
                    sha256: Digest::of(self.module).to_string(),
                    size: self.module.len(),
                    media_type: self.kind.media_type,
                }],
            },
        };
        let payload = json::to_wire(&open).map_err(|_| ())?;
        self.opened = Some(self.send("open", None, None, &payload)?);
        Ok(())
    }

    /// Writes a message of the protocol; its id.
    fn send(
        &mut self,
        kind: &str,
        re: Option<u32>,
        digest: Option<&str>,
        payload: &str,
    ) -> Result<u32, ()> {
        let id = self.ids.next_id();
        let mut bytes = Vec::new();
        encode(
            Direction::ToHost,
            &Outgoing {
                kind,
                id,
                re,
                digest,
                payload,
            },
            Split::Fill,
            &mut bytes,
        )
        .map_err(|_| ())?;
        if terminal::signalled() {
            // Asked to end: the last words are said if the terminal takes them soon.
            terminal::write_all_within(&bytes, terminal::FAREWELL).map_err(|_| ())?;
        } else {
            terminal::write_all(&bytes).map_err(|_| ())?;
        }
        Ok(id)
    }

    /// A message of the view's protocol to the view.
    fn say(&mut self, message: &Message) -> Result<(), ()> {
        self.send("msg", None, None, &json::escape(&message.text()))
            .map(|_| ())
    }

    fn serve(&mut self) -> Outcome {
        let mut buffer = [0u8; 4096];
        loop {
            if terminal::signalled() {
                self.bye("signal");
                return Outcome::Interrupted;
            }
            let wait = self.wait();
            match terminal::read(&mut buffer, wait) {
                Err(_) | Ok(Some(0)) => return self.over(),
                Ok(None) => {}
                Ok(Some(got)) => {
                    let mut events = Vec::new();
                    self.decoder.feed(&buffer[..got], &mut events);
                    for event in events {
                        let Event::Message(message) = event else {
                            continue;
                        };
                        if let Some(outcome) = self.on_message(&message) {
                            return outcome;
                        }
                    }
                }
            }
            if self.push_if_due().is_err() {
                return self.over();
            }
        }
    }

    /// The terminal stopped answering or taking what is written: if a signal asked the program
    /// to end it ends as for one, else the view is over.
    fn over(&mut self) -> Outcome {
        if terminal::signalled() {
            self.bye("signal");
            Outcome::Interrupted
        } else {
            Outcome::Ended
        }
    }

    /// How long to wait for input: until the next reading is due, or a moment (so a signal is
    /// seen soon).
    fn wait(&self) -> i32 {
        let moment = Duration::from_millis(100);
        if self.flood && self.subscription.is_some() {
            // Only a look at the input: the next reading is due already.
            return 0;
        }
        let left = if self.subscription.is_some() {
            self.next_push
                .saturating_duration_since(Instant::now())
                .min(moment)
        } else {
            moment
        };
        i32::try_from(left.as_millis()).unwrap_or(100)
    }

    fn on_message(&mut self, message: &Frame) -> Option<Outcome> {
        match message.kind.as_str() {
            "need" if message.re() == self.opened => {
                let missing: Value = serde_json::from_str(&message.payload).unwrap_or(Value::Null);
                let ours = Digest::of(self.module).to_string();
                let wanted = missing["missing"]
                    .as_array()
                    .is_some_and(|list| list.iter().any(|d| d.as_str() == Some(ours.as_str())));
                if wanted {
                    let payload = base64::encode(self.module);
                    if self.send("file", None, Some(&ours), &payload).is_err() {
                        return Some(Outcome::Ended);
                    }
                }
                None
            }
            // The terminal refused the open, or failed to start the view: it never ran, and
            // the list is shown as text.
            "closed" if self.opened.is_some() && message.re() == self.opened => {
                Some(Outcome::NoView)
            }
            // The view is over, for whatever reason: the person closed it, or it failed. The
            // program has nothing more to show.
            "closed" => {
                self.failure = failure_of(&message.payload);
                Some(Outcome::Ended)
            }
            // The terminal started again and asks for the view again (specification §10.7):
            // the view is a new one and asks for what it needs afresh.
            "welcome" if message.re().is_none() => {
                let offer = offer_of(message, self.kind);
                match offer {
                    Some(token) if self.open(&token).is_ok() => None,
                    _ => Some(Outcome::Ended),
                }
            }
            "msg" => self.on_view(&message.payload),
            _ => None,
        }
    }

    /// What the view said. It is not trusted: a message that is not one of the six is left.
    fn on_view(&mut self, text: &str) -> Option<Outcome> {
        let message = match Message::parse(text) {
            Ok(message) => message,
            Err(Refused::TooLong | Refused::Malformed | Refused::Ambiguous | Refused::NoId) => {
                return None;
            }
        };
        let sent = match message {
            Message::Subscribe(subscribe) if subscribe.topic == "stats" => {
                self.subscription = Some(subscribe.id);
                // The first reading is the view's at once; the next ones are a second apart.
                self.next_push = Instant::now();
                Ok(())
            }
            Message::Subscribe(subscribe) => self.say(&Message::failure(
                subscribe.id,
                "not-found",
                "there is nothing to subscribe to by that name",
            )),
            Message::Cancel(cancel) => {
                if self.subscription == Some(cancel.id) {
                    self.subscription = None;
                }
                Ok(())
            }
            Message::Call(call) if call.method == "upload" => self.on_upload(&call),
            Message::Call(call) => self.say(&Message::failure(
                call.id,
                "not-found",
                "this program does nothing when called",
            )),
            Message::Response(_) | Message::Push(_) | Message::Progress(_) => Ok(()),
        };
        sent.is_err().then_some(Outcome::Ended)
    }

    /// A chunk of the payload the view sends: counted, and the last one is answered with how
    /// much there has been. The view says nothing of how it chunks; the count is the program's.
    fn on_upload(&mut self, call: &bateri_view_messages::Call) -> Result<(), ()> {
        let data = call
            .params
            .get("data")
            .and_then(Value::as_str)
            .map_or(0, str::len);
        self.uploaded += data as u64;
        if call.params.get("last").and_then(Value::as_bool) == Some(true) {
            return self.say(&Message::answer(call.id, json!({ "bytes": self.uploaded })));
        }
        Ok(())
    }

    /// The reading of this second, if the view listens and it is time.
    fn push_if_due(&mut self) -> Result<(), ()> {
        let Some(id) = self.subscription else {
            return Ok(());
        };
        if !self.flood && Instant::now() < self.next_push {
            return Ok(());
        }
        if self.stall.is_some_and(|at| Instant::now() >= at) {
            return Ok(());
        }
        let readings: Vec<Value> = bt_view::demo::readings(self.tick)
            .into_iter()
            .map(|r| json!({ "name": r.name, "cpu": r.cpu, "memory": r.memory, "share": r.share }))
            .collect();
        self.tick = self.tick.wrapping_add(1);
        self.next_push = Instant::now() + EVERY;
        self.say(&Message::push(id, json!({ "readings": readings })))
    }

    /// Ends the view and says why (a `bye` with the token it was opened with), then does what
    /// the protocol asks of a program that ends (specification §10.3): reads, and throws away,
    /// what the terminal has queued up to its answer, and throws away what is left. A frame the
    /// terminal wrote before it learned of the `bye` must not be left in the tty for the shell
    /// that reads it next.
    fn bye(&mut self, reason: &str) {
        let sent = json::to_wire(&Bye {
            token: &self.token,
            reason,
        })
        .map_err(|_| ())
        .and_then(|payload| self.send("bye", None, None, &payload));
        if let Ok(bye) = sent {
            self.read_up_to_the_answer_to(bye);
        }
        terminal::discard_input();
    }

    /// Reads and drops whatever comes until the terminal's `closed` that answers `bye`, or the
    /// wait is over.
    fn read_up_to_the_answer_to(&mut self, bye: u32) {
        let until = Instant::now() + ANSWER_TO_BYE;
        let mut buffer = [0u8; 4096];
        while let Some(left) = until.checked_duration_since(Instant::now()) {
            let wait = i32::try_from(left.as_millis().min(50)).unwrap_or(50);
            match terminal::read(&mut buffer, wait) {
                Err(_) | Ok(Some(0)) => return,
                Ok(None) => {}
                Ok(Some(got)) => {
                    let mut events = Vec::new();
                    self.decoder.feed(&buffer[..got], &mut events);
                    let answered = events.iter().any(|event| {
                        matches!(event, Event::Message(message)
                            if message.kind == "closed" && message.re() == Some(bye))
                    });
                    if answered {
                        return;
                    }
                }
            }
        }
    }
}

/// The `hello`: the versions of the wire this program speaks and the kinds of view it can show
/// (the one it carries), each with the versions of its interface the program is written against.
fn hello_payload(kind: Kind) -> String {
    json::to_wire(&json!({
        "versions": [1],
        "views": [{ "kind": kind.name, "versions": [kind.version] }],
    }))
    .unwrap_or_default()
}

/// The token of a `welcome` that offers one for a host that shows this program's kind of view at
/// the version the program is written against: a version and a token, `bundle` among the
/// features and the program's kind among the views with that version listed. `None` for any
/// other — a host that shares no version, takes no bundles, or whose `views` is empty or has no
/// kind of this program's, or not at its version: the list is shown as text at once.
fn offer_of(message: &Frame, kind: Kind) -> Option<String> {
    let welcome: Value = serde_json::from_str(&message.payload).ok()?;
    welcome.get("version")?;
    let token = welcome.get("token")?.as_str()?.to_owned();
    let features = welcome.get("features")?.as_array()?;
    let views = welcome.get("views")?.as_array()?;
    let shows = views.iter().any(|view| {
        view.get("kind").and_then(Value::as_str) == Some(kind.name)
            && view
                .get("versions")
                .and_then(Value::as_array)
                .is_some_and(|versions| versions.iter().any(|v| v.as_str() == Some(kind.version)))
    });
    (features.iter().any(|f| f.as_str() == Some("bundle")) && shows).then_some(token)
}

/// The words a `closed` gives for a view that failed (`reason` `error` with a `message`), made fit
/// for a terminal that is going to print them: the terminal has cleaned them already, and a
/// program does not rely on that.
fn failure_of(payload: &str) -> Option<String> {
    let closed: Value = serde_json::from_str(payload).ok()?;
    if closed.get("reason")?.as_str()? != "error" {
        return None;
    }
    let words: String = closed
        .get("message")?
        .as_str()?
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    (!words.is_empty()).then_some(words)
}

/// Whether `bytes` hold a terminal's answer to `CSI c`: `ESC [ ? … c`.
fn replied_to_the_fence(bytes: &[u8]) -> bool {
    let Some(at) = bytes.windows(3).position(|w| w == b"\x1b[?") else {
        return false;
    };
    bytes[at + 3..].iter().take(48).any(|b| *b == b'c')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn welcome(payload: &str) -> Frame {
        Frame {
            kind: "welcome".into(),
            id: 1,
            control: vec![("re".into(), "1".into())],
            payload: payload.into(),
        }
    }

    #[test]
    fn a_terminal_that_shows_components_offers_its_token_and_no_other_does() {
        let host = r#""host":{"name":"t","version":"0"}"#;
        let wasm = format!(
            r#"{{"version":1,"token":"abc",{host},"features":["bundle","dismiss"],"views":[{{"kind":"wasm","versions":["0.5"]}},{{"kind":"web","versions":["1.0"]}}]}}"#
        );
        let web = format!(
            r#"{{"version":1,"token":"abc",{host},"features":["bundle","dismiss"],"views":[{{"kind":"web","versions":["1.0"]}}]}}"#
        );
        let nothing = format!(
            r#"{{"version":1,"token":"abc",{host},"features":["bundle","dismiss"],"views":[]}}"#
        );
        let no_bundles = format!(
            r#"{{"version":1,"token":"abc",{host},"features":["dismiss"],"views":[{{"kind":"wasm","versions":["0.5"]}}]}}"#
        );
        // The kind, but not at the version this program is written against.
        let other_version = format!(
            r#"{{"version":1,"token":"abc",{host},"features":["bundle","dismiss"],"views":[{{"kind":"wasm","versions":["0.4"]}}]}}"#
        );
        let none = format!(r#"{{"supported":[2],{host}}}"#);
        let wasm_only = Kind::WASM;
        assert_eq!(offer_of(&welcome(&wasm), wasm_only).as_deref(), Some("abc"));
        assert_eq!(offer_of(&welcome(&web), wasm_only), None);
        assert_eq!(offer_of(&welcome(&nothing), wasm_only), None);
        assert_eq!(offer_of(&welcome(&no_bundles), wasm_only), None);
        assert_eq!(offer_of(&welcome(&other_version), wasm_only), None);
        assert_eq!(offer_of(&welcome(&none), wasm_only), None);
        assert_eq!(offer_of(&welcome("not json"), wasm_only), None);
        // A terminal that runs components and not scripts does not offer a token for a script.
        assert_eq!(offer_of(&welcome(&wasm), Kind::LUA), None);
        let lua = format!(
            r#"{{"version":1,"token":"abc",{host},"features":["bundle","dismiss"],"views":[{{"kind":"lua","versions":["1.0"]}},{{"kind":"web","versions":["1.0"]}}]}}"#
        );
        assert_eq!(offer_of(&welcome(&lua), Kind::LUA).as_deref(), Some("abc"));
    }

    #[test]
    fn the_words_of_a_view_that_failed_are_read_from_the_closed_and_nothing_else_is() {
        let failed = r#"{"reason":"error","code":"view","message":"script:12: boom\u001b[2J"}"#;
        assert_eq!(failure_of(failed).as_deref(), Some("script:12: boom[2J"));
        assert_eq!(failure_of(r#"{"reason":"user"}"#), None);
        assert_eq!(failure_of(r#"{"reason":"error","code":"view"}"#), None);
        assert_eq!(failure_of(r#"{"reason":"error","message":""}"#), None);
        assert_eq!(failure_of("not json"), None);
    }

    #[test]
    fn the_hello_lists_the_one_kind_of_view_this_program_shows_with_its_version() {
        assert_eq!(
            hello_payload(Kind::WASM),
            r#"{"versions":[1],"views":[{"kind":"wasm","versions":["0.5"]}]}"#
        );
        assert_eq!(
            hello_payload(Kind::LUA),
            r#"{"versions":[1],"views":[{"kind":"lua","versions":["1.0"]}]}"#
        );
    }

    /// The size of a message toward the program is the protocol's, and the messages crate and
    /// the sandbox hold to the same number.
    #[test]
    fn a_message_is_held_to_the_protocols_limit() {
        assert_eq!(
            bateri_view_messages::MAX_BYTES,
            pencere::session::MAX_VIEW_MESSAGE
        );
    }

    #[test]
    fn the_answer_to_the_fence_is_told_from_noise() {
        assert!(replied_to_the_fence(b"\x1b[?62;4c"));
        assert!(replied_to_the_fence(b"ls\r\x1b[?1;2c"));
        assert!(!replied_to_the_fence(b"\x1b[?62;4"));
        assert!(!replied_to_the_fence(b"ls\r"));
    }
}
