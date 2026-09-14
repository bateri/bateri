//! Uygulama delegate'i: pencereyi açar, `CAMetalLayer`'ı view'a takar,
//! shell oturumunu başlatır, kareyi süren display link'i bağlar ve kapanış
//! sırasını yürütür. Çizim çağrısı burada **yok**, bu dosyanın işi bağlamak.

use std::cell::OnceCell;
use std::fmt::Write as _;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use bt_core::{Session, SessionOptions, Teardown, Wake, load_shell, smoke_shell};
use bt_gpu::{CellMetrics, DisplayLink, MIN_SAMPLES, Renderer, Stats, Surface, Waker};
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

use crate::child;
use crate::view::BateriView;
use crate::{Options, Run, Workload};

/// Kaydırma geçmişi satır sayısı; ayar dosyası (00X) gelene kadar sabit.
const SCROLLBACK: usize = 10_000;

/// Boşta sıfır karenin bekçisi: [`Workload::Smoke`] yükünde pencere ilk
/// çizimden sonra ~`run_seconds` saniye boşta duruyor.
///
/// **Sayı yeniden ölçüldü ve büyüdü; eski dayanağı çürüktü.** Önceki `2`,
/// "sistem display link'i askıya alıyor, tavan ~3 kare" saptamasına
/// dayanıyordu. O saptama 005 phase-2b'de çürüdü: aynı pencere durumunda
/// [`Workload::Load`] beş saniyede `kare=594` üretiyor, yani **tavan yok** —
/// ölçülen şey tavan değil, kapanış kilitlenmesiyle bozulmuş bir koşuydu.
///
/// Bugünkü iki kutup **ölçüldü** (2026-09-12, debug, bu makine):
///
/// - **Sağlıklı:** otuz bir koşu (3 sn ×18, 5 sn ×11, 10 sn ×2; ikisi
///   `BT_FRAME_STATS=1` ile) → `kare` **1 veya 2**; otuz birde **bir** kez
///   `4`. Süreyle artmıyor: on saniyelik koşu da `2`.
/// - **Bozuk:** `needs_update`'in sonuna koşulsuz bir `wake()` konunca
///   dokuz koşu → `kare` **49–354** (3 sn'de 82–354, 5 sn'de 49–63).
///
/// Aynı makine iki rejimde koşuyor (ölçüm yükü aynı komutla bir kez
/// `kare=21`, bir kez `kare=597` verdi; en olası değişken pencere
/// görünürlüğü, **doğrulanmadı**). Sınır her ikisinde de güvenli: kare
/// akışının serbest olduğu rejimde sağlıklı duman beş koşuda **beşi de**
/// `kare=1` bastı — boşta sıfır kare orada da çalışıyor. Payın rejime bağlı
/// olduğu da kayda geçsin: kısılmış rejimde (ölçüm yükü 5 sn'de `kare=21`,
/// yani ~4 Hz) bozuk bir üç saniyelik duman ~12 kare eder, yani `8`'in
/// **1,5 katı** — sınırı buradan yükseltmemenin sebebi bu.
///
/// **Sınır büyürken kapının algılama tabanı da yükseldi** ve bunun bedeli
/// bugün değil sonra ödenecek. Kapı `n > limit`'te ateşliyor, yani yakalamak
/// için `limit + 1` kare gerekiyor: üç saniyelik bir koşuda eski `2` **1
/// Hz**'lik bir sızıntıyı yakalardı, bugünkü `8` ancak **3 Hz**'i yakalıyor.
/// (İkisi de `make duman`'ın 3 saniyesinden türüyor; süre değişirse eşik de
/// değişir.) Bu depoda öyle bir animasyon **yok**, ama hareket/imleç fiziği
/// seti tam bu şekilde gelecek: durma koşulu unutulmuş 2 Hz'lik bir blink üç
/// saniyede ~6 kare eder — sınırın altında, yani bugünkü kapıdan **yeşil**
/// geçer. O set açıldığında kapı ya süreye ya `istek=`'e bağlanmalı;
/// `istek=` örtülmeden etkilenmiyor ve oran olarak (saniye başına talep) bir
/// eşik verebilir, ama o eşik **ölçülmedi**.
///
/// **Sağlıklı koşudaki 1↔2 oynamasının mekanizması ölçülmedi.** Kare talebi
/// (`istek=`) o koşularda **sabit** kaldı (2–3), yani fazladan kare fazladan
/// **talepten** gelmiyor — geometri/örtülme kancaları olsaydı `istek` de
/// artardı. Geriye taleplerin birleşip birleşmemesi kalıyor (açılış karesi
/// shell'in ilk baytlarından önce çizildiyse ikinci bir kare gerekir) ama bu
/// bir **hipotez**, ölçüm değil.
///
/// Oynama **bu değişikliğin getirdiği bir şey değil**: aynı on beş koşu
/// değiştirilmemiş `854f027` üstünde de koşuldu (`git stash`) ve aynı
/// dağılımı verdi — 3 sn'de sekiz kez `2`, iki kez `1`; 5 sn'de üç kez `1`,
/// iki kez `2`. Yani `2` sınırı sağlıklı koşuların **çoğunun tam üstünde**
/// duruyordu.
///
/// `8` bu iki kutbun arasında: en yüksek sağlıklı gözlemin (`4`) **iki katı**,
/// en düşük bozuk gözlemin (`49`) **altıda biri**. Eski `2` bu boşluğun
/// sağlıklı ucuna sıkışmıştı ve **doğru bir build'de kırmızı düştüğü
/// ölçüldü** (5 saniyelik bir koşu `kare=4` bastı) — yani kapı, koruduğu
/// şeyi değil makinenin o anki gürültüsünü ölçüyordu.
///
/// **Kapının koştuğu tek bağlam `make duman`'dır** —
/// [`AppDelegate::report_and_exit`] yalnız `BT_RUN_SECONDS` yolunda çalışıyor,
/// yani etkileşimli koşu bu sınırı hiç değerlendirmiyor.
///
/// **`.app` paketi (`make kur`) gelince yeniden ölç:** görünür bir pencerede
/// meşru kare sayısı artabilir ve sınır yükselmelidir.
///
/// **Bilinen yanlış pozitif (duruyor):** `DisplayLink::resize` koşulsuz kare
/// istiyor, yani koşu sırasında pencereyi sürüklemek meşru kareler üretir ve
/// sekiz kareyi de aşabilir. `make duman` gözetimsiz koşuyor, bedel kabul
/// edildi; kalıcı çözüm geometri yolundan gelen kareleri sayaç dışında
/// tutmak.
///
/// Sınırın **kare** üstünde durmasının sebebi adı: "boşta sıfır kare" çizilen
/// kareyi söylüyor. `istek=` daha erken bir yerde sayıyor ama kapı değil —
/// eşiği ölçülmedi ve **duman yükünde** ölçülen ilişki (`istek ≈ kare + 2`,
/// hem sağlıklı hem bozuk koşuda) onu `kare`'den daha ayırt edici yapmıyor.
/// Ölçüm yükünde ikisi üç mertebe ayrışıyor (bkz. `bt_gpu`'nun `requests`
/// sayacı); oran olarak bir kapı kurulabilir ama o ölçülmedi.
///
/// [`Workload::Load`] yükünde üst sınır **yok** — orada kare akışı işin
/// kendisi.
const IDLE_FRAME_LIMIT: u64 = 8;

