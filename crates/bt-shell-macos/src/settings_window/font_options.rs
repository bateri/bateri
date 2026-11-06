//! The font options of the Font page: what the selected font offers, laid out for choosing.
//!
//! The list is the font's own ([`bt_shell_common::font_options`], the palette reads the same
//! one). Three options people look for by what they do — Ligatures, Hide Secrets and Slashed
//! Zero — are **rows**: a switch, what it does, and an example of a text before and after it. The
//! font's other stylistic sets and character variants are **tiles** of letters, four to a line:
//! the letters the option changes drawn before and after it, and under them the name the font
//! gives it (or its tag), with the full name and the tag in the tooltip. Eight tiles show, the
//! rest after "Show All"; a tag the file asks for that the font does not list is said in orange
//! under them. A font that offers none (Menlo) hides the group — the window does, from the
//! options it asks for — and changing the font rebuilds everything, so what is on screen is always
//! the selected font's.
//!
//! The examples are text in the selected font with the option's tags set on and off
//! (CoreText's own feature settings through the font descriptor), not a picture per tile.
//!
//! Nothing here keeps a choice of its own: a switch or a tile shows the file's, a click writes the
//! option's edits, and the state comes back from the file like every other control of the window.
//! A refresh that finds the same options only sets the state, so the control being clicked is not
//! replaced under the pointer; the controls are rebuilt only when the list of options (or the
//! font, or "Show All") changes.

use std::cell::{Cell, RefCell};

