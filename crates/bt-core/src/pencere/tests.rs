//! The desk's rules, with the real parser and the crate's own wire: a program
//! is played by writing frames into a pane, and what the desk answers is what
//! reaches the pane's input.
//!
//! The helpers here (`pub(crate)`) are the reader's tests' as well: the order of
//! a `welcome` and a `CSI c` reply is only the reader loop's to keep.

use std::sync::atomic::{AtomicUsize, Ordering};

use ::pencere_host::frame::{
    Direction, Event as FrameEvent, Outgoing, Split, StreamDecoder, encode,
};
use alacritty_terminal::event::{Event as TermEvent, EventListener};
use alacritty_terminal::grid::Dimensions as _;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::vte::ansi::{Handler as _, Processor};

use super::*;
use crate::graphics::{Commands, Hub, Sink};
use crate::handler::{ClusterHandler, KeptModes};
use crate::pointer::PointerShapes;

/// The bytes the pane's input has received, in order: the desk's and the
/// terminal's own answers.
pub(crate) type Input = Arc<Mutex<Vec<u8>>>;

/// The terminal's own answers (`CSI c`'s) go to the same input as the desk's.
#[derive(Clone)]
pub(crate) struct Answers(pub(crate) Input);

impl EventListener for Answers {
    fn send_event(&self, event: TermEvent) {
        if let TermEvent::PtyWrite(text) = event {
            lock(&self.0).extend_from_slice(text.as_bytes());
        }
    }
}

/// Counts [`Wake::view_changed`]; every other call is nobody's here.
#[derive(Default)]
pub(crate) struct CountingWake {
    pub(crate) changes: AtomicUsize,
}

impl Wake for CountingWake {
    fn wake(&self) {}
    fn child_exit(&self, _: Option<i32>) {}
    fn copy_to_clipboard(&self, _: String) {}
    fn title_changed(&self) {}
    fn search_changed(&self) {}
    fn command_started(&self) {}
    fn phase_edge(&self) {}
    fn remote_command_edge(&self) {}
    fn mirror_changed(&self) {}
    fn environment_changed(&self) {}
    fn remote_up(&self) {}
    fn remote_typed(&self) {}
    fn link_hover_lost(&self) {}
    fn pointer_changed(&self) {}
    fn blocks_changed(&self) {}
    fn unseen_changed(&self) {}
    fn program_status_changed(&self) {}
    fn notification(&self) {}
    fn view_changed(&self) {
        self.changes.fetch_add(1, Ordering::AcqRel);
    }
}

/// What the pane is seen as, for the test to change.
pub(crate) type Seen = Arc<Mutex<Observed>>;

/// The graphics side of a pane, with or without a desk, and where its answers
/// go.
pub(crate) struct Rig {
    hub: Hub,
    modes: KeptModes,
    pointer: PointerShapes,
    pub(crate) desk: Option<Desk>,
    pub(crate) input: Input,
    /// What the desk counts of what passes between the program and the module.
    pub(crate) flow: crate::flow::Flow,
}

impl Sink for Rig {
    fn hub(&self) -> &Hub {
        &self.hub
    }

    fn cell(&self) -> (u16, u16) {
        (10, 20)
    }

    fn reply(&self, bytes: Vec<u8>) {
        lock(&self.input).extend_from_slice(&bytes);
    }

    fn kept_modes(&self) -> &KeptModes {
        &self.modes
    }

    fn pointer(&self) -> &PointerShapes {
        &self.pointer
    }

    fn pointer_changed(&self) {}

    fn desk(&self) -> Option<&Desk> {
        self.desk.as_ref()
    }
}

impl Rig {
    /// A rig whose desk, if it has one, sees the pane as `seen` says; the input
    /// is the one the terminal's answers go to as well.
    pub(crate) fn new(views: bool) -> (Arc<Self>, Seen, Arc<CountingWake>) {
        let seen: Seen = Arc::default();
        let observed = Arc::clone(&seen);
        let (rig, wake) = Self::build(
            views.then(|| -> Observer { Box::new(move || *lock(&observed)) }),
            None,
            Discard::nothing(),
        );
        (rig, seen, wake)
    }

    /// A rig whose desk throws the program's leftovers away with `discard`.
    pub(crate) fn with_discard(discard: Discard) -> (Arc<Self>, Arc<CountingWake>) {
        Self::build(Some(Box::new(Observed::default)), None, discard)
    }

    /// A rig with a desk whose observer is `observer`: for a test that looks at
    /// what is held when the desk reads the pane.
    pub(crate) fn with_observer(observer: Observer) -> (Arc<Self>, Arc<CountingWake>) {
        Self::build(Some(observer), None, Discard::nothing())
    }

    /// A rig whose desk shows WebAssembly views of up to `limit` bytes.
    pub(crate) fn with_modules(limit: u64) -> (Arc<Self>, Arc<CountingWake>) {
        Self::build(
            Some(Box::new(Observed::default)),
            Some(limit),
            Discard::nothing(),
        )
    }

    fn build(
        observer: Option<Observer>,
        wasm_entry_limit: Option<u64>,
        discard: Discard,
    ) -> (Arc<Self>, Arc<CountingWake>) {
        let wake = Arc::new(CountingWake::default());
        let flow = crate::flow::Flow::new();
        let desk = observer.map(|observer| {
            let mut options = ViewOptions::new("bateri-test", "0.0.0");
            options.native_entry_limit = wasm_entry_limit;
            options.size = Size {
                width: 800,
                height: 600,
            };
            Desk::new(
                options,
                Theme::BATERI,
                wake.clone(),
                observer,
                flow.clone(),
                discard,
            )
        });
        let rig = Arc::new(Self {
            hub: Hub::new(),
            modes: KeptModes::default(),
            pointer: PointerShapes::default(),
            desk,
            input: Arc::default(),
            flow,
        });
        (rig, wake)
    }

    /// Writes into the pane's input, as the application does.
    pub(crate) fn write(&self) -> impl Fn(Vec<u8>) + '_ {
        |bytes| lock(&self.input).extend_from_slice(&bytes)
    }

    /// The frames the desk wrote since the last call, decoded as a program reads
    /// them; the terminal's answers between them are noise to the decoder.
    pub(crate) fn written(&self) -> Vec<Message> {
        let bytes = mem::take(&mut *lock(&self.input));
        decoded(&bytes)
    }
}

/// The messages in `bytes` as a program reads what a host writes.
pub(crate) fn decoded(bytes: &[u8]) -> Vec<Message> {
    let mut events = Vec::new();
    StreamDecoder::new(Direction::ToProgram).feed(bytes, &mut events);
    events
        .into_iter()
        .filter_map(|event| match event {
            FrameEvent::Message(message) => Some(message),
            FrameEvent::Error(_) => None,
        })
        .collect()
}

/// A frame as a program writes it.
pub(crate) fn wire(
    kind: &str,
    id: u32,
    re: Option<u32>,
    digest: Option<&str>,
    payload: &str,
) -> Vec<u8> {
    let mut out = Vec::new();
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
        &mut out,
    )
    .expect("a frame the wire allows");
    out
}

/// The program's `hello`.
pub(crate) fn hello() -> Vec<u8> {
    wire(
        "hello",
        1,
        None,
        None,
        r#"{"versions":[1],"views":[{"kind":"wasm","versions":["1.0"]},{"kind":"web","versions":["1.0"]}]}"#,
    )
}

/// What follows `"key":` in JSON text, up to the end of the text.
fn after<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let at = text.find(&format!("\"{key}\":"))?;
    Some(&text[at + key.len() + 3..])
}

/// The token a `welcome` offers.
pub(crate) fn token_of(welcome: &Message) -> String {
    let rest = after(&welcome.payload, "token").expect("a token");
    let rest = rest.strip_prefix('"').expect("a string");
    rest[..rest.find('"').expect("its end")].to_owned()
}

/// A pane as the reader has it: `Term`, the parser, the stream's commands and the
/// rig. Bytes go in as the reader's loop takes them: the parser runs to a halt,
/// the parked frame is heard with nothing held, the parser goes on.
pub(crate) struct Pane {
    pub(crate) term: Term<Answers>,
    parser: Processor,
    commands: Commands,
    last_input: bool,
    pub(crate) rig: Arc<Rig>,
}

impl Pane {
    pub(crate) fn new(rig: Arc<Rig>) -> Self {
        let term = Term::new(
            alacritty_terminal::term::Config::default(),
            &TermSize::new(40, 6),
            Answers(Arc::clone(&rig.input)),
        );
        Self {
            term,
            parser: Processor::new(),
            commands: Commands::default(),
            last_input: false,
            rig,
        }
    }

    pub(crate) fn feed(&mut self, bytes: &[u8]) {
        let sink: &dyn Sink = &*self.rig;
        let mut at = 0;
        loop {
            let mut handler = ClusterHandler::new(&mut self.term, false, &mut self.last_input)
                .graphics(Some(sink))
                .commands(&mut self.commands);
            at += self.parser.advance_until_halt(&mut handler, &bytes[at..]);
            let halted = handler.halt_requested();
            drop(handler);
            if !halted {
                break;
            }
            sink.settle_views();
            ClusterHandler::new(&mut self.term, false, &mut self.last_input)
                .graphics(Some(sink))
                .commands(&mut self.commands)
                .resume();
        }
        assert_eq!(
            at,
            bytes.len(),
            "the parser left bytes it was not halted for"
        );
    }

    /// The screen's rows, trailing blanks off.
    pub(crate) fn text(&self) -> Vec<String> {
        text_of(&self.term)
    }
}

fn text_of(term: &Term<Answers>) -> Vec<String> {
    (0..term.screen_lines())
        .map(|line| {
            let row: String = (0..term.columns())
                .map(|column| term.grid()[Line(i32::try_from(line).unwrap_or(0))][Column(column)].c)
                .collect();
            row.trim_end().to_owned()
        })
        .collect()
}

/// The page of the one-file bundle the tests open.
const PAGE: &[u8] = b"<html></html>";

/// `CSI ? 1049 h`: the program takes the alternate screen.
pub(crate) const ALTERNATE_SCREEN: &[u8] = b"\x1b[?1049h";

