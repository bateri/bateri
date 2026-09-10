//! Uygulama delegate'i: pencereyi açar, `CAMetalLayer`'ı view'a takar,
//! shell oturumunu başlatır, kareyi süren display link'i bağlar ve kapanış
//! sırasını yürütür. Çizim çağrısı burada **yok**, bu dosyanın işi bağlamak.

use std::cell::OnceCell;
use std::sync::{Arc, OnceLock};

use bt_core::{Session, SessionOptions, Wake, smoke_shell};
use bt_gpu::{DisplayLink, Renderer, Surface, Waker};
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationDelegate, NSBackingStoreType, NSWindow, NSWindowDelegate,
    NSWindowOcclusionState, NSWindowStyleMask,
};
use objc2_foundation::{
    NSArray, NSNotification, NSObject, NSObjectNSDelayedPerforming, NSObjectProtocol, NSPoint,
    NSRect, NSRunLoopCommonModes, NSSize, ns_string,
};

use crate::Options;
use crate::view::BateriView;

/// Hücrenin @1x piksel boyutu. Gerçek font metriği `bt-atlas` ile (003)
/// gelene kadar yer tutucu: grid ölçüsü ve PTY'ye giden `TIOCSWINSZ` bundan
/// türer, yani sayı yanlışsa yalnız hücreler yanlış boyutta olur — akış
/// doğru kalır.
const CELL_PX: (f64, f64) = (9.0, 18.0);

/// Kaydırma geçmişi satır sayısı; ayar dosyası (00X) gelene kadar sabit.
const SCROLLBACK: usize = 10_000;

/// Pencere geometrisinden türeyen grid ölçüsü.
#[derive(Clone, Copy)]
struct Metrics {
    cols: u16,
    rows: u16,
    cell_px: (u16, u16),
}

/// `bt-core`'un uyandırma ucu.
///
/// `Session::spawn` `Wake`'i link'ten **önce** ister, `Waker` ise link'ten
/// sonra doğar; boşluğu `OnceLock` kapatır. Kaçan kare yok: açılış karesi
/// zaten elle isteniyor ve o ana kadar okunmuş her bayt hasar bayrağında
/// birikmiş olur.
struct ShellWake {
    waker: OnceLock<Waker>,
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
        // `shutdown()`'a giden senkron bir yol o thread'i kendi kendine
        // `join` ettirirdi (`wake.rs` → Sahiplik).
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
}

/// Delegate'in durumu. `OnceCell`: pencere, oturum ve link
/// `applicationDidFinishLaunching` içinde bir kez doğar, sonra yalnız okunur.
pub(crate) struct Ivars {
    renderer: Arc<Renderer>,
    surface: Surface,
    window: OnceCell<Retained<NSWindow>>,
    link: OnceCell<DisplayLink>,
    /// Kapanış sırasının ikinci adımı buradan çağrılır; `DisplayLink` de bir
    /// kopya tutuyor ama oraya `stop()`'tan sonra uzanmak yanlış olurdu.
    session: OnceCell<Arc<Session>>,
    wake: Arc<ShellWake>,
    run_seconds: Option<u64>,
}

