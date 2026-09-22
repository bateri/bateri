# Emoji ve geniş glyph — Tartışma

Karar-listesi biçimi: beş karar noktası birbirinden bağımsız ve ikisi
(K1, K2) yol haritasının "gerçek mimari çatal" dediği yerin ta kendisi.
Sayılar `context.md` → Ölçüm'den; burada tekrar edilmiyor.

## Karar 1: Geniş glyph iki hücreyi nasıl boyar?

`GlyphInstance` 32 bayt, boyut taşımıyor ve hücre ölçüsü kare başına tek
uniform. Üç yol var.

**1A — instance'a boyut eklemek.** `GlyphInstance`'a `size: [f32; 2]` (ya da
tek `cols: u32`) girer, vertex shader dörtlüyü ona göre gerer.
- **Artı:** tek kaynak; ileride üç hücrelik bir şey de aynı yoldan gelir.
- **Eksi:** stride 32'den çıkıyor ve **iki tarafta** `static_assert` kırılıyor
  (`cell.metal` ayrıca `uv0@8`, `rgba@16` ofsetlerini çiviliyor). Shader'ın
  kendi yorumu düzenin dolgusuz örtüşmesini anlatıyor: araya `float2` girince
  `rgba` 32'ye kayıyor ve MSL 48 bayt ediyor. Bedel her glyph'te ödeniyor —
  tek hücrelik binlerce harf de sekiz bayt daha taşıyor.
- **Eksi:** `uv_size` de uniform, yani yuva ölçüsü de instance'a inmek zorunda
  (ikinci `float2`) ya da atlas yuvaları tek boyda kalıp uv'yi shader'da
  çarpmak gerekir.

**1B — iki yarım yuva.** Geniş glyph **bir kez** 2w×h olarak rasterize edilir,
iki hücre boyunda yuvaya **bölünür** ve iki instance basılır: baş hücreye sol
yarı, spacer hücresine sağ yarı.
- **Artı:** **shader ve stride hiç değişmiyor** — stride 32, `cell_px` ve
  `uv_size` uniform kalıyor, `slot_bytes()`, `upload_slot` ve `raster::draw`'un
  assert'i el değmiyor. (İlk yazımı "`bt-gpu` hiç değişmiyor" diyordu ve panel
  bunu çürüttü: `renderer.rs:1186-1211` yuvayı `Sprite::Char(glyph.ch)` ile
  soruyor, yani "hangi yarı" ekseni `GlyphCell`'e ve onu dolduran sink'e
  giriyor. Değişmeyen shader ve stride; `bt-gpu` ile `Atlas` değişiyor. Gerçek
  kıyas ekseni "stride mı, yuva anahtarı mı" ve 1B orada hâlâ kazanıyor.)
- **Artı:** iki yarı iki ayrı yuva, yani doluluk sayacı ve `SLOT_TARGET`
  aritmetiği kendiliğinden doğru kalıyor (geniş karakter iki yuva harcar).
- **Eksi:** yuva anahtarı bir "hangi yarı" eksenini kazanıyor —
  `Sprite::Char(ch)` tek anahtarken iki yuva veremez.
- **Eksi (panel çürüttü):** "`nearest` örneklemede dikiş" — iki yarı 2w'lik
  bir tamponu bölmek yerine **iki kez rasterize** edilirse konusuz kalıyor:
  sağ yarı, glyph'i `-cell_px.0` **tam sayı** piksel ofsetiyle tek yuvalık
  tampona çizdirip CG'ye kırptırmakla elde ediliyor. Ofset tam sayı olduğu
  için AA fazı iki çağrıda birebir aynı ve `slot_bytes` / `upload_slot` /
  `raster::draw`'un assert'i hiç değişmiyor.
- **Eksi (gerçek olan):** `Atlas::slot` tek `(u16, Option<Upload>)` dönüyor ve
  `Upload` **tek** tamponu ödünç alıyor — bir geniş karakter iki yükleme
  üretmek zorunda. Dahası kapasite sınırı **iki yarının arasına** düşebilir
  (sol yuva açılır, sağ `TOFU` → yarım glyph + yarım kutu), yani iki yuva
  **atomik** ayrılmalı ya da birlikte reddedilmeli.

**1C — dördüncü pipeline.** Geniş glyph'ler kendi listesinde, kendi
`cell_px`/`uv_size` uniform'larıyla (ikisi de iki katı) çiziliyor.
- **Artı:** stride'a dokunmuyor, yuva anahtarına eksen eklemiyor.
- **Eksi:** dördüncü pipeline + ikinci draw call + atlasta **komşu iki yuva
  ayırma** (bugün yuva tahsisi tek tek ve sıradan; 2w'lik bir yuva ızgaranın
  satır sonuna denk gelirse sarma kuralı doğuyor).
- **Eksi:** çizim sırası sorusu yeniden açılıyor — kural çizgileri geniş
  glyph'in de üstünde kalmak zorunda.

