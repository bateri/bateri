//! Terminal pane'i: tek bir terminal oturumunun **bütün çekirdeği** — oturum,
//! kareyi süren display link, kendi `Renderer`'ı, `CAMetalLayer` yüzeyi,
//! `BateriView`, kabuğun uyandırma ucu (`ShellWake`), dock payı, geçici punto
//! farkı, sekme kimliği, geçmişte aramanın paneli ve yükleme kuyruğu (039
//! Karar 1–3).
//!
//! `TerminalPane` bir `NSView` alt sınıfı ve bugünkü içerik kapsayıcısının ta
//! kendisi (033 → R4.1): `BateriView` onu autoresizing'le dolduran çocuğu,
//! arama paneli Metal katmanının kardeşi olarak onun içinde yüzüyor. Pencere
//! (`window::TerminalWindow`) pane'i bölmelerin kapsayıcısına
//! (`split_view::SplitView`) takıyor ve krom, başlık, sekme, kapatma sorusu
//! gibi **sekmeye** ait işleri tutuyor; geometri, örtülme ve odak pencereden
//! bütün pane'lere dağıtılıyor. Bir sekmede birden çok pane olabilir (039
//! bölmeleri): her biri kendi oturumu, link'i ve renderer'ıyla.
//!
//! **Sınır üç parça** (Karar 1, 3): pane girdilerini doğumda tek pakette alır
//! ([`PaneLaunch`]: ayar anlık görüntüsü, tema, süreli koşu tarifi, ölçüm
//! defteri, entegrasyon ortamı + dock payı, kimlik, başlangıç dizini ve ilk
//! girdi, hareket bayrakları), olaylarını sahibine [`PaneHost`]'tan verir
//! (başlık, kabuğun çıkışı, yükleme durumu, bildirim, alt başlık tanısı, OSC
//! 52 kopyası) ve menünün karşıladığı her iş burada adlı bir yöntemdir —
//! seçici onu çağıran bir satır. Pane düzeyindeki seçiciler (punto, bul,
//! temizle, kaydır, yükleme iptali) pane'de, çünkü responder zinciri
//! `BateriView` → **pane** → pencere → delegate: hedefsiz menü öğesi onlara
//! odaktaki pane'den varıyor, arama alanı odaktayken de (alan pane'in
//! torunu). Bu modülde `AppDelegate`'e uzanan yol yok: ana kuyruk dönüşleri
//! pane'i sahibin verdiği arama fonksiyonuyla ([`PaneLookup`]) kimlikle
//! buluyor.
//!
//! **Renderer pane başına** (039 Karar 5; 026 → Karar 2a): atlasın anahtarı
//! ölçek ve punto içeriyor, punto farkı ise pane'in.

use std::cell::{Cell, OnceCell, RefCell};
use std::ffi::c_void;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use bt_core::{
    FontOptions, RemoteTarget, SearchCover, SearchDirection, SearchReport, SearchStatus, Session,
    SessionOptions, Settings, TabId, Theme, Wake,
};
use bt_core::{load_shell, smoke_shell};
use bt_gpu::{DisplayLink, GpuError, Layout, Pacer, Renderer, Stats, Surface, Waker};
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAlert, NSApplication, NSAutoresizingMaskOptions, NSBox, NSBoxType, NSButton, NSColor,
    NSControlTextEditingDelegate, NSEventModifierFlags, NSMenuItem, NSPasteboard,
    NSPasteboardNameFind, NSPopoverDelegate, NSSearchFieldDelegate, NSTextFieldDelegate,
    NSTitlePosition, NSView, NSViewFrameDidChangeNotification,
};
use objc2_foundation::{
    NSNotification, NSNotificationCenter, NSObjectProtocol, NSPoint, NSRect, NSSize, NSUUID,
};
use objc2_quartz_core::CAMetalLayer;

use crate::app::{self, Grid, split_into_grid};
use crate::child;
use crate::clipboard::{self, PendingCopy};
use crate::jobs::{self, Foreground, Libproc, Probe, ShellParent};
use crate::notices::{Source, font_messages};
use crate::pacer::MacPacer;
use crate::quote;
use crate::search_bar::{SearchBar, selection_query};
use crate::upload::Uploads;
use crate::uploader::{StopSheet, UploadPopover};
use crate::view::BateriView;
use crate::window::{Closing, Launch, is_dark_background};
use crate::zoom::Zoom;
use crate::{Run, Workload};

/// Pane'in sahibine verdiği olaylar (039 Karar 3) — bugün
/// `window::WindowHost`, yarın bir gömme uygulaması.
///
/// Hepsi **ana thread'de** ve pane'in kimliğiyle çağrılıyor ([`TerminalPane::id`]):
/// sahip birden çok pane tutuyor (bölmeler) ve olayın hangisinden geldiğini
/// bilmeli. Yöntemler AppKit tipi taşımıyor, yani sahip sahte bir uygulamayla
/// sınanabiliyor. Sayfalar ve popover pane view'ının kendi `window()`'unu
/// kullanıyor; onlar için sahibe sorulmuyor.
pub(crate) trait PaneHost {
    /// Başlık, çalışma dizini, uzak durum ya da yükleme yüzdesi değişti:
    /// pencerenin başlığını ve sekmenin noktasını pane'den yeniden okumalı.
    fn title_changed(&self, pane: u64);
    /// Kabuk çıktı: pane'in dayanağı kalmadı, kapanmalı (026 → Karar 5) —
    /// yalnız bu pane, sekme değil (039 Karar 8).
    fn shell_exited(&self, pane: u64);
    /// Klavye bu pane'in terminaline geldi (`BateriView` first responder
    /// oldu): odaktaki pane artık bu — başlık, sekme noktası ve yeni
    /// bölmenin mirası ondan (039 Karar 11).
    fn focused(&self, pane: u64);
    /// Yükleme kuyruğunun ilerlemesi ya da varlığı değişti — uygulamanın
    /// Dock simgesi bütün pane'lerin toplamı ([`TerminalPane::upload_totals`]).
    fn uploads_changed(&self, pane: u64);
    /// Kullanıcıya bildirim (yükleme bitti, hata verdi, bağlantı koptu).
    fn notify(&self, pane: u64, title: &str, body: &str);
    /// Alt başlık tanısı (bugün yalnız fontunki, `sync_geometry`).
    fn post_notices(&self, pane: u64, source: Source, messages: Vec<String>);
    /// Uzaktan kopya (OSC 52). Varsayılan kol genel panoya yazar — Cmd-C'nin
    /// panosu; panoyu ayırmak bir sahip kararı olarak açık duruyor.
    fn copy_to_clipboard(&self, _pane: u64, text: String) {
        clipboard::copy(&NSPasteboard::generalPasteboard(), Some(text));
    }
}

/// En küçük pane'in sütun sayısı (039 Karar 14): bölme bunun altına
/// düşecekse yapılmıyor. Ölçülmüş değil, bir tasarım sabiti — prompt'un iki
/// sütunu, kısa bir komut ve dock'un bağlam satırındaki klasör adı için yer;
/// daha darı kabuğun kendi satır sarmasını anlamsız kılıyor. Gözle kontrolde
/// ayarlanır.
const MIN_PANE_COLS: u16 = 20;

/// En küçük pane'in satır sayısı (039 Karar 14), dock'un payı **hariç**
/// ızgara satırı. Tasarım sabiti: bir komut ve birkaç satırlık çıktısı;
/// tam ekran bir program (vim, htop) bunun altında durum satırından başka
/// bir şey gösteremiyor.
const MIN_PANE_ROWS: u16 = 5;

/// Odakta olmayan pane'in örtüsünün saydamlığı (039 Karar 7): temanın
/// zemini bu oranda metnin üstüne biniyor. Ölçülmüş değil, bir tasarım
/// sabiti (`GUTTER_PT` emsali) — Ghostty'nin `unfocused-split-opacity`'sinin
/// varsayılanı `0.7`, yani örtü `0.3`; aynı oran: odak bir bakışta
/// okunuyor, soluk pane'in metni yine okunuyor. Gözle kontrolde ayarlanır.
const DIM_ALPHA: f64 = 0.3;

define_class!(
    // SAFETY: NSBox alt sınıflama için tasarlanmıştır; DimOverlay `Drop`
    // uygulamaz, ivar'ı yok ve NSBox'ın kurucusuyla (`new`) doğuyor.
    #[unsafe(super(NSBox))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriDimOverlay"]
    pub(crate) struct DimOverlay;

    unsafe impl NSObjectProtocol for DimOverlay {}

    impl DimOverlay {
        /// İsabet testine hiç girmiyor: tık, sürükleme ve tekerlek altındaki
        /// `BateriView`'a düşüyor — soluk pane'e tıklamak onu odaklıyor
        /// (039 phase-3'ün tıklama yolu) ve örtü bunu kesmemeli.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }
    }
);

impl DimOverlay {
    /// Gizli doğuyor; rengi [`DimOverlay::paint`], görünürlüğü sahip
    /// ([`TerminalPane::set_dimmed`]).
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        // SAFETY: `NSBox`'ın `init`'i; alt sınıfın ivar'ı yok.
        let this = Self::alloc(mtm).set_ivars(());
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setBoxType(NSBoxType::Custom);
        this.setTitlePosition(NSTitlePosition::NoTitle);
        this.setBorderWidth(0.0);
        this.setHidden(true);
        this
    }

    /// Temanın zemini, [`DIM_ALPHA`] saydamlığında. `NSColor` sRGB alıyor
    /// (`CLAUDE.md` → Renk uzayı; ayırıcının `separator_srgb` emsali).
    fn paint(&self, theme: &Theme) {
        let [r, g, b] = theme.background_srgb().map(|byte| f64::from(byte) / 255.0);
        self.setFillColor(&NSColor::colorWithSRGBRed_green_blue_alpha(
            r, g, b, DIM_ALPHA,
        ));
    }
}

/// Ana kuyruk dönüşlerinin pane'i kimlikle bulduğu yol; sahip veriyor
/// (bugün `app::pane_by_id`). Düz bir `fn` göstericisi, closure değil: `Send`
/// ve `Copy`, yani okuyucu thread'den ana kuyruğa atılan her iş onu
/// yakalayabiliyor ve bir referans çemberi açmıyor. Kapanışı başlamış pane'i
/// bulmamalı ([`TerminalPane::is_closed`]).
pub(crate) type PaneLookup = fn(MainThreadMarker, u64) -> Option<Retained<TerminalPane>>;

