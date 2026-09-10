# VT motoru

## Hedef

Pencerede gerçek bir shell'in çıktısı belirir: `bt-core` platformsuz terminal
çekirdeği (VT, grid, PTY, hasar), `bt-gpu` hücre arka planlarını ve imleci
`CAMetalDisplayLink` ile yalnız hasar varken çizer, `bt-shell` klavyeyi PTY'ye
akıtır ve kapanış shell çocuğunu düzgün bitirir. 001'in devrettiği notlar kapanır.

## Gereksinimler

- **R1** — `bt-core` `alacritty_terminal`'i kapsüller: `pub` API'de alacritty
  tipi yok; `cargo tree -p bt-core` `objc2`/`core-text`/`metal` içermez.
  - **R1.1** — `Session::spawn(SessionOptions, Arc<dyn Wake>)`: `$SHELL` login
    ya da verilen komut; `TERM=xterm-256color`, `COLORTERM=truecolor` elle;
    `tty::setup_env()` çağrılmaz.
  - **R1.2** — `Session::frame(&self, sink: impl FnMut(CellBg)) -> Option<Cursor>`
    (taslak `&mut dyn` yazıyordu; hücre başına dinamik çağrı olmasın diye
    jenerik, uygulamada karar):
    tek kilit tutuşunda hasar sorgusu, hasar yoksa `None` ve hiç iterasyon;
    varsa varsayılan olmayan arka planlar çözülmüş RGBA ile, imleç, `reset_damage()`.
  - **R1.3** — `Session::write(&[u8])`, `Session::resize(cols, rows, cell_px)`,
    `Session::shutdown()` (`Shutdown` → join → drop; SIGHUP), `Drop` → `shutdown()`.
  - **R1.4** — `Wake` trait'i (`wake()`, `child_exit(Option<i32>)`); `PtyWrite`,
    `ColorRequest`, `TextAreaSizeRequest` adapter içinde yanıtlanır; `Title`,
    `Bell`, `Clipboard*` yoksayılır.
  - **R1.5** — Hücre boyutu `const` assert: alacritty `Cell` 24 bayt.
  - **R1.6** — Sınama: sabit `/bin/sh -c printf` ile PTY açılır, `frame()`
    beklenen arka plan hücrelerini verir; `yaris_*` stres sınamaları `#[ignore]`.
- **R2** — `bt-gpu` `cell_bg` pipeline: instanced quad, `#[repr(C)]` instance
  düzeni `.metal` ile alan alan aynı; `Frame` sink'ten dolar; kare başına
  `newBufferWithBytes`. 001'in `make duman` sözleşmesi bu phase'de korunur.
- **R3** — `bt-gpu::DisplayLink`: `CAMetalDisplayLink` sarmalayıcısı, paused
  başlar, `Waker` (`Send + Sync`) `setPaused(false)`; callback'te `frame()`
  `None` → `setPaused(true)`; asenkron commit, sayaç `addCompletedHandler`'da
  `status != Error` ise. `draw_surface`, `Surface::layer()`, `NoDrawable` silinir.
- **R4** — Duman sözleşmesi `kare=N hucre=K pipeline=ok`; K = çizilen arka plan
  instance'ı (imleç hariç); `BT_RUN_SECONDS` yolunda shell sabit
  (`/bin/sh -c "printf '\033[41m bateri \033[0m\n'; sleep 30"` → K = 8);
  bekçi thread `run_seconds × 3`'te `_exit(70)`.
- **R5** — `BateriView`: `acceptsFirstResponder`, `keyDown:` → saf
  `kod_cevir` → `Session::write`; Enter/Backspace/Tab/oklar/Ctrl-harf; IME yok.
- **R6** — Resize: hücre piksel boyutu `const` yer tutucu; `bounds → WindowSize
  → Session::resize`; `ChildExit` → uygulama sonlanır;
  `applicationWillTerminate:` → `shutdown()`.
- **R7** — Belgeler aynı commit'te: `CLAUDE.md` yedi cümle (taban listesi,
  hücre 24, `bt-core` platform sütunu ve sorumluluk satırı + OSC 133 notu,
  `make duman` satırı, jeton listesi, iskelet paragrafı); `proje.md`
  `test-yaris` reçetesi ve `duman` satırı; `make test-yaris` gerçek hedef.

## Yaklaşım

1. **Phase-1 `bt-core`** — bağımlılık, `Wake`, `Session`, `frame`, sınamalar,
   `make test-yaris` nightly'siz reçete. Pencere ve Metal yok.
2. **Phase-2 `bt-gpu` cell_bg** — shader, `Frame`, instance düzeni; arka planı
   render pass'in `Clear` yükü boyar ve 001'in tam ekran quad'ı silinir
   (uygulamada karar; gerekçe phase-2 → Uygulama Notları). `make duman`
   sözleşmesi değişmez.
3. **Phase-3 `bt-shell` çizim** — `DisplayLink` + `Waker`, `Session` bağlanır,
   asenkron, yeni duman sözleşmesi ve jeton listesi **tek commit'te**;
   `draw_surface` yolu silinir.
4. **Phase-4 `bt-shell` giriş/kapanış** — `BateriView` + klavye, resize
   yolu, `ChildExit`, `applicationWillTerminate:`, bekçi thread.

## Kapsam Dışı

Glyph/atlas/CoreText (003), sRGB kararı (003), tema ve ayar dosyası, sekme/
bölme, seçim/kopyalama, kaydırma görüntüleme, OSC 133 komut blokları, imleç
animasyonu/yanıp sönme, IME/`NSTextInputClient`/Option-as-Meta/kitty klavye,
`TERM` adı değişikliği, Apache-2.0 attribution paneli (bundle seti; `teslim.md`'ye not).

## Göç

Yok: ayar dosyası, tema, terminfo değişmiyor. `make duman` çıktısı jeton
**ekler** (`hucre=`), eskisini korur.

## Akış

```
$SHELL ── PTY ── okuyucu thread (EventLoop) ──parse──► Term (FairMutex)
                       │ Event::Wakeup / PtyWrite / ChildExit
                       ▼
             bt-core adapter ── PtyWrite → Msg::Input
                       │ Wake::wake()
                       ▼
             bt-gpu Waker ── dispatch main ──► DisplayLink.setPaused(false)
                                                     │ needsUpdate (ana thread)
                                                     ▼
                              Session::frame(sink) ── None → setPaused(true)
                                                     │ Some(cursor)
                                                     ▼
                              Frame → cell_bg pipeline → drawable → commit
                                                     │ completed → frames += 1
BateriView keyDown ── kod_cevir ── Session::write ── Msg::Input
applicationWillTerminate / ChildExit / deadline ── Session::shutdown ── SIGHUP
```

## Durum

| Phase | Durum | Commit |
|-------|-------|--------|
| phase-1 | ✅ | ed1c5a3 |
| phase-2 | | |
| phase-3 | | |
| phase-4 | | |
