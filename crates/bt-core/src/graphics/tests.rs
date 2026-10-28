use std::ffi::CString;
use std::os::fd::FromRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use alacritty_terminal::vte::ansi::{Handler, Processor};

use super::apc::Apc;
use super::command::Command;
use super::store::Images;
use super::*;

/// What the parser knows of an 80-column terminal with 10 × 20 pixel cells.
const ENV: Env = Env {
    cell: (10, 20),
    columns: 80,
    lines: 24,
};

const ESC_G: &[u8] = b"\x1b_G";
const ST: &[u8] = b"\x1b\\";

/// How a command that shows an image moves the cursor: `rows` down and `cols`
/// right, at the cell size and width of [`ENV`].
fn moves(rows: u32, cols: u32) -> Motion {
    Motion {
        rows,
        cols,
        wrap: false,
    }
}

/// The movement of an image as wide as the screen: down `rows`, then to the
/// start of the next line.
fn wrapped(rows: u32) -> Motion {
    Motion {
        rows,
        cols: 0,
        wrap: true,
    }
}

/// The movement of a command that leaves the cursor.
const STAYS: Motion = Motion::NONE;

pub(crate) fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::new();
    for group in bytes.chunks(3) {
        let word = group.iter().enumerate().fold(0u32, |word, (at, &byte)| {
            word | u32::from(byte) << (16 - 8 * at)
        });
        for at in 0..4 {
            if at <= group.len() {
                text.push(TABLE[(word >> (18 - 6 * at) & 63) as usize] as char);
            } else {
                text.push('=');
            }
        }
    }
    text
}

/// One command as a program writes it.
pub(crate) fn wire(control: &str, payload: &[u8]) -> Vec<u8> {
    [
        ESC_G,
        control.as_bytes(),
        b";",
        base64(payload).as_bytes(),
        ST,
    ]
    .concat()
}

pub(crate) fn rgba(width: u32, height: u32, seed: u8) -> Vec<u8> {
    (0..width * height * 4)
        .map(|at| seed.wrapping_add(at as u8))
        .collect()
}

fn zlib(bytes: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(bytes, 6)
}

fn png_file(
    width: u32,
    height: u32,
    color: png::ColorType,
    depth: png::BitDepth,
    palette: Option<&[u8]>,
    trns: Option<&[u8]>,
    data: &[u8],
) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(color);
    encoder.set_depth(depth);
    if let Some(palette) = palette {
        encoder.set_palette(palette.to_vec());
    }
    if let Some(trns) = trns {
        encoder.set_trns(trns.to_vec());
    }
    let mut writer = encoder.write_header().expect("header");
    writer.write_image_data(data).expect("data");
    writer.finish().expect("finish");
    out
}

/// The parser's side of the protocol without a terminal behind it: the real
/// parser hands the commands over in the order of the stream, they are
/// received and applied the way `ClusterHandler` does it, to a screen that is
/// only a [`Screen`] — fixed, so that what a command does to a store is
/// told apart from where the cursor goes (`screenful` has the grid for that).
struct Wire<'a> {
    commands: &'a mut Commands,
    hub: &'a Hub,
    screen: Screen,
    now: Instant,
    /// How each record moves the cursor, in the order of the stream.
    motions: &'a mut Vec<Motion>,
    /// What the applied records answered.
    answers: &'a mut Vec<u8>,
    /// What the queries answered.
    queries: &'a mut Vec<Vec<u8>>,
}

impl Wire<'_> {
    /// What the halted parser waits for, as the read loop does it: the
    /// decoding, then the record applied where the command ended.
    fn resolve(&mut self) {
        self.hub.decode_pending();
        if let Some(pending) = self.hub.take_parked() {
            self.answers
                .extend(self.hub.apply(pending, &self.screen).unwrap_or_default());
        }
        self.commands.resume();
    }
}

impl Handler for Wire<'_> {
    fn apc_start(&mut self) {
        self.commands.start(self.hub.remote_seen());
    }

    fn apc_put(&mut self, data: &[u8]) {
        self.commands.put(data);
    }

    fn apc_end(&mut self, cancelled: bool) {
        let hub = self.hub;
        let outcomes = self.commands.end(
            cancelled,
            self.now,
            ENV,
            &|echo| hub.stored_size(echo),
            (hub.remote_seen(), &|| hub.in_remote_session()),
        );
        for outcome in outcomes {
            match outcome {
                Outcome::Query(reply) => self.queries.extend(reply.encode()),
                Outcome::Pending(pending) => {
                    self.motions.push(pending.motion());
                    if matches!(pending, Pending::Raw { .. }) {
                        hub.park(pending);
                        self.commands.halt();
                    } else {
                        self.answers
                            .extend(hub.apply(pending, &self.screen).unwrap_or_default());
                    }
                }
            }
        }
    }

    fn halt_requested(&self) -> bool {
        self.commands.halted()
    }
}

struct Rig {
    commands: Commands,
    parser: Processor,
    hub: Hub,
    /// Which screen the parser is on when a command ends.
    alt: bool,
    /// Where the cursor is when a command ends.
    cursor: (i32, u16),
    /// The answers to queries.
    queries: Vec<Vec<u8>>,
    /// Whether the pane is in a remote session: what the stream and the
    /// decode are told.
    remote: Arc<AtomicBool>,
}

impl Rig {
    fn new() -> Self {
        Self::with_hub(Hub::new())
    }

    fn with_hub(hub: Hub) -> Self {
        let remote = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&remote);
        hub.set_remote_gate(Box::new(move || flag.load(Ordering::Acquire)));
        Self {
            commands: Commands::default(),
            parser: Processor::new(),
            hub,
            alt: false,
            cursor: (0, 0),
            queries: Vec::new(),
            remote,
        }
    }

    fn screen(&self) -> Screen {
        Screen {
            alt: self.alt,
            cursor: self.cursor,
            ..Screen::blank(self.alt)
        }
    }

    /// Feeds `bytes` as one read and applies every command that comes out.
    /// Returns how the records move the cursor and the answers.
    fn run(&mut self, bytes: &[u8]) -> (Vec<Motion>, Vec<u8>) {
        let mut motions = Vec::new();
        let mut now = Instant::now();
        let answers = self.run_at(bytes, &mut now, &mut motions);
        (motions, answers)
    }

    /// [`Rig::run`] at the time `now`, for what depends on the time between
    /// two commands; `motions` is added to.
    fn run_at(&mut self, bytes: &[u8], now: &mut Instant, motions: &mut Vec<Motion>) -> Vec<u8> {
        let mut answers = Vec::new();
        self.feed(bytes, *now, motions, &mut answers);
        answers
    }

    /// The parser given `bytes` the way the read loop gives them: stopped
    /// behind each command that needs decoding, which is then decoded and
    /// applied before the parse goes on.
    fn feed(
        &mut self,
        bytes: &[u8],
        now: Instant,
        motions: &mut Vec<Motion>,
        answers: &mut Vec<u8>,
    ) {
        let screen = self.screen();
        let mut wire = Wire {
            commands: &mut self.commands,
            hub: &self.hub,
            screen,
            now,
            motions,
            answers,
            queries: &mut self.queries,
        };
        let mut rest = bytes;
        loop {
            let used = self.parser.advance_until_halt(&mut wire, rest);
            rest = &rest[used..];
            if !wire.halt_requested() {
                break;
            }
            wire.resolve();
        }
        assert!(
            rest.is_empty(),
            "the parser left bytes it was not halted for"
        );
    }

    /// The parse of `bytes` up to the first halt and no further: how many bytes
    /// it took. What it was halted for is parked in the hub, undecoded; the
    /// halt stands until [`Rig::settle`].
    fn until_halt(&mut self, bytes: &[u8]) -> usize {
        let screen = self.screen();
        let (mut motions, mut answers) = (Vec::new(), Vec::new());
        let mut wire = Wire {
            commands: &mut self.commands,
            hub: &self.hub,
            screen,
            now: Instant::now(),
            motions: &mut motions,
            answers: &mut answers,
            queries: &mut self.queries,
        };
        self.parser.advance_until_halt(&mut wire, bytes)
    }

    /// What the read loop does for a halted parser: the decoding, the record
    /// applied where its command ended, the halt taken back. The answers.
    fn settle(&mut self) -> Vec<u8> {
        let mut answers = Vec::new();
        let mut motions = Vec::new();
        let screen = self.screen();
        let mut wire = Wire {
            commands: &mut self.commands,
            hub: &self.hub,
            screen,
            now: Instant::now(),
            motions: &mut motions,
            answers: &mut answers,
            queries: &mut self.queries,
        };
        wire.resolve();
        answers
    }

    fn main_ids(&self) -> Vec<u32> {
        self.hub.with_images(|images| images.main().ids())
    }

    fn pixels(&self, id: u32) -> Option<Vec<u8>> {
        self.hub.with_images(|images| {
            images
                .main()
                .get(id)
                .map(|image| image.pixels.as_ref().clone())
        })
    }
}

fn ok(id: u32) -> Vec<u8> {
    format!("\x1b_Gi={id};OK\x1b\\").into_bytes()
}

fn error_of(answers: &[u8]) -> String {
    String::from_utf8_lossy(answers).into_owned()
}

// ─── transmission ───────────────────────────────────────────────────────

#[test]
fn a_raw_rgba_transmission_is_stored_and_acknowledged() {
    let mut rig = Rig::new();
    let pixels = rgba(2, 1, 1);
    let (motions, answers) = rig.run(&wire("a=t,f=32,s=2,v=1,i=7", &pixels));
    assert_eq!(motions, [STAYS], "the command comes to one record");
    assert_eq!(answers, ok(7));
    assert_eq!(rig.pixels(7), Some(pixels));
    rig.hub.with_images(|images| {
        let image = images.main().get(7).expect("stored");
        assert_eq!((image.width, image.height), (2, 1));
    });
}

#[test]
fn rgb_gains_an_opaque_alpha_and_default_format_is_rgba() {
    let mut rig = Rig::new();
    rig.run(&wire("a=t,f=24,s=2,v=1,i=1", &[1, 2, 3, 4, 5, 6]));
    assert_eq!(rig.pixels(1), Some(vec![1, 2, 3, 255, 4, 5, 6, 255]));
    rig.run(&wire("s=1,v=1,i=2", &[9, 8, 7, 6]));
    assert_eq!(rig.pixels(2), Some(vec![9, 8, 7, 6]));
}

#[test]
fn a_zlib_payload_is_inflated() {
    let mut rig = Rig::new();
    let pixels = rgba(8, 8, 3);
    let (_, answers) = rig.run(&wire("a=T,f=32,o=z,s=8,v=8,i=4", &zlib(&pixels)));
    assert_eq!(answers, ok(4));
    assert_eq!(rig.pixels(4), Some(pixels));
}

#[test]
fn png_payloads_of_every_color_type_come_out_as_rgba() {
    let mut rig = Rig::new();
    struct Case {
        color: png::ColorType,
        palette: Option<&'static [u8]>,
        trns: Option<&'static [u8]>,
        data: &'static [u8],
        expected: &'static [u8],
    }
    let cases = [
        Case {
            color: png::ColorType::Rgba,
            palette: None,
            trns: None,
            data: &[10, 20, 30, 40, 50, 60, 70, 80],
            expected: &[10, 20, 30, 40, 50, 60, 70, 80],
        },
        Case {
            color: png::ColorType::Rgb,
            palette: None,
            trns: None,
            data: &[10, 20, 30, 40, 50, 60],
            expected: &[10, 20, 30, 255, 40, 50, 60, 255],
        },
        Case {
            color: png::ColorType::Grayscale,
            palette: None,
            trns: None,
            data: &[7, 9],
            expected: &[7, 7, 7, 255, 9, 9, 9, 255],
        },
        Case {
            color: png::ColorType::GrayscaleAlpha,
            palette: None,
            trns: None,
            data: &[7, 100, 9, 200],
            expected: &[7, 7, 7, 100, 9, 9, 9, 200],
        },
        Case {
            color: png::ColorType::Indexed,
            palette: Some(&[1, 2, 3, 4, 5, 6]),
            trns: Some(&[128, 255]),
            data: &[0, 1],
            expected: &[1, 2, 3, 128, 4, 5, 6, 255],
        },
    ];
    for (id, case) in cases.into_iter().enumerate() {
        let id = id as u32 + 1;
        let color = case.color;
        let file = png_file(
            2,
            1,
            color,
            png::BitDepth::Eight,
            case.palette,
            case.trns,
            case.data,
        );
        let (_, answers) = rig.run(&wire(&format!("a=t,f=100,i={id}"), &file));
        assert_eq!(answers, ok(id), "{color:?}");
        assert_eq!(rig.pixels(id).as_deref(), Some(case.expected), "{color:?}");
    }
}

#[test]
fn a_sixteen_bit_png_is_brought_down_to_eight() {
    let mut rig = Rig::new();
    let file = png_file(
        1,
        1,
        png::ColorType::Rgb,
        png::BitDepth::Sixteen,
        None,
        None,
        &[0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc],
    );
    rig.run(&wire("a=t,f=100,i=1", &file));
    assert_eq!(rig.pixels(1), Some(vec![0x12, 0x56, 0x9a, 255]));
}

#[test]
fn a_png_inside_zlib_is_inflated_first() {
    let mut rig = Rig::new();
    let file = png_file(
        1,
        1,
        png::ColorType::Rgba,
        png::BitDepth::Eight,
        None,
        None,
        &[1, 2, 3, 4],
    );
    rig.run(&wire("a=t,f=100,o=z,i=1", &zlib(&file)));
    assert_eq!(rig.pixels(1), Some(vec![1, 2, 3, 4]));
}

// ─── chunks ─────────────────────────────────────────────────────────────

#[test]
fn a_chunked_transmission_is_one_image_whose_chunks_name_only_m() {
    let mut rig = Rig::new();
    let pixels = rgba(4, 4, 9);
    let text = base64(&pixels);
    let pieces: Vec<&[u8]> = text.as_bytes().chunks(24).collect();
    let mut wire_bytes = Vec::new();
    for (at, piece) in pieces.iter().enumerate() {
        let control = match at {
            0 => "a=t,f=32,s=4,v=4,i=11,m=1".to_owned(),
            last if last + 1 == pieces.len() => "m=0".to_owned(),
            _ => "m=1".to_owned(),
        };
        wire_bytes.extend([ESC_G, control.as_bytes(), b";", piece, ST].concat());
    }
    let (motions, answers) = rig.run(&wire_bytes);
    assert_eq!(motions, [STAYS], "the chunks make one record, at the last");
    assert_eq!(answers, ok(11), "one answer, after the last chunk");
    assert_eq!(rig.pixels(11), Some(pixels));
}

