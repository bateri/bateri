//! bateri ▸ Settings… (Cmd-,): ayar penceresi — solda dört kategorili kenar
//! çubuğu, sağda etiket–kontrol ızgarası. İskeletin ve davranışın kararları
//! `.tasks/029-ayarlar-penceresi/discussion.md` → Karar 2–6, 9'da; burada
//! tekrarlanmıyor.
//!
//! Pencere **kendi durumunu tutmaz**: gösterdiği her değer `AppDelegate`'in
//! etkin ayarından ([`SettingsWindow::refresh`]) ve her kontrolün eylemi bir
//! [`SettingsEdit`] kurup `AppDelegate::save_edit`'e verir — dosyaya yazan
//! o, uygulayan da dosyayı okuyan bugünkü yol (`reload_settings`), yani
//! kontrolün değeri ekrana ancak dosyadan dönerek gider.
//!
//! Kontrolün hangi satır olduğu `tag`'inde ([`Key`]); eylem seçicisi kontrolün
//! **türüne** göre (popup, switch, slider, alan, stepper), satıra göre değil.
//! Popup başlığı ↔ enum varyantı eşlemesi kapsamlı `match`'lerde ([`Choice`]),
//! öğelerin sırası `bt-core`'un `NAMES` tablosunun sırası: yeni bir varyant
//! derleme hatası verir, popup'ta sessizce eksik kalmaz.

use std::cell::{OnceCell, RefCell};

use bt_core::{
    CURSOR_BLINK_RANGE, CURSOR_GLOW_RANGE, CURSOR_RADIUS_RANGE, CaretShape, ConfirmClose,
    CursorBlink, CursorMotion, LINE_HEIGHT_RANGE, Osc52, ReduceMotion, SCROLLBACK_MAX,
    SYSTEM_THEME, Settings, SettingsEdit, ShellIntegration, SmoothScroll, UnfocusedCaret,
};
use bt_gpu::FontNotice;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{
    ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel,
};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSButton, NSColor, NSControl, NSControlStateValueOff,
    NSControlStateValueOn, NSControlTextEditingDelegate, NSEventType, NSFont, NSGridCell,
    NSGridCellPlacement, NSGridRowAlignment, NSGridView, NSImage, NSImageView, NSLayoutConstraint,
    NSMenuItem, NSPopUpButton, NSScrollView, NSSlider, NSSplitViewController, NSSplitViewItem,
    NSStackView, NSStepper, NSSwitch, NSTableCellView, NSTableColumn, NSTableView,
    NSTableViewDataSource, NSTableViewDelegate, NSTableViewStyle, NSTextField,
    NSUserInterfaceLayoutOrientation, NSView, NSViewController, NSWindow, NSWindowStyleMask,
    NSWindowTabbingMode, NSWindowTitleVisibility,
};
use objc2_foundation::{
    NSArray, NSIndexSet, NSInteger, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect,
    NSSize, NSString, ns_string,
};

use crate::app;
use crate::zoom::{MAX_SIZE, MIN_SIZE};

/// Pencerenin içerik boyu, punto. Sabit — pencere yeniden
/// boyutlandırılamıyor (en uzun bölmenin altı satırı ve açıklamaları sığıyor,
/// fazlası boşluk olurdu). Tasarım sabiti, ölçülmüş bir sayı değil.
const WINDOW_SIZE: NSSize = NSSize::new(680.0, 500.0);
/// Kenar çubuğunun genişliği: System Settings'inkine yakın, dört kısa başlık
/// için bol. Tasarım sabiti.
const SIDEBAR_WIDTH: f64 = 180.0;
/// Sağ bölmenin iç kenar payı. macOS formlarının 20 puntoluk kenar payı.
const MARGIN: f64 = 20.0;
/// Etiket sütununun genişliği: dört bölmede de aynı, yoksa kategori
/// değişince kontroller yana kayardı. En uzun etiketin ("Confirm before
/// closing:") sığdığı genişlik.
const LABEL_WIDTH: f64 = 170.0;
/// Popup'ların ortak genişliği: aynı sütunda farklı boylarda popup
/// dağınık görünüyor. En uzun öğe ("Only when a program is running") sığıyor.
const POPUP_WIDTH: f64 = 230.0;
/// Slider'ların genişliği; yanında değer etiketi duruyor.
const SLIDER_WIDTH: f64 = 170.0;
/// Açıklama metninin kırılma genişliği: popup'ın genişliği — açıklama
/// üstündeki kontrolün sağ kenarını aşmasın (ilk ekran görüntüsünde aşıyordu).
const NOTE_WIDTH: f64 = POPUP_WIDTH;

/// Kenar çubuğunun satırları.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Category {
    General,
    Appearance,
    Cursor,
    Motion,
}

impl Category {
    const ALL: [Category; 4] = [
        Category::General,
        Category::Appearance,
        Category::Cursor,
        Category::Motion,
    ];

    fn title(self) -> &'static str {
        match self {
            Category::General => "General",
            Category::Appearance => "Appearance",
            Category::Cursor => "Cursor",
            Category::Motion => "Motion",
        }
    }

    /// SF Symbol adı; sembol bulunamazsa ikon boş kalır, satır kalır.
    fn symbol(self) -> &'static str {
        match self {
            Category::General => "gearshape",
            Category::Appearance => "paintpalette",
            Category::Cursor => "character.cursor.ibeam",
            Category::Motion => "wind",
        }
    }
}

/// Bir kontrolün hangi ayar satırı olduğu — kontrolün `tag`'i.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Key {
    ConfirmClose,
    Clipboard,
    Scrollback,
    ShellIntegration,
    Theme,
    LightTheme,
    DarkTheme,
    Font,
    Size,
    LineHeight,
    Shape,
    Blink,
    BlinkSpeed,
    Radius,
    Glow,
    Unfocused,
    CursorMotion,
    SmoothScroll,
    ReduceMotion,
}

impl Key {
    /// Sıra `tag`'in ta kendisi: `ALL[tag]`.
    const ALL: [Key; 19] = [
        Key::ConfirmClose,
        Key::Clipboard,
        Key::Scrollback,
        Key::ShellIntegration,
        Key::Theme,
        Key::LightTheme,
        Key::DarkTheme,
        Key::Font,
        Key::Size,
        Key::LineHeight,
        Key::Shape,
        Key::Blink,
        Key::BlinkSpeed,
        Key::Radius,
        Key::Glow,
        Key::Unfocused,
        Key::CursorMotion,
        Key::SmoothScroll,
        Key::ReduceMotion,
    ];

    fn tag(self) -> NSInteger {
        self as NSInteger
    }

    fn from_tag(tag: NSInteger) -> Option<Key> {
        Self::ALL.get(usize::try_from(tag).ok()?).copied()
    }
}

/// Popup'la seçilen bir dizge enum'u: öğeler `bt-core`'un yazılış
/// tablosundan (`NAMES`, sırası dahil), başlıklar buradaki kapsamlı
/// `match`'ten. Başlık bir UI dizgisi, yazılış dosyanın sözlüğü — ikisi ayrı.
trait Choice: Copy + PartialEq + 'static {
    fn names() -> &'static [(&'static str, Self)];
    fn title(self) -> &'static str;
}

