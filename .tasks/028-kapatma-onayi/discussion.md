# Kapatma onayı — Tartışma

Birbirinden ayrı karar noktaları var; biçim karar-listesi. Karar 1 (kim
"koşuyor" der) ve Karar 2 (soru nasıl sorulur) mimari; kalanlar ürün
davranışının macOS emsallerine göre kurulması.

Emsaller (davranışları, ayrıntı değil):

- **Terminal.app** — "Ask before closing: Never / Always / Only if there are
  processes other than the login shell and …". Soru sekme/pencere için
  **sayfa**, çıkış için uygulama düzeyinde tek uyarı; metin koşan süreçlerin
  adlarını sayar; varsayılan düğme "Terminate", Esc iptal. Oturum kapatma
  ve yeniden başlatmada da soruyor.
- **iTerm2** — "Confirm closing multiple sessions", "Prompt before closing"
  (iş adlarına göre istisna listesi); sayfa; "bir daha sorma" kutusu.
- **Ghostty** — `confirm-close-surface` (`true`/`false`/`always`): kabuk
  entegrasyonu varsa yalnız prompt'ta değilken sorar; "Close Terminal?" /
  "Close Window?" / "Quit Ghostty?" sayfa ve uyarıları, varsayılan düğme
  "Close". Sistem kapanışı/oturum kapatmada sormadan çıkar.
- **Metalterm** — `confirm_close` = `never` / `running` / `always`
  (`docs/ARASTIRMA.md`).

## Karar 1: "Koşan süreç var mı" sorusunun yetkilisi

### Seçenek A — Süreç tablosu: PTY'nin ön plan grubu kabuğun grubu mu?

Kabuğun kimliği `login`'in çocuğu (bağlamdaki ölçüm): `bt-core` yalnız
PTY çocuğunun pid'ini verir (`Session::child_pid`, `Pty::child().id()`
sarmalanmadan önce alınır — platformsuz, `Cargo.toml`'a dokunmaz). Gerisi
`bt-shell`'de, `libc`'nin Apple yarısıyla: çocuğun `e_tpgid`'i ön plan grubu;
kabuk, `login` yolunda çocuğun çocuğu (`proc_listchildpids`), doğrudan
yolda çocuğun kendisi. Ön plan grubu kabuğun grubu değilse iş koşuyor.

**Artıları:**
- Entegrasyonsuz kabukta (`[shell] integration = "off"`, bash, fish), `ssh`
  içinde (ön planda `ssh` var — kapatmak uzak oturumu öldürür, sorulmalı),
  alt kabukta (`bash` yazıldı) aynı cevap.
- Ad aynı kaynaktan: ön plan grubunun süreçleri.
- Kare yolunda değil, yalnız kapanış anında ve ana thread'de birkaç sistem
  çağrısı; boşta sıfır kareye dokunmaz.

**Eksileri:**
- Kabuğun kendi içinde koşan iş (yerleşik döngü, `read` bekleyen bir
  fonksiyon) ayrı bir grup açmıyor — kabuk boşta görünür.
- macOS'a özgü: Linux günü `/proc` ile ikinci bir gövde.

### Seçenek B — OSC 133 safhası (`ShellPhase::Running`)

`Session::shell_state()` zaten var; `Running` ise sor.

**Artıları:**
- Yeni hiçbir şey okunmuyor.

**Eksileri:**
- Entegrasyonsuz kabukta hiç yok; `ssh`'ın içindeki uzak kabukta da
  yok (uzak tarafta entegrasyon yok) — ama `ssh` yerel tarafta `Running`,
  yani o kolu tutuyor; kör kalan `integration = "off"` ve bash/fish.
- Takılı kalabiliyor: `exec bash` gibi entegrasyonsuz bir kabuğa geçişte
  `D` hiç gelmiyor, safha kalıcı `Running` — her ⌘W sorar.
- Ad yok: sayfa "bir şey koşuyor" diyebilir, neyin koştuğunu söyleyemez.

### Seçenek C — A ∪ B

A'nın kör noktasını (yerleşik döngü) B ile kapatmak.

