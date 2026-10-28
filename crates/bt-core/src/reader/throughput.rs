//! The terminal's processing throughput, headless: the six workloads of kitty's
//! `kitten __benchmark__` (the generators of kitty's `tools/cmd/benchmark/main.go`, GPL-3.0,
//! copied as they are), fed through this loop's own read path — the read with the scanner and
//! the graphics tail of the session's PTY wrapper, the lock and the lease, the cluster wrapper,
//! the graphics commands decoded and stored — into a 120 × 80 terminal, and through a bare
//! alacritty `Term` for comparison. What is timed is what kitty times: from the first byte of
//! the data to the last byte of the stream parsed, the three `CSI 5 n` at its end answered.
//!
//! Each run is checked before its number is believed: every byte of the stream was given to the
//! parser, the three answers came, and the screen shows the last line the stream wrote.
//!
//! The reads are a pipe's whole buffer, as the headless benchmarks of other terminals read, and
//! a macOS PTY's: 1 KiB at most, however much is waiting — what a pane reads in fact, and what
//! each read's own cost is paid on.
//!
//! `cargo test --release -p bt-core --lib reader::throughput -- --ignored --nocapture`

use std::io::{self, Read};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event, EventListener, OnResize, WindowSize};
use alacritty_terminal::grid::Dimensions as _;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi;
use polling::{Event as PollingEvent, PollMode, Poller};

use super::{EventLoop, READ_BUFFER_SIZE, State};
use crate::graphics::{self, Hub, Sink};
use crate::handler::KeptModes;
use crate::pointer::PointerShapes;
use crate::shell::Scanner;
use crate::snapshot::Tail;

const COLUMNS: usize = 120;
const LINES: usize = 80;
/// About how much of each workload a run sends: enough for a run to last a second or more.
const TARGET_BYTES: usize = 128 * 1024 * 1024;
/// Runs per workload and path; the median is reported.
const RUNS: usize = 3;

/// The most bytes a read of a macOS PTY gives.
const PTY_READ: usize = 1024;

/// How a run reads the stream and what walks the reads.
#[derive(Clone, Copy)]
struct Path {
    /// The most bytes one read gives.
    read: usize,
    /// The scanner and the tail see every read, as the session's PTY wrapper has them.
    scan: bool,
    /// The cluster wrapper is on, as every window of the application has it.
    cluster: bool,
}

/// A pane's path, read as a pipe is.
const PANE: Path = Path {
    read: READ_BUFFER_SIZE,
    scan: true,
    cluster: true,
};

/// The terminal's answers: counts the `CSI 0 n` the stream's `CSI 5 n` ask for.
#[derive(Clone, Default)]
struct Answers(Arc<AtomicUsize>);

