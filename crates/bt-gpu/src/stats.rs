//! Ölçüm örnekleri: kare yolunun iki CPU aralığı, GPU deltası ve açılış
//! damgası. **Hiçbir şey basmaz** — biriktirir; raporu `bt-shell` yazar.
//!
//! Kapı `BT_FRAME_STATS`: kapalıyken bu modülden hiçbir şey doğmaz (ne halka
//! ayrılır ne saat okunur). Açıkken bedel **örnek başına** bir atomik sayaç
//! ile bir `store` (kare başına üç sütun, yani üçer), üstüne üç saat okuması
//! ve bir `OnceLock` bakışı — dosya yok, kilit yok, ayırma yok (R4.3).
//! Ölçülemeyen kare de aynı mertebede: tek bir `fetch_add` ([`Ring::reject`]).
//!
//! İstatistik (p95 ve en kötü) **kapanışta** ve süreç içinde hesaplanıyor
//! ([`Samples::p95_and_worst`]); dosya yazılmıyor (R5).

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Halka kapasitesinin saniye başına çarpanı — bir **ayırma tavanı**, ölçülmüş
/// bir tazeleme hızı değil. ProMotion'ın üst ucu; daha hızlı bir ekran halkayı
/// doldurur, düşen örnek de sayılır (bkz. [`Samples::dropped`]).
const MAX_REFRESH_HZ: u64 = 120;

/// Kapasitenin mutlak tavanı (~10 dakika @120 Hz). Üç sütun × 8 bayt ile
/// ~1,7 MB: uzun bir koşunun bellekte açacağı deliği sınırlar. Aşan koşuda
/// halka dolar, en eskisi düşer ve **sayılır**.
const MAX_CAPACITY: u64 = MAX_REFRESH_HZ * 600;

/// p95'in anlamlı olabildiği en küçük örnek sayısı — **türetildi, seçilmedi**.
///
/// p95 en yakın-sıra (nearest-rank) yöntemiyle sıralı dizinin
/// `ceil(0.95 × n)`'inci elemanıdır. `n < 20` için o eleman **sonuncudur**,
/// yani p95 ile en kötü aynı sayıya çöker: rapor iki jeton basar, ikisi de
/// aynı sayıyı taşır ve biri "dağılım" diye okunur. Taban bu çökmenin
/// bittiği ilk `n`'dir (`ceil(0.95 × 20) = 19 < 20`).
///
/// R5.6'nın kapısı bu: taban altında sayı **hesaplanmaz**
/// ([`Samples::p95_and_worst`] `None` döner). Bunun raporda nasıl söylendiği
/// bu katmanın işi **değil** — jetonu basan üst katman biliyor; buraya bir
/// jeton yazımı kopyalamak hem katman yönüne aykırı olurdu hem de bir sonraki
/// yeniden adlandırmada sessizce bayatlardı (bir kez bayatladı).
pub const MIN_SAMPLES: usize = 20;

/// Bir sütunun kapanıştaki hâli: halkada duran örnekler (eskiden yeniye,
/// nanosaniye), sığmadığı için düşenler ve hiç yazılmadan elenenler.
///
/// İki sayaç da süs değil: p95'i az örnek üstünden hesaplayan bir rapor *iyi*
/// görünür (R5.2) ve toplanan sayı kadar **toplanamayan** da okunabilir
/// olmalı. İkisi ayrı, çünkü ayrı arıza:
///
/// - `dropped` — örnek üretildi ama halkaya sığmadı (koşu tazeleme
///   tavanından hızlı). Kapasitenin küçük kaldığını söyler. **Sütun
///   başınadır**: tek bir sayı isteyen rapor, elindeki üç anlık görüntünün
///   `dropped`'larının en büyüğünü alır — taze bir okuma **değil**, çünkü
///   imleçler kapanışta hâlâ oynayabilir (uçuşta kalan tamamlanma bloğu) ve
///   iki ayrı okuma jetonları birbiriyle çelişir hâle getirirdi. Bugün üç
///   sütunun kapasitesi eşit ve GPU CPU'dan fazla yazamıyor, yani en büyük
///   olan CPU'nunki; `max` o değişmez bozulsa da doğru kalır.
/// - `rejected` — örnek hiç üretilemedi: [`Stats::record_gpu`] Metal'in
///   sıfır/NaN damgasını eliyor. Bu sayaç olmadan boş bir GPU sütunu
///   "donanım damga vermiyor" ile "hiç kare çizilmedi"den **ayırt
///   edilemezdi**.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Samples {
    pub nanos: Vec<u64>,
    pub dropped: u64,
    pub rejected: u64,
}

