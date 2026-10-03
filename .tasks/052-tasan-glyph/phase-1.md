# Phase 1 — İki metrik ve yuva sahipliği

## Özet

Atlas glyph metriğini (yuva) ızgara metriğinden (hücre) ayırıyor ve `bt-gpu`
yuvayı atlastan okuyor. Shader'a dokunulmuyor, ayar aralığı `≥ 1` kalıyor.

_Requirements: R1, R1.1, R2, R2.1, R2.2, R3, R5_

## Değişiklikler

- **Ön adım, phase-0'ın commit'inin üstünde (kod değişmeden önce)** —
  `crates/bt-atlas/tests/raster_digest.rs`'in konfigürasyonuna
  `letter = 1.3` ekleniyor. Özet bugünkü ağaçta (HEAD) koşup çıktısı
  saklanıyor. Phase'in karşılaştırması bu çıktıya karşı yapılıyor. Ekleme
  phase'in commit'ine giriyor.
- **`crates/bt-atlas/src/rules.rs`**
  - `cell_metrics` iki kola ayrılıyor.
    - `≥ 1`: phase-0'ın kuralı, dokunulmuyor.
    - `< 1`: açık işaretli yuvarlanıyor ve doğal yüksekliğin ascent
      (`round_up(ascent)`) ile descent+leading parçalarına **oranlı** kesiliyor.
      **İki parça da en az 1 px kalıyor**: oranlı kesim bir parçayı 0'a
      indirecekse fazlası öteki parçadan alınıyor. Böylece en küçük puntoda
      da `0 < baseline_px < cell_px.1`.
  - Genişlik tarafında ayrı bir kol yok: `round_up(advance × letter)` zaten
    `< 1`'de küçülüyor.
  - Yeni saf fonksiyon: yuva ile hücreden ofset `(x, y)`. Yatayda genişlik
    farkının yarısı, dikeyde glyph taban çizgisi eksi hücre taban çizgisi.
    Tek kaynak bu fonksiyon.
  - **Ortalama ile sınır ayrı iki kutu** (R2.2).
    - Ortalama her zaman `cols × hücre ilerlemesi` (ızgara metriği).
    - Sınır o kutunun iki yanına pay eklenmiş hâli: `cols = 1`'de yuva,
      `cols = 2`'de `hücre + yuva`.
    - Raster hedefi yuva ve x ofseti pay.
    - `ink_fits_placed` sınırı ayrı bir argüman olarak alıyor ya da paydan
      türetiyor. `≥ 1`'de pay sıfır, iki kutu çakışıyor ve cevap bugünkü.
    - İlk kolun "doğal hücreye sığan aralıklı hücreye de sığar" yorumu
      düzeltiliyor.
    - Yan sonuç: dar `ls`'de `hücre + yuva`'ya sığmayan iki sütunlu glyph
      041'in küçültme yoluna düşüyor. Kesilmiyor, küçük çiziliyor (bilinen
      sınır, phase-3 belgeye yazıyor).
  - `Metrics` ve `rule_envelope` doc'ları iki anlamı söylüyor.
- **`crates/bt-atlas/src/lib.rs`**
  - `Atlas` iki metrik tutuyor:
    - `metrics()`: hücre (ızgara). Adı ve anlamı korunuyor, yani tüketiciler
      ve digest satırı değişmiyor.
    - Yeni `slot_metrics()`: yuva. `Spacing { line: max(l,1), letter:
      max(k,1) }` ile kurulan metrik.
    - Küçük sınıf da aynı çifti alıyor.
  - Yuva geometrisinin bütün sahipleri yuva metriğine geçiyor: `grid_for`,
    `edge_for`, `texture_px`, `slot_origin`, `slot_bytes`, tampon boyları ve
    tofu.
  - `place_small` genelleşiyor (hücre tamponunu yuvaya ofsetle koyan tek
    fonksiyon). Döşenen yordamsal aile (`raster::is_procedural`) ve tofu
    ızgara metriğiyle ayrı tampona çizilip onunla konuyor.
  - Alt çizgi, üstü çizili ve chevron (`Sprite::Rule`) yuva metriğiyle
    çiziliyor: taban çizgisine bağlı.
  - `draw_accepted`:
    - Sağ yarının `x_offset`'i **hücre** adımı. Böylece iki sütunlu kutunun
      ortası hücre ızgarasında kalıyor.
    - Sol yarı `x < hücre_w`, sağ yarı `x ≥ hücre_w` olacak biçimde bölme
      çizgisinde kırpılıyor (R2.1).
  - `rise` yuvada ortalıyor.
- **`crates/bt-atlas/src/raster.rs`, `coretext.rs`, `freetype.rs`,
  `census.rs`**
  - Glyph yolu yuva metriğini alıyor.
  - "Komşuya taşmak mümkün değil" yorumu "hedef yuva; yuva hücreden büyük
    olabilir" diye düzeltiliyor.
  - `census` iki metriği doğru yerde kullanıyor.
- **`crates/bt-gpu`**
  - `slots::slot_layout`, `Renderer::write_slot`'un boy kontrolü ve
    `uv_size` atlasın yuva metriğinden okuyor (`slot_metrics`).
  - `CellMetrics` hücre olarak kalıyor. Dörtgen bu phase'de hâlâ hücre
    boyunda: `< 1` yalnız sınamalardan erişilebilir ve orada glyph kırpık
    çizilir, panik olmaz.

