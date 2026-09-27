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

- [ ] Arama ve delegesi pane'de
- [ ] Pane düzeyi seçiciler + adlı eylem yöntemleri pane'de
- [ ] Yükleme (`uploader.rs`) pane'de
- [ ] `PaneLaunch` + `PaneHost`; pane'de `app::delegate` yok
- [ ] `view.rs` sahibi `superview()`'dan
- [ ] `CLAUDE.md` ve `lib.rs` başlığı
- [ ] Test: `PaneHost`'un sahte bir uygulamasıyla başlık/kopya olayının
      pencereye değil sahibe gittiği (AppKit'siz sınanabilen kısım)
- [ ] Doğrulama geçti (`make hepsi`, `make duman`, `make test-yaris`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
