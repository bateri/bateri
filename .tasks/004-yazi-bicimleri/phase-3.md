# Phase 3 — `bt-gpu`: yüzler ve kurallar çizilir

## Özet

`bt-gpu` `(bold, italic)`'i `Face`'e çevirir, kural çizgilerini `GlyphInstance`
olarak glyph'lerden sonra çizer, duman sözleşmesine `kural=R` jetonunu ekler.
Ekranda görünen değişiklik burada.

_Requirements: R4, R4.1, R4.2, R4.3, R4.4, R5, R5.3, R5.4, R6_

**Geri alması en pahalı phase.** Uzarsa dikiş: önce yüz çevirisini al
(kalın/eğik görünür, kurallar hâlâ yok, `kural=` jetonu girmemiş — `make duman`
yeşil kalır), sonra kuralları.

---

## 1. `Face` çevirisi — **burası, çünkü tek yer**

`crates/bt-gpu/src/frame.rs`

`bt-atlas` `bt-core`'u görmüyor ve görmemeli: o kenar `alacritty_terminal`'i
saf-CoreText crate'ine çekerdi (`CLAUDE.md` → "bağımlılık mimari karardır").
`bt-gpu` ikisini birden gören **tek** katman.

```rust
// bt_core::{bold, italic} bir SGR bayrağı; bt_atlas::Face bir font yüzü.
// Dört varyantları aynı, SEBEPLERİ AYRI — ikisi bilerek iki tiptir ve
// "aynı görünüyorlar" diye birleştirilirse katman yönü ters döner:
// birleşik tip ya bt-core'a girer (bt-atlas onu göremez) ya bt-atlas'a
// (bt-core onu göremez). Çeviri burada kalmalı.
fn yuz(bold: bool, italic: bool) -> Face {
    match (bold, italic) {
        (false, false) => Face::Regular,
        (true, false) => Face::Bold,
        (false, true) => Face::Italic,
        (true, true) => Face::BoldItalic,
    }
}
```

`GlyphCell` yüzü taşır; `GlyphInstance` **taşımaz** (uv0 zaten yuvayı, yuva
da yüzü kodluyor):

```rust
pub(crate) struct GlyphCell {
    pub(crate) pos: [f32; 2],
    pub(crate) ch: char,
    pub(crate) face: Face,
    pub(crate) rgba: [f32; 4],
}
```

---

## 2. Kural listesi

`crates/bt-gpu/src/frame.rs`

**Yeni pipeline, yeni shader, yeni instance tipi YOK.** `cell` pipeline'ı
zaten genel bir kapsama maskesi çizicisi — kodun kendi yorumu
(`cell.metal:55-56`): *"Renk instance'tan gelir, dokudan değil — atlas glyph
başına bir maske tutuyor, bir görüntü değil."* Kural sprite'ı da hücre boyunda
bir maske; `GlyphInstance`'ın `size`'ı zaten yok, dörtlü tam bir hücre.

```rust
/// Kural çizgisi; `GlyphCell`'in kardeşi ve aynı gerekçeyle metriksiz —
/// yuva çözümü atlas ödüncünün yaşadığı yerde (`encode_glyphs`) yapılıyor.
pub(crate) struct RuleCell {
    pub(crate) pos: [f32; 2],
    pub(crate) kind: RuleKind,
    pub(crate) rgba: [f32; 4],
}

pub(crate) struct Frame {
    bg: Vec<Instance>,
    glyphs: Vec<GlyphCell>,
    rules: Vec<RuleCell>,      // yeni
    cell_px: (f32, f32),
    bg_count: usize,
}
```

`push()` bir hücre için **ikiye kadar** kural üretir (alt çizgi + üstü
çizili); rengi `underline_color` varsa o, yoksa `fg`:

```rust
let kural_rgba = cell.underline_color.unwrap_or(cell.fg).to_array();
if let Some(kind) = rule_kind(cell.underline) {
    self.rules.push(RuleCell { pos, kind, rgba: kural_rgba });
}
if cell.strikeout {
    // Üstü çizili SGR 58'i kullanmaz — SGR'de karşılığı yok.
    self.rules.push(RuleCell { pos, kind: RuleKind::Strike, rgba: cell.fg.to_array() });
}
```

**`bg_count` el değmiyor** → `hucre=K` bit bit korunuyor ve `push`'taki
`debug_assert_eq!(bg.len(), bg_count, "arka plan imleçten sonra eklendi")`
bekçisi olduğu gibi kalıyor. Kural sayacı ayrı: `rule_count()`.

---

## 3. Encode sırası — imleç bedavaya çözülüyor

`crates/bt-gpu/src/renderer.rs` → `AtlasDoku::hazirla` / `encode_glyphs`

