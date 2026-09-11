//! Kareyi süren şey: `CAMetalDisplayLink` ve onu uzaktan açan `Waker`.
//!
//! Sözleşme tek cümlede: **link paused durur.** Yeni içerik geldiğinde
//! (`Wake::wake` → [`Waker`]) açılır, hasar tükenince callback onu geri
//! kapatır. "Boşta sıfır kare" bu iki satırda yaşıyor; her `setPaused(false)`
//! bir gerekçe ister ve her kare bir durma koşulu taşır.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Instant;

use bt_core::{DEFAULT_BG, DEFAULT_CURSOR, DirtyFlag, Session};
use dispatch2::{DispatchQueue, MainThreadBound};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::{NSObject, NSObjectProtocol, NSRunLoop, NSRunLoopCommonModes};
// Yalnız tamamlanma bloğunun GPU damgaları için: `GPUStartTime`/`GPUEndTime`
// `MTLCommandBuffer` protokolünde ve trait kapsamda olmadan çağrılamaz.
use objc2_metal::MTLCommandBuffer;
use objc2_quartz_core::{CAMetalDisplayLink, CAMetalDisplayLinkDelegate, CAMetalDisplayLinkUpdate};

use crate::frame::Frame;
use crate::renderer::{CellMetrics, Completion};
use crate::stats::Stats;
use crate::{GpuError, Renderer, Surface};

/// **Kare istemenin tek tanımı**: hasar bayrağını dik, link'i aç.
///
/// Her thread'den çağrılabilir; `Clone`, `Send + Sync`. İkisini ayrı ayrı
/// yapan ikinci bir yol bilerek yok — bayraksız açılan link "hasar yok" deyip
/// anında geri uyur, uyandırılmayan bayrak da kimseyi çizmeye çağırmaz.
#[derive(Clone)]
pub struct Waker {
    inner: Arc<WakerInner>,
}

struct WakerInner {
    /// Hasar bayrağı — oturumun kendisi **değil**.
    ///
    /// `Arc<Session>` (hatta `Weak`, çünkü `upgrade()` onu çağrı süresince
    /// maddileştirir) burada olamaz: bu gövdeyi okuyucu thread de, Metal'in
    /// tamamlanma thread'i de tutuyor ve son güçlü referans oralardan birinde
    /// düşerse `Drop for Session` → `shutdown()` o thread'de koşar — `join`
    /// artık ayrı bir thread'de ve sınırlı, yani panik değil ama yarım
    /// saniyelik bir durma ve hiç bitmeyen bir kapanış.
    /// `wake.rs`'in Sahiplik paragrafı bunu adıyla yasaklıyor.
    dirty: DirtyFlag,
    /// `Retained<CAMetalDisplayLink>` kendiliğinden `Send` değil;
    /// `MainThreadBound` erişimi `MainThreadMarker`'a bağlayarak taşımayı
    /// güvenli kılıyor — ve kimin dokunabileceğini tipte yazıyor.
    ///
    /// **Ama `Drop`'u ana thread dışında bloklar:** ana kuyruğa `exec_sync`
    /// ile iş atıp bekler. Bu gövdeyi Metal'in tamamlanma bloğu da tutuyor,
    /// yani son referans orada düşerse ve ana thread o sırada kapanışta
    /// bekliyorsa ikisi birbirini kilitler. Kapanış yolu bu yüzden
    /// [`DisplayLink::stop`] çağırır ve `DisplayLink`'i **düşürmez**: son
    /// referans hep ana thread'de kalır.
    link: MainThreadBound<Retained<CAMetalDisplayLink>>,
    /// Kare çizilir mi, ritim döner mi.
    gate: Gate,
    /// Ana kuyrukta bekleyen bir "aç" işi var mı.
    ///
    /// Kareler zaten birleşiyordu, **dispatch'ler birleşmiyordu**: alacritty
    /// `Wakeup`'ı işlenen her ≤64 KiB için ve her okuma turunun sonunda
    /// yolluyor, yani sürekli çıktıda saniyede binlerce kez. Her biri bir
    /// kapanış kutulaması, bir kuyruk girişi ve **ana thread'in uyandırılması**
    /// demekti — hepsi aynı idempotent `setPaused(false)` için. PTY yükü bu
    /// yolla doğrudan çizim thread'inin ritmine giriyordu.
    pending: AtomicBool,
}

