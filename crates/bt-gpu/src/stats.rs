//! Ölçüm örnekleri: kare yolunun iki CPU aralığı, GPU deltası ve açılış
//! damgası. **Hiçbir şey basmaz** — biriktirir; raporu `bt-shell` yazar.
//!
//! Kapı `BT_FRAME_STATS`: kapalıyken bu modülden hiçbir şey doğmaz (ne halka
//! ayrılır ne saat okunur). Açıkken bedel **örnek başına** bir atomik sayaç
//! ile bir `store` (kare başına üç sütun, yani üçer), üstüne üç saat okuması
//! ve bir `OnceLock` bakışı — dosya yok, kilit yok, ayırma yok (R4.3).

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

/// Bir sütunun kapanıştaki hâli: halkada duran örnekler (eskiden yeniye,
/// nanosaniye) ve sığmadığı için düşenlerin sayısı.
///
/// `dropped` süs değil: p95'i az örnek üstünden hesaplayan bir rapor *iyi*
/// görünür (R5.2) ve düşen örnek o körlüğün ikinci yarısıdır — toplanan sayı
/// kadar **toplanamayan** da okunabilir olmalı.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Samples {
    pub nanos: Vec<u64>,
    pub dropped: u64,
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
        let pushed = self.pushed.load(Ordering::Relaxed);
        let kept = pushed.min(self.slots.len() as u64);
        // Düşen örnek sayısı, en eski **tutulan** örneğin bilet numarasıyla
        // aynı sayıdır: ondan öncekiler üstüne yazıldı.
        let dropped = pushed - kept;
        Samples {
            nanos: (dropped..pushed)
                .map(|ticket| self.slot(ticket).load(Ordering::Relaxed))
                .filter(|&nanos| nanos != 0)
                .collect(),
            dropped,
        }
    }
}

/// Koşunun ölçüm defteri. Kapı açıkken `bt-shell` kurar, `bt-gpu` doldurur ve
/// kapanışta yine `bt-shell` okur.
pub struct Stats {
    /// Açılış damgası: `main()`'in başında alındı ve buraya **taşındı**.
    /// Kurucunun kendisi okusaydı ölçüm `Renderer::system_default()`'tan
    /// sonra başlar, yani Metal device kurulumunu ve metallib yüklemesini
    /// kaçırırdı (R3.3).
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
    /// `since` süreç başındaki damga, `run_seconds` koşunun bütçesi.
    ///
    /// Kapasite koşu süresinden türüyor: `run_seconds × MAX_REFRESH_HZ`.
    /// Sıfır saniyelik koşu [`Ring::new`]'in alt sınırına düşer (bir yuva) —
    /// kırpma tek yerde durur ve orada `%`'nin yanında durduğu için görünür.
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
        let (Ok(frame), Ok(encode)) = (
            u64::try_from(frame.as_nanos()),
            u64::try_from(encode.as_nanos()),
        ) else {
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
            return;
        }
        self.gpu.push(((end - start) * 1e9) as u64);
    }

    /// İlk tamamlanan kare: açılış süresi burada kapanır, sonrakiler dokunmaz.
    /// Kare başına bedel `OnceLock`'un hızlı yolu: bir atomik okuma.
    pub(crate) fn mark_startup(&self) {
        self.startup.get_or_init(|| self.since.elapsed());
    }

    /// Süreç başından ilk tamamlanan kareye. `None` → hiç kare bitmedi.
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
                dropped: 0
            }
        );

        ring.push(3);
        assert_eq!(
            ring.snapshot(),
            Samples {
                nanos: vec![2, 3],
                dropped: 1
            },
            "en eski örnek düşer ve sayılır"
        );

        ring.push(4);
        ring.push(5);
        assert_eq!(
            ring.snapshot(),
            Samples {
                nanos: vec![4, 5],
                dropped: 3
            }
        );
    }

    #[test]
    fn zero_second_run_still_has_a_ring() {
        // `BT_RUN_SECONDS=0` meşru (deadline hemen ateşler) ve kapasitesiz bir
        // halka imleci modlayamaz — sıfıra bölme değil, panik: `slots[i % 0]`.
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
