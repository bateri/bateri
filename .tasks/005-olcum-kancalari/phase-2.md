# Phase 2 — Zaman yakalama: iki CPU aralığı, bir GPU deltası

## Özet

`BT_FRAME_STATS` kapısı, `Options`'a tipli alanlar, `session.frame()` ve
`draw` çevresinde iki ayrı CPU damgası, tamamlanma bloğundan GPU deltası ve
açılış damgası. Örnekler bellekte birikir; **hiçbir şey basılmaz** — rapor
phase-3'ün işi.

_Requirements: R3, R3.1, R3.2, R3.3, R4, R4.1, R4.2, R4.3_

---

## 1. Kapı ve taşıma — `BT_RUN_SECONDS` deseninin birebir aynısı

`crates/bateri/src/main.rs`, `crates/bt-shell/src/lib.rs`

Açılış damgası **`Renderer::system_default()`'tan önce** alınmalı
(`lib.rs:39`). Sonra alınırsa ölçtüğümüz şey açılışın kendisini kaçırır:
Metal device kurulumu ve metallib yüklemesi açılışın en pahalı parçası.

Bu yüzden `Options` bool değil **`Instant`** taşır:

```rust
pub struct Options {
    pub run_seconds: Option<u64>,
    pub workload: Option<Workload>,
    /// `BT_FRAME_STATS`: `Some` ise ölçüm açık ve damga **süreç başında**
    /// alınmış. Bool olsaydı damgayı `run` içinde almak gerekirdi — yani
    /// `Renderer::system_default()`'tan sonra, açılışın en pahalı parçasını
    /// kaçırarak.
    pub stats_since: Option<Instant>,
}
```

```rust
// main.rs — env TEK YERDE okunur (R4.2). Damga `bt_shell::run`'dan önce.
let stats_since = std::env::var_os("BT_FRAME_STATS").is_some().then(Instant::now);
```

> **Dürüst sınır, `## Yöntem`'e geçecek:** `Instant::now()` `main()`'in
> başında alınıyor, yani ölçülen şey **süreç başlangıcı değil** — dyld ve
> Rust runtime kurulumu bu damgadan önce bitmiş oluyor. Elimizdeki en erken
> nokta bu; sayı "main'den ilk tamamlanan kareye" demek ve `/measure`'ın
> "process başlangıcından" tarifinden bu kadar sapıyor. Yazılmazsa sonraki
> okuyucu sayıyı olduğundan iyi okur.

---

## 2. CPU: **iki** aralık, bir değil

`crates/bt-gpu/src/link.rs` → `needs_update`

Tek aralık 002 #1'i ayıramaz. O iddia `Session::frame()`'in
`FairMutex::lock()` beklemesiyle ilgili ve o bekleme `session.frame`'in
**içinde**; `draw`'ın değil. Tek "frame()+draw" aralığı ikisini toplayıp
ayrımı yok eder.

```rust
// Kapı kapalıyken tek bir `Instant::now()` bile çağrılmaz (R4.1): yakalanmış
// `Option` üstünde dallanma bedava, ama saat okuma değil.
let t0 = stats.is_some().then(Instant::now);

let produced = session.frame(sink);       // kilit + parse + grid
let t1 = t0.map(|_| Instant::now());

// ... `None` → setPaused(true), mevcut yol aynen ...

renderer.draw(&frame)?;                   // encode + commit
let t2 = t1.map(|_| Instant::now());

if let (Some(stats), Some(t0), Some(t1), Some(t2)) = (stats, t0, t1, t2) {
    stats.push_cpu(t1 - t0, t2 - t1);     // halkaya yaz, I/O yok
}
```

> **Dikkat: erken dönüş.** `session.frame` `None` dönünce `setPaused(true)`
> yolu `draw`'a hiç varmıyor (`link.rs:279-281`). O karede `t2` yok ve örnek
> **yazılmaz** — yoksa "encode süresi 0" diye sahte bir örnek girer ve p95'i
> aşağı çeker.

---

## 3. GPU: paylaşılan slot, closure değil

`crates/bt-gpu/src/renderer.rs`

Tamamlanma bloğu kare başına **kurulmuyor** ve doc'u bunu açıkça yasaklıyor
(`renderer.rs:318-323`): *"her kurulum bir heap ayırması ve birkaç `Arc` sayaç
hareketi demek — hepsi tazeleme hızında. Metal `Block_copy` ile kendi
referansını aldığı için aynı blok her komut tamponuna eklenebilir."*

