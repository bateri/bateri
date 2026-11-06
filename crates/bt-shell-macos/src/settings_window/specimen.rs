//! The live examples at the top of Appearance, Font, Cursor, Shell and Tabs and Panes: a setting
//! is shown by drawing with it.
//!
//! The terminal's examples are bateri's own drawing, not a lookalike: a screen of a few coloured
//! lines is written into a session with nothing behind it ([`bt_core::pictured`]) and drawn by
//! the renderer ([`bt_gpu::Stage`]) with the theme, the font, the floor under a program's text
//! and the caret the file has, so a stylistic set, a bolder stroke or a corner radius shows as it
//! will in a pane. The tabs example is a drawing of the bar and the cards in miniature
//! ([`tabs_specimen`]). The cursor rows' options are drawn the same way, one small still each
//! ([`Specimens::caret_stills`]): a picture of a caret that is not the renderer's would show a
//! shape, a thickness and a corner no pane draws.
//!
//! **Nothing here keeps a setting.** An example is made from the inputs the last refresh
//! gave ([`Inputs`]) and is drawn only while its page is the open one; the picture of each card
//! is kept with what it was drawn from and drawn again only when that changes, so a toggle on
//! another page costs no GPU pass. With no GPU the terminal's examples are hidden and the page is
//! its rows.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use bt_core::{
    CaretShape, CaretStyle, FontOptions, Session, SessionOptions, Settings, Theme, UnfocusedCaret,
};
use bt_gpu::{Snapshot, Stage, StageLook};
use objc2::rc::Retained;
use objc2::{AnyThread, MainThreadMarker};
use objc2_app_kit::{
    NSColor, NSFont, NSImage, NSImageScaling, NSImageView, NSLayoutAttribute, NSStackView,
    NSStackViewDistribution, NSTextField, NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_foundation::{NSSize, NSString};

use super::pictures::{PICTURE_HEIGHT, PICTURE_WIDTH, Palette, Picture};
use super::tabs_specimen::{self, TabsView};
use super::{Category, DETAIL_WIDTH, PAGE_MARGIN};
use crate::snapshot;

/// The width of a page's content, in points: what an example is as wide as.
const WIDTH: f64 = DETAIL_WIDTH - 2.0 * PAGE_MARGIN;
/// The room between the two cards of a pair.
const GAP: f64 = 12.0;
/// The terminal examples' corner.
const CORNER: f64 = 8.0;

/// Which example.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Appearance,
    Font,
    Cursor,
    Shell,
    Tabs,
}

impl Kind {
    pub(super) const ALL: [Kind; 5] = [
        Kind::Appearance,
        Kind::Font,
        Kind::Cursor,
        Kind::Shell,
        Kind::Tabs,
    ];

    /// The example a page opens with, if it has one.
    pub(super) fn of(category: Category) -> Option<Kind> {
        match category {
            Category::Appearance => Some(Kind::Appearance),
            Category::Font => Some(Kind::Font),
            Category::Cursor => Some(Kind::Cursor),
            Category::Shell => Some(Kind::Shell),
            Category::Tabs => Some(Kind::Tabs),
            _ => None,
        }
    }

    /// How many cards the example can have: Appearance shows the light and the dark theme side by
    /// side, Cursor shows the caret in this window and in an inactive one.
    fn cards(self) -> usize {
        match self {
            Kind::Appearance | Kind::Cursor => 2,
            Kind::Font | Kind::Shell => 1,
            Kind::Tabs => 0,
        }
    }
}

/// What the examples are made from: the file's settings and the themes the app resolved for them
/// (the window reads no theme file itself).
#[derive(Clone)]
pub(super) struct Inputs {
    pub(super) settings: Settings,
    pub(super) light: Theme,
    pub(super) dark: Theme,
    /// The theme the file's choice gives right now.
    pub(super) on_screen: Theme,
    /// The system is in its dark appearance: which of a pair is "in use now".
    pub(super) dark_appearance: bool,
    pub(super) reduce_motion: bool,
}

/// One card of an example: the theme it stands on, what it is called, and whether the window it
/// pictures is key.
#[derive(Clone, Debug, PartialEq)]
struct Face {
    theme: Theme,
    caption: Option<String>,
    tag: Option<&'static str>,
    focused: bool,
}

fn title_of(name: &str) -> String {
    Theme::embedded_title(name).unwrap_or(name).to_owned()
}