impl Samples {
    /// p95 ve en kötü değer. **Ortalama yok**: bir takılma ortalamayı
    /// oynatmaz ama kullanıcı onu görür (`/measure` → "dağılım, ortalama
    /// değil").
    ///
    /// `None` → örnek sayısı [`MIN_SAMPLES`]'ın altında; o aralıkta iki sayı
    /// da aynı elemandan gelirdi (R5.6). Boş sütun bunun uç hâli.
    ///
    /// **Kendini tüketiyor:** sıralama yerinde yapılıyor ve kapanışta bu
    /// vektörün başka okuyucusu yok. Sayaç yarısını (`nanos.len()`,
    /// [`Samples::dropped`], [`Samples::rejected`]) isteyen çağıran onları
    /// bu çağrıdan **önce** okur; tip sırayı böyle zorluyor.
    ///
    /// Kapanışta bir kez koşuyor, kare yolunda değil: sıralamanın `n log n`'i
    /// bu yüzden meşru.
    pub fn p95_and_worst(mut self) -> Option<(Duration, Duration)> {
        if self.nanos.len() < MIN_SAMPLES {
            return None;
        }
        // Halka eskiden yeniye veriyor; sıralamayan bir uygulama p95 yerine
        // "sondan %5'inci karenin süresi"ni basardı.
        self.nanos.sort_unstable();
        // `ceil(0.95 × n)` tam sayıda: `(95 × n).div_ceil(100)`. Taban
        // yüzünden `1 ≤ rank < n`, yani ne indeks taşar ne de p95 en kötüye
        // çöker. Çarpım da taşmaz: `n` en çok [`MAX_CAPACITY`].
        let rank = (95 * self.nanos.len()).div_ceil(100);
        let p95 = self.nanos[rank - 1];
        // Sıralı dizinin sonu; uzunluk ≥ [`MIN_SAMPLES`] olduğu için var.
        let worst = self.nanos[self.nanos.len() - 1];
        Some((Duration::from_nanos(p95), Duration::from_nanos(worst)))
    }
}

/// Tek sütun: önceden ayrılmış halka ve atomik imleç.
///
/// Kilitsiz, çünkü sütunları iki ayrı thread yazıyor (ana thread'den CPU
/// aralıkları, Metal'in tamamlanma bloğundan GPU deltası). Bloğa bir kilit
/// koymak, bugün orada yalnız atomik olan **başarı yolunun ilk kilidi** olurdu.
///
/// `align(128)`: üç sütunun imleci tek bir önbellek satırına düşerse ana
/// thread ile Metal'in thread'i kare başına o satırı birbirinden çeker (false
/// sharing) — ölçüm aracı ölçtüğü şeyi bozar. Sayı **ölçüldü, seçilmedi**:
/// bu makinede `sysctl hw.cachelinesize` = **128** (Apple Silicon), yani
/// klasik `64` iki komşu `Ring`'i aynı satırda bırakabilirdi. 128, Intel'in
/// 64'ünü de kapsıyor.
#[repr(align(128))]
struct Ring {
    /// Kapasite ≥ 1 (kurucu kırpıyor); imleç bununla modlanıyor.
    slots: Box<[AtomicU64]>,
    /// Toplam yazma sayısı — halkaya sığan değil, **yazılan**. Düşen örnek
    /// bunun kapasiteyi aşan kısmıdır.
    pushed: AtomicU64,
    /// Ölçülemediği için hiç yazılmayan örnek (bkz. [`Samples::rejected`]).
    rejected: AtomicU64,
}

