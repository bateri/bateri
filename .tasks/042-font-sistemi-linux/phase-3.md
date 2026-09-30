# Phase 3 — `FontSystem` trait'i ve CoreText arka ucu

## Özet

Trait (Karar 3'ün yüzeyi) ve `cfg` takma adı kurulur, CoreText yarısı ve iki
çizim fonksiyonu macOS arka ucuna iner, sınamalar bölünür; hâlâ yalnız macOS
(`discussion.md` → Karar 1, 3, 7).

_Requirements: R3, R5, R1.1, R1.2_

## Değişiklikler

- **`crates/bt-atlas/src/` — trait modülü** — `FontSystem` (tek ilişkili tip
  `Font: Clone`; `open`, `open_default`, `derive`, `is_monospaced`,
  `families`, `glyph`, `advance`, `ink`, `raw_metrics`, `cascade -> Option`,
  `at_size`, `size`, `is_last_resort`, `has_color_glyphs`, `shape`,
  `draw_mask`, `draw_color`), `type Backend`, `type Font`. `draw_*` konumu
  hesaplanmış alır, `baseline` yuvanın altından (Karar 3.11).
- **CoreText arka ucu (`font.rs`'in yeni yeri)** — bugünkü gövdeler aynen,
  `PREFERRED`/`FALLBACK` ve `open_default`'un tanısı burada; `raster.rs`'in
  `draw_glyph`/`draw_color_glyph`'in CG bağlamı burada. CoreText/CoreGraphics
  adları yalnız bu modülde.
- **`crates/bt-atlas/src/lib.rs`** — `Atlas`'ın alanları `Font` takma
  adıyla; `slot()` ve `draw_accepted` konumu platformsuz formülle hesaplayıp
  arka uca veriyor. Başlık yorumunun "yalnız objc2-core-* görülür" cümlesi
  arka uca göre.
- **Sınamalar** — Karar 7'nin kuralıyla: değişmez bekçileri `cfg(test)`
  fikstürle (örnek karakterler, varsayılan aile adı) platformsuz, kalibrasyon
  `cfg(target_os = "macos")`; sınıflamanın listesi Uygulama Notları'na.
  `mod census` `cfg(all(test, target_os = "macos"))`.
- **`crates/bt-atlas/Cargo.toml`** — `objc2-core-*` üçlüsü
  `[target.'cfg(target_os = "macos")'.dependencies]` altına (sürüm ve
  özellik oynamıyor; `Cargo.lock` değişmemeli).
- **`Makefile` (`denetim`)** — `bt-atlas`'ta CoreText/CoreGraphics adlarının
  yalnız macOS arka uç dosyasında göründüğünü soran tek grep.

## Kabul

- Tanık ebeveynle aynı; `bt-gpu`, `bt-shell` derlemesi değişmeden geçiyor.
- `make denetim` yeni grep'le yeşil, `Cargo.lock` değişmedi.
- `make hepsi` yeşil; sınama sayısı düşmedi (taşınanlar sayıldı).

## Checklist

- [ ] Trait + takma ad; CoreText arka ucu
- [ ] Çizim konumu platformsuz tarafta
- [ ] Sınamalar sınıflandı, fikstür kuruldu
- [ ] `objc2` bağımlılıkları macOS hedefinde
- [ ] Denetim grep'i
- [ ] Test: tanık ebeveynle aynı
- [ ] Doğrulama geçti (`make hepsi`)