impl EventListener for Answers {
    fn send_event(&self, event: Event) {
        if let Event::PtyWrite(text) = event
            && text == "\x1b[0n"
        {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// The graphics side of a session, without a session.
struct BenchSink {
    hub: Hub,
    modes: KeptModes,
    pointer: PointerShapes,
}

impl Sink for BenchSink {
    fn hub(&self) -> &Hub {
        &self.hub
    }

    fn cell(&self) -> (u16, u16) {
        (10, 20)
    }

    fn reply(&self, _: Vec<u8>) {}

    fn kept_modes(&self) -> &KeptModes {
        &self.modes
    }

    fn pointer(&self) -> &PointerShapes {
        &self.pointer
    }

    fn pointer_changed(&self) {}
}

/// What kitty's benchmark writes for one workload: the setup, the data `reps` times with the
/// reset between, and the closing queries.
struct Workload {
    name: &'static str,
    data: Vec<u8>,
    reps: usize,
}

impl Workload {
    fn new(name: &'static str, data: Vec<u8>) -> Self {
        let reps = (TARGET_BYTES / data.len()).max(1);
        Self { name, data, reps }
    }

    /// kitty's `benchmark_data` without `--render` and without `--with-scrollback`: the
    /// alternate screen, the cursor hidden, every repetition inside a synchronized update.
    fn segment(&self, at: usize) -> Option<&[u8]> {
        const PREFIX: &[u8] = b"\x1b[?1049h\x1b[?25l\x1b[m\x1b[H\x1b[2JRunning\r\n\x1b[?2026h";
        const BETWEEN: &[u8] = b"\x1b[m\x1b[H\x1b[2JRunning\r\n\x1b[?2026l\x1b[?2026h";
        const FINALIZE: &[u8] = b"\x1b[m\x1b[H\x1b[2JWaiting for response indicating parsing \
            finished\r\n\x1b[?2026l\x1b[6n\x1b[5n\x1b[5n\x1b[5n";
        match at {
            0 => Some(PREFIX),
            at if at <= 2 * self.reps => Some(if at % 2 == 1 { &self.data } else { BETWEEN }),
            at if at == 2 * self.reps + 1 => Some(FINALIZE),
            _ => None,
        }
    }

    fn stream_len(&self) -> usize {
        (0..)
            .map_while(|at| self.segment(at))
            .map(<[u8]>::len)
            .sum()
    }

    /// What the rate is counted over, as kitty counts it: the data, not the resets.
    fn data_len(&self) -> usize {
        self.data.len() * self.reps
    }
}

/// The stream as a pipe or a PTY gives it: as much as the buffer and `read` take,
/// `WouldBlock` once it is through.
struct Feed<'a> {
    load: &'a Workload,
    read: usize,
    segment: usize,
    at: usize,
}

impl<'a> Feed<'a> {
    fn new(load: &'a Workload, read: usize) -> Self {
        Self {
            load,
            read,
            segment: 0,
            at: 0,
        }
    }
}

impl Read for Feed<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let want = buf.len().min(self.read);
        let mut given = 0;
        while given < want {
            let Some(segment) = self.load.segment(self.segment) else {
                break;
            };
            let take = (segment.len() - self.at).min(want - given);
            buf[given..given + take].copy_from_slice(&segment[self.at..self.at + take]);
            given += take;
            self.at += take;
            if self.at == segment.len() {
                self.segment += 1;
                self.at = 0;
            }
        }
        if given == 0 {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        Ok(given)
    }
}

/// The session's PTY wrapper as far as a stream with no shell marks takes it: the graphics
/// tail and the scanner see every read before the parser does.
struct Tapped<'a> {
    feed: Feed<'a>,
    sink: Arc<BenchSink>,
    scanner: Scanner,
    tail: Tail,
    events: usize,
    out: io::Sink,
    scan: bool,
}

impl Read for Tapped<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.feed.read(buf)?;
        if !self.scan {
            return Ok(read);
        }
        let mid_sequence = self.tail.mid_sequence();
        self.tail.feed(&buf[..read]);
        let hub = &self.sink.hub;
        if (graphics::may_carry_command(&buf[..read]) || mid_sequence || hub.is_open())
            && hub.in_remote_session()
        {
            hub.note_remote();
        }
        let events = &mut self.events;
        self.scanner.feed(&buf[..read], |_| *events += 1);
        let _ = self.scanner.take_screen_clears();
        Ok(read)
    }
}

impl tty::EventedReadWrite for Tapped<'static> {
    type Reader = Self;
    type Writer = io::Sink;

    unsafe fn register(&mut self, _: &Arc<Poller>, _: PollingEvent, _: PollMode) -> io::Result<()> {
        Ok(())
    }

    fn reregister(&mut self, _: &Arc<Poller>, _: PollingEvent, _: PollMode) -> io::Result<()> {
        Ok(())
    }

    fn deregister(&mut self, _: &Arc<Poller>) -> io::Result<()> {
        Ok(())
    }

    fn reader(&mut self) -> &mut Self::Reader {
        self
    }

    fn writer(&mut self) -> &mut Self::Writer {
        &mut self.out
    }
}

impl tty::EventedPty for Tapped<'static> {
    fn next_child_event(&mut self) -> Option<tty::ChildEvent> {
        None
    }
}

impl OnResize for Tapped<'static> {
    fn on_resize(&mut self, _: WindowSize) {}
}

/// One run: how long it took and what the terminal showed at its end.
struct Run {
    took: Duration,
    parsed: usize,
    answers: usize,
    top: String,
}

