# Phase 3 — Kayıt ve geri yükleme (`bt-shell-macos`)

## Özet

Kapanışta bütün pencereler kaydedilir; açılışta kayıt varsa pencereler,
sekmeler ve bölmeler tek kurulum yolundan — önce yerleşim, sonra kabuklar —
geri gelir.

_Requirements: R3.1, R3.2, R3.3, R3.4, R3.5_

## Değişiklikler

- **`crates/bt-shell-macos/src/window.rs`** —
  - `Launch`'a `tab_id: Option<TabId>`, `replay: Option<Vec<u8>>` ve ilk
    girdinin çalıştırma biti (phase-1); bugünkü yollar `None`/`true` verir.
  - `TerminalWindow::restore(mtm, shape, launches, focused, zoomed)`: bütün
    pane'leri yaratıp `SplitView`'a kayıtlı ağaçla (oranlarıyla) yerleştirir,
    yerleşimden **sonra** her `pane.start`; ağaç pane'lerin en küçük boyutuna
    (`TerminalPane::min_size`) sığmıyorsa `equalize`. Başlamayan pane ağaçtan
    düşer (`add_pane`'in hata kolu emsali), hiçbiri başlamazsa hata döner.
    Bu fonksiyon geri yüklemenin **tek** kurulum yolu — canlı devir burada
    kabuk yerine fd verecek (`context.md` → Sonraki set ile ilişki).
  - Kayıt toplama: pencere çerçevesi, sekme grubundaki sıra
    (`tabGroup.windows`), seçili sekme, key; sekme başına ağaç
    (`SplitView`'ın ağacı), odak, zoom.
  - `setRestorable(false)` pencere doğarken.
- **`crates/bt-shell-macos/src/split_view.rs`** — kayıtlı oranlarla toplu
  yerleşim (ağacı doğrudan kuran giriş) ve ağacın okunması.
- **`crates/bt-shell-macos/src/pane.rs`** — `tab_id` `Launch`'tan
  (yoksa `new_tab_id()`), `replay` ve çalıştırma biti `SessionOptions`'a; kayıt
  için pane'in `SavedPane`'i (`TabId`, `working_directory`, `zoom` adımı,
  `remote_line`).
- **`crates/bt-shell-macos/src/app.rs`** —
  - `Opening::Restore` ve `pane_launch`'ın kayıttan beslenen kolu (tek doğum
    paketi korunur; uzak pane'de ilk girdi hedefin satırı, çalıştırma biti
    `false` — `discussion.md` → Karar 3).
  - `did_finish_launching`: tek `open_window` yerine `restore_or_open`. Kapı
    sırası: hermetik koşu → paket kimliği yok → `restore_windows == Off` →
    kilit alınamadı → kayıt yok/bozuk: her biri bugünkü tek pencere; `Off`'ta
    kilit alınabiliyorsa kalan kayıt `restore::clear` ile silinir (R4.1). Kayıt
    varsa pencere başına ilk sekme `open_window`'un yolundan (çerçeve
    `NSScreen`'in görünür alanına kırpılmış — `objc2-app-kit`'te `NSScreen`
    flag'i yoksa eklenir ve `Cargo.toml` yorumuna emsal biçimde kaydedilir,
    yeni crate değil, kabuk doğmadan önce), sonraki
    sekmeler `show_as_tab_of`; sonra seçili sekme ve key pencere. Hiçbir
    pencere kurulamazsa bugünkü ilk pencere ve hata yolu.
  - `AppDelegate::shutdown`'ın **başında**, `begin_close`'tan önce kayıt
    (`CLAUDE.md`/`app.rs`: kapanışın her adımı orada; hermetik kapı da orada):
    `"all"`'da her pane'in `Session::final_history`'si, `"layout"`'ta geçmişsiz,
    `"off"`'ta ya da pencere yokken `restore::clear`. Kilit `Ivars`'ta
    açılıştan beri tutulur.
  - Durum dizini: `~/Library/Application Support/bateri/session/{paket kimliği}/`
    (`lib.rs`'in `remote_hosts_path` emsali; paket kimliği
    `NSBundle::mainBundle().bundleIdentifier()`, `uploader`'ın paketsiz kapısı emsal).

## Kabul

- Saf kısımların sınamaları (ağaç ↔ kayıt dönüşümünün pencere tarafı, çerçeve
  kırpma, kapı sırası) `bt-shell-macos`'ta ya da `bt-shell-common`'da.
- Hermetik koşu: `make smoke` yeşil ve kayıt dizinine dokunmuyor (kapının
  ilk kolu; sınama `hermetic_run_does_not_set_up_shell_integration` emsali).
- Gözle kontrol (set kapısında): iki pencere, birinde üç sekme, birinde
  ⌘D + ⇧⌘D bölmesi ve büyütülmüş pane; renkli `ls` çıktısı, vim açık bir pane,
  ssh'taki bir pane → ⌘Q → yeniden aç: düzen, oranlar, odak, zoom, dizinler,
  geçmişin renkleri yerinde; vim'li pane'in vim öncesi geçmişi geri gelir;
  ssh pane'inde `ssh host` satırı giriş satırında hazır, çalışmamış. Üç
  yüzeyde bakılır: ızgara (geri gelen geçmiş), doldurma bandı (boş ekranda
  geçmişin tepeye inmesi), dock (uzak satırın hazır duruşu ve yeni prompt).

## Checklist

- [ ] `Launch` alanları ve `pane.rs`'te tüketimi
- [ ] `TerminalWindow::restore` + `SplitView` toplu yerleşim
- [ ] Kayıt toplama ve `shutdown`'daki kayıt
- [ ] Açılışta `restore_or_open`, çerçeve kırpma, sekme grubu, seçili sekme, key
- [ ] `setRestorable(false)`
- [ ] Test: saf parçalar, hermetik kapı
- [ ] Doğrulama geçti (`make check` + `make smoke`)