/// Pencere geometrisi + hücre ölçüsünden türeyen grid.
///
/// Adı `Metrics` değil: hücre metriğinin sahibi artık `bt-gpu`
/// ([`CellMetrics`]) ve iki tip bu dosyada yan yana okunuyor. Buradaki
/// "kaç sütun kaç satır **ve** hangi hücreyle", oradaki yalnız hücre.
///
/// `view` modülüne de açık: fare çevirisi aynı üçlüyü ister ve ayrıştırma
/// iki çağrı yerinde tekrarlanacağına burada bir kez durur.
#[derive(Clone, Copy)]
pub(crate) struct Grid {
    pub(crate) cols: u16,
    pub(crate) rows: u16,
    /// Demet değil `CellMetrics`: ölçü buradan `DisplayLink::resize`'a
    /// olduğu gibi geçiyor. `Grid`'de saklanan bu değer yalnız
    /// `SessionOptions`'a girerken demete iniyor — `split_into_grid`'un
    /// bölmeye soktuğu demet başka bir değer: oraya **gelen** ölçü girer,
    /// `Grid` ondan sonra doğar.
    pub(crate) cell: CellMetrics,
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
}

/// Delegate'in durumu. `OnceCell`: pencere, oturum ve link
/// `applicationDidFinishLaunching` içinde bir kez doğar, sonra yalnız okunur.
pub(crate) struct Ivars {
    /// `Rc`: renderer ana thread'e çivili (bkz. `bt_gpu::DisplayLink`).
    renderer: Rc<Renderer>,
    surface: Surface,
    window: OnceCell<Retained<NSWindow>>,
    /// Fare çevirisinin girdileri pencere boyuyla tazeleniyor (`set_metrics`);
    /// view'a `contentView`'dan (`NSView`) inilemiyor, o yüzden burada tutuluyor.
    view: OnceCell<Retained<BateriView>>,
    link: OnceCell<DisplayLink>,
    /// Kapanış sırasının ikinci adımı buradan çağrılır; `DisplayLink` de bir
    /// kopya tutuyor ama oraya `stop()`'tan sonra uzanmak yanlış olurdu.
    session: OnceCell<Arc<Session>>,
    wake: Arc<ShellWake>,
    /// Süreli koşunun tarifi; `None` → kullanıcının kendi oturumu. Deadline,
    /// bekçi, sabit shell ve rapor **hep birlikte** buna bağlı.
    run: Option<Run>,
    /// Ölçüm defteri — kapı kapalıyken `None` ve hiç ayrılmamış.
    ///
    /// `bt-gpu`'nun tipi ama sahibi burası: `DisplayLink` ile tamamlanma bloğu
    /// birer kopyasını yazıyor, kapanışta okuyan (rapor) bu kopya.
    stats: Option<Arc<Stats>>,
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
            let _ = self.ivars().view.set(view.clone());
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

            if let Some(run) = self.ivars().run {
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
                        run.seconds as f64,
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
            let teardown = self.shutdown();
            // Duman koşusu deadline'a varmadan da bitebilir: shell kendi
            // çıkarsa (`BT_RUN_SECONDS` betiğin uykusundan uzunsa, ya da
            // gerçek bir shell hemen ölürse) `ChildExit` buraya getirir.
            // Rapor basılmadan çıkmak `make duman`'a hiçbir şey ölçmemiş bir
            // koşuyu exit 0 ile yeşil gösterirdi — kapının sahte yeşil verdiği
            // tek yol buydu.
            if let Some(run) = self.ivars().run {
                self.report_and_exit(run, teardown);
            }
        }
    }

    unsafe impl NSWindowDelegate for AppDelegate {
        #[unsafe(method(windowDidResize:))]
        fn window_did_resize(&self, _n: &NSNotification) {
            self.refresh_geometry();
        }

        // Ekranlar arası taşımada boyut (nokta) değişmez ama ölçek değişir;
        // layer-hosting view'da bunu bizden başka kimse yazmaz.
        #[unsafe(method(windowDidChangeBackingProperties:))]
        fn window_did_change_backing(&self, _n: &NSNotification) {
            self.refresh_geometry();
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
            let teardown = self.shutdown();
            // Zamanlayıcı yalnız `run` doluyken kuruldu; `if let` burada bir
            // dal değil o değişmezin okunması. `expect` olmadı, çünkü burası
            // rapor yolu ve kapanışta bir panik raporun kendisini yutardı.
            if let Some(run) = self.ivars().run {
                self.report_and_exit(run, teardown);
            }
        }
    }
);

/// Duman satırının dört sayacı.
///
/// Yapı, çünkü dördü de sayı: konumsal geçirilseler `hucre` ile `glif` yer
/// değiştirdiğinde **derleme geçerdi** ve sınama da aynı sırayı kullandığı
/// için ikisi birlikte yanılırdı (`/code-review` bulgusu).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Counters {
    /// GPU'nun hatasız bitirdiği kare.
    frames: u64,
    /// Son karede sink'in ürettiği arka plan hücresi (imleç hariç).
    cells: usize,
    /// Son karede çizilen glyph.
    glyphs: usize,
    /// Son karede çizilen alt çizgi / üstü çizili.
    rules: usize,
}

