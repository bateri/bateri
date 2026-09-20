# Phase 4 — Kayarak dönüş

## Özet

Doldurma varken yükselen öteleme hedefi de süzülür; 011'in tek yön kuralı
daralıyor ve belgeleri aynı commit'te tadil ediliyor.

_Requirements: R4.1, R4.2, R4.3, R6_

## Değişiklikler

- **`crates/bt-gpu/src/motion.rs`** — `sync_origin`'in guard'ına üçüncü terim:
  `animated && !snap && (target <= slide.target || filled)`.
  - **Konum şart:** `!snap`'in **içinde**. Dışına yazılırsa
    `scrolling_and_geometry_snap_the_origin` kırılır ve tekerlek ile pencere
    boyutlandırma animasyona başlar — `bt-core` tarafındaki
    `display_offset == 0` kapısı tekerleği kesiyor ama **geometri** kolunu
    kesmiyor (R4.2).
  - `filled` biti `Motion::sync`'e `snap` ile aynı sınıfta girer
    (`display_offset` ve `geometry` gibi): `bt-gpu` terminal semantiği
    öğrenmiyor, `link.rs` `cursor.fill > 0`'ı hesaplayıp veriyor.
  - Yeni animatör ve yeni durma koşulu **yok** (R4.3): `Slide::settled()`
    aynen geçerli, link'in uyku kararı değişmiyor, boşta sıfır kare korunuyor.
- **`crates/bt-gpu/src/motion.rs`** — bekçi
  `a_growing_origin_slides_and_a_shrinking_one_snaps` (`:1649`) **yeniden
  adlandırılır**: daralan artık yalnız `fill == 0` iken snap'liyor, yani adı
  yalan oluyor. Yanına doldurma kolu eklenir.
- **Belge tadilleri (R6), aynı commit'te:**
  - `CLAUDE.md` — "Kayma **tek yönlüdür**" cümlesi koşulunu kazanır.
  - `docs/AYARLAR.md:506-516` — "Yalnız yukarı kayar" maddesinin kullanıcı
    dili.
  - `.tasks/011-tabana-yapisik-icerik/` karar kaydı — kural **daraltıldığı**
    için not düşülür, sessizce çelişilmez. Snap'in gerekçesi (`4c291ee`:
    "aşağı iniş *düşmesi* gibi okunuyor") korunuyor: doldurma varken aşağı
    inen şey boşluk değil, üstten geçmiş **geliyor**.

## Kabul

- Ekran dolu → Tab → Ctrl-C: içerik aşağı **süzülüyor**, üstten geçmiş
  satırları giriyor; bekçi kayma karelerinin sayısının sıfırdan büyük ve
  sonlu olduğunu çiviliyor.
- Enter + kısa çıktı kolunda da süzülüyor (phase-2'nin safha kapısız kapısı
  sayesinde).
- Tekerlek ve pencere/font/punto değişimi **hâlâ snap'liyor**.
- `fill == 0` iken daralan içerik **hâlâ snap'liyor** (vim'den çıkış, dolu
  ekranda `clear`) — 011'in kararı bu kolda korunuyor.
- `reduce_motion` / `cursor_motion = "snap"` doldurma varken de snap'liyor
  (`origin_mode()` `Fade`'i `Snap`'e çeviriyor; erişilebilirlik ayarı
  animasyon **eklemez**).
- `make hepsi` yeşil; `make duman` jetonları değişmemiş.

## Bilinen sınır (adıyla yazılır)

`snap` bugün `scrolled || geometry` ve `scrolled` `display_offset`'in
değişmesi. R2.2 `display_offset != 0` iken `fill`'i zaten sıfırlıyor, yani
`filled` ile `scrolled` normalde aynı karede doğru olamaz — **tek istisna**
tekerleğin, `fill`'in hesaplandığı kareye denk gelmesi. Bu, İşletme jürisinin
bayrak için adlandırdığı bir karelik yarışın aynısı. Tasarım değişikliği
gerektirmiyor: guard'ın `!snap`'i onu yutuyor (`snap` kazanıyor, yani o kare
snap'liyor) ve yanlışın yönü güvenli.

## Yayın Etkisi

- **`CLAUDE.md` ve `docs/AYARLAR.md`:** yukarıda; ikisi de bu commit'te.
- **011'in karar kaydı:** not düşülür.
- **Ölçüm bekliyor:** yok — kayma süresi ve `sessiz` bandı 011'in kayıtlı
  borcunda (`kaymanın yerleşme süresi — kanca yok`), bu phase onu
  büyütmüyor.
- shader / terminfo / ayar şeması / tema / shell entegrasyonu / app bundle /
  yeni bağımlılık: yok.

## Checklist

- [ ] Guard'ın üçüncü terimi `!snap` içine yazıldı
- [ ] `filled` biti `sync` imzasına `snap` sınıfında eklendi
- [ ] `motion.rs:1649` yeniden adlandırıldı, doldurma kolu eklendi
- [ ] Test: doldurma varken yükselen hedef süzülüyor ve yerleşiyor
- [ ] Test: tekerlek + geometri hâlâ snap
- [ ] Test: `fill == 0` iken daralan içerik hâlâ snap
- [ ] Test: `reduce_motion` / `snap` stili doldurmada da snap
- [ ] `CLAUDE.md`, `docs/AYARLAR.md`, 011 karar kaydı tadil edildi
- [ ] Doğrulama geçti (`make hepsi`)
- [ ] Yayın etkisi yazıldı