impl Ring {
    /// Kapasite bir kez ayrılır; kare başına ayırma yok (R4.3). Kırpma
    /// **yalnız burada**: `1`'in altı `%`'yi sıfır bölene düşürür, tavanın
    /// üstü de bellekte gereksiz bir delik açar.
    fn new(capacity: u64) -> Self {
        let capacity = capacity.clamp(1, MAX_CAPACITY);
        Self {
            slots: (0..capacity).map(|_| AtomicU64::new(0)).collect(),
            pushed: AtomicU64::new(0),
            rejected: AtomicU64::new(0),
        }
    }

    /// Kare başına bedelin tamamı: bir `fetch_add`, bir `store`.
    ///
    /// İkisi de `Relaxed`, çünkü burada sıralanacak bir şey yok: bileti
    /// benzersiz kılan RMW'nin **atomikliği**, sıralaması değil; yuva da
    /// `Relaxed` okunuyor. `AcqRel` hiçbir şeyi eşleştirmeyen bir bariyeri
    /// kare başına üç kez ödetirdi.
    fn push(&self, nanos: u64) {
        let ticket = self.pushed.fetch_add(1, Ordering::Relaxed);
        self.slot(ticket).store(nanos, Ordering::Relaxed);
    }

    /// Halkaya sığmayıp düşen örnek sayısı — imleci **verilmiş** bir okumadan.
    ///
    /// `pushed`'ı kendisi okumuyor ve bu bir titizlik değil: [`Ring::snapshot`]
    /// aynı imleci hem düşeni hem tutulan aralığı türetmek için kullanıyor.
    /// İki ayrı `load` olsaydı arada yazan bir kare (kapanışta uçuşta kalan
    /// tamamlanma bloğu) ikisini ayrıştırır ve `dropped > pushed` çıkabilirdi:
    /// aralık boşalır, rapor "hiç örnek yok ama düşen var" derdi.
    fn dropped_at(&self, pushed: u64) -> u64 {
        pushed.saturating_sub(self.slots.len() as u64)
    }

    /// Ölçülemeyen kare. Halkaya dokunmuyor, yalnız sayılıyor: bedeli
    /// [`Ring::push`]'un yarısı (tek bir `fetch_add`) ve `Relaxed` olma
    /// sebebi de aynı — burada sıralanacak bir şey yok.
    fn reject(&self) {
        self.rejected.fetch_add(1, Ordering::Relaxed);
    }

    /// Bilet → yuva. Mod sonucu tanımı gereği `slots.len()`'in altında, yani
    /// daralma yok ve "olamaz" diye bir geri düşüş de yok.
    fn slot(&self, ticket: u64) -> &AtomicU64 {
        &self.slots[(ticket % self.slots.len() as u64) as usize]
    }

    /// Kapanışta okunur, kare yolunda değil — `Vec` ayırması bu yüzden meşru.
    ///
    /// **Yazma sürerken okunabilir** ve arada iki pencere var: bileti almış
    /// ama değerini henüz yazmamış bir yazan (`fetch_add` ile `store` arası),
    /// ve halka dolmuşsa üstüne yazılmakta olan en eski yuva. İkisi de aynı
    /// belirtiyi verir — yuva ya **hiç yazılmamış** (sıfır) ya da bayat. Sıfır
    /// eleniyor: gerçek bir örnek sıfır olamaz ([`Stats::record_gpu`] sıfırı
    /// zaten reddediyor, bir kare de sıfır nanosaniye sürmez). Kilit koymak
    /// yalnız atomik olan yazma yolunun ilk kilidini doğururdu; kapanışta
    /// (`link.stop()` sonrası) uçuşta kalan kare zaten bir iki tane.
    fn snapshot(&self) -> Samples {
        // İmleç **bir kez** okunuyor ve iki türev de ondan çıkıyor; gerekçesi
        // [`Ring::dropped_at`]'te.
        let pushed = self.pushed.load(Ordering::Relaxed);
        // Düşen örnek sayısı, en eski **tutulan** örneğin bilet numarasıyla
        // aynı sayıdır: ondan öncekiler üstüne yazıldı.
        let dropped = self.dropped_at(pushed);
        Samples {
            nanos: (dropped..pushed)
                .map(|ticket| self.slot(ticket).load(Ordering::Relaxed))
                .filter(|&nanos| nanos != 0)
                .collect(),
            dropped,
            rejected: self.rejected.load(Ordering::Relaxed),
        }
    }
}

