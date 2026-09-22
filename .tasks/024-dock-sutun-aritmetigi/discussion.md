# Dock sütun saysın — Tartışma

Karar-listesi biçimi. **Yaklaşım çatalı yok** ve bu bir eksiklik değil: kök
tek (indeks sütun sanılıyor), çaresi tek (sütunu genişlikten biriktir). Açık
olan şey **ürün kenarları** — pencere kenarına denk gelen geniş glyph, geniş
hücrede caret, ve grapheme dizilerinin akıbeti.

Aşağıdaki her kararda **boşlukta kullanıcı tarafı seçildi**
(`.claude/skills/rfc` → Bulguyu işleme yolu). 023'ün dersi tam bu: kod kısıtı
gerekçe değil, seçenektir.

## Karar 1: Genişliği kim söyler? → `unicode-width`, ızgaranın crate'i

Sütunu biriktirmek için karakter başına bir genişlik gerekiyor ve `bt-core`
bugün onu hiç okumuyor.

**1A — `unicode-width`'i `bt-core`'un listesine almak.**
- **Artı:** ızgaranın kullandığı **tam olarak aynı** crate ve aynı sürüm
  (alacritty `unicode_width::UnicodeWidthChar` kullanıyor, kilitli 0.2.2).
  Yani dock ile ızgara **tanım gereği** aynı cevabı veriyor.
- **Artı:** grafta zaten var, yani yeni bir bağımlılık **ağacı** doğmuyor —
  `bt-core`'un listesine tek bir kenar ekliyor. `polling`'in 009'da geçtiği
  kapının aynısı (`CLAUDE.md`: "`polling` yeni bir crate değil, `libc` gibi").
- **Eksi:** `CLAUDE.md` bağımlılığı mimari karar sayıyor, yani kayda geçmesi
  gerekiyor — bu dosya o kayıt.

**1B — alacritty'den yeniden ihraç beklemek.** Ölçüldü: alacritty
`UnicodeWidthChar`'ı **içeride** kullanıyor ve ihraç etmiyor. Kol kapalı.

**1C — kendi genişlik tablomuzu yazmak.** İkinci bir genişlik yetkilisi
demek ve ızgaranınkiyle **ayrışacağı gün** belirtisi sessiz: dock bir sütun
kayar. 023'te tam bu riski adıyla yazıp `bt-atlas`'a `unicode-width` sokmayı
reddetmiştim; aynı gerekçe burada **ters yöne** çalışıyor — yetki `bt-core`'da
olmalı ve ızgaranın kaynağıyla aynı olmalı.

→ **1A.** Tek tutarlı cevap ve reddedilmesi ızgara ile dock'un ayrışmasını
kabul etmek olurdu.

## Karar 2: Pencere kenarına denk gelen geniş glyph → asla yarılanmaz

Yatay kaydırma (`skip`) caret sağ kenarı geçince görüntüyü soldan kaydırıyor.
Sütun sayınca yeni bir kenar durumu doğuyor: pencerede **bir** sütun kaldı ve
sıradaki karakter iki sütun istiyor.

- **Yarısını çiz** — sağ yarı kırpılır. **Reddedildi:** 023'ün sözleşmesi
  "kutu ya da tam glyph" ve yarım glyph **sessiz** bir bozulma.
- **Hiç çizme, o sütunu boş bırak** → seçildi. Kullanıcı bir hücre boşluk
  görür ve glyph bir sonraki kaydırmada tam gelir. Aynı kural sol kenarda da:
  `skip` bir geniş karakterin **ortasına** düşerse o karakter atlanır, yarısı
  gösterilmez.

Ölçüt kullanıcı tarafı: boşluk **görünür** bir eksiklik, yarım glyph sessiz
bir bozulma.

## Karar 3: Geniş hücrede caret → ızgarayla parite, tek hücre

Izgarada blok caret bir CJK karakterinin **sol** hücresini kaplıyor (023 bunu
değiştirmedi; `caret_rect` tek hücre veriyor). Dock'ta caret'i iki hücreye
yaymak dock'u ızgaradan **farklı** yapardı ve aynı karakterin iki yüzeyde iki
görüntüsü tam da bu setin kapatmaya geldiği şey.

