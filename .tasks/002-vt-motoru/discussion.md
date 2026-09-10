# VT motoru — Tartışma

## Karar 1: VT motoru ve grid — `alacritty_terminal` mi, `vte` + kendi grid mi?

**(a) `alacritty_terminal 0.26`, `default-features = false`.** `Term`, `Grid`,
scrollback, hasar izleme, seçim, vi modu, kitty klavye, OSC 52/8, sync
update — hepsi hazır ve yıllardır TUI'larla sınanmış. `bt-core` onu **kapsüller**:
dışarıya `Session`, `Snapshot`, `CellBg`, `Cursor` verir; `alacritty_terminal`
tipleri `pub` API'de görünmez (renderer ve kabuk onu hiç import etmez).

- Artı: 001'in issue dersleri (`docs/ARASTIRMA.md` → "Açık sorunlar") tam da
  uyumluluk kuyruğudur; alacritty o kuyruğu geçti. `Term::damage()` kirli
  satır kapısını hazır verir. Okuyucu thread ve PTY (`EventLoop`, `tty`) aynı
  crate'te.
- Eksi: `Cell` 24 bayt, `CLAUDE.md`'nin 16 hedefi düşer (Metalterm 20).
  ~35 crate. `bt-core` `libc`/`rustix` görür — platformsuzluk ilkesi
  "objc2/core-text/metal yok" demektir, Unix PTY zaten Linux'ta da var;
  ilke bozulmaz, `CLAUDE.md`'de netleştirilir.

**(b) `vte 0.15` + kendi grid.** Ayrıştırıcı hazır, `Handler`'ın ~80 metodu,
grid/scrollback/reflow/hasar/seçim bizim. Metalterm'in yolu.

- Artı: hücre düzeni bizim (16 bayt tutulur), yan tablolar tasarlanır.
- Eksi: aylar; ilk hücre çizilmeden önce reflow ve alternate screen yazılır;
  her TUI hatası bizim. 001 panelinin sorusu tam buydu: "`alacritty_terminal`
  neden yetmiyor?" — bugün yetmeyen bir şey yok.

**(c) (a) şimdi, kendi grid'i sonra.** (a)'nın kapsüllemesi bunu zaten
mümkün kılar: `Snapshot` sınırı korunursa 00X'te `Term` yerine kendi
`Screen`'imiz konur, renderer ve kabuk değişmez.

**Öneri: (c) = (a) + kapsülleme sözleşmesi.** `CLAUDE.md` hücre maddesi
yeniden yazılır: "hücre boyutu sabittir ve `const` assert ile bağlıdır;
bugünkü sabit alacritty'nin 24'ü, seyrek veri `CellExtra`'da; kendi grid'e
geçiş `Snapshot` sınırının arkasında yapılır".

## Karar 2: PTY ve okuyucu thread — alacritty `tty` + `EventLoop` mi, `portable-pty` mi?

**(a) `alacritty_terminal::tty` + `event_loop::EventLoop`.** openpty, `$SHELL`
login, `TERM`/`COLORTERM` ortamı, `Msg::{Input, Resize, Shutdown}` kanalı,
`ChildExit` olayı, okuyucu thread — hazır. **(b) `portable-pty` + kendi thread.**
`CLAUDE.md` taban listesinde ama alacritty seçilince ikinci bir PTY katmanı
olur; `EventLoop` zaten `EventedPty` ister.

**Öneri: (a).** `portable-pty` taban listesinden düşer (`CLAUDE.md`).

## Karar 3: Okuyucu thread'den ana thread'e uyandırma

`Event::Wakeup` okuyucu thread'de gelir; display link'i açan çağrı ana
thread'de olmalı. **(a) `DispatchQueue::main().exec_async`** (`dispatch2`,
zaten `bt-gpu` bağımlılığı; `bt-shell`'e girer). **(b) `CFRunLoopSource`** —
daha fazla `unsafe`, kazanç yok. **(c) `performSelectorOnMainThread`** — ObjC
sınıfı gerektirir, `bt-core`'un `EventListener`'ı ObjC bilmemeli.

**Öneri: (a).** `bt-core` generic `EventListener` alır; `bt-shell`'deki
`Listener` `exec_async` ile `AppDelegate`'e "uyan" der. Fazladan uyanma
birleştirilir (`AtomicBool` kirli bayrağı: bayrak zaten setse kuyruğa atma).

## Karar 4: Kare tetikleme — display link sürekli mi, paused mu?

**(a) Link hep açık**, her callback'te hasar varsa çiz, yoksa dön: frame
gönderilmez ama 120 Hz callback koşar — CPU uyanır, "boşta sıfır kare"nin
lafzı tutar ruhu tutmaz. **(b) Link paused**; Wakeup → `setPaused(false)`;
callback'te hasar yoksa (ve imleç yanıp sönmesi kapalıyken) `setPaused(true)`.

