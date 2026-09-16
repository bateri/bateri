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

## Uygulama Notları

- **Uniform tek `#[repr(C)]` yapı** (`CursorBlock { rect, rgba }`), iki ayrı
  binding değil. "`static_assert`'ler" cümlesi ancak bir yapı için anlamlı:
  iki `float4` ayrı bağlansaydı çivilenecek bir ofset kalmazdı. Fragment
  aşamasının `[[buffer(0)]]`'ı — indeksler vertex'inkinden ayrı bir alan,
  gerekçesi `fragment_uniform`'un doc'unda.
- **`char_under_cursor_is_drawn_inverted` taşınmadı, tersine çevrildi.**
  `bt-core`'da kalan yeni sınama
  (`cursor_carries_the_text_color_and_leaves_cells_alone`) R2.1'i koruyor:
  karar sınırdan geçiyor **ve** hücre dokunulmadan kalıyor. Sebep: geri dönüş
  (hücreyi yine `bt-core`'da ters çevirmek) `bt-gpu`'nun piksel
  sınamalarından **geçerdi** — iki kez çevrilen renk aynı piksele varır ve
  belirti yalnız yarım örtülen hücrede, yani phase-3'te çıkardı.
- **Sınırın yarı açıklığı sınanmıyor** ve iddia da edilmiyor. `<` yazıldı ama
  `<=` ile ayrışmıyor: `[[position]]` fragment **merkezini** veriyor (x + 0.5)
  ve hiçbir fragment tam sınıra düşmüyor. Mutasyonla doğrulandı (kapalı sınır
  hiçbir sınamayı kırmadı); shader yorumu ve sınama bunu söylüyor.
- **`cursor_rect_stops_at_its_own_cell` glyph yerine kural bandına bağlandı.**
  İlk hâli komşu hücrenin `M`'inde tam kaplanan piksel arıyordu ve kırmızı
  düştü: varsayılan puntoda o glyph'in hiçbir pikseli `0xff`'e varmıyor. Kural
  bandı hücre genişliğince tam kaplıyor, yani sınır sorusu fontun hangi
  pikseli boyadığına bağlı kalmıyor.
- **`rule_over_cursor_stays_visible`'a `text = WHITE` verildi**: o sınamanın
  sorduğu şey çizim **sırası**, renk değil; iddiaları bit bit korundu.
- **İki doc daha düzeldi** (phase'in saydığı üçün dışında): `Cell::fg` ve
  `Cell::underline_color` kalkan dalı anlatıyordu ("imlecin altındaki hücrede
  ters" / "her zaman `None`").
- **`CLAUDE.md` dokunulmadı**: "Bugünkü hâl" paragrafı imleç bloğunun nerede
  çözüldüğünü söylemiyor, yani düzeltilecek bir çelişki yok.
- **Bekçiler mutasyonla doğrulandı**: shader'ın ezme dalı kapatılınca iki
  sınama kırmızı, dikdörtgen iki hücre genişletilince iki sınama kırmızı.

## Checklist

- [x] `Cursor` metin rengini taşır; `frame()`'in ters çevirme dalı kalkar
- [x] `cell.metal`: iki uniform + dikdörtgen testi; `static_assert`'ler
- [x] `renderer.rs`: fragment uniform'ları, Rust tarafı assert'leri
- [x] `frame.rs`: `push_cursor` dikdörtgen + renk; `bg_count` değişmez
- [x] İki sınama `bt-gpu`'ya taşındı ve piksel üstünden soruyor
      (biri tersine çevrilip `bt-core`'da kaldı, bkz. Uygulama Notları)
- [x] Test: imleç altındaki harf ve kural çizgisi piksel olarak bugünküyle aynı
- [x] Doğrulama geçti (`make hepsi` + `make shader` + `make duman`:
      `hucre=8 glif=6 kural=15`, `icerik=1`)
- [x] Riskli phase: `/code-review` koştu — bu phase'in diff'inde bulgu yok
      (piksel uzayı zinciri, fragment binding alanı, dejenere dikdörtgen ve
      alfa yolu ayrı ayrı doğrulandı). Tek bulgu setin **phase-1** kodunda
      (`app.rs` kapanış yolunda ölçüm kapısı kapalıyken saat okuması) ve kendi
      commit'iyle indi.
- [x] Yayın etkisi yazıldı
