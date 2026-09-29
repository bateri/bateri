# Phase 1 — Tarama ve bekçi

## Özet

Bugünkü kapıyı sembol ve emoji aralıklarında koşturan bir tarama ve gerçek
araçların karakterleri için bir kutu bekçisi. Çizim değişmez.

_Requirements: R1, R1.1, R1.2, R2, R2.1_

## Değişiklikler

- **`crates/bt-atlas/src/font.rs`** (ya da ayrı bir `census` test modülü):
  karakter başına sınıflama yapan saf olmayan bir yardımcı. Dönüşü dört
  kollu bir enum'dur: `InBase`, `Fallback { font, ratio }`,
  `NoFont`, `Rejected { font, ratio }`. Kapının adımlarını (`glyph_index`
  → `for_string` → `accept`) **aynen** kullanır ve ikinci bir kapı yazmaz.
  Oran = mürekkep genişliği / kutu. Taşma solda da olabilir, yani sol ve
  sağ taşmanın büyüğünden türetilir. Tanım yardımcının doc'una yazılır.
- **Tarama sınaması** (`#[ignore]`): aralıklar şunlardır: Arrows,
  Mathematical Operators, Misc Technical, Geometric Shapes, Misc Symbols,
  Dingbats, Misc Math Symbols-A/B, Supplemental Arrows-A/B, Misc Symbols and
  Arrows, emoji blokları (1F300–1FAFF) ve Nerd Font PUA (E000–F8FF).
  Yordamsal aralıklar (`raster::is_procedural`) atlanır. Dört punto/ölçek
  birleşimi için grup sayıları ile `Rejected`'ların oran histogramı ve
  font başına dökümü basılır. `BT_SCAN_FONT` taban aileyi değiştirir.
- **Bekçi sınaması**: `const` karakter listesi. Kaynaklar: Claude Code
  (`⏺ ⎿ ⧉ ✻ ✢ ✳ ✶ · ⏵ ⏸ ↯`), spinner'lar (`⠋ ⠙ ◐ ◓ ⣾ ⣽`), git/starship/p10k
  (`  ✔ ✘ ⇡ ⇣ ❯ ❮ ●`). Her biri 16pt @2x'te `NoFont` ya da `Rejected`
  olmamalı. Bugün dönenler ayrı bir `EXPECTED_TOFU` listesinde durur.
  Sınama, bu listedeki karakterin artık geçtiğini de yakalar ve listeden
  çıkarılmasını ister. Böylece liste yalnız küçülür.
- **`Makefile`**: `tarama` hedefi. `cargo test -p bt-atlas --release --
  --ignored census --nocapture` ya da eşdeğeri. Yorumu ne bastığını ve
  `make hepsi`'de olmadığını söyler.

## Kabul

- `make tarama` dört birleşimin tablosunu basıyor ve `⧉` 16pt'de
  `Rejected` (~1.11, Apple Symbols) olarak görünüyor.
- Bekçi `make hepsi`'de yeşil. `EXPECTED_TOFU` en az `⧉`'i taşıyor.
- Tarama özetinin Uygulama Notları'nda olması: grup sayıları,
  `Rejected` oran dağılımı (1.0–1.2 / 1.2–1.5 / 1.5–1.7 / 1.7+),
  tek sütunlu emojinin üst ucu, `.LastResort`'un oranı. Bu sayılar phase-2'nin
  sınırını belirler.

## Checklist

- [x] Sınıflama yardımcısı, kapının adımlarını paylaşarak
- [x] Tarama sınaması (`#[ignore]`) + `make tarama`
- [x] Bekçi listesi + `EXPECTED_TOFU`
- [x] Test: bekçi yeşil, `EXPECTED_TOFU`'daki bir karakter geçerse sınama kırmızı
- [x] Tarama koştu, özet Uygulama Notları'nda
- [~] make hepsi: fmt/denetim/clippy/bt-core/bt-atlas yeşil; düşen tek sınama 040'ın commit'lenmemiş wgpu_renderer::tests'i (kullanıcı onayı 2026-09-29)

## Uygulama Notları

