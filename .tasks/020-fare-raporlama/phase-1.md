# Phase 1 — Düğme raporu ve arbitraj

## Özet

Fare kipi açıkken Shift'siz basış ve bırakma uygulamaya rapor olarak gider;
Shift basılıyken seçim başlar.

_Requirements: R1, R2, R2.1, R2.2, R3, R4, R5, R6, R8, R9_

## Neden belirti bu phase'de kapanıyor

Bildirilen belirti (tıklanan yere imleç gelmemesi) **1000 seviyesidir**:
uygulamanın imleci basış raporuyla taşınıyor. Hareket raporu phase-2'de ve
belirtiyi kapatmak için gerekmiyor.

## Değişiklikler

- **`crates/bt-core/src/input.rs`**
  - `wheel_report` → `mouse_report`: SGR'ın bırakma biçimi (`m`), X10'un
    bırakma düğmesi (`3` — eski kodlama hangi düğmenin bırakıldığını
    **taşıyamaz**, bu protokolün kendi sınırı), değiştirici bitleri
    **Meta 8 / Control 16**. Shift (4) **hiç kurulmuyor**: arbitraj onu
    yuttuğu için Shift'li basış rapora zaten gelmiyor (R2.2) — bu bir eksik
    değil, kuralın sonucu ve yorumda adıyla yazılı olmalı.
  - Kodlama seçimi `wheel_route`'un gövdesinden `mouse_encoding(mode)`'a
    çıkıyor; iki tüketici, tablo tek yerde.
  - `button_route(mode, shift)` — `wheel_route`'un kardeşi. `MOUSE_MODE`
    `intersects` ile sorulur (`contains` üç biti birden ister, `wheel_route`'un
    yazılı gerekçesi).
  - Modül başlığı düzeliyor: "ok tuşu ve tekerlek raporu" artık yanlış (R9).
- **`crates/bt-core/src/session.rs`** — `Session::mouse_button(button,
  pressed, at, shift)`. **Tek `Term` kilidi**: kip sorusu, `viewport_point`
  ile satırın uygulamanın ekranına inmesi ve gönderim birlikte. Kip
  **dışarıdan sorulmuyor** (`bracketed_paste`'in yazılı gerekçesi, R3).
  Gönderim `send(Msg::Input(..))` — `send_input` **değil**, yani seçim durur
  ve pencere dibe dönmez (R5, `wheel_and_replies_keep_the_selection` emsali).
  Dönüş üç varyantlı enum, `Wheel` emsali (R4).
- **`crates/bt-shell/src/view.rs`** — `mouseDown:`/`mouseUp:` ve **sağ/orta
  tuşun dört selector'ı** (`rightMouseDown:`, `rightMouseUp:`,
  `otherMouseDown:`, `otherMouseUp:`; depoda bugün **hiç yok**, hepsi aynı
  gövdeye tek satırlık kollar). `dragging` yalnız `Select` kolunda kuruluyor
  — bugün `set_selection`'dan **önce koşulsuz** kuruluyor ve `Sent` kolunda
  kurulu kalsaydı `mouseDragged:` var olan **eski** seçimi büyütürdü.
  Rota `mouseDown:`'da kilitleniyor (R6).
- **`CLAUDE.md`** — `bt-core` satırındaki "girdi kodlaması (DECCKM'e uyan
  oklar, tekerlek raporu)" cümlesi düğmeyi de anmak zorunda; arbitraj kuralı
  (Shift) kullanıcıya bakan tek yazılı yer olduğu için **buraya** giriyor (R9).

## Kabul

- Fare kipinde Shift'siz tıklama uygulamaya gidiyor, Shift'li tıklama seçim
  başlatıyor; kip kapalıyken ikisi de bugünkü gibi seçim.
- `Sent` basışın bırakması **her zaman** raporlanıyor (kırpılarak), asla
  düşmüyor.
- Fare kipinde tıklamak seçimi temizlemiyor ve pencereyi dibe döndürmüyor.
- Doldurma bandının üstündeki basış ne rapor ne seçim üretiyor.
- `make hepsi` yeşil.

## Yayın Etkisi

- **`CLAUDE.md`:** `bt-core` satırı + arbitraj kuralının yazılı hâli (aynı
  commit, R9). `docs/AYARLAR.md`'de davranış bölümü yok, yani kullanıcının
  Shift+sürüklemeyi öğrenebileceği **tek yer** `CLAUDE.md` — bilinçli ve
  yazılı bir sınır.
- **Ayar şeması: yok.** Yeni anahtar yok, varsayılan değişmiyor.
- **Görünür davranış değişikliği (geri alma değil, bilgi):** geçmişe
  kaydırılmış pencerede fare kipinde tıklamak artık hiçbir şey yapmıyor —
  bugün seçim başlatıyor.
- **Ölçüm bekliyor: yok.**

## Checklist

- [ ] `input::mouse_report` (SGR `m`, X10 bırakma 3, Meta 8 / Control 16;
      Shift rapora girmiyor ve gerekçesi yorumda)
- [ ] `input::mouse_encoding` paylaşılıyor, `wheel_route` onu tüketiyor
- [ ] `input::button_route` + modül başlığı düzeltmesi
- [ ] `Session::mouse_button` — tek kilit, `send` (`send_input` değil), üç
      varyantlı enum
- [ ] `view.rs`: sol/sağ/orta altı selector, rota latch'i, `dragging` yalnız
      `Select`'te
- [ ] `CLAUDE.md` aynı commit'te
- [ ] Test: `button_route` tablosu — kip açık/kapalı × Shift var/yok
- [ ] Test: **asimetri** — Shift düğmeyi geçersiz kılar, tekerleği kılmaz;
      `mouse_mode_comes_first_on_either_screen`'in **yanında** durmalı (R2.1)
- [ ] Test: `mouse_report` bas/bırak baytları, üç kodlamada; değiştirici
      bitleri; kodlamaya sığmayan koordinatta `None`
- [ ] Test: `release_follows_press` — `Sent` basıştan sonra bırakma kırpılarak
      gidiyor, düşmüyor (R6)
- [ ] Test: rapor seçimi temizlemiyor ve pencereyi dibe döndürmüyor
      (`wheel_and_replies_keep_the_selection` kardeşi, R5)
- [ ] Doğrulama geçti (`make hepsi`)
- [ ] `make duman` — **regresyon alarmı, kapsama kapısı değil**: duman kabuğu
      sabit bir betik, hiçbir fare kipi açmıyor ve depoda fare olayı enjekte
      eden kanca yok, yani bu yol tamamen ölü olsa da yeşil düşer
- [ ] **Gözle kontrol** (gerçek doğrulama, 012/013 emsali): Claude Code'da
      tıklama imleci taşıyor · Shift+sürükleme metin seçiyor · `vim` içinde
      `:set mouse=a` ile tıklama imleci taşıyor · kip kapalı kabukta seçim
      bugünkü gibi
- [ ] Yayın etkisi yazıldı
