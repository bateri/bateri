# Günlük kullanım eşiği — Doğrulama & Yayın

> İlgili: [plan.md](plan.md) · [discussion.md](discussion.md) ·
> [phase-1.md](phase-1.md) · [phase-2.md](phase-2.md) · [phase-3.md](phase-3.md) ·
> [phase-3b.md](phase-3b.md) · [phase-4.md](phase-4.md) · [phase-4b.md](phase-4b.md) ·
> [phase-4c.md](phase-4c.md) · [phase-4d.md](phase-4d.md) · [phase-5.md](phase-5.md)

bateri artık kopyalanıp yapıştırılabilen, seçilip kaydırılabilen ve Dock'tan
açılan bir terminal: fareyle seçilen metin Cmd-C ile panoya gider, Cmd-V
uygulamanın bracketed paste isteğini (DECSET 2004) sorarak yapıştırır;
tekerlek, trackpad ve Shift+PgUp geçmişe kaydırır, tam ekran uygulamada
tekerlek uygulamaya ok ya da fare tekerlek raporu olarak gider; `make kur`
imzasız bir `bateri.app` kurar ve `alacritty_terminal`'ın Apache-2.0 atfını
pakete koyar; kabuk her açılışta ev dizininde ve UTF-8 yereliyle başlar.
`IDLE_FRAME_LIMIT` görünür pencerede yeniden ölçüldü ve değişmedi; bu ölçüm
`docs/OLCUMLER.md`'yi kurdu. Kullanıcının makinesinde taşınacak ayar, tema ya
da terminfo yok; `TERM` değişmedi, `Cargo.lock` oynamadı. Dışarıya etki
**kullanıcıya görünen davranış değişiklikleri** (B'nin başında), **gerçek
olan bir `make` hedefi ve paketi** ve **iş akışı belgelerindeki ölçüm
sahipliği düzeni**. Eşiğin dürüst sınırı: yerel `make kur` kopyası
Gatekeeper'a hiç takılmıyor (quarantine yok); indirilen bir kopyadaki
davranış doğrulanmadı (`plan.md` → Hedef).

## A. Doğrulama (ÖNCELİK)

```sh
make hepsi        # rustc sürümü + fmt --check + clippy -D warnings + test
make test-yaris   # phase-3, 3b ve 4d Term kilidinin yollarını değiştirdi
make duman        # pencereyi açan davranış (fare, tuş, kaydırma) değişti
make kur          # assets/bundle, crates/bateri ve Makefile'ın kur hedefi değişti
```

`make shader` ve `make terminfo` **gerekmez**: `.metal`, `build.rs` ve
`assets/terminfo` bu setin 37 commit'inde el değmedi (`git diff --stat
b9944de..HEAD` bu yollarda boş). `/ship` Bölüm A'yı push'tan önce yeniden
koşturur; aşağıdaki kutular son kod üstündeki kayıtlı koşulardan.

### Beklenen çıktı

- `make hepsi` → exit 0. `make test-yaris` → exit 0 (iki zamanlama profili).
- `make duman` → `kare=N hucre=8 glif=6 kural=15 yuva=U/T yuk=smoke istek=I
  kapanis=clean profil=debug ornek=off pipeline=ok`, çıkış 0; `kare`
  `IDLE_FRAME_LIMIT`'in (`8`) üstüne çıkmaz. İlk dört jeton reçetenin
  sözleşmesi (`proje.md` → Doğrulama) ve dokuz phase'in hepsinde korundu.
  phase-4c'nin ilk koşularında görülen `glif=` sapması değişikliğin hiç
  koşmadığı bir ortamdaydı (A/B'de HEAD ile çalışma ağacı aynı satırı verdi),
  nedeni kanıtlanmadı ve phase-5'in ölçüm koşularında tekrarlanmadı; dağılım
  `docs/OLCUMLER.md` → `## Boşta kare`.
- `make kur` → exit 0 ve `target/release/bateri.app`. Ürünün içerik denetimi
  düşerse `kur: …` + çıkış 2; neyi denetlediği `Makefile`'ın `kur` yorumunda,
  girdi yarısı `make hepsi`'deki `bundle_assets` sınamasında.
- **Bağımlılık:** yeni crate yok, `Cargo.lock` hiçbir commit'te oynamadı. İki
  manifest satırı yalnız var olan bir crate'e feature ekledi: kök
  `Cargo.toml`'da `objc2-app-kit` → `NSPasteboard` (phase-2, `97b9c2c`) ve
  `bt-shell/Cargo.toml`'da `objc2-foundation` → `NSLocale` (phase-4b,
  `87034aa`; karar kaydı `discussion.md` → Karar 6 eki).