## Kabul

- **`raster_digest`.** Ön adımın çıktısıyla ağacın farkı **boş** (R5).
- **`bt-atlas` sınamaları:**
  - `below_one_keeps_the_baseline_inside_the_cell`: `lh ∈ {0.5, 0.7, 0.99}`
    × `MIN_POINT_SIZE`, 13, 29 × @1x/@2x (`0.5 × 4pt@1x` köşesi dahil). `0 < baseline_px < cell_px.1`, hücre yuvadan
    küçük ya da eşit, ofset yuvanın içinde.
  - `slot_equals_cell_at_or_above_one`: `≥ 1` köşelerinde ofset `(0, 0)` ve
    iki metrik eşit.
  - `procedural_family_tiles_the_cell_inside_the_slot`: `lh = ls = 0.7`'de
    `─`/`█`/`│` mürekkebi tam olarak yuvadaki hücre dikdörtgeninde, payda sıfır.
  - `wide_halves_partition_the_glyph`: `ls = 0.7`'de `fixture`'ın geniş
    karakterinin iki yarısının mürekkebi ayrık ve birleşimi tek parça rasterle
    piksel piksel aynı. Glyph'in merkezi iki hücrenin ortasında.
  - `underline_follows_the_baseline`: `lh = 0.5`'te alt çizginin yuvadaki
    yeri `1.0`'dakiyle aynı.
  - Kapasite sınaması değişmeden yeşil.
- `make check`, `make linux` (iki arka uç değişiyor).

## Checklist

- [x] Ön adım: digest'e `letter = 1.3`, HEAD çıktısı saklandı
- [x] `rules.rs`: iki kollu `cell_metrics`, ofset fonksiyonu, kapının kutusu
- [x] `lib.rs`: iki metrik, yuva sahipleri, genel `place`, `Half` kırpması, kural/chevron yuvada
- [x] raster/coretext/freetype/census uyarlandı, yorumlar
- [x] `bt-gpu` yuvayı atlastan okuyor
- [x] Test: yukarıdaki Kabul sınamaları
- [x] `raster_digest` farkı boş (R1.2 phase-0'da ödendi)
- [x] Doğrulama geçti: `make check`, `make linux`

## Uygulama Notları

- **Pay kesirli, ofset tam sayı.** Kapı ve çizim `rules::GlyphBox`'ı
  (`advance` = `cols × hücre ilerlemesi`, `pad` = `(yuva ilerlemesi −
  hücre ilerlemesi) / 2`) paylaşıyor; sınır `advance + 2·pad`, yani tek
  sütunda yuva, iki sütunda `hücre + yuva`. Pay `≥ 1`'de iki özdeş çarpımın
  farkı, yani bit bit `0.0`. Dörtgenin ve kırpmanın ofseti ise
  `rules::slot_offset`'in tam sayı pikseli; ikisi arasındaki fark (≤ 1 px)
  ekranda yerleşim, kırpma değil. Taban fontun harfi `< 1`'de tam olarak
  `x = 0`'a düşüyor, yani `1`'deki rasteri koruyor.
- **`centre_shift`'in `max(0)` tabanı kaldı** (R5 için şart). Sonucu: ilerlemesi
  `hücre + yuva`'yı aşan iki sütunlu glyph ortalanmıyor, yuvanın soluna
  yaslanıyor (mürekkebi yine kapıdan geçiyor). macOS fixture'ı
  (`.LastResort`) `ls = 0.7`'de tam bu kolda (ilerleme 14.30 > sınır 13.31,
  13pt@1x); `wide_halves_partition_the_glyph` iki dalı da sınıyor
  (`0.7` yaslanma, `0.85` ortalama).
- **İlk kapının kol kararı `natural_advance` ile, paysız kaldı**: "bugün tek
  hücreye sığar mı" sorusu aralıktan bağımsız; `< 1`'de çizimin sınırı yuva
  ve yuvanın ilerlemesi zaten doğal ilerleme.
- **Küçük sınıf için ikinci bir `Metrics` tutulmadı.** "Aynı çift" küçük
  sınıfta ızgara metriği (`small_metrics`) + kesirli payı (`context_pad`):
  yuva metriğinin küçük sınıfta tek tüketicisi pay olurdu ve o bir `Metrics`
  değil bir ilerleme farkı. Küçük glyph büyük yuvaya çiziliyor; küçük
  yordamsal sprite `(slot_offset.x, yuva tabanı − küçük taban)`'a konuyor.
- **`cell_metrics`'in `< 1` kolu `deficit_split`**: açık `floor(doğal ×
  (1 − lh))` (işaretli `ceil`), `doğal − 2` ile sınırlı; üst parçanın payı
  en yakına yuvarlanıyor, sonra iki taban. Sentetik bekçi
  `rules::tests::deficit_keeps_both_parts_of_the_cell` (1–40 × 1–15 px,
  `lh ≤ 0` ve NaN dahil). `10 × (1 − 0.9)` `f64`'te 1'in hemen altında
  kaldığı için kesim yok: sınır kapsayıcı yazıldı.
- `census` varsayılan aralıkta koşuyor; çağrıları paysız
  `GlyphBox::unpadded`'la sarıldı, sınıflandırma değişmedi.
- `raster_digest` (HEAD d008a43 + `letter = 1.3` ile ağaç, 57 948 satır):
  fark **boş**.
