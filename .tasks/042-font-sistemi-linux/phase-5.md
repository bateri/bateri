# Phase 5 — Linux arka ucu: renk ve küme

## Özet

Renkli emoji (`CBDT`) ve grapheme dizilerinin şekillendirilmesi Linux arka
ucunda; atlasın bütün değişmezleri Linux'ta yeşil (`discussion.md` → Karar 6).

_Requirements: R7, R8_

## Değişiklikler

- **`Cargo.toml` (workspace) ve `crates/bt-atlas/Cargo.toml`** — `harfrust`,
  Linux hedefine koşullu (eskalasyon onayının kapsamında).
- **FreeType arka ucu** — `has_color_glyphs` (`FT_HAS_COLOR`); `draw_color`:
  `FT_LOAD_COLOR` ile en yakın strike, ön çarpımlı BGRA → RGBA, yalnız
  küçülten alan ortalaması, platformsuz `unpremultiply`; bitmap fontta
  `at_size` ölçek katsayısı ve mürekkep/ilerleme aynı katsayıdan (tek sahip).
  `shape`: aynı bayt tamponu üstünde `harfrust`, tek glyph + onu üreten
  font, yoksa `None`.
- **Sınama fikstürü** — Noto Color Emoji'ye göre emoji ve küme örnekleri;
  phase-4'ün `cfg`'leri kalkar.
- **`tools/linux/Dockerfile`** — `fonts-noto-color-emoji`.

## Kabul

- `make linux` yeşil: `bt-atlas`'ın bütün platformsuz değişmezleri Linux'ta
  geçiyor (renk düzlemi, iki yarı, küme, küçültme).
- `make hepsi` yeşil, tanık ebeveynle aynı.

## Checklist

- [ ] Renkli glyph + örnekleyici (yalnız küçültme)
- [ ] `harfrust` şekillendirmesi, tek bayt kaynağı
- [ ] Taslaklar kalktı, fikstür tamam
- [ ] Test: Linux'ta atlasın bütün değişmezleri; macOS'ta tanık aynı
- [ ] Doğrulama geçti (`make hepsi` + `make linux`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
