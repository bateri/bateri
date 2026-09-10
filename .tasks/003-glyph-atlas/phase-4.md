# Phase 4 — Glyph: `frame()` sınırı, `cell` pipeline, `glif=` kapısı

## Özet

`frame()` sınırı karakteri ve ön plan rengini geçirir, `bt-gpu` ikinci bir
pipeline ile glyph'leri arka planların üstüne çizer, duman sözleşmesi `glif=`
jetonu kazanır ve belgeler aynı commit'te güncellenir. Yazılan görünür olur.

_Requirements: R2, R2.1, R2.2, R2.3, R2.4, R4, R4.1, R4.2, R6, R7_

Atomik ve geri alması en pahalı phase. **Uzarsa dikiş**: önce genişlemiş
sink'i renderer glyph verisini *yoksayarak* al (görünmez, yeşil, `hucre=`
sabit), sonra pipeline'ı ekle.

---

## 1. `frame()` sınırı genişler

`crates/bt-core/src/session.rs`

Karar 1(a): tek zengin tip, tek sink.

```rust
/// Çizilecek tek hücre.
pub struct Cell {
    pub col: u16,
    pub row: u16,
    pub ch: char,
    /// Ön plan, **lineer** RGBA (bkz. `color::lineer_rgba`).
    pub fg: [f32; 4],
    /// `None` = varsayılan arka plan, çizilmez. `Frame::push` yalnız `Some`
    /// gördüğünde `bg_count`'u artırır: `hucre=K` jetonunun anlamı bit bit
    /// korunur ve `sabit_shell_arka_plan_hucreleri_verir` oynamaz.
    pub bg: Option<[f32; 4]>,
}
```

- `Flags::WIDE_CHAR_SPACER` hücreleri **elenir** — elenmezse CJK satırlarında
  ikinci hücreye hayalet glyph çizilir.
- Biçim bayrakları geçmez (R2.3): `INVERSE`/`DIM` zaten renge çözülüyor,
  `BOLD`/`ITALIC`/`UNDERLINE`/`STRIKEOUT` 004'ün işi. Alacritty'nin `Flags`'i
  hiçbir hâlde yeniden ihraç edilmez.
- Atlama koşulu `bg.is_none() && ch == ' '`. Koşul biçim bayrakları gelince
  onları da elemeli — yorumla bağlanır, yoksa altı çizili boşluklar sessizce
  kaybolur.

`CellBg` adı ölür; `bt-gpu`'nun `Frame::push_bg`'si `push` olur.

**Phase-1'den devir — uzayı tipe yaz.** `/code-review` phase-1'de şunu buldu:
`bt-gpu` sRGB olmayan hedefi *temsil edilemez* hâle getirdi (`PIXEL_FORMAT`
`const`), ama simetrik hata `bt-core` tarafında hâlâ tamamen temsil edilebilir
— `CellBg.rgba` `pub` bir `[f32; 4]` ve uzayı yalnız bir yorum söylüyor.
Sınır zaten bu phase'de yeniden yazıldığı için newtype'ın yeri burası:

```rust
/// Lineer RGBA; tek kurucusu paletin dönüşümü.
pub struct LinearRgba([f32; 4]);
```

`fg` ve `bg` bunu taşır, `DEFAULT_BG`/`DEFAULT_CURSOR` tip değiştirir,
`bt-gpu` `.0` ile okur. Maliyeti bu phase'de birkaç satır; ertelenirse
`Cell`'in üçüncü yazımı olur.

---

## 2. `cell` pipeline

`crates/bt-gpu/shaders/cell.metal` + `crates/bt-gpu/src/frame.rs`

Ayrı `.metal`, ayrı `#[repr(C)]` instance, **iki taraflı** assert çifti
(`static_assert` / `offset_of!`) — `cell_bg`'deki örüntünün birebir eşi.

```
GlyphInstance { pos: float2, size: float2, uv0: float2, rgba: float4 }
```

`build.rs` değişmez: dizini tarayıp her `.metal`'i linkliyor. `make shader`
kanaryası bu phase'de **zorunlu**.

