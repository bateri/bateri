//! Terminal penceresi: bir `NSWindow`, onun `BateriView`'ı, `CAMetalLayer`
//! yüzeyi, kendi `Renderer`'ı, shell oturumu ve kareyi süren display link —
//! **pencere başına** olan her şey ve pencerenin `NSWindowDelegate`'i.
//!
//! Uygulama geneli (ayarlar, izleme, alt başlık yuvaları, ölçüm defteri,
//! süreli koşu tarifi, pencere listesi) `app`'te; oradan gelen kayıt anı
//! yolları buradaki yöntemlere **her pencere için** varır. Çizim çağrısı
//! burada da yok, bu dosyanın işi bağlamak.
//!
//! **Renderer pencere başına** (`.tasks/026-sekmeler/discussion.md` → Karar
//! 2a): atlasın anahtarı ölçek ve punto içeriyor, yani paylaşılan tek renderer
//! farklı ölçekli ekranlardaki iki pencerede atlası birbirine çevirir ve sekme
//! başına puntoyu imkânsız kılardı.

use std::cell::{Cell, OnceCell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use block2::RcBlock;
use bt_core::{
    ConfirmClose, FontOptions, SearchCover, SearchDirection, SearchReport, SearchStatus, Session,
    SessionOptions, Settings, ShutdownHandle, Teardown, Theme, Wake,
};
use bt_core::{load_shell, smoke_shell};
use bt_gpu::{DisplayLink, GpuError, Layout, Renderer, Surface, Waker};
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAppearance, NSAppearanceCustomization,
    NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSAutoresizingMaskOptions,
    NSBackingStoreType, NSColor, NSControlTextEditingDelegate, NSEventModifierFlags, NSMenuItem,
    NSModalResponse, NSModalResponseCancel, NSPasteboard, NSPasteboardNameFind,
    NSSearchFieldDelegate, NSTextFieldDelegate, NSTitlebarSeparatorStyle, NSView,
    NSViewFrameDidChangeNotification, NSWindow, NSWindowDelegate, NSWindowOcclusionState,
    NSWindowOrderingMode, NSWindowStyleMask,
};
use objc2_foundation::{
    NSNotification, NSNotificationCenter, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize,
    NSString, ns_string,
};

use crate::app::{self, AppDelegate, Grid, split_into_grid};
use crate::child;
use crate::clipboard::{self, PendingCopy};
use crate::jobs::{self, Foreground, Libproc, ShellParent};
use crate::notices::{Source, font_messages};
use crate::search_bar::{SearchBar, selection_query};
use crate::view::BateriView;
use crate::zoom::Zoom;
use crate::{Run, Workload};

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

/// `bt-core`'un uyandırma ucu — pencere başına bir tane, oturumuyla birlikte.
///
/// `Session::spawn` `Wake`'i link'ten **önce** ister, `Waker` ise link'ten
/// sonra doğar; boşluğu yuvanın `None`'ı kapatır. Kaçan kare yok: açılış
/// karesi zaten elle isteniyor ve o ana kadar okunmuş her bayt hasar
/// bayrağında birikmiş olur.
struct ShellWake {
    /// Pencerenin kimliği: ana kuyruk işleri pencereyi listeden bununla
    /// buluyor (alternatif ekran habercisinin örüntüsü) — `Session`'a ya da
    /// pencereye referans tutmak `wake.rs`'in Sahiplik çemberini kapatırdı.
    id: u64,
    /// Süreli koşu mu: `child_exit` iki yola ayrılıyor ([`Wake::child_exit`]'in
    /// gövdesi). `AppDelegate`'inkinin kopyası; okuyucu thread'den uygulama
    /// delegate'ine uzanılamaz.
    timed: bool,
    /// Link'in `Waker`'ı — **yaprak kilit** altında ve **sökülebilir**.
    ///
    /// Pencere kapanırken ana thread'de `take()` ediliyor
    /// ([`ShellWake::detach`]): bu nesnenin son kopyası `"PTY teardown"`
    /// thread'inde düşebilir (`wake.rs` → Sahiplik) ve `Waker`'ın
    /// `MainThreadBound`'u oraya düşerse `Drop`'u ana kuyruğa senkron iş atar.
    /// Sökülmüş yuva o `Drop`'u yapısal olarak ana thread'e çiviliyor; eskiden
    /// tek koruma pencere listesinin `app.run()`'ı aşmasıydı.
    ///
    /// Kilit yaprak: `wake()` onu `Term` kilidi altında alıp bırakıyor ve
    /// altında başka kilit alınmıyor (`Theme`'in yaprak kilidi emsali).
    waker: Mutex<Option<Waker>>,
    /// OSC 52'nin ana kuyruğa bekleyen metni. `Arc`, çünkü ana kuyruğun işi
    /// `'static` ister ve `Wake`'in çağrısı yalnız `&self` veriyor; iş
    /// `ShellWake`'i değil yalnız yuvayı tutar.
    pending_copy: Arc<PendingCopy>,
    /// Başlık işi ana kuyrukta bekliyor mu — kuyruğa **en çok bir** iş
    /// (`PendingCopy`'nin örüntüsü, yük yerine bayrak: başlığın kendisi
    /// oturumda, iş onu okuyor).
    title_pending: Arc<AtomicBool>,
    /// Arama sayımının defter haberi ana kuyrukta bekliyor mu —
    /// `title_pending`'in ikizi (033).
    search_pending: Arc<AtomicBool>,
}

impl ShellWake {
    /// Yaprak kilidi alır; zehirlenmişse içindekiyle devam eder — yuvanın tek
    /// değişmezi "ya `Waker` var ya yok" ve yarım yazılmış bir hâli olamaz.
    fn slot(&self) -> MutexGuard<'_, Option<Waker>> {
        self.waker.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `Waker`'ı yuvadan söker ve çağırana verir — ana thread'de düşsün diye
    /// ([`ShellWake::waker`]). İkinci çağrı `None`.
    fn detach(&self) -> Option<Waker> {
        self.slot().take()
    }
}

impl Wake for ShellWake {
    fn wake(&self) {
        // Okuyucu thread; `Term` kilidi tutuluyor olabilir. Tek iş: ana
        // kuyruğa "link'i aç" işini at, hemen dön.
        // Kopya **alınmıyor**, kilit altında çağrılıyor: kopya burada son
        // referans kalıp okuyucu thread'de düşebilirdi — sökmenin
        // kapattığı yolun ta kendisi.
        if let Some(waker) = self.slot().as_ref() {
            waker.wake();
        }
    }

    fn child_exit(&self, _code: Option<i32>) {
        // Shell gitti, pencerenin dayanağı kalmadı: **o pencere** kapanır
        // (026 → Karar 5), uygulama değil. Kapanış pencerenin kendi
        // `windowWillClose:`'undan geçiyor — kırmızı düğme, ⌘W ve `exit` aynı
        // sıraya varır.
        //
        // **Süreli koşuda** eski yol: doğrudan `terminate:`. Rapor pencerenin
        // sayaçlarını okuyor ve pencere rapordan önce listeden düşseydi duman
        // reçetesi deadline'dan kısa bittiğinde rapor boş listeyle koşardı
        // (`will_terminate`'in doc'u).
        //
        // Ana kuyruğa atılmasının iki sebebi var ve ikisi de zorunlu: AppKit
        // ana thread ister, ve bu çağrı **okuyucu thread'de** geliyor —
        // kapanışa giden senkron bir yol okuyucu thread'i kendi kapanışında
        // bekletirdi (`wake.rs` → Sahiplik).
        //
        // **Bilinen sınır:** shell'in son çıktısı ekrana gelmeyebilir.
        // alacritty sırayı `ChildExit` → `Wakeup` diye kuruyor, yani buraya
        // geldiğimizde son bayt henüz çizilmemiş olabilir; kapanış da araya
        // bir vsync girmeden koşar. Garanti etmek ya sihirli bir gecikme ya da
        // display link'e "hasar tükendi, şimdi çık" semantiği eklemek olurdu —
        // ikincisi renderer'a terminal bilgisi sokar. `bateri -e cmd` yolu
        // geldiğinde `drain_on_exit` ile birlikte tasarlanacak
        // (`.tasks/002-vt-motoru/phase-4.md` → Uygulama Notları).
        let (timed, id) = (self.timed, self.id);
        DispatchQueue::main().exec_async(move || {
            // audit: ana kuyrukta koşan blok tanımı gereği ana thread'dedir.
            let mtm = MainThreadMarker::new().expect("ana kuyruk ana thread'dir");
            if timed {
                NSApplication::sharedApplication(mtm).terminate(None);
                return;
            }
            // Pencere bu arada kapandıysa (⌘W'nin `SIGHUP`'ı kabuğu öldürdü
            // ve haber sonradan geldi) kapatacak bir şey yok.
            if let Some(window) = app::delegate(mtm).and_then(|app| app.window(id)) {
                window.close();
            }
        });
    }

