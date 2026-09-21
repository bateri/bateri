# Atlas tahliyesi — Tartışma

Soru tek: **atlas dolduğunda ne olacak?** Bugünkü cevap "o oturumda artık yeni
karakter yok" ve savunulabilir bir cevap değil. Aşağıdaki üç yol da bunu
düzeltiyor; ayrıldıkları yer **neyi** düzelttikleri.

Ortak zemin — ölçülmüş kapasite (`docs/OLCUMLER.md` → Atlas yuva ayak izi;
1024 sütunu ölçüm, 2048 ve 4096 aynı hücre ölçüsünden aritmetik):

| punto @2x | 1024² | 2048² | 4096² |
|---|---|---|---|
| 13 (varsayılan) | 1984 | 7936 | 31744 |
| 29 (doyma eşiği) | **406** | 1624 | 6552 |
| 56 | **105** | 420 | 1680 |

Yordamsal aile + tofu = **422 yuva**. Doku maliyeti `R8Unorm`, yani
1 MB / 4 MB / 16 MB.

## Seçenek A: LRU tahliye

Yuva başına son kullanım damgası; dolunca en eski yuva geri alınıp yeni
anahtara veriliyor.

**Artıları:**
- Borcun yol haritasında **adıyla yazılı** sahibi.
- Bellek tavanı sabit: 1 MB, punto ne olursa olsun.
- Çalışma kümesi kapasiteye sığdığı sürece **her** senaryoyu çözüyor —
  büyük punto da, çok glyph'li içerik de.

**Eksileri:**
- **Kare ortasında yanlış glyph çiziyor — ve bu `replaceRegion` değil.**
  (Panel düzeltti; ilk yazımda tehlike yanlış yere bağlanmıştı.) `slot_uv`
  uv'yi **çözüm anında pişiriyor** (`renderer.rs:1264`) ve `instances`'a
  yazıyor; `encode_pass` ise `encode_glyphs`'i **dört kez** çağırıyor
  (`renderer.rs:668`, `700`, `761`, `855`) ve her biri `prepare`'a giriyor
  (`:1015`). Yani dördüncü çağrıda yapılan bir tahliye, ilk üçünün **zaten
  encode edilmiş** uv'lerini geriye dönük geçersizleştirir: ızgaradaki `A`,
  dock'un o yuvaya yüklediği bitmap'le çizilir. Saf CPU yolu — staging
  tamponu + blit encoder bunu **çözmez**. Kaçınmanın tek yolu "bu karede
  kullanılan yuva" pini ve o pin dört geçişi birden kapsamak zorunda, yani
  `bt-atlas`'a bugün hiç olmayan bir **kare** kavramı girer.
- Bozulmanın **tanığı yok**: `hucre=`/`glif=`/`kural=` kıpırdamaz, `yuva=`
  thrash'te de sağlıklı dolulukla aynı oranı basar.
- `replaceRegion` sınırı (`prepare`'ın doc'u) ayrıca duruyor ama **ikincil**.
- **Thrash uçurumu.** Yuva numarası kare verisinde saklanmıyor, her karede
  `resolve()` ile yeniden çözülüyor. Çalışma kümesi kapasiteyi aşarsa her kare
  tahliye edip yeniden **CoreText ile rasterize** eder. 29pt'de ekranda 406'dan
  fazla farklı glyph olması zor değil; o kolda kutu yerine donan bir pencere
  alırız — daha kötü bir kusur.
- Tahliye edilemeyecek yuvaları da bilmek gerekiyor (bu karede kullanılanlar,
  `RULE_RESERVE`, `TOFU`), yani kapı listesi büyüyor.

## Seçenek B: Dokuyu hücre ölçüsüne göre büyüt

`TEXTURE_EDGE` sabit 1024 olmaktan çıkıyor; `ensure()` ızgarayı kurarken
kapasite bir hedefin (ör. 1024 yuva) altına düşüyorsa kenarı ikiye katlıyor.
Küçük puntoda bugünkü hâl aynen korunuyor.

**Artıları:**
- **Kusurun ölçülmüş sebebini doğrudan vuruyor:** kırılma "çok karakter
  gördük"ten değil **büyük hücre az yuva veriyor**dan geliyor. 29pt'de
  1624 yuva, yani ailenin payı %26 — bugünkü 13pt'nin (%21) rahatlığı.
- `replaceRegion` sınırına **dokunmuyor**: hâlâ yalnız hiç görülmemiş yuva
  yazılıyor, canlı yuvanın üstüne yazılmıyor. Yeni tehlike doğmuyor.
