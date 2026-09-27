//! Terminal pane'i: tek bir terminal oturumunun **bütün çekirdeği** — oturum,
//! kareyi süren display link, kendi `Renderer`'ı, `CAMetalLayer` yüzeyi,
//! `BateriView`, kabuğun uyandırma ucu (`ShellWake`), dock payı, geçici punto
//! farkı ve sekme kimliği (039 Karar 1–2).
//!
//! `TerminalPane` bir `NSView` alt sınıfı ve bugünkü içerik kapsayıcısının ta
//! kendisi (033 → R4.1): `BateriView` onu autoresizing'le dolduran çocuğu,
//! arama paneli Metal katmanının kardeşi olarak onun içinde yüzüyor. Pencere
//! (`window::TerminalWindow`) pane'i `contentView` yapıyor ve krom, başlık,
//! sekme, kapatma sorusu gibi **pencereye** ait işleri tutuyor; geometri,
//! örtülme ve odak pencereden buraya dağıtılıyor.
//!
//! **Renderer pane başına** (039 Karar 5; 026 → Karar 2a): atlasın anahtarı
//! ölçek ve punto içeriyor, punto farkı ise pane'in.
//!
//! Bu phase'de pane hâlâ `app::delegate`'e uzanıyor (ayarlar, alt başlık,
//! ölçüm defteri); sahip arayüzü (`PaneLaunch`/`PaneHost`) phase-2'nin işi.

use std::cell::{Cell, OnceCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use bt_core::{FontOptions, RemoteTarget, Session, SessionOptions, Settings, TabId, Theme, Wake};
use bt_core::{load_shell, smoke_shell};
use bt_gpu::{DisplayLink, GpuError, Layout, Renderer, Surface, Waker};
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSAutoresizingMaskOptions, NSPasteboard, NSView,
    NSViewFrameDidChangeNotification,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObjectProtocol, NSRect, NSUUID};

use crate::app::{self, AppDelegate, Grid, split_into_grid};
use crate::child;
use crate::clipboard::PendingCopy;
use crate::jobs::{self, Foreground, Libproc, Probe, ShellParent};
use crate::notices::{Source, font_messages};
use crate::quote;
use crate::view::BateriView;
use crate::window::{Closing, Launch};
use crate::zoom::Zoom;
use crate::{Run, Workload};

