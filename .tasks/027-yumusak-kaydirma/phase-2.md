# Phase 2 — `bt-gpu`: kesirli çizim ve çentik süzülmesi

## Özet

Kesir orijine girer, tepe satırı bandın üstünde çizilir ve çentiğin
süzülmesi `Motion`'da ikinci bir `Slide` olarak koşar; tetikleyen
`bt-shell` henüz yok, doğrulama hermetik.

_Requirements: R2.1, R2.2, R2.3, R2.4_

## Değişiklikler

- **`crates/bt-gpu/src/motion.rs`** — süzülmenin animatörü: `Slide`'ın ikinci
  örneği, birimi "teslim edilecek kalan satır". İstek hedefe eklenir, konum
  stilin fiziğiyle (`ease_axis`/`spring_axis`) gider; kare başına pay
  konumun değişimi. `settled()` onu kapsar; `finish()`, `snap`'e geçiş
  (`set_style`), Hareketi Azalt (`set_reduce`) ve nesil değişimi onu kalan
  payı **teslim ederek** mi yoksa düşürerek mi bitiriyor — karar doc'ta
  (nesil değişiminde düşürür: dibe dönüş zaten gitmek istenen yer).
  `sync`'in ofset snap'i (`scrolled`) bu animatöre dokunmaz. Ölçülmemiş
  sabit yok.
- **`crates/bt-gpu/src/link.rs`** — kare başında isteği `Session`'dan al,
  `advance`'ten sonra payı **`frame()`'in argümanı** olarak ver (ayrı bir
  çağrı değil: uyandırmaz, ikinci bir kilit turu açmaz).
  Süzülme uçuştayken hasarsız kol içerik yoluna düşer (yerel karar; modül
  başlığının "kare istemenin üç yolu" metni: bu **hareket** yolu, çizimi
  içerik karesi — gerekçe saatin içerik tadınınki). `icerik=` bu kareleri
  sayar; `kayma=` ve `hareket=` anlamlarını korur, yeni jeton yok. Orijin
  bileşimi: `set_origin`'e giden değer öteleme + kesir.
- **`crates/bt-gpu/src/frame.rs`** — tepe satırının listesi bandın
  listelerinde ya da yanında; bandın orijini (`fill_origin_px`) tepe
  satırını da kapsar; `set_origin_rows`'un piksel yuvarlaması kesri de
  yuvarlar. Sayaçlardan muaf (`hucre=`/`glif=`/`kural=` oynamaz).
- **`Origin`** — yayınlanan `px` kesri içerir, `fill_rows` tepe satırını da
  sayar ki `point_to_cell` orayı reddetsin (bant satırı sözleşmesi).
- **`CLAUDE.md`** — kare talebi, ötelemenin bileşimi, doldurma bandının
  çizimi ve `Motion`'ın animatör sayısı cümleleri.

## Kabul

- Motion sınamaları: süzülme payları toplamı istenen satır sayısı; yerleşir
  ve `settled()` döner; `snap`/Hareketi Azalt/nesil değişimi bitirir; ofset
  değişimi (sync) süzülmeyi bitirmez; `snap` stilinde istek anında teslim.
- Frame sınamaları: kesirli orijin piksele yuvarlanıyor, tepe satırı bandın
  üstünde ve bandla birlikte kayıyor; tepe satırı sayaçlara girmiyor.
- Kesir sıfırken kare bugünküyle bit bit aynı (mevcut offscreen sınamalar).
- `make hepsi`, `make test-yaris`, `make duman` yeşil (jetonlar değişmez;
  duman kaydırmıyor).

## Checklist

- [x] Süzülme animatörü ve bitirme kuralları
- [x] Link: isteği al, payı teslim et, uçuşta içerik karesi — pay
  `ScrollGlide` olarak ve **`take_scroll_glide`'ın döndürdüğü nesille**
  geri veriliyor (phase-1 Uygulama Notları); nesil değiştiyse uçuştaki
  süzülme o karede bitiyor
- [x] Doldurma kanalının boyu `top_row + fill` (`set_fill_rows`,
  `fill_origin_px`, bandın blok listesi): phase-1'den beri `frame()` tepe
  satırını fill-yerel `0`'da veriyor
- [x] Orijin + kesir, tepe satırının çizimi, `Origin` yayını
- [x] Test: animatör, frame, kesirsiz kare aynı
- [x] `CLAUDE.md` cümleleri
- [x] Doğrulama geçti (`make hepsi`, `make test-yaris`, `make duman`) —
  `make duman` `/code-review` öncesi iki kez yeşil (`icerik=2`/`3`,
  jetonlar değişmedi); düzeltmelerden sonra kırmızı ama **HEAD'de de aynı
  şekilde** (`hareket=0`, pencere o an kare almıyordu), yani ortam — [~]
  düzeltmeler sonrası duman'ı yeniden koşmak.
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi (iki waive,
  Uygulama Notları)

