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

use std::cell::{Cell, OnceCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use bt_core::{FontOptions, Session, SessionOptions, Settings, Teardown, Theme, Wake};
use bt_core::{load_shell, smoke_shell};
use bt_gpu::{DisplayLink, GpuError, Layout, Renderer, Surface, Waker};
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSPasteboard, NSWindow, NSWindowDelegate,
    NSWindowOcclusionState, NSWindowStyleMask,
};
use objc2_foundation::{
    NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, ns_string,
};

use crate::app::{self, AppDelegate, Grid, split_into_grid};
use crate::child;
use crate::clipboard::PendingCopy;
use crate::notices::{Source, font_messages};
use crate::view::BateriView;
use crate::zoom::Zoom;
use crate::{Run, Workload};

/// `bt-core`'un uyandırma ucu — pencere başına bir tane, oturumuyla birlikte.
///
/// `Session::spawn` `Wake`'i link'ten **önce** ister, `Waker` ise link'ten
/// sonra doğar; boşluğu `OnceLock` kapatır. Kaçan kare yok: açılış karesi
/// zaten elle isteniyor ve o ana kadar okunmuş her bayt hasar bayrağında
/// birikmiş olur.
struct ShellWake {
    /// Pencerenin kimliği: ana kuyruk işleri pencereyi listeden bununla
    /// buluyor (alternatif ekran habercisinin örüntüsü) — `Session`'a ya da
    /// pencereye referans tutmak `wake.rs`'in Sahiplik çemberini kapatırdı.
    id: u64,
    waker: OnceLock<Waker>,
    /// OSC 52'nin ana kuyruğa bekleyen metni. `Arc`, çünkü ana kuyruğun işi
    /// `'static` ister ve `Wake`'in çağrısı yalnız `&self` veriyor; iş
    /// `ShellWake`'i değil yalnız yuvayı tutar.
    pending_copy: Arc<PendingCopy>,
    /// Başlık işi ana kuyrukta bekliyor mu — kuyruğa **en çok bir** iş
    /// (`PendingCopy`'nin örüntüsü, yük yerine bayrak: başlığın kendisi
    /// oturumda, iş onu okuyor).
    title_pending: Arc<AtomicBool>,
}

impl Wake for ShellWake {
    fn wake(&self) {
        // Okuyucu thread; `Term` kilidi tutuluyor olabilir. Tek iş: ana
        // kuyruğa "link'i aç" işini at, hemen dön.
        if let Some(waker) = self.waker.get() {
            waker.wake();
        }
    }

