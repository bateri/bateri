# Phase 2 — Blink

## Özet

İmleç yanıp sönsün — **sert** aç/kapa, saniyede iki kare, `bt-core`'a hiç
uğramayan hareket kareleriyle ve adlandırılmış bir durma koşuluyla. Saat
dördüncü bir kare sebebi doğurmuyor; **ikinci bir tat** kazanıyor.

_Requirements: R4, R4.2, R5, R6, R7, R7.1, R7.2, R7.3, R8, R9, R9.1, R9.2,
R10, R11.1, R12 (blink yarısı)_

## Değişiklikler

- **`crates/bt-core/src/settings.rs`** — `[terminal] cursor_blink`, üç değerli
  (`"auto" | "on" | "off"`), varsayılan **`"off"`**. Örüntü `cursor`'ın
  aynısı; bölüm zaten var. `TerminalOptions`'a ikinci alan, `Changes`'e
  dokunulmuyor.
- **`crates/bt-core/src/session.rs`** — `Cursor`'a `blink: bool`.
  - **`Term::cursor_style()` bu phase'de gerçekten çağrılıyor.** phase-1'in
    kullandığı `RenderableCursor` blink bitini **taşımıyor** (phase-1 →
    Uygulama Notları); şekil oradan gelmeye devam ediyor, blink buradan.
  - **Birleşme yeri burası** (R4.2): `"auto"` `term_config`'te çözülüyor
    (`default_cursor_style.blinking`, uygulama açıp kapatabiliyor), `"on"` ve
    `"off"` ise **ezme** — `\e[2 q` gelse de `"on"` yanar, `\e[5 q` gelse de
    `"off"` söner. Ezme `cursor_style()` okunduktan **sonra** uygulanmazsa
    `"on"` sessizce `"auto"`ya iner.
  - `Cursor::visible` yanlışsa blink'in anlamı yok; yüklem tek yerde kalsın.
- **`crates/bt-gpu/src/blink.rs` (yeni)** — saf, ObjC'siz, kilitsiz tip;
  emsali `Gate` ve `Motion`. Taşıdığı üç şey: fazın **mutlak son tarihi**
  (R8 — `dt` biriktirmek `DT_MAX` kırpmasıyla periyodu beşe böler ve arada
  birebir aynı kareler çizdirir), şu anki alfa (1 ya da 0) ve **son içerik
  karesinin damgası** (R9.2). Periyot seçilmiş bir sabit, `OMEGA` ve
  `FADE_DURATION` emsali — doc'unda "seçilmiş, ölçülmemiş" ve gerekçesi.
  - **Damga `last_frame_at` olamaz** (R9.2): o, hareket kolunun `Ok` dalında
    da yazılıyor ve blink kareleri oradan geçiyor — zaman aşımı hiç ateşlemez.
  - Durma koşulu fazı **"açık"a bırakır ve son bir kare çizdirir** (R9.1).
- **`crates/bt-gpu/src/link.rs`** — dört dokunuş:
  1. **`Waker::resume()`** — `wake()`'in üç işinden `dirty.mark()` çıkarılmış
     hâli; aynı kapı, aynı `pending` birleştirmesi, aynı dispatch.
     `requests` sayacını **artırmıyor** (o sayaç hareket karesini saymıyor).
  2. **Uyku testi üç soru soruyor** (R6): `motion.settled()` doğruysa bile
     bekleyen bir faz değişimi varsa uyunmuyor. Terim **tek atımlık** ve
     `settled()`'ın erken dönüşünden **önce** tüketiliyor — yoksa `resume()`
     kare üretmeyen bir uyan/uyu fırdöndüsü yaratır (`link.rs`'in bugünkü
     652–660 aralığı).
  3. **`arm_clock` son tarih tutuyor** (R7): içerik tiki bir deadline olarak
     saklanıyor, her uyku `min(içerik, bir sonraki faz) − now` kuruyor ve
     hangisinin dolduğuna göre `wake()` ya da `resume()` çağırıyor.
     `next_tick == None` saklanan deadline'ı **temizliyor** (R7.3).
  4. **Alfa çarpımı** iki çağrı yerinde: içerik karesi (`push_caret`) ve
     hareket karesi (`move_caret`), ikisi de `motion.alpha() * blink.alpha()`.
- **`crates/bt-gpu/src/motion.rs`** — Hareketi Azalt'ın cevabını link'e veren
  küçük bir erişimci (bugün `mode()` private). R10: indirgeme açıkken blink
  **kapalı**; yan kazanç yapısal, `alpha()` kanalına ikinci yazar doğmuyor.