- **Ölçüm:** `IDLE_FRAME_LIMIT` görünür pencerede debug `make duman` ve
  release paket için yeniden ölçüldü, sınır ve kod değişmedi. Koşular, ortam,
  türetme ve yeniden ölçme tarifi `docs/OLCUMLER.md` → `## Boşta kare` ile
  `## Nasıl yeniden ölçülür`'de; bu belge sayı taşımaz.

### Doğrulama Checklist

- [x] `make hepsi` yeşil — phase-5'te kapıdan sonra (`32f28b2`; kod
      `3907585`'ten beri yalnız yorumda değişti)
- [x] `make test-yaris` yeşil — phase-4d'de kapıdan önce ve sonra
      (`3907585`, kilit yolunun son değişikliği)
- [x] `make duman` yeşil — phase-5'te kapıdan sonra
- [x] `make kur` yeşil — phase-5'in ölçümü sırasında, kaynağı değişmeden;
      içerik denetimi phase-4'te mutasyonlarla kırmızı görüldü
- [x] Paketli açılış: `BT_RUN_SECONDS` yolu paketten çalışıyor (phase-4);
      LaunchServices yolunda çocuğun dizini ve yereli kurala uyuyor (phase-4b
      ve phase-4c yoklamaları — ölçüm değil, açılış kanıtı)
- [x] `Cargo.lock` oynamadı
- [~] `make shader` / `make terminfo` — koşulu doğmadı: girdileri el değmedi
- [x] `[elle]` göz kontrolleri — kullanıcı yaptı (B.2)

## B. Yayın (doğrulamadan SONRA)

Dokuz phase'in `## Yayın Etkisi` bloklarından ve `git log b9944de..HEAD`'den
derlendi. Her phase'de "yok" diyen kalemler atlandı: `.metal`,
terminfo/`TERM`, ayar şeması, tema/materyal, shell entegrasyonu
(`assets/shell/`). Bilerek atlanan iki kutu `[~]`: phase-4'ün About paneli
(paneli açan menü öğesi yok, menü seti 00X) ve phase-5'in `/measure`'ı
(`Skill` aracıyla çağrılmadı, akışı elle izlendi — B.3).

### Kullanıcıya görünen davranış (son hâl)

- **Seçim:** fareyle sürükleyince seçilir; uç, basılan noktanın hücrenin
  hangi yarısına düştüğüne göre belirlenir; sürüklemesiz tık boş seçimdir.
  Ters videolu metnin seçimi normal renklerle görünür. Yazmak, yapıştırmak ya
  da ok tuşu seçimi kaldırır; tekerlek, Shift+PgUp ve Cmd-C kaldırmaz.
- **Pano:** Cmd-C seçimi kopyalar, boş seçim panoyu silmez. Cmd-V uygulama
  2004 istemişse `\e[200~…\e[201~` ile sarar ve yükteki `ESC`/`ETX`'i süzer;
  istememişse ham yazar (satır sonu dönüşümü yok — bilinçli, `discussion.md`
  → Karar 3). Diğer Command tuşları hâlâ yutuluyor (menü 00X).
- **Kaydırma:** birincil ekranda tekerlek/trackpad ve Shift+PgUp geçmişe
  kaydırır; basılı sürüklemede kaydırma seçimi uzatır; yazmak ya da
  yapıştırmak pencereyi dibe döndürür. Fare kipini açan uygulamada (ekran fark
  etmez) tekerlek raporu gider (SGR, UTF-8 ya da düz); fare kipi kapalı tam
  ekran uygulamada DECSET 1007 açıkken ok tuşu gider, Shift ya da
  `\e[?1007l` bunu keser. Düz PgUp/PgDn `\e[5~`/`\e[6~` gönderir.