**Ölçütün kendisi de bir karar** (K1'in altında): geniş yol **ızgaranın sütun
sayısına** mı bağlanacak (`WIDE_CHAR` bayrağı) yoksa **glyph'in kendi
mürekkebine** mi? Ölçülen 65 karakter tam bu ayrımda duruyor: 2 sütunlu ilan
edilmiş, bugün tek hücreye çiziliyor ve **bozuk görünmüyor** (Menlo'nun `☕`'si
tam hücre ilerlemesinde, fullwidth `！` narin bir işaret). Bayrağa bağlamak
onların 21'ini Menlo'dan 2 hücrelik yuvaya taşır — sağ yarısı boş bir raster.

## Karar 2: Renkli bitmap nerede yaşar?

Atlas `R8Unorm` ve bir Metal dokusunun **tek** formatı var; "sprite başına
format bayrağı" bu yüzden kendi başına bir yol değil, aşağıdaki ikisinden
birine iniyor.

**2A — tek atlas, RGBA'ya geçmek.** Doku `RGBA8Unorm_sRGB` olur, maskeler tek
kanalı kullanır.
- **Artı:** tek doku, tek pipeline, tek liste; `prepare` ve `slot_uv`
  dokunulmadan kalıyor.
- **Eksi:** bellek dört katı ve kazanan tarafı yok — 421 yordamsal + ASCII ×
  dört yüz maskeleri üç kanalı boşa harcıyor. `SLOT_TARGET`'ın türetmesi
  (022) doku kenarını bayta değil yuvaya bağlıyor, yani sayı değişmez ama
  ayak izi dörde katlanır.
- **Eksi:** `cell_fragment` "tek kanal kapsama, renk instance'tan" cümlesini
  kaybediyor: her maske örneklemesi artık dört kanal okuyor.

**2B — ikinci doku + kardeş fragment.** Emoji kendi `RGBA8Unorm_sRGB`
dokusunda, `cell_vertex`'i **aynen** paylaşan ayrı bir fragment'le çiziliyor.
- **Artı:** 015'in caret'i bunun emsali — `caret_fragment` `cell_bg`'nin
  vertex'ini paylaşıyor, ayrılan yalnız fragment. Maskelerin dokusu ve
  cümlesi olduğu gibi kalıyor.
- **Artı:** emoji sayısı küçük, yani ikinci dokunun kendi yuva hedefi çok daha
  küçük olabilir.
- **Eksi:** `Atlas` ikinci bir yuva defteri, ikinci bir kapasite ve ikinci bir
  `texture_px()` kazanıyor; `prepare` kare başına dört kez koşuyor ve her biri
  hangi dokuyu soracağını bilmek zorunda.
- **Eksi:** dördüncü pipeline ve bir draw call daha.

**Blend her iki kolda da soru:** bugünkü durum `SourceAlpha /
OneMinusSourceAlpha` (RGB) + `One / OneMinusSourceAlpha` (alfa), yani
**ön çarpımsız** kaynak — `cell.metal` bunu adıyla yazıyor. CoreGraphics emoji
bitmap'ini **ön çarpımlı** veriyor; ya blend çarpanı `One` olur ya da yükleme
sırasında ön çarpım geri alınır. Yanlış seçimin belirtisi kenarda koyu halka
ve sessiz. `renderer.rs`'in "Blend **parametre değil**: üç pipeline da onu
istiyor ve sebepleri ayrı" cümlesi bu kararla dördüncü sebebini kazanıyor.

**Renk uzayı:** doku `RGBA8Unorm_sRGB` olmak zorunda, düz `RGBA8Unorm` değil.
Hedef `BGRA8Unorm_sRGB` ve donanım fragment çıktısını lineer sayıyor; sRGB
olmayan bir dokudan örneklenen emoji paleti **açar** ve belirti CLAUDE.md'nin
"renk uzayı sınırı geçer" maddesindeki sessiz kusurun aynısı olur.

## Karar 3: Spacer hücresinin sağ yarısını kim söyler? → panel kapattı

**Bu başlık bir karar değil.** Üç kolun ikisi temsil düzeyinde kırık ve
kalan tek kol da sorunun kendisini ortadan kaldırıyor; ayrıntı
`## Muhakeme`'de. Kolların gövdesi tarihli kayıt olarak duruyor.

İki olgu ilk yazımda yanlıştı:

1. **Spacer sınırdan zaten geçiyor.** `session.rs:2113` `ch`'i `None`'a
   indiriyor ama `:2126` `drawable`'a `SPACERS`'ı **açıkça** katıyor ve
   gerekçesi doc'ta yazılı (seçili CJK'nın yarısı vurgusuz kalmasın). Yani
   kayıt hazır; soru "ikinci kayıt basılsın mı" değil.
2. **`bt-core` bayrağı zaten okuyor.** `session.rs:2288` `Flags::WIDE_CHAR`'ı
   `last_col` için kullanıyor.