**Eksileri:**
- B'nin takılı kalma kusurunu da ithal ediyor: yanlış alarmın kalıcı hâli
  (her ⌘W'de soru) yerleşik döngünün nadir kaçağından pahalı.

### Adlar

Sayfa neyin kapanacağını adıyla söylemeli. Ön plan grubunun **lideri**
yanlış ad verebiliyor (bağlamdaki üçüncü satır: lider bir `bash`
sarmalayıcısı, program `claude`). Ad bu yüzden grubun **yapraklarından**
(grubun başka bir üyesinin ebeveyni olmayan üyeler), tekrarsız; yaprak
bulunamazsa liderin adı, o da yoksa genel metin.

→ ✅ A, adlar yapraklardan (Karar)

## Karar 2: Soru nasıl sorulur

### Seçenek A — Sekme/pencere için sayfa, çıkış için uygulama uyarısı

⌘W, kırmızı düğme ve ⇧⌘W `NSAlert` **sayfası**
(`beginSheetModalForWindow:completionHandler:`); ⌘Q
`applicationShouldTerminate:` içinde **eşzamanlı** `runModal` ve dönüşü
doğrudan `NSTerminateNow`/`NSTerminateCancel` — `NSTerminateLater` ve
`replyToApplicationShouldTerminate:` gerekmiyor.

**Artıları:**
- Üç emsalin üçü de böyle: pencereye ait soru pencereye yapışık, uygulamaya
  ait soru uygulama düzeyinde.
- Sayfa yalnız o pencereyi kilitliyor; öteki pencerelerde çalışmak sürüyor.

**Eksileri:**
- Sayfanın tamamlanma bloğu `objc2-app-kit`'in `block2` bayrağını ve
  `bt-shell`'e `block2` kenarını istiyor. **Yeni crate değil** (grafta
  `bt-gpu` üzerinden var, sürüm oynamıyor) ama `Cargo.lock`'ta iki
  bağımlılık listesi büyüyor ve `make denetim` uyarır — kayıt bu karar.

### Seçenek B — Her yerde `runModal`

**Artıları:** `Cargo.lock` oynamaz (yalnız `NSAlert` bayrağı).

**Eksileri:** ⌘W'nin sorusu uygulama modali bir panel olur: pencereden
kopuk ekranın ortasında durur ve bütün uygulamayı kilitler. Emsallerin
hiçbiri böyle değil; kullanıcının göreceği fark.

### Seçenek C — Eski delege seçicili sayfa

`beginSheetModalForWindow:modalDelegate:didEndSelector:contextInfo:` blok
istemiyor ama `#[deprecated]` ve `clippy -D warnings` altında `allow`
ister; `contextInfo` ham işaretçi.

**Eksileri:** kaldırılmış bir API'ye yeni kod yazmak; kazancı yalnız iki
kenar.

→ ✅ A — sayfa + ⌘Q `runModal` (Karar)

## Karar 3: Hangi yol sorar

| yol | bugün | yeni |
|---|---|---|
| ⌘W / Close Tab | `performClose:` | `windowShouldClose:` sorar (o sekme) |
| kırmızı düğme | `performClose:` | AppKit'in bugünkü kapsamıyla aynı; kapsam bir grupsa ⇧⌘W'nin tek sorusu |
| ⇧⌘W / Close Window | her sekmeye `performClose:` | grubun tamamı için **tek** sayfa, onayda her sekme `close` |
| ⌘Q, Dock ▸ Quit | `terminate:` | `applicationShouldTerminate:` tek uyarı |
| kabuğun `exit`'i | `close` | değişmez — `close` delegate'e sormaz |
| süreli koşu | — | hiçbir yol sormaz |

- Kırmızı düğmenin çok sekmeli pencerede bir sekmeyi mi grubu mu kapattığı
  **ölçülmedi**; kural kapsamı AppKit'ten almak ve N ayrı sayfa
  doğurmamak. Ölçüm ve kolu phase-2'de.
- Sayfa zaten açıkken ikinci bir ⌘W yeni sayfa açmaz.
- Sayfa açıkken süreç biterse sayfa kalır (emsal: Terminal.app, Ghostty);
  kabuk çıkarsa pencere `close` ile gider ve sayfa onunla.

→ ✅ tablo; süreli koşu kapısı `run`'da (Karar)

## Karar 4: Metin ve düğmeler

- Başlık: "Close this tab?" (grupta birden çok sekme varken ⌘W) / "Close
  this window?" / "Quit bateri?".
- Açıklama süreçleri adıyla sayar: "“claude” is still running. Closing
  ends it." · çok sekmede "Processes are running in 2 tabs: “claude”,
  “vim”. …". `always` kolunda koşan bir şey yoksa kapanacak şeyi söyler.
