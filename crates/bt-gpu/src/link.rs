//! Kareyi süren şey: `CAMetalDisplayLink` ve onu uzaktan açan `Waker`.
//!
//! Sözleşme tek cümlede: **link paused durur.** Yeni içerik geldiğinde
//! (`Wake::wake` → [`Waker`]) açılır, hasar **ve hareket** tükenince callback
//! onu geri kapatır. "Boşta sıfır kare" bu iki satırda yaşıyor; her
//! `setPaused(false)` bir gerekçe ister ve her kare bir durma koşulu taşır.
//!
//! **Kareyi isteyen üç şey var** (008, 013):
//!
//! - **Hasar** — `Waker` üzerinden, başka bir thread'den, bayrak dikerek.
//! - **Hareket** ([`crate::motion`]) — kimseyi uyandırmadan, çünkü zaten
//!   uyanık olan callback'in kendisi karar veriyor: yerleşmemiş bir animasyon
//!   varken `needs_update` uyumayı reddediyor. **Çentiğin süzülmesi de bu
//!   yoldan, yalnız çizimi başka kolda** (027): talebi yine hareketin
//!   (uyandırma yok, hasar yok), ama payı pencereyi `Session::frame`'in içinde
//!   kaydırdığı için uçuştaki kare **içerik** karesi olarak çiziliyor ve
//!   `icerik=`'e giriyor — gerekçe saatin içerik tadınınki: ızgaranın çizilen
//!   çıktısı gerçekten değişiyor. Durma koşulu süzülmenin kendi yerleşmesi.
//! - **Saat** ([`LinkDelegate::arm_clock`]) — link uyumaya giderken kurulan
//!   tek bir gecikmeli uyandırma. **İki tadı var** ve tadını bekleyen işin
//!   cinsi belirliyor: *içerik tadı* [`Waker::wake`] ile hasar diker (koşan
//!   komutun süre sayacı; ızgara gerçekten değişiyor, `icerik=` sayması
//!   doğru), *hareket tadı* [`Waker::resume`] ile dikmez (imlecin yanıp
//!   sönmesi; değişen tek şey caret'in alfası). Kurulan uyandırma yine
//!   **tek**: iki son tarihten yakın olanı seçiliyor
//!   ([`due_clock`]), çünkü `after` iptal edilemiyor ve ikinci bir tik
//!   birincinin kuşağını geçersiz kılardı.
//!
//! **Hareket `Waker`'a dokunmamak zorunda:** [`Waker::wake`] hasar bayrağını
//! koşulsuz dikiyor, yani oradan istenen bir hareket karesi kendini "içerik"
//! diye saydırır, grid'i boşuna yeniden taratır ve boşta sıfır kare kapısının
//! operandını (`icerik=`) şişirirdi. Yani: **animasyonun zamana bağlı kare
//! talebi hareket saatinden geçer.** Yeni bir animasyon (blink, yumuşak
//! kaydırma) oraya girer, [`Waker::wake`]'e değil.
//!
//! **Saatin içerik tadı yasağın istisnası değil, başka bir şey.** Animasyon
//! aynı içeriği farklı çizer; saat **içeriğin kendisini** değiştirir (koşan
//! komutun süre sayacı: ızgaranın çizilen çıktısı gerçekten başkalaşıyor). Bu
//! yüzden `Waker::wake` üzerinden gitmesi ve `icerik=` sayması **doğrudur** —
//! yasağın koruduğu şey bunun tersiydi. Ayıran ölçüt üç şart: içerik gerçekten
//! değişecek, periyodu ekran hızından **çok** düşük olacak ve **adlandırılmış
//! bir durma koşulu** taşıyacak. Üçünü sağlamayan zamana bağlı talep oraya
//! giremez.
//!
//! **Blink üçünden birincisini geçemiyor ve hareket tadı bu yüzden var.**
//! Izgara değişmiyor, yalnız caret'in alfası — yani blink bir hareket
//! karesidir. Ama ekran hızına da bağlanamaz (2 Hz'lik bir değişim için
//! tazeleme hızında kare), o yüzden `Motion`'ın içinde değil kendi tipinde
//! yaşıyor ([`crate::blink`]) ve tetiği saat. [`Waker::resume`] hasar
//! dikmediği için uyanan callback "hasar yok" dalına düşüyor ve orada bugünkü
//! hareket karesi çiziliyor — ızgara taraması yok, `Term` kilidi yok,
//! `bt-core` yolculuğu yok. **Uyku testi bu yüzden üç soru soruyor** (030'dan
//! beri dört — yazım efektleri de `Motion`'ın dışında, aşağıda): blink
//! `Motion`'ın dışında olduğu için `settled()` onu görmüyor ve bekleyen bir
//! faz değişimi sorulmasaydı `resume` kare üretmeyen bir uyan/uyu fırdöndüsü
//! yaratırdı.
//!
//! **Dock'un yazım efektleri de hareket yolundan** (030,
//! [`crate::glyph_fx`]): `Motion`'ın dışında yaşıyorlar (blink emsali) ve
//! uyku testine kendi adlı terimleriyle giriyorlar — uçuşta bir geliş ya da
//! hayalet varken link uyumuyor, liste boşalınca uyuyor. Hasar dikmiyorlar:
//! efektin sürdüğü kare `kare`'yi artırıyor, `icerik`'i değil.
//!
//! Sözleşmenin sonucu tek cümlede: koşan komutu **ya da sönen bir imleci**
//! olan pencere **boşta değildir**; kalan her pencere boştadır ve sıfır kare
//! çizer. İkisi de adlandırılmış bir durma koşulu taşıyor — komut biter, blink
//! ise varsayılan kapalıdır ve açıkken bile klavye sessizliğinden sonra durur
//! ([`crate::blink::Blink`]).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bt_core::{
    Blocks, CaretStyle, Clusters, Cursor, CursorMotion, DirtyFlag, DockBudget, DockCols,
    DockContext, DockState, Erase, Keypress, LinearRgba, SearchRuns, SelectionRun, SelectionRuns,
    Session, Theme,
};
use dispatch2::{DispatchQueue, DispatchTime, MainThreadBound};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::{NSObject, NSObjectProtocol, NSRunLoop, NSRunLoopCommonModes};
// Yalnız tamamlanma bloğunun GPU damgaları için: `GPUStartTime`/`GPUEndTime`
// `MTLCommandBuffer` protokolünde ve trait kapsamda olmadan çağrılamaz.
use objc2_metal::{MTLCommandBuffer, MTLTexture};
use objc2_quartz_core::CAMetalDrawable;
use objc2_quartz_core::{
    CACurrentMediaTime, CAMetalDisplayLink, CAMetalDisplayLinkDelegate, CAMetalDisplayLinkUpdate,
};

use crate::blink::Blink;
use crate::frame::{DOCK_ROWS, Frame};
use crate::glyph_fx::GlyphFx;
use crate::motion::Motion;
use crate::renderer::{CellMetrics, Completion};
use crate::stats::Stats;
use crate::{GpuError, Renderer, Surface};

/// **Hasardan kare istemenin tek tanımı**: hasar bayrağını dik, link'i aç.
///
/// Her thread'den çağrılabilir; `Clone`, `Send + Sync`.
///
/// **İkinci bir kapı var ve bilerek** ([`Waker::resume`]): o hasar dikmeden
/// açıyor. Uzun süre "ikisini ayrı ayrı yapan yol bilerek yok" yazıyordu ve
/// gerekçesi doğruydu — bayraksız açılan link "hasar yok" deyip anında geri
/// uyar. 014 o gerekçeyi **karşıladı**: uyanan callback'in "hasar yok" dalında
/// artık yapacak bir işi olabiliyor (blink'in faz değişimi), yani link boşuna
/// uyanmıyor. Bayraksız açmanın tek meşru sebebi bu.
///
/// **Animasyon [`Waker::wake`]'ten kare istemez** (modül başlığı): hareket,
/// uyanık callback'in kendi kararı. Bu kapıya bağlanan bir animasyon her
/// karesine hasar diker ve `icerik=` sayacını — yani boşta sıfır kare kapısını
/// — kendi karelerinden doldururdu. Yasağın öznesi **bu fonksiyon**, tipin
/// kendisi değil.
///
/// **Saat ise buradan geçer** ([`LinkDelegate::arm_clock`]) ve çelişki değil:
/// sayacın tiki içeriği gerçekten değiştiriyor, yani `icerik=` sayması
/// yerinde. Ayıran üç şart modül başlığında.
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
    /// bekliyorsa ikisi birbirini kilitler. Kapanış yolu bu yüzden önce
    /// [`DisplayLink::stop`] çağırır ve **beklerken** `DisplayLink`'i
    /// düşürmez: son referans ana thread'de kalır. Beklemeyen bir kapanış
    /// (`bt-shell`'de tek sekmenin kapanışı) onu düşürebilir — ana thread
    /// beklemede değilken tamamlanma bloğunun senkron işi yalnız bir tur
    /// gecikir; okuyucu tarafındaki kopyayı ise `bt-shell` kapanışta söküyor.
    link: MainThreadBound<Retained<CAMetalDisplayLink>>,
    /// Kare çizilir mi, ritim döner mi.
    gate: Gate,
    /// Kare **talebi** sayacı — boşta sıfır karenin `kare`'den daha derin
    /// ölçütü.
    ///
    /// `kare` GPU'nun hatasız bitirdiğini sayıyor: bizim tarafımızda doğup
    /// [`Gate`]'te ölen ya da birleşen talepler ona hiç görünmez. Bu sayaç
    /// kapıdan **önce** artıyor, yani talebin kendisini sayıyor — örtülü bir
    /// pencerede kapı kareyi yutsa da talep burada iz bırakır.
    ///
    /// **Ne sayıyor:** [`Waker::wake`]'e yapılan *her* çağrı. Yani yalnız
    /// shell çıktısı değil; [`Retry::draw_failed`]'in yeniden denemesi,
    /// [`DisplayLink::resize`]'ın koşulsuz talebi ve `stopped` mandalı
    /// indikten sonra okuyucudan gelen son uyandırmalar da buraya yazılıyor.
    /// Sayı bu yüzden "kare üretebilecek talep" değil "istenen kare"; kalıcı
    /// bir çizim hatası onu şişirir ve okuyan taraf bunu `kare` ile
    /// karşılaştırarak ayırt eder.
    ///
    /// **Ne saymıyor: hareket karesini** — ne uyanık callback'in kendi
    /// kararıyla çizdiğini, ne de saatin hareket tadıyla ([`Waker::resume`])
    /// uyandırdığını; `resume` bu sayaca bilerek dokunmuyor. Animasyon
    /// [`Waker::wake`]'e hiç dokunmuyor
    /// (modül başlığı), yani bu sayaç `icerik`'e yakın kalırken `kare`
    /// animasyon boyunca ondan kopuyor. Aşağıdaki "duman yükü" ölçümünün
    /// `istek ≈ kare + 2` ilişkisi tam bu yüzden **008'de geçersizleşti**;
    /// sayıların kendisi (o günkü koşuların gözlemi) duruyor, yeni hâli
    /// `icerik` üstünden **ölçüldü** (008 phase-6, otuz sağlıklı koşu):
    /// `istek` otuzunda da `4` iken `icerik` `2`–`3`, `kare` ise 27–30. Sayaç
    /// yine de **sabit değil** — sonraki bir koşu `3` verdi, muhtemelen bu
    /// gövdenin birleştirmesi yüzünden; ölçülmedi.
    ///
    /// Bir **sayaç, kapı değil**: eşiği ölçülmedi ve ölçülmemiş sayı kapıya
    /// yazılmaz (`yuva=` ile aynı kural). Ölçülen (2026-09-12, debug, bu
    /// makine) iki ayrı rejim gösteriyor ve ikisi de sayacın niye ayrı bir
    /// sayı olduğunu söylüyor:
    ///
    /// - **Duman yükü** — sağlıklı koşuda `kare=1–2` iken `istek=2–3`; boşta
    ///   sıfır kare bilerek bozulduğunda `kare=82–354`, `istek=84–357`. İkisi
    ///   bir arada gidiyor, yani burada `kare`'den daha ayırt edici değil.
    /// - **Ölçüm yükü** — `kare=9` (2 sn) / `21` (5 sn) iken `istek`
    ///   **25 000–72 000**. Kare akmıyor ama talep akıyor: aradaki üç
    ///   mertebeyi `kare` hiç göremiyor.
    ///
    /// İkinci rejimin **mekanizmasını ölçmedim** (kapı mı yutuyor, ana thread
    /// mi doyuyor, sistem mi link'i kısıyor); dışarıdan gözlenen iki sayıyı
    /// yazdım.
    ///
    /// `Relaxed`, çünkü hiçbir şeyi sıralamıyor — kapanışta bir kez okunuyor.
    ///
    /// **Kapılı değil ve bedeli ölçüldü.** Sayacı okuyan tek yer süreli koşu
    /// (`report_and_exit`), yani etkileşimli oturumda kimse bakmıyor; buna
    /// rağmen kapı takılmadı, çünkü bedel kapının kendi bedelinden büyük
    /// değil: ölçülen en yüksek uyandırma hızı **~15 000/sn**
    /// (45 167 talep / 3 sn, ölçüm yükü) ve bu, `pending.swap`'in zaten
    /// kirlettiği önbellek satırında saniyede bir kez daha `fetch_add` demek —
    /// mertebe olarak **saniyede on mikrosaniye**. Bir `Option` dallanması
    /// aynı mertebeyi ödetir, üstelik `DisplayLink::new`'e bir parametre
    /// ekleyerek.
    requests: AtomicU64,
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
        // Sayaç kapıdan da bayraktan da **önce**: ölçmek istediğimiz şey
        // talebin kendisi, kapının ondan sonra ne yaptığı değil.
        self.inner.requests.fetch_add(1, Ordering::Relaxed);
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

    /// Link'i **hasar dikmeden** açar — saatin ikinci tadı.
    ///
    /// [`Waker::wake`]'in üç işinden ortadaki çıkarılmış hâli: kapı aynı,
    /// `pending` birleştirmesi aynı, dispatch aynı; `dirty.mark()` **yok**.
    /// Uyanan callback bu yüzden "hasar yok" dalına düşüyor ve orada bir
    /// **hareket** karesi çiziliyor — ızgara yeniden taranmıyor, `Term`
    /// kilidine girilmiyor, `icerik=` artmıyor.
    ///
    /// `requests` de artmıyor: o sayacın sözleşmesi "istenen **içerik**
    /// karesi" ve hareket karesini bilerek saymıyor (`requests`'in doc'u).
    ///
    /// **Birleştirmeyi `wake` ile paylaşması kayıpsız:** ikisi de aynı bloğa
    /// çıkıyor ve blok sırayı `pending` → `setPaused(false)` diye kuruyor;
    /// `wake` bayrağı kapıdan **önce** koşulsuz diktiği için ona birleşen bir
    /// `resume`'un hasarı kaybolmaz, tersi de açılmayı kaybetmez.
    pub(crate) fn resume(&self) {
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
            if !inner.gate.is_open() {
                return;
            }
            inner.link.get(mtm).setPaused(false);
        });
    }

    fn gate(&self) -> &Gate {
        &self.inner.gate
    }

    /// Şimdiye kadarki kare talebi sayısı.
    fn requests(&self) -> u64 {
        self.inner.requests.load(Ordering::Relaxed)
    }
}

