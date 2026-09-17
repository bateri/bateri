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
  `bt-gpu` satırı dock'u sorumluluklarına ekler. **Yapıldı**, aynı commit'te.
- **Ayar şeması: yok.** Dock payı ayar değil, entegrasyonun sonucu
  (`prompt` anahtarı phase-5'in işi) — `docs/AYARLAR.md` dokunulmadı.
- **Tema: yeni rol yok.** Dock'un iki rengi de türetilmiş
  (`Theme::separator_linear` = sönük ön planın zemine bir kez daha karışmış
  hâli); `foreground_linear` ve `dim_linear` var olan rollerin yeni
  okuyucuları. Tema dosyasının biçimi değişmedi, geriye dönük okuma sorusu yok.
- **`make duman`:** jeton **eklenebilir** ama kapı olamaz — reçete `/bin/sh` ve
  dock almıyor, yani her koşuda 0 basar. Bu **yazılı** kabul edilir (R6.1);
  dock'un tanığı phase-7'de seçilir.
- Ayar şeması, terminfo, tema, app bundle: yok. Yeni bağımlılık: yok.

## Uygulama Notları

- **Viewport kimlik değil, dokunun dibine yaslı.** Phase `originY: 0` diyordu;
  o dock'u pencerenin **tepesine** koyardı. Orijin `yükseklik − dock payı` ve
  `Frame` dokunun boyunu bilmediği için hesap `encode_pass`'te. Aynı sebeple
  yüzeyin zemini de orada genişliğini alıyor (`Frame::dock_ground(width_px)`):
  aritmetik `frame.rs`'te kaldı, yalnız genişlik dışarıdan geliyor — yoksa
  `Instance` düzeninin ikinci bir yazarı doğardı.
- **Caret'in dikdörtgeni CPU'da ötelenıyor** (`CursorBlock::shifted_y`) ve bu
  ızgaranınkinin tam tersi: orada instance ötelemeyi geri veriyor, dikdörtgen
  ekran satırında doğuyor. Sebep aynı — dikdörtgen fragment'in
  `[[position]]`'ı ile karşılaştırılıyor, yani viewport dönüşümünden
  **sonraki** uzayda. Kayma unutulsaydı caret bloğu doğru yerde çizilir ama
  altındaki harf kendi ön planıyla kalırdı: beyaz üstüne beyaz, yani kaybolan
  bir hücre. Bekçisi `the_dock_draws_glyphs_and_its_own_caret`.
- **`bt-core`'da yeni bir modül** (`dock.rs`) ve saf: kilit almıyor, `Session`
  görmüyor, sınamaları PTY'siz koşuyor. `Session::dock` yalnız iki yaprak
  kilidi **ardışık** alıp gövdeye veriyor (tema, sonra `shell`) — ayna ile
  safha **aynı** okumadan çıksın diye tek `shell` kilidi: ayrı çağrılardan
  alınsalardı araya düşen bir işaret `>`'i bir kareliğine metinle çelişen bir
  renge boyardı.
- **`>`'in rengi safhadan** ve sözlük blok şeridininkiyle aynı (`Finished` +
  çıkış kodu → başarı/hata, gerisi vurgu). Planda "safha rengiyle" yazıyordu,
  sözlüğün blokla ortak olması buranın kararı: aynı gerçeği iki yerde iki
  türlü anlatmamak için.
- **Taşan satır soldan pencereleniyor, kırpılmıyor.** Planda yoktu; kırpmak
  caret'i ekrandan düşürürdü (yazdığını görmeyen bir giriş satırı) ve uzun bir
  satır kare başına binlerce glyph instance'ı üretirdi. `cols` bu yüzden
  sınırı geçiyor — `DisplayLink` onu `resize`'da tazeliyor.
- **`bt_gpu::Layout` planda yoktu, clippy getirdi:** `DisplayLink::new` sekiz
  argümana çıkınca `too_many_arguments` düştü. Üçlü (`cols`, `dock_rows`,
  `cell`) zaten aynı yerden aynı anda doğuyor; tip onları imzada "birlikte
  değişir" diye söylüyor.
- **Dock payı satırlardan koşullu düşülüyor**, sol payın tersine: dock'u
  olmayan pencereden iki satır götürmek sekiz noktalık sol payla
  kıyaslanmayacak bir bedeldi. Koşullu olabilmesinin şartı ayrımın koşu
  boyunca oynamaması (R5.1) — oynasaydı komut başına bir `TIOCSWINSZ` doğardı.