- Düğmeler "Close" / "Quit" (ilk, Return) ve "Cancel" (Esc — `NSAlert`
  "Cancel" başlıklı düğmeye Esc'i kendisi bağlıyor). Varsayılan düğmenin
  kapatmak olması emsallerin ortak kararı: soru bir hız tümseği, ⌘W'ye
  basan kullanıcının niyeti kapatmak.
- "Bir daha sorma" kutusu yok: kalıcı cevabın yeri ayar anahtarı (Karar 6).

→ ✅ Close/Quit varsayılan, Cancel Esc (Karar)

## Karar 5: Sistem kapanışı ve oturum kapatma

macOS oturum kapatma/yeniden başlatmada da `terminate:` gönderiyor ve
`applicationShouldTerminate:`'ten geçiyor.

- **(a) Sor** (Terminal.app): koşan Claude Code oturumu gece kurulan bir
  güncellemenin yeniden başlatmasında da korunur; bedeli sistemin "bateri
  yeniden başlatmayı durdurdu" demesi.
- **(b) Sorma** (Ghostty): Apple Event'in sebebi okunur
  (`kAEQuitReason`), sistem kaynaklıysa doğrudan çıkılır.

→ ✅ (a) sor (Karar)

## Karar 6: Ayar

`confirm_close = "never" | "running" | "always"`, varsayılan `"running"`.
Bölüm `[terminal]` — tek anahtar için yeni bölüm açmak şablonu da
belgeyi de büyütür; anahtar referansla aynı adı taşıyor. Değer kapanış
anında `AppDelegate`'in güncel ayarından okunuyor, yani kayıt anında
geçerli olması bedava (`Changes`'e ve oturumlara yeni yol yok).
Tanınmayan değer kendi anahtarını değiştirmez (`parse_keeping`).

→ ✅ `[terminal] confirm_close`, varsayılan `running` (Karar)

## Karar 7: Kapsam dışı

- **Arka plan işleri** (`sleep 100 &`): ön planda değiller; Terminal.app
  sayar, Ghostty saymaz. Saymak `disown`/`nohup` ile kapanıştan sağ
  çıkacak süreçleri de sayar (yanlış alarm) ve zsh `exit`'te işleri zaten
  kendisi uyarıyor. Bilinen sınır.
- **Kabuk yerleşiği döngüsü** — Karar 1 A'nın kör noktası, bilinen sınır.
- **`close_on_exit`**, bölme, "bir daha sorma" kutusu.

→ ✅ (Karar)

## Karar Noktaları

Ürün sorusu yok: bariz beklenti (koşan süreçte sor, boşta sorma, `exit`'te
sorma) talebin kendisi; kalan seçimler emsallerle kapanıyor.

## Muhakeme (2026-09-23)

Önerilen: Karar 1 A (süreç tablosu, adlar yapraklardan), 2 A (sayfa +
`runModal`), 3 tablo, 4 Close/Cancel, 5 (a), 6 `[terminal] confirm_close`,
7.

| Mercek | Verdict |
|---|---|
| Sadelik | TEMİZ — A ve sayfa doğru; doğrudan yol kolu ve kırmızı düğmenin iki kolu kırpılabilir |
| Codebase-fit | SORUNLU (hafif) — yön doğru; süreli koşu kapısı, bloğun yakaladığı şey ve ayarın `terminal()` dışında kalması adıyla yazılmalı |
| İşletme | SORUNLU — login root'a ait, `e_tpgid`'i okunamıyor (ölçtü); süreli koşuda asılma; bayrak listesi eksik |

**Kabul edilen itirazlar → plan değişikliği:**

- **`login` root'a ait, `PROC_PIDTBSDINFO` onda 0 dönüyor** (İşletme,
  ölçüldü: 18920 ve 39528'de 0, aynı çağrı kabuklarda 136 ve `e_tpgid`
  `ps`'in TPGID'iyle aynı; `PROC_PIDT_SHORTBSDINFO` ve
  `proc_listchildpids` root'un login'inde de çalışıyor). → Ön plan grubu
  **kabuğun** `e_tpgid`'inden okunur; login atlaması ve ön plan liderinin
  ebeveyni/grubu/adı kısa bilgiden (`SHORTBSDINFO`, `sudo` ile başlamış root
  bir grubun adını da verir). Sahte tabloyla yazılan saf sınama bu kusuru
  göremezdi — o yüzden phase-1'de gerçek PTY sınaması da var.
- **Tespitin başarısızlık kolu adıyla** (İşletme): `login`'in henüz çocuğu
  yoksa (sekme doğar doğmaz ⌘W) boşta; çocuk var ama bilgi okunamıyorsa
  **koşuyor** sayılır ve genel metinle sorulur — sistematik bir kırılma
  sessiz "hiç sormuyor" değil görünür "hep soruyor" olur. Okuyucu thread'i
  ölmüşse (`Session::reader_alive`) boşta: kabuk gitti, bayat pid'e
  sorulmaz.
- **Süreli koşu kapısı `run`'da ve ilk satırda** (Codebase-fit, İşletme):
  süreli koşu ayar okumuyor, yani `confirm_close` orada varsayılan
  `running`; kapı sorunun cevabına bırakılsaydı başsız bir `runModal` bekçi
  kurulmadan asılır ve `make duman` hiç bitmezdi. → Karar saf bir
  fonksiyonda (`should_ask`: süreli koşu → hayır, önce), sınanıyor;
  `applicationShouldTerminate:` ve `windowShouldClose:` onu süreç
  tablosuna **dokunmadan önce** soruyor.
- **Blok yalnız pencere kimliğini yakalıyor** (Codebase-fit, İşletme):
  `alt_screen_notifier`'ın ve `child_exit`'in örüntüsü — pencere
  `app.window(id)` ile bulunuyor, bulunamazsa (sayfa açıkken kabuk çıktı)
  iş düşüyor. Açık sayfanın yuvası `WindowIvars`'ta: `NSAlert`'i yaşatıyor ve
  "sayfa açıkken ikinci soru yok" kapısı o.
- **`confirm_close` `TerminalOptions`'a girmiyor** (Codebase-fit, İşletme):
  `terminal()` izdüşümüne ve `changes().terminal`'a girseydi her kayıtta
  bütün pencerelere boşuna `set_terminal_options` koşardı. Emsal `caret`.
- **Bayrak listesi tam** (İşletme): `addButtonWithTitle:` `NSButton` +
  `NSControl` istiyor. Toplam `NSAlert`, `NSButton`, `NSControl`, `block2`
  ve `bt-shell`'e `block2` kenarı; `Cargo.lock` iki listede birer satır.
  Kilit dosyası değiştiği için phase riskli — son phase olduğu için set
  kapısı kapsıyor.
- **Bayatlayan üç yazılı iddia aynı commit'te** (Codebase-fit):
  `bt-shell/Cargo.toml`'daki "`block2` kenarı yok", `app.rs`'teki "block2
  yok: zamanlayıcı performSelector ile", `will_terminate`'in "kapatma onayı
  yok" cümlesi ve `CLAUDE.md`'nin katman tablosu (`block2`, `libc`'nin
  `proc_*` kullanımı).
