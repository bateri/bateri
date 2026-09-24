# Phase 2 — Vurgunun çizimi ve tema rolleri

## Özet

`search_match` ve `search_current` temaya girer ve `bt-gpu` phase-1'in
koşularını 031'in `selection` pipeline'ıyla ızgarada ve doldurma bandında
çizer.

_Requirements: R2, R3_

## Değişiklikler

- **`crates/bt-core/src/theme.rs`** — iki rol, `roles` dizisi, iki gömülü
  temada değer (tasarım kararı, gözle kontrolle iner), lineer ve odaksız
  (`dim_toward`, `selection_unfocused_linear` emsali) karşılıkları;
  `SearchRuns` renkleri temanın aynı kopyasından hazır verir (031 Karar 9).
- **`crates/bt-gpu/src/frame.rs`** — ızgara ve bant için arama listeleri
  (`fill_rules` emsali); köşeler **eşleşme başına**: `selection_parts` her
  eşleşmenin kendi dilimiyle çağrılır, çünkü `selection_corners` satır başına
  tek koşu ve dizi komşuluğu varsayıyor. Yarıçap `SELECTION_RADIUS`.
- **`crates/bt-gpu/src/renderer.rs`** — `encode_selection`'ın ızgara ve bant
  viewport'unda ek çağrıları; sıra zemin → `search_match` → `search_current`
  → seçim → caret → glyph. Shader ve `#[repr(C)]` düzeni değişmez.
- **`crates/bt-gpu/src/link.rs`** — `push_*`'lar içerik karesinde, hareket
  karesi listeleri koruyor; renk odak bitinden.
- **`docs/AYARLAR.md`** → Temalar — iki rolün satırı ve örnek bloklar;
  belgedeki tema bloğunun sınaması yeni anahtarları görsün.
- **`CLAUDE.md`** — tema paragrafındaki tüketilen roller (`selection`
  emsali, modelin dışında) ve pipeline cümlesinde aramanın `selection`'ı
  paylaştığı.

## Kabul

- Offscreen sınamalar: eşleşme `search_match` renginde, geçerli
  `search_current` renginde; ardışık satırlardaki iki ayrı eşleşme iki şekil,
  sarılan tek eşleşme tek şekil; seçim aramanın üstünde; bant viewport'unda
  vurgu çiziliyor; arama kapalıyken kare bugünküyle aynı.
- Tema: eksik rol tabandan; belge bloğu sınaması yeşil.
- `make hepsi` yeşil.

## Checklist

- [ ] Tema rolleri, gömülü değerler, odaksız karşılıklar
- [ ] `Frame`: ızgara + bant listeleri, eşleşme başına köşe
- [ ] `Renderer`: ek encode'lar ve sıra
- [ ] `docs/AYARLAR.md`, `CLAUDE.md`
- [ ] Test: yukarıdaki senaryolar
- [ ] Doğrulama geçti (`make hepsi`)