/// Çizilen karenin dikey orijini, piksel — **kare yolu yazar, fare yolu
/// okur**. Yanında doldurma bandının boyu (satır) taşınıyor.
///
/// **Band orijinin geometrisinin parçası**, ikinci bir konu değil: bandın
/// kendi viewport'u da buradan türüyor (`Frame::fill_origin_px`,
/// `origin_px − fill_px`) ve fare eşlemesinin sorduğu şey de orijinin
/// **üstünde** ne olduğu — boşluk mu, geçmiş mi. Ayrı bir gövdeye konsaydı
/// iki değer iki ayrı karede yayınlanabilir ve orijini yeni, bandı eski bir
/// fare çevirisi doğardı.
///
/// Değerin **tek sahibi** [`DisplayLink`]: hesabı `Session::frame` yapıyor
/// (`bt_core::Cursor::content_rows`, tek hesap) ama ötelemeye çeviren ve
/// çizime sokan kare yolu, yani okuyan taraf da oradan okumak zorunda —
/// ikinci bir hesap, "fare bir satır kayıyor" diye görünen bir ayrışma
/// demekti.
///
/// **Atomik değil, `Cell`** ve bu bir kısayol değil ölçülü bir gerçek: link
/// callback'i ana run loop'a eklendiği için ana thread'de koşuyor
/// ([`LinkDelegate`] `MainThreadOnly`) ve `point_to_cell`'in çağıranı da
/// (NSView fare olayı) ana thread'de. İki taraf aynı thread'de, yani yarış
/// yok. `Arc<AtomicU32>`'ye kaçmak atomik gerekiyormuş gibi yazmak olurdu ve
/// `make test-yaris`'in "paylaşılan durum" tetiğini gerekçesiz geri
/// getirirdi (`.tasks/011-tabana-yapisik-icerik/discussion.md` → Karar 4 eki).
///
/// `Rc` bu yüzden `Send` değil ve olmamalı: tipin kendisi "ana thread"i
/// söylüyor.
///
/// Okunan değer **son encode edilen karenin** orijini. Bayatlık değil tasarım:
/// tıklama ekrandaki piksele yapılıyor ve o piksel o karede çizildi. Yayın bu
/// yüzden `draw`'ın `Ok` kolunda ([`LinkDelegate::publish_origin`]) —
/// encode edilemeyen kare ekranda hiçbir şeyi değiştirmedi ve onun ötelemesini
/// yayınlamak fareyi görünmeyen bir ızgaraya göre çevirirdi. "Çizilen" değil
/// "encode edilen": `Ok` commit demek, sunum değil, ve asenkron tamamlanma
/// yine düşebilir — kalan pencere tek kare, çünkü `draw_failed` hasar bayrağını
/// geri dikiyor ve sıradaki kare aynı ötelemeyle yeniden çiziliyor.
#[derive(Clone, Default)]
pub struct Origin(Rc<Cell<Drawn>>);

/// [`Origin`]'in gövdesi: tek karenin geometrisi, birlikte yayınlanır.
#[derive(Clone, Copy, Default)]
struct Drawn {
    px: f32,
    fill_rows: u16,
    /// Dock'un giriş bloğunun tepesi (fiziksel piksel, üstten) ve giriş
    /// satırı sayısı (032); `None` → bu karede dock yok.
    dock: Option<(f32, u16)>,
}

impl Origin {
    /// Çizilen karenin dikey orijini, **fiziksel piksel** — kaydırmanın
    /// kesri dahil (`Frame::set_scroll_frac`), yani fare ızgarayı çizildiği
    /// yerde okuyor.
    pub fn px(&self) -> f32 {
        self.0.get().px
    }

    /// Orijinin üstündeki doldurma kanalının boyu, **satır**: bant
    /// (`bt_core::Cursor::fill`) artı kaydırma kesrinin tepe satırı
    /// (`bt_core::Cursor::top_row`). Sıfırsa orada boşluk var, değilse geçmiş
    /// — kesirli konumda tepedeki yarım satır da bandınki gibi seçilemiyor.
    pub fn fill_rows(&self) -> u16 {
        self.0.get().fill_rows
    }

    /// Çizilen karenin dock geometrisi: giriş bloğunun tepesi (**fiziksel
    /// piksel**, dokunun tepesinden) ve giriş satırı sayısı; `None` → dock
    /// yok ya da henüz hiç çizilmedi.
    ///
    /// Orijinle **aynı gövdede** ve aynı sebeple (032): bant büyürken ızgara
    /// yukarı, giriş bloğu satır satır genişliyor ve ikisi ayrı karelerden
    /// yayınlansaydı tıklama orijini yeni, bloğu eski bir kareye göre
    /// çevirebilirdi. Değer **yerleşimin** (`Frame::dock_hit`): metin dibe
    /// yaslı ve animasyon boyunca yerinde duruyor.
    pub fn dock(&self) -> Option<(f32, u16)> {
        self.0.get().dock
    }

