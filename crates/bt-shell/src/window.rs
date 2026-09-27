//! Terminal penceresi: bir `NSWindow`, bölmelerinin kapsayıcısı
//! (`split_view::SplitView`, `contentView`) ve onun pane'leri
//! (`pane::TerminalPane` — oturum, link, renderer, yüzey, `BateriView`,
//! arama paneli, yükleme kuyruğu), ve **sekmeye** ait olan her şey: krom,
//! başlık, sekme noktası, kapatma sorusu, sekme ve bölme eylemleri
//! (`closeTab:`, `closeWindow:`, `selectTab:`, `splitRight:`, `splitDown:`);
//! pencerenin `NSWindowDelegate`'i de burada.
//!
//! **Odaktaki pane** pencerenin first responder'ının pane'i
//! ([`TerminalWindow::focused_pane`]; 039 Karar 11): başlık, `⇄`, yükleme
//! yüzdesi, sekme noktası ve yeni sekmenin/bölmenin mirası ondan. ⌘W onu
//! kapatır, son pane'de sekmeyi (Karar 8).
//!
//! Pane'in sahibi burası (039 Karar 3): pane'in olayları [`WindowHost`]'tan
//! (`PaneHost`) gelip pencereye ya da uygulamaya varıyor, girdileri
//! `AppDelegate::open_window`'un kurduğu `PaneLaunch`'tan. Uygulama geneli
//! (ayarlar, izleme, alt başlık yuvaları, ölçüm defteri, süreli koşu tarifi,
//! pencere listesi) `app`'te; oradan gelen kayıt anı yolları **her pane'e**
//! varır (`TerminalWindow::panes`). Pencerenin geometri, örtülme ve odak
//! bildirimleri de **bütün** pane'lere dağıtılıyor. Çizim çağrısı burada da yok, bu
//! dosyanın işi bağlamak.
//!
//! Renderer pane başına (`pane`'in başlığı; 039 Karar 5).

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::time::Instant;

use block2::RcBlock;
use bt_core::{ConfirmClose, HostMark, Settings, ShutdownHandle, Teardown, Theme};
use bt_gpu::GpuError;
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAppearance, NSAppearanceCustomization,
    NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSBackingStoreType, NSBox,
    NSBoxType, NSColor, NSMenuItem, NSModalResponse, NSModalResponseCancel, NSTitlePosition,
    NSTitlebarSeparatorStyle, NSView, NSWindow, NSWindowDelegate, NSWindowOcclusionState,
    NSWindowOrderingMode, NSWindowStyleMask,
};
use objc2_foundation::{
    NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, ns_string,
};

use crate::Run;
use crate::app::{self, AppDelegate};
use crate::jobs::Foreground;
use crate::notices::Source;
use crate::pane::{PaneHost, PaneLaunch, TerminalPane};
use crate::split::{Axis, Removal};
use crate::split_view::SplitView;
use crate::upload;
use crate::uploader;

/// Temanın zemini koyu mu — pencere kromunun görünümü (Aqua / DarkAqua)
/// buradan ([`TerminalWindow::apply_chrome`]).
///
/// Soru "bu zeminde hangi metin daha okunur: beyaz mı siyah mı" ve cevabı
/// WCAG'ın kontrast oranından: zeminin bağıl parlaklığı (Rec. 709
/// katsayıları, **lineer** bileşenlerden) beyazla daha yüksek kontrast
/// veriyorsa zemin koyudur. Eşik uydurulmuyor, iki oranın eşitliğinden
/// doğuyor; sistemin koyu görünümü de tam olarak "açık metin" demek.
///
/// `bt-core`'un `Theme`'inde değil burada: açıklık bir tema rolü değil,
/// AppKit'in görünüm sözlüğüne bir çeviri.
pub(crate) fn is_dark_background(theme: &Theme) -> bool {
    let [r, g, b, _] = theme.background_linear().to_array();
    let luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    // WCAG: oran = (açık + 0.05) / (koyu + 0.05); beyazın parlaklığı 1.
    let against_white = 1.05 / (luminance + 0.05);
    let against_black = (luminance + 0.05) / 0.05;
    against_white > against_black
}

/// Pencerenin pane'e verdiği sahip tutamağı ([`PaneHost`], 039 Karar 3).
///
/// **Pencereyi kimlikle buluyor**, referansla tutmuyor: pencere pane'i
/// (`contentView` ve ivar) güçlü tutuyor, geri referans bir çember olurdu.
/// Kimlik pencere doğmadan belli (`AppDelegate::open_window` sayacı önce
/// çekiyor), yani tutamak doğum paketine girebiliyor — sonradan kurulan bir
/// yuva yok. Pencere listeden çıktıysa olay düşüyor.
pub(crate) struct WindowHost {
    window: u64,
}

impl WindowHost {
    pub(crate) fn new(window: u64) -> Self {
        Self { window }
    }

    /// Ana thread'deyiz: `PaneHost`'un bütün çağrıları pane'den, ana
    /// thread'de geliyor.
    fn mtm() -> MainThreadMarker {
        // audit: `PaneHost` yalnız ana thread'de çağrılıyor (trait'in doc'u).
        MainThreadMarker::new().expect("PaneHost ana thread'de çağrılır")
    }

    fn window(&self) -> Option<Retained<TerminalWindow>> {
        app::delegate(Self::mtm())?.window(self.window)
    }
}

impl PaneHost for WindowHost {
    fn title_changed(&self, _pane: u64) {
        // Başlık odaktaki pane'den; arka pane'in haberi aynı okumayı yapıyor
        // ve değişmemiş başlığı yeniden yazıyor — ucuz ve dallanmasız.
        if let Some(window) = self.window() {
            window.refresh_title();
        }
    }

    fn shell_exited(&self, pane: u64) {
        // Yalnız o pane (039 Karar 8); son pane sekmeyi kapatıyor.
        if let Some(window) = self.window() {
            window.close_pane(pane);
        }
    }

    fn focused(&self, pane: u64) {
        if let Some(window) = self.window() {
            window.pane_focused(pane);
        }
    }

    fn uploads_changed(&self, _pane: u64) {
        if let Some(app) = app::delegate(Self::mtm()) {
            app.refresh_dock_tile();
        }
    }

    fn notify(&self, _pane: u64, title: &str, body: &str) {
        uploader::notify(Self::mtm(), title, body);
    }

    fn post_notices(&self, _pane: u64, source: Source, messages: Vec<String>) {
        if let Some(app) = app::delegate(Self::mtm()) {
            app.post_notices(source, messages);
        }
    }
}

/// Başlamış bir pencere kapanışı ([`TerminalWindow::begin_close`]).
pub(crate) enum Closing {
    /// Bu çağrı başlattı; sonucu tutamak biliyor.
    Started(ShutdownHandle),
    /// Kapanış daha önce başlamıştı (pencere kapanırken ⌘Q gibi): beklenecek
    /// bir şey yok, gerçek sonucu ilk çağrı biliyordu.
    AlreadyDone,
}

impl Closing {
    /// En geç `deadline`'a kadar bekler ([`ShutdownHandle::wait_until`]).
    pub(crate) fn wait_until(self, deadline: Instant) -> Teardown {
        match self {
            Self::Started(handle) => handle.wait_until(deadline),
            Self::AlreadyDone => Teardown::AlreadyDone,
        }
    }
}

/// Select Tab ▸ öğesinin `tag`'i + sekme sayısı → seçilecek sekmenin sırası.
///
/// ⌘1…⌘8 n. sekme, yoksa `None` (no-op); ⌘9 **son** sekme — Safari,
/// Terminal.app ve tarayıcıların ortak kuralı: dokuzdan fazla sekmede de
/// sonuncuya tek tuşla gidiliyor. Saf, sınanıyor.
pub(crate) fn tab_index(tag: u8, count: usize) -> Option<usize> {
    match tag {
        1..=8 => Some(usize::from(tag) - 1).filter(|&index| index < count),
        9 => count.checked_sub(1),
        _ => None,
    }
}

/// Onaylanan sorunun kapatacağı şey ([`TerminalWindow::ask`]): kimlikler,
/// cevap anında yeniden aranıyor.
#[derive(Clone, Debug)]
enum CloseTarget {
    /// Sekmeler (pencere kimlikleri), bütün pane'leriyle.
    Tabs(Vec<u64>),
    /// Tek pane (pane kimliği); sekme açık kalıyor.
    Pane(u64),
}

/// Shell ▸ Close Tab öğesinin başlığı (039 Karar 8): çok pane'de ⌘W odaktaki
/// pane'i kapatıyor ve öğe "Close", tek pane'de sekmeyi ve "Close Tab".
pub(crate) fn close_title(panes: usize) -> &'static str {
    if panes > 1 { "Close" } else { "Close Tab" }
}

/// Kapatılan şey — sorunun başlığını ve onay düğmesini seçiyor
/// (`.tasks/028-kapatma-onayi/discussion.md` → Karar 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CloseScope {
    /// Sekmesinde başka pane olan bir pane (⌘W; 039 Karar 8).
    Pane,
    /// Grubunda başka sekme olan bir sekme (⌘W).
    Tab,
    /// Grubun bir kısmı, birden çok sekme ("Close Other Tabs").
    Tabs(usize),
    /// Pencerenin tamamı: tek sekmeli pencerede ⌘W, ya da ⇧⌘W.
    Window,
    /// Uygulama (⌘Q, Dock ▸ Quit, oturum kapatma).
    Quit,
}

/// Bir jestin istediği sekme sayısı ve grubun boyu → sorunun kapsamı.
///
/// Grubun tamamı pencere (kırmızı düğme, tek sekmeli pencerede ⌘W), tek sekme
/// sekme (⌘W), aradaki her şey sayılı sekmeler ("Close Other Tabs").
pub(crate) fn close_scope(requested: usize, group: usize) -> CloseScope {
    if requested >= group {
        CloseScope::Window
    } else if requested == 1 {
        CloseScope::Tab
    } else {
        CloseScope::Tabs(requested)
    }
}

