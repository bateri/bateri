# `bt-shell` ayrımı — Bağlam

## Mevcut Durum

`bt-shell` tek crate ve macOS'a bağlı (`CLAUDE.md` → Katman düzeni tablosu):
23 dosya (`lib.rs` + 22 modül), ~27 bin satır. Modüllerin bir kısmı AppKit'i hiç görmüyor, ama crate
bütünüyle `objc2-app-kit`'e bağlı olduğu için hiçbiri Linux'ta derlenemiyor.
Modüllerin platforma göre dökümü (`grep` ile sayıldı, 2026-09-30):

| grup | modüller | platforma değen |
|---|---|---|
| saf | `settings`, `split`, `zoom`, `notices`, `gesture`, `quote`, `keys` | hiçbiri (`keys` yalnız AppKit'in fonksiyon tuşu kod noktalarını sabit olarak taşıyor, API görmüyor) |
| `libc`'li, POSIX | `upload` | `kill`, `waitid` (Linux'ta da var); `jobs::SSH_VALUED`'a bağlı |
| macOS sistem çağrısı | `jobs` | `ProcessTable` trait'i zaten var; tek gövdesi `Libproc` (`proc_*`, `sysctl(KERN_PROCARGS2)`) |
| | `child` | `NSLocale` (yerel), `getpwuid_r` (POSIX), `login -qflp` (macOS), paketin `Contents/Resources` yolu |
| | `watch` | `dispatch2` vnode kaynakları; `install` bir `DispatchQueue` alıyor (iki çağrı yeri de `DispatchQueue::main()`) |
| AppKit | `app`, `view`, `pane`, `window`, `menu`, `settings_window`, `search_bar`, `split_view`, `uploader`, `clipboard`, `updater`, `pacer` | — |

`app.rs` ile `lib.rs`'in de AppKit görmeyen bir yarısı var: süreli koşunun
tipleri (`Run`, `Workload`), bekçi (`watchdog`, `libc` `write` + `_exit`),
duman kapısının sabitleri ve raporu (`IDLE_FRAME_LIMIT`, `QUIET_FLOOR`,
`Counters`, `Measured`, `Report`, `verdict`, `teardown_token`), girdi ve
hareket kararları (`decide_inputs`, `resolve_reduce_motion`,
`resolve_smooth_scroll`), entegrasyon ortamı (`shell_integration_env`,
`dock_rows_*`), ızgara bölmesi (`split_into_grid`), `Opening`/`initial_line`.

Tek dış tüketici `bateri` (bin): `bt_shell::{run, Options, Run, Workload}`.

`make denetim`'in katman kontrolü `bt-gpu`'nun `bt-shell`'e bağlanmadığını
arıyor; `make linux` `-p bt-core -p bt-atlas -p bt-gpu` koşuyor.

## Motivasyon

Linux yolunun dördüncü adımı (040 `discussion.md` → Kullanıcı kararları,
mimari adım 3): winit MVP'sinden önce AppKit'siz kod ortak bir crate'e
ayrılmalı ki Linux kabuğu onu yeniden yazmadan kullansın. Kararlar ve
kısıtlar `docs/YOL-HARITASI.md` → "`bt-shell` ayrımı" satırında; hedef
katman `bateri → bt-shell-{macos,linux} → bt-shell-common → bt-gpu →
{bt-atlas, bt-core}`. Davranış değişmez, macOS'ta kullanıcı hiçbir fark
görmez. Dil kısıtı: `.tasks/040-linux-kapisi-ve-wgpu/plan.md` → Dil kısıtı;
uygulanışının emsali 042 (`.tasks/042-font-sistemi-linux/phase-1.md`,
yerinde çeviri, taşımadan önce ayrı commit).

Linux kollarının tabanı alacritty 0.26.0'ın `tty/unix.rs`'i: macOS dışında
`default_shell_command` kabuğu **argümansız** doğuruyor (`Command::new(shell)`,
login değil); macOS'ta `login -flp` + `exec -a -zsh`. Yol haritası Linux
kolunu `$SHELL -l` diye yazmış — alacritty paritesi değil, bilinçli bir
sapma (discussion.md → Karar 1).

Docker imajında (`tools/linux/Dockerfile`, `rust:1.88-bookworm`) zsh kurulu
ve `/bin/zsh` usrmerge ile var; `jobs`'un gerçek PTY sınaması ve `watch`'ın
geçici dizin sınamaları imaja ek paket istemiyor.
