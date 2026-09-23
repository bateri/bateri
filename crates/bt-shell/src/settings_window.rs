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

use std::cell::{Cell, OnceCell, RefCell};

use bt_core::{
    CURSOR_BLINK_RANGE, CURSOR_GLOW_RANGE, CURSOR_RADIUS_RANGE, CaretShape, ConfirmClose,
    CursorBlink, CursorMotion, LINE_HEIGHT_RANGE, Osc52, ReduceMotion, SCROLLBACK_MAX,
    SYSTEM_THEME, Settings, SettingsEdit, ShellIntegration, SmoothScroll, UnfocusedCaret,
};
use bt_gpu::FontNotice;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{
    ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel,
};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSBox, NSBoxType, NSButton, NSColor, NSControl,
    NSControlStateValueOff, NSControlStateValueOn, NSControlTextEditingDelegate, NSEventType,
    NSFont, NSGridCell, NSGridCellPlacement, NSGridRow, NSGridRowAlignment, NSGridView, NSImage,
    NSImageView, NSLayoutAttribute, NSLayoutConstraint, NSMenuItem, NSPopUpButton, NSScrollView,
    NSSlider, NSSplitViewController, NSSplitViewItem, NSStackView, NSStepper, NSSwitch,
    NSTableCellView, NSTableColumn, NSTableView, NSTableViewDataSource, NSTableViewDelegate,
    NSTableViewStyle, NSTextField, NSTitlePosition, NSUserInterfaceLayoutOrientation, NSView,
    NSViewController, NSWindow, NSWindowStyleMask, NSWindowTabbingMode, NSWindowTitleVisibility,
};
use objc2_foundation::{
    NSArray, NSIndexSet, NSInteger, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect,
    NSSize, NSString, ns_string,
};

use crate::app;
use crate::settings::{self, FileState};
use crate::zoom::{MAX_SIZE, MIN_SIZE};

/// Pencerenin içerik boyu, punto. Sabit — pencere yeniden
/// boyutlandırılamıyor. En uzun bölme (Cursor: altı satır, dört açıklama)
/// üstünde iki satırlık şerit ve bir satır tanısıyla da düğmeye değmiyor;
/// 500'de şeritli Cursor bölmesi düğmeye yapışıyordu (phase-3 gözle
/// kontrolü). Tasarım sabiti, ölçülmüş bir sayı değil.
const WINDOW_SIZE: NSSize = NSSize::new(680.0, 560.0);
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
/// Şeridin metninin kırılma genişliği: sağ bölmenin genişliğinden iki kenar
/// payı, kutunun iki iç payı, sembol ve aralığı düşülmüş hâli.
const BANNER_TEXT_WIDTH: f64 =
    WINDOW_SIZE.width - SIDEBAR_WIDTH - 2.0 * MARGIN - 2.0 * 10.0 - 16.0 - 8.0;
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

    /// Dosyadaki noktalı yolu — `Diagnostic::key`'in dili; satırın tanısı bu
    /// eşleşmeyle bulunuyor. Ayrıştırıcıyla bağı bir sınama tutuyor
    /// (`every_row_receives_its_own_diagnostic`).
    fn path(self) -> &'static str {
        match self {
            Key::ConfirmClose => "terminal.confirm_close",
            Key::Clipboard => "clipboard.osc52",
            Key::Scrollback => "terminal.scrollback",
            Key::ShellIntegration => "shell.integration",
            Key::Theme => "appearance.theme",
            Key::LightTheme => "appearance.light_theme",
            Key::DarkTheme => "appearance.dark_theme",
            Key::Font => "font.family",
            Key::Size => "font.size",
            Key::LineHeight => "font.line_height",
            Key::Shape => "terminal.cursor",
            Key::Blink => "terminal.cursor_blink",
            Key::BlinkSpeed => "terminal.cursor_blink_interval",
            Key::Radius => "terminal.cursor_radius",
            Key::Glow => "terminal.cursor_glow",
            Key::Unfocused => "terminal.cursor_unfocused",
            Key::CursorMotion => "motion.cursor_motion",
            Key::SmoothScroll => "motion.smooth_scroll",
            Key::ReduceMotion => "motion.reduce_motion",
        }
    }
}

/// Kilitli pencerenin şeridinde sebebin altındaki cümle (029 phase-3).
const LOCK_HINT: &str = "Fix the file and save it; this window follows.";