- **Klavye:** oklar DECCKM'e uyar (`\e[?1h` altında `\eOA`…, kapalıyken
  `\e[A`…); Shift+Tab `\e[Z`, fn+Backspace `\e[3~`. Home/End hâlâ yutuluyor
  (borç, B.5).
- **Paket ve açılış:** `make kur` → `target/release/bateri.app` (kimlik
  `io.github.bateri.bateri`, yer tutucu ikon, on `NS*UsageDescription`
  anahtarı, imza ve entitlements yok). Kabuk her açılışta ev dizininde başlar
  — `cargo run` ve `make duman` dahil, yani terminalde `cd proje && cargo
  run` artık o dizinde açmaz (kullanıcı kararı, "istisnasız"). Ortamda yerel
  yoksa çocuğa `LANG={dil}_{bölge}.UTF-8` (kuruluysa) ya da
  `LANG=en_US.UTF-8` gider; kendi sürecimizin ortamı ve dizini değişmez.

### Çözülen çelişkiler (Uygulama Notları ve orkestratör kararları kazandı)

1. **phase-3**'ün Yayın Etkisi "alternate screen'de tekerlek susar" diyor →
   phase-3b R3.3'ü değiştirdi (`discussion.md` → Karar 4 eki); yukarıdaki son
   hâl onun.
2. **phase-4b**'nin Yayın Etkisi düşüş kolunda `LC_CTYPE=UTF-8` diyor →
   phase-4c `LANG=en_US.UTF-8` yaptı (Karar 6 eki, son madde).
3. **phase-3b, 4b ve 4d**'nin Yayın Etkisi "`[elle]` göz kontrolü bekliyor"
   diyor → üçünün de checklist'i kullanıcı damgasıyla kapalı (B.2). Bloklar
   tarihli kayıt olarak yerinde bırakıldı.
4. **phase-3b**'nin Yayın Etkisi `CLAUDE.md` `bt-core` satırını WAIVE önerisi
   diye bırakıyor → orkestratör kabul etmedi, satır phase-4b'de (`87034aa`)
   `CLAUDE.md`'ye girdi.
5. **phase-4d**'nin Yayın Etkisi `/code-review` (4)'ü (uygulama seçili
   hücreyi yeniden yazınca seçim düşmüyor) borç adayı sayıyor → orkestratör:
   "borç değil, not" (alacritty de düşürmüyor); yol haritasına girmedi. Aynı
   bloktaki Ctrl+Shift+Tab (WAIVE (1)) ise borç olarak girdi.
6. **phase-5**'in Yayın Etkisi belge listesi eksik → notlar ve orkestratör
   kararı `.claude/README.md`'yi, `/audit` mercek 6'yı ve `proje.md` →
   tuzaklar'daki sahiplik istisnasını da sayıyor (B.4).

### B.1 `/ship` — dallanmamış `main` push'u `[oto]`

Bu belge yazılırken `origin/main` (`b9944de`) 37 commit geride: 3 planlama
(`03694f6` → `efcc327`), 10 kod (dokuz phase + `097f155`) ve 24 defter
commit'i (hash damgaları, sadakat/orkestratör satırları, göz kontrolü
damgaları ve sonradan eklenen 3b/4b/4c/4d'yi kuran dört "phase ekle"
commit'i — kapsam kararlarını `discussion.md`'ye yazıyorlar). Bu kapanış
commit'i otuz sekizinci. **Hiçbiri push edilmedi**; `/ship` Bölüm A'yı
yeniden koşturup gönderecek.

Faz commit'leri iniş sırasıyla: `77b4afd` (phase-1) → `97b9c2c` (phase-2) →
`6a7a92d` (phase-3) → `dd46e85` (phase-4) → `100ecfa` (phase-3b) →
`87034aa` (phase-4b) → `aa6b3a0` (phase-4c) → `3907585` (phase-4d) →
`32f28b2` (phase-5). phase-3b phase-4'ten sonra eklendi ve sonra indi; plan
sırası doğal sıradır.

