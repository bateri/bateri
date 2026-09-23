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

- [x] `glyph_fx.rs` + birim sınamaları
- [x] `FxInstance` + `glyph_fx.metal` + pipeline + `build.rs`
- [x] `Frame` fx listeleri, `suppress_dock`, `prepare`'in yelpazelemesi
- [x] `link.rs` iki kare yolu, uyku terimi, `finish` kapsamı
- [x] Pencereleme kayması (plan R1.3, kullanıcı kararı): phase-1'in kayma → `Reset` kolu sütun farkına dönüşür (`bt-core`, `window_skip`), `GlyphFx` uçuştaki efektleri o kadar kaydırır; sınama: taşan satırda sona yazmak ve Backspace canlanıyor, efektler yeni pencerede doğru sütunda
- [x] `CLAUDE.md`
- [x] Test: R5 değişmezleri (fade, recede)
- [x] Doğrulama geçti (`make hepsi` + `make shader` + `make duman`)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi
- [~] Gözle kontrol — koşamadı: debug derlemesi paketsiz bir binary ve
  computer-use onu uygulama olarak tanımıyor (`request_access` "installed
  değil" dedi). Kullanıcıya: dock'ta yazınca harfler beliriyor, Backspace'le
  küçülüp gidiyor; taşan uzun satırın sonunda da; yazıp bekleyince harf tam
  renkte kalıyor (yarı saydam asılı kalmıyor).

## Uygulama Notları

- **`DockEdit`'e dördüncü varyant ve `shift` alanı** (R1.3 kullanıcı
  kararı): `Arrive`/`Erase` `shift: i32` taşıyor (eski pencerenin attığı
  sütun eksi yenisininki, sağa pozitif), metni aynı ama pencereyi kayan
  ayna `DockEdit::Shift { by }` basıyor — uçuştakiler bitmiyor, yalnız
  kayıyor. phase-1'in `shifted → Reset` kolu kalktı. Kayan girdi metnin
  sütunlarından (`[DOCK_TEXT_COL, cols)`) taşarsa düşüyor; bunun için
  `dock::TEXT_COL` `pub` oldu ve `DOCK_TEXT_COL` adıyla ihraç ediliyor.
  `EditCells` `FromIterator<Cell>` kazandı (yalnız `bt-gpu` sınamaları kuruyor).
- **Tampon ivar değil yerel `Option`:** karede en çok bir düzenleme var ve
  yalnız içerik karesi okuyor; ikinci sink `frame`'i ödünç almadığı için
  `LinkIvars`'a alan gerekmedi.
- **Susturma `dock_glyphs`'i değiştirmiyor**, ayrı bir `dock_shown` listesi
  kuruyor: hareket karesi dock'u yeniden basmıyor, efekt bitince statik
  glyph oradan geri geliyor. `owns(col, ch)` yerine `GlyphFx::retain`.
- **Uyku terimi `advance`'ten önceki hâle bakıyor** (`link::at_rest`):
  efektin bittiği kare çizilmeden uyunsaydı son çizilen kare yarı saydam bir
  harf olarak ekranda kalırdı. İkinci uyku noktası (faz karesinden sonra)
  `advance`'ten sonraki `is_empty`'yi soruyor.
- **Encode sırası planla aynı, bedeli bölünen çağrı:** uçuşta geliş varken
  dock glyph'leri ve kuralları iki `encode_glyphs` çağrısına bölünüyor
  (glyph → gelişler → kurallar); yoksa bugünkü tek çağrı. Bölünmeseydi altı
  çizili bir harfin çizgisi efekt bitince harfin altından üstüne sıçrardı.
- **`prepare`'in yelpazelemesi `fan`'a çıktı**; `prepare_fx` aynı gövdeden
  geçiyor. Maske dokusunun kurulumu `ensure_texture`'a çıktı.
- **Paket `f32` tam sayı** (`kimlik | düzlem << 5 | yarı << 6`), bit kalıbı
  değil: küçük kalıp denormal sayılıp `flat` aktarımda sıfırlanabilirdi.
- `/code-review` (tek bulgu, giderildi): hareket karesinde çizim hatası
  `GlyphFx`'i bitiriyor ama `Frame`'in efekt listelerini bırakıyordu; sıradaki
  hasarsız kare onları donmuş çizerdi — artık listeler de boşalıyor.
- `motion::DT_MAX` `pub(crate)` oldu (`GlyphFx::advance` aynı kırpmayı
  kullanıyor); `DisplayLink::motion_settled` efektleri de soruyor.
- Süreler: geliş 0,12 s, hayalet 0,16 s; `recede` 0,6 ölçeğe küçülüyor;
  `FX_MAX = 32` — hepsi tasarım sabiti.
- **Gözlenen, bu phase'in değil:** atlasın negatif önbelleği geniş bir tofu
  karakterine ilk soruluşta tek hücre (`Whole`), önbellekten iki yarı veriyor
  (`bt_atlas::Atlas::slot`'un önbellek kolu `half: want` dönüyor); renkli
  font kurulu olmayan ortamda `🎉` sınaması önbelleği ısıtarak bunu atlıyor.
