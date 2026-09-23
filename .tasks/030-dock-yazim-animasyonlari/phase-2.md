# Phase 2 — `GlyphFx`, `glyph_fx` pipeline'ı, `fade` ve `recede`

## Özet

Düzenlemeleri tüketen animatör, beşinci pipeline ve ilk iki efekt: dock'ta
yazılan harf `fade` ile gelir, silinen `recede` ile gider (ayar anahtarı henüz
yok, sabit varsayılan); `snap` ve Hareketi Azalt indirgemesi `bt-gpu`'da.

_Requirements: R3, R3.1, R3.2, R3.3, R3.4, R4, R5, R6, R10_

## Değişiklikler

- **`crates/bt-gpu/src/glyph_fx.rs`** (yeni, saf) — `GlyphFx`: sabit kapasiteli
  girdi listesi (`FX_MAX`, tasarım sabiti; dolunca en eski biter),
  `KEYPRESS_DURATION` / `ERASE_DURATION` (tasarım sabiti, doc'larında
  "seçilmiş, ölçülmüş değil"), `apply(edit)` (`Reset` → hepsi biter; yeni
  düzenlemenin sütununa eşit ya da sağındaki gelişler biter; yeni girdiler),
  `advance(dt)`, `finish()`, `is_empty()`, `owns(col, ch)` (susturma sorusu),
  iki efekt enum'u (`KeypressFx`, `EraseFx` — bu phase'de `Off`/`Fade`,
  `Off`/`Recede`) ve indirgeme: `snap` → ikisi `Off`, `reduce` → geliş `Fade`,
  hayalet `Off` (`discussion.md` → Karar 7; `Motion::mode`'un yanında, stil ve
  `reduce` oradan okunur). Sınamalar ObjC'siz.
- **`crates/bt-gpu/src/motion.rs`** — indirgemenin sorusu için stil/`reduce`
  okuyucusu (gerekirse); `Motion`'ın `Copy`'si ve alanları değişmez.
- **`crates/bt-gpu/src/frame.rs`** — `FxInstance` (`#[repr(C)]`: `pos`, `uv0`,
  `rgba`, `fx: [f32; 4]` = t, paketli kimlik/düzlem/yarı, tohum, yedek;
  `size_of`/`offset_of` assert'leri); iki liste (`dock_ghosts`, `dock_arrivals`,
  `GlyphCell` + fx parametreleri, uv'siz — `GlyphCell`'in gerekçesi);
  `set_dock_fx` (içerik ve hareket karesinin ortak yazıcısı) ve
  `suppress_dock` (render'dan sonra `dock_glyphs`'ten uçuştaki gelişi çıkarır;
  eşleşmeyen girdi biter). Sayaçlara (`glyph_count` vs.) girmezler.
- **`crates/bt-gpu/shaders/glyph_fx.metal`** (yeni) — `glyph_fx_vertex`
  (dörtlüyü efekt payı kadar şişirir, geniş glyph'te iki hücrelik kutunun
  merkezi) ve `glyph_fx_fragment` (ters dönüşüm, yuva sınır testi, iki doku,
  düzlem seçimi, `CursorBlock` karışımı); `static_assert`'ler. Efektler bu
  phase'de `fade` (alfa) ve `recede` (merkeze küçülme + sönme); `t = 1`
  dalı statik yolla aynı aritmetiğe iner.
- **`crates/bt-gpu/build.rs`** — dokunulmaz: `shaders/` dizini izleniyor ve
  her `.metal` derleniyor, yeni dosya kendiliğinden giriyor.
- **`crates/bt-gpu/src/renderer.rs`** — beşinci pipeline; `prepare`'in
  yelpazeleme yolu fx listelerini de besler (ikinci kopya yok, yarı biti
  instance'a); `encode_dock` sırası: zemin → `dock_bg` → caret → hayaletler →
  emoji → `dock_glyphs` → gelişler → kurallar.
- **`crates/bt-gpu/src/link.rs`** — `LinkIvars`'a `glyph_fx: RefCell<GlyphFx>`
  ve `dock_edits` tamponu (doldurma bandının `fill` emsali); içerik karesinde
  tampon → `apply` → `suppress_dock` → `set_dock_fx`; hareket karesinde
  `advance` → `set_dock_fx`; uyku testine adlı terim (`fx.is_empty()`, blink'in
  "üçüncü soru" yorumunun yanında); `Motion::finish`'in üç çağıranı
  (`set_visible(false)`, `snap`'e geçiş, senkron çizim hatası) fx'i de bitirir;
  modül başlığındaki kare talebi listesine fx girer (hareket yolu, `Waker`'a
  dokunmaz).
- **`CLAUDE.md`** — dock animasyonlarının sözleşmesi (fark nerede, toplu
  değişim kuralı, `GlyphFx` ve uyku terimi, beşinci pipeline), pipeline
  sayısı "dört" → "beş", emoji blend cümlesi koda göre düzeltilir
  (`SourceAlpha`, ön çarpım yüklemeden önce geri alınıyor).

## Kabul

- `glyph_fx` birim sınamaları: `Reset`, sütun kuralı, `FX_MAX` taşması, süre
  dolunca boşalma, `finish`, indirgeme tablosu (`snap`, `reduce`).
- Offscreen GPU sınamaları (R5), her efekt için döngüyle (bu phase'de iki):
  geliş `t = 1`'de statik glyph'le piksel piksel aynı; hayalet `t = 1`'de düz
  zemin; komşu yuvası dolu bir glyph komşuyu hiç örneklemiyor; geniş glyph'in
  iki yarısı tek kutu olarak ölçekleniyor (`recede` ortasında iki yarı
  simetrik).
- Link sınaması ya da saf uyku yüklemi: fx doluyken uyumuyor, boşalınca
  uyuyor, hasar dikmiyor.
- `make duman` yeşil ve jetonlar değişmedi (dock'suz reçete).
- Ara karelerin doğruluğu yalnız gözle: dock'ta yazınca harfler beliriyor,
  Backspace'le küçülüp gidiyor; emoji ve CJK dock'ta bütün olarak canlanıyor;
  Cmd-V, ↑ ve Ctrl-U anında. Izgara ve doldurma bandı değişmedi.

## Checklist

- [ ] `glyph_fx.rs` + birim sınamaları
- [ ] `FxInstance` + `glyph_fx.metal` + pipeline + `build.rs`
- [ ] `Frame` fx listeleri, `suppress_dock`, `prepare`'in yelpazelemesi
- [ ] `link.rs` iki kare yolu, uyku terimi, `finish` kapsamı
- [ ] `CLAUDE.md`
- [ ] Test: R5 değişmezleri (fade, recede)
- [ ] Doğrulama geçti (`make hepsi` + `make shader` + `make duman`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
