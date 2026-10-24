//! Color resolution: the single path between a cell's `Color` and the RGBA on
//! screen.
//!
//! [`Theme`] owns the palette: background, foreground, dim foreground, accent
//! (cursor and the running command block), two status colors (success, error)
//! and the 16 ANSI colors in a single value — the six of the eight-role model
//! consumed today. The renderer knows no palette: `frame()` hands out
//! resolved RGBA and the clear, cursor and block stripe colors come from the
//! same theme too. The theme file's parser is in the `theme` module.
//!
//! Colors are written as `0xRRGGBB` — a palette is written that way
//! everywhere and the `Rgb { r, g, b }` triple would make a sixteen-line
//! table unreadable.

use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};

use crate::settings::{HostMark, MINIMUM_CONTRAST};
use crate::shell::Stripe;

mod schemes;

/// Color in the draw target's space: **linear** RGBA.
///
/// A newtype, because only half of the symmetric error couldn't be
/// represented: on the `bt-gpu` side "a non-sRGB target" was closed off with a
/// `const` (`Renderer::PIXEL_FORMAT`), but on this side of the boundary the
/// color was a bare `[f32; 4]` and only a comment stated its space. A color
/// that writes an sRGB-encoded float (`c / 255.0`) there opens up — the
/// midtone `0x1a1c21` turns into the gray `0x5a5d65` — and the symptom is
/// silent. The field is private and its only constructor is
/// [`LinearRgba::from_srgb`], so the conversion is inside the type: the type
/// now carries the space, not a comment.
///
/// No `Eq`, but `PartialEq`: the components are `f32` and what gets compared
/// is always two values that came out of the same table, not two computed
/// values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearRgba([f32; 4]);

impl LinearRgba {
    /// From an sRGB-encoded 8-bit triple — the **only constructor**.
    ///
    /// The input is deliberately `u8`: colors are written as `0xRRGGBB`
    /// everywhere and the linearization stays inside the crate. If a
    /// constructor took linear floats the newtype would be just a name; the
    /// error it closes is "putting an sRGB float into the linear slot" and
    /// that error becomes unrepresentable only when the conversion is **here**.
    ///
    /// This constructor doesn't make the palette's sole ownership, that is a
    /// separate rule (the head of this module): being able to produce a color
    /// here is not the same as writing it into the palette.
    pub const fn from_srgb(r: u8, g: u8, b: u8) -> Self {
        Self([
            // audit: `u8 as usize` is 0..=255, the table has 256 entries — the
            // index is bounded by the type itself, the bounds check is
            // eliminated in codegen too.
            SRGB_LINEAR[r as usize],
            SRGB_LINEAR[g as usize],
            SRGB_LINEAR[b as usize],
            1.0,
        ])
    }

    /// The four components that go to the GPU.
    ///
    /// `const`: constants like `Theme::BATERI.background_linear()` must be
    /// unfoldable at compile time too (`bt-gpu`'s tests).
    pub const fn to_array(self) -> [f32; 4] {
        self.0
    }

    /// The colour sRGB-encoded again, `0..=1` a channel, alpha as it is: for a surface that
    /// takes sRGB (a native control drawn by the platform) and a tone the theme derives in
    /// linear space (a raised ground, the quiet text), which has no stored sRGB value to read.
    pub fn to_srgb(self) -> [f32; 4] {
        let encode = |linear: f32| {
            let linear = linear.clamp(0.0, 1.0);
            if linear <= 0.003_130_8 {
                linear * 12.92
            } else {
                1.055 * linear.powf(1.0 / 2.4) - 0.055
            }
        };
        let [r, g, b, a] = self.0;
        [encode(r), encode(g), encode(b), a]
    }

    /// Its relative luminance (WCAG 2), `0..=1` — [`luminance`]'s measure of a
    /// color already linear, so a drawing that weighs colors by how bright they
    /// look weighs them as the theme's choices are made.
    pub const fn luminance(self) -> f32 {
        let [r, g, b, _] = self.0;
        weigh(r as f64, g as f64, b as f64) as f32
    }

    /// This colour as **ink on `ground`**, moved just far enough that the two
    /// stand at least `minimum` apart ([`ratio`], WCAG 2); unchanged when they
    /// already do, or when `minimum` is `1` or less (the rule is off).
    ///
    /// Only the ink moves: the ground is what the program painted and other
    /// text may sit on it. The ink keeps its own side of the ground — dark
    /// text darkens, light text lightens — and crosses over only when its
    /// side cannot reach the ratio: white text on a light ground has no
    /// lighter to go to, so it darkens to the grey that reads. When neither
    /// side reaches (a mid-grey ground under a ratio above ~4.6) it takes the
    /// end that stands further.
    ///
    /// The arithmetic is **closed-form**, in the linear space the colour is
    /// already in: luminance is a weighted sum of the linear channels whose
    /// weights add up to one, so scaling every channel toward black scales the
    /// luminance by the same factor, and blending every channel toward white
    /// moves it toward one by the same factor. The hue stays (toward black) or
    /// pales (toward white); no search, no 8-bit rounding.
    ///
    /// Not a second constructor in the sense [`Self::from_srgb`] closes off:
    /// the input is already a linear colour and the result is derived from it
    /// inside the type, so no sRGB float can get in this way.
    pub(crate) fn legible_on(self, ground: LinearRgba, minimum: f64) -> LinearRgba {
        let [r, g, b, alpha] = self.0;
        let ink = weigh(r as f64, g as f64, b as f64);
        let [gr, gg, gb, _] = ground.0;
        let base = weigh(gr as f64, gg as f64, gb as f64);
        if minimum <= 1.0 || ratio(ink, base) >= minimum {
            return self;
        }
        // The luminance each side needs: [`ratio`] solved for the ink.
        let darker = (base + 0.05) / minimum - 0.05;
        let lighter = minimum * (base + 0.05) - 0.05;
        let down = match (darker >= 0.0, lighter <= 1.0) {
            (true, true) => ink <= base,
            (true, false) => true,
            (false, true) => false,
            (false, false) => ratio(0.0, base) >= ratio(1.0, base),
        };
        if down {
            let scale = if ink > 0.0 {
                (darker.max(0.0) / ink).min(1.0)
            } else {
                0.0
            };
            let channel = |c: f32| (c as f64 * scale) as f32;
            Self([channel(r), channel(g), channel(b), alpha])
        } else {
            let toward = if ink < 1.0 {
                ((lighter.min(1.0) - ink) / (1.0 - ink)).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let channel = |c: f32| (c as f64 + (1.0 - c as f64) * toward) as f32;
            Self([channel(r), channel(g), channel(b), alpha])
        }
    }
}

/// A color theme: the **single source**.
///
/// The window's clear color ([`Theme::background_linear`]), `frame()`'s "this
/// cell is default, don't draw it" decision, the cursor block
/// ([`Theme::cursor_linear`]) and the answer to the application's color query
/// (OSC 10/11) are all read from the same value. Had they lived in two places,
/// when one changed the window and the cells would be different colors.
///
/// The **nine-role** model: the four base roles, the two status roles
/// (`success`, `error`) and `cursor`, then `info` and `warning`; beside them
/// `selection` and the two search highlights (`search_match`,
/// `search_current`) — outside the nine: the selection and search colors are
/// the terminal's own surface. The last two status roles (warning, info) came
/// only once something drew them — **a role that isn't drawn isn't added**, because the day a key with no consumer enters
/// the theme file it promises a format, and the promise has nothing behind it.
/// The fields are `0xRRGGBB` (the top byte is not read) and `pub`: the type is
/// a record, like `Settings`; the path that establishes validity is the theme
/// parser ([`Theme::parse`]). Alacritty's `Rgb` is not visible in the `pub`
/// surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    /// Default background; the window's ground.
    pub background: u32,
    /// Default foreground.
    pub foreground: u32,
    /// Dim (SGR 2) default foreground. The dim of named and indirect colors
    /// comes from a rule, by blending toward the background ([`dim_toward`]);
    /// this role is only for the default foreground.
    pub dim: u32,
    /// Accent; today the **running command block's stripe**.
    ///
    /// The cursor is no longer here ([`Theme::cursor`]): while the two were fed
    /// from a single value, a "make the cursor gold" request made the stripe
    /// gold too.
    pub accent: u32,
    /// The cursor block's color — the counterpart of ANSI 258 ("cursor color").
    ///
    /// A separate role, because it answers a separate question: `accent` is
    /// "which thing stands out", this is "where is the caret". Slot 258 has
    /// been an alias to `accent` until now and that was a leftover — we
    /// declare `Cs` in terminfo, so an application's ability to change the
    /// cursor color (OSC 12) will be built on this role too.
    ///
    /// **The block is opaque and the letter under it is drawn in the
    /// background color** (`Session::frame`), so this color has to be legible
    /// against the background **and** against the background-colored letter on
    /// top of itself: a light gold on a dark theme, a dark one on a light
    /// theme.
    pub cursor: u32,
    /// The mouse selection's highlight — the ground of the line runs.
    ///
    /// **The text's color doesn't change**: the selected cell is
    /// drawn with its own foreground, so this color has to be legible with the
    /// default foreground **and** with the palette's colored eight, and also
    /// distinct from the background. In an unfocused window it fades toward the
    /// background ([`Theme::selection_unfocused_linear`]).
    pub selection: u32,
    /// The highlight of all matches of the scrollback search (⌘F) — the tone
    /// that stays in the background and says "it's here too".
    ///
    /// The same contract as the selection: the text is drawn with its own
    /// foreground, so the criterion is the selection's (text legible on the
    /// background is legible on the highlight too) and in an unfocused window
    /// it fades toward the background
    /// ([`Theme::search_match_unfocused_linear`]). It must be separate from the
    /// selection in **hue**: the selection is drawn on top of the search and
    /// the two can sit side by side.
    pub search_match: u32,
    /// The current match's highlight — the single match ⏎/⌘G points at;
    /// **more distinct** than `search_match` but under the same contract.
    pub search_current: u32,
    /// Status: success. Today the stripe of a command block that ended with
    /// exit code zero.
    pub success: u32,
    /// Status: error. Today the stripe of a command block that ended with a
    /// nonzero exit code.
    pub error: u32,
    /// Status: info. Today the **remote session**: the `⇄` with the host
    /// in the context line and the dock's top hairline.
    ///
    /// Not `accent`, because that is the running command's stripe and ssh is a
    /// running command too — the same color would carry two meanings
    /// ("something is running" / "you are remote"). It must be legible on the
    /// background (3:1, `color::tests`): the host is text in the context line.
    pub info: u32,
    /// Status: warning. Today the remote host marked **staging**:
    /// `⇄ host`, the dock's top hairline and the tab's dot — and the search
    /// matches' marks on the scroll bar's track
    /// ([`Theme::search_mark_linear`]).
    ///
    /// Its value is the theme's own ANSI yellow (the precedent of `info`'s
    /// cyan). [`Self::cursor`] is gold and deliberately distinct from the
    /// palette's yellow, so this role doesn't get confused with the cursor. It
    /// must be legible on the background (3:1, `color::tests`): the host is
    /// text in the context line.
    pub warning: u32,
    /// The 16 ANSI colors: black, red, green, yellow, blue, magenta, cyan,
    /// white, then the bright eight in the same order.
    pub ansi: [u32; 16],
}

impl Theme {
    /// The embedded dark theme — the one in effect when there is no settings
    /// file and in a timed run.
    ///
    /// The ANSI tones are colors with broken saturation on a neutral gray base.
    /// Black is deliberately distinct from the background — `\e[40m` must be a
    /// visible block, not an undrawn cell. `dim` is the result of vte's
    /// multiplication of `foreground × 2/3` (`f32`, truncation): originally the
    /// dim foreground was computed that way. The role is a value, not a rule —
    /// when the dim of named colors started blending toward the background this
    /// value stayed in place.
    ///
    /// `success` and `error` are the palette's green and red **themselves**:
    /// the same precedent as `accent` being equal to blue. The reason the role
    /// is a separate field is not that the value differs, but that the user can
    /// change the stripe without touching the text colors.
    ///
    /// `const`: `bt-gpu`'s tests take the clear and cursor colors from here in
    /// a `const` context. That is the reason the sRGB table is `const` too.
    // The four-per-line layout and the name comments on the right are
    // load-bearing information: which ANSI name a color falls on can be read
    // only from this alignment. rustfmt opens the table into a single column
    // and destroys the alignment.
    #[rustfmt::skip]
    pub const BATERI: Theme = Theme {
        background: 0x000000,
        foreground: 0xd8d9dd,
        dim: 0x909093,
        accent: 0x7a9cc6,
        // Gold; from the palette's own yellow family (`0xd6b16a`/`0xe8c988`) but
        // distinct from it, or the cursor would read as "yellow text". **A taste
        // decision, not a measurement.** It has to be light on the black
        // background: the letter under it is drawn in the background color, that
        // is black.
        cursor: 0xd9b063,
        // A cool, dark and **low-saturation** slate; from `accent`'s family but
        // much darker than it, because text will be read on it. Criterion: every
        // text color that exceeds 3:1 on the background also exceeds 3:1 on the
        // selection (the weakest is `red`, 3.84). The saturation is deliberately low: a
        // bluer tone (`0x2b3a50`) stayed in the same color family as ANSI blue
        // and made that text harder to read than the ratio says. Distinct in
        // tone from the palette's two near-gray blacks (`0x22252b`, `0x4a4e57`)
        // — a selection must not read as a `\e[40m` block.
        selection: 0x283042,
        // Search is a warm family, distinct in **tone** from the selection's cool
        // slate: matches are a dark, saturated olive-brown (1.64 against the
        // background, in the background; the first value of 1.50 wasn't
        // distinguishable in a real window on the black background), the current
        // match a saturated amber (1.95). The ceiling is set by the selection's
        // criterion — every text color that exceeds 3:1 on the background
        // exceeds it on both highlights; the weakest is `red` on the current
        // match, 3.13 (`search_highlights_keep_every_readable_text_readable`).
        // **A taste decision**, chosen with an offscreen dump.
        search_match: 0x3a3212,
        search_current: 0x503a0c,
        success: 0x8bb58b,
        error: 0xd16d6a,
        // The theme's own ANSI cyan, the same precedent as
        // `success`/`error` being the palette's own colors.
        info: 0x79b3b3,
        // The theme's own ANSI yellow.
        warning: 0xd6b16a,
        ansi: [
            0x22252b, 0xd16d6a, 0x8bb58b, 0xd6b16a, // black   red      green    yellow
            0x7a9cc6, 0xb08ec0, 0x79b3b3, 0xc8c9cc, // blue    magenta  cyan     white
            0x4a4e57, 0xe58b88, 0xa4cba4, 0xe8c988, // the bright eight, same order
            0x9bb8dc, 0xc9aad8, 0x96caca, 0xe6e7ea,
        ],
    };

