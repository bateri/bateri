# Taşan glyph — Tartışma

Hücre fontun istediğinden küçük olduğunda harfin hücreye sığmayan kısmının ne
olacağı sorusu. Ürün yönü kullanıcının talebinde verildi: iTerm2 gibi
**taşsın, kesilmesin** (2026-10-03). Aşağıdaki seçenekler bu yönün içinde.

## Seçenek A: Yuva hücreden ayrılır (mürekkep kutusu)

- **Atlas.** Atlas iki geometri taşıyor: **hücre** (ızgaranın adımı, bugünkü
  `cell_px`) ve **yuva**.
  - Yuva her eksende `max(hücre, doğal hücre)`. Doğal hücre `1.0/1.0`'daki
    hücre.
  - Fark (pay), hücrenin iki yanına eşit dağılıyor. Bu, `line_height`'ın
    fazlayı dağıttığı kuralın aynısı, yalnız işareti ters.
- **Rasterlama.**
  - Glyph yuvaya, hücreye göre **aynı yerde** rasterize ediliyor: taban
    çizgisi ve ortalama hücreden, artı payın ofseti.
  - Yordamsal sprite'lar ve kurallar yuvanın içindeki **hücre**
    dikdörtgenine çiziliyor, pay saydam kalıyor. Böylece kutu çizgileri
    yine hücre ızgarasında döşeniyor.
- **GPU.**
  - `GlyphInstance` değişmiyor. Immediate bloğuna `slot_px` ve
    `slot_offset` giriyor.
  - Dörtgen `pos − slot_offset + corner × slot_px`, uv tam yuva.
  - Yuva hücreye eşitken ofset sıfır ve dörtgen bugünküyle bit bit aynı.
- **Geniş glyph.** İki yarı **bölme çizgisinde** kırpılıyor: sol yarı
  `x < cell_w`, sağ yarı `x ≥ cell_w`. Paylar dışa doğru, yani yarılar
  üst üste binip kenarı iki kez boyamıyor.

**Artıları:**
- Kesme kalkıyor, glyph'in boyu değişmiyor (iTerm2 davranışı).
- `≥ 1`'de yuva hücreye eşit, ofset sıfır. Raster ve dörtgen bugünküyle
  aynı, `raster_digest` bunu kanıtlıyor.
- Atlas kapasitesi değişmiyor: `< 1`'de yuva doğal hücre boyunda, yani
  bugünkü varsayılan punto kadar.
- Örnekleme yine tam yuva, dolgu gerekmiyor: dörtgen yuvayla aynı boyda.

**Eksileri:**
- Yuva ile hücreyi bir arada sanan her tüketici ayrışıyor ve tek tek
  sayılmalı: yordamsal çizim, kurallar, küçük sınıfın `place_small`'ı,
  `Half`, `glyph_fx`'in dörtgeni ve sınır testi, emoji, tofu, `rise`, yedek
  kapının kutusu.
- Shader ve `#[repr(C)]` ikizi değişiyor, yani riskli phase (`make shader`).

## Seçenek B: Glyph'i hücreye sığacak kadar küçült

