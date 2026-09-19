# Phase 1 — Devrin histerezisi

## Özet

Hızlı komutta caret'in dock → ızgara → dock gidiş-dönüşü görünmez olsun:
Dock→Grid geçişi bir süre tutulsun, o sürede geri dönerse hiç olmamış sayılsın.

_Requirements: R1, R1.1, R1.2, R1.3, R1.4, R10 (bayat doc yarısı)_

## Değişiklikler

- **`crates/bt-core/src/shell.rs`** — histerezisin tamamı burada.
  - `ShellLog`'a devrin **damgası**: son `caret_home` cevabı ve ne zaman
    değiştiği. Geçiş `apply_scan`'de gözleniyor — tek giriş noktası ve yaprak
    kilidin altında, yani `Term`'e hiç dokunmadan.
  - `caret_home` üçüncü bir argüman alıyor (tutma süresi). **Yalnız Dock→Grid
    yönü tutuluyor:** ters yön (Grid→Dock) geciktirilirse komut bitince caret
    ızgarada asılı kalır ve kullanıcı yazmaya başladığında dock'ta caret'siz
    bir satır görür — yanlışın yönü güvenli değil.
  - **Devir `line-finish`'te başlıyor, `CommandStart`'ta değil** (ölçüldü,
    `context.md` → Kanıt). `running_since`'e bağlanan bir eşik ilk geçişi
    görmez ve üçüncü bir devir doğurur; bu phase ona **hiç dokunmuyor**.