/// `bt-core`'un uyandırma ucu — pane başına bir tane, oturumuyla birlikte.
///
/// `Session::spawn` `Wake`'i link'ten **önce** ister, `Waker` ise link'ten
/// sonra doğar; boşluğu yuvanın `None`'ı kapatır. Kaçan kare yok: açılış
/// karesi zaten elle isteniyor ve o ana kadar okunmuş her bayt hasar
/// bayrağında birikmiş olur.
struct ShellWake {
    /// Pane'in kimliği: ana kuyruk işleri pane'i (ya da pane'in penceresini)
    /// bununla buluyor (`AppDelegate::pane`, alternatif ekran habercisinin
    /// örüntüsü) — `Session`'a ya da pane'e referans tutmak `wake.rs`'in
    /// Sahiplik çemberini kapatırdı.
    id: u64,
    /// Süreli koşu mu: `child_exit` iki yola ayrılıyor ([`Wake::child_exit`]'in
    /// gövdesi). `AppDelegate`'inkinin kopyası; okuyucu thread'den uygulama
    /// delegate'ine uzanılamaz.
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
        let id = self.id;
        DispatchQueue::main().exec_async(move || {
            if !probe.begin() {
                return;
            }
            // audit: ana kuyrukta koşan blok tanımı gereği ana thread'dedir.
            let mtm = MainThreadMarker::new().expect("ana kuyruk ana thread'dir");
            // Pane bu arada kapandıysa yoklanacak bir kabuk da yok.
            let Some(app) = app::delegate(mtm) else {
                return;
            };
            let Some(pane) = app.pane(id) else {
                return;
            };
            let outcome = pane.probe_remote();
            // Uzak durumun kenarı pencerenin başlığını ve sekmenin noktasını
            // tazeliyor (`TerminalWindow::refresh_title`).
            if outcome.changed
                && let Some(window) = app.window_of_pane(id)
            {
                window.refresh_title();
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
        let (timed, id) = (self.timed, self.id);
        DispatchQueue::main().exec_async(move || {
            // audit: ana kuyrukta koşan blok tanımı gereği ana thread'dedir.
            let mtm = MainThreadMarker::new().expect("ana kuyruk ana thread'dir");
            if timed {
                NSApplication::sharedApplication(mtm).terminate(None);
                return;
            }
            // Pane bu arada kapandıysa (⌘W'nin `SIGHUP`'ı kabuğu öldürdü
            // ve haber sonradan geldi) kapatacak bir şey yok.
            if let Some(window) = app::delegate(mtm).and_then(|app| app.window_of_pane(id)) {
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
            // Pane bu arada kapanmışsa yazacak bir başlık da yok.
            if let Some(window) = app::delegate(mtm).and_then(|app| app.window_of_pane(id)) {
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
            // Arka sekmede de işliyor: haber kare yoluna bağlı değil. Pane
            // bu arada kapandıysa sayacak bir şey yok.
            if let Some(window) = app::delegate(mtm).and_then(|app| app.window_of_pane(id)) {
                window.kick_search();
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
/// kimliği (`id`) yakalıyor, `AppDelegate`'in listesinden pane'i buluyor ve
/// bulamazsa düşüyor — pane o arada kapanmışsa boyutlandıracak bir şey de
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
fn alt_screen_notifier(id: u64) -> Box<dyn Fn()> {
    Box::new(move || {
        DispatchQueue::main().exec_async(move || {
            // audit: ana kuyrukta koşan blok tanımı gereği ana thread'dedir.
            let mtm = MainThreadMarker::new().expect("ana kuyruk ana thread'dir");
            let Some(app) = app::delegate(mtm) else {
                return;
            };
            if let Some(pane) = app.pane(id) {
                pane.alt_screen_did_change(&app);
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
    /// Süreli koşunun tarifi, `AppDelegate`'inkinin kopyası (`Copy`): odak
    /// yolu onu her uygulama geçişinde soruyor ve uygulama delegate'ine
    /// uzanmadan cevaplayabilmeli ([`TerminalPane::apply_focus`]).
    run: Option<Run>,
    /// `Rc`: renderer ana thread'e çivili (bkz. `bt_gpu::DisplayLink`) ve
    /// link de bir kopya tutuyor.
    renderer: Rc<Renderer>,
    surface: Surface,
    /// Fare çevirisinin girdileri pane boyuyla tazeleniyor (`set_metrics`);
    /// geometrinin kaynağı da bu view (`sync_geometry`).
    view: Retained<BateriView>,
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
            if let Some(app) = app::delegate(self.mtm()) {
                self.refresh_geometry(&app);
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
    /// metallib yoksa pane'in çizebileceği bir şey de yok.
    pub(crate) fn new(
        mtm: MainThreadMarker,
        id: u64,
        run: Option<Run>,
        frame: NSRect,
    ) -> Result<Retained<Self>, GpuError> {
        let renderer = Rc::new(Renderer::system_default()?);
        let surface = renderer.surface();
        let view = BateriView::new(mtm, frame);
        // Sıra önemli: önce layer, sonra wantsLayer — tersi AppKit'e kendi
        // layer'ını kurdurur ve CAMetalLayer düşer.
        view.setLayer(Some(surface.ca_layer()));
        view.setWantsLayer(true);
        let this = Self::alloc(mtm).set_ivars(PaneIvars {
            id,
            run,
            renderer,
            surface,
            view: view.clone(),
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
                remote_probe: Arc::default(),
            }),
            zoom: Cell::new(Zoom::default()),
            // Açılışta dock yok: kararı `start` veriyor ve geometriyi ondan
            // sonra hesaplıyor.
            dock_rows: Cell::new(0),
            dock_rows_at_birth: Cell::new(0),
            tab_id: new_tab_id(),
            closed: Cell::new(false),
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

    /// Devralınan punto farkı; oturum doğmadan, [`TerminalPane::request_font`]'tan
    /// önce yazılır ki ilk atlas büyütülmüş puntoyla açılsın.
    pub(crate) fn set_zoom(&self, zoom: Zoom) {
        self.ivars().zoom.set(zoom);
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
    /// [`TerminalPane::apply_font`] uygular.
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
    ///
    /// `launch.initial_input` kabuğun ilk girdisi (037 Karar 6: uzak sekmede
    /// ⌘T, `AppDelegate::open_window`'un kararı); `None` → sıradan yerel kabuk.
    ///
    /// Pane pencereye takılı olmalı (`contentView`): ölçek ondan okunuyor
    /// ([`TerminalPane::sync_geometry`]); değilse hata.
    pub(crate) fn start(
        &self,
        app: &AppDelegate,
        mtm: MainThreadMarker,
        theme: Theme,
        launch: Launch,
    ) -> std::io::Result<()> {
        let (integration, birth) = app.shell_integration();
        self.ivars().dock_rows_at_birth.set(birth);
        self.ivars().dock_rows.set(birth);
        // Grid ölçüsü pencereden türer; oturum ilk boyutuyla doğsun ki
        // shell açılışta doğru `TIOCSWINSZ` görsün.
        let Some(grid) = self.sync_geometry(app) else {
            return Err(std::io::Error::other("pane bir pencereye takılı değil"));
        };
        self.start_session(app, mtm, grid, theme, integration, launch)
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
        launch: Launch,
    ) -> std::io::Result<()> {
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
        session.set_host_marks(&app.settings().remote_hosts);
        // Oturum yuvaya girmeden önce gelmiş bir başlık haberi boş yuva bulup
        // düşmüş olabilir; o pencereyi pencerenin `start`'ı kapatıyor
        // (`TerminalWindow::start` → `refresh_title`), bu çağrı döner dönmez.
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
    pub(crate) fn alt_screen_did_change(&self, app: &AppDelegate) {
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

    /// Bigger, Smaller, Actual Size: bu pane'in geçici punto farkını
    /// `step` ile değiştirir ve fontu uygular. Dosyaya dokunmaz, süreli koşuda
    /// da çalışır — kullanıcının dünyasından bir şey okumuyor.
    pub(crate) fn change_zoom(&self, step: impl FnOnce(Zoom, &FontOptions) -> Zoom) {
        let Some(app) = app::delegate(self.mtm()) else {
            return;
        };
        let zoom = step(self.ivars().zoom.get(), &app.settings().font);
        self.ivars().zoom.set(zoom);
        self.apply_font(&app);
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
    pub(crate) fn apply_font(&self, app: &AppDelegate) {
        let font = self.ivars().zoom.get().apply(&app.settings().font);
        if self.ivars().renderer.set_font(&font) {
            self.refresh_geometry(app);
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

    /// Temayı oturuma takas eder (aynı temada no-op, `Session::set_theme`).
    /// Kromu, sekmenin noktasını ve arama panelini pencere boyuyor
    /// (`TerminalWindow::set_theme`, bu çağrının tek çağıranı).
    pub(crate) fn set_theme(&self, theme: Theme) {
        if let Some(session) = self.ivars().session.get() {
            session.set_theme(theme);
        }
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
    /// [`TerminalPane::apply_focus`]'un kapısıyla susuyor.
    pub(crate) fn keyboard_moved(&self, here: bool) {
        if self.ivars().run.is_some() {
            return;
        }
        if let Some(link) = self.ivars().link.get() {
            link.set_keyboard_in_terminal(here);
        }
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
    /// Çağıranı pencerenin `begin_close`'u (yükleme kuyruğunu bıraktıktan
    /// sonra); onun iki çağıranı var: pencerenin kapanışı (`windowWillClose:`,
    /// tutamak düşüyor) ve uygulamanın kapanışı (`AppDelegate::shutdown`,
    /// bütün tutamaklar tek son tarihe kadar bekleniyor).
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
    pub(crate) fn refresh_geometry(&self, app: &AppDelegate) {
        let Some(grid) = self.sync_geometry(app) else {
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
    fn sync_geometry(&self, app: &AppDelegate) -> Option<Grid> {
        let scale = self.window()?.backingScaleFactor();
        // Pane değil terminal view'ı: ikisi bugün aynı boyda ama çizilen
        // yüzey bu view'ın layer'ı, ölçü de onun olmalı.
        let view = &self.ivars().view;
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
        Some(split_into_grid(
            width_px,
            height_px,
            cell,
            self.ivars().dock_rows.get(),
        ))
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