**Öneri: (b).** Sahiplik `bt-gpu` (001 kararı): `DisplayLink` sarmalayıcısı ve
delegate sınıfı `bt-gpu`'da, run loop main + common modes (canlı boyutlandırma
sırasında da kare gelsin). Callback ana thread'de; encode ana thread'de
(asenkron commit, `waitUntilCompleted` yok) — 002 için kabul, "render yolu
bloklanmaz" maddesi PTY okuma ve ayrıştırmanın ayrı thread'de kalmasıyla
tutulur. Kare sayacı `commit` anında artar; anlamı "gönderilen kare".

## Karar 5: Ne çizilir ve hasar neye karar verir

Drawable'lar üçlü havuzdan gelir ve içerik korunmaz → kısmi yeniden çizim
güvensiz. **Hasar "çizilsin mi"ye karar verir, "ne çizileceğine" değil**: her
karede tam grid. Çizilen: hücre arka planları (varsayılan arka planla aynı
olanlar atlanır), imleç bloğu. Glyph yok (003). Instance buffer her karede
`Snapshot`'tan kurulur (`cols × rows × 16 bayt`, 200×60'ta ~190 KB; üçlü
tamponlanır). [Düzeltme, 002 phase-1 `/audit`: gerçek düzen `Instance` = 32
bayt (`phase-2.md`, `const` assert), 200×60'ta ~375 KB. Üçlü tamponlama
zaten reddedildi; bkz. Muhakeme.] Palet: 16 renk + varsayılan fg/bg sabit (tema modeli 00X).

## Karar 6: Kapanış ve duman çıkışı

`process::exit` `Drop`'ları atlar: shell çocuğu, okuyucu thread, ileride ayar
yazımı. **Öneri:** `AppDelegate::applicationWillTerminate:` →
`Session::shutdown()` (`Msg::Shutdown`, thread join, çocuk `SIGHUP`). Duman
deadline hükmü ivar'a yazar ve `NSApplication::terminate(None)` çağırır;
`applicationWillTerminate:` kapanışı yapar, hüküm kırmızıysa **kapanıştan
sonra** `process::exit(1)` (yeşilde AppKit 0 ile çıkar). Son pencere
kapanınca da aynı yol. Duman çıktısı `kare=N hucre=K`: K, varsayılan olmayan
hücre sayısı — shell çıktısının gerçekten geldiğinin kanıtı (K = 0 → kırmızı).

## Karar 7: Klavye bu sette mi?

Klavyesiz terminal doğrulanabilir (prompt gelir) ama kullanılamaz. **(a) Yok,
003.** **(b) Asgari `keyDown:`**: `BateriView: NSView` alt sınıfı,
`characters` → PTY; Enter/Backspace/Tab/ok tuşları/Ctrl-C eşlenir; IME,
Option modları, kitty protokolü yok. **Öneri: (b)** — ~60 satır; `BateriView`
zaten geometri sahibi olarak planlanmıştı (001 altitude notu); klavye
odağını `acceptsFirstResponder` ile alır.

## Karar Noktaları

Önerilen yön: **1c, 2a, 3a, 4b, 5 tam grid, 6 terminate yolu, 7b.**

Bağımlılık kararları (kullanıcı onayı ister): `alacritty_terminal`
(`default-features = false`) `bt-core`'a; `dispatch2` `bt-shell`'e.
`portable-pty` tabandan düşer.

Kapsam dışı: glyph (003), tema/ayar dosyası, sekme/bölme, IME, seçim/kopyalama,
kaydırma (scrollback vardır ama görüntülenmez), OSC 133 komut blokları, imleç
animasyonu, `TERM` adı (`xterm-256color`).

## Muhakeme (10 Eylül 2026)

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU — `hucre=K` arka planla ölçülürse düz prompt K=0 verir (kapı bozuk); `AtomicBool` birleştirme, üçlü tampon ve exit-code ivar'ı erken; resize/hücre piksel boyutu pinlenmemiş |
| Codebase-fit | Onay, üç düzeltmeyle — alacritty `EventListener`'ı `bt-shell`'e sızarsa kapsül delinir; sayaç `commit`'te değil `addCompletedHandler`'da; `pipeline=ok` jetonu silinemez; `objc2::MainThreadBound` yok (`dispatch2::MainThreadBound` var); `Waker` `bt-gpu`'da olsun |
| İşletme | Koşullu onay — duman shell'i kullanıcının rc'sini test eder; `tty::setup_env()` `TERM=alacritty` yazar; `PtyWrite` ters yolu bağlanmazsa DSR soran prompt'lar asılır; `Shutdown`→join→drop üçlüsü ve bloklayan `Pty::drop`; `test-yaris` nightly'siz her phase'i `[~]` bırakır; CLAUDE.md'de 7 cümle değişir, 3'ü sayılmıştı; Apache-2.0 attribution borcu |

