# Phase 2 — macOS bağlaması, alt komut ve sözleşme

## Özet

Cevabı ana thread'de canlı durumdan hesaplayan cevaplayıcıyı, dinleyiciyi
ve `bateri focus` alt komutunu bağla; tanınmayan alt komut GUI açmasın.

_Requirements: R1, R2, R3, R6, R7, R8_

## Değişiklikler

- **`crates/bt-shell-macos/src/pane.rs` / `stats.rs`** — pane'e son girdi
  damgası (ivar, ana thread'e ait, `focus`'un saatiyle); tek yazarı
  `TerminalPane::note_interaction`, doğumda kurulur. Yük göstergesinin
  `Schedule::interaction`'ı okunmaz (`discussion.md` → Muhakeme, reddedilenler).
- **`crates/bt-shell-macos/src/app.rs`**
  - Cevaplayıcı: soket thread'inden ana kuyruğa bir closure gönderir ve
    phase-1'in sınırıyla bekler; closure `pane_by_tab` ile pane'i bulur,
    `NSApp.isActive && window.isKeyWindow() && window.focused_pane() == pane`
    ve damgadan `idle`'ı hesaplar. Kapanmakta olan pane `None`
    (`find_open`'ın kuralı). Yeni paylaşılan durum yok.
  - Dinleyici `masters()`'ta kendi thread'inde, `Masters::bases()`'in ilk
    dizinine (`OnceLock`, süpürmeyle yarışı güvenli); `masters` süreli koşuda
    kurulmadığı için hermetik kapı ayrıca yazılmaz. Dizin yoksa dinleyici yok,
    sessizce.
- **`crates/bt-shell-macos/src/lib.rs`** — `focus()` girişi (`ssh_argv`'nin
  emsali): argv'yi, kökleri (`socket_bases`) ve stdout'u `focus_main`'e verir.
- **`crates/bateri/src/main.rs`** — `focus` alt komutu askpass/`ssh-argv`'nin
  yanında, Aqua kontrolünden önce; ardından `-` ile başlamayan tanınmayan
  argv[1] tanı satırıyla (stderr, İngilizce) ve sıfırdan farklı kodla çıkar,
  GUI açmaz. `make smoke`'un argümansız açılışı ve LaunchServices'in
  `-psn_…`'i etkilenmez.
- **`CLAUDE.md`** — 038'in `bateri://` cümlesinin yanına tek cümle: dış süreç
  yalnız elindeki UUID için `bateri focus` ile odak + kaba `idle` sorabilir,
  cevap sorgu anında ana thread'de, işaretçi `.tasks/050-odak-sorgusu/`;
  `bateri` satırının alt komut listesine `focus` ve tanınmayan alt komut
  kuralı.

## Kabul

- Elle: bir pane'de `bateri focus --pid $(pgrep -n bateri) $BATERI_TAB_URL`
  → `pane=live focused=1 idle=0`; başka uygulamaya geçip aynı komut
  (Terminal'den) → `focused=0`; bölmede öbür pane'in URL'si → `focused=0`;
  dokunmadan bekleyince `idle` artar; ⌘F alanı odaktayken `focused=1`;
  pane'i kapatınca `pane=none`; bateri kapalıyken `pane=none`; `bateri
  nonsense` pencere açmadan çıkar.
- `make check`, `make bundle`, `make smoke` yeşil.

## Checklist

- [ ] Pane'in son girdi damgası
- [ ] Ana kuyruk cevaplayıcısı + dinleyicinin kurulması
- [ ] `bateri focus` girişi ve `main.rs`'in tanınmayan alt komut kuralı
- [ ] `CLAUDE.md` satırı
- [ ] Test: yukarıdaki elle senaryolar; varsa saf parçalar için birim sınama
- [ ] Doğrulama geçti (`make check` + `make bundle` + `make smoke`)
