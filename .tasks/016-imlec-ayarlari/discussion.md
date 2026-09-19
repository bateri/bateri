# İmleç ayarları — Tartışma

Tek bariz yaklaşım yok: **beş ayrı karar** var ve dördü birbirine bağlı.
Biçim karar-listesi.

## Karar 1: Değerler hangi yoldan iniyor?

**Öneri: `cursor_motion` emsali — doğrudan `DisplayLink`'e, `bt-core`'a
uğramadan.**

Yarıçap, hale ve blink periyodu terminalin **durumu değil**: hiçbiri DECSCUSR
ile birleşmiyor, hiçbiri `frame()` sınırından geçmesi gereken bir karar
üretmiyor. `TerminalOptions`'a konsalardı `bt-core` taşıdığı ama hiç
kullanmadığı üç alan kazanırdı ve `Cursor` sınırı şişerdi — sınır hücresine
alan eklemenin ölçütü "kare başına maliyet" ve bu üç sayı kare başına
**değişmiyor**.

**Reddedilen:** `TerminalOptions`. Tek gerekçesi "`cursor` ve `cursor_blink`
orada" olurdu; o ikisi orada **çünkü uygulamanın isteğiyle birleşiyorlar**,
bunlar birleşmiyor.

## Karar 2: Anahtarlar hangi bölüme?

Bugünkü bölümler: `[terminal]` (`scrollback`, `cursor`, `cursor_blink`),
`[appearance]` (tema üçlüsü), `[font]`, `[clipboard]`, `[motion]`
(`cursor_motion`, `reduce_motion`), `[shell]`.

**Öneri: hepsi `[terminal]`'a, `cursor*` ailesinin yanına.**

Gerekçe okunabilirlik: kullanıcı imleçle ilgili her şeyi tek yerde arıyor.
`cursor_blink` `[terminal]`'da ve periyodu ondan ayrı bir bölüme koymak aynı
özelliğin iki yarısını ikiye bölerdi.