- blend: `src_alpha` / `one_minus_src_alpha`, sRGB hedefte lineer uzayda
  (phase-1'in sebebi tam olarak bu)
- **shader'a gamma düzeltmesi YAZILMAZ.** Kodlamayı ROP yapıyor; `cell.metal`
  içine bir `pow(c, 1/2.2)` eklemek paleti **iki kez** kodlar. `cell_bg.metal`
  ve `frame.rs::Instance` bu uyarıyı taşıyor, yenisi de taşımalı
- atlas `R8Unorm`, `sample(atlas, uv).r` ile alfa; renk `rgba`'dan
- iki pipeline **tek render pass'te**, `viewport_px` uniform'u ortak

**Çizim sırası (R4.1):** `cell_bg` (arka planlar **+ imleç**) → `cell`
(glyph'ler üstte). Bugün imleç `Frame`'in sonunda ve opak; glyph gelince
imlecin altındaki harfi örterdi — 003'ün çözdüğü şikâyetin küçük bir kopyası.

Atlas dokusunun sahibi `Renderer`: `bt-atlas`'ın verdiği bitmap `replaceRegion`
ile yüklenir.

---

## 3. `glif=` jetonu ve kapı

`crates/bt-gpu`, `crates/bt-shell/src/app.rs`, `Makefile`, `proje.md`

Duman sözleşmesi jeton **ekler**: `kare=N hucre=K glif=G pipeline=ok`.
Jetonlar silinmez, eklenir — okuyan taraf tanımadığını atlar.

Bu süs değil kapının kendisi: boş bir atlas ve hiç çizmeyen bir glyph yoluyla
`kare=1 hucre=8 pipeline=ok` yine basılır ve `make duman` yeşil geçerdi. Kapı
`G > 0` de sorar.

`G`'nin sahibi `bt_core::smoke_shell`: `" bateri "` → **6** boşluksuz glyph.
Sayı `hucre=8`in bağlandığı gibi bir `bt-core` sınamasına bağlanır.

**GPU tarafı kanıtı, tam bayt assert etmeden:** offscreen sınamasında bir
hücrenin içinin arka planla **tekdüze olmadığı** doğrulanır. Tam bayt aransaydı
kapı sistem fontunun sürümüne rehin olurdu.

---

## 4. Belgeler (aynı commit)

- `CLAUDE.md`: `bt-atlas` "hâlâ boş" cümlesi ölür; katman tablosunda
  `core-text`/`core-graphics` → `objc2-core-text`/`objc2-core-graphics`
  (+ `objc2-core-foundation`); jeton listesine `glif=`; "bugün ekranda yalnız
  renkli hücreler var" cümlesi ölür
- `.claude/is-akisi/proje.md`: `duman` satırı `glif=` ve `G > 0` kapısını anlatır
- `bt-atlas/src/lib.rs` başlık yorumu: "003+" gelecek zamanı ölür

---

## Phase-3'ten devir

Phase-3'ün kalite kapısı (irtifa merceği + `/code-review`) dört bulgu üretti;
hiçbiri phase-3'te uygulanamadı çünkü dördü de **atlas dokusunun doğduğu** ana
bağlı.

**1. `Atlas::ensure`'ün dönüşü bu phase'de yakalanmak zorunda.**
`Renderer::cell_metrics` içindeki `let _ = atlas.ensure(PUNTO, scale)` bugün
sinyali atıyor: "atlası yeniden kurdum, dokuyu da yeniden ayır". Bugün ayrılacak
doku yok. Bu phase'de doku doğuyor ve sinyal kaçarsa belirti **sessiz**: ızgara
geometrisi değişmiş bir atlastan bayat bir yuva okumak, yuva aralık içinde
kaldığı sürece `slot_origin`'in savunmasına takılmaz ve **başka bir glyph**
çizer. Belirti "harici ekranı çıkardım, harfler karıştı" ve yalnız iki ekranlı
makinede görünür.

Phase-3 `ensure`'ü `#[must_use]` işaretledi ama **bu satır o korumanın dışında
kalıyor** ve bunu bilerek bilmek gerek: `let _ = expr;` rustc'nin kabul ettiği
susturma biçimidir, yani `cell_metrics` derlenirken hiçbir uyarı çıkmaz.
`#[must_use]` bu phase'in **yeni** `ensure` çağrılarında durdurur; mevcut
satırı bu phase elle ele almak zorunda. İlk iş: `cell_metrics`'in `let _`'sini
dokuyu yeniden ayıran yola bağla ya da sinyali oradan taşıyacak bir alan
kur — "işaretliydi, görürüz" yanlış bir güvendi.

**2. Atlası ödünç alan tek yer `Renderer::draw` olsun.**
`Renderer.atlas` bir `RefCell`. `link.rs:260` şu örüntüyü öğretiyor:
`iv.frame.borrow_mut()` alınıyor ve `renderer.draw(..., &frame, ...)` çağrısı
**boyunca tutuluyor**. Aynı şekil atlas için kopyalanırsa — yuva çözümünü
`session.frame(...)` sink'inde hoist et (sink hücre başına koştuğu için doğal
refleks bu), sonra o guard canlıyken `draw`'u çağır, `draw` da `slot_origin`
için atlası ödünç alsın — ilk glyph'li karede `BorrowMutError`. Çizim yolunda
ve `Retry` `GpuError` için tasarlandı, unwind için değil.
Çözüm: `Frame` `char` taşısın, `char → yuva → uv` çözümü `draw` içinde **tek
bir** `borrow_mut` altında yapılsın. Üç şey bedavaya gelir: yeniden-ödünç
temsil edilemez olur; bütün kare tek atlas kuşağıyla çizilir (1. maddedeki
bayat-yuva tehlikesi kuşak sayacı gerektirmeden yok olur); `link.rs`'in sink'i
renderer durumundan uzak kalır.

**3. Atlas ölçeği koşulsuz, `cell_px` koşullu uygulanıyor.**
`geometriyi_esitle` her çağrıda `cell_metrics(scale)` çağırıyor ve o çağrı
atlası **koşulsuz** yeni ölçeğe geçiriyor; `DisplayLink::resize` ise
`iv.cell` alanını yalnız `Session::resize` kabul ederse yazıyor. Bugün ikisi
ayrışsa da zarar yok: reddin iki sebebi var, "dejenere boyut" (o durumda
çizilecek hücre de yok) ve "hiç değişmedi" (o durumda ölçü zaten aynı), ve
bir sonraki geometri olayı ikisini eşitliyor. Doku gelince ayrışma
**ucuz olmaktan çıkar**: @2x rasterize edilmiş bir glyph @1x yuvaya blit
edilir. Bu phase atlas kuşağını çizim tarafında kabul edilen ölçüyle
eşlemeli — 2. maddedeki "tek `borrow_mut`, tek kuşak" çözümü bunu da kapatır.

**4. "Önce metriği sor" sözleşmesi ve GPU kapısının kendi tuzağı.**
`Renderer.atlas` artık `Option` ve `None` doğuyor: metriği hiç sormadan atlası
okuyan bir yol sessizce @1x çizmek yerine "atlas yok" durumuyla karşılaşır.
Bu phase'in glyph kapısı (`hücre içi arka planla tekdüze değil`, §3) tam da
böyle bir yolda koşacak: `cell_bg_pikseli_gpu_tarafinda_boyar` bir `Renderer`
kurup `cell_metrics`'i **hiç çağırmıyor**. Yeni sınama da öyle kurulursa atlas
`None` kalır; kapı ya düşer ya da (eski tasarımda olacağı gibi) @1x atlasla
yeşil geçerdi. Sınama ölçeği açıkça söylemeli.
Ölçeğin `bt-gpu`'ya **iki kapısı** olduğu da kayda geçsin: `Surface::set_size`
ve `cell_metrics`, `app.rs`'te komşu iki satır. Bugün uyuşuyorlar çünkü öyle
yazıldı, kurgu gereği değil. Birleştirmek (`Renderer::resize(surface, w, h,
scale) -> CellMetrics`) R5'in harfiyle çelişir; bu sette yapılmadı, doku
geldiğinde yeniden bakılır.

---

## Uygulama Notları

### R2.2'nin gerekçesi yanlıştı: spacer elenmedi

Kılavuz "`WIDE_CHAR_SPACER` hücreleri **elenir** — elenmezse CJK satırlarında
ikinci hücreye hayalet glyph çizilir" diyordu. Hayalet glyph diye bir şey yok:
alacritty 0.26 spacer hücresini `write_at_cursor(' ')` ile yazıyor
(`term/mod.rs`, geniş karakterin hemen ardından `Flags::WIDE_CHAR_SPACER`
şablona ekleniyor), yani `cell.c` zaten `' '`. Elemek **arka planı** silerdi:
spacer'ın `bg`'si geniş karakterin şablonundan geliyor ve hücreyi tümden
atlamak CJK karakterin sağ yarısını renksiz bırakırdı — kılavuzun engellemek
istediğinden daha görünür bir hata.

Yapılan: bayrak hücreyi elemiyor, **mürekkebi** düşürüyor (`ch = None`; ilk
yazılışında `' '` sentinel'iydi, `/simplify` onu `Option<char>`'a çevirdi).
Aynı mekanizmaya iki bayrak daha bağlandı:

- `LEADING_WIDE_CHAR_SPACER` — satır sonuna sığmayan geniş karakterin bıraktığı
  boşluk, aynı gerekçe.
- `HIDDEN` (`\e[8m`, conceal) — kılavuzda **hiç yoktu**. `INVERSE`/`DIM` gibi
  `bt-core` içinde çözülmesi gereken bir bayrak: geçmediği ve burada da
  elenmediği için gizlenmiş metin ekranda okunurdu. `gizli_metnin_murekkebi_
  dusar_arka_plani_kalir` bunu bağlıyor.

Üçü tek `const MUREKKEPSIZ` maskesinde ve çizen taraf bayrak sormuyor:
`bt-gpu` yalnız `ch.is_some()` diye bakıyor, yani terminal semantiği sınırın
doğru tarafında kaldı.

**Bilinen sınır (kapsam dışı listesinde yoktu, buraya geçiyor):** geniş
karakterin glyph'i **tek yuvaya kırpılıyor**. Atlas sabit yuva ızgarası ve
yuva bir hücre boyunda (R1.4); CJK bir karakter iki hücre kaplıyor ama
rasterize edildiği tampon bir hücre. Arka plan iki hücreyi de kaplıyor, harf
yarım kalıyor. Düzeltmesi ya çift genişlikli yuva ya iki yuvaya bölünmüş
çizim; ikisi de atlas biçimini değiştirir ve bu setin kapsamı dışında.

### `GlyphInstance` 32 bayt: `size` ve uv boyutu uniform oldu

Kılavuz `{pos, size, uv0, rgba}` diyordu. O düzen **iki tarafta ayrışıyor**:
`float4` MSL'de 16 hizalı, Rust'ta `[f32; 4]` 4 hizalı. `pos@0, size@8,
uv0@16` sonrası `rgba` Rust'ta 24'e, MSL'de 32'ye düşer; `sizeof` 40'a karşı
48. Eşlemenin tek yolu sırf hizalama için var olan bir dolgu alanı olurdu.

`size` ve uv boyutu zaten **kare boyunca sabit** (her glyph tam bir hücre,
R1.4), yani instance'ta taşınmaları da gereksizdi. Uniform'a taşınınca düzen
`{pos, uv0, rgba}` = 0/8/16, stride 32 oluyor — `Instance`'la aynı şekil,
dolgusuz, iki taraf da kendi assert'iyle bağlı. Üç uniform ayrı ayrı `float2`
(`viewport_px`, `cell_px`, `uv_size`), tek bir `float4`'e sarılmadı: mevcut
`viewport_px` örüntüsü zaten bu ve `float4` uniform'u CPU tarafında 16 hizalı
bir sarmalayıcı isterdi.

`cell_px` **karenin**, `uv_size` **atlasın**: ikisi normalde aynı ölçekten
doğuyor, ayrıştıkları pencerede glyph esniyor (bkz. devir 3 aşağıda).

### `AtlasDoku`: `ensure`'ün sinyali artık yapısal (devir 1)

Atlas ve dokusu tek `struct`ta, tek `RefCell`te. `Renderer::atlasi_esitle`
atlası değiştiren **tek** yer ve `ensure()` `true` derse dokuyu düşürüyor;
`draw` yenisini kuruyor. Phase-3'ün `let _ = atlas.ensure(...)` satırı öldü,
yani `#[must_use]`'ın "susturuldu" durumu da öldü — öznitelik artık bir niyet
beyanı değil, gerçekten okunan bir dönüş.

Dokuyu `cell_metrics` değil `draw` kuruyor. İki sebep: metrik yolu pencere
boyutlandırmanın sıcak yolunda ve orada bir doku ayırmasına bağlanmamalı;
ayrıca `cell_metrics` hata döndüremiyor, doku ayırması ise başarısız olabilir
(`GpuError::NoAtlasTexture`).

### Yuva çözümü `encode_glyphs`'in içinde (devir 2)

`Frame` `char` taşıyor, uv taşımıyor. `char → yuva → uv` çözümü, eksik
yuvaların yüklenmesi ve instance tamponunun doldurulması tek `borrow_mut`
altında (`AtlasDoku::hazirla`). Ödünç `encode_glyphs`'te doğuyor ve orada
ölüyor; `link.rs`'in `frame.borrow_mut()`'u atlasa hiç dokunmuyor. Kılavuzun
saydığı üç kazanç da geldi: yeniden-ödünç temsil edilemez, kare tek atlas
kuşağıyla çiziliyor, sink renderer durumundan uzak.

### Devir 3'ün kalıntısı: bir kare bulanıklık, bozulma değil

Atlas ölçeği koşulsuz (`cell_metrics` her çağrıda `ensure`), `iv.cell` ise
`Session::resize` kabul ederse yazılıyor. Devir 2'nin çözümü bunu **zararsıza
indiriyor ama sıfırlamıyor**: ölçek değişimiyle bir sonraki geometri olayı
arasında kalan tek karede `frame.cell_px()` ile atlasın metriği ayrışabilir.
Sonuç bozulma değil, esneme — yuva `cell_px` boyundaki dörtlüye örnekleniyor
ve `filter::linear` onu yumuşatıyor. Bir sonraki geometri olayı ikisini
eşitliyor. Kuşak sayacı **eklenmedi**: bedeli görünür, kalıcı değil.

Sampler bu yüzden **`nearest`** (ilk yazılışı `linear`'dı; `/code-review`
düzeltti — bkz. aşağıdaki kapı kaydı, madde 5). Ayrışan karede linear komşu
yuvadan okuyabiliyordu; nearest her zaman yuvanın içinde kalıyor ve birebir
oturan olağan durumda ikisi zaten aynı sonucu veriyor.

### Devir 4: `GpuError::NoAtlas` ve sınamanın ölçeği söylemesi

`Renderer.atlas` `None` doğuyor. Glyph'i olan bir kare atlassız gelirse
`draw` `GpuError::NoAtlas` ile düşüyor; sessizce @1x bir atlas uydurmuyor.
`atlassiz_renderer_glif_cizmeyi_reddeder` bunu bağlıyor.
`glif_hucrenin_icini_arka_planindan_ayirir` ise kılavuzun uyardığı tuzağa
düşmemek için `cell_metrics(1.0)`'ı **açıkça** çağırıyor ve hücre boyutunu
oradan alıyor (uv birebir otursun diye).

**Ölçeğin iki kapısı birleştirilmedi.** `Surface::set_size` ve `cell_metrics`
`app.rs`'te hâlâ komşu iki satır. `Renderer::resize(surface, w, h, scale) ->
CellMetrics` R5'in harfiyle çelişiyor ve doku geldi diye çelişmesi bitmedi;
004'e devrediliyor.

### `LinearRgba` kurucusu sRGB baytından

Phase-1'in devri (`phase-1.md` → "Uzayı tipe yazmak phase-4'e devredildi")
burada kapandı: `bt-gpu` tarafında yanlış format zaten temsil edilemezdi
(`PIXEL_FORMAT` `const`), `bt-core` tarafındaki simetrik kaçak artık kapalı.

Kılavuz `pub struct LinearRgba([f32; 4])` + "`bt-gpu` `.0` ile okur" diyordu.
Alan private kaldı ama kurucu **`from_srgb(r, g, b)`** oldu: lineer float alan
bir kurucu newtype'ı bir ad değişikliğine indirirdi, kapattığı hata tam olarak
"sRGB float'ı lineer yuvaya koymak" ve o hata ancak dönüşüm tipin içinde
olunca temsil edilemez hâle geliyor. Okuma `.0` değil `to_array()` — ikisi de
`const`, `DEFAULT_BG` derleme zamanında açılabiliyor.

Kurucu olmasaydı `bt-gpu`'nun offscreen sınamaları üç ayrık renk kuramazdı:
paletin `pub` yüzeyinde yalnız iki sabit var (`DEFAULT_BG`, `DEFAULT_CURSOR`)
ve sınama hücre yolu ile clear yolunu ayırt edebilmek için üçüncüsünü
istiyor. Alternatif paleti sınama için genişletmekti; renk uzayı garantisini
bozmayan kurucu daha az borç bıraktı.

### Küçük sapmalar

- **`pipeline()` yardımcısı.** İki pipeline aynı `{ad}_vertex` /
  `{ad}_fragment` sözleşmesini izliyor; kurucudaki blok tek fonksiyona indi.
  Yan etkisi `GpuError::MissingFunction`'ın anlamı: artık eksik fonksiyonu
  değil eksik **çifti** adlandırıyor (`cell_bg_vertex / cell_bg_fragment`).
- **`Frame` iki listeye ayrıldı.** `push_bg` → `push`, `instances()` →
  `bg_instances()`, yanına `glyphs()` / `glyph_count()` / `cell_px()`. Tek
  liste tutulsaydı çizim sırası hücre hücre karışırdı.
- **`hucreleri_bekle` (bt-core sınama yardımcısı) ölçüt değiştirdi.** Artık
  sink çağrısı değil **arka planlı hücre** sayıyor: sink mürekkepli hücreleri
  de veriyor ve `bos_yazma_pty_yazicisini_kilitlemez`'in PTY yankısı ("ab")
  sayıyı sessizce şişirip sınamayı zaman aşımına düşürüyordu.
- **`DIM` artık ön plana da uygulanıyor.** Eskiden yalnız ters videoda
  (`bg`'ye dönen `fg` için) uygulanıyordu çünkü `fg` sınırdan geçmiyordu.
  Şimdi geçiyor; sönükleştirme takastan **önce** yapılıyor, yani ters
  videodaki eski davranış birebir korunuyor.
- **Atlas dokusu `MTLStorageMode::Shared`.** Yükleme yolu `replaceRegion`,
  yani CPU doğrudan yazıyor; `Private` + blit encoder + staging tamponu bu
  setin kazancını taşımaz. Ayrık GPU'lu makinede bedeli örneklemede bir kopya;
  `/measure` sonrası yeniden bakılacak bir yer. Başarısızlık **sesli**:
  `newTextureWithDescriptor` `None` → `GpuError::NoAtlasTexture`.
- **Uçuştaki komut tamponu ile `replaceRegion`.** İlk yazılışında "yuvalar
  ayrık, yazma güvenli" diyordu; `/code-review` bunun yetersiz olduğunu
  gösterdi ve iddia hem koddan hem buradan kalktı. Bugünkü hâli aşağıdaki
  kalite kapısı kaydında, waive maddesinde.
- **R7'nin katman tablosu yarısı bu commit'te iş üretmedi.** `CLAUDE.md`'nin
  `bt-atlas` satırı zaten `objc2-core-text` / `objc2-core-graphics` /
  `objc2-core-foundation` diyordu — phase-2 yazmış. Kılavuzun o maddesi
  önceden karşılanmıştı, "yapıldı" diye sayılmıyor.

### Kalite kapısı — `/simplify` (dört mercek, paralel)

**Uygulananlar (8):**

1. **`Cell.ch` `char` değil `Option<char>`** (irtifa). `' '` bir sentinel'di ve
   dört ayrık durumu (gerçek boşluk, `HIDDEN`, iki spacer) tek değere
   indiriyordu; anlamı `bt-core`'da iki, `bt-gpu`'da bir yerde okunuyordu.
   004'te bedeli somut: **altı çizili bir boşluk mürekkep ister**, gizli metin
   istemez — o gün üç okuma yeri de ayrı ayrı elden geçirilecekti. `Option`
   niche ile 4 bayt, yani ayrım bedava. Değişecek tek satır artık
   `frame()`'deki `then_some`.
2. **Atlama koşulu ön plan zincirinden önce** (verimlilik). `resolve(cell.fg)`
   + `dim` + `lineer_rgba`, `Term` kilidi tutulurken **çizilmeyen** hücreler
   için de koşuyordu; boş grid'de bu neredeyse her hücre. Şimdi arka plan
   çözülüp atlama kapısı geçildikten sonra çözülüyor. Semantik birebir aynı:
   sönüklük hâlâ `cell.fg`'den doğan renge gidiyor (`ters_videoda_...`
   sınaması bunu tutuyor). Mürekkepsiz hücrenin `fg`'si hücrenin kendi arka
   planı — görünmez mürekkep; gözlemlenebilir değil ve yorumu bunu söylüyor.
3. **`Renderer::instans_tamponu<T>`** (reuse + sadeleştirme). `encode_bg` ile
   `encode_glyphs` `newBufferWithBytes` sarmalayıcısını satır satır
   tekrarlıyordu. Asıl bedel yorumdaki karardı: "üçlü tamponlama bilinçli
   reddedildi (002), `/measure` sonrası yeniden bakılır" iki çağrı yerinde
   birden değişmek zorundaydı. Karar artık ayırmanın kendisinde.
4. **`vertex_uniform<T>`** (aynı). `NonNull::from(&x).cast(), size_of_val(&x),
   n` üçlüsü dört kez yazılmıştı. Ajanın uyarısı kayda geçti ve uygulandı:
   güvenli jenerik sarmalayıcı SAFETY yükümlülüğünü taşıyor ama **düzen
   sözleşmesini taşımıyor** — hangi indeksin hangi shader bildirimine
   karşılık geldiği çağrı yerinde yazılı kaldı.
5. **`Frame::pos()`** (reuse + sadeleştirme). Izgara→piksel formülü `hucre()`
   ile glyph dalında iki kez yazılmıştı, ama `debug_assert!(w > 0.0 && h >
   0.0, "clear(cell_px) çağrılmadı")` yalnız birinde. Yalnız mürekkep taşıyan
   bir kare bekçinin dışında kalıyordu; formül ve bekçi tek yerde birleşti.
6. **`yukle` `Metrics` alıyor, `slot_bytes()` ile assert ediyor** (reuse).
   `w * h` yuva geometrisinin dördüncü kopyasıydı; `Metrics::slot_bytes`'ın
   kendi doc'u "tek sahip burası" diyor. `unsafe` bloğun ön koşulu artık o
   sahibe bağlı.
7. **`atlasi_esitle` `RefMut::map`'i `get_or_insert_with`'i sararak kullanıyor**
   (sadeleştirme). Bir `expect` ve onu gerekçelendiren bir `// audit:` satırı
   düştü.
8. **`encode_glyphs`'in destructure'ı kalktı, `Frame::cell_px()` `[f32; 2]`
   döndürüyor** (sadeleştirme). `atlas` adı tek fonksiyonda üç ayrı tipe
   gölgeleniyordu; erişimci de iki kez çağrılıyordu.

Ufak: uv aritmetiğinde doku boyutunun tersi döngü dışına alındı (glyph başına
iki f32 bölmesi), `AtlasDoku.instances`'ın doc'u düzeltildi — "tek kuşak"
garantisini o alan değil `encode_glyphs`'in tek `borrow_mut`'u veriyor.

**Reddedilenler (gerekçeli):**

- **Dörtlünün boyunu `frame.cell_px()` yerine atlastan almak** (irtifa 3).
  Ayrışmayı temsil edilemez kılardı ve sampler `nearest`'a inerdi — ama
  **daha kötüsünü** yapardı: glyph'in konumu (`GlyphCell::pos`) ve altındaki
  arka plan karenin ölçüsünden doğuyor, yalnız boyu atlasa bağlamak glyph'i
  hücresinden kaydırırdı. Derin hâli (`Frame` grid koordinatı taşısın, çevrim
  encode anında olsun) `Frame::clear(cell_px)` tasarımını ve devir 3'ün kendi
  şartını ("çizim tarafında **kabul edilen** ölçüyle eşle") ters çevirirdi.
  Gerekçe koda yazıldı.
- **`Frame { bg, cursor: Option<Instance>, glyphs }`** (irtifa 2). `bg_count`
  türetilir ve "imleç sona" sözleşmesi yapısal olurdu; bedeli kare başına
  ikinci bir tampon ayırması ve ikinci bir çizim çağrısı. `push_cursor`'ın
  tek çağrı yeri var ve `debug_assert` `make hepsi`'de koşuyor. İmleç kendi
  pipeline'ını isteyeceği gün (`cursor_motion`) bedava gelir.
- **`cell_metrics` → `set_scale`/`prepare` yeniden adlandırma** (irtifa 4).
  Ad yan etkiyi taşımıyor, doğru bir gözlem — ama `cell_metrics` R5'in adı ve
  `CLAUDE.md`'de yazılı; derin hâli (`CellMetrics::new`'i `bt-gpu` dışına
  kapatmak) phase-3'ün bilinçli kararıyla çelişiyor (`bt-shell` sınamaları
  Metal device'sız kalsın diye kurucu `pub`).