    fn copy_to_clipboard(&self, text: String) {
        // Okuyucu thread, `Term` kilidi tutuluyor: metin kilitsiz yuvaya,
        // ana kuyruğa en çok **bir** iş (`PendingCopy`'nin doc'u). Yuvada
        // bekleyen metin varsa onu alacak iş zaten kuyrukta.
        //
        // Pano genel pano, Cmd-C'ninkiyle aynı (`view.rs` → `copy:`); işin
        // sırası `child_exit`'inkiyle aynı gerekçeden: ana kuyruk.
        if self.pending_copy.put(text) {
            let pending = Arc::clone(&self.pending_copy);
            DispatchQueue::main().exec_async(move || {
                pending.deliver(&NSPasteboard::generalPasteboard());
            });
        }
    }

    fn title_changed(&self) {
        // Okuyucu thread (ya da ayar kaydının thread'i), `Term` kilidi
        // tutuluyor olabilir: bayrağı kur, iş zaten bekliyorsa dön.
        if self.title_pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let pending = Arc::clone(&self.title_pending);
        let id = self.id;
        DispatchQueue::main().exec_async(move || {
            // Bayrak başlık **okunmadan önce** iniyor: okumadan sonra gelen
            // bir değişiklik yeni bir iş ister ve kaçmaz. `swap`, çünkü
            // okuma-değiştirme-yazma yazarın `swap`'ıyla eşleşiyor ve onun
            // yuvaya yazdığını görünür kılıyor.
            pending.swap(false, Ordering::AcqRel);
            // audit: ana kuyrukta koşan blok tanımı gereği ana thread'dedir.
            let mtm = MainThreadMarker::new().expect("ana kuyruk ana thread'dir");
            // Pencere bu arada kapanmışsa yazacak bir başlık da yok.
            if let Some(window) = app::delegate(mtm).and_then(|app| app.window(id)) {
                window.refresh_title();
            }
        });
    }

    fn search_changed(&self) {
        // Okuyucu thread, `Term` kilidi tutuluyor olabilir (ya da ana
        // thread'in `resize`'ı): `title_changed`'in örüntüsü — ana kuyruğa en
        // çok bir iş. Çekirdek zaten kenarda haber veriyor; bu bayrak iki
        // haber arasında iş kuyrukta beklerken ikincisini katlıyor.
        if self.search_pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let pending = Arc::clone(&self.search_pending);
        let id = self.id;
        DispatchQueue::main().exec_async(move || {
            pending.swap(false, Ordering::AcqRel);
            // audit: ana kuyrukta koşan blok tanımı gereği ana thread'dedir.
            let mtm = MainThreadMarker::new().expect("ana kuyruk ana thread'dir");
            // Arka sekmede de işliyor: haber kare yoluna bağlı değil. Pencere
            // bu arada kapandıysa sayacak bir şey yok.
            if let Some(window) = app::delegate(mtm).and_then(|app| app.window(id)) {
                window.kick_search();
            }
        });
    }
}

