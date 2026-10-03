# Phase 2 — Taşan dörtgen

## Özet

Glyph ve emoji dörtgeni yuva boyunda çiziliyor. Taşma payı viewport'ta
kırpılmıyor, çizim sırası taşan mürekkebi koruyor. Aralık hâlâ `≥ 1`, `< 1`
yalnız sınamalarda.

_Requirements: R3, R3.1, R3.2, R3.3, R5_

## Değişiklikler

- **`crates/bt-gpu/shaders/cell.wgsl` + `renderer.rs` `GlyphImmediates`**
  - Immediates'e `slot_px` ve `slot_offset` (vec2 + vec2) giriyor. Mevcut
    `pad` kullanılıyor ya da blok 80 bayta çıkıyor; her iki durumda da
    `offset_of` assert'leri ve `IMMEDIATE_BUDGET` geçerli.
  - `cell_vertex`: dörtgen `pos − slot_offset + corner × slot_px`, uv
    `uv0 + corner × uv_size` (uv zaten tam yuva).
  - `emoji_fragment` aynı vertex'i aldığı için kendiliğinden doğru.
  - Değerler atlastan (`slot_metrics`, phase-1), `frame.cell_px()`'ten
    değil.
- **`crates/bt-gpu/shaders/glyph_fx.wgsl` + `FxImmediates`**
  - `slot_px`/`slot_offset` giriyor.
  - `FX_PAD`'in tabanı, `paint`'in `g`-sınırı, `texel = uv_size / slot_px`,
    `i1` kırpması, `ink_depth` kırpması ve shatter'ın parça ızgarası **yuva**
    kutusuna geçiyor.
  - Glyph'in yuva içindeki yeri `slot_offset` ile hücreye bağlanıyor.
  - `t = 1` eşitliği korunuyor.
- **`crates/bt-gpu/src/renderer.rs` `plan`**
  - Glyph op'ları (ızgara, bant, dock) taşma payı kadar yukarıdan başlayan
    bir viewport alıyor. Negatif y'yi doldurma bandının orijini bugün zaten
    taşıyor ("orijin kaymanın ortasında negatife iniyor", `CLAUDE.md`). O yol
    yeniden kullanılıyor, ikinci bir kırpma mekanizması kurulmuyor. Pay bir immediate kaydırmayla vertex'te geri
    ekleniyor; `≥ 1`'de pay sıfır, op'lar bugünküyle aynı.
  - Sıra: ızgara zemini → arama/seçim → caret → bant zemini → bant araması →
    ızgara glyph'leri → bant glyph'leri → dock. Caret bant zemininin altında
    kalıyor (`CLAUDE.md` → çizim sırası).
  - Dock glyph viewport'u `band_y`'den başlıyor (bandın tepesi). Yeni
    scissor yok.
  - `fx_draw` doc'undaki "dock viewport'u yukarısını kırpar" cümlesi yeni
    sözleşmeye göre düzeltiliyor.

## Kabul

- `make shader` yeşil. `offset_of` assert'leri WGSL düzeniyle aynı.
- **Saf sınama** `slot_quad_is_the_cell_at_or_above_one`: `≥ 1`'de
  immediates'te `slot_offset == 0`, `slot_px == cell_px`, taşma payı `0`.
