# Grapheme dizileri — Tartışma

Karar-listesi biçimi. Karar 1 setin omurgası (üç yol), kalanlar onun
üstünde.

## Karar 1: Dizi ızgarada kaç sütun tutar?

Ölçüm (`context.md`) ayrımı veriyor: bayrakta ızgaranın sütunu
uygulamanınkiyle aynı, ZWJ/ten rengi/VS16'da değil.

### Seçenek A: Yalnız çizim — ızgara bugünkü gibi, bayrak çifti çizimde birleşir

Izgaraya dokunulmaz. `frame()` ardışık iki dar RI hücresini (çiftleme
koşunun başından) tek iki hücrelik glyph olarak verir; aynı hücredeki
`zerowidth` (VS16, aksan) çizime katılır. Atlas dizi anahtarı ve
şekillendirme kazanır.

**Artıları:**
- Okuyucu döngüye, `Term`'e, dock aritmetiğine dokunmuyor; bildirilen
  belirtiyi (bayrak) tam onarıyor.
- wcwidth sayan uygulamalarla (zsh, vim) ızgara bugünkü gibi uyumlu.

**Eksileri:**
- Kalan üç aileyi onaramıyor: `❤️` tek sütunda (iki hücrelik glyph
  sığmaz — bugünkü metin sunumu kalır), `👍🏽` dört, aile altı sütun (tek
  glyph çizilirse arkasında 2–4 sütunluk delik; çizilmezse bugünkü parçalar).
- Claude Code gibi dizi sayan uygulamaların satırı bu dizilerde kaymaya
  devam ediyor. Talebin yarısı "adıyla yazılı sınır" olarak kalır.
- Seçim bayrağı ortasından bölebilir (iki ayrı hücre).

### Seçenek B: Izgara diziyi kümeler — okuyucu döngünün sahibi `bt-core`

`bt-core` okuyucu döngüyü kendisi koşturur (alacritty'nin `event_loop.rs`'i
kapsüllenip uyarlanır; `polling`, `EventedReadWrite`, `TappedPty` zaten
`bt-core`'da) ve ayrıştırıcıya `Term`'i doğrudan değil, `Handler`'ı
`Term`'e **aktaran** ince bir sarmalayıcıyla verir. Sarmalayıcının tek
farkı `input`: gelen kod noktası açık kümeyi **uzatıyorsa** önceki kümenin
baş hücresinin `zerowidth`'ine iner (RI'nin ikincisi, ten rengi, ZWJ'den
sonraki emoji dahil); küme genişleyince (`❤` + `FE0F`: 1 → 2, RI çifti)
baş hücre geniş hücreye çevrilir. Başka her `Handler` çağrısı açık kümeyi
kapatır. Dizi böylece **bir** geniş hücre + spacer olur, sütunu
uygulamanınkiyle aynı.

"Uzatır mı" sorusunun ve kümenin genişliğinin tek yetkilisi
`unicode-width` (Karar 3). Dock'un sütun aritmetiği ve tazelik kapısı aynı
kümelemeden geçer (Karar 5).

**Artıları:**
- Dört aile de tek glyph ve uygulamanın saydığı sütunla: Claude Code'un
  satırları hizalanıyor, `❤️` iki hücrede renkli, aile iki sütun.