define_class!(
    // SAFETY: NSObject alt sınıflama şartı taşımaz; AppDelegate Drop uygulamaz.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriAppDelegate"]
    #[ivars = Ivars]
    pub(crate) struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _n: &NSNotification) {
            let mtm = self.mtm();
            let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(900.0, 600.0));
            let style = NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Miniaturizable
                | NSWindowStyleMask::Resizable;
            // SAFETY: defer=false ile pencere hemen yaratılır. Kurucunun unsafe
            // olma sebebi `releasedWhenClosed`: pencere kontrolcüsü olmadan
            // AppKit kapanışta pencereyi serbest bırakır ve `Ivars.window`'daki
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
            view.setLayer(Some(self.ivars().surface.ca_layer()));
            view.setWantsLayer(true);
            window.setContentView(Some(&view));
            window.setTitle(ns_string!("bateri"));
            // Klavyenin PTY'ye varan yolu buradan başlıyor. `contentView`
            // otomatik first responder DEĞİLDİR; bu satır olmadan pencere
            // key olur, tuşlar view'a hiç uğramaz ve terminal sessizce
            // yazmaz. `acceptsFirstResponder` da şart, ikisi bir arada.
            let ilk = window.makeFirstResponder(Some(&view));
            debug_assert!(ilk, "BateriView first responder olmalı");
            // Delegate bağlanmadan önce ivar dolu olsun: arada düşen bir
            // pencere bildirimi geometriyi boş bulup bayat boyutla çizmesin.
            // OnceCell doluysa didFinishLaunching ikinci kez geldi demek; AppKit
            // bunu yapmaz, yapsaydı ilk pencere kalırdı.
            let _ = self.ivars().window.set(window.clone());
            window.setDelegate(Some(ProtocolObject::from_ref(self)));
            window.center();
            window.makeKeyAndOrderFront(None);
            NSApplication::sharedApplication(mtm).activate();

            // Grid ölçüsü pencereden türer; oturum ilk boyutuyla doğsun ki
            // shell açılışta doğru `TIOCSWINSZ` görsün.
            // audit: pencere ve contentView hemen yukarıda kuruldu; `None`
            // dönmesi programlama hatası olurdu ve yedek bir ölçü uydurmak
            // `CELL_PX`'in ikinci bir kopyasını doğururdu.
            let metrics = self
                .geometriyi_esitle()
                .expect("pencere ve contentView kuruldu");
            self.baglat(mtm, metrics, &view);

            if let Some(s) = self.ivars().run_seconds {
                // block2 yok: zamanlayıcı performSelector ile.
                // SAFETY: `runDeadline:` bu sınıfta tanımlı ve tek
                // Option<&AnyObject> argüman alıyor. Delegate özellikleri zayıf
                // referanstır; self'i yaşatan `run()`'daki `Retained`, o da
                // `app.run()`'ı aşar. Zamanlayıcı ayrıca hedefini kendi tutar.
                // Common modes: canlı boyutlandırma run loop'u tracking moduna
                // sokar, varsayılan modda kurulan zamanlayıcı orada ertelenirdi.
                unsafe {
                    self.performSelector_withObject_afterDelay_inModes(
                        sel!(runDeadline:),
                        None,
                        s as f64,
                        &NSArray::from_slice(&[NSRunLoopCommonModes]),
                    );
                }
            }
        }

        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn should_terminate_after_last_window(&self, _app: &NSApplication) -> bool {
            true
        }

        /// AppKit'in kapanış yolu: kırmızı düğme ve `exit` yazan shell
        /// (`child_exit` → `terminate:`) buraya varır. (Cmd-Q **varmaz**: ana
        /// menü yok, `keyDown:` Command'lı tuşları yutuyor — menü 00X'te.)
        /// Duman deadline'ı da buraya uğramaz, `terminate:` her zaman 0 ile
        /// çıkar ve `runDeadline:` kırmızı düşebilmek zorunda. Ortak olan
        /// bildirim değil sıra: iki yol da [`AppDelegate::kapat`] çağırır ve
        /// kapanışa eklenecek her adım oraya eklenir.
        #[unsafe(method(applicationWillTerminate:))]
        fn will_terminate(&self, _n: &NSNotification) {
            self.kapat();
            // Duman koşusu deadline'a varmadan da bitebilir: shell kendi
            // çıkarsa (`BT_RUN_SECONDS` betiğin uykusundan uzunsa, ya da
            // gerçek bir shell hemen ölürse) `ChildExit` buraya getirir.
            // Rapor basılmadan çıkmak `make duman`'a hiçbir şey ölçmemiş bir
            // koşuyu exit 0 ile yeşil gösterirdi — kapının sahte yeşil verdiği
            // tek yol buydu.
            if self.ivars().run_seconds.is_some() {
                self.rapor_ve_cik();
            }
        }
    }

    unsafe impl NSWindowDelegate for AppDelegate {
        #[unsafe(method(windowDidResize:))]
        fn window_did_resize(&self, _n: &NSNotification) {
            self.geometri_degisti();
        }

        // Ekranlar arası taşımada boyut (nokta) değişmez ama ölçek değişir;
        // layer-hosting view'da bunu bizden başka kimse yazmaz.
        #[unsafe(method(windowDidChangeBackingProperties:))]
        fn window_did_change_backing(&self, _n: &NSNotification) {
            self.geometri_degisti();
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
            let gorunur = self.ivars().window.get().is_some_and(|window| {
                window
                    .occlusionState()
                    .contains(NSWindowOcclusionState::Visible)
            });
            if let Some(link) = self.ivars().link.get() {
                link.set_visible(gorunur);
            }
        }
    }

    impl AppDelegate {
        #[unsafe(method(runDeadline:))]
        fn run_deadline(&self, _arg: Option<&AnyObject>) {
            self.kapat();
            self.rapor_ve_cik();
        }
    }
);