use bt_core::FontOptions;
use bt_shell_common::font_options::{FontOption, TILES_SHOWN, lacking, tiles_shown};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{
    AnyThread, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class,
    msg_send, sel,
};
use objc2_app_kit::{
    NSBezierPath, NSButton, NSColor, NSControl, NSControlSize, NSEvent, NSFont, NSFontDescriptor,
    NSFontFeatureSettingsAttribute, NSLayoutAttribute, NSStackView, NSStringDrawing, NSTextField,
    NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_foundation::{
    NSArray, NSAttributedString, NSDictionary, NSInteger, NSMutableAttributedString, NSNumber,
    NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, ns_string,
};

use super::look::{self, Ink};
use super::pictures::attributes;
use super::switch::ThemeSwitch;
use super::{ROW_WIDTH, SettingsWindow, set_switch};

/// The vertical space between two rows, and between the parts of the section, in points.
const SPACING: f64 = 10.0;
/// The tiles: how many to a line, the gap between them, their height and corner.
const COLUMNS: usize = 4;
const GAP: f64 = 8.0;
const HEIGHT: f64 = 66.0;
const CORNER: f64 = 8.0;
/// The size of the letters on a tile, and of an example's text in a row.
const LETTERS: f64 = 19.0;
const EXAMPLE: f64 = 13.0;

/// The descriptor of the family `name` with `features` set on or off (tag, value).
fn sample_descriptor(name: &str, features: &[(String, bool)]) -> Retained<NSFontDescriptor> {
    let descriptor = NSFontDescriptor::new().fontDescriptorWithFamily(&NSString::from_str(name));
    if features.is_empty() {
        return descriptor;
    }
    let records: Vec<Retained<NSDictionary<NSString, AnyObject>>> = features
        .iter()
        .map(|(tag, on)| {
            let tag = NSString::from_str(tag);
            let value = NSNumber::new_i32(i32::from(*on));
            let values: [&AnyObject; 2] = [tag.as_ref(), value.as_ref()];
            // The OpenType form of a feature setting: `kCTFontOpenTypeFeatureTag` and
            // `kCTFontOpenTypeFeatureValue`, which CoreText reads from a descriptor. (A font
            // that draws an alternate for the tag shows it; the keys are what make it so.)
            NSDictionary::from_slices(
                &[
                    ns_string!("CTFeatureOpenTypeTag"),
                    ns_string!("CTFeatureOpenTypeValue"),
                ],
                &values,
            )
        })
        .collect();
    let list = NSArray::from_retained_slice(&records);
    let values: [&AnyObject; 1] = [list.as_ref()];
    // SAFETY: AppKit's font feature attribute key (an extern static) with the value type it
    // documents — an array of setting dictionaries.
    let attributes =
        unsafe { NSDictionary::from_slices(&[NSFontFeatureSettingsAttribute], &values) };
    // SAFETY: the dictionary is the feature-settings attribute built above.
    unsafe { descriptor.fontDescriptorByAddingAttributes(&attributes) }
}

/// A font of `family` at `size` with `features` set on or off (tag, value); the default chain's
/// font when no family is named, and the system's monospaced font where the family cannot be had.
fn sample_font(family: Option<&str>, size: f64, features: &[(String, bool)]) -> Retained<NSFont> {
    let fallback = || NSFont::monospacedSystemFontOfSize_weight(size, 0.0);
    let name = family
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .or(bt_gpu::app_default_family());
    let Some(name) = name else {
        return fallback();
    };
    NSFont::fontWithDescriptor_size(&sample_descriptor(name, features), size)
        .unwrap_or_else(fallback)
}

/// `text` in `font` and `color`.
fn piece(text: &str, font: &NSFont, color: &NSColor) -> Retained<NSAttributedString> {
    // SAFETY: `attributes` holds a font and a colour under AppKit's keys.
    unsafe {
        NSAttributedString::initWithString_attributes(
            NSAttributedString::alloc(),
            &NSString::from_str(text),
            Some(&attributes(font, color)),
        )
    }
}

/// The example of a row: `text` without the option, an arrow, `text` with it.
fn example(text: &str, before: &NSFont, after: &NSFont) -> Retained<NSMutableAttributedString> {
    let whole = NSMutableAttributedString::new();
    let ink = look::ink(Ink::Text);
    let faint = look::ink(Ink::Dim);
    let arrow = NSFont::systemFontOfSize(12.0);
    whole.appendAttributedString(&piece(text, before, &ink));
    whole.appendAttributedString(&piece("   \u{2192}   ", &arrow, &faint));
    whole.appendAttributedString(&piece(text, after, &ink));
    whole
}

/// One tile: the letters it shows (`None`: "in context"), its name, the tooltip and the two
/// fonts the letters are drawn in.
struct Tile {
    letters: Option<String>,
    name: String,
    before: Retained<NSFont>,
    after: Retained<NSFont>,
    /// Which option of the window's list the tile is.
    option: usize,
}

pub(super) struct TilesIvars {
    tiles: RefCell<Vec<Tile>>,
    on: RefCell<Vec<bool>>,
    enabled: Cell<bool>,
    /// The tile the last click landed on.
    clicked: Cell<Option<usize>>,
    /// The tooltips' strings: the view does not keep them, so they live here as long as they are
    /// installed.
    tips: RefCell<Vec<Retained<NSString>>>,
}

define_class!(
    /// The tiles: four to a line, each a pair of letters and a name, the chosen ones framed.
    // SAFETY: NSControl has no subclassing requirement; Drop is not implemented. The control has
    // no cell, so its enabled state is the class's own.
    #[unsafe(super(NSControl))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriSettingsLetterTiles"]
    #[ivars = TilesIvars]
    pub(super) struct LetterTiles;

    unsafe impl NSObjectProtocol for LetterTiles {}

    impl LetterTiles {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(intrinsicContentSize))]
        fn intrinsic_content_size(&self) -> NSSize {
            let lines = self.ivars().tiles.borrow().len().div_ceil(COLUMNS) as f64;
            NSSize::new(
                ROW_WIDTH,
                lines * HEIGHT + (lines - 1.0).max(0.0) * GAP,
            )
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            if !self.ivars().enabled.get() {
                return;
            }
            let point = self.convertPoint_fromView(event.locationInWindow(), None);
            let Some(index) = tile_at(point, self.ivars().tiles.borrow().len()) else {
                return;
            };
            self.ivars().clicked.set(Some(index));
            // SAFETY: the action and target are the ones the window gave this control; the
            // target is a weak reference to an object that lives for the whole process.
            unsafe {
                let _ = self.sendAction_to(self.action(), self.target().as_deref());
            }
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            self.paint();
        }
    }
);

