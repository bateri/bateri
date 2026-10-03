# Taşan glyph: aralıkları `1`'in altına açmak — Bağlam

## Mevcut Durum

`[font] line_height` ve `letter_spacing` (051) yalnız `1`–`2` arasında kabul
ediliyor (`bt_core::LINE_HEIGHT_RANGE`, `LETTER_SPACING_RANGE`). Alt sınırın
sebebi çizim yolunda: **yuva = hücre = GPU dörtgeni**.

- **Atlas.** Yuvanın boyu `Metrics::cell_px` (`bt-atlas/src/rules.rs`
  `cell_metrics`). Rasterlama yuva boyunda bir tampona yapılıyor ve dışarı
  düşen her piksel kırpılıyor: CoreText'te bitmap bağlamının sınırında
  (`coretext.rs` `draw_mask`), FreeType'ta blit döngüsünde (`freetype.rs`).
  `raster::position`'ın yorumu bunu sözleşme olarak yazıyor: "komşuya taşmak
  mümkün değil, hedef tam bir yuva".
- **GPU.** `GlyphInstance` 32 bayt ve boyut taşımıyor. `cell.wgsl`'in
  `cell_vertex`'i dörtgeni `pos + corner × cell_px`'ten, uv'yi `uv0 + corner ×
  uv_size`'tan kuruyor, yani dörtgen tam bir hücre. `emoji_fragment` aynı
  vertex'i paylaşıyor. Örnekleyici `nearest` ve yuvalar arasında dolgu yok:
  dörtgen yuvadan geniş olsaydı komşu yuva örneklenirdi.
  - `glyph_fx` kendi şişkin dörtgenini kuruyor (`FX_PAD`) ve yuva dışını hiç
    örneklemiyor (`glyph_fx.wgsl` `paint`).
- **Çizim sırası** (`Renderer::plan`):
  - Izgarada önce bütün hücre zeminleri, sonra arama, seçim, caret, en son
    glyph ve kurallar çiziliyor. Yani **aynı yüzeyde** komşu satırın zemini
    taşan bir glyph'i örtmez.
  - Doldurma bandı ızgaradan sonra kendi zeminiyle çiziliyor. Dock en son ve
    opak zeminle çiziliyor. Scissor yalnız dock bandı büyürken var.
  - Caret'in ters çevirmesi piksel başına bir dikdörtgen testi
    (`cell_fragment`), yani dörtgenin hücre olmasına bağlı değil.
- **Hasar** tek bir bayrak. Her içerik karesi bütün listeyi baştan kuruyor ve
  pass `Clear` ile başlıyor, yani taşan bir glyph'in bayat kalacağı satır
  hasarı yok.
- **Yedek glyph kapısı** (`rules::accept`) yatay mürekkebi hücre kutusuna
  ölçüyor. Kutu ya tam glyph ya küçültülmüş glyph veriyor; "taşan glyph"
  kavramı bugün yok.
- **Geniş glyph** iki yuvaya bölünüyor (`Half`): sağ yarı `cell_px.0` kadar
  kaydırılıp soldan kırpılıyor, iki dörtgen yan yana.

## Motivasyon

Kullanıcı `line_height = 1`'de bile satırların arasında boşluk görüyor ve bunu
daraltmak istiyor. iTerm2 iki aralığı da `100`'ün altına indirmeye izin
veriyor. Orada harf kesilmiyor, komşu satıra ya da sütuna taşıyor.

Ölçüm (Menlo 13pt, CoreText, @1x, 2026-10-03, bu oturumda `swift` betiğiyle):

- Font 12.07 pt yukarı, 3.07 pt aşağı istiyor; leading 0.
- ASCII'nin mürekkebi en çok 10.64'e çıkıyor.
- `Ä`/`É`/`İ` tam 12.07'ye çıkıyor.

Yani `1`'deki boşluk büyük harf aksanlarının payı. Bugün alt sınırı `1`'in
altına açmak bu aksanları ve `g y p` kuyruklarını **keserdi**. Bunun sebebi
özelliğin kendisi değil, "yuva = hücre" aritmetiği.