/// The program's `open` of a view of a one-file bundle, with `token`: the frames
/// are written on the alternate screen, as a program that shows a view has taken
/// the pane's, as vim does.
pub(crate) fn bundle_open(token: &str) -> Vec<u8> {
    [ALTERNATE_SCREEN, &open_of_bundle(token, "")].concat()
}

/// The same, for a page that asks for the address `domain` to connect to: the person is asked
/// before the view starts.
pub(crate) fn bundle_open_declaring(token: &str, domain: &str) -> Vec<u8> {
    [
        ALTERNATE_SCREEN,
        &open_of_bundle(token, &format!(r#","access":{{"connect":["{domain}"]}}"#)),
    ]
    .concat()
}

/// The `open` frame of the one-file bundle, with `extra` (members of the `open` after `title`, or
/// nothing), written wherever the pane is.
fn open_of_bundle(token: &str, extra: &str) -> Vec<u8> {
    let digest = Digest::of(PAGE).to_string();
    let open = format!(
        concat!(
            r#"{{"token":"{token}","view":"web","version":"1.0","program":"containers","#,
            r#""title":"Containers"{extra},"bundle":{{"entry":"index.html","files":"#,
            r#"[{{"path":"index.html","sha256":"{digest}","size":{size},"type":"text/html"}}]}}}}"#
        ),
        token = token,
        digest = digest,
        size = PAGE.len(),
        extra = extra
    );
    wire("open", 2, None, None, &open)
}

/// The program's `file` of the page.
pub(crate) fn page_file() -> Vec<u8> {
    wire(
        "file",
        3,
        None,
        Some(&Digest::of(PAGE).to_string()),
        &::pencere_host::base64::encode(PAGE),
    )
}

/// The path to a started view: the program says `hello`, opens a view of a
/// one-file bundle and sends the file. Returns the token the view was opened with.
pub(crate) fn open_a_view(pane: &mut Pane) -> String {
    pane.feed(&hello());
    let welcome = pane.rig.written();
    assert_eq!(welcome.len(), 1, "{welcome:?}");
    assert_eq!(welcome[0].kind, "welcome");
    assert_eq!(welcome[0].re(), Some(1));
    let token = token_of(&welcome[0]);

    pane.feed(&bundle_open(&token));
    let need = pane.rig.written();
    assert_eq!(need.len(), 1, "{need:?}");
    assert_eq!((need[0].kind.as_str(), need[0].re()), ("need", Some(2)));
    assert!(
        need[0].payload.contains(&Digest::of(PAGE).to_string()),
        "{}",
        need[0].payload
    );

    // No session to start up: the page is there to start as soon as its file is.
    pane.feed(&page_file());
    assert!(pane.rig.written().is_empty());
    token
}

fn wake_count(wake: &CountingWake) -> usize {
    wake.changes.load(Ordering::Acquire)
}

// ---- the stream -----------------------------------------------------------

#[test]
fn a_hello_is_answered_with_a_welcome_and_a_token() {
    let (rig, _, wake) = Rig::new(true);
    let mut pane = Pane::new(rig);
    pane.feed(&hello());
    let written = pane.rig.written();
    assert_eq!(written.len(), 1);
    assert_eq!(
        (written[0].kind.as_str(), written[0].re()),
        ("welcome", Some(1))
    );
    assert_eq!(token_of(&written[0]).len(), 22, "16 bytes in base64url");
    assert!(written[0].payload.contains("\"bateri-test\""));
    // Every view arrives as a bundle, so there is no feature to list for it (§5.2): the host
    // lists what it may or may not do, and closing at the program's request is that.
    assert!(
        written[0].payload.contains(r#""features":["dismiss"]"#),
        "{}",
        written[0].payload
    );
    assert!(
        !written[0].payload.contains("bundle"),
        "{}",
        written[0].payload
    );
    assert_eq!(
        wake_count(&wake),
        0,
        "bytes for the program are not the shell's news"
    );
}

#[test]
fn a_pane_without_views_hears_nothing_and_answers_nothing() {
    let (rig, _, wake) = Rig::new(false);
    let mut pane = Pane::new(rig);
    pane.feed(&[b"before ".as_slice(), &hello(), b" after"].concat());
    assert!(lock(&pane.rig.input).is_empty());
    assert_eq!(pane.text()[0], "before  after", "the frame is no text");
    assert_eq!(wake_count(&wake), 0);
}

#[test]
fn the_full_opening_ends_with_a_view_for_the_shell_and_two_events() {
    let (rig, _, wake) = Rig::new(true);
    let mut pane = Pane::new(rig);
    open_a_view(&mut pane);
    assert_eq!(
        wake_count(&wake),
        2,
        "one event for the wait that began with the open, one for the view that is to start"
    );
    let desk = pane.rig.desk.as_ref().expect("a desk");
    let views = desk.take_views();
    assert_eq!(views.len(), 1, "{views:?}");
    let Action::StartView(start) = &views[0] else {
        panic!("a view to start, not {:?}", views[0]);
    };
    assert_eq!(start.kind, ViewKind::Web);
    assert_eq!(start.version, WEB_BRIDGE_VERSION);
    assert_eq!(start.program.as_deref(), Some("containers"));
    assert_eq!(start.title.as_deref(), Some("Containers"));
    assert!(
        start.policy.contains("default-src 'none'"),
        "{}",
        start.policy
    );
    // The first `context` the page is told is the crate's, at the area the application gave
    // and in the roles of the theme.
    let context = start.context.as_deref().expect("a context for a page");
    assert!(context.starts_with(r#"{"context":{"mode":"dark","colors":{"background":""#));
    assert!(
        context.contains(r#""size":{"width":800,"height":600}"#),
        "{context}"
    );
    assert!(!context.contains("access"), "{context}");
    assert!(desk.take_views().is_empty(), "taken once");
    // The bytes of the bundle's page are served from the store.
    let served = desk.apply(|_| {}, |conversation| conversation.serve("index.html"));
    let (media, bytes) = served.expect("the entry is served");
    assert_eq!(
        (media.as_str(), &*bytes),
        ("text/html", &b"<html></html>"[..])
    );
    assert_eq!(
        desk.apply(|_| {}, |conversation| conversation.state()),
        State::Open
    );
}

#[test]
fn a_burst_for_the_shell_is_one_event() {
    let (rig, _, wake) = Rig::new(true);
    let mut pane = Pane::new(rig);
    open_a_view(&mut pane);
    assert_eq!(wake_count(&wake), 2);
    let desk = pane.rig.desk.as_ref().expect("a desk");
    // Two things for the view in a row, nobody has taken the first yet (the view's start is
    // still there, and what the page is told of the theme and the font comes after it).
    desk.set_theme(
        Theme {
            background: 0x102030,
            ..Theme::BATERI
        },
        |_| {},
    );
    desk.set_font(Some("Fira Code"), |_| {});
    assert_eq!(
        wake_count(&wake),
        2,
        "no second event while the slot is full"
    );
    desk.take_views();
    desk.apply(|_| {}, |conversation| conversation.close_view());
    assert_eq!(wake_count(&wake), 3, "emptied, and there is news again");
}

#[test]
fn the_alternate_screen_is_the_one_where_the_open_ended() {
    // The program takes the alternate screen and opens in the same read.
    let (rig, _, _) = Rig::new(true);
    let mut pane = Pane::new(rig);
    pane.feed(&hello());
    let token = token_of(&pane.rig.written()[0]);
    pane.feed(&bundle_open(&token));
    let written = pane.rig.written();
    assert_eq!(written.len(), 1, "{written:?}");
    assert_eq!(
        written[0].kind, "need",
        "an open on the alternate screen is accepted: {written:?}"
    );

    // Not on it: refused with `screen`.
    let (rig, _, _) = Rig::new(true);
    let mut pane = Pane::new(rig);
    pane.feed(&hello());
    let token = token_of(&pane.rig.written()[0]);
    pane.feed(&open_of_bundle(&token, ""));
    let written = pane.rig.written();
    assert_eq!(written.len(), 1, "{written:?}");
    assert_eq!(
        (written[0].kind.as_str(), written[0].re()),
        ("closed", Some(2))
    );
    assert!(
        written[0].payload.contains("\"screen\""),
        "{}",
        written[0].payload
    );
}

#[test]
fn the_screen_of_the_moment_is_the_frames_and_not_a_later_one() {
    // The open is written on the alternate screen and the program leaves it in
    // the same read: the open was written where it was, and is accepted.
    let (rig, _, _) = Rig::new(true);
    let mut pane = Pane::new(rig);
    pane.feed(&hello());
    let token = token_of(&pane.rig.written()[0]);
    pane.feed(
        &[
            b"\x1b[?1049h".as_slice(),
            &open_of_bundle(&token, ""),
            b"\x1b[?1049l",
        ]
        .concat(),
    );
    let written = pane.rig.written();
    assert!(written.iter().any(|m| m.kind == "need"), "{written:?}");
}

#[test]
fn a_frame_while_a_prompt_is_edited_is_ignored() {
    let (rig, seen, _) = Rig::new(true);
    let mut pane = Pane::new(rig);
    lock(&seen).prompt_editing = true;
    pane.feed(&hello());
    assert!(
        pane.rig.written().is_empty(),
        "nothing is answered into a prompt"
    );
    lock(&seen).prompt_editing = false;
    pane.feed(&hello());
    assert_eq!(
        pane.rig.written().len(),
        1,
        "the check is read at every frame"
    );
}

#[test]
fn a_hello_is_not_answered_to_a_shell_in_the_foreground_or_a_tty_that_echoes() {
    for observed in [
        Observed {
            shell_in_foreground: Some(true),
            ..Observed::default()
        },
        Observed {
            tty_canonical_or_echo: Some(true),
            ..Observed::default()
        },
    ] {
        let (rig, seen, _) = Rig::new(true);
        let mut pane = Pane::new(rig);
        *lock(&seen) = observed;
        pane.feed(&hello());
        assert!(pane.rig.written().is_empty(), "{observed:?}");
    }
}

#[test]
fn the_right_token_in_a_bye_ends_the_view_and_the_pane_is_written_nothing_after() {
    let (rig, _, wake) = Rig::new(true);
    let mut pane = Pane::new(rig);
    let token = open_a_view(&mut pane);
    let rig = Arc::clone(&pane.rig);
    let desk = rig.desk.as_ref().expect("a desk");
    desk.take_views();
    assert_eq!(wake_count(&wake), 2);

    // Another token, or none: nothing changes.
    for payload in [
        r#"{"token":"AAAAAAAAAAAAAAAAAAAAAA","reason":"signal"}"#.to_owned(),
        r#"{"reason":"signal"}"#.to_owned(),
    ] {
        pane.feed(&wire("bye", 6, None, None, &payload));
        assert!(
            desk.take_views().is_empty(),
            "a bye that is not the program's"
        );
        assert_eq!(
            desk.apply(|_| {}, |conversation| conversation.state()),
            State::Open
        );
    }

    let bye = format!(r#"{{"token":"{token}","reason":"signal"}}"#);
    pane.feed(&wire("bye", 7, None, None, &bye));
    assert_eq!(desk.take_views(), [Action::DestroyView]);
    assert_eq!(wake_count(&wake), 3);
    assert_eq!(
        desk.apply(|_| {}, |conversation| conversation.state()),
        State::Ended
    );
    // The program waits for one answer to its `bye`, and reads up to it (§10.3).
    let answer = pane.rig.written();
    assert_eq!(answer.len(), 1, "{answer:?}");
    assert_eq!(
        (
            answer[0].kind.as_str(),
            answer[0].re(),
            answer[0].payload.as_str()
        ),
        ("closed", Some(7), r#"{"reason":"program"}"#)
    );
    // The program is gone: the person's close, or anything else, writes nothing.
    desk.apply(pane.rig.write(), |conversation| conversation.close_view());
    desk.program_ended(pane.rig.write());
    assert!(lock(&pane.rig.input).is_empty(), "{:?}", pane.rig.written());
}

#[test]
fn the_end_of_the_command_closes_the_view_and_writes_nothing() {
    let (rig, _, wake) = Rig::new(true);
    let mut pane = Pane::new(rig);
    open_a_view(&mut pane);
    let desk = pane.rig.desk.as_ref().expect("a desk");
    desk.take_views();

    desk.program_ended(pane.rig.write());

    assert_eq!(desk.take_views(), [Action::DestroyView]);
    assert_eq!(wake_count(&wake), 3);
    assert!(lock(&pane.rig.input).is_empty(), "{:?}", pane.rig.written());
}

#[test]
fn a_wait_that_begins_or_ends_is_news_for_the_shell_that_has_the_clock() {
    let (rig, _, wake) = Rig::new(true);
    let mut pane = Pane::new(rig);
    pane.feed(&hello());
    let token = token_of(&pane.rig.written()[0]);
    assert_eq!(
        wake_count(&wake),
        0,
        "an offer waits for nothing the shell ends"
    );

    // An `open` that is answered with a `need` and nothing else: nothing is kept
    // for the shell, but its files are waited for, and only the shell has a clock.
    pane.feed(&bundle_open(&token));
    assert_eq!(pane.rig.written().len(), 1, "the `need`");
    let desk = pane.rig.desk.as_ref().expect("a desk");
    assert_eq!(wake_count(&wake), 1, "the wait began");
    assert!(desk.take_views().is_empty(), "and nothing is kept for it");
    assert_eq!(
        desk.apply(|_| {}, |conversation| conversation.state()),
        State::Opening
    );

    // The files do not come: the shell's wait ran out and the open ends.
    desk.apply(pane.rig.write(), |conversation| conversation.timed_out());
    let written = pane.rig.written();
    assert_eq!(written.len(), 1, "{written:?}");
    assert_eq!(written[0].kind, "closed");
    assert!(
        written[0].payload.contains("timeout"),
        "{}",
        written[0].payload
    );
    assert_eq!(
        desk.apply(|_| {}, |conversation| conversation.state()),
        State::Idle
    );
    assert_eq!(
        wake_count(&wake),
        2,
        "the wait ended: the shell forgets the deadline it set"
    );
    assert!(desk.take_views().is_empty());
}

#[test]
fn a_restart_offers_a_new_token_to_the_program_that_had_a_view() {
    let (rig, seen, _) = Rig::new(true);
    let mut pane = Pane::new(rig);
    let token = open_a_view(&mut pane);
    let desk = pane.rig.desk.as_ref().expect("a desk");
    desk.take_views();
    // As after a restart: the pane is a program's again, no shell around it.
    *lock(&seen) = Observed::default();

    desk.restart(pane.rig.write());
    // The view is torn down first (at once, whatever its kind); the program is offered the
    // restart after it.
    let written = pane.rig.written();
    let offer = written
        .iter()
        .find(|message| message.kind == "welcome")
        .expect("an unsolicited welcome");
    assert_eq!(offer.re(), None);
    assert!(
        offer.payload.contains("\"restart\":true"),
        "{}",
        offer.payload
    );
    assert_ne!(token_of(offer), token, "a new token");
    assert_eq!(desk.take_views(), [Action::DestroyView]);
}

// ---- a stream with no desk ------------------------------------------------

/// A stream that has no desk — a bare replay of the bytes —
/// builds the screen the live one builds and writes nothing.
#[test]
fn a_replay_with_no_desk_builds_the_same_screen_and_writes_nothing() {
    let stream = [
        b"one\r\n".as_slice(),
        &hello(),
        b"two\r\n",
        &wire("msg", 2, None, None, r#"{"call":{"id":9,"method":"ping"}}"#),
        b"three",
    ]
    .concat();

    let (live, _, _) = Rig::new(true);
    let mut live = Pane::new(live);
    live.feed(&stream);
    assert!(!lock(&live.rig.input).is_empty(), "the live pane answers");

    // A replay with commands and no sink.
    let input: Input = Arc::default();
    let mut term = Term::new(
        alacritty_terminal::term::Config::default(),
        &TermSize::new(40, 6),
        Answers(Arc::clone(&input)),
    );
    let mut parser: Processor = Processor::new();
    let (mut commands, mut last_input) = (Commands::default(), false);
    parser.advance(
        &mut ClusterHandler::new(&mut term, false, &mut last_input).commands(&mut commands),
        &stream,
    );
    assert!(!commands.halted(), "nothing is waiting to be heard");
    assert!(lock(&input).is_empty(), "a replay answers nobody");
    assert_eq!(text_of(&term), live.text());

    // A replay with no commands either.
    let mut term = Term::new(
        alacritty_terminal::term::Config::default(),
        &TermSize::new(40, 6),
        Answers(Arc::clone(&input)),
    );
    let mut parser: Processor = Processor::new();
    let mut last_input = false;
    parser.advance(
        &mut ClusterHandler::new(&mut term, false, &mut last_input),
        &stream,
    );
    assert!(lock(&input).is_empty());
    assert_eq!(text_of(&term), live.text());
    assert_eq!(&live.text()[..3], ["one", "two", "three"]);
}

// ---- the token's source ---------------------------------------------------

#[test]
fn a_token_source_that_fails_opens_no_view() {
    let (rig, _, _) = Rig::new(true);
    let mut pane = Pane::new(rig);
    pane.rig
        .desk
        .as_ref()
        .expect("a desk")
        .entropy_failed
        .store(true, Ordering::Release);
    pane.feed(&hello());
    let token = token_of(&pane.rig.written()[0]);
    pane.feed(&bundle_open(&token));
    let written = pane.rig.written();
    assert_eq!(written.len(), 1, "{written:?}");
    assert!(
        written[0].payload.contains("\"policy\""),
        "{}",
        written[0].payload
    );
}

#[test]
fn tokens_come_from_the_system_and_differ() {
    let failed = Arc::new(AtomicBool::new(false));
    let mut entropy = SystemEntropy {
        failed: Arc::clone(&failed),
    };
    let (one, two) = (entropy.token_bytes(), entropy.token_bytes());
    assert_ne!(one, two);
    assert_ne!(one, [0; 16]);
    assert!(!failed.load(Ordering::Acquire));
}

// ---- what the pane is seen as ---------------------------------------------

#[test]
fn a_local_pane_is_seen_by_its_tty_and_a_remote_pane_by_the_shells_marks_alone() {
    let tty = Some(Tty {
        foreground: Some(500),
        canonical_or_echo: true,
    });
    // Local: the foreground group is the shell's own, the tty echoes.
    assert_eq!(
        observed(false, false, tty, Some(500)),
        Observed {
            prompt_editing: false,
            alternate_screen: None,
            shell_in_foreground: Some(true),
            tty_canonical_or_echo: Some(true),
        }
    );
    // A program's own group.
    assert_eq!(
        observed(false, false, tty, Some(400)).shell_in_foreground,
        Some(false)
    );
    // The prompt is the shell's mark and goes through whatever the tty is.
    assert!(observed(true, false, tty, Some(400)).prompt_editing);
    // The tty is `ssh`'s, not the program's: neither check is made.
    let remote = observed(true, true, tty, Some(400));
    assert_eq!(
        (remote.shell_in_foreground, remote.tty_canonical_or_echo),
        (None, None)
    );
    assert!(
        remote.prompt_editing,
        "the remote shell's mark still stands"
    );
    // A tty that could not be read, or has no foreground group: not checked.
    let none = observed(false, false, None, Some(400));
    assert_eq!(
        (none.shell_in_foreground, none.tty_canonical_or_echo),
        (None, None)
    );
    let ungrouped = Some(Tty {
        foreground: None,
        canonical_or_echo: false,
    });
    let seen = observed(false, false, ungrouped, Some(400));
    assert_eq!(
        (seen.shell_in_foreground, seen.tty_canonical_or_echo),
        (None, Some(false))
    );
}

#[test]
fn a_shell_that_cannot_be_found_is_not_a_shell_out_of_the_foreground() {
    // `login(1)` has not forked its shell, or the table cannot be read: whether the shell
    // is in front is not known. "No" would answer a `hello` into a line editor.
    let tty = Some(Tty {
        foreground: Some(500),
        canonical_or_echo: false,
    });
    assert_eq!(observed(false, false, tty, None).shell_in_foreground, None);
}

#[test]
fn the_group_the_shell_leads_is_the_childs_own_only_where_the_child_is_the_shell() {
    // Where the child is the shell: its pid is its group's.
    assert_eq!(Shell::new(None, 700).group(), Some(700));

    // Where the child is `login(1)` (macOS): the shell is its child, in a group of its
    // own, and the terminal's foreground group is compared with that, not with `login`'s
    // pid — which is what the check compared with until the child was named a shell.
    let asked = Arc::new(AtomicUsize::new(0));
    let first_nothing = Arc::clone(&asked);
    let login = Shell::new(
        Some(ShellGroup::new(move |child| {
            assert_eq!(child, 700, "asked of the pane's child");
            // The first time the shell is not forked yet.
            (first_nothing.fetch_add(1, Ordering::AcqRel) > 0).then_some(701)
        })),
        700,
    );
    assert_eq!(login.group(), None, "not forked yet: not known");
    assert_eq!(login.group(), Some(701));
    assert_eq!(login.group(), Some(701));
    assert_eq!(
        asked.load(Ordering::Acquire),
        2,
        "found once, then remembered: the table is not read on every frame"
    );
    // A shell in the foreground of the terminal is seen as one.
    let tty = Some(Tty {
        foreground: Some(701),
        canonical_or_echo: false,
    });
    assert_eq!(
        observed(false, false, tty, login.group()).shell_in_foreground,
        Some(true)
    );
    assert_eq!(
        observed(false, false, tty, Some(700)).shell_in_foreground,
        Some(false),
        "the child's own pid names the wrong group, and said the shell was not in front"
    );
}

#[test]
fn the_ttys_modes_are_read_from_the_masters_copy() {
    // A pseudo-terminal of the test's own: the master's `tcgetattr` is the slave's.
    // SAFETY: plain calls on descriptors this test opens and closes.
    unsafe {
        let master = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
        assert!(master >= 0, "a pseudo-terminal");
        assert_eq!(libc::grantpt(master), 0);
        assert_eq!(libc::unlockpt(master), 0);
        let name = libc::ptsname(master);
        assert!(!name.is_null());
        let slave = libc::open(name, libc::O_RDWR | libc::O_NOCTTY);
        assert!(slave >= 0);
        let mut modes: libc::termios = mem::zeroed();
        assert_eq!(libc::tcgetattr(slave, &mut modes), 0);

        let borrowed = BorrowedFd::borrow_raw(master);
        modes.c_lflag |= libc::ICANON | libc::ECHO;
        assert_eq!(libc::tcsetattr(slave, libc::TCSANOW, &modes), 0);
        assert!(read_tty(borrowed).expect("modes").canonical_or_echo);

        modes.c_lflag &= !(libc::ICANON | libc::ECHO);
        assert_eq!(libc::tcsetattr(slave, libc::TCSANOW, &modes), 0);
        assert!(!read_tty(borrowed).expect("modes").canonical_or_echo);

        // Echo alone is enough to be "not left".
        modes.c_lflag |= libc::ECHO;
        assert_eq!(libc::tcsetattr(slave, libc::TCSANOW, &modes), 0);
        assert!(read_tty(borrowed).expect("modes").canonical_or_echo);

        libc::close(slave);
        libc::close(master);
    }
}

// ---- the look -------------------------------------------------------------

#[test]
fn the_view_is_given_the_themes_own_values_and_the_same_dark_or_light() {
    for name in Theme::embedded_names() {
        let theme = Theme::embedded(name).expect("an embedded theme");
        let view = view_theme(&theme, DEFAULT_FONT_MONO);
        let rgb = |hex: u32| {
            let [r, g, b] = srgb_bytes(hex);
            Rgb(r, g, b)
        };
        assert_eq!(view.foreground, rgb(theme.foreground), "{name}");
        assert_eq!(view.dim, Some(rgb(theme.dim)), "{name}");
        assert_eq!(view.accent, Some(rgb(theme.accent)), "{name}");
        // The caret's colour is the theme's `cursor`, not the accent a page would be given
        // when told none: the two are separate roles of the theme.
        assert_eq!(view.cursor, Some(rgb(theme.cursor)), "{name}");
        assert_eq!(view.success, Some(rgb(theme.success)), "{name}");
        assert_eq!(view.error, Some(rgb(theme.error)), "{name}");
        assert_eq!(view.warning, Some(rgb(theme.warning)), "{name}");
        assert_eq!(view.info, Some(rgb(theme.info)), "{name}");
        // The palette and the selection are the grid's: a page that draws in the terminal's
        // red gets the red a program's red text has.
        assert_eq!(view.selection, Some(rgb(theme.selection)), "{name}");
        assert_eq!(view.ansi, Some(theme.ansi.map(rgb)), "{name}");
        let [r, g, b] = theme.background_srgb();
        assert_eq!(view.background, Rgb(r, g, b), "{name}");
        // The pane's dividers are the view's borders: one step, one rule.
        let [r, g, b] = theme.separator_srgb();
        assert_eq!(view.border, Some(Rgb(r, g, b)), "{name}");
        // The window's look comes from `Theme::is_dark`; a view that called the
        // same theme the other way would sit in a pane of the wrong kind.
        assert_eq!(view.is_dark(), theme.is_dark(), "{name}");
    }
}

#[test]
fn a_font_family_is_first_and_cannot_end_its_quotes() {
    assert_eq!(font_stack(None), DEFAULT_FONT_MONO);
    assert_eq!(font_stack(Some("   ")), DEFAULT_FONT_MONO);
    assert_eq!(
        font_stack(Some("JetBrains Mono")),
        format!("\"JetBrains Mono\", {DEFAULT_FONT_MONO}")
    );
    let hostile = font_stack(Some("Evil\"; } body { display: none; /*"));
    assert_eq!(
        hostile.matches('"').count(),
        2 + 2,
        "the family's own pair, then the system's"
    );
    assert!(!hostile.contains('\\'));
    assert!(
        hostile.starts_with("\"Evil; } body { display: none; /*\""),
        "{hostile}"
    );
}

// ---- the stores -----------------------------------------------------------

#[test]
fn the_in_memory_store_drops_the_oldest_and_never_the_file_just_kept() {
    let mut store = Bounded::new(100);
    let (a, b, c) = (
        Verified::from_bytes(vec![1; 60]),
        Verified::from_bytes(vec![2; 60]),
        Verified::from_bytes(vec![3; 150]),
    );
    let digests = [*a.digest(), *b.digest(), *c.digest()];
    store.insert(a);
    store.insert(b.clone());
    assert!(!store.contains(&digests[0]), "the oldest went");
    assert!(store.contains(&digests[1]));
    store.insert(b);
    assert_eq!(store.files.len(), 1, "the same file is not kept twice");
    // One file over the limit by itself is kept: the open it is for asks again otherwise.
    store.insert(c);
    assert!(store.contains(&digests[2]));
    assert!(!store.contains(&digests[1]));
    assert_eq!(store.total, 150);
}

#[test]
fn clones_of_a_shelf_are_one_store() {
    let mut one = Shelf::in_memory();
    let two = one.clone();
    let file = Verified::from_bytes(b"shared".to_vec());
    let digest = *file.digest();
    assert!(!two.contains(&digest));
    one.insert(file);
    assert!(two.contains(&digest));
    assert_eq!(two.get(&digest).as_deref(), Some(&b"shared"[..]));
    assert_eq!(format!("{one:?}"), "Shelf(..)");
}

#[test]
fn the_systems_random_bytes_are_one_helper_for_the_token_and_the_views_origin() {
    let (one, two) = (
        system_random().expect("bytes"),
        system_random().expect("bytes"),
    );
    assert_ne!(one, two);
    assert_ne!(one, [0u8; 16]);
}

// ---- what a page is told and what it says ---------------------------------

/// The page asks for its `context` again, which the host answers whenever it is asked: the
/// context of a started view, as a page is told it.
fn context_of(pane: &Pane) -> String {
    let desk = pane.rig.desk.as_ref().expect("a desk");
    desk.take_views();
    desk.view_message(r#"{"context":{}}"#, |_| {}, |_| {});
    let told = desk.take_views();
    let Some(Action::ToView(answer)) = told.first() else {
        panic!("an answer for the page, not {told:?}");
    };
    answer.clone()
}

#[test]
fn the_page_is_told_the_area_the_application_gave_and_not_a_guess_in_another_unit() {
    let (rig, _, _) = Rig::new(true);
    let mut pane = Pane::new(rig);
    open_a_view(&mut pane);
    let answer = context_of(&pane);
    // The rig's options say 800 × 600 points. The grid's device pixels, which this crate
    // used to multiply out, are twice that on a Retina display.
    assert!(
        answer.contains(r#""size":{"width":800,"height":600}"#),
        "{answer}"
    );
}

#[test]
fn what_the_person_set_reaches_the_page_in_its_context_and_again_when_it_changes() {
    let (rig, _, _) = Rig::new(true);
    let mut pane = Pane::new(rig);
    open_a_view(&mut pane);
    let desk = pane.rig.desk.as_ref().expect("a desk");
    let first = context_of(&pane);
    assert!(first.contains(r#""reduceMotion":false"#), "{first}");
    assert!(!first.contains("locale"), "{first}");

    // Each change is a `context` for the page, in the crate's words.
    desk.apply(|_| {}, |conversation| conversation.set_reduce_motion(true));
    desk.apply(
        |_| {},
        |conversation| {
            conversation.set_locale(Some("tr-TR".to_owned()), Some("Europe/Istanbul".to_owned()));
        },
    );
    let told = desk.take_views();
    let [Action::ToView(motion), Action::ToView(locale)] = told.as_slice() else {
        panic!("a context for each change, got {told:?}");
    };
    assert!(motion.contains(r#""reduceMotion":true"#), "{motion}");
    assert!(
        locale.contains(r#""locale":"tr-TR","timeZone":"Europe/Istanbul""#),
        "{locale}"
    );
    // The same value again is nothing to tell.
    desk.apply(|_| {}, |conversation| conversation.set_reduce_motion(true));
    assert!(desk.take_views().is_empty());
}

#[test]
fn the_six_messages_pass_between_the_page_and_the_program_unread() {
    let (rig, _, _) = Rig::new(true);
    let mut pane = Pane::new(rig);
    open_a_view(&mut pane);
    let rig = Arc::clone(&pane.rig);
    let desk = rig.desk.as_ref().expect("a desk");
    desk.take_views();
    let input = Arc::clone(&pane.rig.input);
    let write = |bytes: Vec<u8>| lock(&input).extend_from_slice(&bytes);
    // The page's own words go to the lane of the module's, apart from the keys.
    let said = Arc::new(Mutex::new(Vec::new()));
    let page = |bytes: Vec<u8>| lock(&said).extend_from_slice(&bytes);

    // The page's call is the program's, as it was written.
    let call = r#"{"call":{"id":8,"method":"list","params":{"all":true}}}"#;
    desk.view_message(call, write, page);
    let sent = decoded(&mem::take(&mut *lock(&said)));
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(
        (sent[0].kind.as_str(), sent[0].payload.as_str()),
        ("msg", call)
    );
    assert!(lock(&input).is_empty(), "a page's word took the keys' lane");
    // So is a message the host has no name for: it is the program's business to ignore it.
    let odd = r#"{"unheard-of":{"x":1}}"#;
    desk.view_message(odd, write, page);
    assert_eq!(decoded(&mem::take(&mut *lock(&said))).len(), 1);
    // What is not an object is nobody's.
    for nothing in ["[1]", "\"call\"", "42", "not json", ""] {
        desk.view_message(nothing, write, page);
    }
    assert!(lock(&input).is_empty() && lock(&said).is_empty());

    // The program's answers and pushes reach the page as they were written.
    for text in [
        r#"{"response":{"id":8,"result":{"containers":[]}}}"#,
        r#"{"push":{"id":1,"value":{"state":"exited"}}}"#,
        r#"{"progress":{"id":8,"done":1,"total":2}}"#,
    ] {
        pane.feed(&wire("msg", 4, None, None, text));
        assert_eq!(desk.take_views(), [Action::ToView(text.to_owned())]);
    }
    // A program does not speak as the host: a `context` of its own never reaches the page.
    pane.feed(&wire(
        "msg",
        5,
        None,
        None,
        r#"{"context":{"mode":"light"}}"#,
    ));
    assert!(desk.take_views().is_empty());
}

#[test]
fn a_page_closes_itself_and_the_program_is_told_the_view_did() {
    let (rig, _, _) = Rig::new(true);
    let mut pane = Pane::new(rig);
    open_a_view(&mut pane);
    let desk = pane.rig.desk.as_ref().expect("a desk");
    desk.take_views();
    let input = Arc::clone(&pane.rig.input);
    let said = Arc::new(Mutex::new(Vec::new()));
    let keys = |bytes: Vec<u8>| lock(&input).extend_from_slice(&bytes);
    let page = |bytes: Vec<u8>| lock(&said).extend_from_slice(&bytes);
    desk.view_message(r#"{"close":{}}"#, keys, page);
    assert_eq!(desk.take_views(), [Action::DestroyView]);
    // The program is told by the protocol's own word, on the lane every key takes: the page's
    // lane is shut the moment the view is over.
    assert!(lock(&said).is_empty());
    let told = decoded(&mem::take(&mut *lock(&input)));
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!(
        (told[0].kind.as_str(), told[0].payload.as_str()),
        ("closed", r#"{"reason":"view"}"#)
    );
    // Closed at once: nothing the page says now is the program's.
    desk.view_message(r#"{"call":{"id":9,"method":"late"}}"#, keys, page);
    assert!(lock(&input).is_empty() && lock(&said).is_empty());
}

/// What a view says is written as the host's own words are, in the order they were said, and is
/// not left for the shell: [`Action::WriteView`] is not [`Action::Write`] to the writer, and the
/// shell has nothing to do with either.
#[test]
fn what_a_view_says_is_written_like_the_hosts_own_words_and_none_is_left_for_the_shell() {
    let (rig, _, _) = Rig::new(true);
    let mut pane = Pane::new(rig);
    open_a_view(&mut pane);
    let desk = pane.rig.desk.as_ref().expect("a desk");
    desk.take_views();
    let call = r#"{"call":{"id":8,"method":"list","params":{"all":true}}}"#;
    desk.apply(pane.rig.write(), |conversation| {
        conversation.view_message(call);
        // …then the view ends, and the host says so in its own words.
        conversation.view_message(r#"{"close":{}}"#);
    });
    let written = pane.rig.written();
    let kinds: Vec<&str> = written.iter().map(|frame| frame.kind.as_str()).collect();
    assert_eq!(kinds, ["msg", "closed"], "{written:?}");
    assert_eq!(written[0].payload, call, "the view's word, as it was said");
    let views = desk.take_views();
    assert!(
        views
            .iter()
            .all(|action| !matches!(action, Action::Write(_) | Action::WriteView(_))),
        "bytes for the program were left for the shell: {views:?}"
    );
    assert_eq!(views, [Action::DestroyView]);
}

#[test]
fn a_link_the_page_asks_for_is_put_to_the_person_and_the_page_is_never_answered() {
    let (rig, _, _) = Rig::new(true);
    let mut pane = Pane::new(rig);
    open_a_view(&mut pane);
    let desk = pane.rig.desk.as_ref().expect("a desk");
    desk.take_views();
    let input = Arc::clone(&pane.rig.input);
    let write = |bytes: Vec<u8>| lock(&input).extend_from_slice(&bytes);

    // A link that is not http or https is dropped: nobody is asked.
    desk.view_message(r#"{"link":{"url":"file:///etc/passwd"}}"#, write, write);
    assert!(desk.take_views().is_empty());

    desk.view_message(
        r#"{"link":{"url":"https://example.com/docs"}}"#,
        write,
        write,
    );
    let asked = desk.take_views();
    let [Action::OpenLink { link, url }] = asked.as_slice() else {
        panic!("one link to ask about, got {asked:?}");
    };
    assert_eq!(url, "https://example.com/docs");
    desk.apply(write, |conversation| {
        conversation.link_decision(*link, true)
    });
    // Whether the person agreed is not the page's to learn, and not the program's either.
    assert!(desk.take_views().is_empty());
    assert!(lock(&input).is_empty(), "{:?}", decoded(&lock(&input)));
    // An answer that comes again settles nothing.
    desk.apply(write, |conversation| {
        conversation.link_decision(*link, true)
    });
    assert!(lock(&input).is_empty());
}

#[test]
fn a_page_that_asks_for_an_address_waits_for_the_person_and_is_told_what_was_allowed() {
    let (rig, _, _) = Rig::new(true);
    let mut pane = Pane::new(rig);
    pane.feed(&hello());
    let token = token_of(&pane.rig.written()[0]);
    pane.feed(&bundle_open_declaring(&token, "https://api.example.com"));
    assert_eq!(pane.rig.written()[0].kind, "need");
    pane.feed(&page_file());
    let desk = pane.rig.desk.as_ref().expect("a desk");
    let asked = desk.take_views();
    let [Action::Consent(consent)] = asked.as_slice() else {
        panic!("one question for the person, got {asked:?}");
    };
    // The question is about the page's own words and nothing else.
    assert_eq!(consent.title.as_deref(), Some("Containers"));
    assert_eq!(consent.program.as_deref(), Some("containers"));
    assert_eq!(consent.domains.len(), 1);
    assert_eq!(
        desk.apply(|_| {}, |conversation| conversation.state()),
        State::Opening
    );

    desk.apply(pane.rig.write(), |conversation| {
        conversation.consent(consent.id, consent.grant_all());
    });
    let started = desk.take_views();
    let [Action::StartView(start)] = started.as_slice() else {
        panic!("the view starts with the answer, got {started:?}");
    };
    assert!(
        start
            .policy
            .contains("connect-src 'self' https://api.example.com"),
        "{}",
        start.policy
    );
    let context = start.context.as_deref().expect("a context");
    assert!(
        context.contains(r#""access":{"connect":["https://api.example.com"]}"#),
        "{context}"
    );
}

#[test]
fn the_kinds_and_versions_offered_are_what_the_application_can_open_in_the_programs_order() {
    // A pane that shows no components offers the web view alone, at the bridge's version.
    let (rig, _, _) = Rig::new(true);
    let mut pane = Pane::new(rig);
    pane.feed(&hello());
    let welcome = pane.rig.written();
    assert!(
        welcome[0]
            .payload
            .contains(r#""views":[{"kind":"web","versions":["1.0"]}]"#),
        "{}",
        welcome[0].payload
    );

    // One that does offers the three, at the versions of the interfaces it speaks, and the
    // `welcome` follows the program's order and leaves out a kind with no version in common.
    let (rig, _) = Rig::with_modules(1 << 20);
    let mut pane = Pane::new(rig);
    pane.feed(&wire(
        "hello",
        1,
        None,
        None,
        concat!(
            r#"{"versions":[1],"views":[{"kind":"web","versions":["2.0","1.0"]},"#,
            r#"{"kind":"lua","versions":["9.9"]},{"kind":"wasm","versions":["0.4","1.0"]}]}"#
        ),
    ));
    let welcome = pane.rig.written();
    assert!(
        welcome[0].payload.contains(
            r#""views":[{"kind":"web","versions":["1.0"]},{"kind":"wasm","versions":["1.0"]}]"#
        ),
        "{}",
        welcome[0].payload
    );
    // A restart offers everything the application opens, with the versions, in its own order.
    let desk = pane.rig.desk.as_ref().expect("a desk");
    desk.restart(pane.rig.write());
    let offer = pane.rig.written();
    assert!(
        offer[0].payload.contains(concat!(
            r#""views":[{"kind":"wasm","versions":["1.0"]},{"kind":"lua","versions":["1.0"]},"#,
            r#"{"kind":"web","versions":["1.0"]}]"#
        )),
        "{}",
        offer[0].payload
    );
    // The versions are the application's constants, and the Lua API's is the prelude's.
    assert_eq!((WASM_VERSION, LUA_VERSION), ("1.0", "1.0"));
    assert_eq!(WEB_BRIDGE_VERSION, "1.0");
}

// ---- the shelf ------------------------------------------------------------

/// A store whose `insert` waits for the test, as a write of 8 MiB does.
struct Slow {
    inside: std::sync::mpsc::Sender<()>,
    release: Mutex<std::sync::mpsc::Receiver<()>>,
    other: Arc<[u8]>,
    other_digest: Digest,
}

impl Shared for Slow {
    fn contains(&self, digest: &Digest) -> bool {
        *digest == self.other_digest
    }

    fn insert(&self, _file: Verified) {
        self.inside.send(()).expect("the test is there");
        lock(&self.release).recv().expect("released");
    }

    fn get(&self, digest: &Digest) -> Option<Arc<[u8]>> {
        (*digest == self.other_digest).then(|| Arc::clone(&self.other))
    }
}

#[test]
fn a_file_being_stored_does_not_make_a_pane_wait_for_another_file() {
    let (inside, entered) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let other = Verified::from_bytes(b"the web view is waiting for this".to_vec());
    let other_digest = *other.digest();
    let shelf = Shelf::shared(Slow {
        inside,
        release: Mutex::new(released),
        other: Arc::from(&b"the web view is waiting for this"[..]),
        other_digest,
    });
    let mut writer = shelf.clone();
    let writing = std::thread::spawn(move || {
        writer.insert(Verified::from_bytes(b"a big file".to_vec()));
    });
    entered
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("the write began");
    // The write is in progress. A read of another file is served all the same: one lock
    // around the store would hold it until the write was done.
    let (answered, heard) = std::sync::mpsc::channel();
    let reading = std::thread::spawn(move || {
        answered
            .send((shelf.contains(&other_digest), shelf.get(&other_digest)))
            .expect("the test is there");
    });
    let (there, bytes) = heard
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("a read does not wait for a write");
    assert!(there);
    assert_eq!(
        bytes.as_deref(),
        Some(&b"the web view is waiting for this"[..])
    );
    release.send(()).expect("the writer waits");
    writing.join().expect("the writer ends");
    reading.join().expect("the reader ends");
}

// ---- a view that is a WebAssembly component --------------------------------------------

/// What the component is, for the tests: the header of an empty one.
const COMPONENT: &[u8] = b"\0asm\x01\0\0\0";

/// The program's `open` of a view that is a component, on the alternate screen.
fn module_open(token: &str) -> Vec<u8> {
    let open = format!(
        concat!(
            r#"{{"token":"{token}","view":"wasm","version":"1.0","program":"demopen","title":"Containers","#,
            r#""bundle":{{"entry":"view.wasm","files":[{{"path":"view.wasm","sha256":"{digest}","#,
            r#""size":{size},"type":"application/wasm"}}]}}}}"#
        ),
        token = token,
        digest = Digest::of(COMPONENT).to_string(),
        size = COMPONENT.len(),
    );
    [ALTERNATE_SCREEN, &wire("open", 2, None, None, &open)].concat()
}

fn module_file() -> Vec<u8> {
    wire(
        "file",
        3,
        None,
        Some(&Digest::of(COMPONENT).to_string()),
        &::pencere_host::base64::encode(COMPONENT),
    )
}

/// A pane whose program has opened a view that is a component. Returns the pane and the token.
fn open_a_module_view() -> (Pane, String, Arc<CountingWake>) {
    let (rig, wake) = Rig::with_modules(1 << 20);
    let mut pane = Pane::new(rig);
    pane.feed(&hello());
    let welcome = pane.rig.written();
    assert_eq!(welcome.len(), 1, "{welcome:?}");
    assert!(
        welcome[0]
            .payload
            .contains(r#""features":["dismiss"],"views":[{"kind":"wasm","versions":["1.0"]},{"kind":"web","versions":["1.0"]}]"#),
        "{}",
        welcome[0].payload
    );
    let token = token_of(&welcome[0]);
    pane.feed(&module_open(&token));
    let need = pane.rig.written();
    assert_eq!(need.len(), 1, "{need:?}");
    assert_eq!(need[0].kind, "need");
    pane.feed(&module_file());
    // No MCP session: the pane is written nothing, and the view is there to start.
    assert!(pane.rig.written().is_empty());
    (pane, token, wake)
}

#[test]
fn a_component_opens_a_view_of_its_kind_and_messages_go_between_it_and_the_program() {
    let (mut pane, _, _) = open_a_module_view();
    let rig = Arc::clone(&pane.rig);
    let desk = rig.desk.as_ref().expect("a desk");
    let views = desk.take_views();
    let [Action::StartView(start)] = views.as_slice() else {
        panic!("one view to start, got {views:?}");
    };
    assert_eq!(start.kind, ViewKind::Wasm);
    assert_eq!(start.program.as_deref(), Some("demopen"));
    assert_eq!(start.title.as_deref(), Some("Containers"));

    // The program says something to the view: it reaches the shell as it is.
    let text = r#"{"push":{"id":1,"value":{"readings":[]}}}"#;
    pane.feed(&wire("msg", 4, None, None, text));
    assert_eq!(desk.take_views(), [Action::ToView(text.to_owned())]);

    // The view says something to the program: it is written through the pane's input.
    let said = r#"{"subscribe":{"id":1,"topic":"stats"}}"#;
    assert!(desk.module_message(said, pane.rig.write(), pane.rig.write()));
    let wrote = pane.rig.written();
    assert_eq!(wrote.len(), 1, "{wrote:?}");
    assert_eq!(
        (wrote[0].kind.as_str(), wrote[0].payload.as_str()),
        ("msg", said)
    );
}

/// What a script is, for the tests.
const SCRIPT: &[u8] = b"return { render = function() end }";

#[test]
fn a_script_opens_a_view_of_its_kind_and_its_failure_reaches_the_program_with_its_words() {
    let (rig, _wake) = Rig::with_modules(1 << 20);
    let mut pane = Pane::new(rig);
    pane.feed(&wire(
        "hello",
        1,
        None,
        None,
        r#"{"versions":[1],"views":[{"kind":"lua","versions":["1.0"]},{"kind":"web","versions":["1.0"]}]}"#,
    ));
    let welcome = pane.rig.written();
    assert!(
        welcome[0].payload.contains(
            r#""views":[{"kind":"lua","versions":["1.0"]},{"kind":"web","versions":["1.0"]}]"#
        ),
        "{}",
        welcome[0].payload
    );
    let open = format!(
        concat!(
            r#"{{"token":"{token}","view":"lua","version":"1.0","program":"demopen","#,
            r#""bundle":{{"entry":"view.lua","files":[{{"path":"view.lua","sha256":"{digest}","#,
            r#""size":{size},"type":"text/x-lua"}}]}}}}"#
        ),
        token = token_of(&welcome[0]),
        digest = Digest::of(SCRIPT).to_string(),
        size = SCRIPT.len(),
    );
    pane.feed(&[ALTERNATE_SCREEN, &wire("open", 2, None, None, &open)].concat());
    assert_eq!(pane.rig.written()[0].kind, "need");
    pane.feed(&wire(
        "file",
        3,
        None,
        Some(&Digest::of(SCRIPT).to_string()),
        &::pencere_host::base64::encode(SCRIPT),
    ));
    let rig = Arc::clone(&pane.rig);
    let desk = rig.desk.as_ref().expect("a desk");
    let views = desk.take_views();
    let [Action::StartView(start)] = views.as_slice() else {
        panic!("one view to start, got {views:?}");
    };
    assert_eq!(start.kind, ViewKind::Lua);

    // The script made an error: the view closes and the program is told, in the words of the
    // script, the line included.
    desk.apply(pane.rig.write(), |conversation| {
        conversation.view_failed("script:12: attempt to index nil with 'x'")
    });
    let wrote = pane.rig.written();
    assert_eq!(wrote.len(), 1, "{wrote:?}");
    assert_eq!(wrote[0].kind, "closed");
    assert_eq!(
        wrote[0].payload,
        r#"{"reason":"error","code":"view","message":"script:12: attempt to index nil with 'x'"}"#
    );
}

#[test]
fn what_the_program_says_for_the_module_is_counted_until_the_module_takes_it() {
    let (mut pane, _, _) = open_a_module_view();
    let rig = Arc::clone(&pane.rig);
    let desk = rig.desk.as_ref().expect("a desk");
    desk.take_views();
    assert_eq!(rig.flow.inbox(), 0);
    let text = r#"{"push":{"id":1,"value":{"readings":[]}}}"#;
    pane.feed(&wire("msg", 4, None, None, text));
    pane.feed(&wire("msg", 5, None, None, text));
    assert_eq!(desk.take_views().len(), 2);
    let counted = rig.flow.inbox();
    assert!(counted >= 2 * text.len(), "{counted}");
    rig.flow.taken(text.len());
    assert!(rig.flow.inbox() < counted);
    rig.flow.taken(text.len());
    assert_eq!(rig.flow.inbox(), 0);
}

/// The program writes ASCII: an accent is `\u00e9` on the wire. The sandbox counts what it takes
/// with the escapes undone, so what the pane counts of the same message is the same number, or
/// the inbox of a program that writes text with accents never drains and the reader stays shut.
#[test]
fn what_the_program_says_with_escapes_is_taken_by_the_count_the_sandbox_gives() {
    let (mut pane, _, _) = open_a_module_view();
    let rig = Arc::clone(&pane.rig);
    let desk = rig.desk.as_ref().expect("a desk");
    desk.take_views();
    let text = r#"{"push":{"id":1,"value":{"name":"caf\u00e9 \u2603 \ud83d\ude00"}}}"#;
    pane.feed(&wire("msg", 4, None, None, text));
    pane.feed(&wire("msg", 5, None, None, text));
    let heard = desk.take_views();
    assert_eq!(heard.len(), 2);
    assert!(rig.flow.inbox() > 0);
    for action in heard {
        let Action::ToView(message) = action else {
            panic!("a message for the module, not {action:?}");
        };
        // What the sandbox hands `Program::taken`, from the text it was given.
        rig.flow
            .taken(::pencere_host::json::unescaped_len(&message));
    }
    assert_eq!(rig.flow.inbox(), 0);
}

#[test]
fn the_end_of_the_view_forgets_what_the_module_never_took() {
    let (mut pane, token, _) = open_a_module_view();
    let rig = Arc::clone(&pane.rig);
    let desk = rig.desk.as_ref().expect("a desk");
    desk.take_views();
    let text = r#"{"push":{"id":1,"value":{"readings":[]}}}"#;
    for id in 4..8 {
        pane.feed(&wire("msg", id, None, None, text));
    }
    assert!(rig.flow.inbox() > 0);
    // The module has said something the program has not read: that is forgotten too.
    assert!(rig.flow.may_say(100));
    let bye = format!(r#"{{"token":"{token}","reason":"signal"}}"#);
    pane.feed(&wire("bye", 9, None, None, &bye));
    assert_eq!(rig.flow.inbox(), 0);
    assert!(rig.flow.pending() < 100, "{}", rig.flow.pending());
}

#[test]
fn a_module_told_full_is_news_for_the_shell_once_like_any_other() {
    let (pane, _, wake) = open_a_module_view();
    let rig = Arc::clone(&pane.rig);
    let desk = rig.desk.as_ref().expect("a desk");
    desk.take_views();
    let before = wake_count(&wake);
    assert!(!desk.take_room());
    desk.room_returned();
    desk.room_returned();
    assert_eq!(wake_count(&wake), before + 1, "once, however often");
    assert!(desk.take_room());
    assert!(!desk.take_room(), "taken");
    // News for the shell again once it has taken the last.
    desk.room_returned();
    assert_eq!(wake_count(&wake), before + 2);
}

#[test]
fn a_message_the_module_sends_after_its_view_ended_writes_nothing_to_the_pane() {
    let said = r#"{"call":{"id":9,"method":"stop"}}"#;
    type End = fn(&Pane, &str, &Desk);
    let endings: [(&str, End); 5] = [
        ("the program's bye", |pane, token, _| {
            let bye = format!(r#"{{"token":"{token}","reason":"signal"}}"#);
            let desk = pane.rig.desk.as_ref().expect("a desk");
            let mut events = Vec::new();
            let mut stream = StreamDecoder::new(Direction::ToHost);
            stream.feed(&wire("bye", 7, None, None, &bye), &mut events);
            for event in events {
                if let FrameEvent::Message(message) = event {
                    desk.park(message, true);
                }
            }
            desk.settle(pane.rig.write());
        }),
        ("the program's dismiss", |pane, token, _| {
            let dismiss = format!(r#"{{"token":"{token}"}}"#);
            let desk = pane.rig.desk.as_ref().expect("a desk");
            let mut events = Vec::new();
            let mut stream = StreamDecoder::new(Direction::ToHost);
            stream.feed(&wire("dismiss", 7, None, None, &dismiss), &mut events);
            for event in events {
                if let FrameEvent::Message(message) = event {
                    desk.park(message, true);
                }
            }
            desk.settle(pane.rig.write());
        }),
        ("the person closing the view", |pane, _, desk| {
            desk.apply(pane.rig.write(), |conversation| conversation.close_view());
        }),
        ("the end of the program", |pane, _, desk| {
            desk.program_ended(pane.rig.write());
        }),
        ("the view failing", |pane, _, desk| {
            desk.apply(pane.rig.write(), |conversation| {
                conversation.view_failed("the page trapped")
            });
        }),
    ];
    for (name, end) in endings {
        let (pane, token, _) = open_a_module_view();
        let rig = Arc::clone(&pane.rig);
        let desk = rig.desk.as_ref().expect("a desk");
        desk.take_views();
        assert!(
            desk.module_message(said, pane.rig.write(), pane.rig.write()),
            "{name}: while it is open"
        );
        pane.rig.written();

        end(&pane, &token, desk);
        assert!(
            desk.take_views().contains(&Action::DestroyView),
            "{name}: the view is destroyed"
        );
        // Whatever the end wrote (a `closed`), the pane's input is emptied: what matters is
        // what the late message adds.
        lock(&pane.rig.input).clear();

        assert!(
            !desk.module_message(said, pane.rig.write(), pane.rig.write()),
            "{name}"
        );
        assert!(
            lock(&pane.rig.input).is_empty(),
            "{name}: a late message reached the pane's input: {:?}",
            pane.rig.written()
        );
    }
}

/// A message of a view over the size one may have ends the view as a failure and the program is
/// told by the protocol's own word, which takes the lane every key takes: the view's lane is
/// shut by the end of the view, and a `closed` queued on it would be dropped with the view's own
/// words — the program would never be told. Whatever the view is.
#[test]
fn a_message_over_the_limit_ends_the_view_and_the_program_is_told_on_the_keys_lane() {
    let huge = format!(
        r#"{{"call":{{"id":1,"method":"m","params":"{}"}}}}"#,
        "x".repeat(MAX_VIEW_MESSAGE)
    );
    let told = |keys: &[u8], lane: &[u8]| {
        let closed = decoded(keys);
        assert_eq!(closed.len(), 1, "{closed:?}");
        assert_eq!(closed[0].kind, "closed");
        assert!(
            closed[0].payload.contains(r#""reason":"error""#),
            "{}",
            closed[0].payload
        );
        assert!(
            lane.is_empty(),
            "the view's own lane carried a word of the protocol"
        );
    };

    // A web page.
    let (rig, _, _) = Rig::new(true);
    let mut pane = Pane::new(rig);
    open_a_view(&mut pane);
    let rig = Arc::clone(&pane.rig);
    let desk = rig.desk.as_ref().expect("a desk");
    desk.take_views();
    let (keys, lane) = (Mutex::new(Vec::new()), Mutex::new(Vec::new()));
    desk.view_message(
        &huge,
        |bytes| lock(&keys).extend_from_slice(&bytes),
        |bytes| lock(&lane).extend_from_slice(&bytes),
    );
    assert!(desk.take_views().contains(&Action::DestroyView));
    told(&lock(&keys), &lock(&lane));

    // A component.
    let (pane, _, _) = open_a_module_view();
    let rig = Arc::clone(&pane.rig);
    let desk = rig.desk.as_ref().expect("a desk");
    desk.take_views();
    let (keys, lane) = (Mutex::new(Vec::new()), Mutex::new(Vec::new()));
    let wrote = desk.module_message(
        &huge,
        |bytes| lock(&keys).extend_from_slice(&bytes),
        |bytes| lock(&lane).extend_from_slice(&bytes),
    );
    assert!(!wrote, "nothing of the module's was written");
    assert!(desk.take_views().contains(&Action::DestroyView));
    told(&lock(&keys), &lock(&lane));
}

/// Specification §4.8: more than 16 MiB of a view's words waiting for the tty closes the view
/// as a failure. At the bound is not over it; the words that have not been begun are dropped
/// (the gate is the end of a view's), the frame under way is finished, and the program is told
/// on the keys lane, behind none of them.
#[test]
fn a_view_with_more_waiting_for_the_tty_than_the_bound_is_closed_as_a_failure() {
    let told = |wrote: &[Message]| {
        assert_eq!(wrote.len(), 1, "{wrote:?}");
        assert_eq!(wrote[0].kind, "closed");
        assert!(
            wrote[0].payload.contains(r#""reason":"error""#),
            "{}",
            wrote[0].payload
        );
        assert!(
            wrote[0].payload.contains(r#""code":"view""#),
            "{}",
            wrote[0].payload
        );
    };
    for web in [true, false] {
        let pane = if web {
            let (rig, _, _) = Rig::new(true);
            let mut pane = Pane::new(rig);
            open_a_view(&mut pane);
            pane
        } else {
            open_a_module_view().0
        };
        let rig = Arc::clone(&pane.rig);
        let desk = rig.desk.as_ref().expect("a desk");
        desk.take_views();
        let epoch = desk
            .flow
            .module_ticket()
            .expect("the view's words are taken");

        // Nothing waiting, and exactly the bound: the view stays, and the words may be held.
        assert!(desk.backlog(0, pane.rig.write()), "web: {web}");
        assert!(desk.backlog(MAX_BACKLOG, pane.rig.write()), "web: {web}");
        assert!(pane.rig.written().is_empty(), "web: {web}");
        assert!(desk.take_views().is_empty(), "web: {web}");
        assert_eq!(desk.flow.module_ticket(), Some(epoch));

        // A byte over it: the words are not held, the view is closed, the program told, and
        // the words not begun dropped.
        assert!(
            !desk.backlog(MAX_BACKLOG + 1, pane.rig.write()),
            "web: {web}"
        );
        told(&pane.rig.written());
        assert!(
            desk.take_views().contains(&Action::DestroyView),
            "web: {web}"
        );
        assert_eq!(
            desk.flow.module_ticket(),
            None,
            "no word is queued for an ended view"
        );
        assert!(
            !desk.flow.module_may_write(epoch, false),
            "a frame that was not begun is dropped"
        );
        assert!(
            desk.flow.module_may_write(epoch, true),
            "the frame under way is finished, so that the program is not left half a frame"
        );

        // A view that is not there is not closed again, whatever still waits, and its words
        // are of no consequence: they may be held (and are dropped by the gate).
        assert!(
            desk.backlog(MAX_BACKLOG * 2, pane.rig.write()),
            "web: {web}"
        );
        assert!(pane.rig.written().is_empty(), "web: {web}");
        assert!(desk.take_views().is_empty(), "web: {web}");
    }
}

/// Specification §5.4: the words a view fails with reach the program in `closed`'s `message`
/// as they were said, a component of another interface's two versions included.
#[test]
fn the_words_of_a_component_of_another_interface_name_both_versions_to_the_program() {
    let (pane, _, _) = open_a_module_view();
    let rig = Arc::clone(&pane.rig);
    let desk = rig.desk.as_ref().expect("a desk");
    let why = "the page could not be loaded: this view is written against `pencere:view@0.5.0`; \
               this terminal opens `pencere:view@1.0.0`";
    desk.apply(pane.rig.write(), |conversation| {
        conversation.view_failed(why)
    });
    let wrote = pane.rig.written();
    assert_eq!(wrote.len(), 1, "{wrote:?}");
    assert_eq!(wrote[0].kind, "closed");
    for version in ["`pencere:view@0.5.0`", "`pencere:view@1.0.0`"] {
        assert!(wrote[0].payload.contains(version), "{}", wrote[0].payload);
    }
}

#[test]
fn a_pane_that_shows_no_components_refuses_one_and_runs_no_view() {
    let (rig, _, _) = Rig::new(true);
    let mut pane = Pane::new(rig);
    pane.feed(&hello());
    let welcome = pane.rig.written();
    assert!(
        !welcome[0].payload.contains("wasm"),
        "it does not say it shows them: {}",
        welcome[0].payload
    );
    pane.feed(&module_open(&token_of(&welcome[0])));
    let closed = pane.rig.written();
    assert_eq!(closed.len(), 1, "{closed:?}");
    assert_eq!(closed[0].kind, "closed");
    assert!(
        closed[0].payload.contains(r#""code":"unsupported""#),
        "{}",
        closed[0].payload
    );
    let desk = pane.rig.desk.as_ref().expect("a desk");
    assert!(desk.take_views().is_empty());
}

#[test]
fn a_component_over_the_limit_is_refused_before_a_byte_of_it_is_asked_for() {
    let (rig, _) = Rig::with_modules(4);
    let mut pane = Pane::new(rig);
    pane.feed(&hello());
    let token = token_of(&pane.rig.written()[0]);
    pane.feed(&module_open(&token));
    let closed = pane.rig.written();
    assert_eq!(closed.len(), 1, "{closed:?}");
    assert!(
        closed[0].payload.contains(r#""code":"limit""#),
        "{}",
        closed[0].payload
    );
}

// ─── the pane's tty, read for a pane that has no master ────────────────

/// A stream's owner that says what the terminal its program is on is.
struct TtyOps(Mutex<Option<crate::TtyReading>>);

impl PtyOps for TtyOps {
    fn resize(&self, _: BorrowedFd<'_>, _: crate::PtySize) {}
    fn hangup(&self, _: u32, _: BorrowedFd<'_>) {}
    fn tty(&self, _: u32) -> Option<crate::TtyReading> {
        *lock(&self.0)
    }
}

/// The two checks of the hardening rules are made for a pane that runs in a terminal of this
/// Mac as they are for one that holds a PTY: the shell is in front when its group is the
/// terminal's foreground, and the modes are the program's. A terminal that cannot be read
/// leaves both unknown, never "no" (a `hello` is not answered into a line editor on a guess).
#[test]
fn the_tty_of_a_streamed_pane_is_read_through_its_owner() {
    let shell = Arc::new(Mutex::new(ShellLog::new(100)));
    let ops = Arc::new(TtyOps(Mutex::new(None)));
    // The child is the shell, a leader of its own group: 4242.
    let observer = pane_observer(
        shell,
        Arc::new(Mutex::new(None)),
        Some(Arc::clone(&ops) as Arc<dyn PtyOps>),
        4242,
        None,
    );
    let unread = observer();
    assert_eq!(unread.shell_in_foreground, None);
    assert_eq!(unread.tty_canonical_or_echo, None);

    let reading = |foreground, canonical, echo| crate::TtyReading {
        modes: crate::TtyModes { canonical, echo },
        foreground,
    };
    *lock(&ops.0) = Some(reading(Some(4242), false, false));
    let at_prompt = observer();
    assert_eq!(at_prompt.shell_in_foreground, Some(true));
    assert_eq!(at_prompt.tty_canonical_or_echo, Some(false));

    *lock(&ops.0) = Some(reading(Some(9000), true, false));
    let program = observer();
    assert_eq!(program.shell_in_foreground, Some(false));
    assert_eq!(program.tty_canonical_or_echo, Some(true));
}

// ─── what the program left unread, thrown away ─────────────────────────

/// A stream's owner that records where it was asked to drop the program's input.
#[derive(Default)]
struct DropOps(Mutex<Vec<bool>>);

impl PtyOps for DropOps {
    fn resize(&self, _: BorrowedFd<'_>, _: crate::PtySize) {}
    fn hangup(&self, _: u32, _: BorrowedFd<'_>) {}
    fn drop_input(&self, remote: bool) {
        lock(&self.0).push(remote);
    }
}

/// A pane with no master holds no queue to flush: its owner is asked, and told whether the
/// program ran here or on the server the pane is in a session with. Nothing is asked of a
/// pane that has a master, and nothing is flushed for the stream.
#[test]
fn a_pane_with_no_master_asks_the_owner_of_its_terminal_to_drop_the_input() {
    let shell = Arc::new(Mutex::new(ShellLog::new(100)));
    let ops = Arc::new(DropOps::default());
    let discard = pane_discard(
        Arc::clone(&shell),
        Arc::new(Mutex::new(None)),
        Some(Arc::clone(&ops) as Arc<dyn PtyOps>),
        None,
    );
    (discard.throw)(false);
    assert_eq!(
        *lock(&ops.0),
        [false],
        "the program ran in the terminal of the stream"
    );
    (discard.throw)(true);
    assert_eq!(*lock(&ops.0), [false, true], "and now on the server");

    // Where the program is, as the pane knows it: the session the shell is in.
    assert!(!(discard.elsewhere)());
    lock(&shell).set_remote(Some(&crate::RemoteTarget::ssh("rasp")));
    assert!((discard.elsewhere)());
    lock(&shell).set_remote(None);
    assert!(!(discard.elsewhere)());

    // A pane that holds the master flushes it and asks nobody.
    let held = Arc::new(Mutex::new(None));
    (pane_discard(shell, held, None, None).throw)(false);
    assert_eq!(lock(&ops.0).len(), 2);
}

/// A pane that holds a master of its own and shows a server's terminal: the program that ran on
/// the server is not in this tty, so the keeper of the server's terminal is asked to drop what it
/// has not read; a program that ran here is flushed from the master, and nobody is asked.
#[test]
fn a_pane_with_a_master_that_shows_a_servers_terminal_asks_the_keeper_to_drop_the_input() {
    let asked = Arc::new(AtomicUsize::new(0));
    let drop_shown = DropShown::new({
        let asked = Arc::clone(&asked);
        move || {
            asked.fetch_add(1, Ordering::SeqCst);
        }
    });
    let shell = Arc::new(Mutex::new(ShellLog::new(100)));
    let held = Arc::new(Mutex::new(None));
    let discard = pane_discard(
        Arc::clone(&shell),
        Arc::clone(&held),
        None,
        Some(drop_shown),
    );
    (discard.throw)(true);
    assert_eq!(
        asked.load(Ordering::SeqCst),
        1,
        "the program ran on the server"
    );
    (discard.throw)(false);
    assert_eq!(
        asked.load(Ordering::SeqCst),
        1,
        "this tty is where the program ran"
    );
    // No keeper: there is nobody to ask, and the tty of the pipe is left alone.
    (pane_discard(shell, held, None, None).throw)(true);
}

/// Where the program ran is read when its view opens and kept: by the time the discard comes the
/// shell has said the command ended, and `D` has cleared the remote session with it, so a
/// program that ran on a server would be taken for one that ran here and its leftovers thrown
/// away from the wrong terminal.
#[test]
fn what_a_program_on_a_server_left_unread_is_thrown_away_where_it_ran_after_the_session_is_forgotten()
 {
    for (opened_on_a_server, thrown_where) in [(true, true), (false, false)] {
        let elsewhere = Arc::new(AtomicBool::new(opened_on_a_server));
        let thrown = Arc::new(Mutex::new(Vec::new()));
        let discard = Discard::new(
            {
                let elsewhere = Arc::clone(&elsewhere);
                move || elsewhere.load(Ordering::SeqCst)
            },
            {
                let thrown = Arc::clone(&thrown);
                move |remote| lock(&thrown).push(remote)
            },
        );
        let (rig, _) = Rig::with_discard(discard);
        let mut pane = Pane::new(Arc::clone(&rig));
        open_a_view(&mut pane);
        // The module's bytes reached the tty, then the local command ended (`D`): the pane
        // has forgotten the session when the view hears the program is gone.
        rig.flow.wrote_module();
        elsewhere.store(!opened_on_a_server, Ordering::SeqCst);
        rig.desk
            .as_ref()
            .expect("a desk")
            .program_ended(rig.write());
        assert_eq!(*lock(&thrown), [thrown_where]);
    }
}

/// The pane that holds the master still flushes the program's input from it, and leaves the
/// tty alone when the program ran on another machine: the tty is `ssh`'s then.
#[cfg(target_os = "macos")]
#[test]
fn a_pane_with_a_master_flushes_what_the_program_left_unread_unless_it_is_elsewhere() {
    use std::os::fd::{FromRawFd as _, OwnedFd};

    let (mut master, mut slave) = (0, 0);
    // SAFETY: `openpty` fills the two descriptors; null name, termios and winsize are allowed.
    let opened = unsafe {
        libc::openpty(
            &raw mut master,
            &raw mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(opened, 0, "a pty");
    // SAFETY: both descriptors are open and ours alone.
    let (master, slave) = unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
    // No line discipline: what the master writes is at once what the program can read.
    // SAFETY: `termios` is plain data; the calls take the descriptors they were given.
    unsafe {
        let mut modes: libc::termios = std::mem::zeroed();
        assert_eq!(libc::tcgetattr(slave.as_raw_fd(), &raw mut modes), 0);
        libc::cfmakeraw(&raw mut modes);
        assert_eq!(
            libc::tcsetattr(slave.as_raw_fd(), libc::TCSANOW, &raw const modes),
            0
        );
    }
    let unread = || {
        let mut count: libc::c_int = 0;
        // SAFETY: `FIONREAD` writes one `int`.
        let status = unsafe { libc::ioctl(slave.as_raw_fd(), libc::FIONREAD, &raw mut count) };
        assert_eq!(status, 0);
        count
    };
    let write = |bytes: &[u8]| {
        // SAFETY: the buffer is valid for its length.
        let n = unsafe { libc::write(master.as_raw_fd(), bytes.as_ptr().cast(), bytes.len()) };
        assert_eq!(usize::try_from(n), Ok(bytes.len()));
    };
    let shell = Arc::new(Mutex::new(ShellLog::new(100)));
    let held = Arc::new(Mutex::new(Some(File::from(
        master.try_clone().expect("a second descriptor"),
    ))));
    let discard = pane_discard(Arc::clone(&shell), held, None, None);

    write(b"what the page said");
    assert_eq!(unread(), 18);
    (discard.throw)(false);
    assert_eq!(unread(), 0, "the program's input is gone");

    write(b"typed at the ssh");
    (discard.throw)(true);
    assert_eq!(
        unread(),
        16,
        "the program is on another machine: not this tty's to flush"
    );
}
