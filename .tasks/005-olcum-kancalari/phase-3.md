# Phase 3 — Rapor: jetonlar ve belge uyumu

## Özet

p95 ile en kötü değer süreç içinde hesaplanır, jeton satırı genişler
(`ornek=`, `profil=`, CPU/GPU/açılış), ve belgelerdeki kanca adları kodla
uyumlanır. Bu phase'ten sonra `/measure` gerçek sayı okuyabilir.

_Requirements: R5, R5.1, R5.2, R5.3, R5.4, R5.5, R7, R7.1, R7.2, R7.3_

---

## 1. İstatistik süreç içinde — dosya **yok**

`crates/bt-gpu/src/` (halkanın yanında)

Dosya kararı üç koldan çürüdü ve gerekçesi `discussion.md` Karar 4'te; en
somutu şu: `/measure`'ın `allowed-tools`'unda `awk`, `python`, `cut` yok —
yalnız `sort` ve `wc`. Bir TSV'den p95 hesaplamak o araç setiyle eziyet, tek
jeton satırı ise bir `Read`.

```rust
/// p95 ve en kötü. Ortalama **yok**: bir takılma ortalamayı oynatmaz ama
/// kullanıcı onu görür (`/measure` → "dağılım, ortalama değil").
///
/// Örnek sayısı azsa p95 anlamsızdır ama **sessiz** de olmamalı: sayı
/// basılır, yanında `ornek=` gider, yorumu `/measure` yapar.
fn p95_and_worst(halka: &[Duration]) -> Option<(Duration, Duration)>
```

---

## 2. Jeton satırı genişler

`crates/bt-shell/src/app.rs` → `report_and_exit`

Sözleşme aynen: **silinmez, eklenir.** Mevcut beş jeton (`kare`, `hucre`,
`glif`, `kural`, `yuva`) ve `pipeline=ok` yerinde kalır.

```
kare=1 hucre=8 glif=6 kural=15 yuva=21/256 ornek=0 profil=debug pipeline=ok
kare=412 hucre=118 glif=3944 kural=0 yuva=97/256 ornek=412 profil=release \
  cpu_kare_p95=1.8ms cpu_kare_max=4.1ms cpu_encode_p95=0.3ms \
  gpu_p95=2.2ms gpu_max=5.0ms acilis=284ms pipeline=ok
```

Üç jeton yük taşıyor:

- **`ornek=`** — örnekleme **sessizce durabilir**: pencere örtülünce kapı
  kapanıyor (`link.rs:262`, `app.rs:271-281`) ve üç örnek üstünden hesaplanan
  p95 *iyi* görünür. Bundle'sız `cargo run` penceresi tam da onu başlatan
  terminalin arkasına düşüyor, yani bu teorik bir risk değil. `/measure` sayı
  yorumlamadan **önce** buna bakar.
- **`profil=`** — `make duman` **debug** koşuyor, `/measure` **release** şart
  koşuyor. Belgeye güvenmek yerine sayının kendisi profilini söylüyor; debug
  sayısını taban sanmak imkânsız oluyor.
- **`acilis=`** — tek sayı, dağılım değil: açılış koşu başına bir kez olur.

> **`Drop`'a güvenilmez (R5.5).** `report_and_exit` `process::exit` ile
> çıkıyor ve o `Drop` koşturmuyor; bekçinin `_exit(70)`'i atexit zincirini
> bile atlıyor (`lib.rs:79-87`). Jetonlar `println!` ile **açıkça** basılır,
> hiçbir tampon `Drop`'ta boşalmaya bırakılmaz.
>
> **Sıra korunuyor:** `report_and_exit` `shutdown()`'dan **sonra** çağrılıyor
> ve bu bilinçli (`app.rs:411-415`) — asılan bir kapanışta `kare=` satırı hiç
> çıkmasın diye. Yan faydası bize: `shutdown()` içinde `link.stop()` koştuğu
> için rapor anında halka **durağan**. Uçuştaki son bir iki tamamlanma bloğu
> kaçabilir; `ornek=` bunu görünür kılar.

---

## 3. Belgeler — borç cümleleri **silinmez, yeniden yazılır**

`CLAUDE.md`, `.claude/skills/measure/SKILL.md`,
`.claude/is-akisi/sablonlar/context.md`, `.claude/is-akisi/proje.md`