Kurallar glyph'lerden **sonra**, **aynı** instance tamponuna ve **aynı** draw
call'a girer:

```rust
// Tek liste, tek draw call: önce glyph'ler, sonra kurallar. Sıra bilerek —
// üstü çizili harfin ÜSTÜNDEN geçmeli.
for g in glyphs { /* slot(Sprite::Char(g.ch), g.face) → instances.push(...) */ }
for r in rules  { /* slot(Sprite::Rule(r.kind), Face::Regular) → instances.push(...) */ }
```

**İmleç sırası bedava:** glyph geçişi zaten arka planlardan **ve imleçten
sonra** kodlanıyor (`encode_bg(...).and_then(|()| encode_glyphs(...))`,
`renderer.rs:409-412`), imleç ise `bg` listesinin sonunda. Yani kural imlecin
üstüne düşüyor — ve orada rengi `bt-core`'un imleç için zaten tersine
çevirdiği ön plan (SGR 58 yoksa), yani görünür kalması da bedava. 003'ün
glyph için kurduğu mekanizmanın aynısı.

Kurallar **her zaman `Face::Regular`** ile sorulur: kalın metnin altındaki
çizgi kalın değildir.

---

## 4. Duman jetonu ve kapı

`crates/bt-shell/src/app.rs`

```
kare=N hucre=K glif=G kural=R pipeline=ok
```

Kapı `n > 0 && k > 0 && g > 0 && r > 0`. Jeton **bu phase'de** girer —
phase-2'de girseydi `R=0` olurdu ve set ortasında `make duman` kırmızıya
düşerdi. (003 `glif=`'i tam bu yüzden phase-4'te ekledi.)

Reçete phase-2'de yerleşti, yani `R` bu commit'te ilk kez sıfırdan büyük
okunuyor: sayısı `smoke_shell`'in yedi kural hücresinden gelir.

**Jetonun sınırı yoruma yazılır:** `kural=` setin yalnız **kural yarısını**
kapatır ve stil ayrımını göremez — `Face` her zaman `Regular` dönen ya da
kıvrımı düz çizen bir yapı da aynı `R`'yi basar. Yüz yarısının kapısı
phase-1'in birim sınamaları, stil ayrımının kapısı phase-2'nin
`sabit_shell_bes_stili_ayirt_eder`'i, kıvrımın gerçekten dalga olduğunun
kapısı aşağıdaki offscreen sınaması.

---

## 5. Offscreen sınama — kural bandı tekdüze değil

`crates/bt-gpu/src/renderer.rs` (`#[cfg(test)]`)

`glif_hucrenin_icini_arka_planindan_ayirir`'ın kardeşi, **tam bayt assert
etmeden** (yoksa kapı sistem fontunun sürümüne rehin olur):

- Kıvrımlı alt çizgili bir hücre çizilir, kural bandındaki satır okunur.
- Assert: band **x boyunca tekdüze değil** — düz çizgi tekdüzedir, dalga
  değildir. Kıvrımı düz çizgiye düşüren bir kod burada kırmızı düşer.
- İkinci assert: SGR 58 rengi verilen bir kuralın pikselleri ön plan
  renginden **farklı**.

---

## 6. Belgeler

Hepsi bu commit'te (kodla çelişen cümle kuralı):

| dosya | ne |
|---|---|
| `CLAUDE.md` | jeton listesine `kural=`; `make duman` satırı; **"kalanı 004'ün işi (…emoji, kutu çizim)"** cümlesi — emoji ve kutu çizim 004'te değil, ayrı setlerde |
| `Makefile` | `duman` hedefinin yorumu |
| `.claude/is-akisi/proje.md` | doğrulama tablosunun `duman` satırı |
| `crates/bt-gpu/src/renderer.rs` | atlas ödüncünün "tek yer" cümlesi hâlâ doğru mu (kural çözümü aynı fonksiyonda kalıyor, ama cümle okunup doğrulanır) |

---

## Uygulama Notları