    /// Yalnız kare yolu yazar; `pub` değil ve olmamalı.
    fn set(&self, px: f32, fill_rows: u16, dock: Option<(f32, u16)>) {
        self.0.set(Drawn {
            px,
            fill_rows,
            dock,
        });
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
    ///
    /// Dönüş: **bütçe bitti mi** (`false` → bir kare daha istendi). Hasar
    /// yolunda çağıranın buna bakmasına gerek yok, durak orada bayrağın
    /// dikilmemesiyle geliyor; **hareket yolunda gerekiyor**, çünkü oradaki
    /// durak hasar değil animasyonun yerleşmesi (`needs_update`).
    fn draw_failed(&self, e: &GpuError) -> bool {
        eprintln!("bateri: kare çizilemedi: {e}");
        if self.streak.failed() {
            self.waker.wake();
            return false;
        }
        true
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
    ///
    /// **008'den beri bu tek başına yetmiyor:** "hasar yok" dalı artık
    /// koşulsuz uyumuyor, yerleşmemiş bir animasyon varken hareket karesi
    /// çiziyor. Bütçe bitince o dal da animasyonu bitiriyor
    /// ([`crate::motion::Motion::finish`]) — yoksa kalıcı bir hata, kaymanın
    /// süre tavanı dolana kadar tazeleme hızında hata satırı basardı. İki
    /// durak birlikte: bayrak dikilmiyor **ve** bekleyen animasyon kalmıyor.
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
    /// Komut bloklarının tamponu; `frame` ile aynı gerekçeyle uzun ömürlü —
    /// `Session::frame` onu her karede boşaltıp yeniden dolduruyor ve ayrılan
    /// yer korunuyor.
    ///
    /// `Frame`'in **içinde değil yanında**: aynı çağrıda `frame.push`
    /// kapatması da tampon da ödünç alınıyor ve ikisi tek `RefCell`'de
    /// olsaydı çalışma zamanında panik ederdi. Şeridi çizecek liste (piksel
    /// dörtgenleri) `Frame`'in kendi `stripes` alanı; bu tampon ona **girdi**,
    /// kendisi değil — `Frame::push_block` aralıkları buradan okuyup oraya
    /// çeviriyor.
    blocks: RefCell<Blocks>,
    /// Seçimin satır koşuları ve iki rengi (031); `blocks` ile aynı ömür ve
    /// aynı gerekçe — `Frame`'in içinde değil yanında, `Frame::push_selection`
    /// onu dörtgenlere çeviriyor.
    selection: RefCell<SelectionRuns>,
    /// Arama vurgusunun koşuları (033); `selection` ile aynı ömür ve aynı
    /// gerekçe. Bu phase'de yalnız doluyor, çizimi phase-2.
    search: RefCell<SearchRuns>,
    /// Dock seçiminin görsel satır başına koşuları (032); `selection` ile aynı
    /// gerekçe — `bt_core::Session::dock` her içerik karesinde boşaltıp
    /// dolduruyor, kapasite korunuyor.
    dock_selection: RefCell<Vec<SelectionRun>>,
    /// Doldurulan satırların tamponu; `blocks` ile aynı ömür ve **aynı
    /// gerekçe**: `Frame`'in içinde değil yanında.
    ///
    /// Sebep borç kuralının ta kendisi: `Session::frame` iki sink alıyor ve
    /// ikisi de `frame`'i ödünç alsaydı aynı çağrıda iki `&mut` doğardı.
    /// Tampon o ikinci ucu tutuyor, çağrı dönünce hücreler `Frame::push_fill`
    /// ile bandın kendi listelerine geçiyor. Boşaltılıp yeniden doluyor, yani
    /// kare başına ayırma yok; doldurması olmayan pencerede (dock'suz kabuk,
    /// süreli koşu) sınır sink'i hiç çağırmıyor ve tampon boş kalıyor.
    fill: RefCell<Vec<bt_core::Cell>>,
    /// Aynanın tamponu; `blocks` ile aynı gerekçeyle uzun ömürlü —
    /// [`Session::dock`] onu her karede yerinde tazeliyor ve kapasitesi
    /// duruyor, yani kare başına ayırma yok.
    ///
    /// Dock'u olmayan pencerede hiç dokunulmuyor: boş bir `DockState` üç boş
    /// dizgi ve boş bir `Vec`, yani ayırmıyor da.
    dock: RefCell<DockState>,
    /// Bağlam satırının tamponu; `dock` ile aynı gerekçe ve aynı ömür.
    ///
    /// Ayrı tampon, çünkü ayrı ömür: ayna `line-finish`'te sıfırlanıyor,
    /// dizin ile dal prompt'tan prompt'a duruyor (`bt_core::DockContext`).
    dock_context: RefCell<DockContext>,
    /// Dock kaç satır; `0` → bu pencerede dock yok.
    ///
    /// **`Cell`, çünkü artık oynuyor:** dock alternatif ekranda kalkıyor ve
    /// inince geri geliyor (R5.2), yani değer [`DisplayLink::resize`] ile
    /// tazeleniyor. Oynamanın bedeli ızgara yüksekliği, yani bir
    /// `TIOCSWINSZ` — ve o bedel **komut başına değil geçiş başına**
    /// ödeniyor (R5.3): `git log` gibi alternatif ekrana girmeyen komutlar
    /// hiç resize görmüyor.
    ///
    /// Sıfır **iki ayrı şeyi** anlatıyor ve ikisi de "dock çizilmez" demek:
    /// pencerede hiç dock yok (entegrasyonsuz oturum) ya da bu an alternatif
    /// ekrandayız. Ayrımı burada tutmak gerekmiyor — doğum değerinin sahibi
    /// `bt-shell` ve geri getirecek olan da o.
    dock_rows: Cell<u16>,
    /// Alternatif ekranın **son görülen** hâli; nöbet bununla karşılaştırıyor.
    alt_screen: Cell<bool>,
    /// Alternatif ekran değişince çağrılan haberci; `None` → bu pencerede yol
    /// hiç çalışmıyor.
    ///
    /// **Enjekte ediliyor, çağrı değil** (`bt_core::Wake` emsali): `bt-gpu`
    /// `bt-shell`'i göremez, katman yönü tek. Kapanış `bt-shell`'de kuruluyor
    /// ve aynı üç yasağı taşıyor: ana thread'de koşar, **bloklamaz** ve
    /// pencere geometrisini **yerinde değiştirmez** — yalnız ana kuyruğa iş
    /// atar. Sebep bu fonksiyonun çağrıldığı yer: kare tam da çizilmiş
    /// durumda ve drawable ölçüsünü, ızgarayı, yerleşimi orada değiştirmek
    /// çizilen karenin altını oymak olurdu.
    ///
    /// **Yük taşımıyor.** Haberci koştuğunda gerçeği yeniden okuyor, yani
    /// birbirini kovalayan iki geçiş (vim aç-kapa) bayat bir değerle
    /// davranamıyor.
    ///
    /// Dock'u olmayan pencerede `None` ve bu **yapısal**: yol o oturumda hiç
    /// kurulmuyor, bir koşulla kapatılmıyor.
    alt_screen_changed: Option<Box<dyn Fn()>>,
    /// Izgaranın genişliği, sütun; dock'un taşan satırı sarması için
    /// [`Session::dock`]'a giriyor.
    ///
    /// `Cell`: [`DisplayLink::resize`] yazıyor, içerik karesi okuyor — ikisi
    /// de ana thread. `cell` ile **ayrı** duruyor çünkü kaynakları ayrı:
    /// hücre ölçüsü yalnız oturum boyutu kabul ederse tazeleniyor, sütun
    /// sayısı ise pencerenin kendi cevabı.
    cols: Cell<u16>,
    /// Demet değil `CellMetrics`: ızgara geometrisi (hücre ölçüsü **ve** sol
    /// pay) `Renderer::cell_metrics`'ten `bt-shell` üzerinden buraya tip
    /// olarak geliyor, **saklanırken de** tip kalıyor ve `Frame::clear`'a da
    /// tip olarak giriyor — çizim orijini payı oradan okuyor.
    /// (`resize`'ın `Session::resize`'a geçirdiği demet başka bir değer:
    /// oraya **gelen** ölçü gider, saklanan değil — kabul edilmeyen bir
    /// boyut buraya hiç yazılmaz.)
    cell: Cell<CellMetrics>,
    /// Çizilen karenin dikey orijini; fare yolu bu gövdeyi paylaşıyor.
    ///
    /// `Frame::origin_px`'in ikizi değil **yayını**: kare listesi bu crate'in
    /// içinde (`pub(crate)`) ve `bt-shell`'in onu görmesi için bir sebep yok,
    /// oysa fare eşlemesi çizilen orijini görmek **zorunda**. İkisini tek
    /// çağrı yazıyor ([`LinkDelegate::set_origin`]), yani ayrışamazlar.
    origin: Origin,
    /// **İçerik** karesi: `session.frame()` hasar buldu ve kare çizilmeye
    /// karar verildi. Boşta sıfır kare kapısının operandı bu.
    ///
    /// `kare`'den (GPU'nun hatasız bitirdiği kare) ayrı bir sayı: **hareket**
    /// ve **kayma** kareleri de çizilen karelerdir, yani `kare`'yi artırırlar,
    /// ama grid kirli değildir — 200 ms'lik bir imleç kayması 120 Hz'de ~24
    /// kare eder ve `kare ≤ IDLE_FRAME_LIMIT` kapısı kod doğruyken kırmızı
    /// düşerdi. Kapı bu yüzden "boştaki **içerik** karesi"ne bağlanıyor;
    /// sınırın sayısı değil **operandı** değişti.
    ///
    /// Çıkarma (`kare − hareket`) bilerek yok: `kare` Metal'in tamamlanma
    /// thread'inde, bu sayaç ana thread'de artıyor, yani deadline animasyonun
    /// ortasına düşerse fark `u64` sarmasına açık. İki sayaç aynı noktada
    /// artıyor ve kapı yalnız birine bakıyor
    /// (`.tasks/008-hareket-ve-imlec/discussion.md` → Karar 2).
    ///
    /// `Cell`, atomik değil: ikisini de yalnız `needs_update` yazıyor ve o
    /// ana thread'e bağlı (`MainThreadOnly`); okuyan da ana thread
    /// ([`DisplayLink::content_frames`]).
    content_frames: Cell<u64>,
    /// **Hareket** karesi: hasar yok ama yerleşmemiş bir animasyon var.
    ///
    /// `content_frames`'in kardeşi ve bilerek ondan ayrı: ikisi de çizilen
    /// kare sayıyor ama yalnız biri boşta sıfır kare kapısının operandı.
    /// Süreli koşu bunu `hareket=` diye basıyor ve duman kapısının
    /// **gerekli** sayacı (reçetede bir imleç hareketi var, bkz.
    /// `bt_core::smoke_shell`).
    motion_frames: Cell<u64>,
    /// **Kayma** karesi: hasar yok ama ötelemenin animasyonu yerleşmemiş.
    ///
    /// [`Self::motion_frames`]'in kardeşi ve ondan ayrı, çünkü iki animatör
    /// var ve kırmızı bir koşuyu okuyan taraf hangisinin yerleşmediğini
    /// satırdan görmeli. Aynı karede ikisi birden artabilir — sayılar toplanıp
    /// çizilen kareyi vermiyor, her biri kendi animatörünün tanığı.
    ///
    /// Kapıya **girmiyor**: duman reçetesi bir imleç hareketi içeriyor
    /// (`bt_core::smoke_shell`) ama tabana yapışık içerikte tek satırlık bir
    /// prompt kayma üretmeyebilir — ölçülmemiş bir eşiği kapıya yazmıyoruz.
    slide_frames: Cell<u64>,
    /// İmlecin ve içeriğin kayması — kareyi zamana bağlayan tek şey.
    ///
    /// `Cell`, `RefCell` değil: [`crate::motion::Motion`] `Copy` ve ona
    /// dokunan tek yer bu callback (ana thread). `RefCell` çalışırdı ama
    /// `frame` ödüncünün yanında ikinci bir çalışma-zamanı ödüncü demek
    /// olurdu ve kazandırdığı hiçbir şey yok.
    motion: Cell<Motion>,
    /// Dock'un yazım efektleri (030): uçuştaki gelişler ve hayaletler.
    ///
    /// **`motion`'ın içinde değil yanında** ve `RefCell`: liste `Copy`
    /// değil, `Motion`'ı `Copy`'den çıkarmak ya da her `get`/`set`'te
    /// kopyalatmak bedeldi (`.tasks/030-dock-yazim-animasyonlari/discussion.md`
    /// → Karar 4). Ödüncü yalnız bu callback ve `DisplayLink`'in ayar
    /// yolları alıyor, ikisi de ana thread ve çağrı sınırında bırakıyor.
    glyph_fx: RefCell<GlyphFx>,
    /// Geometri (pencere, font, zoom) oynadı: sıradaki içerik karesi imleci
    /// animasyonsuz taşısın.
    ///
    /// Bayrak, çünkü [`DisplayLink::resize`] callback değil — hücre ölçüsünü
    /// değiştiren yol ile onu çizen yol ayrı anlarda koşuyor ve aradaki kareyi
    /// yalnız bu bayrak bağlıyor. Sıradaki içerik karesi onu **tüketir**:
    /// tüketilmeseydi geometriden sonraki her kare snap'lerdi.
    geometry_changed: Cell<bool>,
    /// Bir önceki callback'in damgası; `dt`'nin tabanı.
    ///
    /// Kaynağı `last_frame_at` ile **aynı** (`update.targetTimestamp()`) ve
    /// bu şart: iki ayrı taban iki ayrı zaman yaratır ve `sessiz=` ile
    /// animasyonun saati birbirini tutmazdı. Saat okuması yok, alan kopyası.
    ///
    /// `last_frame_at`'ten ayrı bir alan, çünkü o yalnız **yola çıkan**
    /// karede tazeleniyor; `dt` ise encode edilemeyen karede de ilerlemeli,
    /// yoksa bir hatadan sonra animasyon o kadar süreyi tek adımda atlardı.
    last_update_at: Cell<Option<f64>>,
    /// İçerik karesinde okunan temanın kopyası — hareket karesinin paleti.
    ///
    /// Hareket karesi `session.theme()`'i **çağırmıyor**: o yaprak bir kilit
    /// alıyor ve hareket karesinin `Session`'a hiç dokunmaması tasarımın
    /// kendisi (008 Karar 4). Tema takası zaten kare istiyor
    /// (`Session::set_theme`), yani bir sonraki kare içerik karesi olur ve
    /// kopya orada tazelenir.
    theme: Cell<Theme>,
    /// `bt-core`'un istediği bir sonraki **içerik** tikinin mutlak zamanı.
    ///
    /// İki kaynağı var ve `bt-core` yakın olanı seçip veriyor
    /// (`bt_core::shell::sooner`): koşan komutun süre sayacı ve dock'lu bir
    /// pencerede caret devrinin **tutması**. `bt-gpu` ikisini ayırt etmiyor —
    /// ikisi de içeriği değiştiriyor, yani `icerik=` sayması doğru.
    ///
    /// `None` → beklenen bir şey yok: komut bitti, koşan bloğun çıpası
    /// ekrandan çıktı, entegrasyon hiç yok ya da bekleyen bir devir tutması
    /// kalmadı.
    ///
    /// **Son tarih, süre değil** ve bu blink'in getirdiği zorunluluk: eski
    /// hâlde `arm_clock` her uyku noktasında `Cursor::next_tick`'i baştan
    /// kuruyordu ve saniyede iki kez uyanan bir blink sayacın tikini her
    /// seferinde bir saniye ileri iterdi — tik **hiç** ateşlemezdi. Mutlak
    /// bir damga aradaki uyanmalardan etkilenmiyor.
    ///
    /// Yan kazanç: `arm_clock`'ın doc'unda yazılı olan "hareket karesi
    /// `Cursor`'ı tazelemiyor, yani uzun bir animasyondan sonra kurulan tik
    /// bir animasyon boyu geç kalabilir" kusuru da kapanıyor.
    ///
    /// `None` **temizliyor** (013 kapısının dersi): komut bitince bayat bir
    /// son tarih kalsaydı bir kare fazla istenirdi.
    content_deadline: Cell<Option<f64>>,
    /// İmlecin yanıp sönmesi; fazın sahibi boyayan taraf ([`crate::blink`]).
    blink: Cell<Blink>,
    /// Pencere **odakta mı** — `bt-shell`'in cevabı.
    ///
    /// `bt-core`'a hiç girmiyor (R7): odak bir pencere olgusu ve terminalin
    /// durumuyla ilgisi yok. `bt-gpu` onu iki yerde okuyor — blink'in kapısı
    /// ve caret'in içinin boşalması.
    ///
    /// Varsayılan `true` ve **hermetik koşuda hiç yazılmıyor**: süreli koşu
    /// (`BT_RUN_SECONDS`) odağı okumuyor, yani `make duman` bir makinede
    /// yeşil bir makinede kırmızı düşmüyor. Kapı çağrı yerinde
    /// (`bt-shell`'in delegate'i), varsayılanda değil — varsayılan tek başına
    /// yetmezdi, çünkü koşu sırasında açılan bir Spotlight
    /// `windowDidResignKey:` doğurup kare isterdi.
    focused: Cell<bool>,
    /// Klavye **terminalde** mi — `bt-shell`'in cevabı (033 R7): arama
    /// panelinin alanı first responder olunca `false`.
    ///
    /// **Odak iki bit** (033 → Muhakeme) ve birleştirme burada, tek yerde
    /// ([`LinkIvars::caret_focused`]): caret'in içinin boşalması ve blink'in
    /// durması "pencere key **ve** klavye terminalde" sorusunun cevabı — caret
    /// klavyenin nereye gittiğini söyleyen tek sinyal. Seçim ve arama
    /// vurgusunun solması ise yalnız [`LinkIvars::focused`]'tan: alana
    /// yazarken vurgular tam renkli kalmalı. Tek bit olsaydı ikisinden biri
    /// yanlış olurdu.
    keyboard: Cell<bool>,
    /// İmlecin **ayardan gelen** çizim sayıları.
    ///
    /// `cell`/`motion`/`blink` ile aynı yuvada ve aynı gerekçeyle: kare yolu
    /// bunu her içerik karesinde `Frame`'e veriyor (`clear`'ın ikinci
    /// argümanı) ve hareket karesi `clear` çağırmadığı için değeri koruyor.
    ///
    /// `bt-core`'a **uğramıyor** anlamında değil — değer `bt_core::Settings`'te
    /// yaşıyor ve varsayılanının tek sahibi orası; uğramadığı yer
    /// `TerminalOptions`/`Session`, yani terminalin durum makinesi.
    caret_style: Cell<CaretStyle>,
    /// Blink'in **istenen** yarım periyodu, saniye ([`DisplayLink::set_blink_interval`]).
    ///
    /// Ayrı yuva, çünkü uygulanması bir **kare damgası** istiyor: `Blink`'in
    /// tiki mutlak bir son tarih ve yeniden kurulurken `now` gerekiyor.
    /// Değeri burada bekletip kare yolunda uygulamak tek zaman tabanını
    /// koruyor.
    blink_interval: Cell<f64>,
    /// Kurulmuş tikin kuşağı — eskiyen tik kendini tanıyıp sussun diye.
    ///
    /// `DispatchQueue::after` iptal edilemiyor, yani araya bir içerik karesi
    /// girip saati yeniden kurduğunda eski tik yine ateşlenir. Kuşak
    /// eşleşmiyorsa o tik geçersizdir ve `Waker`'a dokunmaz; yoksa her
    /// yeniden kurulum bir fazladan içerik karesi doğururdu.
    ///
    /// `Arc<AtomicU64>`, çünkü kapatma `Send` olmak zorunda — ateşleyen taraf
    /// ana kuyruk olsa da `after`'ın imzası öyle istiyor.
    clock_generation: Arc<AtomicU64>,
    /// Caret bloğunun altında kalan metnin rengi, son içerik karesinden.
    ///
    /// Hareket karesi `bt-core`'a hiç gitmiyor ve bu değeri oradan alamaz.
    /// **İki kaynaklı** — ızgaranın imleci ya da dock'un caret'i, hangisi o
    /// karede caret'in evi ise; ortak yanları konuma bağlı olmamaları, o
    /// yüzden ikisi de bu alana yazılıyor ve hareket karesi hangisinin
    /// yazdığını sormuyor. `None` → o karede caret yok.
    last_caret_text: Cell<Option<LinearRgba>>,
    /// Son içerik karesindeki caret'in **hedefi**; blink'in "yazıyor mu"
    /// sorusunun tek kaynağı ([`Blink::wake`]).
    ///
    /// Konum `Motion`'da da var ama orası **ara** konumu tutuyor (animasyon
    /// sürerken her karede başka bir değer); burada duran hedefin kendisi ve
    /// karşılaştırma ancak onunla anlamlı.
    last_caret_at: Cell<Option<[f32; 2]>>,
    /// Son **çizilen** karenin damgası (`CAMetalDisplayLinkUpdate`'in hedef
    /// sunum anı), `sessiz=` jetonunun tabanı.
    ///
    /// `update.targetTimestamp()` bir **alan kopyası**, saat okuması değil:
    /// kare başına `CACurrentMediaTime()` çağırmak ölçüm kapısı kapalıyken de
    /// saat okumak olurdu ve kare yolunun "kapı kapalıyken tek bir saat
    /// okuması bile yok" sözleşmesini (`stats`'ın doc'u) kırardı. Tek okuma
    /// deadline'da, [`DisplayLink::quiet_since`]'ta.
    ///
    /// Damga `Ok` dalında yazılıyor: encode edilemeyen kare sessizliği
    /// bölmez, çünkü ekranda hiçbir şey olmadı.
    ///
    /// `None` → hiç kare çizilmedi; jeton o zaman `sessiz=none`.
    last_frame_at: Cell<Option<f64>>,
}

impl LinkIvars {
    /// Caret'in odağı: pencere key **ve** klavye terminalde (033 → Muhakeme,
    /// "odak iki bit"). İçinin boşalması, blink'in kapısı ve hareket
    /// karesindeki yeniden çizimi buradan; vurgu ve seçim rengi yalnız
    /// `focused`'tan.
    fn caret_focused(&self) -> bool {
        self.focused.get() && self.keyboard.get()
    }
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
            // Zamanın tabanı: damga bir **alan kopyası**, saat okuması değil
            // (bkz. `LinkIvars::last_frame_at`). `sessiz=` ile animasyonun
            // saati aynı yerden okunuyor — iki taban iki ayrı zaman yaratırdı.
            let now = update.targetTimestamp();
            // İlk karede `dt` yok: `0.0` ile başlamak, bilinmeyen bir aralığı
            // uydurmaktan iyi. Kırpmayı `Motion::advance` yapıyor ve orada
            // olması şart (gerekçe `motion::DT_MAX`).
            let dt = iv
                .last_update_at
                .replace(Some(now))
                .map_or(0.0, |prev| (now - prev) as f32);
            // audit: callback ana thread'e bağlı ve yeniden girilmez; sink
            // `Session`'a geri girmiyor, yani ikinci bir ödünç doğmuyor.
            let mut frame = iv.frame.borrow_mut();
            let mut motion = iv.motion.get();
            // **Hasar sorusu taramadan önce** (008 Karar 4): hareket karesi
            // listeyi temizlemeden kullanıyor, yani "temizlensin mi" kararı
            // `clear`'dan önce verilmek zorunda. `Session::frame`'in eski
            // `Option`'ı tam bu sırayı imkânsız kılıyordu.
            //
            // **Süzülme uçuştayken hasarsız kare de içerik karesi** (027): payı
            // pencereyi `Session::frame`'in içinde kaydırıyor ve hareket karesi
            // `bt-core`'a hiç gitmiyor. Talep hareketin — kimse uyandırmıyor,
            // `Waker::wake`'e dokunulmuyor — çizimi içeriğin (modül başlığı).
            let damaged = iv.session.take_damage();
            if !damaged && motion.glide_idle() {
                // Hasar yok. İki ihtimal kaldı ve ikisi de burada bitiyor.
                motion.advance(dt);
                // **Uyku testinin üçüncü sorusu.** Blink `Motion`'ın dışında
                // yaşıyor, yani `settled()` onu görmüyor; bu satır olmasaydı
                // `Waker::resume` ile uyanan callback hiçbir şey çizmeden geri
                // uyur ve saat yeniden kurulurdu — kare üretmeyen bir
                // uyan/uyu fırdöndüsü. Terim tek atımlık ve `settled()`'ın
                // erken dönüşünden **önce** tüketiliyor.
                let mut blink = iv.blink.get();
                let flipped = blink.advance(now);
                iv.blink.set(blink);
                // **Dördüncü soru: yazım efektleri** (030). Soru `advance`'ten
                // **önce** soruluyor: bu adımda biten bir efektin son hâli
                // (geliş statik glyph'ine oturdu, hayalet kalktı) henüz
                // çizilmedi ve uyunsaydı ekranda yarı saydam bir harf asılı
                // kalırdı. Liste bu karede boşaldıysa kare çiziliyor, sıradaki
                // callback uyuyor.
                let mut glyph_fx = iv.glyph_fx.borrow_mut();
                let fx_idle = glyph_fx.is_empty();
                glyph_fx.advance(dt);
                if at_rest(motion, flipped, fx_idle) {
                    // Boşta sıfır kare: yeni içerik de yerleşmemiş animasyon
                    // da yok, link uyur. Sıradaki `Wakeup` onu `Waker`
                    // üzerinden geri açar.
                    //
                    // Örnek de **yazılmıyor** ve bu bir dal değil, yolun
                    // şekli: bu karede `draw` hiç koşmadı, "encode = 0 ns"
                    // diye sahte bir örnek p95'i aşağı çekerdi.
                    iv.motion.set(motion);
                    link.setPaused(true);
                    // **Saat yalnız burada kuruluyor** ve yeri zorunlu: link
                    // ancak yapacak başka işi kalmayınca uyuyor, yani tik de
                    // ancak o an gerekiyor. Uyanıkken kurulsaydı her içerik
                    // karesi bir tik daha dikerdi.
                    self.arm_clock(now);
                    return;
                }
                // **Hareket karesi** (imleç ya da öteleme, ikisi de olabilir).
                // `Waker`'a dokunulmuyor (modül başlığı):
                // link zaten uyanık ve bu callback'in kendisi onu sürdürüyor.
                iv.motion.set(motion);
                // İki sayaç, iki animatör: `hareket=` yalnız imlecin,
                // `kayma=` yalnız ötelemenin tanığı. Aynı karede ikisi birden
                // artabilir; toplamları çizilen kare sayısı **değil**.
                if !motion.cursor_settled() {
                    iv.motion_frames.set(iv.motion_frames.get() + 1);
                }
                if !motion.origin_settled() {
                    iv.slide_frames.set(iv.slide_frames.get() + 1);
                }
                let theme = iv.theme.get();
                let bottom = update.drawable().texture().height() as f32;
                // **Ötelemenin ikinci yazma noktası** (R2.5). Bu kolda
                // `frame()` de `clear` de çağrılmıyor, yani öteleme
                // **korunuyor** — ama animasyonun tanımı iki içerik karesi
                // arasında *değişmek* ve korunan bir değer değişemez.
                // `move_caret`'dan **önce**: imlecin dikdörtgeni bu ötelemeyi
                // pişiriyor.
                self.set_origin(&mut frame, motion, bottom);
                // Liste korunuyor, yalnız imleç taşınıyor: grid kirli değil,
                // yani glyph ve kural listeleri hâlâ geçerli. `Term` kilidine
                // saniyede 120 kez girmek "render yolu bloklanmaz" ile tam
                // burada kavga ederdi.
                if let (Some(at), Some(text)) = (motion.position(), iv.last_caret_text.get()) {
                    frame.move_caret(
                        at,
                        text,
                        theme.cursor_linear(),
                        motion.alpha() * iv.blink.get().alpha(),
                        // **Odak her karede taze okunuyor**, `Frame`'de
                        // saklanandan değil: bu bit `bt-gpu`'nun kendi kararı
                        // ve hareket karesi de ona erişiyor.
                        iv.caret_focused(),
                    );
                }
                // Dock'un statik listeleri korunuyor, yalnız efektler
                // yeniden basılıyor (`move_caret` emsali).
                if !fx_idle {
                    frame.set_dock_fx(glyph_fx.iter(), glyph_fx.clusters(), theme.cursor_linear());
                }
                // CPU örneği **yazılmıyor** ve bu bir eksiklik değil:
                // `cpu_kare` `session.frame`'in kilit beklemesini ölçüyor ve
                // bu karede o iş hiç yok. Bir `truncate` + `push_caret`'un
                // mikrosaniyesi aynı sütuna girseydi p95'i aşağı çekerdi —
                // "sahte örnek" yasağının aynısı.
                //
                // **GPU sütunu buna uymuyor ve uyamaz:** tamamlanma bloğu
                // komut tamponuna bağlı (`Renderer::draw`) ve hareket karesi
                // de bir komut tamponu commit ediyor, yani `record_gpu` bu
                // kareleri **görüyor**. Blok aynı zamanda `FailureStreak`'i
                // besliyor; hareket karesini ondan muaf tutmak çizim hatasını
                // görünmez kılardı, yani ayrılık kasıtlı değil **yapısal**.
                // Sonucu bir ölçüm kapsamı kalemi: `ornek=` ile `gpu_ornek=`
                // farklı kare popülasyonlarını sayıyor (GPU'nunki hareket
                // karelerini de içeriyor) ve iki sütunun p95'i imleç kayan
                // bir koşuda doğrudan karşılaştırılamaz. Kalem
                // `docs/OLCUMLER.md` → `## Yöntem`'de yazılı.
                match iv.renderer.draw(
                    &update.drawable(),
                    theme.background_linear(),
                    &frame,
                    &iv.completion,
                ) {
                    // Hareket karesi de **yola çıkan** bir kare: `sessiz=`
                    // yerleşmeden sonraki kuyruğu ölçmeli, animasyonun
                    // başladığı anı değil. Öteleme de burada yayınlanıyor —
                    // kayma karelerinin fare eşlemesini tazeleyen tek yer bu
                    // kol.
                    Ok(()) => {
                        self.publish_origin(&frame);
                        iv.last_frame_at.set(Some(now));
                    }
                    // **Bu dalın kendi durağı** (`/code-review` bulgusu):
                    // hasar yolunda durak bayrağın dikilmemesiydi, burada
                    // öyle olamaz — "hasar yok" dalı yerleşmemiş animasyon
                    // varken uyumuyor. Bütçe bitince animasyon hedefinde
                    // bitiriliyor, yani sıradaki callback hem hasar hem
                    // bekleyen hareket bulamayıp uyuyor. Olmasaydı kalıcı bir
                    // çizim hatası, kaymanın süre tavanı (0,7 sn) dolana kadar
                    // tazeleme hızında hata satırı basardı — `FailureStreak`
                    // tam bunu önlemek için yazılmıştı.
                    //
                    // **Yalnız senkron hata** (`/code-review` bulgusu):
                    // tamamlanma bloğundan gelen asenkron hata da
                    // `draw_failed`'i çağırıyor ama dönüşünü kullanamıyor —
                    // `iv.motion` ana thread'e bağlı bir `Cell` ve blok başka
                    // bir thread'de koşuyor. O yolda animasyonun durağı
                    // `finish()` değil **süre tavanı**, yani bütçe bitse de
                    // hata satırı en çok 0,7 saniye sürer. Kapatmanın yolu
                    // belli (bloktan dikilen, callback'in tükettiği atomik bir
                    // "bitir" bayrağı) ve bedeli de belli: `Motion`'a ikinci
                    // bir giriş noktası. Ölçülmüş bir ihtiyaç beklemeden
                    // atılmadı.
                    Err(e) => {
                        if iv.retry.draw_failed(&e) {
                            motion.finish();
                            iv.motion.set(motion);
                            glyph_fx.finish();
                            // `Frame`'deki efekt listeleri de boşalıyor: link
                            // uyuyor ve sıradaki hasarsız kare (blink'in tiki)
                            // `set_dock_fx`'e uğramadan eski listeleri yarı
                            // yolda donmuş olarak yeniden çizerdi
                            // (`/code-review`).
                            frame.set_dock_fx(
                                std::iter::empty(),
                                &Clusters::default(),
                                theme.cursor_linear(),
                            );
                        }
                    }
                }
                // **Faz karesinden sonra hemen uyuyor.** Yerleşmiş bir
                // animasyon yokken bu kare yalnız blink'in faz değişimi için
                // çizildi; link açık bırakılsaydı bir sonraki vsync'e kadar
                // bir callback ve `CAMetalDisplayLink`'in ondan önce aldığı
                // bir drawable daha ödenirdi — saniyede iki **görünür** kare
                // için dört tur. Hareket sürüyorsa dokunulmuyor: onun ritmi
                // zaten vsync.
                if motion.settled() && glyph_fx.is_empty() {
                    link.setPaused(true);
                    self.arm_clock(now);
                }
                return;
            }
            frame.clear(iv.cell.get(), iv.caret_style.get());
            // CPU **iki** aralık ölçülüyor, bir değil: kilit beklemesi
            // `session.frame`'in içinde, encode ise `draw`'ın. Tek aralık
            // ikisini toplar ve ayrımı yok eder (R3.1).
            //
            // Kapı kapalıyken saat **hiç** okunmuyor (R4.1): `then` de `map`
            // de closure'ı yalnız dolu tarafta koşturuyor, yani kapalı kapının
            // bedeli bir dallanma.
            let t0 = iv.stats.is_some().then(Instant::now);
            // Drawable'ı boştaki tasarruf kapsamaz: `CAMetalDisplayLink` onu
            // callback'ten ÖNCE alıp `update`'in içine koyuyor, `drawable()`'ı
            // çağırmamak alımı iptal etmiyor. (Bu yüzden `nextDrawable`'ın
            // `Option`'ı ve onun `GpuError::NoDrawable`'ı da kalktı:
            // `update.drawable()` başlıkta `nonnull` ve objc2 onu `Option`suz
            // üretiyor.)
            // **İkinci sink tampona akıyor, doğrudan `Frame`'e değil** ve
            // sebep borç kuralı: iki sink de `frame`'i ödünç alsaydı aynı
            // çağrıda iki `&mut` doğardı (`LinkIvars::fill`, `blocks`'un
            // gerekçesinin ikizi). Hücreler çağrı dönünce banda geçiyor.
            let mut fill = iv.fill.borrow_mut();
            fill.clear();
            // **Izgaranın çizildiği yer taramadan önce bildiriliyor**: kayma
            // uçuştayken ızgara hedefinin altında ve tepesinde açılan şeridi
            // doldurma bandı kapatıyor (`Session::set_grid_top`). Değer bu
            // karenin `advance`'inden önceki konum — yerleşmeye giden kayma
            // için gereğinden bir parça büyük, yani fazlası ekranın dışında.
            //
            // **Bandın fazlası düşülmüş** (032): ızgara çizimde bant kadar
            // yukarıda ve açılan şerit o kadar yukarıda.
            let grid_top = motion.origin() - motion.band();
            // **Geçen süre taramadan önce işleniyor** ve sebebi süzülme: payı
            // konumun bu karedeki değişimi ve `frame()`'in argümanı, yani
            // `frame()`'den önce belli olmak zorunda. İmleç ve öteleme için
            // sıra aynı kalıyor — `advance` yine `sync`'ten önce, ve arada
            // `motion`'ı okuyan kimse yok.
            motion.advance(dt);
            // İstek `advance`'ten **sonra** (`Motion::request_glide`): uykudan
            // uyanan link'in kırpılmış `dt`'si yeni çentiğe uygulanmasın.
            motion.request_glide(iv.session.take_scroll_glide());
            let glide = motion.take_glide();
            iv.session.set_grid_top(grid_top.max(0.0).ceil() as u16);
            // Küme tablosu çağrı boyunca `Frame`'in **dışında**: sink'ler
            // `frame`'i ödünç alıyor (`Frame::take_clusters`). `clear`
            // yukarıda onu boşalttı; iki sink aynı tabloya yazıyor.
            let mut clusters = frame.take_clusters();
            let cursor = iv.session.frame(
                |cell| frame.push(cell),
                |cell| fill.push(cell),
                &mut iv.blocks.borrow_mut(),
                &mut iv.selection.borrow_mut(),
                &mut iv.search.borrow_mut(),
                &mut clusters,
                // Pay **uyandırmıyor**: kareyi zaten bu callback çiziyor
                // (`Session::frame`). Nesli değiştiyse orada düşüyor.
                glide,
                // **Tavan bir oran** (`DOCK_MAX_SHARE`, 032 Karar 4): satır
                // sayısını `frame()` `Term` kilidinin altında okuyor, bu katman
                // onun bir kopyasını tutmuyor. Sarmanın genişliği ızgaranınki —
                // dock aynı sütunları kullanıyor.
                DockBudget {
                    share: crate::frame::DOCK_MAX_SHARE,
                    cols: iv.cols.get(),
                },
            );
            // **Nesil ikinci kez, `frame()`'den sonra**: `frame()` kesri
            // geçersiz bulunca (`CSI 3 J`, alternatif ekran, fare kipi) nesli
            // kendisi artırıyor ve uçuştaki süzülme o karede bitmeli. Konum da
            // soruluyor: pay konumu oynatmadıysa pencere geçmişin ucunda ve
            // süzülme orada bitiyor (`Motion::observe_scroll`) — ikisinde de
            // yoksa kırpmaya çarpan paylar için kare üstüne kare çizilirdi.
            frame.put_clusters(clusters);
            motion.observe_scroll(
                cursor.scroll_generation,
                (cursor.display_offset, cursor.scroll_frac),
                glide.rows,
            );
            // Kesir orijinden **önce** (`Frame::set_origin_rows` onu yazıldığı
            // anda topluyor) ve caret'ten önce (`Frame::push_caret` onu
            // ızgaradaki caret'e ekliyor).
            frame.set_scroll_frac(cursor.scroll_frac);
            // **Kanalın boyu hücrelerden önce** (`Frame::set_fill_rows`):
            // `push_fill`'in bekçisi satırı ona göre ölçüyor. Kanal bant artı
            // kesrin tepe satırı; sıfırsa sınır ikinci sink'i hiç çağırmadı,
            // yani döngü de boş dönüyor ve kare doldurmasız hâliyle bit bit
            // aynı.
            frame.set_fill_rows(cursor.top_row + cursor.fill);
            for cell in fill.drain(..) {
                frame.push_fill(cell);
            }
            // Bandın kendi blok işaretleri (`Blocks::fill_slice`): hücrelerden
            // **sonra**, çünkü `push_fill_block`'un bekçisi bandın boyunu
            // okuyor ve o, hücrelerle aynı karede yazılıyor. Izgaranınkiyle
            // aynı `borrow` turundan geçmiyor — bandın listesi ayrı ve
            // `fill_rules`'a düşüyor.
            for block in iv.blocks.borrow().fill_slice() {
                frame.push_fill_block(*block);
            }
            // Şeritler hücrelerle **aynı** karede ve aynı `frame()` çağrısından:
            // ayrı bir sorgudan okunsalardı kaydırma karesinde bir kare geride
            // kalırlardı (010 discussion.md → Karar 2). Sink içinde değil
            // sonrasında, çünkü blok listesi hücre hücre değil kare başına
            // çözülüyor — ve `borrow_mut` yukarıdaki ifadenin sonunda düştüğü
            // için buradaki `borrow` çakışmıyor.
            //
            // **Animasyon yok** (Karar 5): şerit anında beliriyor, `motion`
            // ikinci bir tüketici kazanmıyor ve bu yol hiçbir kare istemiyor —
            // boşta sıfır kare sözleşmesi dokunulmadan kalıyor. Hareket
            // karesinin yolu (yukarıda, `move_caret`) buraya hiç uğramıyor;
            // ızgara değişmediği için şerit de değişmemeli ve `Frame` onu
            // koruyor.
            for block in iv.blocks.borrow().as_slice() {
                frame.push_block(*block);
            }
            // **Seçimin rengi odaktan** (031 Karar 9): iki renk sınırdan hazır
            // geliyor, hangisinin çizileceği burada. Kare kaynağı yeni değil —
            // odağın değişimi zaten bir içerik karesi istiyor
            // ([`DisplayLink::set_focused`]) ve renk o karede dönüyor; hareket
            // karesi listeyi koruyor, rengi de.
            let selection = iv.selection.borrow();
            let rgba = selection.color(iv.focused.get());
            // Dilim bir kerede: köşe kararı komşu satırın koşusuna bakıyor.
            frame.push_selection(selection.as_slice(), rgba);
            drop(selection);
            // **Arama vurgusu da aynı kuralla** (033 Karar 7): iki rol, renk
            // odaktan; ızgaranın ve bandın koşuları aynı `frame()`
            // turundan. Bandınkiler `set_fill_rows`'tan sonra — bekçisi
            // bandın boyunu okuyor — ve renkleri `push_search`'ün yazdığı
            // uniform. Hareket karesi listeleri koruyor, taramaz (R2.2).
            let search = iv.search.borrow();
            let focused = iv.focused.get();
            frame.push_search(
                search.as_slice(),
                search.match_color(focused),
                search.current_color(focused),
            );
            frame.push_fill_search(search.fill_slice());
            drop(search);
            // Kapının operandı burada artıyor: hasar bulundu, kare çizilecek.
            // `kare`'den önce ve ondan bağımsız — GPU'nun bitirmesini
            // beklemiyor (bkz. `LinkIvars::content_frames`).
            iv.content_frames.set(iv.content_frames.get() + 1);
            // Clear ve imleç rengi oturumun temasından: `frame()`'in zemin
            // atlaması ve renk sorusunun yanıtıyla aynı kaynak. Tema yalnız
            // dolu karede okunuyor — boştaki callback yukarıda döndü — ve
            // hareket karesi için saklanıyor.
            let theme = iv.session.theme();
            iv.theme.set(theme);
            // Süre sayacının tiki **mutlak** damgaya çevriliyor; `None`
            // bekleyen son tarihi temizliyor.
            iv.content_deadline
                .set(content_deadline(now, cursor.next_tick));
            // **Dock artık imleçten ÖNCE** ve sıra zorunlu: caret'in hedefi
            // dock'un caret'ini de sorabilmeli (`Dock::caret`), yani o cevap
            // `motion.sync`'ten önce elde olmak zorunda. Listeye girme sırası
            // çizim sırasını **belirlemiyor** — dock'un kendi listeleri ve
            // kendi encode'u var (`Renderer::encode_pass`), yani sıra orada
            // sabit ve buradaki sıra yalnız veri bağımlılığı.
            //
            // Ayna her tuş vuruşunda kare istiyor: yük ayrıştırıcıya da
            // ulaşıyor ve alacritty işlenen her bayt için `Event::Wakeup`
            // basıyor, yani `dirty` bu kola girmeden önce zaten dikilmiş
            // oluyor. Dock bu yüzden kendi kare talebini taşımıyor — boşta
            // sıfır kare sözleşmesi dokunulmadan kalıyor.
            let dock_rows = iv.dock_rows.get();
            // Pencerenin dibi, **pencere uzayında**: dock'un bandı da caret'in
            // hedefi de ona yaslı. Yükseklik dokudan okunuyor, çünkü tek
            // doğru kaynağı o — `rows * cell_h` artık şeridi (yüksekliğin
            // hücre boyuna bölünmesinden artan piksel) görmezdi ve caret bir
            // hücreye kadar yukarıda dururdu. `Renderer::encode_dock` viewport
            // orijinini aynı çıkarmayla kuruyor, yani ikisi aynı satır.
            let viewport_height = update.drawable().texture().height() as f32;
            let mut dock_caret = None;
            // Yazım efektlerinin saati içerik karesinde de ilerliyor: hızlı
            // yazımda her callback hasar buluyor ve hareket kolu hiç koşmuyor.
            let mut glyph_fx = iv.glyph_fx.borrow_mut();
            glyph_fx.advance(dt);
            if dock_rows > 0 {
                // **Yerleşim hücrelerden önce** (`Frame::set_dock_rows`): giriş
                // satırları + bağlam satırı, `frame()`'in bastırmayla aynı
                // okumada verdiği sayıdan. Bandın tepesi burada yazılmıyor —
                // bant animasyonun değeri ve `sync`'ten sonra
                // (`LinkDelegate::set_origin`).
                frame.set_dock_rows(cursor.input_rows.saturating_add(1));
                let mut dock_state = iv.dock.borrow_mut();
                let mut dock_context = iv.dock_context.borrow_mut();
                // **İkinci sink yerel bir yuvaya akıyor**, doğrudan `Frame`'e
                // değil: iki sink de `frame`'i ödünç alamaz (`fill`'in
                // gerekçesi). Karede en çok bir düzenleme var, yani tampon bir
                // `Option`.
                let mut edit = None;
                // Izgaranın tablosunun ikizi, aynı gerekçe.
                let mut dock_clusters = frame.take_dock_clusters();
                // Devrin cevabı `frame()`'den geliyor, dock yeniden
                // hesaplamıyor: üç ön koşulu (dock'u olan pencere, alternatif
                // ekran, aynanın tazeliği) yalnız o biliyor.
                let dock = iv.session.dock(
                    DockCols {
                        grid: iv.cols.get(),
                        // Bağlam satırının bütçesi: **aynı piksel genişliği,
                        // küçük adım**. Dock sol payı ızgarayla paylaşıyor
                        // (`Frame::dock_pos`), yani iki satırın kapladığı
                        // şerit aynı; ayrışan tek şey bir harfin kaç piksel
                        // ilerlettiği. Hesap burada, çünkü `bt-core` piksel
                        // görmüyor.
                        context: crate::frame::context_cols(iv.cols.get(), iv.cell.get()),
                    },
                    // Sayı hesaplandığı yerden geçiyor, dock yeniden
                    // türetmiyor (`caret_in_dock`'un emsali).
                    cursor.input_rows,
                    &mut dock_state,
                    &mut dock_context,
                    cursor.caret_in_dock,
                    &mut iv.dock_selection.borrow_mut(),
                    &mut dock_clusters,
                    |cell| frame.push_dock(cell),
                    |dock_edit| edit = Some(dock_edit),
                );
                frame.put_dock_clusters(dock_clusters);
                // Sıra zorunlu: düzenleme uçuştakileri kaydırıp bitirebiliyor,
                // statik glyph'i bulunamayan geliş ancak dock basıldıktan
                // **sonra** bilinebiliyor ve çizilecek liste en sonda.
                if let Some(edit) = edit {
                    // Dikey pencerenin boyu `dock()`'a geçen sayının ta
                    // kendisi: kaymanın pencereden taşırdığı efekt düşüyor.
                    glyph_fx.apply(edit, motion, cursor.input_rows, frame.dock_clusters());
                }
                frame.suppress_dock(&mut glyph_fx);
                frame.set_dock_fx(glyph_fx.iter(), glyph_fx.clusters(), theme.cursor_linear());
                dock_caret = dock.caret.map(|at| (at, dock.caret_text));
                // Dock'un seçimi ızgaranınkiyle aynı şekil ve aynı renk
                // uniform'u (031 R3.2); renk yukarıda `push_selection`'la
                // yazıldı — pencerede tek seçim, tek renk.
                frame.push_dock_selection(&iv.dock_selection.borrow());
                // İşaret yoksa girişin ilk satırı dikey pencerenin dışında.
                if let Some(sigil) = dock.sigil {
                    frame.push_dock_sigil(sigil);
                }
                // Yüzey hücrelerden **sonra** açılıyor: renkleri getiren çağrı
                // hücreleri basan çağrının ta kendisi (`Frame::open_dock`).
                frame.open_dock(dock.ground, dock.edge, dock.separator);
            } else {
                // Dock yok (alternatif ekran): efektin konusu da yok.
                glyph_fx.finish();
            }
            drop(glyph_fx);
            // **Caret'in tek hedefi.** İki ev var ve ikisi de aynı animatöre
            // giriyor: dock devraldıysa oraya, almadıysa ızgaradaki imlece.
            // Ayrı animatörler olsaydı dock'ta kayma hiç olmaz, devir de bir
            // ışınlanma kalırdı — kullanıcının iki ayrı şikâyeti, tek sebep.
            //
            // Öncelik dock'ta ve ikisi aynı anda `Some` **olamıyor**: devrin
            // cevabı tek yerde hesaplanıp (`Session::frame`) hem ızgaranın
            // `visible`'ına hem dock'un caret'ine aynı değerden veriliyor
            // (`Cursor::caret_in_dock`). Bu cümle bir zamanlar yanlıştı:
            // `dock::render` yüklemi kendi çağırıyor, `frame()`'in üç ön
            // koşulunu bilmiyordu ve bayat aynada ikisi birden doğuyordu —
            // aşağıdaki `.or_else` dock'u seçince taze satır caret'siz
            // kalıyordu (set kapısı, `/code-review`). Sıra yine de yazılı
            // duruyor: caret'in iki yerde çizilmesindense yanlış yerde
            // çizilmesi görünür bir kusurdur.
            //
            // **Bandın fazlası iki hedefte** (032): ızgaranın caret'i ızgarayla
            // birlikte bandın **hedef** fazlası kadar yukarıda, dock'unki dibe
            // yaslı giriş bloğunda, sarılan satırın kendi satırında.
            let band_target = band_target(cursor, dock_rows);
            let caret = dock_caret
                .map(|(at, text)| {
                    let at = dock_caret_at(
                        at.col,
                        at.row,
                        cursor.input_rows,
                        viewport_height,
                        iv.cell.get(),
                    );
                    (at, text)
                })
                .or_else(|| {
                    cursor.visible.then(|| {
                        (
                            [
                                f32::from(cursor.col),
                                f32::from(cursor.row) + f32::from(origin_target(cursor))
                                    - f32::from(band_target),
                            ],
                            cursor.text,
                        )
                    })
                });
            iv.last_caret_text.set(caret.map(|(_, text)| text));
            // **Blink caret'ten SONRA** ve sıra zorunlu: "yazıyor mu"
            // sorusunun cevabı caret'in hedefinin kıpırdaması ve o hedef
            // ancak burada belli oluyor.
            //
            // Ayarın ve uygulamanın birleşimi `bt-core`'dan geliyor
            // (`Cursor::blink`); Hareketi Azalt onu **kapatıyor** —
            // erişilebilirlik ayarı animasyon *eklemez* (`CLAUDE.md`) ve yan
            // kazancı yapısal: `Mode::Fade` ile blink birbirini dışladığı için
            // `alpha()` kanalına ikinci bir yazar doğmuyor.
            let at = caret.map(|(at, _)| at);
            let moved = iv.last_caret_at.replace(at) != at;
            let mut blink = iv.blink.get();
            // **Üçüncü terim odak** (R7.4): odakta olmayan pencerede blink
            // duruyor ve imleç görünür kalıyor. Yeni bir mekanizma değil —
            // "kapalı blink görünür kalır" değişmezi (`content_frame`,
            // `enabled=false` → `lit=true`, `next_flip=None`) bugün gizli
            // imleci koruyor; bu onun üçüncü tüketicisi. Yan kazancı boşta
            // sıfır kare tarafında: odaksız boş pencere saat kurmuyor.
            // Caret'in odağı iki bitin birleşimi ([`LinkIvars::caret_focused`]):
            // alana yazarken de blink duruyor ve caret içi boş.
            let focused = iv.caret_focused();
            // **Ayarın periyodu burada uygulanıyor** ve `content_frame`'den
            // önce: tik mutlak bir son tarih, yani yeniden kurulurken bu
            // karenin damgası gerekiyor. Aynı değerde no-op.
            blink.set_half_period(now, iv.blink_interval.get());
            blink.content_frame(now, cursor.blink && !motion.reduce() && focused);
            // Caret kıpırdadıysa faz açığa dönüyor: yazarken imleç sönmez.
            if moved {
                blink.wake(now);
            }
            // **Faz burada da ilerliyor** ve dönen değer atılıyor: kare zaten
            // çiziliyor, ayrıca bir uyandırma gerekmiyor. Olmasaydı akan
            // çıktıda (her callback hasar buluyor) faz **donardı** — üstelik
            // sönük fazda donabilirdi ve caret çıktı boyunca görünmezdi.
            blink.advance(now);
            iv.blink.set(blink);
            // `motion.advance` taramadan önce koştu ve sıra zorunlu: önce
            // geçen süre eski hedefe işlenir, sonra yeni hedef kurulur. Ters
            // sırada `dt` yeni hedefe uygulanır ve imleç bir kare boyunca
            // gitmediği bir yöne doğru hızlanırdı.
            //
            // **Dolu ızgaranın kayması** (`Motion::scroll_in`): hedef sabitken
            // satırlar geçmişe kaydıysa öteleme o kadar aşağıdan yeniden
            // süzülüyor. `sync`'ten önce, ki tekerlek ve geometri snap'i bunu
            // da silsin.
            motion.scroll_in(cursor.scrolled, cursor.rows);
            motion.sync(
                caret.map(|(at, _)| at),
                origin_target(cursor),
                band_target,
                cursor.display_offset,
                // Geometri bayrağı burada **tüketiliyor**: tüketilmeseydi
                // bir pencere sürüklemesinden sonraki her kare snap'lerdi.
                iv.geometry_changed.replace(false),
                // **Yön kuralının istisnası burada hesaplanıyor** (017 R4.1):
                // üstteki boşluk geçmişle doluyorsa aşağı inen şey boşluk
                // değil, gelen geçmiş — öteleme yükselirken de süzülüyor.
                // `bt-gpu` "doldurma" diye bir terminal kavramı öğrenmiyor;
                // aldığı şey `offset` ve `geometry` gibi tek bir bit.
                cursor.fill > 0,
            );
            iv.motion.set(motion);
            // **Öteleme `sync`'ten sonra** ve bu sıra zorunlu: çizilecek değer
            // hedef değil animasyonun bu karedeki yeri. `push_caret`'ten
            // **önce** olmak da zorunlu — caret'in dikdörtgeni bu ötelemeyi
            // pişiriyor.
            self.set_origin(&mut frame, motion, viewport_height);
            if let (Some(at), Some((_, text))) = (motion.position(), caret) {
                frame.push_caret(
                    at,
                    text,
                    theme.cursor_linear(),
                    motion.alpha() * blink.alpha(),
                    cursor.shape,
                    focused,
                );
            }
            // Birinci aralık burada kapanıyor — `push_caret`'dan **sonra**:
            // imleci listeye koymak sink işidir, encode değil. Damga bir satır
            // yukarıda alınsaydı `cpu_encode` `draw`'ın yanında onu da ölçer
            // ve jetonun adı yalan söylerdi. Çift tek bir `Option`'da taşınıyor
            // ki "ikisi de var ya da hiçbiri" temsil edilebilir tek durum olsun.
            let spans = t0.map(|t0| (t0, Instant::now()));

            // `frame()` bayrağı çizim başlamadan tüketti; hata hâlinde geri
            // dikilmezse bu içerik bir daha istenmez ve pencere bayat kalır.
            // Senkron ve asenkron hata aynı kapıdan geçiyor.
            let drawn = iv.renderer.draw(
                &update.drawable(),
                theme.background_linear(),
                &frame,
                &iv.completion,
            );
            // Encode aralığı `draw`'ın dönüşüyle kapanıyor: ikinci damga
            // buraya, karar dallarından **önce** düşüyor.
            let spans = spans.map(|(t0, t1)| (t1 - t0, Instant::now() - t1));
            match drawn {
                // Örnek yalnız **yola çıkan** karede yazılır: encode
                // edilemeyen kare hiçbir şey ölçmedi.
                Ok(()) => {
                    // Öteleme de yalnız burada yayınlanıyor: fare eşlemesi
                    // ekranda duran karenin ötelemesini okumalı.
                    self.publish_origin(&frame);
                    // Sessizliğin tabanı da yalnız **yola çıkan** karede
                    // tazeleniyor ve aynı sebeple: encode edilemeyen kare
                    // ekranda hiçbir şey değiştirmedi.
                    // Damga callback'in başında alınan `now`; ikinci bir
                    // `targetTimestamp()` çağrısı aynı değeri döndürür ama
                    // `dt`'nin tabanıyla `sessiz=`'in tabanını iki ayrı
                    // okumaya bağlardı — `last_update_at`'in doc'unun adıyla
                    // yasakladığı şey (`/code-review` bulgusu).
                    iv.last_frame_at.set(Some(now));
                    if let Some((stats, (cpu_frame, cpu_encode))) = iv.stats.as_ref().zip(spans) {
                        stats.record_cpu(cpu_frame, cpu_encode);
                    }
                }
                // Dönüş burada okunmuyor: bu dalın durağı bayrağın
                // dikilmemesi ve o `draw_failed`'in kendi içinde.
                Err(e) => {
                    iv.retry.draw_failed(&e);
                }
            }
            // **Alternatif ekran nöbeti, ölçüm damgalarından sonra.** Kapı
            // bir karşılaştırma ve bir atomik okuma; haberci ancak geçişte
            // (vim açılır/kapanır) koşuyor, yani olağan karede bedeli yok.
            // Damgaların dışında, çünkü geçiş karesinde bir `dispatch` maliyeti
            // `cpu_encode`'a binerdi ve o jeton çizimin süresini iddia ediyor.
            self.notice_alt_screen();
        }
    }
);