/// Pane'in doğum paketi (039 Karar 3): girdilerin tamamı tek yapıda, sahipten.
/// Canlı değişim ayrı yoldan, pane'in `set_*` yöntemleriyle.
pub(crate) struct PaneLaunch {
    /// Süreç içi kimlik ([`TerminalPane::id`]).
    pub(crate) id: u64,
    /// Süreli koşunun tarifi; `None` → etkileşimli.
    pub(crate) run: Option<Run>,
    /// Olayların sahibi.
    pub(crate) host: Rc<dyn PaneHost>,
    /// Ana kuyruk dönüşlerinin pane'i bulduğu yol.
    pub(crate) lookup: PaneLookup,
    /// Ölçüm defteri (süreli koşunun `BT_FRAME_STATS`'ı), link'e gidiyor.
    pub(crate) stats: Option<Arc<Stats>>,
    /// Ayarların doğum anındaki kopyası.
    pub(crate) settings: Settings,
    /// Oturumun teması.
    pub(crate) theme: Theme,
    /// Başlangıç dizini ve ilk girdi.
    pub(crate) launch: Launch,
    /// Shell entegrasyonunun ortamı ve dock payı — **tek sorudan**
    /// (`AppDelegate::shell_integration`): iki ayrı çağrı ayrışabilirdi.
    pub(crate) integration: (Vec<(String, String)>, u16),
    /// Hareketi Azalt'ın çözülmüş değeri.
    pub(crate) reduce_motion: bool,
    /// Tekerleğin çözülmüş kipi.
    pub(crate) smooth_scroll: bool,
    /// Devralınan geçici punto farkı (026 → Karar 3).
    pub(crate) zoom: Zoom,
}

/// Doğum paketinin yalnız [`TerminalPane::start`]'ın tükettiği yarısı.
struct Birth {
    stats: Option<Arc<Stats>>,
    settings: Settings,
    theme: Theme,
    launch: Launch,
    integration: (Vec<(String, String)>, u16),
}

/// Başlık haberinin ana kuyruk yarısı: bayrak **her okumadan önce** iniyor
/// (okumadan sonra gelen bir değişiklik yeni bir iş ister ve kaçmaz), sonra
/// pane'in kendi kenarı (`edge`: yükleme kuyruğunun bağlantısı) ve sahip
/// başlığı yeniden okuyor. Kenar da bayraktan sonra olmalı: arada biten ssh
/// iş doğurmaz ve kuyruk ölü bağlantıda kalırdı (`/code-review`). `swap`,
/// çünkü okuma-değiştirme-yazma yazarın `swap`'ıyla eşleşiyor ve onun yuvaya
/// yazdığını görünür kılıyor.
fn announce_title(pending: &AtomicBool, edge: impl FnOnce(), host: &dyn PaneHost, pane: u64) {
    pending.swap(false, Ordering::AcqRel);
    edge();
    host.title_changed(pane);
}

/// OSC 52 kopyasının ana kuyruk yarısı: yuvadaki metin sahibe
/// ([`PaneHost::copy_to_clipboard`]); yuva boşsa (yarışta başka iş almış)
/// olay yok.
fn announce_copy(pending: &PendingCopy, host: &dyn PaneHost, pane: u64) {
    if let Some(text) = pending.take() {
        host.copy_to_clipboard(pane, text);
    }
}

/// Sistemin find panosundaki metin (033 Karar 6: ⌘E'nin uygulamalar arası
/// normu), ⌘E'nin sorgusuyla aynı süzgeçten: ilk satırı, boş ya da yalnız
/// boşluksa `None` ([`selection_query`]).
fn find_pasteboard_text() -> Option<String> {
    // SAFETY: AppKit'in dışa açtığı sabit ad, süreç boyunca yaşıyor.
    let name = unsafe { NSPasteboardNameFind };
    clipboard::read(&NSPasteboard::pasteboardWithName(name))
        .and_then(|text| selection_query(&text, false))
}

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

/// `bt-core`'un uyandırma ucu — pane başına bir tane, oturumuyla birlikte.
///
/// `Session::spawn` `Wake`'i link'ten **önce** ister, `Waker` ise link'ten
/// sonra doğar; boşluğu yuvanın `None`'ı kapatır. Kaçan kare yok: açılış
/// karesi zaten elle isteniyor ve o ana kadar okunmuş her bayt hasar
/// bayrağında birikmiş olur.
struct ShellWake {
    /// Pane'in kimliği: ana kuyruk işleri pane'i bununla buluyor
    /// ([`PaneLookup`], alternatif ekran habercisinin örüntüsü) —
    /// `Session`'a ya da pane'e referans tutmak `wake.rs`'in Sahiplik
    /// çemberini kapatırdı.
    id: u64,
    /// Kimlikten pane'e giden yol, sahipten ([`PaneLaunch::lookup`]).
    lookup: PaneLookup,
    /// Süreli koşu mu: `child_exit` iki yola ayrılıyor ([`Wake::child_exit`]'in
    /// gövdesi). Doğum paketindeki tarifin (`PaneLaunch::run`) özeti;
    /// okuyucu thread'den pane'e uzanılamaz.
    timed: bool,
    /// Link'in `Waker`'ı — **yaprak kilit** altında ve **sökülebilir**.
    ///
    /// Pane kapanırken ana thread'de `take()` ediliyor
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
    /// Uzak oturum yoklamasının silahı ve bekleyen işi (036); `Arc`, çünkü
    /// ana kuyruğun işi onu tutuyor.
    remote_probe: Arc<RemoteProbe>,
}

/// Uzak oturum yoklamasının iki biti (036 Karar 2): **silah** (bu komut için
/// kesin bir cevap henüz yok) ve **bekleyen iş** (ana kuyrukta bir yoklama
/// var — en çok bir tane, `title_pending`'in örüntüsü).
///
/// Silah `C` kenarında kuruluyor; kuruluyken her `wake` (PTY'den gelen
/// çıktı) bir iş atıyor, kesin cevap onu indiriyor ve sonraki çıktılar
/// yoklamıyor. Akan bir `cat`'in bedeli tek yoklama.
///
/// **İş silahı yoklamadan önce indiriyor**, sonra değil, ve kararsız cevapta
/// geri kuruyor: yoklama sürerken okuyucu thread'de gelen yeni bir `C` silahı
/// kurup yeni bir iş atabiliyor ve bitmekte olan eski komutun kesin cevabı
/// onu indirseydi yeni komut hiç yoklanmazdı.
#[derive(Debug, Default)]
struct RemoteProbe {
    armed: AtomicBool,
    pending: AtomicBool,
}

impl RemoteProbe {
    /// `C` kenarı: silahı kurar; ana kuyruğa iş atılacaksa `true`.
    fn command_started(&self) -> bool {
        self.armed.store(true, Ordering::Release);
        self.claim()
    }

    /// Çıktı kenarı (okuyucu thread, `Term` kilidi altında olabilir): silah
    /// kuruluysa ve iş beklemiyorsa `true`. Silahsızken tek bir atomik okuma.
    fn output(&self) -> bool {
        self.armed.load(Ordering::Acquire) && self.claim()
    }

    /// Bekleyen işin yuvasını alır; zaten bekleyen varsa `false`.
    fn claim(&self) -> bool {
        !self.pending.swap(true, Ordering::AcqRel)
    }

    /// Ana kuyruktaki işin başı: yuvayı bırakır ve silahı indirir; silah
    /// kurulu değilse (kesin cevap verildi) yoklama yok.
    fn begin(&self) -> bool {
        self.pending.store(false, Ordering::Release);
        self.armed.swap(false, Ordering::AcqRel)
    }

    /// Kararsız cevap: silah geri kuruluyor, iş atılmıyor — sonraki çıktı
    /// atar.
    fn rearm(&self) {
        self.armed.store(true, Ordering::Release);
    }
}