Sonucu net: **o karenin CPU damgasını closure ile yakalayamazsın.** Blok tek
ve paylaşılmış; yakaladığı şey bütün karelerin ortak durumu olmak zorunda.

```rust
// Blok `frames` sayacını nasıl taşıyorsa örnek halkasını da öyle taşır:
// `Arc` ile, kurulumda bir kez. İçeride yalnız GPUStartTime/EndTime okunur —
// dosya yok, kilit yok, ayırma yok (R4.3).
let samples = self.samples.clone();
// ... completion bloğunun içinde:
if let Some(s) = samples.as_ref() {
    // `GPUEndTime - GPUStartTime`: Metal'in kendi saati. CPU damgasıyla
    // ilişkilendirilmiyor — hangi karenin olduğu değil, DAĞILIM soruluyor.
    s.push_gpu(cmd.GPUEndTime() - cmd.GPUStartTime());
}
```

> **Neden kare eşleştirmesi yok:** CPU örneğiyle GPU örneğini aynı kareye
> bağlamak bir sıra numarası ve kare-arası defter tutmak isterdi. Bekleyen
> sekiz iddianın hiçbiri "şu kare" sorusunu sormuyor; hepsi dağılım sorusu.
> Eşleştirme, iddia doğduğunda gelir.

---

## 4. Örnek halkası — önceden ayrılmış, kare başına ayırma yok

`crates/bt-gpu/src/` (yeni modül ya da `renderer.rs` içinde)

```rust
/// Ölçüm örnekleri. Kapı açıkken kurulur, kapalıyken **hiç var olmaz**.
///
/// Metal'in thread'inden (tamamlanma bloğu) ve ana thread'den (display link)
/// birlikte yazılıyor, o yüzden kilitsiz: her sütun kendi önceden ayrılmış
/// halkası ve atomik imleci. Tamamlanma bloğuna kilit koymak, bugün orada
/// yalnız atomik olan **başarı yolunun ilk kilidi** olurdu.
pub(crate) struct Samples { /* üç halka: cpu_frame, cpu_encode, gpu */ }
```

Kapasite koşunun başında `run_seconds × tazeleme hızı` üst sınırından
ayrılır; dolarsa **en eskisi düşer ve düşen sayılır** (phase-3'ün `ornek=`
jetonu bunu görünür kılar).

---

## Uygulama Notları

- **`Options` tek `Option`'a indi ve `stats_since` onun *içine* girdi**
  (kılavuzun §1'i onu kardeş alan gösteriyordu). Şekil:
  `Options { run: Option<Run> }`, `Run { seconds, workload, stats_since }`.
  Gerekçe devir (2)'nin kendisi: üç alan da **süreye** bağlı — süresiz yük hiç
  bitmez, süresiz ölçüm de hiç raporlanmaz, çünkü rapor `report_and_exit`'te
  ve oraya yalnız deadline varır. Kardeş alan bırakılsaydı `BT_FRAME_STATS=1`
  tek başına örnek toplayıp hiç basmayan, yani **bedeli olan ve çıktısı
  olmayan** bir yol açardı; `main.rs` bunu `BT_SCROLL_TEST`'in eşi bir kolla
  eliyor ("sıfırdan büyük bir BT_RUN_SECONDS ister", çıkış 1). Kazanç
  kılavuzun saydığı yerlerde: üç `unwrap_or(0)`, iki `| None` kolu, bir
  `debug_assert` ve phase-1'in L3 notu (`verdict`'in ulaşılmaz `None` kolu)
  düştü — `verdict` artık `Workload` alıyor, `Option<Workload>` değil.

- **`Stats` `bt-gpu`'da, sahibi `bt-shell`.** Kılavuz gövdeyi `Renderer`'ın
  bir alanı gibi yazıyordu (`self.samples.clone()`); o, `Renderer::system_default`'ı
  genişletmeyi gerektirirdi — oysa renderer `Options` görmeden, `run()`'ın ilk
  satırında doğuyor. Yerine: `bt-shell` `Arc<Stats>`'ı kuruyor (kapı açıkken,
  yani halka **yalnız o zaman** ayrılıyor), `DisplayLink::new`'e ve oradan
  `Renderer::completion`'a kopya geçiyor. R3.2'nin özü korundu: blok kare
  başına kurulmuyor, örnek gövdesi kurulumda bir kez `Arc` ile giriyor.
  Kapanışta okuyan da `bt-shell`'in kendi kopyası — phase-3 raporu oradan
  yazacak.