/// The cards `kind` shows for `inputs`; none hides the example.
fn faces(kind: Kind, inputs: &Inputs) -> Vec<Face> {
    let settings = &inputs.settings;
    let face = |theme: Theme, caption: Option<String>, tag, focused| Face {
        theme,
        caption,
        tag,
        focused,
    };
    match kind {
        Kind::Appearance if settings.follows_system() => vec![
            face(
                inputs.light,
                Some(title_of(&settings.light_theme)),
                (!inputs.dark_appearance).then_some("In use now"),
                true,
            ),
            face(
                inputs.dark,
                Some(title_of(&settings.dark_theme)),
                inputs.dark_appearance.then_some("In use now"),
                true,
            ),
        ],
        Kind::Appearance => vec![face(
            inputs.on_screen,
            Some(title_of(&settings.theme)),
            None,
            true,
        )],
        Kind::Font => vec![face(inputs.on_screen, None, None, true)],
        Kind::Cursor => vec![
            face(inputs.on_screen, Some("This window".to_owned()), None, true),
            face(
                inputs.on_screen,
                Some("An inactive window".to_owned()),
                None,
                false,
            ),
        ],
        // The plain prompt is what the setting leaves when shell integration is off: with it the
        // prompt is the dock, which this example does not draw.
        Kind::Shell if settings.shell_integration != bt_core::ShellIntegration::Auto => {
            vec![face(inputs.on_screen, None, None, true)]
        }
        Kind::Shell | Kind::Tabs => Vec::new(),
    }
}

/// One line of a screen: the bytes that write it and how many columns it takes.
struct Line {
    bytes: String,
    width: usize,
}

impl Line {
    fn new() -> Self {
        Line {
            bytes: String::new(),
            width: 0,
        }
    }

    fn plain(mut self, text: &str) -> Self {
        self.bytes.push_str(text);
        self.width += text.chars().count();
        self
    }

    /// `text` in `rgb` (a colour a program chose, so the floor under its text applies).
    fn ink(mut self, rgb: u32, text: &str) -> Self {
        self.bytes.push_str(&format!(
            "\x1b[38;2;{};{};{}m{text}\x1b[39m",
            (rgb >> 16) & 0xff,
            (rgb >> 8) & 0xff,
            rgb & 0xff
        ));
        self.width += text.chars().count();
        self
    }

    fn styled(mut self, sgr: &str, text: &str) -> Self {
        self.bytes.push_str(&format!("\x1b[{sgr}m{text}\x1b[0m"));
        self.width += text.chars().count();
        self
    }
}

/// `a` moved `share` of the way to `b`, channel by channel.
fn mix(a: u32, b: u32, share: f64) -> u32 {
    let channel = |shift: u32| {
        let (a, b) = (
            f64::from((a >> shift) & 0xff),
            f64::from((b >> shift) & 0xff),
        );
        ((a + (b - a) * share).round() as u32).min(255) << shift
    };
    channel(16) | channel(8) | channel(0)
}

/// The screen of `kind` in `theme`.
fn lines(kind: Kind, theme: &Theme) -> Vec<Line> {
    let prompt = || {
        Line::new()
            .ink(theme.accent, " ~/bateri ")
            .ink(theme.ansi[5], " main ")
            .ink(theme.success, "\u{276f} ")
    };
    match kind {
        Kind::Appearance => {
            // A program's faint text: a step off the ground, which is what the floor under a
            // program's text (Minimum contrast) lifts.
            let faint = mix(
                theme.background,
                theme.foreground,
                if theme.is_dark() { 0.16 } else { 0.2 },
            );
            vec![
                prompt().plain("git status"),
                Line::new().plain("On branch ").styled("1", "main"),
                Line::new().ink(theme.success, "  modified:   settings.toml"),
                Line::new().ink(theme.error, "  deleted:    old.rs"),
                Line::new().ink(faint, "hint: a program\u{2019}s faint text"),
            ]
        }
        Kind::Font => vec![
            prompt().plain("cargo build --release"),
            Line::new()
                .ink(theme.ansi[4], "fn")
                .plain(" ready(flag: &")
                .ink(theme.ansi[4], "str")
                .plain(") -> ")
                .ink(theme.ansi[4], "bool")
                .plain(" { flag != ")
                .ink(theme.ansi[2], "\"0O1lI\"")
                .plain(" && n >= 3 }"),
            Line::new()
                .ink(theme.ansi[6], "OPENAI_API_KEY")
                .plain("=sk-proj-4f9a2c71e0b3d8"),
            Line::new()
                .ink(theme.success, "   ")
                .styled("1", "Compiling")
                .plain(" bateri v0.24.1 ")
                .styled("3", "(~/bateri)"),
            Line::new()
                .ink(theme.success, "    ")
                .styled("1", "Finished")
                .plain(" release in 1.24s ")
                .ink(theme.dim, "\u{2014} $ * @ & % # {} [] ~"),
        ],
        Kind::Cursor => vec![
            Line::new()
                .ink(theme.success, "\u{276f} ")
                .plain("cargo test"),
            Line::new().ink(theme.dim, "running 24 tests"),
        ],
        Kind::Shell => vec![
            Line::new().ink(theme.dim, "VITE v5.4  ready in 312 ms"),
            Line::new()
                .ink(theme.dim, "\u{279c}  Local:  ")
                .ink(theme.accent, "http://localhost:3000/"),
            Line::new(),
            Line::new().plain("user@mac ~/web % npm run dev"),
        ],
        Kind::Tabs => Vec::new(),
    }
}

