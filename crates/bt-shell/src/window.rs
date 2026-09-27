//! Terminal penceresi: bir `NSWindow`, onun tek pane'i (`pane::TerminalPane`
//! — oturum, link, renderer, yüzey, `BateriView`) ve **pencereye** ait olan
//! her şey: krom, başlık, sekme noktası, kapatma sorusu, arama paneli ve
//! yükleme kuyruğu; pencerenin `NSWindowDelegate`'i de burada.
//!
//! Uygulama geneli (ayarlar, izleme, alt başlık yuvaları, ölçüm defteri,
//! süreli koşu tarifi, pencere listesi) `app`'te; oradan gelen kayıt anı
//! yolları **her pane'e** varır (`TerminalWindow::pane`). Pencerenin
//! geometri, örtülme ve odak bildirimleri de pane'e dağıtılıyor. Çizim
//! çağrısı burada da yok, bu dosyanın işi bağlamak.
//!
//! Renderer pane başına (`pane`'in başlığı; 039 Karar 5).

use std::cell::{Cell, OnceCell, RefCell};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use block2::RcBlock;
use bt_core::{
    ConfirmClose, HostMark, SearchCover, SearchDirection, SearchReport, SearchStatus, Session,
    Settings, ShutdownHandle, Teardown, Theme,
};
use bt_gpu::GpuError;
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAppearance, NSAppearanceCustomization,
    NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSBackingStoreType, NSBox,
    NSBoxType, NSButton, NSColor, NSControlTextEditingDelegate, NSEventModifierFlags, NSMenuItem,
    NSModalResponse, NSModalResponseCancel, NSPasteboard, NSPasteboardNameFind, NSPopoverDelegate,
    NSSearchFieldDelegate, NSTextFieldDelegate, NSTitlePosition, NSTitlebarSeparatorStyle, NSView,
    NSWindow, NSWindowDelegate, NSWindowOcclusionState, NSWindowOrderingMode, NSWindowStyleMask,
};
use objc2_foundation::{
    NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, ns_string,
};

use crate::Run;
use crate::app::{self, AppDelegate};
use crate::clipboard;
use crate::jobs::Foreground;
use crate::pane::TerminalPane;
use crate::search_bar::{SearchBar, selection_query};
use crate::upload::{self, Uploads};
use crate::uploader::{StopSheet, UploadPopover};
use crate::view::BateriView;
use crate::zoom::Zoom;

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