- **Sözleşme ve belgeler** (R12): `link.rs` modül başlığı (saatin **iki
  tadı**), `Waker` ve `requests` doc'ları, `Counters::motion` doc'u,
  `Cursor::next_tick`'in "'Ne zaman' sorusunun cevabı burada" cümlesi
  (saat iki deadline'ı `min`'liyor, cümle "içeriğin ne zamanı"na daralıyor),
  `CLAUDE.md`'nin "boşta sıfır kare" maddesi, `docs/AYARLAR.md`.

## Kabul

- `[terminal] cursor_blink = "on"` ile imleç yanıp sönüyor; `"off"`'ta
  `\e[5 q` gönderen bir uygulama bile söndüremiyor; `"auto"`'da uygulamanın
  dediği oluyor.
- **Saniyede iki kare:** blink açıkken `icerik=` artmıyor, `Term` kilidine
  girilmiyor, ızgara yeniden taranmıyor.
- **013'ün sayacı blink açıkken doğru tikliyor** (R7.1) — bugünkü süre temelli
  kurulum onu sonsuza iterdi; bu setin doğurduğu en somut regresyon riski bu.
- Klavye sessizliğinden sonra blink **duruyor** ve imleç **görünür** kalıyor;
  ilk tuşta geri geliyor.
- Hareketi Azalt açıkken blink hiç başlamıyor.
- `make duman` yeşil: hermetik koşu ayar okumuyor ve reçete `/bin/sh` koşup
  DECSCUSR göndermiyor, yani varsayılan kapalıyken saat hiç armed olmuyor.

## Uygulama Notları

- **`last_cursor` ivar'ı öldü.** Tek okuyucusu `arm_clock`'tı; saat son tarihe
  geçince okuyanı kalmadı ve kaldırıldı. Bırakılsaydı aynı gerçeğin ikinci
  kopyası olurdu — 013'ün kapısında tam bu sebeple bir alan silinmişti.