- **Entegrasyon `didFinishLaunching`'te bir kez soruluyor** ve iki cevabı
  birden veriyor: çocuğun ortamı ile dock'un varlığı. İki ayrı çağrı
  ayrışabilirdi — pencereden iki satır giden ama dock'u olmayan bir oturum, ve
  belirti sessiz olurdu. `start_session` ortamı artık parametre olarak alıyor.
- **Dock kendi kare talebini taşımıyor.** Ayna yükü ayrıştırıcıya da ulaşıyor
  ve alacritty işlenen her bayt için `Event::Wakeup` basıyor
  (`event_loop.rs`'te doğrulandı), yani `dirty` zaten dikiliyor. Boşta sıfır
  kare sözleşmesi dokunulmadan kaldı.
- **Negatif `originY` kapatıldı:** dock'tan alçak pencerede fark negatife
  iniyordu ve negatif viewport orijini Metal doğrulamasına düşerdi — süreci
  öldüren bir istisna. Sıfırda kırpılıyor, dejenere cevap "dock pencereyi
  kaplar".
- **`DOCK_ROWS = 2` şimdiden ayrıldı**, alt satır (bağlam) boş: phase-6'da
  doldurulacak. Sonradan büyütmek kullanıcının penceresini bir satır kısaltan
  ikinci bir `TIOCSWINSZ` demekti. Phase'in ikinci bilinçli ara durumu.
- **Bilinen sınır (yazılı):** `region_highlight`'ın numaralı renkleri temanın
  paletinden çözülüyor, uygulamanın OSC 4 ile değiştirdiği tablodan değil — o
  tablo `Term` kilidinin arkasında ve dock `Term`'e hiç dokunmuyor
  (`Event::ColorRequest`'in bilinen sınırıyla aynı kök). Kontrol karakterleri
  de dock'ta mürekkep üretmiyor: yer tutucu (`^C`) çizmek sütun aritmetiğini
  karakter biriminden çıkarır, caret'in yerini de kaydırırdı.

## Checklist

- [x] İkinci `setViewport`, mevcut üç encode'dan sonra — **kimlik değil**
      dokunun dibine yaslı (gerekçe Uygulama Notları'nda)
- [x] Dock'un kendi listeleri; `truncate(bg_count)` onları görmüyor ve
      sayaçlara da girmiyorlar
- [x] Dock içeriği sink'ten **sonra** basılıyor; yüzey de hücrelerden sonra
      açılıyor (renkleri getiren çağrı hücreleri basan çağrı)
- [x] Caret ikinci bağlama; `stride 32` assert'leri **dokunulmadı**,
      `.metal` değişmedi
- [x] Ayırma oturum doğarken kararlaşıyor; entegrasyonsuz oturumda dock yok
      ve ızgaradan satır gitmiyor
- [x] Sınır kaydı **çözülmüş** (safha ve çıkış kodu sınırı geçmiyor;
      `bt_core::Dock` yalnız iki renk, caret sütunu ve caret metni)
- [x] Test: ötelemeden muafiyet — CPU'da `the_dock_never_reads_the_origin`,
      pikselde `the_dock_paints_the_bottom_band_and_the_sliding_grid_cannot_reach_it`
- [x] Test: hareket karesinde dock listesi korunuyor (titreme bekçisi)
- [x] Test: offscreen render, dock'un pikseli **iki pipeline için de**
      okunuyor (zemin/ayraç/hücre + glyph/caret)
- [x] Test: `bt-core`'un çözücüsü — dokuz sınama (işaret, öneri, vurgu,
      standout, safha rengi, `Idle`/`Unavailable`, pencereleme, dejenere
      genişlik, yüzey renkleri) + `race_dock_state_and_frame` artık kare
      yolundan `dock()` de çağırıyor
- [x] Doğrulama geçti: `make hepsi` + `make test-yaris` (iki profilde de yeşil)
- [x] `make duman` yeşil (kullanıcı koştu; ajanın kabuğunda gerçek pencere
      açılmıyor). Jetonlar **oynamadı** — reçete `/bin/sh`, yani
      `dock_rows = 0` ve dock yolu hiç açılmıyor:
      `kare=29 hucre=8 glif=6 kural=15 yuva=13/2048 yuk=smoke istek=4`
      `icerik=2 hareket=27 kayma=0 sessiz=1744.06ms kapanis=clean`
- [x] Gözle doğrulama (Kabul'ün beş maddesi) — kullanıcı gerçek zsh
      oturumunda onayladı. `shell.integration = "off"` kolu ilk denemede
      "olmadı" göründü: sebep kod değil, anahtarın **sonraki oturumda**
      geçerli olması (şemanın tek istisnası) — yeniden başlatınca dock yok ve
      ızgara iki satır daha uzun
- [x] Yayın etkisi yazıldı