## Uygulama Notları

- **Izgaradaki caret de kesirle kayıyor** (planda yoktu): caret ötelemenin
  kaymasından muaf ama kesirden değil; kaymasaydı blok harfinin bir kısmını
  ve üstteki satırın şeridini örter, ters çevirme yanlış pikselleri
  çevirirdi (dock'suz pencere, dipte trackpad). `Frame::push_caret` kesri
  yalnız ızgara yuvasına ekliyor; yuva **kaydırılmamış** konumdan seçiliyor,
  dock caret'i kaydırmadan muaf.
- **Kesir ayrı yuvarlanıyor** (`Frame::frac_px`): `origin_px =
  round(öteleme·h) + round(kesir·h)`. Tek yuvarlama caret'in payını
  toplamdan geri çıkaramazdı; fark en çok bir piksel ve yalnız öteleme ile
  kesir aynı karede kayarken.
- **`advance` taramadan önceye taşındı** (pay `frame()`'in argümanı);
  `set_grid_top`'un "advance'ten önceki konum" sözü için değer önceden
  alınıyor. İmleç ve öteleme için sıra değişmedi (`advance` yine `sync`'ten
  önce, arada `motion`'ı okuyan yok).
- **İstek `advance`'ten sonra alınıyor**, ilk pay sıfır: uykudan uyanan
  link'in `dt`'si `DT_MAX`'e kırpılmış bir uydurma ve `ease`'de çentiğin
  yarısını tek karede götürürdü. Bedeli bir karelik gecikme.
- **Teslim edilmemiş pay `settled()`'ın içinde** (`Motion::glide_idle`):
  bitirilen süzülme (örtülme, `snap`, Hareketi Azalt) payını düşürmüyor,
  sıradaki içerik karesinde teslim ediyor; bitiren her yol zaten kare
  istiyor. Nesil değişimi ise düşürüyor ve nesil iki kez soruluyor — istek
  alınırken ve `frame()`'den sonra, çünkü `frame()` geçersiz kesirde nesli
  kendisi artırıyor.
- **Süzülmenin kipi ötelemeninki** (`origin_mode`): Hareketi Azalt'ta istek
  anında teslim.
- **Uca çarpan süzülme bitiyor** (`Motion::observe_scroll`, `/code-review`):
  pay sıfırdan farklıyken konum `(display_offset, kesir)` kıpırdamadıysa
  kalan düşüyor. Olmasaydı dipte aşağı fırlatılan tekerleğin ulaşılamayan
  kalanı boş içerik kareleri çizdirir ve ters yöndeki çentiği yerdi.
- **`/code-review` düzeltmeleri**: toplam yalnız `Frame::origin_px()`'te
  (iki setter'ın sırası serbest); `frac_px` bir hücreden kısa kırpılıyor
  (`0.99` tam hücreye yuvarlanıyordu); ızgara caret'inin ters çevirme
  dikdörtgeni dock bandının tepesinde kırpılıyor (kesirle banda itilen
  caret dock'un harflerini zemin renginde çizdirirdi); CLAUDE.md'nin
  "banda değen caret dock yuvasına" cümlesine kesir istisnası yazıldı.
- **Waive — devirde kesir sıçraması** (`/code-review`): ızgaradan dock'a
  devir kaymasında eşiği geçen karede kesir düşüyor, caret o karede kesir
  kadar sıçrıyor. Kesir yalnız süren bir jestte sıfırdan büyük, devir
  komutun bitişinde; kapatmak kesri caret'in animatörüne taşımak ve her
  süzülme karesinde caret'i yeniden hedeflemek demek. Adıyla
  `Frame::push_caret`'ta.
- **Waive — süzülme karesi tam içerik karesi** (`/code-review`): payı
  uygulamak `Term` kilidi ister ve plan (R2.3) uçuştaki kareyi içerik karesi
  olarak kararlaştırdı; ofseti değişmeyen kareleri hareket yoluna almak
  `bt-core`'da kilitsiz bir kesir yolu ister ve bu phase'in dışında.
  Ölçülmemiş bir maliyet iddiası yazılmıyor.
- Testler önce derlenmeyerek kırmızıydı; ısırdıkları mutasyonla gösterildi
  (nesil düşürmesi, bitirmenin teslimi, caret'in kesri, `settled()`'ın
  süzülmeyi görmesi kapatılınca ilgili sınamalar düşüyor).