**Faz dışı, aynı push'ta giden kod commit'i: `097f155`** (phase-2 ile phase-3
arasında). Kullanıcı bildirdi: "araba"nın "raba" kısmı seçilip kopyalanınca
"araba" geliyordu. Kök neden phase-1'in seçim uçlarındaydı — uç hücrenin
hangi yarısına basıldığı alınmıyordu. Düzeltme `bt-core`'a `CellHalf` +
`SelectionPoint` getirdi ve aynı yolda dört kusuru daha kapattı: her seçimin
uç hücreleri vurgulanmıyordu, sürüklemede ekrana hiçbir şey eklemeyen kare
isteniyordu, satır sonuna sürüklemede son harf kayboluyordu, geniş karakterde
vurgu yarım kalıyordu. Commit iletisi "Yayın etkisi: yok" diyor ama
kullanıcıya görünen davranış değişti (uç hücre yarısına göre, tek tık boş
seçim) ve yukarıdaki son hâlde sayıldı. Gövdesi `phase-2.md` → Uygulama
Notları → "Kullanıcı bildirimi"; phase-1'in aşılan üç notu orada damgalı.
phase-3 bunun `visible_range`'i ve `SelectionPoint`'i üstüne kurulu — bkz.
Geri Alma.

### B.2 Göz kontrolleri `[elle]` — yapıldı

| phase | ne denendi | ne zaman, hangi yapı |
|---|---|---|
| phase-1 | fareyle seçim, ters video vurgu | 2026-09-14, `097f155` üstünde |
| phase-2 | kopyala-yapıştır turu (terminal içi + dış uygulama) | 2026-09-14, `097f155` üstünde |
| phase-3 | geçmişe kaydırma, yavaş trackpad, basılı sürüklemede kaydırma, girdide dibe dönüş, `less`'te Shift+PgUp | 2026-09-15, `make kur` paketi (`87034aa` sonrası) |
| phase-3b | `man`/`less` tekerlek ve trackpad, `less`'te oklar, `nvim`/`htop` tekerlek, birincil ekranda geçmiş | aynı paket |
| phase-4 | Finder'dan ve Dock'tan açılış, Dock ikonu, öne çıkma, klavye | aynı paket |
| phase-4b | Dock açılışında `pwd` ve yerel, `ğüşıöç İ` girişi, `cargo run`'ın ev dizini | aynı paket |
| phase-4c | ayrı göz kontrolü yok — phase-4b'ninki kapsıyor (değişen yalnız düşüş kolunun değişkeni; bu makinedeki sonucu paketli yoklama gösterdi) | — |
| phase-4d | seçim + yazma, ters video seçimi, zsh menüsünde Shift+Tab, fn+Backspace | 2026-09-15, `make kur` paketi (`3907585` sonrası) |
| phase-5 | ölçüm fazı; pencerenin görünür olduğunu göz değil CGWindowList yoklaması gösterdi | — |

Gözle **hiç görülmeyenler** kapı değil, borç olarak kayıtlı (B.5): About
paneli, indirilen kopyada Gatekeeper, `NS*UsageDescription` dizgilerinin
etkisi.

### B.3 Ölçüm `[komut]` — yapıldı

phase-5 `IDLE_FRAME_LIMIT`'i görünür pencerede yeniden ölçtü; sınır ve kod
değişmedi, ölçüm son koda alındı ve kendi commit'inde indi (R5.1). Sonuç
`docs/OLCUMLER.md` → `## Boşta kare` (2026-09-15 kaydı); paket koşusunun tarifi
ve tuzakları `## Nasıl yeniden ölçülür`'de. **Atlanan kapı `[~]`:** `/measure`
`Skill` aracıyla çağrılmadı; kullanıcı ölçümü açıkça başlattı ve skill'in
adımları `SKILL.md`'den okunarak izlendi (orkestratör kabul etti). Başka hiçbir
phase ölçüm bekleyen iddia bırakmadı. 002/003/004'ün kare süresi ve açılış
iddiaları bu setin konusu değil, açık kalıyor (`005-olcum-kancalari/teslim.md`
→ B.2).

### B.4 Belge ve iş akışı değişiklikleri `[oto]`