**0. Ad ve dil düzeltmesi (`SAPMA` değil).** Phase dosyası `bb04da7`'de, yani
`1dbb084`'ün dil daraltmasından önce yazıldı. Kod tanımlayıcılarının tamamı
İngilizce: `yuz(bold, italic)` → `face(bold, italic)`, `kural_rgba` →
`rule`'un kendi alanı, ve dört sınama adı İngilizceye taşındı —
`kural_bandi_x_boyunca_tekduze_degil` → `rule_band_is_not_uniform_along_x`,
`sgr58_rengi_on_plandan_farkli` → `sgr58_color_differs_from_foreground`,
`kalin_ve_duz_ayri_cizilir` → `bold_and_regular_draw_differently`,
`imlecin_ustundeki_kural_gorunur` → `rule_over_cursor_stays_visible`. Phase
metninin `AtlasDoku::hazirla` dediği yüzeyin kodda karşılığı
`AtlasTexture::prepare` (aynı commit'in çevirisi).

**1. `kural=15`, yedi değil.** Phase metni (madde 4) "sayısı `smoke_shell`'in
yedi kural hücresinden gelir" diyor; reçete okunduğunda sayı 15 çıkıyor ve
**doğru olan 15**: `" bateri "`nin sekiz hücresi `\033[41;1;4m` ile açılıyor,
yani kırmızı arka plan **artı kalın artı düz altı çizili** — sekizi de birer
kural üretiyor, üstüne yedi kural hücresi geliyor. 8 + 7 = 15; on beşinin
hiçbirinde alt çizgi ile üstü çizili aynı hücrede buluşmuyor, yani hücre başına
iki kural dalı duman koşusunda ateşlenmiyor (onu `cell_yields_up_to_two_rules`
sınıyor). `hucre=8 glif=6` bit bit korundu: ölçüldü, `kare=1 hucre=8 glif=6
kural=15 pipeline=ok`. Phase metnindeki sayı bir öngörü hatasıydı, kapsam
sapması değil — jeton yine "kural üretildi" diyor, yalnız daha çok yerden.

**2. Kırmızı-önce koşu kaydı.** Dört offscreen sınama üretim kodundan **önce**
yazıldı ve koştu: `bold_and_regular_draw_differently` `assert_ne!` ile
(iki hücre birebir aynı: yüz çevirisi yok), `rule_band_is_not_uniform_along_x`
"düz alt çizgi hiç çizilmedi" ile, `sgr58_color_differs_from_foreground`
"SGR 58'siz kural ön plan rengiyle çizilmedi" ile,
`rule_over_cursor_stays_visible` "imlecin üstündeki kural örtüldü" ile.
Dördü de hücreyi yalnız clear rengiyle (26, 28, 33) dolu buldu, yani kırmızı
**doğru sebepten** düştü. Tek istisna `frame.rule_count()` assert'i: tipi
olmayan bir çağrı derlemeyi kırdığı için o satır kırmızı koşudan sonra eklendi.

**3. `slot_uv` ayrı fonksiyona çıktı.** Phase metni `encode_glyphs` içinde iki
döngü öngörüyor; yuva çözümü + `upload` dalı + uv aritmetiği ikinci döngüde
kopyalanacaktı. Kopyada unutulan bir `upload_slot` "yuva var ama doku boş"
demek olurdu: ekranda görünmeyen bir kural, hiçbir sayacın düşmediği.
Fonksiyon atlası `&mut Atlas` olarak **açıkça** alıyor, `&mut self` olarak
değil — öyle olsaydı `self.instances.push` ile ödünç kavga ederdi.

**4. `encode_glyphs`'in kapısı iki listeyi birden soruyor.** Yalnız kural
taşıyan bir kare (boş satırın altındaki kıvrım, ya da duman reçetesinin yedi
hücresi tek başına) `glyphs.is_empty()` kapısından geri dönerdi ve hiç
çizilmezdi. Ad değişmedi (checklist onu adıyla anıyor), doc'u değişti.

**5. `pos` `push`'un başında bir kez hesaplanıyor.** Dört dal (arka plan,
glyph, alt çizgi, üstü çizili) aynı aritmetiği istiyor; hücre başına dört
çağrının kazancı yok ve ayrışabilen dört kopya demek. `pos()`'un `clear` bekçisi artık **her** push
edilen hücrede koşuyor, yalnız arka planlı/mürekkepli olanlarda değil.

**6. Checklist'te olmayan iki birim sınaması eklendi.**
`cell_yields_up_to_two_rules` (`frame.rs`): kuralsız hücre kural üretmiyor,
mürekkepsiz hücre üretiyor, alt çizgi rengi SGR 58'den geliyor, üstü çizili
**hep** ön plandan ve ikisi aynı hücrede buluşabiliyor — checklist'in dördüncü
maddesinin ("renk `underline_color ?? fg`, üstü çizili hep `fg`") tek bekçisi
bu, offscreen sınamalar iki kuralı aynı hücrede hiç görmüyor.
`sgr_flags_translate_to_four_faces`: dört kollu `match`'in iki kolu
karıştığında belirti "eğik metin kalın çiziliyor" olur ve `assert_ne!`'e dayanan
offscreen sınama bunu göremez.