impl Waker {
    /// Herhangi bir thread'den çağrılabilir; işi ana kuyruğa atar ve **hemen
    /// döner**. `Wake` sözleşmesi gereği bloklamaz, kilit almaz.
    ///
    /// Uçuşta bir iş varken gelen uyandırmalar **dispatch** düzeyinde düşer ve
    /// bu kayıpsızdır: bayrak her çağrıda dikilir, düşen uyandırma da bayrağı
    /// henüz `setPaused(false)` yapmamış bir bloğun önüne düşer (blok sırayı
    /// `pending` → `setPaused` diye kuruyor), yani link her hâlükârda açılır.
    pub fn wake(&self) {
        // Hasar HER ZAMAN dikilir; görünmezken yalnız link açılmaz. Bayrak
        // tüketilmediği için görünürlük dönünce birikmiş hasar çizilir.
        self.inner.dirty.mark();
        if !self.inner.gate.is_open() {
            return;
        }
        if self.inner.pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let inner = Arc::clone(&self.inner);
        DispatchQueue::main().exec_async(move || {
            // audit: ana kuyrukta koşan blok tanımı gereği ana thread'dedir.
            let mtm = MainThreadMarker::new().expect("ana kuyruk ana thread'dir");
            inner.pending.store(false, Ordering::Release);
            // Kapı **burada da** okunuyor: bu blok kuyruğa girdikten sonra
            // pencere örtülmüş ya da link durdurulmuş olabilir. Okumasaydı
            // link bir kez açılır, bir vsync callback'i ve bir drawable
            // ödenirdi — ve `stop()` sonrası bu, `invalidate`'in ardından
            // gelen `setPaused(false)`'un etkisiz olduğu varsayımına
            // dayanmak olurdu; kodun geri kalanı o varsayımı bilerek yapmıyor.
            if !inner.gate.is_open() {
                return;
            }
            inner.link.get(mtm).setPaused(false);
        });
    }

    fn gate(&self) -> &Gate {
        &self.inner.gate
    }
}

/// Kare istemenin açık/kapalı kapısı — **durma politikasının tamamı**.
///
/// `FailureStreak` gibi ayrı bir tip ve aynı sebeple: ObjC'siz, kilitsiz ve
/// platformsuz olduğu için sınanabilir; `Waker`'a gömülü kalsaydı yalnız
/// gerçek bir pencereyle denenebilirdi.
///
/// Kapı **her iki** tarafta da okunur: çizim tarafında (callback erken döner)
/// ve uyandırma tarafında ([`Waker::wake`]). Yalnız çizim tarafında olsaydı
/// örtülü pencerede konuşkan bir shell link'i tazeleme hızında kaldırıp
/// yatırırdı — kare çizilmez ama her vsync'te bir ana thread callback'i ve
/// `CAMetalDisplayLink`'in callback'ten önce aldığı bir drawable ödenir.
/// Çizim durur, ritim durmaz; sözleşmenin harfi kalır, ruhu gider.
struct Gate {
    /// Pencere görünür mü. İki yönlü: `windowDidChangeOcclusionState:` hem
    /// örtülmeyi hem geri dönmeyi bildirir.
    open: AtomicBool,
    /// Kalıcı durdurma mandalı — bir kez iner, bir daha kalkmaz.
    ///
    /// `open = false` ile aynı şey **değil**: kapanışta pencere delegate'i
    /// sökülmüyor, yani `shutdown()`'tan sonra düşen bir görünürlük bildirimi
    /// kapıyı geri açar ve bekleyen ana thread'e iş atılmaya devam ederdi.
    stopped: AtomicBool,
}

impl Gate {
    fn new() -> Self {
        Self {
            open: AtomicBool::new(true),
            stopped: AtomicBool::new(false),
        }
    }