#[test]
fn a_chunk_may_stop_inside_a_group_of_four_characters() {
    let mut rig = Rig::new();
    let pixels = rgba(2, 2, 5);
    let text = base64(&pixels);
    let (a, b) = text.as_bytes().split_at(7);
    let first = [ESC_G, b"a=t,f=32,s=2,v=2,i=3,m=1;", a, ST].concat();
    let second = [ESC_G, b"m=0;", b, ST].concat();
    let (_, answers) = rig.run(&[first, second].concat());
    assert_eq!(answers, ok(3));
    assert_eq!(rig.pixels(3), Some(pixels));
}

#[test]
fn nothing_is_applied_until_the_last_chunk_and_the_flag_says_so() {
    let mut rig = Rig::new();
    let mut motions = Vec::new();
    let mut now = Instant::now();
    rig.run_at(
        &[ESC_G, b"a=t,f=32,s=1,v=1,i=1,m=1;AAAA", ST].concat(),
        &mut now,
        &mut motions,
    );
    assert!(
        motions.is_empty(),
        "an unfinished transmission comes to no record"
    );
    assert!(rig.commands.open());
    assert!(rig.main_ids().is_empty());
    rig.run_at(&[ESC_G, b"m=0;AA==", ST].concat(), &mut now, &mut motions);
    assert!(!rig.commands.open());
    assert_eq!(rig.main_ids(), [1]);
}

#[test]
fn a_transmission_that_keeps_coming_is_not_one_that_waited_too_long() {
    // Every chunk is inside the wait of the one before it, though the whole
    // is far longer than the wait (a big image over a slow link).
    let mut rig = Rig::new();
    let mut motions = Vec::new();
    let mut now = Instant::now();
    let step = OPEN_TIMEOUT - Duration::from_secs(1);
    rig.run_at(
        &[ESC_G, b"a=t,f=32,s=1,v=1,i=6,m=1;AAAA", ST].concat(),
        &mut now,
        &mut motions,
    );
    now += step;
    rig.run_at(&[ESC_G, b"m=1;AAAA", ST].concat(), &mut now, &mut motions);
    now += step;
    // 4 + 4 + 0 characters is 6 bytes, and a pixel is 4: the last chunk
    // finishes it with the right size.
    let answers = rig.run_at(&[ESC_G, b"m=0;", ST].concat(), &mut now, &mut motions);
    assert!(
        !error_of(&answers).contains("in time"),
        "{}",
        error_of(&answers)
    );
}

/// kitty's own client writes a transmission's action on every chunk when it
/// is not the default one: a chunk that names nothing but the action, `q` and
/// `m` goes on with the transmission, the image is the chunks' and it is shown
/// once, as the first chunk asked.
#[test]
fn a_chunk_that_repeats_its_transmissions_action_goes_on_with_it() {
    for action in ["T", "t"] {
        let mut rig = Rig::new();
        let pixels = rgba(4, 4, 9);
        let text = base64(&pixels);
        let pieces: Vec<&[u8]> = text.as_bytes().chunks(24).collect();
        let mut wire_bytes = Vec::new();
        for (at, piece) in pieces.iter().enumerate() {
            let more = u8::from(at + 1 < pieces.len());
            let control = if at == 0 {
                format!("a={action},f=32,s=4,v=4,i=11,m={more}")
            } else {
                format!("a={action},q=2,m={more}")
            };
            wire_bytes.extend([ESC_G, control.as_bytes(), b";", piece, ST].concat());
        }
        let (motions, answers) = rig.run(&wire_bytes);
        assert_eq!(rig.pixels(11), Some(pixels), "a={action}");
        assert_eq!(
            motions.len(),
            1,
            "a={action}: one record, at the last chunk"
        );
        assert!(answers.is_empty(), "a={action}: the last q= keeps it quiet");
    }
}

#[test]
fn a_command_of_its_own_ends_a_transmission_in_progress() {
    let mut rig = Rig::new();
    rig.run(&[ESC_G, b"a=t,f=32,s=1,v=1,i=1,m=1;AAAA", ST].concat());
    assert!(rig.commands.open());
    rig.run(&wire("a=t,f=32,s=1,v=1,i=2", &[1, 2, 3, 4]));
    assert!(!rig.commands.open());
    assert_eq!(rig.main_ids(), [2]);
}

#[test]
fn a_transmission_that_waits_too_long_is_dropped_and_told_so() {
    let mut rig = Rig::new();
    let mut motions = Vec::new();
    let mut now = Instant::now();
    rig.run_at(
        &[ESC_G, b"a=t,f=32,s=1,v=1,i=6,m=1;AAAA", ST].concat(),
        &mut now,
        &mut motions,
    );
    now += OPEN_TIMEOUT + Duration::from_secs(1);
    let answers = rig.run_at(&[ESC_G, b"m=0;AA==", ST].concat(), &mut now, &mut motions);
    assert!(
        error_of(&answers).starts_with("\x1b_Gi=6;EINVAL:"),
        "{answers:?}"
    );
    assert!(rig.main_ids().is_empty());
}

// ─── what is refused ────────────────────────────────────────────────────

#[test]
fn every_way_a_payload_can_be_wrong_is_an_answer_and_nothing_is_stored() {
    let mut rig = Rig::new();
    let pixels = rgba(2, 2, 0);
    let big = (MAX_DIMENSION + 1).to_string();
    let cases: Vec<(String, Vec<u8>, &str)> = vec![
        (
            "a=t,f=32,s=2,v=2,i=1".into(),
            pixels[..15].to_vec(),
            "ENODATA",
        ),
        (
            "a=t,f=32,s=2,v=2,i=1".into(),
            [pixels.clone(), vec![0]].concat(),
            "EFBIG",
        ),
        ("a=t,f=32,i=1".into(), pixels.clone(), "EINVAL"),
        ("a=t,f=32,s=0,v=2,i=1".into(), pixels.clone(), "EINVAL"),
        (format!("a=t,f=32,s={big},v=1,i=1"), pixels.clone(), "EFBIG"),
        (
            "a=t,f=32,s=10000,v=10000,i=1".into(),
            pixels.clone(),
            "ENOSPC",
        ),
        (
            "a=t,f=32,o=z,s=2,v=2,i=1".into(),
            b"not zlib at all".to_vec(),
            "EINVAL",
        ),
        (
            "a=t,f=32,o=z,s=2,v=2,i=1".into(),
            zlib(&rgba(4, 4, 0)),
            "EFBIG",
        ),
        ("a=t,f=100,i=1".into(), b"not a png".to_vec(), "EINVAL"),
        ("a=t,f=100,i=1".into(), Vec::new(), "EINVAL"),
        ("a=t,t=f,i=1".into(), b"/tmp/x".to_vec(), "EINVAL"),
        ("a=t,f=7,i=1".into(), pixels.clone(), "EINVAL"),
        ("a=f,i=1".into(), pixels.clone(), "EINVAL"),
        ("a=z,i=1".into(), pixels.clone(), "EINVAL"),
    ];
    for (control, payload, code) in cases {
        let (_, answers) = rig.run(&wire(&control, &payload));
        let text = error_of(&answers);
        assert!(
            text.starts_with(&format!("\x1b_Gi=1;{code}:")) && text.ends_with("\x1b\\"),
            "{control}: {text:?}"
        );
        assert!(rig.main_ids().is_empty(), "{control}");
    }
}

#[test]
fn a_png_that_is_cut_short_or_corrupt_does_not_panic() {
    let mut rig = Rig::new();
    let file = png_file(
        8,
        8,
        png::ColorType::Rgba,
        png::BitDepth::Eight,
        None,
        None,
        &rgba(8, 8, 1),
    );
    for cut in [8, 20, file.len() / 2] {
        let (_, answers) = rig.run(&wire("a=t,f=100,i=1", &file[..cut]));
        assert!(error_of(&answers).contains("EINVAL"), "{cut}: {answers:?}");
    }
    let mut flipped = file.clone();
    for at in 0..flipped.len() {
        flipped[at] ^= 0xff;
        rig.run(&wire("a=t,f=100,i=1", &flipped));
        flipped[at] ^= 0xff;
    }
}

#[test]
fn invalid_base64_is_refused() {
    let mut rig = Rig::new();
    let bytes = [ESC_G, b"a=t,f=32,s=1,v=1,i=1;AA!A", ST].concat();
    let (_, answers) = rig.run(&bytes);
    assert!(error_of(&answers).starts_with("\x1b_Gi=1;EINVAL:"));
}

#[test]
fn a_payload_past_the_limit_is_refused_without_keeping_it() {
    let mut rig = Rig::new();
    rig.commands = Commands::with_limit(8);
    let (_, answers) = rig.run(&wire("a=t,f=32,s=2,v=2,i=1", &rgba(2, 2, 0)));
    assert!(error_of(&answers).starts_with("\x1b_Gi=1;EFBIG:"));
    assert!(rig.main_ids().is_empty());
}

#[test]
fn a_command_whose_controls_cannot_be_read_still_tells_who_asked() {
    let mut rig = Rig::new();
    let (_, answers) = rig.run(&wire("i=9,a=t,s=banana", &[0; 4]));
    assert!(error_of(&answers).starts_with("\x1b_Gi=9;EINVAL:"));
}

// ─── answers ────────────────────────────────────────────────────────────

#[test]
fn quiet_hides_the_ok_then_everything() {
    let mut rig = Rig::new();
    let px = [0u8; 4];
    let (_, answers) = rig.run(&wire("a=t,f=32,s=1,v=1,i=1,q=1", &px));
    assert!(answers.is_empty(), "q=1 hides OK");
    let (_, answers) = rig.run(&wire("a=t,f=32,s=1,v=1,i=1,q=1", &px[..2]));
    assert!(error_of(&answers).contains("ENODATA"), "q=1 shows errors");
    let (_, answers) = rig.run(&wire("a=t,f=32,s=1,v=1,i=1,q=2", &px[..2]));
    assert!(answers.is_empty(), "q=2 hides errors too");
}

#[test]
fn a_command_with_neither_id_nor_number_cannot_be_answered() {
    let mut rig = Rig::new();
    let (_, answers) = rig.run(&wire("a=t,f=32,s=1,v=1", &[0; 4]));
    assert!(answers.is_empty());
    assert_eq!(rig.hub.with_images(|images| images.main().len()), 1);
    let (_, answers) = rig.run(&wire("a=t,f=32,s=1,v=1", &[0; 2]));
    assert!(answers.is_empty(), "not even an error");
}

#[test]
fn the_answer_names_the_number_and_the_placement_it_was_given() {
    let mut rig = Rig::new();
    let (_, answers) = rig.run(&wire("a=t,f=32,s=1,v=1,I=5,p=2", &[0; 4]));
    let text = error_of(&answers);
    let id = rig.main_ids()[0];
    assert!(id >= 0x8000_0000, "the terminal chose the id: {id}");
    assert_eq!(text, format!("\x1b_Gi={id},I=5,p=2;OK\x1b\\"));
}

#[test]
fn a_query_answers_as_a_store_would_and_stores_nothing() {
    let mut rig = Rig::new();
    let (motions, answers) = rig.run(&wire("i=4207,a=q,t=d,f=24,s=1,v=1", &[0, 0, 0]));
    assert!(motions.is_empty(), "a query comes to no record");
    assert!(answers.is_empty());
    assert_eq!(rig.queries, [ok(4207)]);
    assert!(rig.main_ids().is_empty());
    rig.queries.clear();
    rig.run(&wire("i=4207,a=q,f=24,s=1,v=1", &[0, 0]));
    assert!(error_of(&rig.queries.concat()).contains("ENODATA"));
}

#[test]
fn terminal_browsers_own_query_text_is_a_valid_query() {
    let mut rig = Rig::new();
    rig.run(b"\x1b_Gi=4207,a=q,t=d,f=24,s=1,v=1;AAAA\x1b\\");
    assert_eq!(rig.queries, [ok(4207)]);
}

#[test]
fn a_broken_query_is_still_a_query() {
    let mut rig = Rig::new();
    let (motions, answers) = rig.run(&wire("i=3,a=q,f=99", &[]));
    assert!(motions.is_empty() && answers.is_empty());
    assert!(error_of(&rig.queries.concat()).starts_with("\x1b_Gi=3;EINVAL:"));
}

// ─── the store ──────────────────────────────────────────────────────────

fn parsed(control: &str) -> Command {
    Command::parse(control.as_bytes()).expect("parses")
}

fn decoded(bytes: usize) -> Decoded {
    Decoded {
        width: 1,
        height: (bytes / 4) as u32,
        pixels: vec![0; bytes],
    }
}

#[test]
fn sending_an_id_again_replaces_the_image() {
    let mut rig = Rig::new();
    rig.run(&wire("a=t,f=32,s=1,v=1,i=1", &[1, 1, 1, 1]));
    rig.run(&wire("a=t,f=32,s=1,v=1,i=2", &[2, 2, 2, 2]));
    rig.run(&wire("a=t,f=32,s=1,v=1,i=1", &[3, 3, 3, 3]));
    assert_eq!(rig.pixels(1), Some(vec![3; 4]));
    assert_eq!(rig.main_ids(), [2, 1], "the replaced id is the newest now");
    rig.hub.with_images(|images| {
        assert_eq!(images.main().bytes(), 8);
        assert_eq!(images.main().get(1).expect("kept").generation, 3);
    });
}

#[test]
fn past_the_quota_images_without_a_placement_go_first_then_the_oldest() {
    let mut images = Images::with_quota(100);
    for id in 1..=4 {
        images.store(
            &Screen::blank(false),
            &parsed(&format!("i={id}")),
            decoded(20),
            Motion::NONE,
        );
    }
    // 1 and 3 are shown; 2 and 4 are not.
    images.main_mut().pin(1);
    images.main_mut().pin(3);
    let reply = images.store(
        &Screen::blank(false),
        &parsed("i=5"),
        decoded(40),
        Motion::NONE,
    );
    assert_eq!(reply.encode(), Some(ok(5)));
    // 120 > 100: the oldest unshown (2) goes; 100 now fits.
    assert_eq!(images.main().ids(), [1, 3, 4, 5]);
    images.store(
        &Screen::blank(false),
        &parsed("i=6"),
        decoded(40),
        Motion::NONE,
    );
    // Unshown 4 goes, then 5 (the next unshown, 40 bytes): 100 fits.
    assert_eq!(images.main().ids(), [1, 3, 6]);
    images.main_mut().pin(6);
    images.store(
        &Screen::blank(false),
        &parsed("i=7"),
        decoded(80),
        Motion::NONE,
    );
    // Everything left is shown: the oldest go until 7 fits (1, 3, 6).
    assert_eq!(images.main().ids(), [7]);
    assert!(images.main().bytes() <= 100);
}

