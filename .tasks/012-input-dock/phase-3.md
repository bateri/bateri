# Phase 3 — Dock yüzeyi: ikinci koordinat uzayı

## Özet

Pencerenin altında kendi koordinat uzayına sahip bir dock çizilsin; üst satırda
`>` işareti, metin, caret, öneri ve renklendirme.

_Requirements: R2.1, R2.2, R2.3, R2.5, R5.1_

## Değişiklikler

- **`crates/bt-gpu/src/renderer.rs`** — `encode_pass`'te mevcut üç encode'dan
  **sonra** ikinci bir `setViewport` (kimlik: `originY: 0`). Dock böylece
  ötelemeden **yapısal olarak** muaf olur; aritmetikle muafiyet
  (`- origin_px`) **çalışmaz**, çünkü `Frame::clear` `origin_px`'i sıfırlıyor
  ve `set_origin` sink'ten **sonra** çağrılıyor.
- **`crates/bt-gpu/src/frame.rs`** — dock'un **kendi listeleri** (zemin + glyph)
  ve kendi caret'i.
  - Ayrı liste şart: `bg`'ye girerse `move_cursor`'ın `truncate(bg_count)`'u
    onları her hareket karesinde siler ve dock **titrer**. `stripes`'in ayrı
    liste olma gerekçesiyle aynı.
  - Dock içeriği **sink'ten sonra** basılır (yukarıdaki sıra kısıtı).
  - Caret: `CursorBlock`'un ikinci bir bağlaması — aynı uniform slot'u, ayrı
    encode çağrısı, **shader değişmez**. Alan eklemek iki taraftaki
    `stride 32` assert'ini kırardı.
- **`crates/bt-shell/src/app.rs`** — ızgara yüksekliğinden dock payı düşülür.
  **Ayırma oturum doğarken kararlaşır** (entegrasyon kuruldu mu —
  `child::zsh_wrapper_dir()` spawn anında biliyor) ve koşu boyunca oynamaz.
  Sonucu: `/bin/sh` koşan duman reçetesi dock **almaz**, yani `smoke_shell` ve
  ona bağlı `hucre=8 glif=6 kural=15` sözleşmesi **dokunulmaz**.
- **`crates/bt-core/src/session.rs`** — dock kaydı `frame()` sınırından
  **çözülmüş** geçer: metin, caret sütunu, renk aralıkları, öneri kuyruğu ve
  `>`'in rengi. Safha ve çıkış kodu sınırı geçmez (`karar burada, boyama
  orada`).

**Bu phase bilinçli bir ara durum bırakıyor: çift görüntü.** Prompt hâlâ
kabuğun ve ZLE aynı metni ızgaraya da çiziyor, yani kullanıcı yazdığını iki
yerde görüyor. Gürültülü ama **zararsız**; phase-4 kapatıyor. Ters sıra
(önce bastırma) promptsuz bir terminal bırakırdı.

## Kabul

- Dock pencerenin altında, kendi zemini ve ayracıyla çiziliyor; `>` işareti
  sıradan bir glyph olarak duruyor.
- Yazarken dock metni, caret'i, önerisi (sönük) ve renklendirmesi güncelleniyor.
- Dock **ötelemeden etkilenmiyor:** içerik kayarken (011'in `Slide`'ı) dock
  yerinde duruyor.
- Hareket karesinde dock **titremiyor** (ayrı liste bekçisi).
- Entegrasyonsuz oturumda (`/bin/sh`, bash) dock **yok** ve pencere tamamen
  ızgara; `make duman` jetonları oynamıyor.
- Offscreen bekçi: dock'un boyadığı piksel iki pipeline için de okunuyor
  (emsal `cell_bg_paints_pixels_on_the_gpu`).

## Yayın Etkisi

- **shader:** `.metal` **değişmiyor** (ikinci viewport ve ikinci uniform
  bağlaması Rust tarafında). Değişirse `make shader` zorunlu ve
  `#[repr(C)]` ↔ MSL düzeni alan alan kontrol edilir.
- **`CLAUDE.md`:** "Bugünkü hâl" paragrafı dock'u ve ikinci viewport'u söyler;
  `bt-gpu` satırı dock'u sorumluluklarına ekler.
- **`make duman`:** jeton **eklenebilir** ama kapı olamaz — reçete `/bin/sh` ve
  dock almıyor, yani her koşuda 0 basar. Bu **yazılı** kabul edilir (R6.1);
  dock'un tanığı phase-7'de seçilir.
- Ayar şeması, terminfo, tema, app bundle: yok. Yeni bağımlılık: yok.

## Checklist

- [ ] İkinci `setViewport` (kimlik), mevcut üç encode'dan sonra
- [ ] Dock'un kendi listeleri; `truncate(bg_count)` onları görmüyor
- [ ] Dock içeriği sink'ten **sonra** basılıyor
- [ ] Caret ikinci bağlama; `stride 32` assert'leri **dokunulmadı**
- [ ] Ayırma oturum doğarken kararlaşıyor; entegrasyonsuz oturumda dock yok
- [ ] Sınır kaydı **çözülmüş** (safha ve çıkış kodu sınırı geçmiyor)
- [ ] Test: ötelemeden muafiyet (içerik kayarken dock sabit)
- [ ] Test: hareket karesinde dock listesi korunuyor (titreme bekçisi)
- [ ] Test: offscreen render, dock'un pikseli okunuyor
- [ ] Doğrulama geçti (`make hepsi`; `make duman` jetonları oynamadı)
- [ ] Yayın etkisi yazıldı
