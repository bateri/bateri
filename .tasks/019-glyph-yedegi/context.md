# Glyph yedeği — Bağlam

## Mevcut Durum

`bt-atlas` glyph'leri CoreText ile rasterize ediyor ve atlası sabit yuva
ızgarasında paketliyor. Font seçimi **iki kademeli** ve ikisi de `font.rs`'in
başındaki iki sabitte yaşıyor:

- `PREFERRED: [&str; 1] = ["SF Mono"]` — tercih sırası; bulunamayan ad
  **sessizce** atlanıyor.
- `FALLBACK: &str = "Menlo"` — garanti taban, macOS'un her sürümünde kurulu.
  Ayrı sabit olmasının sebebi tip düzeyinde güvence: zincir boş dönemiyor.

Yani **aile düzeyinde bir yedek zaten var ve çalışıyor**: istenen aile yoksa
zincir bir sonrakine düşüyor.

**Karakter düzeyinde yedek yok.** Aile seçildikten sonra her glyph o tek
fonttan aranıyor (`font.rs:355`, `glyphs_for_characters`) ve o çağrının
arkasındaki `CTFontGetGlyphsForCharacters` **cascade list'e düşmüyor** —
fontta olmayan karakter `.notdef` (0) veriyor. `bt-atlas` bunu görünce
`TOFU` yuvasına düşüyor (`lib.rs:43`): "sessiz kayıp (glyph hiç çizilmez)
yerine görünür kayıp (kutu çizilir)".

Eksiklik **biliniyordu**: `lib.rs:521`'in `UNKNOWN_CHAR` sabiti doc'unda
"Menlo ve SF Mono CJK içermez ve `CTFontGetGlyphsForCharacters` başka fonta
düşmez" yazıyor. Borç olarak hiç yazılmamıştı; `docs/YOL-HARITASI.md`'ye
2026-09-20'de girdi (`10ee82b`).

## Motivasyon

Kullanıcı 2026-09-20'de ekran görüntüsüyle bildirdi: bateri'de Claude Code
açınca sol alttaki `⏵⏵ auto mode on` göstergesi **iki boş kutu** olarak
çıkıyor.

Bu bir parite açığı, tasarım tercihi değil: iTerm2, Terminal.app ve ghostty
karakter düzeyinde yedek yapıyor. Ok, sembol ve kutu-çizim karakterleri
TUI'lerde ve prompt'larda her yerde, yani belirti günlük kullanımda sürekli
görünüyor.

**Emoji ile karıştırılmamalı.** Yedekten gelen glyph de tek kanallı bir
kapsama maskesi, yani `R8Unorm` atlas olduğu gibi kalıyor ve 020'nin "ikinci
atlas mı, RGBA mı" çatalı bu sette **açılmıyor**. Geniş glyph (çift hücre) de
kapsam dışı — `⏵` tek hücre.

## Kanıt

Üçü de bu makinede (macOS 26.4.1), CoreText'e `ctypes` ile **doğrudan**
soruldu; 13 pt.

**1. Hangi karakter hangi ailede var** (`CTFontGetGlyphsForCharacters`,
`.notdef` mi değil mi):

| Karakter | Menlo | "SF Mono" adıyla açılan |
|---|---|---|
| `⏵` U+23F5 | **YOK** | YOK |
| `→` U+2192 | VAR | YOK |
| `↻` U+21BB | VAR | YOK |
| `░` U+2591 | VAR | YOK |
| `▁` U+2581 | VAR | YOK |

Ekran görüntüsünde `→`, `↻` ve `░` **düzgün çiziliyor**, yalnız `⏵` kutu —
yani etkin font Menlo davranışı gösteriyor ve belirti tek karakterde.

**2. "SF Mono" adıyla gerçekte ne açılıyor** (`CTFontCopyFamilyName`):

| İstenen | Gerçekte açılan |
|---|---|
| `SF Mono` | **Helvetica** |
| `Menlo` | Menlo |
| `Bu Aile Yok 12345` | Helvetica |

SF Mono bu makinede **kurulu değil** (Xcode ile geliyor) ve CoreText onu var
olmayan bir aile gibi ele alıp Helvetica veriyor. `font.rs`'in zinciri bunu
tasarlanmış bir geri düşüş olarak karşılıyor ve Menlo'ya iniyor — yani ilk
tablodaki "SF Mono" sütunu aslında **Helvetica**'nın cevabı ve o sütunun
tamamen YOK çıkması beklenen bir sonuç, ayrı bir kusur değil.