- **Tip adı `Samples` değil `Stats`**; `Samples` bir sütunun **kapanıştaki
  hâli** oldu (`{ nanos, dropped }`). Üç halka + açılış damgası tek gövdede
  yaşıyor ve o gövdenin adı "örnekler" olsaydı damga orada yabancı dururdu.
  Halkanın kendisi `Ring` ve `bt-gpu`'ya private.

- **GPU sıfırı eleniyor** (kılavuzda yoktu, Apple'ın sözleşmesinden çıktı):
  `GPUStartTime`/`GPUEndTime` "başlamadı" / "tamamlanma bildirimi gelmedi"
  hâllerinde **sıfır** dönüyor ve 0 ns'lik bir örnek p95'i aşağı çeker —
  R5.2'nin uyardığı körlüğün ta kendisi. `record_gpu` `start <= 0 || end <=
  start` olan kareyi yazmıyor, dolayısıyla **GPU sütunu CPU sütunundan kısa
  kalabilir**; sütun başına `Samples` okunmasının sebebi de bu. Bu makinede
  gerçek bir pass'te sıfır **görülmedi** (ölçüm aşağıda).

- **Açılış damgası `main()`'in ilk satırında**, `has_aqua_session()`'dan da
  önce: o çağrı bir alt süreç doğuruyor (`launchctl managername`) ve bugün
  açılış yolunun parçası. Damga ondan sonra alınsaydı `acilis=` o süreyi
  sessizce düşerdi. Dürüst sınır yine de duruyor ve kodun doc'unda yazılı:
  ölçülen şey **süreç başlangıcı değil**, "main'den ilk tamamlanan kareye" —
  dyld ve Rust runtime kurulumu bu damgadan önce bitiyor. Damga `Stats`'a
  **taşınıyor**, kurucu kendi okumuyor (R3.3'ün tipteki hâli).

- **Örnek yalnız yola çıkan karede yazılıyor.** `session.frame` `None` dönen
  kare `setPaused(true)` ile erken dönüyor (kılavuzun uyarısı) **ve** `draw`
  `Err` dönen kare de örnek yazmıyor: ikisinde de `draw` ya hiç koşmadı ya da
  bitmedi, "encode = 0 ns" sahte örneği p95'i aşağı çekerdi.

- **Sınamaların ikisi yapısal bekçinin *ikinci hattı*** ve bunu gövdelerinde
  söylüyorlar: `empty_frame_records_no_sample` gerçek erken dönüşü koşamıyor
  (display link ister), `startup_stamp_precedes_renderer_setup` de `main.rs`
  → `bt_shell::run` sırasını koşamıyor. Pinledikleri şey ikinci yarı: üç damga
  tam olmadan halka büyümüyor, ve `Stats` damgayı **alıyor** (kurucudan önce
  geçen süre ölçüme giriyor — 2 ms'lik uyku tam bunu kırmızıya çevirebilsin
  diye var). `clock_untouched_when_gate_closed` ise gerçek bekçi: `stamp`'in
  `clock` parametresi sayılabilir olsun diye var ve üç damganın üçü de o
  fonksiyondan geçiyor.

- **Ölçülenler (2026-09-11, debug profili, bu makine).** Sayı iddia değil
  gözlem. İki koruma, ikisi de ileriye bakıyor: (1) bunlar **taban değildir**
  ve taban olarak kullanılamaz — debug profili, örtülü pencere ve taban altı
  bir örneklem (bir ve üç örnek); ilk `/measure` bunlarla karşılaştırma
  yapmaz, sıfırdan ölçer. (2) `docs/OLCUMLER.md`'ye **taşınmazlar** (R7.3);
  o dosyayı ilk `/measure` kendi koşusuyla kurar. Buradaki işleri tek: halkanın
  gerçekten dolduğunu ve R5.6 kararının dayanağını kayda geçirmek:
  - `BT_FRAME_STATS=1 BT_RUN_SECONDS=3` (smoke): `kare=1` **değişmedi**,
    sütun başına **1 örnek**, düşen 0. Değerler tek örnekten (`n=1`) ve bu
    yüzden büyüklük mertebesinden fazlasını söylemiyor: açılış ~170 ms,
    GPU ~0,3 ms.
  - `BT_SCROLL_TEST=1 BT_FRAME_STATS=1 BT_RUN_SECONDS=3` (load): `kare=3`,
    sütun başına **3 örnek**, düşen 0, `cpu_kare≈0,71–0,76 ms`,
    `cpu_encode≈0,78–2,49 ms` (ilk kare pahalı), `gpu≈0,12–0,36 ms`,
    `acilis≈168–176 ms`. Orkestratörün öngördüğü tablo birebir çıktı: örtülü
    pencerede halka **üç örnek** topluyor ve bu p95 için taban altı —
    kararı R5.6 gereği phase-3 verecek.
  - **GPU damgaları sıfır değil**: hem gerçek koşuda hem
    `completion_records_gpu_delta_and_startup` sınamasında okunuyor.

- **`/simplify`'ın dört mercek ajanı beş şeyi değiştirdi ve üçü aynı yere
  bakıyordu.** Uygulananlar:
  1. **`stamp()` yardımcısı silindi** (reuse + simplification + altitude, üçü
     de bağımsız buldu): gövdesi `gate.map(|_| clock())` idi ve `clock`
     parametresi yalnız `clock_untouched_when_gate_closed`'ın çağrı sayabilmesi
     için vardı — yani sınadığı şey `Option::map`'in tembelliği, yani std'nin
     kendi garantisi. Damgalar artık `is_some().then(Instant::now)` ve
     `Option::map` ile, `main.rs`'in aynı diff'teki deseniyle aynı. Sınama da
     silindi (kutusu `[~]`, gerekçesi checklist'te).
  2. **Damga üçlüsü ikiliye indi.** `record_cpu(Option, Option, Option)` +
     `let ... else return` bir dalı çalışma zamanında savunuyordu, oysa o dal
     üretimde **ulaşılmaz**: eksik damgalı kare erken dönüyor. Artık
     `record_cpu(frame: Duration, encode: Duration)` ve damga çifti tek bir
     `Option` içinde taşınıyor — kural tipte. `empty_frame_records_no_sample`
     ölü bir dalı pinliyordu, yerine `cpu_sample_fills_both_columns` geldi:
     sütunların karışmasını (ya da ikisini aynı halkaya itmeyi) **gerçekten**
     kırmızıya çeviriyor.
  3. **Ölçüm `Renderer::completion`'dan çıktı** (altitude): blok artık başarı
     kolunda **komut tamponunu** geçiriyor ve `mark_startup`/`record_gpu`
     çağrısı `link.rs`'in closure'ında, `retry.streak.succeeded()`'in yanında
     duruyor. Üç kazanç: `renderer.rs` `crate::stats`'ı hiç görmüyor (ölçüm
     politikası "ne çizeceğini bilen" tarafa sızmıyor), kapı hâlâ kapalıyken
     iki ObjC çağrısı yapılmıyor, ve elle yazılmış `assert_send_sync` bloğu
     **silindi** — `on_complete`'in mevcut `Send + Sync` sınırı `Arc<Stats>`'ı
     zaten sorguluyor.
  4. **`startup_nanos: AtomicU64` → `startup: OnceLock<Duration>`** (reuse):
     sıfır-sentinel, `max(1)` düzeltmesi ve CAS gitti; desen zaten depoda
     (`ShellWake.waker`, `Session`'ın göndereni) ve kare başına bedel aynı
     (bir atomik okuma).
  5. **Halka:** `fetch_add` `AcqRel`'den **`Relaxed`**'a indi (bileti benzersiz
     kılan RMW'nin atomikliği, sıralaması değil; yuva zaten `Relaxed`
     okunuyor), iki `usize::try_from(...).unwrap_or(0)` yerine tek `slot()`
     yardımcısı geldi (sessizce yuva 0'ı bozan geri düşüş kalktı), kapasite
     kırpması **tek yerde** kaldı (`Ring::new`) ve `Ring` `#[repr(align(64))]`
     oldu: üç imleç tek önbellek satırına düşerse ana thread ile Metal'in
     thread'i kare başına o satırı birbirinden çeker ve ölçüm aracı ölçtüğü
     şeyi bozar.

  **Uygulanmayan üçü, gerekçeleriyle:**
  - *Halka kapasitesi sabit olsun, `run_seconds`'tan türemesin* (altitude):
     kapasite formülü kılavuzun §4'ünde ve orkestratörün brief'inde **açıkça**
     isteniyor ("kapasite hesabı yine de doğru olmalı"). Onaylı tasarımın
     dışına çıkmadan değiştirilemez.
  - *`Options` tek alanlı, `pub fn run(run: Option<Run>)` yeter*
     (simplification, koşullu olarak işaretledi): `Options` ayar yolunun (00X)
     yeri ve kılavuzun §1'i onu bir yapı olarak adlandırıyor; alan eklemek
     imzayı kırmasın.
  - *`run_deadline`'daki `if let Some(run)` bir tip olmalı* (altitude, düşük
     öncelikli): alternatifi `expect` ve o, rapor yolunda panik demek —
     kapanışta bir panik raporun kendisini yutar (phase-1'in `atlas_occupancy`
     kararıyla aynı gerekçe).

  **Not edilen ama dokunulmayan** (efficiency): halkalar açılış yolunda
  ayrılıyor, yani `acilis=` sayısının **içinde**; `make duman`'ın 3 saniyelik
  koşusunda 8,6 KB, tavanda 1,7 MB. Sayıyı okuyan bunu bilmeli, phase-3'ün
  `## Yöntem` notuna düşer.