- **`AtlasDoku`'dan `instances`'ı çıkarmak** (irtifa 5). Alan değişmeze
  katılmıyor, doğru; ama `Renderer`'a ikinci bir `RefCell` eklemek dağınıklığı
  yer değiştirmekten öteye gitmiyor. Doc'un fazla söyleyen cümlesi düzeltildi.
- **`atlas_dokusu` ile sınamanın `hedef_doku`'sunu birleştirmek** (reuse 3).
  İkisi de `Shared` kullanıyor ama **ayrı sebeplerle**: biri CPU'nun
  `replaceRegion` ile yazması, öteki `getBytes` ile okuması. Tek yorum iki
  kararı anlatamaz.
- **`cell.metal` / `cell_bg.metal` için ortak `.h`** (reuse 5). Paylaşılan
  blok üç satır (`corner`, NDC, y-ters); `static_assert` üçlüleri
  paylaşılamıyor. Üçüncü pipeline geldiğinde (004) kendini öder.
- **`GlyphCell` → Metal tamponuna doğrudan yazmak** (verimlilik 3). Ara
  `Vec<GlyphInstance>` bir kopya daha demek; kaldırmak `contents()` üzerinden
  ham işaretçi yazımı ister. Ölçülmeden `unsafe` yüzeyi büyütmüyoruz →
  `/measure`.
