# Yedek glyph'in küçültülmesi — Tartışma

## Karar 1: sığmayan yedek glyph'e ne olsun → ✅ küçültülsün

Çözüm için üç yol konuşuldu: sığmayanı küçültmek, `⧉`'i yordamsal çizmek,
olduğu gibi bırakmak. Küçültme seçildi. Tek kural bütün aileyi kapsıyor;
yordamsal çizim tek karakterlik bir çare olurdu ve bir sonraki sembolde
kusur yeniden çıkardı.

## Karar 2: tek sütunlu renkli emoji kapsamda mı → ✅ evet

Mürekkebi hücrenin ~1.66 katı olan 78 tek sütunlu emoji (023'ün sayımı)
küçültülünce normal harfin ~%60'ı boyunda görünüyor. Kullanıcı bunu kutuya
tercih etti. Üst sınır bu yüzden en az emojiyi kapsayacak genişlikte
seçilecek; kesin sayı taramanın dağılımından (Karar 4).

## Karar 3: kusurlu karakter önceden nasıl bilinsin → ✅ tarama + bekçi

- **Tarama:** Unicode'un sembol ve emoji aralıklarını atlasın kendi kapısından
  geçirip her karakteri dört gruptan birine koyan, `#[ignore]`'lu bir koşu
  ve onu çağıran `make` hedefi. Gruplar: fontta var / yedekten geldi, sığdı
  / hiçbir fontta yok / kapıdan döndü (taşma oranıyla). 13pt ve 16pt'de,
  @1x ve @2x ölçekte koşar. Kapıya girmiyor, çünkü sonucu sisteme kurulu
  fontlara bağlı.
- **Bekçi:** gerçek araçların bastığı karakterlerden kısa bir liste (Claude
  Code, spinner'lar, git, starship/p10k). Bu karakterler kutu çıkmamalı ve
  sınama `make hepsi`'de koşar. Kullanıcının ileride bulduğu her karakter
  bu listeye eklenir, yani aynı kusur ikinci kez sessizce geri gelmez.

## Karar 4: üst sınır nasıl seçilir → ✅ taramadan, phase-2'de

Sınır bir tasarım sabiti olacak (`GUTTER_PT` emsali) ama değeri önceden
yazılmıyor. Phase-1'in tablosundaki "kapıdan döndü" grubunun oran
dağılımına bakılarak seçilir. Tabloda aranacak şeyler:
- emoji kümesinin üst ucu (Karar 2 onu içeride istiyor),
- `.LastResort`'un oranı. `.LastResort` "hiçbir font yok" demek ve
  küçültülse bile yine bir kutu çizer. Oranı sınırın altına düşüyorsa kapı
  onu ayrıca tanımalı. Bugünkü sözleşme bunu geometriyle çözüyor ve
  `CLAUDE.md` aile adı karşılaştırmasını yasaklıyor. Bu durumda tanıma
  ölçütü phase-2'nin Uygulama Notları'nda gerekçesiyle yazılır.

## Karar 5: küçültme nasıl yapılır → ✅ adayın küçük puntolu kopyası (teknik karar)

Kabul edilen aday (`font::Accepted`) aynı fontun, mürekkebini hücreye
sığdıracak puntodaki kopyası olur. Punto `CTFontCreateCopyWithAttributes`
ile belirlenir, oran = hücre ilerlemesi / mürekkep genişliği. Bu yolla
çizim (`raster::draw_glyph` / `draw_color_glyph`), ortalama
(`centre_shift`) ve kapının kendisi değişmeden kalıyor. Kapı, küçük
kopyayı bugünkü `ink_fits_box` ile yeniden ölçüyor. Böylece "kapı, adayın
çizileceği yerdeki mürekkebi ölçer" kuralı korunuyor.

Reddedilen: çizimde CG dönüşümüyle ölçeklemek. Bu yol ölçeği `raster`'a
ikinci bir parametre olarak taşır, kapının ölçtüğü ile çizimin yerleştirdiği
arasında ikinci bir formül doğurur. `centre_shift`'in "tek formül, iki
tüketici" kuralını bozar.

Dikey yerleşim (taban çizgisinde mi kalsın, yoksa hücrede ortalansın mı)
küçük kopyada kendiliğinden taban çizgisine yakın kalıyor. ~0.9'luk
küçültmede fark görünmüyor, ~0.6'lık emojide görünür. Karar phase-2'de,
gözle kontrol ile verilir ve Uygulama Notları'na yazılır.

## Karar (2026-09-29, kullanıcı onayı)

- **Seçilen:** yedek glyph'in küçültülmesi (Karar 1), tek sütunlu emoji
  dahil (Karar 2), önce tarama ve bekçi (Karar 3), sınır taramadan
  (Karar 4). Karar 5 teknik bir karardır, ajana aittir.
- **Reddedilen:** `⧉` için yordamsal çizim, çünkü tek karakterlik bir
  çaredir. Durumu olduğu gibi bırakmak da reddedildi. Tek sütunlu emojiyi
  kutu bırakmak da reddedildi (kullanıcı küçültmeyi seçti).