- **`/code-review` üç bulgu verdi; biri yüksek, ikisi düşük.**
  - **YÜKSEK — ölçüm yükünün kapanışı asılıyor ve sebebi bulundu.** Bulgu
    `bt-core`'da: `Session::shutdown` (`session.rs:820`) önce `Msg::Shutdown`
    yollayıp okuyucu thread'i **join ediyor**, yani `Pty` ana thread'de
    düşüyor ve `Pty::drop`'un `SIGHUP`'ı master fd'yi **kimse okumazken**
    gidiyor; PTY'yi doldurmakta olan çocuk çıkışın içinde takılıyor
    (`ps` durumu `?Es`) ve `child.wait()` dönmüyor. `smoke_shell` bunu hiç
    görmüyor (~100 bayt yazıp uyuyor); `load_shell` sürekli akıtıyor ve
    betiğin `+ 1`'i çocuğun deadline anında **hâlâ akıtıyor** olmasını
    garantiliyor. Ölçüldü (inceleyen, bu makine): on koşunun dördü, her biri
    12,1 sn (3 sn deadline + 9 sn bekçi bütçesi), çıkış 70 ve **boş stdout**.
    **Bu phase'in doğurduğu bir kusur değil:** `adb8f50` üstünde `git stash`
    ile birebir üretildi, yani phase-1'in `load_shell`'i + 002'nin kapanış
    sırası. **Waive edildi, gerekçesi kapsam:** kalıcı çözüm `bt-core`'da
    sınırlı bekleme (`SIGHUP` → süre → `SIGKILL`) ya da `child.wait()` öncesi
    master'ı boşaltmak; ikisi de `CLAUDE.md`'nin adıyla andığı borç ve bu
    phase `bt-shell` + `bt-gpu` kapsamında. Phase-3'e **kök neden teşhisiyle**
    devredildi — orada önemli, çünkü rapor `report_and_exit`'ten basılıyor ve
    asılan koşuda satır hiç çıkmıyor.
  - **DÜŞÜK — `IDLE_FRAME_LIMIT = 2`'nin bilinen yanlış pozitifi.** Koşu
    sırasında pencereyi sürüklemek meşru kare doğurur ve kapı düşer; phase-1
    bunu ölçüp sabitin doc'una yazmış, burada yalnız bilinçli olduğu
    doğrulandı.
  - **DÜŞÜK — kapı hermetik değildi, artık öyle (düzeltildi).** Kabukta
    ihraç edilmiş bir `BT_SCROLL_TEST` `make duman`'ı **sessizce** yük
    koşusuna çeviriyordu: `var_os` değere değil **varlığa** bakıyor
    (`BT_SCROLL_TEST=` bile yükü seçer), koşu `yuk=load` basıp exit 0 veriyor
    ve duman sözleşmesinin `hucre`/`kural` yarısı hiç sınanmıyordu — kapı
    yeşil, ama iddia ettiğinden başka bir şeyi sınıyor. `Makefile`'ın `duman`
    hedefi artık `env -u BT_SCROLL_TEST -u BT_FRAME_STATS` ile koşuyor;
    doğrulandı: `BT_SCROLL_TEST=1 make duman` → `yuk=smoke`, çıkış 0.
    (Tek dosyalık düzeltme, phase kapsamının içinde: korunması istenen
    kapının kendisi.)

