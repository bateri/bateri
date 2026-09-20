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
- `fill == 0` iken daralan içerik **hâlâ snap'liyor** — 011'in kararı bu kolda
  korunuyor. **Örnekler set kapısında düzeltildi** (2026-09-20, ölçüldü):
  doğru örnekler dock'u olmayan pencere, kasten temizlenmiş ekran ve geçmişe
  kaydırılmış pencere. "vim'den çıkış" **yanlış örnekti** — phase-1b'nin
  kabulü zaten "vim'den çıkışta doldurma çalışır" diyor, yani çıkış karesinde
  `fill > 0` ve öteleme süzülmeye başlıyor; snap'i getiren şey dock'u geri
  getiren resize'ın bir sonraki ana kuyruk turunda dikeceği `geometry`
  bayrağı. Kayma bir kare sürüyor ve yönü savunulur, ama tasarlanmış değil:
  kalem `Motion::sync_origin`'in doc'unda adıyla duruyor.
- `reduce_motion` / `cursor_motion = "snap"` doldurma varken de snap'liyor
  (`origin_mode()` `Fade`'i `Snap`'e çeviriyor; erişilebilirlik ayarı
  animasyon **eklemez**).
- `make hepsi` yeşil; `make duman` jetonları değişmemiş.

## Bilinen sınır (adıyla yazılır)

`snap` bugün `scrolled || geometry` ve `scrolled` `display_offset`'in
değişmesi. `filled` ile `scrolled` **aynı karede doğru olabiliyor** — bu satır
bir dönem tersini söylüyordu, çünkü R2.2 kaydırılmış pencerede `fill`'i
sıfırlıyordu; o kapı teslimde kalktı (2026-09-20) ve bant artık kaydırılmış
pencerede de duruyor. Guard'ın davranışı **değişmedi ve doğru kalıyor**:
`!snap` terimi dıştan yutuyor, yani tekerleğin döndüğü karede `snap` kazanıyor
ve öteleme süzülmeden oturuyor. İstenen de bu — kaydırma bir animasyon değil
doğrudan bir manipülasyon, parmağın altındaki içerik gecikmeli gelmemeli.

## Yayın Etkisi

- **`CLAUDE.md` ve `docs/AYARLAR.md`:** yukarıda; ikisi de bu commit'te.
- **011'in karar kaydı:** not düşülür.
- **`docs/YOL-HARITASI.md`:** plan R6'da yazılı ama bu phase'in
  "Değişiklikler"inde unutulmuştu; borç maddesi aynı commit'te daraltıldı —
  "spinner testeresi" bedeli artık yalnız doldurmanın kapalı olduğu kollarda
  geçerli, çünkü doldurmalı oturumda salınımın iki yarısı da kayıyor.
- **Ölçüm bekliyor:** yok — kayma süresi ve `sessiz` bandı 011'in kayıtlı
  borcunda (`kaymanın yerleşme süresi — kanca yok`), bu phase onu
  büyütmüyor.
- shader / terminfo / ayar şeması / tema / shell entegrasyonu / app bundle /
  yeni bağımlılık: yok.

## Checklist

- [x] Guard'ın üçüncü terimi `!snap` içine yazıldı
- [x] `filled` biti `sync` imzasına `snap` sınıfında eklendi
- [x] `motion.rs:1649` yeniden adlandırıldı, doldurma kolu eklendi
- [x] Test: doldurma varken yükselen hedef süzülüyor ve yerleşiyor
- [x] Test: tekerlek + geometri hâlâ snap
- [x] Test: `fill == 0` iken daralan içerik hâlâ snap
- [x] Test: `reduce_motion` / `snap` stili doldurmada da snap
- [x] `CLAUDE.md`, `docs/AYARLAR.md`, 011 karar kaydı tadil edildi
- [x] Doğrulama geçti (`make hepsi`)
- [x] Yayın etkisi yazıldı

## Uygulama Notları

- **Kırmızı iki yönde ölçüldü**, bir değil. Terimi **hiç** yazmamak yalnız yeni
  bekçiyi (`a_filled_gap_slides_the_origin_down_and_settles`) düşürüyor; terimi
  `!snap`'in **dışına** yazmak (`… && !snap && target <= slide.target ||
  filled`, Rust'ın önceliğiyle `(…) || filled`) üç bekçiyi birden düşürüyor —
  `scrolling_and_geometry_snap_the_origin`,
  `snap_style_never_slides_the_origin`,
  `reduce_motion_snaps_the_origin_instead_of_fading_it`. Yerleşim kuralı artık
  yalnız doc'ta değil, sınamada da çivili. İkinci ölçüm için üç bekçiye
  doldurmalı birer kol eklendi (`filled = true` + tekerlek/geometri, `Snap`
  stili, Hareketi Azalt).
- **Aynı hedefi bildiren doldurmalı kare no-op kalıyor** ve bu R4.3'ün
  taşıyıcısı: `filled` guard'ın **girişini** açıyor, içerideki
  `slide.target != target` kapısına dokunmuyor. Dokunsaydı doldurmanın açık
  olduğu her kare kaymayı yeniden kurar, `settled()` hiç `true` olmaz ve boşta
  sıfır kare sözleşmesi düşerdi. Bekçinin son iddiası tam bu.
- **`docs/YOL-HARITASI.md` phase'in listesinde yoktu**, plan R6'da vardı; borç
  maddesi aynı commit'te daraltıldı. 011'in kaydı iki yere düştü: `teslim.md`
  (kuralın daraltılması ve bedelin daralması) ve `phase-2.md` (yeniden
  adlandırılan bekçinin adı, yoksa o satır ölü bir sınamaya işaret ederdi).
