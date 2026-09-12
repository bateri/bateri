# Glyph ve atlas — Doğrulama & Yayın

> İlgili: [plan.md](plan.md) · [phase-1.md](phase-1.md) · [phase-2.md](phase-2.md) · [phase-3.md](phase-3.md) · [phase-4.md](phase-4.md)

Kullanıcı tarafında değişen tek görünür şey pencerenin kendisi: 002'nin
körlemesine yazma dönemi bitti, ekranda okunabilir metin var. Yolun tamamı
depo içinde kaldı — yeni ayar anahtarı, yeni terminfo, yeni bundle adımı yok
ve `Cargo.lock` yalnız phase-2'de, kullanıcı onaylı bir kararla iki crate
büyüdü. Dışarıya bakan tek sözleşme değişikliği `make duman`'ın çıktısı:
jeton **eklendi** (`glif=G`), hiçbiri silinmedi.

## A. Doğrulama (ÖNCELİK)

```sh
make hepsi
make shader
make duman
make test-yaris
```

`make shader` bu sette **zorunlu**: phase-4 yeni bir `.metal` ekledi
(`crates/bt-gpu/shaders/cell.metal`). `make test-yaris` de zorunlu:
`Session::frame`'in gövdesi `Term` kilidi altında değişti.

`make terminfo` ve `make kur` **koşulmaz** — girdileri henüz yok
(`proje.md`'nin bilinen listesi). Bu atlanmış kapı değil, koşulu doğmamış kapı.

### Beklenen çıktı

- `make hepsi` → çıkış 0 (rustc sürümü + fmt + clippy `-D warnings` + test).
- `make shader` → çıkış 0. MSL `static_assert`'leri burada koşuyor: iki
  `.metal`'in `GlyphInstance`/`Instance` düzeni Rust tarafındaki
  `offset_of!` assert'leriyle ayrışırsa derleme kırılır.
- `make duman` → `kare=N hucre=8 glif=6 pipeline=ok`. `N` tazeleme ritmine
  bağlı (kapı `> 0`); `8` ve `6` sabit shell betiğine bağlı ve
  `bt_core::smoke_shell`'in sahibi (`sabit_shell_arka_plan_hucreleri_verir`,
  `sabit_shell_alti_glif_verir`).
- `make test-yaris` → çıkış 0, iki zamanlama profili de.

Ölçüm değişmedi: `docs/OLCUMLER.md` bu sette **oluşmadı**, çünkü hiçbir sayı
ölçülmedi. Bekleyen iddialar B.2'de.

### Doğrulama Checklist

- [x] `make hepsi` yeşil
- [x] `make shader` yeşil (`cell.metal` yeni)
- [x] `make duman` → `kare=1 hucre=8 glif=6 pipeline=ok`
- [x] `make test-yaris` yeşil
- [~] `make terminfo` / `make kur` — girdisi yok, koşul doğmadı

## B. Yayın (doğrulamadan SONRA)

### B.1 Ölçüm `[komut]`

Beş iddia ölçüm bekliyor; hiçbirine sayı **uydurulmadı**:

1. **phase-1** — `#[inline]` işaretlerinin hücre başına renk yolundaki etkisi
   (`lineer_rgba`/`resolve`/`default`/`dim`). Satır içine alındığı sembol
   tablosuyla doğrulandı, kazancın büyüklüğü ölçülmedi.
2. **phase-2** — `Atlas::slot`'un kare başına hücre başına maliyeti (harita
   araması + `slot_origin`'deki bölme). Phase-4 atlası çizim yoluna bağladı,
   yani artık anlamlı.
3. **phase-2** — atlas doluluğu; `occupancy()` sayacı bunun için var.
4. **phase-4** — ilk karede ve ekran ölçeği değişiminde atlas + doku
   kurulumunun **ana thread'deki** bedeli. `Atlas::new` font zincirini açıyor,
   yanına bir doku ayırması ve tofu yüklemesi eklendi.
5. **phase-4** — ikinci pipeline'ın ve kare başına glyph instance tamponunun
   kare süresine etkisi.

```sh
/measure 003-glyph-atlas
```

Sonuç `docs/OLCUMLER.md`'ye girer — başka hiçbir belgeye sayı yazılmaz.
Ölçüm sonrası yeniden bakılacak iki karar da bunlara bağlı: `replaceRegion`
yerine staging + blit (aşağıda B.3) ve atlas dokusunun `Shared` depolaması.

#### 2026-09-10 — koşuldu, sonuç: **ölçüm aracı yok**