Kod commit'lerinin içinde, aynı push'ta gidiyor:

- `CLAUDE.md`: `make kur` komutu, taban cümlesi ve Apache-2.0 atıf sözleşmesi
  (phase-4); çocuk ortamı maddesi — ev dizini, yerel kuralı ve düşüşü
  (phase-4b, 4c); katman tablosu — `bt-core` girdi kodlaması, `bt-shell`
  `child` ve `NSLocale` (phase-4b); `make duman` satırı ve ölçüm maddesi
  (phase-5).
- `.claude/is-akisi/proje.md`: `make kur` doğrulama satırı ve "henüz yok"
  listesinden çıkış, depoya girmeyenler (phase-4); kapı paragrafı ve
  tuzaklardaki ölçüm sahipliği istisnası (phase-5).
- **İş akışı dosyaları (phase-5):** `.claude/README.md`, `/audit` mercek 6 ve
  `/measure` skill'inin tür tablosu. Yeni kurulan `docs/OLCUMLER.md`'ye ve
  005'ten beri süren "sabitin doc'u kutupları taşır" düzenine uyum; kural
  genişlemedi, var olan uygulama yazıya geçti. Orkestratör kabul etti ve
  kapanış özetinde kullanıcıya söylenir.
- `docs/OLCUMLER.md` kuruldu (phase-5). `crates/bt-shell/src/app.rs`:
  `IDLE_FRAME_LIMIT` ve `Measured` doc'ları (phase-5, yalnız yorum).
- `docs/YOL-HARITASI.md`: iki borç (phase-4), 006 satırına tarihli not ve boşta
  kare borcunun güncellemesi (phase-5), kapsam notu, açık soru ve altı borç
  (bu kapanış commit'i).
- `.tasks/002-vt-motoru/teslim.md` B.1 ve `.tasks/README.md`'nin 002 satırı:
  attribution kapandı (phase-4).

### B.5 Bağımlılık ve borç kaydı `[oto]`

- **Bağımlılık:** yeni crate yok; iki feature bayrağı (A → Beklenen çıktı).
- **Yol haritasına bu kapanış commit'inde altı borç girdi**
  (`docs/YOL-HARITASI.md` → Sete bağlanmamış borçlar): pencereye duyarlı
  hasar (phase-3), fare raporlamasının geri kalanı (phase-3b), klavye
  kalanları — Home/End, değiştiricili oklar, Ctrl+Shift+Tab (phase-3b, 4d),
  yerel ara kolu (phase-4c), küçük hijyen — `make kur`'un boş hedef dizini ve
  pano sınamalarının geçici panoları (phase-4c, Kapsam eki), paket dumanı
  (phase-5). phase-4'ün iki borcu (About paneli; üçüncü taraf bildirimleri +
  Gatekeeper) orada zaten duruyor.
- **Dağıtım setine kalan, ekranda doğrulanmamış kalemler** (phase-4 notları):
  indirilen kopyada Gatekeeper'ın gerçek davranışı; imzasız pakette TCC
  izinlerinin her `make kur`'dan sonra yeniden sorulabilmesi;
  `NS*UsageDescription` dizgilerinin etkisi. Bundle kimliği
  `io.github.bateri.bateri` plan'da yoktu, phase-4'ün kararı: tercihler ve
  kayıtlı pencere durumu ona bağlanır, dağıtımdan önce değiştirmek ucuz,
  sonra değil.
- **Bilerek kalan, borç sayılmayanlar:** ham yapıştırmada satır sonu dönüşümü
  yok (phase-2 WAIVE, Karar 3); 2004 sorgusu ile yazma arasındaki dar pencere
  (phase-2 WAIVE); tuş başına bir `Term` kilidi (phase-3); yedekte `C.UTF-8`
  denenmiyor (phase-4c WAIVE (4), yedek kullanıcı kararı); uygulama seçili
  hücreyi yeniden yazınca seçim düşmüyor (phase-4d, alacritty ile aynı). OSC
  52 köprüsü 007'ye ertelendi (`discussion.md` → Karar).

### Yayın Checklist

<!-- `/ship` bekleyen manuel adımları BU başlık altında arar. -->

