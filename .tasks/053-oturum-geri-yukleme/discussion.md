# Oturum geri yükleme — Tartışma

Karar-listesi biçimi: birbirinden bağımsız karar noktaları.

## Karar 1: Kimin mekanizması — AppKit'in pencere geri yüklemesi mi, kendi dosyamız mı?

- **A) AppKit (`NSWindowRestoration`, `encodeRestorableStateWithCoder:`).**
  Sistemin "Close windows when quitting an application" ayarı macOS'ta
  varsayılan **açık** (`NSQuitAlwaysKeepsWindows` yok): ⌘Q'da ve Sparkle'ın
  `terminate:`'inde AppKit hiçbir şey geri yüklemez, yani setin varlık
  sebebi varsayılan kurulumda çalışmaz. Kodlayıcıya ızgarayı koymak da
  `NSCoder` + kendi arşiv biçimi demek; bölme ağacı ve sekme grubu yine elle.
- **B) Kendi dosyamız.** `~/Library/Application Support/bateri/session/`
  (`remote-hosts`'un emsali; `settings.toml` değil — o elle düzenlenen dosya).
  Kayıt `applicationWillTerminate:`'te, açılış `did_finish_launching`'te.
  Pencereler `setRestorable(false)`: AppKit bir gün kendi kopyasını açarsa
  çift pencere doğardı.

## Karar 2: Biçim ve bağımlılık

- **A) `toml_edit` düzen dosyası + pane başına VT bayt akışı.** Düzen
  (pencere, sekme, ağaç, odak, zoom, pane alanları) küçük ve insan okunur bir
  TOML; geçmiş pane başına ayrı bir dosyada, `bt-core`'un ürettiği **VT
  baytları** (SGR + metin + satır sonu). Oynatma yeni `Term`'e aynı
  ayrıştırıcıdan gider, yani renk, biçim, alt çizgi rengi ve emoji kümeleri
  ayrıştırıcının zaten bildiği yoldan geri kurulur; sarılan satır satır sonu
  taşımadığı için yeni genişlikte kendiliğinden yeniden sarılır. Yeni crate
  yok. `toml_edit` `bt-shell-common`'a **yeni bir kenar** olarak girer
  (`polling`/`unicode-width`/`cursor-icon` emsali: grafta var, sürüm
  oynamıyor; `Cargo.lock`'a yalnız liste satırı).
- **B) `serde` + `serde_json`/`bincode` + alacritty'nin `serde` özelliği.**
  `Grid<Cell>`'i olduğu gibi serileştirir; Set B'nin tam VT durumuna da
  yaklaşır. Bedeli: grafa en az iki yeni crate (+ `serde_derive`'in proc-macro
  zinciri), `alacritty_terminal` tipleri dosya biçimine sızar (`bt-core`'un
  "pub API'de alacritty tipi görünmez" kuralı diskte delinir) ve biçim
  alacritty'nin iç düzenine bağlanır — `=0.26.0` pin'i her yükseldiğinde
  eski kayıt okunmaz olur.
- **C) Kendi ikili biçimimiz.** Hücre başına alan yazan özel kodlayıcı.
  A'nın her şeyini daha çok kodla yapar; SGR yolu ayrıştırıcının zaten sınanmış
  kodunu kullanıyor, C onu ikinci kez yazar.

## Karar 3: Uzak (ssh/mosh) pane geri gelince ne olur?

- **A) Yeniden bağlan.** Yerel kabuk yerel dizinde doğar ve ilk girdisi
  hedefin satırı + `\r` (`SessionOptions::initial_input`) — ⌘T'nin uzak
  sekmede yaptığının aynısı (037 Karar 6), sarmalayıcılı oturumda ilk `A`'yı
  bekliyor.
- **B) Yerel kabuk + "⏎ reconnect".** Kopan ssh'ın durum satırı
  (`DockContext::reconnect`, 037 Karar 8) geri yüklenen pane'de tohumlanır;
  kullanıcı ⏎'ye basınca bağlanır.
- **C) Düz yerel kabuk.** Uzak satır hiç saklanmaz.

## Karar 4: Hangi parçalar saklanır?

Brief: pencereler, sekmeler (grup + sıra), bölme ağacı (eksen + oran),
odaktaki pane, zoom'lanmış pane, pane'in dizini, punto farkı, teması, uzak
hedefi, geçmişin metni (+ renk/biçim). Açık noktalar: tema, pencere
çerçevesi, seçili sekme ve key pencere, `TabId`, blok şeritleri.

## Karar 5: Kullanıcının denetimi

Geçmiş diske iniyor: yazılmış parola istemi, token basan bir komut. Bir ayar
gerekiyor mu, hangi değerlerle?

## Karar 6: Dosyanın ömrü

Okunduktan sonra ne olur; çökmeden sonra ne olur; hermetik koşu?

