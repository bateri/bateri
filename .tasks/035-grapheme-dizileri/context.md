# Grapheme dizileri — Bağlam

## Mevcut Durum

Bir hücre bir kod noktası çiziyor. Zincirin üç halkası da bunu varsayıyor:

- **Izgara** `alacritty_terminal` 0.26'nın `Term::input`'u: her kod noktası
  `unicode-width`'in **karakter** genişliğiyle yerleşiyor; genişliği `0`
  olan önceki hücrenin `CellExtra.zerowidth`'ine iniyor, `1` ve `2` kendi
  hücresini (geniş olan `WIDE_CHAR` + spacer) alıyor. Dizi kavramı yok.
- **Sınır** `bt_core::Cell.ch: Option<char>` taban karakteri taşıyor;
  `Session::frame` `zerowidth`'i okumuyor. Dock `BUFFER`'ı karakter karakter
  yürüyor (`dock::layout_with`, `dock::column_width` — 024) ve sıfır
  genişlikli kod noktası orada da hücre almıyor. Tazelik kapısının ayna
  tarafı sıfır genişlikli kod noktasını atlıyor (`shell.rs`, 024'ün
  üçüncü ölçütü).
- **Atlas** anahtarı `Sprite::Char(char)` (`bt-atlas/src/lib.rs`); raster
  `CTFontGetGlyphsForCharacters` ile tek glyph alıyor, yani şekillendirme
  (ligatür) yok. Geniş glyph'in iki yarısı ve renk düzlemi 023'ten
  (`CLAUDE.md` → 023 paragrafı); grapheme dizileri orada "kapsam dışı ve
  adıyla yazılı" (`.tasks/023-emoji-ve-genis-glyph/discussion.md` → Karar,
  madde 1–2).

## Motivasyon

Kullanıcı gördü (2026-09-25): Claude Code'un çıktısında
`🇹🇷 "YouTube için…"` ve `🇬🇧 "A Chrome extension…"` satırlarının başındaki
bayraklar bateri'de yan yana **iki kutu** çıkıyor. Talep: grapheme dizilerini
— bölgesel gösterge (RI) çiftleri, ZWJ dizileri (`👨‍👩‍👧`), ten rengi
(`👍🏽`), VS16 (`❤️`, `☺️`) — tek glyph olarak, "temiz ve güzel" çizmek.

### Ölçülen: ön teşhisin doğrulanması

Sürücünün ön teşhisi "ikinci kod noktası `CellExtra`'da, sink onu okumuyor"
idi. **Yarısı yanlış** ve yanlış yarısı kararın özü. `alacritty_terminal`
0.26 + `unicode-width` 0.2.2'ye her diziyi basıp ızgarayı okuyan bir sınama
(scratchpad, `Processor::advance` → `grid()[..]`) şunu verdi:

| dizi | ızgaranın yazdığı | ızgara sütunu | `UnicodeWidthStr::width` |
|---|---|---:|---:|
| `🇹🇷` | `🇹` · `🇷` — **iki ayrı dar hücre**, `zerowidth` boş | 2 | 2 |
| `❤️` (`U+2764 FE0F`) | `❤` dar, `FE0F` `zerowidth`'te | 1 | 2 |
| `☺️` | aynı | 1 | 2 |
| `👍🏽` | `👍` geniş + `🏽` **ayrı geniş hücre** | 4 | 2 |
| `👨‍👩‍👧` | üç geniş hücre, ilk ikisinin `zerowidth`'i `ZWJ` | 6 | 2 |
| `🏳️‍🌈` | `🏳` dar (`FE0F`, `ZWJ` `zerowidth`'te) + `🌈` geniş | 3 | 2 |
| `e` + `U+0301` | `e`, aksan `zerowidth`'te | 1 | 1 |

- **Bayrak**: iki RI iki **ayrı** hücre ve her biri tek sütunluk
  (`unicode-width` RI'ye tek tek `1` veriyor). Sütun toplamı uygulamanınkiyle
  **aynı** (2) — bozuk olan yalnız glyph. Tek RI'nin Apple Color Emoji
  glyph'i bir hücreye sığmıyor (Menlo 16pt'de ölçülen mürekkep hücrenin
  1.68 katı) ve kapıdan dönüp kutu oluyor: iki kutu = iki RI.
- **ZWJ, ten rengi, VS16**: ızgaranın sütun sayısı uygulamanınkinden
  **farklı**. `UnicodeWidthStr::width` dizileri biliyor (ailenin tamamı 2,
  `👍🏽` 2, `❤️` 2, `a` + `🏽` 3, `a‍b` 2, dört RI 4 — ölçüldü), yani
  `unicode-width` dizinin genişliğini **zaten** hesaplayabiliyor;
  alacritty onu yalnız karakter karakter soruyor.
- Uygulamalar dizi genişliğini sayıyor: Claude Code'un arayüzü (ink,
  `string-width`) `👍🏽`'ı 2 sütun sayıp arkasındakini 2 sütun sonraya
  yerleştiriyor, ızgara 4 sütun ilerliyor — satırın geri kalanı kayıyor,
  çerçeveler kırılıyor. Bu **bugün de** böyle; bayrak yalnız sütunların
  tesadüfen tuttuğu tek aile.