**3A — sınır hücresi söyler.** `bt_core::Cell` "bu hücre önceki geniş
karakterin sağ yarısı" bilgisini taşır (yeni bir alan ya da `ch`'in yanında
bir bayrak).
- **Artı:** karar `bt-core`'da, yani terminal semantiği terminalde kalıyor;
  `bt-gpu` yalnız "hangi yarı" diye soruyor — `SizeClass` gibi bir raster
  ekseni.
- **Eksi:** sınır hücresine alan eklemek CLAUDE.md'nin adıyla yazılı ölçütüne
  tabi ("sınır hücresine alan eklerken ölçüt kare başına maliyettir").

**3B — `bt-gpu` hatırlar.** `Frame`'in sink'i "önceki hücre geniş miydi" diye
kendi durumunu tutar.
- **Eksi:** katman yönü. `bt-gpu`'ya terminal semantiği sızıyor ve `/audit`'in
  merceği tam buna bakıyor.

**3C — `bt-core` iki glyph basar.** Sink geniş hücreyi görünce **iki** hücre
kaydı üretir; sınır tipi değişmez.
- **Eksi (çürütüldü, kol ölü):** iki kayıt **ayırt edici taşımıyor**. Yan yana
  iki aynı geniş karakter — `ああ` — dört kayıt veriyor ve dördü de aynı `ch`'i
  taşıyor; `Frame::push` kayıtlar arası hiçbir şey hatırlamadığı için hangisinin
  sol, hangisinin sağ yarı olduğu **akıştan çıkarılamıyor**. Çıkarmanın tek
  yolu bekleyen-çift bayrağı, yani **3C = 3B** başka bir adla ve 3B'nin katman
  yönü eksisi ona da aynen geçiyor.
- **Not:** ilk yazımdaki "sınırın bir hücre bir kayıt değişmezi kırılıyor"
  eksisi **yanlış riski** gösteriyordu. Panel tüketicileri tek tek saydı:
  seçim (`anchor`, `session.rs:1264`), fare (`point_to_cell`, `Cell` hiç
  geçmiyor), kopyalama (`selection_text`) ve doluluk (`content_rows`) **hiçbiri
  sınır hücresini görmüyor**; kırılan tek şey `bg_count` defteri olurdu.

## Karar 4: Mürekkep kapısı ne kadar genişliğe izin verir? → karar değil, phase maddesi

Aday `fallback_font`'a `cell_advance` ile soruluyor. Geniş karakterde ölçüt
`cell_advance * sütun` olmalı.

Ölçüm kararı **kolaylaştırıyor**: adayı olan 1346 geniş karakterin
ilerlemesi de mürekkebi de tam olarak hücrenin **1.66** katı, yani 2.0
sınırının altında ve dağılım değil tek değer. Kapıyı sütunla çarpmak ailenin
tamamını kabul ediyor; ikinci bir eşik ya da "emoji mi" sorusu gerekmiyor.
Açık kalan: `centre_shift` de aynı sütun sayısını görmek zorunda, yoksa kapı
bir yerleşimi sınar, çizim başkasını uygular — `font.rs`'in "tek formül, iki
tüketici" kuralı.

## Karar 5: Çizim sırası ve caret