    /// Mandal **okuma** tarafında sorgulanıyor, yazma tarafında değil: iki
    /// bayrağı ayrı ayrı okuyup yazmak (`set_open` mandalı görmez → `stop`
    /// koşar → `set_open` kapıyı açar) durdurulmuş bir kapıyı geri açardı ve o
    /// yarış tam da mandalın var olma sebebini yok ederdi.
    fn is_open(&self) -> bool {
        !self.stopped.load(Ordering::Acquire) && self.open.load(Ordering::Acquire)
    }

    fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }

    /// Görünürlük bildirimi. Sıra önemli: `true`'ya geçerken bunu **önce**
    /// yazan taraf, hemen ardından gelen `request_frame`'in kapıdan geçmesini
    /// garanti eder.
    fn set_open(&self, open: bool) {
        self.open.store(open, Ordering::Release);
    }

    /// Mandalı indirir; bu andan sonra `set_open` ne yazarsa yazsın kapı kapalı.
    fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
    }
}

/// Çizilemeyen karenin politikası: **tek** yer.
///
/// Kare iki ayrı yerde düşebilir — encode edilemeden (senkron `Err`) ya da
/// GPU'da (asenkron, Metal'in thread'inde) — ama ikisi de aynı sınıftır ve
/// aynı cevabı ister. Politikayı iki kez yazmanın bedeli teorik değil: senkron
/// kol kendi durağını (`setPaused(true)`) icat ettiğinde, bu arada okuyucunun
/// diktiği bir hasar bayrağının üstüne link'i uyutup kareyi yutabiliyordu.
/// Durak artık tek: `needs_update`'in "hasar yok → uyu" dalı.
struct Retry {
    waker: Waker,
    streak: FailureStreak,
}

impl Retry {
    /// Kare çizilemedi. İlk hatada bir kare daha istenir; art arda
    /// ikincisinde **hiçbir şey yapılmaz** ve durak kendiliğinden devreye
    /// girer (bkz. [`FailureStreak`]).
    fn draw_failed(&self, e: &GpuError) {
        eprintln!("bateri: kare çizilemedi: {e}");
        if self.streak.failed() {
            self.waker.wake();
        }
    }
}

/// Art arda çizilemeyen kare sayacı — **durma koşulunun tamamı**.
///
/// Ayrı bir tip çünkü sınanabilir olması gerekiyordu: politikanın kendisi
/// ObjC'siz, kilitsiz ve platformsuz; `Waker`'a gömülü kalsaydı yalnız
/// gerçek bir pencereyle denenebilirdi.
#[derive(Default)]
struct FailureStreak(AtomicU32);

impl FailureStreak {
    /// Kare tamamlandı: bütçe geri verilir.
    fn succeeded(&self) {
        self.0.store(0, Ordering::Release);
    }

    /// Hata bildirir. `true` → bir kare daha istenir. Art arda ikinci hatada
    /// `false`: bayrak dikilmediği için sıradaki callback "hasar yok" bulur,
    /// link'i uyutur ve sıradaki `Wakeup` beklenir. Bu olmadan kalıcı bir
    /// hata "dik, dene, düş" döngüsünü tazeleme hızında sonsuza çevirirdi.
    fn failed(&self) -> bool {
        self.0.fetch_add(1, Ordering::AcqRel) == 0
    }
}

