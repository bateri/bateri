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

- [x] Trait + takma ad; CoreText arka ucu
- [x] Çizim konumu platformsuz tarafta
- [x] Sınamalar sınıflandı, fikstür kuruldu
- [x] `objc2` bağımlılıkları macOS hedefinde
- [x] Denetim grep'i
- [x] Test: tanık ebeveynle aynı
- [x] Doğrulama geçti (`make hepsi`)

## Uygulama Notları

- Dosya düzeni: trait + `Backend`/`Font` takma adı + fikstürün yeniden
  ihracı `system.rs`'te; CoreText arka ucu `coretext.rs` (`font.rs`'in
  gövdeleri ve `raster.rs`'in iki CG bağlamı). `font.rs` kalktı: platformsuz
  orkestrasyon (`open_chain`, `Faces`, `accept`, `shrink`, `fallback_font`,
  `shape_cluster`, `metrics`, `space_advance`, `monospaced_families`,
  `family_issue`) `rules.rs`'e indi ve generic değil, doğrudan `Backend`'i
  çağırıyor (turbofish'siz çağrı yerleri; phase-2'nin closure'ları ve ince
  sarmalayıcıları kalktı, çağıranlar `Backend::glyph(..)` diyor).
- Konum formülü `lib.rs`'te değil `raster::position`'da (platformsuz,
  `draw_glyph`/`draw_color_glyph`'in tek kaynağı) — `lib.rs`'in çağrı
  yerleri değişmedi. `unpremultiply` platformsuz tarafta ve yalnız `Drawn`'da.
- `cascade` `&str` alıyor (küme ile tek karakter aynı çağrı);
  `census::classify` `None`'u `NoFont`'a çeviriyor. `shape` cascade'i kendi
  içinde yürüyor (CoreText'te run'ın fontu).
- Fikstür (`coretext::fixture`): `DEFAULT_FAMILY`, `PROPORTIONAL_FAMILY`,
  `SECOND_FAMILY`, `UNKNOWN_CHAR`, `WIDE_CHAR`, `FALLBACK_CHAR`, `INK_CHAR`,
  `GATE_PROBES`, `BASE_SYMBOLS`, `CLUSTERS`, `CLUSTER_BASE`,
  `CLUSTER_SCALE` ve tanı için `family_name(&Font)` (sınama ve census'ün CT
  çağrısı böylece arka uçta). Karar 7'nin listesine ek: ikinci ve orantılı
  aile — aile değişimi ve uyarı bekçileri de platformsuz kalsın diye.
- Sınıflama — **macOS kalibrasyonu** (`cfg(target_os = "macos")`):
  `face_fallback_is_cached_under_the_requested_face` (Menlo'nun `╱`'u),
  `the_default_size_keeps_todays_texture` (1984), `non_bmp_char_path_works`
  (STIX'in `𝔸`'sı), üç uzak/yükleme satırı sınaması (`Some("Menlo")`,
  `REMOTE_MARK` sabiti dahil), `missing_face_falls_back_to_regular`
  (Monaco), `a_wide_char_that_fits_one_cell_keeps_the_single_slot_raster`
  (`☕`); `census` modülü `cfg(all(test, target_os = "macos"))`.
  **Platformsuz, fikstürle**: kapı, yedek ve geniş karakter bekçileri
  (`UNKNOWN_CHAR` → geniş istekte `WIDE_CHAR`; `'𠀀'` → `WIDE_CHAR`), küme
  sınamaları, aile sınamaları, `every_base_glyph_advance_is_the_cell_advance`
  (`BASE_SYMBOLS`). Geri kalanı zaten fontsuzdu. Sınama sayısı aynı (86).
- Denetim grep'i `objc2_core_*` ve `CT…`/`CG…`/`CF…` adlarını yorum dışı
  satırlarda `coretext.rs` dışında arıyor; tek isabet bir assert metnindeki
  `CFRange`'di, metin "cascade'in UTF-16 aralığı" oldu.
- Tanık: ebeveyn (`9cb8e78`) çıktısı düzenlemeden önce aynı oturumda temiz
  ağaçta alındı (worktree adımının yerine); 38 676 satır, fark boş.
  `Cargo.lock` değişmedi.
