# Phase 1 — Atlas: kümeyi dizgi anahtarıyla şekillendirip çiz

## Özet

`bt-atlas` bir grapheme dizgisini `CTLine` ile tek glyph'e şekillendirip
bugünkü mürekkep kapısından ve düzlemlerden geçirerek yuvaya koyuyor; henüz
hiçbir çağıran yok.

_Requirements: R1, R1.1_

## Değişiklikler

- **`Cargo.toml`** (workspace) — `objc2-core-text`'e `CTLine`, `CTRun`,
  `CTStringAttributes` (gerekiyorsa), `objc2-core-foundation`'a
  `CFAttributedString`, `CFDictionary` feature'ları. **Yeni crate yok**;
  `Cargo.lock` değişmemeli — değişirse dur ve kaydını Uygulama Notları'na
  yaz. Mevcut yorum örüntüsüyle (her feature'ın gerekçesi yanında).
- **`crates/bt-atlas/src/lib.rs`** — `Sprite`'a `Cluster(u32)` kolu; kimlik
  atlasın kendi interner'ından (`Atlas::intern(&str) -> Sprite`, dizgi →
  kimlik, ömür boyu, tahliye yok — yuvalarla aynı politika). `Sprite`
  `Copy + Hash` kalır. `Atlas::slot`'ta `Cluster` kolu `Char`'ın font
  kolunun kardeşi: yüz normalizasyonu (renkli küme için yüz anlamsız —
  düz yüze iner), küçük sınıfta `Whole`'a zorlama aynen, yordamsal kapı ona
  uygulanmaz. Kural payı (`RULE_RESERVE`) `Char` ile aynı. `Sprite`'ın doc'u
  "emoji ayrı sete bırakıldı" cümlesini bugüne çevirir.
- **`crates/bt-atlas/src/font.rs` / `raster.rs`** — dizginin adayı
  `CTFontCreateForString` (dizginin tamamı için), şekillendirme
  `CTLineCreateWithAttributedString` → run'ların glyph'leri. **Tek glyph**
  değilse ya da glyph kapıdan (`ink_fits_cell`, `cols` = 1 sonra 2 — 023'ün
  sırası) dönerse `NoGlyph`'in karşılığı; çağıran taban karaktere düşer
  (R1.1). Çizim bugünkü glyph çizim yolunu bir `CGGlyph` ile koşar
  (`glyph_index(ch)` yerine); düzlem yine trait bitinden
  (`has_color_glyphs`). `centre_shift` ve iki yarının tam sayı ofseti
  aynen — yeni bir yerleşim formülü yok.
- **Taban karaktere düşüş `Atlas::slot`'un içinde** — kapı kararını tek
  yerde veriyor (023'ün kuralı) ve dört tüketici (`fan`, `prepare_fx`, üç
  yüzey) bedavaya kazanıyor. Bunun için `Cluster` kaydı taban karakteri de
  bilir (interner dizgiyi tutuyor, ilk karakteri o). Negatif önbellek
  anahtar başına bir kez koşar (bugünkü kural).

## Kabul

- `🇹🇷`, `👨‍👩‍👧`, `👍🏽`, `❤️`, `🌡️` renk düzleminde `Left` + `Right` iki yuva
  alıyor ve sağ yarı boş değil (`rasterized_glyph_is_not_empty` emsali).
- Aynı dizgi ikinci kez sorulunca yeni yuva açılmıyor; farklı dizgiler
  farklı kimlik.
- Şekillenmeyen bir dizgi (ör. iki alakasız emoji `👍👍`) taban karakterin
  glyph'iyle eşleşiyor; kutu değil, yarım glyph değil.
- `make hepsi` yeşil; mevcut atlas sınamaları bit bit aynı.

## Checklist

- [ ] Feature'lar eklendi, `Cargo.lock` değişmedi
- [ ] `Sprite::Cluster` + interner + `slot` kolu
- [ ] `CTLine` şekillendirme + kapı + taban karaktere düşüş
- [ ] Test: beş dizi çift yuva, önbellek, şekillenmeyen dizgi
- [ ] Doğrulama geçti (`make hepsi`)
