//! Kareyi süren şey: `CAMetalDisplayLink` ve onu uzaktan açan `Waker`.
//!
//! Sözleşme tek cümlede: **link paused durur.** Yeni içerik geldiğinde
//! (`Wake::wake` → [`Waker`]) açılır, hasar **ve hareket** tükenince callback
//! onu geri kapatır. "Boşta sıfır kare" bu iki satırda yaşıyor; her
//! `setPaused(false)` bir gerekçe ister ve her kare bir durma koşulu taşır.
//!
//! **Kareyi isteyen iki şey var ve ikincisi uyandırmaz** (008):
//!
//! - **Hasar** — `Waker` üzerinden, başka bir thread'den, bayrak dikerek.
//! - **Hareket** ([`crate::motion`]) — kimseyi uyandırmadan, çünkü zaten
//!   uyanık olan callback'in kendisi karar veriyor: yerleşmemiş bir animasyon
//!   varken `needs_update` uyumayı reddediyor.
//!
//! İkincisi `Waker`'a **dokunmamak zorunda**: [`Waker::wake`] hasar bayrağını
//! koşulsuz dikiyor, yani oradan istenen bir hareket karesi kendini "içerik"
//! diye saydırır, grid'i boşuna yeniden taratır ve boşta sıfır kare kapısının
//! operandını (`icerik=`) şişirirdi. Sözleşmenin sonucu: **zamana bağlı kare
//! talebinin tek yolu hareket saatidir.** Yeni bir animasyon (blink, yumuşak
//! kaydırma) buraya girer, `Waker`'a değil.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bt_core::{Cursor, CursorMotion, DirtyFlag, Session, Theme};
use dispatch2::{DispatchQueue, MainThreadBound};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::{NSObject, NSObjectProtocol, NSRunLoop, NSRunLoopCommonModes};
// Yalnız tamamlanma bloğunun GPU damgaları için: `GPUStartTime`/`GPUEndTime`
// `MTLCommandBuffer` protokolünde ve trait kapsamda olmadan çağrılamaz.
use objc2_metal::MTLCommandBuffer;
use objc2_quartz_core::{
    CACurrentMediaTime, CAMetalDisplayLink, CAMetalDisplayLinkDelegate, CAMetalDisplayLinkUpdate,
};

use crate::frame::Frame;
use crate::motion::Motion;
use crate::renderer::{CellMetrics, Completion};
use crate::stats::Stats;
use crate::{GpuError, Renderer, Surface};