impl Choice for ConfirmClose {
    fn names() -> &'static [(&'static str, Self)] {
        Self::NAMES
    }
    fn title(self) -> &'static str {
        match self {
            ConfirmClose::Never => "Never",
            ConfirmClose::Running => "Only when a program is running",
            ConfirmClose::Always => "Always",
        }
    }
}

impl Choice for ShellIntegration {
    fn names() -> &'static [(&'static str, Self)] {
        Self::NAMES
    }
    fn title(self) -> &'static str {
        match self {
            ShellIntegration::Auto => "Auto",
            ShellIntegration::Blocks => "Blocks only",
            ShellIntegration::Off => "Off",
        }
    }
}

impl Choice for CaretShape {
    fn names() -> &'static [(&'static str, Self)] {
        Self::NAMES
    }
    fn title(self) -> &'static str {
        match self {
            CaretShape::Block => "Block",
            CaretShape::Underline => "Underline",
            CaretShape::Beam => "Beam",
        }
    }
}

impl Choice for CursorBlink {
    fn names() -> &'static [(&'static str, Self)] {
        Self::NAMES
    }
    fn title(self) -> &'static str {
        match self {
            CursorBlink::Auto => "Follow program",
            CursorBlink::On => "On",
            CursorBlink::Off => "Off",
        }
    }
}

impl Choice for UnfocusedCaret {
    fn names() -> &'static [(&'static str, Self)] {
        Self::NAMES
    }
    fn title(self) -> &'static str {
        match self {
            UnfocusedCaret::Hollow => "Hollow",
            UnfocusedCaret::Solid => "Solid",
        }
    }
}

impl Choice for CursorMotion {
    fn names() -> &'static [(&'static str, Self)] {
        Self::NAMES
    }
    fn title(self) -> &'static str {
        match self {
            CursorMotion::Snap => "Snap",
            CursorMotion::Ease => "Ease",
            CursorMotion::Spring => "Spring",
        }
    }
}

impl Choice for ReduceMotion {
    fn names() -> &'static [(&'static str, Self)] {
        Self::NAMES
    }
    fn title(self) -> &'static str {
        match self {
            ReduceMotion::System => "Match System",
            ReduceMotion::On => "On",
            ReduceMotion::Off => "Off",
        }
    }
}

/// Popup'ın öğe başlıkları, `NAMES` sırasıyla.
fn choice_titles<T: Choice>() -> Vec<&'static str> {
    T::names().iter().map(|&(_, value)| value.title()).collect()
}

/// Seçili öğenin varyantı; `-1` (seçim yok) ya da taşan indeks `None`.
fn choice_at<T: Choice>(index: NSInteger) -> Option<T> {
    let index = usize::try_from(index).ok()?;
    T::names().get(index).map(|&(_, value)| value)
}

/// Varyantın öğe indeksi.
fn choice_index<T: Choice>(value: T) -> Option<usize> {
    T::names()
        .iter()
        .position(|&(_, candidate)| candidate == value)
}

/// İki değerli ayarlar switch: açık ↔ `copy`.
fn osc52_on(mode: Osc52) -> bool {
    match mode {
        Osc52::Copy => true,
        Osc52::Off => false,
    }
}

fn smooth_on(smooth: SmoothScroll) -> bool {
    match smooth {
        SmoothScroll::On => true,
        SmoothScroll::Off => false,
    }
}

/// Blink hızı slider'ının konumu (`0..=1`, sağ **hızlı**, yani kısa yarım
/// periyot) ↔ yarım periyot, saniye. Ölçek **logaritmik**: aralık yüz kat
/// (`CURSOR_BLINK_RANGE`) ve doğrusal ölçekte kullanışlı değerlerin hepsi
/// solun ilk yüzdesine sıkışırdı (029 Karar 5). Uçlar açıkça dönüyor, çünkü
/// `exp(ln(x))` bit bit `x` değil ve uçların aralığın uçları olması sözleşme.
fn blink_from_position(position: f64) -> f64 {
    let (min, max) = (*CURSOR_BLINK_RANGE.start(), *CURSOR_BLINK_RANGE.end());
    let t = position.clamp(0.0, 1.0);
    if t <= 0.0 {
        return max;
    }
    if t >= 1.0 {
        return min;
    }
    (max.ln() - t * (max.ln() - min.ln())).exp().clamp(min, max)
}

fn blink_to_position(seconds: f64) -> f64 {
    let (min, max) = (*CURSOR_BLINK_RANGE.start(), *CURSOR_BLINK_RANGE.end());
    let seconds = seconds.clamp(min, max);
    ((max.ln() - seconds.ln()) / (max.ln() - min.ln())).clamp(0.0, 1.0)
}

/// Ondalığı iki basamakta, sondaki sıfırlar olmadan (`0.5`, `1.25`, `13`) —
/// dosyaya yazılanla aynı hassasiyet (`SettingsEdit`'in iki basamağı).
fn decimal_label(value: f64) -> String {
    let text = format!("{value:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    text.to_owned()
}

/// Alanın kabul ettiği `scrollback`: tamsayı, `0..=SCROLLBACK_MAX`.
fn parse_scrollback(text: &str) -> Option<usize> {
    text.trim()
        .parse::<usize>()
        .ok()
        .filter(|&lines| lines <= SCROLLBACK_MAX)
}

/// Alanın kabul ettiği ondalık: sonlu ve aralığın içinde.
fn parse_decimal(text: &str, range: std::ops::RangeInclusive<f64>) -> Option<f64> {
    text.trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && range.contains(value))
}

/// Tema popup'ının bir öğesi.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ThemeItem {
    /// "Match System" — `SYSTEM_THEME`'i yazar.
    System,
    Separator,
    Named(String),
}

/// Tema popup'ının öğeleri ve seçili olanın indeksi — Theme ▸ menüsünün
/// sırası (`menu::fill_themes`): Match System, gömülüler, kullanıcınınkiler.
/// Dosyadaki ad listede yoksa (silinmiş tema) sona o eklenir: popup
/// kullanıcının yazdığını gizlemez (Karar 3'ün Font kuralı).
fn theme_items(
    selected: &str,
    with_system: bool,
    embedded: &[&str],
    user: &[String],
) -> (Vec<ThemeItem>, usize) {
    let mut items = Vec::new();
    if with_system {
        items.push(ThemeItem::System);
        items.push(ThemeItem::Separator);
    }
    items.extend(
        embedded
            .iter()
            .map(|name| ThemeItem::Named((*name).to_owned())),
    );
    if !user.is_empty() {
        items.push(ThemeItem::Separator);
        items.extend(user.iter().cloned().map(ThemeItem::Named));
    }
    let position = |items: &[ThemeItem]| {
        items.iter().position(|item| match item {
            ThemeItem::System => selected == SYSTEM_THEME,
            ThemeItem::Separator => false,
            ThemeItem::Named(name) => name == selected,
        })
    };
    if let Some(index) = position(&items) {
        return (items, index);
    }
    items.push(ThemeItem::Separator);
    items.push(ThemeItem::Named(selected.to_owned()));
    let index = items.len() - 1;
    (items, index)
}

/// Font popup'ının bir öğesi.
#[derive(Clone, Debug, PartialEq, Eq)]
enum FontItem {
    /// Zincir: `family = ""`.
    Default,
    Separator,
    Family(String),
    /// Dosyadaki ama listede olmayan aile; seçmek bir şey yazmaz (zaten o).
    Missing(String),
}