- **Pencere kapanmıyor: `BT_SCROLL_TEST=1 BT_RUN_SECONDS=5` bekçiye düşüyor**
  (`_exit(70)`, jeton satırı hiç basılmıyor) ve bu **phase-2'den önce de
  böyle** — `git stash` ile `adb8f50` üstünde birebir doğrulandı, yani
  phase-1'den kalma. 3 saniyede **seyrek**: art arda beş koşunun dördü çıkış 0, biri aynı bekçiye düştü — yani yarış 3 saniyede de var, yalnız daha az ateşliyor.
  En olası sebep yarış: `load_shell`'in kendi `while` döngüsü tam deadline
  anında bitiyor, `child_exit` ile `runDeadline:` çakışıyor ve `Pty::drop`'un
  `child.wait()`'i asılıyor (`CLAUDE.md`'nin bilinen borcu: sınırlı bekleme
  `bt-core`'a düşüyor). Phase-3'ün checklist'ine yazıldı: R5.6'nın ölçüm
  koşusu bu komutu kullanıyor ve **5 saniye ile koşulamıyor**.

## Yayın Etkisi

- **Ölçüm bekliyor: yok** — bu phase araç üretiyor, iddia değil. Kapı
  kapalıyken hiçbir yol değişmediği için kendi maliyeti de ölçülecek bir şey
  değil; kapalı kapının bedeli `Option` üstünde bir dallanma.