→ Tek hücre, ızgarayla aynı. İkisini birden genişletmek ayrı bir iş ve
`caret_rect`'in tek yerinden geçer (`bt_gpu::frame`), yani ucuz — ama dock'a
özel yapmak yanlış.

**İmlecin hareketi ayrı bir şey ve o karakter başına:** bir sağ ok = bir
karakter, yani imleç geniş bir glyph'in üstünden tek hamlede geçiyor ve bir
Backspace onun tamamını siliyor. Bu zaten böyle — hareketi ZLE yapıyor ve
`CURSOR` karakter indeksi; bu setin değiştirdiği şey o indeksin **hangi
sütuna** düştüğü. İkisini karıştırmamak önemli: "iki hücre" çizimin birimi,
"tek karakter" düzenlemenin birimi.

## Karar 4: Tazelik kapısının iki tarafı aynı birimi okusun

Aynanın `last_ink`'i son boşluk olmayan `char`'ı alıyor, ızgara tarafı
hücrenin `c`'sini. Birleştirici kod noktaları (`VS16`, ZWJ, ten rengi)
hücreye **girmiyor** (alacritty `CellExtra`), yani ayna tarafı ızgaranın hiç
görmediği bir karakteri söylüyor ve kapı kalıcı olarak "bayat" diyor.

→ Ayna tarafı **sıfır genişlikli** kod noktalarını atlıyor. Ölçüt yine
`unicode-width` (genişlik 0), yani Karar 1'in ikinci tüketicisi — ve bu iki
tüketicinin aynı kaynaktan beslenmesi şart, yoksa kapı ile çizim ayrışır.

Kapının yanlış yönü **korunuyor**: şüpheli hâl bastırmayı bırakıyor, yani en
kötü ihtimalle satır iki yerde görünür.

## Karar 5: Grapheme dizileri → kapsam dışı, ama atlama düzeliyor

`❤️` (U+2764 + VS16) ve ZWJ aileleri **bu sette de** taban karakteriyle
çiziliyor: atlasın anahtarı `Sprite::Char(char)` ve bir diziyi ifade edemiyor
(023 Karar 1, kapsam dışı).

Ayrım net olsun diye:

| girdi | bu setten sonra |
|---|---|
| `🎉 📁 🚀` (tek kod noktası, 2 sütun) | dock'ta **renkli, iki hücre** |
| `漢字 ｆｕｌｌ` (CJK, fullwidth) | dock'ta **iki hücre**, caret doğru sütunda |
| `❤️` (VS16) | satır **fırlamıyor**; `❤` tek hücre, renksiz (Menlo'nun kendi glyph'i — ölçüldü) |
| `👨‍👩‍👧` (ZWJ) | satır fırlamıyor; taban `👨` çiziliyor, kalanı görünmüyor |

Son iki satır ızgarada **bugün de** öyle görünüyor, yani set iki yüzeyi
eşitliyor — kusuru kapatmıyor. Kapatan şey grapheme seti ve onun ön koşulu
atlas anahtarının `&str` olması.

## Karar 6: Vurgu geniş karakterde iki hücreye yayılır

`region_highlight` zsh'in komut satırını boyaması (sözdizimi renkleri, seçili
aralık) ve aralıkları **karakter** indeksinde veriyor — `style_at` bu yüzden
indeksle aranmaya devam ediyor, doğru olan o. Ama boyanan **zemin** hücre
başına çiziliyor, yani iki hücrelik bir karakterde bugün yalnız sol hücre
boyanıyor: `"fix 🎉"` dizgisinin sarı zemini emojinin sağ yarısında bitiyor.

→ **Yayılıyor.** Baş hücrenin `wide`'ı zemini de ikinci sütuna taşıyor
(spacer sütununa da bir arka plan hücresi düşüyor) ve bu, ızgaranın
`WIDE_CHAR_SPACER`'a zemin vermesinin aynısı — o kolun gerekçesi `frame()`'de
yazılı ("hücreyi tümden elemek onun sağ yarısını renksiz bırakırdı").