/// Font popup'ının öğeleri ve seçili olanın indeksi. Eşleşme harf duyarsız:
/// zincir de adı öyle buluyor (`bt-atlas`'ın `same_family`'si), yani
/// `family = "menlo"` listedeki `Menlo`'dur.
fn font_items(current: Option<&str>, families: &[String]) -> (Vec<FontItem>, usize) {
    let mut items = vec![FontItem::Default];
    if !families.is_empty() {
        items.push(FontItem::Separator);
        items.extend(families.iter().cloned().map(FontItem::Family));
    }
    let Some(current) = current else {
        return (items, 0);
    };
    let wanted = current.to_lowercase();
    if let Some(index) = items
        .iter()
        .position(|item| matches!(item, FontItem::Family(name) if name.to_lowercase() == wanted))
    {
        return (items, index);
    }
    items.push(FontItem::Separator);
    items.push(FontItem::Missing(current.to_owned()));
    let index = items.len() - 1;
    (items, index)
}

/// Listede olmayan ailenin başlığı: ad ve zincirin onun için söyleyeceği.
fn missing_font_title(name: &str, notice: Option<FontNotice>) -> String {
    match notice {
        Some(FontNotice::FamilyNotFound { .. }) => format!("{name} — not found"),
        Some(FontNotice::NotMonospaced { .. }) => format!("{name} — not monospaced"),
        None => name.to_owned(),
    }
}

/// Bir sayı alanı ve onun stepper'ı.
struct Number {
    field: Retained<NSTextField>,
    stepper: Retained<NSStepper>,
}

/// Bir slider ve değer etiketi.
struct Slide {
    slider: Retained<NSSlider>,
    value: Retained<NSTextField>,
}

/// Pencerenin kontrolleri — `refresh`'in yazdığı, eylemlerin okuduğu.
struct Controls {
    confirm_close: Retained<NSPopUpButton>,
    clipboard: Retained<NSSwitch>,
    scrollback: Number,
    shell_integration: Retained<NSPopUpButton>,
    theme: Retained<NSPopUpButton>,
    light_theme: Retained<NSPopUpButton>,
    light_label: Retained<NSTextField>,
    dark_theme: Retained<NSPopUpButton>,
    dark_label: Retained<NSTextField>,
    font: Retained<NSPopUpButton>,
    size: Number,
    line_height: Number,
    shape: Retained<NSPopUpButton>,
    blink: Retained<NSPopUpButton>,
    blink_speed: Slide,
    blink_speed_label: Retained<NSTextField>,
    radius: Slide,
    glow: Slide,
    unfocused: Retained<NSPopUpButton>,
    cursor_motion: Retained<NSPopUpButton>,
    smooth_scroll: Retained<NSSwitch>,
    reduce_motion: Retained<NSPopUpButton>,
}

pub(crate) struct Ivars {
    window: OnceCell<Retained<NSWindow>>,
    sidebar: OnceCell<Retained<NSTableView>>,
    header: OnceCell<Retained<NSTextField>>,
    /// Kategori başına bir ızgara; yalnız seçili olan görünür.
    panes: OnceCell<Vec<Retained<NSGridView>>>,
    controls: OnceCell<Controls>,
    /// Popup'ların öğe listeleri — seçilen indeksin anlamı. Her `refresh`
    /// yeniden kuruyor (tema dizinine yeni dosya).
    themes: RefCell<Vec<ThemeItem>>,
    light_themes: RefCell<Vec<ThemeItem>>,
    dark_themes: RefCell<Vec<ThemeItem>>,
    fonts: RefCell<Vec<FontItem>>,
    /// Makinedeki eşaralıklı aileler — pencere doğarken **bir kez**: liste
    /// her adayı CoreText'le açıyor ve her kayıtta yeniden kurulmaya değmez.
    families: Vec<String>,
}