**7. Sınama yardımcıları ortaklaştırıldı.** `render_offscreen` (doku + encode +
`commit` + `waitUntilCompleted` + `MTLCommandBufferStatus::Error` kontrolü),
`cell_rows` (hücrenin pikselleri **satır satır** — "x boyunca tekdüze mi"
sorusunu ancak satır yapısı korunursa sorabiliyoruz), `fitting_cell_px` ve
`rule_cell`. `glyph_differs_from_cell_background` da bu yardımcılara taşındı;
iddiaları bit bit aynı kaldı. Kopyalansaydı `Error` kontrolü bir sınamada
unutulur ve o sınama boş bir dokuyu okuyup anlamsız bir renk iddiası düşürürdü.

**8. Kodla çelişen cümleler aynı commit'te düzeltildi; başlıcaları dokuz.** `frame.rs` modül
doc'u ("iki liste" → üç), `Frame::push`'un phase-2 dikiş notu ("bu phase'de
okunmuyor"), `renderer.rs`'te `prepare`'in "yüz phase-3'te geliyor" yorumu,
`AtlasTexture.instances`'ın `char → slot → uv` cümlesi, `encode_pass`'in çizim
sırası yorumu, `encode_glyphs`'in başlığı, `app.rs`'in "üç jeton" cümlesi, ve
`/simplify`'ın bulduğu ikisi: `Frame::pos`'un "**iki** listenin ortak
aritmetiği" doc'u (diff'te `+` satırı olmadığı için ilk taramada görülmedi) ile
kıvrım sınamasının "kanıtlayan **tek** yer burası" iddiası — `bt-atlas`'ın
`curl_is_really_a_wave`'i bitmap'i zaten kanıtlıyor, buranın kanıtladığı o
dalganın **GPU yolundan sağ çıktığı**. Liste kapalı değil: `Frame::push`'un
"ikisi de varsa ikisi de"si, `prepare`'in başlığı ve "glyph başına iki f32
bölmesi" satırı gibi kalemler de aynı taramada düzeldi — `/audit` mercek 10
sayının kapalı okunmasını bulgu saydı, haklı.
Belgeler tablosunun (bölüm 6) sorduğu "atlas ödüncü **bu fonksiyonun içinde
doğar ve burada ölür**" cümlesi okundu ve **hâlâ doğru**: `slot_uv` ödüncü almıyor,
`prepare`'in aldığı tek `borrow_mut`'un altında koşuyor.

### `/simplify` kaydı

Dört mercek (reuse, simplification, efficiency, altitude) paralel koştu.

**Uygulanan:** `cell_bg_paints_pixels_on_the_gpu` da `render_offscreen`'a
taşındı (Reuse 1 — beşinci bir kopya kaldığı sürece yardımcının kendi doc'u
yalan söylüyordu ve `MTLCommandBufferStatus::Error` kontrolü iki yerde
yaşıyordu); `kural=R`'nin sınırı **tek yere** indi (Simplification 3: aynı
cümle dört yerdeydi, üçü bekçi yeniden adlandırıldığında sessizce bayatlardı —
sahibi `Frame::rule_count`, `app.rs` yalnız duman kapısına özgü yarıyı
tutuyor); `Frame::pos`'un doc'u düzeldi (Simplification 4, not 8);
`const WHITE` + `rule_cell(col, underline)` ve `rule_cell_px` →
`fitting_cell_px` (Simplification 2 + Reuse'un notu: ad yalan söylüyordu, glyph
ve kalın sınamaları da onu çağırıyor — `edge` ve `clear` parametre **kaldı**,
ikisi de yük taşıyor: 16'ya karşı 64 ve clear renginin bilerek ayrık olması);
`instances.reserve(glyphs.len() + rules.len())` (Efficiency 3 — iki döngünün
ortasında birden çok kez büyümek yerine bir kez).

**Reddedilen (bulgu değil tercih ya da plan kararı):**

- **`sgr58_color_differs_from_foreground`'ı silmek** (Reuse 2) ve **kıvrım
  sınamasını "kural pikseli var + `assert_ne!`"e indirmek** (Reuse 3'ün silme
  yarısı) — ikisi de `plan.md`'nin onaylı checklist'inde; `/simplify` bir plan
  kararını çeviremez (phase-2 aynı sınırı yaşadı). Reuse 3'ün **yorum yarısı**
  uygulandı.
- **`GlyphCell` + `RuleCell` → tek `SpriteCell` ve tek döngü**
  (Simplification 1 = Altitude 2, iki mercekten aynı bulgu) — `plan.md` R4.2
  `RuleCell`'i adıyla istiyor ve derin biçimi `bt_atlas::Sprite`'ın şeklini
  (`Sprite::Char(char, Face)`) değiştirmeyi, yani diff dışındaki ~28 sınama
  literalini elden geçirmeyi gerektiriyor. Reuse merceği aynı yapıya bağımsız
  olarak baktı ve "bilinçli, dokunma" dedi. Gelecek bir setin işi olarak
  kayda geçiyor: "rules have no face" bugün üç ayrı yorumda yaşıyor, bir gün
  tipte yaşamalı.
- **Kural uv'lerini kare başına altı yuvaya memoize etmek** (Efficiency 1) —
  iki sebeple: `RuleKind`'ın varyant sayısı `bt-gpu`'ya elle yazılırdı
  (`RULE_RESERVE` `bt-atlas`'ta private) ve yedinci bir çeşit eklendiğinde
  **çizim yolunda** indeks paniği doğardı; ve bu maliyet zaten aşağıdaki
  "ölçüm bekliyor" kaleminin ta kendisi — ölçümden önce optimize etmek planın
  istediği ölçümü baştan geçersiz kılardı. Memo, o kalemin **aday çözümü**
  olarak oraya yazıldı.
- **`Faces::effective`'i `[Face; 4]` tablosuna indirmek** (Efficiency 2) —
  düzeltme `bt-atlas`'ta, bu diff'in dışında.
- **`FrameCounts { bg, glyphs, rules }` + tek `store_counts`** (Altitude 1) —
  bulgu gerçek: bir jeton eklemek üç crate'te sekiz düzenleme istiyor. Ama
  düzeltme 002/003'ün `pub` sayaç getter'larını yeniden yazıyor ve onlar bu
  diff'te değil; "geri alması en pahalı phase" bir de tesisat refactor'ü
  taşımamalı. Dördüncü jeton emoji setiyle geliyor — yeri orası.

### `/code-review` kaydı

**13 bulgu: 4 uygulandı, 8 devredildi, 1 reddedildi; waive yok.** Ayrım tek bir
ölçüte dayanıyor: **bu diff o yolu ilk kez canlandırıyor mu, ve doğru davranış
deponun kendi kararlarından belirsizliğe yer bırakmadan çıkıyor mu.** İkisi de
evetse aynı commit'te düzeldi (phase-1'in `renderer.rs` tek satırı ve phase-2'nin
`bt-gpu` sınama literalleri aynı ölçütle geçmişti: envanter kalemi, katman
ihlali değil), değilse hedefiyle birlikte kayda geçti.

**Uygulanan (4):**

- **`bt-atlas`: yüz geri düşüşü önbelleğe girmiyordu — kare başına CoreText.**
  `slot()`'un `NoGlyph if face != Regular` kolu düz yüze özyineliyor ama
  **istenen** anahtarı haritaya yazmıyordu. Bu kol phase-3'e kadar ölüydü
  (`Face::Regular` sabitti); bu commit onu canlandırıyor. Belirtisi: kalın
  yüzde olmayan bir glyph ekranda durduğu sürece **her karede** `raster::draw`
  → `glyph_index` → `NoGlyph` zinciri, ana thread'de, kare bütçesinin ortasında
  — tam olarak hemen üstündeki yorumun "önbelleğe girmeselerdi her karede
  yeniden sorulurdu" gerekçesi. Ödünç sırası düzeltmenin tamamı: `upload`
  `map`'le tüketilip `self.buffer` ödüncü bitiriliyor, `insert`'ten sonra
  `Upload` aynı baytlarla yeniden kuruluyor.
- **`bt-core`: imleç hücresi SGR 58 rengini koruyordu.** `fore` tersine
  dönüyor, `underline_color` dönmüyordu; aynı hücredeki iki kural iki farklı
  davranış gösteriyordu (üstü çizili `fg`'yi kullandığı için ters, alt çizgi
  terminalin seçtiği renkte) ve renk imleç bloğuna yakınsa çizgi büsbütün
  kayboluyordu. Bu alan da phase-3'e kadar okunmuyordu. Çözüm `fg`'nin
  kararına uyuyor: imleç hücresinde `underline_color = None`, yani çizgi
  tersine dönmüş ön planı kullanıyor.
- **`sgr58_color_differs_from_foreground`'ta `u8` taşması.** `red > g.max(b) + 64`
  açık bir clear renginde ya da yeşil/mavi bir kuralda "attempt to add with
  overflow" ile ölürdü — yanlış pikseli gösteren bir assert yerine. `u16`.
- **`proje.md` kendi içinde çelişiyordu:** dört jetonu sayıp "üçü de CPU
  sayacıdır" diyordu; `app.rs`'in aynı diff'teki yorumu "dördü de" diyor.

**Devredilen (8) — hepsi `teslim.md` borç listesine:** DIM'in
`underline_color`'a uygulanmaması (doğru davranış **doğrulanamıyor**:
alacritty'nin çizicisi bağımlılık değil ve `plan.md` R3.5 çözüm yolunu
dim'siz tarif ediyor — tahminle düzeltmek asıl hata olurdu); `dividing_period`'in
asal hücre genişliklerinde `Dotted` ile `Dashed`'i aynı desene indirmesi
(önerilen "en yakın bölen" düzeltmesi `w=13`'te `Dotted`'ı **düz çizgiye**
çeviriyor — tasarım kararı, hata düzeltmesi değil); `RULE_RESERVE = 6`'nın
`RuleKind`'ın varyant sayısına derleme zamanında bağlı olmaması (canlı hata
yok); `curl_is_continuous_across_cell_edges`'in uzunluk bekçisi eksikliği;
`Metrics`'in iki yeni alanının `pub` olması (`pub(crate)` yeterdi);
`curl()`'ün doc'unda bandın altını `position + thickness` demesi (kod
`CURL_FACTOR * thickness` — phase-1'in yön değişiminden kalma bayat cümle);
`Cargo.toml`'daki gerekçe yorumunun `features` dizisinin **içinde** durması;
ve `crates/bt-gpu/shaders/cell.metal:69`'daki `float kapsama` — `1dbb084`'ün
dil taramasının kaçırdığı tek tanımlayıcı, `.metal` da kod. Sonuncusu bu
phase'de düzeltilmedi çünkü `plan.md` R4.4 ve bu dosyanın `Yayın Etkisi`'nin
ikisi de ".metal'e dokunulmaz" diyor; sahibi `/ship` ya da shader'a meşru
biçimde dokunan ilk set.

**Reddedilen (1):** kural uv'lerinin kare başına hash'lenmesi — `/simplify`'ın
Efficiency 1'iyle **aynı bulgu, ikinci mercekten**; gerekçe yukarıda (ölçüm
bekliyor + varyant sayısının crate sınırını geçmesi). Reddin kendisi bu kadar
bağımsız iki yerden gelmesiyle güçlendi, o yüzden aday çözüm `Yayın Etkisi`'ne
yazıldı.