impl LinkDelegate {
    /// Alternatif ekran değiştiyse haberciyi çağırır; değişmediyse hiçbir şey.
    ///
    /// **Kapı burada, haberciye değil**: habercinin kendisi ana kuyruğa iş
    /// atıyor ve her karede bir iş atmak boşta sıfır kare sözleşmesini
    /// (`CLAUDE.md`) sessizce bozardı — kuyruğa düşen her iş ana thread'i
    /// uyandırıyor. Karşılaştırma bir `Cell` okuması, yani olağan karede bu
    /// fonksiyonun bedeli ölçülemez.
    ///
    /// Son görülen değer **haberci çağrılmadan önce** yazılıyor: haberci
    /// senkron koşup (sınamada) buraya geri dönseydi ters sıra ikinci bir
    /// bildirim doğururdu.
    fn notice_alt_screen(&self) {
        let iv = self.ivars();
        let Some(notify) = iv.alt_screen_changed.as_ref() else {
            return;
        };
        let now = iv.session.alt_screen();
        if iv.alt_screen.replace(now) == now {
            return;
        }
        notify();
    }

    /// Bu karede **çizilecek** dikey orijin: animasyonun bu andaki satırı →
    /// piksel.
    ///
    /// **İki yazma noktası, tek fonksiyon** (R2.5): içerik karesi `sync`'ten
    /// sonra, hareket karesi `move_caret`'dan önce çağırıyor. İkinci bir
    /// hesap "fare bir satır kayıyor" diye görünen bir ayrışma demekti.
    ///
    /// **İki tüketiciye tek yazma.** Piksel değeri `Frame`'den geri okunuyor,
    /// yeniden hesaplanmıyor: viewport ile fare eşlemesinin aynı sayıyı
    /// görmesi bu satırın işi — kayma boyunca da (R2.7).
    ///
    /// **Dinlenen karede hiçbir içerik kırpılmıyor** ve bunu ötelemenin
    /// *tanımı* veriyor, `setViewport`'un kırpması değil: içerik
    /// `0..content_rows` aralığında, öteleme `rows - content_rows`, yani en
    /// alt dolu satırın bittiği yer tam `rows` satır. Keyfi bir öteleme (ya
    /// da `content_rows`'u büyüten bir kusur) alt satırları dokunun dışına
    /// taşırdı ve belirti "son satır yok" olurdu. **Kayma boyunca öteleme
    /// hedefinden büyük** — içerik yukarı akıyor — yani en alt satırın bir
    /// kısmı o karelerde pencerenin altında kalıyor: yeni satır alt kenardan
    /// yükselerek geliyor ve kayma bitince tam yerine oturuyor. Tek
    /// `setViewport`'un (R1.1) doğrudan sonucu: dört liste birden kayıyor,
    /// yani yeni satırın yerinde belirip ötekilerin kayması temsil edilebilir
    /// bir şey değil.
    ///
    /// **Kaydırmanın kesri ikinci bilinçli istisna** (`Frame::set_scroll_frac`,
    /// `Frame::set_origin_rows` onu topluyor): ızgara kesir kadar aşağıda,
    /// alt satırın o kadarı pencerenin (dock'lu pencerede dock'un zemininin)
    /// altında kalıyor, tepede açılan şeridi de doldurma kanalının tepe satırı
    /// kapatıyor. Dinlenirken kesir yok — jest en yakın satıra oturuyor.
    ///
    /// **Fare eşlemesine yayınlamıyor.** Öteleme kareye burada pişiyor ama
    /// [`Origin`]'e ancak `draw` `Ok` dönünce yazılıyor
    /// ([`Self::publish_origin`]): encode edilemeyen karede ekranda önceki
    /// kare kalır ve tıklama onun ötelemesine göre çevrilmeli.
    ///
    /// **Bandın birleştiği yer de burası** (032): bandın o anki boyu da iki
    /// kare yolundan buraya yazılıyor ve ızgaranın çizilen orijini
    /// `origin − band` ([`compose`]). Öteleme `u16` hedefli kalıyor, işaretli
    /// bir hedefe geçmiyor — birleştirme yalnız çizimde.
    fn set_origin(&self, frame: &mut Frame, motion: Motion, bottom_px: f32) {
        compose(frame, motion, bottom_px, self.ivars().dock_rows.get() > 0);
    }