define_class!(
    // SAFETY: NSObject alt sınıflama şartı taşımaz; Drop uygulanmıyor.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriSettingsWindow"]
    #[ivars = Ivars]
    pub(crate) struct SettingsWindow;

    unsafe impl NSObjectProtocol for SettingsWindow {}

    unsafe impl NSTableViewDataSource for SettingsWindow {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn number_of_rows(&self, _table: &NSTableView) -> NSInteger {
            Category::ALL.len() as NSInteger
        }
    }

    unsafe impl NSControlTextEditingDelegate for SettingsWindow {}

    unsafe impl NSTableViewDelegate for SettingsWindow {
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn view_for_row(
            &self,
            _table: &NSTableView,
            _column: Option<&NSTableColumn>,
            row: NSInteger,
        ) -> Option<Retained<NSView>> {
            let category = usize::try_from(row)
                .ok()
                .and_then(|row| Category::ALL.get(row).copied());
            category.map(|category| Retained::into_super(sidebar_cell(self.mtm(), category)))
        }

        #[unsafe(method(tableViewSelectionDidChange:))]
        fn selection_did_change(&self, _note: &NSNotification) {
            self.show_selected();
        }
    }

    impl SettingsWindow {
        #[unsafe(method(popupChanged:))]
        fn popup_changed(&self, sender: Option<&AnyObject>) {
            let Some(popup) = sender.and_then(|s| s.downcast_ref::<NSPopUpButton>()) else {
                return;
            };
            let index = popup.indexOfSelectedItem();
            let Some(key) = Key::from_tag(popup.tag()) else {
                return;
            };
            let edit = match key {
                Key::ConfirmClose => choice_at(index).map(SettingsEdit::ConfirmClose),
                Key::ShellIntegration => choice_at(index).map(SettingsEdit::ShellIntegration),
                Key::Shape => choice_at(index).map(SettingsEdit::Cursor),
                Key::Blink => choice_at(index).map(SettingsEdit::CursorBlink),
                Key::Unfocused => choice_at(index).map(SettingsEdit::CursorUnfocused),
                Key::CursorMotion => choice_at(index).map(SettingsEdit::CursorMotion),
                Key::ReduceMotion => choice_at(index).map(SettingsEdit::ReduceMotion),
                Key::Theme => theme_edit(&self.ivars().themes.borrow(), index)
                    .map(SettingsEdit::Theme),
                Key::LightTheme => theme_edit(&self.ivars().light_themes.borrow(), index)
                    .map(SettingsEdit::LightTheme),
                Key::DarkTheme => theme_edit(&self.ivars().dark_themes.borrow(), index)
                    .map(SettingsEdit::DarkTheme),
                Key::Font => font_edit(&self.ivars().fonts.borrow(), index)
                    .map(SettingsEdit::FontFamily),
                _ => None,
            };
            self.save(edit);
        }

        #[unsafe(method(switchChanged:))]
        fn switch_changed(&self, sender: Option<&AnyObject>) {
            let Some(switch) = sender.and_then(|s| s.downcast_ref::<NSSwitch>()) else {
                return;
            };
            let on = switch.state() == NSControlStateValueOn;
            let edit = match Key::from_tag(switch.tag()) {
                Some(Key::Clipboard) => Some(SettingsEdit::Osc52(if on {
                    Osc52::Copy
                } else {
                    Osc52::Off
                })),
                Some(Key::SmoothScroll) => Some(SettingsEdit::SmoothScroll(if on {
                    SmoothScroll::On
                } else {
                    SmoothScroll::Off
                })),
                _ => None,
            };
            self.save(edit);
        }

        /// Slider sürüklenirken yalnız değer etiketi değişir; dosyaya
        /// **bırakınca** yazılır (Karar 5). `continuous` açık, yoksa etiket
        /// sürükleme boyunca donardı: yazıp yazmama kararı olayın türünden —
        /// fare basılı ve sürükleniyorsa ara değer. Klavyeyle (ok tuşu)
        /// değişen slider'ın olayı bir tuş olayı ve yazar.
        #[unsafe(method(sliderChanged:))]
        fn slider_changed(&self, sender: Option<&AnyObject>) {
            let Some(slider) = sender.and_then(|s| s.downcast_ref::<NSSlider>()) else {
                return;
            };
            let Some(key) = Key::from_tag(slider.tag()) else {
                return;
            };
            let position = slider.doubleValue();
            let value = match key {
                Key::BlinkSpeed => blink_from_position(position),
                _ => position,
            };
            if let Some(controls) = self.ivars().controls.get() {
                let label = match key {
                    Key::BlinkSpeed => Some((&controls.blink_speed.value, seconds_label(value))),
                    Key::Radius => Some((&controls.radius.value, decimal_label(value))),
                    Key::Glow => Some((&controls.glow.value, decimal_label(value))),
                    _ => None,
                };
                if let Some((field, text)) = label {
                    field.setStringValue(&NSString::from_str(&text));
                }
            }
            let dragging = NSApplication::sharedApplication(self.mtm())
                .currentEvent()
                .is_some_and(|event| {
                    matches!(
                        event.r#type(),
                        NSEventType::LeftMouseDragged | NSEventType::LeftMouseDown
                    )
                });
            if dragging {
                return;
            }
            let edit = match key {
                Key::BlinkSpeed => Some(SettingsEdit::BlinkInterval(value)),
                Key::Radius => Some(SettingsEdit::CursorRadius(value)),
                Key::Glow => Some(SettingsEdit::CursorGlow(value)),
                _ => None,
            };
            self.save(edit);
        }

        /// Sayı alanı: Enter'da ya da odaktan çıkınca (Karar 5). Kabul
        /// edilmeyen girdi yazılmaz, alan etkin değere döner.
        #[unsafe(method(fieldChanged:))]
        fn field_changed(&self, sender: Option<&AnyObject>) {
            let Some(field) = sender.and_then(|s| s.downcast_ref::<NSTextField>()) else {
                return;
            };
            let text = field.stringValue().to_string();
            let edit = match Key::from_tag(field.tag()) {
                Some(Key::Scrollback) => parse_scrollback(&text).map(SettingsEdit::Scrollback),
                Some(Key::Size) => {
                    parse_decimal(&text, MIN_SIZE..=MAX_SIZE).map(SettingsEdit::FontSize)
                }
                Some(Key::LineHeight) => {
                    parse_decimal(&text, LINE_HEIGHT_RANGE).map(SettingsEdit::LineHeight)
                }
                _ => None,
            };
            if edit.is_none() {
                self.refresh_from_delegate();
                return;
            }
            self.save(edit);
        }

        #[unsafe(method(stepperChanged:))]
        fn stepper_changed(&self, sender: Option<&AnyObject>) {
            let Some(stepper) = sender.and_then(|s| s.downcast_ref::<NSStepper>()) else {
                return;
            };
            let value = stepper.doubleValue();
            let edit = match Key::from_tag(stepper.tag()) {
                // Stepper'ın sınırları `0..=SCROLLBACK_MAX` ve adımı tam
                // sayı: değer negatif ya da kesirli olamıyor.
                Some(Key::Scrollback) => Some(SettingsEdit::Scrollback(value.round() as usize)),
                Some(Key::Size) => Some(SettingsEdit::FontSize(value)),
                Some(Key::LineHeight) => Some(SettingsEdit::LineHeight(value)),
                _ => None,
            };
            self.save(edit);
        }

        /// "Open settings.toml": bugünkü "Settings…" yolu (Karar 8).
        #[unsafe(method(openFile:))]
        fn open_file(&self, _sender: Option<&AnyObject>) {
            if let Some(delegate) = app::delegate(self.mtm()) {
                delegate.edit_settings();
            }
        }
    }
);

/// Eylemin tema öğesi → yazılacak ad.
fn theme_edit(items: &[ThemeItem], index: NSInteger) -> Option<String> {
    match items.get(usize::try_from(index).ok()?)? {
        ThemeItem::System => Some(SYSTEM_THEME.to_owned()),
        ThemeItem::Named(name) => Some(name.clone()),
        ThemeItem::Separator => None,
    }
}

/// Eylemin font öğesi → yazılacak aile; listede olmayan dosya değeri
/// yeniden seçilince yazılacak bir şey yok.
fn font_edit(items: &[FontItem], index: NSInteger) -> Option<String> {
    match items.get(usize::try_from(index).ok()?)? {
        FontItem::Default => Some(String::new()),
        FontItem::Family(name) => Some(name.clone()),
        FontItem::Separator | FontItem::Missing(_) => None,
    }
}

/// Blink hızı etiketi: saniye (`0.5 s`).
fn seconds_label(seconds: f64) -> String {
    format!("{} s", decimal_label(seconds))
}

