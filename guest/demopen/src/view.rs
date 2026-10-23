//! The conversation of `demopen` with its terminal (specification: the capability query, the
//! open with a bundle whose entry is a WebAssembly component, the messages of the view, the
//! end), and with the view once it runs.
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

/// The name of the module in the bundle.
const ENTRY: &str = "containers.wasm";

/// How often the made-up host reports.
const EVERY: Duration = Duration::from_secs(1);

#[derive(Serialize)]
struct Open<'a> {
    token: &'a str,
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
}

/// Shows the view and serves it until it is over.
pub fn run(module: &[u8]) -> Outcome {
    let Ok(tty) = Tty::take() else {
        return Outcome::NoView;
    };
    let mut talk = Conversation {
        tty,
        module,
        ids: Ids::new(),
        decoder: StreamDecoder::new(Direction::ToProgram),
        opened: None,
        token: String::new(),
        subscription: None,
        tick: 0,
        next_push: Instant::now(),
    };
    let outcome = talk.go();
    talk.tty.give_back();
    outcome
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
                payload: r#"{"versions":[1]}"#,
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
                    answer = Some(offer_of(&message));
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
            program: "demopen",
            title: "Containers",
            bundle: Bundle {
                entry: ENTRY,
                files: [File {
                    path: ENTRY,
                    sha256: Digest::of(self.module).to_string(),
                    size: self.module.len(),
                    media_type: pencere::bundle::WASM_MEDIA_TYPE,
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
        terminal::write_all(&bytes).map_err(|_| ())?;
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
            "closed" => Some(Outcome::Ended),
            // The terminal started again and asks for the view again (specification §10.7):
            // the view is a new one and asks for what it needs afresh.
            "welcome" if message.re().is_none() => {
                let offer = offer_of(message);
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
            Message::Call(call) => self.say(&Message::failure(
                call.id,
                "not-found",
                "this program does nothing when called",
            )),
            Message::Response(_) | Message::Push(_) | Message::Progress(_) => Ok(()),
        };
        sent.is_err().then_some(Outcome::Ended)
    }

    /// The reading of this second, if the view listens and it is time.
    fn push_if_due(&mut self) -> Result<(), ()> {
        let Some(id) = self.subscription else {
            return Ok(());
        };
        if Instant::now() < self.next_push {
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

    /// Ends the view and says why (a `bye` with the token it was opened with).
    fn bye(&mut self, reason: &str) {
        let Ok(payload) = json::to_wire(&Bye {
            token: &self.token,
            reason,
        }) else {
            return;
        };
        let _ = self.send("bye", None, None, &payload);
    }
}

/// The token of a `welcome` that offers one for a host that shows a component: version and
/// token, and `bundle` and `wasm` among the features. `None` for any other.
fn offer_of(message: &Frame) -> Option<String> {
    let welcome: Value = serde_json::from_str(&message.payload).ok()?;
    welcome.get("version")?;
    let token = welcome.get("token")?.as_str()?.to_owned();
    let features = welcome.get("features")?.as_array()?;
    let has = |name: &str| features.iter().any(|f| f.as_str() == Some(name));
    (has("bundle") && has("wasm")).then_some(token)
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
            r#"{{"version":1,"token":"abc",{host},"features":["bundle","dismiss","wasm"]}}"#
        );
        let web =
            format!(r#"{{"version":1,"token":"abc",{host},"features":["bundle","dismiss"]}}"#);
        let none = format!(r#"{{"supported":[2],{host}}}"#);
        assert_eq!(offer_of(&welcome(&wasm)).as_deref(), Some("abc"));
        assert_eq!(offer_of(&welcome(&web)), None);
        assert_eq!(offer_of(&welcome(&none)), None);
        assert_eq!(offer_of(&welcome("not json")), None);
    }

    #[test]
    fn the_answer_to_the_fence_is_told_from_noise() {
        assert!(replied_to_the_fence(b"\x1b[?62;4c"));
        assert!(replied_to_the_fence(b"ls\r\x1b[?1;2c"));
        assert!(!replied_to_the_fence(b"\x1b[?62;4"));
        assert!(!replied_to_the_fence(b"ls\r"));
    }
}