/// Kapanış sorulsun mu — üç kapanış yolunun **tek** kararı (R2.1).
///
/// Süreli koşu **ilk** soru ve cevabı her ayarda hayır: süreli koşu ayar
/// okumuyor, yani `confirm` orada varsayılan `running`, ve bekçisi
/// kurulmamış başsız bir soru `make duman`'ı asardı. Süreç tablosu
/// (`running`) yalnız cevap ona bağlıysa, yani yalnız `running`'de okunuyor.
pub(crate) fn should_ask(
    timed: bool,
    confirm: ConfirmClose,
    running: impl FnOnce() -> bool,
) -> bool {
    if timed {
        return false;
    }
    match confirm {
        ConfirmClose::Never => false,
        ConfirmClose::Always => true,
        ConfirmClose::Running => running(),
    }
}

/// Sorulacaksa kapanan her pane'in ön planı, sorulmayacaksa `None` —
/// [`should_ask`]'ın pane'ler üstündeki hâli (039 Karar 11: soru koşan işi
/// sekmelerden değil **pane'lerden** topluyor).
///
/// Tablo `running`'de karar için bir kez okunuyor ve metin aynı okumayı
/// kullanıyor; `always`'de karar tabloya bakmıyor ama metin koşan işin adını
/// yine söylemek istiyor, o yüzden soru kesinleşince okunuyor.
pub(crate) fn foregrounds_to_ask(
    timed: bool,
    confirm: ConfirmClose,
    panes: &[Retained<TerminalPane>],
) -> Option<Vec<Foreground>> {
    let read = || {
        panes
            .iter()
            .map(|pane| pane.foreground())
            .collect::<Vec<_>>()
    };
    let mut seen = None;
    let ask = should_ask(timed, confirm, || {
        let foregrounds = read();
        let running = foregrounds
            .iter()
            .any(|foreground| matches!(foreground, Foreground::Running(_)));
        seen = Some(foregrounds);
        running
    });
    ask.then(|| seen.unwrap_or_else(read))
}

/// Sorunun saydığı birim: sekme mi pane mi (039 Karar 11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unit {
    Tab,
    Pane,
}

impl Unit {
    fn plural(self) -> &'static str {
        match self {
            Unit::Tab => "tabs",
            Unit::Pane => "panes",
        }
    }
}

/// Kapanan pane ve sekme sayısından birim: bir sekmede birden çok pane
/// varsa sayılan şey pane, yoksa sekme — tek pane'li sekmelerde metin
/// bölmelerden önceki metnin bayt bayt aynısı. Saf, sınanıyor.
pub(crate) fn unit_for(panes: usize, tabs: usize) -> Unit {
    if panes > tabs { Unit::Pane } else { Unit::Tab }
}

/// Sorunun metni: başlık, açıklama ve onay düğmesi.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Prompt {
    pub(crate) title: String,
    pub(crate) message: String,
    pub(crate) confirm: &'static str,
}

/// Kapanan pane'lerin ön planlarından sorunun metni (Karar 4). Saf; soruyu
/// kuran üç yolun da tek metin kaynağı ([`alert`]).
///
/// Açıklama koşan işleri **adıyla** sayıyor: tek pane'de adlar
/// ("“claude” is still running."), birden çoğunda `unit` sayısı (sekme ya da
/// pane, [`unit_for`]) ve tekrarsız adlar. Adı okunamayan iş adsız
/// söyleniyor ("A process"), koşan iş hiç yoksa (`always`) kapanacak şey.
pub(crate) fn prompt(scope: CloseScope, unit: Unit, tabs: &[Foreground]) -> Prompt {
    let (title, confirm, verb) = match scope {
        CloseScope::Pane => ("Close this pane?".to_owned(), "Close", "Closing"),
        CloseScope::Tab => ("Close this tab?".to_owned(), "Close", "Closing"),
        CloseScope::Tabs(n) => (format!("Close {n} tabs?"), "Close", "Closing"),
        CloseScope::Window => ("Close this window?".to_owned(), "Close", "Closing"),
        CloseScope::Quit => ("Quit bateri?".to_owned(), "Quit", "Quitting"),
    };
    let running: Vec<&[String]> = tabs
        .iter()
        .filter_map(|tab| match tab {
            Foreground::Running(names) => Some(names.as_slice()),
            Foreground::Idle => None,
        })
        .collect();
    let message = match running.as_slice() {
        [] => idle_message(scope, unit, tabs.len()),
        [names] => {
            let (subject, pronoun) = match names {
                [] => ("A process is".to_owned(), "it"),
                [name] => (format!("{} is", quoted(name)), "it"),
                _ => (format!("{} are", listed(names)), "them"),
            };
            format!("{subject} still running. {verb} ends {pronoun}.")
        }
        many => {
            let mut names: Vec<&String> = Vec::new();
            for name in many.iter().flat_map(|names| names.iter()) {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
            let count = many.len();
            let unit = unit.plural();
            if names.is_empty() {
                format!("Processes are running in {count} {unit}. {verb} ends them.")
            } else {
                let names: Vec<String> = names.into_iter().map(|name| quoted(name)).collect();
                format!(
                    "Processes are running in {count} {unit}: {}. {verb} ends them.",
                    names.join(", ")
                )
            }
        }
    };
    Prompt {
        title,
        message,
        confirm,
    }
}

/// `always`'in koşan işsiz metni: kapanacak şeyi söylüyor.
fn idle_message(scope: CloseScope, unit: Unit, tabs: usize) -> String {
    match (scope, tabs) {
        (CloseScope::Pane, _) => "Closing this pane ends its shell session.".to_owned(),
        (CloseScope::Tab, _) => "Closing this tab ends its shell session.".to_owned(),
        (CloseScope::Tabs(n), _) => format!("Closing these {n} tabs ends their shell sessions."),
        (CloseScope::Window, 0 | 1) => "Closing this window ends its shell session.".to_owned(),
        (CloseScope::Window, n) => {
            format!(
                "Closing this window ends the shell sessions in its {n} {}.",
                unit.plural()
            )
        }
        (CloseScope::Quit, 0 | 1) => "Quitting ends the open shell session.".to_owned(),
        (CloseScope::Quit, n) => format!("Quitting ends {n} open shell sessions."),
    }
}

/// macOS'un tipografik tırnağıyla ad.
fn quoted(name: &str) -> String {
    format!("\u{201c}{name}\u{201d}")
}

/// "“a”", "“a” and “b”", "“a”, “b” and “c”".
fn listed(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => quoted(one),
        [rest @ .., last] => {
            let rest: Vec<String> = rest.iter().map(|name| quoted(name)).collect();
            format!("{} and {}", rest.join(", "), quoted(last))
        }
    }
}

/// [`Prompt`]'tan `NSAlert`: onay ilk düğme (Return), "Cancel" ikinci (Esc).
///
/// Esc **elle** bağlanıyor: belge "Cancel" başlıklı düğmeye Esc'i kendisinin
/// bağladığını söylüyor, ama gerçek pencerede Esc sayfayı kapatmadı
/// (ölçüldü, phase-2 Uygulama Notları); Return ilk düğmede çalışıyordu.
pub(crate) fn alert(mtm: MainThreadMarker, prompt: &Prompt) -> Retained<NSAlert> {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(&prompt.title));
    alert.setInformativeText(&NSString::from_str(&prompt.message));
    alert.addButtonWithTitle(&NSString::from_str(prompt.confirm));
    let cancel = alert.addButtonWithTitle(ns_string!("Cancel"));
    cancel.setKeyEquivalent(ns_string!("\u{1b}"));
    alert
}

/// `view`'ın ya da atalarından birinin pane'i — first responder'dan odaktaki
/// pane'e (`BateriView`, arama alanının alan düzenleyicisi).
fn pane_containing(view: Retained<NSView>) -> Option<Retained<TerminalPane>> {
    let mut current = Some(view);
    while let Some(view) = current {
        match view.downcast::<TerminalPane>() {
            Ok(pane) => return Some(pane),
            // SAFETY: üst view'ı okumak; ana thread'deyiz (`MainThreadOnly`).
            Err(view) => current = unsafe { view.superview() },
        }
    }
    None
}

/// Turun sonunda `windowShouldClose:` isteklerini toplar: bayrağı dikili her
/// grup için tek karar ([`TerminalWindow::should_close_now`]).
fn close_requested_tabs(app: &AppDelegate) {
    while let Some(anchor) = app
        .windows()
        .into_iter()
        .find(|window| window.ivars().close_requested.get())
    {
        anchor.close_requested_group(app);
        // Grubun dışına düşmüş (listede olup grubu çözülemeyen) bir bayrak
        // döngüyü kilitlemesin.
        anchor.ivars().close_requested.set(false);
    }
}

/// Pencerenin durumu — **sekmeye** ait olan: krom, sekme noktası, kapatma
/// sorusu ve odak. Oturumun çekirdeği (oturum, link, renderer, yüzey, view,
/// dock payı, punto, kimlik, arama, yükleme) pane'lerde ([`TerminalPane`],
/// 039 Karar 1–3), pane'ler ve bölme ağacı kapsayıcıda ([`SplitView`]).
pub(crate) struct WindowIvars {
    /// Kendi sayacımız ([`AppDelegate`] dağıtıyor): kapatma sorusunun ve
    /// listeden çıkışın pencereyi bulduğu anahtar. Pane'in kimliği ayrı
    /// ([`TerminalPane::id`]) ve aynı sayaçtan.
    id: u64,
    /// Süreli koşunun tarifi, `AppDelegate`'inkinin kopyası (`Copy`):
    /// kapatma sorusu süreli koşuda hiç sorulmuyor ve uygulama delegate'ine
    /// uzanmadan cevaplayabilmeli ([`TerminalWindow::should_close_now`]).
    run: Option<Run>,
    window: Retained<NSWindow>,
    /// Bölmelerin kapsayıcısı ve `contentView`: `NSWindow` onu zaten güçlü
    /// tutuyor, bu kopya tipli erişim için ([`TerminalWindow::panes`]).
    container: Retained<SplitView>,
    /// Son odaklanan pane'in kimliği — first responder bir pane'in içinde
    /// değilken (pencerenin kendisi) odağın cevabı
    /// ([`TerminalWindow::focused_pane`]). `BateriView` first responder
    /// olunca pane'in `PaneHost::focused` olayı yazıyor.
    focused: Cell<u64>,
    /// Kromun son boyandığı zemin ([`TerminalWindow::apply_chrome`]'un
    /// kapısı); `None`: henüz boyanmadı.
    chrome: Cell<Option<u32>>,
    /// Sekmenin noktasının son kurulan rengi, sRGB
    /// ([`TerminalWindow::refresh_tab_mark`]'ın kapısı); `None`: nokta yok.
    tab_mark: Cell<Option<u32>>,
    /// Bu pencerede açık kapatma sorusu (028 → R2.8): `NSAlert`'i sayfa
    /// süresince yaşatıyor ve "sayfa açıkken ikinci soru yok" kapısı o
    /// ([`TerminalWindow::asking`]). Tamamlanma bloğu her yanıtta boşaltıyor.
    alert: RefCell<Option<Retained<NSAlert>>>,
    /// Bu turda `windowShouldClose:` bu sekmeyi istedi — jestin kapsamı
    /// turun sonunda bu bayraklardan toplanıyor
    /// ([`TerminalWindow::close_requested_tabs`]).
    close_requested: Cell<bool>,
}

