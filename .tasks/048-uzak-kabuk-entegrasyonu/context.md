# Uzak kabuk entegrasyonu — Bağlam

## Mevcut Durum

bateri ssh'ı **sonradan** tanıyor ve uzak tarafa hiçbir şey kurmuyor (036,
`docs/YOL-HARITASI.md` → "tam uzak entegrasyon" satırı bu setin numarasını
alıyor):

- **Algılama** `C` kenarında süreç tablosundan (`jobs::remote` →
  `jobs::ssh_target`, `bt-shell-macos/src/pane.rs` → `RemoteProbe`); hedef
  argv'yle birlikte `bt-core`'a gider (`Session::set_remote`, 037 Karar 1).
  Uzak komutlu ssh yalnız `-t` ile etkileşimli sayılır
  (`crates/bt-shell-common/src/jobs.rs:310`).
- **Uzak OSC 7 bugün de okunuyor**: uzak oturum sürerken her OSC 7 uzak
  yuvaya gidiyor (`ShellLog::apply_scan_answering`, 036 Karar 4). Eksik olan
  uzaktaki **göndericidir** — sunucu OSC 7 basmıyorsa dizin başlıktan
  tahmin ediliyor (`shell::title_directory`), o da tutmazsa pane'in etiketi
  "Remote folder unknown — enable OSC 7 on the server" diyor
  (`remote_helper::REMOTE_CWD_UNKNOWN`).
- **Uzak OSC 133 bilerek yok sayılıyor**: kimliğimizi (`bt_block=`) bir kez
  görmüş oturumda, uzak oturum etkinken kimliksiz `A`/`B`/`C`/`D` hiçbir şeye
  dokunmuyor ve uzak oturumu **yalnız bizim** `A`/`D`'miz bitiriyor
  (`ShellLog::apply`, `crates/bt-core/src/shell.rs:1700`; `command_open`;
  `.tasks/036-ssh-uzak-oturum/phase-3.md` → Uygulama Notları). Yani uzakta
  blok şeridi, chevron ve süre sayacı yok.
- **Yerel sarmalayıcı yalnız zsh'te** (`assets/shell/zsh/`: dört
  `ZDOTDIR` dosyası + `bateri.zsh`, 40 704 bayt). Kimlikli işaretleri ve
  aynayı o basıyor; `[shell] integration = "off"`'ta hiç kurulmuyor,
  `"blocks"`'ta dock'suz kuruluyor (`app::shell_integration_env`).
- **Yeniden koşturma argv'yi aynen kullanıyor**: `⏎ reconnect` ve ⌘T'nin
  "aynı host'a" satırı `RemoteTarget::line`/`argv`'yi kabuğa yazıyor (037
  Karar 6, 8); `-t` ile gelen uzak komut argv'de kalıyor.
- **Dosya işleri** (`upload::ssh_argv`) kullanıcının argv'sinden uzak komutu
  düşürüyor; uzak komut eklemek onları etkilemiyor.

## Motivasyon

Kullanıcı bildirdi (2026-10-02): bir arkadaşının Fedora + oh-my-zsh
sunucusunda pane'in etiketi "Remote folder unknown" diyordu. Başlıktan okuma
düzeltildi (`8ba8d9c`), ama kalıcı çözüm sunucuda OSC 7. Kullanıcının ilkesi:
**kullanıcıya bir şey kopyalatmak iyi bir deneyim değil**; özellik
kendiliğinden çalışmalı ve gerekirse kapatılabilmeli.

Kazanç yalnız dizin değil: uzakta da blok şeridi ve süre sayacı, ⌘-tıkın
göreli yolları her sunucuda, ve ileride uzak dock aynı altyapının üstünde.

### Rakipler ve şikayetler (2026-10-02 taraması)

- **kitty `kitten ssh`**: ayrı komut; POSIX `sh` önyüklemesi, dosyaları
  tty üzerinden bir kerelik parolayla alıyor, `~/.local/share/kitty-ssh-kitten`'e
  açıyor, giriş kabuğunu entegrasyonla exec ediyor
  (sw.kovidgoyal.net/kitty/kittens/ssh). Asıl motivasyonu `xterm-kitty`
  terminfo'su — bizde yok, `TERM=xterm-256color`. Şikayetler: ayrı komut
  yazmak (Ghostty #5892'nin açılışı), sunucuya dosya yazmak (kitty #1139),
  multiplexer içinde prompt'un hiç gelmemesi (#7355, tty aktarımı yüzünden),
  BSD'nin kısıtlı `sh`'ı, `su -` sonrası kayıp (#7240), uzun argüman (#9129,
  firejail). Kaçış kapısı host başına `delegate`.
- **Ghostty**: yerel kabuk entegrasyonu bir `ssh` fonksiyonu tanımlıyor,
  kullanıcı düz `ssh` yazıyor; varsayılan **kapalı**
  (`shell-integration-features = ssh-env,ssh-terminfo`). Sınırı: betikten ya
  da alt kabuktan çağrılan ssh'a fonksiyon ulaşmıyor (#9708).
- **iTerm2**: SSH entegrasyonunun uzakta `/tmp/framer.txt`'e girdi/çıktı
  yazdığı açık (CVE-2025-22275). Ders: `/tmp` yok, kayıt yok, en az iz.

### Kanıt — tasarımı sınırlayan üç olgu

1. **Kaynak ayrımı yok.** Uzakta bizim betiğimiz koşup `bt_block=` basarsa,
   yerel `ShellLog` ilk uzak `A`'yı "yerel prompt geldi" diye okur ve uzak
   oturumu siler (`apply` → `PromptStart` → `context.clear_remote()`).
   Kimliksiz basarsa yok sayılır. İkisi de istenen değil: uzak işaretin
   **kendi** adı olmalı.
2. **Yeniden koşturmaya sızıntı.** Önyükleme `-t host -- <komut>` olarak
   giderse algılama onu etkileşimli sayar (doğrulandı), ama `RemoteTarget::line`
   önyüklemeyi taşır ve reconnect/⌘T onu kabuğa yazar.
3. **Boyut.** `bateri.zsh` 40 704 bayt, base64 ~54 KB; Linux'ta tek argüman
   sınırı 128 KB (`MAX_ARG_STRLEN`), sığıyor. Bedeli: uzak komut uzakta `ps`'de
   görünür (içerik açık kaynak betik, sır değil).