/// Koşunun ölçüm defteri. Kapı açıkken `bt-shell` kurar, `bt-gpu` doldurur ve
/// kapanışta yine `bt-shell` okur.
pub struct Stats {
    /// Açılış damgası: `main()`'in **ilk satırında** alındı ve buraya
    /// **taşındı**. Kurucunun kendisi okusaydı ölçüm
    /// `Renderer::system_default()`'tan sonra başlar, yani Metal device
    /// kurulumunu ve metallib yüklemesini kaçırırdı (R3.3).
    ///
    /// **Süreç başlangıcı değil:** dyld ve Rust runtime kurulumu bu damgadan
    /// önce bitiyor. `main()`'in ilk satırı elimizdeki en erken nokta ve
    /// damgayı oraya koyan taraf neden orada durduğunu kendi yanında
    /// anlatıyor.
    since: Instant,
    /// İlk tamamlanan kareye kadar geçen süre. `OnceLock`: bir kez yazılıyor
    /// ve yazan **Metal'in thread'i** — `ShellWake.waker` ile `Session`'ın
    /// göndereni aynı desen. Elle sıfır-sentinel + CAS aynı işi yapardı ama
    /// "sıfır = henüz yok" diye bir sözleşme uydurur, sonra da ölçülen sıfırı
    /// `max(1)` ile kaçırırdı.
    ///
    /// `Stats`'ın `Send + Sync` olması **şart** (gövdeyi Metal'in thread'i
    /// yazıyor) ve bunu derleyiciye soran yer `Renderer::completion`'ın
    /// `on_complete: … + Send + Sync` sınırı: ölçüm oraya bir `Arc<Stats>`
    /// olarak giriyor.
    startup: OnceLock<Duration>,
    cpu_frame: Ring,
    cpu_encode: Ring,
    gpu: Ring,
}

impl Stats {
    /// `since` açılış damgası, `run_seconds` koşunun bütçesi.
    ///
    /// Damgayı bu kurucu **almıyor**, dışarıdan alıyor; hem gerekçesi hem
    /// ne olmadığı `since` alanının doc'unda.
    ///
    /// Kapasite koşu süresinden türüyor: `run_seconds × MAX_REFRESH_HZ`.
    /// Sıfır saniyelik koşu `Ring::new`'in alt sınırına düşer (bir yuva) —
    /// kırpma tek yerde durur ve orada `%`'nin yanında durduğu için görünür.
    ///
    /// **Ayırma açılış süresinin içinde:** üç halka burada, damga alındıktan
    /// sonra ve ilk kare bitmeden doğuyor, yani [`Stats::startup`] kendi
    /// ölçüm aracının kurulumunu da sayıyor. Bedel
    /// `run_seconds × MAX_REFRESH_HZ × 3 × 8` bayt — türetme, ölçüm değil:
    /// üç saniyelik bir koşuda ~8,6 KB, `MAX_CAPACITY` tavanında ~1,7 MB.
    pub fn new(since: Instant, run_seconds: u64) -> Self {
        let capacity = run_seconds.saturating_mul(MAX_REFRESH_HZ);
        Self {
            since,
            startup: OnceLock::new(),
            cpu_frame: Ring::new(capacity),
            cpu_encode: Ring::new(capacity),
            gpu: Ring::new(capacity),
        }
    }