**Kabul edilen itirazlar → plan değişikliği:**

- **Kapsül `EventListener`'ı da kapsar.** `bt-core` kendi `Wake` trait'ini
  tanımlar (`wake()`, `child_exit()`); alacritty adapter'ı `bt-core` içinde.
  `Event::PtyWrite`, `ColorRequest`, `TextAreaSizeRequest` yanıtları da
  adapter'da, doğrudan `EventLoop` kanalına — ana thread'e hiç çıkmaz.
  `Title`/`Bell`/`Clipboard` yoksayılır; `ChildExit` → `Wake::child_exit`.
- **Sınır tek fonksiyon, maddi `Snapshot` yok.** `Session::frame(&self,
  sink: &mut dyn FnMut(CellBg)) -> Option<Cursor>`-benzeri tek çağrı: kilit
  bir kez alınır, hasar yoksa `None` (hiç iterasyon yok), varsa çözülmüş
  renkli arka planlar ve imleç verilir, `reset_damage()` **aynı kilitte**.
  `TermDamage` yalnız bool olarak okunur. Kilit `FairMutex::lock()`, asla
  `lock_unfair`. Renderer instance buffer'ı sink'ten doğrudan doldurur.
- **`AtomicBool` birleştirme yok.** Wakeup okuma başına en fazla bir kez
  gelir (`MAX_LOCKED_READ`); `exec_async` + `setPaused(false)` idempotent.
  Bayrağın kayıp-uyanma riski kazancından büyük.
- **Üçlü tamponlama yok**: kare başına `newBufferWithBytes`. `/measure`
  sonrası karar.
- **`Waker` `bt-gpu`'da.** `DisplayLink::waker() -> Arc<Waker>` (`Send + Sync`,
  `dispatch2::MainThreadBound<Retained<…>>` + `exec_async`); `bt-shell`'in
  `Wake` impl'i üç satır. `dispatch2` `bt-shell`'e girmez, `CLAUDE.md`
  `bt-shell` satırı değişmez.
- **Sayaç `addCompletedHandler`'da**, `status != Error` ise artar; 001'in
  "tamamlanmak sunulmak değildir" kapısı korunur. `block2` `RcBlock` için
  `bt-gpu`'ya açık bağımlılık girer (bağımlılık kararı; zaten lock'ta).
- **Duman sözleşmesi:** `kare=N hucre=K pipeline=ok` — jeton eklenir,
  silinmez; `CLAUDE.md` jeton listesi ve `proje.md` `duman` satırı aynı
  commit'te. **K = çizilen arka plan instance'ı (imleç hariç)**, render
  yolunu ölçer. Deterministik olsun diye `BT_RUN_SECONDS` yolunda shell
  sabitlenir: `/bin/sh -c "printf '\033[41m bateri \033[0m\n'; sleep 30"`
  → K = 8. Normal açılış `$SHELL` login. `login`, `.zshrc` süresi ve
  `$SHELL` duman yolundan çıkar.
