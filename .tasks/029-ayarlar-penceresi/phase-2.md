# Phase 2 — Pencere, kenar çubuğu ve kontroller

## Özet

Cmd-, ile açılan tek örnekli ayar penceresini kurmak: kenar çubuğu, dört
kategorinin satırları ve her kontrolün phase-1'in yazma yolundan dosyaya
yazması.

_Requirements: R4, R4.1, R4.2, R5_

## Değişiklikler

- **`crates/bt-shell/Cargo.toml`** — `objc2-app-kit`'e gereken bayraklar
  (beklenen: `NSSplitViewController`, `NSSplitViewItem`, `NSSplitView`,
  `NSViewController`, `NSTableView`, `NSTableColumn`, `NSTableCellView`,
  `NSScrollView`, `NSClipView`, `NSImage`, `NSImageView`, `NSGridView`,
  `NSTextField`, `NSPopUpButton`, `NSSlider`, `NSStepper`, `NSSwitch`,
  `NSFont`, `NSLayoutConstraint`, `NSText`), mevcut yorum bloğunun
  örüntüsüyle tek gerekçe paragrafı. Yalnız derleyicinin istediği bayrak
  eklenir. `Cargo.lock` **değişmemeli** (isteğe bağlı bağımlılıklar grafta);
  değişirse dur ve eskale et.
- **`crates/bt-shell/src/settings_window.rs`** (yeni) — modül başlığı
  `discussion.md` → Karar 4'e bağlanır, tekrarlamaz.
  - Pencere: `NSSplitViewController`, sol öğe sidebar davranışlı, sağ öğe
    detay; tam boy içerik, saydam başlık; sabit makul boyut (yeniden
    boyutlandırılabilir değil). `releasedWhenClosed = false`,
    `tabbingMode = disallowed`, başlık `"Settings"`.
  - Kenar çubuğu: tek sütunlu `NSTableView`, kaynak listesi stili, dört satır
    SF Symbol + başlık; seçim sağ bölmeyi değiştirir, son seçim bellekte
    kalır.
  - Sağ bölme: kategori başlığı + `NSGridView` (etiket sütunu sağa yaslı,
    açıklama kontrolün altında küçük ve ikincil renk) + altta sağa yaslı
    **"Open settings.toml"** düğmesi.
  - Satırlar ve kontroller (`discussion.md` → Karar 2, 3, 5, 6, 9):
    - General: Confirm before closing (popup: Never / Only when a program is
      running / Always), Clipboard access (switch; açıklama OSC 52'nin ne
      olduğunu tek cümleyle), Scrollback lines (alan + stepper), Shell
      integration (popup: Auto / Blocks only / Off; not "Takes effect in new
      tabs and windows.").
    - Appearance: Theme (Match System, ayraç, gömülüler, kullanıcı temaları
      — `menu::fill_themes`'in listesiyle aynı kaynak:
      `settings::user_theme_names` + gömülü adlar), Light theme, Dark theme
      (Match System değilken devre dışı), Font (Default + `bt_gpu`'nun aile
      listesi + gerekirse dosyadaki ad), Size (alan + stepper, sınırlar
      `zoom.rs`'ten — sabitleri `pub(crate)`), Line height (alan + stepper,
      adım 0.1, aralık `bt-core`'dan).
    - Cursor: Shape (Block / Underline / Beam), Blink (Off / On / Follow
      program — `auto`), Blink speed (log ölçekli slider + değer etiketi;
      Off'ta devre dışı), Corner radius (slider), Glow (slider), When
      unfocused (Hollow / Solid).
    - Motion: Cursor motion (Spring / Ease / Snap), Smooth scrolling
      (switch), Reduce motion (Match System / On / Off).
    - Popup başlığı ↔ enum varyantı kapsamlı `match`'le; popup'ın öğe listesi
      `bt-core`'un `NAMES` tablosundan sıralanır.
  - Her kontrolün eylemi bir `SettingsEdit` kurup `AppDelegate`'e verir;
    pencere kendi durumunu tutmaz, gösterdiği değer her zaman
    `AppDelegate`'in etkin ayarından (`refresh(&Settings, …)`).
- **`crates/bt-shell/src/app.rs`**
  - `AppDelegate` ivar'ı: `Option<Retained<SettingsWindow>>`, tembel doğar.
  - `openSettings:` → pencereyi göster/öne getir (Hermetic: erken dönüş
    korunur). Bugünkü `edit_settings` "Open settings.toml"un eylemi olur,
    gövdesi değişmez.
  - `save_edit(&SettingsEdit)`: `save_theme`'in genellemesi (yaz → başarıda
    `Source::Write`'ı boşalt + `reload_settings`; hatada `Source::Write`);
    `save_theme` onun çağıranı.
  - `reload_settings`'in sonunda açık pencereye `refresh` (bu phase'de yalnız
    değerler; tanı/kilit phase-3).
- **`crates/bt-shell/src/lib.rs`** — modül kaydı.
- **`crates/bt-shell/src/menu.rs`** — başlık yorumu: Settings… artık pencere
  açıyor; öğe ve kısayol aynı.
- **`crates/bt-shell/src/zoom.rs`** — `MIN_SIZE`/`MAX_SIZE` `pub(crate)`,
  doc'una ikinci tüketici.

## Kabul

- `make hepsi` yeşil; `make duman` yeşil ve jeton satırı değişmedi (süreli
  koşu pencereyi hiç kurmuyor).
- Gözle: Cmd-, pencereyi açar, ikinci Cmd-, yenisini açmaz öne getirir;
  kenar çubuğunda dört kategori ikonlarıyla; her popup/switch değişikliği
  terminal penceresinde **anında** görünür (tema, imleç şekli, font) ve
  `settings.toml`'da yalnız o satır değişmiştir (yorumlar yerinde); slider
  bırakınca yazar; Scrollback alanında yazarken dosya değişmez, Enter'da
  değişir.
- Ayar penceresi key iken ⌘T ona sekme eklemez, ⌘N yeni terminal penceresini
  evde açar, ⌘Q onay sorusunda ayar penceresini saymaz, ⌘W onu kapatır.
- Dosya yokken pencereyi açmak dosya yaratmaz; ilk değişiklik şablonu +
  satırı yazar.
- Shell integration'ı değiştirip ⌘T: yeni sekme yeni değerle doğar, eski
  sekme etkilenmez.

## Checklist

- [ ] Bayraklar gerekçeli; `Cargo.lock` değişmedi
- [ ] Pencere iskeleti, kenar çubuğu, dört bölme, "Open settings.toml"
- [ ] Bütün satırlar ve kontroller; bağımlı satırların devre dışı hâli
- [ ] `save_edit`, `openSettings:` yeniden bağlandı, Hermetic no-op korundu
- [ ] Test: popup başlığı ↔ `NAMES` eşlemesinin kapsamı (saf yardımcı), blink
  slider'ının log eşlemesi uçlarda aralığın uçlarını verir
- [ ] Doğrulama geçti (`make hepsi` + `make duman`)