    /// The embedded light theme — `[appearance] theme = "system"` picks this in
    /// the light appearance (the default of `light_theme`).
    ///
    /// The values were accepted by eye; their criteria:
    ///
    /// - **The meaning of the ANSI names is preserved.** 0 (black) is the dark
    ///   end, 7 and 15 (white) the light end. White text on a light background
    ///   reads poorly, but darkening the color named "white" would break an
    ///   application that uses it as a background block (`\e[47m`, the tmux
    ///   bar). Bright white is still distinct from the background: `\e[107m`
    ///   must remain a visible block, the same reasoning as `BATERI`'s black.
    /// - **The colored eight is legible on a light background.** The dark
    ///   theme's pastels would vanish on white; yellow and cyan are therefore
    ///   in dark, saturated tones (mustard, petrol). The bright eight is a bit
    ///   lighter than the normal but stays usable as a text color — the
    ///   directory in `ls --color`, the added line in `git diff`.
    /// - **The cursor is distinct from the background and the foreground:** a
    ///   dark blue block; because the letter under it is drawn in the background
    ///   color (`Session::frame`), the block must be legible against the
    ///   background color too.
    /// - `dim` is the foreground blended into the background ([`dim_toward`]) —
    ///   the same rule as the dim of named colors, not a separate taste.
    #[rustfmt::skip]
    pub const BATERI_LIGHT: Theme = Theme {
        background: 0xf5f6f8,
        foreground: 0x24262c,
        dim: 0x696b70,
        accent: 0x3d6aa8,
        // On the light theme the gold is **dark**: the block is opaque and the
        // letter under it is drawn with the background (almost white), so the
        // letter would vanish under a light gold. The dark theme's tone can't be
        // carried over directly; the bronze of the same family.
        cursor: 0x8a6512,
        // A light ice blue: the text is dark, so the selection is a step darker
        // than the background. The same criterion as the dark theme's — every
        // text color that exceeds 3:1 on the background exceeds it on the
        // selection too (the weakest is `bright_yellow`, 3.03); the previous
        // value (`0xc9d8ee`) pushed bright yellow, green and cyan below 3:1.
        // Distinct in tone from bright white (`0xdcdee3`).
        selection: 0xdde6f3,
        // On the light theme the same family is a step darker than the
        // background: matches are a pale cream (1.05 — the distinction is
        // carried by hue, not brightness), the current match a saturated honey
        // (1.17). The weakest of the criterion is again `bright_yellow`, 3.01 on
        // the current match; distinct in tone from the selection's ice blue.
        search_match: 0xf9f1d2,
        search_current: 0xfee29a,
        success: 0x3b7a3b,
        error: 0xb5423d,
        // The theme's own ANSI cyan.
        info: 0x23787f,
        // The theme's own ANSI yellow.
        warning: 0x8f6a00,
        ansi: [
            0x2b2e35, 0xb5423d, 0x3b7a3b, 0x8f6a00, // black   red      green    yellow
            0x3a66a6, 0x8a4c9c, 0x23787f, 0xb9bbc1, // blue    magenta  cyan     white
            0x70737b, 0xc9504a, 0x4a8f4a, 0xa67c00, // the bright eight, same order
            0x4a78ba, 0x9d5db0, 0x2f8a92, 0xdcdee3,
        ],
    };

    /// The embedded warm light theme: warm ink on unbleached cream.
    ///
    /// The five roles a screenshot shows were sampled from one, pixel by
    /// pixel; the rest is drawn from the same warm family against the
    /// criteria of [`Self::BATERI_LIGHT`]:
    ///
    /// - **The background is flat.** The sampled surface is a top-to-bottom
    ///   gradient (`0xf7f4ed` → `0xebe4da`); with no material layer to draw
    ///   one, the value is the tone where the text sits.
    /// - **The foreground** is the median of the glyph cores, a warm
    ///   near-black. `dim` is that foreground blended into the background
    ///   ([`dim_toward`]), the rule `BATERI_LIGHT` follows.
    /// - **`accent` and `cursor` are the same burnt orange:** the sample's
    ///   input mark and caret are both that color, and the dock draws its mark
    ///   with `accent` at rest. The letter under the cursor is drawn in the
    ///   background color, 4.70 on the block.
    /// - **The ANSI names keep their meaning** (white is the light end) in
    ///   earth tones; every colored entry, bright ones included, is at least
    ///   3.81 on the background so it stays above 3:1 on the highlights.
    #[rustfmt::skip]
    pub const LINEN: Theme = Theme {
        background: 0xf3efe7,
        foreground: 0x2a2520,
        dim: 0x6d6862,
        accent: 0xa95200,
        cursor: 0xa95200,
        // A pale apricot from `accent`'s family, a step darker than the
        // background (1.14). The criterion is the other themes': every text
        // color that exceeds 3:1 on the background also exceeds it on the
        // selection (the weakest is `bright_green`, 3.35).
        selection: 0xefdfcf,
        // Matches a pale cream (1.06), the current match a honey (1.17) — the
        // same yellow family, told apart by brightness so the distinction
        // survives the unfocused fade. The weakest is `bright_green` on the
        // current match, 3.27.
        search_match: 0xf2e9cc,
        search_current: 0xf6dca8,
        success: 0x4f7a32,
        error: 0xb23a32,
        // The theme's own ANSI cyan.
        info: 0x2d7672,
        // The theme's own ANSI yellow; distinct in hue from the orange cursor.
        warning: 0x8a6500,
        ansi: [
            0x2f2a25, 0xb23a32, 0x4f7a32, 0x8a6500, // black   red      green    yellow
            0x3b6488, 0x8d4f7f, 0x2d7672, 0xbdb5a8, // blue    magenta  cyan     white
            0x766d63, 0xc8503f, 0x578535, 0x977200, // the bright eight, same order
            0x4a76a0, 0xa0608f, 0x33847b, 0xddd6ca,
        ],
    };

    /// The embedded violet theme: vivid colours on a deep violet ground —
    /// bateri's own palette, in the spirit of the neon-on-purple editor
    /// themes, none of whose values it takes.
    ///
    /// The palette below is the design; the roles a palette does not carry
    /// come from it by the rule the adapted palettes follow
    /// ([`Theme::adapted`]), so its search highlights are `BATERI`'s weights
    /// in its own yellow. Its criteria, the ones bateri's own themes are made
    /// to:
    ///
    /// - **Every coloured entry stands at least 6.2:1 on the ground** (the
    ///   weakest is `bright_black`, 6.26 — it is the comment colour, it must
    ///   read), so every text colour stays above 3:1 on the selection and on
    ///   both search highlights too (the weakest is `bright_black` on the
    ///   current match, 3.16) without being moved.
    /// - **Black is a visible block** on the ground (1.27), the reasoning of
    ///   `BATERI`'s black; bright white is pure white.
    /// - **The cursor is amber**, apart from the palette's gold yellow, or it
    ///   would read as "yellow text"; the letter under it is drawn in the
    ///   ground's violet.
    /// - **The selection is a violet slate** from the ground's own family, at
    ///   the weight of `BATERI`'s (1.59) and apart in hue from the warm search
    ///   highlights.
    ///
    /// **A taste decision, not a measurement**, chosen by its ratios.
    pub const ULTRAVIOLET: Theme = Theme::adapted(&Scheme {
        name: "ultraviolet",
        title: "Ultraviolet",
        background: 0x1b1433,
        foreground: 0xece6ff,
        cursor: 0xffb454,
        selection: 0x3d3272,
        #[rustfmt::skip]
        ansi: [
            0x2e2556, 0xff6e8f, 0x5cf2a6, 0xffd23f, // black   red      green    yellow
            0x8796ff, 0xd98aff, 0x4fe3ff, 0xcfc6ea, // blue    magenta  cyan     white
            0x9d93cc, 0xff94ad, 0x8dffc4, 0xffe57a, // the bright eight, same order
            0xadb7ff, 0xe9b0ff, 0x8bedff, 0xffffff,
        ],
    });

    /// The themes embedded in the application, by name. The user's
    /// `themes/{name}.toml` shadows the same name; that decision is in
    /// `bt-shell`'s name resolution.
    pub fn embedded(name: &str) -> Option<Theme> {
        EMBEDDED
            .iter()
            .find(|(embedded, _, _)| *embedded == name)
            .map(|(_, _, theme)| *theme)
    }