/// How many rows `lines` take in a `cols`-column terminal, a line that is longer wrapping onto the
/// next, and one blank row above them.
fn rows_of(lines: &[Line], cols: u16) -> u16 {
    let cols = usize::from(cols.max(1));
    let rows = 1 + lines
        .iter()
        .map(|line| line.width.div_ceil(cols).max(1))
        .sum::<usize>();
    u16::try_from(rows).unwrap_or(u16::MAX)
}

/// The bytes that build the screen in a freshly reset terminal.
fn screen(kind: Kind, lines: &[Line]) -> Vec<u8> {
    // Home on the second row: the first stays blank, the room above the text.
    let mut out = String::from("\x1b[2J\x1b[2;1H");
    if matches!(kind, Kind::Appearance | Kind::Font) {
        // These show text, not the caret.
        out.push_str("\x1b[?25l");
    }
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push_str("\r\n");
        }
        out.push_str(&line.bytes);
        out.push_str("\x1b[0m");
    }
    if kind == Kind::Cursor
        && let Some(first) = lines.first()
    {
        // The caret stands after the typed command, the line above the output it started.
        out.push_str(&format!("\x1b[2;{}H", first.width + 1));
    }
    out.into_bytes()
}

/// A session kept between draws: it is made again only for another theme, caret shape or size.
struct Kept {
    key: (Theme, CaretShape, u16, u16, (u16, u16)),
    session: Session,
}

/// Everything one card's picture depends on.
#[derive(Clone, Debug, PartialEq)]
struct Drawn {
    kind: Kind,
    face: Face,
    font: FontOptions,
    shape: CaretShape,
    caret: CaretStyle,
    contrast: f64,
    scale: f64,
    width: u32,
}

/// The picture of one card: `face` of `kind`, `width_pt` points wide at `scale`; `None` when the
/// session or the GPU said no. The card keeps its session in `kept`.
fn render(
    stage: &mut Stage,
    kept: &mut Option<Kept>,
    kind: Kind,
    face: &Face,
    settings: &Settings,
    width_pt: f64,
    scale: f64,
) -> Option<Snapshot> {
    let metrics = stage.cell_metrics(scale);
    let (cell_w, cell_h) = metrics.cell_px();
    let inset = metrics.grid_inset();
    let width = (width_pt * scale).round() as u32;
    let usable = width.saturating_sub(u32::from(inset.left) + u32::from(inset.right));
    let cols = u16::try_from((usable / u32::from(cell_w.max(1))).max(1)).unwrap_or(u16::MAX);
    let lines = lines(kind, &face.theme);
    let rows = rows_of(&lines, cols);
    let key = (face.theme, settings.cursor, cols, rows, (cell_w, cell_h));
    if kept.as_ref().map(|kept| kept.key) != Some(key) {
        let session = bt_core::pictured(
            SessionOptions {
                command: None,
                working_directory: None,
                home: None,
                env: HashMap::new(),
                cols,
                rows,
                cell_px: (cell_w, cell_h),
                terminal: settings.terminal(),
                theme: face.theme,
                dock: false,
                cluster: true,
                initial_input: None,
                shell_marks: false,
                pane_uuid: None,
                hostname: None,
                replay: None,
                views: None,
            },
            &screen(kind, &lines),
        )
        .ok()?;
        *kept = Some(Kept { key, session });
    }
    let session = &kept.as_ref()?.session;
    // The floor is the session's, read when the picture is drawn: moving it costs no new session.
    session.set_minimum_contrast(settings.minimum_contrast);
    let size = (
        width,
        u32::from(rows) * u32::from(cell_h) + u32::from(inset.foot),
    );
    stage.snapshot_with(
        session,
        cols,
        scale,
        size,
        size,
        StageLook {
            caret_style: settings.caret,
            caret_focused: face.focused,
        },
    )
}

/// The cursor rows' options, each drawn as the renderer draws its caret.
const CARET_PICTURES: [Picture; 5] = [
    Picture::Block,
    Picture::Underline,
    Picture::Beam,
    Picture::Hollow,
    Picture::Solid,
];

/// The caret an option shows: its shape, its style and whether its window is key. The shapes are
/// shown in a key window with the file's corner and glow; the two looks of an inactive window are
/// shown on the block, the one shape that hollows (a hollow beam is the beam), so the pair is
/// never two of the same picture.
fn caret_of(picture: Picture, caret: CaretStyle) -> Option<(CaretShape, CaretStyle, bool)> {
    let unfocused = |unfocused| CaretStyle { unfocused, ..caret };
    Some(match picture {
        Picture::Block => (CaretShape::Block, caret, true),
        Picture::Underline => (CaretShape::Underline, caret, true),
        Picture::Beam => (CaretShape::Beam, caret, true),
        Picture::Hollow => (CaretShape::Block, unfocused(UnfocusedCaret::Hollow), false),
        Picture::Solid => (CaretShape::Block, unfocused(UnfocusedCaret::Solid), false),
        _ => return None,
    })
}

