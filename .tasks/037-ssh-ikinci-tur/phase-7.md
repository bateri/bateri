# Phase 7 — Yüklemenin hedef tasarımı

## Özet

Kullanıcının yeniden onayladığı hedef tasarım
(https://claude.ai/artifact/Qdn7uBQRzorS8uKrP21Pby): liste bir popover,
kendiliğinden yapıştırma yok, sonuç satırı nereye gittiğini söylüyor, uzun
yükleme durdurulmadan önce soruyor, başlık ve sekme ilerlemeyi taşıyor,
bateri arkadayken bildirim geliyor ve çubuğun dolan kısmı hep `info`.

_Requirements: R8, R10_

## Hedef (kullanıcı onayı, 2026-09-26)

Karar 7 → Kullanıcı kararı'nın **üstünde**; çelişirse bu kazanır.

1. **"Show files (N)" bir `NSPopover`** (menü kalkıyor): düğmeye bağlı,
   `transient` (dışarı tık, Esc, düğmeye yeniden basış kapatır). Düğme her
   zaman `Show files (N)`, N kuyruktaki kalem sayısı. İçerik: `Uploading to
   {host}`, kalem başına satır, ayraç ve sağda `Cancel all ⌘.`. Sol sütun:
   ad (klasörde `static/ · 124 files`), durum (akan: çubuk + `18.2 / 96.0 MB
   · 1.2 MB/s`; bekleyen: `Waiting · 48.5 MB`; biten: yeşil `✓ Uploaded ·
   96.0 MB`), sönük `→ {hedef}`. Sağ sütun dikeyde ortalı: akan `Cancel`,
   bekleyen `Remove`, biten düğmesiz. Ada tık hiçbir şey yapmaz; ilerleme
   canlı; kalem sayısı 1'e inerse popover kapanır ve düğme kalkar.
2. **Kendiliğinden yapıştırma yok.**
3. **Sonuç satırları** (~4.5 sn): `✓ backup.tar.gz → /var/www/app`, `✓
   static/ → …`, `✓ 3 files → …`, hedefler farklıysa `✓ 2 files uploaded`;
   `Cancelled — partial file removed`, `Cancelled — 1 of 2 uploaded, partial
   file removed`; `Failed — {sebep} · {k} of {n} uploaded`. Başarı
   `success`, iptal `dim`, hata metni `error`.
4. **Onay sayfası:** `Upload “x” to host?` + `96.0 MB → dir`; `Upload folder
   “static” to host?` + `124 files, 38.2 MB → dir`; `Upload 3 items to
   host?` + toplam, hedef ve adlar; yükleme sürerken `Added to the queue; the
   current upload keeps going.` Replace/Merge ve boş alan bugünkü gibi.
5. **Durdurma sorusu:** akan kalem 30 sn'yi geçtiyse ⌘., satırın
   `Cancel`/`Cancel all`'ı ve popover'ın `Cancel`'ı önce sorar (`Stop
   uploading?` / `Stop all uploads?`; `x: 48.0 of 96.0 MB will be lost.`,
   `1 waiting file won't be uploaded.`, `2 finished files stay on host.`;
   `Keep uploading` varsayılan ve Esc, yıkıcı `Stop`). Altında sormaz;
   `Remove` hiç sormaz; sayfa açıkken yükleme sürer ve kalem biterse sayfa
   kapanır. 30 adlı bir tasarım sabiti.
6. **Başlık ve sekme:** `↑ 64% · ⇄ host`, yüzde başına en çok bir kez, bitince
   eski başlık; ⌘. alternatif ekranda da çalışır.
7. **bateri arkadayken bildirim:** bitince (`3 files uploaded` / `to
   host:dir`), hata ve kopmada (`Upload failed` / `Disk full on host. 0 of
   3 uploaded.`); önde iken yok. Yeni crate gerekirse dur.
8. **Çubuğun rengi:** dolan kısım hep `info`; boş iz işaretli host'ta
   işaretin rengi, işaretsizde ayracınki.
9. Phase-6'nın düğmeleri aynen kalır.

## Değişiklikler

- **`crates/bt-core/src/shell.rs`** — `Transfer::tone`/`lead`
  (`TransferTone`: gövdenin başındaki karakterlerin rengi); `TransferControls`
  doc'u (N biten dahil, `Hide files` yok).