- **`bt-atlas`'ta ASCII için düz indeksli önbellek / yuvada kökeni tutmak**
  (verimlilik 2b-2c). `bt-atlas` API'sine dokunuyor, bu phase'in kapsamı dışı.

**Kayda değer:** merceklerden biri bulguyu kendisi uyguladı (`renderer.rs`'i
düzenledi) — oysa `/simplify`'ın uygulama adımı ana döngünündür. Düzenleme
geri alındı ve bulgular elle, yorum yoğunluğu korunarak uygulandı; ajanın
sürümü "üçlü tamponlama" kararını çıkarılan fonksiyona taşımamış ve üç SAFETY
gerekçesini tek satıra indirmişti.

### Kalite kapısı — `/code-review`

On bulgu; **dokuzu uygulandı**, biri gerekçeli waive. Üçü gerçek hataydı ve
ikisini yeni sınamalar mutasyonla çiviledi.

**Gerçek hatalar (uygulandı):**

1. **İmlecin altındaki harf okunmuyordu.** İmleç bloğu opak ve R4.1 gereği
   glyph'lerin **altında**; harf kendi ön planıyla kalınca açık gri (`#d8d9dd`)
   açık mavinin (`#7a9cc6`) üstüne düşüyordu. Kılavuz R4.1'de sıra sorununu
   çözmüş ama kontrast sorununu görmemişti — ve bu phase'in kendi hedefine
   ("yazılan görünür olur") aykırı. Çözüm `bt-core`'da: imleç konumu döngüden
   önce çözülüyor ve o hücrenin ön planı paletin arka planına dönüyor. Kararın
   adı terminal semantiğidir, `bt-gpu`'nun bileceği bir şey değil.
   `imlecin_altindaki_harf_ters_cizilir` iki mutasyonla doğrulandı (ters
   çevirmeyi kaldır → düşer; **herkesi** ters çevir → düşer).