impl ShellWake {
    /// Uzak oturum yoklamasını ana kuyruğa atar ([`RemoteProbe`]).
    fn dispatch_remote_probe(&self) {
        let probe = Arc::clone(&self.remote_probe);
        let (id, lookup) = (self.id, self.lookup);
        DispatchQueue::main().exec_async(move || {
            if !probe.begin() {
                return;
            }
            // audit: ana kuyrukta koşan blok tanımı gereği ana thread'dedir.
            let mtm = MainThreadMarker::new().expect("ana kuyruk ana thread'dir");
            // Pane bu arada kapandıysa yoklanacak bir kabuk da yok.
            let Some(pane) = lookup(mtm, id) else {
                return;
            };
            let outcome = pane.probe_remote();
            // Uzak durumun kenarı yükleme kuyruğunun, pencerenin başlığının ve
            // sekmenin noktasının kenarı ([`TerminalPane::remote_or_title_changed`]).
            if outcome.changed {
                pane.remote_or_title_changed();
            }
            if outcome.undecided {
                probe.rearm();
            }
        });
    }

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
        // Uzak oturum yoklaması kararsız kaldıysa bu çıktı onu yeniden
        // tetikliyor (036 Karar 2); silahsızken bedel bir atomik okuma.
        if self.remote_probe.output() {
            self.dispatch_remote_probe();
        }
    }

    fn child_exit(&self, _code: Option<i32>) {
        // Shell gitti, pane'in dayanağı kalmadı: **o pencere** kapanır
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
        let (timed, id, lookup) = (self.timed, self.id, self.lookup);
        DispatchQueue::main().exec_async(move || {
            // audit: ana kuyrukta koşan blok tanımı gereği ana thread'dedir.
            let mtm = MainThreadMarker::new().expect("ana kuyruk ana thread'dir");
            if timed {
                NSApplication::sharedApplication(mtm).terminate(None);
                return;
            }
            // Pane bu arada kapandıysa (⌘W'nin `SIGHUP`'ı kabuğu öldürdü
            // ve haber sonradan geldi) kapatacak bir şey yok. Kapatmak
            // sahibin işi ([`PaneHost::shell_exited`]).
            if let Some(pane) = lookup(mtm, id) {
                pane.host().shell_exited(id);
            }
        });
    }

    fn copy_to_clipboard(&self, text: String) {
        // Okuyucu thread, `Term` kilidi tutuluyor: metin kilitsiz yuvaya,
        // ana kuyruğa en çok **bir** iş (`PendingCopy`'nin doc'u). Yuvada
        // bekleyen metin varsa onu alacak iş zaten kuyrukta.
        //
        // Panoyu sahip seçiyor ([`PaneHost::copy_to_clipboard`]; varsayılan
        // genel pano, Cmd-C'ninkiyle aynı); işin sırası `child_exit`'inkiyle
        // aynı gerekçeden: ana kuyruk. Pane bu arada kapandıysa metin düşüyor.
        if self.pending_copy.put(text) {
            let pending = Arc::clone(&self.pending_copy);
            let (id, lookup) = (self.id, self.lookup);
            DispatchQueue::main().exec_async(move || {
                // audit: ana kuyrukta koşan blok tanımı gereği ana thread'dedir.
                let mtm = MainThreadMarker::new().expect("ana kuyruk ana thread'dir");
                if let Some(pane) = lookup(mtm, id) {
                    announce_copy(&pending, pane.host(), id);
                }
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
        let (id, lookup) = (self.id, self.lookup);
        DispatchQueue::main().exec_async(move || {
            // audit: ana kuyrukta koşan blok tanımı gereği ana thread'dedir.
            let mtm = MainThreadMarker::new().expect("ana kuyruk ana thread'dir");
            // Pane bu arada kapanmışsa yazacak bir başlık da yok (bayrak
            // dikili kalıyor; kapanmış pane'in haberi zaten düşüyor).
            if let Some(pane) = lookup(mtm, id) {
                // Uzak durumun kenarı (`D`/`A`'nın silmesi) yükleme kuyruğunun
                // da kenarı — önce o, sonra sahip başlığı okuyor.
                announce_title(&pending, || pane.check_upload_connection(), pane.host(), id);
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
        let (id, lookup) = (self.id, self.lookup);
        DispatchQueue::main().exec_async(move || {
            pending.swap(false, Ordering::AcqRel);
            // audit: ana kuyrukta koşan blok tanımı gereği ana thread'dedir.
            let mtm = MainThreadMarker::new().expect("ana kuyruk ana thread'dir");
            // Arka sekmede de işliyor: haber kare yoluna bağlı değil. Pane
            // bu arada kapandıysa sayacak bir şey yok.
            if let Some(pane) = lookup(mtm, id) {
                pane.kick_search();
            }
        });
    }

    fn command_started(&self) {
        // Okuyucu thread, kilitsiz. Süreli koşu algılamıyor: jetonları
        // bugünkü kalmalı (ve sabit betiğin entegrasyonu da yok).
        if self.timed {
            return;
        }
        if self.remote_probe.command_started() {
            self.dispatch_remote_probe();
        }
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
/// **Hedefsiz eylem değil, pane kimliği.** Responder zinciri key pencereye
/// gidiyor: arka sekmede vim'den çıkış yanlış pane'i boyutlandırırdı. İş
/// kimliği (`id`) yakalıyor, pane'i sahibin yolundan ([`PaneLookup`]) buluyor
/// ve bulamazsa düşüyor — pane o arada kapanmışsa boyutlandıracak bir şey de
/// yok.
///
/// Yakaladığı tek şey bir tamsayı. Eski "hiçbir şey yakalamıyor" kuralının
/// gerekçesi bir **referans çemberiydi** (`DisplayLink` pane'in ivar'ında
/// duruyor, pane'i tutan bir closure onu kendine bağlardı); bir tamsayı
/// çember açmıyor. Üstelik `exec_async` `Send` istiyor ve pane nesnesi
/// ana thread'e çivili — tutabileceği başka bir şey de yoktu.
///
/// Yük taşımıyor: alıcı gerçeği yeniden okuyor ([`TerminalPane::alt_screen_did_change`]),
/// yani birbirini kovalayan iki geçiş (vim aç-kapa) bayat bir değerle
/// davranamıyor.
fn alt_screen_notifier(id: u64, lookup: PaneLookup) -> Box<dyn Fn()> {
    Box::new(move || {
        DispatchQueue::main().exec_async(move || {
            // audit: ana kuyrukta koşan blok tanımı gereği ana thread'dedir.
            let mtm = MainThreadMarker::new().expect("ana kuyruk ana thread'dir");
            if let Some(pane) = lookup(mtm, id) {
                pane.alt_screen_did_change();
            }
        });
    })
}

/// Uzak oturum yoklamasının sonucu ([`TerminalPane::probe_remote`]): iki
/// ayrı cevap, çünkü ikisinin tüketicisi ayrı — kararsızlık silahı geri
/// kuruyor (pane'in işi), uzak durumun değişmesi başlığı ve sekmenin
/// noktasını tazeliyor (pencerenin işi).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RemoteProbeOutcome {
    /// Cevap kararsız: silah kurulu kalır, sonraki çıktı yeniden yoklar.
    pub(crate) undecided: bool,
    /// Oturumun uzak durumu değişti (`Session::set_remote`'un dönüşü).
    pub(crate) changed: bool,
}

/// Pane'in durumu. `OnceCell`: oturum ve link `start` içinde bir kez doğar,
/// sonra yalnız okunur. View, yüzey ve renderer kurucuda doğuyor.
///
/// **Pencere burada tutulmuyor**: pane pencerenin `contentView`'ı, yani
/// pencere onu güçlü tutuyor ve geri referans bir çember olurdu. Pencereye
/// her ihtiyaçta `NSView::window` ile bakılıyor (ölçek, key biti).
pub(crate) struct PaneIvars {
    /// Kendi sayacımız ([`AppDelegate`] dağıtıyor, pencerelerinkiyle aynı
    /// sayaçtan): okuyucu thread'den ana kuyruğa dönen işlerin pane'i
    /// bulduğu anahtar (`AppDelegate::pane`).
    id: u64,
    /// Süreli koşunun tarifi, doğum paketinden (`Copy`): odak yolu onu her
    /// uygulama geçişinde soruyor ([`TerminalPane::apply_focus`]).
    run: Option<Run>,
    /// Olayların sahibi ([`PaneHost`]).
    host: Rc<dyn PaneHost>,
    /// Ana kuyruk dönüşlerinin pane'i bulduğu yol ([`PaneLookup`]).
    lookup: PaneLookup,
    /// Doğum paketinin `start`'ın tükettiği yarısı; `start`'tan sonra `None`.
    birth: RefCell<Option<Birth>>,
    /// Ayarın fontu — punto farkı ona uygulanıyor
    /// ([`TerminalPane::change_zoom`]); canlı değişimi [`TerminalPane::set_font`].
    font: RefCell<FontOptions>,
    /// Hareketi Azalt'ın çözülmüş değeri — arama panelinin animasyonu da
    /// ona bakıyor; canlı değişimi [`TerminalPane::set_reduce_motion`].
    reduce_motion: Cell<bool>,
    /// Tekerleğin çözülmüş kipi — aramanın eşleşmeye gidişi de ona bakıyor;
    /// canlı değişimi [`TerminalPane::set_smooth_scroll`].
    smooth_scroll: Cell<bool>,
    /// `Rc`: renderer ana thread'e çivili (bkz. `bt_gpu::DisplayLink`) ve
    /// link de bir kopya tutuyor.
    renderer: Rc<Renderer>,
    /// The terminal view's layer — this pane owns it (040 → Karar 8): it is
    /// hung on the view here and its scale is set from the window
    /// (`sync_geometry`); `bt-gpu` draws into it through [`Surface`].
    layer: Retained<CAMetalLayer>,
    /// The wgpu surface over `layer`; shared with the link, which acquires
    /// each frame's texture from it.
    surface: Rc<Surface>,
    /// Fare çevirisinin girdileri pane boyuyla tazeleniyor (`set_metrics`);
    /// geometrinin kaynağı da bu view (`sync_geometry`).
    view: Retained<BateriView>,
    /// Odakta olmayan pane'in soluk örtüsü (039 Karar 7): pane'in en üstteki
    /// çocuğu, Metal katmanının kardeşi — kare yoluna girmiyor, bileşimi
    /// CoreAnimation'ın. Görünürlüğü sahip belirliyor.
    dim: Retained<DimOverlay>,
    link: OnceCell<DisplayLink>,
    /// Kapanış sırasının ikinci adımı buradan çağrılır; `DisplayLink` de bir
    /// kopya tutuyor ama oraya `stop()`'tan sonra uzanmak yanlış olurdu.
    session: OnceCell<Arc<Session>>,
    /// Kabuk PTY'nin çocuğu mu, çocuğunun çocuğu mu — oturumla aynı anda,
    /// **komuttan** yazılıyor ([`TerminalPane::start_session`]); koşan işin
    /// tespiti kabuğu bununla buluyor ([`TerminalPane::foreground`]).
    shell_parent: OnceCell<ShellParent>,
    wake: Arc<ShellWake>,
    /// Cmd +/−/0'ın geçici punto farkı — **bu pane'in**: renderer'a giden
    /// font `zoom.apply(&settings.font)` ([`TerminalPane::apply_font`]).
    /// Dosyadaki `size` değişince sıfırlanır ([`TerminalPane::zoom_after_reload`]).
    zoom: Cell<Zoom>,
    /// Dock kaç satır; `0` → bu pane'de dock yok.
    ///
    /// **Oturum doğarken kararlaşıyor** (012 → R5.1): kaynağı entegrasyonun
    /// kurulup kurulmadığı ve o [`TerminalPane::start`]'ta **bir kez**
    /// soruluyor. Yuva o yüzden var: `sync_geometry` her geometri olayında
    /// koşuyor ve ızgara yüksekliğini hesaplarken cevabı bilmek zorunda;
    /// ikinci kez sormak, iki çağrının ayrışabildiği bir gelecekte "pane
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
    /// ([`PaneIvars::dock_rows_at_birth`]) ve ikisinin ayrı durması şart —
    /// yoksa alternatif ekrandan çıkış, dock'u hiç olmayan bir pane'de dock
    /// doğururdu.
    dock_rows: Cell<u16>,
    /// Oturum doğarken kararlaşan dock payı: entegrasyon kurulduysa
    /// `DOCK_ROWS`, kurulmadıysa `0` (R5.1).
    ///
    /// Koşu boyunca **oynamıyor**; alternatif ekranın geri getireceği değer bu
    /// ve tek yazanı oturumun doğumu.
    dock_rows_at_birth: Cell<u16>,
    /// Oturumun kalıcı kimliği (038, 039 Karar 10): kabuğa `TERM_SESSION_ID`
    /// ve `BATERI_TAB_URL` olarak gidiyor, `bateri://tab/<id>` onunla pane'i
    /// buluyor. Pane'in ömrü boyunca sabit; süreç içi [`id`] ayrı bir şey
    /// (ana kuyruk dönüşlerinin anahtarı).
    ///
    /// [`id`]: PaneIvars::id
    tab_id: TabId,
    /// Kapanış başladı ([`TerminalPane::begin_close`]): pane'in penceresi
    /// listeden bir tur sonra çıkıyor (`forget_window`) ve o arada
    /// `AppDelegate::pane` onu bulmamalı — okuyucu thread'in bayat bir haberi
    /// kapanmış oturuma iş yapmasın, `bateri://tab/` da oturumsuz bir
    /// pencereyi ekrana döndürmesin (`/code-review`, 038).
    closed: Cell<bool>,
    /// Geçmişte aramanın paneli (033) — ilk ⌘F'de doğuyor: hiç aranmayan
    /// pane görünümlerini taşımıyor. Sorgu ve anahtarlar panelde, yani
    /// **pane başına** ve kapanınca unutulmuyor (Karar 6).
    search: OnceCell<SearchBar>,
    /// Oturuma verilen son sorgunun durumu — etiketin girdisi.
    search_status: Cell<SearchStatus>,
    /// Sayım dizininin sürücüsü ana kuyrukta bir tur bekliyor mu
    /// ([`TerminalPane::kick_search`]): ikinci bir sürücü kurulmasın.
    search_driving: Cell<bool>,
    /// Finder damlasının uzak dizine yüklenmesi (037 Karar 7): sıra,
    /// ilerleme ve sonuç satırı ([`crate::upload::Uploads`]). Kuyruk **bu
    /// pane'in ssh bağlantısının** — başka sekmeye geçmek onu durdurmuyor.
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

define_class!(
    // SAFETY: NSView alt sınıflama için tasarlanmıştır; TerminalPane `Drop`
    // uygulamaz ve `initWithFrame:` dışında bir kurucu sunmaz.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTerminalPane"]
    #[ivars = PaneIvars]
    pub(crate) struct TerminalPane;

    unsafe impl NSObjectProtocol for TerminalPane {}

    impl TerminalPane {
        /// Terminal view'ının çerçevesi değişti (`NSViewFrameDidChangeNotification`,
        /// gözlemci [`TerminalPane::observe_frame`]'de kuruluyor).
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
            self.refresh_geometry();
        }
    }

    /// "Show files (N)" popover'ının kapanışı (037 phase-7): `transient`
    /// popover'ı AppKit de kapatıyor (dışarı tık) ve düğmenin basılı tonu
    /// ile Esc izleyicisi o zaman da kalkmalı.
    unsafe impl NSPopoverDelegate for TerminalPane {
        #[unsafe(method(popoverWillClose:))]
        fn popover_will_close(&self, _n: &NSNotification) {
            self.upload_list_will_close();
        }

        #[unsafe(method(popoverDidClose:))]
        fn popover_did_close(&self, _n: &NSNotification) {
            self.close_upload_list();
        }
    }

    // Arama alanının delegesi (033): üç protokolün de bütün yöntemleri
    // isteğe bağlı; kullanılanlar aşağıdaki `impl`'de.
    unsafe impl NSControlTextEditingDelegate for TerminalPane {}
    unsafe impl NSTextFieldDelegate for TerminalPane {}
    unsafe impl NSSearchFieldDelegate for TerminalPane {}

    // **Pane düzeyindeki menü seçicileri** (039 Karar 2): her biri adlı bir
    // yöntemin tek satırlık sarmalayıcısı (R2.3) — menüsüz bir sahip aynı
    // yöntemi doğrudan çağırabilir. Hedefsiz eylemin responder zinciri
    // `BateriView` → pane → pencere → delegate, arama alanı odaktayken alan
    // düzenleyicisi → alan → … → pane; yani öğe odaktaki pane'e varıyor ve
    // 033'ün "alan odaktayken zincir `BateriView`'dan geçmiyor" gerekçesi
    // konusuz. Uygulama geneline yayılanlar (`settingsDidChange:`, tema)
    // `AppDelegate`'te, sekme işleri (`closeTab:`, `selectTab:`) pencerede.
    impl TerminalPane {
        /// View ▸ Bigger (Cmd +).
        #[unsafe(method(makeFontBigger:))]
        fn make_font_bigger(&self, _sender: Option<&AnyObject>) {
            self.zoom_in();
        }

        /// View ▸ Smaller (Cmd −).
        #[unsafe(method(makeFontSmaller:))]
        fn make_font_smaller(&self, _sender: Option<&AnyObject>) {
            self.zoom_out();
        }

        /// View ▸ Actual Size (Cmd 0).
        #[unsafe(method(resetFontSize:))]
        fn reset_font_size(&self, _sender: Option<&AnyObject>) {
            self.zoom_reset();
        }

        /// Edit ▸ Find ▸ Find… (⌘F).
        ///
        /// Seçiciler **kendi adlarımız**, `performFindPanelAction:` değil
        /// (033 Karar 10): alan odaktayken first responder AppKit'in alan
        /// düzenleyicisi ve o seçiciyi kendisi uygulayıp yutardı.
        #[unsafe(method(findInScrollback:))]
        fn find_in_scrollback(&self, _sender: Option<&AnyObject>) {
            self.find();
        }

        /// Edit ▸ Find ▸ Find Next (⌘G) ve panelin yukarı oku.
        #[unsafe(method(findNextMatch:))]
        fn find_next_match(&self, _sender: Option<&AnyObject>) {
            self.find_next();
        }

        /// Edit ▸ Find ▸ Find Previous (⇧⌘G) ve panelin aşağı oku.
        #[unsafe(method(findPreviousMatch:))]
        fn find_previous_match(&self, _sender: Option<&AnyObject>) {
            self.find_previous();
        }

        /// Edit ▸ Find ▸ Use Selection for Find (⌘E).
        #[unsafe(method(useSelectionForFind:))]
        fn use_selection_for_find_action(&self, _sender: Option<&AnyObject>) {
            self.use_selection_for_find();
        }

        /// Edit ▸ Clear to Start (⌘K).
        #[unsafe(method(clearToStart:))]
        fn clear_to_start_action(&self, _sender: Option<&AnyObject>) {
            self.clear_to_start();
        }

        /// Edit ▸ Clear Scrollback (⌥⌘K).
        #[unsafe(method(clearScrollback:))]
        fn clear_scrollback_action(&self, _sender: Option<&AnyObject>) {
            self.clear_scrollback();
        }

        /// View ▸ Scroll to Top (⌘Home).
        #[unsafe(method(scrollToTop:))]
        fn scroll_to_top_action(&self, _sender: Option<&AnyObject>) {
            self.scroll_to_top();
        }

        /// View ▸ Scroll to Bottom (⌘End).
        #[unsafe(method(scrollToBottom:))]
        fn scroll_to_bottom_action(&self, _sender: Option<&AnyObject>) {
            self.scroll_to_bottom();
        }

        /// View ▸ Page Up (⌘PgUp).
        #[unsafe(method(scrollPageUp:))]
        fn scroll_page_up_action(&self, _sender: Option<&AnyObject>) {
            self.page_up();
        }

        /// View ▸ Page Down (⌘PgDn).
        #[unsafe(method(scrollPageDown:))]
        fn scroll_page_down_action(&self, _sender: Option<&AnyObject>) {
            self.page_down();
        }

        /// Panelin kapatma düğmesi — Esc ile aynı yol (033 Karar 5).
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

        /// Alanın komut kancası (033 Karar 10): ⏎ bir önceki (daha eski), ⇧⏎
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

        /// Find öğelerinin, temizlemenin, kaydırmanın ve yükleme iptalinin
        /// etkinliği; **bilinmeyen öğe `true`** — punto hep etkin.
        /// Temizleme ve kaydırma alternatif ekranda gri (034 Karar 2):
        /// birincil geçmiş orada erişilemez, gri öğe dürüst bir "burada
        /// olmaz"; oturum yoksa da gri.
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
                // ⌘. yalnız bu pane'de kuyruk varken (037 Karar 7); gri
                // öğenin kısayolu `keyDown:`'a düşüyor ve orada yutuluyor.
                self.ivars().uploads.borrow().active()
            } else {
                true
            }
        }

        /// Shell ▸ Cancel Upload (⌘.) ve popover'ın `Cancel all ⌘.`'u: bu
        /// pane'in **bütün** yükleme kuyruğu (037 Karar 7 → Kullanıcı kararı
        /// 5); akan kalem 30 saniyeyi geçtiyse önce sorar (phase-7). Esc
        /// değil, çünkü klavye o sırada uzak kabuğa gidiyor. Menü kısayolu
        /// `keyDown:`'dan önce yakalanıyor, yani alternatif ekranda (vim)
        /// da çalışıyor — kapısı yalnız kuyruk (`validateMenuItem:`).
        #[unsafe(method(cancelUpload:))]
        fn cancel_upload(&self, _sender: Option<&AnyObject>) {
            self.cancel_uploads();
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
    }
);

impl TerminalPane {
    /// View'ı, yüzeyi ve renderer'ı kurar; oturum ve link **henüz yok**
    /// ([`TerminalPane::start`]). Çerçeve gözlemcisi de henüz yok: pencere
    /// pane'i `contentView` yaptıktan sonra kuruyor
    /// ([`TerminalPane::observe_frame`]).
    ///
    /// Renderer burada doğuyor ve hatası çağırana dönüyor: Metal device ya da
    /// metallib yoksa pane'in çizebileceği bir şey de yok. Font renderer'a
    /// burada **istek** olarak veriliyor, devralınan punto farkıyla
    /// ([`TerminalPane::request_font`]): ilk atlas `start`'ın geometrisinde
    /// büyütülmüş puntoyla açılıyor.
    pub(crate) fn new(
        mtm: MainThreadMarker,
        frame: NSRect,
        launch: PaneLaunch,
    ) -> Result<Retained<Self>, GpuError> {
        let PaneLaunch {
            id,
            run,
            host,
            lookup,
            stats,
            settings,
            theme,
            launch,
            integration,
            reduce_motion,
            smooth_scroll,
            zoom,
        } = launch;
        let renderer = Rc::new(Renderer::system_default()?);
        // The layer is ours (040 → Karar 8): wgpu configures its device,
        // format and drawable size, the scale stays with its owner.
        let layer = CAMetalLayer::new();
        // SAFETY: `layer` is a live `CAMetalLayer`; wgpu retains it.
        let surface = Rc::new(unsafe {
            Surface::from_layer(&renderer, NonNull::from(&*layer).cast::<c_void>())
        }?);
        let view = BateriView::new(mtm, frame);
        // Sıra önemli: önce layer, sonra wantsLayer — tersi AppKit'e kendi
        // layer'ını kurdurur ve CAMetalLayer düşer.
        view.setLayer(Some(&layer));
        view.setWantsLayer(true);
        let font = settings.font.clone();
        let dim = DimOverlay::new(mtm);
        dim.paint(&theme);
        let this = Self::alloc(mtm).set_ivars(PaneIvars {
            id,
            run,
            host,
            lookup,
            birth: RefCell::new(Some(Birth {
                stats,
                settings,
                theme,
                launch,
                integration,
            })),
            font: RefCell::new(font),
            reduce_motion: Cell::new(reduce_motion),
            smooth_scroll: Cell::new(smooth_scroll),
            renderer,
            layer,
            surface,
            view: view.clone(),
            dim: dim.clone(),
            link: OnceCell::new(),
            session: OnceCell::new(),
            shell_parent: OnceCell::new(),
            wake: Arc::new(ShellWake {
                id,
                lookup,
                timed: run.is_some(),
                waker: Mutex::new(None),
                pending_copy: Arc::default(),
                title_pending: Arc::default(),
                search_pending: Arc::default(),
                remote_probe: Arc::default(),
            }),
            zoom: Cell::new(zoom),
            // Açılışta dock yok: kararı `start` veriyor ve geometriyi ondan
            // sonra hesaplıyor.
            dock_rows: Cell::new(0),
            dock_rows_at_birth: Cell::new(0),
            tab_id: new_tab_id(),
            closed: Cell::new(false),
            search: OnceCell::new(),
            search_status: Cell::new(SearchStatus::Empty),
            search_driving: Cell::new(false),
            uploads: RefCell::new(Uploads::default()),
            upload_alert: RefCell::new(None),
            upload_stop: RefCell::new(None),
            upload_list: RefCell::new(None),
            list_closed_at: Cell::new(None),
        });
        // SAFETY: `initWithFrame:` NSView'un tasarlanmış kurucusu ve ivar'lar
        // set edildi.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        // Pane düz bir **kapsayıcı**, `BateriView` onun çocuğu (033 → R4.1):
        // arama paneli terminalin üstünde yüzecek ve Metal katmanının kardeşi
        // olmak zorunda, çocuğu değil — layer-hosting view'ın alt view'ları
        // AppKit'in sözleşmesi dışında. Pane layer-backed, yoksa kardeş panel
        // Metal katmanının **altında** kalabilir. Kendisi hiçbir şey çizmiyor
        // ve olay almıyor: `BateriView` onu tamamen dolduruyor, isabet testi
        // en üstteki çocuğa düşüyor.
        this.setWantsLayer(true);
        // Çocuk pane'e oturtuluyor ve boyu autoresizing'le izliyor. Geometrinin
        // kaynağı yine `BateriView` (`sync_geometry`), bildirimi de onun
        // çerçevesi.
        view.setFrame(this.bounds());
        view.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        this.addSubview(&view);
        // Örtü en üstte: arama paneli `view`'ın hemen üstüne giriyor
        // (`SearchBar::new`), yani o da örtünün altında kalıyor ve soluk
        // pane'in paneli de soluk.
        dim.setFrame(this.bounds());
        dim.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        this.addSubview(&dim);
        let font = this.ivars().font.borrow().clone();
        this.request_font(&font);
        Ok(this)
    }

    /// Terminal view'ının çerçeve bildirimine abone olur
    /// (`viewFrameDidChange:`). İçeriğin boyu pencereden bağımsız da
    /// değişiyor (sekme çubuğu); geometri bu yüzden view'ın kendi
    /// bildiriminden. `postsFrameChangedNotifications` varsayılanda açık.
    ///
    /// Pencerenin kurucusunun **son** adımı: pane `contentView` olmadan önce
    /// gelen bir bildirim geometriyi pencere olmadan kurmaya çalışırdı.
    /// Gözlemci pane'in kapanışında sökülüyor ([`TerminalPane::begin_close`]),
    /// pencerenin kapanışını beklemeden.
    pub(crate) fn observe_frame(&self) {
        // SAFETY: seçici bu sınıfta tanımlı ve tek `&NSNotification` alıyor;
        // ad AppKit'in dışa açtığı sabit, nesne bu pane'in view'ı.
        unsafe {
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                self,
                sel!(viewFrameDidChange:),
                Some(NSViewFrameDidChangeNotification),
                Some(&self.ivars().view),
            );
        }
    }

    pub(crate) fn id(&self) -> u64 {
        self.ivars().id
    }

    /// Oturumun kalıcı kimliği (`TERM_SESSION_ID`, `bateri://tab/<id>`;
    /// 038). Süreç içi [`TerminalPane::id`]'den ayrı: o ana kuyruk
    /// dönüşlerinin anahtarı, bu dışarıya verilen ad.
    pub(crate) fn tab_id(&self) -> &TabId {
        &self.ivars().tab_id
    }

    /// Kapanış başladı mı ([`PaneIvars::closed`]).
    pub(crate) fn is_closed(&self) -> bool {
        self.ivars().closed.get()
    }

    /// Bu pane'in geçici punto farkı — yeni sekme onu devralıyor
    /// (026 → Karar 3).
    pub(crate) fn zoom(&self) -> Zoom {
        self.ivars().zoom.get()
    }

    /// Olayların sahibi ([`PaneHost`]).
    pub(crate) fn host(&self) -> &dyn PaneHost {
        &*self.ivars().host
    }

    /// Ana kuyruk dönüşlerinin pane'i bulduğu yol (`uploader`'ın işleri de
    /// onu yakalıyor).
    pub(crate) fn lookup(&self) -> PaneLookup {
        self.ivars().lookup
    }

    /// Rapor yolu sayaçlarını buradan okuyor (`AppDelegate::report_and_exit`).
    pub(crate) fn renderer(&self) -> &Renderer {
        &self.ivars().renderer
    }

    pub(crate) fn link(&self) -> Option<&DisplayLink> {
        self.ivars().link.get()
    }

    /// Terminal view'ı.
    pub(crate) fn view(&self) -> &BateriView {
        &self.ivars().view
    }

    pub(crate) fn session(&self) -> Option<&Arc<Session>> {
        self.ivars().session.get()
    }

    /// Renderer'a ayarın fontunu bu pane'in punto farkıyla **istek**
    /// olarak verir — açılışın yolu ([`TerminalPane::new`]): atlas `start`'ın
    /// `sync_geometry`'sinde açılıyor ve font yuvasını da o yazıyor. Dönüş
    /// (değişti mi) burada soru değil: geometri henüz hiç kurulmadı.
    fn request_font(&self, font: &FontOptions) {
        let _ = self
            .ivars()
            .renderer
            .set_font(&self.ivars().zoom.get().apply(font));
    }

    /// Dosyadaki font değişti: punto farkı [`Zoom::after_reload`]'ın
    /// kuralıyla güncellenir. Fontu uygulamaz; ayarlar yazıldıktan sonra
    /// [`TerminalPane::apply_font`] uygular.
    pub(crate) fn zoom_after_reload(&self, old: &FontOptions, new: &FontOptions) {
        let zoom = self.ivars().zoom.get().after_reload(old, new);
        self.ivars().zoom.set(zoom);
    }

    /// Doğum paketinin kalanını tüketir: dock payını kararlaştırır, ilk
    /// geometriyi kurar ve oturumu açar.
    ///
    /// Entegrasyon sahip tarafında **bir kez** soruldu ve iki cevabı birden
    /// verdi: çocuğun ortamı ile dock'un varlığı (`AppDelegate::shell_integration`,
    /// [`PaneLaunch::integration`]).
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
    ///
    /// `launch.initial_input` kabuğun ilk girdisi (037 Karar 6: uzak sekmede
    /// ⌘T, `AppDelegate::open_window`'un kararı); `None` → sıradan yerel kabuk.
    ///
    /// Pane pencereye takılı olmalı (`contentView`): ölçek ondan okunuyor
    /// ([`TerminalPane::sync_geometry`]); değilse hata. İkinci çağrı da hata:
    /// paket bir kez tükeniyor.
    pub(crate) fn start(&self, mtm: MainThreadMarker) -> std::io::Result<()> {
        let Some(birth) = self.ivars().birth.take() else {
            return Err(std::io::Error::other("pane ikinci kez başlatıldı"));
        };
        let (_, rows) = birth.integration;
        self.ivars().dock_rows_at_birth.set(rows);
        self.ivars().dock_rows.set(rows);
        // Grid ölçüsü pencereden türer; oturum ilk boyutuyla doğsun ki
        // shell açılışta doğru `TIOCSWINSZ` görsün.
        let Some(grid) = self.sync_geometry() else {
            return Err(std::io::Error::other("pane bir pencereye takılı değil"));
        };
        self.start_session(mtm, grid, birth)
    }

    /// Oturumu açar ve kareyi süren link'i bağlar. Sıra zorunlu: `Session`
    /// `Wake`'i ister, link `Session`'ı ister, `Waker` link'ten doğar.
    fn start_session(
        &self,
        mtm: MainThreadMarker,
        grid: Grid,
        birth: Birth,
    ) -> std::io::Result<()> {
        let Birth {
            stats,
            settings,
            theme,
            launch,
            integration: (integration, _),
        } = birth;
        let Launch {
            working_directory,
            initial_input,
        } = launch;
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
        // Sarmalayıcı kuruldu mu: entegrasyonun ortamı boş değilse kabuk
        // kimliğimizi basacak (`blocks` kademesi dock'suz ama işaretli, yani
        // `dock`'tan türetilemez). Ortam aşağıda `env`'e taşınmadan **önce**.
        let shell_marks = !integration.is_empty();
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
                terminal: settings.terminal(),
                theme,
                // Dock'un **varlığı**, payı değil: `bt-core` caret'i ona göre
                // devrediyor. Kaynağı doğum payının yuvası (`start` onu bir
                // satır önce yazdı) ve alternatif ekran habercisinin kapısı da
                // aynı yuvayı okuyor, yani ayrışamazlar.
                dock: self.ivars().dock_rows_at_birth.get() > 0,
                // Kümeleme (035) bütün pencerelerde açık, süreli koşu dahil.
                // Ayar anahtarı değil (035 Karar 2): geri alma bu tek satır.
                cluster: true,
                // Süreli koşu `open_window`'dan hep `None` alıyor (tek
                // pencere, ⌘T yok), yani sabit betikleri bundan etkilenmiyor.
                initial_input,
                shell_marks,
                // Kimlik her pencerede, süreli koşu dahil (038 Karar 8):
                // değişkenler dosya okumuyor ve jetonları oynatmıyor.
                tab_id: Some(self.ivars().tab_id.clone()),
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
        // Host işaretlerinin listesi doğumda (037 Karar 2); canlı değişimi
        // `AppDelegate::reload_settings` getiriyor ([`Self::set_host_marks`]).
        session.set_host_marks(&settings.remote_hosts);
        // Oturum yuvaya girmeden önce gelmiş bir başlık haberi boş yuva bulup
        // düşmüş olabilir; o pencereyi pencerenin `start`'ı kapatıyor
        // (`TerminalWindow::start` → `refresh_title`), bu çağrı döner dönmez.
        let view = &self.ivars().view;
        view.attach(Arc::clone(&session));
        // Fare çevirisi oturumla aynı grid'i görmeli: ölçü ve sayı yukarıdaki
        // `SessionOptions`'a gidenlerin aynısı. `resize` yolunda da aynı üçlü
        // (`refresh_geometry`) birlikte yazılıyor.
        view.set_metrics(grid, self.ivars().dock_rows.get());
        // The rhythm is the view's display link, as a timer (040 → Karar 7,
        // path (b)); it gets the frame loop to tick right below.
        let pacer = MacPacer::new(mtm, view);
        let link = DisplayLink::new(
            Arc::clone(&pacer) as Arc<dyn Pacer>,
            Rc::clone(&self.ivars().surface),
            Rc::clone(&self.ivars().renderer),
            session,
            Layout {
                cols: grid.cols,
                dock_rows: self.ivars().dock_rows.get(),
                cell: grid.cell,
            },
            stats,
            // **Yol yalnız dock'u olan pencerede kuruluyor** ve bu yapısal
            // (R5.1): dock'suz bir oturumda alternatif ekran geçişi hiçbir
            // şeyi değiştiremeyeceği için nöbet de yok. Bir koşulla
            // kapatılsaydı "hiç resize yok" iddiası bir dalın doğruluğuna
            // bağlı kalırdı.
            (self.ivars().dock_rows_at_birth.get() > 0)
                .then(|| alt_screen_notifier(self.ivars().id, self.ivars().lookup)),
        );
        pacer.attach(mtm, link.ticker());
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
        link.set_cursor_motion(settings.cursor_motion);
        link.set_glyph_fx(settings.keypress, settings.erase);
        // Açılış karesi: `Session` kirli doğar, link'i bir kez elle açıyoruz.
        link.request_frame();
        let _ = self.ivars().link.set(link);
        // Hareketi Azalt link yuvaya girdikten **sonra**: ilk değer (doğum
        // paketinin çözülmüş değeri) bu pane'in link'ine buradan iniyor,
        // sistemin bildirimi ve ayar kaydı sonradan
        // `AppDelegate::apply_reduce_motion` ile bütün pane'lere. Hermetik
        // koşuda çözülmüş değer `false` ve link o değerle doğuyor, yani çağrı
        // no-op (`DisplayLink::set_reduce_motion`).
        self.set_reduce_motion(self.ivars().reduce_motion.get());
        // Tekerleğin kipi de aynı çözülmüş girdiden (Hareketi Azalt ayarın
        // üçüncü girdisi) ve aynı sonraki yoldan (`apply_reduce_motion`).
        self.set_smooth_scroll(self.ivars().smooth_scroll.get());
        // İmlecin çizim sayıları da açılışta bir kez iniyor ve **yuvadan**
        // okunuyor, elde kalan `link`'ten değil: link o çağrıda yuvaya
        // taşındı. `set_caret_style` aynı değerde no-op, yani kayıt anı
        // yoluyla çakışmıyor.
        self.apply_caret(&settings);
        // **Odak da tohumlanıyor** ve gerekçesi aynı sıralama: pencere
        // `makeKeyAndOrderFront` ile key oluyor, yani `windowDidBecomeKey:`
        // link yuvaya girmeden **önce** düşüyor ve o çağrı sessizce atılıyor.
        // Tohumlama olmasaydı arka planda açılan bir pencerede (`open -g`,
        // login item, başka uygulama öndeyken betikten açılış) hiçbir bildirim
        // gelmez ve `focused` `true` kalırdı: odaksız pencere dolu caret
        // çizer ve blink saatini kurardı (`/code-review`).
        self.apply_focus(self.window().is_some_and(|window| window.isKeyWindow()));
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
    pub(crate) fn alt_screen_did_change(&self) {
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
        self.refresh_geometry();
    }

    /// View ▸ Bigger (Cmd +): punto farkı bir adım büyür.
    pub(crate) fn zoom_in(&self) {
        self.change_zoom(Zoom::bigger);
    }

    /// View ▸ Smaller (Cmd −): punto farkı bir adım küçülür.
    pub(crate) fn zoom_out(&self) {
        self.change_zoom(Zoom::smaller);
    }

    /// View ▸ Actual Size (Cmd 0): fark sıfırlanır, ayarın puntosu.
    pub(crate) fn zoom_reset(&self) {
        self.change_zoom(|_, _| Zoom::default());
    }

    /// Bigger, Smaller, Actual Size: bu pane'in geçici punto farkını
    /// `step` ile değiştirir ve fontu uygular. Dosyaya dokunmaz, süreli koşuda
    /// da çalışır — kullanıcının dünyasından bir şey okumuyor.
    fn change_zoom(&self, step: impl FnOnce(Zoom, &FontOptions) -> Zoom) {
        let zoom = step(self.ivars().zoom.get(), &self.ivars().font.borrow());
        self.ivars().zoom.set(zoom);
        self.apply_font();
    }

    /// Ayarın fontu değişti (`AppDelegate::reload_settings`, farkın
    /// [`TerminalPane::zoom_after_reload`]'la güncellenmesinden sonra):
    /// saklanır ve uygulanır.
    pub(crate) fn set_font(&self, font: &FontOptions) {
        self.ivars().font.replace(font.clone());
        self.apply_font();
    }

    /// Renderer'a ayarın fontunu bu pane'in geçici punto farkıyla verir;
    /// istek değiştiyse geometri yeniden kurulur
    /// ([`TerminalPane::refresh_geometry`]: atlas, grid, PTY boyutu, font
    /// yuvası).
    ///
    /// İki kapı, ikisi de gerekli: çağıranın kapısı (fark, basış) bir şeyin
    /// değiştiğini söylüyor, `set_font` renderer'ın zaten o fontu isteyip
    /// istemediğini — uçtaki basış ya da farkla aynı puntoyu yazan kayıt
    /// atlası yeniden kurdurmaz.
    fn apply_font(&self) {
        let font = self.ivars().zoom.get().apply(&self.ivars().font.borrow());
        if self.ivars().renderer.set_font(&font) {
            self.refresh_geometry();
        }
    }

    /// `[remote] hosts` değişti — desen listesi oturuma; etkin uzak host'un
    /// işareti orada yeniden çözülüyor (037 Karar 2). Sekmenin noktası
    /// pencerenin işi (`TerminalWindow::set_host_marks`).
    pub(crate) fn set_host_marks(&self, settings: &Settings) {
        if let Some(session) = self.ivars().session.get() {
            session.set_host_marks(&settings.remote_hosts);
        }
    }

    /// Terminal seçenekleri değişti — oturuma, **tamamıyla**.
    pub(crate) fn set_terminal_options(&self, settings: &Settings) {
        if let Some(session) = self.ivars().session.get() {
            session.set_terminal_options(settings.terminal());
        }
    }

    /// Temayı oturuma takas eder (aynı temada no-op, `Session::set_theme`)
    /// ve arama panelini ona boyar; panel henüz doğmadıysa ilk ⌘F'de
    /// oturumun temasıyla boyanıyor. Kromu ve sekmenin noktasını pencere
    /// boyuyor (`TerminalWindow::set_theme`, bu çağrının tek çağıranı).
    pub(crate) fn set_theme(&self, theme: Theme) {
        if let Some(session) = self.ivars().session.get() {
            session.set_theme(theme);
        }
        if let Some(bar) = self.ivars().search.get() {
            bar.paint(&theme, is_dark_background(&theme));
        }
        self.ivars().dim.paint(&theme);
    }

    /// Soluk örtüyü gösterir ya da gizler (039 Karar 7, R4.4). Karar sahibin
    /// ("odakta değil ve pencerede birden çok pane",
    /// `TerminalWindow::refresh_dim`); kare istemiyor — örtü AppKit'in.
    pub(crate) fn set_dimmed(&self, dimmed: bool) {
        self.ivars().dim.setHidden(!dimmed);
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
        self.ivars().reduce_motion.set(reduce);
        if let Some(link) = self.ivars().link.get() {
            link.set_reduce_motion(reduce);
        }
    }

    /// Kaydırmanın **çözülmüş** kipini view'a verir
    /// (`AppDelegate::smooth_scroll`). Link'e değil view'a: karar olayın
    /// sınıflamasında, `scrollWheel:`'de veriliyor ve `false` kolu bugünkü
    /// satır yolunun ta kendisi (027 Karar 5).
    pub(crate) fn set_smooth_scroll(&self, smooth: bool) {
        self.ivars().smooth_scroll.set(smooth);
        self.ivars().view.set_smooth_scroll(smooth);
    }

    /// Klavye terminale geldi (`here`) ya da arama alanına gitti —
    /// `BateriView`'ın first responder kancaları veriyor (033 R7). Odağın
    /// ikinci biti; iki bitin birleşimi `bt-gpu`'da
    /// (`DisplayLink::set_keyboard_in_terminal`). Süreli koşuda
    /// [`TerminalPane::apply_focus`]'un kapısıyla susuyor.
    ///
    /// Klavyenin gelişi sahibe de odak olayı olarak gidiyor
    /// ([`PaneHost::focused`]); gidişi gitmiyor, çünkü arama alanına geçen
    /// klavye aynı pane'de kalıyor.
    pub(crate) fn keyboard_moved(&self, here: bool) {
        if self.ivars().run.is_some() {
            return;
        }
        if let Some(link) = self.ivars().link.get() {
            link.set_keyboard_in_terminal(here);
        }
        if here {
            self.host().focused(self.ivars().id);
        }
    }

    /// En küçük pane'in boyu, nokta (039 Karar 14): ızgarası tam
    /// [`MIN_PANE_COLS`] × [`MIN_PANE_ROWS`] olan pane — [`split_into_grid`]'in
    /// tersi (sol pay + sütunlar, dock payı + satırlar). Ölçü bu pane'in
    /// hücresi ve dock payı: punto farkı pane başına. Bölmenin kapısı
    /// ([`TerminalPane::grid_fits`]) ve boyutlamanın sınırı
    /// (`SplitView::resize`) buradan. Pencereye takılı değilse `None`.
    pub(crate) fn min_size(&self) -> Option<NSSize> {
        let scale = self.window()?.backingScaleFactor();
        let cell = self.ivars().renderer.cell_metrics(scale);
        let (cell_w, cell_h) = cell.cell_px();
        let width = f64::from(cell.gutter_px()) + f64::from(MIN_PANE_COLS) * f64::from(cell_w);
        let height = f64::from(bt_gpu::dock_px(self.ivars().dock_rows.get(), cell))
            + f64::from(MIN_PANE_ROWS) * f64::from(cell_h);
        Some(NSSize::new(width / scale, height / scale))
    }

    /// Bir hücrenin boyu, nokta — klavyeyle boyutlamanın adımı
    /// (`TerminalWindow::resize_split`). Pencereye takılı değilse `None`.
    pub(crate) fn cell_size(&self) -> Option<NSSize> {
        let scale = self.window()?.backingScaleFactor();
        let (cell_w, cell_h) = self.ivars().renderer.cell_metrics(scale).cell_px();
        Some(NSSize::new(
            f64::from(cell_w) / scale,
            f64::from(cell_h) / scale,
        ))
    }

    /// `size` (nokta) boyunda bir pane'in ızgarası en küçük pane sınırını
    /// geçiyor mu (039 Karar 14) — bölmenin kapısı. Yeni bölme hücreyi ve
    /// dock payını bu pane'den devralıyor ([`TerminalPane::min_size`]).
    /// Pencereye takılı değilse `false`.
    pub(crate) fn grid_fits(&self, size: NSSize) -> bool {
        self.min_size()
            .is_some_and(|min| size.width >= min.width && size.height >= min.height)
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

    /// Uzak oturum yoklaması (036 Karar 2): koşan komutun neslini alır, ön
    /// plan grubunu yoklar ve ssh/mosh bulduysa oturuma bildirir. Dönüş iki
    /// bit ([`RemoteProbeOutcome`]): **kararsız mı** — öyleyse silah kurulu
    /// kalır ve sonraki çıktı yeniden yoklar ([`RemoteProbe`]) — ve uzak
    /// durum değişti mi (pencere başlığı tazelesin).
    ///
    /// Nesil yoklamadan **önce**: arada biten komutun cevabını
    /// `Session::set_remote` reddediyor. Okuyucu bittiyse yoklama yok
    /// ([`Self::foreground`]'ın kuralı: bayat pid'e sorulmaz).
    pub(crate) fn probe_remote(&self) -> RemoteProbeOutcome {
        let settled = RemoteProbeOutcome::default();
        let (Some(session), Some(&parent)) =
            (self.ivars().session.get(), self.ivars().shell_parent.get())
        else {
            return settled;
        };
        if !session.reader_alive() {
            return settled;
        }
        let Some(command) = session.running_command() else {
            return settled;
        };
        match jobs::remote(parent, session.child_pid(), &Libproc) {
            Probe::Undecided => RemoteProbeOutcome {
                undecided: true,
                changed: false,
            },
            Probe::Local => settled,
            Probe::Remote(target) => {
                // Satır argüman başına, okunur kaçırmayla (037 Karar 1);
                // `bt-core` kuralı ikinci kez yazmıyor, dizgiyi saklıyor.
                let line = quote::command_line(&target.argv);
                let target = RemoteTarget {
                    host: target.host,
                    kind: target.kind,
                    argv: target.argv,
                    line,
                };
                RemoteProbeOutcome {
                    undecided: false,
                    changed: session.set_remote(command, Some(&target)),
                }
            }
        }
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
    pub(crate) fn apply_focus(&self, focused: bool) {
        // Kapı `run` **bayrağına** bakıyor, `inputs()`'a değil: `inputs()`
        // `child::home()`'u argüman olarak çözüyor (passwd kaydına kadar
        // gidebilir) ve odak her uygulama geçişinde değişiyor. Bayrağın
        // kopyası bu yüzden pane'in kendisinde: uygulama delegate'ine
        // uzanmak da gerekmiyor.
        if self.ivars().run.is_some() {
            return;
        }
        if let Some(link) = self.ivars().link.get() {
            link.set_focused(focused);
        }
    }

    /// Kapanış sırasının pane'e düşen adımları — **başlatır, beklemez**.
    /// Çağıranları pencerenin `begin_close`'u — pencerenin kapanışı
    /// (`windowWillClose:`, tutamak düşüyor) ve uygulamanın kapanışı
    /// (`AppDelegate::shutdown`, bütün tutamaklar tek son tarihe kadar
    /// bekleniyor) — ile tek pane'in kapanışı (`TerminalWindow::close_pane`,
    /// tutamak düşüyor; bölme doğamadıysa `add_pane`'in geri sökümü).
    ///
    /// Sıra zorunlu: önce yükleme kuyruğu bırakılıyor (süreçler öldürülüyor
    /// ve yarım dosya siliniyor; sonucu gösterecek bir dock kalmadı), **sonra**
    /// ritim, `Waker` ve `SIGHUP` — ters sırada iptal kabuğun `SIGHUP`'ından
    /// sonra giderdi.
    ///
    /// 0. Pane'i kapanmış say ([`PaneIvars::closed`]) ve çerçeve gözlemcisini
    ///    sök: sekme çubuğu kapanırken AppKit içeriği yeniden yerleştirebiliyor
    ///    ve gözlemci kalsaydı kapanmakta olan oturum bir resize (ve düşmüş
    ///    okuyucuya yazılamayan bir `Msg::Resize`) alırdı (`/code-review`).
    /// 1. Ritmi kes (`DisplayLink::stop`): link durur, run loop'tan çıkar ve
    ///    uyandırma kapısı kapanır. Bundan sonra yeni kare istenmez.
    /// 2. `Waker`'ı `ShellWake`'ten **sök** ve burada, ana thread'de düşür
    ///    ([`ShellWake::detach`]): `ShellWake`'in son kopyası okuyucu ya da
    ///    `"PTY teardown"` thread'inde düşebilir ve orada `Waker` taşımamalı.
    /// 3. Oturumun kapanışını başlat (`Session::begin_shutdown`: `SIGHUP` +
    ///    okuyucu thread'in bitişi arkada).
    ///
    /// `DisplayLink` artık **düşürülebilir** ve pane nesnesiyle ana
    /// thread'de düşüyor: son `Waker` kopyası sökmeden sonra ya bu nesnede ya
    /// da Metal'in tamamlanma bloğunda; ikincisi ana kuyruğa senkron iş atar
    /// ama ana thread o sırada beklemede değil — pencere kapanışı beklemiyor,
    /// ⌘Q ise pencereleri bekleme bitene kadar listede tutuyor.
    ///
    /// İdempotent: ikinci çağrı [`Closing::AlreadyDone`] döner (`stop`
    /// mandallı, `detach` `take`, `begin_shutdown` `Option`, gözlemcinin
    /// sökümü kayıt yoksa no-op). Oturum hiç doğmadıysa `None` — kapanacak
    /// bir şey yok.
    pub(crate) fn begin_close(&self) -> Option<Closing> {
        self.abandon_uploads();
        self.ivars().closed.set(true);
        // SAFETY: gözlemci bu nesne, `observe_frame`'de kaydedildi; kayıt
        // yoksa no-op.
        unsafe { NSNotificationCenter::defaultCenter().removeObserver(self) };
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

    /// Pane geometrisi ya da font oynadı: layer'ı eşle, grid'i güncelle,
    /// kare iste. Aynı grid'e düşen font değişimi de yeniden çizilir:
    /// `DisplayLink::resize` kareyi koşulsuz istiyor ve kare istemek hasar
    /// bayrağını da dikiyor.
    ///
    /// Fare girdileri de burada tazeleniyor: view `PaneIvars.view`'da
    /// `Retained<BateriView>` olarak duruyor ve pane nesnesiyle birlikte
    /// gidiyor. Pane bir pencereye takılı değilse ölçek yok ve hiçbir şey
    /// yapılmıyor ([`TerminalPane::sync_geometry`]).
    pub(crate) fn refresh_geometry(&self) {
        let Some(grid) = self.sync_geometry() else {
            return;
        };
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
    ///
    /// Ölçek pane'in **penceresinden** (`NSView::window`); pane bir pencereye
    /// takılı değilse `None` — uydurulmuş bir ölçek atlası yanlış kurardı.
    fn sync_geometry(&self) -> Option<Grid> {
        let scale = self.window()?.backingScaleFactor();
        // Pane değil terminal view'ı: ikisi bugün aynı boyda ama çizilen
        // yüzey bu view'ın layer'ı, ölçü de onun olmalı.
        let view = &self.ivars().view;
        let bounds = view.bounds().size;
        let (width_px, height_px) = (bounds.width * scale, bounds.height * scale);
        // The scale is the layer owner's; the pixel size is the surface's
        // configuration (040 → Karar 8).
        self.ivars().layer.setContentsScale(scale);
        self.ivars().surface.set_size(width_px, height_px);

        // Hücre ölçüsü `bt-gpu` üzerinden `bt-atlas`'ın font metriğinden
        // geliyor ve ölçekle çarpma da orada. Burada ikinci bir yuvarlama
        // kuralı **yok**: eski `.round()` bloğu bilerek silindi. İki kural
        // yan yana dursaydı hangisinin kazandığı çağrı sırasına bağlanır ve
        // belirti bir piksellik hücre kayması, yani sessiz olurdu.
        let renderer = &self.ivars().renderer;
        let cell = renderer.cell_metrics(scale);
        self.host().post_notices(
            self.ivars().id,
            Source::Font,
            font_messages(renderer.font_notice()),
        );
        Some(split_into_grid(
            width_px,
            height_px,
            cell,
            self.ivars().dock_rows.get(),
        ))
    }
}

/// Pane düzeyindeki eylemler (R2.3) ve geçmişte arama (033) — menü
/// seçicileri ve arama panelinin kontrolleri buraya iniyor.
impl TerminalPane {
    /// Uzak durum ya da başlık değişti: önce yükleme kuyruğunun bağlantı
    /// kenarı ([`TerminalPane::check_upload_connection`]; ssh kapandıysa
    /// bekleyenler iptal — 037 Karar 7 → Kullanıcı kararı 6), sonra sahip
    /// başlığı ve sekmenin noktasını yeniden okuyor
    /// ([`PaneHost::title_changed`]).
    pub(crate) fn remote_or_title_changed(&self) {
        self.check_upload_connection();
        self.host().title_changed(self.ivars().id);
    }

    /// Edit ▸ Find ▸ Find… (⌘F): paneli açar, alanı odaklar ve metnini
    /// seçer (033 Karar 5); panel açıksa yalnız odak ve seçim. Pane'in
    /// sorgusu yoksa alan find panosunun metniyle doluyor (Karar 6).
    pub(crate) fn find(&self) {
        self.open_search(true);
    }

    /// Edit ▸ Find ▸ Find Next (⌘G): bir önceki, **daha eski** eşleşme
    /// (033 Karar 3).
    pub(crate) fn find_next(&self) {
        self.search_step(SearchDirection::Older);
    }

    /// Edit ▸ Find ▸ Find Previous (⇧⌘G): daha yeni eşleşme.
    pub(crate) fn find_previous(&self) {
        self.search_step(SearchDirection::Newer);
    }

    /// Edit ▸ Clear to Start (⌘K; 034 Karar 1): ekranı ve geçmişi
    /// siler, o anki blok kalır — `Session::clear_to_start`. Kabuğa bayt
    /// gitmiyor; alternatif ekranda öğe gri ve çağrı zaten no-op.
    pub(crate) fn clear_to_start(&self) {
        if let Some(session) = self.session() {
            session.clear_to_start();
        }
    }

    /// Edit ▸ Clear Scrollback (⌥⌘K): yalnız geçmiş —
    /// `Session::clear_scrollback`.
    pub(crate) fn clear_scrollback(&self) {
        if let Some(session) = self.session() {
            session.clear_scrollback();
        }
    }

    /// View ▸ Scroll to Top (⌘Home): geçmişin başı. `bt-core`'a yeni
    /// kaydırma API'si yok (034 Muhakeme): `scroll_page`'in
    /// `saturating_mul`'u `i32::MAX` sayfayı geçmişin ucuna kırpıyor.
    pub(crate) fn scroll_to_top(&self) {
        self.scroll_pages(i32::MAX);
    }

    /// View ▸ Scroll to Bottom (⌘End): dip — `scroll_locked` bant
    /// kuralıyla dibe iniyor.
    pub(crate) fn scroll_to_bottom(&self) {
        self.scroll_pages(-i32::MAX);
    }

    /// View ▸ Page Up (⌘PgUp): Shift+PgUp'ın yolu.
    pub(crate) fn page_up(&self) {
        self.scroll_pages(1);
    }

    /// View ▸ Page Down (⌘PgDn): Shift+PgDn'ın yolu.
    pub(crate) fn page_down(&self) {
        self.scroll_pages(-1);
    }

    /// Shell ▸ Cancel Upload (⌘.): bu pane'in bütün yükleme kuyruğu, akan
    /// kalem uzun sürdüyse önce sorarak ([`TerminalPane::request_stop`]).
    pub(crate) fn cancel_uploads(&self) {
        self.request_stop(true);
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

    /// Arama paneli — ilk çağrıda kurulur, temaya boyanır.
    fn search_bar(&self) -> &SearchBar {
        self.ivars().search.get_or_init(|| {
            // Panel pane'in içinde, Metal katmanını taşıyan view'ın kardeşi
            // (033 → R4.1; pane o kapsayıcının ta kendisi, 039 Karar 2).
            // Alanın delegesi ve kontrollerin hedefi pane — ikisi de zayıf,
            // paneli pane tutuyor.
            let bar = SearchBar::new(
                self.mtm(),
                self,
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
    /// Sorgu bu çağrıda oturuma verildiyse `true` ([`TerminalPane::apply_search`]).
    fn open_search(&self, focus: bool) -> bool {
        let bar = self.search_bar();
        if bar.query().text.is_empty()
            && let Some(text) = find_pasteboard_text()
        {
            bar.set_text(&text);
        }
        bar.show(!self.ivars().reduce_motion.get());
        if focus && let Some(window) = self.window() {
            window.makeFirstResponder(Some(bar.field()));
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
            session.search_reveal(self.search_cover(), self.ivars().smooth_scroll.get())
        } else {
            SearchReport::default()
        };
        bar.set_count(status, report);
        self.kick_search();
        true
    }

    /// Sayım dizininin sürücüsünü kurar (033 phase-5, Karar 2-B): ana
    /// kuyrukta bir sonraki turda bir parça. Zaten kuruluysa, panel kapalıysa
    /// ya da sorgu sayılacak bir desen değilse no-op.
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

    /// Sürücünün bir turu ana kuyruğa: pane kimlikle bulunuyor
    /// (`ShellWake`'in örüntüsü), kapanan pane'de iş düşüyor.
    fn schedule_search_chunk(&self) {
        let (id, lookup) = (self.ivars().id, self.ivars().lookup);
        DispatchQueue::main().exec_async(move || {
            // audit: ana kuyrukta koşan blok tanımı gereği ana thread'dedir.
            let mtm = MainThreadMarker::new().expect("ana kuyruk ana thread'dir");
            if let Some(pane) = lookup(mtm, id) {
                pane.search_chunk();
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
        let report = session.search_next(
            direction,
            self.search_cover(),
            self.ivars().smooth_scroll.get(),
        );
        bar.set_count(status, report);
        if !report.complete {
            self.kick_search();
        }
    }

    /// Esc ve kapatma düğmesi (033 Karar 5): panel gider, **pencere yerinde
    /// kalır**, geçerli eşleşme ızgaranın seçimi olur ve klavye terminale
    /// döner. Sorgu alanda kalıyor (Karar 6).
    pub(crate) fn close_search(&self) {
        let Some(bar) = self.ivars().search.get() else {
            return;
        };
        bar.hide(!self.ivars().reduce_motion.get());
        bar.forget_applied();
        if let Some(session) = self.session() {
            session.select_search_match();
            session.clear_search();
        }
        self.ivars().search_status.set(SearchStatus::Empty);
        if let Some(window) = self.window() {
            window.makeFirstResponder(Some(self.view()));
        }
    }

    /// Edit ▸ Find ▸ Use Selection for Find (⌘E; 033 Karar 6): seçimin ilk
    /// satırı (ızgara ya da dock) sorgu olur — regex kipinde kaçırılarak —,
    /// find panosuna yazılır ve panel alanı odaklanmış açılır.
    pub(crate) fn use_selection_for_find(&self) {
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

    /// Find Next/Previous'ın kapısı: pane'in sorgusu ya da find panosunda
    /// metin var mı.
    fn has_query(&self) -> bool {
        self.ivars()
            .search
            .get()
            .is_some_and(|bar| !bar.query().text.is_empty())
            || find_pasteboard_text().is_some()
    }

    /// Panelin örttüğü hücreler; panel kapalıysa hiçbiri. Panelin
    /// koordinatı pane'in (kapsayıcısı o).
    fn search_cover(&self) -> SearchCover {
        let Some(bar) = self.ivars().search.get().filter(|bar| bar.is_shown()) else {
            return SearchCover::default();
        };
        let view = self.view();
        view.search_cover(view.convertRect_fromView(bar.resting_frame(), Some(self)))
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

    /// Kuyruğun gönderilen ve toplam baytı; kuyruk yoksa `None` — sahibin
    /// Dock simgesi toplamının girdisi ([`PaneHost::uploads_changed`]).
    pub(crate) fn upload_totals(&self) -> Option<(u64, u64)> {
        self.ivars().uploads.borrow().totals()
    }

    /// Kuyruk sürüyor mu (Dock simgesi, `cancelUpload:`'ın kapısı).
    pub(crate) fn upload_active(&self) -> bool {
        self.ivars().uploads.borrow().active()
    }

    /// Başlığın `↑ N% · ` önekinin yüzdesi; yükleme akmıyorsa `None`
    /// (`upload::titled`).
    pub(crate) fn upload_title_percent(&self) -> Option<u8> {
        self.ivars().uploads.borrow().title_percent()
    }
}

/// Yeni bir sekme kimliği, `NSUUID`'den (038 Karar 2).
fn new_tab_id() -> TabId {
    // `UUIDString` kanonik 8-4-4-4-12 biçimini veriyor; `parse` onu
    // reddederse kusur `bt-core`'un sözleşmesinde, bu satırda değil.
    TabId::parse(&NSUUID::new().UUIDString().to_string())
        .expect("NSUUID'nin UUIDString'i kanonik UUID olmalı")
}

#[cfg(test)]
mod tests {
    #[test]
    fn tab_ids_are_canonical_and_distinct() {
        let (a, b) = (super::new_tab_id(), super::new_tab_id());
        assert_ne!(a, b, "iki NSUUID kimliği ayrı olmalı");
        assert_eq!(bt_core::TabId::from_url(&a.url()), Some(a));
    }

    /// Sahte sahip: olayları kimlikleriyle kaydediyor — pencere yok, pano
    /// yok (039 phase-2).
    #[derive(Default)]
    struct FakeHost(std::cell::RefCell<Vec<(u64, String)>>);

    impl super::PaneHost for FakeHost {
        fn title_changed(&self, pane: u64) {
            self.0.borrow_mut().push((pane, "title".into()));
        }
        fn focused(&self, pane: u64) {
            self.0.borrow_mut().push((pane, "focused".into()));
        }
        fn shell_exited(&self, pane: u64) {
            self.0.borrow_mut().push((pane, "exit".into()));
        }
        fn uploads_changed(&self, pane: u64) {
            self.0.borrow_mut().push((pane, "uploads".into()));
        }
        fn notify(&self, pane: u64, title: &str, _body: &str) {
            self.0.borrow_mut().push((pane, format!("notify {title}")));
        }
        fn post_notices(&self, pane: u64, _source: crate::notices::Source, _messages: Vec<String>) {
            self.0.borrow_mut().push((pane, "notices".into()));
        }
        fn copy_to_clipboard(&self, pane: u64, text: String) {
            self.0.borrow_mut().push((pane, format!("copy {text}")));
        }
    }

    #[test]
    fn title_and_copy_events_reach_the_host_with_the_pane_id() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let host = FakeHost::default();
        // Başlık: bayrak iniyor, olay sahibe pane'in kimliğiyle.
        // Pane'in kenarı bayrak indikten sonra koşuyor.
        let pending = AtomicBool::new(true);
        let edge_saw = std::cell::Cell::new(true);
        super::announce_title(
            &pending,
            || edge_saw.set(pending.load(Ordering::Acquire)),
            &host,
            7,
        );
        assert!(!pending.load(Ordering::Acquire));
        assert!(!edge_saw.get(), "kenar bayrak inmeden koştu");
        // Kopya: yuvadaki metin sahibe gidiyor, genel panoya değil; boş yuva
        // olay doğurmuyor.
        let copy = crate::clipboard::PendingCopy::default();
        assert!(copy.put("osc52".into()));
        super::announce_copy(&copy, &host, 7);
        super::announce_copy(&copy, &host, 7);
        assert_eq!(
            *host.0.borrow(),
            vec![(7, "title".to_owned()), (7, "copy osc52".to_owned())]
        );
    }

    #[test]
    fn remote_probe_repeats_only_while_undecided() {
        use super::RemoteProbe;
        let probe = RemoteProbe::default();
        // Silahsızken çıktı yoklama atmıyor.
        assert!(!probe.output());
        // `C` kenarı silahı kurar ve tek bir iş atar; iş beklerken gelen
        // çıktı ikinci bir iş atmıyor.
        assert!(probe.command_started());
        assert!(!probe.output());
        // İş başlıyor, yoklama kararsız: silah geri kurulur, sonraki çıktı
        // tekrar atar.
        assert!(probe.begin());
        probe.rearm();
        assert!(probe.output());
        // İş başlıyor, cevap kesin: silah inik kalır, çıktı atmıyor.
        assert!(probe.begin());
        assert!(!probe.output());
        // Kesin cevap yoklanırken gelen yeni `C` ezilmiyor.
        assert!(probe.command_started());
        assert!(probe.begin());
        assert!(probe.command_started());
        assert!(probe.begin(), "yeni komut yoklanmalı");
        // Silah inikken kuyruğa düşmüş bir iş yoklamıyor.
        assert!(!probe.begin());
    }
}
