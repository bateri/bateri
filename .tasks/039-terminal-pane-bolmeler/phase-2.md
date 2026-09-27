# Phase 2 — Sınırı kapat: arama, yükleme, eylem API'si, sahip arayüzü

## Özet

Arama panelini, yükleme kuyruğunu ve pane düzeyindeki menü işlerini pane'e
taşı; girdileri `PaneLaunch`'la, olayları `PaneHost`'la ver ve pane'in
`AppDelegate`'e uzanan son yolunu kes. Davranış değişmez.

_Requirements: R2.1, R2.2, R2.3, R5_

## Değişiklikler

- **`crates/bt-shell/src/pane.rs`** — Arama ivar'ları (`search`,
  `search_status`, `search_driving`) ve yöntemleri (`open_search`,
  `apply_search`, `kick_search`, `search_step`, `close_search`,
  `use_selection`, `search_cover`), arama alanının delegesi
  (`NSSearchFieldDelegate` ve `control:textView:doCommandBySelector:`) ve
  `SearchBar`'ın `target`/`delegate`'i pane. Pane düzeyindeki seçiciler
  pane'de (Karar 2: `makeFontBigger:`, `makeFontSmaller:`, `resetFontSize:`,
  `findInScrollback:`, `findNextMatch:`, `findPreviousMatch:`,
  `useSelectionForFind:`, `clearToStart:`, `clearScrollback:`,
  `scrollToTop:`, `scrollToBottom:`, `scrollPageUp:`, `scrollPageDown:`,
  `closeSearch:`, `searchFieldChanged:`, `searchOptionsChanged:`,
  `cancelUpload:`, `uploadRowAction:`) ve onların `validateMenuItem:` kolları;
  her seçici pane'in adlı bir eylem yöntemini çağırıyor (R2.3 — menüsüz
  sahip de aynı yöntemi çağırabilmeli). Yükleme ivar'ları (`uploads`,
  `upload_alert`, `upload_stop`, `upload_list`, `list_closed_at`) ve
  `NSPopoverDelegate` pane'de. Girdiler doğumda `PaneLaunch`'tan (Karar 3:
  ayar anlık görüntüsü, tema, `Run`, `Stats`, entegrasyon ortamı + dock
  payı, kimlik, `Launch`, hareket bayrakları); olaylar `PaneHost`'tan
  (başlık/dizin, kabuk çıktı, yükleme durumu, bildirim, alt başlık tanısı,
  OSC 52 kopyası). Modülde `app::delegate` ve `app.settings()` kalmıyor.
- **`crates/bt-shell/src/uploader.rs`** — `impl TerminalWindow` →
  `impl TerminalPane`; sayfalar pane view'ının `window()`'una, popover pane'in
  `BateriView`'ına bağlanıyor; `on_window` → pane kimliği; başlık öneki ve
  bildirim `PaneHost`'tan; Dock simgesi pane'lerin toplamı (sahip gezer).
  Esc izleyicisinin pencere numarası süzgeci pane'in penceresinden.
- **`crates/bt-shell/src/search_bar.rs`** — `parent` pane; alan pane'in
  çocuğu (arama paneli yine Metal katmanının kardeşi).
- **`crates/bt-shell/src/view.rs`** — Sahip `superview()` downcast'iyle
  (`TerminalPane`); `window_owning` yolu ve delegate downcast'i kalkıyor.
- **`crates/bt-shell/src/window.rs`** — `TerminalWindow` `PaneHost`'u
  uyguluyor (başlık, kapanış, pano, bildirim yönlendirmesi); taşınan
  seçiciler ve arama/yükleme kodu çıkıyor. Kalanlar: `closeTab:`,
  `closeWindow:`, `selectTab:`, krom, kapatma sorusu.
- **`crates/bt-shell/src/app.rs`** — `open_window` `PaneLaunch`'ı kuruyor;
  `window_owning` gereksizse kalkıyor; `refresh_dock_tile` pane'leri geziyor.