2. **Blend'in alfa çarpanı yanlıştı.** Fragment ön çarpımsız veriyor; renk
   için `SourceAlpha`/`OneMinusSourceAlpha` doğru ama aynı çarpanı **alfa
   kanalına** uygulamak `sa² + (1-sa)·dst_a` ediyor ve yarı kapsamalı bir
   kenarda hedefin alfası 1'den 0.75'e düşüyordu. `CAMetalLayer` `opaque`
   bayrağını taşımıyor, yani compositor o deliği onurlandırır: harflerin
   kenarından pencerenin arkası sızardı. Kaynak alfa çarpanı `One` oldu.
   Hiçbir **renk** iddiası bunu göremezdi; offscreen sınamasına alfa kanalı
   iddiası eklendi ve mutasyonla doğrulandı.
3. **Glyph sınaması rezident tofu ile de geçiyordu.** Tek glyph'le uv
   aritmetiğinin bozulması görünmez: uv0 yuva 0'a çakılı kalsaydı shader tofu
   kutusunu örnekler ve "arka plan var + farklı piksel var + ön plan rengi
   taşıyor" iddialarının üçü de geçerdi — `cell_bg`'nin iki instance'la
   kapattığı boşluğun birebir eşi. Sınama iki glyph çiziyor (`M` ve `.`) ve
   ikisinin farklı olduğunu soruyor; `uv0`'ı sabitleyen mutasyon artık düşüyor.