/// The tile's width: the line's width less the gaps, shared by the columns.
fn tile_width() -> f64 {
    ((ROW_WIDTH - (COLUMNS as f64 - 1.0) * GAP) / COLUMNS as f64).floor()
}

/// Where tile `index` is: its left and top edge in the control.
fn tile_origin(index: usize) -> NSPoint {
    NSPoint::new(
        (index % COLUMNS) as f64 * (tile_width() + GAP),
        (index / COLUMNS) as f64 * (HEIGHT + GAP),
    )
}

/// The tile under `point` of `count`; the gaps belong to none.
fn tile_at(point: NSPoint, count: usize) -> Option<usize> {
    if point.x < 0.0 || point.y < 0.0 {
        return None;
    }
    let column = (point.x / (tile_width() + GAP)).floor() as usize;
    let line = (point.y / (HEIGHT + GAP)).floor() as usize;
    let inside = point.x - column as f64 * (tile_width() + GAP) <= tile_width()
        && point.y - line as f64 * (HEIGHT + GAP) <= HEIGHT;
    let index = line * COLUMNS + column;
    (column < COLUMNS && index < count && inside).then_some(index)
}

/// `name` cut with an ellipsis until it takes at most `width` in `attributes`' font.
fn fitted(name: &str, width: f64, font: &NSFont) -> String {
    let measure = |text: &str| -> f64 {
        let attributes = attributes(font, &look::ink(Ink::Text));
        // SAFETY: `attributes` holds the two keys `sizeWithAttributes:` reads.
        unsafe {
            NSString::from_str(text)
                .sizeWithAttributes(Some(&attributes))
                .width
        }
    };
    if measure(name) <= width {
        return name.to_owned();
    }
    let mut kept: Vec<char> = name.chars().collect();
    while !kept.is_empty() {
        kept.pop();
        let text: String = kept.iter().collect::<String>().trim_end().to_owned() + "\u{2026}";
        if measure(&text) <= width {
            return text;
        }
    }
    "\u{2026}".to_owned()
}

