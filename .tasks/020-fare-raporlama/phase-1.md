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

## Uygulama Notları

- **R6 daraltıldı: bırakmada kip yeniden soruluyor.** "Asla düşürülmez" sözü
  **koordinat** içindi, kip için değil. Basış raporlandıktan sonra uygulama
  çıkıp `\e[?1000l` göndermişse (vim kapandı) bırakma raporu **kabuğa**
  giderdi ve `\e[<0;5;3m` bir zsh komut satırına düşerdi; alacritty de
  `on_mouse_release`'te kipi yeniden soruyor. Rota kilidi Shift'i kapsıyor,
  kipi kapsamıyor. `release_follows_press`'in ikinci yarısı (ayrı oturum) bu
  kolu çiviliyor.
- **İmza `shift: bool` değil `MouseModifiers { shift, meta, control }`.**
  Plan `mouse_button(.., shift)` yazıyordu ama R2.2 Meta (8) ve Control (16)
  bitlerinin kurulmasını istiyor ve tek `bool` bunu taşıyamıyor. Shift alanı
  aynı struct'ta duruyor: kuralın iki yarısı ("arbitraja girer", "rapora
  girmez") tek tipte yan yana ve gerekçe tipin doc'unda — R2.2'nin "gerekçesi
  yorumda" kutusu orada kapanıyor.
- **Latch bir bitmask** (`ViewIvars::sent_buttons: Cell<u8>`), tek bir "son
  rota" alanı değil. Sol tuşla seçim sürerken sağ tuşa basmak iki jesti
  **aynı anda** doğuruyor; tek alan olsaydı sol bırakış sağın rotasıyla
  raporlanırdı.
- **Kırpmanın sayısı `MouseEncoding::limit`'e çıktı.** 223/2015 bugün
  `wheel_report`'un gövdesine gömülüydü; ret (`>= limit` → `None`) ile
  kırpma (`min(limit - 1)`) aynı sayıyı okumak zorunda, yoksa kırpılan
  koordinat yine reddedilirdi. Bekçisi
  `clamp_lands_on_the_last_coordinate_the_encoding_accepts`. **Sütun da
  kırpılıyor**, yalnız satır değil: 224 sütunlu pencerede sağ kenara bırakış
  düz kodlamada aksi hâlde düşerdi.
- **Basış ile bırakma farklı hücre kapısından geçiyor.** Basış
  `event_cell`'den (band reddi, R8), bırakma `window_point_cell(.., 0)`'dan
  — tekerleğin işaretçisiyle aynı gerekçe. Band reddi basışa ait; bırakmada
  nokta 0. satır olarak girip `bt-core`'da kırpılıyor.
- **`otherMouse*` yalnız `buttonNumber() == 2`.** AppKit dördüncü düğmeden
  sonrasını da o selector'a yolluyor, X10'un iki biti ise üç düğme taşıyor ve
  `3` bırakmaya ayrılmış — 3+'ı orta tuş diye raporlamak uygulamaya yanlış
  düğme söylerdi.
- **`Click::Select` kolunda seçimi yalnız sol tuş başlatıyor.** Sağ/orta tık
  kip kapalıyken bugünkü gibi hiçbir şey yapmıyor (bağlam tıklaması
  beklenmedik bir vurgu üretirdi). Bedeli sağ tık başına bir `Term` kilidi:
  kip dışarıdan sorulamadığı için çağrı zorunlu.
- **`mouseUp:`'ta `buttonNumber()` kapısı yok**, `mouseDown:`'da var.
  Asimetri bilerek: kapı olsaydı beklenmedik bir numara `dragging`'i bayat
  `true` bırakır ve sonraki her kaydırma eski seçimi sessizce uzatırdı.
- **`Select` kolunda iki `Term` kilidi var** (kip sorusu + `set_selection`)
  ve yarış `mouse_button`'ın doc'una adıyla yazıldı: kip iki kilit arasında
  dönerse çapa yine atılır, sonuç bir kez fazladan seçim ve yönü zararsız.
  Çapayı `bt-core`'a almak `Session`'ın arbitrajına seçim politikasını da
  yüklerdi.
- **Bayat latch biti basışta iniyor.** `follow_pointer`'ın `dragging` için
  kapattığı hâl (`mouseUp:` view'a hiç varmaz: sürüklemenin ortasında bir
  modal, bir sistem jesti) `sent_buttons` için de açıktı: `Sent` basıştan
  sonra kaybolan bırakma biti bırakır, kip bu arada kapanınca sonraki basış
  `Select` olur ve onun bırakması bayat biti bulup rapor yolunu seçer —
  `dragging` hiç düşmez ve sonraki her kaydırma eski seçimi sessizce uzatır.
  Basış kolu biti `event_cell`'den **önce** indiriyor. Phase-2'de daha da
  kritik: 1002'nin "yalnız basılıyken"i doğal olarak bu biti okuyacak.
- **Gözle kontrol otomatikleştirilemedi.** System Events'in `click at`'i AX
  tabanlı ve `mouseDown:` üretmiyor; CGEvent ile gerçek HID olayı üreten
  küçük bir Swift aracı derlendi ama pencere güvenilir biçimde öne
  getirilemediği için tıklama öndeki başka uygulamalara düştü. Deneme
  durduruldu, kutu kullanıcıya devredildi ve **kullanıcı dördünü de
  doğruladı** (2026-09-21).

## Checklist

- [x] `input::mouse_report` (SGR `m`, X10 bırakma 3, Meta 8 / Control 16;
      Shift rapora girmiyor ve gerekçesi yorumda)
- [x] `input::mouse_encoding` paylaşılıyor, `wheel_route` onu tüketiyor
- [x] `input::button_route` + modül başlığı düzeltmesi
- [x] `Session::mouse_button` — tek kilit, `send` (`send_input` değil), üç
      varyantlı enum
- [x] `view.rs`: sol/sağ/orta altı selector, rota latch'i, `dragging` yalnız
      `Select`'te
- [x] `CLAUDE.md` aynı commit'te
- [x] Test: `button_route` tablosu — kip açık/kapalı × Shift var/yok
- [x] Test: **asimetri** — Shift düğmeyi geçersiz kılar, tekerleği kılmaz;
      `mouse_mode_comes_first_on_either_screen`'in **yanında** durmalı (R2.1)
- [x] Test: `mouse_report` bas/bırak baytları, üç kodlamada; değiştirici
      bitleri; kodlamaya sığmayan koordinatta `None`
- [x] Test: `release_follows_press` — `Sent` basıştan sonra bırakma kırpılarak
      gidiyor, düşmüyor (R6)
- [x] Test: rapor seçimi temizlemiyor ve pencereyi dibe döndürmüyor
      (`wheel_and_replies_keep_the_selection` kardeşi, R5)
- [x] Doğrulama geçti (`make hepsi`)
- [x] `make duman` — **regresyon alarmı, kapsama kapısı değil**: duman kabuğu
      sabit bir betik, hiçbir fare kipi açmıyor ve depoda fare olayı enjekte
      eden kanca yok, yani bu yol tamamen ölü olsa da yeşil düşer
- [x] **Gözle kontrol** (gerçek doğrulama, 012/013 emsali): Claude Code'da
      tıklama imleci taşıyor · Shift+sürükleme metin seçiyor · `vim` içinde
      `:set mouse=a` ile tıklama imleci taşıyor · kip kapalı kabukta seçim
      bugünkü gibi
- [x] Yayın etkisi yazıldı
