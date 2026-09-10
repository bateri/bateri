//! Kareyi süren şey: `CAMetalDisplayLink` ve onu uzaktan açan `Waker`.
//!
//! Sözleşme tek cümlede: **link paused durur.** Yeni içerik geldiğinde
//! (`Wake::wake` → [`Waker`]) açılır, hasar tükenince callback onu geri
//! kapatır. "Boşta sıfır kare" bu iki satırda yaşıyor; her `setPaused(false)`
//! bir gerekçe ister ve her kare bir durma koşulu taşır.

use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use bt_core::{DEFAULT_BG, DEFAULT_CURSOR, DirtyFlag, Session};
use dispatch2::{DispatchQueue, MainThreadBound};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::{NSObject, NSObjectProtocol, NSRunLoop, NSRunLoopCommonModes};
use objc2_quartz_core::{CAMetalDisplayLink, CAMetalDisplayLinkDelegate, CAMetalDisplayLinkUpdate};

use crate::frame::Frame;
use crate::renderer::Completion;
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
    /// düşerse `Drop for Session` → `shutdown()` → `join()` o thread'de koşar.
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
    kapi: Kapi,
    /// Ana kuyrukta bekleyen bir "aç" işi var mı.
    ///
    /// Kareler zaten birleşiyordu, **dispatch'ler birleşmiyordu**: alacritty
    /// `Wakeup`'ı işlenen her ≤64 KiB için ve her okuma turunun sonunda
    /// yolluyor, yani sürekli çıktıda saniyede binlerce kez. Her biri bir
    /// kapanış kutulaması, bir kuyruk girişi ve **ana thread'in uyandırılması**
    /// demekti — hepsi aynı idempotent `setPaused(false)` için. PTY yükü bu
    /// yolla doğrudan çizim thread'inin ritmine giriyordu.
    bekleyen: AtomicBool,
}

impl Waker {
    /// Herhangi bir thread'den çağrılabilir; işi ana kuyruğa atar ve **hemen
    /// döner**. `Wake` sözleşmesi gereği bloklamaz, kilit almaz.
    ///
    /// Uçuşta bir iş varken gelen uyandırmalar **dispatch** düzeyinde düşer ve
    /// bu kayıpsızdır: bayrak her çağrıda dikilir, düşen uyandırma da bayrağı
    /// henüz `setPaused(false)` yapmamış bir bloğun önüne düşer (blok sırayı
    /// `bekleyen` → `setPaused` diye kuruyor), yani link her hâlükârda açılır.
    pub fn wake(&self) {
        // Hasar HER ZAMAN dikilir; görünmezken yalnız link açılmaz. Bayrak
        // tüketilmediği için görünürlük dönünce birikmiş hasar çizilir.
        self.inner.dirty.mark();
        if !self.inner.kapi.acik() {
            return;
        }
        if self.inner.bekleyen.swap(true, Ordering::AcqRel) {
            return;
        }
        let inner = Arc::clone(&self.inner);
        DispatchQueue::main().exec_async(move || {
            // audit: ana kuyrukta koşan blok tanımı gereği ana thread'dedir.
            let mtm = MainThreadMarker::new().expect("ana kuyruk ana thread'dir");
            inner.bekleyen.store(false, Ordering::Release);
            // Kapı **burada da** okunuyor: bu blok kuyruğa girdikten sonra
            // pencere örtülmüş ya da link durdurulmuş olabilir. Okumasaydı
            // link bir kez açılır, bir vsync callback'i ve bir drawable
            // ödenirdi — ve `stop()` sonrası bu, `invalidate`'in ardından
            // gelen `setPaused(false)`'un etkisiz olduğu varsayımına
            // dayanmak olurdu; kodun geri kalanı o varsayımı bilerek yapmıyor.
            if !inner.kapi.acik() {
                return;
            }
            inner.link.get(mtm).setPaused(false);
        });
    }

    fn kapi(&self) -> &Kapi {
        &self.inner.kapi
    }
}