/// Ölçüm defterinin kapanıştaki özeti: halkadan okunmuş, henüz biçimlenmemiş.
///
/// Sayaç yarısı (`ornek`, `dusen`, `elenen`) p95'ten **önce** okunuyor, çünkü
/// [`bt_gpu::Samples::p95_and_worst`] kendini tüketiyor — sıra tipin
/// zorladığı bir şey, yorumun değil.
///
/// # Ölçümün dürüst sınırları
///
/// Bu liste `docs/OLCUMLER.md` → `## Yöntem`'in **kaynağıdır**: o dosya henüz
/// yok (ilk `/measure` kuracak, 005 R7.3) ve kurulurken buradan taşınır.
/// Buradaki koşu sayıları da o taşımaya kadar geçici: sayının asıl sahibi o
/// dosya, burası **emanetçi**. Hepsi 2026-09-12, `profil=debug`, tek makine.
///
/// Her kalem **kapsam** ya da **açık kalem** diye etiketli, çünkü okuyanın
/// yapacağı şey farklı: kapsam bilinip geçilir, açık kalem eylem bekler.
///
/// - **Kapsam — `acilis=` iki ucundan da kısa.** Başı `main()`'in ilk satırı,
///   süreç başlangıcı değil; sonu ilk **tamamlanan** kare
///   (`addCompletedHandler`), sunulan kare değil. İkisi de
///   [`bt_gpu::Stats::startup`]'ta yazılı. Ölçüm halkalarının ayrılması bu
///   aralığın **içinde** kalıyor.
/// - **Kapsam — bugüne kadarki bütün sayılar `profil=debug`.** Taban
///   değiller; `/measure` release şart koşuyor ve jeton hangi profilde
///   olduğunu satırın kendisinde söylüyor (R5.3).
/// - **Kapsam — düşen kare ölçülmüyor** (R3, kapsam dışı). `dusen=` halkaya
///   sığmayan **örnek**, atlanan kare değil; kuralı [`bt_gpu::Samples`]'ın
///   doc'unda.
/// - **Açık kalem (jeton boşluğu) — CPU'nun elenen örneği sayılıyor ama
///   basılmıyor.** `Stats::record_cpu` sıfır uzunluklu bir aralığı eliyor ve
///   [`bt_gpu::Samples::rejected`]'a yazıyor; rapor bu sayacı yalnız GPU
///   sütunu için (`gpu_elenen=`) okuyor. Yani elenen bir CPU örneği `ornek=`'i
///   sessizce düşürüyor ve satırda sebebini söyleyen jeton **yok** — GPU
///   tarafında tam bu körlüğü kapatmak için eklenen sayacın CPU'da eksik
///   kalmış hâli (R5.2). Bugün zararsız: eleme yalnız sıfır uzunluklu
///   aralıkta oluyor ve ölçülen koşuların hiçbirinde görülmedi. Kapatmanın
///   yolu belli ve bedeli de belli: bir `cpu_elenen=` jetonu eklemek, yani
///   **makine sözleşmesini genişletmek** — sözleşme "silinmez, eklenir"
///   dediği için geri alınamaz bir adım, o yüzden ölçülmüş bir ihtiyaç
///   beklemeden atılmadı.
/// - **Açık kalem (kayıtlı kusur) — `kapanis=abandoned`** ölçüm koşularının
///   dörtte birinde çıkıyor (on yedi koşuda dört). Örneklere etkisi **yok**,
///   ama sebebi sıra değil: kapanış halkadan **önce** koşuyor
///   ([`AppDelegate::report_and_exit`]'in "Sıra bilinçli" doc'u). Etkisiz
///   olmasının sebebi `shutdown()`'ın beklemeye girmeden **önce**
///   `link.stop()` çağırması: bekleme boyunca yeni kare **istenmiyor**.
///   Halkanın tamamen durağan olduğu anlamına gelmez — uçuşta kalan bir iki
///   tamamlanma hâlâ düşebilir ve o bir örneklik kayma
///   [`Measured::read`]'de yazılı. Bedeli yalnız koşunun duvar saatinde:
///   `SHUTDOWN_GRACE` kadar ekliyor. Bu ölçümün bir **özelliği değil**,
///   kapanış tasarımının borcu ve çaresi adı konmuş durumda
///   (`Session::spawn`'da master'ın bir kopyası); ayrıntısı `CLAUDE.md`'nin
///   kapanış maddesinde.
/// - **Açık kalem (cevaplanmamış soru) — `kare` ile `istek` iki yükte apayrı
///   davranıyor** ve mekanizması **ölçülmedi** (kapı mı yutuyor, ana thread
///   mi doyuyor, sistem mi link'i kısıyor): duman yükünde `istek ≈ kare + 2`,
///   ölçüm yükünde ikisi **mertebelerce** ayrışıyor. Üstüne, ölçüm yükünün
///   kendisi **aynı komut ve aynı derlemeyle** iki farklı rejim verdi: `kare`
///   bir koşuda onlarda, başka bir koşuda yüzlerde. En olası değişken pencere
///   görünürlüğü ama **doğrulanmadı** (bkz. [`IDLE_FRAME_LIMIT`]). Bir kare
///   süresini yorumlayan taraf hangi rejimde olduğunu satırdan
///   **okuyamıyor** — yani iki koşuyu karşılaştırmadan önce bu cevaplanmalı.
struct Measured {
    /// `main()`'in ilk satırından ilk **tamamlanan** kareye. `None` → hiç kare
    /// bitmedi; iki ucun sınırı [`bt_gpu::Stats::startup`]'ta.
    startup: Option<Duration>,
    /// CPU sütunlarının uzunluğu. Tek sayı, çünkü iki CPU sütunu birlikte
    /// yazılıyor (`Stats::record_cpu`) ve hizaları tipte bağlı.
    cpu_samples: usize,
    /// Halkaya sığmayıp düşen örnek — üç sütunun en yükseği.
    ///
    /// Kuralı ve gerekçesi halkanın yanında ([`bt_gpu::Samples::dropped`]);
    /// burada yalnız uygulanıyor ve **elde duran anlık görüntülerden**:
    /// taze bir okuma, `ornek=` ile `dusen=`'i aynı andan almazdı.
    dropped: u64,
    /// GPU sütununun uzunluğu — CPU'dan **kısa olabilir**.
    gpu_samples: usize,
    /// Metal'in sıfır/NaN damgası yüzünden hiç yazılamayan kare. Bu sayı
    /// olmadan boş bir GPU sütunu "donanım damga vermiyor" ile "hiç kare
    /// çizilmedi"den ayırt edilemezdi.
    gpu_rejected: u64,
    cpu_frame: Option<(Duration, Duration)>,
    cpu_encode: Option<(Duration, Duration)>,
    gpu: Option<(Duration, Duration)>,
}