/// `bt-gpu`'nun alternatif ekran habercisi: işi **ana kuyruğa** atar.
///
/// Çağrısı kare yolundan, yani zaten ana thread'den geliyor — kuyruk bir
/// thread geçişi için değil, **bir tur ertelemek** için: çağrıldığı an kare
/// çizilmiş durumda ve pencere geometrisini (drawable ölçüsü, ızgara,
/// `DisplayLink` yerleşimi) orada değiştirmek çizilen karenin altını oymak
/// olurdu.
///
/// **Hedefsiz eylem değil, pencere kimliği.** Responder zinciri key pencereye
/// gidiyor: arka sekmede vim'den çıkış yanlış pencereyi boyutlandırırdı. İş
/// kimliği (`id`) yakalıyor, `AppDelegate`'in listesinden pencereyi buluyor ve
/// bulamazsa düşüyor — pencere o arada kapanmışsa boyutlandıracak bir şey de
/// yok.
///
/// Yakaladığı tek şey bir tamsayı. Eski "hiçbir şey yakalamıyor" kuralının
/// gerekçesi bir **referans çemberiydi** (`DisplayLink` pencerenin ivar'ında
/// duruyor, pencereyi tutan bir closure onu kendine bağlardı); bir tamsayı
/// çember açmıyor. Üstelik `exec_async` `Send` istiyor ve pencere nesnesi
/// ana thread'e çivili — tutabileceği başka bir şey de yoktu.
///
/// Yük taşımıyor: alıcı gerçeği yeniden okuyor ([`TerminalWindow::alt_screen_did_change`]),
/// yani birbirini kovalayan iki geçiş (vim aç-kapa) bayat bir değerle
/// davranamıyor.
fn alt_screen_notifier(id: u64) -> Box<dyn Fn()> {
    Box::new(move || {
        DispatchQueue::main().exec_async(move || {
            // audit: ana kuyrukta koşan blok tanımı gereği ana thread'dedir.
            let mtm = MainThreadMarker::new().expect("ana kuyruk ana thread'dir");
            let Some(app) = app::delegate(mtm) else {
                return;
            };
            if let Some(window) = app.window(id) {
                window.alt_screen_did_change(&app);
            }
        });
    })
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
    let read = || tabs.iter().map(|tab| tab.foreground()).collect::<Vec<_>>();
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

/// Pencerenin durumu. `OnceCell`: oturum ve link `start` içinde bir kez doğar,
/// sonra yalnız okunur. Pencere, view, yüzey ve renderer kurucuda doğuyor.
pub(crate) struct WindowIvars {
    /// Kendi sayacımız ([`AppDelegate`] dağıtıyor): alternatif ekran
    /// habercisinin pencereyi listeden bulduğu anahtar.
    id: u64,
    /// Süreli koşunun tarifi, `AppDelegate`'inkinin kopyası (`Copy`): odak
    /// yolu onu her uygulama geçişinde soruyor ve uygulama delegate'ine
    /// uzanmadan cevaplayabilmeli ([`TerminalWindow::apply_focus`]).
    run: Option<Run>,
    /// `Rc`: renderer ana thread'e çivili (bkz. `bt_gpu::DisplayLink`) ve
    /// link de bir kopya tutuyor.
    renderer: Rc<Renderer>,
    surface: Surface,
    window: Retained<NSWindow>,
    /// Fare çevirisinin girdileri pencere boyuyla tazeleniyor (`set_metrics`);
    /// `contentView` kapsayıcı (`NSView`), terminal onun çocuğu — o yüzden
    /// burada tutuluyor. Geometrinin kaynağı da bu view (`sync_geometry`).
    view: Retained<BateriView>,
    link: OnceCell<DisplayLink>,
    /// Kapanış sırasının ikinci adımı buradan çağrılır; `DisplayLink` de bir
    /// kopya tutuyor ama oraya `stop()`'tan sonra uzanmak yanlış olurdu.
    session: OnceCell<Arc<Session>>,
    /// Kabuk PTY'nin çocuğu mu, çocuğunun çocuğu mu — oturumla aynı anda,
    /// **komuttan** yazılıyor ([`TerminalWindow::start_session`]); koşan işin
    /// tespiti kabuğu bununla buluyor ([`TerminalWindow::foreground`]).
    shell_parent: OnceCell<ShellParent>,
    wake: Arc<ShellWake>,
    /// Cmd +/−/0'ın geçici punto farkı — **bu pencerenin**: renderer'a giden
    /// font `zoom.apply(&settings.font)` ([`TerminalWindow::apply_font`]).
    /// Dosyadaki `size` değişince sıfırlanır ([`TerminalWindow::zoom_after_reload`]).
    zoom: Cell<Zoom>,
    /// Dock kaç satır; `0` → bu pencerede dock yok.
    ///
    /// **Oturum doğarken kararlaşıyor** (012 → R5.1): kaynağı entegrasyonun
    /// kurulup kurulmadığı ve o [`TerminalWindow::start`]'ta **bir kez**
    /// soruluyor. Yuva o yüzden var: `sync_geometry` her pencere olayında
    /// koşuyor ve ızgara yüksekliğini hesaplarken cevabı bilmek zorunda;
    /// ikinci kez sormak, iki çağrının ayrışabildiği bir gelecekte "pencere
    /// iki satır kaybetti ama dock yok" demekti.
    ///
    /// Sonucu: `/bin/sh` koşan duman reçetesi dock **almıyor**, yani
    /// `smoke_shell` ve ona bağlı `hucre=8 glif=6 kural=15` sözleşmesi
    /// dokunulmadan kalıyor.
    ///
    /// `Cell`, `OnceCell` değil: açılış öncesi değeri `0` ve o **doğru** cevap
    /// (henüz oturum yok, ilk kare de yok); `OnceCell` bu yolu bir `unwrap`
    /// ile kapatırdı.
    ///
    /// **Bu alan o anki pay**, doğum değeri değil: alternatif ekranda sıfıra
    /// iniyor ve çıkışta geri geliyor (R5.2). Doğum değeri ayrı bir alanda
    /// ([`WindowIvars::dock_rows_at_birth`]) ve ikisinin ayrı durması şart —
    /// yoksa alternatif ekrandan çıkış, dock'u hiç olmayan bir pencerede dock
    /// doğururdu.
    dock_rows: Cell<u16>,
    /// Oturum doğarken kararlaşan dock payı: entegrasyon kurulduysa
    /// `DOCK_ROWS`, kurulmadıysa `0` (R5.1).
    ///
    /// Koşu boyunca **oynamıyor**; alternatif ekranın geri getireceği değer bu
    /// ve tek yazanı oturumun doğumu.
    dock_rows_at_birth: Cell<u16>,
    /// Kromun son boyandığı zemin ([`TerminalWindow::apply_chrome`]'un
    /// kapısı); `None`: henüz boyanmadı.
    chrome: Cell<Option<u32>>,
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
}

define_class!(
    // SAFETY: NSObject alt sınıflama şartı taşımaz; TerminalWindow Drop uygulamaz.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTerminalWindow"]
    #[ivars = WindowIvars]
    pub(crate) struct TerminalWindow;

    unsafe impl NSObjectProtocol for TerminalWindow {}

    impl TerminalWindow {
        /// İçerik view'ının çerçevesi değişti (`NSViewFrameDidChangeNotification`,
        /// gözlemci [`TerminalWindow::new`]'da kuruluyor).
        ///
        /// Kaynak `windowDidResize:` **değil**, çünkü içerik pencere boyutu
        /// değişmeden de değişiyor: ikinci sekme açılınca sekme çubuğu başlık
        /// alanına giriyor ve içerik kısalıyor, son sekme kalınca çubuk gidip
        /// içerik uzuyor — pencerenin çerçevesi ikisinde de aynı. Pencere
        /// bildirimine bağlı kalınca drawable eski boyda kalıyor, layer onu
        /// yeni boya **geriyordu** ve metin dikeyde bulanıklaşıyordu (ölçüldü,
        /// 026 phase-4 Uygulama Notları). View'ın bildirimi pencere
        /// boyutlandırmasını da kapsıyor, yani tek kaynak.
        #[unsafe(method(viewFrameDidChange:))]
        fn view_frame_did_change(&self, _n: &NSNotification) {
            if let Some(app) = app::delegate(self.mtm()) {
                self.refresh_geometry(&app);
            }
        }
    }

    unsafe impl NSWindowDelegate for TerminalWindow {

        // Ekranlar arası taşımada boyut (nokta) değişmez ama ölçek değişir;
        // layer-hosting view'da bunu bizden başka kimse yazmaz.
        #[unsafe(method(windowDidChangeBackingProperties:))]
        fn window_did_change_backing(&self, _n: &NSNotification) {
            if let Some(app) = app::delegate(self.mtm()) {
                self.refresh_geometry(&app);
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
            if let Some(link) = self.ivars().link.get() {
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
            self.apply_focus(true);
        }

        #[unsafe(method(windowDidResignKey:))]
        fn window_did_resign_key(&self, _n: &NSNotification) {
            self.apply_focus(false);
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
            drop(self.begin_close());
            // Delegate'i şimdi bırak: AppKit kapanmakta olan pencereye bundan
            // sonra bildirim göndermesin (odak, örtülme), nesne düşene kadar
            // bile.
            self.ivars().window.setDelegate(None);
            // Çerçeve gözlemcisi de: sekme çubuğu kapanırken AppKit bu
            // pencerenin içeriğini yeniden yerleştirebiliyor ve gözlemci
            // kalsaydı kapanmakta olan oturum bir resize (ve düşmüş okuyucuya
            // yazılamayan bir `Msg::Resize`) alırdı (`/code-review`).
            // SAFETY: gözlemci bu nesne, `new`'de kaydedildi; kayıt yoksa no-op.
            unsafe { NSNotificationCenter::defaultCenter().removeObserver(self) };
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
            self.change_zoom(Zoom::bigger);
        }

        /// View ▸ Smaller (Cmd −).
        #[unsafe(method(makeFontSmaller:))]
        fn make_font_smaller(&self, _sender: Option<&AnyObject>) {
            self.change_zoom(Zoom::smaller);
        }

        /// View ▸ Actual Size (Cmd 0): fark sıfırlanır, ayarın puntosu.
        #[unsafe(method(resetFontSize:))]
        fn reset_font_size(&self, _sender: Option<&AnyObject>) {
            self.change_zoom(|_, _| Zoom::default());
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
            if let Some(session) = self.ivars().session.get() {
                session.clear_to_start();
            }
        }

        /// Edit ▸ Clear Scrollback (⌥⌘K): yalnız geçmiş —
        /// `Session::clear_scrollback`.
        #[unsafe(method(clearScrollback:))]
        fn clear_scrollback(&self, _sender: Option<&AnyObject>) {
            if let Some(session) = self.ivars().session.get() {
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
                self.ivars()
                    .session
                    .get()
                    .is_some_and(|session| !session.alt_screen())
            } else if action == Some(sel!(findNextMatch:)) || action == Some(sel!(findPreviousMatch:)) {
                self.has_query()
            } else if action == Some(sel!(useSelectionForFind:)) {
                self.ivars()
                    .session
                    .get()
                    .is_some_and(|session| session.has_selection())
            } else {
                true
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
    /// Pencereyi, view'ı, yüzeyi ve renderer'ı kurar; oturum ve link **henüz
    /// yok** ([`TerminalWindow::start`]).
    ///
    /// İki adım olmasının sebebi aradaki iş: ayarlar pencere doğduktan
    /// **sonra** (tanı alt başlığa yazılabilsin) ve geometriden **önce**
    /// okunmak zorunda — font ayarı hücre ölçüsünü, yani ilk grid'i ve kabuğun
    /// gördüğü ilk `TIOCSWINSZ`'yi belirliyor. Tek kurucu o sırayı ya bozar ya
    /// da ayar okumayı pencerenin içine taşırdı.
    ///
    /// Renderer burada doğuyor ve hatası çağırana dönüyor: Metal device ya da
    /// metallib yoksa pencerenin çizebileceği bir şey de yok.
    pub(crate) fn new(
        mtm: MainThreadMarker,
        id: u64,
        run: Option<Run>,
    ) -> Result<Retained<Self>, GpuError> {
        let renderer = Rc::new(Renderer::system_default()?);
        let surface = renderer.surface();
        let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(900.0, 600.0));
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
        let view = BateriView::new(mtm, rect);
        // Sıra önemli: önce layer, sonra wantsLayer — tersi AppKit'e kendi
        // layer'ını kurdurur ve CAMetalLayer düşer.
        view.setLayer(Some(surface.ca_layer()));
        view.setWantsLayer(true);
        // İçerik view'ı düz bir **kapsayıcı**, `BateriView` onun çocuğu
        // (033 → R4.1): arama paneli terminalin üstünde yüzecek ve Metal
        // katmanının kardeşi olmak zorunda, çocuğu değil — layer-hosting
        // view'ın alt view'ları AppKit'in sözleşmesi dışında. Kapsayıcı
        // layer-backed, yoksa kardeş panel Metal katmanının **altında**
        // kalabilir. Kendisi hiçbir şey çizmiyor ve olay almıyor: `BateriView`
        // onu tamamen dolduruyor, isabet testi en üstteki çocuğa düşüyor.
        let container = NSView::initWithFrame(NSView::alloc(mtm), rect);
        container.setWantsLayer(true);
        window.setContentView(Some(&container));
        // Kapsayıcının çerçevesini pencere kuruyor; çocuk ona sonradan
        // oturtuluyor ve boyu autoresizing'le izliyor. Geometrinin kaynağı
        // yine `BateriView` (`sync_geometry`), bildirimi de onun çerçevesi.
        view.setFrame(container.bounds());
        view.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        container.addSubview(&view);
        window.setTitle(ns_string!("bateri"));
        // **Native sekmeler** (026 → Karar 1): aynı kimliği taşıyan pencereleri
        // AppKit tek pencerede sekme olarak topluyor. `tabbingMode` bilerek
        // varsayılanda — sistemin "Prefer tabs" ayarına saygı. Kimliğin tek
        // yazıldığı yer burası, yani bütün pencereler ortak kimlikte.
        window.setTabbingIdentifier(ns_string!("bateri.terminal"));
        // Düğmesiz hareket olayları varsayılan **kapalı**; fare raporu
        // isteyen uygulama (1003) onlarsız işaretçiyi hiç göremez.
        // `NSTrackingArea` gerekmiyor: o yalnız `mouseEntered:`/
        // `mouseExited:` ve cursor rect için, ikisi de istenmiyor, ve
        // view zaten first responder — pencere seviyesindeki
        // `mouseMoved:` ona geliyor. Kipe göre açıp kapamak kipi
        // `bt-shell`'e yayınlamayı isterdi
        // (`.tasks/020-fare-raporlama/discussion.md` → Karar 4).
        window.setAcceptsMouseMovedEvents(true);
        // Klavyenin PTY'ye varan yolu buradan başlıyor. View (kapsayıcının
        // çocuğu da olsa) otomatik first responder DEĞİLDİR; bu satır olmadan pencere
        // key olur, tuşlar view'a hiç uğramaz ve terminal sessizce
        // yazmaz. `acceptsFirstResponder` da şart, ikisi bir arada.
        let accepted = window.makeFirstResponder(Some(&view));
        debug_assert!(accepted, "BateriView first responder olmalı");
        let this = Self::alloc(mtm).set_ivars(WindowIvars {
            id,
            run,
            renderer,
            surface,
            window: window.clone(),
            view,
            link: OnceCell::new(),
            session: OnceCell::new(),
            shell_parent: OnceCell::new(),
            wake: Arc::new(ShellWake {
                id,
                timed: run.is_some(),
                waker: Mutex::new(None),
                pending_copy: Arc::default(),
                title_pending: Arc::default(),
                search_pending: Arc::default(),
            }),
            zoom: Cell::new(Zoom::default()),
            // Açılışta dock yok: kararı `start` veriyor ve geometriyi ondan
            // sonra hesaplıyor.
            dock_rows: Cell::new(0),
            dock_rows_at_birth: Cell::new(0),
            chrome: Cell::new(None),
            alert: RefCell::new(None),
            close_requested: Cell::new(false),
            search: OnceCell::new(),
            search_status: Cell::new(SearchStatus::Empty),
            search_driving: Cell::new(false),
        });
        // SAFETY: NSObject'in init'i argümansızdır ve ivar'lar set edildi.
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        // Delegate bağlanmadan önce ivar'lar dolu: arada düşen bir pencere
        // bildirimi geometriyi boş bulup bayat boyutla çizmesin. Delegate
        // özelliği zayıf; sahibi `AppDelegate`'in pencere listesi.
        window.setDelegate(Some(ProtocolObject::from_ref(&*this)));
        // İçeriğin boyu pencereden bağımsız da değişiyor (sekme çubuğu);
        // geometri bu yüzden view'ın kendi bildiriminden
        // (`viewFrameDidChange:`). `postsFrameChangedNotifications`
        // varsayılanda açık. Gözlemci sökülmüyor: seçicili gözlemcileri
        // merkez macOS 10.11'den beri zayıf tutuyor, düşen pencere sarkan bir
        // kayıt bırakmıyor.
        // SAFETY: seçici bu sınıfta tanımlı ve tek `&NSNotification` alıyor;
        // ad AppKit'in dışa açtığı sabit, nesne bu pencerenin view'ı.
        unsafe {
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                &this,
                sel!(viewFrameDidChange:),
                Some(NSViewFrameDidChangeNotification),
                Some(&this.ivars().view),
            );
        }
        Ok(this)
    }

    pub(crate) fn id(&self) -> u64 {
        self.ivars().id
    }

    /// Bu nesnenin `NSWindow`'u mu — etkin pencere `NSApp.keyWindow`'dan
    /// listede böyle aranıyor (`AppDelegate::key_window`).
    pub(crate) fn owns(&self, window: &NSWindow) -> bool {
        std::ptr::eq(&*self.ivars().window, window)
    }

    /// Bu pencerenin geçici punto farkı — yeni sekme onu devralıyor
    /// (026 → Karar 3).
    pub(crate) fn zoom(&self) -> Zoom {
        self.ivars().zoom.get()
    }

    /// Devralınan punto farkı; oturum doğmadan, [`TerminalWindow::request_font`]'tan
    /// önce yazılır ki ilk atlas büyütülmüş puntoyla açılsın.
    pub(crate) fn set_zoom(&self, zoom: Zoom) {
        self.ivars().zoom.set(zoom);
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

    /// Rapor yolu sayaçlarını buradan okuyor (`AppDelegate::report_and_exit`).
    pub(crate) fn renderer(&self) -> &Renderer {
        &self.ivars().renderer
    }

    pub(crate) fn link(&self) -> Option<&DisplayLink> {
        self.ivars().link.get()
    }

    /// View ▸'nin dört kaydırması: `Session::scroll_page`'in yolu
    /// (Shift+PgUp/PgDn'ın ta kendisi) — kesir sıfırlanıyor, süzülme nesli
    /// artıyor, bant kuralı `scroll_locked`'ta. Alternatif ekranda `None` ve
    /// öğeler zaten gri; cevap burada okunmuyor.
    fn scroll_pages(&self, pages: i32) {
        if let Some(session) = self.ivars().session.get() {
            session.scroll_page(pages);
        }
    }

    pub(crate) fn session(&self) -> Option<&Arc<Session>> {
        self.ivars().session.get()
    }

    /// Kabuğun dışında ön planda koşan iş (028 → Karar 1). Oturum yoksa ya da
    /// okuyucu thread bittiyse boşta: kabuk gitti ve `child_pid` bayatlamış
    /// olabilir, bayat pid'e sorulmaz.
    pub(crate) fn foreground(&self) -> Foreground {
        let (Some(session), Some(&parent)) =
            (self.ivars().session.get(), self.ivars().shell_parent.get())
        else {
            return Foreground::Idle;
        };
        if !session.reader_alive() {
            return Foreground::Idle;
        }
        jobs::foreground(parent, session.child_pid(), &Libproc)
    }

    /// Başlığı oturumdan okuyup pencereye yazar — `ShellWake::title_changed`'in
    /// ana kuyruk işi. Kare yolu başlık hesaplamıyor; yazım yalnız
    /// **değişimde** (026 R2.4). Oturum henüz yoksa başlık kurucunun
    /// `bateri`'si kalıyor.
    pub(crate) fn refresh_title(&self) {
        if let Some(session) = self.ivars().session.get() {
            self.ivars()
                .window
                .setTitle(&NSString::from_str(&session.title()));
        }
    }

    /// Alt başlığın yazımı; metni kuran `AppDelegate::post_notices`.
    pub(crate) fn set_subtitle(&self, subtitle: &NSString) {
        self.ivars().window.setSubtitle(subtitle);
    }

    /// Renderer'a ayarın fontunu bu pencerenin punto farkıyla **istek**
    /// olarak verir — açılışın yolu (`AppDelegate::load_settings`): atlas
    /// hemen ardından gelen `sync_geometry`'de açılıyor ve font yuvasını da o
    /// yazıyor. Dönüş (değişti mi) burada soru değil: geometri henüz hiç
    /// kurulmadı.
    pub(crate) fn request_font(&self, font: &FontOptions) {
        let _ = self
            .ivars()
            .renderer
            .set_font(&self.ivars().zoom.get().apply(font));
    }

    /// Dosyadaki font değişti: punto farkı [`Zoom::after_reload`]'ın
    /// kuralıyla güncellenir. Fontu uygulamaz; ayarlar yazıldıktan sonra
    /// [`TerminalWindow::apply_font`] uygular.
    pub(crate) fn zoom_after_reload(&self, old: &FontOptions, new: &FontOptions) {
        let zoom = self.ivars().zoom.get().after_reload(old, new);
        self.ivars().zoom.set(zoom);
    }

    /// Entegrasyonu sorar, dock payını kararlaştırır, ilk geometriyi kurar ve
    /// oturumu açar.
    ///
    /// Entegrasyon **bir kez** soruluyor ve iki cevabı birden veriyor:
    /// çocuğun ortamı ile dock'un varlığı (`AppDelegate::shell_integration`).
    /// İki ayrı çağrı olsaydı ikisi ayrışabilirdi — pencereden iki satır
    /// giden ama dock'u olmayan (ya da tersi) bir oturum, ve belirti sessiz
    /// olurdu. Geometriden **önce**: ızgara yüksekliği dock payını görmeli,
    /// yoksa kabuk açılışta bir satır fazlasıyla doğar ve ilk kare düzeltme
    /// için bir `TIOCSWINSZ` yer.
    ///
    /// `working_directory` çağıranın kararı (026 → Karar 4: etkin sekmenin
    /// dizini, yoksa ev). Hata çağırana dönüyor: ilk pencerede süreç çıkıyor,
    /// ⌘T/⌘N'de yalnız o pencere kapanıyor — öteki sekmelerin kabukları bir
    /// yenisinin doğamamasıyla ölmemeli.
    pub(crate) fn start(
        &self,
        app: &AppDelegate,
        mtm: MainThreadMarker,
        theme: Theme,
        working_directory: Option<PathBuf>,
    ) -> std::io::Result<()> {
        let (integration, birth) = app.shell_integration();
        self.ivars().dock_rows_at_birth.set(birth);
        self.ivars().dock_rows.set(birth);
        // Grid ölçüsü pencereden türer; oturum ilk boyutuyla doğsun ki
        // shell açılışta doğru `TIOCSWINSZ` görsün.
        let grid = self.sync_geometry(app);
        self.start_session(app, mtm, grid, theme, integration, working_directory)
    }

    /// Oturumu açar ve kareyi süren link'i bağlar. Sıra zorunlu: `Session`
    /// `Wake`'i ister, link `Session`'ı ister, `Waker` link'ten doğar.
    fn start_session(
        &self,
        app: &AppDelegate,
        mtm: MainThreadMarker,
        grid: Grid,
        theme: Theme,
        integration: Vec<(String, String)>,
        working_directory: Option<PathBuf>,
    ) -> std::io::Result<()> {
        // Duman ve ölçüm koşularında shell sabit: sonuç kullanıcının
        // `$SHELL`'ine ve rc dosyasına bağlı olmasın. Betiklerin
        // sahibi `bt-core`; `smoke_shell`'in sekiz hücre ve altı
        // glyph verdiği orada sınanıyor — `hucre=8` ve `glif=6`
        // beklentileri bu yüzden birer belge cümlesi değil, sınanmış
        // birer iddia.
        //
        // Dallanma **yükü** soruyor, süreyi değil: aynı `Run` hem
        // deadline'ı hem bekçiyi kuruyor ve yük onlardan bağımsız.
        //
        // Süresiz oturumda komut artık **`None` değil**: kabuğu
        // alacritty'nin yolundan birebir ama `-q` ile doğuruyoruz,
        // yani `login(1)`'in `Last login:` banner'ı ızgaraya hiç
        // düşmüyor ([`child::login_command`]). Kullanıcı ya da kabuk
        // çözülemezse `None`'a düşüyor ve eski yol geri geliyor.
        //
        // Kabuğun yeri komutla **aynı** dalda kararlaşıyor, ayrı bir
        // sorudan türetilmiyor: `login` yolu (`login_command` ya da `None`,
        // alacritty'nin macOS yolu da `login`) ile süreli koşunun doğrudan
        // betikleri ancak böyle ayrışamaz.
        let (command, shell_parent) = match self.ivars().run {
            None => (child::login_command(), ShellParent::Login),
            Some(run) => (
                Some(match run.workload {
                    Workload::Smoke => smoke_shell(),
                    // Yükün süresi deadline'la aynı: kısa kalırsa pencere
                    // koşunun kuyruğunda boşa düşer ve ölçüm boşta kare
                    // örnekler. Süresiz yük artık **temsil edilemiyor** —
                    // `Run` süreyi yükün yanında taşıyor, o yüzden eski
                    // `unwrap_or(0)` ve onu savunan `debug_assert` düştü.
                    Workload::Load => load_shell(run.seconds),
                }),
                ShellParent::Direct,
            ),
        };
        let session = Session::spawn(
            SessionOptions {
                command,
                // Dizin ve yerel **her** oturumda aynı kuralla, süreli koşu
                // dahil: karar tek kollu (`discussion.md` → Karar 6 eki,
                // "istisnasız") ve iki sabit betik de dizine ve yerele bağlı
                // değil — `printf` ile `sleep`, `date` ile `printf`; yolları
                // mutlak ya da `PATH`'ten, çıktıları ASCII.
                //
                // Dizin artık çağırandan: yeni sekme etkin sekmenin OSC 7
                // dizininde (026 → Karar 4); süreli koşuda ve ilk pencerede
                // `child::working_directory()`.
                working_directory,
                // Başlığın `~` kuralı; dizinle **aynı çözüm** (`child::home`).
                home: child::home(),
                // Shell entegrasyonu yerelin yanında, aynı haritada: ikisi de
                // çocuğa **eklenen** ortam ve ikisi de yalnız çocuğa gidiyor.
                // Anahtarları ayrık (`LANG` ↔ `ZDOTDIR`), yani sıranın
                // önemi yok.
                // Entegrasyonun ortamı **çağırandan** geliyor: aynı cevap
                // dock'un varlığını da belirliyor (`start`) ve burada ikinci
                // kez sorulsaydı iki karar ayrışabilirdi.
                env: child::locale_env().into_iter().chain(integration).collect(),
                cols: grid.cols,
                rows: grid.rows,
                cell_px: grid.cell.cell_px(),
                terminal: app.settings().terminal(),
                theme,
                // Dock'un **varlığı**, payı değil: `bt-core` caret'i ona göre
                // devrediyor. Kaynağı doğum payının yuvası (`start` onu bir
                // satır önce yazdı) ve alternatif ekran habercisinin kapısı da
                // aynı yuvayı okuyor, yani ayrışamazlar.
                dock: self.ivars().dock_rows_at_birth.get() > 0,
                // Kümeleme (035) bütün pencerelerde açık, süreli koşu dahil.
                // Ayar anahtarı değil (035 Karar 2): geri alma bu tek satır.
                cluster: true,
            },
            Arc::clone(&self.ivars().wake) as Arc<dyn Wake>,
        );
        // Shell'siz bir terminal penceresi boş bir kutudur; ne yapılacağı
        // çağıranın (ilk pencere: süreç çıkar; sonrakiler: o pencere kapanır).
        let session = Arc::new(session?);
        // Kapanış sırası oturuma link üzerinden değil buradan uzanır, klavye
        // de kendi kopyasını tutar; üçü de ana thread'de yaşıyor, yani son
        // referansın nerede düşeceği belli (bkz. `shutdown`).
        let _ = self.ivars().session.set(Arc::clone(&session));
        let _ = self.ivars().shell_parent.set(shell_parent);
        // Oturum yuvaya girmeden önce gelmiş bir başlık haberi `refresh_title`'da
        // boş yuva bulup düşmüş olabilir; bir kez elle okumak o pencereyi
        // kapatıyor (değişmemişse aynı `bateri`'yi yazar).
        self.refresh_title();
        let view = &self.ivars().view;
        view.attach(Arc::clone(&session));
        // Fare çevirisi oturumla aynı grid'i görmeli: ölçü ve sayı yukarıdaki
        // `SessionOptions`'a gidenlerin aynısı. `resize` yolunda da aynı üçlü
        // (`refresh_geometry`) birlikte yazılıyor.
        view.set_metrics(grid, self.ivars().dock_rows.get());
        let link = DisplayLink::new(
            mtm,
            &self.ivars().surface,
            Rc::clone(&self.ivars().renderer),
            session,
            Layout {
                cols: grid.cols,
                dock_rows: self.ivars().dock_rows.get(),
                cell: grid.cell,
            },
            app.stats(),
            // **Yol yalnız dock'u olan pencerede kuruluyor** ve bu yapısal
            // (R5.1): dock'suz bir oturumda alternatif ekran geçişi hiçbir
            // şeyi değiştiremeyeceği için nöbet de yok. Bir koşulla
            // kapatılsaydı "hiç resize yok" iddiası bir dalın doğruluğuna
            // bağlı kalırdı.
            (self.ivars().dock_rows_at_birth.get() > 0)
                .then(|| alt_screen_notifier(self.ivars().id)),
        );
        // Uyandırma yolu kapanmadan kare istemiyoruz: aradaki bir `Wakeup`
        // sessizce düşerdi.
        //
        // audit: `start_session` pencere başına bir kez, `start`'tan çağrılır.
        // Sessizce yutulan bir `Err` burada en sinsi hatayı üretirdi: eski
        // link'in `Waker`'ı kalır, pencere shell çıktısına bir daha hiç
        // uyanmaz ve tek satır iz kalmaz.
        assert!(
            self.ivars().wake.slot().replace(link.waker()).is_none(),
            "waker ikinci kez kuruldu"
        );
        // Fare eşlemesinin dikey orijini: `set_metrics` gibi pencere değil
        // **kare** yolundan geliyor, o yüzden link doğduktan sonra ve bir kez.
        // Fare böylece çizilen ötelemeyi okuyor; ikinci bir hesap "tıklama bir
        // satır kayıyor" demekti (`bt_gpu::Origin`).
        view.attach_origin(link.origin());
        // İmlecin stili de ayarın: link `CursorMotion::default()` ile doğuyor
        // ve buradaki çağrı onu dosyanın (ya da hermetik koşuda
        // `Settings::default()`'un) değerine çekiyor. `set_font`'un yeri
        // `load_settings` ama stilinki olamaz: link o an henüz yok.
        // Dock'un yazım efektleri de aynı gerekçeyle burada.
        {
            let settings = app.settings();
            link.set_cursor_motion(settings.cursor_motion);
            link.set_glyph_fx(settings.keypress, settings.erase);
        }
        // Açılış karesi: `Session` kirli doğar, link'i bir kez elle açıyoruz.
        link.request_frame();
        let _ = self.ivars().link.set(link);
        // Hareketi Azalt link yuvaya girdikten **sonra**: ilk değer bu
        // pencerenin link'ine buradan iniyor, sistemin bildirimi ve ayar
        // kaydı sonradan `AppDelegate::apply_reduce_motion` ile bütün
        // pencerelere. Hermetik koşuda çözülmüş değer `false` ve link o
        // değerle doğuyor, yani çağrı no-op (`DisplayLink::set_reduce_motion`).
        self.set_reduce_motion(app.reduce_motion());
        // Tekerleğin kipi de aynı çözülmüş girdiden (Hareketi Azalt ayarın
        // üçüncü girdisi) ve aynı sonraki yoldan (`apply_reduce_motion`).
        self.set_smooth_scroll(app.smooth_scroll());
        // İmlecin çizim sayıları da açılışta bir kez iniyor ve **yuvadan**
        // okunuyor, elde kalan `link`'ten değil: link o çağrıda yuvaya
        // taşındı. `set_caret_style` aynı değerde no-op, yani kayıt anı
        // yoluyla çakışmıyor.
        self.apply_caret(&app.settings());
        // **Odak da tohumlanıyor** ve gerekçesi aynı sıralama: pencere
        // `makeKeyAndOrderFront` ile key oluyor, yani `windowDidBecomeKey:`
        // link yuvaya girmeden **önce** düşüyor ve o çağrı sessizce atılıyor.
        // Tohumlama olmasaydı arka planda açılan bir pencerede (`open -g`,
        // login item, başka uygulama öndeyken betikten açılış) hiçbir bildirim
        // gelmez ve `focused` `true` kalırdı: odaksız pencere dolu caret
        // çizer ve blink saatini kurardı (`/code-review`).
        self.apply_focus(self.ivars().window.isKeyWindow());
        Ok(())
    }

    /// Alternatif ekran değişti: dock kalkıyor ya da iniyor.
    ///
    /// Gönderen kare yolunun habercisi ([`alt_screen_notifier`]) ve bu metot
    /// **bir sonraki ana kuyruk turunda** koşuyor — çizilmiş bir karenin
    /// altını oymamak için.
    ///
    /// **Gerçeği yeniden okuyor**, bildirimin taşıdığına bakmıyor: iki
    /// geçiş birbirini kovalarsa (vim aç-kapa) kuyrukta bekleyen iki iş de
    /// aynı, güncel cevabı görür. Değişmemişse **hiçbir şey yapmıyor** —
    /// "geçiş başına bir resize" (R5.3) iddiasını tutan kapı bu.
    fn alt_screen_did_change(&self, app: &AppDelegate) {
        let Some(session) = self.ivars().session.get() else {
            return;
        };
        let wanted =
            app::dock_rows_for(session.alt_screen(), self.ivars().dock_rows_at_birth.get());
        if self.ivars().dock_rows.replace(wanted) == wanted {
            return;
        }
        // Pay, ızgara ve link **tek blokta**: kare yolu da ana thread'de,
        // yani araya bir kare giremiyor ve yarım bir durum çizilmiyor.
        self.refresh_geometry(app);
    }

    /// Bigger, Smaller, Actual Size: bu pencerenin geçici punto farkını
    /// `step` ile değiştirir ve fontu uygular. Dosyaya dokunmaz, süreli koşuda
    /// da çalışır — kullanıcının dünyasından bir şey okumuyor.
    fn change_zoom(&self, step: impl FnOnce(Zoom, &FontOptions) -> Zoom) {
        let Some(app) = app::delegate(self.mtm()) else {
            return;
        };
        let zoom = step(self.ivars().zoom.get(), &app.settings().font);
        self.ivars().zoom.set(zoom);
        self.apply_font(&app);
    }

    /// Renderer'a ayarın fontunu bu pencerenin geçici punto farkıyla verir;
    /// istek değiştiyse geometri yeniden kurulur
    /// ([`TerminalWindow::refresh_geometry`]: atlas, grid, PTY boyutu, font
    /// yuvası).
    ///
    /// İki kapı, ikisi de gerekli: çağıranın kapısı (fark, basış) bir şeyin
    /// değiştiğini söylüyor, `set_font` renderer'ın zaten o fontu isteyip
    /// istemediğini — uçtaki basış ya da farkla aynı puntoyu yazan kayıt
    /// atlası yeniden kurdurmaz.
    pub(crate) fn apply_font(&self, app: &AppDelegate) {
        let font = self.ivars().zoom.get().apply(&app.settings().font);
        if self.ivars().renderer.set_font(&font) {
            self.refresh_geometry(app);
        }
    }

    /// Terminal seçenekleri değişti — oturuma, **tamamıyla**.
    pub(crate) fn set_terminal_options(&self, settings: &Settings) {
        if let Some(session) = self.ivars().session.get() {
            session.set_terminal_options(settings.terminal());
        }
    }

    /// Temayı oturuma takas eder (aynı temada no-op, `Session::set_theme`)
    /// ve kromu ona boyar ([`TerminalWindow::apply_chrome`]).
    ///
    /// İkisi tek çağrıda, çünkü temayı değiştiren iki yol var
    /// (`AppDelegate::reload_settings`, `AppDelegate::apply_appearance`) ve
    /// biri kromu unutsaydı ızgara yeni temada, başlık çubuğu eskisinde
    /// kalırdı — belirti tam da kullanıcının göreceği dikiş.
    pub(crate) fn set_theme(&self, theme: Theme) {
        if let Some(session) = self.ivars().session.get() {
            session.set_theme(theme);
        }
        self.apply_chrome(&theme);
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

    /// İmlecin stili ve dock'un yazım efektleri link'e gidiyor, oturuma
    /// değil: hangi kareyi çizeceğimizi değil **nasıl** çizeceğimizi
    /// değiştiriyorlar. Efektler **ham** iniyor; `snap` ve Hareketi Azalt'ın
    /// indirgemesi `bt-gpu`'da (`DisplayLink::set_glyph_fx`).
    pub(crate) fn set_cursor_motion(&self, settings: &Settings) {
        if let Some(link) = self.ivars().link.get() {
            link.set_cursor_motion(settings.cursor_motion);
            link.set_glyph_fx(settings.keypress, settings.erase);
        }
    }

    /// İmlecin ayardan inen değerlerini link'e verir.
    ///
    /// **Açılış ile kayıt anı aynı koddan geçiyor** ve gerekçesi bir kusur
    /// sınıfı (`/code-review`, 016): iki liste ayrı yazılsaydı sapabilirlerdi
    /// — yalnız tohumlanan bir anahtar kayıt anında uygulanmaz, yalnız
    /// yeniden yüklenen bir anahtar açılışta varsayılanda kalırdı. İkisi de
    /// sessiz ve `plan.md` o sınıfı adıyla sayıyor ("yarısı inen anahtar
    /// hiçbir kapıda görünmez").
    ///
    /// Tek `Changes::caret` alanı, iki çağrı: varış yerleri ayrı (çizim
    /// sayıları `Frame`'e, periyot `bt_gpu::blink`'e) ama ikisi de aynı
    /// kaydın sonucu — emsal `Changes::motion`'ın iki anahtarı.
    ///
    /// Link yoksa sessizce dönüyor: açılış çağrısı aynı değeri zaten verecek.
    pub(crate) fn apply_caret(&self, settings: &Settings) {
        let Some(link) = self.ivars().link.get() else {
            return;
        };
        link.set_caret_style(settings.caret);
        link.set_blink_interval(settings.blink_interval);
    }

    /// Hareketi Azalt'ın **çözülmüş** değerini link'e verir
    /// (`AppDelegate::reduce_motion`). Değer değişmediyse no-op
    /// (`bt_gpu::DisplayLink::set_reduce_motion`); link yoksa sessizce döner.
    pub(crate) fn set_reduce_motion(&self, reduce: bool) {
        if let Some(link) = self.ivars().link.get() {
            link.set_reduce_motion(reduce);
        }
    }

    /// Kaydırmanın **çözülmüş** kipini view'a verir
    /// (`AppDelegate::smooth_scroll`). Link'e değil view'a: karar olayın
    /// sınıflamasında, `scrollWheel:`'de veriliyor ve `false` kolu bugünkü
    /// satır yolunun ta kendisi (027 Karar 5).
    pub(crate) fn set_smooth_scroll(&self, smooth: bool) {
        self.ivars().view.set_smooth_scroll(smooth);
    }

    /// Klavye terminale geldi (`here`) ya da arama alanına gitti —
    /// `BateriView`'ın first responder kancaları veriyor (033 R7). Odağın
    /// ikinci biti; iki bitin birleşimi `bt-gpu`'da
    /// (`DisplayLink::set_keyboard_in_terminal`). Süreli koşuda
    /// [`TerminalWindow::apply_focus`]'un kapısıyla susuyor.
    pub(crate) fn keyboard_moved(&self, here: bool) {
        if self.ivars().run.is_some() {
            return;
        }
        if let Some(link) = self.ivars().link.get() {
            link.set_keyboard_in_terminal(here);
        }
    }

    /// Arama paneli — ilk çağrıda kurulur, temaya boyanır.
    fn search_bar(&self) -> &SearchBar {
        self.ivars().search.get_or_init(|| {
            let view = &self.ivars().view;
            // İçerik view'ı kapsayıcı (phase-3) ve kurucudan beri hep var;
            // yokluğu ancak kapanan pencerede, o zaman panel view'ın içine
            // düşer ve yine görünür.
            let container: Retained<NSView> = match self.ivars().window.contentView() {
                Some(container) => container,
                None => Retained::into_super(view.clone()),
            };
            let bar = SearchBar::new(
                self.mtm(),
                &container,
                view,
                self,
                ProtocolObject::from_ref(self),
            );
            if let Some(session) = self.ivars().session.get() {
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
        let (Some(bar), Some(session)) = (self.ivars().search.get(), self.ivars().session.get())
        else {
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
        let (Some(bar), Some(session)) = (self.ivars().search.get(), self.ivars().session.get())
        else {
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
        let (Some(bar), Some(session)) = (self.ivars().search.get(), self.ivars().session.get())
        else {
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
        if let Some(session) = self.ivars().session.get() {
            session.select_search_match();
            session.clear_search();
        }
        self.ivars().search_status.set(SearchStatus::Empty);
        self.ivars()
            .window
            .makeFirstResponder(Some(&self.ivars().view));
    }

    /// ⌘E (Karar 6): seçimin ilk satırı sorgu olur (regex kipinde
    /// kaçırılarak), find panosuna yazılır ve panel alanı odaklanmış açılır.
    fn use_selection(&self) {
        let Some(text) = self
            .ivars()
            .session
            .get()
            .and_then(|session| session.selection_text())
        else {
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
        let view = &self.ivars().view;
        view.search_cover(view.convertRect_fromView(bar.resting_frame(), Some(bar.parent())))
    }

    /// Kaydırmanın çözülmüş kipi: süzülme mi anında mı (`smooth_scroll`,
    /// Hareketi Azalt, `snap` — `app::resolve_smooth_scroll`).
    fn smooth_scroll(&self) -> bool {
        app::delegate(self.mtm()).is_some_and(|app| app.smooth_scroll())
    }

    /// Odak değişti — `bt-gpu`'ya iletir.
    ///
    /// **Hermetik koşuda hiç çağrılmıyor** (R7.1) ve kapı burada, `bt-gpu`'nun
    /// varsayılanında değil: `DisplayLink`'in `focused`'ı zaten `true`
    /// doğuyor ama o tek başına yetmez — `make duman` koşarken açılan bir
    /// Spotlight `windowDidResignKey:` doğurur, o da kare ister ve kapı bir
    /// makinede yeşil bir makinede kırmızı düşerdi. Emsal
    /// `app::resolve_reduce_motion`'ın `Inputs`'a bakması.
    ///
    /// Link yoksa sessizce dönüyor: key olayı `start_session`'dan önce de
    /// düşebilir ve o hâlde varsayılan (`true`) zaten doğru.
    fn apply_focus(&self, focused: bool) {
        // Kapı `run` **bayrağına** bakıyor, `inputs()`'a değil: `inputs()`
        // `child::home()`'u argüman olarak çözüyor (passwd kaydına kadar
        // gidebilir) ve odak her uygulama geçişinde değişiyor. Bayrağın
        // kopyası bu yüzden pencerenin kendisinde: uygulama delegate'ine
        // uzanmak da gerekmiyor.
        if self.ivars().run.is_some() {
            return;
        }
        if let Some(link) = self.ivars().link.get() {
            link.set_focused(focused);
        }
    }

    /// Kapanış sırasının pencereye düşen adımları — **başlatır, beklemez**.
    /// İki çağıranı var: pencerenin kapanışı (`windowWillClose:`, tutamak
    /// düşüyor) ve uygulamanın kapanışı (`AppDelegate::shutdown`, bütün
    /// tutamaklar tek son tarihe kadar bekleniyor).
    ///
    /// 1. Ritmi kes (`DisplayLink::stop`): link durur, run loop'tan çıkar ve
    ///    uyandırma kapısı kapanır. Bundan sonra yeni kare istenmez.
    /// 2. `Waker`'ı `ShellWake`'ten **sök** ve burada, ana thread'de düşür
    ///    ([`ShellWake::detach`]): `ShellWake`'in son kopyası okuyucu ya da
    ///    `"PTY teardown"` thread'inde düşebilir ve orada `Waker` taşımamalı.
    /// 3. Oturumun kapanışını başlat (`Session::begin_shutdown`: `SIGHUP` +
    ///    okuyucu thread'in bitişi arkada).
    ///
    /// `DisplayLink` artık **düşürülebilir** ve pencere nesnesiyle ana
    /// thread'de düşüyor: son `Waker` kopyası sökmeden sonra ya bu nesnede ya
    /// da Metal'in tamamlanma bloğunda; ikincisi ana kuyruğa senkron iş atar
    /// ama ana thread o sırada beklemede değil — pencere kapanışı beklemiyor,
    /// ⌘Q ise pencereleri bekleme bitene kadar listede tutuyor.
    ///
    /// İdempotent: ikinci çağrı [`Closing::AlreadyDone`] döner (`stop`
    /// mandallı, `detach` `take`, `begin_shutdown` `Option`). Oturum hiç
    /// doğmadıysa `None` — kapanacak bir şey yok.
    pub(crate) fn begin_close(&self) -> Option<Closing> {
        if let Some(link) = self.ivars().link.get() {
            link.stop();
        }
        drop(self.ivars().wake.detach());
        let session = self.ivars().session.get()?;
        Some(match session.begin_shutdown() {
            Some(handle) => Closing::Started(handle),
            None => Closing::AlreadyDone,
        })
    }

    /// Pencere geometrisi ya da font oynadı: layer'ı eşle, grid'i güncelle,
    /// kare iste. Aynı grid'e düşen font değişimi de yeniden çizilir:
    /// `DisplayLink::resize` kareyi koşulsuz istiyor ve kare istemek hasar
    /// bayrağını da dikiyor.
    ///
    /// Fare girdileri de burada tazeleniyor: view `WindowIvars.view`'da
    /// `Retained<BateriView>` olarak duruyor ve pencere nesnesiyle birlikte
    /// gidiyor.
    pub(crate) fn refresh_geometry(&self, app: &AppDelegate) {
        let grid = self.sync_geometry(app);
        self.ivars()
            .view
            .set_metrics(grid, self.ivars().dock_rows.get());
        if let Some(link) = self.ivars().link.get() {
            link.resize(
                grid.cols,
                grid.rows,
                grid.cell,
                self.ivars().dock_rows.get(),
            );
        }
    }

    /// Layer'ın drawable boyutunu view'ın backing geometrisiyle eşler **ve**
    /// grid ölçüsünü döndürür — ad ikisini birden söylüyor çünkü çağıranın
    /// ikisine de ihtiyacı var ve boyutu yazmadan ölçüyü türetmek yanlış
    /// sonuç verirdi. Ölçek tek kaynaktan okunur ve piksel boyutu ondan
    /// çarpılır; `drawableSize` ile `contentsScale` ayrışırsa bulanıklık olur.
    ///
    /// **Ölçeğin iki kapısı** (`Surface::set_size`, `Renderer::cell_metrics`;
    /// 003'ten beri borç) birleştirilmiyor: ikisinin tek çağıranı bu
    /// fonksiyon, ölçek burada bir kez okunuyor ve ikisine aynı yerelden
    /// gidiyor; font ayarı ölçeğe dokunmuyor
    /// (`.tasks/007-ayarlar-ve-tema/discussion.md` → Karar 6).
    ///
    /// Font yuvası da burada, sonda yazılıyor: atlası (yeniden) kuran tek
    /// yol `cell_metrics` ve font bildirimi ancak ondan sonra güncel. Ekran
    /// değişimi atlası yeniden kursa da aile aynı, yuva oynamaz.
    fn sync_geometry(&self, app: &AppDelegate) -> Grid {
        let window = &self.ivars().window;
        // Kapsayıcı değil terminal view'ı: ikisi bugün aynı boyda ama çizilen
        // yüzey bu view'ın layer'ı, ölçü de onun olmalı.
        let view = &self.ivars().view;
        let scale = window.backingScaleFactor();
        let bounds = view.bounds().size;
        let (width_px, height_px) = (bounds.width * scale, bounds.height * scale);
        self.ivars().surface.set_size(width_px, height_px, scale);

        // Hücre ölçüsü `bt-gpu` üzerinden `bt-atlas`'ın font metriğinden
        // geliyor ve ölçekle çarpma da orada. Burada ikinci bir yuvarlama
        // kuralı **yok**: eski `.round()` bloğu bilerek silindi. İki kural
        // yan yana dursaydı hangisinin kazandığı çağrı sırasına bağlanır ve
        // belirti bir piksellik hücre kayması, yani sessiz olurdu.
        let renderer = &self.ivars().renderer;
        let cell = renderer.cell_metrics(scale);
        app.post_notices(Source::Font, font_messages(renderer.font_notice()));
        split_into_grid(width_px, height_px, cell, self.ivars().dock_rows.get())
    }
}

#[cfg(test)]
mod tests {
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