/// Kare istemenin açık/kapalı kapısı — **durma politikasının tamamı**.
///
/// `Ardisik` gibi ayrı bir tip ve aynı sebeple: ObjC'siz, kilitsiz ve
/// platformsuz olduğu için sınanabilir; `Waker`'a gömülü kalsaydı yalnız
/// gerçek bir pencereyle denenebilirdi.
///
/// Kapı **her iki** tarafta da okunur: çizim tarafında (callback erken döner)
/// ve uyandırma tarafında ([`Waker::wake`]). Yalnız çizim tarafında olsaydı
/// örtülü pencerede konuşkan bir shell link'i tazeleme hızında kaldırıp
/// yatırırdı — kare çizilmez ama her vsync'te bir ana thread callback'i ve
/// `CAMetalDisplayLink`'in callback'ten önce aldığı bir drawable ödenir.
/// Çizim durur, ritim durmaz; sözleşmenin harfi kalır, ruhu gider.
struct Kapi {
    /// Pencere görünür mü. İki yönlü: `windowDidChangeOcclusionState:` hem
    /// örtülmeyi hem geri dönmeyi bildirir.
    acik: AtomicBool,
    /// Kalıcı durdurma mandalı — bir kez iner, bir daha kalkmaz.
    ///
    /// `acik = false` ile aynı şey **değil**: kapanışta pencere delegate'i
    /// sökülmüyor, yani `kapat()`'tan sonra düşen bir görünürlük bildirimi
    /// kapıyı geri açar ve bekleyen ana thread'e iş atılmaya devam ederdi.
    durdu: AtomicBool,
}

impl Kapi {
    fn yeni() -> Self {
        Self {
            acik: AtomicBool::new(true),
            durdu: AtomicBool::new(false),
        }
    }

    /// Mandal **okuma** tarafında sorgulanıyor, yazma tarafında değil: iki
    /// bayrağı ayrı ayrı okuyup yazmak (`ayarla` mandalı görmez → `durdur`
    /// koşar → `ayarla` kapıyı açar) durdurulmuş bir kapıyı geri açardı ve o
    /// yarış tam da mandalın var olma sebebini yok ederdi.
    fn acik(&self) -> bool {
        !self.durdu.load(Ordering::Acquire) && self.acik.load(Ordering::Acquire)
    }

    fn durdu(&self) -> bool {
        self.durdu.load(Ordering::Acquire)
    }

    /// Görünürlük bildirimi. Sıra önemli: `true`'ya geçerken bunu **önce**
    /// yazan taraf, hemen ardından gelen `request_frame`'in kapıdan geçmesini
    /// garanti eder.
    fn ayarla(&self, acik: bool) {
        self.acik.store(acik, Ordering::Release);
    }

