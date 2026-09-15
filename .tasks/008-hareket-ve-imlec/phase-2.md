# Phase 2 — İmlecin altındaki metin piksel işi olur

## Özet

İmlecin piksel dikdörtgeni `cell` pipeline'ına uniform olarak geçer; blok
altındaki glyph ve kural çizgileri rengini oradan alır. `bt-core` ters çevirme
**kararını** vermeye devam eder, rengi `Cursor` ile sınırdan geçirir. Görsel
sonuç bu phase'de birebir bugünküdür.

_Requirements: R2.1, R2.2, R2.3_

## Değişiklikler

- **`crates/bt-core/src/session.rs`** — `Cursor` "blok altındaki metnin
  rengi"ni taşır (lineer RGBA; bugünkü değer temanın zemini). `frame()`'in
  imleç hücresini ters çeviren dalı (`fore`/`underline_color`'ı ezen `let`)
  kalkar: hücre artık kendi renkleriyle sınırdan geçer. Ters video, seçim ve
  SGR 58 yolları **dokunulmadan** kalır; imleç bloğu onların üstünde bir
  piksel kararı olur. `Cursor`'ın ve imleç bloğunun doc'ları (`Cursor`,
  `frame()`'in ilgili paragrafı, `bt-core/src/lib.rs` başlığı) bu ayrımı
  yazar: **karar burada, boyama orada**. (`Cursor`'ın `Eq`'ü düşer — renk
  `f32`; emsali `FontOptions`.)
- **`crates/bt-gpu/shaders/cell.metal`** — iki yeni uniform: imlecin piksel
  dikdörtgeni ve blok altındaki metin rengi. Fragment, dörtlünün kendi piksel
  konumundan dikdörtgen testini yapar ve içerideyse rengi ezer (kapsama/alfa
  yolu değişmez: `rgba.a * coverage` aynı kalır). Görünmez imleç, dejenere bir
  dikdörtgenle temsil edilir — shader'da ikinci bir bayrak yok.
- **`crates/bt-gpu/src/renderer.rs`** — `encode_glyphs` yeni uniform'ları
  `vertex_uniform` emsaliyle **fragment** aşamasına bağlar; `#[repr(C)]`
  karşılığı Rust tarafında `static_assert`'lerle çivilenir (iki taraf kendi
  assert'iyle, `Instance`/`GlyphInstance` emsali).
- **`crates/bt-gpu/src/frame.rs`** — `push_cursor` dikdörtgeni **ve** metin
  rengini saklar; `Frame` ikisini de renderer'a verir. İmleç dikdörtgeni
  `bg_count`'a girmemeye devam eder (`hucre=8` sessizce `9` olmasın).
- **Taşınan sınamalar** — `char_under_cursor_is_drawn_inverted` ve
  `cursor_cell_drops_the_underline_color` artık `bt-core`'da yanlış yerde:
  ikisi de `bt-gpu`'nun offscreen sınamalarına iner ve **piksel** üstünden
  sorar.

## Kabul

- Offscreen sınama: imlecin durduğu hücrenin harfi ve kural çizgisi, bu
  phase'den önceki pikselle aynı renkte; `rule_over_cursor_stays_visible`
  yeşil kalır.
- Offscreen sınama: dikdörtgenin **dışındaki** hücreler etkilenmez (yarım
  örtme bu phase'de imkânsız — dikdörtgen hücreye oturuyor — ama sınama
  hücrenin komşusunu da okur, phase-3 onu kendiliğinden kullanır).
- `make duman` sayaçları değişmez: `hucre=8 glif=6 kural=15`.
- `make shader` yeşil; MSL ve Rust düzeni alan alan aynı.

## Yayın Etkisi

**shader** — `.metal` değişti: `make shader` koşar, uniform yapısı Rust
`#[repr(C)]` karşılığıyla alan alan doğrulanır. terminfo yok · ayar şeması yok
· tema yok · shell entegrasyonu yok · app bundle yok · yeni bağımlılık yok.

`CLAUDE.md`'nin "Bugünkü hâl" paragrafı imleç bloğunun nerede çözüldüğünü
söylüyorsa bu commit'te düzelir. Ölçüm iddiası yok: fragment başına bir
dikdörtgen testinin maliyeti **ölçülmedi** ve iddia edilmiyor.

## Checklist

- [ ] `Cursor` metin rengini taşır; `frame()`'in ters çevirme dalı kalkar
- [ ] `cell.metal`: iki uniform + dikdörtgen testi; `static_assert`'ler
- [ ] `renderer.rs`: fragment uniform'ları, Rust tarafı assert'leri
- [ ] `frame.rs`: `push_cursor` dikdörtgen + renk; `bg_count` değişmez
- [ ] İki sınama `bt-gpu`'ya taşındı ve piksel üstünden soruyor
- [ ] Test: imleç altındaki harf ve kural çizgisi piksel olarak bugünküyle aynı
- [ ] Doğrulama geçti (`make hepsi` + `make shader` + `make duman`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
- [ ] Yayın etkisi yazıldı