impl Measured {
    /// Defteri okur. Kapanışta bir kez koşuyor.
    ///
    /// `link.stop()` çoktan çağrıldı, ama **halkaların hepsi durağan değil**:
    /// iki CPU sütununu bu thread yazıyor (yani onlar durağan), GPU sütununu
    /// Metal'in tamamlanma thread'i yazıyor ve uçuşta kalan bir kare rapor
    /// okunurken hâlâ düşebilir. Sonucu bir örneklik kayma: `kare` ile
    /// `gpu_ornek + gpu_elenen` bu yüzden bire kadar ayrışabilir. `ornek=`
    /// jetonu bunu görünür kılıyor; sayıyı yorumlayan taraf eşitlik
    /// beklememeli.
    fn read(stats: &Stats) -> Self {
        let cpu_frame = stats.cpu_frame();
        let cpu_encode = stats.cpu_encode();
        let gpu = stats.gpu();
        Self {
            startup: stats.startup(),
            cpu_samples: cpu_frame.nanos.len(),
            dropped: cpu_frame.dropped.max(cpu_encode.dropped).max(gpu.dropped),
            gpu_samples: gpu.nanos.len(),
            gpu_rejected: gpu.rejected,
            cpu_frame: cpu_frame.p95_and_worst(),
            cpu_encode: cpu_encode.p95_and_worst(),
            gpu: gpu.p95_and_worst(),
        }
    }
}

/// Başarı satırının **bütün** girdisi.
///
/// Satırı kuran fonksiyonun saf olması gerekiyordu: gerçek bir pencere ve
/// display link olmadan sınanabilsin diye. Altı ayrı argüman olarak
/// geçirilseydi `Counters`'ın kaçtığı hatayı bir üst katmanda tekrarlardı.
struct Report {
    counters: Counters,
    /// Atlasın dolu/toplam yuvası. Bir kapı **değil**, sayaç.
    atlas: (usize, usize),
    workload: Workload,
    /// Koşu boyunca istenen kare — çizilen değil.
    requests: u64,
    /// Kapanışın sonucu; `None` → oturum hiç doğmamıştı.
    teardown: Option<Teardown>,
    /// Ölçüm defteri; `None` → kapı kapalıydı (`BT_FRAME_STATS` verilmedi).
    measured: Option<Measured>,
}

impl Report {
    /// Başarı satırı — **saf**, yani gerçek bir pencere olmadan sınanabilir.
    ///
    /// Jeton sözleşmesi: **silinmez, eklenir.** Eski beşli (`kare`, `hucre`,
    /// `glif`, `kural`, `yuva`) ve `yuk` ile `pipeline=ok` yerinde; yenisi
    /// aralarına giriyor.
    ///
    /// **Dil kuralı, tek yerde:** *anahtarlar* Türkçe ve **donmuş** — sözleşme
    /// "silinmez" diyor, yani bugün `kare=`'yi İngilizceleştirmek onu okuyan
    /// her tarafı kırar. *Değerler* İngilizce, çünkü onları okuyan şey bir tanı
    /// metni değil bir `match` kolu ya da CI grep'i (`yuk=smoke|load` ve
    /// `pipeline=ok` bu deseni bu satır doğmadan önce kurmuştu). Türkçe kalan
    /// tek yer **tanı metni**: stderr satırları ve `assert!` gerekçeleri.
    ///
    /// Satır **açıkça** basılıyor (`report_and_exit`'te bir
    /// `println!`); `Drop`'ta boşalan bir tampona bırakılan hiçbir yol yok —
    /// `process::exit` `Drop` koşturmuyor, bekçinin `_exit(70)`'i atexit'i
    /// bile atlıyor (R5.5).
    fn token_line(&self) -> String {
        let Counters {
            frames,
            cells,
            glyphs,
            rules,
        } = self.counters;
        let (used, total) = self.atlas;
        // `profil=` kapı kapalıyken de basılıyor: `make duman` **debug**
        // koşuyor, `/measure` **release** şart koşuyor ve bir debug sayısını
        // taban sanmak ancak satırın kendisi profilini söylerse imkânsız olur
        // (R5.3).
        let profile = if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        };
        let mut line = format!(
            "kare={frames} hucre={cells} glif={glyphs} kural={rules} \
yuva={used}/{total} yuk={workload} istek={requests} kapanis={teardown} \
profil={profile}",
            workload = self.workload.token(),
            requests = self.requests,
            teardown = teardown_token(self.teardown),
        );
        match &self.measured {
            // Kapı kapalıydı. **`ornek=0` değil:** sıfır, "kapı açıktı ama hiç
            // örnek toplanmadı" ile aynı görünürdü ve R5.2'nin kapatmak
            // istediği körlük tam olarak o. Ölçüm jetonları da hiç basılmıyor;
            // sözleşme jetonun yokluğunu okumaya izin veriyor, yalan bir
            // değeri değil.
            None => line.push_str(" ornek=off"),
            Some(m) => {
                // `write!` bir `String`'e hata döndüremez; `let _` onu
                // görünür kılıyor ve rapor yolunda `unwrap` bırakmıyor.
                let _ = write!(
                    line,
                    " ornek={} dusen={} gpu_ornek={} gpu_elenen={} taban={MIN_SAMPLES}",
                    m.cpu_samples, m.dropped, m.gpu_samples, m.gpu_rejected
                );
                push_span(&mut line, "cpu_kare", m.cpu_frame);
                push_span(&mut line, "cpu_encode", m.cpu_encode);
                push_span(&mut line, "gpu", m.gpu);
                // Açılış tek sayı, dağılım değil: koşu başına bir kez olur.
                let _ = match m.startup {
                    Some(startup) => write!(line, " acilis={}", ms(startup)),
                    None => write!(line, " acilis=none"),
                };
            }
        }
        line.push_str(" pipeline=ok");
        line
    }
}