- **`crates/bt-core/src/session.rs`** — eşiğin kalan süresi `Cursor::next_tick`
  ile isteniyor.
  - **Aynı kilit turunda** hesaplanıyor (`caret_home`'un okunduğu tur), yoksa
    iki ayrı ana ait iki cevap doğar.
  - **`min`'leniyor**, ezilmiyor: bugün `resolve_blocks` `next_tick`'i
    doğrudan yazıyor ve o yol koşan bloğun çıpasının görünür olmasına bağlı —
    devir buna bağlanamaz. Emsal, iki son tarihi tek saatte birleştiren
    `arm_clock` (014 phase-2).
  - Saatin üç şartı bu phase'de yazılı olacak: içerik gerçekten değişiyor
    (caret yer değiştiriyor, doluluk sayısı oynuyor), tek atımlık,
    adlandırılmış durma koşulu (tutma süresi doldu ya da yüklem geri döndü).
- **`crates/bt-core/src/shell.rs` (doc)** — `caret_home`'un serbest fonksiyon
  gerekçesi bayat: "`dock::render` defteri değil kopyalarını taşıyor" diyor ama
  o çağrı kalkmış; üretimde tek çağıran `ShellLog::caret_home`.

## Kabul

- `ls` gibi hızlı bir komutta caret **hiç kıpırdamıyor** — ne yukarı çıkıyor ne
  içerik zıplıyor. İkisi tek yüklemden beslendiği için tek değişiklik ikisini
  de kapatıyor.
- `sleep 2` gibi yavaş bir komutta devir **oluyor** ve gecikmesi tutma süresi
  kadar: caret ızgaraya geçiyor, komut bitince dock'a dönüyor.
- Komut **girdi isteyince** (`cat` beklerken, `ssh` parolası) caret ızgarada;
  tutma süresi o davranışı geciktiriyor ama değiştirmiyor.
- Tutma süresi dolduğunda kare **isteniyor**: çıktısı olmayan bir komutta
  (`sleep 5`) devir kendiliğinden gerçekleşiyor, bir sonraki hasarı beklemiyor.
- Komut bitince saat **sönüyor** — bekleyen bir tutma yokken `next_tick` bu
  yoldan `Some` dönmüyor.

## Uygulama Notları

- **Üçüncü argüman `Duration` değil `bool` oldu.** `caret_home` saf ve saat
  görmüyor; "tutma sürüyor mu" kararı defterden (`ShellLog::caret`) geliyor,
  süre ile damga orada yaşıyor. `Duration` alsaydı yüklem `Instant`'a bağlanır
  ve sınanabilirliğini kaybederdi.
- **`Unavailable` tutmanın dışında bırakıldı** (plan'da yoktu). Ayna arızası
  bir sıçrama **üretmiyor** (kullanıcı geri silmeden `Live`'a dönmüyor), yani
  tutmanın orada kazancı sıfır; bedeli ise caret'in 150 ms boş bir dock'ta
  durması olurdu, oysa o kolun yazılı gerekçesi "gösteremediğimiz satırın
  caret'i de ızgarada durmalı". Carve-out yüklemin **içinde**: dışarıda
  olsaydı `caret_home(_, Unavailable, true)` `Dock` döner ve yüklem yalan
  söylerdi. Bekçisi yeni bir sınama değil — mevcut
  `the_grid_keeps_the_input_line_when_the_mirror_cannot_show_it` **dokunulmadan**
  yeşil kaldı ve carve-out kalkarsa kızarır.
- **`min` saf bir yardımcıya çıkarıldı** (`shell::sooner`). Inline kalsaydı
  sınanamazdı ve ezmeye dönüşmesi iki yönde de sessiz olurdu (ya sayaç donar
  ya devir hiç gerçekleşmez). Emsal `bt_gpu::link`'in `due_clock`'u.
- **Tik sınaması defter düzeyinde**, oturum düzeyinde değil: 150 ms'lik pencere
  bir entegrasyon sınamasında deterministik yakalanmaz. `hold_left`'in
  `Some`→`None` geçişi ile `sooner`'ın birleştirmesi ayrı ayrı çivilendi;
  ikisinin arasındaki tek satırlık bağ (`cursor.next_tick = sooner(...)`)
  gözle doğrulandı.
- **`hold_left` `home != raw`'dan türüyor**, ayrı bir koşuldan değil — ikisi
  ayrı yazılsaydı ayrışabilir ve tutmanın uygulanmadığı kolda boşuna kare
  istenirdi.
- **İki mevcut sınama beklemeye geçti** (`a_running_command_takes_the_cursor_
  back_to_the_grid`, `the_grid_takes_the_input_line_back_when_zle_lets_go`):
  iddiaları değişmedi, yalnız tutma kadar gecikiyor. Yan kazanç, ikisi artık
  "tutma gerçekten doluyor"un da bekçisi — süresiz bir tutma orayı kızdırır.

## `/code-review` (riskli phase: `test-yaris` gerekti)

**12 bulgu; 9 düzeltildi, 3 kullanıcının gözünü bekliyor.** Düzeltilenlerin
ikisi gerçek bekçi kusuruydu ve ikisi de **kanıtlanarak** kapatıldı:

- **`a_fast_command_never_hands_the_caret_over` duyarsızdı.** `now` olaylardan
  **önce** alınıyordu, `saturating_duration_since` sıfıra kırpıyordu ve sınama
  `HANDOVER_HOLD > 0` olan her değerde yeşil kalıyordu — 1 ms'lik bir tutma
  `ls`'in 44 ms'sini hiç yakalamadığı hâlde. Düzeltildikten sonra sabit 1 ns'ye
  indirilerek **kızardığı görüldü**.
- **Tutmanın kare istediğini hiçbir şey sınamıyordu.** İki entegrasyon sınaması
  `frame()`'i döngüde çağırdığı için saati atlıyor. Yeni bekçi defteri
  doğrudan sürüyor (150 ms'lik pencere PTY zamanlamasına bırakılamaz) ve
  bağlayan satır silinerek **kızardığı görüldü**.
- Carve-out'un bekçisi de zamana bağlıydı ve **açığa düşüyordu**; yerine
  saatten bağımsız bir yüklem sorgusu kondu.

Kalan altısı belge ve yapı kusuruydu: sabitin doc'u 230 ms ilişkisini
anmıyordu, `caret()` ham cevabı iki ayrı yoldan türetiyordu, `observe_caret`
her olayda saat okuyordu, üçüncü ön koşulun atlanma gerekçesi **ulaşılamaz**
bir hâli anlatıyordu (gerçek değişmez yazıldı) ve dört doc bayattı
(`Cursor::next_tick`'in "üç yol"u, `link.rs`'in "süre sayacı"nı tekil sanması,
`caret_in_dock`'un ön koşul sayısı, `session.rs`'in "üç tüketici"si).

### Kullanıcının gözünü bekleyen üç bulgu

Üçü de **aynı tezin parçası** ve kodla doğrulandı; hiçbiri hızlı komutu
(setin asıl derdi) etkilemiyor, üçü de **tutmayı aşan** komutlarda görünüyor:

1. **Tutma caret'i yerinde tutmuyor, üçüncü bir yere taşıyor.**
   `line-finish` aynayı `reset()`'liyor, `dock::render` de `Live` olmayan
   yüzeyde caret'i `TEXT_COL`'a koyuyor. Yani `python`+Enter'da caret
   satır sonundan dock'un 2. sütununa kayıyor, sonra (tutma dolunca) ızgaraya.
   Hızlı komutta bu **iki hücrelik** bir kayma ve büyük gidiş-dönüş kalkıyor —
   kazanç orada. Yavaş komutta ise eskiden tek olan hedefleme ikiye bölünüyor.
2. **`content_rows` tutma boyunca imleç terimini düşürmeye devam ediyor**,
   yani Enter'daki bir satırlık büyüme 150 ms **gecikiyor** ve satır sonu
   kaymasıyla artık çakışmıyor. Yeni bir kayma doğmuyor, var olan ikiye
   ayrılıyor.
3. **`cursor_motion = "snap"` ve Hareketi Azalt bedeli ödüyor, karşılığını
   almıyor:** o kiplerde animasyon zaten yok, korunacak bir uçuş da yok.

**Reddedilen alternatif:** inceleme tutmayı `bt-gpu::motion`'a (hedef
debounce'u) taşımayı önerdi. **Uygulanamaz:** animatör yalnız konum görüyor
ve bir devri sıradan bir tuş vuruşundan **ayırt edemez**; genel bir hedef
debounce'u her harfte caret'i 150 ms geciktirirdi. Ayırt etmesi için "devir"
kavramının `bt-gpu`'ya geçmesi gerekirdi ve o katman yönüne aykırı — planın
tutmayı `bt-core`'a koymasının sebebi tam olarak bu.

**Karar kullanıcının**, çünkü üçü de **hissedilir** ve ölçüyle değil gözle
tartılır: hızlı komuttaki kazanç yavaş komuttaki iki bölünmeye değer mi.

## Yayın Etkisi

- **ölçüm bekliyor: "sıçrama azaldı".** İddia **azaltma**, kaldırma değil:
  animasyon ~230 ms'de yerleşiyor ve tutma süresi onun altındaysa belirti
  küçülür ama bitmez. Doğrulaması önce/sonra göz kontrolü; kancası yok ve bu
  set kanca doğurmuyor.
- **seçilmiş sayı:** tutma süresi. `const` doc'unda "seçilmiş, ölçülmemiş" +
  gerekçe (013'ün "bir saniyeyi geçmeyen komutun sayacı gösterilmez" emsali).
  `docs/OLCUMLER.md`'nin konusu **değil**.
- **bilinen sınır:** `CORRECT`'in `[nyae]` sorusu ve `zle -M` mesajı da
  `Input`+`Idle`, yani `line-finish` ile **aynı yüklem durumu** — ayrılamazlar
  ve caret o hâllerde de tutma kadar (150 ms) geç geliyor. Satır ızgarada
  görünüyor (bastırma aynanın `Live` olmasını istiyor, o kapı ayrı ve anında
  açılıyor); geciken yalnız caret.
- shader / ayar şeması / tema / terminfo / app bundle / yeni bağımlılık: yok.
  `make kur` gerekmiyor.
- `CLAUDE.md`: devrin tarifi ("kalan her hâlde dock'un") tutma süresini
  anmalı — bugünkü cümle onsuz yanlış olur.

## Checklist

- [x] `bt-core`: `ShellLog`'da devrin damgası, `apply_scan`'de gözlem
- [x] `bt-core`: `caret_home` tutmayı alıyor (`bool`), **yalnız Dock→Grid** yönü
- [x] `bt-core`: `next_tick` aynı kilit turunda, `resolve_blocks`'unkiyle
      `min`'leniyor (ezilmiyor)
- [x] Test: hızlı komut devir **doğurmuyor** (yüklem düzeyinde)
- [x] Test: yavaş komut devri **doğuruyor**, gecikmesi tutma süresi kadar
- [x] Test: tutma dolunca `next_tick` kare istiyor; komut bitince sönüyor
- [x] Test: `resolve_blocks`'un tiki ezilmiyor (`min` bekçisi)
- [x] `caret_home`'un bayat doc'u düzeltildi
- [x] `CLAUDE.md` devrin tarifi
- [x] Doğrulama geçti (`make hepsi` — exit 0)
- [x] `make test-yaris` (paylaşılan durum: `ShellLog`'a yeni alan) — exit 0
- [x] Riskli phase (`test-yaris` gerekti): `/code-review` koştu — 12 bulgu, 9 düzeltildi, 3 gözle karara bırakıldı
- [x] `make duman` (kullanıcıda, 2026-09-19): `kare=29 hucre=8 glif=6 kural=15
      icerik=2 hareket=27 sessiz=1749.52ms kapanis=clean` — 014'ün tabanıyla
      **bire bir aynı**, yani set hermetik koşuya tek kare eklemedi
- [x] Yayın etkisi yazıldı
