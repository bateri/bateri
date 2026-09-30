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

- [x] Renkli glyph + örnekleyici (yalnız küçültme)
- [x] `harfrust` şekillendirmesi, tek bayt kaynağı
- [x] Taslaklar kalktı, fikstür tamam
- [x] Test: Linux'ta atlasın bütün değişmezleri; macOS'ta tanık aynı
- [x] Doğrulama geçti (`make hepsi` + `make linux`)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi

## Uygulama Notları

- Kilide giren crate'ler: `harfrust` 0.13.3, `read-fonts` 0.43.3,
  `font-types` 0.12.5 (`bytemuck`, `smallvec`, `bitflags`, `once_cell` zaten
  grafta). `harfrust` `default-features = false, features = ["std"]` — `icu`
  kapalı. macOS ürün grafı `diff`'le aynı (`cargo tree --target
  aarch64-apple-darwin -p bateri -e normal`), THIRD-PARTY-LICENSES değişmedi.
- FreeType 2.12 Noto Color Emoji'yi **ölçeklenemez** sayıyor (fc-query
  `scalable=True` dese de `FT_IS_SCALABLE` yanlış; tek strike 109 ppem,
  glyph kutusu 136×128) ve `FT_Set_Char_Size` orada düşüyor: bitmap yüz
  strike'la açılıyor (`select_size`, en küçük ≥ istenen, yoksa en büyük) ve
  `size / strike` katsayısı `FtFont::bitmap_scale`'de tek sahip.
- **Örnekleyici iki yönlü** (plan "yalnız küçültür" diyordu): tam örtüşme
  integrali (kutu süzgeci) yönden bağımsız, yani strike'ın yetişmediği
  büyük puntoda glyph kutuya düşmek yerine bloklu büyüyor. Renkli dış hat
  (`COLR`) da aynı yoldan: FreeType çiziyor, ölçek 1.
- **Cascade'e iki kural eklendi** (ölçüm): Noto'nun charset'inde U+FE0F
  yok, DejaVu Sans hem `❤`'yi hem seçiciyi taşıyor — `❤️` metin
  düzlemine düşüyordu. (1) Birleştirici ve seçiciler (ZWJ/ZWNJ, VS1–16,
  VS17–256) kapsama sayılmıyor; (2) U+FE0F taşıyan metin önce renkli
  fontlarda (fontconfig `color`) aranıyor. Şekillendirme
  `REMOVE_DEFAULT_IGNORABLES` ile: `CTLine` `❤️`'yi tek glyph sayıyor.
- **Bilinen sınır, karar bekliyor:** DejaVu Sans Emoticons bloğunu
  (U+1F600–1F64F) taşıyor ve sıralamada Noto'dan önce, yani tek kod
  noktalı `😀` Linux'ta **tek renk** (maske) çiziliyor, `👍` renkli.
  Unicode'un kuralı (Emoji_Presentation=Yes → renkli) `unicode-width`'in
  tablosuyla (ızgaranın genişlik 2'si) kurulabilir ama `bt-atlas`'a yeni
  bir kenar — onaylı listenin dışı, bu phase'de yapılmadı. Linux'ta pencere
  yok, kullanıcı henüz görmüyor.
- Kümenin gerçekten şekillendiğini `freetype::tests::every_cluster_shapes_into_one_colour_glyph`
  sınıyor: atlas sınaması taban karaktere düşüşte de yeşil kalırdı (bayrağın
  tabanı renkli bir RI harfi). Örnekleyicinin iki saf sınaması ve renkli
  glyph'in ölçülen mürekkebin içinde çizildiği bir sınama da orada.
- Tanık: ebeveyn (`b37fc24`) worktree'de, 38 676 satır, fark boş.
- `/code-review` (medium, --fix): kümenin şekillendirmesi artık **her zaman**
  önce renkli fontlarda arıyor (`cascade_from(.., colour_first)`): bayrak,
  ZWJ ve ten rengi dizileri U+FE0F taşımıyor ve parçaları kapsayan bir metin
  fontu (Symbola) Noto'dan önce sıralanırsa dizi taban karakterine düşerdi.
- **Waive (low):** `COLR` dış hat kolunda kapı tabanın tek renk dış hattını
  ölçüyor, çizilen renkli katmanlar; kesirli öteleme de katmanlar yeniden
  yüklendiği için kayboluyor (≤ 1 px). Yalnız `COLR` fontu (Twemoji COLR)
  etkileniyor, imajın `CBDT` Noto'su değil; düzeltmesi katmanlardan ölçmek. Kabul: orkestratör, 2026-09-30.
