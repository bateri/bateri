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

- [ ] Süzülme animatörü ve bitirme kuralları
- [ ] Link: isteği al, payı teslim et, uçuşta içerik karesi
- [ ] Orijin + kesir, tepe satırının çizimi, `Origin` yayını
- [ ] Test: animatör, frame, kesirsiz kare aynı
- [ ] `CLAUDE.md` cümleleri
- [ ] Doğrulama geçti (`make hepsi`, `make test-yaris`, `make duman`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