fn top_row<U: EventListener>(term: &Term<U>) -> String {
    (0..term.columns())
        .map(|column| term.grid()[Line(0)][Column(column)].c)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

/// Through this loop's read path, as a pane's PTY is read.
fn bateri(load: &'static Workload, path: Path) -> (Run, Arc<BenchSink>) {
    let answers = Answers::default();
    let terminal = Arc::new(FairMutex::new(Term::new(
        Config::default(),
        &TermSize::new(COLUMNS, LINES),
        answers.clone(),
    )));
    let sink = Arc::new(BenchSink {
        hub: Hub::new(),
        modes: KeptModes::default(),
        pointer: PointerShapes::default(),
    });
    let pty = Tapped {
        feed: Feed::new(load, path.read),
        sink: Arc::clone(&sink),
        scanner: Scanner::new(),
        tail: Tail::lean(),
        events: 0,
        out: io::sink(),
        scan: path.scan,
    };
    let mut event_loop = EventLoop::new(
        Arc::clone(&terminal),
        Answers::default(),
        pty,
        false,
        path.cluster,
    )
    .expect("loop");
    event_loop.graphics(Arc::clone(&sink) as Arc<dyn Sink>);
    let mut state = State::default();
    let mut buf = vec![0; READ_BUFFER_SIZE];

    let start = Instant::now();
    let mut parsed = 0;
    loop {
        let got = event_loop.pty_read(&mut state, &mut buf).expect("read");
        if got == 0 {
            break;
        }
        parsed += got;
    }
    let took = start.elapsed();

    let top = top_row(&terminal.lock());
    let run = Run {
        took,
        parsed,
        answers: answers.0.load(Ordering::Relaxed),
        top,
    };
    (run, sink)
}

/// Through alacritty's parser into its `Term`, nothing of ours on the way.
fn bare(load: &Workload) -> Run {
    let answers = Answers::default();
    let mut term = Term::new(
        Config::default(),
        &TermSize::new(COLUMNS, LINES),
        answers.clone(),
    );
    let mut parser: ansi::Processor = ansi::Processor::new();
    let mut feed = Feed::new(load, READ_BUFFER_SIZE);
    let mut buf = vec![0; READ_BUFFER_SIZE];

    let start = Instant::now();
    let mut parsed = 0;
    while let Ok(got) = feed.read(&mut buf) {
        parser.advance(&mut term, &buf[..got]);
        parsed += got;
    }
    let took = start.elapsed();

    Run {
        took,
        parsed,
        answers: answers.0.load(Ordering::Relaxed),
        top: top_row(&term),
    }
}

fn check(path: &str, load: &Workload, run: &Run) {
    assert_eq!(
        run.parsed,
        load.stream_len(),
        "{path} {}: every byte reaches the parser",
        load.name
    );
    assert_eq!(
        run.answers, 3,
        "{path} {}: the three queries answered",
        load.name
    );
    assert_eq!(
        run.top, "Waiting for response indicating parsing finished",
        "{path} {}: the screen the stream left",
        load.name
    );
}

fn median_rate(load: &Workload, runs: &mut [Duration]) -> f64 {
    runs.sort();
    let took = runs[runs.len() / 2];
    load.data_len() as f64 / took.as_secs_f64() / (1024.0 * 1024.0)
}

/// A deterministic stand-in for kitty's `math/rand`: the workloads are random by design, the
/// numbers should not move with the seed.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }

    fn bytes_of(&mut self, n: usize, alphabet: &str) -> Vec<u8> {
        let alphabet = alphabet.as_bytes();
        (0..n)
            .map(|_| alphabet[self.below(alphabet.len())])
            .collect()
    }
}

fn ascii(rng: &mut Rng) -> Vec<u8> {
    rng.bytes_of(
        1024 * 2048 + 13,
        &format!("{ASCII_PRINTABLE}{CONTROL_CHARS}"),
    )
}

fn unicode() -> Vec<u8> {
    format!("{CHINESE_LOREM_IPSUM}{MISC_UNICODE}{CONTROL_CHARS}")
        .repeat(1024)
        .into_bytes()
}

fn unique_unicode() -> Vec<u8> {
    const CELLS: u32 = 256 * 1024;
    const COMBINING: u32 = 0x70;
    let mut data = String::with_capacity(CELLS as usize * 10);
    for cell in 0..CELLS {
        let mut q = cell;
        data.push('a');
        for _ in 0..3 {
            data.push(char::from_u32(0x300 + q % COMBINING).expect("a combining mark"));
            q /= COMBINING;
        }
    }
    data.into_bytes()
}

fn csi(rng: &mut Rng) -> Vec<u8> {
    const SIZE: usize = 1024 * 1024 + 17;
    let printable = format!("{ASCII_PRINTABLE}{CONTROL_CHARS}");
    let mut out = Vec::with_capacity(SIZE + 48);
    while out.len() < SIZE {
        match rng.below(100) {
            0..10 => {
                let n = rng.below(72) + 1;
                out.extend(rng.bytes_of(n, &printable));
            }
            10..30 => out.extend_from_slice(b"\x1b[m\x1b[?1h\x1b[H"),
            30..40 => out.extend_from_slice(b"\x1b[1;2;3;4:3;31m"),
            40..50 => out.extend_from_slice(b"\x1b[38:5:24;48:2:125:136:147m"),
            50..60 => out.extend_from_slice(b"\x1b[58;5;44;2m"),
            60..80 => out.extend_from_slice(b"\x1b[m\x1b[10A\x1b[3E\x1b[2K"),
            _ => out.extend_from_slice(b"\x1b[39m\x1b[10`a\x1b[100b\x1b[?1l"),
        }
    }
    out.extend_from_slice(b"\x1b[m");
    out
}

