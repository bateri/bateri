# Phase 3 — `jobs` ve `child`'ın Linux kolları, `make linux` ortak crate'i koşuyor

## Özet

`ProcessTable`'ın `/proc` gövdesi ve kabuk komutunu ebeveyniyle birlikte
veren `child::shell_command` gelir; `make linux` `bt-shell-common`'ı da
koşar (`discussion.md` → Karar 1).

_Requirements: R3.1, R3.2, R3.4_

## Değişiklikler

- **`crates/bt-shell-common/src/jobs.rs`** (ya da `jobs/` altında adlı
  gövde dosyaları) — `cfg(linux)` `Procfs`: çocuklar, kabuğun ön plan grubu
  (`/proc/<pid>/stat`'ın `tpgid`'i), grubun üyeleri, ad (`comm`), argv
  (`cmdline`); `Libproc`'un sözleşmesiyle aynı cevaplar (okunamayan süreç
  `None`, panik yok). Platformdan bağımsız bir takma ad (`SystemTable` gibi)
  iki gövdeden birini adlandırır. Saf karar (`foreground`, `remote`)
  değişmez; gerçek PTY sınaması iki platformda koşar.
- **`crates/bt-shell-common/src/child.rs`** — `shell_command() ->
  (Option<(String, Vec<String>)>, ShellParent)`: macOS bugünkü
  `login_command` + `Login`; Linux `$SHELL` (yoksa passwd) + `["-l"]` +
  `Direct`; çözülemeyen kabukta `None` ve iki platformda da alacritty'nin
  kendi yolunun ebeveyni (macOS `Login`, Linux `Direct`). `ShellParent`
  `jobs`'tan buraya görünür. `locale_installed`'in doc'u Linux'ta
  `/usr/share/locale`'in yerel dizini olmadığını ve kolun orada koşmadığını
  söyler. `zsh_wrapper_dir`'in paket kolu `cfg(macos)`, Linux release'te
  `None` — bilinen sınır doc'ta adıyla (paketleme seti).
- **`crates/bt-shell/src/pane.rs`** — `start_session`'ın ebeveyni
  `shell_command`'dan (bugün `pane.rs:1251` sabitliyor); `Libproc` yerine
  takma ad. Süreli koşunun `Direct` kolu değişmez.
- **Sınama** — `cfg(macos)`: `shell_command` çözülmüş kabuk ve kullanıcıyla
  `/usr/bin/login` + `Login` verir; `zsh_wrapper_dir` debug'da `Some` (duman
  bu yollara uğramıyor, discussion.md → Karar 5). `cfg(linux)`: `-l` argv'si
  ve `Direct`; `Procfs`'in gerçek PTY sınaması (`sleep` ön planda).
- **`Makefile`** — `LINUX_CRATES` += `-p bt-shell-common`; `linux`
  hedefinin yorumu. **`tools/linux/Dockerfile`** — gerekiyorsa paket, başlık
  yorumu (İngilizce).
- **`.claude/is-akisi/proje.md`** — Doğrulama'nın `make linux` satırındaki
  crate listesi. **`CLAUDE.md`** — Komutlar'ın `make linux` satırı ve
  tablonun `bt-shell-common` satırı (Linux gövdeleri, kapı `make linux`).
- **`upload.rs`** — değişiklik beklenmiyor (`waitid`'in `siginfo_t`'si
  alanıyla okunmuyor, yalnız sıfırlanıyor); `make linux` sınar.

**Bilinen risk (İşletme jürisi):** `child`'ın gerçek zsh sınamaları `-l -i`
koşuyor ve Docker'da Debian'ın `/etc/zsh/{zshenv,zprofile,zshrc}`'sinden
geçiyor (bookworm'un `zshrc`'si kendi `zle-line-init`'ini kuruyor);
`bt-core`'un ayna sınamaları `-l`'siz olduğu için emsal değil. İlk koşuda
gözlenir; düşerse çare (imajda ortam, sınamanın kabuk bayrağı) Uygulama
Notları'na ve gerekçesiyle koda, sınamayı `cfg(macos)`'a çekmek değil.

## Kabul

- `make linux` yeşil: `bt-shell-common` Linux'ta `clippy -D warnings` ve
  `test`, `jobs`'un gerçek PTY sınaması ve `child`'ın zsh sınamaları dahil.
- macOS'ta sınama adları phase-2'ninkinin üst kümesi; yeni adlar yalnız
  eklenenler.
- `make hepsi` yeşil.

## Checklist

- [ ] `Procfs` ve takma ad
- [ ] `shell_command` ve `pane.rs`'in ebeveyni
- [ ] Test: `cfg(macos)` sabitleyici sınama
- [ ] Test: Linux'ta `Procfs` gerçek PTY ve `-l` argv
- [ ] `make linux` += `bt-shell-common`, `proje.md` satırı
- [ ] Doğrulama geçti (`make hepsi` + `make linux`)
