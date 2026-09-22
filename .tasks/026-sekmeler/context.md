# Sekmeler — Bağlam

## Mevcut Durum

bateri **tek pencere, tek oturum**. Varsayım kodun her katında adıyla duruyor:

- **`bt-shell/src/app.rs` → `Ivars`**: `AppDelegate` pencereyi, view'ı,
  `DisplayLink`'i, `Session`'ı, `ShellWake`'i, dock payını
  (`dock_rows`, `dock_rows_at_birth`), alt başlığın yuvalarını (`notices`) ve
  geçici puntoyu (`zoom`) **tekil** alanlar olarak taşıyor. Pencere
  `applicationDidFinishLaunching:` içinde bir kez kuruluyor, `AppDelegate`
  hem uygulamanın hem pencerenin delegate'i (`NSWindowDelegate`: resize,
  backing, örtülme, odak).
- **`lib.rs` başlığı**: "Tek pencere; sekme, bölme ve IME sonraki setlerde."
  `did_finish_launching` AppKit'in pencere sekmelerini **bilerek kapatıyor**
  (`NSWindow::setAllowsAutomaticWindowTabbing(false)`; yorum: "Sekmeler kendi
  setinde ve yolu orada seçilecek").
- **Kapanış tek yoldan**: `ShellWake::child_exit` → ana kuyruk →
  `terminate:`; yani kabuk çıkınca **uygulama** biter.
  `applicationShouldTerminateAfterLastWindowClosed:` → `true`.
  `AppDelegate::shutdown` tek bir `Session::shutdown`'ı en çok
  `SHUTDOWN_GRACE` bekliyor (`CLAUDE.md` → Kapanış sınırlı bekler).
- **Kayıt anı yolları tek hedefe yazıyor**: `reload_settings`,
  `apply_appearance`, `apply_font`, `apply_caret`, `apply_reduce_motion`,
  `apply_focus`, `post_notices` hepsi `self.ivars().session/link/window`'u
  okuyor.
- **Alternatif ekran habercisi pencereden habersiz**:
  `notify_alt_screen_changed` hedefsiz eylemle `altScreenDidChange:`'e
  varıyor ve alıcı **"o"** oturumu yeniden okuyor.
- **Renderer paylaşımlı doğmuş ama tek ölçekli**: `bt_shell::run`
  `Rc<Renderer>` kuruyor; `Renderer::surface()` her çağrıda yeni bir
  `CAMetalLayer` veriyor, `draw`/`cell_metrics` `&self` alıyor. Ama atlasın
  anahtarı **(aile, punto, ölçek, satır aralığı)** ve `sync_atlas` anahtar
  değişince atlası baştan kuruyor (`renderer.rs` → `sync_atlas`,
  `Atlas::ensure`). İki pencere iki ölçekte (Retina + harici 1x ekran) ya da
  iki puntoda aynı renderer'ı paylaşırsa atlas her geometri olayında
  öbürününkine döner.
- **Görünmez pencere zaten kare üretmiyor**:
  `windowDidChangeOcclusionState:` → `DisplayLink::set_visible(false)` link'i
  uyutuyor, uçuştaki kaymayı hedefinde bitiriyor; dönüşte tek kare istiyor.
- **Pencere başlığı sabit** (`"bateri"`). OSC 0/2 başlık dizisi alacritty'den
  `Event::Title`/`ResetTitle` olarak geliyor ve `Adapter`'da **yutuluyor**
  (`session.rs` → `send_event`); olay `Term` kilidi **tutulurken** geliyor ve
  kolun kilit almaması gerektiği `set_terminal_options`'ın doc'unda yazılı.
  Çalışma dizini OSC 7'den `ShellLog::context.cwd`'ye yazılıyor ama dışarıya
  yalnız dock hücresi olarak çıkıyor — `Session`'da dizin okuyucusu **yok**.
  OSC 7'yi sarmalayıcı her `precmd`'de basıyor, `blocks` kademesinde de
  (`assets/shell/zsh/bateri.zsh` → `__bateri_cwd`).
- **Klavye**: menü kısayolları `performKeyEquivalent:` ile `keyDown:`'dan önce
  yakalanıyor; yakalanmayan Cmd'li tuş `keyDown:`'da kapalı izin listesi
  (⌘⌫ ⌘← ⌘→) dışında yutuluyor (`view.rs` → `reaches_terminal`). Control'lü
  olay metin yığınına girmeden `keys::encode_key`'e düşüyor. Ana menü üç
  menü: uygulama, Edit, View (`menu.rs`); **Window menüsü yok**
  (`setWindowsMenu` çağrılmıyor).
- **Süreli koşu** (`BT_RUN_SECONDS`) tek pencereyle koşuyor; `kapanis=`
  jetonu tek `Teardown`'dan (`teardown_token`).

## Motivasyon

Kullanıcı isteği (2026-09-23): *"Best practice'lere uygun klavye kısayolları
vs yapılmış tab sistemi. Temiz olsun — tasarımın temiz olması çok önemli.
Gerekli yerlerde animasyonlar, akıcılık."* Kapsam **yalnız sekme**; yol
haritasındaki "sekme + bölme" satırı ikiye ayrıldı, bölme kendi satırında
bekliyor (`docs/YOL-HARITASI.md`).

Setin asıl sorusu yol haritasının kayıtlı bedeli: komut blokları, Input
Dock ve doldurma bandı "bir pencere = bir oturum" varsayımıyla indi. Sekmeyi
nereye koyduğumuz bu varsayımın **retrofit edilip edilmeyeceğini** belirliyor
— `discussion.md` → Karar 1.

**Referans ürün** (`docs/ARASTIRMA.md`): sekme + iki eksenli bölme var
(satır 173–174), her sekme ve bölme ayrı kabuk süreci (satır 46), pasif
sekmelerin drawable'ları bırakılıyor (satır 36), komut paleti sekmeleri de
listeliyor. Binary'deki dosya adları `mt-gpu`'da `renderer/chrome`,
`renderer/session`, `tiles` ve bir `ui_text` pipeline'ı gösteriyor
(satır 25–31); `mt-shell`'de pencere/sekme modülü yok. **Çıkarım, kanıt
değil:** referans sekme çubuğunu ve bölmeleri kendi Metal yüzeyinde çiziyor
olabilir. Açık sorunlarından biri **"tab bar tema (#24)"** (satır 184) —
yani hangi yolla çizilmiş olursa olsun, çubuğun temaya uymaması referansın
kullanıcılarının da şikâyeti.

**"Temiz tasarım"ın ölçülebilir hâli** (kabul ölçütleri bundan türüyor):

1. **Tek sekmede çubuk yok** — Terminal.app ve Safari'nin varsayılanı;
   sekme açılmadıkça pencere bugünküyle aynı görünür.
2. **Başlık çubuğu ile içerik arasında dikiş yok** — başlık çubuğu temanın
   zemin rengini taşır, ayırıcı çizgi yok; bugün sistemin gri başlığı ile
   temanın siyah zemini arasında görünür bir sınır var.
3. **Çubuk temanın açık/koyuluğuna uyar** — koyu temada koyu çubuk, trafik
   ışıkları ve başlık metni okunur.
4. **Hareket sistemin** — sekme açma, kapama, sürükleyip sıralama ve
   pencereden koparma macOS'un kendi animasyonuyla; bateri'nin ekleyeceği her
   animasyon bir gerekçe ve durma koşulu taşır (`CLAUDE.md` → Boşta sıfır
   kare).
5. **Arka plandaki sekme sıfır kare çizer**, içinde komut koşsa bile; öne
   gelince bir kare, animasyon tekrarı yok.
