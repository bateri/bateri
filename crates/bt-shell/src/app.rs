//! Uygulama delegate'i: pencereyi açar, `CAMetalLayer`'ı view'a takar,
//! shell oturumunu başlatır, kareyi süren display link'i bağlar ve kapanış
//! sırasını yürütür. Çizim çağrısı burada **yok**, bu dosyanın işi bağlamak.

use std::cell::OnceCell;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use bt_core::{Session, SessionOptions, Wake, load_shell, smoke_shell};
use bt_gpu::{CellMetrics, DisplayLink, Renderer, Surface, Waker};
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

use crate::view::BateriView;
use crate::{Options, Workload};

/// Kaydırma geçmişi satır sayısı; ayar dosyası (00X) gelene kadar sabit.
const SCROLLBACK: usize = 10_000;

/// Boşta sıfır karenin bekçisi: [`Workload::Smoke`] yükünde pencere ilk
/// çizimden sonra ~`run_seconds` saniye boşta duruyor.
///
/// **Sayı ölçüldü, türetilmedi** (2026-09-11, bu makine). Üç okuma:
/// sağlıklı duman koşusu `kare=1` (art arda beş koşuda da); boşta sıfır kare
/// bilerek bozulduğunda (`needs_update`'in sonuna koşulsuz `wake()`)
/// `kare=3`; [`Workload::Load`] ile `BT_RUN_SECONDS` 3, 6, 10 → hep `kare=3`,
/// yani ritim süreyle artmıyor. Sebebi görünürlük: bundle'sız süreç öne
/// çıkamıyor, pencere `isVisible()` olsa da `occlusionState` `Visible`
/// taşımıyor ve sistem display link'i askıya alıyor — tavan ~3.
///
/// `2` bu üç sayının arasındaki tek anlamlı yer: sağlıklı koşuyu bir kare
/// payla geçiriyor (açılışta düşen bir geometri olayı için), bozulmuş koşuyu
/// yakalıyor. **Kapının koştuğu tek bağlam `make duman`'dır** —
/// [`AppDelegate::report_and_exit`] yalnız `BT_RUN_SECONDS` yolunda çalışıyor,
/// yani etkileşimli koşu bu sınırı hiç değerlendirmiyor. Plan dosyasındaki
/// `8`, "bozulursa 60 Hz'de üç saniye ~180 kare" türetimine dayanıyordu; o
/// türetim ölçümle çürüdü (tavan 3) ve `8` bu koşumda **hiçbir zaman**
/// ateşleyemezdi.
///
/// **`.app` paketi (`make kur`) gelince yeniden ölç:** görünür bir pencerede
/// tavan kalkar, meşru kare sayısı artar ve sınır yükselmelidir.
///
/// **Bilinen yanlış pozitif:** `DisplayLink::resize` koşulsuz kare istiyor,
/// yani koşu sırasında pencereyi sürüklemek (ya da ekran/ölçek değiştirmek)
/// meşru kareler üretir ve sınırı aşabilir. `make duman` gözetimsiz koşuyor,
/// bedel kabul edildi; kalıcı çözüm geometri yolundan gelen kareleri sayaç
/// dışında tutmak.
///
/// [`Workload::Load`] yükünde üst sınır **yok** — orada kare akışı işin
/// kendisi.
const IDLE_FRAME_LIMIT: u64 = 2;

/// Pencere geometrisi + hücre ölçüsünden türeyen grid.
///
/// Adı `Metrics` değil: hücre metriğinin sahibi artık `bt-gpu`
/// ([`CellMetrics`]) ve iki tip bu dosyada yan yana okunuyor. Buradaki
/// "kaç sütun kaç satır **ve** hangi hücreyle", oradaki yalnız hücre.
#[derive(Clone, Copy)]
struct Grid {
    cols: u16,
    rows: u16,
    /// Demet değil `CellMetrics`: ölçü buradan `DisplayLink::resize`'a
    /// olduğu gibi geçiyor. `Grid`'de saklanan bu değer yalnız
    /// `SessionOptions`'a girerken demete iniyor — `split_into_grid`'un
    /// bölmeye soktuğu demet başka bir değer: oraya **gelen** ölçü girer,
    /// `Grid` ondan sonra doğar.
    cell: CellMetrics,
}