#[test]
fn an_image_larger_than_the_quota_is_refused_and_evicts_nothing() {
    let mut images = Images::with_quota(100);
    images.store(
        &Screen::blank(false),
        &parsed("i=1"),
        decoded(60),
        Motion::NONE,
    );
    let reply = images.store(
        &Screen::blank(false),
        &parsed("i=2"),
        decoded(104),
        Motion::NONE,
    );
    assert!(String::from_utf8_lossy(&reply.encode().expect("answered")).contains("ENOSPC"));
    assert_eq!(images.main().ids(), [1]);
}

#[test]
fn past_the_image_limit_images_without_a_placement_go_first_then_the_oldest() {
    let mut images = Images::with_quota(QUOTA_BYTES);
    let store = |images: &mut Images, id: usize| {
        images.store(
            &Screen::blank(false),
            &parsed(&format!("i={id}")),
            decoded(4),
            Motion::NONE,
        )
    };
    for id in 1..=IMAGE_LIMIT {
        store(&mut images, id);
    }
    assert_eq!(images.main().len(), IMAGE_LIMIT);
    // 1 is shown, 2 is the oldest that is not.
    images.main_mut().pin(1);
    let next = IMAGE_LIMIT + 1;
    assert_eq!(store(&mut images, next).encode(), Some(ok(next as u32)));
    let ids = images.main().ids();
    assert_eq!(ids.len(), IMAGE_LIMIT);
    assert_eq!(ids[..2], [1, 3]);
    assert_eq!(ids.last(), Some(&(next as u32)));
    // A flood of one-pixel images keeps the limit, and the shown one.
    for id in next + 1..=next + 3 * IMAGE_LIMIT {
        store(&mut images, id);
    }
    let ids = images.main().ids();
    assert_eq!(ids.len(), IMAGE_LIMIT);
    assert_eq!(ids[0], 1);
    assert_eq!(images.main().bytes(), IMAGE_LIMIT * 4);
}

#[test]
fn each_screen_has_its_own_store_and_the_alternate_one_goes_when_it_is_left() {
    let mut rig = Rig::new();
    rig.run(&wire("a=t,f=32,s=1,v=1,i=1", &[1; 4]));
    rig.alt = true;
    rig.run(&wire("a=t,f=32,s=1,v=1,i=1", &[2; 4]));
    rig.run(&wire("a=t,f=32,s=1,v=1,i=2", &[3; 4]));
    rig.hub.with_images(|images| {
        assert_eq!(images.main().ids(), [1]);
        assert_eq!(images.alt().ids(), [1, 2]);
        assert_eq!(
            images.main().get(1).expect("kept").pixels.as_ref(),
            &vec![1; 4]
        );
    });
    rig.hub.drop_alt();
    rig.hub.with_images(|images| {
        assert_eq!(images.main().ids(), [1], "the main screen's wait for it");
        assert!(images.alt().ids().is_empty());
        assert_eq!(images.alt().bytes(), 0);
    });
}

#[test]
fn deleting_by_id_frees_the_data_only_in_upper_case() {
    let mut rig = Rig::new();
    for id in 1..=3 {
        rig.run(&wire(&format!("a=t,f=32,s=1,v=1,i={id}"), &[0; 4]));
    }
    rig.run(&wire("a=d,d=i,i=1", &[]));
    assert_eq!(
        rig.main_ids(),
        [1, 2, 3],
        "d=i takes the placements, not the data"
    );
    rig.run(&wire("a=d,d=I,i=1", &[]));
    assert_eq!(rig.main_ids(), [2, 3]);
    rig.run(&wire("a=d,d=I,i=99", &[]));
    assert_eq!(rig.main_ids(), [2, 3], "an unknown id is nothing");
    rig.run(&wire("a=d,d=A", &[]));
    assert!(rig.main_ids().is_empty(), "d=A frees what has no placement");
}

#[test]
fn a_shown_image_keeps_its_data_until_its_last_placement_goes() {
    let mut rig = Rig::new();
    rig.run(&wire("a=T,f=32,s=1,v=1,i=1,p=1", &[0; 4]));
    rig.run(&wire("a=p,i=1,p=2", &[]));
    rig.run(&wire("a=d,d=I,i=1,p=1", &[]));
    assert_eq!(rig.main_ids(), [1], "a placement is left");
    rig.run(&wire("a=d,d=I,i=1,p=2", &[]));
    assert!(rig.main_ids().is_empty(), "the last one took the data");
}

#[test]
fn deleting_all_visible_keeps_ided_data_and_drops_anonymous_images() {
    let mut rig = Rig::new();
    rig.run(&wire("a=t,f=32,s=1,v=1,i=1", &[0; 4]));
    rig.run(&wire("a=t,f=32,s=1,v=1", &[0; 4]));
    assert_eq!(rig.hub.with_images(|images| images.main().len()), 2);
    rig.run(&wire("a=d,d=a", &[]));
    assert_eq!(
        rig.main_ids(),
        [1],
        "nothing can address the anonymous one again"
    );
}

#[test]
fn deleting_by_number_takes_the_newest_image_with_it() {
    let mut rig = Rig::new();
    for _ in 0..3 {
        rig.run(&wire("a=t,f=32,s=1,v=1,I=7", &[0; 4]));
    }
    let before = rig.main_ids();
    assert_eq!(before.len(), 3);
    rig.run(&wire("a=d,d=N,I=7", &[]));
    assert_eq!(rig.main_ids(), before[..2]);
    rig.run(&wire("a=d,d=N,I=8", &[]));
    assert_eq!(rig.main_ids(), before[..2]);
}

#[test]
fn deleting_a_range_of_ids() {
    let mut rig = Rig::new();
    for id in [1, 2, 3, 4, 5] {
        rig.run(&wire(&format!("a=t,f=32,s=1,v=1,i={id}"), &[0; 4]));
    }
    rig.run(&wire("a=d,d=R,x=2,y=4", &[]));
    assert_eq!(rig.main_ids(), [1, 5]);
}

#[test]
fn a_delete_with_an_unknown_specifier_is_refused() {
    let mut rig = Rig::new();
    let (_, answers) = rig.run(&wire("a=d,d=!,i=1", &[]));
    assert!(error_of(&answers).starts_with("\x1b_Gi=1;EINVAL:"));
}

// ─── the hub ────────────────────────────────────────────────────────────

#[test]
fn nothing_parked_is_nothing_to_take() {
    let hub = Hub::new();
    assert!(!hub.is_parked());
    assert!(hub.take_parked().is_none());
    assert_eq!(hub.with_images(|images| images.main().len()), 0);
}

#[test]
fn commands_in_one_read_are_applied_in_the_order_of_the_stream() {
    let mut rig = Rig::new();
    // Both arrive in one read, as in a synchronized update: the parser stops
    // behind the first to have it decoded and applied, and then goes on.
    let both = [
        wire("a=t,f=32,s=1,v=1,i=1", &[1; 4]),
        wire("a=t,f=32,s=1,v=1,i=1", &[2; 4]),
    ]
    .concat();
    let (motions, answers) = rig.run(&both);
    assert_eq!(motions, [STAYS, STAYS]);
    assert_eq!(answers, [ok(1), ok(1)].concat());
    assert_eq!(rig.pixels(1), Some(vec![2; 4]));
    assert!(!rig.hub.is_parked());
}

#[test]
fn a_transmission_halts_the_parser_behind_its_command_and_nothing_else_does() {
    let mut rig = Rig::new();
    let first = wire("a=t,f=32,s=1,v=1,i=1", &[1; 4]);
    let bytes = [first.clone(), b"text".to_vec(), wire("a=d,d=I,i=1", &[])].concat();
    let used = rig.until_halt(&bytes);
    assert_eq!(used, first.len(), "right behind the command");
    assert!(rig.commands.halted());
    assert!(rig.hub.is_parked());
    // Nothing is taken while the halt stands.
    assert_eq!(rig.until_halt(&bytes[used..]), 0);
    assert_eq!(rig.settle(), ok(1));
    assert!(!rig.commands.halted() && !rig.hub.is_parked());
    assert_eq!(rig.main_ids(), [1]);
    // A delete needs no decoding: the parser does not stop for it.
    let rest = &bytes[used..];
    assert_eq!(rig.until_halt(rest), rest.len());
    assert!(!rig.commands.halted());
    assert!(rig.main_ids().is_empty());
}

#[test]
fn a_record_applied_before_it_was_decoded_is_decoded_on_the_spot() {
    let rig = Rig::new();
    rig.hub.park_raw("a=t,f=32,s=1,v=1,i=4", b"BAQEBA==");
    let pending = rig.hub.take_parked().expect("parked");
    assert_eq!(rig.hub.apply(pending, &Screen::blank(false)), Some(ok(4)));
    assert_eq!(rig.pixels(4), Some(vec![4; 4]));
}

#[test]
fn a_query_is_decoded_where_it_ends_and_may_only_be_small() {
    let mut rig = Rig::new();
    let big = vec![0u8; QUERY_LIMIT];
    rig.run(&wire("i=5,a=q,f=32,s=100,v=100", &big));
    let text = error_of(&rig.queries.concat());
    assert!(text.starts_with("\x1b_Gi=5;EFBIG:"), "{text:?}");
    assert!(!rig.hub.is_parked());
}

/// A grey PNG of zeros, one bit a pixel: a few bytes of file for a great
/// many pixels.
fn blank_png(width: u32, height: u32) -> Vec<u8> {
    let row = width.div_ceil(8) as usize;
    png_file(
        width,
        height,
        png::ColorType::Grayscale,
        png::BitDepth::One,
        None,
        None,
        &vec![0; row * height as usize],
    )
}

#[test]
fn a_query_whose_few_bytes_hold_a_great_many_pixels_is_refused_before_they_are_decoded() {
    // 10000 × 8388 pixels in some 26 KB of file, 35 KB of base64: under the
    // query's text limit, and 335 MB of pixels once decoded.
    let huge = blank_png(10_000, 8_388);
    assert!(base64(&huge).len() < QUERY_LIMIT);
    // 600 × 600 is past what a query may come to, whatever carries it.
    let past = blank_png(600, 600);
    for (control, payload) in [
        ("i=5,a=q,f=100,o=z", zlib(&huge)),
        ("i=5,a=q,f=100", huge),
        ("i=5,a=q,f=100", past.clone()),
        ("i=5,a=q,f=100,o=z", zlib(&past)),
        (
            "i=5,a=q,f=32,o=z,s=600,v=600",
            zlib(&vec![0; 600 * 600 * 4]),
        ),
    ] {
        let mut rig = Rig::new();
        let started = Instant::now();
        rig.run(&wire(control, &payload));
        let text = error_of(&rig.queries.concat());
        assert!(text.starts_with("\x1b_Gi=5;EFBIG:"), "{control}: {text:?}");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "{control}: refused from the header, nothing inflated"
        );
        assert!(!rig.hub.is_parked());
    }
    // As big as a query may be, it is answered as it always was.
    let most = blank_png(512, 512);
    let colour = png_file(
        512,
        512,
        png::ColorType::Rgba,
        png::BitDepth::Eight,
        None,
        None,
        &rgba(512, 512, 0),
    );
    assert!(base64(&colour).len() < QUERY_LIMIT);
    for (control, payload) in [
        ("i=6,a=q,f=100", colour),
        ("i=6,a=q,f=100", most.clone()),
        ("i=6,a=q,f=100,o=z", zlib(&most)),
        (
            "i=6,a=q,f=32,o=z,s=512,v=512",
            zlib(&vec![0; 512 * 512 * 4]),
        ),
    ] {
        let mut rig = Rig::new();
        rig.run(&wire(control, &payload));
        assert_eq!(rig.queries, [ok(6)], "{control}");
    }
    // What a query can only ask by being decoded is still asked of a
    // transmission: the store's limits are the store's.
    let mut rig = Rig::new();
    let (_, answers) = rig.run(&wire("a=t,f=100,i=7", &past));
    assert_eq!(answers, ok(7));
}

// ─── bytes ──────────────────────────────────────────────────────────────

#[test]
fn replies_are_formed_as_the_protocol_says() {
    let echo = Echo {
        id: 1,
        number: 2,
        placement: 3,
        quiet: 0,
    };
    assert_eq!(
        Reply::ok(echo).encode(),
        Some(b"\x1b_Gi=1,I=2,p=3;OK\x1b\\".to_vec())
    );
    assert_eq!(
        Reply::failed(echo, Error::invalid("bad")).encode(),
        Some(b"\x1b_Gi=1,I=2,p=3;EINVAL:bad\x1b\\".to_vec())
    );
    assert_eq!(Reply::ok(Echo::default()).encode(), None);
}

#[test]
fn garbled_controls_never_reach_a_command() {
    let mut receiver = Receiver::new();
    let mut seen = 0;
    receiver.receive(
        Apc {
            garbled: true,
            ..Apc::default()
        },
        Instant::now(),
        ENV,
        &|_| None,
        &|| false,
        |_| seen += 1,
    );
    assert_eq!(seen, 0);
}

// ─── placing ─────────────────────────────────────────────────────────────

/// 20 × 40 pixels: two columns by two rows of the tests' 10 × 20 pixel cells.
fn tile() -> Vec<u8> {
    rgba(20, 40, 0)
}

fn placed(rig: &Rig) -> Vec<(u32, i32, u16)> {
    rig.hub.with_images(|images| images.main().placed())
}

#[test]
fn a_transmission_that_shows_places_the_image_at_the_cursor_and_moves_it() {
    let mut rig = Rig::new();
    rig.cursor = (3, 5);
    let (motions, answers) = rig.run(&wire("a=T,f=32,s=20,v=40,i=1", &tile()));
    // One row down before the placement (the image is two rows tall), two
    // columns across after it.
    assert_eq!(motions, [moves(1, 2)]);
    assert_eq!(answers, ok(1));
    // When it is placed the cursor is on the image's last row: the top is one
    // up.
    assert_eq!(placed(&rig), [(1, 2, 5)]);
}

#[test]
fn a_cursor_that_is_asked_to_stay_does_not_move() {
    let mut rig = Rig::new();
    rig.cursor = (3, 5);
    let (motions, _) = rig.run(&wire("a=T,f=32,s=20,v=40,i=1,C=1", &tile()));
    assert_eq!(motions, [STAYS]);
    assert_eq!(placed(&rig), [(1, 3, 5)]);
}

#[test]
fn an_image_as_wide_as_the_screen_leaves_the_cursor_on_the_next_line() {
    let mut rig = Rig::new();
    let (motions, _) = rig.run(&wire("a=T,f=32,s=800,v=20,i=1,c=80,r=1", &rgba(800, 20, 0)));
    assert_eq!(motions, [wrapped(0)]);
}