Emoji **opak renk**, maske değil. Bugünkü tek listeye giremiyor (renk
instance'tan gelmiyor) ve sıra şu üç şartı birden tutmak zorunda: kural
çizgileri en üstte (üstü çizili), caret'in altındaki metin caret'in rengini
alıyor (`cell_fragment`'in `mix`'i), emoji arka planı örtmeli.

- **Sıra önerisi:** arka planlar → caret → **emoji** → glyph + kural.
- **Emoji caret'in `mix`'ine girmiyor:** blok caret'in altındaki bir emoji
  tema renginde boyanamaz (renk dokudan geliyor); orada ya alfa ile kararma
  ya hiçbir şey. Karar gerekiyor.
- **Blok caret bir hücre genişliğinde.** İki hücrelik bir glyph'in üstünde
  duran caret bugün sol yarıyı kaplıyor. `caret_rect` iki dikdörtgen veriyor
  ve "tek yer iki tüketici" kuralıyla yazılı; geniş hücrede genişletmek mi,
  bilinen sınır olarak bırakmak mı?

## Karar Noktaları

Kullanıcıya gidecek sorular, karar başlıklarının üstünde:

1. **Kapsam — tek kod noktası mı, grapheme mi?** `Sprite::Char(char)` bir
   `char` ile anahtarlanıyor; ZWJ dizileri (`👨‍👩‍👧`), ten rengi
   değiştiricileri ve VS16 (`❤️` = U+2764 + U+FE0F) **bir karakter değil**.
   alacritty onları `CellExtra`'da tutuyor. Öneri: bu set tek kod noktalı
   emojiyi çizer, dizi desteği ayrı bir set (anahtar `char` değil `&str`
   olmak zorunda) — yoksa kapsam sessizce ikiye katlanır.
2. **78 tek sütunlu emoji.** Rengi var, iki sütunu yok, mürekkebi 1.66 hücre.
   Öneri: bu sette **kutu kalsın** ve adıyla yazılsın; çaresi küçültme ve o
   yol yol haritasındaki 190 karakterlik kalemle aynı karar (kitty yedek
   glyph'i ölçekliyor, eşaralıklı ızgarada farklı ağırlıkta görünüyor).
3. **Dock'un küçük sınıfı.** Bağlam satırı Japonca bir dizin adı taşıyabilir
   ve küçük sınıfın adımı `context_advance`. Geniş yol orada da koşacak mı,
   yoksa küçük sınıfta kutu mu? (021'in kapısı küçük sınıfta **kapalı** ve
   gerekçesi ölçü ayrışması — emsal var.)
4. **Aynanın `CURSOR`'u karakter indeksi, sütun değil.** Dock'un giriş
   satırında CJK varsa caret yanlış hücrede durur. Bu **bugün de** böyle;
   bu sette düzeltilecek mi, bilinen sınır olarak mı yazılacak?

## Muhakeme (2026-09-22)

Panel `/rfc` adım 6'nın pahalı karar sınıfından koştu: seçim katman yönüne
(`bt-gpu`'ya terminal semantiği) ve sınır `Cell`'ine alan eklemeye dokunuyor.

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU |
| Codebase-fit | SORUNLU |
| İşletme | SORUNLU |

Üçü de "bu iş yapılmasın" demiyor; üçü de **çatalların fazla** ve **sıranın
yazılmamış** olduğunu söylüyor. Üç mercek dört noktada bağımsız olarak aynı
yere geldi (K1=1B, K3'ün çökmesi, K4'ün karar olmaması, geniş-önce sırası),
yani bunlar tek jürinin zevki değil.

**Kabul edilen itirazlar → plana girecek değişiklikler:**

- **Set tek yönlü kilitli; sıra zorunlu.** Geniş glyph kolu tek başına ≈440
  karakteri (Hiragino 204 + PingFang 220 + Apple Symbols 12 + diğer 10)
  **sıradan maske** olarak çiziyor: doku `R8Unorm` kalıyor, shader'a
  dokunulmuyor, sRGB/ön çarpım/blend soruları hiç açılmıyor. Emoji kolu tek
  başına **sıfır** karakter çiziyor (922'si geniş yolu bekliyor, 78'i kapsam
  dışı). → Phase sırası: **P1 geometri**, **P2 renk**. Set bölünmüyor,
  bağımlılık kabul ediliyor.
- **K4 kendi başına regresyon; geometriyle aynı commit'te inmeli.** Kapı
  `cell_advance * 2` ile 1.66× adayı kabul eder, `centre_shift` iki sütuna göre
  kaydırır, raster tamponu ise tek yuva (`Atlas.buffer = vec![0u8;
  slot_bytes()]`, `lib.rs:315`) → mürekkebin yarısı kırpılır. Bugün o
  karakterler **temiz kutu**; K4-alone yarım glyph, yani `CLAUDE.md`'nin
  "kutu görünür bir eksiklik, kırpılmış glyph sessiz bir bozulma" cümlesini
  projenin kendi sözleşmesiyle bozar.
- **Kapı `min(sütun, mürekkep)` olmak zorunda.** Yalnız mürekkebe bakan bir
  kapı 78 tek sütunlu emojiye iki hücre verir ve **spacer olmadığı için**
  komşunun üstüne taşar.
- **Kapının sırası da karar ve K1'in altındaki soruyu o çözüyor.** Sıra:
  geniş hücrede **önce** `ink_fits_cell(…, cell_advance)`; geçerse bugünkü tek
  yuvalı yol (raster **bit bit aynı**), geçmezse `cell_advance * 2` ile ikinci
  kapı, o da geçmezse kutu. Böylece "kutu ya da tam glyph" sözleşmesi geniş
  karakterde de aynen sürüyor, ölçülen **65 çalışan çizimin hiçbiri oynamıyor**
  ve o 65 ikinci bir yuva da harcamıyor — ölçütü mürekkebe bağlamanın kapasite
  bulgusundan doğan ikinci ve bağımsız argümanı bu. **Tuzak:** cascade'den
  gelen 44 narin karakter (`、 。 》 ！`) bu sıradan geçmek **zorunda**, yoksa
  `centre_shift`'leri iki hücrelik kutuya göre hesaplanır ve bugünkü
  konumlarından kayarlar.
- **Sütun sayısı `bt-atlas`'ta hesaplanamaz.** `unicode-width` bugün hiçbir
  kendi crate'imizin listesinde değil (yalnız `alacritty_terminal`'ın
  bağımlılığı) ve "bağımlılık mimari karardır". Daha kötüsü **ikinci bir
  genişlik yetkilisi** doğar. → `bt-atlas` kutu genişliğini **argüman olarak**
  alır (`box_advance`); `ink_fits_cell` ile `centre_shift` ikisi de aynı sayıyı
  görür ("tek formül, iki tüketici" zaten öyle yazılmış).
- **Atlas kapasitesi 021→022'yi tekrarlıyor ve bu kez sınırsız bir aile ile.**
  Bugün kapıdan dönen CJK **sıfır** yuva harcıyor (`lib.rs:617` negatif
  önbelleğe `TOFU` bağlıyor, `next` artmıyor — doğrulandı). Bu setten sonra
  kabul edilen her geniş karakter **iki** yuva harcar: 13pt@2x'te karakterlere
  açık 1976 yuva → **988 ayırt edici geniş karakter**, 80×24'te bir ekran 960
  geniş hücre. Tahliye yok, yani sıradan bir CJK oturumu 988'i aşınca o
  oturumda ilk kez görülen **her** karakter (Latin harfleri dahil) `ensure()`
  atlası yeniden kurana kadar kutu çıkar; belirti görünür, sebebi görünmez,
  hiçbir sayaç kıpırdamaz. **022'nin bekçisi bunu göremiyor**
  (`capacity_clears_the_family_at_every_accepted_size` yordamsal aileyi sayıyor
  ve o aile baştan sona tek sütunlu → yeşil kalır). → Set kendi bekçisini
  getirir (`full_atlas_returns_tofu_without_caching` geniş bir karakterle
  genişletilir: bir geniş karakter tam iki yuva harcar, dolu atlasta **tofu**
  döner) ve sınır adıyla yazılır. Çare (022'nin adlandırdığı `encode_pass`
  sınırında geri dönüşüm) bu setin kapsamı **değil**.
- **İki yuva atomik.** Kapasite sınırı iki yarının arasına düşerse sol yuva
  açılır, sağ `TOFU` olur → yarım glyph + yarım kutu. İkisi birlikte ayrılır
  ya da birlikte reddedilir.
- **Yüzey üç, sink iki.** `bt-gpu`'da glyph/kural listeleri üç grup (ızgara
  `frame.rs:522`, dock `:652`, doldurma bandı `:666`) ve `encode_glyphs` kare
  başına dört kez koşuyor. `bt-core`'da atlama kapısının **iki kopyası** var
  (ızgara sink'i `session.rs:2113`, doldurma sink'i `:2469`) ve dosyanın kendi
  yorumu şartı yazıyor: "iki taraf aynı hücreye bakıp farklı cevap verseydi
  doldurulan satır ekrana çıktığındakinden farklı görünürdü". → Geniş bayrağı
  **iki sink'e**, yelpazeleme ve emoji çizimi **üç yüzeye** birden iner.
  Emsal ve bedeli yazılı: 017 bandın hücrelerini aldı, **blok işaretini
  almadı** ve kusuru kullanıcının gözü buldu.
- **Üçüncü bayrak atlanmış.** Spacer iki değil üç hâl: `WIDE_CHAR_SPACER` ve
  `LEADING_WIDE_CHAR_SPACER` (`session.rs:2036`). İkincisi satır sonuna
  sığmayan geniş karakterin bıraktığı boşluk ve **sağ yarı almamalı**; alırsa
  satır sonunda kopmuş bir glyph çıkar.
- **Dock'ta geniş yol yapısal olarak ölü olmalı.** `dock.rs:200` sütunu
  **karakter indeksinden** veriyor (`TEXT_COL + offset`, `offset = index -
  skip`), spacer yok, genişlik farkındalığı yok; caret de öyle (`:212`).
  Aynada CJK varsa iki hücrelik bir glyph komşu karakterin üstüne boyar. →
  Geniş bayrağı dock kayıtlarında **hiç kurulmaz** ve bu bir varsayım değil
  adıyla yazılmış bir değişmez olur. 021'in "kapı küçük sınıfta kapalı"
  emsali **yetmez**: dock'un giriş satırı `SizeClass::Normal`.
- **sRGB ve ön çarpımın tanığı sentetik olmalı.** İkisi de sessiz kusur ve
  bugün tanığı yok. Emoji bitmap'i CoreGraphics'ten geliyor, macOS sürümleri
  arasında bit bit sabit değil → gerçek emojiye bakan bir bekçi yanlış güven
  verir. Örüntü depoda: `cell_bg_paints_pixels_on_the_gpu` ancak bir **ara
  ton** ile görüyor, çünkü `0.0` ve `1.0` sRGB transferinin sabit noktaları
  (`renderer::tests::MIDTONE`). → Bilinen ara tonlu bir RGBA yuvası yüklenip
  offscreen okunur.
- **Blend'de değişen tek çarpan RGB kaynağı.** Bugün RGB `SourceAlpha /
  OneMinusSourceAlpha`, alfa `One / OneMinusSourceAlpha`. Ön çarpımlı emoji
  için RGB kaynağı `One` olur; **alfa tarafı zaten doğru**. Bedeli: `pipeline()`
  008 phase-5'te attığı `enum` parametresini geri alıyor — yazılı bir kararın
  geri alınması, planda gerekçelenir.
- **2A bugün yazıldığı hâliyle çalışmıyor.** `cell_fragment`
  `coverage = atlas.sample(s, in.uv).r` yapıp rengi instance'tan alıyor ve
  `GlyphInstance`'ta **ayırt edici alan yok** — RGBA dokuda maske ile görüntüyü
  ayıran hiçbir şey kalmıyor. Dürüst hâli maskeleri `(1,1,1,coverage)` olarak
  yazıp fragment'i `sample * in.rgba`'ya indirmek. → Kıyasa bu hâliyle girer.
- **2A'nın "bellek dört katı" eksisi zayıf.** Varsayılanda doku 1024×1024,
  yani 1 MiB → 4 MiB (pencere başına 3 MiB); `MAX_EDGE`'de bile 16 → 64 MiB ve
  orası nadir köşe. Gerçek bedel **sıcak maske yolunda 4×**: her `upload_slot`
  dört kat bayt yazıyor, her glyph fragment'i dört kanal örnekliyor. → Eksi
  bellekten bant genişliğine düzeltilir.
- **`bt-gpu` değişiyor; shader ile `GlyphInstance` değişmiyor.** Codebase-fit'in
  sketch'i "kayıtlar arası durum YOK" diyor ve bu doğru, ama 021'in
  "`bt-gpu` habersiz" örüntüsünün **yarısı**: `GlyphCell` bir `half` alanı
  kazanıyor ve `Frame::push` tek kaydı iki `GlyphCell`'e yelpazeliyor — üç
  yüzeyde birden. Kayıtlar arası hatırlama yok, `bt-gpu`'ya dokunmama **yok**.
- **Ön çarpımın ikinci kolu da yazılı olsun.** Blend çarpanını değiştirmenin
  alternatifi yükleme sırasında ön çarpımı **geri almak**: bedeli düşük alfada
  hassasiyet kaybı ve yuva başına bir CPU turu. P2 kararı iki kolu da görsün.
- **`LEADING_WIDE_CHAR_SPACER`'ın tuzağı yalnız kuyrukta.** Sarılan geniş
  karakterin `wide` bayrağını taşıyan **baş** hücresi bir sonraki satırın 0.
  sütununda ve yelpazeleme orada sorunsuz; alınmaması gereken şey önceki
  satırın sonunda kalan o boşluk.
- **`Sprite`'a varyant eklenmiyor, anahtara eksen ekleniyor.** `Sprite`'ın
  doc'unun reddettiği şey renderer'a **terminal semantiği** sızdırmaktı; "hangi
  yarı" ekseni çağıran tarafından **taşınırsa** `SizeClass`'ın tam kardeşi
  (`Face`'e dik, `Atlas::slot`'un anahtarında normalize). `bt-gpu`'da
  **hesaplanırsa** reddedilen gerekçeye tam olarak girer.
- **Belge yükü `discussion.md`'de hiç anılmamış.** `CLAUDE.md` ("Pipeline
  **üç**", "atlası `R8Unorm` dokuya bağlar", "Emoji ve geniş glyph henüz yok"
  paragrafı, `bt-atlas`/`bt-gpu` sorumluluk satırları), `renderer.rs:1093`
  ("Blend parametre değil: üç pipeline"), `docs/YOL-HARITASI.md` (023 satırı,
  atlas doyma borcu, 190 karakterlik küçültme kalemi) ve `docs/OLCUMLER.md`
  (envanter taraması — dosya kendini ölçülmüş sayıların **tek sahibi** ilan
  ediyor). → **Karar: kayıt**, `## Atlas yuva ayak izi` altına; 021'in aile
  sayısı zaten orada yaşıyor. Yazılı istisna kolu reddedildi: 019/015'in
  emsalleri phase dosyasındaki tek tük sayılardı, bu ise 3521 kod noktalı bir
  tarama.
- **`yuva=` jetonu ikinci düzlemi görmüyor.** Duman kapısının `yuva=13/2048`
  sayacı yalnız maske atlasını sayıyor. Tek satırlık karar; "jeton silinmez,
  eklenir".

**Reddedilenler:**

- **K2 = 2A (tek doku).** Sadelik'in dürüst yeniden yazımı gerçekten daha az
  kavram ve K5'in çizim sırası yarısını **tümden kapatıyor** — bu bilerek
  takas ediliyor. Red gerekçesi **iki** kalem: (a) emoji yuvaları maskelerle
  **aynı havuza** giriyor ve
  yukarıdaki kapasite bulgusu 922 × 2 yuvayı o havuza bindiriyor; (b)
  `cell.metal`'ın "tek kanal kapsama, renk instance'tan" cümlesi hayatta
  kalıyor. **Üçüncü bir gerekçe yazılmıştı ve geri alındı:** "sıcak maske
  yolu ölçülmemiş 4× örnekleme bedeli ödüyor" cümlesi `CLAUDE.md`'de olmayan
  bir kurala ("ölçülmemiş bedel en sıcak yola yazılmaz") dayanıyordu ve
  gerçek kural **ters yöne** kesiyor: encode sütunu kare bütçesinin %3'ünde
  ölçüldü ve o sayı önbelleği **kapalı tuttu**, çünkü saklanacak kazanç
  ölçülebilir değildi. Aynı mantıkla %3'lük bir yolda 4× örnekleme bir red
  gerekçesi değil, bir `/measure` sorusu — P2'de K2 kararlaşırken ölçülüp
  karşılaştırılabilir. İki gerekçe yeterli. Reddin bedeli açık: emoji çizimi
  **üç yüzeyde** ayrı ayrı kazanılmak zorunda.
- **İkinci bir `Atlas` açmak.** 2B'nin doğru biçimi `Atlas`'ın **içinde**
  ikinci bir düzlem. İkinci bir `Atlas` beş CoreText türetmesini (dört yüz +
  `small`) ve aynı anahtardan **ikinci bir `Metrics`**'i doğurur; `sync_atlas`
  (`renderer.rs:451`) tam bunu önlemek için var ve `context_cell_w`'nin doc'u
  aynı kokuyu adıyla yazıyor. İkinci düzlem ayrıca kendi **monoton** `next`'ini
  tutmak zorunda: uv `prepare` anında pişiyor ve dördüncü geçiş koşarken ilk
  üçün tamponları zaten encode edilmiş, yani kare ortasında anlamı değişen
  paylaşımlı atlas durumu yasak.
- **K1 = 1A ve 1C.** 1A iki taraflı `static_assert` çiftini kırıyor
  (`frame.rs:78-80` + `cell.metal:22-24`) ve tuzağı `cell.metal:11`'de yazılı:
  sona eklenen bir `float2` Rust'ta 40, MSL'de 48 bayt eder — naif ekleme iki
  tarafı uzlaştırmıyor, açık dolgu şart. İkisi de dokuda **komşu** 2w'lik bir
  bölge istiyor (tek `uv0` + `uv_size` ile bitişiklik zorunlu) ve
  `slot_origin`'in `slot < capacity` bekçisi satır sonu taşmasını **görmüyor**.
  1B bu ikisinden de muaf: `next` skaler bir bump tahsisçisi ve her instance
  kendi `uv0`'ını taşıdığı için iki yarının yan yana olması **gerekmiyor**.
- **K5'in "geniş hücrede blok caret" sorusu.** Geniş glyph iki ayrı tek
  hücrelik dörtlü olarak çizildiği için `cell_fragment`'in `mix`'i **fragment
  başına** `cursor.rect` ile karşılaştırıyor ve caret'in yarım kapsaması
  kendiliğinden doğru çıkıyor — 008'in "blok iki hücre arasındayken hücrenin
  yarısı ezilir" davranışının aynısı. Yeni karar gerekmiyor; soru listeden
  düşüyor.
- **`Cell`'e alan eklemenin ikinci bir ölçüm kampanyası istemesi.** Sayı
  ölçüldü: `bt_core::Cell` bugün **68 bayt, hiza 4, dolgu sıfır**; bir `bool`
  onu **72**'ye çıkarıyor (+%5.9). `CLAUDE.md`'nin ölçütü kare başına maliyet
  ve tipin **tek tamponlanan dizisi** doldurma bandının `Vec<bt_core::Cell>`'i
  (`link.rs:514`, kapasitesi korunuyor); kalan her yol satır içine alınmış
  değer-geçişli sink. Planın bu iki cümleyi yazması yeterli.

## Karar (2026-09-22, kullanıcı onayı — dört karar noktası bana bırakıldı)

Panelin kapattığı kalemler yukarıda (`## Muhakeme`). Kullanıcıya sunulan dört
karar noktası da **bana bırakıldı** ("bunların anlamını bilmiyorum"), yani
aşağıdaki dördünün gerekçesi burada yazılı olmak zorunda: chat'te kalan karar
kaybolur ve bu dördünün hiçbirinin ikinci bir sahibi yok.

- **Seçilen — K1 = 1B, iki kez rasterize.** Sağ yarı `-cell_px.0` **tam sayı**
  piksel ofsetiyle tek yuvalık tampona çizdirilip CG'ye kırptırılıyor; iki yuva
  **atomik** ayrılıyor. `slot_bytes`, `upload_slot`, `raster::draw`'un assert'i,
  shader ve `GlyphInstance` stride'ı el değmiyor. `bt-gpu` yine de değişiyor:
  `GlyphCell` bir `wide: bool` kazanıyor ve **yelpazeleme `prepare`'de**.
  Yelpazelemenin yeri planın en ince kararı ve sonradan düzeltildi: ilk yazım
  onu `Frame::push`'a koymuştu, ama "bir yuva mı iki mi" kararı kapı sırasının
  (R3) koştuğu yerde, yani `Atlas::slot`'ta veriliyor ve `push` atlası ödünç
  alamıyor (`GlyphCell`'in uv'siz olmasının yazılı sebebi). Menlo'nun `☕`'si
  köşeyi gösteriyor: `Cell.wide` kurulu ama tek hücrelik kapı geçiyor, yani
  tek instance olmalı. `prepare` atlası zaten ödünç almış, `metrics`'i elinde
  ve kare başına dört kez koşuyor — yani üç yüzey de tek gövdeden kazanılıyor.
- **Reddedilen — 1A, 1C.** Gerekçe `## Muhakeme` → Reddedilenler.
- **Seçilen — kapı sırası: tek hücre önce.** Geniş hücrede önce
  `ink_fits_cell(…, cell_advance)`, geçerse bugünkü tek yuvalı yol, geçmezse
  `cell_advance * 2`, o da geçmezse kutu. Ölçülen 65 çalışan çizim oynamıyor ve
  ikinci yuva harcamıyor.
- **Seçilen — K3 yok: `wide: bool` yalnız baş hücrede.** `Cell` 68 → 72 bayt.
- **Seçilen — K2 = `Atlas`'ın içinde ikinci düzlem** (`RGBA8Unorm_sRGB`,
  yuvalar yine hücre boyunda) + `cell_vertex`'i aynen paylaşan kardeş fragment.
