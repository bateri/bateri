# Phase 6 — Bağlam satırı: dizin ve git dalı

## Özet

Dock'un alt satırında, sol altta `[tam klasör yolu] | [git dalı]` yan yana
dursun.

_Requirements: R2.4_

## Değişiklikler

- **`crates/bt-core/src/session.rs`** — **OSC 7 bağlanır.** Bugün
  `Event::Title`/`ResetTitle` sessizce düşüyor ve dizin hiçbir yerde tutulmuyor;
  011 Karar 12 dizini **tam bu yüzden** kapsam dışı bırakmıştı, bu phase o
  kararı bilerek geri alıyor.
  - OSC 7 `file://host/path` biçiminde geliyor; **yüzde çözme** ve yabancı
    host'un elenmesi burada. Bozuk URI **panik değil** yoksayma.
  - Dizin `DockState`'in yanında yaşar, `Term` kilidinin dışında.
- **`assets/shell/zsh/bateri.zsh`** — **git dalı `precmd`'den** gelir; terminal
  kendi `git` sürecini **doğurmaz** (pahalı ve tasarımı ayrı bir iş).
  - Bedeli **prompt başına bir fork** (`git rev-parse --abbrev-ref HEAD`).
    Büyük depoda hissedilir — p10k'nın `gitstatusd` daemon'ı bu yüzden var.
    Hızlandırma **kapsam dışı** ve borç olarak yazılır.
  - Depo değilse dal **boş**; ayraç da çizilmez.
  - Detached HEAD'de dal yerine kısa SHA.
- **`crates/bt-gpu/src/frame.rs`** — dock'un alt satırı çizilir.
  - **Taşma kuralı:** yol **soldan** kısaltılır (kuyruk daha bilgilendirici:
    `…/bateri-term/bateri`), dal **asla** kısalmaz. Kısaltma sınırı dock'un
    genişliğinden türetilir, sabit değil.
  - Alt satır sönük (`dim` rolü); ayraç `|`.

## Kabul