impl SettingsWindow {
    /// Pencereyi ve bütün kontrolleri kurar; görünür yapmaz.
    pub(crate) fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            window: OnceCell::new(),
            sidebar: OnceCell::new(),
            header: OnceCell::new(),
            panes: OnceCell::new(),
            controls: OnceCell::new(),
            themes: RefCell::new(Vec::new()),
            light_themes: RefCell::new(Vec::new()),
            dark_themes: RefCell::new(Vec::new()),
            fonts: RefCell::new(Vec::new()),
            families: bt_gpu::monospaced_families(),
        });
        // SAFETY: NSObject'in init'i argümansızdır ve ivar'lar set edildi.
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.build();
        this
    }

    /// Pencereyi öne getirir (ilk açılışta ortalar); kategori son seçilen.
    pub(crate) fn show(&self) {
        let Some(window) = self.ivars().window.get() else {
            return;
        };
        if !window.isVisible() {
            window.center();
        }
        NSApplication::sharedApplication(self.mtm()).activate();
        window.makeKeyAndOrderFront(None);
    }

    /// Kontrolleri etkin ayarla doldurur — pencerenin gösterdiği değerin tek
    /// kaynağı. Programla kurulan değer eylem tetiklemiyor, yani bir kontrolün
    /// eyleminin içinden (yaz → `reload_settings` → buraya) çağrılması döngü
    /// doğurmaz.
    pub(crate) fn refresh(&self, settings: &Settings, embedded: &[&str], user: &[String]) {
        let Some(c) = self.ivars().controls.get() else {
            return;
        };
        select_choice(&c.confirm_close, settings.confirm_close);
        set_switch(&c.clipboard, osc52_on(settings.osc52));
        set_number(
            &c.scrollback,
            settings.scrollback as f64,
            &settings.scrollback.to_string(),
        );
        select_choice(&c.shell_integration, settings.shell_integration);

        let (items, index) = theme_items(&settings.theme, true, embedded, user);
        fill_themes(&c.theme, &items, index);
        self.ivars().themes.replace(items);
        let follows = settings.follows_system();
        let (items, index) = theme_items(&settings.light_theme, false, embedded, user);
        fill_themes(&c.light_theme, &items, index);
        self.ivars().light_themes.replace(items);
        let (items, index) = theme_items(&settings.dark_theme, false, embedded, user);
        fill_themes(&c.dark_theme, &items, index);
        self.ivars().dark_themes.replace(items);
        set_enabled(&c.light_theme, &c.light_label, follows);
        set_enabled(&c.dark_theme, &c.dark_label, follows);

        let (items, index) = font_items(settings.font.family.as_deref(), &self.ivars().families);
        fill_fonts(&c.font, &items, index);
        self.ivars().fonts.replace(items);
        set_number(
            &c.size,
            settings.font.size,
            &decimal_label(settings.font.size),
        );
        set_number(
            &c.line_height,
            settings.font.line_height,
            &decimal_label(settings.font.line_height),
        );

        select_choice(&c.shape, settings.cursor);
        select_choice(&c.blink, settings.cursor_blink);
        set_slide(
            &c.blink_speed,
            blink_to_position(settings.blink_interval),
            &seconds_label(settings.blink_interval),
        );
        let blinks = settings.cursor_blink != CursorBlink::Off;
        set_enabled(&c.blink_speed.slider, &c.blink_speed_label, blinks);
        let value_color = if blinks {
            NSColor::secondaryLabelColor()
        } else {
            NSColor::disabledControlTextColor()
        };
        c.blink_speed.value.setTextColor(Some(&value_color));
        set_slide(
            &c.radius,
            settings.caret.radius_ratio,
            &decimal_label(settings.caret.radius_ratio),
        );
        set_slide(
            &c.glow,
            settings.caret.glow,
            &decimal_label(settings.caret.glow),
        );
        select_choice(&c.unfocused, settings.caret.unfocused);

        select_choice(&c.cursor_motion, settings.cursor_motion);
        set_switch(&c.smooth_scroll, smooth_on(settings.smooth_scroll));
        select_choice(&c.reduce_motion, settings.reduce_motion);
    }

    /// Kabul edilmeyen girdiyi geri almak için: etkin ayarla yeniden doldur.
    fn refresh_from_delegate(&self) {
        if let Some(delegate) = app::delegate(self.mtm()) {
            delegate.refresh_settings_window();
        }
    }

    fn save(&self, edit: Option<SettingsEdit>) {
        let Some(delegate) = app::delegate(self.mtm()) else {
            return;
        };
        match edit {
            Some(edit) => delegate.save_edit(&edit),
            // Ayraç ya da tanınmayan kontrol: ekrandaki seçim dosyayla
            // ayrışmasın.
            None => delegate.refresh_settings_window(),
        }
    }

    /// Kenar çubuğunun seçimine göre başlığı ve ızgarayı değiştirir.
    fn show_selected(&self) {
        let (Some(sidebar), Some(header), Some(panes)) = (
            self.ivars().sidebar.get(),
            self.ivars().header.get(),
            self.ivars().panes.get(),
        ) else {
            return;
        };
        let Some(index) = usize::try_from(sidebar.selectedRow())
            .ok()
            .filter(|&index| index < Category::ALL.len())
        else {
            return;
        };
        header.setStringValue(&NSString::from_str(Category::ALL[index].title()));
        for (i, pane) in panes.iter().enumerate() {
            pane.setHidden(i != index);
        }
    }

    fn target(&self) -> &AnyObject {
        self.as_ref()
    }

    /// Bir kontrolü bu nesnenin eylemine bağlar.
    fn wire(&self, control: &NSControl, key: Key, action: Sel) {
        control.setTag(key.tag());
        // SAFETY: hedef zayıf referans; bu nesne `AppDelegate`'in ivar'ında
        // süreç boyunca yaşıyor.
        unsafe {
            control.setTarget(Some(self.target()));
            control.setAction(Some(action));
        }
    }

    fn popup<T: Choice>(&self, key: Key) -> Retained<NSPopUpButton> {
        let popup = new_popup(self.mtm());
        for title in choice_titles::<T>() {
            popup.addItemWithTitle(&NSString::from_str(title));
        }
        self.wire(&popup, key, sel!(popupChanged:));
        popup
    }

    fn string_popup(&self, key: Key) -> Retained<NSPopUpButton> {
        let popup = new_popup(self.mtm());
        self.wire(&popup, key, sel!(popupChanged:));
        popup
    }

    fn switch(&self, key: Key) -> Retained<NSSwitch> {
        let switch = NSSwitch::new(self.mtm());
        self.wire(&switch, key, sel!(switchChanged:));
        switch
    }

    fn number(&self, key: Key, min: f64, max: f64, step: f64, width: f64) -> Number {
        let mtm = self.mtm();
        let field = NSTextField::new(mtm);
        field.setAlignment(objc2_app_kit::NSTextAlignment::Right);
        width_constraint(&field, width);
        self.wire(&field, key, sel!(fieldChanged:));
        if let Some(cell) = field.cell() {
            // Odaktan çıkınca da eylem: Karar 5'in "Enter'da ya da odaktan
            // çıkınca"sı.
            cell.setSendsActionOnEndEditing(true);
        }
        let stepper = NSStepper::new(mtm);
        stepper.setMinValue(min);
        stepper.setMaxValue(max);
        stepper.setIncrement(step);
        stepper.setValueWraps(false);
        self.wire(&stepper, key, sel!(stepperChanged:));
        Number { field, stepper }
    }

    fn slide(&self, key: Key, min: f64, max: f64) -> Slide {
        let mtm = self.mtm();
        let slider = NSSlider::new(mtm);
        slider.setMinValue(min);
        slider.setMaxValue(max);
        slider.setContinuous(true);
        width_constraint(&slider, SLIDER_WIDTH);
        self.wire(&slider, key, sel!(sliderChanged:));
        let value = NSTextField::labelWithString(ns_string!(""), mtm);
        value.setTextColor(Some(&NSColor::secondaryLabelColor()));
        value.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(
            NSFont::systemFontSize(),
            0.0,
        )));
        Slide { slider, value }
    }

    /// Pencerenin iskeleti ve bütün kontroller.
    fn build(&self) {
        let mtm = self.mtm();
        let rect = NSRect::new(NSPoint::new(0.0, 0.0), WINDOW_SIZE);
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::FullSizeContentView;
        // SAFETY: defer=false ile pencere hemen yaratılır; `releasedWhenClosed`
        // hemen altında kapatılıyor (terminal penceresinin gerekçesi).
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect,
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // SAFETY: yalnız sahiplik semantiğini değiştirir; Retained sahibi biziz.
        // Kapatmak gizler, yeniden açınca aynı kategoride döner (Karar 4).
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(ns_string!("Settings"));
        // Başlık bölmenin başlığında; pencere başlığı Window menüsü için.
        window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        window.setTitlebarAppearsTransparent(true);
        // ⌘T ayar penceresine sekme eklemesin.
        window.setTabbingMode(NSWindowTabbingMode::Disallowed);

        let sidebar = self.build_sidebar();
        let detail = self.build_detail();

        let sidebar_controller = NSViewController::new(mtm);
        sidebar_controller.setView(&sidebar);
        let detail_controller = NSViewController::new(mtm);
        detail_controller.setView(&detail);
        let sidebar_item = NSSplitViewItem::sidebarWithViewController(&sidebar_controller);
        sidebar_item.setCanCollapse(false);
        sidebar_item.setMinimumThickness(SIDEBAR_WIDTH);
        sidebar_item.setMaximumThickness(SIDEBAR_WIDTH);
        let detail_item = NSSplitViewItem::splitViewItemWithViewController(&detail_controller);
        let split = NSSplitViewController::new(mtm);
        split.addSplitViewItem(&sidebar_item);
        split.addSplitViewItem(&detail_item);
        window.setContentViewController(Some(&split));
        window.setContentSize(WINDOW_SIZE);
        if let Some(table) = self.ivars().sidebar.get() {
            window.setInitialFirstResponder(Some(table));
        }
        let _ = self.ivars().window.set(window);

        if let Some(table) = self.ivars().sidebar.get() {
            table.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(0), false);
        }
        self.show_selected();
    }

    fn build_sidebar(&self) -> Retained<NSView> {
        let mtm = self.mtm();
        let table = NSTableView::new(mtm);
        table.setStyle(NSTableViewStyle::SourceList);
        table.setHeaderView(None);
        table.setAllowsEmptySelection(false);
        let column = NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), ns_string!("c"));
        table.addTableColumn(&column);
        // SAFETY: kaynak ve delegate zayıf referans; bu nesne süreç boyunca
        // yaşıyor (`AppDelegate`'in ivar'ı).
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(self)));
            table.setDelegate(Some(ProtocolObject::from_ref(self)));
        }
        let scroll = NSScrollView::new(mtm);
        scroll.setDocumentView(Some(&table));
        scroll.setDrawsBackground(false);
        scroll.setHasVerticalScroller(false);
        let _ = self.ivars().sidebar.set(table);
        Retained::into_super(scroll)
    }

    fn build_detail(&self) -> Retained<NSView> {
        let mtm = self.mtm();
        let detail = NSView::new(mtm);

        let header = NSTextField::labelWithString(ns_string!(""), mtm);
        header.setFont(Some(&NSFont::boldSystemFontOfSize(17.0)));
        add_pinned(&detail, &header);
        let safe = detail.safeAreaLayoutGuide();
        activate(&[
            header
                .topAnchor()
                .constraintEqualToAnchor_constant(&safe.topAnchor(), 4.0),
            header
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&detail.leadingAnchor(), MARGIN),
        ]);

        let (panes, controls) = self.build_panes();
        for pane in &panes {
            add_pinned(&detail, pane);
            activate(&[
                pane.topAnchor()
                    .constraintEqualToAnchor_constant(&header.bottomAnchor(), 18.0),
                pane.leadingAnchor()
                    .constraintEqualToAnchor_constant(&detail.leadingAnchor(), MARGIN),
            ]);
        }

        // SAFETY: hedef zayıf referans ve süreç boyunca yaşıyor; seçici bu
        // sınıfın `openFile:`'ı.
        let open = unsafe {
            NSButton::buttonWithTitle_target_action(
                ns_string!("Open settings.toml"),
                Some(self.target()),
                Some(sel!(openFile:)),
                mtm,
            )
        };
        add_pinned(&detail, &open);
        activate(&[
            open.trailingAnchor()
                .constraintEqualToAnchor_constant(&detail.trailingAnchor(), -MARGIN),
            open.bottomAnchor()
                .constraintEqualToAnchor_constant(&detail.bottomAnchor(), -MARGIN),
        ]);

        let _ = self.ivars().header.set(header);
        let _ = self.ivars().panes.set(panes);
        let _ = self.ivars().controls.set(controls);
        detail
    }

    fn build_panes(&self) -> (Vec<Retained<NSGridView>>, Controls) {
        let mtm = self.mtm();

        // General
        let confirm_close = self.popup::<ConfirmClose>(Key::ConfirmClose);
        let clipboard = self.switch(Key::Clipboard);
        let scrollback = self.number(Key::Scrollback, 0.0, SCROLLBACK_MAX as f64, 1000.0, 80.0);
        let shell_integration = self.popup::<ShellIntegration>(Key::ShellIntegration);
        let general = Form::new(mtm);
        general.row("Confirm before closing:", &confirm_close, None);
        general.row(
            "Clipboard access:",
            &clipboard,
            Some("Lets programs copy to the clipboard, even over ssh (OSC 52)."),
        );
        general.row("Scrollback lines:", &number_view(mtm, &scrollback), None);
        general.row(
            "Shell integration:",
            &shell_integration,
            Some("Takes effect in new tabs and windows."),
        );

        // Appearance
        let theme = self.string_popup(Key::Theme);
        let light_theme = self.string_popup(Key::LightTheme);
        let dark_theme = self.string_popup(Key::DarkTheme);
        let font = self.string_popup(Key::Font);
        let size = self.number(Key::Size, MIN_SIZE, MAX_SIZE, 1.0, 56.0);
        let line_height = self.number(
            Key::LineHeight,
            *LINE_HEIGHT_RANGE.start(),
            *LINE_HEIGHT_RANGE.end(),
            0.1,
            56.0,
        );
        let appearance = Form::new(mtm);
        appearance.row("Theme:", &theme, None);
        let light_label = appearance.row("Light theme:", &light_theme, None);
        let dark_label = appearance.row(
            "Dark theme:",
            &dark_theme,
            Some("Used when Theme is Match System."),
        );
        appearance.row("Font:", &font, None);
        appearance.row("Size:", &number_view(mtm, &size), None);
        appearance.row("Line height:", &number_view(mtm, &line_height), None);

        // Cursor
        let shape = self.popup::<CaretShape>(Key::Shape);
        let blink = self.popup::<CursorBlink>(Key::Blink);
        let blink_speed = self.slide(Key::BlinkSpeed, 0.0, 1.0);
        let radius = self.slide(
            Key::Radius,
            *CURSOR_RADIUS_RANGE.start(),
            *CURSOR_RADIUS_RANGE.end(),
        );
        let glow = self.slide(
            Key::Glow,
            *CURSOR_GLOW_RANGE.start(),
            *CURSOR_GLOW_RANGE.end(),
        );
        let unfocused = self.popup::<UnfocusedCaret>(Key::Unfocused);
        let cursor = Form::new(mtm);
        cursor.row(
            "Shape:",
            &shape,
            Some("Programs like vim can change it while they run."),
        );
        cursor.row(
            "Blink:",
            &blink,
            Some("Follow program blinks only when the running program asks."),
        );
        let blink_speed_label = cursor.row("Blink speed:", &slide_view(mtm, &blink_speed), None);
        cursor.row("Corner radius:", &slide_view(mtm, &radius), None);
        cursor.row("Glow:", &slide_view(mtm, &glow), None);
        cursor.row(
            "When unfocused:",
            &unfocused,
            Some("How the cursor looks in a window that is not active."),
        );

        // Motion
        let cursor_motion = self.popup::<CursorMotion>(Key::CursorMotion);
        let smooth_scroll = self.switch(Key::SmoothScroll);
        let reduce_motion = self.popup::<ReduceMotion>(Key::ReduceMotion);
        let motion = Form::new(mtm);
        motion.row(
            "Cursor motion:",
            &cursor_motion,
            Some("How the cursor travels to its new place."),
        );
        motion.row("Smooth scrolling:", &smooth_scroll, None);
        motion.row(
            "Reduce motion:",
            &reduce_motion,
            Some("On turns animations into fades and instant jumps."),
        );

        let panes = vec![general.grid, appearance.grid, cursor.grid, motion.grid];
        let controls = Controls {
            confirm_close,
            clipboard,
            scrollback,
            shell_integration,
            theme,
            light_theme,
            light_label,
            dark_theme,
            dark_label,
            font,
            size,
            line_height,
            shape,
            blink,
            blink_speed,
            blink_speed_label,
            radius,
            glow,
            unfocused,
            cursor_motion,
            smooth_scroll,
            reduce_motion,
        };
        (panes, controls)
    }
}