Hücre doğal hücreden küçükse glyph o oranda küçük puntoyla rasterize ediliyor
(041'in `SHRINK` emsali).

**Artıları:**
- GPU'ya ve yuva aritmetiğine hiç dokunmuyor.

**Eksileri:**
- **Kullanıcının istediği şey değil.** Harf aralığı daralıyor ama harfler de
  küçülüyor. `line_height = 0.8` ile `size` küçültmek arasındaki fark
  kayboluyor. Ürün yönüyle çelişiyor; reddedildi.

## Karar Noktaları

1. **Yüzeyler arası dikiş.** Dikiş üç yerde:
   - **Izgara ile doldurma bandı arası.** Bant ızgaradan sonra kendi
     zeminiyle çiziliyor, yani ızgaranın tepe satırının yukarı taşan aksanı
     bant görünürken örtülür. Öneri: çizim sırası "önce iki yüzeyin
     zeminleri, sonra vurgular, sonra caret, sonra iki yüzeyin glyph'leri".
     Bant ile ızgara aynı geçmişin iki parçası, aralarındaki dikiş kesmemeli.
   - **Dock.** Ayrı bir panel. Izgaradan taşan glyph'i dock'un opak zemini
     örtmeye devam ediyor. Dock'un kendi glyph'leri **bandına scissor'la**
     kırpılıyor, yani bandın dışına taşmıyor.
   - **Pencere kenarı.** Sert kenar olarak kalıyor.
2. **Caret'in ters çevirmesi.** Komşu harfin caret hücresine taşan kuyruğu da
   ters çevriliyor (zemin renginde çiziliyor). Test piksel başına olduğu için
   bu kendiliğinden oluyor ve tutarlı. Ek iş yok.
3. **Alt sınır.** Öneri iki aralıkta da `0.5`.
   - Atlas kapasitesi alt sınırdan etkilenmiyor (yuva doğal hücre).
   - `0.5` artık okunmaz, ama kullanıcı kendisi seçiyor.
   - Bu bir ürün kararı.
4. **Yedek glyph kapısı.** Ölçtüğü kutu taşma artık mümkün olduğu için
   **yuvanın** kutusu (`max(hücre, doğal) × cols`). `≥ 1`'de bugünküyle aynı.

## Muhakeme (2026-10-03)

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU |
| Codebase-fit | SORUNLU |
| İşletme | SORUNLU |

Üç mercek de Seçenek A'nın yönünü doğru buldu. İtirazların hepsi atlas
geometrisinin nasıl kurulacağı, kanıt ve sıra üstüne.

**Kabul edilen itirazlar → tasarım değişikliği:**

- **"Yuva" `Metrics`'e alan olarak eklenmesin; atlas iki `Metrics`
  taşısın** (sadelik, codebase-fit).
  - **Glyph metriği**: aralık her eksende `max(·, 1)`'e kırpılmış hâli.
    Yuva budur ve glyph rasterlama yolu bugünkü kodun **aynısı** olur:
    `position`, renk düzlemi, küme, `rise`.
  - **Izgara metriği**: gerçek aralıkla hücre.
  - Yuvanın ofseti iki metriğin farkından tek bir fonksiyonla türüyor.
  - Döşenen yordamsal aile (blok, çizgi, Braille) ve tofu ızgara metriğiyle
    ayrı tampona çiziliyor, sonra `place_small`'un genelleştirilmiş hâliyle
    yuvaya ofsetli kopyalanıyor. Böylece yaklaşık 20 raster fonksiyonuna
    dokunulmuyor (`census::draw_wide` emsali).
  - `centre_shift`'in `max(0)` sorunu (codebase-fit 2) kendiliğinden
    kalkıyor. Kutu yuvanın genişliği, yani doğal ilerleme; glyph her zaman
    sığıyor ve hücrenin ortasında kalıyor.
- **`cell_metrics` `1`'in altını bugün ifade edemiyor** (işletme 1,
  codebase-fit 3). `round_up` her ölçüyü `≥ 1`'e kırpıyor, yani `extra` her
  `lh < 1`'de `+1` çıkıyor. Eşit bölmeyle `0.5`'te de `baseline_px ==
  cell_px.1` oluyor.
  → Izgara metriğinin açığı **işaretli** yuvarlanıyor ve yukarı ile aşağıya
  **ascent:descent oranında** dağıtılıyor (eşit değil). Taban çizgisi
  `lh > 0`'da hücrenin içinde kalıyor, değişmez korunuyor. `≥ 1`'deki eşit
  dağıtım kuralı aynen kalıyor.
- **Alt çizgi, üstü çizili ve chevron** glyph metriğiyle, taban çizgisine
  bağlı çiziliyor (codebase-fit 3). Hücre dikdörtgenine çizilmiyor. Yani
  harfin kuyruğu nereye taşıyorsa alt çizgi de onunla gidiyor.
- **İki sütunlu kutu `hücre + yuva`, `2 × yuva` değil** (codebase-fit). İki
  yarı bölme çizgisinde kırpılıyor, böylece birleşimleri tek parça rasterin
  aynısı oluyor. Bunu bir CPU sınaması bekliyor.
- **Yedek glyph kapısı yuvanın kutusunu ölçüyor** (codebase-fit). Sözleşme
  "kutu, **yuvaya** tam sığan glyph ya da küçültülmüş glyph" oluyor.
  Gerekçe: yedek glyph kararı aralıktan bağımsız kalıyor ve taban harfleriyle
  tutarlı.
- **Phase sınırı crate sınırı değil** (işletme). `bt-gpu` atlasın
  `cell_px`'ini yuva diye okuyor: `slot_layout`, `write_slot`'un
  `assert_eq`'i, `uv_size`. Phase-1 yuva sahipliğini **iki crate'te
  birden** taşıyor, shader'a dokunmuyor. `slot_px` ve `slot_offset`
  atlastan geliyor, `frame.cell_px()`'ten değil (ölçek değişimi karesi).
