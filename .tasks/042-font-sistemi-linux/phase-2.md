# Phase 2 — Kural yarısının taşınması ve tiplerin nötrlenmesi

## Özet

`font.rs`'in fontsuz kuralları platformsuz modül(ler)e taşınır; `CGFloat` →
`f64`, `CGRect` → `InkRect`. CoreText çağrıları yerinde kalır (`discussion.md`
→ Karar 2).

_Requirements: R2, R1.1, R1.2_

## Değişiklikler

- **`crates/bt-atlas/src/` (yeni platformsuz modül, adı phase'in)** —
  `centre_shift`, `ink_fits_placed`, `fit_ratio`, `accept`'in sırası,
  `shrink`'in iki turu (`SHRINK_STEPS`), `SHRINK_LIMIT`, `Accepted` + `rise`,
  `metrics()`'in hücre formülü (ham ölçüler bir yapıda giriyor),
  `rule_envelope`, `round_up`, `Faces` + `effective`, `open_chain`'in
  istenen-aile kolu + `same_family`, `unpremultiply`, `InkRect`. İşlem sırası
  ve formüller harfi harfine — bit eşitliğin gerekçesi `CGFloat == f64`.
- **`crates/bt-atlas/src/font.rs`** — yalnız CoreText'i soran fonksiyonlar
  kalıyor; `glyph_ink` `InkRect` döndürüyor, glyph numarası çağırana `u32`.
- **`crates/bt-atlas/src/lib.rs`, `census.rs`** — yeni yollara çağrı
  düzeltmeleri; mantık aynı.

## Kabul

- `git diff --color-moved` taşınan blokları taşınmış gösteriyor; taşınan
  gövdelerde değişen tek şey tip adları.
- Tanık (phase-0) ebeveynle aynı; `make tarama` yardımcı sinyal olarak aynı.
- `make hepsi` yeşil.

## Checklist

- [ ] Kural yarısı platformsuz modülde, CoreText tipi görmüyor
- [ ] `InkRect` ve `f64` imzaları; `u32` glyph
- [ ] Test: tanık ebeveynle aynı
- [ ] Doğrulama geçti (`make hepsi`)
