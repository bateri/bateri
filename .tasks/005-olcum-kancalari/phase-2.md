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

## Yayın Etkisi

- **Ölçüm bekliyor: yok** — bu phase araç üretiyor, iddia değil. Kapı
  kapalıyken hiçbir yol değişmediği için kendi maliyeti de ölçülecek bir şey
  değil; kapalı kapının bedeli `Option` üstünde bir dallanma.
- `make duman` çıktısı **bu phase'de değişmez**: örnekler birikiyor ama
  basılmıyor. `kare=1 hucre=8 glif=6 kural=15 yuva=U/T` aynen durur.
- Yeni bağımlılık **yok** (`std::time::Instant`), `Cargo.lock` oynamamalı.
  `.metal` ve `build.rs` el değmiyor — GPU zamanı komut tamponundan okunuyor,
  shader'dan değil.
- **Boşta sıfır kare riski burada doğuyor.** Phase-1'in üst sınırı bu yüzden
  önce kuruldu; `make duman` bu phase'de de yeşil kalmalı ve `kare` oynamamalı.

---

## Checklist

- [ ] `Options.stats_since: Option<Instant>` — bool **değil**; damga `main.rs`'te, `bt_shell::run`'dan önce
- [ ] `main.rs`: `BT_FRAME_STATS` tek yerde okunur (R4.2)
- [ ] `link.rs`: `t0`/`t1`/`t2` — iki ayrı CPU aralığı; `None` dönen karede örnek **yazılmaz**
- [ ] `renderer.rs`: GPU deltası tamamlanma bloğundan, `Arc`'lı halkayla — blok kare başına **kurulmadı**
- [ ] `Samples` halkası: önceden ayrılmış, kilitsiz, düşen örnek sayılıyor
- [ ] Test: `clock_untouched_when_gate_closed` — kapı `None` iken `Instant::now()` çağrılmadığını gösteren bekçi (sayaç ya da tip düzeyinde)
- [ ] Test: `empty_frame_records_no_sample` — `session.frame` `None` dönünce halka büyümüyor
- [ ] Test: `full_ring_drops_oldest_and_counts` — kapasite aşımı sessiz değil
- [ ] Test: `startup_stamp_precedes_renderer_setup` — damganın `system_default()` öncesinde alındığını bağlayan sınama
- [ ] **phase-1'den devir (2):** `Options` iki ayrı `Option` taşıyor (`run_seconds`, `workload`) ama `main.rs` ikisini **hep birlikte** `Some` yapıyor — tip imkânsız durumlara izin veriyor ve bedeli üç `unwrap_or(0)` + iki `| None` kolu + "ulaşılmaz dal" yorumu. Bu phase zaten `Options`'a `stats_since` ekliyor: şekli orada `Option<Run { seconds, workload }>`'a indir (phase-1 `/simplify`'ının iki jürisi de önerdi, phase-1'de onaylı tasarımın dışına çıkmamak için uygulanmadı)
- [ ] **phase-1'den devir (3):** boşta sıfır karenin bekçisi bugün **GPU karesini** sayıyor (`kare`), yani örtülme ve display link askıya alınması onu köreltiyor. Daha derin ölçüt **kare talebi** (`Waker::wake` / `request_frame`): bizim tarafımızda, örtülmeden etkilenmez ve bilerek bozulmuş boşta-sıfır-kareyi anında yakalardı. Yeni sayaç + yeni jeton demek, o yüzden phase-1'de yapılmadı — bu phase sayaç işine zaten giriyor
- [ ] **phase-1'den devir:** `make duman`'ın penceresi örtülü (`occlusionState` `Visible` taşımıyor), sistem display link'i askıya alıyor ve kare sayısı hasardan bağımsız **~3'te doyuyor** — yük 260 kat hızlandıktan sonra bile. `IDLE_FRAME_LIMIT` bu ölçüme göre `8`den `2`ye indirildi ve sabotajla ateşlediği doğrulandı, yani kapı artık kör değil. **Ama örnekleme ayrı bir körlük:** örtülen pencerede örnek akışı sessizce durur ve az örnek üstünden hesaplanan p95 *iyi* görünür (R5.2) — `ornek=` tam bunun detektörü. Bu koşuda `ornek=` kaç çıkıyorsa **ölçülüp yazılsın**; doyan ritim (3 kare) p95 için anlamlı bir örneklem vermiyor olabilir ve o zaman `/measure` gerçek pencere şart koşmalı
- [ ] Doğrulama geçti (`proje.md` → Doğrulama; `make duman` **zorunlu**: `kare` oynamamalı)
- [ ] `/simplify` çalıştırıldı, bulgular uygulandı
- [ ] `/code-review` çalıştırıldı, bulgular giderildi
- [ ] `/audit` çalıştırıldı, bulgular giderildi
- [ ] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
