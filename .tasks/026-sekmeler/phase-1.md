# Phase 1 — Pencereyi nesneye çıkar

## Özet

Pencere başına durum `AppDelegate`'ten kendi nesnesine taşınır; tek pencere,
davranış ve duman jetonları bit bit aynı.

_Requirements: R1, R1.1, R1.2, R1.3, R1.4_

## Değişiklikler

- **`crates/bt-shell/src/window.rs`** (yeni) — pencere başına nesne
  (`TerminalWindow` ya da eşdeğeri) ve onun `NSWindowDelegate`'i
  (`define_class!`). Taşıdığı: `NSWindow`, `BateriView`, `Surface`, kendi
  `Rc<Renderer>`'ı (Karar 2a), `DisplayLink`, `Arc<Session>`, `Arc<ShellWake>`,
  `dock_rows` + `dock_rows_at_birth`, `zoom`. Pencere kurulumu
  (`did_finish_launching`'teki stil maskesi, layer-önce-`wantsLayer` sırası,
  `setAcceptsMouseMovedEvents`, first responder, `releasedWhenClosed(false)`)
  ve `start_session` / `sync_geometry` / `refresh_geometry` /
  `alt_screen_did_change` / `apply_focus` buraya iner; yorumları ve
  gerekçeleri kodla birlikte taşınır, kopyalanmaz. `NSWindowDelegate`
  olayları (resize, backing, örtülme, key/resign) **kendi** penceresinin
  yöntemlerine varır.
- **`crates/bt-shell/src/app.rs`** — `Ivars` uygulama geneline daralır:
  ayarlar, `config_watch`/`theme_watch`, `notices`, `stats`, `run` ve
  pencere listesi. Kayıt anı yolları (`reload_settings`, `apply_appearance`,
  `apply_font` — artık pencerenin zoom'uyla —, `apply_caret`,
  `apply_reduce_motion`, `post_notices`'in alt başlık yazımı) **listeyi
  dolaşır**. `shutdown` listeyi dolaşır (bu phase'de hâlâ seri; tek pencere).
  `load_settings` ile `choose_theme` uygulama genelinde kalır, fontu
  pencerelerin renderer'larına verir. `report_and_exit` sayaçları pencere
  nesnesinden okur; süreli koşuda tek pencere olduğu için değerler aynı.
- **Eylemlerin yönü** — yayılan eylemler (`settingsDidChange:`,
  `appearanceDidChange:`, `selectTheme:`, `matchSystemTheme:`,
  `openSettings:`) `AppDelegate`'te kalır ve pencere nesnesi bu seçicileri
  **uygulamaz** (uygularsa key pencere onları yutar). Pencereye ait eylemler
  (`makeFontBigger:`, `makeFontSmaller:`, `resetFontSize:`) pencere
  delegate'ine iner ve kendi penceresinin zoom'unu değiştirir — sekme başına
  puntonun (Karar 3) ön koşulu.
- **Alternatif ekran habercisi** (`notify_alt_screen_changed`) hedefsiz
  eylemi **bırakır**: closure pencerenin kimliğini (kendi sayacımız)
  yakalar, `exec_async` içinde `AppDelegate`'in listesinden pencereyi bulur,
  bulamazsa düşer. Responder zinciri key pencereye gider — arka sekmede
  vim'den çıkış yanlış pencereyi boyutlandırırdı. "Hiçbir şey yakalamıyor"
  kuralının gerekçesi referans çemberiydi; bir tamsayı onu açmaz, doc bunu
  söyler.
- **`crates/bt-shell/src/lib.rs`** — `run` artık `Renderer` kurmuyor; ilk
  pencere kendi renderer'ını kuruyor. `Run::stats_since`'in doc'u ("açılışın
  en pahalı parçası") damganın hâlâ ilk renderer'dan **önce** alındığını
  doğrular, gerekirse düzeltir.
- **`crates/bt-shell/src/view.rs`** — `viewDidChangeEffectiveAppearance`'ın
  hedefsiz eylemi aynen; alıcı uygulama genelinde ve bütün pencerelere uygular.

## Kabul

- `make duman` jeton satırı bugünküyle aynı: `hucre=8 glif=6 kural=15`,
  `kapanis=clean`, `icerik` ≤ `IDLE_FRAME_LIMIT`.
- `app.rs`'in bugünkü sınamaları (`split_into_grid`, dock payı, jeton satırı,
  verdict, hermetik girişler) yerlerinde ve yeşil; pencereye taşınan saf
  fonksiyonların sınamaları onlarla birlikte taşınmış.
- Uygulama elle açıldığında tek pencere, tema/ayar kaydı canlı uygulanıyor,
  Cmd +/−/0, vim'e girip çıkınca dock kalkıp iniyor — bugünkü gibi.

## Checklist

- [x] Pencere başına durum `window.rs`'e, uygulama geneli `app.rs`'te
- [x] `NSWindowDelegate` pencere nesnesinde; olaylar kendi penceresine
- [x] Kayıt anı yolları pencere listesini dolaşıyor
- [x] Yayılan eylemler `AppDelegate`'te, punto eylemleri pencere delegate'inde
- [x] Alternatif ekran habercisi pencere kimliği taşıyor
- [x] Her pencerenin kendi `Renderer`'ı; `bt-gpu` API'si değişmedi
- [x] Test: taşınan saf fonksiyonların sınamaları yeşil
- [~] Doğrulama: `make hepsi` geçti; `make duman` [~] ortam — başka bir oturumdan kalan `bateri` penceresi ekranda, HEAD 3cdc11a ve 0c66b02'de de birebir aynı `MotionUnsettled`; set sonunda kullanıcı yeniden koşacak

## Uygulama Notları

- **Saf fonksiyonlar `app.rs`'te kaldı** (`Grid`, `split_into_grid`,
  `dock_rows_for`, `dock_rows_at_birth`, `shell_integration_env`;
  `pub(crate)`), sınamaları da yerinde — taşınan sınama yok. Entegrasyon
  kararı `Inputs`'a bağlı ve `Inputs` `app`'e özel; pencere onu
  `AppDelegate::shell_integration()`'dan tek çağrıyla (ortam + doğum payı)
  alıyor.