    /// The embedded themes' names, in the table's order: bateri's own, then
    /// the adapted palettes by title.
    pub fn embedded_names() -> impl Iterator<Item = &'static str> {
        EMBEDDED.iter().map(|(name, _, _)| *name)
    }

    /// What the menus show for an embedded theme — `Catppuccin Mocha` for
    /// `catppuccin-mocha`. The name stays what the settings file takes; a
    /// user's theme has no title, its name is shown.
    pub fn embedded_title(name: &str) -> Option<&'static str> {
        EMBEDDED
            .iter()
            .find(|(embedded, _, _)| *embedded == name)
            .map(|(_, title, _)| *title)
    }

    /// The embedded themes' names as View ▸ Theme ▸ groups them, a line between
    /// groups: bateri's own, then the adapted palettes on a dark background,
    /// then those on a light one ([`Theme::is_dark`]) — each in the table's
    /// order, i.e. by title.
    pub fn embedded_groups() -> [Vec<&'static str>; 3] {
        let (own, adapted) = EMBEDDED.split_at(OWN.len());
        let names = |dark: bool| {
            adapted
                .iter()
                .filter(|(_, _, theme)| theme.is_dark() == dark)
                .map(|(name, _, _)| *name)
                .collect()
        };
        [
            own.iter().map(|(name, _, _)| *name).collect(),
            names(true),
            names(false),
        ]
    }

    /// A palette adapted from another project ([`Scheme`]) as a theme: the
    /// scheme's own colours where bateri's roles have a counterpart, the rest
    /// derived from them.
    ///
    /// **The scheme's ANSI colours, background and foreground are never
    /// touched** — that is what makes it the theme people know. The derived
    /// roles follow the rules bateri's own themes were made by, and every
    /// target is a ratio one of them already stands at, so no new taste
    /// constant enters ([`Theme::BATERI`] on a dark background,
    /// [`Theme::BATERI_LIGHT`] on a light one — the *reference*):
    ///
    /// - `dim` is the foreground blended into the background ([`dim_toward`]),
    ///   the rule `BATERI_LIGHT` follows.
    /// - `accent`, `success` and `error` are the palette's blue, green and red
    ///   — `BATERI`'s precedent: the roles are separate so a user can change a
    ///   stripe without touching the text colours.
    /// - `info` and `warning` are the palette's cyan and yellow, and `cursor`
    ///   the scheme's own, each moved just far enough from the background to
    ///   read on it ([`readable`]): the first two are text in the dock's
    ///   context line and the letter under the cursor is drawn in the
    ///   background colour. Most schemes need no move; a light scheme's
    ///   yellow usually does.
    /// - `selection` is the scheme's own when it is a highlight: closer to
    ///   the background than to the foreground — many schemes give the
    ///   foreground itself, written for terminals that swap the selected
    ///   text's colours, which bateri does not — and standing out from the
    ///   background at least as much as the reference's selection does in an
    ///   unfocused window. Otherwise it is the accent laid over the
    ///   background ([`tint`]) up to the reference's selection — `BATERI`'s
    ///   cool slate is drawn from its accent's family too.
    /// - `search_match` and `search_current` are the palette's yellow laid
    ///   over the background up to the reference's two highlights — search is
    ///   a warm family against the selection's cool one, and the current
    ///   match stands out more.
    ///
    /// The text under the selection and the search highlights is held to the
    /// minimum contrast when drawn (`session::cell_style`), so the highlights
    /// need not leave room under every text colour of the scheme — many put a
    /// text colour right at 3:1 on their background.
    pub(crate) const fn adapted(scheme: &Scheme) -> Theme {
        let ground = rgb(scheme.background);
        let dark = Theme {
            background: scheme.background,
            ..Theme::BATERI
        }
        .is_dark();
        let reference = if dark {
            Theme::BATERI
        } else {
            Theme::BATERI_LIGHT
        };
        let base = reference.background_rgb();
        let ansi = scheme.ansi;
        let selection = rgb(scheme.selection);
        let unfocused = dim_toward(rgb(reference.selection), base);
        let own_selection = contrast(selection, ground)
            < contrast(selection, rgb(scheme.foreground))
            && contrast(selection, ground) >= contrast(unfocused, base);
        let selection = if own_selection {
            selection
        } else {
            tint(
                rgb(ansi[4]),
                ground,
                contrast(rgb(reference.selection), base),
            )
        };
        let yellow = rgb(ansi[3]);
        let search_match = tint(yellow, ground, contrast(rgb(reference.search_match), base));
        let search_current = tint(
            yellow,
            ground,
            contrast(rgb(reference.search_current), base),
        );
        Theme {
            background: scheme.background,
            foreground: scheme.foreground,
            dim: pack(dim_toward(rgb(scheme.foreground), ground)),
            accent: ansi[4],
            cursor: pack(readable(rgb(scheme.cursor), ground, dark)),
            selection: pack(selection),
            search_match: pack(search_match),
            search_current: pack(search_current),
            success: ansi[2],
            error: ansi[1],
            info: pack(readable(rgb(ansi[6]), ground, dark)),
            warning: pack(readable(rgb(ansi[3]), ground, dark)),
            ansi,
        }
    }

    /// The default background's **sRGB** bytes (`[r, g, b]`) — the window
    /// chrome's ground (`bt-shell`, `NSColor` sRGB). The path to the GPU is
    /// [`Theme::background_linear`]; both come from the same value and the rule
    /// that unpacks the bytes is single ([`rgb`]), so the title bar and the
    /// clear color can't drift apart.
    pub const fn background_srgb(&self) -> [u8; 3] {
        let Rgb { r, g, b } = rgb(self.background);
        [r, g, b]
    }

    /// The window's clear color, **linear** RGBA.
    pub const fn background_linear(&self) -> LinearRgba {
        linear_rgba(rgb(self.background))
    }

    /// The cursor block's color, **linear** RGBA. Here so it doesn't sit as a
    /// constant in the renderer: the color decision is the theme's, the draw
    /// decision the renderer's.
    pub const fn cursor_linear(&self) -> LinearRgba {
        linear_rgba(rgb(self.cursor))
    }

    /// The accent color, **linear** RGBA — today **only the running command
    /// block's stripe**. The cursor was split into its own role
    /// ([`Theme::cursor_linear`]); while the two were fed from a single value,
    /// a "make the cursor gold" request made the stripe gold too.
    pub const fn accent_linear(&self) -> LinearRgba {
        linear_rgba(rgb(self.accent))
    }

    /// The selection highlight's color, **linear** RGBA — in a focused window.
    pub const fn selection_linear(&self) -> LinearRgba {
        linear_rgba(rgb(self.selection))
    }

    /// The selection in an unfocused window: **one third** toward the
    /// background ([`dim_toward`]).
    ///
    /// Not a new role but a derived value (the precedent of
    /// [`Theme::quiet_linear`]): the selection isn't erased, it's pulled back —
    /// when focus returns the same selection is in the same place. Which one
    /// gets drawn is `bt-gpu`'s decision, because focus doesn't enter `bt-core`.
    pub const fn selection_unfocused_linear(&self) -> LinearRgba {
        linear_rgba(dim_toward(rgb(self.selection), self.background_rgb()))
    }

    /// The search matches' highlight, **linear** RGBA — in a focused window.
    pub const fn search_match_linear(&self) -> LinearRgba {
        linear_rgba(rgb(self.search_match))
    }

    /// The match highlight in an unfocused window: the selection's rule
    /// ([`Theme::selection_unfocused_linear`]), the same one third.
    pub const fn search_match_unfocused_linear(&self) -> LinearRgba {
        linear_rgba(dim_toward(rgb(self.search_match), self.background_rgb()))
    }

    /// The current match's highlight, **linear** RGBA — in a focused window.
    pub const fn search_current_linear(&self) -> LinearRgba {
        linear_rgba(rgb(self.search_current))
    }

    /// The current match's highlight in an unfocused window; the selection's rule.
    pub const fn search_current_unfocused_linear(&self) -> LinearRgba {
        linear_rgba(dim_toward(rgb(self.search_current), self.background_rgb()))
    }

    /// The default foreground, **linear** RGBA; the color of the text the dock
    /// writes.
    ///
    /// The grid doesn't go through this path — there the color is resolved from
    /// the cell's `Color` with [`resolve_fg`]. The dock's cell has no `Color`:
    /// the mirror carries plain text and the default is the role itself.
    pub const fn foreground_linear(&self) -> LinearRgba {
        linear_rgba(rgb(self.foreground))
    }

    /// The dim foreground, **linear** RGBA; the color of the suggestion tail
    /// (`POSTDISPLAY`) in the dock.
    ///
    /// The role isn't new, its reader is: the default foreground with SGR 2
    /// ([`resolve_fg`]) comes from here too. A suggestion is "text not yet
    /// written" and the definition of dimness — reduce the difference from the
    /// background — describes exactly that.
    pub const fn dim_linear(&self) -> LinearRgba {
        linear_rgba(rgb(self.dim))
    }

    /// The dim of the dim — text that **must be read but doesn't stand out**.
    ///
    /// **Not a new role but a derived value** (a role that isn't drawn isn't
    /// added): the dim foreground blended into the background once
    /// more, the rule again [`dim_toward`].
    ///
    /// Its only consumer is the **parent directories** in the dock's context
    /// line: while the active folder and the branch stay at `dim`, the path
    /// carrying them is pulled back. It's still ink, so it has to be legible —
    /// [`separator_linear`] is one step dimmer and that is no longer ink.
    pub const fn quiet_linear(&self) -> LinearRgba {
        linear_rgba(self.quiet_rgb())
    }

    const fn quiet_rgb(&self) -> Rgb {
        dim_toward(rgb(self.dim), self.background_rgb())
    }

    /// Whether the background is dark: it gives a higher contrast ratio with
    /// white than with black (WCAG), so light text is what reads on it. The
    /// threshold is not invented, it is where the two ratios are equal. One
    /// answer for everything that differs between a dark and a light theme:
    /// the window's appearance (`bt-shell`) and the split tab's ground
    /// ([`Theme::ground_srgb`]).
    pub const fn is_dark(&self) -> bool {
        let background = self.background_rgb();
        contrast(background, rgb(0xffffff)) > contrast(background, rgb(0x000000))
    }

    /// The split tab's ground, **sRGB** bytes: a gradient from `[0]` at the
    /// window's top left to `[1]` at its bottom right, under the title row and
    /// round the cards (`bt-shell`). A tab with one pane has none, its title
    /// row is the background.
    ///
    /// **Not a role but a value derived from four** (a role that isn't drawn
    /// isn't added): the background is lifted a step toward the foreground —
    /// a dark theme's ground is lighter than its panes, a light theme's
    /// darker, so the cards read as cards on it — and from there the top left
    /// leans toward the accent and the bottom right toward the cursor. Every
    /// theme gets its own ground from its own colours; the steps are larger
    /// on a dark theme, where a small difference from black does not show.
    pub const fn ground_srgb(&self) -> [[u8; 3]; 2] {
        let (start, end) = if self.is_dark() { (38, 28) } else { (28, 22) };
        let base = self.step(self.background_rgb());
        let start = toward(base, rgb(self.accent), start);
        let end = toward(base, rgb(self.cursor), end);
        [[start.r, start.g, start.b], [end.r, end.g, end.b]]
    }

    /// The dock's ground, the surface its input row stands on, **linear**
    /// RGBA: the background lifted toward the light, so the dock reads as a
    /// plate one level above the content — it catches more light than what
    /// it casts its shadow on ([`Theme::dock_shadow_linear`]).
    ///
    /// **Not a role but a value derived from the theme's two ends**: "toward
    /// the light" is white on a light theme and the foreground on a dark one
    /// (black has no lighter neighbour but the ink, and white would turn a
    /// tinted dark theme grey). A dark theme takes the step every surface
    /// above the panes takes ([`Theme::band_linear`]'s rule); a light one goes
    /// most of the way to white, where a step of a few percent would not
    /// show against a pale background. **Chosen by eye, from the design.**
    /// On a pure white background there is no lighter to go to and the plate
    /// is the background — its shadow carries the edge there, as the rim
    /// ([`Theme::dock_rim_linear`]) does on pure black, where a shadow
    /// cannot show.
    pub const fn dock_ground_linear(&self) -> LinearRgba {
        linear_rgba(self.dock_ground_rgb())
    }

    const fn dock_ground_rgb(&self) -> Rgb {
        if self.is_dark() {
            self.step(self.background_rgb())
        } else {
            toward(self.background_rgb(), WHITE, 60)
        }
    }

    /// The dock's top edge, **linear** RGBA: the plate's rim where it catches
    /// the light — its ground most of the way toward the light end on a light
    /// theme (an edge brighter than both sides of it), a little of the way on
    /// a dark one (a faint lit line; on pure black it is what tells the plate
    /// from the content, since a shadow cannot show there). It is the top
    /// line's colour whenever nothing else colours it (a host, a program, an
    /// upload). **Chosen by eye, from the design.**
    pub const fn dock_rim_linear(&self) -> LinearRgba {
        let (end, share) = if self.is_dark() {
            (rgb(self.foreground), 14)
        } else {
            (WHITE, 90)
        };
        linear_rgba(toward(self.dock_ground_rgb(), end, share))
    }

    /// The colour of the shadow the dock casts on the content above it,
    /// **linear** and opaque: the theme's dark end — black on a dark theme,
    /// the foreground on a light one, so a warm theme's shadow is warm. How
    /// strongly it is drawn is [`Theme::dock_depth`].
    pub const fn dock_shadow_linear(&self) -> LinearRgba {
        linear_rgba(if self.is_dark() {
            BLACK
        } else {
            rgb(self.foreground)
        })
    }

    /// How strongly the dock's shadow and its glow are drawn where they meet
    /// the dock, `(shadow, glow)`, `0..=1`; both fade to nothing a fixed
    /// height above it (`bt-gpu`).
    ///
    /// **A number per kind of theme, not a colour with an alpha** (opacity is
    /// the drawing's, `LinearRgba` carries none): the same opacity reads very
    /// differently on the two, so the light/dark answer that decides
    /// everything else that differs between them ([`Theme::is_dark`]) decides
    /// this too. On a light theme the shadow is a faint darkening and the
    /// glow — the shadow taking on the colour of what the top edge says, a
    /// host or a program — a tint; on a dark one the shadow must be deep to
    /// show at all and the glow is light given off. **Chosen by eye, from the
    /// design.**
    pub const fn dock_depth(&self) -> (f32, f32) {
        if self.is_dark() {
            (0.55, 0.30)
        } else {
            (0.14, 0.18)
        }
    }

    /// The tone the dock's folder row stands on, **linear** RGBA: the dock's
    /// ground ([`Theme::dock_ground_linear`]) a step toward the foreground —
    /// a dark theme's row is lighter than the input row above it, a light
    /// theme's darker — so the two rows read as two surfaces without a line
    /// between them.
    ///
    /// **Not a role but a value derived from the rule for "a surface a step
    /// above"** ([`Theme::step`]), the same rule the split tab's ground
    /// starts from ([`Theme::ground_srgb`], a step above the panes), so the
    /// two can't drift apart.
    pub const fn band_linear(&self) -> LinearRgba {
        linear_rgba(self.band_rgb())
    }

    /// The folder row's tone, as **sRGB** bytes (`[r, g, b]`): the tests read
    /// the colour as a person writes it, and a toolkit that paints with sRGB
    /// (the command palette's bottom band) takes the tone from here instead of
    /// stepping the ground again. The path to the GPU is
    /// [`Theme::band_linear`], both from the same value.
    pub const fn band_srgb(&self) -> [u8; 3] {
        let Rgb { r, g, b } = self.band_rgb();
        [r, g, b]
    }

    /// The dock's ground as **sRGB** bytes, the other reading of
    /// [`Theme::dock_ground_linear`]: the surface a toolkit view that wants to
    /// stand like the dock's plate (the command palette) is filled with.
    pub const fn dock_ground_srgb(&self) -> [u8; 3] {
        let Rgb { r, g, b } = self.dock_ground_rgb();
        [r, g, b]
    }

    const fn band_rgb(&self) -> Rgb {
        self.step(self.dock_ground_rgb())
    }

    /// The background `steps` steps above, **linear** RGBA: [`Theme::step`]'s
    /// rule applied to the panes' own background `steps` times — the tone
    /// of a surface standing that many levels over the content (a card, a
    /// field, a key cap in a UI page). `0` is the background itself.
    ///
    /// **Not a role but a value derived from the rule for "a surface a step
    /// above"**, the same one the dock's folder row and the split tab's
    /// ground use, so a page's cards, the dock and the ground round the
    /// cards cannot drift apart. A page names the level, never a colour.
    pub const fn raised_linear(&self, steps: u8) -> LinearRgba {
        let mut tone = rgb(self.background);
        let mut left = steps;
        while left > 0 {
            tone = self.step(tone);
            left -= 1;
        }
        linear_rgba(tone)
    }

    /// `base` a step toward the foreground: **the one rule for "a surface a
    /// step above"** — the folder row above the dock's ground, the split
    /// tab's ground above the panes. The step is larger on a dark theme,
    /// where a small difference from black does not show; another theme gets
    /// its own tone from its own colours.
    const fn step(&self, base: Rgb) -> Rgb {
        let lift = if self.is_dark() { 8 } else { 4 };
        toward(base, rgb(self.foreground), lift)
    }

    /// The hairlines' color, **linear** RGBA.
    ///
    /// The **third** application of the same rule ([`dim_linear`] →
    /// [`quiet_linear`] → this): all three steps are born from a single
    /// `dim_toward` chain, no separate taste constant enters.
    ///
    /// The extra step is deliberate and the criterion is this: the separator is
    /// **not ink**. Had it stopped at `quiet_linear`, the lines would have the
    /// same weight as the quietest text next to them and the eye would take them
    /// for something to read too (the user: "darken the lines' colors
    /// further, don't make them so noticeable"). It must be seen but must not be
    /// something to read.
    pub const fn separator_linear(&self) -> LinearRgba {
        linear_rgba(self.separator_rgb())
    }

    /// The hairlines' color, as **sRGB** bytes (`[r, g, b]`) — the separator
    /// between splits (`bt-shell`, `NSColor` sRGB). The path to the
    /// GPU is [`Theme::separator_linear`]; both come from the same chain (the
    /// precedent of [`Theme::background_srgb`] and `background_linear`), so the
    /// separator and the dock's lines can't drift apart.
    pub const fn separator_srgb(&self) -> [u8; 3] {
        let Rgb { r, g, b } = self.separator_rgb();
        [r, g, b]
    }

    const fn separator_rgb(&self) -> Rgb {
        dim_toward(self.quiet_rgb(), self.background_rgb())
    }

    /// The **linear** RGBA of the palette's color number `index`.
    ///
    /// The sibling of [`resolve`] without the `Colors` table and its only
    /// caller is the dock: the mirror's `region_highlight` carries a number,
    /// while the table an application changes with OSC 4 is behind the `Term`
    /// lock (the same root as `Event::ColorRequest`'s known limit). **Known
    /// limit:** the dock doesn't see a palette changed with OSC 4, it draws the
    /// theme's.
    pub(crate) fn indexed_linear(&self, index: u8) -> LinearRgba {
        linear_rgba(self.default(index as usize))
    }

    /// The stripe color of a block that ended successfully, **linear** RGBA.
    pub const fn success_linear(&self) -> LinearRgba {
        linear_rgba(rgb(self.success))
    }

    /// The stripe color of a block that ended with an error, **linear** RGBA.
    pub const fn error_linear(&self) -> LinearRgba {
        linear_rgba(rgb(self.error))
    }

    /// A command block's stripe colour, **linear** — the **one** mapping from
    /// a block's drawable state to its role (running `accent`, success
    /// `success`, error `error`): the grid's and the band's markers and the
    /// scroll bar's block marks all read it, so a mark on the track and the
    /// stripe beside its command cannot differ.
    pub(crate) const fn stripe_linear(&self, stripe: Stripe) -> LinearRgba {
        linear_rgba(rgb(self.stripe_rgb(stripe)))
    }

    /// [`Theme::stripe_linear`] as **sRGB** bytes (`[r, g, b]`) — the
    /// scroll bar tip's dot (`bt-shell`, `NSColor` sRGB); the same mapping.
    pub(crate) const fn stripe_srgb(&self, stripe: Stripe) -> [u8; 3] {
        let Rgb { r, g, b } = rgb(self.stripe_rgb(stripe));
        [r, g, b]
    }

    const fn stripe_rgb(&self, stripe: Stripe) -> u32 {
        match stripe {
            Stripe::Running => self.accent,
            Stripe::Success => self.success,
            Stripe::Error => self.error,
        }
    }

    /// The info role, **linear** RGBA — the remote session's host and the
    /// dock's top hairline.
    pub const fn info_linear(&self) -> LinearRgba {
        linear_rgba(rgb(self.info))
    }

    /// The warning role, **linear** RGBA — the remote host marked staging.
    pub const fn warning_linear(&self) -> LinearRgba {
        linear_rgba(rgb(self.warning))
    }

    /// A search match's mark on the scroll bar's track, **linear**: `warning`
    /// laid over the background at [`SEARCH_MARK_PERCENT`] and drawn
    /// **opaque** — the mark sits over the thumb and must hide it, not mix
    /// with it.
    ///
    /// **Not `search_match`**: that highlight is a ground for text, tuned dark
    /// enough to read over (`0x3a3212` on black), and a two-point line of it
    /// does not show at all. A mark is read by itself, against the
    /// background, so it takes the theme's yellow role. Not a new role but a
    /// derived value (the precedent of [`Theme::quiet_linear`]).
    pub const fn search_mark_linear(&self) -> LinearRgba {
        linear_rgba(self.search_mark_rgb())
    }

    /// The **current** match's mark: whichever of `warning` and the palette's
    /// bright yellow (`[ansi] bright_yellow`) stands further from the
    /// background ([`contrast`]).
    ///
    /// The choice is a measure, not a per-theme constant: on the dark theme
    /// the brighter yellow is the brighter mark, on the light theme
    /// `warning`'s darker yellow is — and a user theme whose bright yellow
    /// is not a yellow at all still gets the one that reads.
    pub const fn search_current_mark_linear(&self) -> LinearRgba {
        linear_rgba(self.search_current_mark_rgb())
    }

    const fn search_mark_rgb(&self) -> Rgb {
        over(
            rgb(self.warning),
            SEARCH_MARK_PERCENT,
            self.background_rgb(),
        )
    }

    const fn search_current_mark_rgb(&self) -> Rgb {
        let (warning, bright) = (rgb(self.warning), rgb(self.ansi[11]));
        let ground = self.background_rgb();
        if contrast(bright, ground) > contrast(warning, ground) {
            bright
        } else {
            warning
        }
    }

    /// The **single** path from a remote host's mark to a color,
    /// **linear**: production is `error`, staging `warning`, development
    /// `success`, unmarked `info`; a direct color is itself, linearized from
    /// sRGB.
    ///
    /// Meaning and color meet here, so a theme change carries the mark along by
    /// itself: `Session` holds only the mark, the frame resolves the color from
    /// that frame's theme.
    pub const fn mark_linear(&self, mark: HostMark) -> LinearRgba {
        linear_rgba(rgb(self.mark_rgb(mark)))
    }

    /// The mark's color, **sRGB** `0xRRGGBB` — the mapping itself
    /// ([`Theme::mark_linear`] linearizes it). For the tab's dot, which wants
    /// sRGB (`NSColor`); the linear value is only the GPU's.
    pub const fn mark_rgb(&self, mark: HostMark) -> u32 {
        match mark {
            HostMark::Production => self.error,
            HostMark::Staging => self.warning,
            HostMark::Development => self.success,
            HostMark::None => self.info,
            HostMark::Rgb(hex) => hex,
        }
    }

    /// The palette's color number `index`. The numbering is alacritty's
    /// `term::color` table: 0..16 ANSI, 16..232 cube, 232..256 gray ramp, 256+
    /// role colors.
    ///
    /// Not cold: in a fresh session the `Colors` table is empty (only OSC
    /// 4/10/11 fill it), so `resolve` falls here for every cell.
    #[inline]
    pub(crate) fn default(&self, index: usize) -> Rgb {
        match index {
            0..=15 => rgb(self.ansi[index]), // audit: the arm bounds the index to 0..16
            16..=231 => {
                let n = index - 16;
                Rgb {
                    r: CUBE[n / 36],
                    g: CUBE[(n / 6) % 6],
                    b: CUBE[n % 6],
                }
            }
            232..=255 => {
                let v = 8 + 10 * (index - 232) as u8; // audit: the arm bounds it, 238 at most
                Rgb { r: v, g: v, b: v }
            }
            256 => rgb(self.foreground),
            257 => rgb(self.background),
            // Cursor color: from its own role. Until now it was an alias to
            // `accent` and two separate questions were answered from one value.
            258 => rgb(self.cursor),
            // 259..=266 the dim ANSI eight, 267 bright foreground, 268 dim
            // foreground.
            259..=266 => dim_toward(rgb(self.ansi[index - 259]), self.background_rgb()),
            267 => rgb(self.foreground),
            268 => rgb(self.dim),
            // The table has 269 entries; an index that falls here means
            // alacritty changed. We stay silent by returning the background
            // instead of a color, we don't panic.
            _ => rgb(self.background),
        }
    }

    /// The default background as `Rgb`; `frame()` does the comparison with this
    /// and doesn't look for f32 equality.
    pub(crate) const fn background_rgb(&self) -> Rgb {
        rgb(self.background)
    }
}

