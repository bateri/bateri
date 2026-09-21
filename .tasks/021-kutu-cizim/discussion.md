# Kutu çizim — Tartışma

Tek bariz yaklaşım var (**yordamsal çizim**, `RuleKind` emsali) ama yedi
karar noktası taşıyor. Bu yüzden seçenek biçimi değil karar-listesi.

Kararların üçü (1, 3, 4) panelden **sonra** değişti; gerekçeleri
`## Muhakeme`'de.

## Karar 1: Kapı nerede duruyor?

**A. `Atlas::slot`'un `Sprite::Char` kolunda**, `Sprite::Rule` kolunun
birebir ikizi olarak. Karakter yordamsal çizilebiliyorsa font hiç
sorulmuyor.

- `bt-gpu` ve `bt-core` **hiç değişmiyor**: `Sprite::Char(glyph.ch)`
  çağrısı aynı, yuva aritmetiği aynı.
- Sıra zorunlu ve kapı **yedekten önce**: `─` Menlo'da *var*, yani "fontta
  yoksa yordamsal çiz" yanlış kol olurdu — o karakter hiç yedeğe gitmiyor
  ama yine de bozuk. `⠋` ise Menlo'da yok ve yedek koşarsa Apple Braille
  gelip genişlik kapısından döner. İkisini de kapatan tek yer bu.
- `raster::draw` **font yolu olarak saf kalıyor** ve `DrawResult`'ın doc'u
  ("fontun cevabı") gerilime girmiyor.

**B. `raster::draw`'ın başında.** İlk turda önerilen yer; **panel çürüttü.**

- `draw`'ın imzası (`raster.rs:38-44`) `Face` de `SizeClass` de **görmüyor**,
  yani Karar 5 orada bedava değil — yeni parametre ister.