/// Piksel geometrisi + hücre ölçüsü → grid.
///
/// `sync_geometry`'den ayrı duruyor çünkü saf olan tek parça bu; geri
/// kalanı pencere ve layer, yani sınanamaz. Hücre ölçüsü **argüman**: bu
/// gövdeye gizlenmiş bir sabit `cell_metrics_come_from_outside`'i düşürür.
///
/// Kapsamı bu kadar, daha fazlası değil: `CELL_PX`'in asıl durduğu satır
/// `sync_geometry`'deki `cell_metrics(scale)` çağrısıydı ve orası bir
/// pencere ile Metal device istediği için sınanmıyor. `CellMetrics::new`
/// bilerek `pub`, yani oraya yazılacak bir `CellMetrics::new(9, 18)` yer
/// tutucuyu diriltir ve buradaki iki sınama yeşil kalır.
fn split_into_grid(width_px: f64, height_px: f64, cell: CellMetrics) -> Grid {
    let (cell_w, cell_h) = cell.cell_px();
    // `as u16` f64'te doygundur (NaN ve negatif → 0, büyük → 65535) ve kesme
    // tam olarak istediğimiz taban yuvarlama; sıfır sütun/satırı
    // `Session::resize` zaten yoksayar (simge durumundaki pencere). Bölen
    // sıfır olamaz ve bunu tip taşıyor: `CellMetrics`'in alanı private ve
    // kurucusu (`CellMetrics::new`) sıfırı eliyor; üretimdeki kaynağı
    // `Renderer::cell_metrics`, oranın garantisi de `bt-atlas`'ın ≥ 1
    // kırpması.
    Grid {
        cols: (width_px / f64::from(cell_w)) as u16,
        rows: (height_px / f64::from(cell_h)) as u16,
        cell,
    }
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
    /// `Rc`: renderer ana thread'e çivili (bkz. `bt_gpu::DisplayLink`).
    renderer: Rc<Renderer>,
    surface: Surface,
    window: OnceCell<Retained<NSWindow>>,
    link: OnceCell<DisplayLink>,
    /// Kapanış sırasının ikinci adımı buradan çağrılır; `DisplayLink` de bir
    /// kopya tutuyor ama oraya `stop()`'tan sonra uzanmak yanlış olurdu.
    session: OnceCell<Arc<Session>>,
    wake: Arc<ShellWake>,
    run_seconds: Option<u64>,
    workload: Option<Workload>,
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
            let accepted = window.makeFirstResponder(Some(&view));
            debug_assert!(accepted, "BateriView first responder olmalı");
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
            // hücre boyutu için ikinci bir kaynak doğururdu — tek kaynak
            // `Renderer::cell_metrics`.
            let grid = self
                .sync_geometry()
                .expect("pencere ve contentView kuruldu");
            self.start_session(mtm, grid, &view);

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
        /// bildirim değil sıra: iki yol da [`AppDelegate::shutdown`] çağırır ve
        /// kapanışa eklenecek her adım oraya eklenir.
        #[unsafe(method(applicationWillTerminate:))]
        fn will_terminate(&self, _n: &NSNotification) {
            self.shutdown();
            // Duman koşusu deadline'a varmadan da bitebilir: shell kendi
            // çıkarsa (`BT_RUN_SECONDS` betiğin uykusundan uzunsa, ya da
            // gerçek bir shell hemen ölürse) `ChildExit` buraya getirir.
            // Rapor basılmadan çıkmak `make duman`'a hiçbir şey ölçmemiş bir
            // koşuyu exit 0 ile yeşil gösterirdi — kapının sahte yeşil verdiği
            // tek yol buydu.
            if self.ivars().run_seconds.is_some() {
                self.report_and_exit();
            }
        }
    }

    unsafe impl NSWindowDelegate for AppDelegate {
        #[unsafe(method(windowDidResize:))]
        fn window_did_resize(&self, _n: &NSNotification) {
            self.geometry_changed();
        }

        // Ekranlar arası taşımada boyut (nokta) değişmez ama ölçek değişir;
        // layer-hosting view'da bunu bizden başka kimse yazmaz.
        #[unsafe(method(windowDidChangeBackingProperties:))]
        fn window_did_change_backing(&self, _n: &NSNotification) {
            self.geometry_changed();
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
            let visible = self.ivars().window.get().is_some_and(|window| {
                window
                    .occlusionState()
                    .contains(NSWindowOcclusionState::Visible)
            });
            if let Some(link) = self.ivars().link.get() {
                link.set_visible(visible);
            }
        }
    }

    impl AppDelegate {
        #[unsafe(method(runDeadline:))]
        fn run_deadline(&self, _arg: Option<&AnyObject>) {
            self.shutdown();
            self.report_and_exit();
        }
    }
);

