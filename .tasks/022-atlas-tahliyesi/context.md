# Atlas tahliyesi — Bağlam

## Mevcut Durum

`bt-atlas`'ın `Atlas`'ı 1024×1024 piksellik **tek** `R8Unorm` dokuyu sabit
yuva ızgarasına bölüyor: `grid = (1024 / hücre_genişliği, 1024 /
hücre_yüksekliği)`, kapasite ikisinin çarpımı. Yuva dağıtımı **bump**:
`next: u16` birden başlıyor (`TOFU = 0` rezerve) ve her yeni anahtarda bir
artıyor. Geri alan yok.

Anahtar `(Sprite, Face, SizeClass)` ve `slots: HashMap` hem **pozitif**
kayıtları (gerçek yuva) hem **negatif** olanları (fontun tanımadığı karakter
→ `TOFU`) taşıyor. Negatif tarafın kendi tavanı var
(`negative_cache_cap` = kapasitenin iki katı) ve dolunca **toptan**
boşaltılıyor (`retain(|_, &mut slot| slot != TOFU)`) — yani negatif önbellek
zaten bir tahliye biçimi tanıyor, pozitif taraf tanımıyor.

Kurallara pay ayrılmış: `RULE_RESERVE = 7` yuvayı karakterler yiyemiyor, yani
atlas dolsa da altı çizili hücrenin çizgisi tofu'ya düşmüyor.

Dolduğunda ne oluyor: `self.next >= cap` kolunda `slot()` `(TOFU, None)`
dönüyor ve bu kararı **önbelleğe yazmıyor**. Yazmama bilinçli ve gerekçesi
kodda duruyor — dolu atlas fontun kalıcı bir gerçeği değil atlasın geçici
hâli, tahliye gelince o kayıt yanlış olurdu. Kolun yorumu tahliyeyi adıyla ve
yer tutucu numarasıyla (`00X`) bekliyor; `TEXTURE_EDGE`'in doc'u da öyle
("Tahliye yok — dolan atlas tofu'ya düşer, LRU 00X'in işi").

Sonuç: atlas dolduğu andan itibaren o oturumda **ilk kez görülen her
karakter** kutu çıkıyor. Kendiliğinden düzelmiyor ve kurtulma kapısı dar:
`Atlas::ensure` yalnız `(family, point_size, scale, line_height)` dörtlüsünü
karşılaştırıyor, yani atlası baştan kuran tek şey **font/punto/ölçek/satır
aralığı değişimi**. **Pencere yeniden boyutlandırmak atlası kurmuyor** —
resize bu dörtlüden hiçbirini değiştirmiyor ve `sync_atlas` dokuyu yalnız
`ensure` `true` dediğinde düşürüyor. Yani kullanıcının elindeki tek çare
puntoyu oynatmak (Cmd +/−/0) ya da pencereyi başka bir ekrana taşımak.

## Motivasyon

021 bu eşiği hem **yaklaştırdı** hem **ölçülebilir** kıldı. Blok elemanları,
Braille, çizgi çizim ve teknik küme artık fonttan değil yordamsal
çiziliyor ve hepsi yuva harcıyor. Braille'in 256'sı bu setten **önce sıfır**
harcıyordu (genişlik kapısından dönüp negatif önbelleğe düşüyordu, o önbellek
ise tavanlı ve toptan boşaltılıyor); bugün 256 gerçek yuva istiyor.