/// Delegate'in durumu. Ana thread'e ait olanlar `Cell`/`RefCell`; `retry`
/// paylaşılıyor çünkü onu Metal'in tamamlanma thread'i de çağırır.
struct LinkIvars {
    /// `Rc`, `Arc` değil: `Renderer` artık `Sync` değil. Kaldırılabilir sebep
    /// glyph atlasının `CFRetained<CTFont>`'u (`Send` değil), **yapısal**
    /// sebep atlası saran `RefCell` — tamamen thread-güvenli bir fontla bile
    /// `Arc` "başka thread'e geçebilir" diye yanlış bir söz verirdi. `retry`
    /// ile `session` gerçekten geçtikleri için `Arc` kalıyor.
    renderer: Rc<Renderer>,
    session: Arc<Session>,
    retry: Arc<Retry>,
    /// Kapının çizim tarafı buradan okunuyor; gövdenin tek sahibi `Waker`
    /// (uyandırma tarafı da aynı kapıya bakmak zorunda).
    waker: Waker,
    /// Tamamlanma bloğu kurulumda bir kez ayrılır ve burada yaşar.
    completion: Completion,
    /// Ölçüm kapısı. `None` → kapı kapalı ve kare yolu bu phase'den **önceki**
    /// hâliyle koşar: tek bir saat okuması bile yok (R4.1). Kapı açıkken de
    /// aynı gövdeyi tamamlanma bloğu paylaşıyor (`Arc`), çünkü GPU deltası
    /// Metal'in thread'inde doğuyor.
    stats: Option<Arc<Stats>>,
    /// Kare listesi uzun ömürlü: her karede `clear` ile dolar, ayrılan yer
    /// korunur (kare başına yeniden ayırma yok).
    frame: RefCell<Frame>,
    /// Demet değil `CellMetrics`: ölçü `Renderer::cell_metrics`'ten
    /// `bt-shell` üzerinden buraya tip olarak geliyor ve **saklanırken de**
    /// tip kalıyor. Saklanan bu değer yalnız `Frame::clear`'a girerken
    /// demete iniyor, çünkü kare kurucusu `#[repr(C)]` tarafına sayı yazıyor.
    /// (`resize`'ın `Session::resize`'a geçirdiği demet başka bir değer:
    /// oraya **gelen** ölçü gider, saklanan değil — kabul edilmeyen bir
    /// boyut buraya hiç yazılmaz.)
    cell: Cell<CellMetrics>,
}

define_class!(
    // SAFETY: NSObject alt sınıflama şartı taşımaz; LinkDelegate Drop uygulamaz.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriLinkDelegate"]
    #[ivars = LinkIvars]
    struct LinkDelegate;

    unsafe impl NSObjectProtocol for LinkDelegate {}

    unsafe impl CAMetalDisplayLinkDelegate for LinkDelegate {
        /// Link ana run loop'a eklendiği için bu callback **ana thread'de**
        /// koşar; `MainThreadOnly` sınıf o sözleşmeyi tipte tutuyor.
        #[unsafe(method(metalDisplayLink:needsUpdate:))]
        fn needs_update(&self, link: &CAMetalDisplayLink, update: &CAMetalDisplayLinkUpdate) {
            let iv = self.ivars();
            // Görünmeyen pencereye çizmek boşa iş değil, pil sözleşmesinin
            // ihlali: örtülü pencerede konuşkan bir shell her tazelemede tam
            // bir kare çizdirirdi.
            if !iv.waker.gate().is_open() {
                link.setPaused(true);
                return;
            }
            // audit: callback ana thread'e bağlı ve yeniden girilmez; sink
            // `Session`'a geri girmiyor, yani ikinci bir ödünç doğmuyor.
            let mut frame = iv.frame.borrow_mut();
            frame.clear(iv.cell.get().cell_px());
            // CPU **iki** aralık ölçülüyor, bir değil: kilit beklemesi
            // `session.frame`'in içinde, encode ise `draw`'ın. Tek aralık
            // ikisini toplar ve ayrımı yok eder (R3.1).
            //
            // Kapı kapalıyken saat **hiç** okunmuyor (R4.1): `then` de `map`
            // de closure'ı yalnız dolu tarafta koşturuyor, yani kapalı kapının
            // bedeli bir dallanma.
            let t0 = iv.stats.is_some().then(Instant::now);
            // Hasar yoksa encode ve commit'i hiç yapmıyoruz. Drawable'ı bu
            // tasarruf kapsamaz: `CAMetalDisplayLink` onu callback'ten ÖNCE
            // alıp `update`'in içine koyuyor, `drawable()`'ı çağırmamak alımı
            // iptal etmiyor. (Bu yüzden `nextDrawable`'ın `Option`'ı ve onun
            // `GpuError::NoDrawable`'ı da kalktı: `update.drawable()` başlıkta
            // `nonnull` ve objc2 onu `Option`suz üretiyor.)
            let Some(cursor) = iv.session.frame(|cell| frame.push(cell)) else {
                // Boşta sıfır kare: yeni içerik yok, link uyur. Sıradaki
                // `Wakeup` onu `Waker` üzerinden geri açar.
                //
                // Örnek de **yazılmıyor** ve bu bir dal değil, yolun şekli:
                // bu karede `draw` hiç koşmadı, "encode = 0 ns" diye sahte bir
                // örnek p95'i aşağı çekerdi.
                link.setPaused(true);
                return;
            };
            frame.push_cursor(cursor, DEFAULT_CURSOR);
            // Birinci aralık burada kapanıyor — `push_cursor`'dan **sonra**:
            // imleci listeye koymak sink işidir, encode değil. Damga bir satır
            // yukarıda alınsaydı `cpu_encode` `draw`'ın yanında onu da ölçer
            // ve jetonun adı yalan söylerdi. Çift tek bir `Option`'da taşınıyor
            // ki "ikisi de var ya da hiçbiri" temsil edilebilir tek durum olsun.
            let spans = t0.map(|t0| (t0, Instant::now()));

            // `frame()` bayrağı çizim başlamadan tüketti; hata hâlinde geri
            // dikilmezse bu içerik bir daha istenmez ve pencere bayat kalır.
            // Senkron ve asenkron hata aynı kapıdan geçiyor.
            let drawn = iv
                .renderer
                .draw(&update.drawable(), DEFAULT_BG, &frame, &iv.completion);
            // Encode aralığı `draw`'ın dönüşüyle kapanıyor: ikinci damga
            // buraya, karar dallarından **önce** düşüyor.
            let spans = spans.map(|(t0, t1)| (t1 - t0, Instant::now() - t1));
            match drawn {
                // Örnek yalnız **yola çıkan** karede yazılır: encode
                // edilemeyen kare hiçbir şey ölçmedi.
                Ok(()) => {
                    if let Some((stats, (cpu_frame, cpu_encode))) = iv.stats.as_ref().zip(spans) {
                        stats.record_cpu(cpu_frame, cpu_encode);
                    }
                }
                Err(e) => iv.retry.draw_failed(&e),
            }
        }
    }
);