Bu, `settings.toml`'un "aile verilmezse SF Mono" yorumunun bu makinede
**pratikte Menlo** anlamına geldiğini de gösteriyor. Yorum yanlış değil
(zincir gerçekten SF Mono'yu tercih ediyor) ama eksik; set kapanırken
netleştirilmesi ucuz.

## Mevcut Mimari

```
bt-gpu (kare yolu)
  └─ Atlas::glyph(char, face, size_class)      ← kare başına binlerce çağrı
       ├─ yuva haritasında var mı?  ──── evet ──→ yuva numarası
       └─ yok ──→ rasterize:
                    font.glyphs_for_characters(..)   (font.rs:355)
                      └─ CTFontGetGlyphsForCharacters
                           ├─ glyph > 0 ──→ CGBitmapContext'e çiz (R8)
                           └─ glyph == 0 (.notdef) ──→ TOFU (yuva 0)
```

Zincirde **tek font** var: `Atlas` hangi aileyi açtıysa bütün karakterler
ondan aranıyor.

**Bir dönem burada "yedek gelince yuva anahtarına gerçek fontun kimliği
girmek zorunda" yazıyordu; panel üç mercekten birden çürüttü ve cümle
kalktı.** Anahtar bir **soru**, yuva bir **cevap**: aynı atlas ömründe
`(karakter, yüz, boy)` her zaman aynı yedeğe çözülüyor (taban font sabit →
`for_string` sabit), ve taban değişince `Atlas::ensure` atlası **komple**
yeniden kuruyor (`lib.rs:232`). Yani aynı anahtarın iki farklı fonta
çözüldüğü bir hâl **yok**.

Deponun kendi emsali de bunu söylüyor: `Faces::effective` ve
`NoGlyph → Regular` kolu bugün "istenen anahtar → **başka bir yüzün** çizdiği
yuva" kaydediyor ve `font.rs:178`'in doc'u açıkça yazıyor: *"istenen yüz ile
çizilen yüz aynı olmak zorunda değil."* Karakter yedeği o merdivenin üçüncü
basamağı, yeni bir anahtar ekseni değil. Anahtar bugünkü hâliyle kalıyor —
sıfır bayt büyüme, sıcak yolda sıfır ek hash maliyeti.

## Kod taraması (2026-09-20)

Depoda doğrulananlar — her biri `dosya:satır`:

- **Yuva anahtarı** `HashMap<(Sprite, Face, SizeClass), u16>` (`lib.rs:130`) ve
  üç tipin de `#[repr(u8)]` olması **bilerek**: "hash'e giren her bayt kare
  başına hücre başına ödeniyor" (`raster.rs:97`, `font.rs:32`, `font.rs:53`).
  Font kimliği anahtarta **yok**.
- **Atlas beş `CTFont` tutuyor**: dört yüz (`Faces`, `font.rs:95`) artı bağlam
  satırının küçük yüzü (`lib.rs:112`).
- **`.notdef` kontrolünün gerçek yeri** `font.rs:364`
  (`(glyphs[0] != 0).then_some(glyphs[0])`); `raster.rs:40` onu `NoGlyph`'e
  çeviriyor.
- **Yüz düzeyinde geri düşüş zaten var** (`lib.rs:405-418`): `Bold`'da
  bulunamayan glyph `Regular`'a düşüyor ve **istenen** yüzün anahtarı da
  haritaya yazılıyor. Karakter düzeyindeki yedek bu merdivenle **kompoze
  olmak zorunda** — altına mı üstüne mi gireceği bir karar.
- **Negatif önbellek var** (`lib.rs:419-439`): tofu'ya düşen karakter
  haritaya yazılıyor, tavan kapasitenin iki katı, tavan dolunca yalnız
  negatif kayıtlar atılıyor. Varlık sebebi sıcak yol: `Atlas::slot` **ana
  thread'de, display link callback'inin içinde** koşuyor.
- **Bağımlılık gerekmiyor:** `objc2-core-text 0.3.2` (Cargo.lock'ta sabit)
  `CTFont::for_string`'i zaten sunuyor (`CTFontCreateForString`'in bağlayıcısı)
  ve `CTFontCreateForStringWithLanguage` de bağlı. `CLAUDE.md`'nin "`bt-atlas`
  `objc2` çekirdeğini görmez, kullanılan her şey C API'si" sözleşmesi
  korunuyor.
- **Rasterizasyon tek kanal**: `CGImageAlphaInfo::Only`, 8 bit, dolgusuz
  (`raster.rs:52`); GPU tarafı `R8Unorm` (`renderer.rs:1282`). `raster::draw`
  zaten `font: &CTFont` alıyor (`raster.rs:33`), yani başka bir fontu çizdirmek
  **imza değişikliği bile istemiyor**.
- **Atlas punto/ölçek değişiminde komple yeniden kuruluyor** (`Atlas::ensure`,
  `lib.rs:232`): yuvalar geçersizleşmiyor, yok oluyor. Yedek fontların ömrü de
  buna bağlanmalı ve aynı `effective_point_size` ile açılmalı (`lib.rs:474`).

## Tasarımı değiştiren iki bulgu

**1. Emoji riski — kapsam sınırı sandığım kadar temiz değil.** context'in
üstünde "yedekten gelen glyph de tek kanallı maske" yazıyor ve bu **yalnız
yedek renkli bir fonta düşmediği sürece** doğru. `CTFontCreateForString` bir
emoji kod noktası için **Apple Color Emoji** döndürür; `R8Unorm` atlas onu
sessizce tek renkli bir **siluete** çevirir. Yani kapsamı "emoji 021'de" diye
yazmak yetmiyor — yedek yolunun renkli fontu **aktif olarak elemesi** gerekiyor
(`kCTFontTraitColorGlyphs`), yoksa 021'in işini yarım ve yanlış yapmış oluruz.
Belirti sessiz: emoji kutu yerine gri bir şekil olur.

**2. Dört sınama kırılacak** ve hepsi aynı varsayım üstünde: "bu karakter
hiçbir şeye çözülmez".

| Sınama | Varsayımı |
|---|---|
| `tofu_box_is_drawn_and_resident` (`lib.rs:598`) | `'漢'` → TOFU |
| `unknown_char_is_cached` (`lib.rs:615`) | `'漢'` → TOFU ve haritada |
| `negative_cache_is_capped_and_evicted` (`lib.rs:835`) | CJK havuzunun tamamı TOFU |
| `non_bmp_char_path_works` (`lib.rs:885`) | `'𝔸'` → TOFU |

Yedek gelince `'漢'` PingFang SC'ye, `'𝔸'` de bir matematik fontuna çözülür
(ölçüldü). Bu sınamaların **hepsi yeniden kurulmalı** ve yerlerine gerçekten
hiçbir fontta olmayan bir kod noktası (tahsis edilmemiş bir PUA ya da
`U+0E0100` gibi) konmalı. İşin en görünmeyen maliyeti bu: negatif önbelleğin
bütün sınama altyapısı "hiçbir şey çözülmez" üstüne kurulu.

## Panelin yakaladığı gerileme (2026-09-20)

**Bugün CJK sıfır yuva harcıyor.** Tanınmayan karakter negatif önbelleğe
yazılıyor (`lib.rs:419-439`) ve `TOFU` rezident — `unknown_char_is_cached`
bunu `occupancy().0 == 1` ile sabitliyor. `lib.rs`'in kendi yorumu da bu
tavanı gerekçelendiriyor: *"Bir ikili dosyayı `cat`'lemek milyonlarca ayrı
codepoint üretebilir… crate'in tavanı olmayan tek sayısı burasıydı."*

**Yedek bunu tersine çevirebilir.** Her ayrı hanzi gerçek bir yuva yerse ve
atlas dolarsa (`next >= cap`, `lib.rs:337`) `slot()` o andan itibaren **her
yeni karakteri** — Latin harfler dahil — tofu'ya düşürüyor ve önbelleklemiyor.
Tahliye yok; tek kurtuluş atlasın yeniden kurulması (punto/ölçek/aile
değişimi). Aritmetik: @2x Menlo 13pt'de ızgara **2048 yuva**, yaygın hanzi
listesi tek başına 3500 karakter.

Yani set, **nadir bir tofu'yu (`⏵`) yaygın bir tofu'ya çevirme riski**
taşıyor. Aşağıdaki genişlik kapısı bunu kapatıyor (CJK reddediliyor, negatif
önbellekte kalıyor) — yani kapı bir zevk kararı değil **kapasite koruması**.

## Açık Sorular

- ~~Yuva anahtarına eklenecek font kimliği~~ — **çürütüldü** (yukarıda).
- ~~Yedek araması cache'lenmeli~~ — **zaten cache'li**: arama `NoGlyph`
  yaprağının içine girerse `slots` haritası onu anahtar başına atlas ömründe
  **bir kez**e kilitliyor. Yeni tip, yeni tavan, yeni tahliye politikası
  gerekmiyor; pozitif ve negatif önbelleğin ikisi de yerinde duruyor.
- **Dar glyph hücrenin neresine?** `raster.rs` bugün `x = 0.0` yazıyor ve bu
  monospace varsayımından geliyordu. `⏵` hücreden **dar** (0.84×), yani sola
  yapışık mı çizilecek ortalanacak mı — açık.
- **Genişlik kapısının ölçütü advance mi mürekkep kutusu mu?** Kırpan şey
  mürekkep, ama mürekkep kutusu glyph'e özgü ve ölçülmemiş bir tolerans
  sayısı ister.
- `TOFU` kararı **kendiliğinden korunmuyor, bir kapıya bağlanmak zorunda.**
  Panelin ölçümü: `CTFontCreateForString`'in **başarısızlık kipi yok** —
  atanmamış her kod noktası `.LastResort`'tan glyph 4 alıyor, yani
  `glyph != 0` ve `Drawn` koluna düşüyor. Kapı olmazsa `lib.rs:43`'ün
  "fontun `.notdef`'ine güvenmiyoruz, kendi kutumuzu çiziyoruz" kararı arka
  kapıdan sessizce delinir.
