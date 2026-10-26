//! The UI kit's sample page — a debug build's way to look at what `bt-gpu`'s
//! boxes and proportional text draw, in the pane's own theme and next to the
//! grid it covers: corner radii, borders, soft shadows, per-corner radii,
//! translucent fills, and text at several sizes and weights in the system
//! face and in the terminal's.
//!
//! Debug builds only (`Debug ▸ Show UI Sample Page`, closed by Esc or ⌃C): it
//! is a gauge for the kit, not a feature. Everything here is a [`UiPage`];
//! the pane hands it to its link, which asks for the list whenever the
//! surface needs a frame.

use bt_gpu::{TextFamily, TextStyle, UiContext, UiList, UiPage, UiQuad, UiRect, tint};

/// The page. Holds only what the pane knows and the context does not: the
/// terminal font's family and point size, to set text in the grid's own face.
pub(crate) struct SamplePage {
    family: Option<String>,
    size_pt: f64,
}

impl SamplePage {
    pub(crate) fn new(family: Option<String>, size_pt: f64) -> Self {
        Self { family, size_pt }
    }
}

/// A cursor over the page being laid out: the list, the context and the
/// point-to-pixel scale in one place, so the page reads in points.
struct Sheet<'a, 'b> {
    cx: &'a UiContext<'b>,
    list: UiList,
    scale: f32,
}

impl Sheet<'_, '_> {
    /// Points to whole pixels.
    fn pt(&self, points: f32) -> f32 {
        (points * self.scale).round()
    }

    fn quad(&mut self, quad: UiQuad) {
        self.list.quad(quad);
    }

    /// A box given in points.
    fn rect(&self, x: f32, y: f32, w: f32, h: f32) -> UiRect {
        UiRect::new(self.pt(x), self.pt(y), self.pt(w), self.pt(h))
    }

    /// One line of text at `(x, y)` points (the line box's top-left); returns
    /// its width and height in points. Nothing is drawn where the platform has
    /// no proportional text, and the size is still reported as zero.
    fn text(
        &mut self,
        family: &TextFamily,
        size_pt: f32,
        weight: u16,
        text: &str,
        at: (f32, f32),
        color: [f32; 4],
    ) -> (f32, f32) {
        let style = TextStyle {
            family: family.clone(),
            size_px: f64::from(size_pt) * f64::from(self.scale),
            weight,
            italic: false,
        };
        let Some(font) = self.cx.renderer.ui_font(&style) else {
            return (0.0, 0.0);
        };
        let Some(line) = self.cx.renderer.ui_shape(font, text) else {
            return (0.0, 0.0);
        };
        let origin = [self.pt(at.0), self.pt(at.1)];
        self.cx
            .renderer
            .ui_text(&mut self.list, &line, origin, color, UiRect::UNBOUNDED);
        (
            line.width as f32 / self.scale,
            line.height() as f32 / self.scale,
        )
    }
}