/// What a caret's still shows: `ls` typed and the caret after it.
const TYPED: &str = "ls ";
/// The columns the still is centred on: the typed text and the caret's cell.
const TYPED_COLS: u16 = 4;

/// Where the typed text starts in a caret's session and how big the session is: as many columns
/// beside it and rows above it as half a picture needs, so the still can be cut from round it
/// whatever the font's cell and the glow is drawn, not cut at a row's edge. The text is on the
/// last row: the renderer sets a screen's rows on its foot, and a blank row under the caret's
/// would not be drawn there but above.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Middle {
    /// The typed text's first column and its row.
    col: u16,
    row: u16,
    cols: u16,
    rows: u16,
}

impl Middle {
    fn of(picture: (u32, u32), cell: (u16, u16)) -> Self {
        let (cell_w, cell_h) = (u32::from(cell.0.max(1)), u32::from(cell.1.max(1)));
        let side = u16::try_from(picture.0.div_ceil(2).div_ceil(cell_w)).unwrap_or(u16::MAX / 4);
        let above = u16::try_from(picture.1.div_ceil(2).div_ceil(cell_h)).unwrap_or(u16::MAX / 4);
        Middle {
            col: side,
            row: above,
            cols: side + TYPED_COLS + side,
            rows: above + 1,
        }
    }

    /// The bytes that write the typed text there; the caret ends after it.
    fn screen(self) -> Vec<u8> {
        format!("\x1b[2J\x1b[{};{}H{TYPED}", self.row + 1, self.col + 1).into_bytes()
    }

    /// The middle of the typed text and its caret, in pixels from the session's top-left corner.
    fn centre(self, cell: (u16, u16), left: u16) -> (u32, u32) {
        let (cell_w, cell_h) = (u32::from(cell.0), u32::from(cell.1));
        (
            u32::from(left) + u32::from(self.col) * cell_w + u32::from(TYPED_COLS) * cell_w / 2,
            u32::from(self.row) * cell_h + cell_h / 2,
        )
    }
}

/// `size` pixels of `still` round `centre`; what falls outside it (under the screen's foot, or
/// beside a font too big for a picture) is the still's ground, its top-left pixel.
fn cut(still: &Snapshot, centre: (u32, u32), size: (u32, u32)) -> Option<Snapshot> {
    let ground = still.rgba.get(..4)?;
    let mut rgba = ground.repeat(size.0 as usize * size.1 as usize);
    let left = i64::from(centre.0) - i64::from(size.0 / 2);
    let top = i64::from(centre.1) - i64::from(size.1 / 2);
    // The columns of the cut that are inside the still, and where they are in it.
    let from = left.max(0);
    let to = (left + i64::from(size.0)).min(i64::from(still.width));
    if from < to {
        let (width, offset) = ((to - from) as usize * 4, (from - left) as usize * 4);
        for line in 0..i64::from(size.1) {
            let y = top + line;
            if y < 0 || y >= i64::from(still.height) {
                continue;
            }
            let source = (y as usize * still.width as usize + from as usize) * 4;
            let target = line as usize * size.0 as usize * 4 + offset;
            rgba.get_mut(target..target + width)?
                .copy_from_slice(still.rgba.get(source..source + width)?);
        }
    }
    Some(Snapshot {
        width: size.0,
        height: size.1,
        rgba,
    })
}

/// The still of `picture`'s caret in `theme`, a picture's size at `scale`; `None` for a picture
/// that is not a caret's, or when the session or the GPU said no. The session is kept in `kept`.
fn render_caret(
    stage: &mut Stage,
    kept: &mut Option<Kept>,
    picture: Picture,
    theme: Theme,
    settings: &Settings,
    scale: f64,
) -> Option<Snapshot> {
    let (shape, caret_style, focused) = caret_of(picture, settings.caret)?;
    let metrics = stage.cell_metrics(scale);
    let cell = metrics.cell_px();
    let inset = metrics.grid_inset();
    let size = (
        (PICTURE_WIDTH * scale).round() as u32,
        (PICTURE_HEIGHT * scale).round() as u32,
    );
    let middle = Middle::of(size, cell);
    let key = (theme, shape, middle.cols, middle.rows, cell);
    if kept.as_ref().map(|kept| kept.key) != Some(key) {
        let session = bt_core::pictured(
            SessionOptions {
                command: None,
                working_directory: None,
                home: None,
                env: HashMap::new(),
                cols: middle.cols,
                rows: middle.rows,
                cell_px: cell,
                terminal: Settings {
                    cursor: shape,
                    ..settings.clone()
                }
                .terminal(),
                theme,
                dock: false,
                cluster: true,
                initial_input: None,
                shell_marks: false,
                pane_uuid: None,
                hostname: None,
                replay: None,
                views: None,
            },
            &middle.screen(),
        )
        .ok()?;
        *kept = Some(Kept { key, session });
    }
    let session = &kept.as_ref()?.session;
    let whole = (
        u32::from(inset.left) + u32::from(middle.cols) * u32::from(cell.0) + u32::from(inset.right),
        u32::from(middle.rows) * u32::from(cell.1) + u32::from(inset.foot),
    );
    let still = stage.snapshot_with(
        session,
        middle.cols,
        scale,
        whole,
        whole,
        StageLook {
            caret_style,
            caret_focused: focused,
        },
    )?;
    cut(&still, middle.centre(cell, inset.left), size)
}