- **Reddedilen — 2A (tek doku) ve ikinci bir `Atlas`.** Gerekçeler
  `## Muhakeme` → Reddedilenler; 2A'nın kapattığı K5 yarısı bilerek takas
  edildi ve bedeli (üç yüzeyde ayrı kazanım) yazılı.

### Kullanıcının bana bıraktığı dört karar

1. **Grapheme dizileri kapsam dışı.** İçeride: **tek kod noktalı** emoji.
   Dışarıda: ZWJ dizileri (`👨‍👩‍👧`), ten rengi değiştiricileri, VS15/VS16.
   Gerekçe temsil: `Sprite::Char(char)` bir `char` ile anahtarlanıyor ve bir
   diziyi ifade edemiyor; sonraki setin anahtarı **`char` değil `&str`** olmak
   zorunda ve o, atlas anahtarının tamamını değiştiren ayrı bir iş.
   **Görünür sonucu dürüstçe:** `❤️` (U+2764 + U+FE0F) bugün maske yolundan
   metin sunumlu `❤` olarak çiziliyor ve bu setten sonra da öyle çizilecek —
   VS16 alacritty'nin ızgarasında sıfır genişlikli birleştirici olarak
   `CellExtra`'da yaşıyor, yani taban karakter çiziliyor ve seçici görünmüyor.
   Bu **bugünkü davranışın korunması**, yeni bir regresyon değil.