/// A terminal palette: the four roles a terminal palette carries and the
/// sixteen ANSI colours. [`Theme::adapted`] makes a theme of it. An adapted
/// one is another project's, as its authors chose it — written by a script
/// from the scheme's file, never by hand (`schemes`); bateri's own
/// [`Theme::ULTRAVIOLET`] is designed as one.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Scheme {
    /// What `[appearance] theme` takes.
    pub(crate) name: &'static str,
    /// What the menus show ([`Theme::embedded_title`]).
    pub(crate) title: &'static str,
    pub(crate) background: u32,
    pub(crate) foreground: u32,
    pub(crate) cursor: u32,
    pub(crate) selection: u32,
    pub(crate) ansi: [u32; 16],
}

/// bateri's own themes — name, title, theme — designed here.
const OWN: [(&str, &str, Theme); 4] = [
    ("bateri", "bateri", Theme::BATERI),
    ("bateri-light", "bateri Light", Theme::BATERI_LIGHT),
    ("linen", "Linen", Theme::LINEN),
    ("ultraviolet", "Ultraviolet", Theme::ULTRAVIOLET),
];

/// The table of embedded themes, [`Theme::embedded`] reads it: bateri's own,
/// then the adapted palettes ([`schemes::SCHEMES`], by title), derived at
/// compile time.
const EMBEDDED: [(&str, &str, Theme); OWN.len() + schemes::SCHEMES.len()] = {
    let mut table = [("", "", Theme::BATERI); OWN.len() + schemes::SCHEMES.len()];
    let mut i = 0;
    while i < table.len() {
        // audit: `i` is below the table's length, the sum of the two lists';
        // evaluated at compile time, so a slip is a build error, not a panic.
        table[i] = if i < OWN.len() {
            OWN[i]
        } else {
            let scheme = &schemes::SCHEMES[i - OWN.len()];
            (scheme.name, scheme.title, Theme::adapted(scheme))
        };
        i += 1;
    }
    table
};

/// `color` moved toward the end of the scale away from `ground` — white on a
/// dark ground, black on a light one — just far enough to read on it as text
/// ([`MINIMUM_CONTRAST`], the bar bateri's own themes hold their text roles
/// to); unchanged when it already does. Toward the end rather than toward the
/// foreground, so the hue stays: a light theme's yellow becomes a mustard, not
/// a grey.
///
/// In 8-bit sRGB and by whole percent ([`toward`]), not closed-form like
/// [`LinearRgba::legible_on`]: the result is a theme's `0xRRGGBB` and is
/// computed once, at compile time.
const fn readable(color: Rgb, ground: Rgb, dark: bool) -> Rgb {
    let end = if dark { rgb(0xffffff) } else { rgb(0x000000) };
    let mut percent = 0;
    while percent < 100 && contrast(toward(color, end, percent), ground) < MINIMUM_CONTRAST {
        percent += 1;
    }
    toward(color, end, percent)
}

/// `color` laid over `ground` ([`over`]) just strongly enough to stand
/// `target` apart from it ([`contrast`]) — a highlight in `color`'s hue at a
/// chosen weight; `color` itself when even that does not reach.
const fn tint(color: Rgb, ground: Rgb, target: f64) -> Rgb {
    let mut percent = 0;
    while percent < 100 && contrast(over(color, percent, ground), ground) < target {
        percent += 1;
    }
    over(color, percent, ground)
}

/// An [`Rgb`] as the theme's `0xRRGGBB`.
const fn pack(color: Rgb) -> u32 {
    ((color.r as u32) << 16) | ((color.g as u32) << 8) | color.b as u32
}

/// The **single rule** for a dim (SGR 2) color: the color moves one third of
/// the way toward the background.
///
/// Why toward the background: dimness means "reduce the difference from the
/// background". Originally the rule was vte's `× 2/3` (`impl Mul<f32> for Rgb`,
/// its comment literally "the default dim is just *2/3") and that is blending
/// toward black itself — on a light background it **darkened** dim text and
/// made it stand out.
///
/// The ratio is a design constant, not a measurement. One third was chosen
/// because on a black background it gives a result bit-for-bit identical to
/// vte's multiplication (`dim_on_black_is_vte`): the familiar dimness shifts
/// least on the dark theme, and since `BATERI`'s background is close to black
/// the values open up by a few steps.
///
/// The space is sRGB 8-bit, the same as vte's; linearization stays at the
/// boundary (`linear_rgba`). Blending in linear space would dim far less
/// perceptually on a dark background. Integer division truncates — vte's
/// `as u8` truncates too.
///
/// The target is the **theme's** background, not the one the application
/// changes with OSC 11: the clear color and `frame()`'s skip decision read from
/// the theme too.
///
/// `const`: on the palette path like `Theme::default` and a single integer
/// arithmetic.
const fn dim_toward(color: Rgb, background: Rgb) -> Rgb {
    Rgb {
        r: dim_channel(color.r, background.r),
        g: dim_channel(color.g, background.g),
        b: dim_channel(color.b, background.b),
    }
}

