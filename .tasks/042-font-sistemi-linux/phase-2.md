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

- [x] Kural yarısı platformsuz modülde, CoreText tipi görmüyor
- [x] `InkRect` ve `f64` imzaları; `u32` glyph
- [x] Test: tanık ebeveynle aynı
- [x] Doğrulama geçti (`make hepsi`)

## Uygulama Notları

- Platformsuz modül tek dosya: `crates/bt-atlas/src/rules.rs`. `Face`,
  `SizeClass`, `FontIssue`, `Metrics` de oraya indi (fontsuz tipler;
  `lib.rs`'in `pub use` adları aynı). `Face::traits` `font.rs`'te serbest
  `face_traits` oldu, `unreachable!` ve gerekçesi aynen.
- Trait yok (phase-3'ün işi): kural yarısı ölçüyü **değer** olarak alıyor,
  fonta ortada dönmesi gereken yerde **closure** (`open_chain`'in üç
  çağrısı, `Faces::derive_with`, `accept`'in küçültme kolu, `shrink`'in
  `at`/`fits`'i, `Accepted::rise`'ın tembel `ink`'i). `font.rs` bugünkü
  imzalarla ince sarmalayıcılar tutuyor (`accept`, `shrink`, `metrics`,
  `open_chain`, `Faces::from_chain`/`derive`); phase-3 closure'ları trait
  çağrısına çeviriyor.
- `accept`'in iki kapısı adayın ölçüsünü bir kez alıp aynı `(advance,
  ink)` çiftini iki kez sınıyor (önce iki kez aynı ölçü okunuyordu — saf
  okuma); `shrink`'te `is_last_resort` artık kısa devresiz okunuyor, yalnız
  reddedilen adayda koşan saf bir okuma.
- `u32` glyph'in `CGGlyph`'e daralması tek yerde (`font::cg_glyph`).
- `unpremultiply`'ın iki sınaması `raster.rs`'ten `rules.rs`'e taşındı;
  sınama sayısı aynı (87).
- `CLAUDE.md`'deki taşınan yollar `rules::` oldu; var olmayan
  `font::ink_fits_cell` yanındaki cümleyle birlikte `font::ink_fits_box`'a
  düzeltildi.
- Tanık: ebeveyn (`c1db50d`) ile 38 682 satır, fark yalnız süre satırı.
  `make tarama` koşulmadı (yardımcı sinyal, tanık değil).