impl LetterTiles {
    pub(super) fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(TilesIvars {
            tiles: RefCell::new(Vec::new()),
            on: RefCell::new(Vec::new()),
            enabled: Cell::new(true),
            clicked: Cell::new(None),
            tips: RefCell::new(Vec::new()),
        });
        // SAFETY: `init` is NSControl's designated initializer (zero frame); the size is the
        // intrinsic one.
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        this
    }

    /// The tile the last click landed on, as the option's place in the window's list.
    pub(super) fn clicked(&self) -> Option<usize> {
        let at = self.ivars().clicked.get()?;
        self.ivars().tiles.borrow().get(at).map(|tile| tile.option)
    }

    fn redraw(&self) {
        let view: &NSView = self.as_super().as_super();
        view.setNeedsDisplay(true);
    }

    /// Shows `tiles`, with a tooltip each (`tips`, in the same order).
    fn set_tiles(&self, tiles: Vec<Tile>, tips: Vec<Retained<NSString>>) {
        let view: &NSView = self.as_super().as_super();
        view.removeAllToolTips();
        for (index, tip) in tips.iter().enumerate() {
            let origin = tile_origin(index);
            let area = NSRect::new(origin, NSSize::new(tile_width(), HEIGHT));
            // SAFETY: an `NSString` owner is the tooltip's text itself; the view does not keep
            // it, so `tips` does, for as long as the rectangle stays. The user data is unused.
            unsafe {
                view.addToolTipRect_owner_userData(area, tip.as_ref(), std::ptr::null_mut());
            }
        }
        self.ivars().tiles.replace(tiles);
        self.ivars().tips.replace(tips);
        self.ivars().clicked.set(None);
        self.invalidateIntrinsicContentSize();
        self.redraw();
    }

    /// Frames the chosen tiles and lets the control take clicks or not.
    fn set_state(&self, on: Vec<bool>, enabled: bool) {
        let changed = self.ivars().on.replace(on.clone()) != on
            || self.ivars().enabled.replace(enabled) != enabled;
        if changed {
            self.redraw();
        }
    }

    fn paint(&self) {
        let ivars = self.ivars();
        let enabled = ivars.enabled.get();
        let on = ivars.on.borrow();
        let name_font = NSFont::systemFontOfSize(11.0);
        let arrow_font = NSFont::systemFontOfSize(12.0);
        let strength = if enabled { 1.0 } else { 0.4 };
        for (index, tile) in ivars.tiles.borrow().iter().enumerate() {
            let origin = tile_origin(index);
            let area = NSRect::new(origin, NSSize::new(tile_width(), HEIGHT));
            let chosen = on.get(index).copied().unwrap_or(false);
            let face =
                NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(area, CORNER, CORNER);
            look::faded(Ink::Text, 0.05 * strength).setFill();
            face.fill();
            if chosen {
                let frame = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                    NSRect::new(
                        NSPoint::new(origin.x + 1.0, origin.y + 1.0),
                        NSSize::new(tile_width() - 2.0, HEIGHT - 2.0),
                    ),
                    CORNER - 1.0,
                    CORNER - 1.0,
                );
                frame.setLineWidth(2.0);
                look::faded(Ink::Accent, strength).setStroke();
                frame.stroke();
            }
            let ink = look::faded(Ink::Text, strength);
            let faint = look::faded(Ink::Dim, strength);
            // The letters: before, an arrow, after — or "in context" for an option the font
            // does not say the letters of.
            let line_top = origin.y + 8.0;
            match &tile.letters {
                Some(letters) => {
                    let (before, after) = (&tile.before, &tile.after);
                    let width = |text: &str, font: &NSFont| -> f64 {
                        let attributes = attributes(font, &ink);
                        // SAFETY: as in `fitted`.
                        unsafe {
                            NSString::from_str(text)
                                .sizeWithAttributes(Some(&attributes))
                                .width
                        }
                    };
                    let gap = 8.0;
                    let arrow = "\u{2192}";
                    let total = width(letters, before)
                        + gap
                        + width(arrow, &arrow_font)
                        + gap
                        + width(letters, after);
                    let mut x = origin.x + (tile_width() - total) / 2.0;
                    for (text, font, color, space) in [
                        (letters.as_str(), before, &ink, gap),
                        (arrow, &arrow_font, &faint, gap),
                        (letters.as_str(), after, &ink, 0.0),
                    ] {
                        let attributes = attributes(font, color);
                        // SAFETY: as in `fitted`; drawing happens inside `drawRect:`.
                        unsafe {
                            NSString::from_str(text).drawAtPoint_withAttributes(
                                NSPoint::new(x, line_top + if text == arrow { 5.0 } else { 0.0 }),
                                Some(&attributes),
                            );
                        }
                        x += width(text, font) + space;
                    }
                }
                None => {
                    let text = "in context";
                    let attributes = attributes(&name_font, &faint);
                    // SAFETY: as above.
                    unsafe {
                        let width = NSString::from_str(text)
                            .sizeWithAttributes(Some(&attributes))
                            .width;
                        NSString::from_str(text).drawAtPoint_withAttributes(
                            NSPoint::new(origin.x + (tile_width() - width) / 2.0, line_top + 8.0),
                            Some(&attributes),
                        );
                    }
                }
            }
            // The name, cut to the tile.
            let name = fitted(&tile.name, tile_width() - 12.0, &name_font);
            let attributes = attributes(&name_font, &faint);
            // SAFETY: as above.
            unsafe {
                let width = NSString::from_str(&name)
                    .sizeWithAttributes(Some(&attributes))
                    .width;
                NSString::from_str(&name).drawAtPoint_withAttributes(
                    NSPoint::new(
                        origin.x + (tile_width() - width) / 2.0,
                        origin.y + HEIGHT - 20.0,
                    ),
                    Some(&attributes),
                );
            }
        }
    }
}

/// What the stack was built from: a different one is built again.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Signature {
    family: Option<String>,
    ids: Vec<String>,
    all: bool,
}

