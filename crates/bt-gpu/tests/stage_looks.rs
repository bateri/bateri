//! What the settings change in a picture the stage draws: a theme, a font's own switch, the
//! caret's shape and focus, the floor under a program's text and the weight of the strokes — the
//! pixels, not the model. These are the guards under the settings window's live examples, which
//! show a setting by drawing the terminal with it.
//!
//! A binary of its own on purpose: the app's fonts are registered for the whole process (the
//! stylistic-set guard needs a family that has one on every machine), and the lib tests must
//! never see them.
#![cfg(target_os = "macos")]

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use bt_core::{
    CaretShape, CaretStyle, CursorBlink, FontOptions, Osc52, Session, SessionOptions,
    TerminalOptions, Theme, UnfocusedCaret,
};
use bt_gpu::{Snapshot, Stage, StageLook, register_app_fonts};

const COLS: u16 = 24;
const ROWS: u16 = 3;

fn fonts_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts")
}

fn font(family: &str) -> FontOptions {
    register_app_fonts(&fonts_dir());
    FontOptions {
        family: Some(family.to_owned()),
        size: 16.0,
        ..FontOptions::default()
    }
}

fn session(
    stage: &Stage,
    theme: Theme,
    cursor: CaretShape,
    contrast: f64,
    bytes: &[u8],
) -> Session {
    let session = bt_core::pictured(
        SessionOptions {
            command: None,
            working_directory: None,
            home: None,
            env: HashMap::new(),
            cols: COLS,
            rows: ROWS,
            cell_px: stage.cell_metrics(1.0).cell_px(),
            terminal: TerminalOptions {
                scrollback: 0,
                osc52: Osc52::Off,
                cursor,
                blink: CursorBlink::default(),
            },
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
        bytes,
    )
    .expect("a pictured session");
    session.set_minimum_contrast(contrast);
    session
}

fn picture(stage: &mut Stage, session: &Session, look: StageLook) -> Snapshot {
    let metrics = stage.cell_metrics(1.0);
    let (cell_w, cell_h) = metrics.cell_px();
    let inset = metrics.grid_inset();
    let size = (
        u32::from(inset.left) + u32::from(COLS) * u32::from(cell_w) + u32::from(inset.right),
        u32::from(ROWS) * u32::from(cell_h) + u32::from(inset.foot),
    );
    stage
        .snapshot_with(session, COLS, 1.0, size, size, look)
        .expect("a picture")
}

/// How much of the picture is not `ground`: the sum of every pixel's largest channel distance.
fn ink(still: &Snapshot, ground: Theme) -> u64 {
    let bg = [
        (ground.background >> 16) as u8,
        (ground.background >> 8) as u8,
        ground.background as u8,
    ];
    still
        .rgba
        .chunks_exact(4)
        .map(|px| {
            (0..3)
                .map(|c| u64::from(px[c].abs_diff(bg[c])))
                .max()
                .unwrap_or(0)
        })
        .sum()
}

fn plain(text: &str) -> Vec<u8> {
    format!("\x1b[2J\x1b[H{text}").into_bytes()
}

#[test]
fn a_stylistic_set_the_font_has_changes_its_letters() {
    let mut options = font("Fira Code");
    let mut stage = Stage::new(&options).expect("a stage");
    let theme = Theme::BATERI;
    let bytes = plain("rrrrrrrrrrrr\r\nrrrrrrrrrrrr");
    let session = session(&stage, theme, CaretShape::Block, 1.0, &bytes);
    let off = picture(&mut stage, &session, StageLook::default());
    options.features.insert(
        "fira code".to_owned(),
        BTreeMap::from([("ss01".to_owned(), true)]),
    );
    stage.set_font(&options);
    let on = picture(&mut stage, &session, StageLook::default());
    assert_eq!(off.rgba.len(), on.rgba.len());
    assert_ne!(off.rgba, on.rgba, "ss01 left the letters as they were");
    // And back: the choice is the picture's, not the stage's memory.
    options.features.clear();
    stage.set_font(&options);
    assert_eq!(picture(&mut stage, &session, StageLook::default()), off);
}

#[test]
fn a_theme_is_the_ground_the_picture_stands_on() {
    let mut stage = Stage::new(&font("JetBrains Mono")).expect("a stage");
    let [_, _, light] = Theme::embedded_groups();
    let light = Theme::embedded(light[0]).expect("an embedded light theme");
    assert!(!light.is_dark() && Theme::BATERI.is_dark());
    let bytes = plain("hello");
    let mut corners = Vec::new();
    for theme in [Theme::BATERI, light] {
        let session = session(&stage, theme, CaretShape::Block, 1.0, &bytes);
        let still = picture(&mut stage, &session, StageLook::default());
        // The bottom right: no text, no caret, only what the theme paints under everything.
        let at = still.rgba.len() - 4;
        let px = &still.rgba[at..at + 3];
        let want = [
            (theme.background >> 16) as u8,
            (theme.background >> 8) as u8,
            theme.background as u8,
        ];
        for c in 0..3 {
            assert!(
                px[c].abs_diff(want[c]) <= 2,
                "the ground is {px:?}, the theme's background is {want:?}"
            );
        }
        corners.push(px.to_vec());
    }
    assert_ne!(corners[0], corners[1]);
}

#[test]
fn the_caret_shape_and_its_focus_are_in_the_picture() {
    let mut stage = Stage::new(&font("JetBrains Mono")).expect("a stage");
    let theme = Theme::BATERI;
    let bytes = plain("$ ");
    let key = StageLook {
        caret_style: CaretStyle::default(),
        caret_focused: true,
    };
    let mut seen = Vec::new();
    for shape in [CaretShape::Block, CaretShape::Underline, CaretShape::Beam] {
        let session = session(&stage, theme, shape, 1.0, &bytes);
        seen.push(picture(&mut stage, &session, key));
    }
    assert_ne!(seen[0], seen[1], "a block and an underline are one picture");
    assert_ne!(seen[0], seen[2], "a block and a beam are one picture");
    assert_ne!(seen[1], seen[2], "an underline and a beam are one picture");
    // A block in a window that is not key is hollow: less of it is lit than of the solid one.
    let session = session(&stage, theme, CaretShape::Block, 1.0, &bytes);
    let hollow = picture(
        &mut stage,
        &session,
        StageLook {
            caret_style: CaretStyle {
                unfocused: UnfocusedCaret::Hollow,
                ..CaretStyle::default()
            },
            caret_focused: false,
        },
    );
    assert!(
        ink(&hollow, theme) < ink(&seen[0], theme),
        "the inactive block is not hollow"
    );
    // The glow is a halo around the caret: more of the picture is lit with it.
    let glowing = picture(
        &mut stage,
        &session,
        StageLook {
            caret_style: CaretStyle {
                glow: 1.0,
                ..CaretStyle::default()
            },
            caret_focused: true,
        },
    );
    let dark = picture(
        &mut stage,
        &session,
        StageLook {
            caret_style: CaretStyle {
                glow: 0.0,
                ..CaretStyle::default()
            },
            caret_focused: true,
        },
    );
    assert!(
        ink(&glowing, theme) > ink(&dark, theme),
        "the glow lit nothing around the caret"
    );
}

#[test]
fn bolder_text_has_more_ink() {
    let mut options = font("JetBrains Mono");
    let theme = Theme::BATERI;
    let bytes = plain("Hamburgefonstiv\r\nHamburgefonstiv");
    let mut stage = Stage::new(&options).expect("a stage");
    let session = session(&stage, theme, CaretShape::Block, 1.0, &bytes);
    let thin = ink(&picture(&mut stage, &session, StageLook::default()), theme);
    options.smoothing = true;
    stage.set_font(&options);
    let bold = ink(&picture(&mut stage, &session, StageLook::default()), theme);
    assert!(
        bold > thin,
        "smoothing drew {bold} of ink, not more than {thin}"
    );
}

#[test]
fn a_minimum_contrast_opens_a_faint_line() {
    let mut stage = Stage::new(&font("JetBrains Mono")).expect("a stage");
    let theme = Theme::BATERI;
    // A program's text one step off the ground: what the floor exists for.
    let faint = theme.background.wrapping_add(0x0b0b0b);
    let bytes = plain(&format!(
        "\x1b[38;2;{};{};{}mhint hint hint hint",
        (faint >> 16) & 0xff,
        (faint >> 8) & 0xff,
        faint & 0xff
    ));
    let low = {
        let session = session(&stage, theme, CaretShape::Block, 1.0, &bytes);
        ink(&picture(&mut stage, &session, StageLook::default()), theme)
    };
    let high = {
        let session = session(&stage, theme, CaretShape::Block, 7.0, &bytes);
        ink(&picture(&mut stage, &session, StageLook::default()), theme)
    };
    assert!(high > low * 2, "the floor lifted {low} of ink to {high}");
}