/// Bir bölmenin ızgarası: sol sütun sağa yaslı etiket, sağ sütun kontrol;
/// açıklama kontrolün altında kendi satırında, küçük ve ikincil renkte.
struct Form {
    mtm: MainThreadMarker,
    grid: Retained<NSGridView>,
}

impl Form {
    fn new(mtm: MainThreadMarker) -> Self {
        let grid = NSGridView::new(mtm);
        grid.setRowSpacing(6.0);
        grid.setColumnSpacing(10.0);
        grid.setRowAlignment(NSGridRowAlignment::FirstBaseline);
        Form { mtm, grid }
    }

    /// Satırı ekler, etiketini döndürür (bağımlı satırın soluklaşması için).
    fn row(&self, label: &str, control: &NSView, note: Option<&str>) -> Retained<NSTextField> {
        let mtm = self.mtm;
        let text = NSTextField::labelWithString(&NSString::from_str(label), mtm);
        let first = self.grid.numberOfRows() == 0;
        let row = self
            .grid
            .addRowWithViews(&NSArray::from_slice(&[text.as_super().as_super(), control]));
        if !first {
            // Satır grupları arasında açıklamanın payından geniş bir boşluk.
            row.setTopPadding(10.0);
        }
        if self.grid.numberOfRows() == 1 {
            let labels = self.grid.columnAtIndex(0);
            labels.setXPlacement(NSGridCellPlacement::Trailing);
            labels.setWidth(LABEL_WIDTH);
        }
        if let Some(note) = note {
            let empty = NSGridCell::emptyContentView(mtm);
            let note = NSTextField::wrappingLabelWithString(&NSString::from_str(note), mtm);
            note.setFont(Some(&NSFont::systemFontOfSize(
                NSFont::smallSystemFontSize(),
            )));
            note.setTextColor(Some(&NSColor::secondaryLabelColor()));
            note.setPreferredMaxLayoutWidth(NOTE_WIDTH);
            let row = self
                .grid
                .addRowWithViews(&NSArray::from_slice(&[&*empty, note.as_super().as_super()]));
            row.setTopPadding(-2.0);
        }
        text
    }
}