/// The controls on screen, by the option they are.
struct Built {
    signature: Signature,
    /// The rows' switches and the option each is.
    switches: Vec<(usize, Retained<ThemeSwitch>)>,
    /// The tiles, with the options they show.
    tiles: Option<(Retained<LetterTiles>, Vec<usize>)>,
    /// The orange line about a tag the font lacks.
    warning: Retained<NSTextField>,
}

/// The Options row's control: the stack the rows, the tiles and the warning are in.
pub(super) struct Options {
    stack: Retained<NSStackView>,
    built: RefCell<Option<Built>>,
    /// "Show All" was chosen: every tile shows.
    all: Cell<bool>,
}

impl Options {
    /// The empty stack the row's control is, before there is anything to show.
    pub(super) fn stack(mtm: MainThreadMarker) -> Retained<NSStackView> {
        let stack = NSStackView::stackViewWithViews(&NSArray::new(), mtm);
        stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        stack.setAlignment(NSLayoutAttribute::Leading);
        stack.setSpacing(SPACING);
        stack
    }

    pub(super) fn new(stack: Retained<NSStackView>) -> Self {
        Self {
            stack,
            built: RefCell::new(None),
            all: Cell::new(false),
        }
    }

    /// The stack, as the row's control view.
    pub(super) fn view(&self) -> Retained<NSView> {
        let view: &NSView = &self.stack;
        view.retain()
    }

    /// Flips "Show All" and "Show Fewer"; the window refreshes from the file after it.
    pub(super) fn toggle_all(&self) {
        self.all.set(!self.all.get());
    }

    /// Shows the rows and the tiles of `options`, on as `font`'s choices say; each switch's tag
    /// and each tile's option is the option's place in `options`, which the window keeps for the
    /// action. The same options as last time only update the state in place.
    pub(super) fn show(
        &self,
        window: &SettingsWindow,
        options: &[FontOption],
        font: &FontOptions,
        enabled: bool,
    ) {
        let tile_options: Vec<usize> = options
            .iter()
            .enumerate()
            .filter(|(_, option)| !option.is_row())
            .map(|(at, _)| at)
            .collect();
        let signature = Signature {
            family: font.family.clone(),
            ids: options
                .iter()
                .map(|option| option.id().to_owned())
                .collect(),
            all: self.all.get(),
        };
        let mut built = self.built.borrow_mut();
        if built.as_ref().map(|built| &built.signature) != Some(&signature) {
            *built = Some(self.build(window, options, &tile_options, font, signature));
        }
        let Some(built) = built.as_ref() else {
            return;
        };
        for (at, switch) in &built.switches {
            set_switch(switch, options[*at].is_on(font));
            switch.setEnabled(enabled);
        }
        if let Some((tiles, shown)) = &built.tiles {
            let on = shown.iter().map(|at| options[*at].is_on(font)).collect();
            tiles.set_state(on, enabled);
        }
        let missing = lacking(font);
        built
            .warning
            .setStringValue(&NSString::from_str(&warning_text(font, &missing)));
        built.warning.setHidden(missing.is_empty());
    }

