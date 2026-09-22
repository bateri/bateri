# Phase 3 — Pencereleri ve sekmeleri aç

## Özet

Native tabbing açılır; ⌘N/⌘T/⌘W/⇧⌘W, sekme geçişi ve ⌘1…9 menüden çalışır;
kabuk çıkışı ve pencere kapanışı yalnız o sekmeyi kapatır, ⌘Q oturumları
paralel kapatır, arka plandaki sekme kare çizmez.

_Requirements: R3, R3.1, R3.2, R3.3, R3.4, R3.5, R3.6, R3.7, R5_

## Değişiklikler

- **`crates/bt-shell/src/app.rs`** —
  - `setAllowsAutomaticWindowTabbing(false)` satırı ve yorumu kalkar.
    Pencere açmak tek bir yöntemde (`open_window(from: Option<&TerminalWindow>, as_tab: bool)`
    ya da eşdeğeri): dizin `from.session.working_directory()` yoksa
    `child::working_directory()`, zoom `from.zoom` yoksa varsayılan; sekme
    ise `from`'un penceresine `addTabbedWindow:ordered:` (`NSWindowOrderingMode::Above`).
    İlk pencere `didFinishLaunching`'te `from = None` ile.
  - Eylemler (hedefsiz, `AppDelegate`'te — pencere yokken de çalışsınlar):
    `newWindow:` (⌘N), `newTab:` (⌘T; pencere yoksa yeni pencere) ve
    AppKit'in `+` düğmesinin gönderdiği `newWindowForTab:`. Etkin pencere
    `NSApp.keyWindow`'dan listede aranır.
  - `applicationShouldTerminateAfterLastWindowClosed:` → `run.is_some()`;
    `applicationShouldHandleReopen:hasVisibleWindows:` görünür pencere
    yoksa yeni pencere açar. İkisinin gerekçesi doc'ta (Karar 5, Muhakeme).
  - `shutdown` (⌘Q ve deadline): listedeki her oturum için başlat, **tek**
    `now + SHUTDOWN_GRACE`'e kadar hepsini bekle. Süreli koşu tek pencere:
    sonucu `kapanis=`'e bugünkü gibi gider; etkileşimli yolda sonuç atılır.
    Liste bekleme bitene kadar düşmez. `shutdown`'ın doc'undaki "son
    referans" paragrafı yeni yapıya göre yeniden yazılır.
- **`crates/bt-shell/src/window.rs`** —
  - `tabbingIdentifier` ortak sabit; `tabbingMode` varsayılan (sistemin
    "Prefer tabs" ayarına saygı).
  - `windowWillClose:`: `link.stop()` → `Waker`'ı `ShellWake`'ten **sök**
    ve ana thread'de düşür → oturumun kapanışını başlat, tutamağı beklemeden
    bırak → pencereyi `AppDelegate`'in listesinden çıkar (nesne ana
    thread'de düşer). Süreli koşuda bu yol koşmaz: `child_exit`
    `terminate:`'e gidiyor.
  - Pencereye ait eylemler: `closeWindow:` (⇧⌘W; `tabbedWindows` ya da
    kendisi, hepsine `performClose:`), `selectTab:` (menü öğesinin `tag`'i
    1…9 → saf eşleme → `tabGroup.selectedWindow`).
  - Görünürlük: seçili olmayan sekmenin `windowDidChangeOcclusionState:`
    aldığını **doğrula** (geçici `eprintln!`, commit'e girmez); gelmiyorsa
    `windowDidBecomeMain`/sekme grubunun seçili penceresiyle aynı
    `set_visible` yoluna bağla ve Uygulama Notları'na yaz.
- **`ShellWake`** — `waker: OnceLock<Waker>` → yaprak `Mutex<Option<Waker>>`;
  `wake()` kilidi alıp bırakır (Term altında alınması serbest — yaprak),
  `detach()` ana thread'de `take()` eder. `child_exit`: süreli koşuda
  bugünkü `terminate:`; değilse ana kuyrukta kimliğiyle pencereyi bulup
  `close()`. `Waker`'ın doc'undaki ve `wake.rs`'teki sahiplik paragrafı
  "sökülüyor" diye güncellenir.
- **`crates/bt-shell/src/menu.rs`** — **Shell** menüsü (New Window ⌘N, New
  Tab ⌘T, ayırıcı, Close Tab ⌘W → `performClose:`, Close Window ⇧⌘W).
  **Window** menüsü `setWindowsMenu` ile: Minimize ⌘M, Zoom, ayırıcı, Show
  Previous Tab ⇧⌘[ / Show Next Tab ⇧⌘] (`selectPreviousTab:`/`selectNextTab:`),
  Select Tab ▸ (Tab 1…8 ⌘1…⌘8, Last Tab ⌘9), ayırıcı, Bring All to Front.
  Önce AppKit'in tabbing açılınca Window ve View menüsüne **kendisinin**
  eklediği öğelere bakılır (Show Tab Bar, Show All Tabs, Merge All Windows,
  Move Tab to New Window, ⌃⇥/⌃⇧⇥'li sekme öğeleri); ⌃⇥ AppKit'ten gelmiyorsa
  Window menüsüne `Control` / `Control|Shift` maskeli öğe olarak eklenir;
  aynı kısayol iki öğede durmaz. Window menüsünün delegate'i **yok**
  (`menuHasKeyEquivalent` `false` dönen delegate Theme ▸'ye özgü). Modül
  başlığı güncellenir.
- **Saf eşleme** — `tab_index(tag: u8, count: usize) -> Option<usize>`:
  1…8 → `tag-1` (count'tan küçükse), 9 → `count-1`, `count == 0` → `None`;
  sınaması yanında.
- **⌃⇥ doğrulaması** — menü yakalıyorsa `keyDown:` el değmez. Yakalamıyorsa
  `keyDown:`'ın Control kolunun başında tek dal (Ctrl+Tab/Ctrl+Shift+Tab →
  `selectNextTab:`/`selectPreviousTab:`) ve gerekçesi Uygulama Notları'na.
  İki hâlde de Ctrl-I zsh'te hâlâ sekme karakteri ekliyor.
- **`crates/bt-shell/src/lib.rs`** — başlık ("Tek pencere; sekme, bölme…")
  ve `run`'ın doc'u ("son pencere kapanınca ve shell çıkınca … terminate:")
  yeni kapanışa göre.
- **`CLAUDE.md`** — Bugünkü hâl'e sekmeler için kural + tek cümle +
  işaretçi (`discussion.md` → Karar); `bt-shell` satırı; Kapanış maddesi
  (pencere kapanışı beklemez, ⌘Q paralel, süreli koşu `terminate:`).

## Kabul

- `tab_index` sınaması; `split_into_grid` ve duman sınamaları yeşil.
- `make duman` jetonları aynı; `make test-yaris` yeşil.
- Elle (sahneler phase-4'ün gözle kontrolünde de tekrarlanır):
  ⌘T iki sekme açıyor, ikincisi birincinin dizininde; ⇧⌘]/⇧⌘[, ⌃⇥/⌃⇧⇥,
  ⌘1, ⌘9 geçiyor; sekme sürükleniyor ve pencereden koparılıyor; ⌘W sekmeyi,
  son sekmede pencereyi kapatıyor ve o sekmenin zsh'i `ps`'te kalmıyor;
  `exit` yalnız o sekmeyi kapatıyor; son pencere kapanınca uygulama menü
  çubuğunda duruyor, Dock ikonu yeni pencere açıyor; ⌘Q üç sekmeyle
  gecikmesiz kapanıyor; arka plandaki sekmede `sleep 60` koşarken örtülme
  olayı geliyor (geçici log) ve sekmeye dönünce süre sayacı güncel.

## Checklist

- [ ] Tabbing açık, ortak kimlik, `open_window` tek yol
- [ ] ⌘N / ⌘T / `+` / ⌘W / ⇧⌘W; Shell ve Window menüleri, AppKit öğeleri çiftlenmedi
- [ ] ⌃⇥ yolu doğrulandı (Uygulama Notları)
- [ ] ⌘1…⌘9 saf eşleme ve sınaması
- [ ] Pencere kapanışı: stop → waker sök → kapanışı başlat → düş
- [ ] `child_exit` süreli koşuda `terminate:`, değilse o pencere
- [ ] Son pencere / reopen; ⌘Q paralel tek son tarih
- [ ] Arka plan sekmesinin örtülme sinyali doğrulandı
- [ ] `lib.rs`, `menu.rs`, `wake.rs` başlıkları ve `CLAUDE.md` aynı commit'te
- [ ] Doğrulama geçti (`make hepsi` + `make test-yaris` + `make duman`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