/// `color` moved `percent` of the way toward `target`, channel by channel in
/// sRGB, rounded.
const fn toward(color: Rgb, target: Rgb, percent: u16) -> Rgb {
    Rgb {
        r: toward_channel(color.r, target.r, percent),
        g: toward_channel(color.g, target.g, percent),
        b: toward_channel(color.b, target.b, percent),
    }
}

const fn toward_channel(color: u8, target: u8, percent: u16) -> u8 {
    // audit: the callers pass at most 100, so at most
    // ((100 - p)·255 + p·255 + 50) / 100 = 255, fits in `u8`.
    (((100 - percent) * color as u16 + percent * target as u16 + 50) / 100) as u8
}

const fn dim_channel(color: u8, background: u8) -> u8 {
    // audit: at most (2·255 + 255) / 3 = 255, fits in `u8`.
    ((2 * color as u16 + background as u16) / 3) as u8
}

/// How strongly a search match's mark lays `warning` over the background,
/// percent ([`Theme::search_mark_linear`]) — a design constant (the approved
/// design's value): quieter than the current match's full-strength mark, so
/// the one ⏎ points at stands out among many, yet still clear of the
/// background on both embedded themes (`search_marks_read_on_the_ground`).
pub(crate) const SEARCH_MARK_PERCENT: u16 = 55;

/// `color` laid over `background` at `percent`, as an **opaque** color —
/// [`dim_toward`]'s space and arithmetic (sRGB 8-bit, truncating), with the
/// weight as a parameter.
const fn over(color: Rgb, percent: u16, background: Rgb) -> Rgb {
    const fn channel(color: u8, percent: u16, background: u8) -> u8 {
        // audit: `percent ≤ 100` keeps the sum at most 255·100, so the result
        // fits in `u8`.
        ((color as u16 * percent + background as u16 * (100 - percent)) / 100) as u8
    }
    let percent = if percent > 100 { 100 } else { percent };
    Rgb {
        r: channel(color.r, percent, background.r),
        g: channel(color.g, percent, background.g),
        b: channel(color.b, percent, background.b),
    }
}

/// A color's relative luminance (WCAG 2), `0..=1` — **the one copy** of the
/// measure the theme's choices and their guards are made with. The channels
/// come from the transfer table ([`SRGB_LINEAR`]), summed in `f64`.
pub(crate) const fn luminance(color: Rgb) -> f64 {
    const fn channel(c: u8) -> f64 {
        // audit: `u8 as usize` is 0..=255 and the table has 256 entries.
        SRGB_LINEAR[c as usize] as f64
    }
    weigh(channel(color.r), channel(color.g), channel(color.b))
}

/// The luminance of three linear channels: WCAG 2's weights, written once.
const fn weigh(r: f64, g: f64, b: f64) -> f64 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// The contrast ratio between two colors (WCAG 2), `1..=21` — whichever
/// order they come in.
pub(crate) const fn contrast(a: Rgb, b: Rgb) -> f64 {
    ratio(luminance(a), luminance(b))
}

/// WCAG 2's ratio between two **luminances**, `1..=21`, whichever order they
/// come in — the formula written once: [`contrast`] measures two palette
/// colors with it, [`LinearRgba::legible_on`] two drawn ones.
const fn ratio(x: f64, y: f64) -> f64 {
    let (light, dark) = if x > y { (x, y) } else { (y, x) };
    (light + 0.05) / (dark + 0.05)
}

/// Whether `c` is drawn as a **picture**, not read as a letter: box drawing,
/// block elements, Braille, the legacy computing symbols and Powerline's
/// separators. [`LinearRgba::legible_on`] leaves these alone.
///
/// A program that paints with them chooses the colour pair on purpose: an
/// image in half blocks (`▀` with a foreground one shade off its background),
/// a faint indent guide (`│`), a Powerline arrow whose ink is the next
/// segment's ground. Lifting their ink would put seams in the picture and
/// make the guides shout — the rule is for text that cannot be read, and
/// these are not text.
pub(crate) const fn is_drawing(c: char) -> bool {
    matches!(
        c,
        '\u{2500}'..='\u{259f}'
            | '\u{2800}'..='\u{28ff}'
            | '\u{1fb00}'..='\u{1fbff}'
            | '\u{e0a0}'..='\u{e0d7}'
    )
}

/// [`contrast`] between two `0xRRGGBB` colors — the theme's format — for the
/// shell's questions about a theme (is its background dark; is a surface
/// distinct from it), so they ask the very measure the theme's own choices
/// are made with.
pub const fn contrast_ratio(a: u32, b: u32) -> f64 {
    contrast(rgb(a), rgb(b))
}

/// The channel steps of the 6×6×6 color cube (the xterm convention).
const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// Converts a cell's color to the screen's color.
///
/// `colors` is the table the application changes with OSC 4/10/11 and its
/// entries can be `None`; on an empty entry we fall back to the theme's
/// palette.
///
/// `#[inline]`: the same hot path as `linear_rgba` and the same reasoning.
/// It is marked **together with** `Theme::default`; if only one were taken the
/// call would shift onto the other.
#[inline]
pub(crate) fn resolve(color: Color, colors: &Colors, theme: &Theme) -> Rgb {
    let index = match color {
        Color::Spec(spec) => return spec,
        Color::Named(named) => named as usize,
        Color::Indexed(index) => index as usize,
    };
    // The table has 269 entries; `NamedColor` is 268 at most, `Indexed` 255 at
    // most.
    colors[index].unwrap_or_else(|| theme.default(index)) // audit: the index is bounded
}

/// Resolves the color **born from the cell's `fg`**, dimness included.
///
/// The single rule of dimness is here, because it is applied in two places:
/// to the foreground in a normal cell, to the background in reverse video
/// (`Session::frame`). Written separately in the two arms, one could change
/// while the other stayed old.
///
/// If the default foreground is dim, the theme's `dim` role is taken **before**
/// resolution — the alacritty application's `DimForeground`. Its consequence: a
/// foreground changed with OSC 10 becomes, in a dim cell, the role itself, not
/// that color's dim (alacritty does the same). Other colors are resolved and
/// blended into the background ([`dim_toward`]).
///
/// `#[inline]`: the same hot path as `resolve`.
#[inline]
pub(crate) fn resolve_fg(color: Color, dim: bool, colors: &Colors, theme: &Theme) -> Rgb {
    match color {
        Color::Named(NamedColor::Foreground) if dim => rgb(theme.dim),
        color if dim => dim_toward(resolve(color, colors, theme), theme.background_rgb()),
        color => resolve(color, colors, theme),
    }
}

const fn rgb(hex: u32) -> Rgb {
    Rgb {
        r: (hex >> 16) as u8,
        g: (hex >> 8) as u8,
        b: hex as u8,
    }
}

/// A role's `0xRRGGBB` as sRGB bytes, for a surface that is not the GPU's: the
/// one rule that unpacks the bytes ([`rgb`]), which
/// [`Theme::background_srgb`] and [`Theme::separator_srgb`] already use. A
/// second unpacking would be a place where a role and its drawn colour can part.
pub(crate) const fn srgb_bytes(hex: u32) -> [u8; 3] {
    let Rgb { r, g, b } = rgb(hex);
    [r, g, b]
}

/// The two ends of the light, for the derived values that lean toward one
/// rather than toward a role (the dock's ground, rim and shadow).
const WHITE: Rgb = rgb(0xffffff);
const BLACK: Rgb = rgb(0x000000);

/// sRGB-encoded 8-bit channel → linear f32. Its source is the transfer
/// function of IEC 61966-2-1 (`c/12.92`, after the knee `((c+0.055)/1.055)^2.4`).
///
/// The table is **not** a hand-inspected list of constants but derived data:
/// `srgb_table_follows_transfer_function` ties every entry to the formula.
/// The reason for a table is `const`ness — `powf` isn't `const` on stable,
/// whereas [`Theme::background_linear`] and [`Theme::accent_linear`] are
/// `const fn`.
// rustfmt opens the table into one line per entry: 64 lines become 256 and
// the rest of the file becomes unreadable. The same reasoning as the ANSI
// table of `Theme::BATERI`.
#[rustfmt::skip]
const SRGB_LINEAR: [f32; 256] = [
    0.0, 0.000303527, 0.000607054, 0.000910581,
    0.001214108, 0.001517635, 0.001821162, 0.0021246888,
    0.002428216, 0.0027317428, 0.00303527, 0.0033465358,
    0.0036765074, 0.004024717, 0.004391442, 0.0047769533,
    0.0051815165, 0.0056053917, 0.006048833, 0.0065120906,
    0.00699541, 0.007499032, 0.008023193, 0.008568126,
    0.009134059, 0.009721218, 0.010329823, 0.010960094,
    0.011612245, 0.012286488, 0.0129830325, 0.013702083,
    0.014443844, 0.015208514, 0.015996294, 0.016807375,
    0.017641954, 0.01850022, 0.019382361, 0.020288562,
    0.02121901, 0.022173885, 0.023153367, 0.024157632,
    0.02518686, 0.026241222, 0.027320892, 0.02842604,
    0.029556835, 0.030713445, 0.031896032, 0.033104766,
    0.034339808, 0.035601314, 0.03688945, 0.038204372,
    0.039546236, 0.0409152, 0.04231141, 0.04373503,
    0.045186203, 0.046665087, 0.048171826, 0.049706567,
    0.051269457, 0.052860647, 0.054480277, 0.05612849,
    0.05780543, 0.059511237, 0.061246052, 0.063010015,
    0.064803265, 0.06662594, 0.06847817, 0.070360094,
    0.07227185, 0.07421357, 0.07618538, 0.07818742,
    0.08021982, 0.08228271, 0.08437621, 0.08650046,
    0.08865558, 0.09084171, 0.093058966, 0.09530747,
    0.09758735, 0.099898726, 0.10224173, 0.104616486,
    0.107023105, 0.10946171, 0.11193243, 0.114435375,
    0.116970666, 0.11953843, 0.122138776, 0.12477182,
    0.12743768, 0.13013647, 0.13286832, 0.13563333,
    0.13843161, 0.14126329, 0.14412847, 0.14702727,
    0.14995979, 0.15292615, 0.15592647, 0.15896083,
    0.16202937, 0.1651322, 0.1682694, 0.17144111,
    0.1746474, 0.17788842, 0.18116425, 0.18447499,
    0.18782078, 0.19120169, 0.19461784, 0.19806932,
    0.20155625, 0.20507874, 0.20863687, 0.21223076,
    0.2158605, 0.2195262, 0.22322796, 0.22696587,
    0.23074006, 0.23455058, 0.23839757, 0.24228112,
    0.24620132, 0.25015828, 0.2541521, 0.25818285,
    0.26225066, 0.2663556, 0.2704978, 0.2746773,
    0.27889428, 0.28314874, 0.28744084, 0.29177064,
    0.29613826, 0.30054379, 0.3049873, 0.30946892,
    0.31398872, 0.31854677, 0.3231432, 0.3277781,
    0.33245152, 0.33716363, 0.34191442, 0.34670407,
    0.3515326, 0.35640013, 0.3613068, 0.3662526,
    0.3712377, 0.37626213, 0.38132602, 0.38642943,
    0.39157248, 0.39675522, 0.40197778, 0.4072402,
    0.4125426, 0.41788507, 0.42326766, 0.4286905,
    0.43415365, 0.43965718, 0.4452012, 0.4507858,
    0.45641103, 0.462077, 0.4677838, 0.47353148,
    0.47932017, 0.48514995, 0.49102086, 0.49693298,
    0.5028865, 0.50888133, 0.5149177, 0.52099556,
    0.5271151, 0.5332764, 0.5394795, 0.54572445,
    0.55201143, 0.5583404, 0.5647115, 0.57112485,
    0.57758045, 0.58407843, 0.59061885, 0.59720176,
    0.60382736, 0.61049557, 0.6172066, 0.6239604,
    0.63075715, 0.63759685, 0.6444797, 0.65140563,
    0.65837485, 0.6653873, 0.67244315, 0.6795425,
    0.6866853, 0.69387174, 0.7011019, 0.70837575,
    0.7156935, 0.7230551, 0.73046076, 0.7379104,
    0.7454042, 0.7529422, 0.7605245, 0.76815116,
    0.7758222, 0.7835378, 0.7912979, 0.7991027,
    0.80695224, 0.8148466, 0.82278574, 0.8307699,
    0.838799, 0.8468732, 0.8549926, 0.8631572,
    0.8713671, 0.8796224, 0.8879231, 0.8962694,
    0.9046612, 0.91309863, 0.92158186, 0.9301109,
    0.9386857, 0.9473065, 0.9559733, 0.9646863,
    0.9734453, 0.9822506, 0.9911021, 1.0,
];