- **Saatin seçim mantığı saf bir fonksiyona çıkarıldı** (`due_clock`), çünkü
  sınanamıyordu: `arm_clock` ObjC sınıfına bağlı ve gerçek bir pencere
  istiyor. 013'ün regresyon bekçisi (`a_blinking_cursor_does_not_starve_the_
  duration_counter`) ancak bu ayrımdan sonra yazılabildi.
- **Hareketi Azalt için `mode()` değil ham bayrak** (`Motion::reduce`):
  `Snap` stilinde `mode()` `Fade` dönmüyor ama indirgeme yine açık ve blink
  yine kapanmalı.
- **`Event::CursorBlinkingChange`'e dokunulmadı ve gerek de yok.** DECSCUSR
  baytları PTY'den geliyor, alacritty onları işleyince `Event::Wakeup`
  yolluyor ve hasar bayrağını diken o (`AdapterInner`'ın doc'u) — yani başka
  çıktı üretmeyen bir `\e[5 q` bile bir içerik karesi doğuruyor ve
  `cursor_style()` orada yeniden okunuyor. `discussion.md` bunu zaten
  "gereklilik değil doğrulama kalemi" diye kaydetmişti.
- **Phase dışı, ama phase'in kapısını tıkıyordu:** `make test-yaris` bu
  phase'den **önce** kırmızıydı ve sebebi blink değildi. `git bisect` ile
  bulundu (`3273647`, 012): `race_dock_state_and_frame` dock'u olmayan bir
  oturumdan dock caret'i bekliyordu. Kendi commit'iyle düzeltildi
  (`ccadf55`), phase'in commit'ine karışmadı.

### Kapı (`/code-review`, riskli phase)

**11 bulgu, 11'i düzeltildi.** Dördü gerçek davranış kusuruydu ve hiçbirini
bir sayaç görmezdi:

- **Ayar pencere doğarken yutuluyordu.** `Adapter::new` blink'i varsayılanında
  kuruyor, `Session::spawn` onu hiç yazmıyordu; tek yazıcı
  `set_terminal_options` olduğu için özellik **her açılışta ölü** kalıyor,
  kullanıcı ayar dosyasını alakasız bir sebeple yeniden kaydedince
  "kendiliğinden düzeliyordu". Tema zaten `Adapter::new`'dan geçiyordu; blink
  de oradan geçiyor artık. Bekçisi `the_blink_setting_reaches_the_first_frame`.
- **Akan çıktıda faz donuyordu.** `advance` yalnız "hasar yok" dalından
  çağrılıyordu, yani `cargo build` gibi sürekli çıktıda faz hiç ilerlemiyor —
  ve **sönük fazda** donabiliyordu: caret çıktı boyunca görünmez kalırdı.
  İçerik dalı da fazı ilerletiyor artık (dönen değer atılıyor, kare zaten
  çiziliyor).
- **Gizli imleç saati açık tutuyordu.** `\e[?25l` gönderen bir TUI'de hiçbir
  caret çizilmiyor ama blink armed kalıyor, pencere saniyede iki kez uyanıp
  **birebir aynı** kareyi çiziyordu; TUI'nin kendi çıktısı `IDLE_STOP`'u da
  tazelediği için sızıntı sınırsızdı. Ölçüt `visible` **olamaz** (dock devrinde
  `false`, ona bakmak dock'ta blink'i öldürürdü): `cursor_visible ||
  caret_in_dock`. Bekçisi `a_hidden_cursor_does_not_blink`.
- **Hareketi Azalt boştaki blink'i durdurmuyordu.** `set_reduce_motion` yalnız
  yarıda kalan bir animasyon varsa kare istiyordu; blink `Motion`'ın dışında
  yaşadığı ve kapısı yalnız **içerik** karesinde okunduğu için boştaki pencerede
  hiç uygulanmıyordu. Artık değişimin kendisi kare istiyor.

Kalanlar: `Blink::default()`'in `lit`'i kendi değişmezini çiğniyordu (elle
yazıldı), iki sınama boş iddia taşıyordu (`content_deadline` saf yardımcıya
çıkarıldı; `term_config` fixture'ı `CursorBlink::On` aldı), üç bayat doc
(`settled()`'ın doc'u `reduce()`'a kaymıştı, `content_deadline` ölü
`last_cursor`'ın doc'unu miras almıştı, `last_caret_text` hâlâ ona atıf
yapıyordu) ve faz karesinden sonra link'in bir vsync fazladan açık kalması.

## Yayın Etkisi

- **ayar şeması** — `[terminal] cursor_blink` eklendi; silinen anahtar yok.
  `docs/AYARLAR.md` ve `Settings::TEMPLATE` birlikte.
- **`CLAUDE.md`** — "boşta sıfır kare" maddesi: saatin iki tadı, blink'in
  durma koşulu ve "blink'i açık olan pencere boşta değildir".
- **belge** — `bt-gpu::link` modül başlığı sözleşmenin kendisi; kodla aynı
  commit'te.
- shader / terminfo / app bundle / shell entegrasyonu / yeni bağımlılık: yok.
- **ölçüm bekliyor:** "hareket tadının içerik tadından ucuzluğu" — yönü koddan
  kanıtlı (`Term` kilidi yok, ızgara taraması yok, `bt-core` yolculuğu yok),
  **büyüklüğü ölçülmedi**.
- **Bilinen sınır, jeton eklenmiyor** (plan → Karar 6): varsayılan kapalıyken
  kapının hiçbir katı bozuk bir blink'i görmüyor — koruma bir jeton değil
  varsayılanın kendisi. `docs/YOL-HARITASI.md`'deki borcun kapsamı büyüyor
  ("meşru periyodik kare"), vadesi gelmiyor.

## Checklist

- [x] `bt-core`: `[terminal] cursor_blink` (üç değerli, varsayılan `"off"`),
      `TerminalOptions` ikinci alan
- [x] `bt-core`: `Cursor.blink` — `Term::cursor_style()` + `"on"`/`"off"`
      ezmesi `frame()`'de
- [x] `bt-gpu`: `blink.rs` — mutlak son tarih, alfa, kendi içerik damgası
- [x] `bt-gpu`: `Waker::resume()` (hasar dikmiyor, `requests` artmıyor)
- [x] `bt-gpu`: uyku testinin üçüncü sorusu — tek atımlık, `settled()`'dan önce
- [x] `bt-gpu`: `arm_clock` son tarih + `min` + `None` temizliği
- [x] `bt-gpu`: alfa çarpımı iki çağrı yerinde; Hareketi Azalt kapısı
- [x] Test: faz mutlak son tarihten geliyor (uzun uykuda tek adımda dönüyor)
- [x] Test: **013'ün sayacı blink açıkken tikliyor** (regresyon bekçisi)
- [x] Test: `None` deadline'ı temizliyor; durma fazı "açık"a bırakıyor
- [x] Test: `cursor_blink` round-trip, `"on"`/`"off"` ezmesi, tanınmayan değer
- [x] Sözleşme ve belgeler (`link.rs` başlığı, `CLAUDE.md`, `docs/AYARLAR.md`,
      `Counters::motion`, `Waker`/`requests`, `Cursor::next_tick`)
- [x] Doğrulama geçti (`make hepsi`)
- [x] `make test-yaris` (paylaşılan durum: `Waker`'a ikinci giriş noktası)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi
- [x] `make duman` — kullanıcı koştu (2026-09-19), yeşil:
      `kare=29 hucre=8 glif=6 kural=15 icerik=2 hareket=27 sessiz=1755.67ms
      kapanis=clean`. `icerik` blink'ten önceki değerinde ve `sessiz` tabanın
      iki katı: saat hermetik koşuda hiç kurulmuyor.
- [x] Yayın etkisi yazıldı