- `make duman` çıktısı **bu phase'de değişmez**: örnekler birikiyor ama
  basılmıyor. `kare=1 hucre=8 glif=6 kural=15 yuva=U/T yuk=smoke` aynen durur.
- Yeni bağımlılık **yok** (`std::time::Instant`), `Cargo.lock` oynamamalı.
  `.metal` ve `build.rs` el değmiyor — GPU zamanı komut tamponundan okunuyor,
  shader'dan değil.
- **Boşta sıfır kare riski burada doğuyor.** Phase-1'in üst sınırı bu yüzden
  önce kuruldu; `make duman` bu phase'de de yeşil kalmalı ve `kare` oynamamalı.

**Ölçülen teslim (2026-09-11, debug):**

- `make duman` → `kare=1 hucre=8 glif=6 kural=15 yuva=13/2048 yuk=smoke
  pipeline=ok`, çıkış 0; art arda üç koşuda da bit bit aynı. Jeton **eklenmedi
  ve silinmedi** — bu phase hiçbir şey basmıyor.
- Kapı **açıkken** de aynı satır: `BT_FRAME_STATS=1 BT_RUN_SECONDS=3` →
  `kare=1 …`, çıkış 0 (iki koşu). Yalnız `make duman`'a bakmak yetmezdi:
  o koşu kapıyı hiç açmıyor ve enstrümantasyonun boşta kare üretip
  üretmediğini gösteremezdi.
- Yeni bağımlılık yok, `Cargo.lock` oynamadı, `.metal`/`build.rs`/`assets`
  el değmedi. Kullanıcının makinesinde değişen bir şey yok: yeni ayar
  anahtarı, tema biçimi, `TERM` ya da shell entegrasyon dosyası doğmadı.
- **Yeni kullanım hatası kolu:** `BT_FRAME_STATS` süresiz (ya da
  `BT_RUN_SECONDS=0` ile) verilirse süreç *"sıfırdan büyük bir
  BT_RUN_SECONDS ister"* deyip çıkış 1 veriyor — `BT_SCROLL_TEST`'in eşi.
- **Ölçüm bekliyor: yok** (araç üretildi, iddia değil). Kapalı kapının bedeli
  bir `Option` dallanması: ne saat okunuyor ne halka ayrılıyor.

---

## Checklist