/// Sistemin find panosundaki metin (Karar 6: ⌘E'nin uygulamalar arası
/// normu), ⌘E'nin sorgusuyla aynı süzgeçten: ilk satırı, boş ya da yalnız
/// boşluksa `None` ([`selection_query`]).
fn find_pasteboard_text() -> Option<String> {
    // SAFETY: AppKit'in dışa açtığı sabit ad, süreç boyunca yaşıyor.
    let name = unsafe { NSPasteboardNameFind };
    clipboard::read(&NSPasteboard::pasteboardWithName(name))
        .and_then(|text| selection_query(&text, false))
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

/// Kapatılan şey — sorunun başlığını ve onay düğmesini seçiyor
/// (`.tasks/028-kapatma-onayi/discussion.md` → Karar 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CloseScope {
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

/// Sorulacaksa kapanan her sekmenin ön planı, sorulmayacaksa `None` —
/// [`should_ask`]'ın pencereler üstündeki hâli.
///
/// Tablo `running`'de karar için bir kez okunuyor ve metin aynı okumayı
/// kullanıyor; `always`'de karar tabloya bakmıyor ama metin koşan işin adını
/// yine söylemek istiyor, o yüzden soru kesinleşince okunuyor.
pub(crate) fn foregrounds_to_ask(
    timed: bool,
    confirm: ConfirmClose,
    tabs: &[&TerminalWindow],
) -> Option<Vec<Foreground>> {
    let read = || {
        tabs.iter()
            .map(|tab| tab.pane().foreground())
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

/// Sorunun metni: başlık, açıklama ve onay düğmesi.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Prompt {
    pub(crate) title: String,
    pub(crate) message: String,
    pub(crate) confirm: &'static str,
}

/// Kapanan sekmelerin ön planlarından sorunun metni (Karar 4). Saf; soruyu
/// kuran üç yolun da tek metin kaynağı ([`alert`]).
///
/// Açıklama koşan işleri **adıyla** sayıyor: tek sekmede adlar
/// ("“claude” is still running."), birden çok sekmede sekme sayısı ve
/// tekrarsız adlar. Adı okunamayan iş adsız söyleniyor ("A process"), koşan
/// iş hiç yoksa (`always`) kapanacak şey.
pub(crate) fn prompt(scope: CloseScope, tabs: &[Foreground]) -> Prompt {
    let (title, confirm, verb) = match scope {
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
        [] => idle_message(scope, tabs.len()),
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
            if names.is_empty() {
                format!("Processes are running in {count} tabs. {verb} ends them.")
            } else {
                let names: Vec<String> = names.into_iter().map(|name| quoted(name)).collect();
                format!(
                    "Processes are running in {count} tabs: {}. {verb} ends them.",
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
fn idle_message(scope: CloseScope, tabs: usize) -> String {
    match (scope, tabs) {
        (CloseScope::Tab, _) => "Closing this tab ends its shell session.".to_owned(),
        (CloseScope::Tabs(n), _) => format!("Closing these {n} tabs ends their shell sessions."),
        (CloseScope::Window, 0 | 1) => "Closing this window ends its shell session.".to_owned(),
        (CloseScope::Window, n) => {
            format!("Closing this window ends the shell sessions in its {n} tabs.")
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

/// Pencerenin durumu — **pencereye** ait olan: krom, sekme noktası, kapatma
/// sorusu, arama paneli ve yükleme kuyruğu. Oturumun çekirdeği (oturum,
/// link, renderer, yüzey, view, dock payı, punto, kimlik) pencerenin tek
/// pane'inde ([`TerminalPane`], 039 Karar 1–2).
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
    /// Pencerenin tek pane'i ve `contentView`'ı: `NSWindow` onu zaten güçlü
    /// tutuyor, bu kopya tipli erişim için ([`TerminalWindow::pane`]).
    pane: Retained<TerminalPane>,
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
    /// Geçmişte aramanın paneli (033) — ilk ⌘F'de doğuyor: hiç aranmayan
    /// sekme görünümlerini taşımıyor. Sorgu ve anahtarlar panelde, yani
    /// **sekme başına** ve kapanınca unutulmuyor (Karar 6).
    search: OnceCell<SearchBar>,
    /// Oturuma verilen son sorgunun durumu — etiketin girdisi.
    search_status: Cell<SearchStatus>,
    /// Sayım dizininin sürücüsü ana kuyrukta bir tur bekliyor mu
    /// ([`TerminalWindow::kick_search`]): ikinci bir sürücü kurulmasın.
    search_driving: Cell<bool>,
    /// Finder damlasının uzak dizine yüklenmesi (037 Karar 7): sıra,
    /// ilerleme ve sonuç satırı ([`crate::upload::Uploads`]). Kuyruk **bu
    /// sekmenin ssh bağlantısının** — başka sekmeye geçmek onu durdurmuyor.
    uploads: RefCell<Uploads>,
    /// Açık yükleme sayfası (onay ya da hata): sayfa süresince yaşıyor.
    upload_alert: RefCell<Option<Retained<NSAlert>>>,
    /// Açık durdurma sorusu (037 phase-7, [`crate::uploader`]).
    upload_stop: RefCell<Option<StopSheet>>,
    /// Açık "Show files (N)" popover'ı (037 phase-7).
    upload_list: RefCell<Option<UploadPopover>>,
    /// Popover'ı kapatan olayın zamanı (`popoverWillClose:`): düğmeye
    /// yeniden basış popover'ı yeniden açmasın.
    list_closed_at: Cell<Option<f64>>,
}

/// Yeni kabuğun doğum bilgisi — [`TerminalWindow::start`]'ın çağırandan
/// aldığı iki karar (`AppDelegate::open_window`).
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
        #[unsafe(method(windowDidChangeBackingProperties:))]
        fn window_did_change_backing(&self, _n: &NSNotification) {
            if let Some(app) = app::delegate(self.mtm()) {
                self.pane().refresh_geometry(&app);
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
            if let Some(link) = self.pane().link() {
                link.set_visible(visible);
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
        #[unsafe(method(windowDidBecomeKey:))]
        fn window_did_become_key(&self, _n: &NSNotification) {
            self.pane().apply_focus(true);
            self.rehover_upload();
        }

        #[unsafe(method(windowDidResignKey:))]
        fn window_did_resign_key(&self, _n: &NSNotification) {
            self.pane().apply_focus(false);
            self.unhover_upload();
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
            // Pane'in kapanışı çerçeve gözlemcisini de söküyor ve pane'i
            // `AppDelegate::pane`'in aramasından düşürüyor
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

    // Arama alanının delegesi (033): üç protokolün de bütün yöntemleri
    // isteğe bağlı; kullanılanlar aşağıdaki `impl`'de.
    /// "Show files (N)" popover'ının kapanışı (037 phase-7): `transient`
    /// popover'ı AppKit de kapatıyor (dışarı tık) ve düğmenin basılı tonu
    /// ile Esc izleyicisi o zaman da kalkmalı.
    unsafe impl NSPopoverDelegate for TerminalWindow {
        #[unsafe(method(popoverWillClose:))]
        fn popover_will_close(&self, _n: &NSNotification) {
            self.upload_list_will_close();
        }

        #[unsafe(method(popoverDidClose:))]
        fn popover_did_close(&self, _n: &NSNotification) {
            self.close_upload_list();
        }
    }

    unsafe impl NSControlTextEditingDelegate for TerminalWindow {}
    unsafe impl NSTextFieldDelegate for TerminalWindow {}
    unsafe impl NSSearchFieldDelegate for TerminalWindow {}

    // **Pencereye ait eylemler** burada, uygulama geneline yayılanlar
    // (`settingsDidChange:`, tema, `openSettings:`)
    // `AppDelegate`'te. Hedefsiz eylemin responder zinciri view → pencere →
    // **pencere delegate'i** → `NSApp` → app delegate; yani bu nesne yayılan
    // bir seçiciyi uygulasaydı key pencere onu yutar ve öteki pencereler hiç
    // duymazdı. Punto ise pencerenin kendi durumu — sekme başına puntonun
    // (026 → Karar 3) ön koşulu.
    impl TerminalWindow {
        /// View ▸ Bigger (Cmd +).
        #[unsafe(method(makeFontBigger:))]
        fn make_font_bigger(&self, _sender: Option<&AnyObject>) {
            self.pane().change_zoom(Zoom::bigger);
        }

        /// View ▸ Smaller (Cmd −).
        #[unsafe(method(makeFontSmaller:))]
        fn make_font_smaller(&self, _sender: Option<&AnyObject>) {
            self.pane().change_zoom(Zoom::smaller);
        }

        /// View ▸ Actual Size (Cmd 0): fark sıfırlanır, ayarın puntosu.
        #[unsafe(method(resetFontSize:))]
        fn reset_font_size(&self, _sender: Option<&AnyObject>) {
            self.pane().change_zoom(|_, _| Zoom::default());
        }

        /// Shell ▸ Close Tab (⌘W): **yalnız bu sekme**, gerekirse sorarak.
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

        /// Shell ▸ Close Window (⇧⌘W): pencereyi **bütün sekmeleriyle**.
        ///
        /// Grubun tamamı için **tek** soru (R2.3) ve onayda her sekme
        /// `close` ile kapanıyor — `performClose:` değil, çünkü o her sekmenin
        /// `windowShouldClose:`'undan geçer ve sekme başına ikinci bir soru
        /// doğururdu. Kapanışın kendisi yine her sekmenin `windowWillClose:`'u.
        #[unsafe(method(closeWindow:))]
        fn close_window(&self, _sender: Option<&AnyObject>) {
            self.close_group_asking();
        }

        /// Edit ▸ Find ▸ Find… (⌘F): paneli açar, alanı odaklar ve metnini
        /// seçer (Karar 5); panel açıksa yalnız odak ve seçim. Sekmenin
        /// sorgusu yoksa alan find panosunun metniyle doluyor (Karar 6).
        ///
        /// Seçiciler **kendi adlarımız**, `performFindPanelAction:` değil
        /// (Karar 10): alan odaktayken first responder AppKit'in alan
        /// düzenleyicisi ve o seçiciyi kendisi uygulayıp yutardı. Eylemler
        /// pencerenin delegesinde, çünkü alan odaktayken responder zinciri
        /// `BateriView`'dan geçmiyor.
        #[unsafe(method(findInScrollback:))]
        fn find_in_scrollback(&self, _sender: Option<&AnyObject>) {
            self.open_search(true);
        }

        /// Edit ▸ Find ▸ Find Next (⌘G) ve panelin yukarı oku: bir önceki,
        /// **daha eski** eşleşme (Karar 3).
        #[unsafe(method(findNextMatch:))]
        fn find_next_match(&self, _sender: Option<&AnyObject>) {
            self.search_step(SearchDirection::Older);
        }

        /// Edit ▸ Find ▸ Find Previous (⇧⌘G) ve panelin aşağı oku: daha yeni.
        #[unsafe(method(findPreviousMatch:))]
        fn find_previous_match(&self, _sender: Option<&AnyObject>) {
            self.search_step(SearchDirection::Newer);
        }

        /// Edit ▸ Find ▸ Use Selection for Find (⌘E; Karar 6): seçim (ızgara
        /// ya da dock) sorgu olur — regex kipinde kaçırılarak — ve sistemin
        /// find panosuna da yazılır, panel açılır.
        #[unsafe(method(useSelectionForFind:))]
        fn use_selection_for_find(&self, _sender: Option<&AnyObject>) {
            self.use_selection();
        }

        /// Edit ▸ Clear to Start (⌘K; 034 Karar 1): ekranı ve geçmişi
        /// siler, o anki blok kalır — `Session::clear_to_start`. Kabuğa bayt
        /// gitmiyor; alternatif ekranda öğe gri ve çağrı zaten no-op.
        #[unsafe(method(clearToStart:))]
        fn clear_to_start(&self, _sender: Option<&AnyObject>) {
            if let Some(session) = self.session() {
                session.clear_to_start();
            }
        }

        /// Edit ▸ Clear Scrollback (⌥⌘K): yalnız geçmiş —
        /// `Session::clear_scrollback`.
        #[unsafe(method(clearScrollback:))]
        fn clear_scrollback(&self, _sender: Option<&AnyObject>) {
            if let Some(session) = self.session() {
                session.clear_scrollback();
            }
        }

        /// View ▸ Scroll to Top (⌘Home): geçmişin başı. `bt-core`'a yeni
        /// kaydırma API'si yok (034 Muhakeme): `scroll_page`'in
        /// `saturating_mul`'u `i32::MAX` sayfayı geçmişin ucuna kırpıyor.
        #[unsafe(method(scrollToTop:))]
        fn scroll_to_top(&self, _sender: Option<&AnyObject>) {
            self.scroll_pages(i32::MAX);
        }

        /// View ▸ Scroll to Bottom (⌘End): dip — `scroll_locked` bant
        /// kuralıyla dibe iniyor.
        #[unsafe(method(scrollToBottom:))]
        fn scroll_to_bottom(&self, _sender: Option<&AnyObject>) {
            self.scroll_pages(-i32::MAX);
        }

        /// View ▸ Page Up (⌘PgUp): Shift+PgUp'ın yolu.
        #[unsafe(method(scrollPageUp:))]
        fn scroll_page_up(&self, _sender: Option<&AnyObject>) {
            self.scroll_pages(1);
        }

        /// View ▸ Page Down (⌘PgDn): Shift+PgDn'ın yolu.
        #[unsafe(method(scrollPageDown:))]
        fn scroll_page_down(&self, _sender: Option<&AnyObject>) {
            self.scroll_pages(-1);
        }

        /// Panelin kapatma düğmesi — Esc ile aynı yol (Karar 5).
        #[unsafe(method(closeSearch:))]
        fn close_search_action(&self, _sender: Option<&AnyObject>) {
            self.close_search();
        }

        /// Alanın eylemi: her metin değişimi (`sendsSearchStringImmediately`)
        /// ve ⊗ düğmesi.
        #[unsafe(method(searchFieldChanged:))]
        fn search_field_changed(&self, _sender: Option<&AnyObject>) {
            self.apply_search();
        }

        /// `Aa` ya da `.*` anahtarı değişti.
        #[unsafe(method(searchOptionsChanged:))]
        fn search_options_changed(&self, _sender: Option<&AnyObject>) {
            self.apply_search();
        }

        /// Alanın komut kancası (Karar 10): ⏎ bir önceki (daha eski), ⇧⏎
        /// bir sonraki (daha yeni) eşleşme; Esc paneli kapatır —
        /// `NSSearchField`'ın "metni sil" varsayılanı yerine. Kalan komutlar
        /// alanın kendisine (`false`).
        ///
        /// Shift seçiciden okunamıyor — iki tuş da `insertNewline:` — o yüzden
        /// olayın kendisinden.
        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn control_do_command(
            &self,
            _control: &AnyObject,
            _text_view: &AnyObject,
            command: Sel,
        ) -> bool {
            if command == sel!(insertNewline:) {
                let shift = NSApplication::sharedApplication(self.mtm())
                    .currentEvent()
                    .is_some_and(|event| event.modifierFlags().contains(NSEventModifierFlags::Shift));
                self.search_step(if shift {
                    SearchDirection::Newer
                } else {
                    SearchDirection::Older
                });
                true
            } else if command == sel!(cancelOperation:) {
                self.close_search();
                true
            } else {
                false
            }
        }

        /// Find öğelerinin, temizlemenin ve kaydırmanın etkinliği;
        /// **bilinmeyen öğe `true`** — punto, sekme ve kapatma eylemleri
        /// bugünkü gibi hep etkin. Temizleme ve kaydırma alternatif ekranda
        /// gri (034 Karar 2): birincil geçmiş orada erişilemez, gri öğe
        /// dürüst bir "burada olmaz"; oturum yoksa da gri.
        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: &NSMenuItem) -> bool {
            let action = item.action();
            // `return` yok: `define_class!` `bool`'u gövdenin sonunda çeviriyor.
            if action.is_some_and(is_scrollback_action) {
                self.session()
                    .is_some_and(|session| !session.alt_screen())
            } else if action == Some(sel!(findNextMatch:)) || action == Some(sel!(findPreviousMatch:)) {
                self.has_query()
            } else if action == Some(sel!(useSelectionForFind:)) {
                self.session()
                    .is_some_and(|session| session.has_selection())
            } else if action == Some(sel!(cancelUpload:)) {
                // ⌘. yalnız bu sekmede kuyruk varken (037 Karar 7); gri
                // öğenin kısayolu `keyDown:`'a düşüyor ve orada yutuluyor.
                self.ivars().uploads.borrow().active()
            } else {
                true
            }
        }

        /// Shell ▸ Cancel Upload (⌘.) ve popover'ın `Cancel all ⌘.`'u: bu
        /// sekmenin **bütün** yükleme kuyruğu (037 Karar 7 → Kullanıcı kararı
        /// 5); akan kalem 30 saniyeyi geçtiyse önce sorar (phase-7). Esc
        /// değil, çünkü klavye o sırada uzak kabuğa gidiyor. Menü kısayolu
        /// `keyDown:`'dan önce yakalanıyor, yani alternatif ekranda (vim)
        /// da çalışıyor — kapısı yalnız kuyruk (`validateMenuItem:`).
        #[unsafe(method(cancelUpload:))]
        fn cancel_upload(&self, _sender: Option<&AnyObject>) {
            self.request_stop(true);
        }

        /// Popover satırının düğmesi (`Cancel`/`Remove`): `tag` kalemin
        /// kimliği, sırası değil — sıra biten ve çıkarılan kalemlerle kayar.
        #[unsafe(method(uploadRowAction:))]
        fn upload_row_action_sent(&self, sender: Option<&AnyObject>) {
            let Some(button) = sender.and_then(|sender| sender.downcast_ref::<NSButton>()) else {
                return;
            };
            if let Ok(id) = u64::try_from(button.tag()) {
                self.upload_row_action(id);
            }
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

/// Alternatif ekranda gri olan altı öğe mi (034 Karar 2): temizlemenin iki
/// kipi ve dört kaydırma — hepsi birincil geçmişe dokunuyor ve o geçmiş
/// alternatif ekranda erişilemez.
fn is_scrollback_action(action: Sel) -> bool {
    [
        sel!(clearToStart:),
        sel!(clearScrollback:),
        sel!(scrollToTop:),
        sel!(scrollToBottom:),
        sel!(scrollPageUp:),
        sel!(scrollPageDown:),
    ]
    .contains(&action)
}

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
    /// da metallib yoksa pencerenin çizebileceği bir şey de yok. `pane_id`
    /// pane'in kimliği ([`TerminalPane::id`]), pencerenin kimliğiyle aynı
    /// sayaçtan.
    pub(crate) fn new(
        mtm: MainThreadMarker,
        id: u64,
        pane_id: u64,
        run: Option<Run>,
    ) -> Result<Retained<Self>, GpuError> {
        let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(900.0, 600.0));
        let pane = TerminalPane::new(mtm, pane_id, run, rect)?;
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
        // İçerik view'ı pane (layer-backed düz kapsayıcı, `BateriView` onun
        // çocuğu — `TerminalPane::new`); çerçevesini pencere kuruyor, çocuk
        // boyu autoresizing'le izliyor.
        window.setContentView(Some(&pane));
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
            pane: pane.clone(),
            chrome: Cell::new(None),
            tab_mark: Cell::new(None),
            alert: RefCell::new(None),
            close_requested: Cell::new(false),
            search: OnceCell::new(),
            search_status: Cell::new(SearchStatus::Empty),
            search_driving: Cell::new(false),
            uploads: RefCell::new(Uploads::default()),
            upload_alert: RefCell::new(None),
            upload_stop: RefCell::new(None),
            upload_list: RefCell::new(None),
            list_closed_at: Cell::new(None),
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

    /// Pencerenin tek pane'i (039 Karar 1; bölmeler phase-3'te).
    pub(crate) fn pane(&self) -> &TerminalPane {
        &self.ivars().pane
    }

    /// Pane'in `Retained` kopyası — kimlikle arama (`AppDelegate::pane`).
    pub(crate) fn pane_handle(&self) -> Retained<TerminalPane> {
        self.ivars().pane.clone()
    }

    /// `bateri://tab/<id>`'nin tek etkisi (038 Karar 4, 6): küçültülmüşse
    /// geri açar, seçili sekme ve key yapar, uygulamayı öne alır. Kabuğa bayt
    /// göndermez.
    ///
    /// `makeKeyAndOrderFront` sekme grubundaki pencereyi seçili sekme
    /// yapıyor (`selectTab:`'ın emsali); küçültülmüş pencerede ise yalnız
    /// sırayı değiştirip Dock'ta bırakırdı, `deminiaturize` o yüzden önce.
    pub(crate) fn bring_to_front(&self) {
        let window = &self.ivars().window;
        if window.isMiniaturized() {
            window.deminiaturize(None);
        }
        window.makeKeyAndOrderFront(None);
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

    /// ⌘W: bu sekme için soru, ya da sorulmayacaksa hemen kapanış.
    fn close_tab_asking(&self) {
        let Some(app) = app::delegate(self.mtm()) else {
            return;
        };
        let Some(group) = self.group_unless_asking(&app) else {
            return;
        };
        let Some(this) = app.window(self.id()) else {
            return;
        };
        let scope = close_scope(1, group.len());
        self.confirm_close(&app, &group, &[this], scope);
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
        let tabs: Vec<&TerminalWindow> = targets.iter().map(|tab| &**tab).collect();
        let confirm = app.settings().confirm_close;
        let Some(foregrounds) = foregrounds_to_ask(self.ivars().run.is_some(), confirm, &tabs)
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
        host.ask(&prompt(scope, &foregrounds), ids);
    }

    /// Soruyu bu pencereye sayfa olarak açar; onayda `targets`'taki sekmeleri
    /// kapatır.
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
    fn ask(&self, prompt: &Prompt, targets: Vec<u64>) {
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
                for window in targets.iter().filter_map(|&id| app.window(id)) {
                    window.close();
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

    /// View ▸'nin dört kaydırması: `Session::scroll_page`'in yolu
    /// (Shift+PgUp/PgDn'ın ta kendisi) — kesir sıfırlanıyor, süzülme nesli
    /// artıyor, bant kuralı `scroll_locked`'ta. Alternatif ekranda `None` ve
    /// öğeler zaten gri; cevap burada okunmuyor.
    fn scroll_pages(&self, pages: i32) {
        if let Some(session) = self.session() {
            session.scroll_page(pages);
        }
    }

    /// Yükleme kuyruğu (`uploader`'ın yarısı).
    pub(crate) fn uploads(&self) -> &RefCell<Uploads> {
        &self.ivars().uploads
    }

    /// Açık yükleme sayfasının yuvası.
    pub(crate) fn upload_alert(&self) -> &RefCell<Option<Retained<NSAlert>>> {
        &self.ivars().upload_alert
    }

    /// Açık durdurma sorusunun yuvası.
    pub(crate) fn upload_stop(&self) -> &RefCell<Option<StopSheet>> {
        &self.ivars().upload_stop
    }

    /// Açık "Show files (N)" popover'ının yuvası.
    pub(crate) fn upload_list(&self) -> &RefCell<Option<UploadPopover>> {
        &self.ivars().upload_list
    }

    /// Popover'ı kapatan olayın zamanı.
    pub(crate) fn list_closed_at(&self) -> &Cell<Option<f64>> {
        &self.ivars().list_closed_at
    }

    /// Bu sekmenin `NSWindow`'u (sayfanın sahibi).
    pub(crate) fn ns_window(&self) -> &NSWindow {
        &self.ivars().window
    }

    /// Terminal view'ı (listenin açıldığı yer) — pane'inki.
    pub(crate) fn view(&self) -> &BateriView {
        self.pane().view()
    }

    /// Pane'in oturumu; arama ve yükleme bu phase'de pencerede ve oturuma
    /// buradan uzanıyor.
    pub(crate) fn session(&self) -> Option<&Arc<Session>> {
        self.pane().session()
    }

    /// Başlığı oturumdan okuyup pencereye yazar — `ShellWake::title_changed`'in
    /// ana kuyruk işi. Kare yolu başlık hesaplamıyor; yazım yalnız
    /// **değişimde** (026 R2.4). Oturum henüz yoksa başlık kurucunun
    /// `bateri`'si kalıyor.
    ///
    /// Sekmenin noktası da buradan tazeleniyor ([`Self::refresh_tab_mark`]):
    /// uzak durumun iki kenarı (`set_remote`'un dönüşü, `D`/`A`'nın silmesini
    /// getiren `title_changed`) başlığınkilerle aynı (037 Karar 4).
    pub(crate) fn refresh_title(&self) {
        self.apply_title();
        self.refresh_tab_mark();
        // Uzak durumun kenarı yükleme kuyruğunun da kenarı: ssh kapandıysa
        // bekleyenler iptal (037 Karar 7 → Kullanıcı kararı 6).
        self.check_upload_connection();
    }

    /// Pencerenin (ve sekmenin) başlığını oturumdan yazar; yükleme akarken
    /// önünde `↑ N% · ` (037 phase-7, `upload::titled`). Uzak durumun
    /// kenarını sormuyor — yükleme yolu onu kendi kenarında çağırıyor ve
    /// `check_upload_connection` kuyruğu bitirip buraya geri dönerdi.
    pub(crate) fn apply_title(&self) {
        if let Some(session) = self.session() {
            let percent = self.ivars().uploads.borrow().title_percent();
            self.ivars()
                .window
                .setTitle(&NSString::from_str(&upload::titled(
                    percent,
                    &session.title(),
                )));
        }
    }

    /// Uzak sekmenin host'u ve çözülmüş işareti; yerelde `None`
    /// (`Session::remote_mark`).
    pub(crate) fn remote_mark(&self) -> Option<(String, HostMark)> {
        self.session()?.remote_mark()
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
        let color = self.session().and_then(|session| {
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

    /// Pane'in oturumunu açar ([`TerminalPane::start`]) ve başlığı bir kez
    /// oturumdan okur: oturum yuvaya girmeden önce gelmiş bir başlık haberi
    /// boş yuva bulup düşmüş olabilir; bu okuma o pencereyi kapatıyor
    /// (değişmemişse aynı `bateri`'yi yazar). Hata çağırana dönüyor: ilk
    /// pencerede süreç çıkıyor, ⌘T/⌘N'de yalnız o pencere kapanıyor.
    pub(crate) fn start(
        &self,
        app: &AppDelegate,
        mtm: MainThreadMarker,
        theme: Theme,
        launch: Launch,
    ) -> std::io::Result<()> {
        self.pane().start(app, mtm, theme, launch)?;
        self.refresh_title();
        Ok(())
    }

    /// `[remote] hosts` değişti — desen listesi pane'in oturumuna
    /// ([`TerminalPane::set_host_marks`]), sekmenin noktası yeni çözümden.
    pub(crate) fn set_host_marks(&self, settings: &Settings) {
        self.pane().set_host_marks(settings);
        self.refresh_tab_mark();
    }

    /// Temayı pane'in oturumuna takas eder ([`TerminalPane::set_theme`]) ve
    /// kromu ona boyar ([`TerminalWindow::apply_chrome`]).
    ///
    /// İkisi tek çağrıda, çünkü temayı değiştiren iki yol var
    /// (`AppDelegate::reload_settings`, `AppDelegate::apply_appearance`) ve
    /// biri kromu unutsaydı ızgara yeni temada, başlık çubuğu eskisinde
    /// kalırdı — belirti tam da kullanıcının göreceği dikiş.
    pub(crate) fn set_theme(&self, theme: Theme) {
        self.pane().set_theme(theme);
        self.apply_chrome(&theme);
        // Sekmenin noktası işaretin rolünden; rol yeni temada başka bir renk.
        self.refresh_tab_mark();
        // Arama panelinin yüzeyi de temadan; panel henüz doğmadıysa ilk
        // ⌘F'de oturumun temasıyla boyanıyor.
        if let Some(bar) = self.ivars().search.get() {
            bar.paint(&theme, is_dark_background(&theme));
        }
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
    /// (`AppDelegate::open_window`): tema oradan geliyor ve sonra boyamak
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

    /// Arama paneli — ilk çağrıda kurulur, temaya boyanır.
    fn search_bar(&self) -> &SearchBar {
        self.ivars().search.get_or_init(|| {
            // Panel pane'in içinde, Metal katmanını taşıyan view'ın kardeşi
            // (033 → R4.1; pane o kapsayıcının ta kendisi, 039 Karar 2).
            let pane: &NSView = self.pane();
            let bar = SearchBar::new(
                self.mtm(),
                pane,
                self.view(),
                self,
                ProtocolObject::from_ref(self),
            );
            if let Some(session) = self.session() {
                let theme = session.theme();
                bar.paint(&theme, is_dark_background(&theme));
            }
            bar
        })
    }

    /// Paneli açar (açıksa yerinde bırakır) ve sorguyu uygular; `focus`
    /// ise alanı odaklayıp metnini seçer (⌘F).
    /// Sorgu bu çağrıda oturuma verildiyse `true` ([`TerminalWindow::apply_search`]).
    fn open_search(&self, focus: bool) -> bool {
        let bar = self.search_bar();
        if bar.query().text.is_empty()
            && let Some(text) = find_pasteboard_text()
        {
            bar.set_text(&text);
        }
        let animate = app::delegate(self.mtm()).is_some_and(|app| !app.reduce_motion());
        bar.show(animate);
        if focus {
            self.ivars().window.makeFirstResponder(Some(bar.field()));
            // SAFETY: gönderen isteğe bağlı; alanın kendi eylemi.
            unsafe { bar.field().selectText(None) };
        }
        self.apply_search()
    }

    /// Alanın ve anahtarların sorgusu değiştiyse oturuma verir, geçerli
    /// eşleşmeyi açığa çıkarır ve etiketi yazar; verdiyse `true`. Aynı sorgu
    /// no-op.
    fn apply_search(&self) -> bool {
        let (Some(bar), Some(session)) = (self.ivars().search.get(), self.session()) else {
            return false;
        };
        if !bar.is_shown() {
            return false;
        }
        let query = bar.query();
        if !bar.take_change(&query) {
            return false;
        }
        let status = session.set_search(&query);
        self.ivars().search_status.set(status);
        let report = if status == SearchStatus::Ready {
            session.search_reveal(self.search_cover(), self.smooth_scroll())
        } else {
            SearchReport::default()
        };
        bar.set_count(status, report);
        self.kick_search();
        true
    }

    /// Sayım dizininin sürücüsünü kurar (phase-5, Karar 2-B): ana kuyrukta
    /// bir sonraki turda bir parça. Zaten kuruluysa, panel kapalıysa ya da
    /// sorgu sayılacak bir desen değilse no-op.
    ///
    /// Çağıranları: sorgu değişimi, gezinme (sırası bilinmeyen yeni bir
    /// eşleşme bir geçiş daha isteyebilir) ve defter haberi
    /// ([`Wake::search_changed`]) — sonuncusu arka sekmede de.
    pub(crate) fn kick_search(&self) {
        let shown = self.ivars().search.get().is_some_and(SearchBar::is_shown);
        if !shown
            || self.ivars().search_status.get() != SearchStatus::Ready
            || self.ivars().search_driving.replace(true)
        {
            return;
        }
        self.schedule_search_chunk();
    }

    /// Sürücünün bir turu ana kuyruğa: pencere kimlikle bulunuyor
    /// (`ShellWake`'in örüntüsü), kapanan sekmede iş düşüyor.
    fn schedule_search_chunk(&self) {
        let id = self.id();
        DispatchQueue::main().exec_async(move || {
            // audit: ana kuyrukta koşan blok tanımı gereği ana thread'dedir.
            let mtm = MainThreadMarker::new().expect("ana kuyruk ana thread'dir");
            if let Some(window) = app::delegate(mtm).and_then(|app| app.window(id)) {
                window.search_chunk();
            }
        });
    }

    /// Dizinin bir parçası ve etiket; sayım bitmediyse bir sonraki tura
    /// yeniden kuruluyor — tuş olayları turların arasına giriyor. Durma
    /// koşulu çekirdeğin `complete`'i (geçiş bitti **ve** bekleyen defter
    /// haberi yok), panelin kapanması ya da aramanın düşmesi.
    fn search_chunk(&self) {
        let (Some(bar), Some(session)) = (self.ivars().search.get(), self.session()) else {
            self.ivars().search_driving.set(false);
            return;
        };
        let status = self.ivars().search_status.get();
        if !bar.is_shown() || status != SearchStatus::Ready {
            self.ivars().search_driving.set(false);
            return;
        }
        let Some(report) = session.search_step() else {
            self.ivars().search_driving.set(false);
            return;
        };
        bar.set_count(status, report);
        if report.complete {
            self.ivars().search_driving.set(false);
        } else {
            self.schedule_search_chunk();
        }
    }

    /// ⏎ / ⌘G / ⇧⏎ / ⇧⌘G: panel kapalıysa önce açılıyor (odak yerinde
    /// kalıyor), sonra bir sonraki eşleşme.
    ///
    /// Açılış sorguyu **yeniden** verdiyse (Esc aramayı kapatmıştı) adım
    /// o seçimin kendisi: `set_search` en yakın eşleşmeyi seçip açığa
    /// çıkardı ve üstüne bir adım daha ⇧⌘G'yi en eskiye sardırırdı
    /// (`/code-review`).
    fn search_step(&self, direction: SearchDirection) {
        if self.open_search(false) {
            return;
        }
        let (Some(bar), Some(session)) = (self.ivars().search.get(), self.session()) else {
            return;
        };
        let status = self.ivars().search_status.get();
        if status != SearchStatus::Ready {
            return;
        }
        let report = session.search_next(direction, self.search_cover(), self.smooth_scroll());
        bar.set_count(status, report);
        if !report.complete {
            self.kick_search();
        }
    }

    /// Esc ve kapatma düğmesi (Karar 5): panel gider, **pencere yerinde
    /// kalır**, geçerli eşleşme ızgaranın seçimi olur ve klavye terminale
    /// döner. Sorgu alanda kalıyor (Karar 6).
    fn close_search(&self) {
        let Some(bar) = self.ivars().search.get() else {
            return;
        };
        let animate = app::delegate(self.mtm()).is_some_and(|app| !app.reduce_motion());
        bar.hide(animate);
        bar.forget_applied();
        if let Some(session) = self.session() {
            session.select_search_match();
            session.clear_search();
        }
        self.ivars().search_status.set(SearchStatus::Empty);
        self.ivars().window.makeFirstResponder(Some(self.view()));
    }

    /// ⌘E (Karar 6): seçimin ilk satırı sorgu olur (regex kipinde
    /// kaçırılarak), find panosuna yazılır ve panel alanı odaklanmış açılır.
    fn use_selection(&self) {
        let Some(text) = self.session().and_then(|session| session.selection_text()) else {
            return;
        };
        let bar = self.search_bar();
        let Some(query) = selection_query(&text, bar.regex()) else {
            return;
        };
        bar.set_text(&query);
        // Pano **düz** metni taşıyor: öteki uygulamalar regex kipini bilmiyor.
        if let Some(plain) = selection_query(&text, false) {
            // SAFETY: AppKit'in dışa açtığı sabit ad, süreç boyunca yaşıyor.
            let name = unsafe { NSPasteboardNameFind };
            clipboard::copy(&NSPasteboard::pasteboardWithName(name), Some(plain));
        }
        self.open_search(true);
    }

    /// Find Next/Previous'ın kapısı: sekmenin sorgusu ya da find panosunda
    /// metin var mı.
    fn has_query(&self) -> bool {
        self.ivars()
            .search
            .get()
            .is_some_and(|bar| !bar.query().text.is_empty())
            || find_pasteboard_text().is_some()
    }

    /// Panelin örttüğü hücreler; panel kapalıysa hiçbiri.
    fn search_cover(&self) -> SearchCover {
        let Some(bar) = self.ivars().search.get().filter(|bar| bar.is_shown()) else {
            return SearchCover::default();
        };
        let view = self.view();
        view.search_cover(view.convertRect_fromView(bar.resting_frame(), Some(bar.parent())))
    }

    /// Kaydırmanın çözülmüş kipi: süzülme mi anında mı (`smooth_scroll`,
    /// Hareketi Azalt, `snap` — `app::resolve_smooth_scroll`).
    fn smooth_scroll(&self) -> bool {
        app::delegate(self.mtm()).is_some_and(|app| app.smooth_scroll())
    }

    /// Kapanış sırasının pencereye düşen adımları — **başlatır, beklemez**.
    /// İki çağıranı var: pencerenin kapanışı (`windowWillClose:`, tutamak
    /// düşüyor) ve uygulamanın kapanışı (`AppDelegate::shutdown`, bütün
    /// tutamaklar tek son tarihe kadar bekleniyor).
    ///
    /// Sıra zorunlu: önce yükleme kuyruğu bırakılıyor (süreçler öldürülüyor
    /// ve yarım dosya siliniyor; sonucu gösterecek bir dock kalmadı), **sonra**
    /// pane'in kapanışı ([`TerminalPane::begin_close`]: ritim, `Waker`,
    /// `SIGHUP`) — ters sırada iptal kabuğun `SIGHUP`'ından sonra giderdi.
    /// İdempotent; oturum hiç doğmadıysa `None`.
    pub(crate) fn begin_close(&self) -> Option<Closing> {
        self.abandon_uploads();
        self.pane().begin_close()
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

    use super::{CloseScope, close_scope, is_dark_background, prompt, should_ask, tab_index};
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
        let tab = prompt(CloseScope::Tab, &vim);
        assert_eq!(
            (tab.title.as_str(), tab.confirm),
            ("Close this tab?", "Close")
        );
        let tabs = prompt(CloseScope::Tabs(2), &vim);
        assert_eq!(
            (tabs.title.as_str(), tabs.confirm),
            ("Close 2 tabs?", "Close")
        );
        let window = prompt(CloseScope::Window, &vim);
        assert_eq!(
            (window.title.as_str(), window.confirm),
            ("Close this window?", "Close")
        );
        let quit = prompt(CloseScope::Quit, &vim);
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
        let message = |names: &[&str]| prompt(CloseScope::Tab, &[running(names)]).message;
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
            prompt(CloseScope::Window, &tabs).message,
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
            prompt(CloseScope::Window, &tabs).message,
            "Processes are running in 3 tabs: “claude”, “vim”. Closing ends them."
        );
        assert_eq!(
            prompt(CloseScope::Quit, &tabs).message,
            "Processes are running in 3 tabs: “claude”, “vim”. Quitting ends them."
        );
        // Hiçbirinin adı okunamadıysa yalnız sayı.
        assert_eq!(
            prompt(CloseScope::Quit, &[running(&[]), running(&[])]).message,
            "Processes are running in 2 tabs. Quitting ends them."
        );
    }

    #[test]
    fn always_says_what_closes_when_nothing_runs() {
        let message = |scope, tabs: usize| prompt(scope, &vec![Foreground::Idle; tabs]).message;
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
    fn no_tabs_or_unknown_tags_select_nothing() {
        assert_eq!(tab_index(1, 0), None);
        assert_eq!(tab_index(9, 0), None);
        assert_eq!(tab_index(0, 3), None);
        assert_eq!(tab_index(10, 12), None);
    }
}