/// Yeni kabuğun doğum bilgisi — doğum paketinin (`PaneLaunch::launch`)
/// çağırandan aldığı iki karar (`AppDelegate::open_window`).
pub(crate) struct Launch {
    /// Başlangıç dizini (026 → Karar 4: etkin sekmenin dizini, yoksa ev).
    pub(crate) working_directory: Option<PathBuf>,
    /// Kabuğun ilk girdisi (037 Karar 6); `None` → sıradan yerel kabuk.
    pub(crate) initial_input: Option<String>,
}

define_class!(
    // SAFETY: NSObject alt sınıflama şartı taşımaz; TerminalWindow Drop uygulamaz.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTerminalWindow"]
    #[ivars = WindowIvars]
    pub(crate) struct TerminalWindow;

    unsafe impl NSObjectProtocol for TerminalWindow {}

    unsafe impl NSWindowDelegate for TerminalWindow {

        // Ekranlar arası taşımada boyut (nokta) değişmez ama ölçek değişir;
        // layer-hosting view'da bunu bizden başka kimse yazmaz.
        //
        // Bölmelerin sınırları aygıt pikseline oturuyor, yani ölçek değişince
        // önce yeniden yerleşim, sonra **her** pane'in geometrisi: çerçevesi
        // değişmeyen pane'in bildirimi gelmiyor.
        #[unsafe(method(windowDidChangeBackingProperties:))]
        fn window_did_change_backing(&self, _n: &NSNotification) {
            self.ivars().container.layout_panes();
            for pane in self.panes() {
                pane.refresh_geometry();
            }
        }

        // Görünürlük yolu: compositor, örtülü ya da simge durumundaki bir
        // pencerenin layer içeriğini atabilir. Grid değişmediği için hiçbir
        // hasar bayrağı dikilmez ve link uyumaya devam eder — geri dönen
        // pencere boş kalır.
        //
        // Tek kanca yetiyor: simge durumu da örtülme de `occlusionState`'i
        // düşürür, yani `windowDidDeminiaturize:` bunun altkümesi olurdu.
        // **Seçili olmayan sekme de bu yoldan gidiyor** (026 R3.7, ölçüldü):
        // sekme arkaya düşünce `visible=false`, öne gelince `true` geliyor,
        // yani arka sekme sıfır kare çiziyor ve sekmeye özel bir kanca yok.
        // Genel sinyalin üstüne özel durum dizmek, listenin hiç kapanmaması
        // demek (tam ekran, Space, `unhide`, ekran uyanması...).
        #[unsafe(method(windowDidChangeOcclusionState:))]
        fn window_did_change_occlusion(&self, _n: &NSNotification) {
            // Bildirim iki yönde de gelir; örtülmeye GİDERKEN kare istemek
            // kimsenin görmeyeceği bir kare çizmek olurdu.
            let visible = self
                .ivars()
                .window
                .occlusionState()
                .contains(NSWindowOcclusionState::Visible);
            for pane in self.panes() {
                if let Some(link) = pane.link() {
                    link.set_visible(visible);
                }
            }
        }

        // **Odak yolu.** Odakta olmayan pencerede caret'in içi boşalıyor ve
        // blink duruyor; ikisi de `bt-gpu`'nun kararı, `bt-core` odağı hiç
        // görmüyor (015 R7).
        //
        // Yukarıdaki "tek kanca yetiyor" gerekçesi **buraya geçmiyor**: orada
        // örtülme ile simge durumu aynı genel sinyalin (`occlusionState`) iki
        // hâli, burada iki ayrı olgu var ve AppKit ikisini ayrı bildirimlerle
        // veriyor — birleştirecek genel bir sinyal yok.
        //
        // Pencerenin key biti **bütün** pane'lere (039 Karar 7); odakta
        // olmayan pane'in içi boş caret'i ikinci bitten, kendi `BateriView`'ının
        // first responder kancalarından geliyor.
        #[unsafe(method(windowDidBecomeKey:))]
        fn window_did_become_key(&self, _n: &NSNotification) {
            for pane in self.panes() {
                pane.apply_focus(true);
                pane.rehover_upload();
            }
        }

        #[unsafe(method(windowDidResignKey:))]
        fn window_did_resign_key(&self, _n: &NSNotification) {
            for pane in self.panes() {
                pane.apply_focus(false);
                pane.unhover_upload();
            }
        }

        /// Kırmızı düğme ve sekme çubuğunun menüsü (Close Tab, Close Other
        /// Tabs): kapanmadan önce sorulsun mu (028 → Karar 3). `false`
        /// kapanışı durduruyor; soru sorulduysa kapanış onun cevabında
        /// ([`TerminalWindow::ask`]). Ana menünün ⌘W'si buraya uğramıyor
        /// (`closeTab:`).
        ///
        /// Kabuğun çıkışı buraya **uğramıyor**: `close` delegate'e sormaz
        /// (R2.6).
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &NSWindow) -> bool {
            self.should_close_now()
        }

        /// Pencere (ya da sekme) kapanıyor: kırmızı düğme, ⌘W, ⇧⌘W ve
        /// kabuğun çıkışı (`ShellWake::child_exit` → `close`) buraya varır.
        ///
        /// Kapanış **beklenmiyor** (026 → Karar 5): başlatılıyor ve tutamak
        /// düşüyor, `"PTY teardown"` thread'i işini arkada bitiriyor — tek
        /// sekmeyi kapatmak ana thread'i yarım saniyeye kadar durdurmamalı.
        /// Sıra [`TerminalWindow::begin_close`]'ta.
        ///
        /// Süreli koşuda bu yol koşmuyor: kabuğun çıkışı `terminate:`'e gidiyor
        /// ve rapor pencereyi listede bulmak zorunda.
        ///
        /// **Listeden çıkış bir tur ertelenir.** Listenin tuttuğu `Retained`
        /// bu nesnenin tek güçlü referansı (pencerenin delegate özelliği zayıf)
        /// ve burada düşseydi nesne kendi metodunun içinde, AppKit `-close`'un
        /// ortasındayken serbest kalırdı — `NSWindow`'un `Retained`'ı da
        /// onunla. Ertelenen iş kimliği taşıyor (alternatif ekran habercisinin
        /// örüntüsü); nesne yine ana thread'de düşüyor.
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _n: &NSNotification) {
            // Pane'lerin kapanışı çerçeve gözlemcilerini de söküyor ve
            // pane'leri `AppDelegate::pane`'in aramasından düşürüyor
            // (`TerminalPane::begin_close`).
            drop(self.begin_close());
            // Delegate'i şimdi bırak: AppKit kapanmakta olan pencereye bundan
            // sonra bildirim göndermesin (odak, örtülme), nesne düşene kadar
            // bile.
            self.ivars().window.setDelegate(None);
            let id = self.ivars().id;
            DispatchQueue::main().exec_async(move || {
                // audit: ana kuyrukta koşan blok tanımı gereği ana thread'dedir.
                let mtm = MainThreadMarker::new().expect("ana kuyruk ana thread'dir");
                if let Some(app) = app::delegate(mtm) {
                    app.forget_window(id);
                }
            });
        }
    }

    // **Sekmeye ait eylemler** burada (kapatma, sekme seçimi, bölme);
    // pane düzeyindekiler (punto, bul, temizle, kaydır, yükleme iptali)
    // pane'de (039 Karar 2), uygulama geneline yayılanlar
    // (`settingsDidChange:`, tema, `openSettings:`) `AppDelegate`'te.
    // Hedefsiz eylemin responder zinciri view → pane → kapsayıcı → pencere →
    // **pencere delegate'i** → `NSApp` → app delegate; yani bu nesne yayılan
    // bir seçiciyi uygulasaydı key pencere onu yutar ve öteki pencereler hiç
    // duymazdı.
    impl TerminalWindow {

        /// ⌘W'nin başlığı ve bölmenin etkinliği; **bilinmeyen öğe `true`**.
        /// Çok pane'de ⌘W "Close" (odaktaki pane), tek pane'de "Close Tab"
        /// (039 Karar 8). Bölme, yarılardan biri en küçük pane sınırının
        /// altına düşecekse gri (Karar 14).
        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: &NSMenuItem) -> bool {
            let action = item.action();
            // `return` yok: `define_class!` `bool`'u gövdenin sonunda çeviriyor.
            if action == Some(sel!(closeTab:)) {
                item.setTitle(&NSString::from_str(close_title(self.panes().len())));
                true
            } else if action == Some(sel!(splitRight:)) {
                self.can_split(Axis::Horizontal)
            } else if action == Some(sel!(splitDown:)) {
                self.can_split(Axis::Vertical)
            } else {
                true
            }
        }

        /// Shell ▸ Split Right (⌘D): odaktaki pane'i ikiye böler, yenisi
        /// sağda (039 Karar 8, 9).
        #[unsafe(method(splitRight:))]
        fn split_right(&self, _sender: Option<&AnyObject>) {
            self.split(Axis::Horizontal);
        }

        /// Shell ▸ Split Down (⇧⌘D): odaktaki pane'i ikiye böler, yenisi
        /// altta.
        #[unsafe(method(splitDown:))]
        fn split_down(&self, _sender: Option<&AnyObject>) {
            self.split(Axis::Vertical);
        }

        /// Shell ▸ Close Tab (⌘W): çok pane'de **odaktaki pane**, tek pane'de
        /// **yalnız bu sekme** (039 Karar 8); gerekirse sorarak.
        ///
        /// `performClose:` değil, çünkü AppKit'in onu yorumlaması durumlu
        /// (ölçüldü, phase-2 Uygulama Notları): kırmızı düğmenin grup kapanışı
        /// bir `windowShouldClose:` `false`'uyla durdurulunca sonraki
        /// `performClose:` da grubun her sekmesine `windowShouldClose:`
        /// gönderiyor ve ⌘W pencereyi sorar oluyordu. Kendi eylemimiz
        /// kapsamı kendisi biliyor; soru ve kapanış ⇧⌘W'ninkiyle aynı yol.
        #[unsafe(method(closeTab:))]
        fn close_tab(&self, _sender: Option<&AnyObject>) {
            self.close_tab_asking();
        }

        /// Shell ▸ Close Window (⇧⌘W): pencereyi **bütün sekmeleri ve
        /// pane'leriyle**.
        ///
        /// Grubun tamamı için **tek** soru (R2.3) ve onayda her sekme
        /// `close` ile kapanıyor — `performClose:` değil, çünkü o her sekmenin
        /// `windowShouldClose:`'undan geçer ve sekme başına ikinci bir soru
        /// doğururdu. Kapanışın kendisi yine her sekmenin `windowWillClose:`'u.
        #[unsafe(method(closeWindow:))]
        fn close_window(&self, _sender: Option<&AnyObject>) {
            self.close_group_asking();
        }

        /// Window ▸ Select Tab ▸ Tab n (⌘1…⌘8) ve Last Tab (⌘9): öğenin `tag`'i
        /// ([`crate::menu`]) sekme grubunda bir sıraya iner ([`tab_index`]).
        /// Olmayan sekme no-op.
        #[unsafe(method(selectTab:))]
        fn select_tab(&self, sender: Option<&AnyObject>) {
            let Some(item) = sender.and_then(|sender| sender.downcast_ref::<NSMenuItem>()) else {
                return;
            };
            let Ok(tag) = u8::try_from(item.tag()) else {
                return;
            };
            let windows = self.tab_windows();
            if let Some(window) = tab_index(tag, windows.len()).map(|index| &windows[index]) {
                window.makeKeyAndOrderFront(None);
            }
        }
    }
);