    /// Builds the stack for `options`: a line for each row, then the tiles and what goes with them.
    fn build(
        &self,
        window: &SettingsWindow,
        options: &[FontOption],
        tile_options: &[usize],
        font: &FontOptions,
        signature: Signature,
    ) -> Built {
        for view in self.stack.arrangedSubviews().iter() {
            self.stack.removeArrangedSubview(&view);
            view.removeFromSuperview();
        }
        let mtm = window.mtm();
        let family = font.family.as_deref();
        let mut switches = Vec::new();
        for (at, option) in options
            .iter()
            .enumerate()
            .filter(|(_, option)| option.is_row())
        {
            let switch = ThemeSwitch::new(mtm);
            switch.setTag(NSInteger::try_from(at).unwrap_or_default());
            // SAFETY: the target is a weak reference and the window lives for the whole
            // process; the selector is the window's own action.
            unsafe {
                switch.setTarget(Some(window.target()));
                switch.setAction(Some(sel!(fontOptionChanged:)));
            }
            let title = super::text_label(&NSString::from_str(option.title()), mtm);
            title.setFont(Some(&NSFont::systemFontOfSize(NSFont::systemFontSize())));
            let mut texts: Vec<Retained<NSView>> =
                vec![Retained::into_super(Retained::into_super(title))];
            if let Some((about, sample)) = option.about() {
                let about = super::text_label(&NSString::from_str(about), mtm);
                about.setFont(Some(&NSFont::systemFontOfSize(12.0)));
                about.setTextColor(Some(&look::ink(Ink::Dim)));
                texts.push(Retained::into_super(Retained::into_super(about)));
                let before = sample_font(family, EXAMPLE, &option.features(false));
                let after = sample_font(family, EXAMPLE, &option.features(true));
                let line = super::text_label(ns_string!(""), mtm);
                line.setAttributedStringValue(&example(sample, &before, &after));
                texts.push(Retained::into_super(Retained::into_super(line)));
            }
            let text_views: Vec<&NSView> = texts.iter().map(|view| &**view).collect();
            let text = super::stack(
                mtm,
                &text_views,
                NSUserInterfaceLayoutOrientation::Vertical,
                2.0,
                NSLayoutAttribute::Leading,
            );
            let line = super::stack(
                mtm,
                &[switch.as_super().as_super(), &text],
                NSUserInterfaceLayoutOrientation::Horizontal,
                10.0,
                NSLayoutAttribute::Top,
            );
            self.stack.addArrangedSubview(&line);
            switches.push((at, switch));
        }
        let mut tiles = None;
        if !tile_options.is_empty() {
            let heading = super::text_label(ns_string!("Alternate letters"), mtm);
            heading.setFont(Some(&NSFont::boldSystemFontOfSize(12.0)));
            heading.setTextColor(Some(&look::ink(Ink::Dim)));
            self.stack.addArrangedSubview(&heading);
            let shown_count = tiles_shown(tile_options.len(), signature.all);
            let shown: Vec<usize> = tile_options[..shown_count].to_vec();
            let control = LetterTiles::new(mtm);
            let mut built = Vec::new();
            let mut tips = Vec::new();
            for at in &shown {
                let option = &options[*at];
                built.push(Tile {
                    letters: option.letters(),
                    name: option.title().to_owned(),
                    before: sample_font(family, LETTERS, &option.features(false)),
                    after: sample_font(family, LETTERS, &option.features(true)),
                    option: *at,
                });
                tips.push(NSString::from_str(&format!(
                    "{} ({})",
                    option.title(),
                    option.tags().join(", ")
                )));
            }
            control.set_tiles(built, tips);
            // SAFETY: the target is a weak reference and the window lives for the whole process;
            // the selector is the window's own action.
            unsafe {
                control.setTarget(Some(window.target()));
                control.setAction(Some(sel!(fontTileClicked:)));
            }
            self.stack.addArrangedSubview(&control);
            if tile_options.len() > FIRST {
                let title = if signature.all {
                    "Show Fewer".to_owned()
                } else {
                    format!("Show All ({})", tile_options.len())
                };
                // SAFETY: the target is a weak reference and the window lives for the whole
                // process; the selector is the window's own action.
                let button = unsafe {
                    NSButton::buttonWithTitle_target_action(
                        &NSString::from_str(&title),
                        Some(window.target()),
                        Some(sel!(showAllTiles:)),
                        mtm,
                    )
                };
                button.setControlSize(NSControlSize::Small);
                self.stack.addArrangedSubview(&button);
            }
            tiles = Some((control, shown));
        }
        let warning = super::text_label(ns_string!(""), mtm);
        warning.setFont(Some(&NSFont::systemFontOfSize(12.0)));
        warning.setTextColor(Some(&look::ink(Ink::Warning)));
        warning.setHidden(true);
        self.stack.addArrangedSubview(&warning);
        Built {
            signature,
            switches,
            tiles,
            warning,
        }
    }
}

/// How many tiles show before "Show All".
const FIRST: usize = TILES_SHOWN;

/// The title of the options group: the font's name, since the list is its own.
pub(super) fn heading(font: &FontOptions) -> String {
    let family = font
        .family
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .or(bt_gpu::app_default_family());
    match family {
        Some(name) => format!("{name} options"),
        None => "Font options".to_owned(),
    }
}