    fn child_exit(&self, _code: Option<i32>) {
        // Shell gitti, terminal penceresinin dayanağı kalmadı: uygulama
        // sonlanır. Sonlanmayı `terminate:` yürütüyor ki kapanış tek kapıdan
        // geçsin — kırmızı düğme, `exit` ve duman deadline'ı aynı
        // `applicationWillTerminate:`'a varır.
        //
        // Ana kuyruğa atılmasının iki sebebi var ve ikisi de zorunlu: AppKit
        // ana thread ister, ve bu çağrı **okuyucu thread'de** geliyor —
        // `shutdown()`'a giden senkron bir yol okuyucu thread'i kendi
        // kapanışında bekletirdi (`wake.rs` → Sahiplik). Kapanış sınırlı
        // olduğundan bu artık `EDEADLK` paniği değil, yarım saniyelik bir
        // durma ve hiç tamamlanmayan bir kapanış — yasak aynı kalıyor.
        //
        // **Bilinen sınır:** shell'in son çıktısı ekrana gelmeyebilir.
        // alacritty sırayı `ChildExit` → `Wakeup` diye kuruyor, yani buraya
        // geldiğimizde son bayt henüz çizilmemiş olabilir; `terminate:` de
        // araya bir vsync girmeden koşar. Garanti etmek ya sihirli bir
        // gecikme ya da display link'e "hasar tükendi, şimdi çık" semantiği
        // eklemek olurdu — ikincisi renderer'a terminal bilgisi sokar.
        // `bateri -e cmd` yolu geldiğinde `drain_on_exit` ile birlikte
        // tasarlanacak (`.tasks/002-vt-motoru/phase-4.md` → Uygulama Notları).
        DispatchQueue::main().exec_async(|| {
            // audit: ana kuyrukta koşan blok tanımı gereği ana thread'dedir.
            let mtm = MainThreadMarker::new().expect("ana kuyruk ana thread'dir");
            NSApplication::sharedApplication(mtm).terminate(None);
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
    /// view'a `contentView`'dan (`NSView`) inilemiyor, o yüzden burada tutuluyor.
    view: Retained<BateriView>,
    link: OnceCell<DisplayLink>,
    /// Kapanış sırasının ikinci adımı buradan çağrılır; `DisplayLink` de bir
    /// kopya tutuyor ama oraya `stop()`'tan sonra uzanmak yanlış olurdu.
    session: OnceCell<Arc<Session>>,
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
        #[unsafe(method(windowDidResize:))]
        fn window_did_resize(&self, _n: &NSNotification) {
            if let Some(app) = app::delegate(self.mtm()) {
                self.refresh_geometry(&app);
            }
        }

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
    }

    // **Pencereye ait eylemler** burada, uygulama geneline yayılanlar
    // (`settingsDidChange:`, `appearanceDidChange:`, tema, `openSettings:`)
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
    }
);

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
        window.setContentView(Some(&view));
        window.setTitle(ns_string!("bateri"));
        // Düğmesiz hareket olayları varsayılan **kapalı**; fare raporu
        // isteyen uygulama (1003) onlarsız işaretçiyi hiç göremez.
        // `NSTrackingArea` gerekmiyor: o yalnız `mouseEntered:`/
        // `mouseExited:` ve cursor rect için, ikisi de istenmiyor, ve
        // view zaten first responder — pencere seviyesindeki
        // `mouseMoved:` ona geliyor. Kipe göre açıp kapamak kipi
        // `bt-shell`'e yayınlamayı isterdi
        // (`.tasks/020-fare-raporlama/discussion.md` → Karar 4).
        window.setAcceptsMouseMovedEvents(true);
        // Klavyenin PTY'ye varan yolu buradan başlıyor. `contentView`
        // otomatik first responder DEĞİLDİR; bu satır olmadan pencere
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
            wake: Arc::new(ShellWake {
                id,
                waker: OnceLock::new(),
                pending_copy: Arc::default(),
                title_pending: Arc::default(),
            }),
            zoom: Cell::new(Zoom::default()),
            // Açılışta dock yok: kararı `start` veriyor ve geometriyi ondan
            // sonra hesaplıyor.
            dock_rows: Cell::new(0),
            dock_rows_at_birth: Cell::new(0),
        });
        // SAFETY: NSObject'in init'i argümansızdır ve ivar'lar set edildi.
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        // Delegate bağlanmadan önce ivar'lar dolu: arada düşen bir pencere
        // bildirimi geometriyi boş bulup bayat boyutla çizmesin. Delegate
        // özelliği zayıf; sahibi `AppDelegate`'in pencere listesi.
        window.setDelegate(Some(ProtocolObject::from_ref(&*this)));
        Ok(this)
    }

    /// Pencereyi ortalayıp öne alır.
    pub(crate) fn show(&self) {
        self.ivars().window.center();
        self.ivars().window.makeKeyAndOrderFront(None);
    }

    pub(crate) fn id(&self) -> u64 {
        self.ivars().id
    }

    /// Rapor yolu sayaçlarını buradan okuyor (`AppDelegate::report_and_exit`).
    pub(crate) fn renderer(&self) -> &Renderer {
        &self.ivars().renderer
    }

    pub(crate) fn link(&self) -> Option<&DisplayLink> {
        self.ivars().link.get()
    }

    pub(crate) fn session(&self) -> Option<&Arc<Session>> {
        self.ivars().session.get()
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
    pub(crate) fn start(&self, app: &AppDelegate, mtm: MainThreadMarker, theme: Theme) {
        let (integration, birth) = app.shell_integration();
        self.ivars().dock_rows_at_birth.set(birth);
        self.ivars().dock_rows.set(birth);
        // Grid ölçüsü pencereden türer; oturum ilk boyutuyla doğsun ki
        // shell açılışta doğru `TIOCSWINSZ` görsün.
        // audit: pencere ve contentView kurucuda kuruldu; `None` dönmesi
        // programlama hatası olurdu ve yedek bir ölçü uydurmak hücre boyutu
        // için ikinci bir kaynak doğururdu — tek kaynak `Renderer::cell_metrics`.
        let grid = self
            .sync_geometry(app)
            .expect("pencere ve contentView kuruldu");
        self.start_session(app, mtm, grid, theme, integration, birth > 0);
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
        dock: bool,
    ) {
        let session = Session::spawn(
            SessionOptions {
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
                command: match self.ivars().run {
                    None => child::login_command(),
                    Some(run) => Some(match run.workload {
                        Workload::Smoke => smoke_shell(),
                        // Yükün süresi deadline'la aynı: kısa kalırsa pencere
                        // koşunun kuyruğunda boşa düşer ve ölçüm boşta kare
                        // örnekler. Süresiz yük artık **temsil edilemiyor** —
                        // `Run` süreyi yükün yanında taşıyor, o yüzden eski
                        // `unwrap_or(0)` ve onu savunan `debug_assert` düştü.
                        Workload::Load => load_shell(run.seconds),
                    }),
                },
                // Dizin ve yerel **her** oturumda aynı kuralla, süreli koşu
                // dahil: karar tek kollu (`discussion.md` → Karar 6 eki,
                // "istisnasız") ve iki sabit betik de dizine ve yerele bağlı
                // değil — `printf` ile `sleep`, `date` ile `printf`; yolları
                // mutlak ya da `PATH`'ten, çıktıları ASCII.
                working_directory: child::working_directory(),
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
                // devrediyor. Aynı doğum kararının öteki tüketicisi
                // `dock_rows_at_birth`; ikisi çağıranda tek ifadeden çıkıyor
                // (`start`'ın `birth`'ü), yani ayrışamazlar.
                dock,
            },
            Arc::clone(&self.ivars().wake) as Arc<dyn Wake>,
        );
        let session = match session {
            Ok(session) => Arc::new(session),
            // `didFinishLaunching` hata döndüremez ve shell'siz bir terminal
            // penceresi boş bir kutudur: sessizce açık kalmaktansa çık.
            Err(e) => {
                eprintln!("bateri: shell başlatılamadı: {e}");
                std::process::exit(1);
            }
        };
        // Kapanış sırası oturuma link üzerinden değil buradan uzanır, klavye
        // de kendi kopyasını tutar; üçü de ana thread'de yaşıyor, yani son
        // referansın nerede düşeceği belli (bkz. `shutdown`).
        let _ = self.ivars().session.set(Arc::clone(&session));
        // Oturum yuvaya girmeden önce gelmiş bir başlık haberi `refresh_title`'da
        // boş yuva bulup düşmüş olabilir; bir kez elle okumak o pencereyi
        // kapatıyor (değişmemişse aynı `bateri`'yi yazar).
        self.refresh_title();
        let view = &self.ivars().view;
        view.attach(Arc::clone(&session));
        // Fare çevirisi oturumla aynı grid'i görmeli: ölçü ve sayı yukarıdaki
        // `SessionOptions`'a gidenlerin aynısı. `resize` yolunda da aynı üçlü
        // (`refresh_geometry`) birlikte yazılıyor.
        view.set_metrics(grid);
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
            self.ivars().wake.waker.set(link.waker()).is_ok(),
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
        link.set_cursor_motion(app.settings().cursor_motion);
        // Açılış karesi: `Session` kirli doğar, link'i bir kez elle açıyoruz.
        link.request_frame();
        let _ = self.ivars().link.set(link);
        // Hareketi Azalt link yuvaya girdikten **sonra**: ilk değer bu
        // pencerenin link'ine buradan iniyor, sistemin bildirimi ve ayar
        // kaydı sonradan `AppDelegate::apply_reduce_motion` ile bütün
        // pencerelere. Hermetik koşuda çözülmüş değer `false` ve link o
        // değerle doğuyor, yani çağrı no-op (`DisplayLink::set_reduce_motion`).
        self.set_reduce_motion(app.reduce_motion());
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

    /// Temayı oturuma takas eder; aynı temada no-op (`Session::set_theme`).
    pub(crate) fn set_theme(&self, theme: Theme) {
        if let Some(session) = self.ivars().session.get() {
            session.set_theme(theme);
        }
    }

    /// İmlecin stili link'e gidiyor, oturuma değil: hangi kareyi çizeceğimizi
    /// değil **nasıl** çizeceğimizi değiştiriyor.
    pub(crate) fn set_cursor_motion(&self, settings: &Settings) {
        if let Some(link) = self.ivars().link.get() {
            link.set_cursor_motion(settings.cursor_motion);
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

    /// Kapanış sırasının pencereye düşen iki adımı; sırayı ve gerekçesini
    /// `AppDelegate::shutdown` anlatıyor.
    ///
    /// 1. Ritmi kes (`DisplayLink::stop`): link durur, run loop'tan çıkar ve
    ///    uyandırma kapısı kapanır. Bundan sonra yeni kare istenmez.
    /// 2. Oturumu kapat: `SIGHUP` + okuyucu thread'in bitişi, **sınırlı**
    ///    bekleyerek (`bt-core`'un `SHUTDOWN_GRACE`'i).
    ///
    /// `DisplayLink` bilerek **düşürülmüyor**, yalnız durduruluyor (gerekçe
    /// `AppDelegate::shutdown`'da). Sonuç raporu besliyor (`kapanis=`): oturum
    /// hiç doğmadıysa `None` ve o da bir cevap — kapanacak bir şey yoktu.
    pub(crate) fn shutdown(&self) -> Option<Teardown> {
        if let Some(link) = self.ivars().link.get() {
            link.stop();
        }
        self.ivars().session.get().map(|session| session.shutdown())
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
        let Some(grid) = self.sync_geometry(app) else {
            return;
        };
        self.ivars().view.set_metrics(grid);
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
    fn sync_geometry(&self, app: &AppDelegate) -> Option<Grid> {
        let window = &self.ivars().window;
        let view = window.contentView()?;
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
        Some(split_into_grid(
            width_px,
            height_px,
            cell,
            self.ivars().dock_rows.get(),
        ))
    }
}