/// Bir sütunun iki jetonu.
///
/// Taban altında sayı **yok** (R5.6): `insufficient` basılır ve sebebi aynı
/// satırdaki `ornek=`/`gpu_ornek=` ile `taban=` çiftinde okunur. İkisi
/// **birlikte** susuyor, çünkü ikisi de aynı `Option`'dan geliyor: taban
/// altında p95 zaten en kötünün kopyasıdır, yani basılacak iki sayı değil bir
/// sayı ve iki ad olurdu.
fn push_span(line: &mut String, name: &str, span: Option<(Duration, Duration)>) {
    let _ = match span {
        Some((p95, worst)) => write!(line, " {name}_p95={} {name}_max={}", ms(p95), ms(worst)),
        None => write!(line, " {name}_p95=insufficient {name}_max=insufficient"),
    };
}

/// Süreyi jeton değerine çevirir: iki ondalıklı milisaniye.
///
/// Tek biçim, `acilis=` dâhil. İki ayrı hassasiyet okuyanı jeton başına kural
/// ezberlemeye zorlardı; makine sözleşmesinin istediği tam tersi.
fn ms(value: Duration) -> String {
    format!("{:.2}ms", value.as_secs_f64() * 1e3)
}

/// `kapanis=` jetonunun değeri.
///
/// Sınır dolan koşu ve panikle biten okuyucu bugüne kadar **yeşil bir
/// satırla** geçiyordu: stderr'de bir satır vardı, jetonda iz yoktu. Her
/// sonuç ayrı bir kelime, çünkü ayrı arıza — `bool` olsaydı okuyan taraf
/// hangisi olduğunu satırın dışında aramak zorunda kalırdı.
///
/// Değerler İngilizce ve varyant adının `kebab-case` hâli; kuralın gerekçesi
/// tek yerde, [`Report::token_line`]'ın doc'unda.
fn teardown_token(teardown: Option<Teardown>) -> &'static str {
    match teardown {
        // Oturum hiç doğmadı: kapanacak bir şey de yoktu.
        None => "none",
        Some(Teardown::Clean) => "clean",
        Some(Teardown::ReaderPanicked) => "reader-panicked",
        Some(Teardown::Abandoned) => "abandoned",
        Some(Teardown::Panicked) => "panicked",
        Some(Teardown::Unbounded) => "unbounded",
        Some(Teardown::AlreadyDone) => "already-done",
    }
}

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
    /// Kapanış yolunda **panik** oldu. Sayaçlar yerinde olabilir ama koşu
    /// yeşil geçemez: projenin "PTY ve ayrıştırma yolunda panik yok" kuralı
    /// ihlal edilmiş demektir ve `kapanis=` jetonunu **hiç kimse okumasa**
    /// bile kapı bunu görmek zorunda.
    ///
    /// [`Teardown::Abandoned`] ve [`Teardown::Unbounded`] buraya **girmez**:
    /// ikisi de kayıtlı borç (çocuk çıkışın içinde takılıyor; OS thread
    /// sınırı) ve ilki ölçüm yükünün dört koşusundan birinde oluyor — kapıya
    /// bağlansaydı `make duman` bilinen bir borç yüzünden kırmızı düşerdi.
    ShutdownPanicked {
        which: &'static str,
    },
}