/// Duman kapısının kararı.
///
/// `bool` **değil**: hata yolunun iki ayrı iletisi var ve `bool` onları
/// kapının dışında yeniden türetmeye zorlardı. Politika o zaman üç yere
/// dağılırdı (başarı satırı, "sıfır" iletisi, "fazla" iletisi) ve yalnız biri
/// sınanmış olurdu — kapı düşerken yanlış arızayı tarif eden bir koşu tam da
/// böyle doğar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verdict {
    Pass,
    /// Sayaçlardan biri sıfır: pipeline'ın bir halkası hiç çalışmamış.
    /// `required` iletinin gereklilik yarısı ve yüke göre değişiyor — `Load`
    /// düz metin akıtıyor, orada `hucre` ile `kural` yapısal olarak sıfır.
    MissingCounter {
        required: &'static str,
    },
    /// Kare sayısı üst sınırı aştı: boşta sıfır kare bozulmuş.
    ExcessFrames {
        limit: u64,
    },
}

/// Kapının saf hâli — gerçek bir display link ve pencere istemeden sınanır.
///
/// Karar [`AppDelegate::report_and_exit`]'in gövdesinde kalsaydı sınırın
/// yönünü (8 mi 180 mi, `Load` muaf mı) yalnız `make duman` bilirdi ve hiçbir
/// sınamada yazılı olmazdı.
fn verdict(n: u64, k: usize, g: usize, r: usize, workload: Option<Workload>) -> Verdict {
    match workload {
        // Ölçüm yükü düz metin akıtıyor: arka plan da kural da **yok** ve
        // olmayacak. İkisini sormak, duman reçetesini hiç koşmayan bir koşuya
        // o reçetenin sayılarını sormak olurdu — kapı her ölçüm koşusunda
        // düşerdi. Kare akışı burada işin kendisi: üst sınır da yok.
        Some(Workload::Load) => {
            if n == 0 || g == 0 {
                Verdict::MissingCounter {
                    required: "kare ve glif >0 olmalı",
                }
            } else {
                Verdict::Pass
            }
        }
        // Duman reçetesi: dördü de > 0 **ve** kare sayısı üst sınırlı.
        Some(Workload::Smoke) | None => {
            if n == 0 || k == 0 || g == 0 || r == 0 {
                Verdict::MissingCounter {
                    required: "dördü de >0 olmalı",
                }
            } else if n > IDLE_FRAME_LIMIT {
                Verdict::ExcessFrames {
                    limit: IDLE_FRAME_LIMIT,
                }
            } else {
                Verdict::Pass
            }
        }
    }
}

