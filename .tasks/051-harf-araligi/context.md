# Harf aralığı (`[font] letter_spacing`) — Bağlam

## Mevcut Durum

Hücrenin iki ölçüsü iki ayrı yerden geliyor ve yalnız biri ayarlanabiliyor:

- **Yükseklik** fontun `ascent + descent + leading`'inden, `[font]
  line_height` çarpanıyla (`bt_atlas::rules::cell_metrics`; fazlalık glyph'in
  altına ve üstüne eşit dağılıyor). Ayar, gerekçesi ve sınırı
  `docs/AYARLAR.md` → `[font]`'ta; aralık `bt_core::LINE_HEIGHT_RANGE`
  (`1.0..=2.0`, alt uç `g j p q y`'nin kuyruğu, üst uç atlas bütçesi).
- **Genişlik** boşluk karakterinin kesirli ilerlemesinden
  (`rules::space_advance`) ve **ayarı yok**. Aynı sayı `Atlas::cell_advance`
  olarak iki tüketiciye daha gidiyor: glyph'i hücrede ortalayan
  `rules::centre_shift` ve yedek glyph'in mürekkep kapısı (`rules::accept`,
  `ink_fits_placed`). Izgaranın adımı yuvarlanmış hâli (`Metrics::cell_px.0`).
  Bağlam satırının küçük sınıfı kendi ilerlemesini aynı fonksiyondan alıyor
  (`Atlas::context_advance`, `context_cell_w`).

Atlasın anahtarı (`bt_atlas::Key`) aile, punto, ölçek ve `line_height`;
anahtar değişince atlas ve dokular yeniden kuruluyor (`Atlas::ensure`,
`Renderer::cell_metrics`). Punto değişimi (Cmd +/−, `bt-shell-common::zoom`)
çarpanı olduğu gibi taşıyor. Ayar penceresinde Line height bir `Number`
denetimi (`settings_window`). PTY'nin sütun sayısı, fare eşlemesi, caret,
seçim, dock'un sütun bütçesi ve yordamsal sprite'lar hücre ölçüsünü
`Metrics`'ten okuyor; ikinci bir genişlik kaynağı yok.

## Motivasyon

Kullanıcının bir arkadaşı harflerin arasında daha çok boşluk istiyor
("font horizontal space"). Eşaralıklı ızgarada harf aralığı demek hücre
genişliği demek: harfin boyu aynı kalıyor, sütunlar açılıyor. Başka
terminallerdeki karşılıkları: iTerm2'de Profiles ▸ Text ▸ "Horizontal
spacing" (yüzde), Ghostty'de `adjust-cell-width`, Alacritty'de
`font.offset.x`, Kitty'de `modify_font cell_width`. Metalterm envanterinde
(`docs/ARASTIRMA.md`) karşılığı yok.

`line_height` sette yatay karşılığını taşıyacak bütün parçalar hazır: hücre
genişliği tek bir sayıdan türüyor, glyph hücrede zaten **evrensel olarak**
ortalanıyor (`CLAUDE.md` → Proje, eşaralıklı fontta kaydırma bugün sıfır), yani
genişleyen hücrede harf kendiliğinden ortada duruyor. Yordamsal karakterler
(kutu çizgileri, bloklar, alt çizgiler) hücreyi boydan boya doldurduğu için
geniş hücrede de birbirine bitişik kalıyor.
