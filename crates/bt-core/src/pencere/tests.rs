//! The desk's rules, with the real parser and the crate's own wire: a program
//! is played by writing frames into a pane, and what the desk answers is what
//! reaches the pane's input.
//!
//! The helpers here (`pub(crate)`) are the reader's tests' as well: the order of
//! a `welcome` and a `CSI c` reply is only the reader loop's to keep.

use std::sync::atomic::{AtomicUsize, Ordering};

use ::pencere::frame::{Direction, Event as FrameEvent, Outgoing, Split, StreamDecoder, encode};
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
    fn bell(&self) {}
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
        );
        (rig, seen, wake)
    }

    /// A rig with a desk whose observer is `observer`: for a test that looks at
    /// what is held when the desk reads the pane.
    pub(crate) fn with_observer(observer: Observer) -> (Arc<Self>, Arc<CountingWake>) {
        Self::build(Some(observer), None)
    }

    /// A rig whose desk shows WebAssembly views of up to `limit` bytes.
    pub(crate) fn with_modules(limit: u64) -> (Arc<Self>, Arc<CountingWake>) {
        Self::build(Some(Box::new(Observed::default)), Some(limit))
    }

    fn build(
        observer: Option<Observer>,
        wasm_entry_limit: Option<u64>,
    ) -> (Arc<Self>, Arc<CountingWake>) {
        let wake = Arc::new(CountingWake::default());
        let flow = crate::flow::Flow::new();
        let desk = observer.map(|observer| {
            let mut options = ViewOptions::new("bateri-test", "0.0.0");
            options.wasm_entry_limit = wasm_entry_limit;
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
                Box::new(|| {}),
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
        r#"{"versions":[1],"views":["wasm","web"]}"#,
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

/// The id of a JSON-RPC request or response in `text`.
fn rpc_id(text: &str) -> u32 {
    let rest = after(text, "id").expect("an id");
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().expect("a number")
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
    open_of_bundle(token, "")
}

/// The same, for a bundle whose manifest declares `domain` as an address the view connects to:
/// the person is asked before the view starts.
pub(crate) fn bundle_open_declaring(token: &str, domain: &str) -> Vec<u8> {
    open_of_bundle(
        token,
        &format!(r#","meta":{{"csp":{{"connectDomains":["{domain}"]}}}}"#),
    )
}

/// The `open` of the one-file bundle with `meta` (the manifest's own member, or nothing).
fn open_of_bundle(token: &str, meta: &str) -> Vec<u8> {
    let digest = Digest::of(PAGE).to_string();
    let open = format!(
        concat!(
            r#"{{"token":"{token}","view":"web","tool":{{"name":"list_containers","arguments":{{}}}},"#,
            r#""bundle":{{"entry":"index.html","files":[{{"path":"index.html","sha256":"{digest}","#,
            r#""size":{size},"type":"text/html"}}]{meta}}}}}"#
        ),
        token = token,
        digest = digest,
        size = PAGE.len(),
        meta = meta
    );
    [ALTERNATE_SCREEN, &wire("open", 2, None, None, &open)].concat()
}

/// The program's `file` of the page.
pub(crate) fn page_file() -> Vec<u8> {
    wire(
        "file",
        3,
        None,
        Some(&Digest::of(PAGE).to_string()),
        &::pencere::base64::encode(PAGE),
    )
}

/// The program's answer to the host's MCP `initialize`.
pub(crate) fn initialize_answer(request: &Message) -> Vec<u8> {
    let id = rpc_id(&request.payload);
    let answer = format!(
        concat!(
            r#"{{"jsonrpc":"2.0","id":{id},"result":{{"protocolVersion":"2025-11-25","#,
            r#""capabilities":{{"tools":{{}}}},"serverInfo":{{"name":"containers","version":"0"}}}}}}"#
        ),
        id = id
    );
    wire("msg", 4, None, None, &answer)
}

/// The program's answer to the host's MCP `tools/list`: one tool with a view.
pub(crate) fn tools_answer(request: &Message) -> Vec<u8> {
    let id = rpc_id(&request.payload);
    let tools = format!(
        concat!(
            r#"{{"jsonrpc":"2.0","id":{id},"result":{{"tools":[{{"name":"list_containers","#,
            r#""inputSchema":{{"type":"object"}},"_meta":{{"ui":{{"resourceUri":"ui://containers/view"}}}}}}]}}}}"#
        ),
        id = id
    );
    wire("msg", 5, None, None, &tools)
}

/// Whether `message` is the host's MCP request for `method`.
pub(crate) fn is_request(message: &Message, method: &str) -> bool {
    message.kind == "msg"
        && message
            .payload
            .contains(&format!("\"method\":\"{method}\""))
}

/// The path to a started view: the program says `hello`, opens a view of a
/// one-file bundle, sends the file, and answers the MCP start-up. Returns the
/// token the view was opened with.
fn open_a_view(pane: &mut Pane) -> String {
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

    pane.feed(&page_file());
    let initialize = pane.rig.written();
    assert_eq!(initialize.len(), 1, "{initialize:?}");
    assert!(is_request(&initialize[0], "initialize"), "{initialize:?}");
    pane.feed(&initialize_answer(&initialize[0]));
    let next = pane.rig.written();
    assert_eq!(next.len(), 2, "{next:?}");
    assert!(next[0].payload.contains("notifications/initialized"));
    assert!(is_request(&next[1], "tools/list"), "{next:?}");
    pane.feed(&tools_answer(&next[1]));
    token
}

fn wake_count(wake: &CountingWake) -> usize {
    wake.changes.load(Ordering::Acquire)
}

/// An `open` of a tool with no bundle, with `token`.
fn bare_open(token: &str) -> Vec<u8> {
    wire(
        "open",
        2,
        None,
        None,
        &format!(r#"{{"token":"{token}","view":"web","tool":{{"name":"t","arguments":{{}}}}}}"#),
    )
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
    assert!(
        written[0].payload.contains("\"bundle\""),
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
    assert_eq!(start.source, ViewSource::Bundle);
    assert_eq!(start.tool, "list_containers");
    assert!(
        start.policy.contains("default-src 'none'"),
        "{}",
        start.policy
    );
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
    // Two things for the view in a row, nobody has taken the first yet. (The
    // view has not initialized, so it is told nothing: what is asked of the
    // shell is the view's start, which is already there.)
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
    pane.feed(&[ALTERNATE_SCREEN, &bare_open(&token)].concat());
    let written = pane.rig.written();
    assert_eq!(written.len(), 1, "{written:?}");
    assert!(
        written[0].payload.contains("\"method\":\"initialize\""),
        "an open on the alternate screen is accepted: {written:?}"
    );

    // Not on it: refused with `screen`.
    let (rig, _, _) = Rig::new(true);
    let mut pane = Pane::new(rig);
    pane.feed(&hello());
    let token = token_of(&pane.rig.written()[0]);
    pane.feed(&bare_open(&token));
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
            &bare_open(&token),
            b"\x1b[?1049l",
        ]
        .concat(),
    );
    let written = pane.rig.written();
    assert!(
        written
            .iter()
            .any(|m| m.payload.contains("\"method\":\"initialize\"")),
        "{written:?}"
    );
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
    // As after a handover: the pane is a program's again, no shell around it.
    *lock(&seen) = Observed::default();

    desk.restart(pane.rig.write());
    // The view is torn down first; the program is offered the restart once that
    // is done (the view has not initialized, so it is torn down at once).
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

/// A stream that has no desk — the journal's replay and the handover's VT —
/// builds the screen the live one builds and writes nothing.
#[test]
fn a_replay_with_no_desk_builds_the_same_screen_and_writes_nothing() {
    let stream = [
        b"one\r\n".as_slice(),
        &hello(),
        b"two\r\n",
        &wire(
            "msg",
            2,
            None,
            None,
            r#"{"jsonrpc":"2.0","method":"ping","id":9}"#,
        ),
        b"three",
    ]
    .concat();

    let (live, _, _) = Rig::new(true);
    let mut live = Pane::new(live);
    live.feed(&stream);
    assert!(!lock(&live.rig.input).is_empty(), "the live pane answers");

    // The journal's replay: commands, no sink.
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

    // The handover's VT: no commands either.
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
    pane.feed(&[ALTERNATE_SCREEN, &bare_open(&token)].concat());
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
        assert_eq!(view.success, Some(rgb(theme.success)), "{name}");
        assert_eq!(view.error, Some(rgb(theme.error)), "{name}");
        assert_eq!(view.warning, Some(rgb(theme.warning)), "{name}");
        assert_eq!(view.info, Some(rgb(theme.info)), "{name}");
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

// ---- the area the view is told ---------------------------------------------

/// The view's `ui/initialize`, which comes first.
const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"ui/initialize","params":{"protocolVersion":"2025-11-21","appCapabilities":{},"appInfo":{"name":"t","version":"0"}}}"#;

/// What the view is told its container is: the host context of its `ui/initialize`
/// answer, from a started view.
fn container_of(pane: &mut Pane) -> String {
    let desk = pane.rig.desk.as_ref().expect("a desk");
    desk.take_views();
    desk.apply(|_| {}, |conversation| conversation.view_message(INITIALIZE));
    let told = desk.take_views();
    let Some(Action::ToView(answer)) = told.first() else {
        panic!("an answer for the view, not {told:?}");
    };
    answer.clone()
}

#[test]
fn the_view_is_told_the_area_the_application_gave_and_not_a_guess_in_another_unit() {
    let (rig, _, _) = Rig::new(true);
    let mut pane = Pane::new(rig);
    open_a_view(&mut pane);
    let answer = container_of(&mut pane);
    // The rig's options say 800 × 600 points. The grid's device pixels, which this crate
    // used to multiply out, are twice that on a Retina display.
    assert!(answer.contains("\"width\":800"), "{answer}");
    assert!(answer.contains("\"height\":600"), "{answer}");
}

// ---- the requests a view has out ------------------------------------------

#[test]
fn a_message_of_the_view_that_goes_to_the_program_says_so() {
    let (rig, _, _) = Rig::new(true);
    let mut pane = Pane::new(rig);
    open_a_view(&mut pane);
    let desk = pane.rig.desk.as_ref().expect("a desk");
    desk.take_views();
    let input = Arc::clone(&pane.rig.input);
    let write = |bytes: Vec<u8>| lock(&input).extend_from_slice(&bytes);
    // The view's start-up and a `ping` are the host's own to answer.
    assert!(!desk.view_message(INITIALIZE, write));
    assert!(!desk.view_message(r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#, write));
    // A request for a tool is the program's: it is out until the program answers.
    assert!(desk.view_message(
        r#"{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"list_containers","arguments":{}}}"#,
        write,
    ));
    let sent = decoded(&mem::take(&mut *lock(&input)));
    assert!(
        sent.iter().any(|message| is_request(message, "tools/call")),
        "{sent:?}"
    );
    // Given up on, the view hears that it failed.
    desk.take_views();
    desk.apply(|_| {}, Conversing::requests_timed_out);
    let told = desk.take_views();
    assert!(
        told.iter().any(|action| matches!(
            action,
            Action::ToView(text) if text.contains("did not answer in time")
        )),
        "{told:?}"
    );
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
            r#"{{"token":"{token}","view":"wasm","program":"demopen","title":"Containers","#,
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
        &::pencere::base64::encode(COMPONENT),
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
            .contains(r#""features":["bundle","dismiss"],"views":["wasm","web"]"#),
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
    assert!(desk.module_message(said, pane.rig.write()));
    let wrote = pane.rig.written();
    assert_eq!(wrote.len(), 1, "{wrote:?}");
    assert_eq!(
        (wrote[0].kind.as_str(), wrote[0].payload.as_str()),
        ("msg", said)
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
            desk.apply(pane.rig.write(), |conversation| conversation.view_failed());
        }),
    ];
    for (name, end) in endings {
        let (pane, token, _) = open_a_module_view();
        let rig = Arc::clone(&pane.rig);
        let desk = rig.desk.as_ref().expect("a desk");
        desk.take_views();
        assert!(
            desk.module_message(said, pane.rig.write()),
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

        assert!(!desk.module_message(said, pane.rig.write()), "{name}");
        assert!(
            lock(&pane.rig.input).is_empty(),
            "{name}: a late message reached the pane's input: {:?}",
            pane.rig.written()
        );
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