- Dock'un alt satırında sol altta tam yol ve dal yan yana duruyor.
- `cd` yapınca yol **anında** güncelleniyor (OSC 7 prompt'ta basılıyor).
- Depo olmayan dizinde dal ve ayraç yok, yalnız yol.
- Detached HEAD'de kısa SHA görünüyor.
- Dar pencerede yol soldan kısalıyor, dal tam kalıyor.
- Bozuk ya da yabancı host'lu OSC 7 yoksayılıyor; panik yok.

## Yayın Etkisi

- **`CLAUDE.md`:** `bt-core` satırı OSC listesinde 7'nin artık **tüketildiğini**
  söyler ve tarayıcının kolu "iki" değil **üç**; "Bugünkü hâl"in "alt satırı
  (bağlam) boş" cümlesi de kalktı. `shell.rs` ile `dock.rs`'in modül
  başlıkları ve `DOCK_ROWS`'un doc'u aynı commit'te güncellendi.
- **`make kur` zorunlu** (`assets/shell/*` değişti).
- **Ölçüm bekliyor + araç da borç:** prompt başına `git` fork'unun maliyeti.
  `BT_INPUT_LATENCY_SAMPLES` yok, yani `/measure` bugün kapatamaz; belirti
  "büyük depoda prompt gecikmesi" ve kullanıcının göreceği tek yüzey o.
- **Borç:** dal için daemon/önbellek — `docs/YOL-HARITASI.md` → "Sete
  bağlanmamış borçlar"a kalem olarak yazıldı (ölçüm borcu da aynı kalemde).
- Ayar şeması: **yok** (yol ve dal her zaman görünür; gizleme anahtarı bu setin
  işi değil). shader, terminfo, tema, app bundle: yok. Yeni bağımlılık: yok.

## Uygulama Notları

- **OSC 7 `session.rs`'te değil tarayıcının üçüncü kolunda.** Bu bölümün
  "`Event::Title`/`ResetTitle` sessizce düşüyor" gerekçesi **yanlıştı**:
  `vte` OSC 7'yi hiç ayrıştırmıyor (`vte-0.15.0/src/ansi.rs`'in
  `osc_dispatch`'i 0, 2, 4, 8, 10–12, 22, 50, 52, 104, 110–112'yi tanıyor;
  7 `unhandled`'a düşüyor), yani düşen bir olay yok — **hiç olay doğmuyor**.
  Kol bu yüzden 133 ve 8133'ün yanına, `bt-core::shell::Scanner`'a eklendi:
  `Arm::Cwd`, kendi tamponu ve kendi sınırı (`CWD_PAYLOAD_LIMIT`, 4 KiB =
  `PATH_MAX` × en kötü 3× yüzde kodlama, yuvarlanmış).
- **Yük alanlara bölünmüyor.** `parse_mark`/`parse_dock`'un aksine `7;`
  sonrası tek parça okunuyor: `;` bir dosya adında geçerli ve bölseydik
  `/tmp/a;b` yolu `/tmp/a` olurdu. Sınaması var.
- **"Yabancı host" = adlı her host** (`LOCAL_AUTHORITIES`: boş ve
  `localhost`). Plan "yabancı host elenmesi" derken doğal okuma
  `gethostname` ile karşılaştırmaktı; o **yapılmadı**, çünkü `bt-core`'un üç
  bağımlılığı var (`alacritty_terminal`, `polling`, `toml_edit`) ve `libc`/
  `rustix` eklemek bir mimari karar (`proje.md` → Yayın etkisi). Betiğimiz bu
  yüzden **boş yetkiyle** basıyor (`file:///…`), yani kapı hiçbir zaman bir ad
  uyuşmasına bağlı değil — makine yeniden adlandırılınca sessizce kapanmıyor.
  *Bilinen sınır:* `file://$HOST$PWD` basan üçüncü taraf kancalar (oh-my-zsh'in
  `termsupport.zsh`'i) yoksayılıyor; kayıp yalnız komutun **ortasında** yapılan
  bir `cd`'nin canlı yansıması, dizin sonraki prompt'ta zaten geliyor.
  *Çare, istenirse:* yine bağımlılık değil politika — `bt-shell` (elinde `libc`
  var) adı okur ve `SessionOptions` ile geçirir (`decide_locale` emsali).
- **Taşma kuralı `bt-gpu/src/frame.rs`'te değil `bt-core/src/dock.rs`'te.**
  Bu bölüm onu renderer'a yazmıştı; deponun kendi kuralı (`dock.rs` modül
  başlığı: "karar burada, boyama orada"; `proje.md` → "Renderer'a terminal
  semantiği eklenmez") onu `bt-core`'a koyuyor — hangi yarının kısalacağı bir
  ürün kararı. **`frame.rs` hiç değişmedi:** `Frame::push_dock` satırı zaten
  hücreden okuyor, yani `row: 1` taşıyan hücreler bugünkü yolla çiziliyor.
  Değişen tek şey `DOCK_ROWS`'un doc'u (alt satır artık dolu).
- **Dizin ile dal `DockState`'in içine konamazdı**, yanına kondu
  (`DockContext`). İki sebep ve ikisi de yapısal: (1) `DockEvent::Update`
  tarayıcının hazırladığı kaydı `clone_from` ile toptan alıyor, yani **başka
  bir koldan** gelen dizin her tuş vuruşunda silinirdi; (2) `DockState::reset`
  `line-finish`'te koşuyor, yani Enter'a basıldığı anda bağlam kaybolurdu.
- **Dal aynanın kanalında ama aynanın durumunda değil.** `8133;b;{b64}`
  (`discussion.md` → "dalı aynanın kanalından gönderir"); `status`'a ve metne
  dokunmuyor, bozuk gövde `Unavailable` doğurmuyor — okunamayan bir dal giriş
  satırını ızgaraya geri göndermemeli. Kendi OSC numarasını hak etmiyor:
  dizinin aksine dal için bir sözleşme yok.
- **Bağlam satırı `Live` kapısının üstünde çiziliyor.** Altında kalsaydı komut
  koşarken (`Idle`) dizin kaybolurdu; kullanıcının ona en çok baktığı an o.
- **Ek kare isteği gerekmedi.** Doğrulandı: alacritty'nin `pty_read`'i
  `processed > 0` olan her turda `Event::Wakeup` yolluyor
  (`alacritty_terminal-0.26.0/src/event_loop.rs`), yani OSC 7 baytları tek
  başına kareyi getiriyor. "`cd` yapınca anında güncelleniyor" kabulü yapısal;
  buraya bir `request_frame` eklemek gereksiz olurdu.
- **Dal komutu tek fork:** `git rev-parse --abbrev-ref HEAD`; yanıtı `HEAD`
  ise (detached) kısa SHA için ikinci bir çağrı. Depo dışında da tek çağrı —
  `symbolic-ref` ile başlayan kol orada iki fork ederdi.
- **Betik sınaması `PWD`'yi ortamdan alamıyor** (zsh onu başlangıçta kendisi
  kuruyor): sınama geçici dizinleri **gerçekten yaratıp** `cd` ediyor. Taşınan
  baytlar da o adlarda (`a b`, `a%b`, `a;b`, `çığır`, `😀`).
- **İki zevk kararı, bir kez verildi:** bağlam satırı `TEXT_COL`'dan başlıyor
  (iki satır tek sol kenarı paylaşsın, `>` payda assın) ve yol **tam** kalıyor,
  `~` kısaltması yok (plan "tam klasör yolu" diyor; uzunluğu zaten soldan
  kısaltma taşıyor).
- **Kısaltma bileşen sınırına yaslanmıyor:** `…` + kuyruktan tam bütçe kadar
  karakter. Sınıra yaslamak sütun boşa bırakırdı; kazancı zevk, kaybı bilgi.
- **`/code-review` iki bulgu getirdi, ikisi de düzeltildi:**
  - *`b` kolunun alansız hâli aynayı düşürüyordu.* Kolun kendi doc'u "dalın
    bozukluğu aynayı düşürmez" diyordu ama `fields.next()` `None` verince
    `Malformed` dönüyordu — yani kesilmiş bir `ESC]8133;b BEL` giriş satırını
    dock'tan düşürüp ızgaraya geri gönderirdi. Artık gövdesiz `b` de "dal yok"
    demek; sınaması `a_broken_branch_never_drops_the_mirror` üç bozulma
    biçimini birden tutuyor.
  - *`anchor()`'ın doc bloğu `last_ink_in_row`'a geçmişti* (phase-4 fonksiyonu
    bloğun altına eklemiş). `anchor` doc'suz kalmış, `last_ink_in_row` da
    kendisini anlatmayan bir paragraf taşıyordu. Fonksiyon bloğun üstüne geri
    alındı; gövde birebir aynı (diff saf taşıma).
  - Üçüncü bulgu (`can_be_typed`'ın vi kipinde yapıştırmayı ham akıtması)
    **phase-5'in diff'inde** ve ölçülmüş bir ürün kararına dokunuyor; bu
    phase'e alınmadı, kullanıcıya soruldu.

## Checklist

- [x] OSC 7 bağlandı; yüzde çözme, yabancı host elenmesi, bozuk URI yoksayılıyor
- [x] Dal `precmd`'den geliyor; terminal `git` doğurmuyor
- [x] Depo değilse dal ve ayraç yok; detached HEAD'de kısa SHA
- [x] Alt satır çiziliyor: yol solda, ayraç, dal — yan yana, sönük
- [x] Taşmada yol **soldan** kısalıyor, dal kısalmıyor
- [x] Test: OSC 7 ayrıştırma (yüzde kodlu yol, yabancı host, bozuk URI)
- [x] Test: taşma kısaltması
- [x] Doğrulama geçti (`make hepsi` + `make kur`)
- [x] `make test-yaris` geçti — tarayıcı okuma yolunda; bu phase'i **riskli**
      yapan kapı (`proje.md` → Doğrulama)
- [x] `/code-review` koştu (riskli phase)
- [x] `make duman` geçti (kullanıcının penceresinde):
      `kare=29 hucre=8 glif=6 kural=15 icerik=2 hareket=27 sessiz=1749.36ms
      kapanis=clean`. Sözleşme değerleri (`hucre=8 glif=6 kural=15`) **aynı**:
      reçete `/bin/sh` koşuyor, yani dock almıyor (R5.1) ve bağlam satırı o
      koşuda hiç doğmuyor
- [x] Yayın etkisi yazıldı ("ölçüm bekliyor + araç da borç" dahil)