impl UiPage for SamplePage {
    fn build(&mut self, cx: &UiContext<'_>) -> UiList {
        let theme = cx.theme;
        let system = TextFamily::System;
        let terminal = TextFamily::Terminal(self.family.clone());
        let mut sheet = Sheet {
            cx,
            list: UiList::new(),
            scale: cx.scale as f32,
        };

        let foreground = tint(theme.foreground_linear(), 1.0);
        let dim = tint(theme.dim_linear(), 1.0);
        let card = tint(theme.band_linear(), 1.0);
        let line = tint(theme.separator_linear(), 1.0);
        let accent = theme.accent_linear();
        let ground = theme.background_linear();
        let shadow = tint(theme.dock_shadow_linear(), 0.55);

        let margin = 28.0;
        let mut y = margin;

        let (_, h) = sheet.text(&system, 26.0, 700, "bateri UI kit", (margin, y), foreground);
        y += h + 2.0;
        let (_, h) = sheet.text(
            &system,
            13.0,
            400,
            "Boxes and proportional text, drawn by the renderer in the pane's theme. Esc or \u{2303}C closes.",
            (margin, y),
            dim,
        );
        y += h + 24.0;

        // --- Corner radius ----------------------------------------------
        let (_, h) = sheet.text(&system, 11.0, 600, "CORNER RADIUS", (margin, y), dim);
        y += h + 8.0;
        let side = 64.0;
        let mut x = margin;
        for radius in [0.0_f32, 4.0, 8.0, 16.0, 28.0, 32.0] {
            let rect = sheet.rect(x, y, side, side);
            sheet.quad(
                UiQuad::new(rect)
                    .fill(card)
                    .border(sheet.pt(1.0), line)
                    .radius(sheet.pt(radius)),
            );
            sheet.text(
                &terminal,
                11.0,
                400,
                &format!("{radius}"),
                (x, y + side + 4.0),
                dim,
            );
            x += side + 18.0;
        }
        y += side + 4.0 + 18.0 + 22.0;

        // --- Border and shadow ------------------------------------------
        let (_, h) = sheet.text(&system, 11.0, 600, "BORDER AND SHADOW", (margin, y), dim);
        y += h + 10.0;
        let (w, h) = (164.0, 88.0);
        // A shadow needs something lighter than the ground to fall on: a
        // translucent panel of the text colour stands behind the cards.
        let panel = sheet.rect(
            margin - 14.0,
            y - 8.0,
            3.0 * w + 2.0 * 20.0 + 28.0,
            h + 16.0,
        );
        sheet.quad(
            UiQuad::new(panel)
                .fill(tint(theme.foreground_linear(), 0.12))
                .radius(sheet.pt(16.0)),
        );
        let cards: [(&str, bool, bool); 3] = [
            ("1 pt border", true, false),
            ("24 pt shadow", false, true),
            ("Both, accent edge", true, true),
        ];
        let mut x = margin;
        for (label, bordered, shadowed) in cards {
            let rect = sheet.rect(x, y, w, h);
            let mut quad = UiQuad::new(rect).fill(card).radius(sheet.pt(12.0));
            if bordered {
                let edge = if shadowed { tint(accent, 1.0) } else { line };
                let width = if shadowed { 2.0 } else { 1.0 };
                quad = quad.border(sheet.pt(width), edge);
            }
            if shadowed {
                quad = quad.shadow(0.0, sheet.pt(8.0), sheet.pt(24.0), shadow);
            }
            sheet.quad(quad);
            sheet.text(&system, 13.0, 500, label, (x + 14.0, y + 14.0), foreground);
            sheet.text(
                &terminal,
                11.0,
                400,
                "radius 12",
                (x + 14.0, y + h - 28.0),
                dim,
            );
            x += w + 20.0;
        }
        y += h + 44.0;

        // --- Per-corner radii and translucent fills ---------------------
        let (_, h) = sheet.text(&system, 11.0, 600, "SHAPES AND FILLS", (margin, y), dim);
        y += h + 10.0;
        let mut x = margin;
        // A tab: round on top only.
        let rect = sheet.rect(x, y, 108.0, 36.0);
        sheet.quad(
            UiQuad::new(rect)
                .fill(card)
                .radii([sheet.pt(12.0), sheet.pt(12.0), 0.0, 0.0]),
        );
        sheet.text(&system, 13.0, 500, "Tab", (x + 14.0, y + 9.0), foreground);
        x += 108.0 + 16.0;
        // A pill button: the accent, with the ground's colour on it.
        let rect = sheet.rect(x, y, 108.0, 36.0);
        sheet.quad(
            UiQuad::new(rect)
                .fill(tint(accent, 1.0))
                .radius(sheet.pt(18.0)),
        );
        sheet.text(
            &system,
            13.0,
            600,
            "Run",
            (x + 40.0, y + 9.0),
            tint(ground, 1.0),
        );
        x += 108.0 + 16.0;
        // A chat bubble: three round corners and a small one.
        let rect = sheet.rect(x, y, 168.0, 36.0);
        sheet.quad(
            UiQuad::new(rect)
                .fill(tint(theme.info_linear(), 0.18))
                .border(sheet.pt(1.0), tint(theme.info_linear(), 0.7))
                .radii([
                    sheet.pt(16.0),
                    sheet.pt(16.0),
                    sheet.pt(16.0),
                    sheet.pt(3.0),
                ]),
        );
        sheet.text(
            &system,
            13.0,
            400,
            "Per-corner radii",
            (x + 14.0, y + 9.0),
            foreground,
        );
        y += 36.0 + 14.0;
        // Translucent status fills.
        let mut x = margin;
        for (label, color) in [
            ("success", theme.success_linear()),
            ("warning", theme.warning_linear()),
            ("error", theme.error_linear()),
        ] {
            let rect = sheet.rect(x, y, 108.0, 30.0);
            sheet.quad(
                UiQuad::new(rect)
                    .fill(tint(color, 0.16))
                    .border(sheet.pt(1.0), tint(color, 0.75))
                    .radius(sheet.pt(8.0)),
            );
            sheet.text(
                &system,
                12.0,
                500,
                label,
                (x + 14.0, y + 7.0),
                tint(color, 1.0),
            );
            x += 108.0 + 16.0;
        }
        y += 30.0 + 36.0;

        // --- Type ---------------------------------------------------------
        let (_, h) = sheet.text(&system, 11.0, 600, "TYPE", (margin, y), dim);
        y += h + 8.0;
        for weight in [300_u16, 400, 500, 600, 700, 800] {
            let (_, h) = sheet.text(
                &system,
                15.0,
                weight,
                &format!("{weight}  The quick brown fox jumps over the lazy dog 0123456789"),
                (margin, y),
                foreground,
            );
            y += h + 1.0;
        }
        y += 10.0;
        let mut x = margin;
        for size in [11.0_f32, 13.0, 15.0, 20.0, 28.0] {
            let (w, _) = sheet.text(
                &system,
                size,
                400,
                &format!("{size} pt"),
                (x, y + (28.0 - size) * 0.8),
                foreground,
            );
            x += w + 18.0;
        }
        y += 36.0;
        let size = self.size_pt as f32;
        let (_, h) = sheet.text(
            &terminal,
            size,
            400,
            "ls -la ~/Projects/bateri    # the grid's own family, set proportionally",
            (margin, y),
            foreground,
        );
        y += h + 4.0;
        let mut x = margin;
        for (word, color) in [
            ("accent", accent),
            ("success", theme.success_linear()),
            ("warning", theme.warning_linear()),
            ("error", theme.error_linear()),
            ("info", theme.info_linear()),
        ] {
            let (w, _) = sheet.text(&terminal, size, 700, word, (x, y), tint(color, 1.0));
            x += w + 16.0;
        }

        sheet.list
    }
}