**Sapmalar.**
- Yardımcı ve sınamalar ayrı bir test modülünde (`crates/bt-atlas/src/census.rs`,
  `#[cfg(test)] mod census`). Kapının adımları paylaşılsın diye `font.rs`'ten
  iki parça çıktı: `cascade_candidate` (`fallback_font`'un 1. adımı) ve
  `ink_fits_placed` (`ink_fits_box`'un fontsuz gövdesi); `accept`
  `pub(crate)` oldu. Kapının davranışı değişmedi.
- `Rejected` **iki** oran taşıyor (`ratio` ve `fit`), çünkü tek oran
  phase-2'yi yanıltırdı. Tanımlar aşağıda ve `classify`'ın doc'unda.
- Bekçinin iki Nerd Font karakteri phase dosyasında boşluğa dönüşmüştü
  (baytlar U+0020). `U+E0A0` (dal) ve `U+E0B0` (powerline) olarak alındı.
- Bekçi beklentiyi **olgu** olarak sabitliyor, yani makineye bağlı. Nerd
  Font kurulu bir makinede `U+E0A0`/`U+E0B0` çizilir ve bekçi "listeden çıkar"
  diye kırmızı düşer. Bilinen sınır, sınamanın doc'unda yazılı.
- Tarama karakterleri tek sütunlu soruyor (`cols = 1`), çünkü `bt-atlas`
  `unicode-width` görmüyor. Reddedilenin iki hücreye sığıp sığmadığı ayrı
  sütunda (`2h`). Karakterin ızgarada kaç sütun olduğu `bt-core`'un sorusu.

**Oranların tanımı** (ikisi de kutuya bölünüyor; kutu burada bir hücre, yani
kesirli hücre ilerlemesi):
- `ratio` = mürekkep genişliği / kutu. Mürekkebin hücreden ne kadar geniş
  olduğunu söylüyor ve yerleşimden bağımsız.
- `fit` = kapının **bugünkü yerleşimle** (`centre_shift`, sola yapışma
  dahil) geçmesi için glyph'in kaç kat küçültülmesi gerektiği. Sol ve sağ
  taşmanın büyüğünü taşıyor. Kapının kendi kuralına (`ink_fits_placed`)
  karşı ikiye bölmeyle bulunuyor.
- İkisi ayrışıyor. Küçültme ilerlemeyi de küçültüyor, ve ilerlemesi hücreyi
  hâlâ aşan glyph ortaya gelmiyor, sola yapışıyor. `⧉` bunun örneği:
  `ratio` 1.11, `fit` 1.22. 1/1.11 ile küçültülen `⧉` yeniden sınamada
  **yine döner**.

**Tarama özeti** (bu makine, taban Menlo, `make tarama`; yordamsal
aralıklar atlandı). Grup sayıları dört birleşimde aynı:

| grup | sayı |
|---|---|
| tabanda (`InBase`) | 787 |
| yedekten sığdı (`Fallback`) | 392 |
| hiçbir fontta yok (`NoFont`) | 0 |
| kapıdan döndü (`Rejected`) | 9005 |
| … bunun `.LastResort`'u | 7189 (6291'i PUA) |
| … `.LastResort` dışı | 1816 (1239'u Apple Color Emoji) |

`NoFont` hiç doğmuyor: kimsenin çizemediği karakterde cascade `.LastResort`'u
veriyor ve o bir glyph döndürüyor, yani "yok" cevabı `Rejected`'a düşüyor.

`Rejected` oran dağılımı (`.LastResort` hariç, 1816 aday):

| | <1.2 | 1.2–1.5 | 1.5–1.7 | 1.7+ |
|---|---|---|---|---|
| `ratio` @2x (13pt, 16pt) | 348 | 172 | 1280 | 16 |
| `fit` @2x | 235 | 265 | 1290 | 26 |
| `ratio` @1x (13pt, 16pt) | 348 | 172 | 41 | 1255 |
| `fit` @1x | 235 | 265 | 51 | 1265 |

İlk kova 1.0'ın altını da taşıyor: sola taşan ya da sola yapışıp sağdan
taşan aday mürekkebi hücreden dar olsa da dönüyor (en küçük `ratio` 0.49,
Hiragino Sans).

- **Tek sütunlu emojinin üst ucu.** Apple Color Emoji'nin bütün glyph'leri
  aynı oranda: @2x'te `ratio` 1.661, `fit` 1.661–1.681. @1x'te `ratio`
  **2.076**, `fit` 2.124. @1x'te emoji iki hücreye de sığmıyor (`2h` 0), yani
  kapı @1x ekranda **geniş** emojiyi de bugün reddediyor. Bu 041'in
  kapsamında değil ama sınırı etkiliyor: @1x'i kapsayan bir sınır 2.12'nin
  üstünde olmalı.
- **`.LastResort`.** Oran sabit: `ratio` 1.494, `fit` 1.660, dört
  birleşimde de. Emojinin **altında**, yani emojiyi kapsayan her sınır
  `.LastResort`'u da kapsar. Geometri onu ayıramıyor ve R3.2 için phase-2'de
  ayrı bir ölçüt gerekiyor. Döndüğü yerler: PUA'nın 6291'i ve PUA dışında
  `U+2700`, `U+275F–2760`, `U+27CE–27CF`, Misc Symbols and Arrows'un 161'i
  (`2B4D–2BFF` arasındaki boşluklar), emoji bloğunun 732'si (atanmamış kod
  noktaları ve Apple Color Emoji'de olmayan Unicode 7 pikto grafları).
- **`⧉`**: Apple Symbols, `ratio` 1.11, `fit` 1.22, dört birleşimde de.

Font başına döküm (@2x; kabul / ret, `ratio` aralığı, `fit` aralığı):

| font | kabul | ret | `ratio` | `fit` |
|---|---|---|---|---|
| Apple Symbols | 276 | 357 | 0.834–2.169 | 1.001–2.250 |
| STIX Two Math | 31 | 106 | 0.870–2.490 | 1.012–2.583 |
| Apple SD Gothic Neo | 26 | 33 | 1.000–1.440 | 1.204–1.447 |
| Hiragino Sans | 6 | 32 | 0.490–1.661 | 1.078–1.661 |
| Zapf Dingbats (`❶…➓`) | 0 | 30 | 1.189 | 1.249 |
| Symbol | 21 | 5 | 0.860–1.870 | 1.217–2.803 |
| Lucida Grande | 2 | 5 | 0.961–1.381 | 1.141–1.521 |
| Superclarendon (PUA) | 8 | 2 | 0.924–0.968 | 1.015–1.043 |
| Sukhumvit Set | 1 | 2 | 1.365–1.402 | 1.448–1.455 |
| Helvetica Neue | 0 | 2 | 1.889 | 1.925–1.980 |
| Inter Display (kullanıcı fontu) | 0 | 2 | 4.083 | 4.221–4.264 |
| .SF Compact (`⭘`) | 0 | 1 | 1.247 | 1.354 |
| Apple Color Emoji | 0 | 1239 | 1.661 | 1.661–1.681 |
| Athelas / Monaco | 10 / 11 | 0 | — | — |

**Bekçi** (Menlo 16pt @2x): 26 karakterin 23'ü çiziliyor. Kutu çıkanlar
`EXPECTED_TOFU`'da: `⧉` (Apple Symbols, phase-2'nin hedefi), `U+E0A0` ve
`U+E0B0` (`.LastResort`; R3.2 yüzünden küçültme bunları boşaltmaz).

**Doğrulama.** `make hepsi` exit 2: `fmt`, `denetim`, `clippy -D warnings`
temiz; `bt-atlas` 80 geçti (1 ignored = tarama), `bt-core` 602 geçti. Düşen
tek sınama `bt-gpu`'da `wgpu_renderer::tests::wgpu_matches_the_metal_oracle_on_every_scene`
ve sebebi sahnenin kurulumu (`wgpu_renderer.rs:921`, "sahne caret'i dock
yuvasına koymadı" — `Frame::dock_caret`), atlasla ilgisi yok. Dosya 040'ın
izlenmeyen WIP'i ve `frame.rs` koşu sırasında 040 tarafından değiştirildi.