/// Alan + stepper yan yana.
fn number_view(mtm: MainThreadMarker, number: &Number) -> Retained<NSView> {
    hstack(
        mtm,
        &[
            number.field.as_super().as_super(),
            number.stepper.as_super().as_super(),
        ],
        4.0,
    )
}

/// Slider + değer etiketi yan yana.
fn slide_view(mtm: MainThreadMarker, slide: &Slide) -> Retained<NSView> {
    hstack(
        mtm,
        &[
            slide.slider.as_super().as_super(),
            slide.value.as_super().as_super(),
        ],
        8.0,
    )
}

fn hstack(mtm: MainThreadMarker, views: &[&NSView], spacing: f64) -> Retained<NSView> {
    let stack = NSStackView::stackViewWithViews(&NSArray::from_slice(views), mtm);
    stack.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
    stack.setSpacing(spacing);
    Retained::into_super(stack)
}

fn new_popup(mtm: MainThreadMarker) -> Retained<NSPopUpButton> {
    let popup = NSPopUpButton::new(mtm);
    popup.setPullsDown(false);
    width_constraint(&popup, POPUP_WIDTH);
    popup
}

fn width_constraint(view: &NSView, width: f64) {
    view.widthAnchor()
        .constraintEqualToConstant(width)
        .setActive(true);
}

fn add_pinned(parent: &NSView, child: &NSView) {
    child.setTranslatesAutoresizingMaskIntoConstraints(false);
    parent.addSubview(child);
}

fn activate(constraints: &[Retained<NSLayoutConstraint>]) {
    for constraint in constraints {
        constraint.setActive(true);
    }
}

/// Kenar çubuğunun bir satırı: SF Symbol + başlık.
fn sidebar_cell(mtm: MainThreadMarker, category: Category) -> Retained<NSTableCellView> {
    let cell = NSTableCellView::new(mtm);
    let label = NSTextField::labelWithString(&NSString::from_str(category.title()), mtm);
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str(category.symbol()),
        None,
    );
    let icon = NSImageView::new(mtm);
    if let Some(image) = image {
        icon.setImage(Some(&image));
    }
    add_pinned(&cell, &icon);
    add_pinned(&cell, &label);
    activate(&[
        icon.leadingAnchor()
            .constraintEqualToAnchor_constant(&cell.leadingAnchor(), 4.0),
        icon.centerYAnchor()
            .constraintEqualToAnchor(&cell.centerYAnchor()),
        icon.widthAnchor().constraintEqualToConstant(18.0),
        label
            .leadingAnchor()
            .constraintEqualToAnchor_constant(&icon.trailingAnchor(), 6.0),
        label
            .centerYAnchor()
            .constraintEqualToAnchor(&cell.centerYAnchor()),
    ]);
    // SAFETY: iki alan da zayıf; görünümler hücrenin alt görünümü olarak
    // hücreyle yaşıyor.
    unsafe {
        cell.setImageView(Some(&icon));
        cell.setTextField(Some(&label));
    }
    cell
}

fn select_choice<T: Choice>(popup: &NSPopUpButton, value: T) {
    if let Some(index) = choice_index(value) {
        popup.selectItemAtIndex(index as NSInteger);
    }
}

