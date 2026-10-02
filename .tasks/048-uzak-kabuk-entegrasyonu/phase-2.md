# Phase 2 — Uzakta OSC 7: fonksiyon, önyükleme ve öğrenme

## Özet

Özelliği aç: yerel zsh'in `ssh` fonksiyonu, uzak önyükleme ve kabuk başına
küçük betikler (OSC 7 + motd), yardımcı oturumun selamından öğrenme, sessiz
düşüşün etiketi ve paketleme.

_Requirements: R1.4, R3_

## Değişiklikler

- **`assets/shell/zsh/bateri.zsh`** — kullanıcının dosyaları yüklendikten
  sonra (`__bateri_end`), kullanıcının `ssh` alias'ı ya da fonksiyonu yoksa
  `ssh` fonksiyonu: `"$BATERI_BIN" ssh-argv -- "$@"` (tty biti `-t 0 && -t 1`
  ile), cevap boşsa ya da komut başarısızsa `command ssh "$@"`, doluysa
  `command ssh` + dönen argv. `[shell] integration = "off"`'ta zaten
  kurulmuyor; `"blocks"`'ta var.
- **`assets/shell/remote/`** (yeni, kaynak) — önyükleme (POSIX `sh`): yükü
  base64 yedek zinciriyle (`base64 -d`, `base64 -D`, `b64decode`,
  `openssl base64 -d`) açar, `~/.local/share/bateri/shell/`'e geçici ad +
  `mv` ile yazar (`mktemp` yok), motd'u basar (`/etc/motd`, varsa
  `/run/motd.dynamic`), giriş kabuğunu seçer: zsh → `ZDOTDIR` dansı, bash →
  `--rcfile` (giriş dosyası sırası kitty'nin bash yöntemiyle), fish → kendi
  OSC 7'si doğrulanırsa hiçbir şey, değilse `vendor_conf.d` + `XDG_DATA_DIRS`
  geri yazımı; başka kabuk ya da yazma hatası → `exec "$SHELL" -l` ve
  bateri'nin okuyacağı tek satırlık neden (biçimi burada karar; ör. bir OSC
  ile). Uzak kipte `ssh` fonksiyonu yok.
- **`assets/shell/zsh/`** — `ZDOTDIR` dansı (`__bateri_begin`/`__bateri_end`)
  yerel sarmalayıcı ile uzak zsh betiğinin paylaştığı ayrı bir dosyaya çıkar;
  yerel davranış bit bit aynı.
- **`crates/bt-shell-common/src/ssh_wrap.rs`** — önyükleme yükü gömülür
  (`include_str!` ya da paketteki dosyadan; karar Uygulama Notları'nda),
  `decide` artık sarabiliyor. Tırnak tek ve ters bölüsüz (uzak giriş kabuğu
  fish/csh olabilir, 037'nin kuralı).
- **`crates/bt-shell-macos/src/app.rs`** — `BATERI_BIN`
  (`shell_integration_env`'in yanında), durum dosyasının yolu.
- **`crates/bt-shell-macos/src/`** (yardımcı oturumun selamını alan yer) —
  host öğrenilmemişse `ssh_wrap::host_key` + `posix` yazımı, arka planda ve
  uzak nesil başına en çok bir kez.
- **Önyüklemenin nedeni → pane'in etiketi** — bugünkü etiket yuvası
  (`REMOTE_CWD_UNKNOWN`'ın yanında), metni İngilizce.
- **`Makefile`** (`bundle` kopya + `cmp` listeleri), **`crates/bateri/src/bundle_assets.rs`**
  (envanter) — yeni dosyalar.
- **Sınama altyapısı** — sshd'nin yaptığı `"$SHELL" -c '<komut>'`
  `bt-shell-common`'da `jobs`'un gerçek PTY'siyle, geçici ev dizininde; giriş
  kabuğu zsh/bash/fish/dash/busybox (`make linux` imajında olmayanlar için
  `tools/linux/Dockerfile`'a paket — imaj değişikliği bir karar, Uygulama
  Notları'na). Hata kolları: salt okunur ev, `base64`'süz PATH.

## Kabul

- PTY simülasyonunda her giriş kabuğu için: prompt'a varış, beklenen OSC 7
  baytları, kullanıcının giriş dosyasının etkisi (bir değişken) görünüyor,
  ev dizininde rc dosyası değişmemiş, motd basılmış.
- Hata kolları: salt okunur ev ve `base64`'süz PATH'te düz kabuk + neden
  satırı.
- Öğrenme sınaması: selam → `posix` satırı; ikinci selam yazmıyor; okunamayan
  dosyada sarma yok.
- `make check`, `make bundle`, `make linux` yeşil.
- Gözle kontrol (set kapısında): öğrenilmiş bir host'a `ssh` → pane'in
  etiketi "Remote folder unknown" demiyor, dock'ta `⇄ host  /yol`, `cd` ile
  değişiyor; `⏎ reconnect` satırı kullanıcının yazdığı.

## Checklist

- [ ] `bateri.zsh`'te `ssh` fonksiyonu
- [ ] `ZDOTDIR` dansı paylaşılan dosyaya
- [ ] `assets/shell/remote/`: önyükleme + zsh/bash/fish betikleri + motd
- [ ] `ssh_wrap`: yük ve gerçek sarma
- [ ] `BATERI_BIN`, öğrenme kancası (`ssh_wrap::host_key` + `record(Posix)`), etiketteki neden (durum dosyasının yolu phase-1'de geldi: `bt-shell-macos::remote_hosts_path`)
- [ ] `Makefile` + `bundle_assets`
- [ ] Test: PTY simülasyonu (5 kabuk), hata kolları, öğrenme
- [ ] Doğrulama geçti (`make check` + `make bundle` + `make linux`)