/// Kapının saf hâli — gerçek bir display link ve pencere istemeden sınanır.
///
/// Karar [`AppDelegate::report_and_exit`]'in gövdesinde kalsaydı sınırın
/// yönünü (8 mi 180 mi, `Load` muaf mı) yalnız `make duman` bilirdi ve hiçbir
/// sınamada yazılı olmazdı.
fn verdict(counters: Counters, workload: Workload, teardown: Option<Teardown>) -> Verdict {
    let Counters {
        frames: n,
        cells: k,
        glyphs: g,
        rules: r,
    } = counters;
    // Panik **sayaçlardan önce** sorulmuyor: eksik bir sayaç daha temel bir
    // arıza ve okuyanı önce oraya göndermek doğru. Ama sayaçlar yerindeyse
    // panik yeşile dönüşemez.
    let panicked = match teardown {
        Some(Teardown::ReaderPanicked) => Some("okuyucu thread"),
        Some(Teardown::Panicked) => Some("kapanış thread'i"),
        _ => None,
    };
    match workload {
        // Ölçüm yükü düz metin akıtıyor: arka plan da kural da **yok** ve
        // olmayacak. İkisini sormak, duman reçetesini hiç koşmayan bir koşuya
        // o reçetenin sayılarını sormak olurdu — kapı her ölçüm koşusunda
        // düşerdi. Kare akışı burada işin kendisi: üst sınır da yok.
        Workload::Load => {
            if n == 0 || g == 0 {
                Verdict::MissingCounter {
                    required: "kare ve glif >0 olmalı",
                }
            } else if let Some(which) = panicked {
                Verdict::ShutdownPanicked { which }
            } else {
                Verdict::Pass
            }
        }
        // Duman reçetesi: dördü de > 0 **ve** kare sayısı üst sınırlı.
        Workload::Smoke => {
            if n == 0 || k == 0 || g == 0 || r == 0 {
                Verdict::MissingCounter {
                    required: "dördü de >0 olmalı",
                }
            } else if n > IDLE_FRAME_LIMIT {
                Verdict::ExcessFrames {
                    limit: IDLE_FRAME_LIMIT,
                }
            } else if let Some(which) = panicked {
                Verdict::ShutdownPanicked { which }
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
        // Halka **yalnız** kapı açıkken ayrılıyor: kapalı kapının bedeli bir
        // `Option` dallanması olmalı, bir ayırma değil (R4.1). Kapasitenin
        // koşu süresinden türemesi de `bt-gpu`'nun işi — tazeleme hızını bilen
        // taraf o.
        let stats = opts
            .run
            .and_then(|run| run.stats_since.map(|since| Stats::new(since, run.seconds)))
            .map(Arc::new);
        let this = Self::alloc(mtm).set_ivars(Ivars {
            renderer,
            surface,
            window: OnceCell::new(),
            view: OnceCell::new(),
            link: OnceCell::new(),
            session: OnceCell::new(),
            wake: Arc::new(ShellWake {
                waker: OnceLock::new(),
            }),
            run: opts.run,
            stats,
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
                // Dallanma **yükü** soruyor, süreyi değil: aynı `Run` hem
                // deadline'ı hem bekçiyi kuruyor ve yük onlardan bağımsız.
                command: self.ivars().run.map(|run| match run.workload {
                    Workload::Smoke => smoke_shell(),
                    // Yükün süresi deadline'la aynı: kısa kalırsa pencere
                    // koşunun kuyruğunda boşa düşer ve ölçüm boşta kare
                    // örnekler. Süresiz yük artık **temsil edilemiyor** —
                    // `Run` süreyi yükün yanında taşıyor, o yüzden eski
                    // `unwrap_or(0)` ve onu savunan `debug_assert` düştü.
                    Workload::Load => load_shell(run.seconds),
                }),
                // Dizin ve yerel **her** oturumda aynı kuralla, süreli koşu
                // dahil: karar tek kollu (`discussion.md` → Karar 6 eki,
                // "istisnasız") ve iki sabit betik de dizine ve yerele bağlı
                // değil — `printf` ile `sleep`, `date` ile `printf`; yolları
                // mutlak ya da `PATH`'ten, çıktıları ASCII.
                working_directory: child::working_directory(),
                env: child::locale_env().into_iter().collect(),
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
        // Fare çevirisi oturumla aynı grid'i görmeli: ölçü ve sayı yukarıdaki
        // `SessionOptions`'a gidenlerin aynısı. `resize` yolunda da aynı üçlü
        // (`refresh_geometry`) birlikte yazılıyor.
        view.set_metrics(grid);
        let link = DisplayLink::new(
            mtm,
            &self.ivars().surface,
            Rc::clone(&self.ivars().renderer),
            session,
            grid.cell,
            self.ivars().stats.clone(),
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
    /// 2. Oturumu kapat: `SIGHUP` + okuyucu thread'in bitişi. **Sınırlı
    ///    bloklar** — en çok `bt-core`'un `SHUTDOWN_GRACE`'i kadar (yarım
    ///    saniye); ölmeyen çocuk arkada bırakılıyor. Tek istisna kapanış
    ///    thread'inin kurulamaması (OS thread sınırı): o dalda sınır yok ve
    ///    kesecek olan yine bekçi. Eskiden sınır **hiç** yoktu ve kesen
    ///    yalnız bekçiydi (`crate::watchdog`); bugün bekçi bu adımın değil
    ///    kapanış yolunun geri kalanının bekçisi.
    ///
    /// `DisplayLink` bilerek **düşürülmüyor**, yalnız durduruluyor. İçindeki
    /// `Waker`'ı Metal'in tamamlanma bloğu da tutuyor ve onun
    /// `MainThreadBound<Retained<CAMetalDisplayLink>>`'i ana thread dışında
    /// düşerse `Drop`'u ana kuyruğa **senkron** iş atıp bekler: ana thread o
    /// sırada 2. adımın beklemesinde olurdu ve ikisi birbirini kilitlerdi.
    /// Kapanışın yeni sınırı bu kilitlenmeyi en çok yarım saniyelik bir
    /// beklemeye indirir ama kuralı kaldırmaz — üstelik tehlikeli thread
    /// listesini **uzatır**: sınır dolduğunda `bt-core`'un `"PTY teardown"`
    /// thread'i `Adapter` üzerinden `Waker`'ın bir kopyasını tutmaya devam
    /// eder (`wake.rs` → Sahiplik). `Ivars` `app.run()`'ı aştığı sürece o son
    /// referans ne Metal'in thread'inde ne kapanış thread'inde olmaz.
    fn shutdown(&self) -> Option<Teardown> {
        // Bekçinin bütçesi **kapanıştan** başlıyor, süreç başından değil:
        // açılış (Metal device, metallib yükleme, ilk pencere) soğuk bir
        // makinede saniyeler sürebilir ve o süre bütçeden düşseydi sağlıklı
        // bir koşu `_exit(70)` ile kırmızı düşerdi.
        if self.ivars().run.is_some() {
            crate::watchdog();
        }
        if let Some(link) = self.ivars().link.get() {
            link.stop();
        }
        // Sonuç raporu besliyor (`kapanis=`): oturum hiç doğmadıysa `None` ve
        // o da bir cevap — kapanacak bir şey yoktu.
        self.ivars().session.get().map(|session| session.shutdown())
    }

    /// Duman koşusunun raporu ve çıkışı — **kapanıştan sonra** çağrılır.
    ///
    /// Sıra bilinçli: `shutdown()` sınırlı da olsa bekler ve o sınırı da aşan
    /// bir kapanışta bekçi süreci 70 ile keser, yani öyle bir kapanışta
    /// `kare=` satırı hiç çıkmaz. Ters sırada `make duman` yeşil bir satırla
    /// kırmızı bir çıkış kodunu birlikte verirdi.
    ///
    /// Satır burada **açıkça** yazılıyor; `Drop`'a güvenen hiçbir yol yok.
    /// `process::exit` `Drop` koşturmaz ve bekçinin `_exit(70)`'i atexit'i
    /// bile atlar.
    ///
    /// `teardown` argüman, ivar değil: kapanışın sonucunu **çağıran** biliyor
    /// ve bir ivar'a saklamak onu ikinci bir kez okunabilir kılardı.
    fn report_and_exit(&self, run: Run, teardown: Option<Teardown>) -> ! {
        let renderer = &self.ivars().renderer;
        // Dört sayaç dört ayrı şey söyler: `kare` GPU'nun hatasız bitirdiği
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
        // Dördü bir **yapıda** taşınıyor, konumsal argüman olarak değil: üçü
        // aynı tip ve yer değiştirseler derleme geçerdi.
        let (n, k, g, r) = (
            renderer.frames(),
            renderer.last_bg_count(),
            renderer.last_glyph_count(),
            renderer.last_rule_count(),
        );
        let counters = Counters {
            frames: n,
            cells: k,
            glyphs: g,
            rules: r,
        };
        // Beşinci jeton `yuva=U/T` bir kapı değil, bir **sayaç**: atlasın kaç
        // yuvasının dolduğunu söylüyor ve `/measure` doluluk oranını ondan
        // okuyacak. Kapıya girmemesinin sebebi anlamı: boş bir atlas da
        // meşrudur (glyph'siz bir kare) ve dolu bir atlas da — arıza eşiği
        // ölçülmeden bilinmiyor, ölçülmemiş sayı da kapıya yazılmaz. `istek=`
        // de öyle: kare **talebi** `kare`'nin göremediği yeri görüyor ama
        // eşiği ölçülmedi.
        let report = Report {
            counters,
            atlas: renderer.atlas_occupancy(),
            workload: run.workload,
            requests: self.ivars().link.get().map_or(0, DisplayLink::requests),
            teardown,
            // Kapı kapalıysa defter hiç doğmadı; `Option` bunu taşıyor ve
            // rapor `ornek=off` diyor — uydurulmuş bir sıfır değil.
            measured: self.ivars().stats.as_deref().map(Measured::read),
        };
        // Jetonlar **yalnız** başarı satırında ve yalnız stdout'ta: makine
        // sözleşmesi o. Hata satırları aynı sayıları taşıyor ama jeton
        // biçiminde değil, yoksa `kare=` arayan bir CI adımı düşen koşudan
        // kare sayısı okurdu.
        let secs = run.seconds;
        match verdict(counters, run.workload, teardown) {
            Verdict::Pass => {
                println!("{}", report.token_line());
                std::process::exit(0);
            }
            // Ayrı ileti, çünkü ayrı arıza: burada dört sayacın dördü de
            // yerinde ve okuyanı sıfır aramaya göndermek zaman kaybettirirdi.
            Verdict::ExcessFrames { limit } => eprintln!(
                "bateri: boşta sıfır kare bozuldu — {secs} saniyelik koşuda {n} kare çizildi (kare talebi {}), üst sınır {limit}",
                report.requests
            ),
            Verdict::MissingCounter { required } => eprintln!(
                "bateri: {secs} saniyelik koşuda çizilen kare {n}, üretilen hücre {k}, çizilen glif {g}, çizilen kural {r} ({required})"
            ),
            // Sayaçlar yerinde ama kapanış yolunda panik var: jeton satırı
            // basılmıyor ki `kare=` arayan bir CI adımı bu koşuyu ölçüm
            // sanmasın.
            Verdict::ShutdownPanicked { which } => eprintln!(
                "bateri: kapanış yolunda panik ({which}) — sayaçlar yerinde ama koşu geçerli değil"
            ),
        }
        std::process::exit(1);
    }

    /// Pencere geometrisi oynadı: layer'ı eşle, grid'i güncelle, kare iste.
    ///
    /// Fare girdileri de burada tazeleniyor: view `Ivars.view`'da
    /// `Retained<BateriView>` olarak duruyor. Pencere kapanınca ikisi
    /// birlikte gidiyor — `Ivars` delegate'te, delegate `run()`'un
    /// `Retained`'ında, o da `app.run()`'ı aşıyor.
    fn refresh_geometry(&self) {
        let Some(grid) = self.sync_geometry() else {
            return;
        };
        if let Some(view) = self.ivars().view.get() {
            view.set_metrics(grid);
        }
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

    fn report(counters: Counters, workload: Workload) -> Report {
        Report {
            counters,
            atlas: (13, 2048),
            workload,
            requests: 4,
            teardown: Some(Teardown::Clean),
            measured: None,
        }
    }

    fn smoke_counters() -> Counters {
        Counters {
            frames: 1,
            cells: 8,
            glyphs: 6,
            rules: 15,
        }
    }

    #[test]
    fn smoke_counts_unchanged() {
        // Duman sözleşmesinin **bit bit** aynı kalması gereken yarısı. Jeton
        // eklemek serbest; bu dördü bu sırayla, bu değerlerle ve satırın
        // **başında** durur — `make duman`'ı okuyan taraf (ve `proje.md`'nin
        // doğrulama tablosu) onları metin olarak arıyor.
        let line = report(smoke_counters(), Workload::Smoke).token_line();
        assert!(
            line.starts_with("kare=1 hucre=8 glif=6 kural=15 "),
            "duman sayaçları oynadı: {line}"
        );
    }

    #[test]
    fn token_line_preserves_old_tokens() {
        // Sözleşme: **silinmez, eklenir.** Rapor genişlerken düşen bir jeton
        // sessizdir — okuyan taraf tanımadığını atlayabilir, kaybolanı
        // arayamaz.
        let line = report(smoke_counters(), Workload::Smoke).token_line();
        for token in [
            "kare=1",
            "hucre=8",
            "glif=6",
            "kural=15",
            "yuva=13/2048",
            "yuk=smoke",
        ] {
            assert!(line.contains(token), "{token} düştü: {line}");
        }
        assert!(line.ends_with(" pipeline=ok"), "{line}");

        // Kapı kapalıyken ölçüm jetonları **yok** ve `ornek=0` da yok: sıfır,
        // "kapı açıktı ama hiç örnek toplanmadı" ile karışırdı ve R5.2'nin
        // kapatmak istediği körlük tam olarak o.
        assert!(line.contains(" ornek=off"), "{line}");
        assert!(!line.contains("cpu_kare_p95"), "{line}");
        assert!(!line.contains("acilis="), "{line}");

        // `yuk=` yükle değişiyor ve dizgi tipin yanında duruyor.
        let load = report(
            Counters {
                frames: 9,
                cells: 0,
                glyphs: 12,
                rules: 0,
            },
            Workload::Load,
        )
        .token_line();
        assert!(load.contains("yuk=load"), "{load}");
    }

    #[test]
    fn measured_tokens_report_every_column() {
        // Kapı açıkken üç sütunun **her biri** kendi jetonunu alıyor ve GPU'nun
        // kısa kalması görünür oluyor: `ornek=` ile `gpu_ornek=` ayrı sayılar,
        // çünkü Metal'in sıfır damgası bir kareyi CPU'ya yazdırıp GPU'ya
        // yazdırmayabiliyor.
        let mut r = report(smoke_counters(), Workload::Load);
        r.measured = Some(Measured {
            startup: Some(Duration::from_millis(284)),
            cpu_samples: 594,
            dropped: 2,
            gpu_samples: 591,
            gpu_rejected: 3,
            cpu_frame: Some((Duration::from_micros(1800), Duration::from_micros(4100))),
            cpu_encode: None,
            gpu: Some((Duration::from_micros(2200), Duration::from_micros(5000))),
        });
        let line = r.token_line();
        for token in [
            "ornek=594",
            "dusen=2",
            "gpu_ornek=591",
            "gpu_elenen=3",
            &format!("taban={MIN_SAMPLES}"),
            "cpu_kare_p95=1.80ms",
            "cpu_kare_max=4.10ms",
            "gpu_p95=2.20ms",
            "gpu_max=5.00ms",
            "acilis=284.00ms",
        ] {
            assert!(line.contains(token), "{token} yok: {line}");
        }
        // Taban altındaki sütun sayı **basmıyor** (R5.6) ve sebebi aynı
        // satırdaki `ornek=`/`taban=` çiftinde okunuyor. p95 ile en kötü
        // birlikte susuyor: az örnekte ikisi zaten aynı elemandır.
        assert!(line.contains("cpu_encode_p95=insufficient"), "{line}");
        assert!(line.contains("cpu_encode_max=insufficient"), "{line}");
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
        let counters = |frames, cells, glyphs, rules| Counters {
            frames,
            cells,
            glyphs,
            rules,
        };
        let clean = Some(Teardown::Clean);
        let smoke = |n, k, g, r| verdict(counters(n, k, g, r), Workload::Smoke, clean);
        let load = |n, k, g, r| verdict(counters(n, k, g, r), Workload::Load, clean);
        let excess = Verdict::ExcessFrames {
            limit: IDLE_FRAME_LIMIT,
        };

        // Bugünkü duman koşusunun ta kendisi: bir kare, sekiz hücre, altı
        // glyph, on beş kural.
        assert_eq!(smoke(1, 8, 6, 15), Verdict::Pass);
        // Sınırın kendisi geçer, bir fazlası düşer. Eski kapı (`n > 0`) sıfırı
        // görüyordu ama fazlayı görmüyordu ve boşta sıfır kareyi bozan bir
        // değişikliğin belirtisi tam olarak fazla kare.
        assert_eq!(smoke(IDLE_FRAME_LIMIT, 8, 6, 15), Verdict::Pass);
        assert_eq!(smoke(IDLE_FRAME_LIMIT + 1, 8, 6, 15), excess);
        // Sağlıklı koşunun **ölçülen** tavanı (2026-09-12, otuz bir koşuda
        // bir kez): dört kare meşru ve geçmeli. Eski sınır (`2`) tam burada
        // doğru bir build'i kırmızıya düşürüyordu.
        assert_eq!(smoke(4, 8, 6, 15), Verdict::Pass);
        // Bozuk koşunun **ölçülen** alt ucu: boşta sıfır kare bilerek
        // bozulduğunda dokuz koşuda en düşük sayı 49'du. Sınır bunu yakalamak
        // zorunda.
        assert_eq!(smoke(49, 8, 6, 15), excess);
        assert_eq!(smoke(354, 8, 6, 15), excess);
        // `Load` yükünde akış işin kendisi: aynı sayı geçmeli. Sınırın yüke
        // bağlı olduğu tek yerde yazılı ve burada sınanıyor.
        assert_eq!(load(354, 8, 6, 15), Verdict::Pass);
        // Ölçüm yükünün **gerçek** sayıları: düz metin akıyor, arka plan ve
        // kural yapısal olarak sıfır. Dört sayaç da sorulsaydı her ölçüm
        // koşusu kırmızı düşerdi. `glif=1836` üç ölçümde de aynı çıktı
        // (2026-09-12); `kare` ise ortama bağlı ve tam bu yüzden `Load`
        // yükünde **kapı yok**: aynı makinede aynı komut bir rejimde 9 (2 sn)
        // ile 21 (5 sn), ötekinde 49–234 (2 sn) ile 597 (5 sn) verdi.
        assert_eq!(load(21, 0, 1836, 0), Verdict::Pass);
        assert_eq!(load(594, 0, 1836, 0), Verdict::Pass);

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
    fn shutdown_panic_cannot_pass_the_gate() {
        // `/code-review` bulgusu: `kapanis=` jetonu görünür oldu ama kapı onu
        // okumuyordu, yani kapanış yolunda panikleyen bir koşu hâlâ
        // `pipeline=ok` basıp 0 ile çıkıyordu — jetonun eklenme gerekçesinin
        // tam tersi.
        let good = Counters {
            frames: 1,
            cells: 8,
            glyphs: 6,
            rules: 15,
        };
        for teardown in [Teardown::ReaderPanicked, Teardown::Panicked] {
            assert!(
                matches!(
                    verdict(good, Workload::Smoke, Some(teardown)),
                    Verdict::ShutdownPanicked { .. }
                ),
                "{teardown:?} yeşil geçemez"
            );
            assert!(
                matches!(
                    verdict(good, Workload::Load, Some(teardown)),
                    Verdict::ShutdownPanicked { .. }
                ),
                "{teardown:?} ölçüm yükünde de yeşil geçemez"
            );
        }

        // Kayıtlı borçlar kapıya **bağlanmadı**: `Abandoned` ölçüm yükünün
        // dört koşusundan birinde oluyor (ölçüldü) ve `make duman`'ı bilinen
        // bir borç yüzünden kırmızıya düşürmek kapıyı işe yaramaz kılardı.
        for teardown in [
            Some(Teardown::Clean),
            Some(Teardown::Abandoned),
            Some(Teardown::Unbounded),
            Some(Teardown::AlreadyDone),
            None,
        ] {
            assert_eq!(verdict(good, Workload::Smoke, teardown), Verdict::Pass);
        }

        // Eksik sayaç panikten **önce** geliyor: okuyanı önce daha temel
        // arızaya göndermek doğru.
        assert_eq!(
            verdict(
                Counters { frames: 0, ..good },
                Workload::Smoke,
                Some(Teardown::Panicked)
            ),
            Verdict::MissingCounter {
                required: "dördü de >0 olmalı"
            }
        );
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