- [x] `Options.stats_since: Option<Instant>` — bool **değil**; damga `main.rs`'te, `bt_shell::run`'dan önce (alan `Run`'ın **içinde**; gerekçe Uygulama Notları)
- [x] `main.rs`: `BT_FRAME_STATS` tek yerde okunur (R4.2) — damga **ilk satırda**, `has_aqua_session()`'dan da önce
- [x] `link.rs`: `t0`/`t1`/`t2` — iki ayrı CPU aralığı; `None` dönen karede örnek **yazılmaz** (`draw` `Err` dönen karede de)
- [x] GPU deltası tamamlanma bloğundan, `Arc`'lı halkayla — blok kare başına **kurulmadı**; sıfır damgalı kare eleniyor. Kod `renderer.rs`'te **değil** `link.rs`'in closure'ında (`/simplify` → altitude): `Renderer::completion` artık başarı kolunda komut tamponunu geçiriyor ve ölçüm politikası renderer'a sızmıyor
- [x] `Samples` halkası: önceden ayrılmış, kilitsiz, düşen örnek sayılıyor (tip adı `Stats`/`Ring`, `Samples` sütun anlık görüntüsü oldu — Uygulama Notları)
- [~] Test: `clock_untouched_when_gate_closed` — **yazıldı, sonra `/simplify`'da silindi** (dosyada YOK). Üç mercek ajanı da bağımsız olarak aynı şeyi buldu: sınadığı şey `Option::map`'in `None` üstünde closure koşturmaması, yani **std'nin garantisi**; onu sınanabilir kılan `stamp(gate, clock)` sarmalayıcısı da yalnız o sınama için vardı. Kural yerinde ve yorumda yazılı (`link.rs`: `is_some().then(Instant::now)`), ama artık onu koruyan şey sınama değil std'nin kendisi. Phase-1'in ölçütüyle aynı gerekçe: hiçbir koşulda kırmızı düşemeyen sınama, sinyal değil süs
- [~] Test: `empty_frame_records_no_sample` — **yazıldı, sonra `/simplify`'da silindi** (dosyada YOK); yerine `cpu_sample_fills_both_columns` var. Gerekçe: sınadığı dal (`record_cpu`'ya eksik damga gelmesi) üretimde **ulaşılmaz** — eksik damgalı kare erken dönüyor — ve o dalı tutan `Option` üçlüsü de aynı `/simplify`'da kalktı. Kural artık tipte: `record_cpu` iki `Duration` alıyor, yarım örnek **temsil edilemiyor**. Yeni sınama gerçek bir hatayı yakalıyor: iki sütunun karışması ya da ikisinin aynı halkaya itilmesi
- [x] Test: `full_ring_drops_oldest_and_counts` — kapasite aşımı sessiz değil (+ `zero_second_run_still_has_a_ring`, `gpu_zero_timestamps_record_nothing`, `completion_records_gpu_delta_and_startup`)
- [x] Test: `startup_stamp_precedes_renderer_setup` — tipin üstlendiği yarıyı pinliyor (`Stats` damgayı alıyor, okumuyor); sıranın kendisi yapısal
- [x] **phase-1'den devir (2):** `Options` iki ayrı `Option` taşıyor (`run_seconds`, `workload`) ama `main.rs` ikisini **hep birlikte** `Some` yapıyor — tip imkânsız durumlara izin veriyor ve bedeli üç `unwrap_or(0)` + iki `| None` kolu + "ulaşılmaz dal" yorumu. Bu phase zaten `Options`'a `stats_since` ekliyor: şekli orada `Option<Run { seconds, workload }>`'a indir (phase-1 `/simplify`'ının iki jürisi de önerdi, phase-1'de onaylı tasarımın dışına çıkmamak için uygulanmadı) — **uygulandı**: `Options { run: Option<Run> }`, üç `unwrap_or(0)`, iki `| None` kolu, `debug_assert` ve L3'ün ulaşılmaz `None` kolu düştü
- [~] **phase-1'den devir (3):** boşta sıfır karenin bekçisi bugün **GPU karesini** sayıyor (`kare`), yani örtülme ve display link askıya alınması onu köreltiyor. Daha derin ölçüt **kare talebi** (`Waker::wake` / `request_frame`): bizim tarafımızda, örtülmeden etkilenmez ve bilerek bozulmuş boşta-sıfır-kareyi anında yakalardı. Yeni sayaç + yeni jeton demek, o yüzden phase-1'de yapılmadı — bu phase sayaç işine zaten giriyor. **Yapılmadı ve phase-3'e devredildi:** yeni bir sayaç + yeni bir jeton demek, jeton basmak ise bu phase'in bilerek dışında ("hiçbir şey basılmıyor") — sayacı bu phase'de kurup jetonu phase-3'te basmak, kapının yarısını iki commit'e bölerdi. Kutunun kendisi de bunu zaten söylüyor: ölçüt `Waker::wake` sayısı, yani bu phase'in dokunduğu ölçüm halkasının değil **kapının** işi. Satır `phase-3.md` checklist'ine yazıldı
- [x] **phase-1'den devir:** `make duman`'ın penceresi örtülü (`occlusionState` `Visible` taşımıyor), sistem display link'i askıya alıyor ve kare sayısı hasardan bağımsız **~3'te doyuyor** — yük 260 kat hızlandıktan sonra bile. `IDLE_FRAME_LIMIT` bu ölçüme göre `8`den `2`ye indirildi ve sabotajla ateşlediği doğrulandı, yani kapı artık kör değil. **Ama örnekleme ayrı bir körlük:** örtülen pencerede örnek akışı sessizce durur ve az örnek üstünden hesaplanan p95 *iyi* görünür (R5.2) — `ornek=` tam bunun detektörü. Bu koşuda `ornek=` kaç çıkıyorsa **ölçülüp yazılsın**; doyan ritim (3 kare) p95 için anlamlı bir örneklem vermiyor olabilir ve o zaman `/measure` gerçek pencere şart koşmalı. **Ölçüldü:** smoke + kapı açık → sütun başına **1** örnek, load (3 sn) → sütun başına **3** örnek, düşen 0; `kare` iki koşuda da oynamadı. Yani halka çalışıyor ama örtülü pencerede taban altı kalıyor — R5.6'nın kararı phase-3'te
- [x] Doğrulama geçti (`make hepsi` → 0; `make duman` → `kare=1 hucre=8 glif=6 kural=15 yuva=13/2048 yuk=smoke pipeline=ok`, çıkış 0, üç koşuda da aynı; kapı **açıkken** de `kare=1`; `make test-yaris` → 0 — tamamlanma thread'ine paylaşılan durum girdi. `make shader` ve `make terminfo` **gerekmedi**: `.metal`, `build.rs` ve `assets/terminfo` el değmedi)
- [x] `/simplify` çalıştırıldı (dört mercek ajanı), beş bulgu uygulandı; uygulanmayan üçü gerekçesiyle Uygulama Notları'nda
- [x] `/code-review` çalıştırıldı (Skill fork'u ~14 dk sürdü ve döndü; `proje.md` basamak 2'deki `code-reviewer` subagent'ı da paralel koşturuldu, ikisi ayrı bulgu seti verdi). Subagent: NaN/sonsuz kapısı (**gerçek kusur, düzeltildi + `gpu_rejects_nan_and_infinity` sınaması eski kapıda kırmızı düştüğü doğrulanarak eklendi**), `u64::MAX` doyurmasının zehirlemesi (düzeltildi), `cpu_encode`'un `push_cursor`'ı da ölçmesi (düzeltildi), halkanın yırtılma penceresinin doc'ta eksik anlatılması (doc düzeltildi + yazılmamış yuva eleniyor). Skill fork'u: kapanış kilitlenmesi (**waive**, aşağıda), `IDLE_FRAME_LIMIT` yanlış pozitifi (phase-1'de bilinçli), `make duman`'ın hermetik olmaması (**düzeltildi**)
- [x] `/audit` çalıştırıldı: mekanik mercekler inline (1 katman yönü ✓, 3 panik yolu ✓, 10'un tanımlayıcı yarısı ✓; 2 ilgisiz — `Cargo.toml`/`Cargo.lock` el değmedi, 4/5/9 ilgisiz — ayar, shell üçlüsü, `.metal`/`Cell` el değmedi), yargı mercekleri üç ajanla: **7 temiz** (`Arc<Stats>`'ın `Drop`'u bekleme kenarı taşımıyor, `OnceLock` Metal'in thread'inde bloklamıyor, kare yolunda kilit/ayırma yok), **8 temiz** (üç bozma yolunun üçü de kapalı, `setPaused(true)` yolları birebir korundu), **10 üç bulgu** — `align(64)` bu makinede **yetmiyordu** (`hw.cachelinesize` = 128, ölçüldü) ve yorum tutamayacağı bir söz veriyordu → `align(128)`; modül başlığı kare başına bedeli eksik sayıyordu → düzeltildi; `bt-gpu`'nun `lib.rs` başlığı yeni sorumluluğu anmıyordu → eklendi. Mercek 6 (ölçüm sahipliği) ayrıca sorgulandı: sayılar **kalıyor**, çerçevesi güçlendirildi ("taban değil", "`OLCUMLER.md`'ye taşınmaz", `n=1` işareti)
- [x] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [x] Commit: `8df1ef6`