**Yan kazanç: phase-1'in "sınanamaz" dediği dal sınandı.** Phase-1
`## Uygulama Notları` 9'da glyph düzeyindeki geri düşüş için "Menlo'da düz
yüzün taşıyıp kalın yüzün taşımadığı bir karakter bulunamadı" yazıyordu.
Bulundu: **kutu çizim karakterleri** (`─ ━ │ ┃ ┄`, U+2500 bloğu) Menlo
Regular'da var, Menlo Bold'da yok (ölçüldü, bu makine). Yani dal seyrek bir uç
durum değil — kalın bir TUI çerçevesi tam bu koldan geçiyor.
`face_fallback_is_cached_under_the_requested_face` hem geri düşüşü hem
önbelleklemeyi sınıyor; ikisi de mutasyonla doğrulandı.

### `/audit` kaydı

**İlgisiz mercekler (elendi, kayda geçiyor):** 4 (ayar/tema şeması — model
yok), 5 (shell üçlüsü — `assets/shell/` el değmedi).

**Mekanik mercekler (inline):**

- **1 katman yönü ve platformsuzluk — temiz.** `cargo tree -p bt-core` platform
  kütüphanesi vermiyor ve kaynakta grep boş; `bt-atlas` yalnız
  `objc2-core-{foundation,graphics,text}`; `bt-gpu` ağacında `bt-shell` yok.
  `Face`/`Sprite`/`RuleKind` `bt-core`'a sızmadı, `(bold, italic) → Face`
  çevirisi `bt-gpu`'da kaldı.