impl AppDelegate {
    pub(crate) fn new(
        mtm: MainThreadMarker,
        renderer: Rc<Renderer>,
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
            workload: opts.workload,
        });
        // SAFETY: NSObject'in init'i argümansızdır ve ivar'lar set edildi.
        unsafe { msg_send![super(this), init] }
    }

    /// Oturumu açar ve kareyi süren link'i bağlar. Sıra zorunlu: `Session`
    /// `Wake`'i ister, link `Session`'ı ister, `Waker` link'ten doğar.
    fn start_session(&self, mtm: MainThreadMarker, grid: Grid, view: &BateriView) {
        let session = Session::spawn(
            SessionOptions {
                // Duman ve ölçüm koşularında shell sabit: sonuç kullanıcının
                // `$SHELL`'ine ve rc dosyasına bağlı olmasın. Betiklerin
                // sahibi `bt-core`; `smoke_shell`'in sekiz hücre ve altı
                // glyph verdiği orada sınanıyor — `hucre=8` ve `glif=6`
                // beklentileri bu yüzden birer belge cümlesi değil, sınanmış
                // birer iddia.
                //
                // Dallanma **yükü** soruyor, süreyi değil: `run_seconds`
                // deadline'ı ve bekçiyi de kuruyor ve üçü tek koşula
                // bağlanırsa ölçüm koşusu ikisinden birini kaybeder.
                command: self.ivars().workload.map(|w| match w {
                    Workload::Smoke => smoke_shell(),
                    // Yükün süresi deadline'la aynı olmalı: kısa kalırsa
                    // pencere koşunun kuyruğunda boşa düşer ve ölçüm boşta
                    // kare örnekler.
                    //
                    // `main.rs` süresiz yükü eliyor, ama `Options` `pub` ve
                    // alanları da `pub`: dışarıdan `run_seconds: None` +
                    // `workload: Some(Load)` kurulabilir ve o koşu **sessizce
                    // 0 ile** çıkardı (`will_terminate` raporu `run_seconds`'a
                    // bakıyor). Yorum bir değişmezi savunamaz; `debug_assert`
                    // savunur. Kalıcı çözüm tipin kendisi — `Options`'ın tek
                    // alana inmesi 005 phase-2'ye devredildi.
                    Workload::Load => {
                        debug_assert!(
                            self.ivars().run_seconds.is_some(),
                            "ölçüm yükü süresiz kurulamaz"
                        );
                        load_shell(self.ivars().run_seconds.unwrap_or(0))
                    }
                }),
                cols: grid.cols,
                rows: grid.rows,
                cell_px: grid.cell.cell_px(),
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
        // referansın nerede düşeceği belli (bkz. `shutdown`).
        let _ = self.ivars().session.set(Arc::clone(&session));
        view.attach(Arc::clone(&session));
        let link = DisplayLink::new(
            mtm,
            &self.ivars().surface,
            Rc::clone(&self.ivars().renderer),
            session,
            grid.cell,
        );
        // Uyandırma yolu kapanmadan kare istemiyoruz: aradaki bir `Wakeup`
        // sessizce düşerdi.
        //
        // audit: `start_session` yalnız `didFinishLaunching`'ten, bir kez çağrılır.
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
    ///    (`crate::watchdog`).
    ///
    /// `DisplayLink` bilerek **düşürülmüyor**, yalnız durduruluyor. İçindeki
    /// `Waker`'ı Metal'in tamamlanma bloğu da tutuyor ve onun
    /// `MainThreadBound<Retained<CAMetalDisplayLink>>`'i ana thread dışında
    /// düşerse `Drop`'u ana kuyruğa **senkron** iş atıp bekler: ana thread o
    /// sırada 2. adımın `join`'inde olurdu ve ikisi birbirini kilitlerdi.
    /// `Ivars` `app.run()`'ı aştığı sürece o son referans hiçbir zaman
    /// Metal'in thread'inde olmaz.
    fn shutdown(&self) {
        // Bekçinin bütçesi **kapanıştan** başlıyor, süreç başından değil:
        // açılış (Metal device, metallib yükleme, ilk pencere) soğuk bir
        // makinede saniyeler sürebilir ve o süre bütçeden düşseydi sağlıklı
        // bir koşu `_exit(70)` ile kırmızı düşerdi.
        if let Some(s) = self.ivars().run_seconds {
            crate::watchdog(s);
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
    /// Sıra bilinçli: `shutdown()` bloklar ve asılırsa bekçi süreci 70 ile keser,
    /// yani asılan bir kapanışta `kare=` satırı hiç çıkmaz. Ters sırada
    /// `make duman` yeşil bir satırla kırmızı bir çıkış kodunu birlikte verirdi.
    ///
    /// Satır burada **açıkça** yazılıyor; `Drop`'a güvenen hiçbir yol yok.
    /// `process::exit` `Drop` koşturmaz ve bekçinin `_exit(70)`'i atexit'i
    /// bile atlar.
    fn report_and_exit(&self) -> ! {
        let n = self.ivars().renderer.frames();
        let k = self.ivars().renderer.last_bg_count();
        let g = self.ivars().renderer.last_glyph_count();
        let r = self.ivars().renderer.last_rule_count();
        // Dört jeton dört ayrı şey söyler: `kare` GPU'nun hatasız bitirdiği
        // kare sayısı, `hucre` sink'in ürettiği arka plan hücresi, `glif`
        // çizilen glyph, `kural` çizilen alt çizgi/üstü çizili. Biri sıfırken
        // diğerleri yeşil geçemez — kare>0 & hucre=0 "pencere var, shell
        // çıktısı yok" demek; hucre>0 & glif=0 ise "hücreler boyanıyor ama
        // harf yok", yani 002'nin körlemesine yazma dönemine sessizce geri
        // düşmek: `glif` kapısı olmasaydı `frame()` sınırı karakteri hiç
        // geçirmese bile `kare=1 hucre=8 pipeline=ok` basılırdı. `kural`ın
        // kapattığı yarı da aynı biçimde ayrı: sınır beş alt çizgi çeşidini,
        // üstü çiziliyi ve SGR 58'i taşıyor ve o yolun tamamı `glif`'ten
        // bağımsız — duman reçetesinin yedi kural hücresi mürekkepsiz.
        //
        // Kapsamadığı — dördü de birer **CPU** sayacı ve `kural` bunun üstüne
        // stil ayrımını da göremez; sınırın tamamı `Frame::rule_count`'ta
        // yazılı ve tek yerde duruyor. Buraya yalnız duman kapısına özgü olan
        // yarı düşüyor: jeton setin **yüz yarısını hiç sormuyor** — `Face`
        // çevirisini hep `Regular` döndüren bir yapı da aynı dört sayıyı
        // basar, çünkü kalın bir glyph de bir glyph'tir. O yarının kapısı
        // `bt-gpu`'nun `sgr_flags_translate_to_four_faces` ve
        // `bold_and_regular_draw_differently` sınamaları.
        //
        // Beşinci jeton `yuva=U/T` bir kapı değil, bir **sayaç**: atlasın kaç
        // yuvasının dolduğunu söylüyor ve `/measure` doluluk oranını ondan
        // okuyacak. Kapıya girmemesinin sebebi anlamı: boş bir atlas da
        // meşrudur (glyph'siz bir kare) ve dolu bir atlas da — arıza eşiği
        // ölçülmeden bilinmiyor, ölçülmemiş sayı da kapıya yazılmaz.
        let (used, total) = self.ivars().renderer.atlas_occupancy();
        // Jetonlar (`kare=`, `hucre=`, `glif=`, `kural=`, `yuva=`) **yalnız**
        // başarı satırında ve yalnız stdout'ta: makine sözleşmesi o. Hata
        // satırları aynı sayıları taşıyor ama jeton biçiminde değil, yoksa
        // `kare=` arayan bir CI adımı düşen koşudan kare sayısı okurdu.
        let secs = self.ivars().run_seconds.unwrap_or(0);
        let workload = self.ivars().workload;
        // `yuk=` jetonu, `Load`'un başarı satırındaki `hucre=0 kural=0`'ı
        // okunabilir kılıyor: o iki sıfır ölçüm yükünde **beklenen** (düz
        // metin akıyor), duman yükünde ise ölü bir boru hattı demek. Jeton
        // olmadan satırı okuyan taraf ikisini ayıramazdı — ve sözleşme jeton
        // eklemeye zaten izin veriyor, silmeye vermiyor.
        let load = match workload {
            Some(Workload::Load) => "load",
            Some(Workload::Smoke) | None => "smoke",
        };
        match verdict(n, k, g, r, workload) {
            Verdict::Pass => {
                println!(
                    "kare={n} hucre={k} glif={g} kural={r} yuva={used}/{total} yuk={load} pipeline=ok"
                );
                std::process::exit(0);
            }
            // Ayrı ileti, çünkü ayrı arıza: burada dört sayacın dördü de
            // yerinde ve okuyanı sıfır aramaya göndermek zaman kaybettirirdi.
            Verdict::ExcessFrames { limit } => eprintln!(
                "bateri: boşta sıfır kare bozuldu — {secs} saniyelik koşuda {n} kare çizildi, üst sınır {limit}"
            ),
            Verdict::MissingCounter { required } => eprintln!(
                "bateri: {secs} saniyelik koşuda çizilen kare {n}, üretilen hücre {k}, çizilen glif {g}, çizilen kural {r} ({required})"
            ),
        }
        std::process::exit(1);
    }

    /// Pencere geometrisi oynadı: layer'ı eşle, grid'i güncelle, kare iste.
    fn geometry_changed(&self) {
        let Some(grid) = self.sync_geometry() else {
            return;
        };
        if let Some(link) = self.ivars().link.get() {
            link.resize(grid.cols, grid.rows, grid.cell);
        }
    }

    /// Layer'ın drawable boyutunu view'ın backing geometrisiyle eşler **ve**
    /// grid ölçüsünü döndürür — ad ikisini birden söylüyor çünkü çağıranın
    /// ikisine de ihtiyacı var ve boyutu yazmadan ölçüyü türetmek yanlış
    /// sonuç verirdi. Ölçek tek kaynaktan okunur ve piksel boyutu ondan
    /// çarpılır; `drawableSize` ile `contentsScale` ayrışırsa bulanıklık olur.
    fn sync_geometry(&self) -> Option<Grid> {
        let window = self.ivars().window.get()?;
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
        let cell = self.ivars().renderer.cell_metrics(scale);
        Some(split_into_grid(width_px, height_px, cell))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics(w: u16, h: u16) -> CellMetrics {
        CellMetrics::new(w, h).expect("sıfır olmayan hücre")
    }

    #[test]
    fn cell_metrics_come_from_outside() {
        // Yer tutucunun ölmüş olmasının sınanabilir hâli: aynı pencere, iki
        // farklı hücre ölçüsü, iki farklı grid. Gövdeye geri sızan bir sabit
        // ikisini eşitler ve bu sınama düşer.
        let narrow = split_into_grid(900.0, 600.0, metrics(9, 18));
        let wide = split_into_grid(900.0, 600.0, metrics(18, 36));
        assert_eq!((narrow.cols, narrow.rows), (100, 33));
        assert_eq!((wide.cols, wide.rows), (50, 16));
    }

    #[test]
    fn idle_limit_catches_excess_frames() {
        let smoke = |n, k, g, r| verdict(n, k, g, r, Some(Workload::Smoke));
        let load = |n, k, g, r| verdict(n, k, g, r, Some(Workload::Load));
        let excess = Verdict::ExcessFrames {
            limit: IDLE_FRAME_LIMIT,
        };

        // Bugünkü duman koşusunun ta kendisi: bir kare, sekiz hücre, altı
        // glyph, on beş kural.
        assert_eq!(smoke(1, 8, 6, 15), Verdict::Pass);
        // Sınırın kendisi geçer, bir fazlası düşer. Eski kapı (`n > 0`) sıfırı
        // görüyordu ama fazlayı görmüyordu ve boşta sıfır kareyi bozan bir
        // değişikliğin belirtisi tam olarak fazla kare: görünür bir pencerede
        // 60 Hz'de üç saniye ~180 kare eder ve o koşu yeşil geçerdi.
        assert_eq!(smoke(IDLE_FRAME_LIMIT, 8, 6, 15), Verdict::Pass);
        assert_eq!(smoke(IDLE_FRAME_LIMIT + 1, 8, 6, 15), excess);
        assert_eq!(smoke(180, 8, 6, 15), excess);
        // Bilerek bozulmuş koşunun **ölçülen** sayısı (2026-09-11):
        // `needs_update`'in sonuna koşulsuz `wake()` konunca duman `kare=3`
        // bastı. Sınır bu sayıyı yakalamak zorunda — eski `8` yakalamıyordu.
        assert_eq!(smoke(3, 8, 6, 15), excess);
        // `Load` yükünde akış işin kendisi: aynı sayı geçmeli. Sınırın yüke
        // bağlı olduğu tek yerde yazılı ve burada sınanıyor.
        assert_eq!(load(180, 8, 6, 15), Verdict::Pass);
        // Ölçüm yükünün **gerçek** sayıları (`BT_SCROLL_TEST=1
        // BT_RUN_SECONDS=3`, 2026-09-11): düz metin akıyor, arka plan ve kural
        // yapısal olarak sıfır. Dört sayaç da sorulsaydı her ölçüm koşusu
        // kırmızı düşerdi.
        assert_eq!(load(3, 0, 1836, 0), Verdict::Pass);

        // Sıfır, fazladan **önce** gelir: ikisi birden bozuksa okuyan taraf
        // önce eksik halkayı arasın. Kolların sırasını ters çeviren bir
        // değişiklik burada kırmızı düşer.
        assert_eq!(
            smoke(200, 0, 6, 15),
            Verdict::MissingCounter {
                required: "dördü de >0 olmalı"
            }
        );

        // Alt sınır iki yükte de duruyor ve her sayaç ayrı bir kapı. Karar
        // `MissingCounter` olmalı, `ExcessFrames` değil: düşen koşu okuyanı
        // doğru arızaya göndermeli.
        for (got, required) in [
            (smoke(0, 8, 6, 15), "dördü de >0 olmalı"),
            (smoke(1, 0, 6, 15), "dördü de >0 olmalı"),
            (smoke(1, 8, 0, 15), "dördü de >0 olmalı"),
            (smoke(1, 8, 6, 0), "dördü de >0 olmalı"),
            (load(0, 0, 1836, 0), "kare ve glif >0 olmalı"),
            (load(3, 0, 0, 0), "kare ve glif >0 olmalı"),
        ] {
            assert_eq!(got, Verdict::MissingCounter { required });
        }
    }

    #[test]
    fn zero_window_does_not_panic() {
        // Simge durumuna alınan pencere 0×0 bounds verir; `Session::resize`
        // sıfır grid'i yoksayıyor ama buraya gelen yolun panik etmemesi
        // gerekiyor — bölme değil, `as u16` doygunluğu taşıyor.
        let g = split_into_grid(0.0, 0.0, metrics(9, 18));
        assert_eq!((g.cols, g.rows), (0, 0));
    }
}