**Ayrıca uygulananlar (6):**

4. **`Cell.fg` artık koşulsuz çözülüyor.** `/simplify`'ın verimlilik
   düzeltmesi fazla ileri gitmişti: mürekkepsiz hücrede alan hücrenin arka
   planını taşıyordu. Kapıdan **sonra** çözmek asıl kazancı (çizilmeyen
   hücreler hiç ödemiyor) zaten veriyor; koşullu yapmak 004'ün kural
   çizgisini mürekkepsiz bir hücrede arka plan rengiyle, yani görünmez,
   çizdirirdi.
5. **Sampler `linear` → `nearest`.** `linear`'ı ayrışan karede daha iyi diye
   savunmuştum; incelemede tersi çıktı: dörtlü yuvadan genişse son sütun
   komşu yuvanın ilk sütununu karıştırıyor. Yuvalar arasında pay yok,
   `clamp_to_edge` yalnız doku kenarında kırpıyor ve Metal yeni dokuyu
   sıfırlamıyor — yani komşu **yazılmamış** olabilir. `nearest` her zaman
   yuvanın içinde kalıyor ve birebir oturan olağan durumda ikisi zaten aynı.
   Bu, `## Uygulama Notları`'nda daha önce yazdığım "linear sebebi"ni
   geçersiz kılıyor ve o cümle düzeltildi.
6. **`GpuError::MissingFunction` eksik sembolün adını geri aldı.**
   `{ad}_vertex` türetmesi "ikisinden biri" demekle yetiniyordu; `pipeline()`
   artık iki adı ayrı parametre alıyor.
7. **`viewport_px` kare başına tek yerde** (`encode_pass`), iki encoder'a
   parametre. İki objc mesajı ve ayrışabilen iki tanım düştü; `texture` da
   iki encode fonksiyonunun parametresi olmaktan çıktı.
8. **`Upload.origin` yeniden türetilmiyor.** `bt-atlas` yuva numarasıyla
   köşeyi bilerek aynı dönüşte veriyor; `slot_origin` artık yalnız
   önbelleklenmiş ve tofu yoluna kalıyor.
9. **`atlasi_esitle` `RefMut` değil `Metrics` döndürüyor.** Atlas ödüncünün
   bir çağrı sınırını aşabildiği tek yer orasıydı — ki `encode_glyphs`'in
   doc'u tam olarak bunun olmamasına dayanıyor.

**Waive (gerekçeli):**