- Thrash yok, kare başına yeni defter yok, `bt-gpu` hiç değişmiyor.
- Bellek **ihtiyaca göre**: 13pt'de 1 MB (bugünkü), 29pt'de 4 MB. Büyük punto
  zaten ekrana az satır sığdıran bir kip.
- `TEXTURE_EDGE`'in kendi doc'u onu "ölçüm iddiası değil, bir **kapasite
  tercihi**" diye tanımlıyor — yani bu yol kodun açık bıraktığı kapı.

**Eksileri:**
- **Uçurumu kaldırmıyor, taşıyor.** 56pt'de 2048² 420 yuva veriyor ve aile
  422 istiyor — kıl payı yetmiyor, yani zoom tavanına (72pt) kadar güvence
  için 4096² (16 MB) gerekiyor.
- Sekme/bölme (024) gelince doku **sekme başına** çarpılır; bugünkü tasarımda
  `Atlas` `Renderer`'ın alanı.
- Kapasiteyi aşan çalışma kümesi hâlâ çözümsüz (13pt'de ~2000 farklı glyph:
  ağır CJK). Bu senaryo **ölçülmedi**.

## Seçenek C: Dolunca sıfırla

`next >= cap` kolunda tofu dönmek yerine haritayı boşaltıp `next = 1` yapmak;
yuvalar talep geldikçe yeniden doluyor.

**Artıları:**
- En küçük değişiklik; tek kolda birkaç satır.
- Oturum kendini toparlıyor, kalıcı kutu bitiyor.
- Bellek sabit.

**Eksileri:**
- **Kitlesel yeniden yazma**, yani `replaceRegion` sınırını A'dan da sert
  vuruyor: tek karede ekranın tamamı yeniden yükleniyor.
- Çalışma kümesi kapasiteyi aşarsa **her kare** sıfırlanır — A'nın thrash'i
  ama tahliye seçiciliği olmadan.
- Kullanıcıya görünen bir "silkinme" üretiyor ve gerekçesi yok.

## Karar Noktaları

1. **Hangi kusuru çözüyoruz?** Ölçülmüş olan "büyük punto → az yuva"; ölçülmemiş
   olan "çok glyph → dolan atlas". B birincisini kapatıyor ve ikincisini
   açık bırakıyor; A ikisini birden hedefliyor ama karşılığında `bt-gpu`'nun
   yükleme yolunu ve bir thrash riskini getiriyor. Depo kuralı
   ("ölçülmemiş bir kazanç için önbellek eklenmiyor") B'yi işaret ediyor —
   ama burada eklenecek şey bir önbellek değil bir **tahliye**, yani kuralın
   harfi tam oturmuyor.
2. **`replaceRegion` düzeltmesi bu sete mi giriyor?** A ve C onu ön koşul
   yapıyor, B yapmıyor. Girerse set `bt-atlas` işi olmaktan çıkıp `bt-gpu`
   işi de oluyor.
3. **Bellek bütçesi var mı?** Bugün yazılı bir tavan yok
   (`docs/OLCUMLER.md` → `## Bellek` boş). 024 sekme/bölme dokuyu çarpacak.
   Ölçülmemiş bir bütçeye karşı 4–16 MB'ı savunmak zor; ama "sekme başına"
   olup olmayacağı da bu setin kararı değil.
4. **Yol haritasındaki ad.** Borç "tahliye (LRU)" diye yazılı. B seçilirse
   borç kapanmıyor, **daralıyor** ve madde o hâliyle yeniden yazılmalı —
   isim uğruna daha pahalı yolu seçmek yanlış olur.
5. **Kullanıcıya görünürlük.** Hiçbir seçenek "atlas doldu" demiyor — dolma
   kullanıcı tarafında sessiz. Geliştirici tarafında **değil**: `yuva=U/T`
   taban jeton satırının içinde (`app.rs`, `ornek=` dalının üstünde) ve her
   `make duman` koşusunda basılıyor; bir sınama onu orada arıyor
   (`yuva=13/2048`). Yani doluluk kanalı **zaten var** ve seçenekler ona
   bedava bir bekçi bağlayabilir. Eksik olan yalnız kullanıcıya dönük tanı ve
   onu bu set çözmeyebilir.

## Muhakeme (2026-09-22)

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU — **B**, tek düzeltmeyle: sabit piksel değil **yuva** cinsinden |
| Codebase-fit | SORUNLU — **C-düzeltilmiş** (kare sınırında geri dönüşüm), A reddedilir, B tamamlayıcı |
| İşletme | SORUNLU — **B**; A koşullu, C **KIRMIZI** |

**Kabul edilen itirazlar → tasarım değişikliği:**