Borcun sahibi yol haritasında adıyla yazılı ("Atlas dolunca geri dönüşü yok
ve 021 eşiği yaklaştırdı") ve sırası da: **önce ölçüm, sonra LRU.** Ölçüm
2026-09-21'de koştu, yani sıradaki adım bu set.

Belirtinin kullanıcı yüzeyi iki kapıdan geliyor ve ikisi de sıradan:

1. **Punto.** Varsayılan 13pt, Cmd + adımı 1pt (`zoom::STEP`), tavan 72pt
   (`zoom::MAX_SIZE`). Retina'da doyma eşiği 29pt, yani **16 basış**. Kırılma
   bölgesinde hiçbir uyarı yok ve uygulama 72'ye kadar bırakıyor.
   `settings.toml`'a `font_size = 28` yazan kullanıcı oraya doğrudan düşüyor.
2. **İçerik.** Braille bloğunu tarayan bir TUI (`btop`'un grafikleri, Claude
   Code'un spinner'ı) tek başına 256 yuva isteyebiliyor.

## Kanıt

Ölçüm `docs/OLCUMLER.md` → `## Atlas yuva ayak izi` (2026-09-21, commit
`51e1459`, taban `b17fa78`; debug ile release birebir aynı sayı). Buraya
kopyalanmıyor, sahibi orası; kararı ilgilendiren iki satır:

- **Yordamsal ailenin kendisi 421 yuva** (021 öncesi 160), tofu ile 422.
- **Retina'da doyma eşiği 29pt:** kapasite 406, aile sığmıyor. 28pt'de
  kapasite 450, yani aile sığıyor ama geriye **24 yuva** kalıyor — kullanıcının
  kendi metni için yer yok. Ölçek 1'de eşik 58pt; kırılma noktası hücrenin
  piksel boyu, puntonun kendisi değil.

Puntoya göre kapasite (aynı ölçümden): 13pt → 1984, 20pt → 800, 26pt → 512,
28pt → 450, **29pt → 406**, 56pt → 105.

Ayak izi bir **tavan**, bir maliyet değil: yuvalar istendikçe harcanıyor, yani
aileyi hiç kullanmayan içerik 421'i ödemiyor. Kırılma o yüzden "29pt'ye
çıkınca hemen" değil, "29pt'ye çıkıp aileyi tarayan bir şey koşunca".

## Mevcut Mimari

```
bt-atlas::Atlas                           bt-gpu::AtlasTexture
  slots: HashMap<(Sprite,Face,Size), u16>   texture: MTLTexture (R8Unorm)
  next: u16   ← bump, geri alma yok         prepare(glyphs, rules) her karede
  grid: (w, h)                                └─ resolve() → atlas.slot(...)
  capacity() = grid.0 * grid.1                     ├─ (slot, None)  → uv
                                                   └─ (slot, Some(bitmap))
                                                        → upload_slot()
                                                           replaceRegion
```

Her kare her hücre için `resolve()` çağrılıyor, yani yuva numarası kare
verisinde **saklanmıyor**, her karede yeniden çözülüyor. Bu tahliye için iyi
haber (kareler arası bayat yuva referansı yok) ama bir tuzak da taşıyor:
tahliye edilen anahtar bir sonraki karede yeniden isteniyor, yani çalışma
kümesi kapasiteyi aşarsa her kare her şeyi yeniden rasterize eder.

## Karara giren iki kısıt

**1. `replaceRegion` uçuşta okunan dokuya yazıyor — bilinen sınır, kayıtlı.**
Gerekçesi `AtlasTexture::prepare`'ın doc'unda: yuvaların ayrık olması
yetmiyor, doku düzeni doğrusal değil ve bir yuvaya yazmak komşu yuvaların
sütunlarını taşıyan karoların oku-değiştir-yaz'ı olabiliyor. **Bugün belirti
nadir ve tek karelik**, çünkü yalnız *hiç görülmemiş* bir karakter yükleniyor.
Tahliye bunu **rutin** hâle getirir: canlı bir yuvanın üstüne yazmak sıradan
bir olay olur. Doc'un kendi yazdığı doğru biçim — staging tamponu + aynı
komut tamponunda blit encoder — bu setin ön koşulu hâline gelebilir.

**2. `TEXTURE_EDGE = 1024` bir ölçüm iddiası değil, adıyla "bir kapasite
tercihi".** Yani büyütmek kodun kendi doc'unun açık bıraktığı bir yol ve
tahliyenin rakibi olarak tartışılmayı hak ediyor.
