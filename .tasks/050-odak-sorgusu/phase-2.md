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

## Uygulama Notları

- **Damga stats'ın kancasında:** `last_input` (`Cell<Moment>`, doğumda
  `Moment::now()`) `stats.rs`'in `note_interaction`'ının ilk satırında
  yazılıyor; erişim `TerminalPane::input_stamp` (`stats_driver`'ın emsali).
  Altı view kancası + `windowDidBecomeKey` yeni çağrı yeri olmadan kapsandı.
- **`Masters::bases` `pub` oldu** (bt-shell-common'da tek satır); dinleyici
  süpürme thread'inde, `sweep`'ten **önce** kuruluyor — süpürme ölü soket
  başına ssh koşturuyor, o arada gelen sorgu beklemesin.
- **Ana kuyruk sırası sınırsız bırakıldı:** `ANSWER_WAIT`'ten sonra koşan
  hop düşmüş alıcıya yollayıp kayboluyor; ikinci bir sayaç kurulmadı (R5'in
  sınırı bekleme, sunucunun `MAX_IN_FLIGHT`'ı eşzamanlılığı zaten kısıyor).
- **`focus` kökleri `socket_bases(child::home().as_deref(), uid)`** —
  dinleyicinin (`app::masters`) ifadesinin aynısı; `ssh_argv`'nin "ev yoksa
  çık"ı değil, yoksa `--pid` örneği bulamazdı. UTF-8 olmayan argv kullanım
  hatası (`focus::USAGE` `pub` oldu, metin tek yerde).
- **Tanınmayan alt komut 64 (EX_USAGE)**, aşağıdaki 78'in (EX_CONFIG)
  sysexits ailesinden; kural saf fonksiyon (`is_unknown_subcommand`) ve
  sınaması `main.rs`'de.
- **Set kapısı (/code-review) iki bulgu:** (1) `accept`'in geçici hatası
  (`EMFILE`/`ENFILE`/`ENOBUFS`/`ENOMEM`) dinleyiciyi
  kalıcı öldürüyordu → 50 ms bekleyip dinlemeye devam (`focus::is_transient`,
  sınamalı). (2) ⌘F alanına yazmak `focused=1` sayılıyor ama damgayı
  yenilemiyordu → alanın eylemi ve komut kancası da `note_interaction`.
  Kalan: yalnız menü kısayoluyla (⌘V, ⌘K…) geçen süre damgalanmıyor — WAIVE.
- **Elle doğrulanan (GUI'siz):** `bateri nonsense` → pencere yok, 64;
  `bateri focus` / `focus bad` / `--pid 0` → kullanım satırı, 2;
  `--pid 1` → `pane=none`, 0; pid'siz → kullanıcının 050 öncesi açık
  bateri'si soketsiz olduğu için `pane=unknown`, 3 (phase-1'in kuralı).
  Açık pencere isteyen senaryolar (focused=1/0, bölme, ⌘F, idle artışı,
  pane kapanınca none) gözle kontrolde.

## WAIVE

- **Menü kısayolu damga değil:** `performKeyEquivalent:` ile giden ⌘V/⌘K/⌘G
  `keyDown:`'a uğramıyor ve `idle`'ı sıfırlamıyor. Yalnız kısayolla geçen bir
  aralık nadir, hata yönü `focused=1` iken `idle`'ın fazla görünmesi (tüketici
  odaktayken zaten susar); kapatmak her pane seçicisine ya da bir
  `sendEvent:` hunisine dokunmayı isterdi — 050'nin kapsamından büyük.

## Checklist

- [x] Pane'in son girdi damgası
- [x] Ana kuyruk cevaplayıcısı + dinleyicinin kurulması
- [x] `bateri focus` girişi ve `main.rs`'in tanınmayan alt komut kuralı
- [x] `CLAUDE.md` satırı
- [x] Test: yukarıdaki elle senaryolar; varsa saf parçalar için birim sınama
- [x] Doğrulama geçti (`make check` + `make bundle` + `make smoke`)