Bedeli hücre başına bir arka plan kaydı. **Kullanıcıya sorulmadı ve
sorulmamalıydı:** yarım boyanmış bir vurgu bariz bir kusur ve iki yüzeyi
eşitlemek bu setin varlık sebebi — `/rfc` → Bulguyu işleme yolu ("boşlukta
kullanıcı tarafı"). İlk yazımı bunu bir karar noktası olarak kullanıcıya
sormuştu; kullanıcı ne olduğunu anlamadı ve haklıydı.

## Karar Noktaları

**Kullanıcıya gidecek soru yok.** Altı kararın tamamı ya ölçümle ya
"boşlukta kullanıcı tarafı" varsayılanıyla kapandı. Bu bölüm bilerek boş
bırakılmıyor, **yok**: 023'ün dersi, kullanıcının değerlendiremeyeceği bir
teknik ayrımı ona sormanın karar üretmediğini gösterdi.

## Muhakeme

**Panel koşmadı.** `/rfc` adım 6'nın koşulu iki şartın **birlikte**
sağlanması: `discussion.md`'de birden çok yaklaşım **ve** pahalı sınıfa
dokunan bir seçim. İkincisi var (yeni crate bağımlılığı) ama **birincisi
yok** — Karar 1'in üç kolundan ikisi ölçümle kapandı (1B: ihraç yok; 1C:
ikinci genişlik yetkilisi) ve kalan tek kol. Panelin sınayacağı bir çatal
olmadığı için üç `opus` ajanı açmak gürültü olurdu.

**Kapalı kolun kaydı yine burada:** 1C reddedildi ve gerekçesi 023'ün kendi
kuralının tersine çevrilmiş hâli — orada `bt-atlas`'a genişlik sokmayı
reddetmiştim, burada `bt-core`'a sokmak **zorunlu**, çünkü yetkinin yeri
ızgarayla aynı katman.

## Karar (2026-09-22, kullanıcı onayı — "boşlukta hep kullanıcı tarafını seç")

Kullanıcı bu sette bir yetki verdi ve kararların tamamı ondan türüyor:
*"talebin kendisini düşünürken de eğer boşluk varsa hep kullanıcı tarafını
seçmen lazım."* Yani aşağıdaki altı kararın hiçbiri tek tek onaylanmadı;
onaylanan şey **varsayılanın yönü**.

- **Seçilen — K1: `unicode-width` `bt-core`'a giriyor.** Izgaranın kullandığı
  crate'in aynısı ve aynı sürümü. **Reddedilen:** alacritty'den ihraç beklemek
  (ölçüldü, ihraç etmiyor) ve kendi tablomuz (ikinci genişlik yetkilisi).
- **Seçilen — K2: pencere kenarında geniş glyph yarılanmıyor**, o sütun boş
  kalıyor. **Reddedilen:** yarısını çizmek — 023'ün "kutu ya da tam glyph"
  sözleşmesi ve yarım glyph sessiz bir bozulma.
- **Seçilen — K3: caret tek hücre, ızgarayla parite.** İmlecin **hareketi**
  karakter başına ve o değişmiyor.
- **Seçilen — K4: tazelik kapısının ayna tarafı sıfır genişlikli kod
  noktalarını atlıyor**, yani iki taraf aynı birimi okuyor.
- **Seçilen — K5: grapheme dizileri kapsam dışı**, ama atlama onlarda da
  düzeliyor.
- **Seçilen — K6: vurgu geniş karakterde iki hücreye yayılıyor.**

**Bir kararın süreç kaydı var:** K6 ilk yazımda kullanıcıya soru olarak
sunuldu ve kullanıcı ne olduğunu anlamadı — haklıydı, `region_highlight`
ürün diliyle sorulabilen bir şey değil ve cevabı bariz. Aynı hata bu oturumda
ikinci kez oldu (ilki 023'ün dört karar noktası) ve ikisinin de kaydı
`urun-kararlari-ux-once` hafızasında.

**023'ün bir değişmezi bu sette kalkıyor.** `the_dock_never_marks_a_cell_wide`
bir commit önce yazıldı ve gerekçesi sağlamdı *o aritmetikle*; aritmetik
değişince değişmez de kalkıyor ve yerine tersi geliyor. Bu, "yapısal olarak
zorunda" ifadesinin neden denetlenmesi gerektiğinin canlı örneği: kısıt
gerçekten zorunlu olsaydı bir sonraki sette kaldırılamazdı.