/// Converts a color to the **linear** RGBA the renderer expects.
///
/// The name carries the space because the one thing that can be silently wrong
/// in this repo is which space a float is in: the draw target is
/// `BGRA8Unorm_sRGB` and the hardware treats the fragment output as linear and
/// encodes on write. If `c / 255.0` were returned here the palette would open
/// up (the midtone `0x1a1c21` → the gray `0x5a5d65`); the only guard that sees
/// this is `bt-gpu`'s `cell_bg_paints_pixels_on_the_gpu` test and it sees it
/// only with a **midtone** color.
///
/// The same floats go to `MTLClearColor` too — Metal reads the clear color as
/// linear on an sRGB target as well, so the window ground and the cells are
/// corrected from a single source.
///
/// `#[inline]`: called per cell, per frame (`Session::frame`) and LTO is off
/// (there is no `[profile.release]`); without the mark the body didn't cross
/// the crate boundary — `nm -u` showed an undefined symbol in the release
/// rlib, i.e. a real call remained around a table lookup.
#[inline]
pub(crate) const fn linear_rgba(color: Rgb) -> LinearRgba {
    LinearRgba::from_srgb(color.r, color.g, color.b)
}

/// `0xRRGGBB` → **linear** RGBA.
///
/// Its only caller is the mirror's `#rrggbb`-writing `region_highlight` record
/// ([`crate::shell::HighlightColor::Rgb`]): that color never goes through the
/// palette, the shell says it directly. That it is the same format the palette
/// is written in (`0xRRGGBB`) is no coincidence — the theme's fields are too.
pub(crate) const fn linear_hex(hex: u32) -> LinearRgba {
    linear_rgba(rgb(hex))
}

#[cfg(test)]
mod tests {
    use super::*;

    const THEME: Theme = Theme::BATERI;

    #[test]
    fn a_theme_is_dark_when_white_reads_better_on_it() {
        assert!(Theme::BATERI.is_dark());
        assert!(!Theme::BATERI_LIGHT.is_dark());
        assert!(!Theme::LINEN.is_dark());
    }

    #[test]
    fn the_ground_leans_from_the_accent_to_the_cursor() {
        // Black lifted 8 % toward the foreground, then 38 % toward the accent
        // and 28 % toward the cursor; the light theme takes the smaller steps.
        assert_eq!(
            Theme::BATERI.ground_srgb(),
            [[0x39, 0x46, 0x56], [0x49, 0x3e, 0x29]]
        );
        assert_eq!(
            Theme::BATERI_LIGHT.ground_srgb(),
            [[0xbc, 0xc9, 0xdc], [0xd7, 0xd0, 0xbf]]
        );
    }

    #[test]
    fn the_band_is_one_step_toward_the_foreground_from_the_docks_ground() {
        // The dock's ground lifted 8 % toward the foreground on a dark theme,
        // 4 % on a light one.
        assert_eq!(Theme::BATERI.band_srgb(), [0x21, 0x21, 0x22]);
        assert_eq!(Theme::LINEN.band_srgb(), [0xf2, 0xf1, 0xec]);
        // The GPU's value is the same colour, not a second computation.
        for theme in [Theme::BATERI, Theme::BATERI_LIGHT, Theme::LINEN] {
            let [r, g, b] = theme.band_srgb();
            assert_eq!(theme.band_linear(), LinearRgba::from_srgb(r, g, b));
            assert_eq!(
                theme.band_srgb(),
                {
                    let Rgb { r, g, b } = theme.step(theme.dock_ground_rgb());
                    [r, g, b]
                },
                "one rule for a step above"
            );
            assert_ne!(
                theme.band_linear(),
                theme.dock_ground_linear(),
                "the folder row must be told apart from the input row"
            );
        }
    }

    #[test]
    fn a_raised_surface_is_the_background_stepped_that_many_times() {
        for theme in [Theme::BATERI, Theme::BATERI_LIGHT, Theme::LINEN] {
            assert_eq!(theme.raised_linear(0), theme.background_linear());
            let once = theme.step(rgb(theme.background));
            let twice = theme.step(once);
            assert_eq!(
                theme.raised_linear(1),
                linear_rgba(once),
                "one step is the rule's"
            );
            assert_eq!(theme.raised_linear(2), linear_rgba(twice));
            assert_ne!(theme.raised_linear(1), theme.raised_linear(2));
        }
        // Black lifted 8 % toward the foreground, once and again.
        let [r, g, b] = Theme::BATERI.dock_ground_srgb();
        assert_eq!(
            Theme::BATERI.raised_linear(1),
            LinearRgba::from_srgb(r, g, b),
            "on pure black the first step is the dock's ground"
        );
    }

    #[test]
    fn the_docks_ground_is_the_background_lifted_toward_the_light() {
        // Black lifted 8 % toward the foreground (the step every surface
        // above the panes takes); Linen 60 % toward white.
        assert_eq!(Theme::BATERI.dock_ground_srgb(), [0x11, 0x11, 0x12]);
        assert_eq!(Theme::LINEN.dock_ground_srgb(), [0xfa, 0xf9, 0xf5]);
        for (_, _, theme) in EMBEDDED {
            let [r, g, b] = theme.dock_ground_srgb();
            assert_eq!(theme.dock_ground_linear(), LinearRgba::from_srgb(r, g, b));
            // Never darker than the content: the plate catches more light
            // than what it casts its shadow on. Where it is lifted its rim is
            // lighter still; where it cannot be (a pure white background)
            // the shadow carries the edge; on pure black the shadow is the
            // content's own colour and the rim carries it.
            let (ground, content) = (
                theme.dock_ground_linear().luminance(),
                theme.background_linear().luminance(),
            );
            assert!(ground >= content, "{theme:?}");
            if ground > content {
                assert!(theme.dock_rim_linear().luminance() > ground, "{theme:?}");
            } else {
                assert!(
                    theme.dock_shadow_linear().luminance() < content,
                    "{theme:?}"
                );
            }
            assert!(
                theme.dock_shadow_linear().luminance() <= content,
                "{theme:?}"
            );
        }
        // Pure white has no lighter to go to: the plate is the background and
        // its shadow carries the edge.
        let white = Theme {
            background: 0xffffff,
            ..Theme::LINEN
        };
        assert_eq!(white.dock_ground_srgb(), [0xff, 0xff, 0xff]);
    }

    #[test]
    fn the_docks_depth_follows_the_kind_of_theme() {
        // The shadow is the theme's dark end; how strongly it is drawn is the
        // light/dark answer, not a colour with an alpha.
        assert_eq!(
            Theme::BATERI.dock_shadow_linear(),
            LinearRgba::from_srgb(0, 0, 0)
        );
        assert_eq!(
            Theme::LINEN.dock_shadow_linear(),
            linear_hex(Theme::LINEN.foreground)
        );
        let (dark_shadow, dark_glow) = Theme::BATERI.dock_depth();
        let (light_shadow, light_glow) = Theme::LINEN.dock_depth();
        assert!(dark_shadow > light_shadow && dark_glow > light_glow);
        for strength in [dark_shadow, dark_glow, light_shadow, light_glow] {
            assert!(strength > 0.0 && strength <= 1.0);
        }
    }

    #[test]
    fn the_split_grounds_base_is_one_step_above_the_panes() {
        // One copy of "a step above": with nothing to lean toward (accent and
        // cursor are the step itself) the ground is the background a step
        // toward the foreground.
        for theme in [Theme::BATERI, Theme::LINEN] {
            let Rgb { r, g, b } = theme.step(theme.background_rgb());
            let flat = Theme {
                accent: u32::from_be_bytes([0, r, g, b]),
                cursor: u32::from_be_bytes([0, r, g, b]),
                ..theme
            };
            assert_eq!(flat.ground_srgb(), [[r, g, b]; 2]);
        }
        assert_eq!(
            {
                let Rgb { r, g, b } = Theme::LINEN.step(Theme::LINEN.background_rgb());
                [r, g, b]
            },
            [0xeb, 0xe7, 0xdf]
        );
    }

    #[test]
    fn the_ground_follows_the_theme_it_is_drawn_from() {
        // Derived, not stored: another accent moves the top left and nothing
        // else, another cursor the bottom right.
        let accent = Theme {
            accent: 0xff0000,
            ..Theme::BATERI
        };
        assert_ne!(accent.ground_srgb()[0], Theme::BATERI.ground_srgb()[0]);
        assert_eq!(accent.ground_srgb()[1], Theme::BATERI.ground_srgb()[1]);
        let cursor = Theme {
            cursor: 0x00ff00,
            ..Theme::BATERI
        };
        assert_eq!(cursor.ground_srgb()[0], Theme::BATERI.ground_srgb()[0]);
        assert_ne!(cursor.ground_srgb()[1], Theme::BATERI.ground_srgb()[1]);
    }

    #[test]
    fn default_background_has_one_source() {
        // The cell's `Named(Background)`, `frame()`'s skip decision and the
        // window's clear color must come from the **same** field of the theme;
        // if they part ways, empty cells are painted differently from the window.
        let background = rgb(THEME.background);
        assert_eq!(THEME.default(NamedColor::Background as usize), background);
        assert_eq!(THEME.background_rgb(), background);
        assert_eq!(THEME.background_linear(), linear_rgba(background));
        // The cursor too from its **own** role: so the answer to the color
        // question and the drawn block don't drift apart. 258 was an alias to
        // `accent` once.
        assert_eq!(
            THEME.default(NamedColor::Cursor as usize),
            rgb(THEME.cursor)
        );
        assert_eq!(THEME.accent_linear(), linear_rgba(rgb(THEME.accent)));
    }

    #[test]
    fn separator_has_one_source() {
        // The splits' separator (sRGB, `NSColor`) and the dock's hairlines
        // (linear, GPU) come from the same value: if they part ways, the split
        // line sits in a different tone than the dock's line.
        for (_, _, theme) in EMBEDDED {
            let [r, g, b] = theme.separator_srgb();
            let hex = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
            assert_eq!(linear_hex(hex), theme.separator_linear());
        }
    }

    #[test]
    fn bateri_palette_is_pinned() {
        // The palette's 19 values are tied to a **hand-written** list: an
        // expectation computed from the table would itself carry a typo that
        // slipped into the table. Whoever changes the values on purpose changes
        // this list too.
        #[rustfmt::skip]
        const EXPECTED: [(usize, u32); 19] = [
            (0, 0x22252b), (1, 0xd16d6a), (2, 0x8bb58b), (3, 0xd6b16a),
            (4, 0x7a9cc6), (5, 0xb08ec0), (6, 0x79b3b3), (7, 0xc8c9cc),
            (8, 0x4a4e57), (9, 0xe58b88), (10, 0xa4cba4), (11, 0xe8c988),
            (12, 0x9bb8dc), (13, 0xc9aad8), (14, 0x96caca), (15, 0xe6e7ea),
            (256, 0xd8d9dd), // foreground
            (257, 0x000000), // background
            (258, 0xd9b063), // cursor
        ];
        for (index, hex) in EXPECTED {
            assert_eq!(THEME.default(index), rgb(hex), "{index}");
        }
    }

    #[test]
    fn bateri_light_palette_is_pinned() {
        // The light-theme counterpart of `bateri_palette_is_pinned`, same
        // reasoning; the dim foreground role is in the list too (268), because
        // it was hand-picked as well.
        #[rustfmt::skip]
        const EXPECTED: [(usize, u32); 20] = [
            (0, 0x2b2e35), (1, 0xb5423d), (2, 0x3b7a3b), (3, 0x8f6a00),
            (4, 0x3a66a6), (5, 0x8a4c9c), (6, 0x23787f), (7, 0xb9bbc1),
            (8, 0x70737b), (9, 0xc9504a), (10, 0x4a8f4a), (11, 0xa67c00),
            (12, 0x4a78ba), (13, 0x9d5db0), (14, 0x2f8a92), (15, 0xdcdee3),
            (256, 0x24262c), // foreground
            (257, 0xf5f6f8), // background
            (258, 0x8a6512), // cursor
            (268, 0x696b70), // dim foreground
        ];
        let light = Theme::BATERI_LIGHT;
        for (index, hex) in EXPECTED {
            assert_eq!(light.default(index), rgb(hex), "{index}");
        }
        // Bright white is distinct from the background: `\e[107m` is a visible
        // block.
        assert_ne!(light.ansi[15], light.background);
        // The `dim` role comes from the rule itself: the foreground blended
        // into the background.
        assert_eq!(
            rgb(light.dim),
            dim_toward(rgb(light.foreground), light.background_rgb())
        );
    }

    #[test]
    fn linen_palette_is_pinned() {
        // The warm light theme's counterpart of `bateri_light_palette_is_pinned`,
        // same reasoning.
        #[rustfmt::skip]
        const EXPECTED: [(usize, u32); 20] = [
            (0, 0x2f2a25), (1, 0xb23a32), (2, 0x4f7a32), (3, 0x8a6500),
            (4, 0x3b6488), (5, 0x8d4f7f), (6, 0x2d7672), (7, 0xbdb5a8),
            (8, 0x766d63), (9, 0xc8503f), (10, 0x578535), (11, 0x977200),
            (12, 0x4a76a0), (13, 0xa0608f), (14, 0x33847b), (15, 0xddd6ca),
            (256, 0x2a2520), // foreground
            (257, 0xf3efe7), // background
            (258, 0xa95200), // cursor
            (268, 0x6d6862), // dim foreground
        ];
        let linen = Theme::LINEN;
        for (index, hex) in EXPECTED {
            assert_eq!(linen.default(index), rgb(hex), "{index}");
        }
        assert_ne!(linen.ansi[15], linen.background);
        assert_eq!(
            rgb(linen.dim),
            dim_toward(rgb(linen.foreground), linen.background_rgb())
        );
    }