Sonuç: yalnız atlası ya da sink'i düzeltmek bayrağı onarır ama ötekileri
**onaramaz** — iki hücrelik bir glyph tek sütuna (`❤️`) sığmaz, altı
sütunluk bir aile tek glyph'le çizilince arkasında dört sütunluk delik kalır.
Talebin "tek glyph" dediği şey, uygulamanın saydığı sütunla birlikte
geldiğinde anlamlı; yani bu set bir **ızgara** setidir, atlas işi onun
parçası.

### Ölçülen: alacritty'nin sınırı

- `EventLoop` `Term<U>`'yu somut tipte tutuyor ve baytları
  `state.parser.advance(&mut **terminal, …)` ile doğrudan `Term`'e veriyor
  (`event_loop.rs`, 486 satır). Yani `Handler::input`'un araya girilecek bir
  kancası **bugünkü** okuyucu döngüsünde yok; kancayı kurmanın yolu döngünün
  sahibi olmak. Bayt yeniden yazma (`TappedPty`'de) çıkış değil: hiçbir bayt
  dizisi alacritty'ye bir RI'yi komşusunun `zerowidth`'ine koydurmuyor.
- `Cell::push_zerowidth` bir üst sınır koymuyor (`Vec`), yani on kod
  noktalı `🧑🏻‍❤️‍💋‍🧑🏼` da tek hücreye sığar.
- Seçimin metni `zerowidth`'i içeriyor (`term/mod.rs`, satır metni),
  arama (`RegexIter`) içermiyor — yalnız `cell.c`.

### Ölçülen: şekillendirme

`CTLineCreateWithAttributedString` (Menlo 16pt, `CTFontCreateForString`
cascade'i) örneklerin **hepsinde** Apple Color Emoji'den **tek glyph**
veriyor: `🇹🇷 🇬🇧 👨‍👩‍👧 👍🏽 ❤️ ☺️ 🏳️‍🌈 🌡️ 1️⃣`. Ölçüt olarak alınacak tek çıkarım
bu: dizi glyph'inin geometrisi tek kod noktalı emojinin **aynısı** (ilerleme
ve mürekkep örneklerin hepsinde birebir aynı çıktı) ve o emoji 023'ten beri
iki hücrelik kapıdan geçiyor. Sayının kendisi kapı değil — ölçü
`CTLineGetImageBounds`, kapınınki `font::ink_fits_cell`.

`CTLine`/`CTRun` `objc2-core-text` 0.3.2'de feature (`CTLine`, `CTRun`),
`CFAttributedString`/`CFDictionary` `objc2-core-foundation`'da feature —
yeni crate değil. `unicode-segmentation` grafta **yok** (`cargo tree -i`).

### Referans

`docs/ARASTIRMA.md` → Terminal davranışı: Metalterm'in hücresi 20 bayt ve
"emoji, grapheme kümeleri … yan tablolarda" — referans ürün dizileri hücrenin
yan tablosunda tutuyor; sütun modeline dair kayıt yok.