`/measure 003-glyph-atlas` koştu ve sayı **üretmedi**. Beş iddianın hiçbiri
bugünkü depoda ölçülebilir değil; eksik olan ölçüm değil **kanca**:

| # | iddia | gereken kanca | durum |
|---|---|---|---|
| 1 | `#[inline]` renk yolu | `cargo bench -p bt-core` | bench hedefi yok |
| 2 | `Atlas::slot` maliyeti | `cargo bench -p bt-atlas` | bench hedefi yok |
| 3 | atlas doluluğu | `occupancy()`'yi raporlayan yol | sayaç var, raporlayan yok |
| 4 | ilk karede atlas + doku kurulumu | `BT_STARTUP_TRACE` | kanca yok |
| 5 | ikinci pipeline'ın kare süresine etkisi | `BT_FRAME_LOG` + `BT_SCROLL_TEST` | kanca yok |

Kanıt: `cargo bench --workspace -- --list` → **`0 benchmarks`**; `benches/`
dizini yok; `BT_FRAME_LOG`/`BT_SCROLL_TEST`/`BT_STARTUP_TRACE`/
`BT_INPUT_LATENCY_SAMPLES` depoda **yalnız belgelerde** geçiyor, hiçbir
`env::var` çağrısında değil (crate'lerdeki üç çağrı:
`MACOSX_DEPLOYMENT_TARGET`, `OUT_DIR`, `BT_RUN_SECONDS`). `occupancy()`'nin
tek çağıranı `bt-atlas`'ın kendi sınamaları.

`xcrun xctrace` da geri düşüş **değil**: boşta sıfır kare kuralı gereği sabit
duman betiğinde `kare=1` çıkıyor, yani örneklenecek bir dağılım (p95, en kötü
kare) ve tekrarlanabilir bir taban koşusu yok — iş yükü kancası olmadan
profiler boş bir zaman çizelgesi kaydeder.

`docs/OLCUMLER.md` bu yüzden **oluşturulmadı**: dosyanın ilk iki bölümü
(`## Yöntem`, `## Nasıl yeniden ölçülür`) kancaların biçimine bağlıdır ve
onları kanca yokken yazmak yöntemi uydurmak olurdu. Dosyayı kancaları ekleyen
set kurar.

Bu, 001 phase-3'ün öngördüğü durum: *"Kare süresi ölçümü `BT_FRAME_LOG`
kancasıyla gelir; bu sette kanca yok, `/measure` 'ölçüm aracı yok' der."*
Kutu `[ ]` kalıyor — ölçüm bir kapı değil, atlanmış da değil. **005 sonrası güncellendi:** kanca seti geldi (`BT_FRAME_STATS`, `BT_SCROLL_TEST`, açılış damgası) ve `/measure` artık sayı üretebiliyor. Tarihli kayıtlar o gün doğruydu, dokunulmadı.

### B.2 Göz kontrolü `[elle]`

İki şey hiçbir otomatik kapının göremediği yerde:

1. **`CAMetalLayer.colorspace` `nil`** (phase-1'den devir). Format sRGB
   etiketli, katman etiketsiz; saklanan baytların değişmediği kanıtlandı ama
   **kompozit edilmiş pikseli** hiçbir sınama görmüyor. P3 ekranda renklerin
   doygunlaşıp doygunlaşmadığı gözle bakılır.
2. **Glyph'lerin gerçekten okunabilir olduğu.** `glif_hucrenin_icini_arka_planindan_ayirir`
   "hücrenin içi arka planla tekdüze değil" ve "iki yuva iki farklı şey
   çiziyor" diyor — harfin doğru yerde ve doğru boyda olduğunu **söylemiyor**.
   Ters bir taban çizgisi ya da bir piksellik kayma ancak gözle görülür.

```sh
make duman   # pencere 3 saniye açık kalır
```

Bakılacak: ilk satırda kırmızı zemin üstünde ` bateri `, imlecin altındaki
karakterin okunur kalması, harflerin kenarında saydamlık olmaması.

### B.3 Bilinen sınırlar — kayıt `[oto]`

Yapılacak bir şey yok, kayda geçiyor:

- **`replaceRegion` uçuşta okunan dokuya yazıyor.** Nadir ve tek karelik:
  hiç görülmemiş bir karakter, önceki kare hâlâ koşarken yüklenirse o karede
  bozuk çizilebilir. Doğru biçimi staging tamponu + aynı komut tamponunda
  blit encoder'ı, ya da uçuştaki kare sayısını semaforla sınırlamak; ikisi de
  kare ritmine dokunuyor ve 002'nin "üçlü tamponlama reddedildi" kararıyla
  aynı masada. Gerekçe `AtlasDoku::hazirla`'nın doc'unda.
- **Geniş karakterin glyph'i tek yuvaya kırpılıyor.** Atlas sabit yuva
  ızgarası ve yuva bir hücre boyunda; CJK bir karakter iki hücre kaplıyor.
  Arka plan iki hücreyi de kaplıyor, harf yarım kalıyor. Düzeltmesi atlas
  biçimini değiştirir.
- **Atlas dokusu `MTLStorageMode::Shared`.** Yükleme yolu `replaceRegion`,
  yani CPU doğrudan yazıyor. Ayrık GPU'lu makinede bedeli örneklemede bir
  kopya; başarısızlık **sesli** (`GpuError::NoAtlasTexture`), sessiz değil.
- **Ölçeğin `bt-gpu`'ya iki kapısı var** (`Surface::set_size` ve
  `cell_metrics`, `app.rs`'te komşu iki satır). Birleştirmek R5'in harfiyle
  çelişiyor; 004'e devredildi.

### B.4 Bağımlılık kaydı `[oto]`

`Cargo.lock` bu sette **iki crate** aldı: `objc2-core-text` 0.3.2 ve
`objc2-core-graphics` 0.3.2, ikisi de phase-2'de ve kullanıcı onaylı bir
kararla (`discussion.md → ## Karar`; servo ailesi `core-text` reddedildi,
ikinci bir CF sarmalayıcı yığını olurdu). phase-1, phase-3 ve phase-4
`Cargo.toml`/`Cargo.lock`'a **hiç dokunmadı**.

### Yayın Checklist

<!-- `/ship` bekleyen manuel adımları BU başlık altında arar. -->

- [ ] B.1 `/measure 003-glyph-atlas` `[komut]` — beş ölçüm bekliyor.
      **005 kanca setini getirdi.** 2026-09-10'daki "ölçüm aracı yok" sonucu o
      güne aitti. Kapanabilenler: `#3` (atlas doluluğu → `yuva=` jetonu), `#4`
      ve `#5` (açılış damgası ve kare sütunları). **Açık kalan:** `#1` ve `#2`
      — ikisi de `cargo bench` istiyor ve bench seti henüz yok
- [x] B.2 Göz kontrolü `[elle]` — 2026-09-10, kullanıcı baktı: harfler hücrelerine
      oturuyor, kenarlarda saydamlık yok, kırmızı zemin doğru. `colorspace` `nil`
      kalıyor: P3 ekranda gözle sapma görülmedi
- [x] B.3 Bilinen sınırlar `[oto]` — kod yorumlarında ve phase notlarında yazılı
- [x] B.4 Bağımlılık kaydı `[oto]` — `Cargo.lock` depoda, gerekçe `discussion.md → Karar`

## Geri Alma

- **Kod:** dört phase, dört commit — `b5a0585` (sRGB), `ae50826` (`bt-atlas`),
  `2f19277` (metrik geçişi), `3c91814` (glyph). Sırayla revert edilebilir; her
  biri tek başına `make hepsi` yeşil bırakacak şekilde kesildi. Aralarındaki
  üç damga commit'i (`6dc5256`, `9855239`, `aa2e1d8`) yalnız `.tasks/` defterine
  dokunuyor.
- **`make duman` sözleşmesi:** `kare=N hucre=K pipeline=ok` →
  `kare=N hucre=K glif=G pipeline=ok`. Jeton **eklendi, silinmedi**; `kare=`
  ya da `hucre=` arayan bir okuyucu bozulmaz. Geri alınırsa `glif=` kaybolur
  ve onu arayan taraf uyarlanmalı.
- **Görünür geometri:** phase-3 hücre boyutunu yer tutucudan gerçek font
  metriğine geçirdi, yani pencerede görünen sütun/satır sayısı değişti.
  `TIOCSWINSZ` onu izliyor; geri alınırsa eski sayıya döner.
- **Belge:** `CLAUDE.md`, `.claude/is-akisi/proje.md` ve `Makefile`
  değişiklikleri ilgili commit'lerin içinde, ayrı geri alma istemez.
- **Türetilmiş dosya yok:** `default.metallib` `target/` altında, depoya
  girmiyor. Ayar dosyası, tema ve terminfo el değmedi.