- **Kırmızı düğmenin kapsamı önce ölçülür, yalnız var olan kol yazılır**
  (Sadelik) — ve ölçüm sekme çubuğunun "Close Other Tabs"ını da kapsar
  (Codebase-fit: o da arka sekmelere birer `performClose:` gönderiyor).
  Soruyu kuran tek fonksiyon pencere listesi alıyor (⌘W bir, ⇧⌘W grup, ⌘Q
  hepsi), yani ölçüm hangi kolu gösterirse göstersin yeni gövde doğmuyor.
- **Anahtar phase-2'ye** (İşletme): okunmayan bir anahtarı yayınlayan commit
  olmasın.
- **`runModal`'dan önce uygulama öne alınır** (İşletme): arka plandaki
  uygulamanın modali pencerelerin arkasında kalabilir; Dock ▸ Quit tam o
  yol. Gözle kontrolde doğrulanır.

**Reddedilenler:**

- *Tek kural, bayraksız: "boşta ⇔ ön plan grubu çocuğun kendisi ya da
  doğrudan çocuğu"* (Codebase-fit, Sadelik) — `login` yolunda doğru, ama
  doğrudan yolda (çocuk kabuğun kendisi) `sleep`'in ebeveyni de çocuk ve
  koşan iş boşta görünürdü. Doğrudan yol bugün pencerede koşmuyor ama
  **gerçek PTY sınamasının** koştuğu yol o (login root izni istiyor ve
  sınamada doğurulamıyor). Çocuğun ne olduğu (`login` mi kabuk mu) kabuğu
  doğuran `bt-shell`'in bildiği bir olgu; ad karşılaştırması değil, doğuran
  tarafın kaydı. İki kol da sahte tabloyla, doğrudan kol gerçek PTY'yle
  sınanıyor — "sınanmayan if" kalmıyor.