- **Pencere uygulama delegate'ine referans tutmuyor**: `app::delegate(mtm)`
  `NSApp.delegate()`'ten downcast ediyor. Üç çağıran: alternatif ekran
  habercisi (kimlik → `AppDelegate::window(id)`), punto eylemleri (ayarın
  fontu), geometri (font tanısının alt başlığı). Kayıt anı yolları ise
  pencere yöntemlerine `&AppDelegate` geçiyor.
- **Renderer hatası**: `run` renderer kurmuyor ama imzası (`Result<(),
  GpuError>`) bin crate'ine dokunmamak için duruyor ve `Err` bugün dönmüyor;
  hata `didFinishLaunching`'te aynı satırla (`bateri: {hata}`) ve çıkış 1 ile
  basılıyor. phase-3'te ⌘T/⌘N'in `TerminalWindow::new` hatası süreci
  bitirmemeli — o yol bu kalıbı kopyalamamalı.
- **Hareketi Azalt ikiye bölündü**: pencerenin ilk değeri kendi
  `start_session`'ında (`AppDelegate::reduce_motion()`), sistem gözlemcisi
  uygulama genelinde ve `didFinishLaunching`'te bir kez
  (`observe_reduce_motion`). Hermetik koşuda çözülmüş değer `false` ve link o
  değerle doğuyor, yani ilk çağrı no-op.
- **İki "oturum yok" kapısı liste diline çevrildi**: `reload_settings` artık
  oturumsuzken erken dönmüyor (her pencere yöntemi kendi yuvasına bakıp
  sessizce dönüyor; olay `didFinishLaunching` dönmeden düşemediği için
  ulaşılmaz dal), `apply_appearance`'ın kapısı "hiçbir pencerede oturum yok".
- **Kapanış ve rapor ilk pencereyi okuyor**: `shutdown` listeyi sırayla
  dolaşıp ilk pencerenin `Teardown`'unu, `report_and_exit` ile `sessiz=`
  ilk pencerenin sayaçlarını veriyor; süreli koşuda tek pencere olduğu için
  değerler aynı.
- `docs/YOL-HARITASI.md`'de tek bir işaretçi (`AppDelegate::apply_focus` →
  `TerminalWindow::apply_focus`) bu commit'te düzeltildi; set sonundaki yol
  haritası işi ayrı.