**R7.2 — kanca adları.** Belgelerde yazan `BT_FRAME_LOG` adı bu tasarımda
**yalan** olurdu: log yok, dosya yok. `BT_FRAME_STATS` olur. `BT_STARTUP_TRACE`
ayrı bayrak değil — açılış aynı bayrağın altında tek sayı. Dört yer güncellenir
(adların bugün geçtiği yerler `context.md`'de listeli).

**R7.1 — bench sözü.** `CLAUDE.md` bugün *"kancalar gelince bu cümle kalkar ve
`cargo bench` satırı yukarıdaki bloğa geri gelir"* diyor. Kancalar geldi ama
bench gelmedi (`discussion.md` Karar 7). Cümle **silinmez**: bu set bir borç
cümlesini başka bir borçla takas ediyor ve bunu gizlemek, tam da bu setin
düzelttiği hatanın kendisi olurdu.

Yeni hâli şunu söylemeli: ölçüm kancaları **var**, `docs/OLCUMLER.md` ilk
`/measure` ile doğacak, `cargo bench` satırı **bench setini** bekliyor ve
bunun bedeli 003 #1 ile #2'nin bench yarısının açık kalması.

**R7.3 — `docs/OLCUMLER.md` bu sette yazılmaz.** `/measure` skill'i zaten
"dosya yoksa ilk ölçüm onu kurar" diyor. Ama phase-2'nin **dürüst sınırı**
(açılış damgası `main()`'in başında, süreç başlangıcında değil) kaybolmamalı:
kodun doc yorumunda durur ve ilk `/measure` onu `## Yöntem`'e taşır.

`.tasks/README.md`: 002, 003, 004 satırları artık "ölçüm aracı yok" demiyor —
"`/measure` koşulabilir; bench'e bağlı iddialar bench setini bekliyor".

---

## Uygulama Notları

- **Kılavuzun `ornek=0`'ı `ornek=kapali` oldu (sapma, R5.2 gereği).** §2'nin
  örnek satırı kapalı kapıda `ornek=0` gösteriyordu. Sıfır, "kapı açıktı ve
  hiç örnek toplanmadı" ile **aynı görünür** — oysa ikincisi bir arıza
  (pencere yutuldu, hiç kare bitmedi) ve R5.2 tam olarak o körlüğü kapatmak
  için var. Kapalı kapıda ölçüm jetonlarının hiçbiri basılmıyor; sözleşme
  jetonun **yokluğunu** okumaya izin veriyor ("okuyan taraf tanımadığı jetonu
  atlayabilir"), yalan bir değere değil.

- **Jeton seti ve `ornek=`'in tekil olamaması (devir kararı).** GPU sütunu
  CPU'dan kısa kalabildiği için `ornek=` tek sayı olamazdı; karar **sütun
  başına ayrı jeton**:
  `ornek=` (CPU, iki sütun birlikte yazıldığı için tek sayı), `gpu_ornek=`,
  `gpu_elenen=` (Metal'in sıfır/NaN damgası), `dusen=` (halkaya sığmayan) ve
  `taban=` (R5.6'nın eşiği, satırdan okunabilsin diye). `dusen=` **tek**
  sayı ve CPU'dan geliyor: GPU yalnız çizilen karede yazdığı için `pushed`'ı
  CPU'nunkini aşamaz ve üç sütunun kapasitesi aynı, yani CPU'nunki bir
  **tavan**. Tam satır:

  ```
  kare= hucre= glif= kural= yuva= yuk= istek= kapanis= profil= \
  ornek= dusen= gpu_ornek= gpu_elenen= taban= \
  cpu_kare_p95= cpu_kare_max= cpu_encode_p95= cpu_encode_max= \
  gpu_p95= gpu_max= acilis= pipeline=ok
  ```

- **`MIN_SAMPLES = 20` türetildi, seçilmedi.** p95 en yakın-sıra yöntemiyle
  `sorted[ceil(0.95 × n) − 1]`; `n < 20` için o indeks **son** elemandır, yani
  p95 ile en kötü aynı sayıya çöker ve rapor bir sayıyı iki kez adlandırır.
  `20`, çökmenin bittiği ilk `n`. Sınama iki yönlü: `n = 19` → `None`,
  `n = 20` kesin artan girdide → p95 ≠ max.

- **`p95_and_worst` `Samples`'ın metodu ve `self`'i tüketiyor** (kılavuz onu
  `&[Duration]` alan bir serbest fonksiyon olarak yazmıştı). Halka `Vec<u64>`
  nanosaniye tutuyor; `Duration` dizisine çevirmek kapanışta gereksiz bir
  kopya olurdu. Tüketmesinin yan faydası sıra: sayaç yarısı (`nanos.len()`,
  `dropped`, `rejected`) p95'ten **önce** okunmak zorunda ve bunu tip
  söylüyor.

- **`kapanis=` için `bt-core` değişti** (`Session::shutdown` artık
  [`Teardown`] döndürüyor, `SHUTDOWN_GRACE` `pub`). Checklist bunu "burada
  karar, orada uygulama" diye devretmişti ve devam edecek bir phase yok:
  phase-3b belge phase'i. Beş varyant (`Clean`/`Abandoned`/`Panicked`/
  `Unbounded`/`AlreadyDone`) `bool`'a katlanmadı — `Verdict`'le aynı gerekçe.
  İkinci çağrı `AlreadyDone` döner ve `Drop` bunu `let _` ile yutuyor:
  gerçek sonucu ilk çağrı biliyor.

- **`istek=` sayacı `Waker::wake`'in ilk satırında**, kapıdan da hasar
  bayrağından da önce: ölçülen şey talebin kendisi. `DisplayLink::requests()`
  ile okunuyor. **Kapı değil, sayaç** — eşiği ölçülmedi.

- **`/code-review`'un dört ölçüm-yolu bulgusu kapandı:** (1) `record_gpu`'nun
  elediği kare artık sayılıyor (`Ring::rejected`, `Samples::rejected`,
  `gpu_elenen=` jetonu); (2) `completion_hands_over_live_gpu_timestamps`
  artık `nanos.len() + rejected == 1` diyor — donanımın sıfır dönmesi
  `make hepsi`'yi kırmızıya düşürmüyor ve kaybolan sinyalin yeni yeri
  `gpu_elenen=` jetonu (sınamanın doc'unda yazılı); (3) `verdict` dört
  konumsal sayaç yerine `Counters` alıyor; (4) `Workload::token()` dizgiyi
  tipin yanına taşıdı.

- **Hata satırı da bir jeton kazanmadı ama bir sayı kazandı (sapma):**
  `ExcessFrames` iletisi artık kare talebini de yazıyor ("… 87 kare çizildi
  (kare talebi 90) …"). Kılavuzda yoktu; ölçüm sırasında eklendi, çünkü kapı
  düşen koşuda jeton satırı **hiç basılmıyor** ve `istek=` tam da o koşuda
  en çok gereken sayı — bozuk bir boşta-sıfır-kareyi teşhis eden şey kare ile
  talep arasındaki oran. Jeton biçiminde **değil**: sözleşme jetonu yalnız
  başarı satırına koyuyor, yoksa `istek=` arayan bir CI adımı düşen koşudan
  sayı okurdu.

---

### `/simplify` — dört mercek, dördü döndü

Uygulananlar: (1) `Samples` test fixture'ları `..Default::default()`'a
devredildi (alan eklendiğinde dört çağrı yerinin haberdar edilmesi gerekmesin);
(2) `Counters` iki kez adlandırılıyordu — sayaçlar bir kez bağlanıp yapıya
konuyor; (3) test `taban=20`'yi elle yazıyordu, artık `MIN_SAMPLES`'tan
türüyor; (4) düşen örneğin değişmezi `bt-shell`'de tekrarlanıyordu.

(4) iki turda oturdu ve arada **kendi soktuğum bir kusur** var: önce
`Stats::dropped()` diye bir yüzey açtım (değişmez halkanın yanına gitsin
diye), ama o üç halkanın imlecini anlık görüntülerden **ayrı** okuyordu — yani
`dusen=` ile `ornek=` farklı anlardan gelebiliyordu. Aynı kusurun küçüğünü
`Ring::snapshot`'a da sokmuştum (`pushed` iki kez okunuyordu). İkisi de
düzeltildi: `Ring::dropped_at(pushed)` imleci **verilmiş** bir okumadan alıyor
ve `Measured::read` elindeki üç anlık görüntünün `dropped`'larının en büyüğünü
kullanıyor. Kuralın yazılı yeri yine `bt-gpu` (`Samples::dropped`'ın doc'u).

Uygulanmayan: `idle_limit_catches_excess_frames`'in test-yerel
`smoke(n,k,g,r)` kapanışları dört konumsal sayı alıyor — `Counters`'ın
kapattığı footgun'ın test içindeki hâli. Eşleme tek yerde ve görünür,
yarıçapı test; okunabilirlik lehine bırakıldı.

### `/code-review` — iki koşucu

`proje.md`'nin iniş sırasının **birinci** basamağı çalıştı (Skill fork'u) ama
geç döndü; beklerken ikinci basamak da koşuldu (`code-reviewer` subagent'ı) ve
ikisi de rapor verdi. İkisinin birleşiminden **altı** bulgu uygulandı:

1. **YÜKSEK — panikleyen okuyucu `kapanis=clean` diyordu** (ikinci koşucu).
   Kapanış thread'i `done.send(())`'i **koşulsuz** yolluyordu, yani
   `reader.join()` `Err` dönse bile `recv_timeout` `Ok` görüyor ve sonuç
   `Clean` oluyordu. Jetonun eklenme gerekçesinin tam tersi: panik yalnız
   stderr'de kalıyordu. Kanal artık okuyucunun sonucunu taşıyor ve yeni bir
   varyant var — `Teardown::ReaderPanicked`.
2. **YÜKSEK — `watchdog()` panikleyebiliyordu** (birinci koşucu).
   `std::thread::spawn` thread kurulamayınca panikler; çağrı yeri bir ObjC
   callback'i, yani panik `extern "C"` sınırından geçemez ve süreç **abort**
   eder — ne jeton satırı ne `_exit(70)`. Üstelik senaryo tam olarak
   `bt-core`'un `Teardown::Unbounded` ile hayatta kalmayı seçtiği senaryo.
   Artık `thread::Builder::spawn` ve `Err` dalında bir stderr satırı.
3. **`kapanis=` görünürdü ama kapı onu okumuyordu** (iki koşucu da).
   Panikleyen bir kapanış `pipeline=ok` basıp 0 ile çıkıyordu. `verdict`
   artık `Option<Teardown>` alıyor ve `ReaderPanicked`/`Panicked` yeşil
   geçemiyor (`Verdict::ShutdownPanicked`). `Abandoned` ile `Unbounded`
   **bilerek kapıya bağlanmadı**: ikisi de kayıtlı borç ve ilki ölçüm
   yükünün dört koşusundan birinde oluyor — bağlansaydı `make duman` bilinen
   bir borç yüzünden kırmızı düşerdi.
4. **Sıfıra kırpılan ve doyan örnekler sessizce kayboluyordu** (iki koşucu
   da). `Ring::snapshot` yazılmamış yuvayı sıfırdan tanıyor, yani halkaya
   giren **gerçek** bir sıfır hiçbir jetonda görünmeden yok oluyordu; ayrıca
   `record_gpu`'nun `f64` kapısı dönüşümden sonrasını korumuyordu — bir
   nanosaniyenin altındaki delta sıfıra kırpılıyor, sonlu ama saçma büyük bir
   delta `u64::MAX`'a **doyup** p95'i tek başına sahipleniyordu (tam da
   `is_finite` kapısının önlemek için yazıldığı sonuç). İkisi de artık
   eleniyor ve **sayılıyor**; iki yeni sınama.
5. **`istek=`'in ne saydığı doc'ta eksikti.** Yalnız shell çıktısı değil;
   yeniden deneme, `resize` ve mandal indikten sonraki uyandırmalar da
   sayılıyor. Doc bunu artık söylüyor.
6. **`Measured::read`'in "halka durağan" cümlesi yanlıştı.** İki CPU sütunu
   durağan (onları bu thread yazıyor), GPU sütunu **değil** — Metal'in
   thread'i rapor okunurken hâlâ yazabilir. Sonucu bir örneklik kayma ve doc
   artık `kare` ile `gpu_ornek + gpu_elenen` arasında eşitlik beklenmemesi
   gerektiğini yazıyor.

**Jeton değerleri Türkçeden İngilizceye çevrildi** (bulgu: değerler tutarsızdı
— `yuk=smoke|load` İngilizce, benim eklediklerim Türkçe). Depodaki tek örnek
enum varyantının adını basıyor ve satır bir makine sözleşmesi; `CLAUDE.md`'nin
`make duman` satırlarına tanıdığı Türkçe izni **tanı metni** için, bir `match`
kolunun okuduğu değer için değil. Bugünkü değerler: `kapanis=clean|
reader-panicked|abandoned|panicked|unbounded|already-done|none`, `ornek=off`,
`*_p95=insufficient`, `acilis=none`. Jeton **adları** Türkçe kaldı.

**Waive edilenler, gerekçeleriyle:**

- **`CLAUDE.md` `IDLE_FRAME_LIMIT = 2` diyor, kod `8`** — ve `CLAUDE.md`'nin
  kendi kuralı "çelişirse ikisinden biri aynı commit'te düzelir" diyor. Bu
  phase'e **belgeye dokunma** talimatı verildi ve gerekçesi tam da bu sabitin
  yeniden ölçülecek olmasıydı; ölçüm bitti, sayı kesinleşti ve satır
  `phase-3b`'nin checklist'inde adıyla duruyor. Tutarsızlık **bir commit**
  sürüyor ve kapsamı belge.
- **`Makefile` ile `CLAUDE.md`'deki jeton satırı bayat** — aynı sebeple
  `phase-3b`'de.
- **`shutdown_with_busy_writer` CPU doyuran bir çocuk bırakıyor** — phase-2b'nin
  sınaması, bu diff'te değil (`git show HEAD` ile doğrulandı).
- **Sınır dolduğunda thread + fd + çocuk sızıyor** — phase-2b'nin kayıtlı
  borcu, üç yerde yazılı; çözümü `bt-core`'un kapanış tasarımı.
- **`IDLE_FRAME_LIMIT` gürültüyü emmek için genişletildi** (altitude, iki
  koşucu) — önerilen kalıcı çare "geometri yolundan gelen kareleri sayaç
  dışında tutmak". **Ölçüm bunu çürütüyor:** oynama sırasında `istek` sabit
  kaldı, yani fazladan kare fazladan **talepten** gelmiyor ve geometri
  kancalarını saymamak bu oynamayı kapatmazdı. Çare `resize` yanlış
  pozitifini kapatır (kayıtlı), bu oynamayı değil.
- **Ölçüm sayıları dört ayrı doc'ta, `docs/OLCUMLER.md` ise yok** — phase-2b'nin
  `/audit`'i bu deseni açıkça onayladı: bir `const`'u gerekçelendiren sayı o
  `const`'un yanında yaşar; `docs/OLCUMLER.md` **başarım iddialarının** sahibi
  ve R7.3 gereği bu sette yazılmıyor.

---

### `/audit` — on mercek elendi, üçü fan-out

**İlgisiz (sebebiyle):** 4 (`settings.rs`/tema diff'te yok), 5 (`assets/shell/`
el değmedi), 9 (`.metal` ve `build.rs` el değmedi; `repr(C)`, `Cell` ve boyut
assert'i dokunulmadı — `bt-gpu`'da yalnız Rust tarafı).

**Mekanik, inline koşuldu — dördü de temiz:** 1 (katman/platformsuzluk:
`cargo tree -p bt-core` ve `-p bt-atlas` temiz, `bt-gpu → bt-shell` kenarı
yok, `crates/bt-core/src` içinde `objc2|core_text|core_graphics` yok),
2 (`Cargo.toml` ve `Cargo.lock` diff'te **yok**), 3 (`bt-core`'un eklenen
satırlarında `unwrap`/`expect`/`panic!` yok), 6 (kalıcı belgelerin hiçbiri
diff'te değil; ölçülmemiş başarım iddiası taraması boş).

**Yargı, üç ajan paralel (`opus`):**

- **Mercek 7 (thread ve blokaj): temiz.** Render yoluna giren tek yeni kod iki
  `Relaxed` atomik; sıralayan/ayıran iki fonksiyon (`Ring::snapshot`,
  `p95_and_worst`) yalnız `report_and_exit`'ten, `link.stop()` sonrası
  çağrılıyor. Yeni AppKit çağrısı yok, yeni kilit yok, `recv_timeout`
  beklemesine kilit tutularak girilmiyor. `thread::Builder`'a geçiş bu mercek
  için **iyileştirme**. Bir yorum yanlışı yakalandı ve düzeltildi: bekçinin
  doc'u OS thread sınırı dalında "kesen taraf biziz" diyordu, oysa o koşulda
  bekçinin **kendi** thread'i de kurulamaz.
- **Mercek 8 (boşta sıfır kare): temiz.** Diff yeni animasyon/zamanlayıcı
  eklemiyor; `wake`/`request_frame`/`setPaused` çağrısı eklenmiyor (sayaç
  talebi **sayıyor**, üretmiyor); `ShutdownPanicked` kolu `ExcessFrames`'ten
  **sonra** duruyor, yani panik boşta-kare kapısını maskeleyemiyor. `8`'in
  gerekçesi ölçümle örtüşüyor ve iki uç sınamada pinli. İki not kayda geçti:
  (a) sınır genişleyince kapının algılama tabanı ~0,7 Hz'den ~2,7 Hz'e çıktı —
  bu diff'te öyle bir animasyon yok ama motion seti tam bu şekilde gelecek;
  (b) kısılmış rejimde bozuk bir duman ~12 kare eder, yani pay 6 kat değil
  **1,5 kat** — sınırı buradan yükseltmemenin sebebi. İkisi de sabitin
  doc'una ve phase-3b'ye yazıldı.
- **Mercek 10 (belge ve üslup): dört bulgu, dördü de düzeltildi.**
  (1) `WATCHDOG_BUDGET`'in doc'u basılmayan bir jeton değeri anıyordu
  (`kapanis=asildi` → `abandoned`); (2) `Teardown`'un doc'u "beş sonuç"
  diyordu, `ReaderPanicked` ile altı oldu — sayı ve hangilerinin arıza olduğu
  düzeltildi; (3) jeton dili kuralı yalnız `teardown_token`'da yazılıydı ve
  üç noktada yanlıştı (`repeated` bir varyant adı değildi, `off`/
  `insufficient`/`none` gerekçesizdi, "tek örnek `yuk=`" yanlıştı —
  `pipeline=ok` daha eski) → kural `Report::token_line`'ın doc'una taşındı ve
  `repeated` → `already-done` oldu; (4) `SHUTDOWN_GRACE` `pub` olmuştu ama
  doc'u tüketicisini söylemiyordu. Ayrıca `bt-gpu`'nun başlık yorumu
  `MIN_SAMPLES`'ın sahipliğini anıyor.

  **Sorulan noktanın cevabı:** Türkçe anahtar / İngilizce değer ayrımı
  **savunulabilir ve yeni değil** — `pipeline=ok` ile `yuk=smoke|load` deseni
  bu satır doğmadan kurmuştu. Diff'te stdout jeton satırında tek Türkçe değer,
  stderr'de tek İngilizce cümle yok. Kusur uygulamada değil **belgelenişinde**
  idi ve o düzeltildi.

---

### Ölçülenler (2026-09-12, debug, bu makine)

Sayı iddia değil **gözlem** ve hiçbiri taban değil: debug profili, `.app`
paketi yok. `docs/OLCUMLER.md`'ye taşınmıyorlar (R7.3).

**1. `IDLE_FRAME_LIMIT` — dayanağı çürüktü, yeniden ölçüldü, `2` → `8`.**

| koşu | n | `kare` |
|---|---|---|
| sağlıklı duman, 3 sn | 18 | on kez `1`, sekiz kez `2` |
| sağlıklı duman, 5 sn | 11 | dokuz kez `1`, bir kez `2`, bir kez **`4`** |
| sağlıklı duman, 10 sn | 2 | ikisi de `2` |
| sağlıklı duman, 5 sn (kare akışının serbest olduğu rejim) | 5 | beşi de `1` |
| bozuk duman (`needs_update` sonuna koşulsuz `wake()`), 3 sn | 7 | 82–354 |
| bozuk duman, 5 sn | 2 | 49–63 |

**Oynama bu phase'in getirdiği bir şey değil ve bu ölçüldü:** on beş koşu
(3 sn ×10, 5 sn ×5) değiştirilmemiş `854f027` üstünde de koşuldu (`git
stash`, sonra `git stash pop` ve diff'in birebir aynı olduğu doğrulandı) ve
aynı dağılımı verdi — 3 sn'de sekiz kez `2` iki kez `1`, 5 sn'de üç kez `1`
iki kez `2`. Yani eski `2` sınırı sağlıklı koşuların **çoğunun tam üstünde**
duruyordu; bir kare daha eklenmesi yetiyordu.

İki şey birden çıktı. Birincisi: **eski kapı doğru bir build'de kırmızı
düştü** — beş saniyelik bir sağlıklı koşu `kare=4` bastı ve `2`'yi aştı.
Yani phase-2b'nin uyarısı teorik değilmiş, üretildi. İkincisi: "tavan ~3
kare" tamamen öldü — bozuk koşu 3 değil **49–354** kare çiziyor. `8` iki
kutbun arasında: en yüksek sağlıklı gözlemin iki katı, en düşük bozuk
gözlemin altıda biri.

**2. Bekçi bütçesi — `run_seconds × 3` → `SHUTDOWN_GRACE + 2 sn`.**
Kapanış yolunun duvar saati ölçüldü (`/usr/bin/time`, altı koşu — beşi temiz,
biri `asildi`; kapanış yolunda kalıcı `Instant::now()` **eklenmedi**,
phase-2b'nin kararı duruyor): temiz kapanışlarda toplam süre koşu süresini
~0,18 sn aşıyor (**içinde açılış da var**), sınırın dolduğu koşuda
**+0,49 sn**. Yani ölçülen tavan
`SHUTDOWN_GRACE`'in kendisi ve hiçbir şey koşu süresiyle ölçeklenmiyor.
Yeni bütçe 2,5 sn = ölçülen tavanın **beş katı**; eski bütçe 3 sn'lik duman
için 9 sn, 60 sn'lik bir ölçüm koşusu için **3 dakika** veriyordu. Yeni
bütçeyle on ölçüm koşusu koşuldu, **hiçbiri bekçiye düşmedi**.

**3. GPU sütununun uzunluğu — bu makinede CPU'dan kısa değil.**
Yedi ölçüm koşusunda `gpu_ornek == ornek` ve `gpu_elenen=0`. Yani Metal
sıfır damga vermiyor; sütun başına ayrı jeton yine de duruyor, çünkü
elemenin **meşru** olduğu `record_gpu`'nun sözleşmesinde yazılı ve başka bir
donanımda görünmesi gerek.

**4. Boşta ölçütün derinliği — `istek=` iki rejim gösteriyor.**

| koşu | `kare` | `istek` |
|---|---|---|
| sağlıklı duman 3 sn | 1–2 | 2–3 |
| bozuk duman 3 sn | 82–354 | 84–357 |
| ölçüm yükü 2 sn | 9 (4/4) | 25 000–30 000 |
| ölçüm yükü 5 sn | 21 (3/3) | 70 000–72 600 |

Duman yükünde `istek ≈ kare + 2` ve **`kare` oynarken `istek` sabit
kalıyor** (1↔2'ye karşı 2–3): yani sağlıklı koşudaki fazladan kare fazladan
**talepten** gelmiyor — mekanizması ölçülmedi, geometri/örtülme kancaları
olsaydı `istek` de artardı.
Ölçüm yükünde ise **üç mertebe** ayrışıyorlar: kare akmıyor, talep akıyor.
Bu, sayacın var olma gerekçesinin gözlenmiş hâli — ama kapı yapılmadı:
eşiği ölçülmedi ve iki rejimin **mekanizması da ölçülmedi** (kapı mı
yutuyor, ana thread mi doyuyor, sistem mi link'i kısıyor). Dışarıdan
gözlenen iki sayı yazıldı, adı varsayılmadı.

> **İki rejim de aynı binary'de görüldü.** Ölçüm yükü 5 sn önce `kare=21`
> verdi, oturumun sonunda **`kare=597`** — phase-2b'nin `594`'üyle aynı
> mertebe, aynı komut, aynı derleme. Yani phase-2b ile buradaki fark bir
> ölçüm hatası değil, makinenin o anki durumu; en olası değişken pencere
> görünürlüğü ama **nedensellik doğrulanmadı** (aradaki farkı ölçen bir
> kanca yok).
>
> Sonucu etkilemiyor ve iki yönden: bozuk duman koşusu kısıtlı rejimde bile
> 49–354 kare çiziyor, görünür rejimde daha da fazla olurdu — `8` her iki
> hâlde de yakalar. Sağlıklı duman ise görünür rejimde de `kare=1` bastı.
>
> Yan fayda: `kare=597`'lik koşu tabanı aştığı için **p95 jetonları ilk kez
> gerçek sayı bastı** — `cpu_kare_p95=0.82ms cpu_kare_max=2.12ms
> cpu_encode_p95=0.86ms cpu_encode_max=2.47ms gpu_p95=0.24ms gpu_max=1.63ms
> acilis=293.64ms`, `ornek=597 dusen=0 gpu_ornek=597 gpu_elenen=0`. Sayılar
> **taban değil** (debug profili); buradaki işleri borunun uçtan uca
> çalıştığını göstermek.

**5. `kapanis=` jetonu ilk koşuda işini yaptı.** Ölçüm yükünün on yedi
koşusunun **dördünde** `kapanis=asildi` çıktı (~%24; phase-2b 2/8 ölçmüştü).
Bu koşular eskiden **yeşil bir jeton satırıyla** geçiyordu ve sınırın
dolduğu yalnız stderr'de görünüyordu.

---

## Yayın Etkisi

- **Duman sözleşmesi korundu, ölçüldü:** `make duman` →
  `kare=1 hucre=8 glif=6 kural=15 yuva=13/2048 yuk=smoke istek=2
  kapanis=temiz profil=debug ornek=kapali pipeline=ok`, çıkış 0. İlk dört
  jeton bit bit aynı; yenisi **eklendi**, hiçbiri silinmedi. `Smoke` yükünde
  ölçüm jetonları yok (kapı kapalı) ve `ornek=0` yerine `ornek=kapali`
  (sapma, gerekçesi notlarda).
- **Jeton değerleri İngilizce** (`kapanis=clean`, `ornek=off`,
  `*_p95=insufficient`, `acilis=none`); jeton **adları** Türkçe kaldı.
  Gerekçe notlarda.
- **Yeni kapı kolu:** kapanış yolunda panik (`ReaderPanicked`/`Panicked`)
  artık `pipeline=ok` basamıyor, çıkış 1. `Abandoned`/`Unbounded` bilerek
  kapıya bağlanmadı.
- **Hata satırı değişti:** boşta sıfır kare kapısı düştüğünde stderr artık
  kare talebini de yazıyor. Jeton değil, tanı metni — `make duman`'ı okuyan
  taraf için ek bilgi, sözleşme için değişiklik yok.
- **`IDLE_FRAME_LIMIT` `2` → `8`** ve bu bir davranış değişikliği: `make
  duman` artık sekiz kareye kadar yeşil geçiyor. Gerekçe ölçüm, notlarda;
  eski sınır doğru bir build'i kırmızıya düşürüyordu.
- **Bekçi bütçesi `run_seconds × 3` → sabit 2,5 sn.** Uzun ölçüm koşuları
  artık dakikalarca beklemiyor; kısa koşular için pay ölçülen tavanın beş
  katı.
- **Belge: bu phase'de hiç dokunulmadı** — hepsi phase-3b'de (kanca adları,
  `CLAUDE.md`'nin bench borcu ve `make duman` satırı, `/measure` skill'i,
  `context.md`, `proje.md`, `.tasks/README.md`). Sebebi sıra: yukarıdaki
  dört yeniden ölçüm iki sabiti değiştirdi ve aynı commit'te yazılan belge
  bayat doğardı.
- **`docs/OLCUMLER.md` yazılmadı** — ilk `/measure` kuracak.
- **Ölçüm bekliyor: yok.** Dört devredilen ölçümün dördü de koşuldu ve
  sayıları yukarıda; sabitlerin ikisi bu ölçümlerle değişti.
- Yeni bağımlılık yok, `.metal`/`build.rs`/`assets` el değmiyor.
- **Kapanan iddialar:** kare süresi iddialarının tamamı, açılış/ölçek
  iddiaları ve atlas doluluğu (phase-1). **Açık kalan:** 003 #1 ve 003 #2'nin
  bench yarısı — kayıtlı ve gerekçeli.

---

## Checklist

- [x] **phase-1'den devir:** başarı satırı `9788d95` ile iki jeton kazandı — `yuva=U/T` **ve** `yuk=smoke|load`. `plan.md`'nin Akış şeması `yuk=`'ü göstermiyor (phase-1'de, `/code-review` bulgusu üzerine eklendi): oradaki satırı olduğu gibi kopyalayan bir `println!` jetonu **sessizce düşürür** ve sözleşme "silinmez, eklenir" der. Rapor genişlerken ikisi de korunacak; korunduğunu `make duman` çıktısında gözle doğrula
- [x] `p95_and_worst` — ortalama **yok**; boş/az örnekte davranışı tanımlı
- [x] Jetonlar `report_and_exit`'te `println!` ile **açıkça** basılıyor; `Drop`'a güvenen yol yok
- [x] `ornek=` jetonu — düşen örnekler dâhil (`dusen=`)
- [x] `profil=` jetonu (`cfg!(debug_assertions)`)
- [x] `acilis=` tek sayı olarak
- [x] **R5.6 örnek tabanı** — taban altında p95 **basılmaz**, `ornek=` yerine
      gerekçe çıkar. **Gerekçe düzeltildi:** orkestratörün "örtülü pencere
      display link'i askıya alıyor, tavan 3 kare" saptaması phase-2b ile
      çürüdü — aynı pencere durumunda `kare=594` (5 sn). Düşük sayının sebebi
      kapanış kilitlenmesinin kök nedeniydi. Taban gereksinimi ilkeye dayanır:
      az örnek üstünden p95 anlamsızdır. `.app` paketi ve öne getirilmiş
      pencere şartı **kalktı** — bu makinede ölçüm bugün de anlamlı
- [x] **phase-2'den devir — okuma API'si.** Halka `bt-gpu`'da ve `bt-shell`'in elinde bir `Arc<Stats>` var (`AppDelegate` ivar'ı, `report_and_exit` oradan okur). Yüzey: `Stats::startup() -> Option<Duration>` ve sütun başına `cpu_frame()` / `cpu_encode()` / `gpu() -> Samples { nanos: Vec<u64>, dropped: u64 }` — değerler **nanosaniye**, eskiden yeniye. p95 bu üç sütunun her birinden ayrı hesaplanır; `Duration` isteyen bir `p95_and_worst` imzası `from_nanos` ile besleniyor
- [x] **phase-2'den devir — GPU sütunu CPU'dan kısa olabilir.** `GPUStartTime`/`GPUEndTime` Metal'de sıfır dönebiliyor ("başlamadı" / "bildirim gelmedi") ve o kare **hiç örnek yazmıyor** (0 ns p95'i aşağı çeker). `ornek=` jetonu bu yüzden tek sayı olamaz: ya sütun başına verilir ya da en küçüğü basılıp hangi sütun olduğu söylenir — **karar phase-3'ün**. Düşen örnek (`dropped`) de aynı jetonun içinde görünmeli
- [x] **phase-2'den devir — `BT_FRAME_STATS` süre ister.** `main.rs` `BT_SCROLL_TEST`'in eşi bir kolla süresiz (ve sıfır saniyelik) ölçümü eliyor: çıkış 1 + *"sıfırdan büyük bir BT_RUN_SECONDS ister"*. Sebebi rapor yolu: `report_and_exit` yalnız deadline ile koşuyor, yani süresiz ölçüm hiç basılmazdı. Belge güncellemesi (R7.2) kanca adını yazarken bu şartı da yazmalı
- [x] **phase-2'den devir — halka kapasitesi.** `Stats::new(since, run_seconds)` içinde: `run_seconds × MAX_REFRESH_HZ` (120, bir **ayırma tavanı**; ölçülmüş tazeleme hızı değil), mutlak tavan `MAX_CAPACITY` (~10 dk). Kapı kapalıyken **hiç ayrılmıyor**. Sabitler `bt-gpu/src/stats.rs`'te ve rapor bunlara değil `dropped`'a bakmalı
- [x] **phase-2'den devir — R5.6'nın ölçüm koşusu 5 saniyeyle koşulamıyor** → **phase-2b kapattı**, kutu artık bir *bilgi*: kök neden (`Session::shutdown`'ın sınırsız beklemesi) `bt-core`'da sınırlandı ve ölçüldü — `BT_SCROLL_TEST=1 BT_RUN_SECONDS=5` dört koşuda dördü temiz (`kare≈592–595`, jeton satırı basılıyor), `BT_RUN_SECONDS=2` sekiz koşuda sekizi temiz (öncesi 5/8 asılma). Yani R5.6'nın ölçüm koşusu **5 saniyeyle koşulabilir** ve phase-3 "asılan koşu atılır" demek zorunda değil
- [x] **phase-2'den devir (phase-1 devir 3) — boşta sıfır karenin daha derin ölçütü.** Bekçi bugün **GPU karesini** sayıyor (`kare`), yani örtülme ve display link'in askıya alınması onu köreltiyor (tavan 3). Daha derin ölçüt **kare talebi**: `Waker::wake` / `request_frame` sayısı — bizim tarafımızda, örtülmeden etkilenmez ve bilerek bozulmuş boşta-sıfır-kareyi anında yakalar. Yeni sayaç **ve yeni jeton** demek; jeton basmak bu phase'in işi
- [x] **phase-2b'den devir — `IDLE_FRAME_LIMIT`'in dayanağı ÇÜRÜDÜ, kapı yanlış bir sayıyla ölçülü.** `app.rs:61`'in doc'u "`Workload::Load` ile `BT_RUN_SECONDS` 3, 6, 10 → hep `kare=3` … sistem display link'i askıya alıyor — tavan ~3" diyor ve `load(3, 0, 1836, 0)` sınaması bunu "gerçek sayılar" diye pinliyor. Ölçüldü (phase-2b, iki ayrı koşucu, aynı makine): `BT_SCROLL_TEST=1` ile **`kare=232` (2 sn), `kare=352` (3 sn), `kare=593` (5 sn)** — yani tavan yok, örtülme o koşularda olmuyor. `CLAUDE.md`: *"ölçülmemiş sayı yazılmaz"*; burada **çürütülmüş** bir sayı bir kapıyı boyutlandırıyor. Sonuç iki yanlı: (a) `Load` sınamasının beklediği `kare=3` artık gerçeği anlatmıyor, (b) tavan olmadığı için `Smoke`'u `kare=1`'de tutan tek şey boşta sıfır kare ve her meşru uyandırma (`windowDidChangeOcclusionState:` → `request_frame`, `windowDidResize:` → `resize` → koşulsuz `request_frame`) bir kare ekliyor: üç saniyelik gözetimsiz bir koşuda iki böyle olay `make duman`'ı doğru bir build'de kırmızıya düşürür ve mesaj yanlış kusuru gösterir (resize yarısı bilinen yanlış pozitif diye yazılı, örtülme yarısı **değil**). Karar phase-3'ün: sabitin doc'u ölçüme göre yeniden yazılır, kapı ya ölçülmüş bir üst sınıra ya da "kare talebi" ölçütüne (aşağıdaki devir) bağlanır
- [x] **phase-2b'den devir — `kapanis=` jetonu.** Kapanış artık sınırlı, ama sınır dolan koşuda çocuk arkada bırakılıyor ve o koşu bugün **yeşil bir jeton satırıyla** geçiyor (stderr'de bir satır var, jeton satırında iz yok; ölçüldü: 8 koşuda 2–3). Jeton sözleşmesi "silinmez, **eklenir**" — `kapanis=temiz|asildi` (ya da benzeri) `report_and_exit`'e eklenirse `make duman` ve `/measure` bunu görebilir. `bt-core` tarafında bir yüzey gerekiyor: `Session::shutdown` bugün sonucu **döndürmüyor**, yani ya dönüş tipi ya da bir sorgu eklenecek — `bt-core` değişikliği olduğu için burada karar, orada uygulama
- [x] **phase-2b'den devir — `/code-review`'un ölçüm yolu bulguları** (dördü de phase-1/phase-2 kodunda, phase-2b kapsamı dışı): (1) `stats.rs:182` `record_gpu` elenen örneği **saymıyor** — sıfır damgalı bir donanımda `gpu()` boş dönüyor ve bu "hiç kare çizilmedi"den ayırt edilemiyor; R5.2'nin kapatmak istediği tam bu körlük, `rejected: AtomicU64` aynı bedelle kapatır. (2) `renderer.rs:1151` `completion_hands_over_live_gpu_timestamps` donanımın sıfır dönmesini `make hepsi`'nin kırmızısına çeviriyor, oysa `record_gpu`'nun kendi doc'u sıfırı **meşru** sayıyor. (3) `app.rs:367` `verdict` dört konumsal sayaç alıyor (üçü aynı tip): `hucre`/`glif`/`kural` yer değiştirse derleme geçer, sınama da aynı sırayı kullandığı için ikisi birlikte yanılır — `report_and_exit` bu phase'de zaten genişliyor, isimli alanlı bir yapı sınıfı siler. (4) `app.rs:592` `Workload → "load"/"smoke"` eşlemesi çağrı yerinde duruyor; jeton makine sözleşmesi olduğuna göre dizginin yeri tipin yanı (`fn token(self) -> &'static str`)
- [x] **phase-2b'den devir — bekçi bütçesi (`run_seconds × 3`) artık koruduğu şeyle ilgisiz.** Kapanış `bt-core`'da sınırlandığı için bekçinin kapsamı "kapanış yolunun *başka* asılmaları"na daraldı ve bunların hiçbiri koşu süresiyle ölçeklenmiyor: 3 saniyelik duman 9 saniye, 60 saniyelik bir ölçüm koşusu **3 dakika** bekliyor. Doğru ölçü `SHUTDOWN_GRACE` + sabit bir pay; sayı **ölçülmeden** değiştirilmedi, `lib.rs`'in yorumu bugünkü durumu söylüyor
- [x] Test: `few_samples_suppress_p95` — taban altında sayı basılmıyor
- [x] Test: `p95_returns_none_on_empty_ring`
- [x] Test: `token_line_preserves_old_tokens` — beş eski jeton ve `pipeline=ok` yerinde
- [x] Test: `smoke_counts_unchanged` — `kare=1 hucre=8 glif=6 kural=15` bit bit aynı
- [x] Doğrulama geçti (`proje.md` → Doğrulama; `make duman` **zorunlu**)
- [x] `/simplify` çalıştırıldı, bulgular uygulandı — `Skill` ile, dört mercek ajanı paralel; dördü de döndü, dört bulgu uygulandı, biri (test-yerel konumsal kapanışlar) gerekçesiyle bırakıldı
- [x] `/code-review` çalıştırıldı, bulgular giderildi — `Skill` fork'u koştu ama geç döndü; `proje.md`'nin iniş sırası gereği beklerken **ikinci basamak** da koşuldu (`code-reviewer` subagent'ı) ve ikisi de rapor verdi. Altı bulgu uygulandı (ikisi YÜKSEK: panikleyen okuyucu `clean` diyordu, `watchdog()` panikleyip süreci abort ettirebiliyordu), altısı gerekçeli waive
- [x] `/audit` çalıştırıldı, bulgular giderildi — `Skill` ile; 4/5/9 ilgisiz (kanıtıyla), 1/2/3/6 inline ve temiz, 7/8/10 fan-out (`opus`). 7 ve 8 temiz (birer yorum/kayıt notuyla), 10'un dört bulgusunun dördü düzeltildi
- [x] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