/// What the orange line says of the tags the font does not list.
fn warning_text(font: &FontOptions, missing: &[String]) -> String {
    let family = font
        .family
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("The default font");
    let tags = missing
        .iter()
        .map(|tag| format!("\u{201c}{tag}\u{201d}"))
        .collect::<Vec<_>>()
        .join(", ");
    match missing.len() {
        0 => String::new(),
        1 => format!("{family} has no feature {tags}; the line in the file does nothing."),
        _ => format!("{family} has no features {tags}; those lines in the file do nothing."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A click lands on the tile under it; the gaps and the room beyond the last land on none.
    #[test]
    fn a_click_lands_on_the_tile_under_it() {
        let wide = tile_width();
        assert_eq!(tile_at(NSPoint::new(-1.0, 3.0), 6), None);
        assert_eq!(tile_at(NSPoint::new(2.0, 3.0), 6), Some(0));
        assert_eq!(tile_at(NSPoint::new(wide + GAP / 2.0, 3.0), 6), None);
        assert_eq!(tile_at(NSPoint::new(wide + GAP + 2.0, 3.0), 6), Some(1));
        // The second line starts at the fifth tile.
        assert_eq!(tile_at(NSPoint::new(2.0, HEIGHT + GAP + 2.0), 6), Some(4));
        // The sixth tile is the last: the seventh place is empty.
        assert_eq!(
            tile_at(
                NSPoint::new(2.0 * (wide + GAP) + 2.0, HEIGHT + GAP + 2.0),
                6
            ),
            None
        );
        assert_eq!(tile_at(NSPoint::new(2.0, 3.0 * (HEIGHT + GAP)), 6), None);
    }

    /// The descriptor of an example carries the option's tags, on or off, under the keys CoreText
    /// reads, and a font without any asks for none.
    #[test]
    fn a_sample_descriptor_carries_the_feature_settings() {
        let settings = |descriptor: &NSFontDescriptor| -> Option<Retained<AnyObject>> {
            // SAFETY: the attribute key is an extern static of AppKit's.
            unsafe { descriptor.objectForKey(NSFontFeatureSettingsAttribute) }
        };
        assert!(settings(&sample_descriptor("Menlo", &[])).is_none());
        let descriptor = sample_descriptor(
            "Menlo",
            &[("ss01".to_owned(), true), ("calt".to_owned(), false)],
        );
        let list = settings(&descriptor).expect("the feature settings");
        let list = list.downcast_ref::<NSArray>().expect("an array");
        assert_eq!(list.count(), 2);
        let first = list.objectAtIndex(0);
        let first = first
            .downcast_ref::<NSDictionary>()
            .expect("a setting is a dictionary");
        assert_eq!(first.count(), 2);
    }

    /// Four tiles and their gaps are the row's width, within a point.
    #[test]
    fn four_tiles_fill_the_row() {
        let used = COLUMNS as f64 * tile_width() + (COLUMNS as f64 - 1.0) * GAP;
        assert!(ROW_WIDTH - used >= 0.0 && ROW_WIDTH - used < COLUMNS as f64);
        assert_eq!(tile_origin(0), NSPoint::new(0.0, 0.0));
        assert_eq!(tile_origin(COLUMNS).x, 0.0);
        assert_eq!(tile_origin(COLUMNS).y, HEIGHT + GAP);
    }

    /// One tag the font lacks, or several, in a sentence of the font's name.
    #[test]
    fn the_warning_names_the_font_and_the_tags() {
        let font = FontOptions {
            family: Some("Obel Mono".to_owned()),
            ..FontOptions::default()
        };
        assert_eq!(warning_text(&font, &[]), "");
        assert_eq!(
            warning_text(&font, &["ss19".to_owned()]),
            "Obel Mono has no feature \u{201c}ss19\u{201d}; the line in the file does nothing."
        );
        assert!(
            warning_text(&FontOptions::default(), &["a".to_owned(), "b".to_owned()])
                .starts_with("The default font has no features")
        );
    }
}