- **Tehlike yanlış yere bağlanmıştı.** A ve C'nin asıl riski `replaceRegion`
  değil, `slot_uv`'nin uv'yi çözüm anında pişirmesi ve `prepare`'ın kare
  başına **dört kez** koşması. Yukarıdaki A maddesi düzeltildi; ret gerekçesi
  artık bu.
- **Sabit yuva cinsinden yazılacak.** `TEXTURE_EDGE`'in kendi doc'u onu
  "kapasite tercihi" diyor ama atlasın umursamadığı birimde ifade ediyor:
  kararın içindeki her sayı (aile 421, tofu ile 422, eşik 406,
  `RULE_RESERVE` 7, `negative_cache_cap`, `occupancy`) yuva cinsinden. Sabit
  yuvaya taşınınca doyma tablosu bir **değişmeze** dönüşüyor.
- **Hedef ölçülmüş 422'den türetilecek**, havadan "1024" yazılmayacak.
- **`MAX_EDGE` tavanı yazılacak** (4096), yoksa büyük punto × `line_height`
  köşesinde aritmetik sınırsız büyür.
- **İki olgusal hata düzeltildi** (aynı turda, metne işlendi): `yuva=` her
  `make duman` koşusunda basılıyor (yalnız ölçümde değil), ve **pencere
  yeniden boyutlandırmak atlası kurmuyor** — `ensure` yalnız dörtlüyü
  karşılaştırıyor, yani kurtulma kapısı yazdığımdan dar.

**Reddedilenler:**

- **A (LRU)** — ölçülmemiş bir senaryo ("13pt'de ~2000 farklı glyph") için
  beş parça getiriyor: yuva başına damga, dört geçişi kapsayan kare pini,
  thrash guard, muafiyet listesi ve `bt-gpu`'da yükleme yolu. Üstüne arıza
  biçimi düzelttiği kusurdan kötü: kutu **görünür**, thrash'in donan
  penceresi **görünmez**. Ve thrash guard'ın kendi görevi bugünkü davranışa
  geri düşmek.
- **C (dolunca sıfırla)** — yazıldığı hâliyle A'nın yanlış-kare sınıfını
  seçicilik olmadan üretiyor.
- **C-düzeltilmiş** (Codebase-fit'in alternatifi: geri dönüşümü `prepare`'ın
  başına koy, "orada canlı uv yok") — **önermesi yanlış ve kaynaktan
  doğrulandı**: `prepare` kare başına dört kez koşuyor, yani 2–4. çağrıların
  başında önceki geçişlerin uv'leri **canlı**. Kancanın yeri `prepare` değil
  `encode_pass` olmalı. Hatanın **şekli** kayda değer, çünkü bu setin içinde
  **ikinci kez** aynı şekil: bileşen düzeyinde doğru (`prepare` tek çağrı),
  bileşimde yanlış (dört çağrı) — ilk kez bu dosyanın kendi A maddesinde
  olmuştu. Fikir çürük değil, **yeri** yanlış; ölçülmüş bir ihtiyaç doğarsa
  doğru yeriyle geri gelir (aşağıda).

## Karar (2026-09-22, otonom akış — panelden geçmiş öneri)

- **Seçilen: B — atlas dokusunun kenarı hedef **yuva sayısından** türüyor.**
  Ölçülmüş kusur "büyük hücre → az yuva" ve B onu tam oradan vuruyor: tek
  crate (`bt-atlas`), `bt-gpu` tek satır değişmiyor, yeni API/ödünç/sınır
  doğmuyor, `replaceRegion` sınırına dokunulmuyor, kusurun biçimi bugünküyle
  aynı kalıyor (kutu — yani görünür kayıp, `TOFU`'nun kendi felsefesi).
  Varsayılan 13pt'de **bit bit aynı**: kapasite 1984 ≥ hedef, kenar 1024'te
  kalıyor.
- **Reddedilen: A** — ölçülmemiş senaryo için beş parça ve daha kötü bir
  arıza biçimi (yukarıda).
- **Reddedilen: C ve C-düzeltilmiş** — birincisi yanlış-kare üretiyor,
  ikincisinin kancası yanlış yerde.
- **Borç kapanmıyor, daralıyor.** Yol haritasındaki "tahliye (LRU)" maddesi
  bu setle **yeniden yazılacak**: kapasite artık hücre ölçüsünden türüyor,
  geriye kalan senaryo "tek karede hedeften fazla farklı glyph" ve o
  **ölçülmedi**. Ölçülürse çaresi C-düzeltilmiş'in doğru yeriyle
  (`encode_pass` sınırında geri dönüşüm + yapışkan durma koşulu), LRU değil.
  İsim uğruna pahalı yolu seçmek yanlış olurdu.