- **`crates/bt-core/src/dock.rs`** — `Dock::track` (boş iz) ve ilerlemede
  `edge = info`; gövdenin tonlu çizimi; `ButtonLabel::HideFiles` kalkıyor;
  `transfer_button_span` (popover'ın çıpası); `UPLOAD_GLYPHS`'e `→`.
- **`crates/bt-gpu/src/frame.rs`, `link.rs`** — `set_dock_progress(progress,
  track)`: çubuğun zemini `track`, ikinci ayraç kendi renginde.
- **`crates/bt-atlas/src/lib.rs`** — küçük sınıfın sözlük sınamasına `→`.
- **`crates/bt-shell/src/upload.rs`** — kuyruk kalemleri tutuyor (kimlik,
  hâl; biten kalıyor); `end_line`/`end_notice`/`Tally`; `sheet(…, busy)`;
  `stop_request`/`stop`/`STOP_ASK_AFTER`; `list()` (popover'ın saf modeli);
  `percent`/`titled`; `remove(id)`; yapıştırma ve `remote_path` kalkıyor.
- **`crates/bt-shell/src/uploader.rs`** — `NSMenu` listesi yerine popover
  (yerinde tazeleme, Esc izleyicisi, yeniden basışın ayırt edilmesi); durdurma
  sayfası; başlık öneki; bildirim.
- **`crates/bt-shell/src/window.rs`** — ivar'lar, `NSPopoverDelegate`,
  `uploadRowAction:` (`cancelUploadItem:`'ın yerine), `cancelUpload:` →
  soru, `apply_title`.
- **`crates/bt-shell/src/view.rs`** — `context_span_rect`.
- **`crates/bt-shell/Cargo.toml`** — `NSPopover`, `NSParagraphStyle` (yalnız
  başlık bayrakları).
- **`CLAUDE.md`** — yükleme paragrafı.

## Kabul

- `upload`: sonuç satırlarının hepsi ve tonları, bildirim metinleri, başlık
  öneki ve yüzde başına tek yazım, 30 sn eşiği (altında/üstünde) ve sorunun
  metni, yapıştırmanın kalkması (`finish` yalnız sonuç veriyor), popover'ın
  satırları (ad, hâl, hedef, düğme), `Remove`'un sormadan çıkarması, tek
  kalemin durdurulup listeden çıkması, sayfa açıkken kalem değişirse
  durdurmanın başkasına dokunmaması, onay sayfasının dört biçimi.
- `bt-core`: dolan kısım `info`, boş iz işaret/ayraç, sonuç satırının renkleri,
  `Hide files` yok, `transfer_button_span`.
- `bt-gpu`: çubuğun zemini `track`, ikinci ayraç değişmiyor.
- Gözle kontrol: aşağıdaki Checklist satırı.

## Uygulama Notları

- **Durdurma sorusunun Esc'i elle** (yerel olay izleyicisi): `NSAlert` bir
  düğmeye tek tuş eşdeğeri veriyor; `Keep uploading` ilk düğme olarak Return'ü
  taşıyor ve Esc'i de ona vermek Return'ü alırdı (`defaultButtonCell` de
  eşdeğeri `\r`'ye geri çeviriyor — Swift ile denendi). Popover'ın Esc'i de
  aynı yoldan: terminal penceresi key kalıyor ve Esc yoksa uzak kabuğa
  giderdi.
- **Düğmeye yeniden basış:** `transient` popover dışarıdaki basışta kendini
  kapatıyor ve aynı basış view'a da varıyor; `popoverWillClose:` kapatan
  olayın zamanını saklıyor ve aynı olay popover'ı yeniden açmıyor.
- **Tek kalemi durdurmak onu listeden çıkarıyor** (demo): gideni biten
  sayılıyor, gitmeyeni çubuğun paydasından düşüyor (çubuk geri gitmesin);
  geriye yalnız biten kalemler kaldıysa sonuç `✓`. Tek kalemli kuyrukta satırın
  durdurması kuyruğun iptali.
- **Bağlantı kopunca sonuç satırı** spec'te adıyla yok: §3'ün hata kalıbı
  (`Failed — connection to {host} lost · k of n uploaded`) ve §7'nin bildirimi
  (`Upload failed` / `Connection to {host} lost. k of n uploaded.`).
- **`1 finished file stays`**: tekilde fiil tekil (demo çoğulu tekilde de
  kullanıyordu).
- **Bildirim `UNUserNotificationCenter`** — karar kaydı: kullanıcı
  `objc2-user-notifications` bağımlılığına **2026-09-27'de onay verdi**
  (0.3.2, objc2 ailesinin graftaki nesli; varsayılan set kırpık, çünkü
  `objc2-core-location`'ı çekiyordu — `Cargo.lock`'a yalnız bu paket ve
  `bt-shell`'in kenarı girdi). **Reddedilen:** phase'in ilk hâli olan
  `NSUserNotification` (Foundation'ın varsayılan setinde, yeni crate
  istemiyordu) — macOS 11'den beri kullanımdan kalkmış ve macOS 26'da
  göründüğü doğrulanmamıştı. İzin (`Alert`; ses yok, eski yol da ses
  çalmıyordu) **ilk bildirimde** isteniyor ve bildirim cevabın bloğunda
  kuruluyor; reddedilirse yükleme değişmiyor, yalnız bildirim yok. Delegate
  kurulmuyor: `willPresentNotification`'ı olmayan merkez önde gelen bildirimi
  susturuyor, yani "önde iken yok" teslim anında da geçerli. Paket kimliği
  yoksa (`cargo run`, sınamalar, süreli koşu) merkez hiç çağrılmıyor —
  `currentNotificationCenter` orada istisna atıyor. Lisans `Zlib OR
  Apache-2.0 OR MIT`: `THIRD-PARTY-LICENSES.txt`/`Credits.html`'e bu commit'te
  satır girmedi — objc2 ailesinin bildirimi `docs/YOL-HARITASI.md`'deki MIT
  borcunun parçası. Gerekçeler `notify`'ın doc'unda.
- **⌘. alternatif ekranda:** koddan doğrulandı — menü kısayolu
  `performKeyEquivalent:` ile `keyDown:`'dan önce yakalanıyor ve
  `validateMenuItem:`'ın `cancelUpload:` kapısı yalnız kuyruğa bakıyor.
- **Durum satırının gövdesi değişmedi** (spec'in dışında): demo satırda
  kuyruğun toplam baytını gösteriyordu, bateri akan kalemin baytını.
- **Set kapısı `/code-review` bulguları (dördü de giderildi):** sırada
  bekleyen yokken akan kalemi popover'dan durdurmak bitenlerin `✓`'sünü
  gösteriyordu — artık kuyruğun iptali (`Cancelled — 1 of 2 uploaded, …`);
  ssh kapandıktan sonra akan tek kalem başarıyla bitince `Failed — connection
  lost` diyordu — her kalem bittiyse sonuç `✓`; durdurma sayfasının Esc'i
  pencere numarasını sayfa gösterilmeden okuyordu — olay anında pencereyle
  karşılaştırılıyor; damlanın yoklaması sürerken ⌘. durdurma sorusunu
  açabiliyordu — `Uploads::asking` de kapı. `/audit`: mekanik temiz
  (`Cargo.toml` uyarısı aşağıda), kullanımdan kalkmış API'nin `use`'undaki
  `#[allow]`'a gerekçe yazıldı; ayar/tema ve hücre/shader mercekleri ilgisiz.
- **`objc2-app-kit`'e `NSPopover` ve `NSParagraphStyle` bayrakları** (yalnız
  başlık, `Cargo.lock` oynamadı; `make denetim`'in `Cargo.toml` uyarısının
  karar kaydı bu satır).

## Checklist

- [x] `bt-core`: ton, boş iz, `Hide files`'ın kalkması, `transfer_button_span`, `→`
- [x] `bt-gpu`: çubuğun zemini
- [x] `upload`: kalemler, sonuç/bildirim metinleri, sayfa, durdurma, liste, başlık; yapıştırma kalktı
- [x] `uploader`/`window`/`view`: popover, durdurma sayfası, başlık, bildirim
- [x] `CLAUDE.md`
- [x] Test: yukarıdaki Kabul maddeleri
- [x] Doğrulama geçti (`make hepsi` + `make test-yaris` + `make duman`)
- [x] Gözle kontrol (devir mesajının cümlesi): ssh sekmesinde (1) 96 MB'lık tek dosya bırak → sayfa `Upload “backup.tar.gz” to host?` + `96.0 MB → dir`; (2) üç dosyayı birden bırak → tek sayfada adlar, yükleme sürerken `Added to the queue…`; (3) `Show files (3)` → popover (çubuk, `Waiting`, `✓ Uploaded`, hedef, `Cancel`/`Remove`), dışarı tık / Esc / düğmeye yeniden basış kapatıyor; (4) 30 sn'den sonra ⌘. → `Stop all uploads?`, Esc `Keep uploading`; (5) uzakta `vim` → dock yok, başlık ve sekme `↑ N% · ⇄ host`, ⌘. çalışıyor; (6) başka uygulamaya geç → bitince macOS bildirimi; sonuç satırı `✓ 3 files → dir` yeşil, hiçbir yol kabuğa yazılmadı, prod'da çubuk camgöbeği dolup kırmızı izde ilerliyor