    /// Çizilen ötelemeyi **ve doldurma bandının boyunu** fare eşlemesine
    /// yayınla — yalnız `draw` `Ok` dönünce.
    ///
    /// Ayrı bir adım, çünkü [`Origin`]'in sözleşmesi **encode edilen** kareyi
    /// söylüyor: `Err` kolunda ekranda önceki kare kalıyor ve o kareyi
    /// yayınlamak tıklamayı ekranda olmayan bir ötelemeye göre çevirirdi.
    /// Aralığı daraltıyor, kapatmıyor — `Ok` "commit edildi" demek, "ekranda"
    /// demek değil; asenkron tamamlanma yine düşebilir ve sözleşme bu yüzden
    /// "çizilen" değil "encode edilen" diyor.
    ///
    /// İki değer **tek** yazmada gidiyor: ikisi de aynı karenin geometrisi ve
    /// ayrı yayınlansalardı fare, orijini yeni bandı eski bir kareye göre
    /// çevirebilirdi. Hareket karesi de buraya uğruyor — band orada korunuyor
    /// (`Frame` temizlenmiyor), yani kayma boyunca yayınlanan değer sabit.
    fn publish_origin(&self, frame: &Frame) {
        self.ivars()
            .origin
            .set(frame.origin_px(), frame.fill_rows(), frame.dock_hit());
    }