## Muhakeme (2026-10-03)

| Mercek | Verdict |
|---|---|
| Sadelik | TEMİZ |
| Codebase-fit | SORUNLU |
| İşletme | SORUNLU |

Yönü değiştiren itiraz yok (KIRMIZI yok); hepsi Karar 2'nin biçimini, kurulum
yolunu ve kapanış/dosya işletmesini sıkılaştırıyor. İkinci tur gerekmedi.

**Kabul edilen itirazlar → plan değişikliği:**
- *`toml_edit` makine dosyası için gerekçesiz* (Sadelik, Codebase-fit):
  kütüphanenin gerekçesi elle düzenlenen dosyada yorumun ve bilinmeyen
  anahtarın korunmasıydı (007); kayıt makinenin yazdığı ve açılışta tüketilen
  bir dosya. bateri'nin kendi durum dosyası zaten satır biçiminde ve geçici ad
  + `rename` ile yazılıyor (`bt-shell-common/src/ssh_wrap.rs`, `remote-hosts`).
  → Düzen sürümlü **satır biçimi**, ağaç ön-sıralı jeton dizisi;
  `bt-shell-common → toml_edit` kenarı **yok**, `Cargo.toml` hiç değişmiyor.
- *Set B'nin uyum kuralını bu sette tasarlama* (Sadelik): → tek `version`
  tamsayısı, tanınmayan sürüm = kayıt yok; "Set B tutmazsa buraya düş"
  Set B'nin kuralı ve bekçisi bu sette yazılmıyor. Kararlı `TabId` kalıyor
  (ucuz ve Set B'nin dış kancaları ona bağlı).
- *Oranlı ağaç ardışık `add_pane` ile kurulamaz* (Codebase-fit): `add_pane`
  hedefi ortadan bölüp **hemen** `pane.start` çağırıyor
  (`bt-shell-macos/src/window.rs`); her kabuk ara boyutta `TIOCSWINSZ` görür,
  oynatılan geçmiş ara genişlikte sarılır. → `TerminalWindow::restore`: bütün
  pane'leri kayıtlı ağaçla **önce** yerleştirir, kabukları yerleşimden
  **sonra** başlatır — Set B'nin "kabuk yerine fd" noktası tam burası.
- *`PaneLaunch`'ta kimlik ve geçmiş alanı yok; `Zoom`'un adımı özel*
  (Codebase-fit): → `Launch`'a `tab_id: Option<TabId>` ve `replay`,
  `Zoom`'a adım okuyan/kuran erişimci.
- *Sabit yol birden çok kimlik/örnekle oturum çalar* (İşletme): gözle kontrol
  paketi (`dev.bateri.agent-check`) ve `open -n` aynı dizine dokunurdu.
  → Dizin **paket kimliğiyle** adlanır, paketsiz süreç (`cargo run`) geri
  yüklemez ve yazmaz (`uploader`'ın bildirim emsali), dizin bir **kilitle**
  sahiplenilir — kilidi alamayan ikinci örnek okumaz, yazmaz.
- *Kapanış adımı `shutdown`'a girer; kısmi yazım tanımsız* (İşletme):
  `CLAUDE.md` ve `app.rs` "kapanışa eklenecek her adım `AppDelegate::shutdown`'a"
  diyor. → Kayıt `shutdown`'ın başında, hermetik kapı orada; önce geçmiş
  dosyaları, **en son** düzen dosyasının `rename`'i (commit noktası); açılışta
  düzen oynatmadan **önce** tüketilir (bozuk geçmişle çöken açılış aynı kaydı
  tekrar denemesin); eksik/bozuk geçmiş yalnız o pane'i boş başlatır; yetim
  dosyalar her kayıtta süpürülür.
- *`swap_alt` yıkıcı* (İşletme): Set B'nin geri düşüşü kodlayıcıyı canlı
  `Term`'de çağırırsa vim'in ekranını bozardı. → Yöntem adıyla ve doc'uyla
  "yalnız kapanışta" (`Session::final_history`); `CLAUDE.md`'nin
  "`inactive_grid` erişilemez" cümlesi aynı phase'de düzelir.
- *Bastırılan giriş satırı ve çıpasız prompt satırları* (İşletme): OSC 8
  düşünce son prompt satırı bastırılmaz, yarım `BUFFER` geçmişin dibinde bayat
  durur. → Kodlayıcı çıpaları düşürmeden **önce** okur: kabuk `Input`'tayken son
  bloğun çıpa satırından itibaren kesilir; akış satır sonuyla biter (zsh'in
  `PROMPT_SP` işareti doğmasın). Doldurma bandı oynatılan satırları geçmiş
  olarak tepeye çeker — istenen, adıyla yazılı.
- *Kullanılamayan ayar dosyası* (Codebase-fit, İşletme; `Settings::for_unusable_file`
  emsali): geçmişi diske yazmak görünmez yan etki. → O kolda `"layout"`:
  düzen gelir, geçmiş yazılmaz.
- *Çerçeve başka ekrana / küçük ekrana düşer* (İşletme): → görünür ekrana
  kırpılır; yapraklar en küçük pane sınırının altına düşerse ağaç eşitlenir.
- *Time Machine geçmişi yedekler* (İşletme): → `docs/AYARLAR.md`'de yazılı.
- *Karar 3A diskteki dosyadan kabuğa komut + `\r`, N parola, prod host'a
  kendiliğinden bağlanma* (Codebase-fit, İşletme): 038 Karar 5'in "dış girdi
  kabuğa komut koşturmaz" değişmeziyle aynı ruhta. → Karar 3 değişti (aşağıda).

**Reddedilenler:**
- *Alt ekrandaki pane'in geçmişini hiç saklama* (Sadelik, ürün sorusu olarak
  işaretli) — vim açıkken ⌘Q yaygın yol ve kullanıcı o pane'in geçmişini
  kaybederdi; `swap_alt` kapanışa kilitlendiği için bedeli kalmadı.
- *Önce SIGHUP, kodlamayı bekleme süresinin içinde yap* (İşletme, isteğe bağlı)
  — `begin_close` oturumun tutamağını düşürüyor; kodlama oturum canlıyken
  yapılmak zorunda. Süre ölçülmedi ve plan sayı yazmıyor (`/measure`'ın işi).
- *Geçmişe `scrollback`'ten bağımsız tavan* — ürün yüzeyi ekler; tavan zaten
  kullanıcının `scrollback`'i.
- *İki değerli ayar* (Sadelik, ürün sorusu) — `"layout"` gizlilik kolu ve
  kullanılamayan dosyanın düştüğü değer; üç değer kalıyor.

## Karar (2026-10-03, otonom akış)

Panelden geçmiş öneri; `/akis` otonom modunda kullanıcı onayı alınmadı.
Ürün kararı olan iki seçim (Karar 3, Karar 5) aşağıda **ürün** diye işaretli.

- **Karar 1 → ✅ B, kendi dosyamız.** AppKit'in geri yüklemesi varsayılan
  sistem ayarında ⌘Q'da ve Sparkle'da hiç çalışmıyor. Pencereler
  `setRestorable(false)`.
- **Karar 2 → ✅ Sürümlü satır biçimi + pane başına VT bayt akışı; yeni crate
  ve yeni kenar yok.** Düzen `remote-hosts` emsalinde satır dosyası (ilk satır
  sürüm, ağaç ön-sıralı jeton dizisi, metin alanları kaçışlı); geçmiş `bt-core`'un
  SGR + metin kodlayıcısından, oynatma ayrıştırıcının + `ClusterHandler`'ın
  yolundan. `toml_edit` kenarı panelde düştü (Muhakeme).
  **Reddedilen:** `serde` + alacritty `serde` (B) — iki+ yeni crate, alacritty
  tipi diske sızar, pin her yükseldiğinde eski kayıt okunmaz; kendi ikili
  biçim (C) — ayrıştırıcının sınanmış SGR yolunu ikinci kez yazar; `toml_edit`
  düzen dosyası (A'nın ilk hâli) — gerekçesi makine dosyasında yok.
- **Karar 3 → ✅ (ürün) Uzak pane: yerel kabuk, hedefin satırı giriş
  satırında hazır, çalıştırılmamış.** Pane eski yerel dizininde doğar ve ilk
  girdisi `ssh prod` gibi hedef satırı — **`\r` olmadan**; kullanıcı ⏎'ye
  basınca bağlanır. Dock'lu ve dock'suz kabukta aynı yoldan çalışır
  (`initial_input`'un teslimi), "sekmem geri geldi" bir tuş uzakta.
  **Reddedilen:** kendiliğinden bağlanma (A) — diskteki dosyadan kabuğa
  komut koşturur (038 Karar 5'in ruhu), güncellemeden sonra N pane aynı anda
  parola sorar, `production` işaretli host'a kullanıcı dokunmadan bağlanır,
  ağ yoksa N hata; `⏎ reconnect` tohumlamak (B) — yalnız dock'lu kabukta var
  ve `DockContext::reconnect`'in "bağlantı koptu" anlamını bulandırır; düz
  yerel kabuk (C) — hedef kaybolur. Uzak dizin gelmez: canlı devrin işi.
- **Karar 4 → ✅ Saklananlar:** pencere çerçevesi (görünür ekrana kırpılı),
  sekme sırası, seçili sekme, key pencere, bölme ağacı (eksen + oran), odak,
  zoom, pane'in `TabId`'si, yerel dizini, punto adımı, uzak satırı ve geçmişi.
  **Saklanmayanlar:** tema (pane'e özgü tema yok, Theme ▸ ayar dosyasına
  yazıyor — saklamak ikinci kaynak olurdu), blok şeritleri ve süreleri
  (çıpaları yeni kabuğun numaralarıyla çakışır, rengi kabuk defterinden;
  biten bloğun işareti set sonrası geri geldi — aşağıda, Set sonrası
  düzeltmeler), dock aynası, arama, seçim, kaydırma konumu.
- **Karar 5 → ✅ (ürün) `[terminal] restore_windows = "all" | "layout" |
  "off"`, varsayılan `"all"`.** Metalterm'in adı; `"layout"` geçmiş dosyası
  yazmaz, `"off"` hiçbir şey yazmaz ve kalanı siler. Kullanılamayan ayar
  dosyasında `"layout"` (`for_unusable_file` emsali: görünmez yan etki —
  geçmişi diske yazmak — kapalıya düşer, görünen fayda kalır).
- **Karar 6 → ✅ Ömür:** dizin `~/Library/Application Support/bateri/session/{paket kimliği}/`,
  `0700`, dosyalar `0600`, kilitle sahiplenilir; paketsiz süreç ve hermetik
  koşu (`BT_RUN_SECONDS`) okumaz/yazmaz. Kayıt `AppDelegate::shutdown`'ın
  başında, önce geçmişler, en son düzenin `rename`'i; pencere yoksa kayıt
  silinir. Açılışta düzen oynatmadan önce tüketilir; çökme kurtarma kapsam
  dışı.


## Set sonrası düzeltmeler (2026-10-03, gözle kontrol)

- **Dizin adıyla geri geliyor.** Kayıt `+/tmp` tutuyordu ama başlık ve dock
  `/private/tmp` gösteriyordu: `chdir` sembolik bağı çözmüyor, kabuk ise
  `PWD`'yi miras değer `.`'yı göstermiyorsa `getcwd()`'den kuruyor ve miras
  değer bizim sürecimizinki (`/`). Çare `Session::spawn`'da: mutlak
  `working_directory` çocuğa `PWD` olarak da gidiyor, `TERM` katmanında.
  Yanlışın yönü güvenli — kabuk `.`'yı göstermeyen `PWD`'ye inanmıyor. Aynı
  kusur ⌘T/bölme mirasında da vardı (yeni pane OSC 7 dizininde doğuyor) ve
  aynı satırla kapandı. `login -qflp` ortamı koruyor (ölçüldü: `PWD=/tmp`
  ile `/tmp`, `PWD=/` ile `/private/tmp`). Bekçisi
  `working_directory_keeps_its_symlinked_name_in_pwd`.
- **Karar 4'e ek — blok işareti geri geliyor** (kullanıcı kararı
  2026-10-03, gözle kontrolde: geri yüklenen komut satırlarında chevron
  yoktu, yerinde `PS1`'in iki boşluğu). "Blok şeritleri saklanmaz" kararının
  gerekçesi (çıpalar yeni kabuğun numaralarıyla çakışır, renk defterden)
  doğru kaldı ama sonucu kısmak değil kodu açmak oldu: kodlayıcı biten
  bloğun çıpasını kendi üçüncü şemamızla yazıyor —
  `bateri://sblock/<k>.<success|error>`, `k` kayıt içinde blok sırası. Rol
  kapanıştaki defterden (`ShellLog::saved_stripes`, `history_cut` ile aynı
  yaprak kilit turunda kopya — `Term` altında defter okunamıyor), renk canlı
  temadan (`BlockKey::Saved`, `ShellLog::stripe` defteri sormadan anahtarı
  veriyor), yani tema değişince de doğru. Koşan, `Pending` ya da kodu
  okunamayan blok **çıpasız** — ölü komutu `accent`'le "koşuyor" göstermek
  yanlış, nötr rol yok; "bilinmeyen çizilmez". Süre sayacı gelmiyor.
  Devam satırı kuralı (`block_row_continues`) aynı anahtar her satırda
  olduğu için olduğu gibi çalışıyor; ızgara ve doldurma bandı aynı
  `shell.stripe` çağrısından çözülüyor. Dış OSC 8 bağlantıları eskisi gibi
  düşüyor (metni kalıyor); `bateri://` önekinin bağlantı kapısı
  (`live_hyperlink`, `links::action`) yeni şemayı da yutuyor. İkinci kayıt
  (geri yüklenmiş pane yeniden kapanınca) baytı baytına aynı çıkıyor.
  Bekçiler `snapshot::tests` (rol, devam satırı, ikinci kayıt) ve
  `final_history_saves_a_finished_blocks_anchor_with_its_role`,
  `a_replayed_saved_anchor_marks_its_command_row_in_the_live_theme`.