/// Everything a caret's still depends on.
#[derive(Clone, Debug, PartialEq)]
struct CaretDrawn {
    theme: Theme,
    font: FontOptions,
    caret: CaretStyle,
    scale: f64,
}

/// One caret option's still and its session, kept between draws.
struct CaretTile {
    picture: Picture,
    kept: RefCell<Option<Kept>>,
    drawn: RefCell<Option<(CaretDrawn, Retained<NSImage>)>>,
}

/// One card of a terminal example: the picture, and its name and tag under it.
struct Card {
    column: Retained<NSStackView>,
    image: Retained<NSImageView>,
    line: Retained<NSStackView>,
    caption: Retained<NSTextField>,
    tag: Retained<NSTextField>,
    kept: RefCell<Option<Kept>>,
    drawn: RefCell<Option<Drawn>>,
}

impl Card {
    fn new(mtm: MainThreadMarker) -> Self {
        let image = NSImageView::new(mtm);
        image.setImageScaling(NSImageScaling::ScaleProportionallyDown);
        image.setWantsLayer(true);
        if let Some(layer) = image.layer() {
            layer.setCornerRadius(CORNER);
            layer.setMasksToBounds(true);
        }
        let label = |mtm: MainThreadMarker| {
            let label = NSTextField::labelWithString(&NSString::from_str(""), mtm);
            label.setFont(Some(&NSFont::systemFontOfSize(11.0)));
            label.setTextColor(Some(&NSColor::secondaryLabelColor()));
            label
        };
        let (caption, tag) = (label(mtm), label(mtm));
        let line = super::stack(
            mtm,
            &[&caption, &tag],
            NSUserInterfaceLayoutOrientation::Horizontal,
            8.0,
            NSLayoutAttribute::FirstBaseline,
        );
        line.setDistribution(NSStackViewDistribution::EqualSpacing);
        let column = super::stack(
            mtm,
            &[&image, &line],
            NSUserInterfaceLayoutOrientation::Vertical,
            6.0,
            NSLayoutAttribute::Leading,
        );
        for view in [&image as &NSView, &line] {
            view.widthAnchor()
                .constraintEqualToAnchor(&column.widthAnchor())
                .setActive(true);
        }
        Card {
            column,
            image,
            line,
            caption,
            tag,
            kept: RefCell::new(None),
            drawn: RefCell::new(None),
        }
    }

    fn name(&self, face: &Face) {
        self.line.setHidden(face.caption.is_none());
        self.caption
            .setStringValue(&NSString::from_str(face.caption.as_deref().unwrap_or("")));
        self.tag
            .setStringValue(&NSString::from_str(face.tag.unwrap_or("")));
        self.tag.setHidden(face.tag.is_none());
    }
}

enum Body {
    Cards(Vec<Card>),
    Tabs(Retained<TabsView>),
}

/// One page's example: the view the page carries and what draws into it.
struct Specimen {
    kind: Kind,
    view: Retained<NSView>,
    body: Body,
}

/// The examples of all the pages, and the renderer they share.
pub(super) struct Specimens {
    all: Vec<Specimen>,
    carets: Vec<CaretTile>,
    /// Opened with the first example drawn; dropped when the window closes.
    stage: RefCell<Option<Stage>>,
    /// The GPU said no: the terminal examples stay hidden.
    no_gpu: Cell<bool>,
}