fn set_switch(switch: &NSSwitch, on: bool) {
    switch.setState(if on {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
}

fn set_number(number: &Number, value: f64, text: &str) {
    number.field.setStringValue(&NSString::from_str(text));
    number.stepper.setDoubleValue(value);
}

fn set_slide(slide: &Slide, position: f64, text: &str) {
    slide.slider.setDoubleValue(position);
    slide.value.setStringValue(&NSString::from_str(text));
}

/// Bağımlı satır: kontrol devre dışı, etiket soluk — gizlenmiyor (Karar 6).
fn set_enabled(control: &NSControl, label: &NSTextField, enabled: bool) {
    control.setEnabled(enabled);
    let color = if enabled {
        NSColor::labelColor()
    } else {
        NSColor::disabledControlTextColor()
    };
    label.setTextColor(Some(&color));
}

fn fill_themes(popup: &NSPopUpButton, items: &[ThemeItem], selected: usize) {
    let titles = items.iter().map(|item| match item {
        ThemeItem::System => Some("Match System".to_owned()),
        ThemeItem::Separator => None,
        ThemeItem::Named(name) => Some(name.clone()),
    });
    fill_popup(popup, titles, selected);
}

fn fill_fonts(popup: &NSPopUpButton, items: &[FontItem], selected: usize) {
    let titles = items.iter().map(|item| match item {
        FontItem::Default => Some("Default (SF Mono, or Menlo)".to_owned()),
        FontItem::Separator => None,
        FontItem::Family(name) => Some(name.clone()),
        FontItem::Missing(name) => Some(missing_font_title(name, bt_gpu::family_notice(name))),
    });
    fill_popup(popup, titles, selected);
}

/// Popup'ı baştan kurar. `addItemWithTitle:` **değil** menüye doğrudan
/// ekleme: o yöntem aynı başlıklı öğeyi tekilleştiriyor ve ayraç ekleyemiyor.
fn fill_popup(
    popup: &NSPopUpButton,
    titles: impl Iterator<Item = Option<String>>,
    selected: usize,
) {
    let mtm = popup.mtm();
    popup.removeAllItems();
    let Some(menu) = popup.menu() else {
        return;
    };
    for title in titles {
        let item = match title {
            Some(title) => {
                // SAFETY: eylemsiz öğe; popup'ın kendi eylemi seçimi taşıyor.
                unsafe {
                    NSMenuItem::initWithTitle_action_keyEquivalent(
                        NSMenuItem::alloc(mtm),
                        &NSString::from_str(&title),
                        None,
                        ns_string!(""),
                    )
                }
            }
            None => NSMenuItem::separatorItem(mtm),
        };
        menu.addItem(&item);
    }
    popup.selectItemAtIndex(selected as NSInteger);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Her popup'ın başlıkları `NAMES`'in her varyantını kapsıyor, boş ve
    /// yinelenen başlık yok, indeks ↔ varyant iki yönde tutarlı.
    fn check<T: Choice + std::fmt::Debug>() {
        let titles = choice_titles::<T>();
        assert_eq!(titles.len(), T::names().len());
        for (i, title) in titles.iter().enumerate() {
            assert!(!title.is_empty(), "boş başlık: {i}");
            assert_eq!(
                titles.iter().filter(|other| *other == title).count(),
                1,
                "yinelenen başlık: {title}"
            );
            let value = choice_at::<T>(i as NSInteger).expect("indeks bir varyant");
            assert_eq!(choice_index(value), Some(i), "{value:?}");
        }
        assert_eq!(choice_at::<T>(-1), None);
        assert_eq!(choice_at::<T>(titles.len() as NSInteger), None);
    }

    #[test]
    fn popup_titles_cover_every_name() {
        check::<ConfirmClose>();
        check::<ShellIntegration>();
        check::<CaretShape>();
        check::<CursorBlink>();
        check::<UnfocusedCaret>();
        check::<CursorMotion>();
        check::<ReduceMotion>();
    }

    #[test]
    fn switches_cover_both_names() {
        for &(_, mode) in Osc52::NAMES {
            assert_eq!(osc52_on(mode), mode == Osc52::Copy);
        }
        for &(_, smooth) in SmoothScroll::NAMES {
            assert_eq!(smooth_on(smooth), smooth == SmoothScroll::On);
        }
    }

    #[test]
    fn keys_round_trip_through_their_tags() {
        for (i, key) in Key::ALL.iter().enumerate() {
            assert_eq!(key.tag(), i as NSInteger);
            assert_eq!(Key::from_tag(key.tag()), Some(*key));
        }
        assert_eq!(Key::from_tag(-1), None);
        assert_eq!(Key::from_tag(Key::ALL.len() as NSInteger), None);
    }

    #[test]
    fn blink_slider_ends_are_the_range_ends() {
        let (min, max) = (*CURSOR_BLINK_RANGE.start(), *CURSOR_BLINK_RANGE.end());
        // Sol yavaş (uzun yarım periyot), sağ hızlı.
        assert_eq!(blink_from_position(0.0), max);
        assert_eq!(blink_from_position(1.0), min);
        assert_eq!(blink_from_position(-3.0), max);
        assert_eq!(blink_from_position(7.0), min);
        assert_eq!(blink_to_position(max), 0.0);
        assert_eq!(blink_to_position(min), 1.0);
        // Logaritmik: orta nokta geometrik ortalama.
        let middle = blink_from_position(0.5);
        assert!((middle - (min * max).sqrt()).abs() < 1e-9, "{middle}");
        for seconds in [0.1, 0.5, 1.0, 2.5] {
            let back = blink_from_position(blink_to_position(seconds));
            assert!((back - seconds).abs() < 1e-9, "{seconds} → {back}");
        }
    }

    #[test]
    fn decimals_are_shown_as_written() {
        assert_eq!(decimal_label(0.5), "0.5");
        assert_eq!(decimal_label(13.0), "13");
        assert_eq!(decimal_label(1.25), "1.25");
        assert_eq!(decimal_label(1.2000000000000002), "1.2");
        assert_eq!(seconds_label(0.5), "0.5 s");
    }

    #[test]
    fn fields_refuse_what_the_file_would_refuse() {
        assert_eq!(parse_scrollback(" 2500 "), Some(2500));
        assert_eq!(parse_scrollback("0"), Some(0));
        assert_eq!(parse_scrollback("-1"), None);
        assert_eq!(parse_scrollback("abc"), None);
        assert_eq!(parse_scrollback(&(SCROLLBACK_MAX + 1).to_string()), None);
        assert_eq!(parse_decimal("14.5", MIN_SIZE..=MAX_SIZE), Some(14.5));
        assert_eq!(parse_decimal("NaN", MIN_SIZE..=MAX_SIZE), None);
        assert_eq!(parse_decimal("500", MIN_SIZE..=MAX_SIZE), None);
        assert_eq!(parse_decimal("0.9", LINE_HEIGHT_RANGE), None);
    }

    #[test]
    fn theme_list_keeps_the_file_value_visible() {
        let user = vec!["paper".to_owned()];
        let (items, index) = theme_items(SYSTEM_THEME, true, &["bateri", "bateri-light"], &user);
        assert_eq!(items[index], ThemeItem::System);
        assert_eq!(
            theme_edit(&items, index as NSInteger).as_deref(),
            Some(SYSTEM_THEME)
        );
        let (items, index) = theme_items("paper", true, &["bateri"], &user);
        assert_eq!(
            theme_edit(&items, index as NSInteger).as_deref(),
            Some("paper")
        );
        // Silinmiş tema: sona eklenip seçili.
        let (items, index) = theme_items("gone", false, &["bateri"], &[]);
        assert_eq!(index, items.len() - 1);
        assert_eq!(items[index], ThemeItem::Named("gone".to_owned()));
        assert!(!items.contains(&ThemeItem::System));
        // Ayraç yazmaz.
        let (items, _) = theme_items("bateri", true, &["bateri"], &[]);
        assert_eq!(items[1], ThemeItem::Separator);
        assert_eq!(theme_edit(&items, 1), None);
    }

    #[test]
    fn font_list_matches_case_insensitively_and_keeps_unknowns() {
        let families = vec!["Menlo".to_owned(), "SF Mono".to_owned()];
        let (items, index) = font_items(None, &families);
        assert_eq!((items[index].clone(), index), (FontItem::Default, 0));
        assert_eq!(font_edit(&items, 0).as_deref(), Some(""));
        let (items, index) = font_items(Some("menlo"), &families);
        assert_eq!(items[index], FontItem::Family("Menlo".to_owned()));
        let (items, index) = font_items(Some("Comic Sans"), &families);
        assert_eq!(items[index], FontItem::Missing("Comic Sans".to_owned()));
        assert_eq!(font_edit(&items, index as NSInteger), None);
        assert_eq!(
            missing_font_title(
                "Helvetica",
                Some(FontNotice::NotMonospaced {
                    family: "Helvetica".to_owned()
                })
            ),
            "Helvetica — not monospaced"
        );
    }
}