    /// Karenin iki CPU aralığı: `session.frame` ve `draw`.
    ///
    /// İkisi de `Duration`, `Option` değil: eksik damgalı bir kare
    /// (`session.frame` `None` döndü ya da `draw` düştü) buraya **hiç
    /// varmıyor** — çağrı yolunun şekli bunu tipe bağlıyor. `Option` alan bir
    /// imza o kuralı çalışma zamanına indirir ve "encode = 0 ns" sahte örneği
    /// p95'i aşağı çekerdi.
    pub(crate) fn record_cpu(&self, frame: Duration, encode: Duration) {
        // `as_nanos` `u128` döndürüyor. Taşma 584 yıllık bir kare demek, yani
        // olamaz; olursa da **doyurmuyoruz**: `u64::MAX`'lık bir örnek p95'i
        // ve en kötüyü tek başına sahiplenirdi. Ölçülemeyen kare, uç değerli
        // bir kare değil **yok** bir karedir — `record_gpu`'nun sıfır kuralı
        // ile aynı ilke. İki sütun birlikte yazılıyor ki hizaları bozulmasın.
        // Sıfır da eleniyor, taşma gibi: [`Ring::snapshot`] yazılmamış yuvayı
        // sıfırdan tanıyor, yani halkaya giren gerçek bir sıfır **sessizce**
        // kaybolurdu — ne `ornek=`'te ne `gpu_elenen=`'de görünürdü ve iki CPU
        // sütununun uzunluğu birbirinden ayrılırdı. Elenen sayılıyor.
        let (Ok(frame @ 1..), Ok(encode @ 1..)) = (
            u64::try_from(frame.as_nanos()),
            u64::try_from(encode.as_nanos()),
        ) else {
            // İki sütun birlikte eleniyor: hizaları bozulmasın diye birlikte
            // yazılıyorlar, o hâlde birlikte de sayılmalılar.
            self.cpu_frame.reject();
            self.cpu_encode.reject();
            return;
        };
        self.cpu_frame.push(frame);
        self.cpu_encode.push(encode);
    }

    /// Karenin GPU deltası; `start`/`end` Metal'in kendi saatinden (saniye).
    ///
    /// Metal ikisini de **sıfır** döndürebiliyor ("has not started" / "CPU has
    /// not received completion notification"): o karede ölçülmüş bir şey yok
    /// ve 0 ns'lik bir örnek p95'i aşağı çekerdi. Sıfır örnek yazmamak, GPU
    /// sütununun CPU sütunundan kısa kalması demek — `Samples` sütun başına
    /// okunuyor, tam da bu yüzden.
    pub(crate) fn record_gpu(&self, start: f64, end: f64) {
        // Kapı **olumlu** yazıldı ve `is_finite` şart: karşılaştırmanın
        // olumsuz hâli NaN'ı geçirirdi (`NaN <= 0.0` da `end <= NaN` da
        // `false`) ve `f64 → u64` dönüşümü NaN'ı sıfıra indirdiği için ortaya
        // tam da elemeye çalıştığımız **0 ns'lik sahte örnek** çıkardı.
        // Sonsuz da öyle: `u64::MAX`'a doyar ve p95'i tek başına sahiplenir.
        if !(start.is_finite() && end.is_finite() && start > 0.0 && end > start) {
            // Eleme **sayılıyor**: sayılmasaydı boş bir GPU sütunu "donanım
            // damga vermedi" ile "hiç kare çizilmedi"yi aynı görünüme
            // katlardı ve rapor bunu söyleyemezdi.
            self.gpu.reject();
            return;
        }
        // Kapı `f64` tarafındaydı; asıl örnek **dönüşümden sonra** doğuyor ve
        // iki uç orada hâlâ açıktı: bir nanosaniyenin altındaki delta sıfıra
        // kırpılıyor (snapshot onu yazılmamış yuva sanardı), sonlu ama saçma
        // büyük bir delta ise `as u64` ile `u64::MAX`'a **doyuyor** ve p95 ile
        // en kötüyü tek başına sahipleniyordu — tam da `is_finite` kapısının
        // önlemek için yazıldığı sonuç. Ölçülemeyen kare uç değerli değil
        // **yok** sayılır; ikisi de eleniyor ve sayılıyor.
        let nanos = (end - start) * 1e9;
        if !(nanos >= 1.0 && nanos < u64::MAX as f64) {
            self.gpu.reject();
            return;
        }
        self.gpu.push(nanos as u64);
    }

    /// İlk tamamlanan kare: açılış süresi burada kapanır, sonrakiler dokunmaz.
    /// Kare başına bedel `OnceLock`'un hızlı yolu: bir atomik okuma.
    pub(crate) fn mark_startup(&self) {
        self.startup.get_or_init(|| self.since.elapsed());
    }

    /// `since` damgasından ilk **tamamlanan** kareye. `None` → hiç kare
    /// bitmedi.
    ///
    /// İki ucu da dar ve ikisi de adıyla anılmalı: baş **süreç başlangıcı
    /// değil** (bkz. `since`), son da **sunulan** kare değil —
    /// `addCompletedHandler` GPU'nun komut tamponunu bitirdiğini söyler,
    /// ekrana çıktığını değil.
    pub fn startup(&self) -> Option<Duration> {
        self.startup.get().copied()
    }

