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

- [ ] `cell.wgsl` + `GlyphImmediates`: `slot_px`/`slot_offset`, assert'ler
- [ ] `glyph_fx.wgsl` + `FxImmediates`: yuva kutusu
- [ ] `plan`: taşma viewport'u, ızgara/bant sırası, dock viewport'u
- [ ] Test: yukarıdaki Kabul sınamaları
- [ ] Doğrulama geçti: `make check`, `make shader`, `make linux`, `make smoke`
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