    #[test]
    fn ultraviolet_palette_is_pinned() {
        // The violet theme's counterpart of `bateri_palette_is_pinned`, same
        // reasoning; its other roles come from the adapted palettes' rule.
        #[rustfmt::skip]
        const EXPECTED: [(usize, u32); 19] = [
            (0, 0x2e2556), (1, 0xff6e8f), (2, 0x5cf2a6), (3, 0xffd23f),
            (4, 0x8796ff), (5, 0xd98aff), (6, 0x4fe3ff), (7, 0xcfc6ea),
            (8, 0x9d93cc), (9, 0xff94ad), (10, 0x8dffc4), (11, 0xffe57a),
            (12, 0xadb7ff), (13, 0xe9b0ff), (14, 0x8bedff), (15, 0xffffff),
            (256, 0xece6ff), // foreground
            (257, 0x1b1433), // background
            (258, 0xffb454), // cursor
        ];
        let violet = Theme::ULTRAVIOLET;
        for (index, hex) in EXPECTED {
            assert_eq!(violet.default(index), rgb(hex), "{index}");
        }
        assert!(violet.is_dark());
        assert_eq!(violet.selection, 0x3d3272, "its own selection is kept");
        // Every coloured entry reads on the ground with room for the highlights.
        for color in violet.ansi.iter().skip(1) {
            assert!(contrast(*color, violet.background) >= 6.2, "#{color:06x}");
        }
        assert!(
            contrast(violet.ansi[0], violet.background) > 1.2,
            "black is a block"
        );
    }

    #[test]
    fn embedded_themes_are_found_by_name() {
        assert_eq!(Theme::embedded("bateri"), Some(Theme::BATERI));
        assert_eq!(Theme::embedded("bateri-light"), Some(Theme::BATERI_LIGHT));
        assert_eq!(Theme::embedded("linen"), Some(Theme::LINEN));
        assert_eq!(
            Theme::embedded("Bateri"),
            None,
            "the name is case-sensitive"
        );
        assert_eq!(Theme::embedded(""), None);
        let mocha = Theme::embedded("catppuccin-mocha").expect("an adapted theme");
        assert_eq!((mocha.background, mocha.foreground), (0x1e1e2e, 0xcdd6f4));
        assert_eq!(
            Theme::embedded_title("catppuccin-mocha"),
            Some("Catppuccin Mocha")
        );
        assert_eq!(Theme::embedded_title("bateri"), Some("bateri"));
        assert_eq!(Theme::embedded_title("paper"), None);
    }

    #[test]
    fn an_adapted_theme_keeps_the_schemes_own_colours() {
        // The palette people know is the scheme's: its ANSI colours,
        // background and foreground arrive untouched, and the roles that
        // stand for a palette colour are that colour.
        for scheme in &schemes::SCHEMES {
            let theme = Theme::embedded(scheme.name).expect("in the table");
            assert_eq!(theme, Theme::adapted(scheme), "{}", scheme.name);
            assert_eq!(
                (theme.background, theme.foreground, theme.ansi),
                (scheme.background, scheme.foreground, scheme.ansi),
                "{}",
                scheme.name
            );
            assert_eq!(
                (theme.accent, theme.success, theme.error),
                (scheme.ansi[4], scheme.ansi[2], scheme.ansi[1]),
                "{}",
                scheme.name
            );
            assert_eq!(
                rgb(theme.dim),
                dim_toward(rgb(theme.foreground), theme.background_rgb()),
                "{}",
                scheme.name
            );
        }
    }

    #[test]
    fn an_adapted_theme_moves_a_role_only_as_far_as_it_must() {
        // The cursor, `info` and `warning` read on the background (the
        // letter under the cursor is drawn in the background colour); one
        // that already did is the scheme's colour itself.
        for scheme in &schemes::SCHEMES {
            let theme = Theme::adapted(scheme);
            for (role, source) in [
                (theme.cursor, scheme.cursor),
                (theme.info, scheme.ansi[6]),
                (theme.warning, scheme.ansi[3]),
            ] {
                let ratio = contrast(role, theme.background);
                assert!(
                    ratio >= MINIMUM_CONTRAST,
                    "{}: #{role:06x} {ratio:.2}",
                    scheme.name
                );
                if contrast(source, theme.background) >= MINIMUM_CONTRAST {
                    assert_eq!(role, source, "{}", scheme.name);
                }
            }
        }
        // A light scheme's yellow on its cream (2.3:1) becomes the darker
        // yellow that reads — the hue kept, not greyed toward the foreground.
        let latte = Theme::embedded("catppuccin-latte").expect("in the table");
        let warning = rgb(latte.warning);
        assert!(
            warning.r > warning.g && warning.g > warning.b,
            "{warning:?}"
        );
    }

    #[test]
    fn an_adapted_highlight_stands_out_like_the_reference_ones() {
        // The selection and the two search highlights stand apart from the
        // background, and the derived ones as far as bateri's own theme of
        // the same side does (the reference).
        for scheme in &schemes::SCHEMES {
            let theme = Theme::adapted(scheme);
            let reference = if theme.is_dark() {
                Theme::BATERI
            } else {
                Theme::BATERI_LIGHT
            };
            let apart = |role: u32, theme: &Theme| contrast(role, theme.background);
            assert!(apart(theme.selection, &theme) > 1.0, "{}", scheme.name);
            for (role, target) in [
                (theme.search_match, reference.search_match),
                (theme.search_current, reference.search_current),
            ] {
                assert!(
                    apart(role, &theme) >= apart(target, &reference),
                    "{}: #{role:06x}",
                    scheme.name
                );
            }
        }
        // A scheme that gives the foreground as its selection (for terminals
        // that swap the selected text's colours) gets a highlight instead.
        let github = Theme::embedded("github-light").expect("in the table");
        assert_ne!(github.selection, github.foreground);
        assert!(contrast(github.selection, github.background) < 1.5);
        // A scheme's own highlight is kept: Dracula's current-line grey.
        let dracula = Theme::embedded("dracula").expect("in the table");
        assert_eq!(dracula.selection, 0x44475a);
    }

