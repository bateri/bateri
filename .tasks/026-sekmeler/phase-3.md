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

- [x] Tabbing açık, ortak kimlik, `open_window` tek yol
- [x] ⌘N / ⌘T / `+` / ⌘W / ⇧⌘W; Shell ve Window menüleri, AppKit öğeleri çiftlenmedi
- [x] ⌃⇥ yolu doğrulandı (Uygulama Notları)
- [x] ⌘1…⌘9 saf eşleme ve sınaması
- [x] Pencere kapanışı: stop → waker sök → kapanışı başlat → düş
- [x] `child_exit` süreli koşuda `terminate:`, değilse o pencere
- [x] Son pencere / reopen; ⌘Q paralel tek son tarih
- [x] Arka plan sekmesinin örtülme sinyali doğrulandı
- [x] `lib.rs`, `menu.rs`, `wake.rs` başlıkları ve `CLAUDE.md` aynı commit'te
- [~] Doğrulama geçti (`make hepsi` + `make test-yaris` yeşil; `make duman`
      [~] ortam (bkz. phase-1): aynı `MotionUnsettled`, `hareket=7`, sessiz
      ~915 ms; geçici bir satırla basılan jetonlar `hucre=8 glif=6 kural=15
      kapanis=clean pipeline=ok`)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi (tek bulgu: Dock
      ikonu açık bir About panelinde pencere açmıyordu — ölçüt artık yalnız
      pencere listesi; ⇧⌘W'nin arka sekmeyi kapattığı elle sınanmıştı)

## Uygulama Notları

- **⌃⇥ menü kısayolu olarak yakalanıyor** (ölçüldü, System Events ile tuş
  gönderip hangi pencerenin key olduğuna bakarak): `keyDown:` el değmedi.
  ⌃⇥ `"\t"` + Control, ⌃⇧⇥ `"\t"` + Control|Shift ile eşleşiyor. Ctrl-I zsh'e
  hâlâ sekme olarak gidiyor (tamamlama çalıştı).
- **AppKit sekme gezinme öğelerini eklemiyor** (ölçüldü, menü dökümü ve
  açılmış Window menüsünün AX listesi): tabbing açıkken View'a Show/Hide Tab
  Bar ve Show All Tabs, Window'a yerleşim öğeleri (Fill, Center, Move &
  Resize, Full Screen Tile…) ve alternatifler, Shell'e Close All (⌥⌘W)
  geliyor; Show Previous/Next Tab, Move Tab to New Window ve Merge All Windows
  **gelmiyor**. Karar 6'nın "AppKit'in" dediği son ikisi bu yüzden bizim
  Window menümüzde (NSWindow eylemleri, AppKit doğruluyor). ⇧⌘[ / ⇧⌘] görünür
  öğe (`{`/`}`), ⌃⇧⇥ / ⌃⇥ **gizli** öğe + `allowsKeyEquivalentWhenHidden` —
  bir öğe iki kısayol taşıyamıyor ve başlık iki kez görünmesin (Safari'nin
  deyimi); her kısayol tek öğede.
- **Seçili olmayan sekme `windowDidChangeOcclusionState:` alıyor** (geçici
  log): arkaya düşen sekme `visible=false`, öne gelen `true`. Ek kanca
  gerekmedi; yorum `window.rs`'te.
- **Türkçe Q'da** ⇧⌘] / ⇧⌘[ (fiziksel ] / [ tuşları) çalıştı; AppKit'in
  kısayol yerelleştirmesi `,`'ü `ö`'ye, `+`'yı `:`'ya çeviriyor (menü dökümü).
- **Listeden çıkış bir tur erteleniyor** (`AppDelegate::forget_window`):
  listenin `Retained`'ı nesnenin tek güçlü referansı; `windowWillClose:`
  içinde düşseydi nesne kendi metodunda serbest kalırdı. Kapanan pencerenin
  delegate'i orada `None` yapılıyor.
- **Oturum açılamazsa da süreç çıkmıyor** (phase-1 notunun genişlemesi):
  `start_session`'daki `Session::spawn` hatası eskiden `process::exit(1)`'di;
  artık `start` `io::Result` dönüyor, ⌘T/⌘N'de pencere kapanıyor ve satır
  stderr'e, yalnız ilk pencerede süreç çıkıyor.
- **Ayarlar ilk pencereden önce okunuyor**: `load_settings` artık tema
  döndürmüyor ve pencere listesine uzanmıyor; yeni pencere fontu
  (`request_font`) ve alt başlığı (`notices.subtitle()`) doğarken alıyor,
  tema etkin pencerenin oturumundan ya da `resolve_theme`'den
  (`choose_theme`) geliyor.
- **`start_session`'ın `dock` argümanı kalktı** (clippy: sekiz argüman);
  değer bir satır önce yazılan `dock_rows_at_birth` yuvasından.
- **Dock ikonu**: pencere hiç yokken yeni pencere, varsa (simge durumunda da)
  AppKit'in varsayılanı — simge durumundakini geri getirmek. Elle
  sınanamadı (paketsiz binary'ye reopen olayı gönderilemedi) → phase-4 gözle
  kontrolünün 8. sahnesinde zaten var.
- **Elle sınanan** (System Events, kendi başlattığım süreç): ⌘T ikinci
  sekmeyi birincinin dizininde açıyor; ⌃⇥/⌃⇧⇥, ⇧⌘]/⇧⌘[, ⌘1, ⌘2, ⌘9;
  `+` düğmesi; Move Tab to New Window / Merge All Windows; `exit` ve ⌘W yalnız
  o sekmeyi kapatıyor ve zsh `ps`'te kalmıyor; ⇧⌘W iki sekmeyi kapatıyor ve
  uygulama açık kalıyor; pencere yokken ⌘N evde açıyor; üç sekmeyle ⌘Q
  ~0,3 sn'de (osascript'in 0,3 sn'lik gecikmesi hariç) bitiyor ve çocuk kalmıyor.
- `NSWindowTabGroup` bayrağı eklendi (`tabbedWindows` tek sekmede `nil`);
  `Cargo.lock` oynamadı.
- `make hepsi` bir koşuda `bt-shell` lib sınamalarında bilinen SIGSEGV
  (008 phase-5, 021 phase-1) ile düştü; ardından `bt-shell` üç koşuda ve
  `make hepsi` yeşil.