- **`replaceRegion` uçuşta okunan dokuya yazıyor.** İddiam ("yuvalar ayrık,
  yazma güvenli") yetersizdi: doku düzeni doğrusal değil, bir yuvaya yazmak
  komşu yuvaların sütunlarını da taşıyan karoların oku-değiştir-yaz'ı
  olabiliyor. Belirti nadir ve tek karelik — hiç görülmemiş bir karakter,
  önceki kare hâlâ koşarken yüklenirse o karede bozuk çizilebilir. Doğru
  biçimi bir staging tamponu + **aynı komut tamponunda** blit encoder'ı
  (Metal'in kendi hazard takibi sıralar), ya da uçuştaki kare sayısını bir
  semaforla sınırlamak. İkisi de kare ritmine dokunuyor ve 002'nin "üçlü
  tamponlama reddedildi, `/measure` sonrası yeniden bakılır" kararıyla aynı
  masada. Bu sette yapılmadı; yanlış güvenlik iddiası koddan silindi ve
  yerine sınırın kendisi yazıldı (`AtlasDoku::hazirla` doc'u).

### Kalite kapısı — `/audit`

**İlgisiz (4):** mercek 2 (`Cargo.toml`/`Cargo.lock` el değmedi), 4 (ayar ve
tema modeli yok), 5 (`assets/shell/` el değmedi) ve mercek 9'un terminfo yarısı.

**Mekanik mercekler inline (3), üçü de temiz:**

- **1 (katman yönü)** — `cargo tree`: `bt-core`'da `objc2`/`core-text`/
  `core-graphics`/`metal` yok, `bt-atlas`'ta `objc2-core-*` üçlüsü dışında
  `objc2` yok, `bt-gpu → bt-shell` kenarı yok; `grep` `bt-core/src`'de kaçak
  bulmadı. `bt-shell`'in `Cargo.toml`'unda `bt-atlas` **yok** (kaynaktaki iki
  isabet yalnız yorum metni).
- **3 (panik yolu)** — `git diff -U0 -- crates/bt-core/src`'in eklediği
  `unwrap`/`expect`'lerin tamamı `#[cfg(test)]` içinde (yeni imleç sınaması).
  Ürün yolunda yeni panik yok.
- **6 (ölçüm sahipliği)** — `docs/OLCUMLER.md` hâlâ yok ve diff sayı iddiası
  taşımıyor. "glyph başına iki f32 bölmesi", "iki objc mesajı", "bir kopya"
  gibi cümleler **iş sayımıdır**, ölçüm değil: koddan okunarak doğrulanır ve
  bir süre/bellek iddiası taşımaz. Ölçüm bekleyen iki madde `## Yayın
  Etkisi`'nde.

**Yargı mercekleri fan-out (4, paralel, opus):**

- **8 (boşta sıfır kare)** — **temiz**, kanıtla: alacritty'nin boş hücresi
  `c: ' '`, `bg: Named(Background)` ve `resolve` onu `BG_RGB` veriyor, yani
  yeni atlama koşulu (`bg.is_none() && ch.is_none()`) boş grid'in her
  hücresine uyuyor; `dirty.swap` hâlâ `frame()`'in **ilk** deyimi ve imleç
  çözümü kilidin ardında, yani hasarsız karede fazladan iş yok; `encode_glyphs`
  boş glyph listesinde atlas ödüncünden **önce** dönüyor, yani glyph'siz
  karede `hazirla` hiç koşmuyor; `link.rs`'in iki `setPaused(true)` yolu da
  yerinde ve diff'i tek satır.
- **9 (shader/Rust düzen uyumu)** — **temiz**. Ofset tablosu iki tarafta da
  `pos@0 / uv0@8 (size@8) / rgba@16`, stride 32, dolgu yok; dört assert
  (`static_assert` + `__builtin_offsetof`, `size_of` + `offset_of!`) bağlıyor.
  Buffer indeksleri (0/1/2/3) ve `[[texture(0)]]` Rust tarafıyla eşleşiyor;
  vertex descriptor yok (dörtlü `vertex_id`'den türüyor). Blend
  `sourceAlpha=One` ile ön çarpımsız "over"ın doğru hâli. `bt-core`'un 24 bayt
  assert'i el değmedi ve doc'u bu diff'te `bt_core::Cell`'le karışmayı önlemek
  için güncellendi.
- **7 (thread ve blokaj)** — bir bulgu, **uygulandı** (aşağıda). `BorrowMutError`
  yolu **temiz**: atlas ödüncünün tamamı iki yerde (`atlasi_esitle`,
  `encode_glyphs`) ve `cell_metrics`'in tek üretim çağrısı `geometriyi_esitle`,
  yani çizim yolundan ulaşılamıyor; `draw` içinde run loop döndüren bir çağrı
  yok. Yeni kilit ya da kilit sırası yok; sink `Session`'a geri girmiyor.
  Ana thread'de rasterizasyon `CLAUDE.md`'nin "render yolu bloklanmaz"
  kuralının saydığı sınıflardan biri değil (PTY `read`, kilit, `sleep`, G/Ç);
  pahalı CoreText parçası (`Atlas::new` → font zinciri) `cell_metrics`'te
  kalıyor. Biçim: **patlama**, yeni karakter başına bir kez.
- **10 (belge ve üslup borcu)** — altı belge–kod çelişkisi, **hepsi
  düzeltildi** (aşağıda). Temiz çıkanlar: hizalama aritmetiği,
  `GlyphCell`'in `BorrowMutError` iddiası, `#[must_use]` cümlesi, shader
  yorumları, dil kuralı (`pub` adlar İngilizce; `Blend`/`AtlasDoku`/
  `atlasi_esitle`/`hazirla`/`instans_tamponu`/`yukle`/`MUREKKEPSIZ` hepsi
  modül-private), `expect`/`assert` gerekçeleri, depoda hiç `#[allow]` yok.

**Mercek 7'nin bulgusu — negatif önbellek dolunca rasterizasyon her kareye
yayılıyordu.** `Atlas::slot` bu sette **ilk kez** çizim yoluna girdi. Normal
hâlde bu bir patlama, çünkü hem pozitif hem negatif (tofu) çözüm
önbellekleniyor. Ama `yuvalar` dolduğunda (negatif kayıtlar haritayı doldurmuş,
yuva hâlâ boş) `GlifYok` kolu **hiç yazmıyordu** ve ekranda duran her
desteklenmeyen karakter her karede `CTFontGetGlyphsForCharacters` +
`CGBitmapContextCreate` ödüyordu — ana thread'de, kare bütçesinin ortasında.
Ulaşılabilir: bir ikili dosyayı `cat`'lemek binlerce ayrık desteklenmeyen
codepoint üretiyor.

Düzeltme `bt-atlas`'ta: tavan kapasitenin **iki katı** oldu (bir katı
pozitiflerin olabildiği en büyük değer, ikincisi negatife bırakılan pay) ve
tavan dolunca negatif kayıtlar **toptan atılıyor** — "artık hiç önbellekleme"
değil. Bellek hâlâ bağlı, bedel amortize (iki tahliye arasına en az
`kapasite()` yeni kayıt sığıyor), pozitif kayıtlar korunuyor.
`negatif_onbellek_tavanlidir` sınaması `negatif_onbellek_tavanli_ve_tahliyeli`
oldu ve üç mutasyonla doğrulandı: eski davranışa dönmek, tahliyenin pozitifleri
de atması, tavanın kalkması — üçü de düşüyor.

**Mercek 10'un altı çelişkisi (düzeltildi):**

1. `atlasi_esitle`'nin doc'u "ödüncü verir" diyordu (imza `Metrics` döndürüyor)
   ve "`draw` yalnız var olanı okur" diyordu — oysa `draw` yuva açıyor.
   Ayrım netleşti: burası atlasın **anahtarını** (ızgara geometrisini)
   değiştiren tek yer, `draw` ızgarayı değiştirmiyor.
2. Bu dosyadaki "`replaceRegion` yazma güvenli" notu, `/code-review`'da
   çürütülüp koddan silinmişken burada ayakta kalmıştı.
3. `' '` sentinel dili üç yerde yaşıyordu (`renderer.rs` sınama yorumu, bu
   dosyada iki yer) — kod `Option<char>` kullanıyor.
4. `frame.rs`'in sınama yorumu "`LinearRgba`'nın tek kurucusu paletin dönüşümü,
   yani sınama kendi rengini uyduramıyor" diyordu; `from_srgb` `pub` ve aynı
   diff'teki offscreen sınamaları tam olarak onu kullanıyor.
5. `bt-core/src/lib.rs`'in kapsül listesi `LinearRgba`'yı saymıyordu — oysa
   sınırı bu diff'te geçmeye başladı.
6. **`glif` kapısının ne kanıtladığı iki yerde fazla söylüyordu.** `app.rs` ve
   `proje.md` "boş bir atlas ve hiç çizmeyen bir glyph yolu yeşil geçerdi"
   diyordu; `glyph_count()` bir **CPU** sayacı, o yolu düşürmez. Kapının
   gerçekten gördüğü şey `frame()` sınırının karakteri geçirmemesi; GPU
   tarafını offscreen sınaması kanıtlıyor. (`plan.md` R6 aynı iyimser cümleyi
   taşıyor; onaylı tasarım kaydı olduğu için orada bırakıldı, düzeltme burada.)

**Ayrıca (mercek dışı, ana döngünün kendi taraması):** `app.rs`'te
`CellMetrics`'in "kurucusu `Renderer::cell_metrics`" diyen yorumu — phase-3'ün
`renderer.rs`'te düzelttiği iddianın burada kalmış kopyası; `CellMetrics::new`
`pub`. Düzeltildi.

## Yayın Etkisi

- **shader** — `crates/bt-gpu/shaders/cell.metal` **yeni**. `make shader`
  koştu (çıkış 0). `GlyphInstance` düzeni iki tarafta da kendi assert'iyle
  bağlı: Rust'ta `size_of` + `offset_of!`, MSL'de `static_assert` +
  `__builtin_offsetof`. `build.rs` değişmedi (dizini tarıyor).
- **duman sözleşmesi** — `kare=N hucre=K pipeline=ok` →
  `kare=N hucre=K glif=G pipeline=ok`. Jeton **eklendi**, hiçbiri silinmedi;
  okuyan taraf tanımadığını atlar. Kapı `G > 0` de soruyor. `Makefile`,
  `CLAUDE.md` (komut satırı + jeton listesi) ve `.claude/is-akisi/proje.md`
  (doğrulama tablosu) aynı commit'te güncellendi.
- **belge** — `CLAUDE.md` § Proje: "glyph'in kendisi hâlâ çizilmiyor" ve
  "bugün ekranda yalnız renkli hücreler var" cümleleri öldü.
  `bt-atlas/src/lib.rs` ve `bt-gpu/src/lib.rs` başlık yorumları dokunun
  sahipliğini anlatıyor.
- **ölçüm bekliyor:** ilk karede ve ölçek değişiminde atlas + doku
  kurulumunun ana thread'deki bedeli (phase-3 mercek 7'nin işaret ettiği an:
  `Atlas::new` bugün ucuz, rasterizasyon `slot()`'ta tembel, ama artık yanına
  bir doku ayırması ve tofu yüklemesi eklendi).
- **ölçüm bekliyor:** ikinci pipeline'ın ve kare başına glyph instance
  tamponunun kare süresine etkisi.
- terminfo / `TERM`, ayar şeması, tema-materyal biçimi, shell entegrasyonu,
  app bundle: **yok**. `Cargo.toml` ve `Cargo.lock` el değmedi.

---

## Checklist

- [x] `bt-core`: `CellBg` → `Cell`, `bg: Option`, `WIDE_CHAR_SPACER` **elenmedi, mürekkebi düşürüldü** (gerekçe notlarda; `HIDDEN` de aynı maskeye girdi)
- [x] `cell.metal` + `GlyphInstance` + iki taraflı assert çifti
- [x] `Frame`: glyph listesi, `push_bg` → `push`, imleç `cell_bg`'de kalır
- [x] Renderer: atlas dokusu, `replaceRegion`, iki pipeline tek pass
- [x] `glif=` jetonu, `make duman` kapısı `G > 0`
- [x] Test: `smoke_shell` altı glyph verir (`bt-core`)
- [x] Test: offscreen — hücre içi arka planla tekdüze değil (`bt-gpu`)
- [x] Belgeler: `CLAUDE.md`, `proje.md`, `bt-atlas/lib.rs` (+ `bt-gpu/lib.rs`, `Makefile`)
- [x] Doğrulama geçti: `make hepsi` (0), `make shader` (0 — `cell.metal`
      yeni), `make duman` (`kare=2 hucre=8 glif=6 pipeline=ok`),
      `make test-yaris` (0 — `Session::frame` gövdesi `Term` kilidi altında
      değişti). Koşulu tetiklenmeyenler: `make terminfo` ve `make kur`
      (girdileri henüz yok, `proje.md`'nin bilinen listesi) — atlanmış kapı
      değil, **koşulu doğmamış** kapı
- [x] `/simplify` çalıştırıldı (dört mercek, paralel): 8 uygulandı, 7 gerekçeli
      red — ayrıntısı `## Uygulama Notları` → "Kalite kapısı"
- [x] `/code-review` çalıştırıldı: 10 bulgudan 9'u uygulandı (üçü gerçek
      hata: imleç altındaki harfin okunmaması, blend alfa çarpanı, tofu ile
      geçen glyph sınaması), 1 gerekçeli waive (`replaceRegion` hazard'ı)
- [x] `/audit` çalıştırıldı: 4 mercek ilgisiz, 5 temiz (1/3/6 inline, 8/9
      fan-out), mercek 7'nin 1 bulgusu ve mercek 10'un 6 belge–kod çelişkisi
      giderildi (+ ana döngünün kendi bulduğu 1 çelişki)
- [x] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [x] Commit: `3c91814`
