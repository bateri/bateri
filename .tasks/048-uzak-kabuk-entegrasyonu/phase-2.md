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

## Uygulama Notları

- **Tel iki katlı**: uzak komut `exec sh -c '<tek satır>' bateri-boot`; tek
  satır (Rust'ta, `ssh_wrap::one_liner`) çözücü zincirini (`base64 -d`,
  `base64 -D`, `b64decode -r`, `openssl base64 -d -A`) dener, çözülen metin
  sihirli satırla (`bateri_boot=1`) başlıyorsa `eval` eder, yoksa `8133;f;decode`
  basıp düz giriş kabuğunu açar. Okunur betik çözülen yükte
  (`assets/shell/remote/boot.sh`) ve tırnak kısıtı yok. **Tırnak kuralı
  genişledi** (phase-1 yalnız `'`'yu reddediyordu): tek satır `'`, `\`, `!`
  ve satır sonu taşımıyor (`ssh_wrap::is_inline`) — fish tek tırnak içinde
  `\'`/`\\` okuyor, csh `!`'de geçmiş genişletiyor ve tırnak içinde satır
  sonunu reddediyor; ESC baytı bu yüzden `awk`'ın `%c`'sinden. Gerçek sshd
  üstünden tcsh ve BusyBox ash ile ölçüldü (aşağıda).
- **Yük derlemede gömülü** (`include_str!`), pakete kopyalanmıyor: alt komut
  her `ssh`'ta koşuyor ve paket dosyası okumamalı. Pakete giren tek yeni dosya
  paylaşılan `assets/shell/zsh/zdotdir.zsh` (envanter beşten altıya;
  `Makefile` kopya + `cmp`, `bundle_assets`, `child`'ın iki listesi). Uzak
  komut ~30 KB (Linux'un tek argüman sınırı 128 KiB; sınama 64 KiB'ın altını
  bekliyor).
- **Dosyalar yükte tek tırnaklı literal** (`'` → `'\''`) + `printf '%s'`,
  here-document değil: `sh` bash olan sunucuda (RHEL, bash < 5.1) her
  here-document `/tmp`'de dosya olur — "`/tmp` yok" kuralı. Yazım alt
  kabukta `umask 077` ile (kullanıcının kabuğu kendi `umask`'ını tutuyor);
  göreli olmayan `XDG_DATA_HOME` okunuyor, yoksa `~/.local/share`.
- **zsh uzakta yerelin dört dosyasını aynen** yazıyor; `ZDOTDIR` dansı
  (`__bateri_dir`/had/user, `__bateri_begin`/`__bateri_end`/`__bateri_restore`)
  `zdotdir.zsh`'e çıktı ve iki gövde de (`bateri.zsh`, uzakta
  `remote/zsh/bateri.zsh`) onu kendi üst düzeyinden `source` ediyor; dock
  kararları yerelde kendi kez-bekçisiyle (`${+__bateri_dock}`) kaldı. Yerel
  davranışın tanığı `the_zsh_wrapper_loads_the_users_files_and_reports_marks`
  ve dock/blok sınamaları — yeşil, değişmeden.
- **`ssh` fonksiyonu `__bateri_end`'de değil `__bateri_hooks`'ta** (plan
  `__bateri_end` diyordu): dans uzakla paylaşılıyor ve uzakta `ssh` yok;
  `__bateri_hooks` kullanıcının `.zshrc`'sinden sonra koşuyor. Kullanıcının
  `.zlogin`'de tanımladığı `ssh` fonksiyonu bizimkini zaten eziyor, alias da
  fonksiyonu yeniyor. `BATERI_BIN` kabuk değişkenine (`__bateri_bin`) alınıp
  ortamdan siliniyor (`BATERI_DOCK` emsali). Bekçisi
  `the_wrappers_ssh_function_asks_the_binary` (gerçek zsh: `--tty` yalnız
  terminalde, NUL ayrık cevap, kullanıcının fonksiyonu kazanıyor).
- **bash `--posix -l` + `ENV`** (planın `--rcfile`'ı değil; plan da "kitty'nin
  yöntemi" diyordu): `--rcfile` giriş kabuğunda yok sayılıyor, POSIX kipinde
  etkileşimli bash yalnız `$ENV`'i okuyor; dosyamız kipten çıkıp
  `/etc/profile` + ilk okunur `~/.bash_profile`/`~/.bash_login`/`~/.profile`'ı
  üst düzeyde okuyor — gerçek giriş kabuğu (`logout`, `~/.bash_logout`).
  POSIX kipinin geçmiş dosyası `~/.sh_history` olurdu: kullanıcının
  `HISTFILE`'ı yoksa önyükleme bash'in varsayılanını yazıp dosyamız ihracını
  geri alıyor. **bash 4'ten önce `--rcfile -i`**: Apple'ın `/bin/bash` 3.2'si
  `--posix`'te `$ENV`'i okumuyor (ölçüldü; upstream 3.2/4.0/4.4/5.0 okuyor) —
  orada kabuk giriş kabuğu değil, dosyalar yine giriş sırasıyla okunuyor.
- **fish her zaman `vendor_conf.d`** (planın "kendi OSC 7'si doğrulanırsa
  hiçbir şey" kolu yok): fish'in kendi OSC 7'si sürüme ve terminali
  tanımasına bağlı ve önyükleme bunu bilemez (fish 3.6 bizim ortamımızda hiç
  basmadı, ölçüldü); ikinci, aynı rapor zararsız ve phase-3'ün 133'ü dosyayı
  zaten istiyor. Dosya ilk iş `XDG_DATA_DIRS`'i geri alıyor (yoksa siliyor).
- **Uzak OSC 7'nin yetkisi sunucunun adı** (`$HOST`/`$HOSTNAME`/`$hostname`),
  yerelin boş yetkisi değil: yabancı yetki yoklama inmeden de uzak yuvaya
  gidiyor, boş yetki o pencerede yerel dizini ezerdi. Yerel sarmalayıcı
  değişmedi.
- **Nedenin teli `8133;f;{write|decode|shell}`** (biçim burada karar):
  sabit kod, sunucudan metin yok. `bt-core`'da `DockEvent` değil ayrı bir
  `ScanEvent::RemoteSetup` — phase-1'in uzak 8133 kapısı onu yutmuyor ve
  dock'un durumuna dokunmuyor (bilinmeyen kod hiçbir şey); yuvası
  `DockContext::remote_setup`, `remote_cwd` gibi `C`/`D`/`A` ile siliniyor.
  Etiket `remote_helper::remote_cwd_unknown(fault)`: nedeni söylüyor, yoksa
  bugünkü "enable OSC 7" metni.
- **Öğrenme kancası `RemoteHelper::with_greeted`**: worker selamı alınmış
  oturumun argv'siyle, **cevaptan sonra** ve uzak nesil başına bir kez
  çağırıyor (ilk ⌘-hover `ssh -G`'yi beklemesin); pane onu yalnız masters
  varken (süreli koşu dışında) kuruyor. `ssh_wrap::learn` `ssh -G`'yi aynı
  argv'yle soruyor (rotanın `-o`'ları user/host/port'u değiştirmiyor) ve
  `knows` ise yazmıyor.
- **motd**: `/run/motd.dynamic` sonra `/etc/motd` (`-ef` ile aynı dosya bir
  kez), `~/.hushlogin` susturuyor. **Gerçek sshd ile doğrulandı** (Docker'da
  geçici Debian bookworm sshd, anahtarlı; kullanıcılar zsh/bash/fish/tcsh/
  BusyBox ash): motd bir kez, sshd'nin `Last login`'i yok (komutlu oturumda
  `do_login` koşmuyor — kabul), üç kabukta her prompt'ta OSC 7 ve
  kullanıcının giriş dosyası, tcsh ve ash'te `8133;f;shell` + düz kabuk.
  Container sınamadan sonra silindi.
- **`$0` farkı**: giriş kabuğu `-l` ile başlıyor, yani `$0` yolun kendisi
  (`-zsh` değil; `exec -a` POSIX değil) — `$0`'ın başındaki `-`'ye bakan giriş
  dosyası farkı görür.
- **Sınama altyapısı** `ssh_wrap::remote_shells`: sshd'nin
  `"$SHELL" -c '<komut>'`'u gerçek PTY'de (`bt_core::Session`), geçici ev;
  kurulu olmayan kabuk `SKIPPED` (macOS'ta fish ve BusyBox yok). Salt okunur
  ev root'ta `chmod`'la kurulamadığı için dizinin yerinde bir dosya. İmaja
  `fish` ve `busybox` girdi (`tools/linux/Dockerfile`, yalnız sınama
  paketleri). `child::tests::screen` paylaşılmak için `pub(crate)`.

- **`/code-review` (10 bulgu; 8'i giderildi, 2'si waive):** bash 3.2'nin
  `printf "'c"`'si 0x7F üstü baytı işaret genişletiyordu (`ğ` →
  `%FFFFFFFFFFFFFFC4`) — maskeleniyor, sınamaların klasörü artık `a ğ`;
  sshd'nin ilk durağı (`zsh -c`) `~/.zshenv`'i zaten okuduğu için ortamdaki
  `ZDOTDIR` kullanıcının başlangıç değeri değil — önyükleme onu taşımıyor,
  dans `~/.zshenv`'den başlıyor (bekçisi `.zshenv`'de `ZDOTDIR` + ihraç
  edilmemiş değişken); tek satırın `eval` kolundan dönen yük de düz giriş
  kabuğuna düşüyor (bağlantı kapanmıyor); `__bateri_percent` iki kopya
  yerine `zdotdir.zsh`'te tek; aynı içerikli dosya yeniden yazılmıyor
  (`cmp -s`); öğrenme kancası worker'ı bekletmiyor (pane'in kancası kendi
  thread'inde); yükün literalleri `upload::sq` (crate'in tek tırnak kuralı);
  iki doc-comment birleşmesi ve yorumlardaki Türkçe alıntılar düzeltildi.
  **Waive:** her bağlantıda ~30 KB'lık yük (yorumlar dahil; sürüm/özet
  damgası phase-5'in paylaşılan bağlantısıyla "önce yokla" — discussion →
  Karar 3-C — açılırsa ucuzlar) ve motd'un iki yönlü sınırı (bilinen sınır,
  `boot.sh`'in başlığında: `PrintMotd no`'lu sunucuda motd görünür, çözücü
  yoksa motd yok).

## Checklist

- [x] `bateri.zsh`'te `ssh` fonksiyonu
- [x] `ZDOTDIR` dansı paylaşılan dosyaya
- [x] `assets/shell/remote/`: önyükleme + zsh/bash/fish betikleri + motd
- [x] `ssh_wrap`: yük ve gerçek sarma
- [x] `BATERI_BIN`, öğrenme kancası (`ssh_wrap::host_key` + `record(Posix)`), etiketteki neden (durum dosyasının yolu phase-1'de geldi: `bt-shell-macos::remote_hosts_path`)
- [x] `Makefile` + `bundle_assets`
- [x] Test: PTY simülasyonu (5 kabuk), hata kolları, öğrenme
- [x] Doğrulama geçti (`make check` + `make bundle` + `make linux`)