- **`crates/bt-shell/src/lib.rs`** — başlık yorumu: pencere/pane ayrımı.
- **`CLAUDE.md`** — `bt-shell` satırı ve "Sekmeler macOS'un kendi sekmeleri"
  paragrafı: pane nesnesi, sahip arayüzü, menü seçicilerinin zinciri
  (arama alanı paragrafındaki "karşılayanı `TerminalWindow`" cümlesi pane'e).

## Kabul

- `make hepsi` yeşil; `make duman` jeton değerleri aynı; `make test-yaris`
  yeşil (OSC 52 kopyası `PaneHost`'tan geçiyor).
- `grep 'app::delegate' crates/bt-shell/src/pane.rs` boş.
- Gözle: ⌘F alanı odaktayken ⌘G/⇧⌘G/Esc, ⌘E, ⌘K/⌥⌘K, ⌘Home/⌘End, Cmd +/−/0,
  Edit menüsünün etkin/gri öğeleri; ssh sekmesinde Finder damlası → onay
  sayfası, `Show files (N)` popover'ı, ⌘. soru sayfası, başlıkta yüzde ve
  Dock simgesi.

## Checklist

- [x] Arama ve delegesi pane'de
- [x] Pane düzeyi seçiciler + adlı eylem yöntemleri pane'de
- [x] Yükleme (`uploader.rs`) pane'de
- [x] `PaneLaunch` + `PaneHost`; pane'de `app::delegate` yok
- [x] `view.rs` sahibi `superview()`'dan
- [x] `CLAUDE.md` ve `lib.rs` başlığı
- [x] Test: `PaneHost`'un sahte bir uygulamasıyla başlık/kopya olayının
      pencereye değil sahibe gittiği (AppKit'siz sınanabilen kısım)
- [x] Doğrulama geçti (`make hepsi`, `make duman`, `make test-yaris`)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi

## Uygulama Notları

- **Sahip tutamağı pencereyi kimlikle buluyor** (`window::WindowHost {
  window: u64 }`, `app.window(id)`): pencere kimliği `open_window`'da pencere
  doğmadan çekiliyor, yani tutamak doğum paketine girebiliyor — sonradan
  kurulan yuva, geri referans ya da çember yok. `PaneHost`'un yöntemleri
  AppKit tipi değil pane kimliği (`u64`) alıyor: sahte sahiple sınanabilen
  kısım bu sayede var ve phase-3'te sahip olayın hangi pane'den geldiğini
  biliyor.
- **Ana kuyruk dönüşleri `PaneLookup` ile** (`fn(MainThreadMarker, u64) ->
  Option<Retained<TerminalPane>>`, sahibin verdiği `app::pane_by_id`): düz
  `fn` göstericisi `Send + Copy`, yani `ShellWake`, alternatif ekran
  habercisi, uzak yoklama, arama sürücüsü, yükleme thread'leri ve Esc
  izleyicileri onu yakalıyor. `AppDelegate::window_of_pane` kalktı; `pane(id)`
  doğrudan `find_open`'dan (`lookup_skips_closed_panes` bekçisi aynen).
- **Doğum paketi `new`'de**, `start`'ta değil: `TerminalPane::new(mtm, frame,
  PaneLaunch)` kalıcı alanları ivar'a yazıyor (sahip, arama yolu, font,
  Hareketi Azalt, tekerlek kipi, punto farkı), yalnız `start`'ın tükettiği
  yarıyı (`Birth`: `Stats`, ayar kopyası, tema, `Launch`, entegrasyon)
  `RefCell<Option<_>>`'da tutuyor. `request_font` artık `new`'in son adımı
  (önceki sıra: `new` → `set_zoom` → `request_font`; arada iş yoktu).
  Entegrasyon, tema ve dizin `open_window`'da pencere doğmadan çözülüyor —
  hepsi ayardan ya da `from`'dan, geometriye bağlı değil.
- **Başlık yolu ikiye bölündü**: pencerenin `refresh_title`'ı yalnız başlık +
  sekme noktası; yükleme kuyruğunun bağlantı kenarı
  (`check_upload_connection`) pane'in ve olaydan **önce** koşuyor
  (`TerminalPane::remote_or_title_changed`, başlık işi). Yükleme yüzdesinin
  başlığı `host.title_changed` — kenarı sormuyor, döngü yok.
- **Retain çemberi kapatıldı**: `SearchBar` kapsayıcısını (`parent`)
  tutuyordu; pane paneli tuttuğu için pane → panel → pane çemberi pane'i
  (renderer, link) hiç düşürmezdi. Alan kalktı, `resting_frame` ölçüyü
  yüzeyin `superview`'ından alıyor, `search_cover` pane'i kaynak veriyor.
- **Kapanış sırası pane'de**: `begin_close` önce `abandon_uploads`, sonra
  ritim/`Waker`/`SIGHUP` (iptal `SIGHUP`'tan önce).
- **Davranış farkı (bilinçli)**: OSC 52 kopyası `put` ile ana kuyruk işi
  arasında pane'i kapanmışsa artık düşüyor (eskiden genel panoya yine
  yazılıyordu) — metin sahibe gidiyor ve kapanmış pane'in sahibi yok.
  `PendingCopy::deliver` kalktı; iş `take` + `PaneHost::copy_to_clipboard`
  (varsayılan kol `clipboard::copy(genel pano)`), pano sınaması iki adımla.
- Pencerenin `validateMenuItem:`'ı kalktı: kalan üç eylemi hep etkindi ve
  yanıt vermeyen hedefin öğesi etkin sayılıyor. `session()`/`view()`/
  `ns_window()`/`uploads()` ileticileri pencereden kalktı.
- Pane modülü `app`'ten yalnız saf parçaları alıyor (`Grid`,
  `split_into_grid`, `dock_rows_for`); `grep 'app::delegate' pane.rs` boş,
  `uploader.rs`'te de yok.
- Test-first: `title_and_copy_events_reach_the_host_with_the_pane_id` önce
  derlenmeyerek kırmızı (trait ve iki yardımcı yoktu).
- Duman öncesi/sonrası aynı: `kare=29 hucre=8 glif=6 kural=15 yuva=13/1984
  yuva2=0/1984 yuk=smoke istek=4 icerik=2 hareket=27 kayma=0 kapanis=clean
  pipeline=ok`.
- Gözle kontrol bu otonom koşuda yapılmadı (⌘F alanında ⌘G/⇧⌘G/Esc, ⌘E,
  ⌘K/⌥⌘K, ⌘Home/⌘End, Cmd +/−/0, Edit menüsünün gri öğeleri; ssh
  sekmesinde damla → onay sayfası, popover, ⌘. sorusu, başlık yüzdesi, Dock
  simgesi) — set kapısında.
- `/code-review` (medium) tek bulgu (düşük): başlık işi yükleme bağlantı
  kenarını `title_pending` inmeden okuyordu — arada biten ssh yeni iş
  doğurmaz, kuyruk ölü bağlantıda kalırdı. Giderildi: `announce_title`
  bayrağı indirip **sonra** kenarı (`edge`) ve sahibi çağırıyor; bekçi aynı
  sınamada (kenar bayrağı inik görmeli). Kapı yeniden yeşil.