2. **78 tek sütunlu emoji kutu kalıyor** (`🌡 🎙 🏋 🏔`). Alternatifi 1.66
   hücrelik mürekkebi tek hücreye **küçültmek** ve o, yol haritasındaki 190
   karakterlik küçültme kaleminin ta kendisi — eşaralıklı bir ızgarada
   ölçeklenmiş glyph komşularından farklı ağırlıkta görünüyor ve karar o
   kaleme ait. **Parite açığı değil:** bu 78 karakterin sunumu Unicode'da
   metin varsayılanlı, yani gerçek terminaller de burada ayrışıyor (kitty
   ölçekliyor, alacritty kutu çiziyor).
3. **Aynanın `CURSOR`'u bilinen sınır.** Kusur **bu setten önce de var** ve bu
   set onu **kötüleştirmiyor**: dock'ta geniş yol yapısal olarak ölü
   (`dock.rs:200` sütunu karakter indeksinden veriyor), yani bugünkü
   davranış aynen sürüyor. Çaresi ya ZLE aynasının sütun göndermesi ya
   `bt-core`'un `BUFFER` üstünde gösterim genişliği hesaplaması — ikisi de
   dock'un kendi seti. → `docs/YOL-HARITASI.md` → sete bağlanmamış borçlar,
   "Dock çok satırlı girişi göstermiyor" maddesinin yanına.
4. **Duman jetonuna `yuva2=U/T` ekleniyor.** Bu bir soru değildi; projenin
   kendi kuralı cevaplıyor — "jeton silinmez, **eklenir**" ve duman kapısı her
   `make duman`'da koşan **tek** tanık. Göremediği bir düzlem tam olarak
   021'in Braille şekli olurdu: sıfır yuva harcayan, sessiz. Biçim `yuva=`'yi
   aynalıyor, yeri P2'nin checklist'i.