impl TerminalWindow {
    /// Pencereyi ve tek pane'ini (view, yüzey, renderer) kurar; oturum ve
    /// link **henüz yok** ([`TerminalWindow::start`]).
    ///
    /// İki adım olmasının sebebi aradaki iş: ayarlar pencere doğduktan
    /// **sonra** (tanı alt başlığa yazılabilsin) ve geometriden **önce**
    /// okunmak zorunda — font ayarı hücre ölçüsünü, yani ilk grid'i ve kabuğun
    /// gördüğü ilk `TIOCSWINSZ`'yi belirliyor. Tek kurucu o sırayı ya bozar ya
    /// da ayar okumayı pencerenin içine taşırdı.
    ///
    /// Renderer pane'le doğuyor ve hatası çağırana dönüyor: Metal device ya
    /// da metallib yoksa pencerenin çizebileceği bir şey de yok. `launch`
    /// ilk pane'in doğum paketi (kimliği pencerenin kimliğiyle aynı sayaçtan,
    /// sahibi bu pencerenin [`WindowHost`]'u); pencere onu kapsayıcının tek
    /// pane'i olarak doğuruyor, bölmeler sonradan ([`TerminalWindow::add_pane`]).
    pub(crate) fn new(
        mtm: MainThreadMarker,
        id: u64,
        launch: PaneLaunch,
    ) -> Result<Retained<Self>, GpuError> {
        let run = launch.run;
        let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(900.0, 600.0));
        let pane = TerminalPane::new(mtm, rect, launch)?;
        let container = SplitView::new(mtm, rect, &pane);
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable;
        // SAFETY: defer=false ile pencere hemen yaratılır. Kurucunun unsafe
        // olma sebebi `releasedWhenClosed`: pencere kontrolcüsü olmadan
        // AppKit kapanışta pencereyi serbest bırakır ve `WindowIvars.window`'daki
        // Retained sarkar; hemen altında kapatıyoruz.
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
        unsafe { window.setReleasedWhenClosed(false) };
        // İçerik view'ı bölmelerin kapsayıcısı; çerçevesini pencere kuruyor,
        // pane'leri kapsayıcı oturtuyor (`SplitView::layout_panes`; tek
        // pane'de sınırın tamamı).
        window.setContentView(Some(&container));
        window.setTitle(ns_string!("bateri"));
        // **Native sekmeler** (026 → Karar 1): aynı kimliği taşıyan pencereleri
        // AppKit tek pencerede sekme olarak topluyor. `tabbingMode` bilerek
        // varsayılanda — sistemin "Prefer tabs" ayarına saygı. Kimliğin tek
        // yazıldığı yer burası, yani bütün pencereler ortak kimlikte.
        window.setTabbingIdentifier(ns_string!("bateri.terminal"));
        // Düğmesiz hareket olayları varsayılan **kapalı**; fare raporu
        // isteyen uygulama (1003) onlarsız işaretçiyi hiç göremez.
        // `NSTrackingArea` gerekmiyor: `mouseEntered:`/`mouseExited:`
        // istenmiyor, yükleme düğmelerinin el imleci `NSView`'ın kendi
        // cursor rect'inden (`BateriView::upload_cursor_rects`), ve view
        // zaten first responder — pencere seviyesindeki `mouseMoved:` ona
        // geliyor. Kipe göre açıp kapamak kipi
        // `bt-shell`'e yayınlamayı isterdi
        // (`.tasks/020-fare-raporlama/discussion.md` → Karar 4).
        window.setAcceptsMouseMovedEvents(true);
        // Klavyenin PTY'ye varan yolu buradan başlıyor. View (pane'in
        // çocuğu da olsa) otomatik first responder DEĞİLDİR; bu satır olmadan pencere
        // key olur, tuşlar view'a hiç uğramaz ve terminal sessizce
        // yazmaz. `acceptsFirstResponder` da şart, ikisi bir arada.
        let accepted = window.makeFirstResponder(Some(pane.view()));
        debug_assert!(accepted, "BateriView first responder olmalı");
        let this = Self::alloc(mtm).set_ivars(WindowIvars {
            id,
            run,
            window: window.clone(),
            container: container.clone(),
            focused: Cell::new(pane.id()),
            chrome: Cell::new(None),
            tab_mark: Cell::new(None),
            alert: RefCell::new(None),
            close_requested: Cell::new(false),
        });
        // SAFETY: NSObject'in init'i argümansızdır ve ivar'lar set edildi.
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        // Delegate bağlanmadan önce ivar'lar dolu: arada düşen bir pencere
        // bildirimi geometriyi boş bulup bayat boyutla çizmesin. Delegate
        // özelliği zayıf; sahibi `AppDelegate`'in pencere listesi.
        window.setDelegate(Some(ProtocolObject::from_ref(&*this)));
        // İçeriğin boyu pencereden bağımsız da değişiyor (sekme çubuğu);
        // geometri bu yüzden view'ın kendi bildiriminden, gözlemcisi pane
        // (`TerminalPane::observe_frame`). Kurucunun son adımı: önceki
        // adımların yerleşimi geometriyi pencere hazır olmadan kurdurmasın.
        pane.observe_frame();
        Ok(this)
    }

    pub(crate) fn id(&self) -> u64 {
        self.ivars().id
    }

    /// Sekmenin pane'leri, ağaç sırasıyla (soldan sağa, yukarıdan aşağı).
    /// Hiç boş değil: son pane kapanırken sekme kapanıyor. Süreli koşuda tek
    /// pane (039 Karar 12).
    pub(crate) fn panes(&self) -> Vec<Retained<TerminalPane>> {
        self.ivars().container.panes()
    }

    /// Odaktaki pane (039 Karar 11): pencerenin first responder'ının pane'i
    /// — `BateriView` ya da arama alanının alan düzenleyicisi, ikisi de
    /// pane'in torunu. First responder bir pane'in içinde değilse (pencere
    /// kendisi) son odaklanan pane ([`WindowIvars::focused`]), o da yoksa ilk
    /// pane.
    pub(crate) fn focused_pane(&self) -> Retained<TerminalPane> {
        let panes = self.panes();
        let responder = self
            .ivars()
            .window
            .firstResponder()
            .and_then(|responder| responder.downcast::<NSView>().ok())
            .and_then(pane_containing);
        let focused = self.ivars().focused.get();
        responder
            .filter(|pane| panes.iter().any(|candidate| candidate.id() == pane.id()))
            .or_else(|| panes.iter().find(|pane| pane.id() == focused).cloned())
            .or_else(|| panes.first().cloned())
            // audit: kapsayıcı hiç boşalmıyor (`SplitIvars::panes`): son
            // pane'i kapatmak sekmeyi kapatıyor ve pencere kurucusu bir
            // pane'le doğuyor.
            .expect("sekmenin en az bir pane'i var")
    }

    /// Pane'in `BateriView`'ı first responder oldu (`PaneHost::focused`):
    /// odak ona geçti, başlık ve sekme noktası onun.
    ///
    /// Başlık **bir ana kuyruk turu sonra** okunuyor: olay
    /// `becomeFirstResponder`'ın içinden geliyor, pencerenin
    /// `firstResponder`'ı o an henüz eski view olabilir ve
    /// [`Self::focused_pane`] onu ivar'dan önce soruyor — başlık eski pane'den
    /// yazılırdı. İş kimlik yakalıyor (`windowWillClose:`'un örüntüsü).
    pub(crate) fn pane_focused(&self, id: u64) {
        if self.ivars().focused.replace(id) == id {
            return;
        }
        let window = self.id();
        DispatchQueue::main().exec_async(move || {
            // audit: ana kuyrukta koşan blok tanımı gereği ana thread'dedir.
            let mtm = MainThreadMarker::new().expect("ana kuyruk ana thread'dir");
            if let Some(window) = app::delegate(mtm).and_then(|app| app.window(window)) {
                window.refresh_title();
            }
        });
    }

    /// Klavyeyi `pane`'e verir (first responder) ve odağı ona taşır.
    fn focus_pane(&self, pane: &TerminalPane) {
        let _ = self.ivars().window.makeFirstResponder(Some(pane.view()));
        self.pane_focused(pane.id());
    }

    /// Odaktaki pane `axis`'te bölünebilir mi (039 Karar 14): iki yarının
    /// ızgarası da en küçük pane sınırını geçmeli
    /// ([`TerminalPane::grid_fits`]). Yeni pane odaktakinin punto farkını
    /// devraldığı için ölçü odaktakinin hücresinden.
    fn can_split(&self, axis: Axis) -> bool {
        let pane = self.focused_pane();
        self.ivars()
            .container
            .halves(pane.id(), axis)
            .is_some_and(|(first, second)| pane.grid_fits(first) && pane.grid_fits(second))
    }

    /// ⌘D / ⇧⌘D: odaktaki pane'den yeni bir bölme — doğum paketini uygulama
    /// kuruyor (`AppDelegate::open_split`: dizin, punto farkı, tema ve uzak
    /// satır odaktakinden, 039 Karar 9). Sınırın altına düşecekse no-op
    /// (Karar 14).
    fn split(&self, axis: Axis) {
        if !self.can_split(axis) {
            return;
        }
        let Some(app) = app::delegate(self.mtm()) else {
            return;
        };
        let from = self.focused_pane();
        if let Some(this) = app.window(self.id()) {
            app.open_split(&this, &from, axis);
        }
    }

    /// Yeni pane'i `target`'ın `axis`'teki ikinci yarısına koyar, oturumunu
    /// açar ve klavyeyi ona verir. Sıra zorunlu: pane'in `start`'ı ölçeği
    /// pencereden okuyor, yani önce kapsayıcıya takılıyor; çerçeve
    /// gözlemcisi yerleşimden sonra. Oturum doğamazsa pane geri sökülüyor —
    /// oturumsuz bir yaprak kalmıyor — ve hata çağırana.
    pub(crate) fn add_pane(
        &self,
        mtm: MainThreadMarker,
        launch: PaneLaunch,
        target: u64,
        axis: Axis,
    ) -> Result<(), String> {
        let container = &self.ivars().container;
        let (_, half) = container
            .halves(target, axis)
            .ok_or_else(|| "bölünecek pane yok".to_owned())?;
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), half);
        let pane = TerminalPane::new(mtm, frame, launch).map_err(|e| e.to_string())?;
        if !container.insert(target, axis, &pane) {
            return Err("bölünecek pane yok".to_owned());
        }
        pane.observe_frame();
        if let Err(e) = pane.start(mtm) {
            drop(pane.begin_close());
            let _ = container.remove_leaf(pane.id());
            drop(container.detach(pane.id()));
            return Err(format!("shell başlatılamadı: {e}"));
        }
        self.focus_pane(&pane);
        Ok(())
    }

    /// Yalnız `id` pane'ini kapatır, **sormadan** (kabuğun çıkışı, onaylanmış
    /// soru). Son pane'se sekmenin kapanışı ([`TerminalWindow::close`]).
    ///
    /// Odaktaki pane kapanıyorsa odak ağaçtaki komşuya ([`Removal::Removed`])
    /// ve **sökümden önce**: first responder'ı taşıyan view'ı sökmek pencereyi
    /// responder'sız bırakırdı. Kapanış pane'in kendi sırası
    /// ([`TerminalPane::begin_close`], beklenmiyor); Dock simgesinin toplamı
    /// yeniden, çünkü kapanan pane'in kuyruğu gitti.
    pub(crate) fn close_pane(&self, id: u64) {
        let container = &self.ivars().container;
        let Some(pane) = container.pane(id) else {
            return;
        };
        let was_focused = self.focused_pane().id() == id;
        match container.remove_leaf(id) {
            Removal::Missing => {}
            Removal::Last => self.close(),
            Removal::Removed { focus } => {
                if was_focused && let Some(next) = container.pane(focus) {
                    self.focus_pane(&next);
                }
                drop(pane.begin_close());
                drop(container.detach(id));
                self.refresh_title();
                if let Some(app) = app::delegate(self.mtm()) {
                    app.refresh_dock_tile();
                }
            }
        }
    }

    /// `bateri://tab/<id>`'nin tek etkisi (038 Karar 4, 6; 039 Karar 10):
    /// küçültülmüşse geri açar, seçili sekme ve key yapar, uygulamayı öne
    /// alır ve klavyeyi kimliğin pane'ine verir. Kabuğa bayt göndermez.
    ///
    /// `makeKeyAndOrderFront` sekme grubundaki pencereyi seçili sekme
    /// yapıyor (`selectTab:`'ın emsali); küçültülmüş pencerede ise yalnız
    /// sırayı değiştirip Dock'ta bırakırdı, `deminiaturize` o yüzden önce.
    pub(crate) fn bring_to_front(&self, pane: &TerminalPane) {
        let window = &self.ivars().window;
        if window.isMiniaturized() {
            window.deminiaturize(None);
        }
        window.makeKeyAndOrderFront(None);
        self.focus_pane(pane);
        NSApplication::sharedApplication(self.mtm()).activate();
    }

    /// Bu nesnenin `NSWindow`'u mu — etkin pencere `NSApp.keyWindow`'dan
    /// listede böyle aranıyor (`AppDelegate::key_window`).
    pub(crate) fn owns(&self, window: &NSWindow) -> bool {
        std::ptr::eq(&*self.ivars().window, window)
    }

    /// Pencereyi kapatır (`windowWillClose:` yolundan), **sormadan**: kabuğun
    /// çıkışı ve onaylanmış bir kapatma sorusu.
    ///
    /// Bu pencerede açık bir soru varsa önce o düşüyor, `Cancel` cevabıyla:
    /// cevabı bekleyen blok "kapat" dışındaki her cevabı iptal sayıyor. Soru
    /// başka sekmeler içinse ("Close Other Tabs" ve sayfayı taşıyan seçili
    /// sekmenin kabuğu çıktı) jest düşüyor ve o sekmeler açık kalıyor —
    /// **bilinen sınır**, yanlışın yönü güvenli: hiçbir şey sorulmadan
    /// kapanmıyor, jest yinelenebilir.
    pub(crate) fn close(&self) {
        let alert = self.ivars().alert.take();
        if let Some(alert) = alert {
            self.ivars()
                .window
                .endSheet_returnCode(&alert.window(), NSModalResponseCancel);
        }
        self.ivars().window.close();
    }

    /// Bu pencerede kapatma sorusu açık mı.
    pub(crate) fn asking(&self) -> bool {
        self.ivars().alert.borrow().is_some()
    }

    /// Sekme grubunun terminal pencereleri, sırasıyla; grup yoksa yalnız bu.
    fn tab_group(&self, app: &AppDelegate) -> Vec<Retained<TerminalWindow>> {
        self.tab_windows()
            .iter()
            .filter_map(|window| app.window_owning(window))
            .collect()
    }

    /// `windowShouldClose:`'un gövdesi (⌘W, Close Tab, kırmızı düğme, "Close
    /// Other Tabs"): şimdi kapansın mı.
    ///
    /// **Karar bu çağrıda verilmiyor, bir tur sonra ve jestin tamamı için**
    /// ([`close_requested`]). Ölçüldü (phase-2 Uygulama Notları): kırmızı düğme
    /// çok sekmeli pencerede grubun **her** sekmesine, "Close Other Tabs" öteki
    /// her sekmeye birer `windowShouldClose:` gönderiyor, ikisi de aynı olay
    /// turunda. Tek sekmeye bakan bir karar kırmızı düğmede sekme sekme soru
    /// açar ya da ilk sekmeyi "Close this tab?" diye sorup geri kalanını
    /// bırakırdı; "bir jest, en çok bir soru" ancak jestin kapsamını görerek
    /// tutuyor.
    ///
    /// Süreli koşu ve `never` hiç sormuyor: cevap şimdi belli, yani AppKit'in
    /// kendi kapanışı (`true`) — ertelemenin tek sebebi soru. Grupta soru
    /// zaten açıksa ikinci bir istek doğmuyor.
    fn should_close_now(&self) -> bool {
        if self.ivars().run.is_some() {
            return true;
        }
        let Some(app) = app::delegate(self.mtm()) else {
            return true;
        };
        if app.settings().confirm_close == ConfirmClose::Never {
            return true;
        }
        let Some(group) = self.group_unless_asking(&app) else {
            return false;
        };
        // Jestin ilk isteği turun sonuna tek iş kuruyor; sonrakiler yalnız
        // bayrağını dikiyor ve aynı işin kapsamına giriyor. İş isteyen
        // pencereyi değil **bayrakları** arıyor: ilk isteyen o arada
        // kapanmışsa (kabuğu aynı turda çıktı) öteki sekmelerin bayrağı
        // kalıcı olarak dikili kalır ve kırmızı düğme bir daha iş kurmazdı
        // (`/code-review`).
        let first = !group.iter().any(|tab| tab.ivars().close_requested.get());
        self.ivars().close_requested.set(true);
        if first {
            DispatchQueue::main().exec_async(|| {
                // audit: ana kuyrukta koşan blok tanımı gereği ana thread'dedir.
                let mtm = MainThreadMarker::new().expect("ana kuyruk ana thread'dir");
                if let Some(app) = app::delegate(mtm) {
                    close_requested_tabs(&app);
                }
            });
        }
        false
    }

    /// Grubun sekmeleri — grupta açık bir soru yoksa. "Bir jest, en çok bir
    /// soru" kapısının tek kopyası.
    fn group_unless_asking(&self, app: &AppDelegate) -> Option<Vec<Retained<TerminalWindow>>> {
        let group = self.tab_group(app);
        (!group.iter().any(|tab| tab.asking())).then_some(group)
    }

    /// Bu turdaki isteklerin grubu için tek karar; bayraklar sıfırlanıyor.
    fn close_requested_group(&self, app: &AppDelegate) {
        let group = self.tab_group(app);
        let mut targets = Vec::new();
        for tab in &group {
            if tab.ivars().close_requested.replace(false) {
                targets.push(tab.clone());
            }
        }
        if targets.is_empty() || group.iter().any(|tab| tab.asking()) {
            return;
        }
        let scope = close_scope(targets.len(), group.len());
        self.confirm_close(app, &group, &targets, scope);
    }

    /// ⌘W: çok pane'de odaktaki pane için soru; tek pane'de bu sekme için.
    /// Sorulmayacaksa hemen kapanış.
    fn close_tab_asking(&self) {
        let Some(app) = app::delegate(self.mtm()) else {
            return;
        };
        let Some(group) = self.group_unless_asking(&app) else {
            return;
        };
        if self.panes().len() > 1 {
            self.close_pane_asking(&app);
            return;
        }
        let Some(this) = app.window(self.id()) else {
            return;
        };
        let scope = close_scope(1, group.len());
        self.confirm_close(&app, &group, &[this], scope);
    }

    /// ⌘W çok pane'li sekmede: yalnız odaktaki pane, koşan iş varsa yalnız
    /// onu sorarak (039 Karar 8).
    fn close_pane_asking(&self, app: &AppDelegate) {
        let pane = self.focused_pane();
        let confirm = app.settings().confirm_close;
        let Some(foregrounds) = foregrounds_to_ask(
            self.ivars().run.is_some(),
            confirm,
            std::slice::from_ref(&pane),
        ) else {
            self.close_pane(pane.id());
            return;
        };
        self.ask(
            &prompt(CloseScope::Pane, Unit::Pane, &foregrounds),
            CloseTarget::Pane(pane.id()),
        );
    }

    /// ⇧⌘W: grubun tamamı için tek soru, ya da sorulmayacaksa hemen kapanış.
    fn close_group_asking(&self) {
        let Some(app) = app::delegate(self.mtm()) else {
            return;
        };
        if let Some(group) = self.group_unless_asking(&app) {
            self.confirm_close(&app, &group, &group, CloseScope::Window);
        }
    }

    /// `targets` kapanacak; sorulacaksa soru grubun **seçili** sekmesine sayfa
    /// olarak açılıyor, değilse hepsi hemen kapanıyor.
    ///
    /// Sayfa seçili sekmede, çünkü arka sekmeye takılan sayfa görünmez — ve
    /// "Close Other Tabs"ta kapanacak sekmelerin hiçbiri seçili değil. **Tek
    /// hedef arka sekmeyse** (sekme çubuğunda arka sekmenin ×'i) o sekme önce
    /// seçiliyor ve soru onda: "Close this tab?" gözün baktığı sekmeyi
    /// sormalı, başka birini değil (`/code-review`).
    fn confirm_close(
        &self,
        app: &AppDelegate,
        group: &[Retained<TerminalWindow>],
        targets: &[Retained<TerminalWindow>],
        scope: CloseScope,
    ) {
        let Some(first) = targets.first() else {
            return;
        };
        let panes: Vec<Retained<TerminalPane>> =
            targets.iter().flat_map(|tab| tab.panes()).collect();
        let unit = unit_for(panes.len(), targets.len());
        let confirm = app.settings().confirm_close;
        let Some(foregrounds) = foregrounds_to_ask(self.ivars().run.is_some(), confirm, &panes)
        else {
            targets.iter().for_each(|tab| tab.close());
            return;
        };
        let selected = self
            .ivars()
            .window
            .tabGroup()
            .and_then(|tab_group| tab_group.selectedWindow())
            .and_then(|window| app.window_owning(&window));
        let host = match (targets, selected.as_ref()) {
            ([only], Some(selected)) if only.id() != selected.id() => {
                only.ivars().window.makeKeyAndOrderFront(None);
                only
            }
            _ => selected
                .as_ref()
                .or_else(|| group.iter().find(|tab| tab.id() == self.id()))
                .unwrap_or(first),
        };
        let ids = targets.iter().map(|tab| tab.id()).collect();
        host.ask(&prompt(scope, unit, &foregrounds), CloseTarget::Tabs(ids));
    }

    /// Soruyu bu pencereye sayfa olarak açar; onayda `targets`'taki sekmeleri
    /// ya da pane'i kapatır.
    ///
    /// **Blok yalnız kimlik yakalıyor** (R2.8, alternatif ekran habercisinin
    /// örüntüsü): pencereleri cevap anında listeden buluyor, bulamadığını
    /// atlıyor. Yalnız `NSAlertFirstButtonReturn` kapatıyor — kabuk sayfa
    /// açıkken çıkarsa [`TerminalWindow::close`] sayfayı `Cancel`'la düşürüyor
    /// ve `forget_window` bir tur ertelendiği için pencere o arada listede hâlâ
    /// bulunabiliyor.
    ///
    /// Kapanış **bir ana kuyruk turu ertelenir** (`windowWillClose:`'un
    /// örüntüsü): cevap AppKit'in sayfa sökümünün içinde geliyor ve pencereyi
    /// orada kapatmak sökümün altını oyardı.
    fn ask(&self, prompt: &Prompt, targets: CloseTarget) {
        let alert = alert(self.mtm(), prompt);
        let host = self.id();
        let answered = RcBlock::new(move |response: NSModalResponse| {
            // audit: sayfanın tamamlanma bloğu AppKit'in ana thread'inde koşar.
            let mtm = MainThreadMarker::new().expect("sayfa bloğu ana thread'dedir");
            if let Some(window) = app::delegate(mtm).and_then(|app| app.window(host)) {
                drop(window.ivars().alert.take());
            }
            if response != NSAlertFirstButtonReturn {
                return;
            }
            let targets = targets.clone();
            DispatchQueue::main().exec_async(move || {
                // audit: ana kuyrukta koşan blok tanımı gereği ana thread'dedir.
                let mtm = MainThreadMarker::new().expect("ana kuyruk ana thread'dir");
                let Some(app) = app::delegate(mtm) else {
                    return;
                };
                match &targets {
                    CloseTarget::Tabs(ids) => {
                        for window in ids.iter().filter_map(|&id| app.window(id)) {
                            window.close();
                        }
                    }
                    // Pane'in sekmesi soruyu taşıyan pencere; pane o arada
                    // kapandıysa (kabuğu çıktı) no-op.
                    CloseTarget::Pane(pane) => {
                        if let Some(window) = app.window(host) {
                            window.close_pane(*pane);
                        }
                    }
                }
            });
        });
        self.ivars().alert.replace(Some(alert.clone()));
        alert.beginSheetModalForWindow_completionHandler(&self.ivars().window, Some(&answered));
    }

    /// Pencereyi `from`'un sekme grubuna, seçili sekmenin **sağına** ekler ve
    /// öne alır.
    pub(crate) fn show_as_tab_of(&self, from: &TerminalWindow) {
        from.ivars()
            .window
            .addTabbedWindow_ordered(&self.ivars().window, NSWindowOrderingMode::Above);
        self.ivars().window.makeKeyAndOrderFront(None);
    }

    /// Ayrı pencere olarak öne alır; `from` varsa ondan kademeli (üst sol köşe
    /// bir adım sağ-aşağı), yoksa ekranın ortasında.
    pub(crate) fn show_after(&self, from: Option<&TerminalWindow>) {
        let window = &self.ivars().window;
        match from {
            // `NSZeroPoint`'le çağrı pencereyi oynatmıyor, bir sonraki
            // pencerenin köşesini veriyor — AppKit'in kademeleme deyimi.
            Some(from) => {
                let next = from.ivars().window.cascadeTopLeftFromPoint(NSPoint::ZERO);
                window.cascadeTopLeftFromPoint(next);
            }
            None => window.center(),
        }
        window.makeKeyAndOrderFront(None);
    }

    /// Pencerenin sekme grubundaki pencereler, sırasıyla; grup yoksa yalnız
    /// kendisi.
    ///
    /// `tabbedWindows` değil `tabGroup`: ilki çubuk görünmüyorken `nil`
    /// veriyor, yani tek sekmede ⇧⌘W hiçbir şey kapatmazdı.
    fn tab_windows(&self) -> Vec<Retained<NSWindow>> {
        let window = &self.ivars().window;
        match window.tabGroup() {
            Some(group) => group.windows().to_vec(),
            None => vec![window.clone()],
        }
    }

    /// Başlığı **odaktaki** pane'in oturumundan okuyup pencereye yazar —
    /// pane'in `PaneHost::title_changed` olayı ([`WindowHost`]) ve odak
    /// değişimi ([`TerminalWindow::pane_focused`]; 039 Karar 11). Kare yolu başlık
    /// hesaplamıyor; yazım yalnız **değişimde** (026 R2.4). Oturum henüz
    /// yoksa başlık kurucunun `bateri`'si kalıyor.
    ///
    /// Sekmenin noktası da buradan tazeleniyor ([`Self::refresh_tab_mark`]):
    /// uzak durumun iki kenarı (`set_remote`'un dönüşü, `D`/`A`'nın silmesini
    /// getiren `title_changed`) başlığınkilerle aynı (037 Karar 4). Yükleme
    /// kuyruğunun bağlantı kenarı pane'in, olaydan **önce**
    /// (`TerminalPane::remote_or_title_changed`).
    pub(crate) fn refresh_title(&self) {
        self.apply_title();
        self.refresh_tab_mark();
    }

    /// Pencerenin (ve sekmenin) başlığını oturumdan yazar; yükleme akarken
    /// önünde `↑ N% · ` (037 phase-7, `upload::titled`; yüzde pane'in
    /// kuyruğundan).
    fn apply_title(&self) {
        let pane = self.focused_pane();
        if let Some(session) = pane.session() {
            let percent = pane.upload_title_percent();
            self.ivars()
                .window
                .setTitle(&NSString::from_str(&upload::titled(
                    percent,
                    &session.title(),
                )));
        }
    }

    /// Odaktaki pane'in uzak host'u ve çözülmüş işareti; yerelde `None`
    /// (`Session::remote_mark`).
    pub(crate) fn remote_mark(&self) -> Option<(String, HostMark)> {
        self.focused_pane().session()?.remote_mark()
    }

    /// Sekmenin noktası (037 Karar 4): işaretli uzak host'ta sekme
    /// başlığının yanında işaretin renginde küçük, dolu bir daire
    /// (`NSWindowTab.accessoryView`); işaretsiz uzakta ve yerelde yok —
    /// işaretsiz uzak sekme başlığında zaten `⇄` taşıyor ve her ssh
    /// sekmesine bir nokta prod'un kırmızısını sulandırırdı.
    ///
    /// Renk oturumun temasından, dock'unkiyle aynı eşlemeden
    /// (`Theme::mark_rgb`), sRGB — `NSColor` onu kendisi kodluyor. Tetikleri
    /// uzak durumun kenarları ([`Self::refresh_title`]), ayar
    /// ([`Self::set_host_marks`]) ve tema ([`Self::set_theme`]); aynı renkte
    /// no-op, yani AppKit'e her başlık haberinde yeni bir view gitmiyor.
    ///
    /// Çizim `NSBox` (033 panelinin emsali): katman yoluyla renk istemek
    /// `CGColor`'u, yani `objc2-core-graphics` kenarını isterdi. Nokta yalnız
    /// sekme çubuğu görünürken var; tek sekmeli pencerede gösterge dock'un
    /// üst çizgisi.
    fn refresh_tab_mark(&self) {
        let pane = self.focused_pane();
        let color = pane.session().and_then(|session| {
            let (_, mark) = session.remote_mark()?;
            (mark != HostMark::None).then(|| session.theme().mark_rgb(mark))
        });
        if self.ivars().tab_mark.replace(color) == color {
            return;
        }
        let tab = self.ivars().window.tab();
        let Some(color) = color else {
            tab.setAccessoryView(None);
            return;
        };
        const DIAMETER: f64 = 8.0;
        let mtm = self.mtm();
        let dot = NSBox::new(mtm);
        dot.setBoxType(NSBoxType::Custom);
        dot.setTitlePosition(NSTitlePosition::NoTitle);
        dot.setBorderWidth(0.0);
        dot.setCornerRadius(DIAMETER / 2.0);
        let byte = |shift: u32| f64::from((color >> shift) & 0xff) / 255.0;
        dot.setFillColor(&NSColor::colorWithSRGBRed_green_blue_alpha(
            byte(16),
            byte(8),
            byte(0),
            1.0,
        ));
        // Sekme aksesuarını Auto Layout boyutlandırıyor: ölçü kısıtla.
        dot.setTranslatesAutoresizingMaskIntoConstraints(false);
        dot.widthAnchor()
            .constraintEqualToConstant(DIAMETER)
            .setActive(true);
        dot.heightAnchor()
            .constraintEqualToConstant(DIAMETER)
            .setActive(true);
        tab.setAccessoryView(Some(&dot));
    }

    /// Alt başlığın yazımı; metni kuran `AppDelegate::post_notices`.
    pub(crate) fn set_subtitle(&self, subtitle: &NSString) {
        self.ivars().window.setSubtitle(subtitle);
    }

    /// İlk pane'in oturumunu açar ([`TerminalPane::start`], doğum paketinden)
    /// ve başlığı bir kez oturumdan okur: oturum yuvaya girmeden önce gelmiş
    /// bir başlık haberi boş yuva bulup düşmüş olabilir; bu okuma o pencereyi
    /// kapatıyor (değişmemişse aynı `bateri`'yi yazar). Hata çağırana
    /// dönüyor: ilk pencerede süreç çıkıyor, ⌘T/⌘N'de yalnız o pencere
    /// kapanıyor.
    pub(crate) fn start(&self, mtm: MainThreadMarker) -> std::io::Result<()> {
        self.focused_pane().start(mtm)?;
        self.refresh_title();
        Ok(())
    }

    /// `[remote] hosts` değişti — desen listesi her pane'in oturumuna
    /// ([`TerminalPane::set_host_marks`]), sekmenin noktası yeni çözümden.
    pub(crate) fn set_host_marks(&self, settings: &Settings) {
        for pane in self.panes() {
            pane.set_host_marks(settings);
        }
        self.refresh_tab_mark();
    }

    /// Temayı pane'lere verir ([`TerminalPane::set_theme`]: oturum ve arama
    /// paneli), ayırıcıyı ([`SplitView::set_theme`]) ve kromu ona boyar
    /// ([`TerminalWindow::apply_chrome`]).
    ///
    /// İkisi tek çağrıda, çünkü temayı değiştiren iki yol var
    /// (`AppDelegate::reload_settings`, `AppDelegate::apply_appearance`) ve
    /// biri kromu unutsaydı ızgara yeni temada, başlık çubuğu eskisinde
    /// kalırdı — belirti tam da kullanıcının göreceği dikiş.
    pub(crate) fn set_theme(&self, theme: Theme) {
        for pane in self.panes() {
            pane.set_theme(theme);
        }
        self.ivars().container.set_theme(&theme);
        self.apply_chrome(&theme);
        // Sekmenin noktası işaretin rolünden; rol yeni temada başka bir renk.
        self.refresh_tab_mark();
    }

    /// Pencere kromunu temaya boyar (026 → Karar 1, Seçenek C): başlık
    /// çubuğu saydam ve ayırıcısız, pencerenin zemini temanın `background`'ı,
    /// görünümü (trafik ışıkları, başlık metni, sekme çubuğu) zeminin
    /// açıklığından ([`is_dark_background`]).
    ///
    /// Saydam başlık çubuğunun altında görünen şey pencerenin zemini, yani
    /// tek sekmede başlık ile içerik **tek yüzey**: clear rengi aynı temadan
    /// (`Theme::background_linear`). Renk burada **sRGB** kuruluyor, lineer
    /// değil — lineer değer `bt-gpu`'nun, çünkü onu sRGB'ye donanım
    /// kodluyor; `NSColor`'a lineer vermek zemini açardı (`CLAUDE.md` →
    /// Renk uzayı).
    ///
    /// Pencereye görünüm kurmak onu sistemin görünümünden **koparıyor**:
    /// view artık sistemin açık/koyu değişimini görmüyor ve görünüm değişimi
    /// uygulamanın kendisinden izleniyor (`AppDelegate::observe_appearance`).
    ///
    /// Kurucuda değil pencere görünmeden hemen önce ilk kez çağrılıyor
    /// (`AppDelegate::open_window` → [`TerminalWindow::set_theme`]): tema
    /// oradan geliyor ve sonra boyamak
    /// her ⌘T'de bir kare sistemin gri çubuğunu gösterirdi.
    pub(crate) fn apply_chrome(&self, theme: &Theme) {
        // Krom yalnız zeminden türüyor; aynı zeminde AppKit'e yeniden renk ve
        // görünüm vermek her ayar kaydında bütün başlık çubuklarını yeniden
        // çizdirirdi (`Session::set_theme`'in aynı temada no-op olmasının
        // ikizi).
        if self.ivars().chrome.replace(Some(theme.background)) == Some(theme.background) {
            return;
        }
        let window = &self.ivars().window;
        window.setTitlebarAppearsTransparent(true);
        window.setTitlebarSeparatorStyle(NSTitlebarSeparatorStyle::None);
        let [r, g, b] = theme.background_srgb().map(|byte| f64::from(byte) / 255.0);
        window.setBackgroundColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(
            r, g, b, 1.0,
        )));
        // SAFETY: AppKit'in dışa açtığı iki sabit `NSString`; süreç boyunca
        // yaşıyorlar ve yalnız okunuyorlar (`NSRunLoopCommonModes` emsali).
        let name = unsafe {
            if is_dark_background(theme) {
                NSAppearanceNameDarkAqua
            } else {
                NSAppearanceNameAqua
            }
        };
        window.setAppearance(NSAppearance::appearanceNamed(name).as_deref());
    }

    /// Kapanış sırasının pencereye düşen adımları — **başlatır, beklemez**.
    /// İki çağıranı var: pencerenin kapanışı (`windowWillClose:`, tutamaklar
    /// düşüyor) ve uygulamanın kapanışı (`AppDelegate::shutdown`, bütün
    /// tutamaklar tek son tarihe kadar bekleniyor). Sıra pane'in
    /// ([`TerminalPane::begin_close`]: yükleme kuyruğu, ritim, `Waker`,
    /// `SIGHUP`) ve **her** pane için; dönüş ağaç sırasıyla pane başına bir
    /// sonuç. İdempotent; oturumu hiç doğmamış pane'in yeri `None`.
    pub(crate) fn begin_close(&self) -> Vec<Option<Closing>> {
        self.panes().iter().map(|pane| pane.begin_close()).collect()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn term_program_version_is_the_workspace_version() {
        // Karar 3'ün bekçisi: `bt-core`'un sabiti ile uygulamanın sürümü aynı
        // alandan (`version.workspace = true`); biri ayrışırsa burada kızarır.
        assert_eq!(bt_core::TERM_PROGRAM_VERSION, env!("CARGO_PKG_VERSION"));
    }

    use std::cell::Cell;

    use super::{
        CloseScope, Unit, close_scope, is_dark_background, prompt, should_ask, tab_index, unit_for,
    };
    use crate::jobs::Foreground;
    use bt_core::{ConfirmClose, Theme};

    const ALL: [ConfirmClose; 3] = [
        ConfirmClose::Never,
        ConfirmClose::Running,
        ConfirmClose::Always,
    ];

    fn running(names: &[&str]) -> Foreground {
        Foreground::Running(names.iter().map(|&name| name.to_owned()).collect())
    }

    fn with_background(background: u32) -> Theme {
        Theme {
            background,
            ..Theme::BATERI
        }
    }

    #[test]
    fn black_is_dark_and_white_is_light() {
        assert!(is_dark_background(&with_background(0x000000)));
        assert!(!is_dark_background(&with_background(0xffffff)));
    }

    #[test]
    fn embedded_themes_get_their_own_appearance() {
        assert!(is_dark_background(&Theme::BATERI), "bateri koyu");
        assert!(
            !is_dark_background(&Theme::BATERI_LIGHT),
            "bateri-light açık"
        );
    }

    #[test]
    fn mid_grey_splits_at_equal_contrast() {
        // Beyaz ve siyahla kontrastın eşitlendiği parlaklık √(1.05·0.05) −
        // 0.05, sRGB'de #757575 ile #767676 arasına düşüyor: orta gri açık
        // (siyah metin daha okunur), birkaç ton koyusu koyu.
        assert!(!is_dark_background(&with_background(0x808080)));
        assert!(is_dark_background(&with_background(0x606060)));
    }

    #[test]
    fn numbered_tabs_select_the_nth_or_nothing() {
        assert_eq!(tab_index(1, 3), Some(0));
        assert_eq!(tab_index(3, 3), Some(2));
        assert_eq!(tab_index(4, 3), None, "olmayan sekme no-op");
        assert_eq!(tab_index(8, 8), Some(7));
        assert_eq!(
            tab_index(8, 20),
            Some(7),
            "⌘8 dokuzdan fazla sekmede de sekizinci"
        );
    }

    #[test]
    fn nine_selects_the_last_tab() {
        assert_eq!(tab_index(9, 1), Some(0), "tek sekmede ⌘9 o sekme");
        assert_eq!(tab_index(9, 3), Some(2));
        assert_eq!(tab_index(9, 20), Some(19));
    }

    #[test]
    fn a_timed_run_never_asks_and_never_reads_the_table() {
        // Süreli koşu ayar okumuyor ve başsız bir soru `make duman`'ı asardı:
        // cevap her ayarda hayır **ve** tablo hiç okunmuyor (R2.1).
        for confirm in ALL {
            for busy in [false, true] {
                let reads = Cell::new(0);
                let ask = should_ask(true, confirm, || {
                    reads.set(reads.get() + 1);
                    busy
                });
                assert!(!ask, "{confirm:?}");
                assert_eq!(reads.get(), 0, "{confirm:?}: tablo okundu");
            }
        }
    }

    #[test]
    fn never_and_always_decide_without_the_table() {
        for busy in [false, true] {
            let reads = Cell::new(0);
            let read = || {
                reads.set(reads.get() + 1);
                busy
            };
            assert!(!should_ask(false, ConfirmClose::Never, read));
            assert!(should_ask(false, ConfirmClose::Always, read));
            assert_eq!(reads.get(), 0, "karar tabloya bağlı değilken okundu");
        }
    }

    #[test]
    fn running_asks_only_while_a_job_runs() {
        let reads = Cell::new(0);
        let ask = |busy| {
            should_ask(false, ConfirmClose::Running, || {
                reads.set(reads.get() + 1);
                busy
            })
        };
        assert!(ask(true));
        assert!(!ask(false));
        assert_eq!(reads.get(), 2);
    }

    #[test]
    fn titles_and_buttons_follow_the_scope() {
        let vim = [running(&["vim"])];
        let tab = prompt(CloseScope::Tab, Unit::Tab, &vim);
        assert_eq!(
            (tab.title.as_str(), tab.confirm),
            ("Close this tab?", "Close")
        );
        let tabs = prompt(CloseScope::Tabs(2), Unit::Tab, &vim);
        assert_eq!(
            (tabs.title.as_str(), tabs.confirm),
            ("Close 2 tabs?", "Close")
        );
        let window = prompt(CloseScope::Window, Unit::Tab, &vim);
        assert_eq!(
            (window.title.as_str(), window.confirm),
            ("Close this window?", "Close")
        );
        let quit = prompt(CloseScope::Quit, Unit::Tab, &vim);
        assert_eq!(
            (quit.title.as_str(), quit.confirm),
            ("Quit bateri?", "Quit")
        );
    }

    #[test]
    fn the_gesture_scope_comes_from_how_many_tabs_it_asked_for() {
        // Ölçülen jestler (phase-2 Uygulama Notları): ⌘W tek sekme, kırmızı
        // düğme grubun tamamı, "Close Other Tabs" seçili olmayanlar.
        assert_eq!(close_scope(1, 1), CloseScope::Window, "tek sekmeli pencere");
        assert_eq!(close_scope(1, 3), CloseScope::Tab, "⌘W");
        assert_eq!(close_scope(3, 3), CloseScope::Window, "kırmızı düğme");
        assert_eq!(close_scope(2, 3), CloseScope::Tabs(2), "Close Other Tabs");
    }

    #[test]
    fn one_tab_names_what_runs_in_it() {
        let message =
            |names: &[&str]| prompt(CloseScope::Tab, Unit::Tab, &[running(names)]).message;
        assert_eq!(
            message(&["claude"]),
            "“claude” is still running. Closing ends it."
        );
        assert_eq!(
            message(&["make", "cc"]),
            "“make” and “cc” are still running. Closing ends them."
        );
        assert_eq!(
            message(&["a", "b", "c"]),
            "“a”, “b” and “c” are still running. Closing ends them."
        );
        // Tablo okunamadı ama iş koşuyor sayıldı (R1.5): adsız.
        assert_eq!(message(&[]), "A process is still running. Closing ends it.");
    }

    #[test]
    fn only_the_running_tab_counts_in_a_group() {
        // Üç sekmeli pencerede tek sekmede iş var: sayı değil ad.
        let tabs = [Foreground::Idle, running(&["vim"]), Foreground::Idle];
        assert_eq!(
            prompt(CloseScope::Window, Unit::Tab, &tabs).message,
            "“vim” is still running. Closing ends it."
        );
    }

    #[test]
    fn many_tabs_are_counted_and_names_are_not_repeated() {
        let tabs = [
            running(&["claude"]),
            Foreground::Idle,
            running(&["vim"]),
            running(&["claude"]),
        ];
        assert_eq!(
            prompt(CloseScope::Window, Unit::Tab, &tabs).message,
            "Processes are running in 3 tabs: “claude”, “vim”. Closing ends them."
        );
        assert_eq!(
            prompt(CloseScope::Quit, Unit::Tab, &tabs).message,
            "Processes are running in 3 tabs: “claude”, “vim”. Quitting ends them."
        );
        // Hiçbirinin adı okunamadıysa yalnız sayı.
        assert_eq!(
            prompt(CloseScope::Quit, Unit::Tab, &[running(&[]), running(&[])]).message,
            "Processes are running in 2 tabs. Quitting ends them."
        );
    }

    #[test]
    fn always_says_what_closes_when_nothing_runs() {
        let message =
            |scope, tabs: usize| prompt(scope, Unit::Tab, &vec![Foreground::Idle; tabs]).message;
        assert_eq!(
            message(CloseScope::Tab, 1),
            "Closing this tab ends its shell session."
        );
        assert_eq!(
            message(CloseScope::Tabs(2), 2),
            "Closing these 2 tabs ends their shell sessions."
        );
        assert_eq!(
            message(CloseScope::Window, 1),
            "Closing this window ends its shell session."
        );
        assert_eq!(
            message(CloseScope::Window, 3),
            "Closing this window ends the shell sessions in its 3 tabs."
        );
        assert_eq!(
            message(CloseScope::Quit, 1),
            "Quitting ends the open shell session."
        );
        assert_eq!(
            message(CloseScope::Quit, 4),
            "Quitting ends 4 open shell sessions."
        );
    }

    #[test]
    fn one_pane_per_tab_keeps_the_tab_wording() {
        // Tek pane'li sekmelerde birim sekme ve metin bölmelerden öncekinin
        // aynısı (039 Karar 11); pane sayısı sekmeyi aşınca pane.
        assert_eq!(unit_for(1, 1), Unit::Tab);
        assert_eq!(unit_for(3, 3), Unit::Tab);
        assert_eq!(unit_for(2, 1), Unit::Pane);
        assert_eq!(unit_for(4, 3), Unit::Pane);
    }

    #[test]
    fn many_panes_are_counted_as_panes() {
        let panes = [running(&["vim"]), running(&["claude"]), Foreground::Idle];
        assert_eq!(
            prompt(CloseScope::Window, Unit::Pane, &panes).message,
            "Processes are running in 2 panes: “vim”, “claude”. Closing ends them."
        );
        assert_eq!(
            prompt(CloseScope::Window, Unit::Pane, &vec![Foreground::Idle; 3]).message,
            "Closing this window ends the shell sessions in its 3 panes."
        );
        // Tek koşan iş adıyla, birimden bağımsız.
        assert_eq!(
            prompt(
                CloseScope::Window,
                Unit::Pane,
                &[Foreground::Idle, running(&["vim"])]
            )
            .message,
            "“vim” is still running. Closing ends it."
        );
    }

    #[test]
    fn one_pane_asks_about_the_pane() {
        let pane = prompt(CloseScope::Pane, Unit::Pane, &[running(&["htop"])]);
        assert_eq!(
            (pane.title.as_str(), pane.confirm, pane.message.as_str()),
            (
                "Close this pane?",
                "Close",
                "“htop” is still running. Closing ends it."
            )
        );
        assert_eq!(
            prompt(CloseScope::Pane, Unit::Pane, &[Foreground::Idle]).message,
            "Closing this pane ends its shell session."
        );
    }

    #[test]
    fn no_tabs_or_unknown_tags_select_nothing() {
        assert_eq!(tab_index(1, 0), None);
        assert_eq!(tab_index(9, 0), None);
        assert_eq!(tab_index(0, 3), None);
        assert_eq!(tab_index(10, 12), None);
    }
}
