# VT motoru — Bağlam

## Mevcut Durum

001 iskeleti kurdu: `bt-gpu` tek renkli tam ekran quad'ı verilen drawable'a
senkron (`waitUntilCompleted`) çiziyor, `bt-shell` pencereyi açıp açılış ve
boyut değişiminde `draw_surface` çağırıyor, `bateri` `BT_RUN_SECONDS` dolunca
`process::exit` ile çıkıyor. `bt-core` ve `bt-atlas` boş. Terminal yok:
PTY, VT ayrıştırma, grid, hücre, imleç, klavye — hiçbiri.

001'in `teslim.md` → "002'ye devredilen notlar" bu setin girdisidir:

- `draw` drawable alır; display link gelince `draw_surface`/resize yolu yalnız
  kirli işaretler. `waitUntilCompleted` kalkınca drawable geri basıncı
  `nextDrawable`'a taşınır; sayaç anlamı değişir.
- Duman çıkışı `process::exit` yerine `stop:`/`terminate:` üzerinden
  `applicationWillTerminate:`'a uğramalı (PTY çocuğu gelince `Drop` şart).
- `Cell` 16 bayt hedefi ve `const` assert 002'de ölçülür.
- sRGB pixel format kararı glyph harmanlamasıyla birlikte verilir (003'e kalır).
- İkinci `NSView` sınıfı (`BateriView`) klavye/IME/display link ile gelir.

## Motivasyon

Bir terminalin "terminal" olduğu ilk an: pencerede gerçek bir shell'in çıktısı
görünür. Glyph atlası (003) olmadan bile hücre arka planları ve imleç bloğu bu
anı verir — `ls --color` satırları, `htop`'un renkli çubukları, prompt'un
konumu. Bu set üç sözleşmeyi koda bağlar:

1. **`bt-core` platformsuz terminal çekirdeği**: VT durum makinesi, grid,
   scrollback, PTY, hasar (damage) izleme. Dışarıya alacritty tipleri değil,
   kendi `Snapshot`'ı verir; renderer ne çizeceğini alır, ne anlama geldiğini
   bilmez.
2. **Boşta sıfır kare, kirli satırla uyanma**: `CAMetalDisplayLink` paused
   durur, PTY çıktısı geldiğinde açılır, hasar bitince yine durur. 001'in
   senkron `waitUntilCompleted`'ı kalkar.
3. **Kapanış yolu**: shell çocuğu ve okuyucu thread `applicationWillTerminate:`
   üzerinden kapanır; duman çıkışı da oradan geçer.

## Kanıt

Bu oturumda ölçüldü (10 Eylül 2026, scratch crate):

| soru | sonuç |
|---|---|
| `alacritty_terminal 0.26` `Cell` boyutu | **24 bayt**, hiza 8: `c: char` 4 + `fg`/`bg: Color` 4+4 + `flags` 2 + dolgu + `extra: Option<Arc<CellExtra>>` 8. Seyrek veri (zero-width, alt çizgi rengi, hyperlink) zaten yan tabloda (`CellExtra`) |
| bağımlılık ağacı | 39 crate `serde` ile (varsayılan); `default-features = false` ile `serde` düşer. Kalanlar: `vte`, `polling`, `rustix`, `rustix-openpty`, `signal-hook`, `parking_lot`, `regex-automata` (arama), `base64` (OSC 52), `home`, `libc`, `unicode-width`, `bitflags`, `log` |
| gömme API'si | `Term<T: EventListener>` + `Term::damage()` (satır aralıkları) + `tty::new` (openpty, `$SHELL` login) + `event_loop::EventLoop::spawn` (okuyucu thread, `Msg::Input/Resize/Shutdown` kanalı) + `Event::{Wakeup, Title, Bell, ChildExit, PtyWrite, ...}` |
| `vte 0.15` ile kendi grid | `ansi::Handler` trait'i yaklaşık 80 metot; grid, scrollback, reflow, seçim, hasar sıfırdan. Metalterm bunu yaptı (`mt-core/screen`, `reflow`, `term/state`) |
| ana thread uyandırma | `dispatch2::DispatchQueue::main().exec_async(closure)` — `dispatch2` zaten `bt-gpu` bağımlılığı |
| display link | `CAMetalDisplayLink::initWithMetalLayer`, `addToRunLoop_forMode` (main run loop, common modes), `setPaused`, delegate `metalDisplayLink:needsUpdate:` → `CAMetalDisplayLinkUpdate::drawable()` |

`CLAUDE.md`'nin "hedefimiz 16" cümlesi alacritty ile tutulamaz; karar K1'de.

## Mevcut Mimari

```
bateri → bt-shell ──────────────► bt-gpu ──► bt-atlas (boş)
          AppDelegate              Renderer     bt-core (boş)
          pencere, draw_surface    Surface
                                   quad pipeline, senkron kare
```

Hedef (bu set):

```
                 ┌── okuyucu thread (alacritty EventLoop) ── PTY ── $SHELL
                 │        │ Event::Wakeup
bt-core          │        ▼
  Session ◄──────┘   Listener (bt-shell) ── dispatch main ──► link.setPaused(false)
  Term<…> (Mutex)                                                    │
  Snapshot { hücre bg, imleç }                    needsUpdate ◄──────┘
                    ▲                                  │
bt-gpu              └── Frame::from(snapshot) ── cell_bg pipeline ── drawable
                                                       │ hasar yoksa → setPaused(true)
bt-shell  BateriView keyDown → Session::write ; applicationWillTerminate → Session::shutdown
```
