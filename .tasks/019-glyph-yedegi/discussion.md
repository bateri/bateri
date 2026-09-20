# Glyph yedeği — Tartışma

Tek tasarım sorusu var — **eksik glyph hangi yoldan aranacak** — ve üç yol
karşılaştırılıyor. Üçünün de ortak şartı aynı: arama **cache'lenmeli** ve
`TOFU` kararı korunmalı.

## Ölçüm (2026-09-20, bu makine, macOS 26.4.1, 13 pt)

CoreText'e `ctypes` ile doğrudan soruldu.

**1. `CTFontCreateForString` gerçekten buluyor mu** (taban: Menlo):

| Karakter | Dönen aile |
|---|---|
| `⏵` U+23F5 | **STIX Two Math** |
| `漢` U+6F22 | **PingFang SC** |
| `→` U+2192 | Menlo (zaten var) |
| `█` U+2588 | Menlo (zaten var) |

Yani yol çalışıyor ve zaten var olan karakterde tabanı **değiştirmiyor**.

**2. Cascade list uzunluğu** (`CTFontCopyDefaultCascadeListForLanguages`,
Menlo, dil verilmeden): **42 descriptor**.

**3. Maliyet:** `CTFontCreateForString` **3.9 µs/çağrı** (2000 çağrı, ctypes
ek yüküyle — Rust'ta daha ucuz olur, yani bu bir **tavan**).

**Maliyetin doğru okunuşu:** 3000 hücrenin hepsi her karede sorulsaydı
11.8 ms/kare ederdi ve bu tek başına 60 fps'i yerdi — ama o senaryo
**gerçekçi değil**: `Atlas` zaten yuva cache'i tutuyor ve arama yalnız
**ilk kez görülen** karakterde koşuyor. Yedek de aynı cache'in arkasına
girerse maliyet karakter başına bir kez, oturum ömrü boyunca. Sayının
söylediği şey "yol pahalı" değil, **"cache'siz uygulanamaz"**.

**4. Yedek ne buluyor ve renkli mi** (`CTFontGetSymbolicTraits`,
`kCTFontTraitColorGlyphs` biti):

| Karakter | Bulunan aile | Renkli mi |
|---|---|---|
| `⏵` U+23F5 | STIX Two Math | tek kanal |
| `漢` U+6F22 | PingFang SC | tek kanal |
| `𝔸` U+1D538 | STIX Two Math | tek kanal |
| `🎉` U+1F389 | **Apple Color Emoji** | **RENKLİ** |
| `📁` U+1F4C1 | **Apple Color Emoji** | **RENKLİ** |
| `█` U+2588 | Menlo (zaten var) | tek kanal |

Süzgeç **çalışıyor**: renkli fontlar tek bir bitle ayırt edilebiliyor.

**5. `CTFontCreateForString` hiçbir zaman "bulamadım" demiyor:**

| Kod noktası | Dönen aile |
|---|---|
| U+E0100 (variation selector) | PingFang HK |
| U+F8FF (PUA, Apple logosu) | Monaco |
| U+FFFFF, U+10FFFD, U+2FFFE | **`.LastResort`** |

Tahsis edilmemiş kod noktaları bile `.LastResort`'a düşüyor — Apple'ın
"kutu içinde kategori simgesi" çizen özel fontu. Yani **yedek her zaman bir
font buluyor** ve "çözülemedi" diye bir hâl kendiliğinden doğmuyor.

## Seçenek A: `CTFontCreateForString` ile karakter başına yedek

Eksik glyph görülünce macOS'un kendi cascade mekanizmasına sorulur; dönen
`CTFont` o karakterin gerçek fontudur.

**Artıları:**
- macOS'un kararı: script tespiti, dil tercihi ve sistem font önceliği bizde
  değil işletim sisteminde. `漢` → PingFang SC ölçümü bunun çalıştığını
  gösteriyor.
- Az kod: tek çağrı, zaten CoreText'e bağlıyız.
- Zaten var olan karakterde tabanı değiştirmiyor (ölçüldü), yani sıcak yol
  davranışı aynen korunuyor.

**Eksileri:**
- Her çağrı `CFString` + `CTFont` üretiyor; cache olmadan kullanılamaz
  (ölçüldü).
- Dönen fontun **kimliğini** yuva anahtarına çevirmek gerekiyor; aile adı
  yeterli mi, yoksa descriptor kimliği mi — açık soru.

## Seçenek B: Cascade list'i bir kez alıp kendimiz gezmek — **DÜŞTÜ**

> Panel (üç mercek birden) B'nin baş satış argümanını çürüttü: "kimlikler
> küçük, zincir indeksi bir `u8` olarak anahtara girer" **olmayan bir
> problemin çözümü** (bkz. Karar Noktası 2). Geriye 42 descriptor açmak, dil
> seçmek ve macOS'un script/dil aklını elle taklit etmek kalıyor — karşılığı
> olmadan. Codebase-fit ayrıca ölçtü: B `CFArray` feature'ını workspace
> `Cargo.toml`'una yazdırır, A ise **sıfır Cargo değişikliği** ister.
> Aşağıdaki gövde kayıt için duruyor.


`CTFontCopyDefaultCascadeListForLanguages` ile 42 descriptor alınır, fontlar
önceden açılır, eksik glyph sırayla aranır.

**Artıları:**
- Font nesneleri önceden açık: arama sırasında ayırma yok.
- Kimlikler sabit ve küçük — yuva anahtarına **zincirdeki indeks** girebilir
  (bir `u8`), aile adı taşımaya gerek kalmaz.
- Hangi fontların denendiği görünür, yani tanısı kolay.

**Eksileri:**
- 42 fontu açmak ve her eksik karakterde sırayla taramak, macOS'un tek
  çağrıda yaptığı işi elle yapmak demek — ve onun script/dil aklı bizde yok.
- Cascade list **dile bağlı** (`ForLanguages`); hangi dili vereceğimiz yeni
  bir karar ve yanlışı sessiz (Çince/Japonca aynı kod noktasında ayrışır).
- 42 sayısı bu makinede ölçüldü; başka kurulumda değişir, yani döngünün
  maliyeti öngörülemez.

## Seçenek C: Kullanıcının ayarlayabileceği yedek liste

`[font] fallback = ["Symbols Nerd Font", "Apple Symbols"]`.

**Bu bir alternatif değil, A ya da B'nin üstüne bir katman.** Tek başına
yetmez: liste boşken davranış yine tanımsız kalır ve kullanıcının kurulumunu
bilmediğimiz için varsayılan yine sistemden gelmek zorunda.

**Artıları:**
- Nerd Font kuran kullanıcı ikonları sisteme sormadan bağlayabilir.
- Öngörülebilir ve tanısı kolay.

**Eksileri:**
- Tek başına UX'e aykırı: çalışması için kullanıcıdan kurulum istiyor, oysa
  beklenti "kutudan çıktığı gibi doğru çizsin".
- Ayar şeması, canlı izleme ve doğrulama yükü getiriyor (016'nın yolu).
- Kapsamı büyütüyor; asıl şikâyet (`⏵` kutu çıkıyor) A ya da B ile zaten
  kapanıyor.

## Karar Noktaları

1. **Hangi yol** — A, B, yoksa "A şimdi, C sonra"?

**1b. Kabul kapısı ne olacak?** → ✅ **Tek yüklem: glyph hücreye sığıyor mu.**

Panel (Sadelik) üç ayrı süzgeç yerine **tek geometrik ölçüt** önerdi ve
ölçüm onu doğruladı. Bağımsız doğrulama (2026-09-20, Menlo 13pt, hücre
**7.827 px**):

| Karakter | Yedek font | advance | oran | Karar |
|---|---|---|---|---|
| `⏵` U+23F5 — **şikâyetin kendisi** | STIX Two Math | 6.54 | 0.84× | **KABUL** |
| `✓` U+2713, `⚠` U+26A0, `▶` U+25B6 | Menlo (zaten var) | 7.83 | 1.00× | KABUL |
| U+F8FF (PUA, Apple logosu) | Monaco | 7.80 | 1.00× | KABUL |
| `𝔸` U+1D538 | STIX Two Math | 8.41 | 1.07× | RED → TOFU |
| `漢` U+6F22 | PingFang SC | 13.00 | 1.66× | RED → TOFU |
| U+E0B0, U+10FFFD (atanmamış/PUA) | `.LastResort` | 14.30 | 1.83× | RED → TOFU |
| `🎉` U+1F389 | Apple Color Emoji | 17.00 | 2.17× | RED → TOFU |

**Tek `u16` kıyası üç kapıyı birden kapatıyor** ve üçünün de ayrı gerekçesi
var:

- **Emoji** (2.17×) — `R8Unorm` atlasta gri siluete dönüşmesi engellenmiş
  oluyor; 021'in çatalı gerçekten açılmıyor.
- **`.LastResort`** (1.83×) — her tanınmayan kod noktası yuva yemiyor, yani
  `context.md`'nin "panelin yakaladığı gerileme" bölümündeki kapasite riski
  kapanıyor. Kapı bir zevk kararı değil **kapasite koruması**.
- **Geniş glyph** (CJK 1.66×) — `bt-core` zaten iki sütun tutuyor; tek hücreye
  kırpılmış bir hanzi **sessiz** bozulma olurdu, kutu ise görünür eksiklik
  (`lib.rs:43`'ün ilkesi).

**Sihirli dizge yok:** `.LastResort` için aile adı karşılaştırması, emoji için
trait biti **gerekmiyor**; ölçüt geometrik ve makineden bağımsız.

**Görünür bedel, kullanıcıya bir cümleyle söylenmeli:** CJK, emoji ve geniş
oklar bugünkü gibi kutu kalıyor. Çift hücre 021'in işi.

2. ~~**Yuva anahtarına ne girecek?**~~ → ✅ **Hiçbir şey; anahtar
   değişmiyor.** Üç mercek de aynı gerekçeyle kapattı: anahtar bir soru,
   yuva bir cevap; aynı atlas ömründe `(karakter, yüz, boy)` hep aynı yedeğe
   çözülüyor ve taban değişince `Atlas::ensure` her şeyi yeniden kuruyor.
   Emsal `font.rs:178`: "istenen yüz ile çizilen yüz aynı olmak zorunda
   değil." Sıfır bayt büyüme.
3. **Negatif sonuç cache'leniyor mu?** Hiçbir fontta olmayan karakter
   (`TOFU`'ya düşen) her karede yeniden aranmamalı.
4. **Yüz merdiveniyle sıra:** bugün `Bold`'da bulunamayan glyph önce
   `Regular`'a düşüyor (`lib.rs:405-418`). Karakter yedeği bu merdivenin
   **altına mı üstüne mi** girecek? Üstüne girerse Menlo-Bold'da olmayan ama
   Menlo-Regular'da olan bir karakter yabancı bir fonttan gelir ve kalınlık
   sessizce kaybolur; altına girerse merdiven önce tüketilir ve yedek yalnız
   gerçekten hiçbir yüzde olmayan karakterde koşar. Öneri: **altına**.
5. **Kırılacak sınamalar:** dördü de "bu karakter çözülmez" varsayımı üstünde
   (`context.md` → Tasarımı değiştiren iki bulgu). Yeniden kurulmaları bu
   setin kapsamında ve phase bölmesi bunu hesaba katmalı.
6. **Ölçüm borcu:** kare süresi tabanı bu sette de alınmıyor mu? Yedek sıcak
   yola girmiyor (yalnız ilk görülen karakterde koşuyor), yani `/measure`
   şartı bu set için **zayıf** — ama "ölçülmemiş sayı yazılmaz" gereği
   teslim belgesine "ölçüm bekliyor" satırı girmeli mi?

## Muhakeme (2026-09-20)

| Mercek | Verdict |
|---|---|
| Sadelik / YAGNI | **SORUNLU** |
| Codebase-fit | **SORUNLU** |
| İşletme | **SORUNLU** |

Üçü de yönü (Seçenek A) onayladı; üçü de **öncülleri** çürüttü. Hiçbiri
KIRMIZI değil — düzeltme "başka yol seç" değil, "belgeyi düzelt + kapı ekle".

### Kabul edilen itirazlar

1. **"Font kimliği anahtara girmek zorunda" yanlış öncüldü** (üç mercek
   birden). Karar Noktası 2 kapandı, Seçenek B düştü, `context.md` düzeltildi.
2. **"TOFU korunuyor" ve "emoji kapsam dışı" koşulsuz güvence olarak
   yazılıydı ve ölçüm ikisini de yanlışlıyor.** `CTFontCreateForString`'in
   başarısızlık kipi yok; `.LastResort` her koda cevap veriyor, emoji `Drawn`
   koluna giriyor. İkisi de artık **kapıya bağlı**.
3. **Kapasite gerilemesi** (Sadelik + İşletme): bugün CJK sıfır yuva
   harcıyor, yedek onu gerçek yuvaya çevirip atlası doldurabilir ve o
   oturumda **her yeni karakter** tofu olur. `context.md`'ye ayrı bölüm
   olarak girdi; genişlik kapısı onu kapatıyor.
4. **Metrik uyumu planda yoktu** (İşletme + Codebase-fit): `raster::draw`
   monospace varsayıyor (`x = 0.0`), yedek bunu iki yönde birden bozuyor.
   Genişlik kapısı geniş yönü kapatıyor; **dar** yönü (`⏵` 0.84×, sola
   yapışık mı ortalı mı) açık kalem olarak yazıldı.
5. **Süzgeç ile arama aynı phase'de inmek zorunda** (İşletme): "arama var,
   süzgeç yok" ara durumu `make hepsi`'yi yeşil bırakamaz — TOFU hiç
   üretilemediği için üç sınama yeniden **kurulamaz** bile. Phase bölmesine
   girdi.
6. **`SizeClass::Small` hiç anılmamıştı** (Codebase-fit): yedeğin tabanı
   `self.small` olmalı, yoksa dock'un bağlam satırındaki yedek glyph'ler
   gösterim puntosunda çıkar. Genişlik kapısının sınırı da `context_cell_px`.
7. **"3.9 µs bir tavan" yanlıştı** (İşletme): sayı ısınmış cache'ten geliyor,
   yani bir **taban**. Yeni bir font ailesinin soğuk ilk açılışı ana thread'de
   ödeniyor ve ölçülmedi → `teslim.md`'ye ölçüm borcu.

### Reddedilen / kapsam dışı bırakılan

- **Genişlik kapısının ölçütü mürekkep kutusu olsun** (Codebase-fit'in açık
  kalemi): **advance seçildi.** Mürekkep kutusu glyph'e özgü ve ölçülmemiş
  bir tolerans sayısı ister ("ölçülmemiş sayı yazılmaz"); advance fontun
  kendi sözleşmesidir ve deterministik. Bedeli ölçüldü ve kabul edildi: `𝔸`
  (1.07×) kıl payı reddediliyor. Yanlışın yönü güvenli — reddedilen karakter
  kutu olur, kırpık çizilmez.
- **`make duman`'ın jeton satırına karakter eklemek** (İşletme'nin kendi
  önerisi de bunu reddediyor): üç sınama ve üç tarihsel kayıt `hucre=8
  glif=6 kural=15`'e bağlı; sözleşme değişimi ayrı commit ister. Yerine
  `bt-atlas` birim sınaması + `teslim.md`'ye "duman bu yolu koşmuyor" bilinen
  sınırı.
- **Seçenek C** (kullanıcı ayarlı liste): zaten "alternatif değil katman"
  diye geri çekilmişti; panel de eklenecek bir şey bulmadı. Bu setin dışında
  kalıyor ve gerekçesi güçlendi — ayar anahtarı **geri alınamaz** ("bilinmeyen
  anahtar asla silinmez").