impl LinkDelegate {
    fn new(mtm: MainThreadMarker, ivars: LinkIvars) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ivars);
        // SAFETY: NSObject'in init'i argümansızdır ve ivar'lar set edildi.
        unsafe { msg_send![super(this), init] }
    }
}

/// Ekranın tazeleme ritmine bağlı kare sürücüsü.
///
/// Sahiplik zinciri: bu yapı link'i ve delegate'i tutar, delegate `Session`'ı
/// ve `Renderer`'ı tutar. Link'in `delegate` özelliği **zayıftır**, yani
/// çember kapanmaz: `Session` → `Wake` → [`Waker`] → link yolu geri delegate'e
/// güçlü bir referansla dönmez.
pub struct DisplayLink {
    link: Retained<CAMetalDisplayLink>,
    /// Zayıf `delegate` özelliğinin gerçek sahibi; düşerse callback susar.
    delegate: Retained<LinkDelegate>,
    waker: Waker,
}

impl DisplayLink {
    /// Ana thread'de kurulur: link ana run loop'a eklenir ve callback'in ana
    /// thread'de koşacağı sözleşmesi böyle doğar.
    pub fn new(
        mtm: MainThreadMarker,
        surface: &Surface,
        renderer: Rc<Renderer>,
        session: Arc<Session>,
        cell: CellMetrics,
        stats: Option<Arc<Stats>>,
    ) -> Self {
        let link =
            CAMetalDisplayLink::initWithMetalLayer(CAMetalDisplayLink::alloc(), surface.layer());
        let waker = Waker {
            inner: Arc::new(WakerInner {
                dirty: session.dirty_flag(),
                link: MainThreadBound::new(link.clone(), mtm),
                gate: Gate::new(),
                pending: AtomicBool::new(false),
            }),
        };
        let retry = Arc::new(Retry {
            waker: waker.clone(),
            streak: FailureStreak::default(),
        });
        let completion = {
            let retry = Arc::clone(&retry);
            // Blok kare başına kurulmuyor (bkz. `Renderer::completion`), yani
            // ölçüm gövdesi de kurulumda bir kez giriyor: `Arc` ile, tıpkı
            // `retry` gibi (R3.2 — closure ile kare damgası yakalanamaz).
            let stats = stats.clone();
            renderer.completion(move |result| match result {
                Ok(cmd) => {
                    retry.streak.succeeded();
                    // Ölçüm kapısı **burada**: kapalıyken tek bir ObjC çağrısı
                    // bile yapılmıyor (R4.1). Açılış damgası da burada
                    // kapanıyor, `draw`'da değil — ölçülen şey "main'den ilk
                    // **tamamlanan** kareye" ve commit etmek bitirmek değildir.
                    if let Some(stats) = &stats {
                        stats.mark_startup();
                        // `GPUEndTime - GPUStartTime` Metal'in kendi saati; CPU
                        // damgasıyla ilişkilendirilmiyor, çünkü soru "hangi
                        // kare" değil **dağılım**.
                        stats.record_gpu(cmd.GPUStartTime(), cmd.GPUEndTime());
                    }
                }
                Err(e) => retry.draw_failed(&e),
            })
        };
        let delegate = LinkDelegate::new(
            mtm,
            LinkIvars {
                renderer,
                session,
                retry,
                waker: waker.clone(),
                completion,
                stats,
                frame: RefCell::new(Frame::default()),
                cell: Cell::new(cell),
            },
        );
        link.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        // Paused doğar: ilk kareyi de isteyen olmalı (`request_frame`).
        link.setPaused(true);
        // SAFETY: ana run loop'a ana thread'den ekleniyor (`mtm`). Common
        // modes: canlı boyutlandırma run loop'u tracking moduna sokar,
        // varsayılan modda eklenen link orada susardı.
        unsafe { link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes) };
        Self {
            link,
            delegate,
            waker,
        }
    }

    /// Başka thread'lerden kare istemenin yolu; `Wake` uygulaması bunu tutar.
    pub fn waker(&self) -> Waker {
        self.waker.clone()
    }

    /// Bir kare iste.
    ///
    /// Çağıranını ilgilendiren, grid'in değiştiği değil, **çizilmiş olanın
    /// artık geçerli olmadığıdır**: drawable boyutu oynadı, örtülme kalktı.
    /// Bu yüzden hasar bayrağını da diker — ve bunu [`Waker`] ile yapar,
    /// yani "kare iste"nin tek bir tanımı vardır. Bedeli tek karedir; durma
    /// koşulu callback'in kendisi.
    pub fn request_frame(&self) {
        self.waker.wake();
    }

    /// Pencerenin görünürlüğü değişti.
    ///
    /// Görünmezken hem çizim hem **ritim** durur: link uyutulur, callback
    /// erken döner ve `Waker` de link'i bir daha hiç açmaz (hasarı yine de
    /// diker). Görünürlük dönünce bir kare istenir — compositor örtülüyken
    /// layer içeriğini atmış olabilir, içerik aynı olsa da yeniden çizilmeli.
    pub fn set_visible(&self, visible: bool) {
        self.waker.gate().set_open(visible);
        if visible {
            self.request_frame();
        } else {
            self.link.setPaused(true);
        }
    }

    /// Ritmi **kalıcı olarak** keser: uyandırma mandalı iner, link durur ve
    /// run loop'tan çıkar. Geri dönüşü yok — `set_visible(true)` de artık
    /// hiçbir şey yapmaz, ve bu bir söz değil `stopped` mandalının kendisi.
    ///
    /// Kapanış yolu bunu `Drop` yerine çağırır çünkü `DisplayLink`'in kendisi
    /// kapanış boyunca **yaşamak zorunda** (gerekçe `bt-shell`'in kapanış
    /// sırasında). Uyandırma tarafı da kapanıyor: açık kalsaydı okuyucunun
    /// son `Wakeup`'ları ana kuyruğa iş atmaya devam eder ve kapanışta bekleyen
    /// ana thread'i meşgul ederdi.
    pub fn stop(&self) {
        // `invalidate` Apple'ın belgelerinde tek atımlık bir sökme; ikinci kez
        // çağrılınca ne olduğu yazmıyor. İdempotentliği varsaymak yerine
        // mandalın kendisiyle sağlıyoruz — `Drop` de buradan geçiyor.
        if self.waker.gate().is_stopped() {
            return;
        }
        self.waker.gate().stop();
        self.link.setPaused(true);
        self.link.invalidate();
    }

    /// Pencere geometrisi oynadı: grid'i ve hücre boyutunu güncelle, kare iste.
    ///
    /// `Session::resize` boyutun **hiçbir** bileşeni (sütun, satır, hücre
    /// piksel boyutu) değişmediyse erken döner ve hiçbir şey işaretlemez,
    /// oysa drawable boyutu değişmiş olabilir: hücre sınırını geçmeyen bir
    /// sürükleme grid'i aynı bırakır, `windowDidChangeBackingProperties:`
    /// ölçek oynamadan da atabilir. Kareyi bu yüzden `request_frame`
    /// koşulsuz istiyor; yoksa layer eski drawable'ı gerdirir.
    ///
    /// Hücre piksel boyutu **yalnız oturum kabul ederse** uygulanır: dejenere
    /// boyut yoksayılıyor (simge durumundaki pencere 0 sütun hesaplatır) ve
    /// onu burada uygulamak grid'i eski ölçüde bırakıp çizimi yeni ölçüye
    /// kaydırırdı — PTY'nin bildiği `TIOCSWINSZ` ile de ayrışırdı.
    pub fn resize(&self, cols: u16, rows: u16, cell: CellMetrics) {
        let iv = self.delegate.ivars();
        if iv.session.resize(cols, rows, cell.cell_px()) {
            iv.cell.set(cell);
        }
        self.request_frame();
    }
}