#[test]
fn a_png_is_sized_from_its_header_without_decoding_it() {
    let png = png_file(
        30,
        40,
        png::ColorType::Rgba,
        png::BitDepth::Eight,
        None,
        None,
        &rgba(30, 40, 3),
    );
    // 30 × 40 pixels are three columns by two rows.
    let around = [moves(1, 3)];
    let mut rig = Rig::new();
    let (motions, _) = rig.run(&wire("a=T,f=100,i=1", &png));
    assert_eq!(motions, around);
    // The same inside a zlib stream: the head of it is inflated.
    let mut rig = Rig::new();
    let (motions, _) = rig.run(&wire("a=T,f=100,o=z,i=1", &zlib(&png)));
    assert_eq!(motions, around);
    // The same in chunks.
    let mut rig = Rig::new();
    let text = base64(&png);
    let (head, tail) = text.split_at(64);
    let chunks = [
        ESC_G,
        b"a=T,f=100,i=1,m=1;",
        head.as_bytes(),
        ST,
        ESC_G,
        b"m=0;",
        tail.as_bytes(),
        ST,
    ]
    .concat();
    let (motions, _) = rig.run(&chunks);
    assert_eq!(motions, around);
    assert_eq!(placed(&rig), [(1, -1, 0)]);
}

#[test]
fn an_image_whose_size_cannot_be_told_leaves_the_cursor_alone() {
    let mut rig = Rig::new();
    let (motions, answers) = rig.run(&wire(
        "a=T,f=100,i=1",
        b"not a PNG, not even the start of one",
    ));
    assert_eq!(motions, [STAYS]);
    assert!(error_of(&answers).contains("EINVAL"));
    assert!(placed(&rig).is_empty());
}

#[test]
fn placing_a_stored_image_is_sized_from_the_store_or_from_the_command_before_it() {
    let both = [
        wire("a=t,f=32,s=20,v=40,i=1", &tile()),
        wire("a=p,i=1,p=9", &[]),
    ]
    .concat();
    let around = moves(1, 2);
    // Both in one read: the image is not decoded when the placement is sized.
    let mut rig = Rig::new();
    rig.cursor = (4, 0);
    let (motions, _) = rig.run(&both);
    assert_eq!(motions, [STAYS, around]);
    // The image was sent long ago, and the receiver has forgotten it.
    let mut rig = Rig::new();
    rig.run(&wire("a=t,f=32,s=20,v=40,i=1", &tile()));
    rig.commands = Commands::default();
    let (motions, answers) = rig.run(&wire("a=p,i=1,p=9", &[]));
    assert_eq!(motions, [around]);
    assert_eq!(answers, b"\x1b_Gi=1,p=9;OK\x1b\\");
}

#[test]
fn placing_what_is_not_there_is_an_answer_and_moves_nothing() {
    let mut rig = Rig::new();
    let (motions, answers) = rig.run(&wire("a=p,i=9", &[]));
    assert_eq!(motions, [STAYS]);
    assert!(error_of(&answers).starts_with("\x1b_Gi=9;ENOENT:"));
    assert!(placed(&rig).is_empty());
}

#[test]
fn an_image_sent_by_number_is_placed_by_number() {
    let mut rig = Rig::new();
    rig.run(&wire("a=t,f=32,s=20,v=40,I=5", &tile()));
    let (_, answers) = rig.run(&wire("a=p,I=5", &[]));
    let answer = error_of(&answers);
    assert!(
        answer.contains("I=5") && answer.ends_with(";OK\x1b\\"),
        "{answer:?}"
    );
    assert_eq!(placed(&rig).len(), 1);
}

#[test]
fn a_placement_id_moves_the_placement_it_names_and_no_id_adds_one() {
    let mut rig = Rig::new();
    rig.run(&wire("a=t,f=32,s=20,v=40,i=1", &tile()));
    for (cursor, control) in [
        ((1, 1), "a=p,i=1,p=3,C=1"),
        ((7, 2), "a=p,i=1,p=3,C=1"),
        ((9, 9), "a=p,i=1,p=4,C=1"),
        ((12, 0), "a=p,i=1,C=1"),
        ((13, 0), "a=p,i=1,C=1"),
    ] {
        rig.cursor = cursor;
        rig.run(&wire(control, &[]));
    }
    assert_eq!(
        placed(&rig),
        [(1, 7, 2), (1, 9, 9), (1, 12, 0), (1, 13, 0)],
        "p=3 was moved, the two without an id were added"
    );
}

#[test]
fn a_flood_of_puts_keeps_the_newest_placements_up_to_the_limit() {
    let mut rig = Rig::new();
    rig.run(&wire("a=t,f=32,s=1,v=1,i=1", &[0; 4]));
    rig.cursor = (5, 5);
    rig.run(&wire("a=p,i=1,C=1", &[]));
    rig.cursor = (0, 0);
    let put = wire("a=p,i=1,C=1,q=1", &[]);
    rig.run(&put.repeat(100_000));
    rig.cursor = (3, 7);
    rig.run(&put);
    let placed = placed(&rig);
    assert_eq!(placed.len(), PLACEMENT_LIMIT);
    assert!(!placed.contains(&(1, 5, 5)), "the oldest went");
    assert_eq!(placed.last(), Some(&(1, 3, 7)), "the newest is kept");
    assert_eq!(rig.main_ids(), [1]);
}

#[test]
fn sending_an_id_again_ends_the_placements_of_the_image_it_replaces() {
    let mut rig = Rig::new();
    rig.run(&wire("a=T,f=32,s=20,v=40,i=1,C=1", &tile()));
    rig.run(&wire("a=T,f=32,s=20,v=40,i=2,C=1", &tile()));
    assert_eq!(placed(&rig).len(), 2);
    rig.run(&wire("a=t,f=32,s=20,v=40,i=1", &tile()));
    assert_eq!(placed(&rig), [(2, 0, 0)]);
}

#[test]
fn a_layer_is_signed() {
    let mut rig = Rig::new();
    rig.run(&wire("a=T,f=32,s=20,v=40,i=1,z=-1073741825,C=1", &tile()));
    let z = rig
        .hub
        .with_images(|images| images.main().placements()[0].spec.z);
    assert_eq!(z, -1_073_741_825);
}

#[test]
fn the_layer_changes_at_the_protocols_two_thresholds() {
    let mut rig = Rig::new();
    let layers: Vec<_> = [i32::MIN, -1_073_741_825, -1_073_741_824, -1, 0, i32::MAX]
        .into_iter()
        .map(|z| {
            rig.cursor = (0, 0);
            rig.run(&wire(&format!("a=T,f=32,s=20,v=40,i=1,z={z},C=1"), &tile()));
            let mut pictures = Pictures::default();
            rig.hub.with_images(|images| {
                images.collect(
                    false,
                    (10, 20),
                    80,
                    Window {
                        first: 0,
                        last: 23,
                        shift: 0,
                    },
                    None,
                    &mut pictures,
                );
            });
            pictures.grid()[0].layer()
        })
        .collect();
    use crate::PictureLayer::{Ground, Over, Text};
    assert_eq!(layers, [Ground, Ground, Text, Text, Over, Over]);
}

#[test]
fn deleting_the_visible_placements_leaves_what_has_gone_up_into_the_history() {
    let mut rig = Rig::new();
    rig.run(&wire("a=t,f=32,s=20,v=40,i=1", &tile()));
    for line in [-5, 0, 30] {
        rig.cursor = (line, 0);
        rig.run(&wire("a=p,i=1,C=1", &[]));
    }
    rig.run(&wire("a=d,d=a", &[]));
    assert_eq!(placed(&rig), [(1, -5, 0)]);
}

/// Three placements of one 2 × 2 cell image: A at line 2, column 2; B at line
/// 5, column 10, layer 5; C at line 2, column 20.
fn scene() -> Rig {
    let mut rig = Rig::new();
    rig.run(&wire("a=t,f=32,s=20,v=40,i=1", &tile()));
    for (cursor, control) in [
        ((2, 2), "a=p,i=1,p=1,C=1"),
        ((5, 10), "a=p,i=1,p=2,C=1,z=5"),
        ((2, 20), "a=p,i=1,p=3,C=1"),
    ] {
        rig.cursor = cursor;
        rig.run(&wire(control, &[]));
    }
    rig
}

fn left_after(control: &str) -> Vec<(i32, u16)> {
    let mut rig = scene();
    rig.cursor = (3, 3);
    rig.run(&wire(control, &[]));
    placed(&rig)
        .into_iter()
        .map(|(_, line, col)| (line, col))
        .collect()
}

#[test]
fn deleting_by_position_takes_the_placements_that_cover_it() {
    // Cells are 1-based on the wire: column 3, row 3 is line 2, column 2's
    // neighbour on the right and below A's corner.
    assert_eq!(left_after("a=d,d=p,x=3,y=3"), [(5, 10), (2, 20)]);
    assert_eq!(left_after("a=d,d=p,x=5,y=3"), [(2, 2), (5, 10), (2, 20)]);
    assert_eq!(left_after("a=d,d=q,x=11,y=6,z=5"), [(2, 2), (2, 20)]);
    assert_eq!(
        left_after("a=d,d=q,x=11,y=6,z=4").len(),
        3,
        "the layer differs"
    );
    assert_eq!(left_after("a=d,d=x,x=21"), [(2, 2), (5, 10)]);
    assert_eq!(left_after("a=d,d=y,y=4"), [(5, 10)], "A and C cover line 3");
    assert_eq!(left_after("a=d,d=z,z=5"), [(2, 2), (2, 20)]);
    // The cursor of `scene` is at line 3, column 3: A's.
    assert_eq!(left_after("a=d,d=c"), [(5, 10), (2, 20)]);
}

#[test]
fn what_is_deleted_by_position_frees_the_image_only_in_upper_case() {
    let mut rig = Rig::new();
    rig.run(&wire("a=T,f=32,s=20,v=40,i=1,C=1", &tile()));
    rig.run(&wire("a=d,d=z,z=0", &[]));
    assert_eq!(rig.main_ids(), [1], "the data stays for the next placement");
    rig.run(&wire("a=p,i=1,C=1", &[]));
    rig.run(&wire("a=d,d=Z,z=0", &[]));
    assert!(rig.main_ids().is_empty());
}

#[test]
fn the_frame_gets_the_pictures_in_drawing_order() {
    let mut rig = scene();
    // One in the history, one across the grid's top, one past the right edge.
    for cursor in [(-2, 4), (-1, 0), (0, 85)] {
        rig.cursor = cursor;
        rig.run(&wire("a=p,i=1,C=1", &[]));
    }
    let mut pictures = Pictures::default();
    rig.hub.with_images(|images| {
        images.collect(
            false,
            (10, 20),
            80,
            Window {
                first: 0,
                last: 23,
                shift: 0,
            },
            Some(Window {
                first: -4,
                last: -1,
                shift: 4,
            }),
            &mut pictures,
        );
    });
    let rows = |list: &[Picture]| -> Vec<(i32, u16, i32)> {
        list.iter()
            .map(|picture| (picture.row, picture.col, picture.z))
            .collect()
    };
    // By layer, then by age; the one at column 85 is off the screen.
    assert_eq!(
        rows(pictures.grid()),
        [(2, 2, 0), (2, 20, 0), (-1, 0, 0), (5, 10, 5)]
    );
    // Rows of the band count from its top (line -4).
    assert_eq!(rows(pictures.band()), [(2, 4, 0), (3, 0, 0)]);
    let first = &pictures.grid()[0];
    assert_eq!((first.width, first.height), (20, 40));
    assert_eq!(first.source, [0, 0, 20, 40]);
    assert_eq!(first.size, (20, 40));
    assert_eq!(first.pixels.len(), 20 * 40 * 4);
}

#[test]
fn a_placement_cut_by_its_source_rectangle_is_drawn_that_big() {
    let mut rig = Rig::new();
    rig.run(&wire(
        "a=T,f=32,s=20,v=40,i=1,C=1,x=10,y=20,w=10,h=20,X=3,Y=4",
        &tile(),
    ));
    let mut pictures = Pictures::default();
    rig.hub.with_images(|images| {
        images.collect(
            false,
            (10, 20),
            80,
            Window {
                first: 0,
                last: 23,
                shift: 0,
            },
            None,
            &mut pictures,
        );
    });
    let picture = &pictures.grid()[0];
    assert_eq!(picture.source, [10, 20, 10, 20]);
    assert_eq!((picture.width, picture.height), (10, 20));
    assert_eq!((picture.x_offset, picture.y_offset), (3, 4));
}

// ─── placing, with a grid ────────────────────────────────────────────────

mod screenful {
    use std::sync::Mutex;

    use alacritty_terminal::event::{Event, EventListener};
    use alacritty_terminal::grid::Dimensions;
    use alacritty_terminal::index::{Column, Line};
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::term::{Config, Term};
    use alacritty_terminal::vte::ansi::Processor;

    use super::*;
    use crate::handler::{ClusterHandler, KeptModes};
    use crate::pointer::{PointerShape, PointerShapes};

    const CELL: (u16, u16) = (10, 20);

    /// Where everything the program is told goes, in the order it is told:
    /// the graphics answers and what `Term` itself answers.
    #[derive(Clone, Default)]
    struct Told(Arc<Mutex<Vec<u8>>>);

    impl EventListener for Told {
        fn send_event(&self, event: Event) {
            if let Event::PtyWrite(text) = event {
                self.0
                    .lock()
                    .expect("lock")
                    .extend_from_slice(text.as_bytes());
            }
        }
    }

    struct Sinker {
        hub: Hub,
        told: Told,
        modes: KeptModes,
        pointer: PointerShapes,
        /// How many times the shown pointer shape changed.
        pointer_changes: AtomicUsize,
    }

    impl Sink for Sinker {
        fn hub(&self) -> &Hub {
            &self.hub
        }

        fn cell(&self) -> (u16, u16) {
            CELL
        }

        fn reply(&self, bytes: Vec<u8>) {
            self.told.0.lock().expect("lock").extend(bytes);
        }

        fn kept_modes(&self) -> &KeptModes {
            &self.modes
        }

        fn pointer(&self) -> &PointerShapes {
            &self.pointer
        }