    /// `session.frame` aralığı: kilit beklemesi + ayrıştırma + grid + sink.
    pub fn cpu_frame(&self) -> Samples {
        self.cpu_frame.snapshot()
    }

    /// `draw` aralığı: encode + commit.
    pub fn cpu_encode(&self) -> Samples {
        self.cpu_encode.snapshot()
    }

    /// GPU'nun komut tamponunu işlediği süre.
    pub fn gpu(&self) -> Samples {
        self.gpu.snapshot()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats() -> Stats {
        Stats::new(Instant::now(), 1)
    }

    fn column(nanos: Vec<u64>) -> Samples {
        // Sıfırlar `Default`'tan: `Samples`'a alan eklendiğinde bu fixture'ın
        // (ve aşağıdaki üç iddianın) haberdar edilmesi gerekmesin.
        Samples {
            nanos,
            ..Default::default()
        }
    }

    #[test]
    fn p95_returns_none_on_empty_ring() {
        // Hiç örnek yokken hesaplanacak bir dağılım da yok. Tabanın uç hâli
        // ama ayrı sınanıyor: boş sütun "ölçüm hiç koşmadı" demenin tek yolu
        // ve `unwrap` ile karşılansaydı rapor yolunda panik olurdu.
        let stats = stats();
        assert_eq!(stats.gpu().p95_and_worst(), None);
        assert_eq!(stats.cpu_frame().p95_and_worst(), None);
        assert_eq!(column(Vec::new()).p95_and_worst(), None);
    }

    #[test]
    fn few_samples_suppress_p95() {
        // Taban **türetildi, seçilmedi**: en yakın-sıra p95'in indeksi
        // `ceil(0.95 × n) − 1` ve `n < 20` için o indeks **son** elemandır,
        // yani p95 ile en kötü aynı sayı olur. Taban altında basılan bir p95,
        // en kötünün kopyasını "dağılım" diye sunardı (R5.6).
        let below = column((1..=MIN_SAMPLES as u64 - 1).collect());
        assert_eq!(below.p95_and_worst(), None, "taban altında sayı yok");

        // Tabanın **tam üstünde** ikisi ayrışıyor; ayrışmasaydı taban bir
        // sayıyı iki kez adlandırmaktan başka bir şey yapmazdı.
        let (p95, worst) = column((1..=MIN_SAMPLES as u64).collect())
            .p95_and_worst()
            .expect("tabanda sayı hesaplanır");
        assert_eq!(worst, Duration::from_nanos(MIN_SAMPLES as u64));
        assert_eq!(p95, Duration::from_nanos(MIN_SAMPLES as u64 - 1));
        assert!(p95 < worst, "p95 en kötünün kopyası değil");

        // Halka eskiden yeniye veriyor, sıralı değil: sıralamayı atlayan bir
        // uygulama burada kırmızı düşer.
        let reversed = column((1..=MIN_SAMPLES as u64).rev().collect());
        assert_eq!(reversed.p95_and_worst(), Some((p95, worst)));
    }

    #[test]
    fn rejected_gpu_samples_are_counted() {
        // `/code-review` bulgusu: elenen kare sayılmayınca boş bir GPU sütunu
        // "donanım sıfır döndürüyor" ile "hiç kare çizilmedi"den ayırt
        // edilemiyor — R5.2'nin kapatmak istediği körlüğün ta kendisi.
        let stats = stats();
        stats.record_gpu(0.0, 0.0);
        stats.record_gpu(f64::NAN, 1.0);
        stats.record_gpu(1.0, 1.002);
        let gpu = stats.gpu();
        assert_eq!(gpu.nanos, vec![2_000_000]);
        assert_eq!(gpu.rejected, 2, "elenen kare sayılır");
        // CPU sütunu bu koşuda hiç yazılmadı, yani elenen de yok. CPU'nun
        // **kendi** eleme yolu ulaşılmaz değil — sıfır uzunluklu aralık onu
        // ateşliyor ve `cpu_rejects_zero_spans` bunu pinliyor; ulaşılmaz olan
        // yalnız taşma kolu (584 yıllık bir kare demek).
        assert_eq!(stats.cpu_frame().rejected, 0);
    }

    #[test]
    fn cpu_sample_fills_both_columns() {
        // İki sütun ayrı ayrı doluyor ve **sırası karışmıyor**: ikisini aynı
        // halkaya iten ya da argümanları yer değiştiren bir düzenleme burada
        // kırmızı düşer. Ayrımın kendisi R3.1: tek aralık 002 #1'i (kilit
        // beklemesi) encode'dan ayıramaz.
        let stats = stats();
        stats.record_cpu(Duration::from_millis(3), Duration::from_micros(400));
        assert_eq!(stats.cpu_frame().nanos, vec![3_000_000]);
        assert_eq!(stats.cpu_encode().nanos, vec![400_000]);
        // Boş kare örnek yazmıyor ve bu **tipte**: eksik damgalı kare
        // `record_cpu`'ya hiç varmıyor (`link.rs`'in erken dönüşü), yani
        // sınanacak bir dal yok — sınama olsaydı hiçbir koşulda kırmızı
        // düşemezdi.
        assert!(
            stats.gpu().nanos.is_empty(),
            "CPU örneği GPU sütununa yazmaz"
        );
    }

    #[test]
    fn gpu_zero_timestamps_record_nothing() {
        // Metal "başlamadı" / "tamamlanma bildirimi gelmedi" hâllerinde sıfır
        // döndürüyor. Sıfırdan türeyen 0 ns'lik örnek p95'i aşağı çeker ve
        // rapor **iyi** görünür — R5.2'nin uyardığı körlüğün ta kendisi.
        let stats = stats();
        stats.record_gpu(0.0, 0.0);
        stats.record_gpu(0.0, 1.0);
        stats.record_gpu(2.0, 2.0);
        stats.record_gpu(3.0, 2.0);
        assert!(stats.gpu().nanos.is_empty(), "ölçülmemiş kare örnek yazmaz");

        stats.record_gpu(1.0, 1.002);
        assert_eq!(stats.gpu().nanos, vec![2_000_000]);
    }

    #[test]
    fn gpu_rejects_zero_and_saturating_deltas() {
        // `/code-review` bulgusu: kapı `f64` tarafındaydı, örnek ise
        // dönüşümden **sonra** doğuyor. Bir nanosaniyenin altındaki delta
        // sıfıra kırpılıp snapshot'ın "yazılmamış yuva" filtresine takılıyor
        // (hiçbir jetonda görünmeden kayboluyor); sonlu ama saçma büyük bir
        // delta `u64::MAX`'a doyup p95'i tek başına sahipleniyor.
        let stats = stats();
        // Yarım nanosaniye: pozitif, sonlu, `end > start` — eski kapıdan
        // geçerdi.
        stats.record_gpu(1.0, 1.0 + 0.5e-9);
        // Sonlu ama `u64`'e sığmayan: ~1,8e10 saniyenin üstü.
        stats.record_gpu(1.0, 1.0 + 2.0e10);
        assert!(
            stats.gpu().nanos.is_empty(),
            "sıfıra kırpılan ve doyan delta örnek yazmaz"
        );
        assert_eq!(stats.gpu().rejected, 2, "ikisi de sayılır");

        // Tam bir nanosaniye en küçük **meşru** örnek.
        stats.record_gpu(1.0, 1.0 + 1e-9);
        assert_eq!(stats.gpu().nanos, vec![1]);
    }

    #[test]
    fn cpu_rejects_zero_spans() {
        // Aynı körlüğün CPU tarafı: sıfır uzunlukta bir aralık halkaya
        // girseydi snapshot onu yazılmamış yuva sanıp atardı ve iki CPU
        // sütununun uzunluğu birbirinden ayrılırdı.
        let stats = stats();
        stats.record_cpu(Duration::ZERO, Duration::from_millis(1));
        stats.record_cpu(Duration::from_millis(1), Duration::ZERO);
        assert!(stats.cpu_frame().nanos.is_empty());
        assert!(stats.cpu_encode().nanos.is_empty());
        assert_eq!(stats.cpu_frame().rejected, 2);
        assert_eq!(
            stats.cpu_encode().rejected,
            2,
            "iki sütun birlikte yazılıyor, birlikte eleniyor"
        );
    }

    #[test]
    fn gpu_rejects_nan_and_infinity() {
        // `/code-review` bulgusu: kapı olumsuz yazılınca (`start <= 0.0 || end
        // <= start`) NaN **geçiyordu** — iki karşılaştırma da `false` döner —
        // ve `f64 → u64` NaN'ı sıfıra indirdiği için halkaya tam da elemeye
        // çalıştığımız 0 ns'lik örnek giriyordu. Sonsuz da `u64::MAX`'a doyup
        // p95'i tek başına sahipleniyordu.
        let stats = stats();
        for (start, end) in [
            (f64::NAN, 1.0),
            (1.0, f64::NAN),
            (f64::NAN, f64::NAN),
            (1.0, f64::INFINITY),
            (f64::NEG_INFINITY, 1.0),
        ] {
            stats.record_gpu(start, end);
        }
        assert!(
            stats.gpu().nanos.is_empty(),
            "ölçülemeyen kare uç değerli değil, YOK sayılır"
        );
    }

    #[test]
    fn full_ring_drops_oldest_and_counts() {
        // Kapasite aşımı **sessiz değil**: en eskisi düşer ve düşen sayılır.
        // Kapasite `run_seconds × MAX_REFRESH_HZ` olduğu için taşmayı burada
        // elle kuruyoruz.
        let ring = Ring::new(2);
        ring.push(1);
        ring.push(2);
        assert_eq!(
            ring.snapshot(),
            Samples {
                nanos: vec![1, 2],
                ..Default::default()
            }
        );

        ring.push(3);
        assert_eq!(
            ring.snapshot(),
            Samples {
                nanos: vec![2, 3],
                dropped: 1,
                ..Default::default()
            },
            "en eski örnek düşer ve sayılır"
        );

        ring.push(4);
        ring.push(5);
        assert_eq!(
            ring.snapshot(),
            Samples {
                nanos: vec![4, 5],
                dropped: 3,
                ..Default::default()
            }
        );
    }

    #[test]
    fn zero_second_run_still_has_a_ring() {
        // Sıfır saniyelik bir defter **binary'den artık doğmuyor**
        // (`main.rs` `BT_FRAME_STATS`'i sıfırdan büyük bir `BT_RUN_SECONDS`'a
        // bağlıyor, yoksa çıkış 1), ama kurucu `pub` ve kütüphane API'sinden
        // çağrılabilir: kapasitesiz bir halka imleci modlayamaz — sıfıra
        // bölme değil, panik (`slots[i % 0]`). Alt sınır o yüzden duruyor.
        let stats = Stats::new(Instant::now(), 0);
        stats.record_gpu(1.0, 1.001);
        assert_eq!(stats.gpu().nanos.len(), 1);
    }

    #[test]
    fn startup_stamp_precedes_renderer_setup() {
        // Asıl sıra **yapısal**: damga `main()`'in ilk satırında alınıyor ve
        // `Options` ile taşınıyor, yani `bt_shell::run` içindeki
        // `Renderer::system_default()` ondan sonra koşuyor. Bu sınama tipin
        // üstlendiği yarıyı pinler: `Stats` damgayı **alıyor**, kendisi
        // okumuyor — okusaydı açılışın en pahalı parçası ölçümün dışında
        // kalırdı ve sayı olduğundan iyi görünürdü.
        let since = Instant::now();
        // "Renderer kurulumu": kurucu damgayı kendisi alsaydı bu süre ölçümün
        // dışında kalırdı.
        std::thread::sleep(Duration::from_millis(2));
        let stats = Stats::new(since, 1);
        assert!(stats.startup().is_none(), "kare bitmeden açılış süresi yok");

        stats.mark_startup();
        let first = stats.startup().expect("ilk kare açılışı kapatır");
        assert!(
            first >= Duration::from_millis(2),
            "açılış süresi kurulumu kapsamalı: {first:?}"
        );

        std::thread::sleep(Duration::from_millis(2));
        stats.mark_startup();
        assert_eq!(
            stats.startup(),
            Some(first),
            "ilk kare kazanır, sonrası değil"
        );
    }
}
