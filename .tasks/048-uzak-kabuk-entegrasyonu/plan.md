# Uzak kabuk entegrasyonu

## Hedef

Kullanıcı düz `ssh` yazdığında bateri uzakta kabuk entegrasyonunu kendisi
kurar: dizin (OSC 7) ve komut blokları (OSC 133) uzakta da çalışır, sunucunun
rc dosyalarına dokunulmaz, özellik genel olarak ve host başına kapatılabilir.
Kırılma riski olan hiçbir bağlantı sarılmaz; sarılamayan her yol bugünkü
sonradan algılamaya düşer. Kararlar: `discussion.md` → Muhakeme ve
`## Karar (2026-10-02, kullanıcı onayı)`.

## Gereksinimler

- **R1** — Sarma kararı tek yerde ve güvenli yönde.
  - **R1.1** — Yalnız etkileşimli ssh sarılır: uzak komut yok, etkileşimsiz
    bayrak yok, stdin/stdout tty. `scp`, `rsync`, `git`, `ssh host komut`,
    pipe'lı ssh dokunulmadan geçer.
  - **R1.2** — `ssh -G` okunur; `RemoteCommand` dolu, `RequestTTY no` ya da
    `SessionType` `default` dışıysa sarılmaz.
  - **R1.3** — Ayar: `[remote] integration` (varsayılan açık), host başına
    `integration` alanı, `production` işaretli host'ta varsayılan kapalı;
    okunamayan `settings.toml`'da kapalı.
  - **R1.4** — Host ancak öğrenildiyse sarılır (ilk bağlantıda öğren);
    öğrenilen kalıcı.
- **R2** — Argv temiz kalır: `wrap`/`unwrap` gidiş-dönüş eşit; `jobs`'un
  hedefi, `⏎ reconnect` ve ⌘T kullanıcının yazdığı satırı görür; dosya
  işleri değişmez.
- **R3** — Uzakta OSC 7.
  - **R3.1** — zsh, bash ve fish giriş kabuğunda, kullanıcının kendi giriş
    dosyaları aynen okunarak; rc dosyasına yazılmaz.
  - **R3.2** — Dosyalar yalnız `~/.local/share/bateri/shell/`'e (geçici ad +
    `mv`), `/tmp` ve kayıt yok; yazılamazsa düz giriş kabuğu ve pane'in
    etiketinde neden.
  - **R3.3** — motd'u önyükleme basar.
  - **R3.4** — Başka giriş kabuğunda (sh, csh, BusyBox) entegrasyonsuz düz
    kabuk.
- **R4** — Uzak oturum etkinken OSC 8133 yerel dock'a ulaşmaz.
- **R5** — Uzakta komut blokları: şerit, chevron ve süre sayacı; kimlik yerel
  ssh bloğuyla önekli, ssh bitince şeritler geçmişte kalır, yerel safha ve
  uzak oturum uzak işaretten etkilenmez.
- **R6** — Ayar penceresinde onay kutusu, Shell menüsünde host başına aç/kapa,
  `docs/AYARLAR.md`.
- **R7** — Sarılmış etkileşimli oturum 047'nin soketinde master olur
  (kullanıcının kendi `ControlMaster`'ı yoksa); dosya işleri onun üstünden
  parolasız geçer.

## Yaklaşım

1. **Karar altyapısı (`bt-shell-common`).** `ssh_wrap`: `wrap`/`unwrap` saf
   fonksiyonları (önek tek yerde üretilir, `unwrap` konumla), `ssh -G`
   okuması ve kuralları, `host_key` (kanonik üçlü), host durum dosyası
   (`posix`/`touched`). `jobs::ssh_target` yürüyüşten önce `unwrap` uygular.
2. **Ayar (`bt-core::settings`).** `[remote] integration` ve `HostRule`'a
   `integration`; çözüm "host kuralı → prod işareti → genel anahtar".
3. **Alt komut (`crates/bateri`).** `bateri ssh-argv -- …` Aqua denetiminden
   önce ayrılır, gövdesi `bt_shell_macos` üzerinden `ssh_wrap`'te; sarılmış
   argv'yi basar ya da boş döner.
4. **Sızıntı savunması (`bt-core::shell`).** Uzak oturum etkinken OSC 8133
   yok sayılır.
5. **Yerel fonksiyon ve uzak betikler (`assets/shell/`).** `bateri.zsh`
   kullanıcının dosyalarından sonra `ssh` fonksiyonunu tanımlar (kullanıcının
   `ssh` alias/fonksiyonu yoksa); fonksiyon `$BATERI_BIN ssh-argv`'yi sorar,
   boşsa `command ssh`. Önyükleme POSIX `sh`, base64'lü yükü açar; kabuk başına
   küçük betik, `ZDOTDIR` dansı yerel sarmalayıcıyla paylaşılan dosyada.
6. **Öğrenme (`bt-shell-macos`).** Yardımcı oturumun selamı geldiğinde host
   öğrenilmemişse `posix` yazılır.
7. **Uzak bloklar (`bt-core`).** Faz + saat + halka `BlockTrack` tipine çıkar,
   yerelde ve uzakta iki örnek; `bt_remote=<P>.<n>` ve `bateri://rblock/`
   `ShellLog::apply`'de kimlik hesabından önce ayrılır; çizim döngüleri iki
   ad alanını okur.
8. **Arayüz.** Ayar penceresi (Remote Files) ve Shell menüsü
   (`SettingsEdit`).
9. **Paylaşılan bağlantı.** `wrap`, 047'nin `ssh_route` soketini
   `ControlMaster=auto` ile ekler; kullanıcının kendi master'ı varsa eklemez.

## Kapsam Dışı

- Uzak dock (OSC 8133 uzakta; ayna, düzenleme widget'ı, tazelik) — ayrı set.
- İç içe ssh (uzakta `ssh` fonksiyonu tanımlanmaz), `su -`/`sudo -i`, uzak
  tmux içindeki kabuklar, mosh — bugünkü gibi entegrasyonsuz.
- Yerel bash/fish sarmalayıcısı (yerelde `ssh` fonksiyonu yalnız zsh'te).
- "Remove bateri files from this host" düğmesi (liste tutuluyor, düğme sonra).
- terminfo taşımak (`TERM=xterm-256color` kalır).
- `Last login:` satırının taklidi.

## Akış

```
yerel zsh: ssh web-01
  └─ ssh() ── "$BATERI_BIN" ssh-argv -- web-01
               ├─ etkileşimli mi? ssh -G kuralları? ayar? öğrenildi mi?
               ├─ hayır → (boş) ──────────────► command ssh web-01   (bugünkü yol)
               └─ evet  → ssh -t web-01 -- <önek> exec sh -c '<önyükleme>'
                                │
uzak sshd: $SHELL -c '<komut>' ─┘
  └─ sh: ~/.local/share/bateri/shell/ ← tmp+mv (yazılamazsa: exec "$SHELL" -l + neden)
         motd bas → giriş kabuğunu entegrasyonla exec (zsh ZDOTDIR / bash --rcfile / fish)
         prompt başına: OSC 7 (phase-2) · OSC 133 bt_remote=<P>.<n> (phase-3)

bateri: C kenarı → jobs::ssh_target(unwrap(argv)) → Session::set_remote(host)   ← satır "ssh web-01"
        uzak OSC 7 → uzak yuva (bugünkü yol)   ·  uzak 8133 → yok sayılır
        yardımcı oturumun selamı → host öğrenilmemişse remote-hosts'a posix
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | ✅ |
| phase-3 | |
| phase-4 | |
| phase-5 | |
| kapı | |