        fn pointer_changed(&self) {
            self.pointer_changes.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// What a session does with the bytes a program writes, minus the PTY: a
    /// real grid behind the real parser, the wrapper that receives the
    /// commands, and the read loop's way of decoding between two stretches of
    /// the parse.
    struct Screenful {
        sink: Sinker,
        term: Term<Told>,
        parser: Processor,
        commands: Commands,
        last_input: bool,
        /// What the parser was given: the stream, commands in it.
        stream: Vec<u8>,
    }

    fn size(columns: usize, lines: usize) -> TermSize {
        TermSize::new(columns, lines)
    }

    impl Screenful {
        fn new(columns: usize, lines: usize, history: usize) -> Self {
            let config = Config {
                scrolling_history: history,
                ..Config::default()
            };
            let told = Told::default();
            Self {
                sink: Sinker {
                    hub: Hub::new(),
                    told: told.clone(),
                    modes: KeptModes::default(),
                    pointer: PointerShapes::default(),
                    pointer_changes: AtomicUsize::new(0),
                },
                term: Term::new(config, &size(columns, lines), told),
                parser: Processor::new(),
                commands: Commands::default(),
                last_input: false,
                stream: Vec::new(),
            }
        }

        /// The bytes of one read, parsed the way the read loop parses them:
        /// stopped behind each command that needs decoding, which is decoded
        /// before the parse goes on.
        fn feed(&mut self, bytes: &[u8]) {
            self.stream.extend_from_slice(bytes);
            let mut rest = bytes;
            loop {
                let mut handler = ClusterHandler::new(&mut self.term, true, &mut self.last_input)
                    .graphics(Some(&self.sink))
                    .commands(&mut self.commands);
                let used = self.parser.advance_until_halt(&mut handler, rest);
                let halted = handler.halt_requested();
                drop(handler);
                rest = &rest[used..];
                if !halted {
                    break;
                }
                self.sink.hub.decode_pending();
                ClusterHandler::new(&mut self.term, true, &mut self.last_input)
                    .graphics(Some(&self.sink))
                    .commands(&mut self.commands)
                    .resume();
            }
            assert!(
                rest.is_empty(),
                "the parser left bytes it was not halted for"
            );
        }

        /// Brings the placements to the grid as it is, as a frame does.
        fn look(&self) {
            self.sink.hub.look(&self.term, CELL);
        }

        fn placed(&self) -> Vec<(u32, i32, u16)> {
            self.look();
            self.sink.hub.with_images(|images| images.main().placed())
        }

        fn placed_alt(&self) -> Vec<(u32, i32, u16)> {
            self.look();
            self.sink.hub.with_images(|images| images.alt().placed())
        }

        fn cursor(&self) -> (i32, usize) {
            let point = self.term.grid().cursor.point;
            (point.line.0, point.column.0)
        }

        fn history(&self) -> usize {
            self.term.history_size()
        }

        /// Everything the program was told since the last call.
        fn told(&self) -> Vec<u8> {
            std::mem::take(&mut self.sink.told.0.lock().expect("lock"))
        }
    }

    /// The same bytes parsed with no store to place into and nobody to answer,
    /// as a bare replay parses them (the cell's size from its own
    /// bookkeeping): the terminal it leaves.
    fn replayed(stream: &[u8], columns: usize, lines: usize, history: usize) -> Term<Told> {
        let config = Config {
            scrolling_history: history,
            ..Config::default()
        };
        let mut term = Term::new(config, &size(columns, lines), Told::default());
        let mut parser: Processor = Processor::new();
        let mut commands = Commands::default();
        commands.set_cell(CELL);
        let mut last_input = false;
        parser.advance(
            &mut ClusterHandler::new(&mut term, true, &mut last_input).commands(&mut commands),
            stream,
        );
        term
    }

    /// `n` line feeds.
    fn lines(n: usize) -> Vec<u8> {
        b"\r\n".repeat(n)
    }

    fn at(line: usize, col: usize) -> Vec<u8> {
        format!("\x1b[{};{}H", line + 1, col + 1).into_bytes()
    }

    fn tile_command(control: &str) -> Vec<u8> {
        wire(control, &tile())
    }

    #[test]
    fn the_cursor_goes_to_the_cell_after_the_image_on_its_last_row() {
        let mut screen = Screenful::new(20, 10, 100);
        screen.feed(&at(3, 5));
        screen.feed(&tile_command("a=T,f=32,s=20,v=40,i=1"));
        assert_eq!(screen.placed(), [(1, 3, 5)]);
        assert_eq!(screen.cursor(), (4, 7), "two columns across, one row down");
        // The cursor that is asked to stay stays.
        screen.feed(&at(0, 0));
        screen.feed(&tile_command("a=T,f=32,s=20,v=40,i=2,C=1"));
        assert_eq!(screen.cursor(), (0, 0));
        assert_eq!(screen.placed()[1], (2, 0, 0));
    }

    #[test]
    fn an_image_below_the_bottom_scrolls_the_screen_and_keeps_its_place() {
        let mut screen = Screenful::new(20, 10, 100);
        screen.feed(&at(8, 0));
        // Four rows from line 8: two of them below the screen.
        screen.feed(&wire("a=T,f=32,s=10,v=80,i=1", &rgba(10, 80, 0)));
        assert_eq!(screen.history(), 2, "the screen scrolled by two");
        assert_eq!(screen.cursor(), (9, 1), "on the last row of the image");
        assert_eq!(screen.placed(), [(1, 6, 0)], "its top is three rows above");
    }

    #[test]
    fn a_synchronized_update_places_where_the_cursor_is_when_the_frame_ends() {
        let mut screen = Screenful::new(20, 10, 100);
        let mut update = b"\x1b[?2026h".to_vec();
        update.extend(at(2, 3));
        update.extend(tile_command("a=T,f=32,s=20,v=40,i=1,C=1"));
        update.extend(b"\x1b[?2026l");
        screen.feed(&update);
        assert_eq!(screen.placed(), [(1, 2, 3)]);
        // And with the movement: the top is one row above the last one.
        let mut update = b"\x1b[?2026h".to_vec();
        update.extend(at(5, 6));
        update.extend(tile_command("a=T,f=32,s=20,v=40,i=2"));
        update.extend(b"\x1b[?2026l");
        screen.feed(&update);
        assert_eq!(screen.placed()[1], (2, 5, 6));
    }

    #[test]
    fn a_clear_in_the_same_read_deletes_what_came_before_it_and_not_what_comes_after() {
        let mut screen = Screenful::new(20, 10, 100);
        screen.feed(&tile_command("a=T,f=32,s=20,v=40,i=1,C=1"));
        let mut read = b"\x1b[2J".to_vec();
        read.extend(at(4, 4));
        read.extend(tile_command("a=T,f=32,s=20,v=40,i=2,C=1"));
        screen.feed(&read);
        assert_eq!(screen.placed(), [(2, 4, 4)]);
    }

    #[test]
    fn a_replay_of_the_stream_has_the_cursor_where_the_image_left_it_without_the_image() {
        let mut screen = Screenful::new(20, 10, 100);
        screen.feed(b"one\r\ntwo");
        screen.feed(&at(8, 3));
        screen.feed(&wire("a=T,f=32,s=10,v=80,i=1", &rgba(10, 80, 0)));
        screen.feed(b"after");
        // The stream as the program wrote it, the image in it; the copy reads
        // that, with nothing to place it in.
        assert!(String::from_utf8_lossy(&screen.stream).contains("_G"));
        let copy = replayed(&screen.stream, 20, 10, 100);
        let point = |term: &Term<Told>| term.grid().cursor.point;
        assert_eq!(point(&copy), point(&screen.term));
        assert_eq!(copy.history_size(), screen.term.history_size());
        for line in -2..10 {
            let row = |term: &Term<Told>| -> String {
                term.grid()[Line(line)]
                    .into_iter()
                    .map(|cell| cell.c)
                    .collect()
            };
            assert_eq!(row(&copy), row(&screen.term), "line {line}");
        }
    }

    #[test]
    fn a_replay_moves_the_cursor_as_the_live_terminal_does_whatever_the_command() {
        let commands = [
            // Across, down, wide, staying, by cells alone, by a stored image.
            wire("a=T,f=32,s=20,v=40,i=1", &tile()),
            wire("a=T,f=32,s=800,v=20,i=2,c=80,r=1", &rgba(800, 20, 0)),
            wire("a=T,f=32,s=20,v=40,i=3,C=1", &tile()),
            wire("a=p,i=1,p=2", &[]),
            wire("a=p,i=9", &[]),
            wire("a=T,t=f,f=100,i=4,c=3,r=2", b"/nowhere"),
            wire("a=T,f=32,s=10,v=500,i=5", &rgba(10, 500, 0)),
        ];
        let mut screen = Screenful::new(80, 24, 100);
        for command in &commands {
            screen.feed(&at(5, 7));
            screen.feed(command);
            let copy = replayed(&screen.stream, 80, 24, 100);
            let cursor = screen.cursor();
            let point = copy.grid().cursor.point;
            assert_eq!((point.line.0, point.column.0), cursor, "{command:?}");
            assert_eq!(copy.history_size(), screen.history(), "{command:?}");
        }
    }

    #[test]
    fn a_placement_moves_up_with_the_output() {
        let mut screen = Screenful::new(20, 10, 100);
        screen.feed(&at(3, 5));
        screen.feed(&tile_command("a=T,f=32,s=20,v=40,i=1"));
        assert_eq!(screen.placed(), [(1, 3, 5)]);
        // The cursor is at line 4; five line feeds reach the bottom and seven
        // more scroll the screen.
        screen.feed(&lines(12));
        assert_eq!(screen.history(), 7);
        assert_eq!(screen.placed(), [(1, -4, 5)]);
    }

    #[test]
    fn a_placement_that_has_gone_up_off_the_history_is_gone() {
        let mut screen = Screenful::new(20, 10, 5);
        screen.feed(&at(3, 5));
        screen.feed(&tile_command("a=T,f=32,s=20,v=40,i=1"));
        screen.feed(&lines(40));
        assert_eq!(screen.placed(), []);
        // Its image stays for the program to place again.
        assert_eq!(
            screen.sink.hub.with_images(|images| images.main().ids()),
            [1]
        );
    }

    #[test]
    fn at_the_bottom_of_a_full_history_a_placement_goes_up_with_the_output() {
        let mut screen = Screenful::new(20, 10, 10);
        // Fill the history: the text of every line is its own.
        for line in 0..60 {
            screen.feed(format!("line {line}\r\n").as_bytes());
        }
        assert_eq!(screen.history(), 10, "the history is full");
        screen.feed(&tile_command("a=T,f=32,s=20,v=40,i=1"));
        let before = screen.placed()[0].1;
        screen.feed(&lines(3));
        assert_eq!(screen.placed()[0].1, before - 3);
        // And much further, but not past the ring.
        screen.feed(&lines(5));
        assert_eq!(screen.placed()[0].1, before - 8);
    }

    #[test]
    fn a_placement_on_the_main_screen_waits_for_it_through_the_alternate_one() {
        let mut screen = Screenful::new(20, 10, 100);
        screen.feed(&at(5, 5));
        screen.feed(&tile_command("a=T,f=32,s=20,v=40,i=1,C=1"));
        screen.feed(b"\x1b[?1049h");
        screen.feed(&at(2, 2));
        screen.feed(&tile_command("a=T,f=32,s=20,v=40,i=7,C=1"));
        assert_eq!(screen.placed_alt(), [(7, 2, 2)]);
        screen.feed(b"\x1b[?1049l");
        assert_eq!(screen.placed_alt(), []);
        assert_eq!(screen.placed(), [(1, 5, 5)], "where it was");
    }

    #[test]
    fn a_placement_on_the_alternate_screen_scrolls_with_it() {
        let mut screen = Screenful::new(20, 10, 100);
        screen.feed(b"\x1b[?1049h");
        screen.feed(&at(5, 0));
        screen.feed(&tile_command("a=T,f=32,s=20,v=40,i=1,C=1"));
        assert_eq!(screen.placed_alt(), [(1, 5, 0)]);
        // From line 5 to the bottom, then two more: the screen scrolls by two.
        screen.feed(&lines(4 + 2));
        assert_eq!(screen.history(), 0, "there is no history to look in");
        assert_eq!(screen.placed_alt(), [(1, 3, 0)]);
        // Wholly above the screen, it is gone.
        screen.feed(&lines(5));
        assert_eq!(screen.placed_alt(), []);
    }

    #[test]
    fn clearing_the_screen_takes_the_placements_that_reach_onto_it() {
        let mut screen = Screenful::new(20, 10, 100);
        // One that has gone up into the history, one on the screen.
        screen.feed(&at(0, 0));
        screen.feed(&tile_command("a=T,f=32,s=20,v=40,i=1,C=1"));
        screen.feed(&lines(30));
        assert_eq!(screen.placed(), [(1, -21, 0)], "wholly in the history");
        screen.feed(&at(2, 2));
        screen.feed(&tile_command("a=T,f=32,s=20,v=40,i=2,C=1"));
        assert_eq!(screen.placed().len(), 2);
        // Erasing part of it does not touch an image.
        screen.feed(b"\x1b[J\x1b[1J\x1b[2K");
        assert_eq!(screen.placed().len(), 2);
        screen.feed(b"\x1b[2J");
        assert_eq!(screen.placed().len(), 1, "the one in the history stays");
        screen.feed(&at(2, 2));
        screen.feed(&tile_command("a=T,f=32,s=20,v=40,i=3,C=1"));
        screen.feed(b"\x1b[3J");
        assert_eq!(screen.placed(), [], "ED 3 takes all of them");
    }

    #[test]
    fn a_reset_takes_the_images_too() {
        let mut screen = Screenful::new(20, 10, 100);
        screen.feed(&tile_command("a=T,f=32,s=20,v=40,i=1,C=1"));
        screen.feed(b"\x1b[?1049h");
        screen.feed(&tile_command("a=T,f=32,s=20,v=40,i=2,C=1"));
        screen.feed(b"\x1bc");
        assert_eq!(screen.placed(), []);
        assert_eq!(screen.placed_alt(), []);
        screen.sink.hub.with_images(|images| {
            assert!(images.main().ids().is_empty() && images.alt().ids().is_empty());
        });
    }

    #[test]
    fn the_sequence_the_graphics_marker_once_was_is_an_unknown_mode_like_any_other() {
        // A program's own `CSI ? … 8166 h` does what an unknown private mode
        // does, which is nothing: no cursor movement, no record, no answer.
        for sequence in [
            &b"\x1b[?8166h"[..],
            b"\x1b[?16384;16384;16384;16384;16385;8166h",
            b"\x1b[?16390;16384;16384;16384;16384;8166h",
            b"\x1b[?8166;8166h",
        ] {
            let mut screen = Screenful::new(20, 10, 100);
            screen.feed(&at(2, 2));
            screen.feed(sequence);
            assert_eq!(screen.cursor(), (2, 2), "{sequence:?}");
            assert_eq!(screen.placed(), []);
            assert!(screen.told().is_empty());
        }
    }

    #[test]
    fn a_command_moves_the_cursor_by_no_more_than_a_screen() {
        // Live, and in a replay with no store: the same rows, no more than the
        // screen has, whatever the command says.
        let bytes = wire("a=T,f=32,s=10,v=1,i=1,c=1,r=500", &rgba(10, 1, 0));
        let mut screen = Screenful::new(20, 5, 100);
        screen.feed(&bytes);
        assert_eq!(screen.cursor(), (4, 1));
        assert!(screen.history() <= 5, "{} lines scrolled", screen.history());
        // And its top is a screen's height above the cursor, not 499 rows.
        assert_eq!(screen.placed(), [(1, -1, 0)]);
        let copy = replayed(&bytes, 20, 5, 100);
        assert_eq!(copy.grid().cursor.point.line.0, 4);
        assert!(copy.history_size() <= 5);
    }

    #[test]
    fn a_command_with_no_store_to_place_into_moves_the_cursor_all_the_same() {
        // A copy with no sink: the command applies nothing, and its
        // movement is the stream's.
        let mut term = Term::new(Config::default(), &size(20, 10), Told::default());
        let mut parser: Processor = Processor::new();
        let mut commands = Commands::default();
        commands.set_cell(CELL);
        let mut last_input = false;
        let mut run = |term: &mut Term<Told>, bytes: &[u8]| {
            parser.advance(
                &mut ClusterHandler::new(term, true, &mut last_input).commands(&mut commands),
                bytes,
            );
            let point = term.grid().cursor.point;
            (point.line.0, point.column.0)
        };
        let (line, column) = run(&mut term, &at(2, 2));
        // One down and two right.
        assert_eq!(
            run(&mut term, &wire("a=T,f=32,s=20,v=40,i=1", &tile())),
            (line + 1, column + 2)
        );
        // An image as wide as the screen: down a line more, to its start.
        assert_eq!(
            run(
                &mut term,
                &wire("a=T,f=32,s=200,v=20,i=2", &rgba(200, 20, 0))
            ),
            (line + 2, 0)
        );
        // One that stays.
        assert_eq!(
            run(&mut term, &wire("a=T,f=32,s=20,v=40,i=3,C=1", &tile())),
            (line + 2, 0)
        );
        // And without a `Commands` at all the command is let go.
        let mut bare = Term::new(Config::default(), &size(20, 10), Told::default());
        let mut parser: Processor = Processor::new();
        parser.advance(
            &mut ClusterHandler::new(&mut bare, true, &mut last_input),
            &wire("a=T,f=32,s=20,v=40,i=1", &tile()),
        );
        assert_eq!(bare.grid().cursor.point.line.0, 0);
    }

    #[test]
    fn a_name_refused_in_a_remote_session_moves_the_cursor_as_its_replay_does() {
        let mut screen = Screenful::new(80, 24, 100);
        screen.sink.hub.set_remote_gate(Box::new(|| true));
        screen.feed(&at(5, 7));
        screen.feed(&wire("a=T,t=f,f=100,s=30,v=40,i=1", b"/nowhere"));
        assert_eq!(screen.cursor(), (6, 10), "by the size the keys give");
        assert!(error_of(&screen.told()).contains("EBADF"));
        let copy = replayed(&screen.stream, 80, 24, 100);
        let point = copy.grid().cursor.point;
        assert_eq!((point.line.0, point.column.0), screen.cursor());
    }

    #[test]
    fn a_query_is_answered_in_the_stream_before_the_device_attributes_that_follow_it() {
        let mut screen = Screenful::new(20, 10, 100);
        screen.feed(
            &[
                wire("i=31,a=q,t=d,f=24,s=1,v=1", &[0, 0, 0]),
                b"\x1b[c".to_vec(),
            ]
            .concat(),
        );
        let told = error_of(&screen.told());
        let graphics = told.find("\x1b_Gi=31;OK").expect("the query is answered");
        let attributes = told.find("\x1b[?").expect("the terminal answers too");
        assert!(graphics < attributes, "{told:?}");
    }

    #[test]
    fn a_query_that_names_a_file_is_answered_in_the_stream_too() {
        let file = Scratch::temp("kitty-query", &[7; 4]);
        let mut screen = Screenful::new(20, 10, 100);
        screen.feed(
            &[
                wire("i=32,a=q,t=f,f=32,s=1,v=1", file.name()),
                b"\x1b[c".to_vec(),
            ]
            .concat(),
        );
        let told = error_of(&screen.told());
        let graphics = told.find("\x1b_Gi=32;OK").expect("the query is answered");
        let attributes = told.find("\x1b[?").expect("the terminal answers too");
        assert!(graphics < attributes, "{told:?}");
    }

    #[test]
    fn a_command_cut_by_the_reads_is_one_command() {
        let bytes = [
            at(1, 1),
            wire("a=T,f=32,s=20,v=40,i=1", &tile()),
            b"after".to_vec(),
        ]
        .concat();
        let mut whole = Screenful::new(20, 10, 100);
        whole.feed(&bytes);
        let expected = whole.told();
        for first in 0..bytes.len() {
            let mut cut = Screenful::new(20, 10, 100);
            cut.feed(&bytes[..first]);
            cut.feed(&bytes[first..]);
            assert_eq!(cut.cursor(), whole.cursor(), "{first}");
            assert_eq!(cut.placed(), whole.placed(), "{first}");
            assert_eq!(cut.told(), expected, "{first}");
        }
    }

    #[test]
    fn the_pixel_mouse_flag_follows_the_stream_and_the_mode_is_answered() {
        let mut screen = Screenful::new(20, 10, 100);
        screen.feed(b"\x1b[?1016$p");
        assert_eq!(screen.told(), b"\x1b[?1016;2$y", "off: reset");
        screen.feed(b"\x1b[?1016h");
        assert!(screen.sink.modes.pixel_mouse());
        screen.feed(b"\x1b[?1016$p");
        assert_eq!(screen.told(), b"\x1b[?1016;1$y", "on: set");
        screen.feed(b"\x1b[?1016l");
        assert!(!screen.sink.modes.pixel_mouse());
        // A reset takes the mode with the rest.
        screen.feed(b"\x1b[?1016h\x1bc");
        assert!(!screen.sink.modes.pixel_mouse());
        // The other mouse modes are alacritty's and do not move it.
        screen.feed(b"\x1b[?1016h\x1b[?1000h\x1b[?1006h\x1b[?1000l");
        assert!(screen.sink.modes.pixel_mouse());
    }

    #[test]
    fn the_pointer_shape_follows_the_stream_one_stack_per_screen() {
        let mut screen = Screenful::new(20, 10, 100);
        let shown = |screen: &Screenful| screen.sink.pointer.shown();
        screen.feed(b"\x1b]22;pointer\x1b\\");
        assert_eq!(shown(&screen), Some(PointerShape::Pointer));
        // The alternate screen keeps a stack of its own…
        screen.feed(b"\x1b[?1049h");
        assert_eq!(shown(&screen), None);
        screen.feed(b"\x1b]22;>text\x1b\\\x1b]22;?__current__,wait\x07");
        assert_eq!(shown(&screen), Some(PointerShape::Text));
        assert_eq!(
            screen.told(),
            b"\x1b]22;text,1\x07",
            "answered in stream order"
        );
        // …and leaving it brings the main screen's back.
        screen.feed(b"\x1b[?1049l");
        assert_eq!(shown(&screen), Some(PointerShape::Pointer));
        // A reset empties both.
        screen.feed(b"\x1bc\x1b[?1049h");
        assert_eq!(shown(&screen), None);
        assert_eq!(
            screen.sink.pointer_changes.load(Ordering::Relaxed),
            5,
            "told once per change: pointer, none, text, pointer, none"
        );
    }

    #[test]
    fn the_resize_report_flag_follows_the_stream_and_reports_the_size_when_set() {
        let mut screen = Screenful::new(20, 10, 100);
        screen.feed(b"\x1b[?2048$p");
        assert_eq!(screen.told(), b"\x1b[?2048;2$y", "off: reset");
        // Set, it tells the size there and then: the grid, and its pixels in the cell the
        // session gives.
        screen.feed(b"\x1b[?2048h");
        assert!(screen.sink.modes.resize_reports());
        let (w, h) = CELL;
        let report = format!("\x1b[48;10;20;{};{}t", 10 * u32::from(h), 20 * u32::from(w));
        assert_eq!(screen.told(), report.as_bytes());
        screen.feed(b"\x1b[?2048$p");
        assert_eq!(screen.told(), b"\x1b[?2048;1$y", "on: set");
        // Off says nothing; a reset takes it with the rest, 1016 too.
        screen.feed(b"\x1b[?2048l");
        assert!(!screen.sink.modes.resize_reports());
        assert_eq!(screen.told(), b"");
        screen.feed(b"\x1b[?2048h\x1b[?1016h");
        screen.told();
        screen.feed(b"\x1bc");
        assert!(!screen.sink.modes.resize_reports());
        assert!(!screen.sink.modes.pixel_mouse());
    }

    #[test]
    fn a_replay_keeps_the_modes_in_its_own_flags_and_answers_nobody() {
        let modes = KeptModes::default();
        let mut term = Term::new(Config::default(), &size(20, 10), Told::default());
        let mut parser: Processor = Processor::new();
        let mut last_input = false;
        parser.advance(
            &mut ClusterHandler::new(&mut term, true, &mut last_input).kept_modes(&modes),
            b"\x1b[?1016h\x1b[?2048h\x1b[?1016$p\x1b[?2048$p",
        );
        assert!(modes.pixel_mouse());
        assert!(modes.resize_reports());
        // Without modes to keep they are read and do nothing.
        let mut bare = Term::new(Config::default(), &size(20, 10), Told::default());
        parser.advance(
            &mut ClusterHandler::new(&mut bare, true, &mut last_input),
            b"\x1b[?1016h\x1b[?2048h",
        );
    }

    #[test]
    fn a_flood_of_small_commands_is_parsed_to_its_end_and_replays_the_same() {
        // The shortest commands that move the cursor (a known image placed), the
        // shortest that move none, and a transmission whose answer is a record.
        let mut input = wire("a=t,f=32,s=30,v=40,i=1", &tile_30x40());
        let movers = wire("a=p,i=1", &[]);
        let stayers = [ESC_G, b"a=d", ST].concat();
        let bare = [ESC_G, ST].concat();
        for at in 0..3000 {
            input.extend_from_slice(match at % 3 {
                0 => &movers,
                1 => &stayers,
                _ => &bare,
            });
            input.push(b"x\n"[at % 2]);
        }
        input.extend_from_slice(b"END");
        let mut screen = Screenful::new(80, 24, 10_000);
        for chunk in input.chunks(4096) {
            screen.feed(chunk);
        }
        let copy = replayed(&screen.stream, 80, 24, 10_000);
        let point = |term: &Term<Told>| term.grid().cursor.point;
        assert_eq!(point(&copy), point(&screen.term));
        assert_eq!(copy.history_size(), screen.history());
        let line = point(&screen.term).line;
        let last: String = (0..80)
            .map(|column| screen.term.grid()[line][Column(column)].c)
            .collect();
        assert!(last.contains("END"), "{last:?}");
    }
}

// ─── files and shared memory ────────────────────────────────────────────

/// The whole answer to a command whose file or object could not be used: the
/// same words whatever the reason.
fn unreadable(id: u32) -> Vec<u8> {
    format!("\x1b_Gi={id};EBADF:Failed to read image file\x1b\\").into_bytes()
}

fn unique(stem: &str) -> String {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    format!(
        "{stem}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

/// A file for a test to name; it is removed when the guard goes.
struct Scratch(PathBuf);

impl Scratch {
    fn in_dir(dir: &Path, stem: &str, bytes: &[u8]) -> Self {
        let path = dir.join(unique(stem));
        std::fs::write(&path, bytes).expect("a scratch file");
        Self(path)
    }

    /// In the temporary directory, where a program leaves what it sends.
    fn temp(stem: &str, bytes: &[u8]) -> Self {
        Self::in_dir(&std::env::temp_dir(), stem, bytes)
    }

    /// A file that is in no temporary directory.
    fn elsewhere(stem: &str, bytes: &[u8]) -> Self {
        Self::in_dir(Path::new(env!("CARGO_MANIFEST_DIR")), stem, bytes)
    }

    fn name(&self) -> &[u8] {
        self.0.as_os_str().as_bytes()
    }

    fn exists(&self) -> bool {
        std::fs::symlink_metadata(&self.0).is_ok()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// A shared memory object holding `bytes`, made the way a program that sends
/// pixels this way makes one. Removed when the guard goes, if it is still
/// there.
struct Object(String);

impl Object {
    fn new(bytes: &[u8]) -> Self {
        let name = format!("/{}", unique("bt-gfx"));
        let c_name = CString::new(name.as_str()).expect("a name");
        // SAFETY: `c_name` is NUL-terminated and outlives the calls; the
        // object is created, sized, written through a mapping of exactly
        // its size and closed here, and the mapping is released before the
        // function returns.
        unsafe {
            let fd = libc::shm_open(
                c_name.as_ptr(),
                libc::O_CREAT | libc::O_EXCL | libc::O_RDWR,
                0o600,
            );
            assert!(fd >= 0, "shm_open: {}", std::io::Error::last_os_error());
            assert_eq!(libc::ftruncate(fd, bytes.len() as libc::off_t), 0);
            let map = libc::mmap(
                std::ptr::null_mut(),
                bytes.len(),
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            );
            assert_ne!(map, libc::MAP_FAILED, "mmap");
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), map.cast::<u8>(), bytes.len());
            libc::munmap(map, bytes.len());
            libc::close(fd);
        }
        Self(name)
    }

    fn name(&self) -> &[u8] {
        self.0.as_bytes()
    }

    fn exists(&self) -> bool {
        let c_name = CString::new(self.0.as_str()).expect("a name");
        // SAFETY: a read-only open of a NUL-terminated name, closed at once.
        unsafe {
            let fd = libc::shm_open(c_name.as_ptr(), libc::O_RDONLY, 0);
            if fd >= 0 {
                libc::close(fd);
            }
            fd >= 0
        }
    }
}

impl Drop for Object {
    fn drop(&mut self) {
        let c_name = CString::new(self.0.as_str()).expect("a name");
        // SAFETY: a NUL-terminated name.
        unsafe { libc::shm_unlink(c_name.as_ptr()) };
    }
}

#[test]
fn a_file_is_read_where_it_lies_and_left_there() {
    let pixels = rgba(2, 1, 9);
    let file = Scratch::temp("bt-image", &pixels);
    let mut rig = Rig::new();
    let (motions, answers) = rig.run(&wire("a=t,t=f,f=32,s=2,v=1,i=7", file.name()));
    assert_eq!(motions, [STAYS]);
    assert_eq!(answers, ok(7));
    assert_eq!(rig.pixels(7), Some(pixels));
    assert!(file.exists(), "t=f does not delete");
}

#[test]
fn a_temporary_file_is_read_and_deleted() {
    let pixels = rgba(2, 1, 4);
    let file = Scratch::temp("tty-graphics-protocol-image", &pixels);
    let mut rig = Rig::new();
    let (_, answers) = rig.run(&wire("a=t,t=t,f=32,s=2,v=1,i=7", file.name()));
    assert_eq!(answers, ok(7));
    assert_eq!(rig.pixels(7), Some(pixels));
    assert!(!file.exists(), "t=t deletes once it is read");
}

#[test]
fn rgb_zlib_and_png_come_out_of_a_file_as_they_do_out_of_the_stream() {
    let mut rig = Rig::new();
    let rgb = Scratch::temp("bt-image", &[10, 20, 30, 40, 50, 60]);
    rig.run(&wire("a=t,t=f,f=24,s=2,v=1,i=1", rgb.name()));
    assert_eq!(rig.pixels(1), Some(vec![10, 20, 30, 255, 40, 50, 60, 255]));
    let packed = Scratch::temp("bt-image", &zlib(&rgba(8, 8, 1)));
    rig.run(&wire("a=t,t=f,f=32,o=z,s=8,v=8,i=2", packed.name()));
    assert_eq!(rig.pixels(2), Some(rgba(8, 8, 1)));
    let png = png_file(
        2,
        2,
        png::ColorType::Rgba,
        png::BitDepth::Eight,
        None,
        None,
        &rgba(2, 2, 5),
    );
    let file = Scratch::temp("bt-image", &png);
    rig.run(&wire("a=t,t=f,f=100,i=3", file.name()));
    assert_eq!(rig.pixels(3), Some(rgba(2, 2, 5)));
}

#[test]
fn a_named_png_moves_the_cursor_by_its_keys_and_never_by_its_file() {
    let png = png_file(
        30,
        40,
        png::ColorType::Rgba,
        png::BitDepth::Eight,
        None,
        None,
        &rgba(30, 40, 3),
    );
    // 30 × 40 pixels are three columns by two rows.
    for (name, medium) in [("bt-image", "f"), ("tty-graphics-protocol-image", "t")] {
        let file = Scratch::temp(name, &png);
        let mut rig = Rig::new();
        let (motions, _) = rig.run(&wire(
            &format!("a=T,t={medium},f=100,s=30,v=40,i=1"),
            file.name(),
        ));
        assert_eq!(motions, [moves(1, 3)], "t={medium}");
        assert_eq!(placed(&rig), [(1, -1, 0)], "t={medium}");
        // The keys are the program's word and the file is not looked at: a
        // wrong size moves the cursor by the wrong size.
        let mut rig = Rig::new();
        let (motions, _) = rig.run(&wire(
            &format!("a=T,t={medium},f=100,s=10,v=20,i=1"),
            file.name(),
        ));
        assert_eq!(motions, [moves(0, 1)], "t={medium}");
    }
    // Without them the cursor stays: the size is in the file, and the file is
    // read long after the stream moved on.
    let file = Scratch::temp("bt-image", &png);
    let mut rig = Rig::new();
    let (motions, _) = rig.run(&wire("a=T,t=f,f=100,i=1", file.name()));
    assert_eq!(motions, [STAYS]);
    // And the file is not even opened to find out.
    let temp = Scratch::temp("tty-graphics-protocol-image", &png);
    let bytes = wire("a=T,t=t,f=100,i=2", temp.name());
    assert_eq!(rig.until_halt(&bytes), bytes.len());
    assert!(temp.exists());
    rig.settle();
}

#[test]
fn what_kitten_icat_sends_for_a_file_moves_the_cursor_by_the_size_it_says() {
    // `kitten icat photo.png` as it goes out: the file, its kind and its size
    // — for a PNG too — and nothing about the cursor.
    let png = tall_png();
    let file = Scratch::temp("bt-image", &png);
    let mut rig = Rig::new();
    let (motions, answers) = rig.run(&wire("a=T,t=f,f=100,s=30,v=40,i=7,q=2", file.name()));
    assert_eq!(motions, [moves(1, 3)]);
    assert!(answers.is_empty(), "q=2 asked not to hear");
    assert_eq!(rig.pixels(7), Some(rgba(30, 40, 3)));
    assert!(file.exists());
    // And through a temporary file it made for the occasion.
    let temp = Scratch::temp("tty-graphics-protocol-icat", &png);
    let (motions, _) = rig.run(&wire("a=T,t=t,f=100,s=30,v=40,i=8", temp.name()));
    assert_eq!(motions, [moves(1, 3)]);
    assert!(!temp.exists());
}

#[test]
fn a_file_is_not_touched_while_the_command_is_received_only_when_it_is_decoded() {
    let file = Scratch::temp("tty-graphics-protocol-image", &rgba(1, 1, 2));
    let mut rig = Rig::new();
    let bytes = wire("a=t,t=t,f=32,s=1,v=1,i=3", file.name());
    assert_eq!(rig.until_halt(&bytes), bytes.len());
    assert!(rig.hub.is_parked());
    assert!(
        file.exists(),
        "nothing is read or deleted while it is received"
    );
    assert_eq!(rig.settle(), ok(3));
    assert!(!file.exists(), "the decode read it");
}

#[test]
fn part_of_a_file_is_read_by_size_and_offset() {
    let pixels = rgba(2, 1, 7);
    let mut bytes = vec![0xee; 16];
    bytes.extend_from_slice(&pixels);
    bytes.extend_from_slice(&[0xdd; 5]);
    let file = Scratch::temp("bt-image", &bytes);
    let mut rig = Rig::new();
    rig.run(&wire("a=t,t=f,f=32,s=2,v=1,O=16,S=8,i=1", file.name()));
    assert_eq!(rig.pixels(1), Some(pixels.clone()));
    // Without a size the read goes to the end, and a raw image takes only
    // what its dimensions call for: the five bytes after it are not read.
    rig.run(&wire("a=t,t=f,f=32,s=2,v=1,O=16,i=2", file.name()));
    assert_eq!(rig.pixels(2), Some(pixels));
    // Too little is the same answer as no file at all, with no number in it.
    for control in [
        "a=t,t=f,f=32,s=2,v=1,O=16,S=4,i=3",
        "a=t,t=f,f=32,s=2,v=1,O=1000,i=3",
        "a=t,t=f,f=32,s=40,v=40,i=3",
    ] {
        let (_, answers) = rig.run(&wire(control, file.name()));
        assert_eq!(answers, unreadable(3), "{control}");
    }
}

#[test]
fn a_temporary_file_that_may_not_be_taken_is_refused_and_left_alone() {
    let pixels = rgba(1, 1, 1);
    let mut rig = Rig::new();
    let control = "a=t,t=t,f=32,s=1,v=1,i=5";
    // In the temporary directory but not named for the protocol.
    let plain = Scratch::temp("bt-image", &pixels);
    // Named for it but in a directory that is not a temporary one.
    let away = Scratch::elsewhere("tty-graphics-protocol-image", &pixels);
    // Named for it, in the temporary directory, but a link to a file that
    // is neither.
    let target = Scratch::elsewhere("bt-image", &pixels);
    let link = Scratch(std::env::temp_dir().join(unique("tty-graphics-protocol-link")));
    std::os::unix::fs::symlink(&target.0, &link.0).expect("a link");
    for file in [&plain, &away, &link] {
        let (_, answers) = rig.run(&wire(control, file.name()));
        assert_eq!(answers, unreadable(5), "{:?}", file.0);
        assert!(file.exists(), "{:?} was touched", file.0);
    }
    assert!(target.exists());
    assert!(rig.main_ids().is_empty());
    // The same file, asked for as an ordinary file, is read; the temporary
    // directory is no reason to refuse it.
    let (_, answers) = rig.run(&wire("a=t,t=f,f=32,s=1,v=1,i=5", plain.name()));
    assert_eq!(answers, ok(5));
}

#[test]
fn what_a_name_comes_to_after_it_was_judged_is_judged_again() {
    // A directory in the name is made a link to another one between the
    // judging of the name and the open of it: what is opened is a file where
    // a temporary file may not be, and the open handle says so. It is not
    // read, and not deleted.
    let dir = std::env::temp_dir().join(unique("tty-graphics-dir"));
    std::fs::create_dir(&dir).expect("a directory");
    let leaf = "tty-graphics-protocol-swapped";
    std::fs::write(dir.join(leaf), rgba(1, 1, 1)).expect("a file");
    let outside = Path::new(env!("CARGO_MANIFEST_DIR")).join(unique("bt-outside"));
    std::fs::create_dir(&outside).expect("a directory");
    std::fs::write(outside.join(leaf), rgba(1, 1, 2)).expect("a file");
    let name = dir.join(leaf).as_os_str().as_bytes().to_vec();
    let (moved, link, target) = (dir.with_extension("moved"), dir.clone(), outside.clone());
    let swapping = name.clone();
    source::on_open(Some(Box::new(move |named| {
        if named == swapping {
            let _ = std::fs::rename(&link, &moved);
            let _ = std::os::unix::fs::symlink(&target, &link);
        }
    })));
    let mut rig = Rig::new();
    let (_, answers) = rig.run(&wire("a=t,t=t,f=32,s=1,v=1,i=1", &name));
    source::on_open(None);
    assert_eq!(answers, unreadable(1));
    assert!(
        outside.join(leaf).exists(),
        "what the open handle was is deleted"
    );
    assert!(rig.main_ids().is_empty());
    let _ = std::fs::remove_file(&dir);
    let _ = std::fs::remove_dir_all(dir.with_extension("moved"));
    let _ = std::fs::remove_dir_all(&outside);
}

#[test]
fn a_file_the_protocol_may_not_name_is_refused_before_it_is_opened() {
    let mut rig = Rig::new();
    let names: [&[u8]; 7] = [
        b"/dev/null",
        b"/dev/zero",
        b"/dev/stdin",
        b"/proc/self/status",
        b"/sys/kernel",
        b"relative/image.png",
        b"~/image.png",
    ];
    for name in names {
        let (_, answers) = rig.run(&wire("a=t,t=f,f=32,s=1,v=1,i=1", name));
        assert_eq!(answers, unreadable(1), "{}", String::from_utf8_lossy(name));
    }
    // A pipe is no file, and opening it frees a writer waiting for a reader:
    // the writer here stays where it was.
    let fifo = std::env::temp_dir().join(unique("bt-fifo"));
    let c_path = CString::new(fifo.as_os_str().as_bytes()).expect("a path");
    // SAFETY: a NUL-terminated path.
    assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
    let released = Arc::new(AtomicBool::new(false));
    let writer = {
        let (fifo, released) = (fifo.clone(), Arc::clone(&released));
        std::thread::spawn(move || {
            let _ = std::fs::OpenOptions::new().write(true).open(&fifo);
            released.store(true, Ordering::Release);
        })
    };
    std::thread::sleep(Duration::from_millis(50));
    let (_, answers) = rig.run(&wire(
        "a=t,t=f,f=32,s=1,v=1,i=1",
        fifo.as_os_str().as_bytes(),
    ));
    assert_eq!(answers, unreadable(1));
    std::thread::sleep(Duration::from_millis(100));
    assert!(!released.load(Ordering::Acquire), "the writer was let go");
    // (Opened for reading, the pipe lets the writer go.)
    let _reader = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(&fifo);
    writer.join().expect("the writer");
    let _ = std::fs::remove_file(&fifo);
    // A descriptor of the terminal's own is no name for a file: the pipe
    // keeps what was put in it, which nothing read.
    let mut ends = [0; 2];
    // SAFETY: `ends` has room for the two descriptors `pipe` writes.
    assert_eq!(unsafe { libc::pipe(ends.as_mut_ptr()) }, 0);
    // SAFETY: the descriptors were just made and belong to these two files.
    let (reader, mut writer) = unsafe {
        (
            std::fs::File::from_raw_fd(ends[0]),
            std::fs::File::from_raw_fd(ends[1]),
        )
    };
    std::io::Write::write_all(&mut writer, b"kept").expect("a pipe");
    let name = format!("/dev/fd/{}", ends[0]);
    let (_, answers) = rig.run(&wire("a=t,t=f,f=32,s=1,v=1,i=1", name.as_bytes()));
    assert_eq!(answers, unreadable(1));
    let mut kept = [0; 4];
    // SAFETY: a read into a buffer of that size.
    let got = unsafe { libc::read(ends[0], kept.as_mut_ptr().cast(), kept.len()) };
    assert_eq!((got, &kept), (4, b"kept"), "something read the pipe");
    drop((reader, writer));
    // A directory is no file either.
    let (_, answers) = rig.run(&wire(
        "a=t,t=f,f=32,s=1,v=1,i=1",
        std::env::temp_dir().as_os_str().as_bytes(),
    ));
    assert_eq!(answers, unreadable(1));
    assert!(rig.main_ids().is_empty());
}

#[test]
fn what_is_in_an_answer_is_never_the_programs_own_text() {
    // An answer is written into the PTY's input: a name repeated in it would
    // be typed into whatever reads that.
    let mut rig = Rig::new();
    let (_, answers) = rig.run(&wire(
        "a=t,t=f,f=32,s=1,v=1,i=1",
        b"/nonexistent/evil\n; touch pwned",
    ));
    assert_eq!(answers, unreadable(1));
    let text = error_of(&answers);
    assert!(
        !text.contains("evil") && !text.contains("pwned"),
        "{text:?}"
    );
}

#[test]
fn a_name_that_cannot_be_one_is_refused_where_it_is_read() {
    let mut rig = Rig::new();
    let long = vec![b'a'; NAME_LIMIT + 1];
    let names: [&[u8]; 3] = [b"", b"/tmp/a\0b", &long];
    for name in names {
        let (_, answers) = rig.run(&wire("a=t,t=f,f=32,s=1,v=1,i=1", name));
        let text = error_of(&answers);
        assert!(text.starts_with("\x1b_Gi=1;EINVAL:"), "{text:?}");
    }
    let (_, answers) = rig.run(b"\x1b_Ga=t,t=f,f=32,s=1,v=1,i=1;not base64!\x1b\\");
    assert!(error_of(&answers).contains("EINVAL"));
    // A name past what the collector keeps is cut and refused, not read.
    let (_, answers) = rig.run(&wire(
        "a=t,t=f,f=32,s=1,v=1,i=1",
        &vec![b'a'; NAME_LIMIT * 3],
    ));
    assert!(error_of(&answers).contains("EINVAL"));
    assert!(rig.main_ids().is_empty());
}

#[test]
fn a_name_is_whole_in_its_command_and_m_does_not_wait_for_more() {
    let file = Scratch::temp("bt-image", &rgba(1, 1, 3));
    let mut rig = Rig::new();
    let (_, answers) = rig.run(&wire("a=t,t=f,f=32,s=1,v=1,m=1,i=2", file.name()));
    assert_eq!(answers, ok(2));
    assert!(!rig.commands.open());
}

#[test]
fn an_image_too_big_to_read_is_refused_without_touching_its_file() {
    let file = Scratch::temp("tty-graphics-protocol-image", &[0; 16]);
    let mut rig = Rig::new();
    // 320 MB of pixels fits the quota but not a transmission.
    let (_, answers) = rig.run(&wire("a=t,t=t,f=32,s=10000,v=8000,i=1", file.name()));
    assert!(
        error_of(&answers).starts_with("\x1b_Gi=1;EFBIG:"),
        "{answers:?}"
    );
    assert!(file.exists(), "refused before the file was looked at");
}

#[test]
fn shared_memory_is_read_and_removed() {
    let pixels = rgba(2, 1, 6);
    let object = Object::new(&pixels);
    let mut rig = Rig::new();
    let (motions, answers) = rig.run(&wire("a=t,t=s,f=32,s=2,v=1,i=4", object.name()));
    assert_eq!(motions, [STAYS]);
    assert_eq!(answers, ok(4));
    assert_eq!(rig.pixels(4), Some(pixels));
    assert!(!object.exists(), "t=s removes the object once it is read");
}

#[test]
fn shared_memory_is_read_from_an_offset_far_past_a_page() {
    // Further in than any page is long, and not on a page boundary.
    let at = 16384 + 24;
    let pixels = rgba(2, 1, 8);
    let mut bytes = vec![0xaa; at];
    bytes.extend_from_slice(&pixels);
    let object = Object::new(&bytes);
    let mut rig = Rig::new();
    let control = format!("a=t,t=s,f=32,s=2,v=1,O={at},S=8,i=1");
    let (_, answers) = rig.run(&wire(&control, object.name()));
    assert_eq!(answers, ok(1));
    assert_eq!(rig.pixels(1), Some(pixels));
}

#[test]
fn an_object_that_cannot_be_used_is_removed_all_the_same() {
    // More pixels than the object has: even where an object is rounded up to
    // a page, 200 × 200 of them are more than one.
    let object = Object::new(&[1, 2, 3]);
    let mut rig = Rig::new();
    let (_, answers) = rig.run(&wire("a=t,t=s,f=32,s=200,v=200,i=1", object.name()));
    assert_eq!(answers, unreadable(1));
    assert!(!object.exists(), "the program handed it over");
    // One that is not there, and names that are not names of objects.
    for name in [&b"/bt-gfx-none"[..], b"no-slash", b"/a/b", b"/", b"/.."] {
        let (_, answers) = rig.run(&wire("a=t,t=s,f=32,s=2,v=1,i=1", name));
        assert_eq!(answers, unreadable(1), "{}", String::from_utf8_lossy(name));
    }
    assert!(rig.main_ids().is_empty());
}

#[test]
fn a_query_that_names_its_pixels_is_answered_where_it_ends_and_keeps_nothing() {
    let mut rig = Rig::new();
    let plain = Scratch::temp("bt-image", &[1, 2, 3]);
    let (motions, answers) = rig.run(&wire("i=9,a=q,t=f,f=24,s=1,v=1", plain.name()));
    assert_eq!(motions, [STAYS], "it is a record, answered in its place");
    assert_eq!(answers, ok(9));
    assert!(plain.exists());
    let temp = Scratch::temp("tty-graphics-protocol-image", &[1, 2, 3]);
    let (_, answers) = rig.run(&wire("i=9,a=q,t=t,f=24,s=1,v=1", temp.name()));
    assert_eq!(answers, ok(9));
    assert!(!temp.exists(), "a query takes a temporary file too");
    let object = Object::new(&[1, 2, 3]);
    let (_, answers) = rig.run(&wire("i=9,a=q,t=s,f=24,s=1,v=1", object.name()));
    assert_eq!(answers, ok(9));
    assert!(!object.exists());
    // What a terminal that cannot read it says, for the program to fall back.
    let (_, answers) = rig.run(&wire("i=9,a=q,t=f,f=24,s=1,v=1", b"/nonexistent/x"));
    assert_eq!(answers, unreadable(9));
    assert!(rig.queries.is_empty(), "none of these was answered early");
    assert!(rig.main_ids().is_empty(), "a query stores nothing");
}

#[test]
fn a_query_that_names_a_file_asks_about_the_whole_of_it() {
    // A query does not have a size of its own: it asks whether this image can
    // be shown, however big it is.
    let file = Scratch::temp("bt-image", &rgba(200, 200, 1));
    let mut rig = Rig::new();
    let (_, answers) = rig.run(&wire("i=2,a=q,t=f,f=32,s=200,v=200", file.name()));
    assert_eq!(answers, ok(2));
    assert!(rig.main_ids().is_empty());
}

#[test]
fn whatever_is_wrong_with_a_named_file_is_one_answer() {
    // Missing, corrupt, short, the wrong format: told apart, they would say
    // which files exist and what is in them.
    let junk = Scratch::temp("bt-image", b"not an image at all, not even close");
    let short = Scratch::temp("bt-image", &[1, 2, 3]);
    let mut rig = Rig::new();
    let cases = [
        ("a=t,t=f,f=32,s=2,v=1,i=1", b"/nonexistent/file".as_slice()),
        ("a=t,t=f,f=100,i=1", junk.name()),
        ("a=t,t=f,f=32,o=z,s=2,v=1,i=1", junk.name()),
        ("a=t,t=f,f=32,s=2,v=1,i=1", short.name()),
        ("a=t,t=f,f=24,s=100,v=100,i=1", short.name()),
    ];
    for (control, name) in cases {
        let (_, answers) = rig.run(&wire(control, name));
        assert_eq!(answers, unreadable(1), "{control}");
    }
    assert!(rig.main_ids().is_empty());
}

// ─── what a read becomes ────────────────────────────────────────────────

/// A 30 × 40 pixel PNG: three columns and two rows at the test cell size.
fn tall_png() -> Vec<u8> {
    png_file(
        30,
        40,
        png::ColorType::Rgba,
        png::BitDepth::Eight,
        None,
        None,
        &rgba(30, 40, 3),
    )
}

#[test]
fn a_named_images_size_is_remembered_for_the_placement_that_follows_it() {
    // `a=p` of an image whose pixels a file held, in the same read: the size
    // the keys gave.
    let file = Scratch::temp("bt-image", &tall_png());
    let mut rig = Rig::new();
    let input = [
        wire("a=T,t=f,f=100,s=30,v=40,i=1,C=1", file.name()),
        wire("a=p,i=1", &[]),
    ]
    .concat();
    let (motions, _) = rig.run(&input);
    // The first leaves the cursor; the second moves it by the size.
    assert_eq!(motions, [STAYS, moves(1, 3)]);
}

#[test]
fn a_command_comes_to_at_most_two_records() {
    // A command is one record, or none, and two when it ended a chunked
    // transmission that had run out of time (a failure) and has its own.
    let mut rig = Rig::new();
    rig.run(&wire("a=t,f=32,s=30,v=40,i=1", &tile_30x40()));
    let mut motions = Vec::new();
    let mut now = Instant::now();
    rig.run_at(
        &[ESC_G, b"a=t,f=32,s=1,v=1,i=6,m=1;AAAA", ST].concat(),
        &mut now,
        &mut motions,
    );
    let before = motions.len();
    now += OPEN_TIMEOUT + Duration::from_secs(1);
    // A placement with the biggest numbers it has: down a screen's worth and,
    // as wide as 9000 cells are, to the start of the next line.
    rig.run_at(&wire("a=p,i=1,c=9000,r=500", &[]), &mut now, &mut motions);
    assert_eq!(motions.len() - before, 2);
    // Everything else a command can do ends in one record, or none.
    for control in [
        "a=p,i=1,c=9000,r=500",
        "a=T,f=32,s=1,v=1,i=2,c=9000,r=500",
        "a=d",
        "a=f,i=1",
        "garbage",
        "a=p,i=999",
    ] {
        let mut rig = Rig::new();
        rig.run(&wire("a=t,f=32,s=30,v=40,i=1", &tile_30x40()));
        let mut motions = Vec::new();
        rig.run_at(
            &wire(control, &rgba(1, 1, 0)),
            &mut Instant::now(),
            &mut motions,
        );
        assert!(motions.len() <= 1, "{control}");
    }
}

#[test]
fn a_named_png_with_cells_and_no_size_moves_the_cursor_by_the_cells() {
    let file = Scratch::temp("bt-image", &tall_png());
    let mut rig = Rig::new();
    let (motions, _) = rig.run(&wire("a=T,t=f,f=100,c=20,r=5,i=1", file.name()));
    assert_eq!(motions, [moves(4, 20)]);
    // A placement of it later is made from the store, which has the size by
    // then (the read that sent it had none to remember).
    let (motions, _) = rig.run(&wire("a=p,i=1", &[]));
    assert_eq!(motions, [moves(1, 3)]);
}

fn tile_30x40() -> Vec<u8> {
    rgba(30, 40, 3)
}

// ─── a remote session ───────────────────────────────────────────────────

/// A rig, and the flag that makes its pane remote.
fn remote_rig() -> (Rig, Arc<AtomicBool>) {
    let rig = Rig::new();
    let remote = Arc::clone(&rig.remote);
    (rig, remote)
}

#[test]
fn in_a_remote_session_a_name_is_refused_without_touching_what_it_names() {
    let (mut rig, remote) = remote_rig();
    remote.store(true, Ordering::Release);
    let plain = Scratch::temp("bt-image", &rgba(1, 1, 1));
    let temp = Scratch::temp("tty-graphics-protocol-image", &rgba(1, 1, 1));
    let object = Object::new(&rgba(1, 1, 1));
    let png = Scratch::temp("bt-image", &tall_png());
    for (control, name, moved) in [
        ("a=t,t=f,f=32,s=1,v=1,i=1", plain.name(), STAYS),
        ("a=t,t=t,f=32,s=1,v=1,i=1", temp.name(), STAYS),
        ("a=t,t=s,f=32,s=1,v=1,i=1", object.name(), STAYS),
        // And one that moves the cursor: it still does, by its keys alone —
        // a replay with no store cannot know the name was refused, and the
        // cursor has to be where the replay puts it.
        ("a=T,t=f,f=100,s=30,v=40,i=1", png.name(), moves(1, 3)),
    ] {
        let (motions, answers) = rig.run(&wire(control, name));
        assert_eq!(answers, unreadable(1), "{control}");
        assert_eq!(motions, [moved], "{control}");
    }
    // A query is answered where it ends, as every query with its pixels in
    // the sequence is.
    for (control, name) in [
        ("i=1,a=q,t=f,f=32,s=1,v=1", plain.name()),
        ("i=1,a=q,t=t,f=32,s=1,v=1", temp.name()),
        ("i=1,a=q,t=s,f=32,s=1,v=1", object.name()),
    ] {
        rig.queries.clear();
        let (motions, answers) = rig.run(&wire(control, name));
        assert!(motions.is_empty() && answers.is_empty(), "{control}");
        assert_eq!(rig.queries, [unreadable(1)], "{control}");
    }
    assert!(plain.exists() && temp.exists() && png.exists());
    assert!(object.exists(), "shared memory is left alone too");
    assert!(rig.main_ids().is_empty());
    // What the pane's own program sends in the sequence is its own.
    let (_, answers) = rig.run(&wire("a=t,f=32,s=1,v=1,i=2", &rgba(1, 1, 1)));
    assert_eq!(answers, ok(2));
    // And the session ending is the end of the refusal.
    remote.store(false, Ordering::Release);
    let (_, answers) = rig.run(&wire("a=t,t=f,f=32,s=1,v=1,i=3", plain.name()));
    assert_eq!(answers, ok(3));
}

#[test]
fn a_session_that_ended_before_the_decode_still_refuses_what_was_read_in_it() {
    // The scanner has acted on the whole of a read before the parser gets to
    // it: a `D` and a prompt in it end the remote session, and the name a
    // program there sent in the same read would be read here, as local. The
    // read noted that the pane was remote ([`Hub::note_remote`]), and the
    // command remembers it.
    let (mut rig, remote) = remote_rig();
    let temp = Scratch::temp("tty-graphics-protocol-image", &rgba(1, 1, 1));
    remote.store(true, Ordering::Release);
    rig.hub.note_remote();
    // What the stream says next ends the session.
    remote.store(false, Ordering::Release);
    let (motions, answers) = rig.run(&wire("a=t,t=t,f=32,s=1,v=1,i=1", temp.name()));
    assert_eq!(answers, unreadable(1));
    assert_eq!(motions, [STAYS]);
    assert!(temp.exists());
    // Once the parser has caught up with the reads, it is forgotten.
    rig.hub.forget_remote();
    let (_, answers) = rig.run(&wire("a=t,t=t,f=32,s=1,v=1,i=2", temp.name()));
    assert_eq!(answers, ok(2));
    assert!(!temp.exists());
}

#[test]
fn a_command_that_began_in_a_remote_read_remembers_it_through_the_reads_that_follow() {
    // The note is the parser's to take back only when it is in no command: a
    // command cut by the reads keeps what its first read noted.
    let (mut rig, remote) = remote_rig();
    let temp = Scratch::temp("tty-graphics-protocol-image", &rgba(1, 1, 1));
    let bytes = wire("a=t,t=t,f=32,s=1,v=1,i=1", temp.name());
    let (head, rest) = bytes.split_at(10);
    remote.store(true, Ordering::Release);
    rig.hub.note_remote();
    rig.run(head);
    remote.store(false, Ordering::Release);
    rig.hub.forget_remote();
    let (_, answers) = rig.run(rest);
    assert_eq!(answers, unreadable(1));
    assert!(temp.exists());
}

#[test]
fn a_remote_session_found_while_a_command_waits_refuses_that_command_too() {
    // The probe that finds a session remote runs apart from the stream: a
    // command ended just before it is decoded just after, and must not be read.
    let (mut rig, remote) = remote_rig();
    let temp = Scratch::temp("tty-graphics-protocol-image", &rgba(1, 1, 1));
    let png = Scratch::temp("bt-image", &tall_png());
    for (bytes, id) in [
        (wire("a=t,t=t,f=32,s=1,v=1,i=1", temp.name()), 1),
        (wire("i=2,a=q,t=f,f=32,s=1,v=1", png.name()), 2),
    ] {
        remote.store(false, Ordering::Release);
        assert_eq!(rig.until_halt(&bytes), bytes.len());
        remote.store(true, Ordering::Release);
        assert_eq!(rig.settle(), unreadable(id));
    }
    assert!(temp.exists() && png.exists(), "neither was opened");
    assert!(rig.main_ids().is_empty());
}