- **Izgara ile doldurma bandı arasındaki dikiş iki parça** (codebase-fit 1).
  - Izgaranın viewport'u tepesinin üstünü **kırpıyor**, yani zemin sırası
    tek başına yetmez.
  - Glyph op'ları taşma payı kadar yukarıdan başlayan bir viewport alıyor ve
    pay vertex'te geri ekleniyor.
  - Sıra şöyle: ızgara zemini → vurgular → **caret** → bant zemini →
    vurgular → ızgara glyph'leri → bant glyph'leri. Caret bant zemininin
    altında kalıyor (işletme 3'ün korunan kuralı, `CLAUDE.md` → çizim sırası),
    ızgara glyph'leri bandın zemininden sonra geliyor.
- **Dock'a yeni scissor yok** (sadelik 2, codebase-fit). Dock glyph'lerinin
  viewport'u bandın tepesinden (`band_y`) başlıyor. Böylece nefes payına
  taşan aksan kesilmiyor, bandın üstüne de çıkmıyor.
  → Bekçi: `< 1`'de üst giriş satırında aksanlı bir harfin geliş efekti `t = 1`'de
  statik glyph'le aynı olmalı.
- **`glyph_fx` yuvayı görüyor** (codebase-fit): `FX_PAD`'in tabanı,
  `paint`'in sınırı, `texel`, `i1` kırpması, `ink_depth` ve shatter'ın parça
  ızgarası.
- **`≥ 1`'de aynılığın kanıtı** (işletme 2).
  - `raster_digest` önce **üst commit'te** `letter = 1.3`'ü de tarayacak
    şekilde genişletiliyor.
  - `metrics()` adı ızgara metriğinde kalıyor (yuva için yeni ad), yani
    digest satırı değişmiyor.
  - GPU tarafında saf bir sınama var: `≥ 1`'de `slot_offset == 0` ve
    `slot_px == cell_px`. Mevcut offscreen sınamalar değişmeden yeşil
    kalıyor.
- **Bekçiler `fixture` karakterleriyle yazılıyor** (işletme 2), yani
  lavapipe'ta da koşuyor:
  - kuyruk komşu satırın renkli zemininde ön plan renginde görünüyor,
  - geniş glyph'in iki yarısı ayrık,
  - ızgaranın tepe satırının aksanı bant görünürken de görünüyor,
  - dock'un geliş efekti `t = 1`'de statik glyph'le aynı.

**Reddedilenler:**

- Dock'a kalıcı scissor (taslaktaki öneri). Viewport tepeyi zaten kırpıyor;
  scissor fx payını keserdi (`renderer.rs` `plan`'ın sözleşmesi).

**Ürün kararına giden iki bulgu:**

- **`1.0`'daki fazladan piksel** (işletme a, codebase-fit). Bugün
  `line_height = 1.0` hücreyi fontun istediğinden **1 px uzun** yapıyor.
  `round_up` `extra`'yı 1'e kırpıyor; kodun kendi yorumu "fazla sıfır"
  diyor. İşaretli yuvarlama bunu kendiliğinden düzeltir ve **varsayılan
  satır aralığı 1 px daralır**.
- **Alt sınır** (Karar Noktası 3).

## Karar (2026-10-03, kullanıcı onayı + teknik karar)

- **Seçilen (ürün, kullanıcı):** taşma, küçültme değil. Seçenek A, panelin
  düzeltmeleriyle (Muhakeme → Kabul edilenler).
- **Seçilen (ürün, kullanıcı):** `1.0`'daki fazladan piksel **düzeliyor**.
  `line_height = 1.0` tam olarak fontun aralığı oluyor, iTerm2'nin `100`'ü
  gibi.
  - Varsayılan satır aralığı 1 px daralıyor.
  - "`≥ 1`'de bit bit aynı" kanıtı bu farkı adıyla kabul ediyor: yalnız
    yükseklik ve yuva baytları değişiyor, glyph'in taban çizgisi yerinde
    kalıyor.
- **Seçilen (ürün, kullanıcı):** alt sınır iki aralıkta da `0.5`.
- **Seçilen (teknik):**
  - Atlasta iki metrik var: glyph metriği (yuva) ve ızgara metriği (hücre).
    Ofset iki metrikten tek bir fonksiyonla türüyor.
  - Açık işaretli yuvarlanıyor ve ascent:descent oranında dağıtılıyor.
  - Alt çizgi ve chevron glyph'e bağlı çiziliyor. Döşenen yordamsal aile ve
    tofu hücreye çiziliyor.
  - Kapı yuvanın kutusunu ölçüyor, iki sütunda `hücre + yuva`.
  - GPU'da `slot_px`/`slot_offset` atlastan geliyor. Glyph viewport'u taşma
    payı kadar yukarıdan başlıyor.
  - Çizim sırası: ızgara zemini, vurgular, caret, bant zemini, vurgular,
    ızgara glyph'leri, bant glyph'leri.
  - Dock'a scissor yok, glyph viewport'u bandın tepesinden başlıyor.
  - Gerekçeler Muhakeme'de.
- **Reddedilen:** Seçenek B (küçültme). Kullanıcının istediği değil.
  `line_height < 1` ile `size` küçültmek arasındaki fark kaybolurdu.
- **Reddedilen:** dock'a kalıcı scissor (Muhakeme → Reddedilenler).