impl Specimens {
    pub(super) fn new(mtm: MainThreadMarker) -> Self {
        let all = Kind::ALL
            .iter()
            .map(|kind| match kind.cards() {
                0 => {
                    let tabs = TabsView::new(mtm);
                    tabs.heightAnchor()
                        .constraintEqualToConstant(tabs_specimen::HEIGHT)
                        .setActive(true);
                    Specimen {
                        kind: *kind,
                        view: Retained::into_super(tabs.clone()),
                        body: Body::Tabs(tabs),
                    }
                }
                count => {
                    let cards: Vec<Card> = (0..count).map(|_| Card::new(mtm)).collect();
                    let views: Vec<&NSView> =
                        cards.iter().map(|card| &*card.column as &NSView).collect();
                    let row = super::stack(
                        mtm,
                        &views,
                        NSUserInterfaceLayoutOrientation::Horizontal,
                        GAP,
                        NSLayoutAttribute::Top,
                    );
                    row.setDistribution(NSStackViewDistribution::FillEqually);
                    Specimen {
                        kind: *kind,
                        view: Retained::into_super(row),
                        body: Body::Cards(cards),
                    }
                }
            })
            .collect();
        Specimens {
            all,
            carets: CARET_PICTURES
                .iter()
                .map(|picture| CaretTile {
                    picture: *picture,
                    kept: RefCell::new(None),
                    drawn: RefCell::new(None),
                })
                .collect(),
            stage: RefCell::new(None),
            no_gpu: Cell::new(false),
        }
    }

    /// The view `kind`'s page carries.
    pub(super) fn view(&self, kind: Kind) -> Option<&NSView> {
        self.all
            .iter()
            .find(|specimen| specimen.kind == kind)
            .map(|specimen| &*specimen.view)
    }

    /// Draws `kind`'s example from `inputs` at `scale` pixels to the point.
    pub(super) fn show(&self, kind: Kind, inputs: &Inputs, scale: f64) {
        let Some(specimen) = self.all.iter().find(|specimen| specimen.kind == kind) else {
            return;
        };
        match &specimen.body {
            Body::Tabs(view) => {
                let settings = &inputs.settings;
                view.set_look(tabs_specimen::Look {
                    palette: Palette::of(&inputs.on_screen),
                    position: settings.tab_position,
                    icons: settings.tab_icons,
                    brand: settings.tab_icon_colors == bt_core::IconColors::Brand,
                    running: settings.tab_running,
                    details: settings.tab_details == bt_core::TabDetails::Always
                        && settings.tab_position == bt_core::TabPosition::Side,
                    split: settings.split_style,
                    dim_others: settings.dim_unfocused_splits,
                    focus_frame: settings.focus_frame,
                    reduce_motion: inputs.reduce_motion,
                });
            }
            Body::Cards(cards) => {
                let faces = faces(kind, inputs);
                let mut stage = self.stage(inputs);
                let Some(stage) = stage.as_mut() else {
                    specimen.view.setHidden(true);
                    return;
                };
                specimen.view.setHidden(faces.is_empty());
                let width_pt = if faces.len() > 1 {
                    (WIDTH - GAP) / 2.0
                } else {
                    WIDTH
                };
                for (i, card) in cards.iter().enumerate() {
                    let Some(face) = faces.get(i) else {
                        card.column.setHidden(true);
                        continue;
                    };
                    card.column.setHidden(false);
                    card.name(face);
                    paint(stage, card, kind, face, &inputs.settings, width_pt, scale);
                }
            }
        }
    }

    /// The renderer, opened on the first draw and set to the file's font; `None` inside when the
    /// GPU said no.
    fn stage(&self, inputs: &Inputs) -> std::cell::RefMut<'_, Option<Stage>> {
        let mut stage = self.stage.borrow_mut();
        if stage.is_none() && !self.no_gpu.get() {
            *stage = Stage::new(&inputs.settings.font).ok();
            self.no_gpu.set(stage.is_none());
        }
        if let Some(stage) = stage.as_mut() {
            stage.set_font(&inputs.settings.font);
        }
        stage
    }

    /// The cursor rows' options as the renderer draws the caret, in the theme on screen, at
    /// `scale` pixels to the point; empty with no GPU, and an option whose still could not be made
    /// is left out (the row draws its own picture for it). A still is made again only when what it
    /// shows changed.
    pub(super) fn caret_stills(
        &self,
        inputs: &Inputs,
        scale: f64,
    ) -> Vec<(Picture, Retained<NSImage>)> {
        let mut stage = self.stage(inputs);
        let Some(stage) = stage.as_mut() else {
            return Vec::new();
        };
        let drawn = CaretDrawn {
            theme: inputs.on_screen,
            font: inputs.settings.font.clone(),
            caret: inputs.settings.caret,
            scale,
        };
        let mut stills = Vec::new();
        for tile in &self.carets {
            if let Some((was, image)) = tile.drawn.borrow().as_ref()
                && *was == drawn
            {
                stills.push((tile.picture, image.clone()));
                continue;
            }
            let Some(still) = render_caret(
                stage,
                &mut tile.kept.borrow_mut(),
                tile.picture,
                inputs.on_screen,
                &inputs.settings,
                scale,
            ) else {
                continue;
            };
            let Some(image) = snapshot::image(&still) else {
                continue;
            };
            let image = NSImage::initWithCGImage_size(
                NSImage::alloc(),
                &image,
                NSSize::new(PICTURE_WIDTH, PICTURE_HEIGHT),
            );
            tile.drawn.replace(Some((drawn.clone(), image.clone())));
            stills.push((tile.picture, image));
        }
        stills
    }

    /// Gives the renderer's textures and the sessions back (the window closed); the next draw
    /// makes them again, and tries for a renderer again if the last try found none.
    pub(super) fn release(&self) {
        self.stage.replace(None);
        self.no_gpu.set(false);
        for tile in &self.carets {
            tile.kept.replace(None);
            tile.drawn.replace(None);
        }
        for specimen in &self.all {
            if let Body::Cards(cards) = &specimen.body {
                for card in cards {
                    card.kept.replace(None);
                    card.drawn.replace(None);
                }
            }
        }
    }
}