- **`tty::setup_env()` asla çağrılmaz**; `TERM=xterm-256color`,
  `COLORTERM=truecolor` `Options.env` ile. (alacritty `ALACRITTY_WINDOW_ID` ve
  `WINDOWID`'yi koşulsuz yazar, kapatılamaz — kayda geçti.)
- **Kapanış:** `Session::shutdown()` = `Msg::Shutdown` → `join` → dönen
  `(EventLoop, State)`'i drop (SIGHUP burada gider); `applicationWillTerminate:`
  çağırır. Duman deadline: hükmü basar, `shutdown()`, `exit(kod)` — exit-code
  ivar ve `terminate:` hop'u yok. `bateri`'de `run_seconds × 3` bekçi thread'i
  `_exit(70)`: kapanış asılırsa `make` sonsuza kadar beklemez. Okuyucu thread
  ölümü `JoinHandle::is_finished()` ile görünür; `Session` `shutdown()`'suz
  drop edilirse alacritty'nin `panic!("event loop channel closed")`'ı okuyucu
  thread'de kalır — `Drop` impl'i `shutdown()` çağırır.
- **Resize pinlendi:** hücre piksel boyutu `const` yer tutucu (9×18 @1x,
  ölçekle çarpılır; 003 font metriğiyle değiştirir); tek yol
  `bounds → WindowSize → Term::resize + Msg::Resize`; reflow alacritty'nin,
  sınanmaz. `ChildExit` → uygulama sonlanır (`terminate`).
- **Display link pencereyle aynı phase'de**: pencereye takılı olmayan layer
  için doğrulanamaz. Bölüm dört phase: `bt-core` → `bt-gpu` cell_bg (001 duman
  sözleşmesi korunur) → `bt-shell` çizim (link, `Waker`, asenkron, **yeni
  duman sözleşmesi tek commit'te**) → `bt-shell` giriş/kapanış (klavye,
  `ChildExit`, `shutdown`, bekçi).
- **`make test-yaris` nightly'siz reçete:** `#[ignore]` işaretli
  `yaris_*` stres sınamaları (N thread `Wake` + `frame` yarışı) `--ignored`
  ile + `--test-threads=1` karşılaştırma; TSan nightly gelince eklenir.
  `proje.md` satırı phase-1'de düzeltilir; aksi hâlde 002'nin her phase'i
  `[~]` kalırdı.
- **Klavye:** saf `fn kod_cevir(chars, mods) -> Option<Cow<[u8]>>` (AppKit'siz
  sınanır) + `keyDown:` beş satır; `makeFirstResponder`; workspace
  `objc2-app-kit` feature listesine `NSEvent`. IME/`NSTextInputClient`,
  Option-as-Meta, kitty kapsam dışı (ölü tuşlar bozuk, not).
- **Cargo:** `objc2-quartz-core` feature `CAMetalDisplayLink` (`bt-gpu`).
  `draw_surface`, `Surface::layer()`, `GpuError::NoDrawable` link gelince
  silinir; ikinci çizim yolu bırakılmaz.
- **CLAUDE.md'de yedi cümle** (phase'lere dağıtıldı): taban listesi
  (`portable-pty` düşer, `alacritty_terminal` girer); hücre 24 bayt;
  `bt-core` platform sütunu ("macOS'a özgü hiçbiri; Unix PTY serbest");
  `bt-core` sorumluluk satırı (PTY girer, OSC 133 alacritty'de **yok** —
  komut blokları kapsül sınırına kanca isteyecek, not); `make duman` satırı;
  jeton listesi; "iskelet 001 ile kuruldu, VT motoru 002'de gelir" paragrafı.
- **Apache-2.0 attribution**: `alacritty_terminal` tek Apache lisanslı
  bağımlılık; `.app` içine THIRD-PARTY metni bundle setinin borcu, 002
  `teslim.md`'sine `[elle]` not.

**Reddedilenler:**

- "K = grid'de boş olmayan hücre" (Sadelik) — `Term` içeriğini ölçer, render
  yolunu değil; sabit shell + çizilen instance sayısı ikisini birden verir.
- Exit-code ivar + `terminate:` hop'u (kendi K6'm) — `shutdown()` + `exit`
  aynı işi daha az parçayla yapar; `applicationWillTerminate:` normal kapanışın
  yolu olarak kalır.
- `dispatch2`'nin `bt-shell`'e girmesi (kendi K3'üm) — `Waker` `bt-gpu`'da.
- Sayacın `commit`'te artması (kendi K4'üm).

## Karar (10 Eylül 2026, kullanıcı onayı)

- **Seçilen:** K1c `alacritty_terminal 0.26` (`default-features = false`)
  `bt-core` içinde kapsüllü — dışarıya kendi `Wake` trait'i, `CellBg`/`Cursor`
  ve tek `frame()` fonksiyonu; hücre 24 bayt · K2a alacritty `tty` +
  `EventLoop`, `setup_env()` yok, `TERM=xterm-256color` elle · K3 `Waker`
  `bt-gpu`'da (`dispatch2` ana kuyruk), bayrak yok · K4b paused link, sayaç
  `addCompletedHandler` · K5 tam grid, arka planlar + imleç, kare başına tek
  buffer · K6 `applicationWillTerminate:` → `shutdown()`, duman `shutdown()` +
  `exit`, bekçi thread · K7 asgari klavye. **Bağımlılık kararları (kullanıcı
  onaylı):** `alacritty_terminal` → `bt-core`; `block2` → `bt-gpu`;
  `portable-pty` tabandan düşer. Gerekçeler `## Muhakeme`'de.
- **Reddedilen:** K1b `vte` + kendi grid — aylar, bugün yetmeyen bir şey yok;
  kapsül sınırı ileride geçişi mümkün kılar · K2b `portable-pty` — ikinci PTY
  katmanı · K3b/c `CFRunLoopSource`, `performSelectorOnMainThread` · K4a link
  hep açık — boşta drawable dequeue · exit-code ivar + `terminate:` hop'u ·
  `AtomicBool` birleştirme · üçlü tamponlama (`/measure` sonrası) · "K = boş
  olmayan hücre" (içeriği ölçer, render yolunu değil).