- Çizim tarafı sadeleşiyor: küme **tek hücrede** (`c` + `zerowidth`), yani
  sink komşu hücreye bakmıyor; seçim ve kopya kümeyi bölemiyor (alacritty
  satır metnine `zerowidth`'i zaten katıyor).
- Modern terminallerin (kitty, WezTerm, Ghostty, iTerm2, Terminal.app)
  varsayılanı; referans ürün de dizileri yan tabloda tutuyor.

**Eksileri:**
- Okuyucu döngü `bt-core`'a geçiyor: ~490 satırlık bir kopya, kapanış
  sırası (`Session::shutdown`, `(EventLoop, State)` çifti) ve yarış
  sınamalarıyla yeniden bağlanmalı. `Handler`'ın 66 metodu aktarılmalı —
  unutulan bir metot sessiz bir özellik kaybı.
- wcwidth sayan uygulamalar (vim, less, tmux'un eski sürümleri) bu
  dizilerde artık **onlar** kayıyor. Dock'lu zsh'te giriş satırı zaten
  dock'ta çiziliyor; ızgarada kalan zsh satırında (kabul edilen komut) zsh'in
  saydığı sütun ile ızgaranınki ayrışabilir.
- Arama (`RegexIter`) `zerowidth`'i görmüyor: `🇹🇷` aramak bugün iki hücrede
  eşleşiyor, B'den sonra eşleşmez (aksanlı `é`'nin bugünkü hâliyle aynı).

### Seçenek C: `alacritty_terminal`'ı yamalamak (`[patch.crates-io]` / çatal)

`Term::input`'u kaynağında değiştirmek.

**Artıları:**
- Okuyucu döngü ve `Handler` aktarımı gerekmiyor; değişiklik en yerinde.

**Eksileri:**
- Bağımlılık kararı ve kalıcı bakım: her alacritty sürümünde yamanın
  yeniden uygulanması. `CLAUDE.md`'nin "kapsüller, kendi ayrıştırıcımızı
  yazmıyoruz" kuralının ruhuna B'den uzak — B ayrıştırıcıya dokunmuyor,
  yalnız `input`'u süzüyor.

**Öneri: B.** A bildirilen belirtiyi onarır ama talebin kendisini ("dizileri
tek glyph") dört aileden birinde karşılar ve kalan üçünde çizilecek glyph'in
sığacağı sütun yok; boşluk kullanıcı lehine okunur (`CLAUDE.md` → İş akışı).
C, B'nin kazancını bakım borcuyla alıyor.

## Karar 2: Kümeleme koşulsuz mu, DECSET 2027 ile mi?

Ghostty/foot/contour kümelemeyi mod 2027'ye bağlıyor (uygulama ister),
kitty/WezTerm koşulsuz. Bildiren uygulama (Claude Code, `string-width`)
mod pazarlığı yapmadan diziyi 2 sayıyor; 2027'ye bağlamak belirtiyi
onarmaz. Öneri: **koşulsuz**, DECRQM cevabı bu sette yok.

## Karar 3: "Uzatır mı" kuralı nereden — yeni crate mı?

- **3A — `unicode-segmentation`** (UAX #29). Grafta **yok**; yeni bağımlılık.
  Sınırı tam biliyor ama genişliği bilmiyor — genişlik yine
  `unicode-width`'ten, yani iki tablo ve ayrışma riski (024 Karar 1'in
  reddettiği şey).
- **3B — `unicode-width`'in kendisi.** Kural: kod noktası açık kümeyi
  uzatır ⇔ genişliği `0` **ya da** `width(küme ++ c) < width(küme) +
  width(c)` (dizi tablosu onu yuttu) **ya da** eşlenmemiş bir RI'nin
  arkasındaki RI **ya da** `ZWJ`'nin arkasında emoji sunumu alabilen kod
  noktası (`width(c ++ FE0F) == 2`, UAX #29 GB11'in karşılığı). Dördüncü kol
  ölçümden: onsuz `🧑🏻‍❤️‍💋‍🧑🏼` ikiye bölünüyordu, çünkü `❤` VS16'sız metin
  sunumlu ve ara dizgi tabloda yok. Scratchpad sınamasında 17 örneğin 17'si
  doğru: `👨‍👩‍👧`, `👍🏽`, `❤️`, `🏳️‍🌈`, `1️⃣`, İskoç bayrağı (etiket dizisi),
  on kod noktalı öpücük birer küme ve 2 sütun; `🇹🇷🇬` = çift + tek RI;
  `a🏽` = iki küme (1 + 2); `👨‍a` ve `a‍b`'de `a`/`b` ayrı küme; `é` tek
  küme 1 sütun. Küme genişliği `UnicodeWidthStr::width(küme)`. Tablo ızgaranın, dock'un ve
  tazelik kapısının **zaten** paylaştığı tek yetkili.
- Öneri **3B**: yeni crate yok, ikinci tablo yok. Sınırı: UAX #29'un emoji
  dışı kümeleri (Hangul jamo dizileri, Hint yazılarının SpacingMark'ları)
  bugünkü gibi karakter karakter kalır — adıyla yazılır.

## Karar 4: Küme sınırdan ve atlasa nasıl geçer?

`Sprite` `Copy + Hash` ve `bt-atlas` `bt-core`'u görmüyor; sınır `Cell`'i
kare başına maliyet (72 bayt, `session.rs`'in doc'u).

- **4A — satır içi küçük dizgi** `Cell`'de (RGI'nin en uzunu 10 kod
  noktası, 35 bayt UTF-8): her çizilen hücre ~40 bayt büyür, kümesiz
  hücreler de öder.
- **4B — kare başına yan tablo**: `frame()`'in `&mut` tamponlarının
  yanında (`SelectionRuns` emsali) bir küme tablosu; `Cell` yalnız
  `Option<küme indeksi>` (4 bayt niche) taşır, taban `ch` aynen kalır. Atlas
  dizgiyi **kendi** tablosunda interner'la tutup `Sprite::Cluster(u32)`
  anahtarı verir — `Sprite` `Copy` kalır, dizgi sahipliği iki uçta yerel.
- **4C — oturum ömürlü interner `bt-core`'da**: kimlik kararlar arası
  kararlı ama sonsuz büyüyor ve `bt-gpu`'nun dizgiyi okumak için bir kilide
  ihtiyacı var.
- Öneri **4B**.

## Karar 5: Dock, tazelik ve seçim aynı kümelemeden mi?

Karar 1B'den sonra zorunlu (gözlem, seçim değil): ızgara aileyi 2 sütun,
`dock::layout_with` 6 sütun sayarsa bastırmanın aralığı (`dock::grid_span`)
ile ızgara ayrışır (032'nin "iki aritmetik ayrışır" belirtisi); tazelik
kapısı aynanın son mürekkebini (`🇷`) ızgaranın baş hücresiyle (`🇹`)
karşılaştırır ve kalıcı olarak "bayat" der — 024'ün belirtisi, satır her
tuşta ızgaraya fırlar. Öneri: tek küme yürüyüşü `bt-core`'da (Karar 3'ün
fonksiyonu), `layout_with` kümeyle ilerler, tazelik kapısı son kümenin
**baş** karakterini karşılaştırır; dock seçimi, `d;S;E` silme aralığı ve
yazım efektlerinin farkı küme sınırına hizalanır (yarım bayrak silinmez).

## Karar 6: Kapsam — 78 tek sütunlu emoji, tek RI, bağlam satırı

- **VS16'lı 78** (`🌡️ 🎙️ 🏔️`): Karar 3B'nin kuralıyla küme 2 sütun →
  **bu setle bedava çizilir** (context'teki şekillendirme ölçümü: tek glyph).
- **Çıplak 78** (`🌡`): uygulama da 1 sütun sayıyor; iki hücrelik mürekkep
  tek sütuna ancak küçültmeyle sığar ve o 023'ün yazdığı ayrı karar
  (yol haritasının küçültme kalemi). **Kapsam dışı**, bugünkü kutu kalır.
- **Eşlenmemiş tek RI**: 1 sütun, glyph'i hücreye sığmıyor → kutu kalır;
  uygulama da 1 sayıyor ve Unicode'da tek başına bir anlamı yok.
- **Dock'un bağlam satırı** (küçük sınıf): 021/024 emsaliyle karakter
  biriminde kalır, küme çizmez — adıyla yazılı sınır.
- **Arama**: kümenin `zerowidth`'i aranmıyor (alacritty'nin `RegexIter`'ı) —
  bayrak ve ailenin ikinci kod noktası aranamaz; `é`'nin bugünkü sınırı.

## Karar 7: Dock'ta ⌫ / ⌦ / ← / → kümeyi bölmesin

Gözlem: düz ⌫/⌦/←/→ zsh'e gidiyor (`keys::encode_key`) ve ZLE kod noktası
kod noktası yürüyor — `🇹🇷`'de ⌫ yalnız `🇷`'yi siler (kalan tek RI kutu),
← `CURSOR`'u iki RI'nin arasına koyar. Beklenen macOS metin alanının
davranışı: küme bölünmez. Öneri: **düzenleme kapısı açıkken**
(`Session::can_edit_dock`) caret'in bitişiğindeki küme birden çok kod
noktalıysa bu dört tuş `keys::dock_key` → `Session::dock_key` kolundan
bugünkü widget komutuna gider (`d;S;E;L`: ⌫/⌦ kümenin `[S,E)`'si, ←/→
`S == E` kümenin sınırı) — kabuğa yeni bir yüzey yok, widget ikisini de
zaten yapıyor. Kapı kapalıyken (`vicmd`, bağlamasız kabuk) davranış kod
noktası birimi kalır. Caret yine de bir kümenin içine düşerse (kapı
kapalı, ya da zsh oraya koydu) **kümenin başında** çizilir: ← ile sondan
bir adım geri gelen caret görünür biçimde yer değiştirsin.

## Muhakeme (2026-09-25)

Panel `/rfc` adım 6'nın pahalı karar sınıfından koştu: seçim her karede CPU
hesabına (`Session::frame`, `bt-gpu`'nun encode'u), sınır hücresine ve
`Cargo.toml`'a (sürüm sabitleme, feature) dokunuyor. Üç jüri opus.

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU |
| Codebase-fit | SORUNLU |
| İşletme | SORUNLU |

Üçü de B'yi doğru omurga buluyor (okuyucu döngünün sahipliği kanıtlı:
`event_loop.rs` `Term`'i somut tipte ayrıştırıcıya veriyor); itirazlar B'nin
**nasıl** yazıldığına.

**Kabul edilen itirazlar → plan değişikliği:**

- **3B'nin genel "toplam küçüldü" kolu emoji'ye sınırlı değil** (Sadelik;
  scratchpad'de doğrulandı: Arapça `لا` 2 → 1 tek küme, `⌚︎` 2 → 1 tek küme).
  Arapça metin wcwidth sayan kabukla ayrışır, `⌚︎` geniş hücreyi daraltma
  yolu ister. → Kural **yalnız emoji kollarından**: (1) sıfır genişlik —
  alacritty'nin bugünkü dalı; (2) eşlenmemiş RI'nin arkasındaki RI; (3) ten
  rengi (`U+1F3FB..1F3FF`) **iki sütunlu** kümenin arkasında; (4) `ZWJ`'nin
  arkasında emoji sunumu alabilen kod noktası (`width(c ++ FE0F) == 2`).
  Genişlik yine `UnicodeWidthStr::width(küme)` ve ızgara yalnız **1 → 2**
  genişletir, hiç daraltmaz (VS15 bugünkü gibi `zerowidth`'e iner, hücre
  dar kalır). Emoji dışı kümeleme (Arapça, İbranice, Lisu…) bu setin
  talebi değil; bugünkü davranış aynen.
- **"Baş hücre geniş hücreye çevrilir" `Term::input`'un özel yollarını
  yeniden yazdırırdı** (Sadelik + Codebase-fit; `write_at_cursor`,
  `wrapline` özel — `term/mod.rs:958, 984`). → Genişleme alacritty'nin kendi
  geniş yolundan: imleç baş hücreye geri alınır (`input_needs_wrap = false`),
  hücre temizlenir, `Term::input(yer tutucu geniş karakter)` çağrılır —
  satır sonu, `LEADING_WIDE_CHAR_SPACER`, bölge kaydırması ve IRM
  alacritty'de kalır — ardından yazılan hücrenin `c`'si taban karakter,
  `zerowidth`'i kümenin kalanı yapılır. Sınamalar son sütunu, IRM'yi ve
  kaydırma bölgesinin dibini kapsar.
- **Açık kümenin konumu saklanmaz, ızgaradan türetilir** (Codebase-fit;
  İşletme'nin yarış senaryosu). Okumalar parça parça gelir ve arada ana
  thread `Term`'i değiştirir (resize, 034'ün temizlemeleri). → Baş hücre her
  çağrıda alacritty'nin `zerowidth` dalının yöntemiyle bulunur (imleç − 1,
  spacer'dan geri); RI eşlenmişliği de hücreden (`c` RI ve `zerowidth`'te RI
  yok). Sarmalayıcının tek durumu "son `Handler` çağrısı `input` mıydı"
  biti. Açıklık bugünkü `zerowidth` yolunun açıklığıyla aynı, yeni bir yarış
  doğmuyor; bekçisi resize'la yarışan bir `race_*` sınaması.
- **`Handler` aktarımının bekçisi** (üç jüri): vte'nin her metodu boş
  varsayılanlı. → Aktarımlar tek `macro_rules!` listesinden ve `impl`'in
  üstünde `#[deny(clippy::missing_trait_methods)]` (clippy'nin restriction
  lint'i): trait'in açıkça yazılmamış her metodu `make hepsi`'de kırmızı —
  bekçi mekanik, sayı tutan bir sınama değil; `alacritty_terminal`
  `=0.26.0`'a sabitlenir (bugün `"0.26"` — `cargo update` kopyanın altından
  döngüyü ve vte'yi kaydırırdı; `Cargo.lock` değişmez).
- **Kopya döngünün sessiz yolları** (İşletme): DEC 2026 zaman aşımındaki
  `stop_sync` da sarmalayıcıdan geçer (Claude Code senkron çıktı
  kullanıyor), `Wakeup` kuralı (`sync_bytes_count() < processed`) aynen
  kopyalanır, kopya dosyaya Apache-2.0 §4(b) bildirimi girer, `TappedPty`
  kalır ve modül başlığının kilit sırası (`_terminal_lease`) ile `TappedPty`
  / `Reader` doc'ları **döngüyü taşıyan phase'de** yeniden yazılır.
- **Yan tablonun sahibi `bt-gpu`** (Codebase-fit): listeler hareket
  karelerinde yeniden kullanılıyor, doldurma bandının `Vec<Cell>`'i
  kareler arası tamponlanıyor. → Küme tablosu listelerle birlikte yaşayan
  ve temizlenen bir `bt-gpu` tamponu (`SelectionRuns` emsali: `frame()`,
  `dock()` ve doldurma sink'i doldurur); atlas interning'i yalnız
  `prepare`/`prepare_fx`/`fan`'da (sink atlası ödünç alamıyor — 023).
  Dört tüketici: ızgara, bant, dock, yazım efektleri.
- **Sıra: görünür tutarsız ara hâl yok** (İşletme). Kümeleme açılıp çizim
  sonraki phase'e kalırsa `👍🏽` tenini, aile iki kişisini kaybeder; dock
  düzeni kümeyle, seçim kod noktasıyla yürürse 032'nin "iki aritmetik"
  belirtisi doğar. → Kümeleme bir **oturum seçeneği** (`SessionOptions`,
  varsayılan kapalı) ve bütün tüketicileri — sarmalayıcı, dock düzeni,
  tazelik, seçim/silme/fark, çizim — aynı bayrağı okur; phase'ler onu
  sınamalarda açar, son phase varsayılanı çevirir. Geri alma o tek satır.

**Reddedilenler:**

- **"Mutasyon nesli" doğrulama kuralı** (İşletme) — baş hücre ızgaradan
  türetildiği için saklanan bir konum yok ve nesle gerek kalmıyor; nesil
  açıklığı kapatmaz, yalnız bugünkü `zerowidth` yolunun açıklığını ikinci
  bir mekanizmayla tekrarlar.
- **Sınamanın vte kaynağındaki `fn` satırlarını sayması** — kaynak dosyayı
  okuyan bir sınama kırılgan; `missing_trait_methods` aynı soruyu
  derleyicinin trait bilgisiyle soruyor.

## Karar (2026-09-25, kullanıcı onayı)

Panelden geçmiş öneri; `/akis` sürücüsü A (yalnız bayrak), B ve "önce A
sonra B" seçeneklerini bedelleriyle — okuyucu döngünün `bt-core`'a geçmesi,
wcwidth sayan uygulamaların kayması, ⌘F'nin diziyi bulmaması — kullanıcıya
sordu ve kullanıcı **B**'yi seçti. Aşağıdaki iki ürün etkisi kararın
**bedeli** olarak yazılı.

- **Seçilen — Karar 1B: ızgara emoji dizisini tek hücrede kümeler.**
  `bt-core` okuyucu döngünün sahibi olur (alacritty'nin döngüsünün
  kapsüllenmiş kopyası, `TappedPty` aynen), ayrıştırıcı `Term`'i
  `Handler`'ı aktaran bir sarmalayıcıdan görür ve yalnız `input` farklıdır;
  dizi bir geniş hücre + `zerowidth` olur ve uygulamanın saydığı sütunu
  tutar. Muhakeme'nin kabul edilen maddeleri kararın parçası: emoji kolları
  (dört kol, yalnız 1 → 2 genişleme), genişlemenin `Term::input` üstünden
  yer tutucuyla yapılması, konumun ızgaradan türetilmesi, makro + lint
  bekçisi, `=0.26.0`, `stop_sync`, oturum seçeneğiyle aşamalı açılış.
  Gerekçe: talep dört aile ve üçünde çizilecek glyph'in sığacağı sütun
  ancak ızgarada doğuyor; boşluk kullanıcı lehine okunur.
- **Seçilen — Karar 2: koşulsuz**, DECSET 2027 / DECRQM yok. Bildiren
  uygulama mod pazarlığı yapmıyor.
- **Seçilen — Karar 3B (daraltılmış): kural `unicode-width`'ten**, yeni crate
  yok. Tek saf fonksiyon `bt-core`'da (açık kümeyi uzatır mı + küme
  genişliği) ve bir dizgi üstünde kümeleri veren yürüyüş onun üstünde;
  sarmalayıcı, dock düzeni, bastırmanın ızgara yürüyüşü ve tazelik kapısı
  **yalnız** onu çağırır.
- **Seçilen — Karar 4B: küme yan tablosu `bt-gpu`'nun**, listelerle yaşar;
  sınır `Cell`'i `Option` bir küme indeksi taşır (taban `ch` aynen), atlas
  dizgiyi kendi tablosunda interner'la tutar ve `Sprite::Cluster(u32)`
  verir; şekillendirme `CTLine` (mevcut crate'lere feature). Küme glyph'i
  tek glyph'e şekillenmezse ya da kapıdan dönerse **taban karakterin**
  glyph'i çizilir — kutu ya da tam glyph sözleşmesi korunuyor.
- **Seçilen — Karar 5: dock, tazelik, seçim ve düzenleme aynı kümelemeden.**
  Tazelik son kümenin **baş** karakterini karşılaştırır; dock seçimi, `d;S;E`
  aralığı ve yazım efektlerinin farkı küme sınırına hizalı.
- **Seçilen — Karar 6 kapsamı**: VS16'lı 78 bedava çiziliyor; çıplak 78,
  tek RI, bağlam satırı ve emoji dışı kümeler kapsam dışı. Çizimde küme
  yolu yalnız **geniş** (iki sütunlu, birden çok kod noktalı) kümeye
  açılıyor: tek sütunlu birleştirici taşıyan hücre (`é`, Arapça hareke,
  Devanagari, `⌚︎`) bugünkü gibi taban karakterle çiziliyor — aksanı
  çizmek ayrı bir istek, atlas kapasitesini ve bugünkü metin çizimini
  değiştirir; bu set sormadan açmıyor.
- **Seçilen — Karar 7:** kapı açıkken dört tuş kümeyi widget'la bütün
  yürütür; kapı kapalıyken kod noktası birimi, caret kümenin başında.
- **Reddedilen — 1A (yalnız çizim):** bayrağı onarır, `❤️ 👍🏽 👨‍👩‍👧`'yi
  onaramaz ve Claude Code'un satır kaymasını bırakır.
- **Reddedilen — 1C (alacritty yaması):** B'nin kazancını kalıcı bakım
  borcuyla alıyor.
- **Reddedilen — 3A (`unicode-segmentation`):** yeni crate ve ikinci tablo;
  sınırı genişlikten ayrı bilen bir kaynak 024 Karar 1'in reddettiği ayrışma.
- **Reddedilen — 4A / 4C:** 4A her çizilen hücreye ~40 bayt, 4C sonsuz
  büyüyen ve kilit isteyen bir interner.

**Kararın kullanıcıya görünen bedeli (ürün etkisi):**

1. **wcwidth sayan uygulamalar bu dizilerde kayar** (vim, less, eski tmux;
   ızgarada kalan zsh satırı — `blocks` kademesi ve kabul edilmiş komut).
   Bugün kayan taraf Claude Code gibi dizi sayan uygulamalar; karar kaymayı
   modern terminallerin tarafına alıyor.
2. **`🇹🇷`, `👍🏽` ⌘F'te dizi olarak bulunmaz** (taban karakteriyle —
   `🇹`, `👍` — bulunur) — arama kümenin ikinci kod noktasını görmüyor (aksanlı `é`'nin bugünkü sınırı). Bugün bayrak iki hücrede
   aranabiliyor.
3. **zsh'in kendi aritmetiği (wcwidth) üçüncü bir yürüyüş.** Dock'ta
   `👍🏽`'lı bir satır tam sarma sınırına düşerse zsh satırı ızgaradan
   farklı yerde sarar; bastırmanın aralığı (`grid_span`) bir satır eksik ya
   da fazla kalabilir. Bayrak etkilenmiyor (wcwidth de 2 diyor). Phase 3
   bunu bir sınamayla ölçer; kapanmıyorsa "dock'ta çok sütunlu diziyle
   sarılan satır" adıyla bilinen sınır.