/// Draws `face` into `card` unless the picture on it is already of this.
fn paint(
    stage: &mut Stage,
    card: &Card,
    kind: Kind,
    face: &Face,
    settings: &Settings,
    width_pt: f64,
    scale: f64,
) {
    let drawn = Drawn {
        kind,
        face: face.clone(),
        font: settings.font.clone(),
        shape: settings.cursor,
        caret: settings.caret,
        contrast: settings.minimum_contrast,
        scale,
        width: (width_pt * scale).round() as u32,
    };
    if card.drawn.borrow().as_ref() == Some(&drawn) {
        return;
    }
    let Some(still) = render(
        stage,
        &mut card.kept.borrow_mut(),
        kind,
        face,
        settings,
        width_pt,
        scale,
    ) else {
        return;
    };
    let Some(image) = snapshot::image(&still) else {
        return;
    };
    let size = NSSize::new(
        f64::from(still.width) / scale,
        f64::from(still.height) / scale,
    );
    let image = NSImage::initWithCGImage_size(NSImage::alloc(), &image, size);
    card.image.setImage(Some(&image));
    card.drawn.replace(Some(drawn));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(settings: Settings) -> Inputs {
        let [_, _, light] = Theme::embedded_groups();
        Inputs {
            settings,
            light: Theme::embedded(light[0]).expect("a light theme"),
            dark: Theme::BATERI,
            on_screen: Theme::BATERI,
            dark_appearance: true,
            reduce_motion: false,
        }
    }

    /// The system's theme is a pair of cards, the one the system is in tagged; a theme of its own
    /// is one card.
    #[test]
    fn the_theme_is_a_pair_when_it_follows_the_system_and_a_card_when_it_does_not() {
        let mut settings = Settings::default();
        assert!(settings.follows_system());
        let pair = faces(Kind::Appearance, &inputs(settings.clone()));
        assert_eq!(pair.len(), 2);
        assert_eq!((pair[0].tag, pair[1].tag), (None, Some("In use now")));
        assert_ne!(pair[0].theme, pair[1].theme);

        settings.theme = "bateri".to_owned();
        let one = faces(Kind::Appearance, &inputs(settings));
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].tag, None);
    }

    /// The caret is shown in this window and in an inactive one, and nowhere else.
    #[test]
    fn the_cursor_is_shown_focused_and_not() {
        let cards = faces(Kind::Cursor, &inputs(Settings::default()));
        assert_eq!(
            cards.iter().map(|card| card.focused).collect::<Vec<_>>(),
            [true, false]
        );
        assert_eq!(faces(Kind::Font, &inputs(Settings::default())).len(), 1);
    }

    /// The plain prompt is the Shell example, and only without integration: with it the prompt is
    /// the dock, which is not drawn here.
    #[test]
    fn the_shell_example_is_the_prompt_without_integration() {
        let with = |shell_integration| {
            faces(
                Kind::Shell,
                &inputs(Settings {
                    shell_integration,
                    ..Settings::default()
                }),
            )
        };
        assert!(with(bt_core::ShellIntegration::Auto).is_empty());
        assert_eq!(with(bt_core::ShellIntegration::Off).len(), 1);
    }

    /// A screen is written from the second row, a long line wraps, and the caret is hidden where
    /// the example is about text.
    #[test]
    fn a_screen_wraps_and_keeps_its_first_row_blank() {
        let theme = Theme::BATERI;
        let font = lines(Kind::Font, &theme);
        let bytes = String::from_utf8(screen(Kind::Font, &font)).expect("text");
        assert!(bytes.starts_with("\x1b[2J\x1b[2;1H\x1b[?25l"));
        let wide = rows_of(&font, 200);
        let narrow = rows_of(&font, 40);
        assert_eq!(usize::from(wide), 1 + font.len());
        assert!(narrow > wide, "a narrower terminal wraps the long line");
        let cursor =
            String::from_utf8(screen(Kind::Cursor, &lines(Kind::Cursor, &theme))).expect("text");
        // The caret goes back to the end of the typed command: "❯ cargo test" is 12 columns.
        assert!(cursor.ends_with("\x1b[2;13H"), "{cursor:?}");
        assert!(!cursor.contains("?25l"));
    }

    /// Each example has what it exists to show: the faint line, the secret and the
    /// look-alike letters, the dock's neighbours.
    #[test]
    fn every_example_carries_what_it_shows() {
        let theme = Theme::BATERI;
        let text = |kind| {
            lines(kind, &theme)
                .iter()
                .map(|line| line.bytes.clone())
                .collect::<String>()
        };
        assert!(text(Kind::Appearance).contains("faint text"));
        assert!(text(Kind::Font).contains("0O1lI"));
        assert!(text(Kind::Font).contains("sk-proj-"));
        assert!(text(Kind::Cursor).contains("cargo test"));
        assert!(text(Kind::Shell).contains("npm run dev"));
        // The faint line is a step off the ground, between it and the text.
        let faint = mix(theme.background, theme.foreground, 0.16);
        assert_ne!(faint, theme.background);
        assert_ne!(faint, theme.foreground);
    }

    /// Each caret option is the renderer's caret: the three shapes are three pictures, the two
    /// looks of an inactive window two, and every still stands on the theme's background with
    /// the caret in its middle.
    #[test]
    fn the_caret_options_are_the_renderers_caret() {
        let mut stage = Stage::new(&FontOptions::default()).expect("a stage");
        let settings = Settings::default();
        let theme = Theme::BATERI;
        let mut seen = Vec::new();
        for picture in CARET_PICTURES {
            let mut kept = None;
            let still = render_caret(&mut stage, &mut kept, picture, theme, &settings, 2.0)
                .expect("a still");
            assert_eq!((still.width, still.height), (140, 92));
            let want = theme.background;
            let got = corner(&still);
            for (i, shift) in [16, 8, 0].into_iter().enumerate() {
                assert!(
                    got[i].abs_diff((want >> shift) as u8) <= 2,
                    "{picture:?}: the ground is {got:?}, the theme's background is {want:06x}"
                );
            }
            let cursor = theme.cursor;
            let near = |px: &[u8]| {
                [16, 8, 0]
                    .iter()
                    .zip(px)
                    .all(|(shift, c)| c.abs_diff((cursor >> shift) as u8) <= 24)
            };
            assert!(
                still.rgba.chunks(4).any(near),
                "{picture:?}: no pixel is the caret's colour"
            );
            seen.push(still.rgba);
        }
        // The three shapes are three pictures; an inactive window's two looks are two (a solid
        // one is the key window's block, which is what Solid means).
        let differ = |i: usize, j: usize| {
            assert!(
                seen[i] != seen[j],
                "{:?} and {:?} are the same picture",
                CARET_PICTURES[i],
                CARET_PICTURES[j]
            );
        };
        differ(0, 1);
        differ(0, 2);
        differ(1, 2);
        differ(3, 4);
        assert!(
            seen[0] == seen[4],
            "a solid caret is the key window's block"
        );
    }

    fn corner(still: &Snapshot) -> [u8; 3] {
        let at = still.rgba.len() - 4;
        [still.rgba[at], still.rgba[at + 1], still.rgba[at + 2]]
    }

    /// The card is the terminal drawn with the theme: its ground is the theme's background, so a
    /// light and a dark theme are two grounds; and the focus of the window is the caret's.
    #[test]
    fn a_card_is_drawn_with_the_theme_and_the_windows_focus() {
        let mut stage = Stage::new(&FontOptions::default()).expect("a stage");
        let settings = Settings::default();
        let inputs = inputs(settings.clone());
        let faces = faces(Kind::Appearance, &inputs);
        let mut grounds = Vec::new();
        for face in &faces {
            let mut kept = None;
            let still = render(
                &mut stage,
                &mut kept,
                Kind::Appearance,
                face,
                &settings,
                240.0,
                1.0,
            )
            .expect("a picture");
            let want = face.theme.background;
            let got = corner(&still);
            for (i, shift) in [16, 8, 0].into_iter().enumerate() {
                assert!(
                    got[i].abs_diff((want >> shift) as u8) <= 2,
                    "the ground is {got:?}, the theme's background is {want:06x}"
                );
            }
            grounds.push(got);
        }
        assert_ne!(grounds[0], grounds[1]);

        let cursor = super::faces(Kind::Cursor, &inputs);
        let mut seen = Vec::new();
        for face in &cursor {
            let mut kept = None;
            seen.push(
                render(
                    &mut stage,
                    &mut kept,
                    Kind::Cursor,
                    face,
                    &settings,
                    240.0,
                    1.0,
                )
                .expect("a picture"),
            );
        }
        assert_ne!(
            seen[0].rgba, seen[1].rgba,
            "the caret is the same in the window that is key and the one that is not"
        );
    }
}