impl Drop for DisplayLink {
    fn drop(&mut self) {
        // Run loop link'i kendi tutar: `invalidate` çağrılmazsa callback ekran
        // tazeleme hızında atmaya devam eder ve delegate zayıf olduğu için
        // sessizce hiçbir şey çizmez — pil giden, belirtisi olmayan tam da o
        // döngü. Ana thread: `DisplayLink` `Send` değil, doğduğu yerde düşer.
        // `stop` mandalıyla korumalı: kapanış yolundan zaten çağrılmışsa
        // burada hiçbir şey yapmaz.
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stopped_gate_does_not_reopen_on_visibility() {
        // Kapanışta pencere delegate'i sökülmüyor: `stop()`'tan sonra düşen
        // bir `windowDidChangeOcclusionState:` kapıyı geri açsaydı, kapanışta
        // `shutdown()`'ın `join`'inde bekleyen ana thread'e iş atılmaya devam
        // ederdi. Mandal bunu koda bağlıyor, yorum cümlesine değil.
        let gate = Gate::new();
        assert!(gate.is_open(), "link görünür pencereyle doğar");

        gate.set_open(false);
        assert!(!gate.is_open(), "örtülen pencere kapıyı kapatır");
        gate.set_open(true);
        assert!(gate.is_open(), "örtülme kalkınca kapı geri açılır");

        gate.stop();
        assert!(!gate.is_open());
        gate.set_open(true);
        assert!(!gate.is_open(), "durdurulmuş kapı bildirimle geri açılmaz");
    }

    #[test]
    fn stop_condition_kicks_in_on_second_failure() {
        // Checklist'in "durma koşulu zorunlu" maddesi bu sınamayla bağlı:
        // kalıcı bir çizim hatası kare talebini tazeleme hızında tekrarlarsa
        // belirtisi yok, faturası pil. Politika burada, ObjC'siz.
        let streak = FailureStreak::default();
        assert!(streak.failed(), "ilk hata bir kez daha denenir");
        assert!(!streak.failed(), "art arda ikinci hata kare talebini keser");
        assert!(!streak.failed(), "sonrası da kesik kalır");

        streak.succeeded();
        assert!(streak.failed(), "tamamlanan kare bütçeyi geri verir");
    }
}
