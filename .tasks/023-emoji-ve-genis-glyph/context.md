# Emoji ve geniş glyph — Bağlam

## Mevcut Durum

Atlas **tek kanallı bir kapsama maskesi**: doku `R8Unorm`, `cell_fragment`
örneklediği tek kanalı alfa olarak okuyor ve rengi instance'tan alıyor
(`crates/bt-gpu/shaders/cell.metal`: "atlas glyph başına bir maske tutuyor,
bir görüntü değil"). Yuvalar **sabit ızgarada** ve her yuva tam bir hücre
boyunda; yuva anahtarı `(Sprite, Face, SizeClass)` ve doku kenarı hedeflenen
yuva sayısından türüyor (`SLOT_TARGET` = 1024, 022).

Çizim yolunun iki sözleşmesi bu seti doğrudan bağlıyor:

- **`GlyphInstance` 32 bayt ve boyut taşımıyor.** `pos`, `uv0`, `rgba`;
  `cell_px` ile `uv_size` kare başına birer **uniform**. Gerekçe shader'ın
  kendi yorumunda yazılı ("bu sette her glyph tam bir hücre boyunda") ve
  stride iki tarafta ayrı ayrı `static_assert` ile çivili — Rust'ta
  `GlyphInstance`, MSL'de aynı adın `sizeof`'u. Aynı sınıra 012 de çarpmıştı:
  prompt işareti `>` "bugünkü `Frame`'de temsil edilemiyor, çünkü pay glyph
  almıyor ve `GlyphInstance`'ın boyu kare başına tek uniform".
- **Tek liste, tek draw call.** `AtlasTexture::prepare` glyph'leri ve kural
  çizgilerini aynı `instances` listesine basıyor; sıra bilerek (üstü çizili
  harfin üstünden geçmeli) ve `encode_pass` bunu arka planlardan ve caret'ten
  sonra kodluyor.

Izgara tarafında **geniş karakter zaten iki sütun**: alacritty hücreye
`WIDE_CHAR`, komşusuna `WIDE_CHAR_SPACER` basıyor ve `Session::frame` spacer'ı
mürekkepsiz sayıyor (glyph vermiyor, arka planını geniş karakterin
şablonundan alıyor). Yani **sütun aritmetiği hazır**; eksik olan glyph'in iki
hücreyi birden boyaması.

Kapı 019'un **mürekkep kapısı** (`font::ink_fits_cell`): cascade'den gelen
aday, çizileceği yerde bir hücrenin dışına boyuyorsa kutu kalıyor. Kapının
argümanı **tek hücrenin** ilerlemesi (`cell_advance`) ve emoji ile CJK aynı
kapıdan eleniyor — aile adı karşılaştırması ya da trait biti yok.

## Motivasyon

İki eksik, tek mimari çatal. Emoji renkli bir bitmap ve `R8Unorm` dokuda
yaşayamıyor; geniş glyph iki hücre genişliğinde bir dörtgen istiyor ve
`GlyphInstance` bunu ifade edemiyor. İkisi tek sette, çünkü **2 sütunlu
emoji ikisini birden istiyor**: renk *ve* iki hücrelik geometri.

Borç 003'ten beri kayıtlı (`teslim.md` B.3: "geniş karakter tek yuvaya
kırpılıyor") ve 004'ün `plan.md`'si ikisini aynı sete bağlamış. Sıra
2026-09-22'de kullanıcı kararıyla materyal yüzeyin önüne geçti
(`docs/YOL-HARITASI.md` → on üçüncü kayma).

### Ölçüm — envanter (2026-09-22)

Bu makine, Menlo 16pt@2x, hücre ilerlemesi **19.266 px**. Tarama
`bt-atlas`'ın içinden geçici bir sınamayla koştu: her kod noktası için taban
fontta glyph var mı, yoksa cascade hangi adayı veriyor
(`CTFont::for_string`), adayın ilerlemesi ve **mürekkebi** hücrenin kaç katı,
ve bugünkü kapı (`font::fallback_font`) onu kabul ediyor mu. Sütun sayısı
Unicode'un `East_Asian_Width`'inden (`W`/`F` → 2), yani ızgaranın
`unicode-width` üzerinden kullandığı ölçütle aynı kaynak. Taranan aralıklar
019/021'in envanterindekiler artı CJK ve emoji blokları
(U+3000–30FF, U+4E00–4E7F, U+1F300–1F5FF, U+1F600–1F64F, U+1F900–1F9FF,
U+FF00–FF60): **3521 kod noktası, 2559'u kutu.**

Kutu kalanların adayına göre dağılımı:

| aday fontu | 1 sütun | 2 sütun | toplam |
|---|---:|---:|---:|
| Apple Color Emoji | 78 | **922** | 1000 |
| `.LastResort` (makinede font yok) | 825 | 0 | 825 |
| Hiragino Sans | 31 | 204 | 235 |
| PingFang SC | 0 | 220 | 220 |
| Apple Symbols | 116 | 12 | 128 |
| STIX Two Math | 104 | 0 | 104 |
| Zapf Dingbats | 30 | 0 | 30 |
| Arial Unicode MS, Hiragino Sans GB, diğer | 7 | 10 | 17 |

Üç sayı kapsamı belirliyor:

1. **Adayı olan her 2 sütunlu karakterin ilerlemesi tam olarak hücrenin
   1.66 katı** — emoji, PingFang ve Hiragino'da aynı, dağılım değil tek
   değer (1.66..1.66). Yani mürekkebi **iki hücreye sığıyor: 1346/1346.**
   Kapının argümanını `cell_advance * 2` yapmak bu ailenin tamamını kabul
   ediyor; ölçüm bunu varsayım değil sayı olarak veriyor.
2. **Emojinin 78'i 1 sütunlu** (`🌡 🌤 🌦 🎖 🎙 🏋 🏔` …) ve mürekkebi
   **1.66 hücre**, yani tek hücreye sığmıyor. Bunlar rengi olan ama iki
   sütunu **olmayan** karakterler: geometri kolu onlara yardım etmiyor ve
   çareleri ayrı bir karar (küçültme).
3. **65 karakter 2 sütunlu ve bugün çiziliyor** — 21'i Menlo'nun kendi
   glyph'i (`◽ ◾ ☔ ☕ ♈…♓ ♿ ⚓ ⚡`), 44'ü cascade'den narin mürekkeple
   geçenler (`丨 、 。 》 」 ！ １ Ｉ ｜`). Bugün sol hücreye çiziliyorlar,
   spacer boş. Geniş yolu **ızgaranın sütun sayısına** bağlayan bir tasarım
   bu 65'in görüntüsünü de değiştirir — kusuru değil, *bugün çalışan* bir
   çizimi. Kararın ölçütü bu yüzden "geniş ilan edilmiş mi" ile "geniş
   boyuyor mu" arasında.

`.LastResort`'un 825'i (içinde U+1FB00–1FBFF'in tamamı) bu setin konusu
değil: orada kutu **fontun yokluğu** ve çaresi 021'in yordamsal ailesinin
devamı (`docs/YOL-HARITASI.md` → sete bağlanmamış borçlar).