- **2 yeni bağımlılık — temiz.** `Cargo.toml` ve `Cargo.lock` el değmedi.
- **3 panik yolu — temiz.** `bt-core`'a giren tek eşleşme `#[test]`
  özniteliği (grep'in `\[[a-z_]+\]` deseni yakalıyor). `bt-atlas`/`bt-gpu`'ya
  giren `expect`'lerin hepsi `#[cfg(test)]` içinde.
- **6 ölçüm sahipliği — temiz.** Belgelere giren satırlarda fps/gecikme/bellek
  iddiası yok; `docs/OLCUMLER.md` yok ve oluşturulmadı. `kural=15`, `hucre=8`,
  `glif=6` birer **iş sayımı** (reçeteden okunuyor), ölçüm değil.

**Yargı mercekleri (dört ajan, paralel, opus):**

- **7 thread ve blokaj — temiz.** Yeni işin tamamı `Term` kilidi
  **bırakıldıktan sonra** koşuyor (`link.rs` sırası: `session.frame(...)` döner,
  sonra `renderer.draw(...)`). `bt-atlas`'ın yeni `insert`'i ödünç ömrünü
  uzatmıyor. Dört yüz `Atlas::new`'de **eager** açıldığı için `glyph.face`'in
  çizim yoluna girmesi font açma ya da dosya G/Ç getirmiyor — bu diff'in açtığı
  tek gerçek risk buydu, kapalı. `last_rule_count` `Relaxed` (kardeşleriyle
  aynı), ödünç sırası `frame` → `atlas` ve tersi yok.
- **8 boşta sıfır kare — temiz.** Genişleyen kapı (`glyphs.is_empty() &&
  rules.is_empty()`) kare **kabulünün** aşağısında; kabulün tek yeri
  `Session::frame`'in `dirty.swap` kapısı ve o el değmedi. Yeni animasyon,
  zamanlayıcı ya da kare talebi yok; `link.rs` diff'te bile değil.
- **9 hücre boyutu ve shader/Rust düzeni — temiz.** 24 baytlık assert
  alacritty'nin **grid** hücresini ölçüyor ve el değmedi; `CLAUDE.md`'nin yeni
  yan tümcesi tam olarak bunu söylüyor. `RuleCell` GPU'ya hiç ulaşmıyor
  (`instance_buffer`'ın iki çağıranı `Instance` ve `GlyphInstance`), o ikisinin
  `offset_of` assert'leri ve `.metal` tarafındaki `static_assert`'ler oynamadı.
  Kural yuvası glyph yuvasıyla bayt bayt aynı geometride; instance sayısı tampon
  uzunluğundan türüyor.
- **10 belge ve üslup borcu — altı bulgu, altısı da uygulandı.** Bu mercek
  002'de 4, 003'te 6, phase-2'de 3 çelişki bulmuştu; bu phase'de altı ve hepsi
  **sayı** hatası: `render_offscreen`'in doc'u "beş sınama" diyordu (altı —
  `/simplify`'ın taşıdığı altıncıyı saymayı kaçırmıştım),
  `expect("hazirla dokuyu kurdu")` `1dbb084`'te ölen bir adı işaret ediyordu,
  `push`'un yeni yorumu "üç dal" diyordu (dört: arka plan, glyph, alt çizgi,
  üstü çizili), `cell_yields_up_to_two_rules`'un "dört iddia"sı beş assert
  sayıyordu, bu dosyanın not 8'i "dokuz cümle" derken listesi kapalı değildi ve
  "Checklist madde 6" aslında **bölüm** 6'yı kastediyordu. Ayrıca `proje.md`'de
  bir ünlü uyumu ve `CLAUDE.md`'de bir satır sarma düzeltildi. Tanımlayıcıların
  tamamı İngilizce, yorumlar ve tanı çıktıları Türkçe, `#[allow]` yok.

## Yayın Etkisi

- **shader** — **yok.** `.metal` dosyalarına dokunulmadı: kural sprite'ları
  var olan `cell` pipeline'ından geçiyor. `make shader` koşulu doğmuyor →
  `[~]`. (`discussion.md → Karar 4`, sprite rotası.)
- **terminfo / `TERM`** — yok.
- **ayar şeması** — yok.
- **tema / materyal** — yok.
- **shell entegrasyonu** — yok.
- **app bundle** — yok.
- **yeni bağımlılık** — yok; `Cargo.toml` ve `Cargo.lock` el değmedi
  (doğrulandı: `git diff HEAD --name-only` ikisini de göstermiyor).
- **belge** — madde 6'nın dört dosyası, **ve `CLAUDE.md` içinde beş ayrı
  kalem**: `make duman` satırı, jeton sözleşmesi listesine `kural=`, 003
  anlatısının 004 ile tazelenmesi, hücre maddesine grid/sınır ayrımı yan
  tümcesi (phase-2 devri) ve aynı maddenin "004'ün işi" cümlesinin
  düşürülmesi. `renderer.rs`'in "atlas ödüncü bu fonksiyonda doğar ve ölür"
  cümlesi okundu ve **hâlâ doğru** (bölüm 6'nın dördüncü satırı).
  Ayrıca `/code-review`'un canlandırdığı iki düzeltme kendi belgelerini
  getirdi: `Cell::underline_color`'ın doc'una imleç yan tümcesi, `slot()`'un
  geri düşüş koluna önbellek gerekçesi.
- **kod, phase dosyasının öngördüğü üç crate'in dışına çıktı** —
  `bt-atlas` ve `bt-core`'a birer `/code-review` bulgusu için dokunuldu;
  ikisi de **bu diff'in canlandırdığı** yollar (phase-3'e kadar `Face`
  hep `Regular`, `underline_color` hiç okunmuyordu). Gerekçe ve ölçüt
  `### /code-review kaydı`'nda.
- **duman sözleşmesi** — jeton **eklendi, silinmedi**: `kural=R`. Okuyan
  taraf tanımadığı jetonu atlar; geri alınırsa `kural=` kaybolur ve bu
  `teslim.md → Geri Alma`'ya yazılır. Ölçülen satır:
  `kare=1 hucre=8 glif=6 kural=15 pipeline=ok` — `hucre` ve `glif` bit bit
  korundu, `R` phase metninin öngördüğü 7 değil **15** (not 1).
- **ölçüm bekliyor:** kare başına kural instance'larının ve `slot()`'un
  ikinci çağrı sınıfının kare süresine etkisi — 003 `teslim.md` B.1
  **#2** ve **#5**'in genişlemesi. Kancası yok; `/measure` yine "ölçüm aracı
  yok" der. **Aday çözüm** (`/simplify` → Efficiency 1, ölçümden önce
  uygulanmadı): kural uv'lerini `prepare`'de kare başına altı yuvalık bir
  memoya almak — `Atlas::slot` kural başına değil kare başına en çok altı kez
  çağrılır. Bedeli `RuleKind`'ın varyant sayısını `bt-gpu`'ya yazmak; ölçüm
  kazancı gösterirse `RULE_RESERVE` `pub` edilerek ödenir.
- **ölçüm bekliyor:** altı çizili hücrenin artık **iki** tam hücre dörtlüsünü
  alfa pipeline'ından geçirmesi (glyph + kural), oysa kuralın kapsaması yalnız
  birkaç bant satırı. Fragment tarafı; "aynı tampon, aynı draw call" kararının
  (`discussion.md` → Karar 4) doğrudan sonucu ve `cell_rule` pipeline'ı
  kapsam dışında bırakılırken bilinen bedeldi.

---

## Checklist

- [x] `yuz(bold, italic) -> Face` çevirisi + "iki tip bilerek ayrı" yorumu
- [x] `GlyphCell` yüzü taşır; `GlyphInstance` **taşımaz**
- [x] `RuleCell` + `Frame.rules` + `rule_count()`; `bg_count` el değmedi
- [x] `push()` hücre başına ikiye kadar kural üretir; renk `underline_color ?? fg`, üstü çizili hep `fg`
- [x] `encode_glyphs`: kurallar glyph'lerden **sonra**, aynı tamponda, aynı draw call'da, `Face::Regular` ile
- [x] `app.rs`: `kural={r}` jetonu, kapı `r > 0`, jetonun sınırı yoruma yazıldı
- [x] Test: `kural_bandi_x_boyunca_tekduze_degil` — kıvrım gerçekten dalga (offscreen)
- [x] Test: `sgr58_rengi_on_plandan_farkli` (offscreen)
- [x] Test: `kalin_ve_duz_ayri_cizilir` — aynı karakter iki yüzle iki farklı piksel kümesi verir (offscreen, `assert_ne!`)
- [x] Test: `imlecin_ustundeki_kural_gorunur` — imleç hücresindeki kural imleç bloğuyla örtülmüyor
- [x] Belgeler: `CLAUDE.md` (jeton + duman satırı + "004'ün işi" cümlesi), `Makefile`, `proje.md`
- [x] **phase-2'den devir** (`/audit` mercek 9): `CLAUDE.md`'nin hücre maddesine "24 bayt **grid** hücresidir; `frame()` sınırının `Cell`'i ayrı bir kare kaydıdır" yan tümcesi. Sınır hücresi 004'te 5 alandan 10'a çıktı ve maddeyi okuyan "hücreye alan eklendi, yan tablo neden yok" diye okuyor; gerekçe `bt-core/src/lib.rs` ve `session.rs`'te yazılı ama `CLAUDE.md`'de değil
- [x] **phase-2'den devir** (`/audit` mercek 10, bulgu değil not): `CLAUDE.md`'nin 003 anlatısındaki "`frame()` sınırı karakteri ve ön plan rengini geçiriyor" cümlesi artık eksik — sınır beş alan daha geçiriyor
- [x] Doğrulama geçti — `make hepsi` → 0, `make test-yaris` → 0, `make duman` → `kare=1 hucre=8 glif=6 kural=15 pipeline=ok`; `make shader` **[~]**: `.metal` ve `build.rs` el değmedi (doğrulandı), koşulu doğmuyor
- [x] `/simplify` çalıştırıldı, bulgular uygulandı (kaydı yukarıda: 5 uygulandı, 5 reddedildi)
- [x] `/code-review` çalıştırıldı — 13 bulgu: 4 uygulandı, 8 devredildi, 1 reddedildi; waive yok
- [x] `/audit` çalıştırıldı — 8 mercek koştu, 2 elendi; tek bulgulu mercek 10 (6 bulgu, 6'sı da uygulandı)
- [x] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