/// A 1024 × 1024 RGBA image sent uncompressed in 128 KiB chunks of unpadded base64, then
/// freed — `delete: false` keeps it, for the check that it was taken at all.
/// The keys as kitty's client writes them: the ones that differ from Go's zero value, in its
/// order — `a=t` is the zero, so it is never written.
fn images(delete: bool) -> Vec<u8> {
    const SIDE: usize = 1024;
    const CHUNK: usize = 128 * 1024;
    let text = base64(&vec![0; 4 * SIDE * SIDE]);
    let chunks: Vec<&[u8]> = text.as_bytes().chunks(CHUNK).collect();
    let mut out = Vec::with_capacity(text.len() + 1024);
    for (at, chunk) in chunks.iter().enumerate() {
        let more = u8::from(at + 1 < chunks.len());
        let control = if at == 0 {
            format!("q=2,f=32,m={more},s={SIDE},v={SIDE},i=12345")
        } else {
            format!("q=2,m={more}")
        };
        out.extend_from_slice(b"\x1b_G");
        out.extend_from_slice(control.as_bytes());
        out.push(b';');
        out.extend_from_slice(chunk);
        out.extend_from_slice(b"\x1b\\");
    }
    if delete {
        out.extend_from_slice(b"\x1b_Ga=d,q=2,d=I,i=12345\x1b\\");
    }
    out
}

/// Unpadded standard base64, as kitty's `RawStdEncoding`.
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for group in bytes.chunks(3) {
        let word = group.iter().enumerate().fold(0u32, |word, (at, &byte)| {
            word | (u32::from(byte) << (16 - 8 * at))
        });
        for at in 0..=group.len() {
            out.push(TABLE[((word >> (18 - 6 * at)) & 0x3f) as usize] as char);
        }
    }
    out
}

fn long_escape_codes(rng: &mut Rng) -> Vec<u8> {
    let text = rng.bytes_of(8024, ASCII_PRINTABLE);
    let mut one = b"\x1b]6;".to_vec();
    one.extend_from_slice(&text);
    one.push(0x07);
    one.repeat(1024)
}

#[test]
#[ignore = "a benchmark: run in release, by hand"]
fn kitty_benchmark_workloads() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let loads: Vec<&'static Workload> = vec![
        Workload::new("ASCII", ascii(&mut rng)),
        Workload::new("Unicode", unicode()),
        Workload::new("Unique Unicode", unique_unicode()),
        Workload::new("CSI", csi(&mut rng)),
        Workload::new("Images", images(true)),
        Workload::new("Long escape codes", long_escape_codes(&mut rng)),
    ]
    .into_iter()
    .map(|load| &*Box::leak(Box::new(load)))
    .collect();

    // The image is taken, not dropped on the way: one repetition that keeps it.
    let kept = &*Box::leak(Box::new(Workload {
        name: "Images (kept)",
        data: images(false),
        reps: 1,
    }));
    let (run, sink) = bateri(kept, PANE);
    check("bateri", kept, &run);
    assert_eq!(
        sink.hub.with_images(|images| images.alt().ids()),
        [12345],
        "the image is stored on the screen it was sent to"
    );

    let pty = Path {
        read: PTY_READ,
        ..PANE
    };
    println!(
        "{COLUMNS} × {LINES} cells, {RUNS} runs, median; MB/s as kitty counts it (2^20 bytes of data)"
    );
    println!(
        "{:<18} {:>14} {:>14} {:>14}",
        "", "bateri", "bateri, 1 KiB", "alacritty"
    );
    for load in loads {
        let (mut pipe, mut kib, mut bare_runs) = (Vec::new(), Vec::new(), Vec::new());
        for _ in 0..RUNS {
            for (path, runs) in [(PANE, &mut pipe), (pty, &mut kib)] {
                let (run, _) = bateri(load, path);
                check("bateri", load, &run);
                runs.push(run.took);
            }
            let run = bare(load);
            check("alacritty", load, &run);
            bare_runs.push(run.took);
        }
        println!(
            "{:<18} {:>14.1} {:>14.1} {:>14.1}",
            load.name,
            median_rate(load, &mut pipe),
            median_rate(load, &mut kib),
            median_rate(load, &mut bare_runs),
        );
    }
}