- [ ] B.1 `/ship` push `[oto]` — yapılacak: 37 commit + bu kapanış commit'i;
      push'tan önce `make hepsi` · `make test-yaris` · `make duman` · `make kur`
- [x] B.2 Göz kontrolleri `[elle]` — kullanıcı yaptı, 2026-09-14 ve
      2026-09-15 (tablo; phase-4c phase-4b'ninkine yaslandı, phase-5 ölçüm fazı)
- [x] B.3 Ölçüm `[komut]` — phase-5, sonuç `docs/OLCUMLER.md` → `## Boşta
      kare`; `/measure` `Skill` aracıyla çağrılmadı (`[~]`, gerekçe B.3)
- [x] B.4 Belge ve iş akışı değişiklikleri `[oto]` — kod commit'lerinde;
      `.claude/` dosyalarının değiştiği kapanış özetinde kullanıcıya söylenir
- [x] B.5 Bağımlılık ve borç kaydı `[oto]` — `Cargo.lock` oynamadı; altı borç
      `docs/YOL-HARITASI.md`'de

## Geri Alma

- **Henüz push edilmediği için** en basit geri alma `/ship`'i koşmamak: sorun
  push'tan önce bulunursa revert gerekmez, düzeltme forward-fix olarak eklenir.
  Aşağıdaki zincir yalnız `main`'e gittikten **sonra** bir sorun çıkarsa
  geçerli; sıra tersten.
- **`32f28b2` (phase-5)** yalnız belge ve yorum: geri alınırsa
  `docs/OLCUMLER.md` silinir, "henüz yok" cümleleri (`CLAUDE.md`, `Measured`'ın
  doc'u, `/measure`) ve `.claude/` iş akışı değişiklikleri birlikte döner —
  kendi içinde tutarlı, `IDLE_FRAME_LIMIT` zaten `8`. Bu kapanış commit'i o
  dosyaya bağlandığı için onunla birlikte gider.
- **Seçim ve girdi zinciri — ortadan tek başına geri alınamaz, üstündekilerle
  birlikte gider:** `3907585` (4d: `send_input`'ta seçim temizliği, XOR,
  tuşlar) → `100ecfa` (3b: `scroll_locked`, `send_input`, `input.rs`) →
  `6a7a92d` (3: `update_selection`, kaydırma, girdide dibe dönüş) →
  `097f155` (`SelectionPoint`, `visible_range`) → `97b9c2c` (2: pano,
  `selection_text`'i tüketir) → `77b4afd` (1: kök). `100ecfa` geri alınırsa
  `87034aa`'nın `CLAUDE.md`'ye yazdığı `bt-core` girdi kodlaması cümlesi de
  aynı işlemde düzelmeli.
- **Açılış zinciri:** `aa6b3a0` (4c) `87034aa`'nın (4b) `child.rs`'i üstünde.
  4c tek başına geri alınırsa düşüş `LC_CTYPE=UTF-8`'e döner; 4b geri
  alınırsa kabuk yeniden çağıranın dizininde (Dock'ta `/`) ve yerelsiz başlar,
  `NSLocale` feature satırı da gider (`Cargo.lock` yine oynamaz).
- **`dd46e85` (phase-4, paket)** kod yollarından bağımsız: geri alınırsa
  `make kur` yeniden "henüz yok" der; `002-vt-motoru/teslim.md` B.1 ve
  indeksin 002 satırındaki "attribution kapandı" notları aynı commit'le geri
  gelir. Kurulu paket depoda değil: `target/release/bateri.app` ve Dock
  sabitlemesi elle kaldırılır.
- **Ölçüm kaydı:** kod commit'lerinden biri geri alınırsa `docs/OLCUMLER.md`'nin
  2026-09-15 kaydı "son koda alındı" (R5.1) dayanağını yitirir; sınır o koda
  karşı yeniden ölçülür, körü körüne bırakılmaz.
- **Ayar şeması, tema, terminfo, shell entegrasyonu:** `plan.md` → Göç net —
  hiçbiri değişmedi, doğrulanacak bir geri düşüş yok. **`Cargo.lock`:** hiçbir
  adımda oynamadı, hiçbir revert'te de oynamaz.