/// Sağ bölmenin üstündeki şerit; satırı yoksa görünmez.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Banner {
    /// Alt başlığın metinleri, aynen: yazma hatası, kilidin sebebi, hiçbir
    /// satıra düşmeyen tanı.
    lines: Vec<String>,
    /// Altında, ikincil renkte: ne yapılacağı.
    hint: Option<&'static str>,
}

/// Pencerenin dosyanın hâlinden gördüğü (029 Karar 7) — saf, yani üç hâli
/// sınama pencere kurmadan görüyor.
#[derive(Debug, PartialEq, Eq)]
struct Status {
    /// Bütün kontroller devre dışı; "Open settings.toml" varsayılan düğme.
    locked: bool,
    banner: Banner,
    /// Kabul edilmeyen değerler: satır → tanının iletisi (açıklamanın
    /// yerine). Satır numarası ve dosya adı yok — satırın kendisi bağlam.
    rows: Vec<(Key, String)>,
}

/// Dosyanın hâli + yazma yuvası → pencerenin göreceği.
///
/// Bir satıra düşmeyen tanı (bölüm olmayan bölüm, emekli anahtar) şeride
/// gidiyor: alt başlıkta görünüp pencerede görünmeyen bir tanı kullanıcıyı
/// iki yere bakmaya zorlardı. Yazma hatası şeridin başında, çünkü
/// kullanıcının az önce yaptığı şeyin cevabı (alt başlığın sırası).
fn status(state: &FileState, write: &[String]) -> Status {
    let mut banner = Banner {
        lines: write.to_vec(),
        hint: None,
    };
    let mut rows = Vec::new();
    let locked = match state {
        FileState::Missing => false,
        FileState::Locked(reason) => {
            banner.lines.push(reason.clone());
            banner.hint = Some(LOCK_HINT);
            true
        }
        FileState::Usable(diagnostics) => {
            for diagnostic in diagnostics {
                let key = diagnostic
                    .key
                    .and_then(|path| Key::ALL.into_iter().find(|key| key.path() == path));
                match key {
                    Some(key) => rows.push((key, diagnostic.message.clone())),
                    None => banner.lines.push(settings::notice(diagnostic)),
                }
            }
            false
        }
    };
    Status {
        locked,
        banner,
        rows,
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
    /// Stepper'ın kendi aralığı; dosyadaki değer dışındaysa o değeri de
    /// kapsayacak kadar genişletiliyor ([`set_number`]).
    range: (f64, f64),
    /// Dosyanın değeri ve alanda gösterilen yazılışı — son tazelemeden.
    /// Değişmemiş bir alandan geçip çıkmak (Tab) yazmasın diye eylem buna
    /// bakıyor.
    shown: RefCell<(f64, String)>,
}

impl Number {
    /// Alanın metni dosyadakinin aynısı mı: yazılışı aynı ya da değeri
    /// yazılacak hassasiyette (iki basamak) aynı.
    fn unchanged(&self, text: &str, value: Option<f64>) -> bool {
        let shown = self.shown.borrow();
        let round = |value: f64| (value * 100.0).round();
        text.trim() == shown.1 || value.is_some_and(|value| round(value) == round(shown.0))
    }
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
    dark_theme: Retained<NSPopUpButton>,
    font: Retained<NSPopUpButton>,
    size: Number,
    line_height: Number,
    shape: Retained<NSPopUpButton>,
    blink: Retained<NSPopUpButton>,
    blink_speed: Slide,
    radius: Slide,
    glow: Slide,
    unfocused: Retained<NSPopUpButton>,
    cursor_motion: Retained<NSPopUpButton>,
    smooth_scroll: Retained<NSSwitch>,
    reduce_motion: Retained<NSPopUpButton>,
    /// Dört bölmenin satırları: kilit, bağımlı satır ve satır tanısı
    /// buradan.
    rows: Vec<Row>,
}

/// Izgaranın bir satırı: etiket, kontrolleri ve altındaki not satırı.
struct Row {
    key: Key,
    label: Retained<NSTextField>,
    controls: Vec<Retained<NSControl>>,
    /// Açıklama ya da tanı; ikisi de yoksa not satırı gizli (boşluk
    /// bırakmıyor).
    note: Retained<NSTextField>,
    note_row: Retained<NSGridRow>,
    description: Option<&'static str>,
}

impl Row {
    /// Satırın kontrolleri açık mı, etiketi soluk mu (Karar 6'nın bağımlı
    /// satırı ve Karar 7'nin kilidi aynı kapıdan).
    fn set_enabled(&self, enabled: bool) {
        for control in &self.controls {
            control.setEnabled(enabled);
        }
        let color = if enabled {
            NSColor::labelColor()
        } else {
            NSColor::disabledControlTextColor()
        };
        self.label.setTextColor(Some(&color));
    }

    /// Notu tanıya ya da açıklamaya kurar; ikisi de yoksa satırı gizler.
    /// Kapalı satırın açıklaması da etiketiyle birlikte soluyor.
    fn set_note(&self, diagnostic: Option<&str>, enabled: bool) {
        let (text, color) = match (diagnostic, self.description) {
            (Some(diagnostic), _) => (diagnostic, NSColor::systemOrangeColor()),
            (None, Some(description)) if enabled => (description, NSColor::secondaryLabelColor()),
            (None, Some(description)) => (description, NSColor::tertiaryLabelColor()),
            (None, None) => {
                self.note_row.setHidden(true);
                return;
            }
        };
        self.note.setStringValue(&NSString::from_str(text));
        self.note.setTextColor(Some(&color));
        self.note_row.setHidden(false);
    }
}

pub(crate) struct Ivars {
    window: OnceCell<Retained<NSWindow>>,
    sidebar: OnceCell<Retained<NSTableView>>,
    header: OnceCell<Retained<NSTextField>>,
    banner: OnceCell<BannerView>,
    /// Pencere bir kez gösterildi mi (ortalamanın kapısı).
    shown_once: Cell<bool>,
    pane_tops: OnceCell<PaneTops>,
    /// "Open settings.toml": kilitliyken varsayılan düğme (Enter).
    open: OnceCell<Retained<NSButton>>,
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
            // İzleme döngüsü sürükleme dışında olay da taşıyabiliyor (Force
            // Touch basıncı, periyodik olay): onlar da jestin ortası.
            let dragging = NSApplication::sharedApplication(self.mtm())
                .currentEvent()
                .is_some_and(|event| {
                    matches!(
                        event.r#type(),
                        NSEventType::LeftMouseDragged
                            | NSEventType::LeftMouseDown
                            | NSEventType::Pressure
                            | NSEventType::Periodic
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
            let Some(controls) = self.ivars().controls.get() else {
                return;
            };
            let (number, value) = match Key::from_tag(field.tag()) {
                Some(Key::Scrollback) => (
                    &controls.scrollback,
                    parse_scrollback(&text).map(|lines| lines as f64),
                ),
                Some(Key::Size) => (&controls.size, parse_decimal(&text, MIN_SIZE..=MAX_SIZE)),
                Some(Key::LineHeight) => (
                    &controls.line_height,
                    parse_decimal(&text, LINE_HEIGHT_RANGE),
                ),
                _ => return,
            };
            // Değişmeyen alandan geçmek yazmaz: yuvarlanmış yazılış
            // (`1.125` → "1.13") dosyadaki değeri sessizce değiştirirdi.
            if number.unchanged(&text, value) {
                return;
            }
            let edit = value.map(|value| match Key::from_tag(field.tag()) {
                Some(Key::Scrollback) => SettingsEdit::Scrollback(value as usize),
                Some(Key::Size) => SettingsEdit::FontSize(value),
                _ => SettingsEdit::LineHeight(value),
            });
            match edit {
                Some(edit) => self.save(Some(edit)),
                // Kabul edilmeyen girdi: alan dosyadaki yazılışa döner.
                // Doğrudan, tazelemeden değil — tazeleme düzenlenmekte olan
                // alana dokunmuyor ([`set_number`]).
                None => {
                    let shown = number.shown.borrow().1.clone();
                    field.setStringValue(&NSString::from_str(&shown));
                }
            }
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
            banner: OnceCell::new(),
            shown_once: Cell::new(false),
            pane_tops: OnceCell::new(),
            open: OnceCell::new(),
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

    /// Pencere açık mı (simge durumunda da) — kapalıyken tazelemenin
    /// anlamı yok, yeniden açılış tazeliyor.
    pub(crate) fn is_open(&self) -> bool {
        self.ivars()
            .window
            .get()
            .is_some_and(|window| window.isVisible() || window.isMiniaturized())
    }

    /// Pencereyi öne getirir (ilk açılışta ortalar); kategori son seçilen.
    pub(crate) fn show(&self) {
        let Some(window) = self.ivars().window.get() else {
            return;
        };
        // Yalnız ilk açılışta: kapatmak ve simge durumu da `isVisible`'ı
        // düşürüyor ve kullanıcının taşıdığı yer her açılışta kaybolurdu.
        if !self.ivars().shown_once.replace(true) {
            window.center();
        }
        NSApplication::sharedApplication(self.mtm()).activate();
        window.makeKeyAndOrderFront(None);
    }

    /// Kontrolleri etkin ayarla doldurur — pencerenin gösterdiği değerin tek
    /// kaynağı. Programla kurulan değer eylem tetiklemiyor, yani bir kontrolün
    /// eyleminin içinden (yaz → `reload_settings` → buraya) çağrılması döngü
    /// doğurmaz.
    ///
    /// Dosyanın hâli (`state`) ve yazma yuvası (`write`) kilidi, şeridi ve
    /// satır tanılarını kuruyor ([`status`]); her tazeleme hepsini baştan
    /// kurduğu için düzelen hâlin izi kalmıyor.
    pub(crate) fn refresh(
        &self,
        settings: &Settings,
        state: &FileState,
        write: &[String],
        embedded: &[&str],
        user: &[String],
    ) {
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

        let status = status(state, write);
        for row in &c.rows {
            let depends = match row.key {
                Key::LightTheme | Key::DarkTheme => follows,
                Key::BlinkSpeed => blinks,
                _ => true,
            };
            let enabled = !status.locked && depends;
            row.set_enabled(enabled);
            let diagnostic = status
                .rows
                .iter()
                .find(|(key, _)| *key == row.key)
                .map(|(_, message)| message.as_str());
            row.set_note(diagnostic, enabled);
        }
        // Değer etiketi bir etiket, kontrol değil: soluklaşması elle.
        let value_color = if !status.locked && blinks {
            NSColor::secondaryLabelColor()
        } else {
            NSColor::disabledControlTextColor()
        };
        c.blink_speed.value.setTextColor(Some(&value_color));
        for slide in [&c.radius, &c.glow] {
            let color = if status.locked {
                NSColor::disabledControlTextColor()
            } else {
                NSColor::secondaryLabelColor()
            };
            slide.value.setTextColor(Some(&color));
        }
        if let Some(banner) = self.ivars().banner.get() {
            banner.show(&status.banner);
        }
        self.layout_panes(!status.banner.lines.is_empty());
        if let Some(open) = self.ivars().open.get() {
            // Kilitte dosyayı onarmak bir tık — Enter — uzakta.
            open.setKeyEquivalent(if status.locked {
                ns_string!("\r")
            } else {
                ns_string!("")
            });
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

    /// Izgaraları şeridin altına ya da başlığın altına bağlar. Önce eski
    /// takım bırakılıyor: ikisi bir an birlikte etkin olsa çelişirlerdi.
    fn layout_panes(&self, banner_shown: bool) {
        let Some(tops) = self.ivars().pane_tops.get() else {
            return;
        };
        let (on, off) = if banner_shown {
            (&tops.under_banner, &tops.under_header)
        } else {
            (&tops.under_header, &tops.under_banner)
        };
        for constraint in off {
            constraint.setActive(false);
        }
        activate(on);
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
        Number {
            field,
            stepper,
            range: (min, max),
            shown: RefCell::new((0.0, String::new())),
        }
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

        let banner = BannerView::new(mtm);
        add_pinned(&detail, &banner.frame);
        activate(&[
            banner
                .frame
                .topAnchor()
                .constraintEqualToAnchor_constant(&header.bottomAnchor(), 12.0),
            banner
                .frame
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&detail.leadingAnchor(), MARGIN),
            banner
                .frame
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&detail.trailingAnchor(), -MARGIN),
        ]);

        // Izgaranın tepesi iki yerden birine bağlı: şerit yokken başlığa,
        // varken şeride ([`SettingsWindow::layout_panes`]). Şerit gizlenince
        // yer kaplamıyor, yani ızgara başlığın altına geri çıkıyor.
        let (panes, controls) = self.build_panes();
        let mut under_header = Vec::new();
        let mut under_banner = Vec::new();
        for pane in &panes {
            add_pinned(&detail, pane);
            activate(&[pane
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&detail.leadingAnchor(), MARGIN)]);
            under_header.push(
                pane.topAnchor()
                    .constraintEqualToAnchor_constant(&header.bottomAnchor(), 18.0),
            );
            under_banner.push(
                pane.topAnchor()
                    .constraintEqualToAnchor_constant(&banner.frame.bottomAnchor(), 16.0),
            );
        }
        activate(&under_header);
        banner.frame.setHidden(true);

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
        let _ = self.ivars().banner.set(banner);
        let _ = self.ivars().pane_tops.set(PaneTops {
            under_header,
            under_banner,
        });
        let _ = self.ivars().open.set(open);
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
        let mut general = Form::new(mtm);
        general.row(
            Key::ConfirmClose,
            "Confirm before closing:",
            &confirm_close,
            &[&confirm_close],
            None,
        );
        general.row(
            Key::Clipboard,
            "Clipboard access:",
            &clipboard,
            &[&clipboard],
            Some("Lets programs copy to the clipboard, even over ssh (OSC 52)."),
        );
        general.row(
            Key::Scrollback,
            "Scrollback lines:",
            &number_view(mtm, &scrollback),
            &number_controls(&scrollback),
            None,
        );
        general.row(
            Key::ShellIntegration,
            "Shell integration:",
            &shell_integration,
            &[&shell_integration],
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
        let mut appearance = Form::new(mtm);
        appearance.row(Key::Theme, "Theme:", &theme, &[&theme], None);
        appearance.row(
            Key::LightTheme,
            "Light theme:",
            &light_theme,
            &[&light_theme],
            None,
        );
        appearance.row(
            Key::DarkTheme,
            "Dark theme:",
            &dark_theme,
            &[&dark_theme],
            Some("Used when Theme is Match System."),
        );
        appearance.row(Key::Font, "Font:", &font, &[&font], None);
        appearance.row(
            Key::Size,
            "Size:",
            &number_view(mtm, &size),
            &number_controls(&size),
            None,
        );
        appearance.row(
            Key::LineHeight,
            "Line height:",
            &number_view(mtm, &line_height),
            &number_controls(&line_height),
            None,
        );

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
        let mut cursor = Form::new(mtm);
        cursor.row(
            Key::Shape,
            "Shape:",
            &shape,
            &[&shape],
            Some("Programs like vim can change it while they run."),
        );
        cursor.row(
            Key::Blink,
            "Blink:",
            &blink,
            &[&blink],
            Some("Follow program blinks only when the running program asks."),
        );
        cursor.row(
            Key::BlinkSpeed,
            "Blink speed:",
            &slide_view(mtm, &blink_speed),
            &[&blink_speed.slider],
            None,
        );
        cursor.row(
            Key::Radius,
            "Corner radius:",
            &slide_view(mtm, &radius),
            &[&radius.slider],
            None,
        );
        cursor.row(
            Key::Glow,
            "Glow:",
            &slide_view(mtm, &glow),
            &[&glow.slider],
            None,
        );
        cursor.row(
            Key::Unfocused,
            "When unfocused:",
            &unfocused,
            &[&unfocused],
            Some("How the cursor looks in a window that is not active."),
        );

        // Motion
        let cursor_motion = self.popup::<CursorMotion>(Key::CursorMotion);
        let smooth_scroll = self.switch(Key::SmoothScroll);
        let reduce_motion = self.popup::<ReduceMotion>(Key::ReduceMotion);
        let mut motion = Form::new(mtm);
        motion.row(
            Key::CursorMotion,
            "Cursor motion:",
            &cursor_motion,
            &[&cursor_motion],
            Some("How the cursor travels to its new place."),
        );
        motion.row(
            Key::SmoothScroll,
            "Smooth scrolling:",
            &smooth_scroll,
            &[&smooth_scroll],
            None,
        );
        motion.row(
            Key::ReduceMotion,
            "Reduce motion:",
            &reduce_motion,
            &[&reduce_motion],
            Some("On turns animations into fades and instant jumps."),
        );

        let rows = [general.rows, appearance.rows, cursor.rows, motion.rows]
            .into_iter()
            .flatten()
            .collect();
        let panes = vec![general.grid, appearance.grid, cursor.grid, motion.grid];
        let controls = Controls {
            confirm_close,
            clipboard,
            scrollback,
            shell_integration,
            theme,
            light_theme,
            dark_theme,
            font,
            size,
            line_height,
            shape,
            blink,
            blink_speed,
            radius,
            glow,
            unfocused,
            cursor_motion,
            smooth_scroll,
            reduce_motion,
            rows,
        };
        (panes, controls)
    }
}

/// Izgaraların tepesini bağlayan iki kısıt takımı; biri etkin.
struct PaneTops {
    under_header: Vec<Retained<NSLayoutConstraint>>,
    under_banner: Vec<Retained<NSLayoutConstraint>>,
}

/// Şeridin görünümü: hafif turuncu zeminli yuvarlak bir kutu, solda uyarı
/// sembolü, sağda metin ve altında ikincil renkte ne yapılacağı. Renkler
/// sistemin anlamsal renkleri — açık ve koyu görünümde ayrı ayrı doğru.
struct BannerView {
    frame: Retained<NSBox>,
    lines: Retained<NSTextField>,
    hint: Retained<NSTextField>,
}

impl BannerView {
    fn new(mtm: MainThreadMarker) -> Self {
        let frame = NSBox::new(mtm);
        frame.setBoxType(NSBoxType::Custom);
        frame.setTitlePosition(NSTitlePosition::NoTitle);
        frame.setCornerRadius(8.0);
        frame.setBorderWidth(1.0);
        let orange = NSColor::systemOrangeColor();
        frame.setFillColor(&orange.colorWithAlphaComponent(0.10));
        frame.setBorderColor(&orange.colorWithAlphaComponent(0.35));
        frame.setContentViewMargins(NSSize::new(10.0, 8.0));

        let icon = NSImageView::new(mtm);
        if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            ns_string!("exclamationmark.triangle.fill"),
            None,
        ) {
            icon.setImage(Some(&image));
        }
        icon.setContentTintColor(Some(&orange));
        let lines = NSTextField::wrappingLabelWithString(ns_string!(""), mtm);
        lines.setPreferredMaxLayoutWidth(BANNER_TEXT_WIDTH);
        let hint = NSTextField::wrappingLabelWithString(ns_string!(""), mtm);
        hint.setFont(Some(&NSFont::systemFontOfSize(
            NSFont::smallSystemFontSize(),
        )));
        hint.setTextColor(Some(&NSColor::secondaryLabelColor()));
        hint.setPreferredMaxLayoutWidth(BANNER_TEXT_WIDTH);

        let text = NSStackView::stackViewWithViews(
            &NSArray::from_slice(&[lines.as_super().as_super(), hint.as_super().as_super()]),
            mtm,
        );
        text.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        text.setAlignment(NSLayoutAttribute::Leading);
        text.setSpacing(2.0);
        let content = NSView::new(mtm);
        add_pinned(&content, &icon);
        add_pinned(&content, &text);
        activate(&[
            icon.leadingAnchor()
                .constraintEqualToAnchor(&content.leadingAnchor()),
            icon.firstBaselineAnchor()
                .constraintEqualToAnchor(&lines.firstBaselineAnchor()),
            icon.widthAnchor().constraintEqualToConstant(16.0),
            text.leadingAnchor()
                .constraintEqualToAnchor_constant(&icon.trailingAnchor(), 8.0),
            text.trailingAnchor()
                .constraintLessThanOrEqualToAnchor(&content.trailingAnchor()),
            text.topAnchor()
                .constraintEqualToAnchor(&content.topAnchor()),
            text.bottomAnchor()
                .constraintEqualToAnchor(&content.bottomAnchor()),
        ]);
        frame.setContentView(Some(&content));
        BannerView { frame, lines, hint }
    }

    /// Şeridi kurar; satırı yoksa gizler.
    fn show(&self, banner: &Banner) {
        if banner.lines.is_empty() {
            self.frame.setHidden(true);
            return;
        }
        self.lines
            .setStringValue(&NSString::from_str(&banner.lines.join("\n")));
        match banner.hint {
            Some(hint) => {
                self.hint.setStringValue(&NSString::from_str(hint));
                self.hint.setHidden(false);
            }
            None => self.hint.setHidden(true),
        }
        self.frame.setHidden(false);
    }
}

/// Bir bölmenin ızgarası: sol sütun sağa yaslı etiket, sağ sütun kontrol;
/// açıklama kontrolün altında kendi satırında, küçük ve ikincil renkte.
struct Form {
    mtm: MainThreadMarker,
    grid: Retained<NSGridView>,
    rows: Vec<Row>,
}

impl Form {
    fn new(mtm: MainThreadMarker) -> Self {
        let grid = NSGridView::new(mtm);
        grid.setRowSpacing(6.0);
        grid.setColumnSpacing(10.0);
        grid.setRowAlignment(NSGridRowAlignment::FirstBaseline);
        Form {
            mtm,
            grid,
            rows: Vec::new(),
        }
    }

    /// Satırı ve altındaki not satırını ekler. Not satırı açıklaması olmayan
    /// satırda da var — gizli; kabul edilmeyen değerin tanısı oraya çıkıyor
    /// (Karar 7).
    fn row(
        &mut self,
        key: Key,
        label: &str,
        control: &NSView,
        controls: &[&NSControl],
        description: Option<&'static str>,
    ) {
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
        let empty = NSGridCell::emptyContentView(mtm);
        let note = NSTextField::wrappingLabelWithString(ns_string!(""), mtm);
        note.setFont(Some(&NSFont::systemFontOfSize(
            NSFont::smallSystemFontSize(),
        )));
        note.setPreferredMaxLayoutWidth(NOTE_WIDTH);
        let note_row = self
            .grid
            .addRowWithViews(&NSArray::from_slice(&[&*empty, note.as_super().as_super()]));
        note_row.setTopPadding(-2.0);
        let row = Row {
            key,
            label: text,
            controls: controls.iter().map(|control| control.retain()).collect(),
            note,
            note_row,
            description,
        };
        row.set_note(None, true);
        self.rows.push(row);
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

/// Alanın ve stepper'ın ikisi de satırın kontrolü (kilit ikisini birden
/// kapatıyor).
fn number_controls(number: &Number) -> [&NSControl; 2] {
    [&number.field, &number.stepper]
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

/// Alanı ve stepper'ı dosyanın değerine kurar.
///
/// - **Düzenlenmekte olan alana dokunulmaz**: tazeleme her kayıtta geliyor
///   (dışarıdan kayıt, başka bir kontrolün yazması, izleyicinin ardından
///   gelen olayı) ve değer atamak düzenlemeyi iptal edip yazılanı silerdi.
///   Gösterilen değer yine güncellenir; alanın eylemi ona bakıyor.
/// - **Stepper'ın aralığı dosyadaki değeri kapsar**: `size = 100` ayrıştırıcı
///   için geçerli ve stepper onu 72'ye kırpsaydı "yukarı" tıkı küçültürdü.
fn set_number(number: &Number, value: f64, text: &str) {
    *number.shown.borrow_mut() = (value, text.to_owned());
    if number.field.currentEditor().is_none() {
        number.field.setStringValue(&NSString::from_str(text));
    }
    let (min, max) = number.range;
    number.stepper.setMinValue(min.min(value));
    number.stepper.setMaxValue(max.max(value));
    number.stepper.setDoubleValue(value);
}

fn set_slide(slide: &Slide, position: f64, text: &str) {
    slide.slider.setDoubleValue(position);
    slide.value.setStringValue(&NSString::from_str(text));
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
    use bt_core::Diagnostic;

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

    /// Her satırın anahtarı ayrıştırıcının tanısında geçen anahtarın ta
    /// kendisi: bütün anahtarları yanlış türde yazan bir dosyanın tanıları
    /// satırlara bire bir düşüyor, eşleşmeyen ne tanı ne satır kalıyor.
    #[test]
    fn every_row_receives_its_own_diagnostic() {
        let text = "[terminal]\nscrollback = []\ncursor = []\ncursor_blink = []\n\
                    cursor_radius = []\ncursor_glow = []\ncursor_unfocused = []\n\
                    cursor_blink_interval = []\nconfirm_close = []\n\
                    [appearance]\ntheme = []\nlight_theme = []\ndark_theme = []\n\
                    [font]\nfamily = []\nsize = []\nline_height = []\n\
                    [clipboard]\nosc52 = []\n\
                    [motion]\ncursor_motion = []\nreduce_motion = []\nsmooth_scroll = []\n\
                    [shell]\nintegration = []\n";
        let parsed = Settings::parse_keeping(text, &Settings::default()).expect("ayrıştırılır");
        let seen = status(&FileState::Usable(parsed.diagnostics), &[]);
        assert_eq!(seen.banner, Banner::default(), "eşleşmeyen tanı yok");
        let mut keys: Vec<Key> = seen.rows.iter().map(|(key, _)| *key).collect();
        keys.sort_by_key(|key| key.tag());
        assert_eq!(keys, Key::ALL);
        // Yazma tarafı da aynı yolu söylüyor: satırın düzenlemesi, satırın
        // anahtarı (yazma reddinin tanısı da o satıra düşsün).
        for key in Key::ALL {
            let edit = match key {
                Key::ConfirmClose => SettingsEdit::ConfirmClose(ConfirmClose::Never),
                Key::Clipboard => SettingsEdit::Osc52(Osc52::Off),
                Key::Scrollback => SettingsEdit::Scrollback(1),
                Key::ShellIntegration => SettingsEdit::ShellIntegration(ShellIntegration::Off),
                Key::Theme => SettingsEdit::Theme(String::new()),
                Key::LightTheme => SettingsEdit::LightTheme(String::new()),
                Key::DarkTheme => SettingsEdit::DarkTheme(String::new()),
                Key::Font => SettingsEdit::FontFamily(String::new()),
                Key::Size => SettingsEdit::FontSize(13.0),
                Key::LineHeight => SettingsEdit::LineHeight(1.0),
                Key::Shape => SettingsEdit::Cursor(CaretShape::Beam),
                Key::Blink => SettingsEdit::CursorBlink(CursorBlink::On),
                Key::BlinkSpeed => SettingsEdit::BlinkInterval(0.5),
                Key::Radius => SettingsEdit::CursorRadius(0.1),
                Key::Glow => SettingsEdit::CursorGlow(0.5),
                Key::Unfocused => SettingsEdit::CursorUnfocused(UnfocusedCaret::Solid),
                Key::CursorMotion => SettingsEdit::CursorMotion(CursorMotion::Snap),
                Key::SmoothScroll => SettingsEdit::SmoothScroll(SmoothScroll::On),
                Key::ReduceMotion => SettingsEdit::ReduceMotion(ReduceMotion::On),
            };
            assert_eq!(edit.path(), key.path(), "{key:?}");
        }
    }

    #[test]
    fn file_state_decides_lock_banner_and_rows() {
        // Dosya yok: açık, şeritsiz, satırlar açıklamalarıyla.
        assert_eq!(
            status(&FileState::Missing, &[]),
            Status {
                locked: false,
                banner: Banner::default(),
                rows: Vec::new(),
            }
        );

        // Kilit: sebep alt başlıktakinin aynısı, altında ne yapılacağı.
        let reason = "settings.toml: line 1: invalid TOML".to_owned();
        let seen = status(&FileState::Locked(reason.clone()), &[]);
        assert!(seen.locked);
        assert_eq!(seen.banner.lines, [reason]);
        assert_eq!(seen.banner.hint, Some(LOCK_HINT));
        assert!(seen.rows.is_empty());

        // Kabul edilmeyen değer kendi satırında, yalnız iletisiyle; satıra
        // düşmeyen tanı (emekli anahtar) şeritte, alt başlıktaki biçimiyle.
        let rejected = Diagnostic {
            key: Some("terminal.cursor"),
            line: Some(2),
            message: "`terminal.cursor` must be one of …".to_owned(),
        };
        let retired = Diagnostic {
            key: None,
            line: Some(4),
            message: "`shell.prompt` is no longer read".to_owned(),
        };
        let seen = status(&FileState::Usable(vec![rejected, retired]), &[]);
        assert!(!seen.locked);
        assert_eq!(
            seen.rows,
            [(Key::Shape, "`terminal.cursor` must be one of …".to_owned())]
        );
        assert_eq!(
            seen.banner.lines,
            ["settings.toml: line 4: `shell.prompt` is no longer read"]
        );
        assert_eq!(seen.banner.hint, None);

        // Yazma hatası şeridin başında, kilitsiz.
        let write = ["settings.toml could not be written: denied".to_owned()];
        let seen = status(&FileState::Usable(Vec::new()), &write);
        assert!(!seen.locked);
        assert_eq!(seen.banner.lines, write);
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