- *`tcgetpgrp(master)` ile `bt-core`'da POSIX yolu* (Codebase-fit, "sor"
  diye) — `bt-core`'a `libc` kenarı ve master fd'nin bir kopyasını ister;
  kabuğun kimliği ve adlar yine platform tablosundan gelmek zorunda, yani
  Linux günü ikinci gövde yine doğuyor. Kazancı yok.
- *`child_pid` canlılığı için `proc_listchildpids(getpid())`* (İşletme) —
  biçiciyle yarışı doğrulanmadı; `reader_alive` aynı soruyu var olan bir
  API'yle cevaplıyor.

## Karar (2026-09-23, otonom akış)

- **Seçilen (Karar 1):** A — süreç tablosu yetkili. `bt-core` yalnız
  `Session::child_pid` verir; tespit `bt-shell`'de, `libc`'nin Apple
  yarısıyla (yeni bağımlılık yok). Kabuk: `login` yolunda çocuğun çocuğu
  (`proc_listchildpids`), doğrudan yolda çocuğun kendisi — hangisi olduğunu
  kabuğu doğuran `bt-shell` kaydeder. Ön plan grubu kabuğun `e_tpgid`'i
  (`PROC_PIDTBSDINFO`, aynı kullanıcı); lider ve grup üyeleri kısa bilgiden
  (`PROC_PIDT_SHORTBSDINFO`). Ön plan grubu kabuğun grubu değilse iş
  koşuyor. Adlar grubun yapraklarından, tekrarsız; yoksa liderin, o da
  yoksa genel metin. Başarısızlık kolu: çocuksuz `login` ve ölü okuyucu →
  boşta, okunamayan tablo → koşuyor (genel metin). Karar saf bir
  fonksiyonda, kirli yarı ince bir okuyucu.
- **Seçilen (Karar 2):** A — ⌘W / kırmızı / ⇧⌘W için sayfa, ⌘Q için
  `applicationShouldTerminate:`'te `runModal` (uygulama önce öne alınır).
  **Bağımlılık kaydı:** `objc2-app-kit`'in `NSAlert`, `NSButton`,
  `NSControl` ve `block2` bayrakları, `bt-shell`'e `block2` kenarı. Yeni
  crate ve sürüm değişimi yok (`block2` 0.6 grafta `bt-gpu` ve `dispatch2`
  üzerinden); `Cargo.lock`'ta `objc2-app-kit`'in ve `bt-shell`'in
  listelerine birer satır — `make denetim`'in uyarısının kaydı bu madde.
  Blok yalnız pencere kimliğini yakalar.
- **Seçilen (Karar 3):** tablodaki gibi; süreli koşu kapısı `run`'da, ilk
  satırda. Kırmızı düğme ve "Close Other Tabs" önce ölçülür; kural "bir
  jest, en çok bir soru".
- **Seçilen (Karar 4):** "Close"/"Quit" varsayılan (Return), "Cancel" Esc;
  metin süreçleri adıyla sayar.
- **Seçilen (Karar 5):** (a) — sistem kapanışı ve oturum kapatmada da
  sorulur (Terminal.app emsali). Koşan bir Claude Code oturumunu bir
  yeniden başlatma da sessizce öldürmemeli; iptal edilmiş bir yeniden
  başlatma geri alınabilir, öldürülmüş oturum alınamaz. İstemeyen
  `confirm_close = "never"` der.
- **Seçilen (Karar 6):** `[terminal] confirm_close = "never" | "running" |
  "always"`, varsayılan `"running"`, kapanış anında okunur,
  `TerminalOptions`'ın dışında.
- **Seçilen (Karar 7):** arka plan işleri, kabuk yerleşiği döngüsü ve
  `exec vim` (kabuğun pid'ini devralan program boşta görünür) bilinen
  sınır; "bir daha sorma" kutusu, `close_on_exit`, bölme kapsam dışı.
- **Reddedilen:** 1 B (entegrasyonsuz kabukta kör, `exec bash`'ta kalıcı
  `Running`), 1 C (B'nin kalıcı yanlış alarmını ithal ediyor), 2 B (⌘W'nin
  sorusu pencereden kopuk ve bütün uygulamayı kilitliyor — emsallerin
  hiçbiri), 2 C (kaldırılmış API), 5 (b) (sistem kaynaklı çıkış da bir
  kayıp), bayraksız tek kural ve `tcgetpgrp` (Muhakeme → Reddedilenler).