impl AppDelegate {
    pub(crate) fn new(
        mtm: MainThreadMarker,
        renderer: Arc<Renderer>,
        opts: Options,
    ) -> Retained<Self> {
        let surface = renderer.surface();
        let this = Self::alloc(mtm).set_ivars(Ivars {
            renderer,
            surface,
            window: OnceCell::new(),
            link: OnceCell::new(),
            session: OnceCell::new(),
            wake: Arc::new(ShellWake {
                waker: OnceLock::new(),
            }),
            run_seconds: opts.run_seconds,
        });
        // SAFETY: NSObject'in init'i argümansızdır ve ivar'lar set edildi.
        unsafe { msg_send![super(this), init] }
    }

    /// Oturumu açar ve kareyi süren link'i bağlar. Sıra zorunlu: `Session`
    /// `Wake`'i ister, link `Session`'ı ister, `Waker` link'ten doğar.
    fn baglat(&self, mtm: MainThreadMarker, metrics: Metrics, view: &BateriView) {
        let session = Session::spawn(
            SessionOptions {
                // Duman koşusunda shell sabit: sonuç kullanıcının `$SHELL`'ine
                // ve rc dosyasına bağlı olmasın. Betiğin sahibi `bt-core` ve
                // sekiz hücre verdiği orada sınanıyor — `hucre=8` beklentisi
                // bu yüzden bir belge cümlesi değil, sınanmış bir iddia.
                command: self.ivars().run_seconds.map(|_| smoke_shell()),
                cols: metrics.cols,
                rows: metrics.rows,
                cell_px: metrics.cell_px,
                scrollback: SCROLLBACK,
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
        // referansın nerede düşeceği belli (bkz. `kapat`).
        let _ = self.ivars().session.set(Arc::clone(&session));
        view.baglan(Arc::clone(&session));
        let link = DisplayLink::new(
            mtm,
            &self.ivars().surface,
            Arc::clone(&self.ivars().renderer),
            session,
            metrics.cell_px,
        );
        // Uyandırma yolu kapanmadan kare istemiyoruz: aradaki bir `Wakeup`
        // sessizce düşerdi.
        //
        // audit: `baglat` yalnız `didFinishLaunching`'ten, bir kez çağrılır.
        // Sessizce yutulan bir `Err` burada en sinsi hatayı üretirdi: eski
        // link'in `Waker`'ı kalır, pencere shell çıktısına bir daha hiç
        // uyanmaz ve tek satır iz kalmaz.
        assert!(
            self.ivars().wake.waker.set(link.waker()).is_ok(),
            "waker ikinci kez kuruldu"
        );
        // Açılış karesi: `Session` kirli doğar, link'i bir kez elle açıyoruz.
        link.request_frame();
        let _ = self.ivars().link.set(link);
    }

    /// Kapanış sırasının **tek** yeri; her çıkış yolu buradan geçer
    /// (`applicationWillTerminate:` ve `runDeadline:`). **Sıra zorunlu.**
    ///
    /// İki kapanış adımı idempotent (`stop` mandalıyla, `shutdown` `Option`
    /// ile); bekçi değil — ikinci bir çağrı ikinci bir thread doğururdu. Bugün
    /// çağrı tek: iki yol da `process::exit`'e varıyor ve ana thread `join`'de
    /// beklerken zamanlayıcı ateşleyemiyor.
    ///
    /// 1. Ritmi kes (`DisplayLink::stop`): link durur, run loop'tan çıkar ve
    ///    uyandırma kapısı kapanır. Bundan sonra yeni kare istenmez.
    /// 2. Oturumu kapat: `SIGHUP` + okuyucu thread'in `join`'i. **Bloklar** —
    ///    sinyali yutan bir çocuk (`trap '' HUP`) `Pty::drop`'un
    ///    `child.wait()`'inde süresiz bekletir; kesecek olan bekçi thread
    ///    (`crate::bekci`).
    ///
    /// `DisplayLink` bilerek **düşürülmüyor**, yalnız durduruluyor. İçindeki
    /// `Waker`'ı Metal'in tamamlanma bloğu da tutuyor ve onun
    /// `MainThreadBound<Retained<CAMetalDisplayLink>>`'i ana thread dışında
    /// düşerse `Drop`'u ana kuyruğa **senkron** iş atıp bekler: ana thread o
    /// sırada 2. adımın `join`'inde olurdu ve ikisi birbirini kilitlerdi.
    /// `Ivars` `app.run()`'ı aştığı sürece o son referans hiçbir zaman
    /// Metal'in thread'inde olmaz.
    fn kapat(&self) {
        // Bekçinin bütçesi **kapanıştan** başlıyor, süreç başından değil:
        // açılış (Metal device, metallib yükleme, ilk pencere) soğuk bir
        // makinede saniyeler sürebilir ve o süre bütçeden düşseydi sağlıklı
        // bir koşu `_exit(70)` ile kırmızı düşerdi.
        if let Some(s) = self.ivars().run_seconds {
            crate::bekci(s);
        }
        if let Some(link) = self.ivars().link.get() {
            link.stop();
        }
        if let Some(session) = self.ivars().session.get() {
            session.shutdown();
        }
    }

    /// Duman koşusunun raporu ve çıkışı — **kapanıştan sonra** çağrılır.
    ///
    /// Sıra bilinçli: `kapat()` bloklar ve asılırsa bekçi süreci 70 ile keser,
    /// yani asılan bir kapanışta `kare=` satırı hiç çıkmaz. Ters sırada
    /// `make duman` yeşil bir satırla kırmızı bir çıkış kodunu birlikte verirdi.
    fn rapor_ve_cik(&self) -> ! {
        let n = self.ivars().renderer.frames();
        let k = self.ivars().renderer.last_bg_count();
        // İki jeton iki ayrı şey söyler: `kare` GPU'nun hatasız bitirdiği
        // kare sayısı, `hucre` sink'in ürettiği arka plan hücresi. Biri
        // sıfırken diğeri yeşil geçemez — kare>0 & hucre=0 "pencere var,
        // shell çıktısı yok" demektir ve tam da kaçırmak istemediğimiz şey.
        if n > 0 && k > 0 {
            println!("kare={n} hucre={k} pipeline=ok");
            std::process::exit(0);
        }
        // Jetonlar (`kare=`, `hucre=`) **yalnız** başarı satırında ve yalnız
        // stdout'ta: makine sözleşmesi o. Hata satırı aynı sayıları taşıyor
        // ama jeton biçiminde değil, yoksa `kare=` arayan bir CI adımı düşen
        // koşudan kare sayısı okurdu.
        eprintln!(
            "bateri: {} saniyelik koşuda çizilen kare {n}, üretilen hücre {k} (ikisi de >0 olmalı)",
            self.ivars().run_seconds.unwrap_or(0)
        );
        std::process::exit(1);
    }

    /// Pencere geometrisi oynadı: layer'ı eşle, grid'i güncelle, kare iste.
    fn geometri_degisti(&self) {
        let Some(metrics) = self.geometriyi_esitle() else {
            return;
        };
        if let Some(link) = self.ivars().link.get() {
            link.resize(metrics.cols, metrics.rows, metrics.cell_px);
        }
    }

    /// Layer'ın drawable boyutunu view'ın backing geometrisiyle eşler **ve**
    /// grid ölçüsünü döndürür — ad ikisini birden söylüyor çünkü çağıranın
    /// ikisine de ihtiyacı var ve boyutu yazmadan ölçüyü türetmek yanlış
    /// sonuç verirdi. Ölçek tek kaynaktan okunur ve piksel boyutu ondan
    /// çarpılır; `drawableSize` ile `contentsScale` ayrışırsa bulanıklık olur.
    fn geometriyi_esitle(&self) -> Option<Metrics> {
        let window = self.ivars().window.get()?;
        let view = window.contentView()?;
        let scale = window.backingScaleFactor();
        let bounds = view.bounds().size;
        let (width_px, height_px) = (bounds.width * scale, bounds.height * scale);
        self.ivars().surface.set_size(width_px, height_px, scale);

        // Hücre en az 1 piksel: sıfır bölme yok. `as u16` f64'te doygundur
        // (NaN ve negatif → 0, büyük → 65535) ve kesme tam olarak istediğimiz
        // taban yuvarlama; sıfır sütun/satırı `Session::resize` zaten
        // yoksayar (simge durumundaki pencere).
        let cell_w = (CELL_PX.0 * scale).round().max(1.0);
        let cell_h = (CELL_PX.1 * scale).round().max(1.0);
        Some(Metrics {
            cols: (width_px / cell_w) as u16,
            rows: (height_px / cell_h) as u16,
            cell_px: (cell_w as u16, cell_h as u16),
        })
    }
}