/// **Hasardan kare istemenin tek tanımı**: hasar bayrağını dik, link'i aç.
///
/// Her thread'den çağrılabilir; `Clone`, `Send + Sync`. İkisini ayrı ayrı
/// yapan ikinci bir yol bilerek yok — bayraksız açılan link "hasar yok" deyip
/// anında geri uyur, uyandırılmayan bayrak da kimseyi çizmeye çağırmaz.
///
/// **Zamana bağlı kare buradan istenmez** (modül başlığı): hareket, uyanık
/// callback'in kendi kararı. Buraya bağlanan bir animasyon her karesine hasar
/// diker ve `icerik=` sayacını — yani boşta sıfır kare kapısını — kendi
/// karelerinden doldururdu.
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
    /// **Ne saymıyor: hareket karesini.** Animasyon `Waker`'a hiç dokunmuyor
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

    fn gate(&self) -> &Gate {
        &self.inner.gate
    }

    /// Şimdiye kadarki kare talebi sayısı.
    fn requests(&self) -> u64 {
        self.inner.requests.load(Ordering::Relaxed)
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
    /// Demet değil `CellMetrics`: ölçü `Renderer::cell_metrics`'ten
    /// `bt-shell` üzerinden buraya tip olarak geliyor ve **saklanırken de**
    /// tip kalıyor. Saklanan bu değer yalnız `Frame::clear`'a girerken
    /// demete iniyor, çünkü kare kurucusu `#[repr(C)]` tarafına sayı yazıyor.
    /// (`resize`'ın `Session::resize`'a geçirdiği demet başka bir değer:
    /// oraya **gelen** ölçü gider, saklanan değil — kabul edilmeyen bir
    /// boyut buraya hiç yazılmaz.)
    cell: Cell<CellMetrics>,
    /// **İçerik** karesi: `session.frame()` hasar buldu ve kare çizilmeye
    /// karar verildi. Boşta sıfır kare kapısının operandı bu.
    ///
    /// `kare`'den (GPU'nun hatasız bitirdiği kare) ayrı bir sayı ve ayrılığın
    /// sebebi sonraki phase: **hareket** karesi de çizilen bir karedir, yani
    /// `kare`'yi artırır, ama grid kirli değildir — 200 ms'lik bir imleç
    /// kayması 120 Hz'de ~24 kare eder ve `kare ≤ IDLE_FRAME_LIMIT` kapısı kod
    /// doğruyken kırmızı düşerdi. Kapı bu yüzden "boştaki **içerik** karesi"ne
    /// bağlanıyor; sınırın sayısı değil **operandı** değişti.
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
    /// İmlecin kayması — kareyi zamana bağlayan tek şey.
    ///
    /// `Cell`, `RefCell` değil: [`crate::motion::Motion`] `Copy` ve ona
    /// dokunan tek yer bu callback (ana thread). `RefCell` çalışırdı ama
    /// `frame` ödüncünün yanında ikinci bir çalışma-zamanı ödüncü demek
    /// olurdu ve kazandırdığı hiçbir şey yok.
    motion: Cell<Motion>,
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
    /// Son içerik karesinin imleci — hareket karesinin `visible` ve `text`
    /// kaynağı.
    ///
    /// Konumu **kullanılmıyor**: kayan imlecin yeri `Motion`'da, burası yalnız
    /// `bt-core`'un konuma bağlı olmayan iki kararını taşıyor. İkisi `Motion`'a
    /// kopyalanmadı çünkü orası saf bir fizik modülü; renk ve görünürlük
    /// terminal semantiği (`CLAUDE.md` → renderer'a terminal semantiği
    /// eklenmez).
    ///
    /// `None` → henüz hiç içerik karesi çizilmedi; o hâlde hareket de yok.
    last_cursor: Cell<Option<Cursor>>,
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
            if !iv.session.take_damage() {
                // Hasar yok. İki ihtimal kaldı ve ikisi de burada bitiyor.
                motion.advance(dt);
                if motion.settled() {
                    // Boşta sıfır kare: yeni içerik de yerleşmemiş animasyon
                    // da yok, link uyur. Sıradaki `Wakeup` onu `Waker`
                    // üzerinden geri açar.
                    //
                    // Örnek de **yazılmıyor** ve bu bir dal değil, yolun
                    // şekli: bu karede `draw` hiç koşmadı, "encode = 0 ns"
                    // diye sahte bir örnek p95'i aşağı çekerdi.
                    iv.motion.set(motion);
                    link.setPaused(true);
                    return;
                }
                // **Hareket karesi.** `Waker`'a dokunulmuyor (modül başlığı):
                // link zaten uyanık ve bu callback'in kendisi onu sürdürüyor.
                iv.motion.set(motion);
                iv.motion_frames.set(iv.motion_frames.get() + 1);
                let theme = iv.theme.get();
                // Liste korunuyor, yalnız imleç taşınıyor: grid kirli değil,
                // yani glyph ve kural listeleri hâlâ geçerli. `Term` kilidine
                // saniyede 120 kez girmek "render yolu bloklanmaz" ile tam
                // burada kavga ederdi.
                if let (Some(at), Some(cursor)) = (motion.position(), iv.last_cursor.get()) {
                    frame.move_cursor(cursor, at, theme.accent_linear(), motion.alpha());
                }
                // CPU örneği **yazılmıyor** ve bu bir eksiklik değil:
                // `cpu_kare` `session.frame`'in kilit beklemesini ölçüyor ve
                // bu karede o iş hiç yok. Bir `truncate` + `push_cursor`'un
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
                    // başladığı anı değil.
                    Ok(()) => iv.last_frame_at.set(Some(now)),
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
                        }
                    }
                }
                return;
            }
            frame.clear(iv.cell.get().cell_px());
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
            let cursor = iv.session.frame(|cell| frame.push(cell));
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
            iv.last_cursor.set(Some(cursor));
            // Sıra zorunlu: önce geçen süre eski hedefe işlenir, sonra yeni
            // hedef kurulur. Ters sırada `dt` yeni hedefe uygulanır ve imleç
            // bir kare boyunca gitmediği bir yöne doğru hızlanırdı.
            motion.advance(dt);
            motion.sync(
                cursor.col,
                cursor.row,
                cursor.visible,
                cursor.display_offset,
                // Geometri bayrağı burada **tüketiliyor**: tüketilmeseydi
                // bir pencere sürüklemesinden sonraki her kare snap'lerdi.
                iv.geometry_changed.replace(false),
            );
            iv.motion.set(motion);
            if let Some(at) = motion.position() {
                frame.push_cursor(cursor, at, theme.accent_linear(), motion.alpha());
            }
            // Birinci aralık burada kapanıyor — `push_cursor`'dan **sonra**:
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
        // Açılış teması: ilk içerik karesi onu zaten tazeleyecek, ama alanın
        // `Option` olması için bir sebep yok — oturumun teması her an geçerli
        // bir cevap.
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
                cell: Cell::new(cell),
                content_frames: Cell::new(0),
                motion_frames: Cell::new(0),
                motion: Cell::new(Motion::default()),
                geometry_changed: Cell::new(false),
                last_frame_at: Cell::new(None),
                last_update_at: Cell::new(None),
                // İlk içerik karesine kadar kullanılmıyor: hareket karesi
                // ancak `Motion`'da bir konum varsa çiziyor ve orayı dolduran
                // tek yer içerik karesi — o da temayı tazeliyor.
                theme: Cell::new(theme),
                last_cursor: Cell::new(None),
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

    /// Yerleşmemiş animasyon yüzünden çizilen kare — `hareket=` jetonu.
    ///
    /// Duman kapısının **gerekli** sayacı: reçetede bir imleç hareketi var
    /// (`bt_core::smoke_shell`), yani sıfır "animasyon hiç koşmadı" demek.
    /// `icerik=`'e girmiyor ve bu kapının kendisi (008 Karar 2).
    pub fn motion_frames(&self) -> u64 {
        self.delegate.ivars().motion_frames.get()
    }

    /// Animasyon durdu mu — kapının **ölçüm istemeyen** yarısı.
    ///
    /// Süreli koşu bunu deadline'da bir kez soruyor: `false` ise koşu kırmızı
    /// (`Verdict::MotionUnsettled`). Hızdan bağımsız olması bütün değeri —
    /// `IDLE_FRAME_LIMIT` ancak yeterince hızlı bir sızıntıyı görüyor, bu
    /// soru ise durma koşulu unutulmuş **her** animasyonu görüyor, ne kadar
    /// yavaş olursa olsun.
    ///
    /// Gördüğünün sınırı: yalnız [`crate::motion`]'dan geçen animasyonlar.
    /// Altyapıyı atlayıp kendi kendine kare isteyen bir yolu bu soru göremez;
    /// onun kapısı [`Self::quiet_since`]'ın ölçülmüş eşiği.
    pub fn motion_settled(&self) -> bool {
        self.delegate.ivars().motion.get().settled()
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
        let finished = motion.set_style(style);
        iv.motion.set(motion);
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
        let finished = motion.set_reduce(reduce);
        iv.motion.set(motion);
        if finished {
            self.request_frame();
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
    /// **İmleç bu karede snap'ler.** Geometri değişiminde imleç hareket
    /// etmedi, altındaki ızgara hareket etti (008 Karar 5) — animasyon onu
    /// olmadığı bir yerden geliyormuş gibi gösterirdi. Bayrak koşulsuz
    /// dikiliyor, `Session::resize`'ın kabulüne bağlı değil: hücre ölçüsü
    /// değişmese de pencere oynamış olabilir.
    pub fn resize(&self, cols: u16, rows: u16, cell: CellMetrics) {
        let iv = self.delegate.ivars();
        if iv.session.resize(cols, rows, cell.cell_px()) {
            iv.cell.set(cell);
        }
        iv.geometry_changed.set(true);
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