- Karar 2'nin yüz normalizasyonu yüklemi `slot`'a zaten koyuyor, yani
  `is_procedural(ch)` **iki yerde** yaşardı. Kayma yönü asimetrik ve
  tehlikeli tarafı sessiz: kapıda var / normalizasyonda yok → aynı bitmap
  dört yüz için dört yuva, 416 yerine **1664** yuva (`slot`'un kendi doc'u:
  *"ayrışan bir anahtar bayt bayt aynı bitmap'i ayrı yuvalarda tutar, atlas
  kat kat hızlı dolar ve belirti sessizdir"*).

**C. Üçüncü bir `Sprite` varyantı (`Sprite::Box(char)`).**

- **Eksi:** `bt-gpu`'nun bugünkü tek satırı "bu karakter hangi sprite"
  sorusuna dönüşür, yani renderer terminal semantiği taşımaya başlar
  (`proje.md` → tuzaklar).

**Öneri: A.**

**Yordamsal çizim fontu koşulsuz yener** ve bu kararın parçası: kullanıcı
Braille'i olan bir font seçse de yordamsal çizim kazanır. Gerekçe döşeme —
fontun em kutusu hücre kutusu değil ve bir fontun bunu vermesini garanti
edecek hiçbir ölçüt yok. 012 phase-9'un prompt işareti kararının aynısı:
*terminalin kendi işareti, kullanıcının fontunun değil.*

## Karar 2: Kutu çizim yüze duyarlı mı?

**Hayır, `Face::Regular`'a normalize edilir** — `slot`'un normalizasyon
`match`'ine dördüncü kol.

Unicode ince/kalın ayrımını **karakterin kendisinde** taşıyor (`─` U+2500
ince, `━` U+2501 kalın), yani SGR bold'un çizgiyi kalınlaştırması bilginin
iki kez kodlanması olurdu. Yan kazanç: dört yüz tek yuvayı paylaşıyor ve
kalın TUI çerçevelerinde atlas baskısı **düşüyor**.

**Bedeli bir sınama** ve phase-1'in borcu:
`face_fallback_is_cached_under_the_requested_face`'in fikstürü tam da `─`
(`lib.rs:1070-1099`, doc'u *"düz yüzde var, kalın yüzde yok… kalın bir TUI
çerçevesi bu koldan geçiyor"*). Normalizasyondan sonra `(Char('─'), Bold)`
anahtarı **hiç oluşmuyor** ve sınamanın taşıyıcı iddiası sessizce ölüyor —
`bold == regular` ile `occupancy == 2` yeşil kalır. O sınama
`DrawResult::NoGlyph if face != Regular` kolunun **tek** bekçisi, yani
fikstür değişmeli: Menlo Regular'da olan, Bold'da olmayan ve yordamsal
aralıkların dışında bir karakter — **ölçülerek** bulunacak
(`the_gate_decides_by_width_alone`'un tarama örüntüsü emsal).

## Karar 3: Kapsam — hangi karakterler?

Üç blok tartışmasız: **U+2500–U+257F** (çizgi çizim, 128), **U+2580–U+259F**
(blok elemanları, 32), **U+2800–U+28FF** (Braille, 256). Kenar iki küme:

**A. Yuvarlak köşeler (`╭╮╯╰`, U+256D–U+2570) — içeride.** Dört karakter ve
**ayrı bir teknik değil**: `chevron`'un mesafe alanı (`distance_to_segment`
+ `(half_stroke + 0.5 - d).clamp(0,1)`, `raster.rs:262-280`) yayda
`|hypot(x-cx, y-cy) - r|` olarak aynen çalışıyor. Aynı primitif düz kolları
da çiziyor.

**B. Köşegenler (`╱╲╳`, U+2571–U+2573) — dışarıda.** Üç karakter ve gerçek
bir kapsam kararı: mesafe alanı onları da çizebilirdi, ama üçü de nadir ve
setin ölçüsünü tutmak kapsamı kapalı tutmaktan geçiyor. Fonttan gelmeye
devam etsinler.

**Ara durum bozuk değil:** kapsanmayan karakter bugünkü yolundan geliyor.

**Öneri: A içeride, B dışarıda.**

*İlk turda yay için emsal olarak `curl` gösterilmişti ve **yanlıştı**;
gerekçe `## Muhakeme` → İtiraz 3.*

## Karar 4: Tasarım sabitleri ve bir çatal

Setin gerçek tasarım işi. Ürettiği şey **ölçüm değil tasarım sabiti**
(`GUTTER_PT` ve `CURL_FACTOR` emsali) ve her biri adıyla, gerekçesiyle
yazılacak — "ölçülmemiş sayı yazılmaz" kuralının istisnası ölçüm iddiası
taşımayan sabittir.

**Türetme ya da yeniden kullanım olanlar (karar değil):** ince çizgi
kalınlığı = `underline_px.1` (depoda "çizgi kalınlığı"nın zaten bir cevabı
var), merkez = `cell_px`'in yarısı (fontun `strikeout`'u değil — x-height'a
bağlı ve iki `│` alt alta gelince kırılırdı), kesirli bölme = mevcut
`coverage`, çift çizgi aralığı = `RuleKind::Double`'ın bugünkü
`position + 2.0 * thickness`'ı, yay yarıçapı = `min(cx, cy)`.

**Gerçek sabitler — üç:**

1. **Kalın çizginin çarpanı.** `━` ince'nin kaç katı? İnce
   `underline_px.1`'den geliyor ama kalın için ikinci bir sayı gerek.
2. **Gölgelerin dama adımı** (`░▒▓`, U+2591–U+2593): desen piksel
   ızgarasına çivili (25/50/75%) ve adımı sabit — komşu hücrelerde faz
   tutması için mutlak olmalı, `WAVE_COUNT`'un döşeme kaygısının aynısı.
3. **Braille'in nokta geometrisi:** 2×4 ızgarada nokta yarıçapı ve kenar
   payı. İlk turda listede yoktu ve phase-3'te uydurulacaktı.

**Ve bir çatal — kesikli çizgilerin periyodu:**

`raster::dividing_period` (`raster.rs:305-316`) periyodu hücre genişliğini
**tam bölen** en yakın değere çekiyor ve gerekçesi yazılı (faz hücre
sınırında kırılmasın). Ama kesikli ailesi üç yoğunluk istiyor (`╌` ikili,
`┄` üçlü, `┈` dörtlü) ve ölçüldü: `w = 8`'de üçlü periyot 3 → **4**, yani
`┄` ile `╌` **aynı sprite'a çöküyor**; asal genişlikte (w = 29) her yoğunluk
29'a çıkıyor, yani hücre başına tek tire.

- **A. `dividing_period` korunur** — üç yoğunluk bazı puntolarda ayrışmaz.
  Dürüst ve döşeme garantili; bedeli görünür bir bilgi kaybı.
- **B. Kesikli için kısıt gevşetilir**, faz `curl` gibi piksel merkezinden
  örneklenir — üç yoğunluk her puntoda ayrışır, ama hücre sınırında desen
  kayabilir.

**Öneri: A.** Döşeme bu setin **varlık sebebi**; onu üç yoğunluğun
ayrışması için feda etmek işi kendi amacına çevirir. Kayıp adıyla
yazılacak. Seçim doğrudan sınanabilirliği de belirliyor (Karar 7): A ile
"416 sprite ikili olarak farklı" **yazılamaz**, çöken çiftler adıyla muaf
tutulur.

## Karar 5: Küçük sınıf (`SizeClass::Small`)

**A. Küçük sınıfta kapı kapalı** — kutu karakteri fonttan gelmeye devam
eder.

Gerekçe **döşeme değil ölçü ayrışması**: `Metrics` büyük hücrenin, yani
yordamsal sprite büyük hücre genişliğinde çizilir; bağlam satırının sütun
adımı ise küçük yüzün ilerlemesi (`Frame::column_px`). Komşu hücreler
örtüşürdü. Küçük glyph'lerin bugün örtüşmemesinin sebebi dar mürekkepleri —
hücreyi tam dolduran bir sprite o güvenceyi kaybeder.

*İlk turdaki gerekçe ("bağlam satırında kutu karakteri yapısal olarak
doğmuyor") **yanlıştı**: satır yol ve dal taşıyor, ikisi de kullanıcı
verisi ve bir dizin adı `─` içerebilir. Karar doğru kol, gerekçe düzeldi.*

**Mekanizma:** kapalı kapı `slot`'un çizim `match`'inde bir `size ==
Normal` guard'ı; normalizasyon kolunun deseni de `SizeClass::Normal` olarak
yazılacak, `_` değil — `_` yazılsaydı küçük istek `Normal`'e zorlanır ve
kapı tam da kapatılmak istenen yerde açılırdı.

## Karar 6: Phase bölünmesi — **iki** phase, bloklar önce

1. **Altyapı + bloklar + Braille.** Kapı, yüz normalizasyonu, üyelik
   yüklemi, `rect` primitifi ve değişmez sınama iskeleti burada doğuyor;
   üstüne blok elemanları (dikdörtgen doldurma, sekizde birlik dilimler,
   dama deseni) ve Braille (2×4 nokta). **Kullanıcının bildirdiği iki
   kusur da burada kapanıyor** — maskot bloklardan, spinner Braille'den.
2. **Çizgiler.** 128 karakterlik tablo, dört kol × {yok, ince, kalın,
   çift}, kesikli aile ve yuvarlak köşeler. Setin en çok karar taşıyan
   yarısı, iskelet oturduktan sonra.

**Braille kendi phase'ini hak etmiyor:** tablosu **sıfır satır** — kod
noktasının alt 8 biti doğrudan nokta maskesi (Unicode öyle tanımlıyor) ve
çizimi phase-1'in `rect` primitifinin sekiz küçük dikdörtgene uygulanması.

**Sıra bilerek ters çevrildi** (ilk tur "çizgiler önce" diyordu, gerekçesi
"en büyük phase"): bildirilen iki kusur en büyük işin arkasında
bekleyemez; altyapı 128 yerine 32 karakterle sınanır; ve ara durum daha az
tutarsız olur — btop/lazygit aynı karede çizgi ile bloğu yan yana basıyor,
*tutarlı* kusur "font biraz tuhaf" diye okunur, *tutarsız* kusur "şu
karakter bozuk" diye.

## Karar 7: Doğrulama — değişmezler, tablo değil

Panelin en sert itirazı ve kabul edildi: **arıza kipi bugünkünden kötüye
gidiyor.** Bugünkü kusur dürüst (şekil doğru, hücre dolmuyor → "terminal
biraz bozuk"); yanlış tablonun kusuru sessiz (`├` yerine `┤` çizilirse
görüntü kusursuz görünür ve kullanıcı htop'u suçlar). Ayar anahtarı da yok,
yani kaçış yolu yok — çıta yükseliyor.

Sprite başına el yazması sınama (bugünkü yedi kural için yazılan örüntü)
416'ya ölçeklenmez. Yerine **değişmez** sınanacak:

- **Braille:** `sprite(0x2800|mask)` == set bitlerin sprite'larının
  piksel-max'i; `0x2800` boş; sekiz noktanın desteği ayrık.
- **Bloklar:** `█` **bit bit 255** (bildirilen kusurun tam tersi — `> 0`
  değil eşitlik); `▀` + `▄` doygun toplamı 255; sekizde bir merdiveni
  monoton iç içe.
- **Çizgiler:** **dikiş sürekliliği** (sağ kolu olan karakterde `sütun w-1`
  profili == `sütun 0`; alt kolu olanda `satır h-1` == `satır 0`) ve
  **birleşim yasası** (`┌ ∪ ┘ == ┼` — ayrık kol kümeli iki karakterin
  piksel-max'i birleşim kümesinin sprite'ı).

**Oracle bağımsız olmalı:** kol kümesi tablosu sınamaya **ikinci kez ve
Unicode adlarından** yazılır. Uygulamanın tablosunu okuyan bir sınama
hiçbir şey kanıtlamaz.

**Değişmezler en az üç (punto, ölçek) çiftinde koşar** — depo bunu zaten
öğrendi (`envelope_stays_inside_cell`'in doc'u: *"gerçek fontla kırpma dalı
hiç ateşlenmiyor"*).

**Birleşim yasasının göremediği tek şey "doğru geometri, yanlış karakter"**
(aynalanmış tablo). Onu gören tek şey gözle kontrol ve phase
checklist'inde adıyla duracak: phase başına bir örnek sayfa ekran
görüntüsü, artı bildirilen iki uygulama (Claude Code'un maskotu ve
spinner'ı, `tree`/`htop`).

## Kapsam dışı

- **Emoji ve geniş glyph** — setin ayrılma gerekçesi: renkli bitmap ve
  "ikinci atlas mı, RGBA mı" çatalı bu sette hiç açılmıyor.
- **Köşegenler** (`╱╲╳`) — Karar 3B.
- **Geometrik şekiller** (U+25A0–U+25FF: `■▲●`) — kutu çizim değil, döşeme
  sorunu yok.
- **Symbols for Legacy Computing** (U+1FB00–U+1FBFF, sextant/octant) —
  kitty destekliyor, kimse istemedi.
- **Powerline glyph'leri** (U+E0B0–) — özel kullanım alanı, font işi.
- **Ayar anahtarı** ("kutu çizimi fonttan al") — 020'nin gerekçesinin
  aynısı: anahtar eklemek geri alınamaz ve emsal terminallerin hepsi
  koşulsuz çiziyor. Karşılığı Karar 7'nin yükselttiği doğrulama çıtası.
- **Yuva rezervi** — `RULE_RESERVE`'ün kutu ailesi için karşılığı **yok** ve
  bu bir asimetri: dolu atlasta fonta hiç ihtiyaç duymayan bir `─` yine
  `TOFU`'ya düşüyor. 416 karaktere pay ayırmak kapasitenin beşte birini
  bağlardı; tahliye (LRU) zaten 00X'in işi.

## Karar (2026-09-21, kullanıcı onayı)

Panelden geçmiş öneriler sunuldu, kullanıcı onayladı ("tamamdır phase
dosyalarına geç"). İki soru açıkça kullanıcıya bırakılmıştı ve ikisinde de
önerilen kol geçerli.

- **Karar 1 → A.** Kapı `Atlas::slot`'un `Sprite::Char` kolunda,
  `Sprite::Rule` kolunun ikizi; yedekten **önce**, `size == Normal`
  guard'ıyla.
  - *Reddedilen:* B (`raster::draw`'ın başı) — imza `Face`/`SizeClass`
    görmüyor ve yüklem iki yerde yaşardı.
  - *Reddedilen:* C (`Sprite::Box` varyantı) — renderer'a terminal
    semantiği sızdırırdı.
- **Karar 2 → yüz `Face::Regular`'a normalize.** Bedeli bir fikstür ve
  phase-1'in borcu: `face_fallback_is_cached_under_the_requested_face`'in
  `─`'si ölçülerek başka bir karakterle değişecek.
- **Karar 3 → yuvarlak köşeler içeride, köşegenler dışarıda.** Kullanıcıya
  sorulan iki sorudan biriydi; mesafe alanı köşegenleri de çizebilirdi ama
  kapsam kapalı tutuldu.
- **Karar 4 → üç tasarım sabiti** (kalın çarpanı, dama adımı, Braille
  geometrisi) ve **kesikli çatalında A**: `dividing_period` korunuyor,
  `┄`/`╌`'nin bazı puntolarda çökmesi kabul ediliyor ve adıyla yazılıyor.
  Kullanıcıya sorulan ikinci soruydu; gerekçe tek cümle — **döşeme bu setin
  varlık sebebi**, onu üç yoğunluğun ayrışması için feda etmek işi kendi
  amacına çevirir.
- **Karar 5 → küçük sınıfta kapı kapalı**, gerekçe ölçü ayrışması (sprite
  büyük hücre genişliğinde, sütun adımı küçük yüzün ilerlemesi).
- **Karar 6 → iki phase, bloklar + Braille önce.** Bildirilen iki kusur
  (maskot, spinner) ilk phase'te kapanıyor.
- **Karar 7 → doğrulama değişmezlerle**, oracle sınamaya ikinci kez ve
  Unicode adlarından yazılıyor, en az üç (punto, ölçek) çiftinde koşuyor;
  "doğru geometri, yanlış karakter"i yalnız gözle kontrol görüyor ve o
  phase checklist'inde adıyla duruyor.
- **Ayar anahtarı yok.** 020'nin gerekçesinin aynısı; karşılığı Karar 7'nin
  yükselttiği çıta.

## Muhakeme (2026-09-21)

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU |
| Codebase-fit | SORUNLU |
| İşletme | SORUNLU |

Üç jüri de yönü onayladı (yordamsal çizim doğru çare, `RuleKind` emsali
yerinde, katman yönü korunuyor, `bt-gpu`/`bt-core` gerçekten değişmiyor) ve
üçü de **kararların kendisine** itiraz etti. Kabul edilen itirazların hepsi
kodda doğrulandı.

**Kabul edilen itirazlar → karar değişikliği:**

- **Kapının yeri yanlıştı** (üç jüri de, ayrı ayrı kanıtladı) → Karar 1
  yeniden yazıldı. `raster::draw`'ın imzası `Face` de `SizeClass` de
  görmüyor (`raster.rs:38-44`), yani Karar 5 orada bedava değil; ve Karar
  2'nin yüklemi `slot`'a zaten giriyor, yani yüklem iki yerde yaşardı. İki
  kopyanın kayması sessiz ve pahalı: 416 yerine 1664 yuva.
- **Karar 2 mevcut bir sınamayı kırıyor** (Codebase-fit; doğrulandı) →
  Karar 2'ye fikstür borcu yazıldı.
  `face_fallback_is_cached_under_the_requested_face`'in fikstürü `─` ve
  normalizasyondan sonra taşıyıcı iddiası sessizce ölüyor.
- **`GATE_PROBES` içinde `⠋` var** (Codebase-fit; doğrulandı, `lib.rs:628`)
  → `the_gate_decides_by_width_alone` Braille'in phase'inde kırılır. Çare
  probu silmek değil, sınamanın `continue` kapısını kapının kendi
  yüklemiyle genişletmek; `wide > 0` bekçisi `𝔸 漢 🎉` ile ayakta kalıyor.
- **Karar 3'ün emsali yanlıştı** (Sadelik + Codebase-fit) → `curl` sütun
  başına tek bir `y` örnekliyor ve **sığ** eğride çalışıyor; çeyrek yayın
  dikey teğetinde bant kopardı. Doğru emsal `chevron`'un mesafe alanı — ve
  asıl kazanç: aynı primitif düz kolları da çiziyor, yani yay ayrı bir
  teknik değil.
- **Karar 4 şişkindi** (Sadelik) → dördün üçü türetme ya da yeniden
  kullanımdı; gerçek sabit üç (kalın çarpanı, dama adımı, Braille
  geometrisi) ve üçüncüsü ilk turda **eksikti** (İşletme).
- **`dividing_period` çatalı hiç sorulmamıştı** (İşletme; doğrulandı,
  `raster.rs:305-316`) → Karar 4'e girdi. `w = 8`'de `┄` ile `╌` aynı
  sprite'a çöküyor ve bu doğrudan Karar 7'nin yazılabilirliğini etkiliyor.
- **Phase sırası ters** (İşletme) ve **Braille kendi phase'ini hak
  etmiyor** (Sadelik) → Karar 6 iki phase'e indi, bloklar öne alındı. İki
  jüri bağımsız olarak aynı yere vardı.
- **Doğrulama kör ve arıza kipi kötüleşiyor** (İşletme) → Karar 7 eklendi.
  Bu setin en büyük borcu ve `make duman` burada kanıt değil: duman betiği
  donmuş (`session.rs` doc'u: *"ikinci bir yük buraya eklenmez"*),
  jetonlar birebir aynı kalacak.
- **Karar 5'in gerekçesi yanlıştı** (İşletme) → bağlam satırı yol ve dal
  taşıyor, ikisi de kullanıcı verisi; bir dizin adı `─` içerebilir. Karar
  doğru kol, gerekçe ölçü ayrışmasına çevrildi.
- **`RULE_RESERVE` asimetrisi ve belge sürüklenmesi** (Codebase-fit) →
  Kapsam dışı'na ve phase'lerin Yayın Etkisi'ne yazıldı.

**Reddedilenler:**

- **Yuva rezervi kutu ailesine de ayrılsın** — önerilmedi ama tartışıldı;
  416 karaktere pay kapasitenin beşte birini bağlar ve tahliye zaten
  00X'in işi. Asimetri adıyla yazılıyor, çözülmüyor.
- **Tablo boyutu sayısı plana yazılsın** (Sadelik ~145 satır dedi) — kod
  boyutu tahmini bir ölçüm değil ve phase'e bağlayıcı sayı olarak girmez;
  plan niteliksel yazıyor ("tablo yalnız çizgilerde").

**Panelin ölçtüğü ve plana kayıt olarak giren sayı:**

İşletme yuva ayak izini `Atlas::occupancy` ile ölçtü (bu makine, Menlo):
aile bugün **160** yuva harcıyor (Braille sıfır — genişlik kapısından
dönüp negatif önbellekte `TOFU`'ya bağlanıyor), setten sonra **416**
harcayacak. Kapasite puntoyla düşüyor: 13pt@2x'te 1984 yuva, 32pt@2x'te
**338** — yani en büyük puntoda ailenin kendisi kapasiteyi aşıyor ve
tahliye yok. Atlas doyması **bugün de mümkün** (büyük puntoda birkaç yüz
ASCII); set onu daha erken erişilebilir kılıyor. Bu bir kapı değil kayıt:
`## Yayın Etkisi` → *"ölçüm bekliyor: kutu ailesinin yuva ayak izi ve
büyük puntoda doyma eşiği"*. **Kare süresi iddiası yok** ve bu da
yazılacak — çizim yuva başına ömürde bir kez, kare başına yeni iş yok.