- **Offscreen** (`fixture` karakterleriyle, lavapipe'ta da):
  - `descender_paints_over_the_next_rows_background`: `lh = 0.6`'da
    0. satırda `fixture`'ın kuyruklu harfi, 1. satır kırmızı zeminli. Kuyruğun
    pikselleri 1. satırda ön plan renginde.
  - `grid_top_accent_survives_the_fill_band`: bant görünürken ızgaranın tepe
    satırındaki yüksek mürekkep bandın içinde görünüyor.
  - `caret_stays_under_the_fill_band`: banda kayan ızgara caret'ini bandın
    zemini örtüyor (bugünkü kural).
  - `dock_glyph_stays_inside_its_band`: dock'un üst satırının taşması bandın
    tepesinin üstünü boyamıyor.
  - `arrival_effect_matches_static_glyph_below_one`: `lh = 0.6`'da üst giriş
    satırında yüksek mürekkepli harf, geliş `t = 1` statik glyph'le piksel
    piksel aynı.
- Mevcut offscreen sınamalar değişmeden yeşil (R5).
- `make check`, `make linux`, `make smoke`.

## Checklist

- [x] `cell.wgsl` + `GlyphImmediates`: `slot_px`/`slot_offset`, assert'ler
- [x] `glyph_fx.wgsl` + `FxImmediates`: yuva kutusu
- [x] `plan`: taşma viewport'u, ızgara/bant sırası, dock viewport'u
- [x] Kural ailesi (`Sprite::Rule`) phase-1'den beri yuva genişliğinde çiziliyor: `< 1`'de komşu dörtgenle payı kadar örtüşüyor ve `curl`/`dotted`/`dashed` deseninin periyodu yuvanın genişliği — dikişin bozulmadığını bekçiyle/gözle doğrula (phase-1'den devir)
- [x] Test: yukarıdaki Kabul sınamaları
- [x] Doğrulama geçti: `make check`, `make shader`, `make linux` · [~] `make smoke`: `frames=0` üst commit'te (41ec701) de aynı — ortam
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi

## Uygulama Notları

- **Immediates.** `GlyphImmediates` 80 bayt: `cell_px` çıktı (`cell.wgsl`
  onu yalnız dörtgen boyu için okuyordu), yerine `slot_px`@40,
  `slot_offset`@56 ve `lift: f32`@64. `FxImmediates` 64 bayt: `cell_px`
  kalıyor (genlikler hücre oranı), `slot_px`@40 ve `slot_offset`@48 giriyor.
  Değerler tek tipten, `renderer::SlotQuad::of(&Atlas)`.
- **Kaldırma `glyph_draws`'un içinde.** Taşma payı yalnız atlas ödünç
  alınınca biliniyor, o yüzden kaldırılmış `Op::Viewport`'u ve geri dönüşü
  `glyph_draws` itiyor (liste boşsa ya da pay sıfırsa hiç itmiyor — `≥ 1`'de
  op listesi yalnız sıra değişikliğiyle bugünkü). Dock'un payı
  `origin_y − band_y` ile sınırlı (büyüyen bantta sıfır, scissor kırpıyor);
  `Op::Fx`'in `origin_y`'ye geri dönüşü değişmedi, çünkü sonraki glyph
  listesi kendi viewport'unu kuruyor.
- **`glyph_fx`'in kutusu.** Dönüşümün merkezi hücrede kaldı; `extrude`'un
  çıpası ve `iris`'in yarıçapı da (plan yalnız shatter'ı saymıştı) yuva
  kutusuna geçti — taşan mürekkep diyaframın ilk karesinde kesilmesin.
- **Kural ailesi (phase-1'den devir) `< 1`'de bozuktu ve düzeltme atlasta.**
  Yuva genişliğinde çizilen kural komşu dörtgenle payı kadar örtüşüyor,
  noktalı/kesikli/kıvrık desenin periyodu yuvayı bölüyordu. Kural artık
  `rules::rule_metrics` ile (hücre genişliği, yuvanın yüksekliği ve taban
  çizgisi) çizilip `place` ile `slot_offset.x`'e konuyor; chevron da.
  Bekçi `rules_tile_the_cell_column_inside_the_slot`. `raster_digest`
  (HEAD 41ec701 ile ağaç, 57 948 satır): fark **boş**.
- **`descender_paints_over_the_next_rows_background` yalnız @2x.**
  Kesim ascent:descent oranında; 13pt@1x'te `lh = 0.6` alttan tek piksel
  alıyor ve o fontun kendi descent boşluğu — `g j y p _ , ( Q }`'nun hiçbiri
  1. satıra taşmıyor (ölçüldü). @2x'te `g` iki satır taşıyor.
- **Test karakterleri `fixture`'dan değil**: kuyruklu `g` ve aksanlı `É`
  hem Menlo'da hem DejaVu Sans Mono'da var; `fixture`'da bu rolde sabit yok.
  `make linux` (lavapipe) yeni sınamalarla yeşil.
- **`/code-review` bulgusu (giderildi):** yalnız yukarı kaydırılan viewport'un
  alt kenarı da `lift` kadar yukarı çıkıyor, dock'suz pencerede (vim,
  `blocks`) dipteki satırın son pikselleri kesiliyordu. Kaldırılmış liste
  ayrı bir op (`Op::Lifted`): viewport `lift` kadar **uzun** ve
  `viewport_px` immediate'i de o boyda. Bekçi
  `a_lifted_viewport_keeps_the_window_bottom` (düzeltmesiz kırmızı, ölçüldü).