    /// **Saat**: kare talebinin üçüncü sebebi (modül başlığı).
    ///
    /// Link uyumaya giderken çağrılıyor ve **iki tadı** var
    /// ([`due_clock`] hangisinin dolduğuna bakıyor):
    ///
    /// - **İçerik tadı** — ilerletilecek bir süre sayacı varsa
    ///   (`Cursor::next_tick`) [`Waker::wake`] ile isteniyor ve hasar bayrağını
    ///   dikmesi **doğru**: ızgaranın çizilen çıktısı gerçekten değişiyor, yani
    ///   `icerik=` sayması yerinde.
    /// - **Hareket tadı** — blink'in faz değişimi [`Waker::resume`] ile, hasar
    ///   **dikmeden**: ızgara değişmiyor, yalnız caret'in alfası. Hareketin
    ///   yasağı bunun tersini korumak içindi (hareket karesinin kendini içerik
    ///   diye saydırması), yani bu kol yasağa uyuyor.
    ///
    /// Durma koşulu **ikisinde de** `None`: sayaç tarafında komut bitti, çıpa
    /// ekrandan çıktı ya da entegrasyon hiç yok; blink tarafında ayar kapalı,
    /// caret çizilmiyor ya da hareketsizlik süresi doldu. İkisi birden `None`
    /// ise tik kurulmuyor ve pencere boşta sıfır kareye dönüyor.
    ///
    /// **Kapı kapalıyken kurulmuyor:** örtülü pencerede zaten
    /// `setPaused(true)` daha yukarıdan dönüyor, yani görünmeyen bir sayacı
    /// güncellemek için kimse uyanmıyor. Görünürlük dönünce `Gate` bir kare
    /// istiyor ve saat oradan yeniden kuruluyor.
    fn arm_clock(&self, now: f64) {
        let iv = self.ivars();
        // **Kuşak her uyku noktasında artıyor, tik kurulmasa da.** `after`
        // iptal edilemiyor; iptalin tek yolu bekleyen tikin kendi kuşağını
        // geçersiz bulması. Artış aşağıdaki erken dönüşlere takılsaydı durma
        // koşulu bir periyot geç işlerdi (`/code-review`, 013 kapı).
        let generation = iv.clock_generation.fetch_add(1, Ordering::Relaxed) + 1;
        // **İki son tarih, tek uyandırma.** Hangisi önce doluyorsa o kuruluyor
        // ve tadını o belirliyor: içerik tiki hasar diker (`icerik=` sayması
        // doğru, ızgara gerçekten değişiyor), blink dikmez (yalnız caret'in
        // alfası değişiyor). İkisi ayrı ayrı kurulsaydı `after` iptal
        // edilemediği için biri ötekinin kuşağını geçersiz kılardı.
        let Some((due, damages)) = due_clock(iv.content_deadline.get(), iv.blink.get().next_flip())
        else {
            // Ne koşan bir sayaç ne sönen bir imleç: pencere boşta sıfır
            // kareye dönüyor.
            return;
        };
        // Geçmişte kalan son tarih **sıfıra doyuyor**: hemen ateşleyen bir tik
        // bir kare fazla ister, biriken bir gecikme ise sonsuza kadar geç
        // kalırdı. Sonsuz/NaN bir damga temsil edilemez ve saat kurulmuyor —
        // panik yolu değil, pencere bir sonraki hasarda zaten uyanıyor.
        let Ok(delay) = Duration::try_from_secs_f64((due - now).max(0.0)) else {
            return;
        };
        let Ok(when) = DispatchTime::try_from(delay) else {
            return;
        };
        let token = Arc::clone(&iv.clock_generation);
        let waker = iv.waker.clone();
        // Hata kolu bugün temsil edilmiyor (`dispatch2` koşulsuz `Ok` dönüyor)
        // ama imza fallible ve sonucu yutmanın bedeli bilinir olmalı: düşen
        // bir tik yalnız sayacı ya da blink'i durdurur — bir sonraki hasar
        // karesi link'i uyandırır, uyku noktasında saat yeniden kurulur.
        let _ = DispatchQueue::main().after(when, move || {
            if token.load(Ordering::Relaxed) == generation {
                if damages {
                    waker.wake();
                } else {
                    waker.resume();
                }
            }
        });
    }

    fn new(mtm: MainThreadMarker, ivars: LinkIvars) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ivars);
        // SAFETY: NSObject'in init'i argümansızdır ve ivar'lar set edildi.
        unsafe { msg_send![super(this), init] }
    }
}

/// Bu karenin öteleme **hedefi**, satır: içerik tabana yapışsın diye ızgaranın
/// kaç satırı üstte boş kalacak.
///
/// **İçerik tabana yapışır** kararı burada, `bt-core`'da değil: o taraf yalnız
/// kaç satırın dolu olduğunu söylüyor (`Cursor::content_rows`), nereye
/// yapışacağı bir yerleşim kararı ve çizenin (`CLAUDE.md` → karar burada,
/// boyama orada).
///
/// `saturating_sub`: sözleşme `content_rows ≤ rows` (`bt-core`'da
/// `debug_assert`) ve doyma sürüm derlemesinde ötelemeyi sıfıra, yani tavana
/// yapışık yerleşime düşürüyor — sarma ızgarayı ekranın dışına atardı.
///
/// **Hedef, çizilen değer değil:** aradaki farkı `crate::motion` kapatıyor
/// (kayma) ve çizen taraf ötelemeyi ondan okuyor ([`LinkDelegate::set_origin`]).
fn origin_target(cursor: Cursor) -> u16 {
    cursor.rows.saturating_sub(cursor.content_rows)
}

/// Bu karenin bant **fazlası** hedefi, satır: çizilecek giriş satırlarının
/// PTY payına sığmayanı (032). Dock'u olmayan karede (alternatif ekran,
/// entegrasyonsuz kabuk) sıfır — bant yok, ızgara ötelenmiyor.
fn band_target(cursor: Cursor, dock_rows: u16) -> u16 {
    if dock_rows == 0 {
        return 0;
    }
    cursor.input_rows.saturating_sub(DOCK_ROWS - 1)
}

/// Kareye bandın ve ötelemenin **o anki** değerini yazar — iki kare yolunun
/// ortak noktası ([`LinkDelegate::set_origin`]).
///
/// Sıra: önce bant, sonra öteleme; `Frame::origin_px` ikisini okuma anında
/// birleştiriyor (`öteleme − bandın fazlası`), yani sıra sonucu
/// değiştirmiyor ama bandın tepesi (`Frame::dock_top_px`) caret'ten
/// **önce** yazılmak zorunda — caret'in yuvası ona bakıyor.
///
/// Dock yoksa bant hiç yazılmıyor: `clear`'ın bıraktığı "söylenmedi"
/// ızgaraya sıfır fazla katıyor ve caret'in yuva sınırı sonsuzda kalıyor.
/// Ayrı bir fonksiyon, çünkü bileşim bekçisi onu `LinkDelegate`'siz
/// koşturuyor.
///
/// Kapı pencerenin dock'u **ve** bu karenin açık yüzeyi: alternatif ekrandan
/// çıkışta pencerenin payı geri gelmiş ama son içerik karesi dock'suz olabilir
/// ve o arada koşan hareket karesi bandı yazsaydı ızgaranın alt satırındaki
/// caret çizilmeyen dock yuvasına düşüp kaybolurdu (`/code-review`).
fn compose(frame: &mut Frame, motion: Motion, bottom_px: f32, dock: bool) {
    if dock && frame.dock().is_some() {
        frame.set_dock_band(bottom_px, motion.band());
    }
    frame.set_origin_rows(motion.origin());
}

/// Dock caret'inin hedefi, **ekran hücresi** cinsinden — [`Motion`]'ın uzayı.
///
/// Dikey bileşen tam sayı **değil** ve olamaz: dock bandı nefes payı kadar
/// aşağıdan başlıyor ve bandın kendisi de ızgaranın hücre ızgarasına oturmuyor
/// (yükseklik hücre boyuna tam bölünmediğinde aradaki artık şerit dock ile
/// içerik arasında kalıyor, `Renderer::encode_dock`). Kesirli hedef bu yüzden
/// bir kaçamak değil doğru cevap.
///
/// **Neden piksel değil de hücre:** `Motion`'ın yay sabitleri ve durma eşiği
/// hücre biriminde ayarlı. Uzayı piksele çevirmek o eşiği sessizce değiştirir
/// ve animasyonun hissi ölçülmemiş bir sayıya bağlanırdı.
///
/// **Dibe yaslı** (032): `row`. giriş satırının tepesi, `input_rows` satırlık
/// bir bandın dibe yaslı yerleşiminde — bandın o anki (animasyonlu) boyundan
/// değil, çünkü hücreler yerleşimde duruyor ve caret onların üstünde.
fn dock_caret_at(
    col: u16,
    row: u16,
    input_rows: u16,
    bottom_px: f32,
    cell: CellMetrics,
) -> [f32; 2] {
    let cell_h = f32::from(cell.cell_px().1);
    let pad = f32::from(cell.gutter_px());
    let top = bottom_px - crate::frame::band_px(input_rows, cell);
    [f32::from(col), (top + pad) / cell_h + f32::from(row)]
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

/// Kare yolunun **açılış geometrisi**: ızgaranın genişliği, dock payı ve
/// hücre ölçüsü.
///
/// Üçü tek tip, çünkü üçü de aynı yerden (`bt-shell`'in `Grid`'i ve dock
/// kararı) aynı anda doğuyor ve [`DisplayLink::new`]'a birlikte giriyor.
/// Ayrı parametreler olsalardı imza yedi argümanı aşıyordu — ama asıl kazanç
/// o değil: bir tip, "bu üçü birlikte değişir" cümlesini imzada söylüyor.
///
/// Satır sayısı **yok** ve bilerek: ızgaranın yüksekliği oturumun
/// (`SessionOptions.rows`) ve kare yolu onu `Cursor::rows` ile **aynı
/// okumadan** alıyor (`bt_core::Cursor::rows`'un doc'u). İkinci bir kopya tam
/// olarak orada yasaklanmış.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    /// Izgaranın genişliği, sütun; dock'un taşan satırı sarması için
    /// gerekiyor. [`DisplayLink::resize`] tazeliyor.
    pub cols: u16,
    /// Dock kaç satır; `0` → bu pencerede dock yok.
    ///
    /// **Doğum değeri oturumun sabiti** (R5.1: entegrasyon kuruldu mu) ama
    /// bu alan onun *o andaki* hâli: alternatif ekranda dock kalkıyor ve
    /// çıkışta iniyor (R5.2), yani [`DisplayLink::resize`] onu da taşıyor.
    /// Sıfıra düşüren iki ayrı sebebi ayırt etmek `bt-shell`'in işi —
    /// entegrasyonsuz bir oturumda alternatif ekrandan çıkmak dock
    /// **doğurmamalı**.
    pub dock_rows: u16,
    /// Hücre ölçüsü ve sol pay; `Frame::clear`'ın taşıdığı değer.
    pub cell: CellMetrics,
}