const ASCII_PRINTABLE: &str = concat!(
    "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ  `~!@#$%^&*()_+-=[]{}\\|;:'\",<.>/?",
);
const CONTROL_CHARS: &str = concat!("\n\t",);
const CHINESE_LOREM_IPSUM: &str = concat!(
    "旦海司有幼雞讀松鼻種比門真目怪少：扒裝虎怕您跑綠蝶黃，位香法士錯乙音造活羽詞坡村目園尺封鳥朋；法松夕點我冬停雪因科對只貓息加黃住蝶，明鴨乾春呢風乙時昔孝助？小紅女父故去。",
    "飯躲裝個哥害共買去隻把氣年，己你校跟飛百拉！快石牙飽知唱想土人吹象毛吉每浪四又連見、欠耍外豆雞秋鼻。住步帶。",
    "打六申幾麼：或皮又荷隻乙犬孝習秋還何氣；幾裏活打能花是入海乙山節會。種第共後陽沒喜姐三拍弟海肖，行知走亮包，他字幾，的木卜流旦乙左杯根毛。",
    "您皮買身苦八手牛目地止哥彩第合麻讀午。原朋河乾種果「才波久住這香松」兄主衣快他玉坐要羽和亭但小山吉也吃耳怕，也爪斗斥可害朋許波怎祖葉卜。",
    "行花兩耍許車丟學「示想百吃門高事」不耳見室九星枝買裝，枝十新央發旁品丁青給，科房火；事出出孝肉古：北裝愛升幸百東鼻到從會故北「可休笑物勿三游細斗」娘蛋占犬。我羊波雨跳風。",
    "牛大燈兆新七馬，叫這牙後戶耳、荷北吃穿停植身玩間告或西丟再呢，他禾七愛干寺服石安：他次唱息它坐屋父見這衣發現來，苗會開條弓世者吃英定豆哭；跳風掃叫美神。",
    "寸再了耍休壯植己，燈錯和，蝶幾欠雞定和愛，司紅後弓第樹會金拉快喝夕見往，半瓜日邊出讀雞苦歌許開；發火院爸乙；四帶亮錯鳥洋個讀。",
);
const MISC_UNICODE: &str = concat!(
    "‘’“”‹›«»‚„ 😀😛😇😈😉😍😎😮👍👎 —–§¶†‡©®™ →⇒•·°±−×÷¼½½¾",
    "…µ¢£€¿¡¨´¸ˆ˜ ÀÁÂÃÄÅÆÇÈÉÊË ÌÍÎÏÐÑÒÓÔÕÖØ ŒŠÙÚÛÜÝŸÞßàá âãäåæçèéêëìí",
    "îïðñòóôõöøœš ùúûüýÿþªºαΩ∞ ū̀n̂o᷵H̨a̠b̡͓̐c̡͓̐X̡͓̐",
);

/// What each of our walks costs: the pane's path with the scanner and the tail, the cluster
/// wrapper, or both, left out.
#[test]
#[ignore = "a benchmark: run in release, by hand"]
fn where_the_time_goes() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let short = |name, data: Vec<u8>| {
        let reps = (64 * 1024 * 1024 / data.len()).max(1);
        &*Box::leak(Box::new(Workload { name, data, reps }))
    };
    let loads: Vec<&'static Workload> = vec![
        short("ASCII", ascii(&mut rng)),
        short("Unicode", unicode()),
        short("Unique Unicode", unique_unicode()),
        short("CSI", csi(&mut rng)),
        short("Long escape codes", long_escape_codes(&mut rng)),
    ];
    println!(
        "{:<18} {:>9} {:>9} {:>9} {:>9}",
        "", "full", "no scan", "no clust", "neither"
    );
    for load in loads {
        let mut rates = Vec::new();
        for (scan, cluster) in [(true, true), (false, true), (true, false), (false, false)] {
            let mut runs = Vec::new();
            for _ in 0..RUNS {
                let path = Path {
                    scan,
                    cluster,
                    ..PANE
                };
                let (run, _) = bateri(load, path);
                check("bateri", load, &run);
                runs.push(run.took);
            }
            rates.push(median_rate(load, &mut runs));
        }
        println!(
            "{:<18} {:>9.1} {:>9.1} {:>9.1} {:>9.1}",
            load.name, rates[0], rates[1], rates[2], rates[3]
        );
    }
}