**Gerilim var ve yazılı olsun:** `cursor_motion` `[motion]`'da. Yani "imleçle
ilgili her şey `[terminal]`'da" **bugün de doğru değil**. Karşı öneri
(`[motion]`'a koymak) `cursor_blink_interval` için savunulabilir — bir ritim
sayısı — ama o zaman `cursor_blink` ile ayrı düşer.

**Üçüncü yol (reddedildi):** yeni bir `[cursor]` bölümü açıp hepsini oraya
toplamak. Temiz görünüyor ama `cursor` ve `cursor_blink`'i taşımak gerekirdi
ve **anahtar silinmez** — ikisi `[terminal]`'da emekli olarak kalır, yani
kullanıcı aynı ayarı iki yerde görürdü. Kazancından çok karışıklık.

## Karar 3: Birim — oran mı piksel mi?

**Öneri: oran, bugünkü türetmelerin aynısı.**

Yarıçap hücre yüksekliğinin, hale sol payın oranı. Punto büyüyünce ikisi de
büyüyor ve bu 015'in yazılı kuralı ("ikinci bir tasarım sabiti yok").
Piksel verilseydi kullanıcı Cmd +/− yaptığında imleç orantısız kalırdı ve
`docs/OLCUMLER.md` konusu olmayan bir sayı aygıt pikseline bağlanırdı.

**Alfa istisna:** `0..1`, oran değil mutlak — neyin oranı olacağı belli değil.

## Karar 4: Kabul edilmeyen değer ne olsun?

**Öneri: deponun mevcut kuralı — kendi anahtarını değiştirmez ve tanı
bırakır** (`Settings::parse_keeping`). Kırpma (clamp) **yok**: `1.5` yazan
kullanıcı sessizce `1.0` almamalı, yanlış yazdığını görmeli.

Aralıklar: yarıçap `0.0..=0.5` (yarım = hücrenin yarısı, ötesi anlamsız),
hale payı `0.0..=2.0`, alfa `0.0..=1.0`.

## Karar 5: Blink periyodunun alt sınırı ne?

**Bu setin tek gerçek yeni riski.** Periyot ucuz ama sınırsız değil: 10 ms
yazan biri saniyede 100 kare ister ve pil yanar.

**Öneri: `0.05..=5.0` saniye**, `const` doc'unda "seçilmiş, ölçülmemiş" ve
gerekçesiyle. Alt sınırın gerekçesi ekran hızı: 50 ms yarım periyot saniyede
20 kare eder, 60 Hz'in üçte biri — hâlâ "boşta sıfır kare"nin ihlali değil
ama tavanı orada durduruyor.

**İkinci soru: `IDLE_STOP` (15 s) de açılsın mı?** Öneri **evet**, aynı
tesisat ve aynı aile; kitty'nin `cursor_stop_blinking_after`'ı emsal. Sıfır
"hiç durma" demek olmalı ve o **bilinçli** bir seçim: pencere odaktayken
süresiz blink, kullanıcının açıkça istediği hâl.

## Kapsam dışı

- **Blink'e opacity / fade** — kullanıcı bedeli duyunca çıkardı
  (`context.md` → Kapsamdan çıkarılan).
- **Kenar kalınlığı** (içi boş caret) — `rule_px`'ten geliyor ve fontun kendi
  metriği; ayara açmak "ikinci tasarım sabiti yok" kuralını deler.
- **`cursor_motion`'ın yeni stilleri** — 008'den beri ayrı.
- **Tema başına imleç görünüşü** — tema dosyası renk taşıyor, geometri değil.


## Muhakeme (2026-09-19)

Üç mercek de **SORUNLU** verdi; üçü de bağımsız olarak aynı iki itirazı
getirdi. Kabul edilenler aşağıda, kapsam **daralarak** çıktı: beş anahtar → üç.

### Kabul — `cursor_blink_stop` (`IDLE_STOP`) kapsam dışı

**Üç mercek de ayrı ayrı söyledi.** Gerekçeler birikimli:

- **İstenmedi ve türü farklı.** Kullanıcının cümlesi "köşe ve shadow"; durma
  sayacı ne köşe ne gölge, üstelik **zevk sayısı değil politika**. Setin bütün
  gerekçesi olan "gözle ayar için derleme döngüsü yanlış araç" sürtünmesi o
  sabitte hiç yaşanmadı — gözle ayarlamak için 15 saniye kıpırdamadan oturmak
  gerekir.
- **`= 0` yazılı bir değişmezi silerdi.** `blink.rs`'in kendi doc'u: *"Bu sabit
  blink'i bu deponun merkezî vaadiyle barıştıran şey: onsuz, açık bir blink
  pencereyi kalıcı olarak boşta-değil yapardı."* `CLAUDE.md` de aynısını
  söylüyor. İzin vermek **anahtar değil sözleşme** düzenlemesi olurdu.
- **Kapı göremez.** Süreli koşu ayar dosyasını hiç okumuyor, yani ihlal
  `make duman`'a görünmez — sessiz erozyon.
- **Çapraz anahtar sessizliği.** `idle_stop < period` olan her ikili caret'i
  hiç söndürmez; iki anahtar da tek başına geçerli, çapraz kural yok.
- kitty emsali **taşınmıyor**: kitty boşta sıfır kare sözü vermiyor.

Pair düşünce `cursor_blink_interval`'in tek başına maliyeti **çok azaldı**:
durma koşulu yerinde kalıyor, çapraz kural doğmuyor, `CLAUDE.md`'nin
"sessizlikten sonra durur" cümlesi doğru kalıyor. Geriye tek risk kalıyor ve
adı konuyor: **kapı bozuk bir periyodu göremez** (014'ün defteri bunu zaten
yazmıştı), tek koruma alt sınır.

### Kabul — hale payı ve alfası **tek çarpana** iniyor

Kanıt setin kendi tarihinde: pay 1.0 → 0.5 → 0.4, alfa 0.35 → 0.14 → 0.10.
İkisi de **aynı iki göz turunda, aynı yönde** indi; kullanıcı iki eksende
değil **tek histe** gezindi. İki ayrı anahtar ayrıca anlamsız hâl üretiyor:
`pay = 2.0, alfa = 0` hiçbir şeyin 16 pikselik halesini boyayan bir dörtlü
verir ("kapalı"nın iki yazılışı olması tek başına koku).

`cursor_glow` bir **çarpan**: `pay = gutter_px * 0.4 * glow`,
`alfa = 0.10 * glow`. Bugünkü hâl `1.0`, kapalı `0.0`. Bugünkü iki sabit
**taban olarak yerinde kalıyor**, yani "ikinci bir tasarım sabiti yok" kuralı
korunuyor.

### Kabul — varsayılanın **tek sahibi** olacak, yoksa bekçi kör kalır

Bugün `renderer.rs`'in iki piksel bekçisi örnekleme noktasını üretimdeki
orandan hesaplıyor ve yorumu bunu şart koşuyor. Oran ayara açılınca
`Frame::default()` yine bir değer taşımak zorunda, ayrıştırıcının varsayılanı
ise `bt-core`'da doğar — **iki literal**, hiçbir sınama ikisini bağlamıyor ve
sevk edilen hale başka ölçüde olsa da bekçi yeşil geçer.

**Çare:** varsayılanlar `bt-core`'da `pub const` olarak doğar; `Settings`'in
`Default`'u onları okur, `bt-gpu` **aynı const'ları import eder**
(`Frame::default()` ve iki bekçi). Tek literal, herkes ondan okuyor.

*Kayda geçsin:* `CursorMotion`'ın doc'u *"sayıların ayar modelinde durması
onları iki yerden değiştirilebilir kılardı"* diyor ve bu set o kuralı
**bilerek** deliyor — kullanıcı sayıların kendisini istedi. Delik tek yerde ve
gerekçesi burada.

### Kabul — dört tesisat şartı plana yazılıyor

1. **`CellMetrics` taşıyıcı değil.** 32 çağrı yeri var ve `GUTTER_PT`'nin
   doc'u zaten "ayar değil sabit" diyor; font geometrisiyle zevk sayısı aynı
   tipe binemez.
2. **Değer `LinkIvars`'ta `Cell<_>` olarak yaşıyor** ve içerik karesinde
   `clear`'ın ikinci argümanı olarak `Frame`'e giriyor — emsal `cell`,
   `motion`, `blink`. Hareket karesi `clear` çağırmıyor, yani `Frame` değeri
   koruyor.
3. **Her setter değişimde kare istiyor.** Yazılmasaydı boştaki pencerede
   kaydedilen yarıçap ekrana hiç düşmezdi; blink periyodunda daha keskin,
   çünkü kurulmuş `after` iptal edilemiyor.
4. **Varsayılanların sahipliği göç ediyor** (yukarıda).

### Kabul — `ranged_float` **ismen** plana yazılıyor

Aralıklı ondalık ayrıştırıcı depoda **iki kez elle** yazılmış (`line_height`,
`font_size`; ~35'er satır, kendi hata metinleriyle). Plan tek bir yardımcıyı
adlandırmazsa phase "emsali izle" der ve emsal kopyanın kendisidir — üç kopya
daha, ~100 satır. Mevcut ikisinin o yardımcıya taşınıp taşınmayacağı ayrı bir
karar; taşınmazsa depo aynı işin iki yolunu **bilerek** taşır.

### Kabul — kısmi iniş sessiz, phase kuralı buna göre

Şablon satırı ayrıştırıcıdan önce inerse kullanıcı anahtarı yazar ve **hiçbir
şey olmaz, tanı da çıkmaz**; `Changes` alanı unutulursa ayar yalnız sonraki
açılışta uygulanır. İkisi de `make hepsi`'yi yeşil bırakır. **Phase kuralı:
anahtar başına tek commit** ve o commit altı yeri birden taşır (ayrıştırıcı,
`Changes` alanı, açılış tohumu, kayıt anı, `TEMPLATE`, `docs/AYARLAR.md`).

### Kabul — `caret_sdf_override`'ın gerekçesi düşüyor

Doc'u test-only varlığını *"üretimde bir kurucusu olsaydı ölü kod olurdu"*
diye savunuyor. Üretimde setter doğunca bu gerekçe kalkıyor: yarıçap ve hale
sınamaları **gerçek yoldan** sürülecek, ezme yalnız **kenar** için kalacak.

### Red — bölümü `[motion]`'a taşımak

Codebase-fit haklı bir çentik buldu: `CaretShape`'in doc'u `[terminal]`'ı
*"dosya kodu aynalıyor — değer `TerminalOptions` ile `Session`'a iniyor"* diye
savunuyor ve **bu üç anahtar `TerminalOptions`'a girmiyor**, yani o gerekçe
onlar için kullanılamaz.

Ama bu bir **ihlal değil**: ayna zaten tek yönlü bile değil — `osc52`
`[clipboard]`'da olduğu hâlde `TerminalOptions`'a giriyor. Yani ayna
**betimleyici**, buyurucu değil.

`[motion]`'a taşımak da çözmüyor: `cursor_radius` ve `cursor_glow` hareket
değil, **duran görünüş**. Üçünü bölmek (görünüş → `[appearance]`, ritim →
`[motion]`) imleç ayarlarını **üç bölüme** dağıtırdı.

**Karar `[terminal]`, gerekçesi değişiyor:** bölüm kullanıcının **neyi
ayarladığını** adlandırıyor, hangi struct'ın taşıdığını değil. `CaretShape`'in
doc'una bu cümle ekleniyor, ayna betimleyici olarak işaretleniyor.


## Karar (2026-09-19, kullanıcı onayı)

- **Üç anahtar, `[terminal]`'da:** `cursor_radius`, `cursor_glow`,
  `cursor_blink_interval`. Panelin daralttığı hâliyle onaylandı.
- **`cursor_glow` bir çarpan**, iki sabit taban olarak yerinde kalıyor.
- **`cursor_blink_stop` (`IDLE_STOP`) kapsam dışı** — üç merceğin de istediği.
- **Varsayılanın tek sahibi `bt-core`'da `pub const`**; `bt-gpu` import ediyor.
- **Akış `cursor_motion` emsali:** `TerminalOptions`'a girmiyor, `Session`
  görmüyor.
- **Bölüm `[terminal]`**, gerekçesi değişerek: bölüm kullanıcının neyi
  ayarladığını adlandırıyor, hangi struct'ın taşıdığını değil.
- **Reddedilenler:** `[motion]`'a taşımak (yarıçap ve gölge hareket değil),
  üç bölüme bölmek (imleç ayarları dağılırdı), `IDLE_STOP`'u açmak.


## Karar 6: odaksız imlecin görünüşü (2026-09-20, kullanıcı)

**Karar: ayara açılıyor** — `[terminal] cursor_unfocused`, varsayılan
`"hollow"` (bugünkü davranış), alternatifi `"solid"`.

**Nasıl geldi:** kullanıcı odak kaybında içi boşalan imleci günlük kullanımda
yadırgadı ve "böyle mi olması lazımdı anlamadım" diye sordu. Soru kusur raporu
diye okunup bir tanı turu koşuldu; **ölçüm davranışın doğru ve anında
olduğunu** gösterdi (her `apply_focus`'u hemen bir içerik karesi izliyor, kapı
hep açık). Yani mesele kusur değil **zevk**.

Üç seçenek sunuldu ve kullanıcı üçüncüyü seçti:

1. Kalsın — macOS terminallerinin ortak davranışı.
2. Kalksın — tek satır, kod azalır.
3. **Ayar olsun** — tesisat zaten phase-1'de kuruldu, bir anahtar daha.

**Gerekçe:** set tam da imlecin görünüşünü kullanıcıya vermek için açıldı;
"beğenmezsem kapatırım" demek onun işi, bizim değil.

**Yan kazanç, adıyla:** tanı turu gerçek ama **latent** bir kusur buldu —
hareket karesi odağın bayat kopyasını taşıyordu (`ddf6c4f`). Kullanıcının
gördüğü şey o değildi ve bu kayda geçsin: düzeltme doğru, teşhis yanlıştı.