impl DisplayLink {
    /// Ana thread'de kurulur: link ana run loop'a eklenir ve callback'in ana
    /// thread'de koşacağı sözleşmesi böyle doğar.
    pub fn new(
        mtm: MainThreadMarker,
        surface: &Surface,
        renderer: Rc<Renderer>,
        session: Arc<Session>,
        layout: Layout,
        stats: Option<Arc<Stats>>,
        alt_screen_changed: Option<Box<dyn Fn()>>,
    ) -> Self {
        // Açılış teması: ilk içerik karesi onu zaten tazeleyecek, ama alanın
        // `Option` olması için bir sebep yok — oturumun teması her an geçerli
        // bir cevap. Alternatif ekranın açılış hâli de aynı sebeple okunuyor:
        // nöbetin ilk karşılaştırması bir değere ihtiyaç duyuyor ve "henüz
        // bilmiyorum" hâli, doğumda alternatif ekranda olmayan bir oturum için
        // ilk karede sahte bir geçiş üretirdi.
        let alt_screen = session.alt_screen();
        let theme = session.theme();
        let link =
            CAMetalDisplayLink::initWithMetalLayer(CAMetalDisplayLink::alloc(), surface.layer());
        let waker = Waker {
            inner: Arc::new(WakerInner {
                dirty: session.dirty_flag(),
                link: MainThreadBound::new(link.clone(), mtm),
                gate: Gate::new(),
                requests: AtomicU64::new(0),
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
                Err(e) => {
                    retry.draw_failed(&e);
                }
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
                blocks: RefCell::new(Blocks::default()),
                selection: RefCell::new(SelectionRuns::default()),
                search: RefCell::new(SearchRuns::default()),
                dock_selection: RefCell::new(Vec::new()),
                fill: RefCell::new(Vec::new()),
                dock: RefCell::new(DockState::default()),
                dock_context: RefCell::new(DockContext::default()),
                dock_rows: Cell::new(layout.dock_rows),
                alt_screen: Cell::new(alt_screen),
                alt_screen_changed,
                cols: Cell::new(layout.cols),
                cell: Cell::new(layout.cell),
                // Sıfır: ilk içerik karesine kadar öteleme yok ve o kare
                // değeri söylüyor. Fare yolu bu arada tavana yapışık
                // ızgarayı okuyor, yani açılıştaki tek karelik pencerede de
                // çizilenle aynı şeyi görüyor.
                origin: Origin::default(),
                content_frames: Cell::new(0),
                motion_frames: Cell::new(0),
                slide_frames: Cell::new(0),
                motion: Cell::new(Motion::default()),
                glyph_fx: RefCell::new(GlyphFx::default()),
                geometry_changed: Cell::new(false),
                last_frame_at: Cell::new(None),
                last_update_at: Cell::new(None),
                // İlk içerik karesine kadar kullanılmıyor: hareket karesi
                // ancak `Motion`'da bir konum varsa çiziyor ve orayı dolduran
                // tek yer içerik karesi — o da temayı tazeliyor.
                theme: Cell::new(theme),
                content_deadline: Cell::new(None),
                blink: Cell::new(Blink::default()),
                focused: Cell::new(true),
                keyboard: Cell::new(true),
                caret_style: Cell::new(CaretStyle::default()),
                blink_interval: Cell::new(bt_core::CURSOR_BLINK_INTERVAL),
                clock_generation: Arc::new(AtomicU64::new(0)),
                last_caret_text: Cell::new(None),
                last_caret_at: Cell::new(None),
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

    /// Çizilen karenin dikey orijinini (ve üstündeki doldurma bandının boyunu)
    /// okuyan uç; fare eşlemesi bunu tutar.
    ///
    /// [`Self::waker`] ile aynı örüntü — paylaşılan gövdenin kopyası — ama
    /// yönü ters: `Waker` dışarıdan **yazılıyor**, bu dışarıdan **okunuyor**.
    /// Yazma tarafı bilerek dışarı açılmıyor: orijinin tek sahibi kare yolu
    /// ([`Origin`]).
    pub fn origin(&self) -> Origin {
        self.delegate.ivars().origin.clone()
    }

    /// Koşu boyunca istenen kare sayısı — çizilen değil, **istenen**.
    ///
    /// Rapor bunu `istek=` jetonuyla basıyor; `kare` ile arasındaki fark
    /// birleşen ve kapıda ölen taleplerdir (bkz. [`WakerInner::requests`]).
    pub fn requests(&self) -> u64 {
        self.waker.requests()
    }

    /// Boşta sıfır kare kapısının operandı: çizilmeye **karar verilen** içerik
    /// karesi. Rapor bunu `icerik=` jetonuyla basıyor.
    ///
    /// `kare` ile arasında **sıra ilişkisi yok** ve ikisini karıştırmak
    /// kapıyı yanlış okumak demek: hareket karesi de bir komut tamponu
    /// commit ediyor, yani `kare`'ye yazılıp buraya yazılmıyor
    /// ([`Self::motion_frames`]). Ölçülen sağlıklı duman koşusu `kare` 27–30
    /// iken `icerik` 2–3 (`docs/OLCUMLER.md` → `## Boşta kare`); farkın büyük
    /// kısmı imleç kayması, kalanı encode edilemeyen ve uçuşta kalan kareler.
    /// Kırmızı bir koşuyu okuyan taraf da bunu kullanıyor: üçü birden yüksekse
    /// hasar akıyor, yalnız `kare` yüksekse animasyon yerleşmiyor.
    pub fn content_frames(&self) -> u64 {
        self.delegate.ivars().content_frames.get()
    }

    /// Yerleşmemiş **imleç** animasyonu yüzünden çizilen kare — `hareket=`
    /// jetonu. 011'den beri saf imleç tanığı: yalnız kayma yüzünden çizilen
    /// kareyi `kayma=` sayıyor ve bu sayaç onları görmüyor.
    ///
    /// Duman kapısının **gerekli** sayacı: reçetede bir imleç hareketi var
    /// (`bt_core::smoke_shell`), yani sıfır "animasyon hiç koşmadı" demek.
    /// `icerik=`'e girmiyor ve bu kapının kendisi (008 Karar 2).
    pub fn motion_frames(&self) -> u64 {
        self.delegate.ivars().motion_frames.get()
    }

    /// Yerleşmemiş **kayma** yüzünden çizilen kare — `kayma=` jetonu.
    ///
    /// [`Self::motion_frames`]'in kardeşi, toplananı değil: aynı karede ikisi
    /// birden artabilir. Bir **sayaç, kapı değil** — eşiği ölçülmedi
    /// ([`LinkIvars::slide_frames`]).
    pub fn slide_frames(&self) -> u64 {
        self.delegate.ivars().slide_frames.get()
    }

    /// Animasyon durdu mu — kapının **ölçüm istemeyen** yarısı.
    ///
    /// Süreli koşu bunu deadline'da bir kez soruyor: `false` ise koşu kırmızı
    /// (`Verdict::MotionUnsettled`). Hızdan bağımsız olması bütün değeri —
    /// `IDLE_FRAME_LIMIT` ancak yeterince hızlı bir sızıntıyı görüyor, bu
    /// soru ise durma koşulu unutulmuş **her** animasyonu görüyor, ne kadar
    /// yavaş olursa olsun.
    ///
    /// Gördüğünün sınırı: yalnız [`crate::motion`]'dan ve dock'un yazım
    /// efektlerinden ([`crate::glyph_fx`]) geçen animasyonlar.
    /// Altyapıyı atlayıp kendi kendine kare isteyen bir yolu bu soru göremez;
    /// onun kapısı [`Self::quiet_since`]'ın ölçülmüş eşiği.
    pub fn motion_settled(&self) -> bool {
        let iv = self.delegate.ivars();
        iv.motion.get().settled() && iv.glyph_fx.borrow().is_empty()
    }

    /// Son çizilen kareden bu yana geçen süre — `sessiz=` jetonu.
    /// `None` → hiç kare çizilmedi.
    ///
    /// **Koşunun tek saat okuması.** Kare yolunda damga bir alan kopyası
    /// (`LinkIvars::last_frame_at`); `CACurrentMediaTime()` yalnız burada,
    /// yani deadline'da bir kez çağrılıyor. Ölçüm kapısı (`BT_FRAME_STATS`)
    /// kapalıyken kare başına saat okunmaması sözleşmesi bu ayrımda duruyor.
    ///
    /// **Neyin arasını ölçüyor:** damga karenin *hedef sunum* anı, yani
    /// gelecekte bir nokta. Deadline son kareden bir tazeleme içinde düşerse
    /// fark negatif çıkar; değer sıfıra doyuruluyor. Yorumlayan taraf
    /// `sessiz=0.00ms`'i "deadline anında kare akıyordu" diye okumalı,
    /// "tam o anda çizildi" diye değil.
    ///
    /// **Kapının en duyarlı katı** ve `bt-gpu`'nun dışında değerlendiriliyor:
    /// eşik ölçülmüş bir sözleşme (`bt-shell`'in `QUIET_FLOOR`'u, 008 phase-6)
    /// ve duman yükünde altı kırmızı. Buradaki sorumluluk yalnız sayıyı
    /// dürüstçe üretmek — `None` "hiç kare çizilmedi", `0.00ms` "deadline
    /// anında kare akıyordu".
    ///
    /// Gördüğünün sınırı [`Self::motion_settled`]'ınkinin tümleyeni: o,
    /// altyapıdan geçen animasyonu hızından bağımsız görüyor; bu ise
    /// altyapıyı atlayan **her** kare kaynağını görüyor, ama yalnız periyodu
    /// eşikten kısaysa.
    pub fn quiet_since(&self) -> Option<Duration> {
        let last = self.delegate.ivars().last_frame_at.get()?;
        Some(Duration::try_from_secs_f64(CACurrentMediaTime() - last).unwrap_or(Duration::ZERO))
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
            // Uçuştaki kayma **hedefinde bitiriliyor**: link duracağı için
            // `advance` bir daha koşmaz ve animasyon sonsuza kadar
            // "yerleşmemiş" kalırdı — süreli koşu deadline'da kod doğruyken
            // `MotionUnsettled` derdi. Gerekçenin tamamı [`Motion::finish`]'te.
            let iv = self.delegate.ivars();
            let mut motion = iv.motion.get();
            motion.finish();
            iv.motion.set(motion);
            // Yazım efektleri de: arka sekmede donan bir efekt geri gelince
            // görülmemiş bir fazdan devam ederdi.
            iv.glyph_fx.borrow_mut().finish();
            self.link.setPaused(true);
        }
    }

    /// İmlecin kayma stili değişti: kullanıcı `settings.toml`'u kaydetti ya da
    /// pencere açılıyor (`bt-shell` çözülmüş değeri veriyor,
    /// `Renderer::set_font` emsali — `bt-gpu` ayar dosyası görmez).
    ///
    /// **Kare istemesinin sebebi "hasar yok" dalının şekli:** orada yerleşmiş
    /// bir animasyon hiç çizmeden link'i uyutuyor. `snap`'e geçen kullanıcının
    /// uçuştaki imleci hedefinde bitiriliyor ([`Motion::set_style`]) ama o
    /// yeni konum ekrana ancak bir kare çizilirse düşer — istenmeseydi imleç
    /// ara hücrede asılı kalır ve onu yerine koyan şey alakasız bir shell
    /// çıktısı olurdu. İstek yalnız gerçekten bitirilen kaymada gidiyor:
    /// aynı stili yeniden yazan kayıt ve yerleşmiş bir imleç no-op
    /// (`Session::set_theme`'in aynı stili takas etmeme kuralı).
    ///
    /// Öteki iki stile geçiş kare istemiyor: uçuştaki kayma sürüyorsa link
    /// zaten uyanık ve sıradaki hareket karesi yeni stili uyguluyor.
    pub fn set_cursor_motion(&self, style: CursorMotion) {
        let iv = self.delegate.ivars();
        let mut motion = iv.motion.get();
        let mut finished = motion.set_style(style);
        iv.motion.set(motion);
        // `snap` yazım efektlerini de kapatıyor (`Motion::glyph_fx`) ve
        // uçuştakiler hedefinde bitiyor — aynı gerekçe, aynı kare talebi.
        if style == CursorMotion::Snap {
            let mut glyph_fx = iv.glyph_fx.borrow_mut();
            finished |= !glyph_fx.is_empty();
            glyph_fx.finish();
        }
        if finished {
            self.request_frame();
        }
    }

    /// Dock'un yazım efektleri değişti (`[motion] keypress` / `erase`):
    /// kullanıcı `settings.toml`'u kaydetti ya da pencere açılıyor.
    ///
    /// Adlar **ham** geliyor — [`DisplayLink::set_cursor_motion`]'ın aksine
    /// burada `bt-shell`'in çözeceği bir şey yok: `snap` ile Hareketi
    /// Azalt'ın indirgemesi imlecin kipiyle aynı yerde, `bt-gpu`'da
    /// (`Motion::glyph_fx`), ve ikisinin girdisi zaten link'te.
    ///
    /// Değişim uçuştakileri bitiriyor ([`GlyphFx::set_effects`]) ve bitirilen
    /// bir şey varsa kare istiyor — `set_cursor_motion`'ın gerekçesi: "hasar
    /// yok" dalı yerleşmiş animasyonu çizmeden uyuyor, istenmeseydi yarı
    /// saydam bir harf ekranda asılı kalırdı. Aynı seçim ve boş liste no-op.
    pub fn set_glyph_fx(&self, keypress: Keypress, erase: Erase) {
        let finished = self
            .delegate
            .ivars()
            .glyph_fx
            .borrow_mut()
            .set_effects(keypress, erase);
        if finished {
            self.request_frame();
        }
    }

    /// Hareketi Azalt açıldı ya da kapandı — `bt-shell` **çözülmüş** değeri
    /// veriyor: üç değerli `reduce_motion` ile sistemin cevabını o birleştiriyor
    /// ve `bt-gpu` ne ayar dosyası ne `NSWorkspace` görüyor
    /// ([`DisplayLink::set_cursor_motion`] ile aynı örüntü).
    ///
    /// **İki yön de kare isteyebilir** ve sebebi yine "hasar yok" dalının
    /// şekli: uçuştaki animasyon her iki yönde de hedefinde bitiriliyor
    /// ([`crate::motion::Motion::set_reduce`]) ve yerleşmiş bir animasyon o
    /// dalda hiç çizilmeden uyuyor — istenmeseydi imleç ara hücrede ya da yarı
    /// saydam asılı kalırdı. Yerleşmiş imleçte ve aynı değerde no-op.
    pub fn set_reduce_motion(&self, reduce: bool) {
        let iv = self.delegate.ivars();
        let mut motion = iv.motion.get();
        let changed = motion.reduce() != reduce;
        let finished = motion.set_reduce(reduce);
        iv.motion.set(motion);
        // Yazım efektleri de iki yönde bitiyor (`Motion::set_reduce`'un
        // gerekçesi); kare talebi aşağıdaki `changed`'den.
        if changed {
            iv.glyph_fx.borrow_mut().finish();
        }
        // **Değişimin kendisi kare istiyor, yalnız yarıda kalan animasyon
        // değil.** Eski hâl `Motion` her animasyonun sahibiyken doğruydu;
        // blink onun dışında yaşıyor ve kapısı yalnız **içerik** karesinde
        // okunuyor (`Cursor::blink` ile birleşiyor). Boştaki bir pencerede
        // Hareketi Azalt açılınca kare istenmezse blink sönmeye devam ederdi —
        // `CLAUDE.md`'nin "açıkken blink hiç başlamaz" sözü yalan olurdu.
        if finished || changed {
            self.request_frame();
        }
    }

    /// İmlecin çizim sayıları değişti — `bt-shell` ayar dosyasından veriyor.
    ///
    /// **Aynı değerde no-op, değişimde kare** ve gerekçe kardeşlerininkiyle
    /// aynı ([`DisplayLink::set_cursor_motion`], [`DisplayLink::set_focused`]):
    /// boştaki bir pencerede kaydedilen yarıçap bir sonraki hasara kadar
    /// ekrana hiç düşmezdi ve kullanıcı ayarın çalışmadığını sanırdı.
    pub fn set_caret_style(&self, style: CaretStyle) {
        let iv = self.delegate.ivars();
        if iv.caret_style.replace(style) == style {
            return;
        }
        self.request_frame();
    }

    /// Blink'in yarım periyodu değişti — `bt-shell` ayar dosyasından veriyor.
    ///
    /// **Bekleyen tik yeniden kuruluyor** ([`crate::blink::Blink::set_half_period`])
    /// ve kare isteniyor; ikisi de zorunlu. Kurulmuş bir `after` **iptal
    /// edilemiyor** (`arm_clock`), yani uyuyan bir pencerede yalnız alanı
    /// yazmak yeni ritmi bir sonraki flip'e kadar geciktirirdi — kullanıcı
    /// kaydeder, hiçbir şey olmaz.
    ///
    /// **Değer yuvaya konuyor, tik burada kurulmuyor**: bu yol callback'in
    /// dışında koşuyor ve elinde bir kare damgası yok. Deponun tek zaman
    /// tabanı display link'in damgası (`update.targetTimestamp()`); burada bir
    /// saat okumak ikinci bir zaman yaratırdı. Uygulama kare yolunda, istenen
    /// kare geldiğinde.
    pub fn set_blink_interval(&self, half_period: f64) {
        let iv = self.delegate.ivars();
        if iv.blink_interval.replace(half_period) == half_period {
            return;
        }
        self.request_frame();
    }

    /// Klavye terminale geldi ya da gitti — `bt-shell`'in view'ı first
    /// responder olunca/bırakınca veriyor (033 R7; arama panelinin alanı).
    ///
    /// [`DisplayLink::set_focused`]'ın kuralı: aynı değerde no-op, değişimde
    /// kare — caret'in içi boşalacak ya da dolacak, blink duracak ya da
    /// başlayacak.
    pub fn set_keyboard_in_terminal(&self, keyboard: bool) {
        let iv = self.delegate.ivars();
        if iv.keyboard.replace(keyboard) == keyboard {
            return;
        }
        self.request_frame();
    }

    /// Pencere odağı değişti — `bt-shell`'in `NSWindowDelegate`'i veriyor.
    ///
    /// **Aynı değerde no-op** (015 R7.2; emsal [`crate::Session::set_theme`]):
    /// açılıştaki `windowDidBecomeKey:` tam bu yola düşüyor ve bedava bir
    /// içerik karesi yazardı.
    ///
    /// **Değişimin kendisi kare istiyor** ve gerekçesi `set_reduce_motion`'ın
    /// aynısı: caret'in içi boşalacak ya da dolacak, blink duracak ya da
    /// başlayacak — boştaki bir pencere bunların hiçbirini bir sonraki hasara
    /// kadar göstermezdi.
    pub fn set_focused(&self, focused: bool) {
        let iv = self.delegate.ivars();
        if iv.focused.replace(focused) == focused {
            return;
        }
        self.request_frame();
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
    /// **Sol pay aynı kapıdan geçiyor.** Kapı ayrılsaydı reddedilen bir
    /// boyutta pay yeni, ızgara eski kalır ve glyph'ler `cols` hesabından
    /// kayardı. Payın hücre ölçüsüyle **birlikte** değişmesi ise bir kod
    /// değişmezi değil, bugünkü ölçeklerin sonucu (`/audit`, 010 kapı): ikisi
    /// de ölçeğin fonksiyonu (`Renderer::cell_metrics` tek çağrıda veriyor)
    /// ama `round(8.0 * scale)` ile `round_up(cell_w * scale)` ayrı
    /// fonksiyonlar. macOS'un tam sayılı backing ölçeklerinde (1.0, 2.0)
    /// ayrışamıyorlar; kesirli bir ölçek gelirse yalnız payı değişen bir
    /// metrik `Session::resize`'ın "zaten aynı" dalına takılıp düşerdi ve
    /// `Frame::pos_at` ile `point_to_cell` bir kare boyunca ayrışırdı.
    /// Kapanacağı yer o ölçeğin geldiği gün burasıdır.
    /// **İmleç bu karede snap'ler.** Geometri değişiminde imleç hareket
    /// etmedi, altındaki ızgara hareket etti (008 Karar 5) — animasyon onu
    /// olmadığı bir yerden geliyormuş gibi gösterirdi. Bayrak koşulsuz
    /// dikiliyor, `Session::resize`'ın kabulüne bağlı değil: hücre ölçüsü
    /// değişmese de pencere oynamış olabilir.
    pub fn resize(&self, cols: u16, rows: u16, cell: CellMetrics, dock_rows: u16) {
        let iv = self.delegate.ivars();
        if iv.session.resize(cols, rows, cell.cell_px()) {
            iv.cell.set(cell);
        }
        // Dock payı **kapının dışında** ve `cols` ile aynı gerekçe: reddedilen
        // bir boyutta (simge durumundaki pencere) dock zaten hiçbir şey
        // çizmiyor, ama payı eski değerde bırakmak alternatif ekrandan
        // çıkarken dock'u bir kare geç geri getirirdi.
        iv.dock_rows.set(dock_rows);
        // Sütun sayısı **kapının dışında**: dock'un sarması çizilen
        // genişliği görmeli ve reddedilen bir boyutta (simge durumundaki
        // pencere) `cols` zaten sıfır — dock o karede metin çizmiyor
        // (`bt_core::dock::render`), yani ızgaranın eski ölçüde kalmasıyla
        // çelişen bir şey yapmıyor.
        iv.cols.set(cols);
        iv.geometry_changed.set(true);
        // Yazım efektleri de bitiyor (`Motion`'ın geometri snap'inin
        // kardeşi): sütun sayısı ya da hücre değişince dock'un sarması
        // yeni bir ayna gelmeden kayabiliyor ve uçuştakiler eski sütunlarında
        // başka bir harfin üstünde kalırdı.
        iv.glyph_fx.borrow_mut().finish();
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

/// Süre sayacının tikini **mutlak** bir son tarihe çevirir; `None` bekleyen
/// son tarihi **temizler**.
///
/// Ayrı bir fonksiyon, `arm_clock`'ınki ile aynı gerekçeyle: kusurun kendisi
/// burada yaşıyor ve `needs_update`'in gövdesinde sınanamıyordu. `None`'ın
/// temizlemesi 013'ün kapısının dersi — biten komutun bayat son tarihi
/// kalsaydı bir kare fazla istenirdi.
fn content_deadline(now: f64, tick: Option<Duration>) -> Option<f64> {
    tick.map(|tick| now + tick.as_secs_f64())
}

/// Saatin iki son tarihinden hangisi önce doluyor ve **hangi tadı** istiyor
/// (`true` → hasar diken içerik tadı, `false` → hasar dikmeyen hareket tadı).
///
/// Ayrı bir fonksiyon, çünkü 013'ün kapısında düzeltilen kusurun yeni kılığı
/// tam burada yaşıyor ve `arm_clock`'ın gövdesinde sınanamıyordu: o metot
/// ObjC sınıfına bağlı ve gerçek bir pencere istiyor.
///
/// **Sözleşme:** içerik son tarihi blink'in tiklerinden **etkilenmiyor**.
/// Eski hâlde saat bir süre tutuyordu ve her uyku noktasında baştan
/// kuruluyordu; saniyede iki kez uyanan bir blink, koşan komutun bir
/// saniyelik tikini her seferinde bir saniye ileri iter ve tik hiç
/// ateşlemezdi.
fn due_clock(content: Option<f64>, flip: Option<f64>) -> Option<(f64, bool)> {
    match (content, flip) {
        (Some(content), Some(flip)) if flip < content => Some((flip, false)),
        (Some(content), _) => Some((content, true)),
        (None, Some(flip)) => Some((flip, false)),
        (None, None) => None,
    }
}

/// "Hasar yok" dalının uyku sorusu: hareket yerleşmiş, blink'in fazı
/// dönmemiş ve yazım efektlerinde **bu adımdan önce** uçuşta bir şey yok.
///
/// Efektin sorusu `advance`'ten önceki hâle bakıyor ve bu şart: bu adımda
/// biten bir efektin son hâli (geliş statik glyph'ine oturdu, hayalet kalktı)
/// henüz çizilmedi; uyunsaydı ekranda yarı saydam bir harf asılı kalırdı.
/// Liste boşaldığı karede çiziliyor, sıradaki callback uyuyor.
fn at_rest(motion: Motion, flipped: bool, fx_idle: bool) -> bool {
    motion.settled() && !flipped && fx_idle
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dock_caret_target_lands_on_the_band_not_the_grid_row() {
        // **Kesirli hedef bir kaçamak değil, doğru cevap.** Dock bandı iki
        // yerden ızgaranın hücre ızgarasından kayıyor: nefes payı kadar
        // aşağıdan başlıyor ve bandın kendisi de pencerenin yüksekliği hücre
        // boyuna tam bölünmediğinde artan şeridin altında duruyor. Hedefi tam
        // sayıya yuvarlasaydık caret bir hücreye kadar yukarıda dururdu.
        let cell = CellMetrics::new(9, 18, 9, 8, 1).expect("ölçü");
        // 600 px pencere, iki satırlık dock: 2×18 satır + 2×8 dış pay +
        // 1×16 satır arası = 68, yani band 532'de başlıyor.
        let dock_top = 600.0 - crate::frame::dock_px(2, cell);
        assert_eq!(dock_top, 532.0);

        let [col, row] = dock_caret_at(3, 0, 1, 600.0, cell);
        assert_eq!(col, 3.0, "sütun ızgarayla aynı uzayda");
        // Caret bandın **ilk satırında**, yani dış payın altında: (532+8)/18.
        assert_eq!(row, 540.0 / 18.0);
        // Ve o satır ızgaranın son satırının (548/18 = 30.4) **altında**:
        // yuvarlansaydı ikisi çakışırdı.
        assert!(row > dock_top / 18.0, "caret banda inmedi");

        // **Dibe yaslı** (032): üç giriş satırlık bantta bandın tepesi iki
        // satır yukarıda (600 − 104 = 496), ilk satır (496+8)/18'de ve **son**
        // satır tek satırlık bantın satırıyla aynı yerde — bant yukarı
        // büyüyor, caret'in yazdığı satır yerinden oynamıyor.
        assert_eq!(dock_caret_at(3, 0, 3, 600.0, cell)[1], 504.0 / 18.0);
        assert_eq!(dock_caret_at(3, 2, 3, 600.0, cell)[1], row);
    }

    #[test]
    fn the_grid_the_fill_band_and_the_dock_band_meet_in_every_frame() {
        // **Bileşim bekçisi** (032 phase-2, `n = 3`): ızgaranın alt kenarı,
        // doldurma bandı ve dock bandının üst kenarı **aynı karede**
        // çakışıyor — animasyonun ortasında da. Bileşenleri ayrı ayrı sınamak
        // yetmez: bant ve öteleme iki ayrı animatör, birleştikleri yer çizim
        // (`compose`) ve iki ayrı yuvarlama bir piksel ayrışabilirdi.
        //
        // @1x, 9×18 hücre, pay 8, 600 px pencere: PTY payı `2·18 + 2·8 + 16 =
        // 68`, ızgara `⌊532/18⌋ = 29` satır ve artık şerit 10 px. Şerit bant
        // büyürken de 10 px kalmalı: ızgara bandla birlikte yukarı gidiyor.
        let cell = CellMetrics::new(9, 18, 9, 8, 1).expect("ölçü");
        const BOTTOM: f32 = 600.0;
        const ROWS: f32 = 29.0;
        const FILL: u16 = 2;
        let strip = BOTTOM - crate::frame::dock_px(DOCK_ROWS, cell) - ROWS * 18.0;
        assert_eq!(strip, 10.0);

        // İçerik tabana yaslı (öteleme 5), bant tek satırda; sonra dock üç
        // giriş satırı istiyor: bandın fazlası 0 → 2.
        let mut motion = Motion::default();
        motion.sync(Some([0.0, 30.0]), 5, 0, 0, false, false);
        motion.sync(Some([0.0, 30.0]), 5, 2, 0, false, false);
        let mut frames = 0;
        let mut mid = false;
        loop {
            let mut frame = Frame::default();
            frame.clear(cell, CaretStyle::default());
            frame.set_dock_rows(4);
            frame.set_fill_rows(FILL);
            frame.open_dock(
                Theme::BATERI.background_linear(),
                Theme::BATERI.accent_linear(),
                Theme::BATERI.accent_linear(),
            );
            compose(&mut frame, motion, BOTTOM, true);

            // İçeriğin alt kenarı: orijin + dolu satırlar (`29 − 5`).
            let grid_bottom = frame.origin_px() + (ROWS - 5.0) * 18.0;
            let band_top = BOTTOM - frame.dock_band_px();
            assert_eq!(
                band_top - grid_bottom,
                strip,
                "kare {frames}: ızgara ile bant ayrıştı (bant {})",
                motion.band()
            );
            assert_eq!(
                frame.fill_origin_px() + f32::from(FILL) * 18.0,
                frame.origin_px(),
                "kare {frames}: doldurma bandı ızgaradan koptu"
            );
            mid |= motion.band() > 0.0 && motion.band() < 2.0;
            if motion.settled() {
                break;
            }
            motion.advance(1.0 / 120.0);
            frames += 1;
            assert!(frames < 1000, "bant yerleşmedi");
        }
        assert!(mid, "animasyonun ortası hiç sınanmadı");
        // Yerleşince bant yerleşimin boyunda ve ızgara iki satır yukarıda.
        let mut frame = Frame::default();
        frame.clear(cell, CaretStyle::default());
        frame.set_dock_rows(4);
        frame.open_dock(
            Theme::BATERI.background_linear(),
            Theme::BATERI.accent_linear(),
            Theme::BATERI.accent_linear(),
        );
        compose(&mut frame, motion, BOTTOM, true);
        assert_eq!(frame.dock_band_px(), frame.dock_layout_px());
        assert_eq!(frame.origin_px(), (5.0 - 2.0) * 18.0);
    }

    #[test]
    fn a_full_grid_is_clipped_from_the_top_while_the_band_is_tall() {
        // Dolu ızgarada (öteleme 0) bant büyüyünce orijin **negatife** iniyor
        // ve ızgaranın tepesi pencerenin dışında kalıyor — geçici, giriş
        // bitince döner. Fare aynı orijini okuyor (`Origin::px`), yani
        // görünen satıra tıklanan nokta doğru satır (`point_to_cell`'in
        // negatif orijin kolu).
        let cell = CellMetrics::new(9, 18, 9, 8, 1).expect("ölçü");
        let mut motion = Motion::default();
        motion.sync(None, 0, 2, 0, false, false);
        let mut frame = Frame::default();
        frame.clear(cell, CaretStyle::default());
        frame.set_dock_rows(4);
        frame.open_dock(
            Theme::BATERI.background_linear(),
            Theme::BATERI.accent_linear(),
            Theme::BATERI.accent_linear(),
        );
        compose(&mut frame, motion, 600.0, true);
        assert_eq!(frame.origin_px(), -36.0);
        // Dock'suz pencerede bant hiç yazılmıyor: orijin yalnız öteleme.
        let mut frame = Frame::default();
        frame.clear(cell, CaretStyle::default());
        compose(&mut frame, motion, 600.0, false);
        assert_eq!(frame.origin_px(), 0.0);
        // Pencerenin payı var ama bu karenin yüzeyi kapalı (vim'den çıkışın
        // arası): bant yazılmıyor, caret'in yuva sınırı sonsuzda kalıyor.
        let mut frame = Frame::default();
        frame.clear(cell, CaretStyle::default());
        compose(&mut frame, motion, 600.0, true);
        assert_eq!(frame.origin_px(), 0.0, "yüzeysiz karede bant yazıldı");
    }

    #[test]
    fn glyph_effects_keep_the_link_awake_and_draw_their_last_frame() {
        // Yazım efektinin uyku terimi: uçuşta bir şey varken link uyumuyor,
        // efektin bittiği adım yine de çiziliyor ve ancak ondan sonraki
        // callback uyuyor. Hasar dikmiyor — `GlyphFx` `Waker`'ı hiç görmüyor.
        use crate::glyph_fx::{GlyphFx, KEYPRESS_DURATION};
        let mut fx = GlyphFx::default();
        let arrival = bt_core::DockEdit::Arrive {
            row: 0,
            col: bt_core::DOCK_TEXT_COL,
            cells: [bt_core::Cell {
                col: bt_core::DOCK_TEXT_COL,
                ch: Some('a'),
                ..bt_core::Cell::default()
            }]
            .into_iter()
            .collect(),
            shift: 0,
        };
        fx.apply(arrival, Motion::default(), 1, &Clusters::default());
        let dt = 1.0 / 120.0;
        let mut drawn = 0usize;
        let mut emptied_on_a_drawn_frame = false;
        loop {
            let fx_idle = fx.is_empty();
            fx.advance(dt);
            if at_rest(Motion::default(), false, fx_idle) {
                break;
            }
            drawn += 1;
            emptied_on_a_drawn_frame |= fx.is_empty();
            assert!(drawn < 1000, "efekt hiç yerleşmedi");
        }
        assert!(
            emptied_on_a_drawn_frame,
            "efektin son hâli çizilmeden uyundu"
        );
        let expected = (KEYPRESS_DURATION / dt).ceil() as usize;
        assert!(
            drawn.abs_diff(expected) <= 1,
            "efekt {drawn} kare sürdü, süresi {expected} kare"
        );
        // Boş listede ilk soruda uyunuyor: efektsiz pencere boşta.
        assert!(at_rest(
            Motion::default(),
            false,
            GlyphFx::default().is_empty()
        ));
    }

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

    #[test]
    fn the_clock_picks_the_nearer_deadline_and_its_flavour() {
        // İçerik tiki hasar diker (ızgara gerçekten değişiyor), blink dikmez
        // (yalnız caret'in alfası).
        assert_eq!(due_clock(Some(1.0), Some(0.5)), Some((0.5, false)));
        assert_eq!(due_clock(Some(1.0), None), Some((1.0, true)));
        assert_eq!(due_clock(None, Some(0.5)), Some((0.5, false)));
        assert_eq!(due_clock(None, None), None, "boşta saat kuruldu");
        // Eşitlikte içerik kazanıyor: kare zaten çizilecek, hareket tadına
        // ikinci bir uyandırma gerekmiyor.
        assert_eq!(due_clock(Some(1.0), Some(1.0)), Some((1.0, true)));
    }

    #[test]
    fn a_blinking_cursor_does_not_starve_the_duration_counter() {
        // **013'ün regresyon bekçisi.** Koşan bir komutun sayacı t=1.0'da
        // tiklemeli; blink 0.5'te bir uyanıyor. Her uyanışta saat yeniden
        // kuruluyor ve eski (süre temelli) hâlde sayacın tiki her seferinde
        // bir saniye ileri itilir, yani **hiç** ateşlemezdi.
        //
        // Son tarih mutlak olduğu için blink'in tikleri onu oynatmıyor.
        let content = Some(1.0);
        let mut blink = Blink::default();
        blink.content_frame(0.0, true);

        // t=0: blink daha yakın, hareket tadı kuruluyor.
        assert_eq!(due_clock(content, blink.next_flip()), Some((0.5, false)));

        // t=0.5: blink döndü ve kendi tikini ileri attı; sayacınki **yerinde**.
        assert!(blink.advance(0.5), "blink dönmedi");
        assert_eq!(blink.next_flip(), Some(1.0));
        assert_eq!(
            due_clock(content, blink.next_flip()),
            Some((1.0, true)),
            "sayacın tiki blink tarafından itildi"
        );
    }

    #[test]
    fn a_finished_command_clears_the_clock() {
        // **R7.3.** Koşan komutun tiki mutlak damgaya çevriliyor; komut
        // bitince (`next_tick` `None`) saklanan son tarih **temizleniyor**.
        // Eşleme `Some`'ı korusaydı ya da `None`'ı yoksaysaydı 013'ün
        // kapısında düzeltilen kusur geri gelirdi.
        assert_eq!(
            content_deadline(5.0, Some(Duration::from_millis(400))),
            Some(5.4)
        );
        assert_eq!(
            content_deadline(5.0, None),
            None,
            "biten komut saati bıraktı"
        );
        // Temizlenmiş son tarih ve sönmeyen bir imleç: saat hiç kurulmuyor.
        assert_eq!(due_clock(content_deadline(5.0, None), None), None);
    }
}