    #[test]
    fn the_embedded_groups_are_the_whole_table() {
        let [own, dark, light] = Theme::embedded_groups();
        assert_eq!(own, ["bateri", "bateri-light", "linen", "ultraviolet"]);
        assert!(
            dark.iter()
                .all(|name| Theme::embedded(name).is_some_and(|t| t.is_dark()))
        );
        assert!(
            light
                .iter()
                .all(|name| Theme::embedded(name).is_some_and(|t| !t.is_dark()))
        );
        let grouped: Vec<_> = own.iter().chain(&dark).chain(&light).copied().collect();
        let mut all: Vec<_> = Theme::embedded_names().collect();
        assert_eq!(grouped.len(), all.len());
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), grouped.len(), "a name is in the table twice");
        // Each adapted group in title order: the menus list it as it comes.
        for group in [&dark, &light] {
            let titles: Vec<_> = group
                .iter()
                .map(|name| Theme::embedded_title(name).expect("titled").to_lowercase())
                .collect();
            assert!(titles.is_sorted(), "{titles:?}");
        }
        // A name is what the settings file takes and a file name: lowercase,
        // no space.
        for name in grouped {
            assert!(
                name.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
                "{name}"
            );
        }
    }

    #[test]
    fn text_on_a_highlight_reaches_the_floor_on_every_embedded_theme() {
        // The guard the adapted palettes rely on: whatever text colour sits
        // under the selection or a search match, the drawn ink reaches the
        // floor against that highlight (`session::cell_style`).
        for (name, _, theme) in EMBEDDED {
            for highlight in [theme.selection, theme.search_match, theme.search_current] {
                let ground = linear_rgba(rgb(highlight));
                for text in [theme.foreground, theme.dim].into_iter().chain(theme.ansi) {
                    let ink = linear_rgba(rgb(text)).legible_on(ground, MINIMUM_CONTRAST);
                    assert!(
                        apart(ink, ground) >= MINIMUM_CONTRAST - 1e-4,
                        "{name}: #{text:06x} on #{highlight:06x}"
                    );
                }
            }
        }
    }

    #[test]
    fn srgb_table_follows_transfer_function() {
        // The table is not 256 hand-written numbers but the formula frozen. The
        // reference is computed in f64; since the table is f32 the epsilon only
        // covers f32 rounding (absolute error ≤ ~6e-8).
        assert_eq!(SRGB_LINEAR[0], 0.0);
        assert_eq!(SRGB_LINEAR[255], 1.0, "white must stay 1.0 in linear too");
        for (i, &linear) in SRGB_LINEAR.iter().enumerate() {
            let c = i as f64 / 255.0;
            let expected = if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            };
            assert!(
                (f64::from(linear) - expected).abs() < 1e-7,
                "{i}: {linear} != {expected}"
            );
            // Proof that the table really **linearizes**, without the GPU: a
            // linear value is strictly smaller than its sRGB-encoded form
            // (`i/255`). Had `c / 255.0` been put in place of the table, this
            // line would fail. The ends (0 and 255) are the transfer function's
            // fixed points, equality comes from there and they are left outside
            // the range.
            if (1..255).contains(&i) {
                assert!(f64::from(linear) < c, "{i}: {linear} !< {c}");
            }
        }
    }

    #[test]
    fn palette_indices_follow_xterm() {
        assert_eq!(THEME.default(1), rgb(THEME.ansi[1]));
        // 16 = the cube's start (0,0,0), 231 = its end (255,255,255).
        assert_eq!(THEME.default(16), rgb(0x000000));
        assert_eq!(THEME.default(231), rgb(0xffffff));
        // The gray ramp starts at 8 and goes up by 10.
        assert_eq!(THEME.default(232), rgb(0x080808));
        assert_eq!(THEME.default(255), rgb(0xeeeeee));
    }

    #[test]
    fn dim_colors_move_toward_the_background() {
        // The rule's real invariant: a dim color sits between its source and the
        // background — channel by channel. On the dark theme that means "darkens",
        // on the light ones "lightens"; every embedded background is tested so
        // that a light theme fails if the rule goes back to multiplying toward
        // black.
        for (_, _, theme) in EMBEDDED {
            let bg = theme.background_rgb();
            for index in 259..=266 {
                let (source, dimmed) = (theme.default(index - 259), theme.default(index));
                for (s, d, b) in [
                    (source.r, dimmed.r, bg.r),
                    (source.g, dimmed.g, bg.g),
                    (source.b, dimmed.b, bg.b),
                ] {
                    assert!(
                        s.min(b) <= d && d <= s.max(b),
                        "{index}: {source:?} → {dimmed:?}"
                    );
                }
            }
        }
        let sum = |c: Rgb| u32::from(c.r) + u32::from(c.g) + u32::from(c.b);
        // On the dark theme red's dim darkens, on the light theme it lightens.
        assert!(sum(Theme::BATERI.default(260)) < sum(Theme::BATERI.default(1)));
        assert!(sum(Theme::BATERI_LIGHT.default(260)) > sum(Theme::BATERI_LIGHT.default(1)));
        // Hand-written: `(2·0xd1 + 0) / 3`, `(2·0x6d + 0) / 3`, `(2·0x6a + 0) / 3`,
        // with integer division. Because the background is **pure black** the
        // blend term drops out and dimming reduces to vte's `× 2/3` — the same
        // identity is tied for all channel values by [`dim_on_black_is_vte`].
        assert_eq!(THEME.default(260), rgb(0x8b4846));
    }

    #[test]
    fn dim_on_black_is_vte() {
        // The ratio's rationale (`dim_toward`'s doc): on a black background the
        // rule is bit-for-bit identical to vte's `× 2/3`. Every channel value is
        // tested; since `f32`'s `2/3` is slightly above the exact value the
        // truncation lands in the same place.
        let black = rgb(0x000000);
        for c in 0..=255u8 {
            let color = Rgb { r: c, g: c, b: c };
            assert_eq!(dim_toward(color, black), color * (2.0 / 3.0), "{c}");
        }
        // `BATERI`'s `dim` role is the original calculation frozen.
        assert_eq!(rgb(THEME.dim), dim_toward(rgb(THEME.foreground), black));
    }

    #[test]
    fn dim_default_foreground_takes_the_role() {
        // The role comes **before** resolution: even if the foreground was
        // changed in the table (OSC 10), the dim default foreground is the
        // theme's `dim`. The role was deliberately chosen distinct from the
        // foreground's `× 2/3` so that the test notices if the two paths get mixed.
        let theme = Theme {
            dim: 0x123456,
            ..THEME
        };
        let mut colors = Colors::default();
        colors[NamedColor::Foreground] = Some(rgb(0xffffff));
        let foreground = Color::Named(NamedColor::Foreground);
        assert_eq!(resolve_fg(foreground, true, &colors, &theme), rgb(0x123456));
        // A non-dim foreground reads the table.
        assert_eq!(
            resolve_fg(foreground, false, &colors, &theme),
            rgb(0xffffff)
        );
        // A named color is resolved and blended into the background; the role
        // doesn't touch it.
        let red = Color::Named(NamedColor::Red);
        assert_eq!(
            resolve_fg(red, true, &colors, &theme),
            dim_toward(rgb(THEME.ansi[1]), THEME.background_rgb())
        );
    }

    #[test]
    fn osc_table_overrides_palette() {
        let mut colors = Colors::default();
        let custom = rgb(0x010203);
        colors[1] = Some(custom);
        let red = Color::Named(NamedColor::Red);
        assert_eq!(resolve(red, &colors, &THEME), custom);
        assert_eq!(resolve(Color::Indexed(1), &colors, &THEME), custom);
        // A color given directly never asks the table.
        let green = rgb(THEME.ansi[2]);
        assert_eq!(resolve(Color::Spec(green), &colors, &THEME), green);
        // An entry not in the table comes from the theme.
        assert_eq!(resolve(Color::Indexed(2), &colors, &THEME), green);
    }

    /// WCAG contrast ratio between two `0xRRGGBB` — the production measure
    /// the theme's own choices are made with, not a second copy of it.
    fn contrast(a: u32, b: u32) -> f64 {
        contrast_ratio(a, b)
    }

    #[test]
    fn luminance_follows_the_transfer_function() {
        // The table path gives what the formula gives: the guards below (and
        // their margins of a hundredth) were tuned against the formula.
        for hex in [0x000000, 0xffffff, 0x3a3212, 0xd6b16a, 0x8f6a00, 0xf5f6f8] {
            let formula = |shift: u32| {
                let c = f64::from((hex >> shift) & 0xff) / 255.0;
                if c <= 0.04045 {
                    c / 12.92
                } else {
                    ((c + 0.055) / 1.055).powf(2.4)
                }
            };
            let expected = 0.2126 * formula(16) + 0.7152 * formula(8) + 0.0722 * formula(0);
            let got = luminance(rgb(hex));
            assert!(
                (got - expected).abs() < 1e-6,
                "#{hex:06x}: {got} ≠ {expected}"
            );
            // A linear color weighs the same, to `f32`'s precision.
            let [r, g, b] = [16, 8, 0].map(|shift| ((hex >> shift) & 0xff) as u8);
            let linear = f64::from(LinearRgba::from_srgb(r, g, b).luminance());
            assert!((linear - expected).abs() < 1e-6, "#{hex:06x}: {linear}");
        }
        assert!((contrast(0xffffff, 0x000000) - 21.0).abs() < 1e-6);
        assert_eq!(contrast(0xd6b16a, 0x000000), contrast(0x000000, 0xd6b16a));
    }

    #[test]
    fn search_marks_read_on_the_ground() {
        // The current match's mark is the brighter of the two yellows on the
        // dark theme and `warning` itself on the light ones — the approved
        // design's values — and reads on the background like text (3:1).
        // A match's mark is quieter than it and still clear of the background
        // (2:1); it is opaque, so it hides the thumb it sits over.
        let hex = |color: Rgb| {
            (u32::from(color.r) << 16) | (u32::from(color.g) << 8) | u32::from(color.b)
        };
        for (theme, expected) in [
            (Theme::BATERI, 0xe8c988),
            (Theme::BATERI_LIGHT, 0x8f6a00),
            (Theme::LINEN, 0x8a6500),
        ] {
            let current = hex(theme.search_current_mark_rgb());
            assert_eq!(current, expected, "the current mark");
            assert_eq!(
                theme.search_current_mark_linear(),
                linear_rgba(rgb(expected))
            );
            let matched = hex(theme.search_mark_rgb());
            assert_eq!(theme.search_mark_linear(), linear_rgba(rgb(matched)));
            let (current, matched) = (
                contrast(current, theme.background),
                contrast(matched, theme.background),
            );
            assert!(
                current >= 3.0,
                "the current mark on the ground: {current:.2}"
            );
            assert!(
                matched >= 2.0 && matched < current,
                "a match's mark: {matched:.2} (current {current:.2})"
            );
        }
        // A bright yellow that is not the brighter mark is not taken: a theme
        // whose `bright_yellow` sinks into the ground keeps `warning`.
        let mut theme = Theme::BATERI;
        theme.ansi[11] = 0x101010;
        assert_eq!(hex(theme.search_current_mark_rgb()), theme.warning);
    }

    #[test]
    fn the_info_role_reads_on_the_ground() {
        // The host is **text** in the context line, so the criterion is
        // text's — 3:1 on the background. `warning` is in the
        // same place (the host marked staging) with the same criterion.
        for (_, _, theme) in EMBEDDED {
            for role in [theme.info, theme.warning] {
                let ratio = contrast(role, theme.background);
                assert!(ratio >= 3.0, "#{role:06x} on the background {ratio:.2}");
            }
        }
    }

    #[test]
    fn a_host_mark_takes_its_role_color() {
        // Meaning → role; a direct color is linearized from sRGB.
        let theme = Theme::BATERI;
        assert_eq!(
            theme.mark_linear(HostMark::Production),
            theme.error_linear()
        );
        assert_eq!(theme.mark_linear(HostMark::Staging), theme.warning_linear());
        assert_eq!(
            theme.mark_linear(HostMark::Development),
            theme.success_linear()
        );
        assert_eq!(theme.mark_linear(HostMark::None), theme.info_linear());
        assert_eq!(
            theme.mark_linear(HostMark::Rgb(0xc678dd)),
            LinearRgba::from_srgb(0xc6, 0x78, 0xdd)
        );
        // The tab's dot takes sRGB from the same mapping.
        assert_eq!(theme.mark_rgb(HostMark::Production), theme.error);
        assert_eq!(theme.mark_rgb(HostMark::Rgb(0xc678dd)), 0xc678dd);
    }

    #[test]
    fn search_highlights_keep_every_readable_text_readable() {
        // bateri's own themes were made to the selection's criterion for the
        // two search highlights: every text color (foreground, `dim`, 16
        // ANSI) that exceeds 3:1 on the background also exceeds 3:1 on the
        // highlight, so the text reads without being moved. An adapted
        // palette is not held to it — many put a text colour right at 3:1 on
        // their background, which leaves no room for a visible highlight — and
        // its text under a highlight is held to the floor when drawn instead
        // (`text_on_a_highlight_reaches_the_floor_on_every_embedded_theme`).
        //
        // On every theme the current match must be **more distinct** than the
        // other and the role values must say so in brightness too — a
        // distinction left to hue alone would vanish when it fades in an
        // unfocused window.
        for (name, _, theme) in EMBEDDED {
            let own = OWN.iter().any(|(own, _, _)| *own == name);
            let texts = [theme.foreground, theme.dim]
                .into_iter()
                .chain(theme.ansi)
                .filter(|&text| own && contrast(text, theme.background) >= 3.0);
            for text in texts {
                for highlight in [theme.search_match, theme.search_current] {
                    let ratio = contrast(text, highlight);
                    assert!(
                        ratio >= 3.0,
                        "#{text:06x} text on #{highlight:06x} highlight {ratio:.2}"
                    );
                }
            }
            let (matched, current) = (
                contrast(theme.search_match, theme.background),
                contrast(theme.search_current, theme.background),
            );
            assert!(
                matched > 1.0 && current > matched,
                "{name}: the current match isn't more distinct than the other: \
                 {matched:.2} / {current:.2}"
            );
        }
    }

    /// How far apart two drawn colours stand — [`ratio`] on their linear
    /// channels, at `f64` (the precision `legible_on` solves in).
    fn apart(a: LinearRgba, b: LinearRgba) -> f64 {
        let lum = |c: LinearRgba| {
            let [r, g, b, _] = c.to_array();
            weigh(f64::from(r), f64::from(g), f64::from(b))
        };
        ratio(lum(a), lum(b))
    }

    fn linear(hex: u32) -> LinearRgba {
        linear_hex(hex)
    }

    #[test]
    fn legible_ink_is_left_alone() {
        // Already apart, or the rule off: the very colour comes back.
        let light = Theme::BATERI_LIGHT;
        let ground = light.background_linear();
        let ink = light.foreground_linear();
        assert_eq!(ink.legible_on(ground, 3.0), ink);
        let white = linear(0xffffff);
        assert_eq!(white.legible_on(ground, 1.0), white);
        assert_eq!(white.legible_on(ground, 0.5), white);
    }

    #[test]
    fn white_on_a_light_ground_darkens_to_the_floor() {
        // The case that asked for the rule: a dark-theme program's white text
        // (truecolor, past the palette) on the light theme, 1.08:1. White has
        // no lighter side, so it crosses over and lands on the floor exactly.
        let ground = Theme::BATERI_LIGHT.background_linear();
        let white = linear(0xffffff);
        assert!(apart(white, ground) < 1.1);
        for floor in [3.0, 4.5, 7.0] {
            let lifted = white.legible_on(ground, floor);
            let got = apart(lifted, ground);
            assert!((got - floor).abs() < 1e-4, "{floor}: {got}");
            assert!(lifted.luminance() < ground.luminance(), "{lifted:?}");
        }
    }

    #[test]
    fn ink_keeps_its_own_side_while_that_side_reaches() {
        // A dark ink on a dark ground lightens only when darker cannot reach:
        // ANSI black on black (1.37) has nothing darker to go to.
        let black = Theme::BATERI.background_linear();
        let ansi_black = linear(Theme::BATERI.ansi[0]);
        let lifted = ansi_black.legible_on(black, 3.0);
        assert!(lifted.luminance() > ansi_black.luminance(), "{lifted:?}");
        assert!((apart(lifted, black) - 3.0).abs() < 1e-4);
        // A grey darker than a light ground darkens, a grey lighter than a dark
        // ground lightens — neither flips over the ground.
        let paper = Theme::BATERI_LIGHT.background_linear();
        let grey = linear(0xb0b0b0);
        assert!(grey.legible_on(paper, 3.0).luminance() < grey.luminance());
        let ink = linear(0x3a3a3a);
        assert!(ink.legible_on(black, 3.0).luminance() > ink.luminance());
    }

    #[test]
    fn darkening_keeps_the_hue() {
        // Toward black every channel scales by one factor: the ratios between
        // them, i.e. the hue, stay.
        let ground = Theme::BATERI_LIGHT.background_linear();
        let pale = linear(0xb1b9f9); // a dark theme's suggestion blue
        let [r, g, b, a] = pale.to_array();
        let [r2, g2, b2, a2] = pale.legible_on(ground, 3.0).to_array();
        assert!((r2 / r - b2 / b).abs() < 1e-5 && (g2 / g - b2 / b).abs() < 1e-5);
        assert!(r2 < r && a2 == a);
    }

    #[test]
    fn an_unreachable_floor_takes_the_end_that_stands_further() {
        // A mid grey stands below 21:1 from both ends; the ink goes all the
        // way to the further one rather than stopping short.
        let ground = linear(0x777777);
        let ink = linear(0x808080);
        let lifted = ink.legible_on(ground, 21.0);
        let (to_black, to_white) = (
            apart(linear(0x000000), ground),
            apart(linear(0xffffff), ground),
        );
        assert!((apart(lifted, ground) - to_black.max(to_white)).abs() < 1e-4);
    }

    #[test]
    fn every_palette_colour_reaches_the_floor_on_every_embedded_theme() {
        // The property behind the default: whatever a program picks from the
        // palette — the 256 colours and the named ones' dim — reads at 3:1 on
        // the background, and a colour that already did is not touched.
        for (name, _, theme) in EMBEDDED {
            let ground = theme.background_linear();
            let colours = (0..256).map(|index| theme.default(index)).chain(
                (0..16).map(|index| dim_toward(rgb(theme.ansi[index]), rgb(theme.background))),
            );
            for colour in colours {
                let ink = linear_rgba(colour);
                let lifted = ink.legible_on(ground, 3.0);
                assert!(apart(lifted, ground) >= 3.0 - 1e-4, "{name}: {colour:?}");
                if apart(ink, ground) >= 3.0 {
                    assert_eq!(lifted, ink, "{name}: {colour:?}");
                }
            }
        }
    }

    #[test]
    fn pictures_are_not_text() {
        for c in [
            '─',
            '│',
            '╭',
            '▀',
            '█',
            '░',
            '⠿',
            '\u{1fb00}',
            '\u{e0b0}',
            '\u{e0b6}',
        ] {
            assert!(is_drawing(c), "{c:?}");
        }
        for c in ['a', 'W', '→', '✻', '·', '…', ' ', 'ş'] {
            assert!(!is_drawing(c), "{c:?}");
        }
    }
    #[test]
    fn a_colour_made_from_srgb_is_the_same_srgb_again() {
        for hex in [0x000000_u32, 0xffffff, 0x1a1c21, 0x5a5d65, 0xe8a33d] {
            let [r, g, b] = [(hex >> 16) as u8, (hex >> 8) as u8, hex as u8];
            let back = LinearRgba::from_srgb(r, g, b).to_srgb();
            for (channel, want) in back.iter().zip([r, g, b]) {
                assert_eq!((channel * 255.0).round() as u8, want, "{hex:06x}");
            }
        }
    }
}