    /// Mandalı indirir; bu andan sonra `ayarla` ne yazarsa yazsın kapı kapalı.
    fn durdur(&self) {
        self.durdu.store(true, Ordering::Release);
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
    ardisik: Ardisik,
}

impl Retry {
    /// Kare çizilemedi. İlk hatada bir kare daha istenir; art arda
    /// ikincisinde **hiçbir şey yapılmaz** ve durak kendiliğinden devreye
    /// girer (bkz. [`Ardisik`]).
    fn cizilemedi(&self, e: &GpuError) {
        eprintln!("bateri: kare çizilemedi: {e}");
        if self.ardisik.hata() {
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
struct Ardisik(AtomicU32);

impl Ardisik {
    /// Kare tamamlandı: bütçe geri verilir.
    fn basarili(&self) {
        self.0.store(0, Ordering::Release);
    }

    /// Hata bildirir. `true` → bir kare daha istenir. Art arda ikinci hatada
    /// `false`: bayrak dikilmediği için sıradaki callback "hasar yok" bulur,
    /// link'i uyutur ve sıradaki `Wakeup` beklenir. Bu olmadan kalıcı bir
    /// hata "dik, dene, düş" döngüsünü tazeleme hızında sonsuza çevirirdi.
    fn hata(&self) -> bool {
        self.0.fetch_add(1, Ordering::AcqRel) == 0
    }
}

/// Delegate'in durumu. Ana thread'e ait olanlar `Cell`/`RefCell`; `retry`
/// paylaşılıyor çünkü onu Metal'in tamamlanma thread'i de çağırır.
struct LinkIvars {
    renderer: Arc<Renderer>,
    session: Arc<Session>,
    retry: Arc<Retry>,
    /// Kapının çizim tarafı buradan okunuyor; gövdenin tek sahibi `Waker`
    /// (uyandırma tarafı da aynı kapıya bakmak zorunda).
    waker: Waker,
    /// Tamamlanma bloğu kurulumda bir kez ayrılır ve burada yaşar.
    completion: Completion,
    /// Kare listesi uzun ömürlü: her karede `clear` ile dolar, ayrılan yer
    /// korunur (kare başına yeniden ayırma yok).
    frame: RefCell<Frame>,
    cell_px: Cell<(u16, u16)>,
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
            if !iv.waker.kapi().acik() {
                link.setPaused(true);
                return;
            }
            // audit: callback ana thread'e bağlı ve yeniden girilmez; sink
            // `Session`'a geri girmiyor, yani ikinci bir ödünç doğmuyor.
            let mut frame = iv.frame.borrow_mut();
            frame.clear(iv.cell_px.get());
            // Hasar yoksa encode ve commit'i hiç yapmıyoruz. Drawable'ı bu
            // tasarruf kapsamaz: `CAMetalDisplayLink` onu callback'ten ÖNCE
            // alıp `update`'in içine koyuyor, `drawable()`'ı çağırmamak alımı
            // iptal etmiyor. (Bu yüzden `nextDrawable`'ın `Option`'ı ve onun
            // `GpuError::NoDrawable`'ı da kalktı: `update.drawable()` başlıkta
            // `nonnull` ve objc2 onu `Option`suz üretiyor.)
            let Some(cursor) = iv.session.frame(|cell| frame.push_bg(cell)) else {
                // Boşta sıfır kare: yeni içerik yok, link uyur. Sıradaki
                // `Wakeup` onu `Waker` üzerinden geri açar.
                link.setPaused(true);
                return;
            };
            frame.push_cursor(cursor, DEFAULT_CURSOR);

            // `frame()` bayrağı çizim başlamadan tüketti; hata hâlinde geri
            // dikilmezse bu içerik bir daha istenmez ve pencere bayat kalır.
            // Senkron ve asenkron hata aynı kapıdan geçiyor.
            if let Err(e) = iv
                .renderer
                .draw(&update.drawable(), DEFAULT_BG, &frame, &iv.completion)
            {
                iv.retry.cizilemedi(&e);
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
        renderer: Arc<Renderer>,
        session: Arc<Session>,
        cell_px: (u16, u16),
    ) -> Self {
        let link =
            CAMetalDisplayLink::initWithMetalLayer(CAMetalDisplayLink::alloc(), surface.layer());
        let waker = Waker {
            inner: Arc::new(WakerInner {
                dirty: session.dirty_flag(),
                link: MainThreadBound::new(link.clone(), mtm),
                kapi: Kapi::yeni(),
                bekleyen: AtomicBool::new(false),
            }),
        };
        let retry = Arc::new(Retry {
            waker: waker.clone(),
            ardisik: Ardisik::default(),
        });
        let completion = {
            let retry = Arc::clone(&retry);
            renderer.completion(move |sonuc| match sonuc {
                Ok(()) => retry.ardisik.basarili(),
                Err(e) => retry.cizilemedi(&e),
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
                frame: RefCell::new(Frame::default()),
                cell_px: Cell::new(cell_px),
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
        self.waker.kapi().ayarla(visible);
        if visible {
            self.request_frame();
        } else {
            self.link.setPaused(true);
        }
    }

    /// Ritmi **kalıcı olarak** keser: uyandırma mandalı iner, link durur ve
    /// run loop'tan çıkar. Geri dönüşü yok — `set_visible(true)` de artık
    /// hiçbir şey yapmaz, ve bu bir söz değil `durdu` mandalının kendisi.
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
        if self.waker.kapi().durdu() {
            return;
        }
        self.waker.kapi().durdur();
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
    pub fn resize(&self, cols: u16, rows: u16, cell_px: (u16, u16)) {
        let iv = self.delegate.ivars();
        if iv.session.resize(cols, rows, cell_px) {
            iv.cell_px.set(cell_px);
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
    fn durdurulan_kapi_gorunurlukle_geri_acilmaz() {
        // Kapanışta pencere delegate'i sökülmüyor: `stop()`'tan sonra düşen
        // bir `windowDidChangeOcclusionState:` kapıyı geri açsaydı, kapanışta
        // `shutdown()`'ın `join`'inde bekleyen ana thread'e iş atılmaya devam
        // ederdi. Mandal bunu koda bağlıyor, yorum cümlesine değil.
        let kapi = Kapi::yeni();
        assert!(kapi.acik(), "link görünür pencereyle doğar");

        kapi.ayarla(false);
        assert!(!kapi.acik(), "örtülen pencere kapıyı kapatır");
        kapi.ayarla(true);
        assert!(kapi.acik(), "örtülme kalkınca kapı geri açılır");

        kapi.durdur();
        assert!(!kapi.acik());
        kapi.ayarla(true);
        assert!(!kapi.acik(), "durdurulmuş kapı bildirimle geri açılmaz");
    }

    #[test]
    fn durma_kosulu_art_arda_ikinci_hatada_devreye_girer() {
        // Checklist'in "durma koşulu zorunlu" maddesi bu sınamayla bağlı:
        // kalıcı bir çizim hatası kare talebini tazeleme hızında tekrarlarsa
        // belirtisi yok, faturası pil. Politika burada, ObjC'siz.
        let sayac = Ardisik::default();
        assert!(sayac.hata(), "ilk hata bir kez daha denenir");
        assert!(!sayac.hata(), "art arda ikinci hata kare talebini keser");
        assert!(!sayac.hata(), "sonrası da kesik kalır");

        sayac.basarili();
        assert!(sayac.hata(), "tamamlanan kare bütçeyi geri verir");
    }
}
