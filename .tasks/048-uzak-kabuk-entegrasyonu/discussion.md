# Uzak kabuk entegrasyonu — Tartışma

Karar-listesi biçimi. **[ürün]** işaretli noktalar kullanıcıya gider (sonucu
ekranda ya da ayar dosyasında ayrışıyor); kalanı teknik ve gerekçesiyle
öneri olarak yazıldı.

## Karar 1: kullanıcının `ssh`'ı nasıl yakalanıyor

- **A — yerel zsh'te `ssh` fonksiyonu (Ghostty emsali).** `bateri.zsh`
  kullanıcının dosyalarından **sonra** (`__bateri_end`) bir `ssh` fonksiyonu
  tanımlar; kullanıcı düz `ssh` yazar. Kullanıcının kendi `ssh` alias'ı ya da
  fonksiyonu varsa tanımlanmaz (onunki kazanır, bilinen sınır).
- **B — Enter'da satırı yeniden yazmak.** Dock `BUFFER`'ı bildiği için
  `ssh host`'u gönderirken değiştirebilirdi. Geçmişe değişmiş satır düşer,
  alias/değişken genişlemesini bilemez; reddedildi.
- **C — ayrı komut (`bateri ssh`, kitty emsali).** En çok şikayet alan nokta
  tam bu (Ghostty #5892); reddedildi. Fonksiyonun içi zaten bu komutu
  çağırabilir (Karar 2), yani isteyen betikten de kullanabilir.

Öneri: **A**. Fonksiyonun ulaşmadığı yerler (betik, alt kabuk, `exec ssh`,
kullanıcının alias'ı, yerel bash/fish) bugünkü sonradan algılamaya düşer —
yani hiçbir şey bugünden kötüye gitmez (Ghostty #9708'in yedeği bizde zaten
var).

## Karar 2: kararın ve argv'nin mantığı nerede

- **A — zsh'te.** Fonksiyon argv'yi kendisi ayrıştırır, host desenlerini
  `[[ $host == $~pat ]]` ile eşler. İkinci bir ssh ayrıştırıcısı ve ikinci bir
  glob eşleyici doğar (`jobs::ssh_target`, `settings::host_mark` ile
  ayrışır); ayar değişince açık kabuklar eski kararı taşır.
- **B — `bateri` ikilisinde bir alt komut.** Fonksiyon
  `"$BATERI_BIN" ssh-argv -- "$@"` çağırır; Rust tarafı aynı ayrıştırıcıyla
  (`jobs`'un yürüyüşü) "sarılır mı" kararını verir, `settings.toml`'u o an
  okur, host'u eşler ve sarılmış argv'yi döner; boş ya da hata → fonksiyon
  `command ssh "$@"`. Mantık `bt-shell-common`'da saf ve sınanabilir; yerel
  bash/fish sarmalayıcıları geldiğinde onlar da tek satır.

Öneri: **B** (tek ayrıştırıcı, canlı ayar). Gereği: `crates/bateri/src/main.rs`
Aqua denetiminden **önce** alt komutu ayırmalı; ikilinin yolu sarmalayıcıya
ortamla gider (`BATERI_BIN`, `shell_integration_env`'in yanında). Bedeli her
`ssh`'ta bir süreç doğuşu; ölçülmedi, iddia yazılmıyor.

Sarma kuralı `ssh_target`'ınkiyle aynı: uzak komut yoksa, `-N -W -O -Q -G -V
-T` yoksa ve stdin/stdout tty ise. `ssh host uptime`, `ssh -t host tmux`,
pipe'lı ssh, `scp`/`rsync`/`git` dokunulmadan geçer.

## Karar 3: betik uzağa nasıl gidiyor

- **A — satır içi uzak komut.** `ssh -t … host -- exec sh -c '<önyükleme>'`;
  önyükleme base64'lü yükü açar. Tırnak tek ve ters bölüsüz, çünkü uzak giriş
  kabuğu fish ya da csh olabilir (037'nin yükleme kuralı). Tek gidiş-dönüş,
  tty'ye dokunmaz, yani kitty #7355'in multiplexer takılması doğmaz.
- **B — tty üzerinden aktarım (kitty).** Yük argv'de görünmez, ama bir
  kaçış dizisi protokolü, tek kullanımlık parola ve multiplexer sorunları
  gelir. Reddedildi (bugün gerek yok: yük sınırın yarısında).
- **C — önce yoklama, sonra yalnız eksikse gönder.** Paylaşılan bir master
  bağlantı (047) olunca ucuzlar; ilk sürümde değil, not.

Öneri: **A**. Yükün içeriği (Karar 5) küçük tutulursa `ps` görünürlüğü de
kozmetik kalır.

## Karar 4: uzak işaretlerin kaynağı — mimarinin özü

Bugünkü kapı (context → Kanıt 1) iki kaynak tanıyor: bizim yerel kabuğumuz
(`bt_block=`) ve yabancı (kimliksiz). Üçüncü bir kaynak gerekiyor: **bizim
uzak kabuğumuz**.

- Uzak betik kendi alanını basar (`bt_remote=N`; `bt_block=` değil). Yerel
  `ShellLog`'un kapısı aynen kalır: uzak işaret ne uzak oturumu bitirir ne
  yerel safhayı (`Running`, dock'un caret'i, saat) oynatır.
- Uzak işaretler **ayrı bir defter** besler: uzak komutun satırı, rengi ve
  süresi — çizimde ızgaranın blok şeridi ve sayacı. Uzak oturum
  bittiğinde (bizim `D`'miz) defter sıfırlanır.
- Blok kimliği prompt'un OSC 8 çıpasından okunuyor (`bateri://block/N`);
  uzak sayaç yerel sayaçla çakışır, yani uzak çıpanın **ayrı ad alanı**
  olmalı (`bateri://rblock/…` ya da uzak oturum nesliyle önekli kimlik) —
  `block_id`'nin "yabancı `aid` kimlik değildir" kuralının emsali.
- Uzak giriş satırı **ızgarada kalır** (bastırma ve dock caret'i uzakta
  zaten kapalı, 036 Karar 8); uzak dock ayrı bir set.

Teknik karar; phase-2'nin konusu. Phase-1 (yalnız OSC 7) `bt-core`'a hiç
dokunmuyor, çünkü uzak OSC 7 bugün de doğru yuvaya gidiyor.

## Karar 5: uzakta ne koşuyor, nereye yazıyor

- **Yer**: `${XDG_DATA_HOME:-$HOME/.local/share}/bateri/shell/<sürüm>/` —
  `/tmp` değil, kayıt yok (CVE-2025-22275). Her bağlantıda geçici ad +
  `mv` ile yeniden yazılır; sürüm dizin adında, yani eski sürümün dosyası
  yenisini bozmaz. Yazılamazsa (disk dolu, salt okunur ev) önyükleme
  **sessizce** giriş kabuğunu entegrasyonsuz exec eder — kullanıcı düz ssh'ı
  görür.
- **rc dosyalarına yazılmaz**, okunur: zsh'te `ZDOTDIR` takası (yerel
  sarmalayıcının aynı dört dosyası ve `__bateri_begin`/`__bateri_end`
  dansı), bash'te `--rcfile` + giriş dosyalarının sırayla okunması, fish'te
  `XDG_DATA_DIRS` → `vendor_conf.d`. Proje kuralı üç kabuk; uzakta üçü de.
- **Ne basıyor**: OSC 7 (phase-1) ve OSC 133 `bt_remote=` ile (phase-2).
  zsh için iki yol: (a) `bateri.zsh`'i bir "uzak" kipiyle yeniden kullanmak
  (tek betik, uzak dock ileride bir bayrak), (b) ayrı küçük uzak betikler.
  Öneri (a) zsh'te — iki sarmalayıcı ayrıştığı gün biri sessizce bozulur;
  bash/fish için küçük betik (yerelde de onların sarmalayıcısı yok).
- Giriş kabuğu sshd'nin `$SHELL`'inden; zsh/bash/fish dışında (sh, csh,
  BusyBox) entegrasyonsuz exec.
- **Bilinen sınırlar**: `su -`/`sudo -i` ve uzak tmux içindeki kabuklar
  entegrasyonsuz (yerelde de öyle); mosh sarılmaz (bugünkü algılamada
  kalır).

## Karar 6: argv'nin geri kalanı — sızıntı ve dosya işleri

- Önyükleme uzak komutu tanınır bir önekle başlar; `jobs::ssh_target` onu
  görünce eklediğimiz `-t`'yi ve komutu **düşürür**, yani `RemoteTarget::line`
  kullanıcının yazdığı `ssh host` olur. `⏎ reconnect` ve ⌘T o satırı
  kabuğa yazar → fonksiyondan yine geçer → yine entegre.
- `upload::ssh_argv` uzak komutu zaten düşürüyor; değişiklik yok.
- Teknik karar.

## Karar 7 [ürün]: açma/kapama ve varsayılan

- Anahtar: `[remote] integration = true | false`; host başına
  `[remote] hosts`'un girdisine `integration = false` (ilk eşleşen kazanır,
  `mark` ile aynı kural). Ayar penceresinin Remote Files sayfasında bir onay
  kutusu; Shell menüsünde "Mark “{host}” as ▸"ın yanında host başına aç/kapa.
- **Varsayılan** — kullanıcının kararı:
  - *Açık*: her şey kendiliğinden; bedeli dokunulan her sunucuda (paylaşılan
    `root`/`deploy` hesapları dahil) bir `~/.local/share/bateri` dizini.
  - *Kapalı* (Ghostty'nin seçimi): iz yok, ama özelliği kimse bulmaz.
  - *Öneri*: **açık, `production` işaretli host'ta kapalı** — işaret,
    kullanıcının o host için verdiği "dikkat" sinyali.
- `[shell] integration = "off"` → sarmalayıcı yok, fonksiyon yok;
  `"blocks"` → var (uzak entegrasyon dock'a bağlı değil).

## Karar 8 [ürün]: görünen farklar

- Uzak komutlu oturumda sshd `Last login:` satırını basmayabilir (motd
  PAM'den gelir ve kalır). **Doğrulanacak**; basmıyorsa ya kabul edilir ya da
  önyükleme `lastlog`/`last` ile taklit eder. Kullanıcıya sorulacak: bu satır
  kaybolsa fark eder mi?
- Uzak sunucuda bırakılan dizini silmek için bir yol ("Remove bateri files
  from this host") ilk sürümde gerekli mi?

## Karar 9: 047 ile kesişim — kullanıcının oturumu master olsun mu

Fonksiyon argv'yi zaten kurduğu için `-o ControlMaster=auto -o
ControlPath=<047'nin soket dizini>/%C -o ControlPersist=…` ekleyebilir:
kullanıcının parolayı terminalde bir kez yazdığı oturum master olur ve 047'nin
dosya işleri pencere açmadan onun üstünden geçer. Kullanıcı `~/.ssh/config`'te
kendi `ControlMaster`'ını tanımlamışsa eklenmez (`ssh -G` ile okunur; komut
satırının `-o`'su config'i ezerdi). Öneri: bu setin **son** phase'i, 047'nin
soket düzeni oturduktan sonra.

## Kaba phase taslağı

_İlk taslak; Muhakeme onu değiştirdi, geçerli bölme `plan.md`'de._

1. **Uzakta OSC 7.** `bateri ssh-argv` alt komutu + `bateri.zsh`'in `ssh`
   fonksiyonu + uzak önyükleme (üç kabuk, yalnız OSC 7) + `jobs`'un
   önyüklemeyi ayıklaması. `bt-core` değişmez.
2. **Uzakta bloklar.** `bt_remote=` işaretleri, ayrı uzak defter ve çıpa ad
   alanı, şerit ve süre sayacı (`bt-core` + betikler).
3. **Ayar.** `[remote] integration` + host başına alan, ayar penceresi,
   Shell menüsü, `docs/AYARLAR.md`.
4. **Paylaşılan bağlantı.** Karar 9 (047'den sonra).

Uzak dock (OSC 8133 uzakta) **ayrı bir set**: ayna, düzenleme widget'ı ve
tazelik kapısı uzak gecikmeyle yeniden düşünülmeli.

## Muhakeme (2026-10-02)

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU |
| Codebase-fit | SORUNLU |
| İşletme | SORUNLU |

Yön (yerel `ssh` fonksiyonu, satır içi önyükleme, `jobs`'ta ayıklama, rc
dosyasına yazmamak, uzak işaretin kendi alanı) üç mercekte de doğru bulundu.
Ortak bulgu: phase-1 hem fazla yük taşıyor (bütün `bateri.zsh`) hem eksik
koruma (kapatma düğmesi ve config okuması phase-3'te).

**Kabul edilen itirazlar → plan değişikliği:**
- "Hiçbir şey bugünden kötüye gitmez" iddiası tutmuyor: `~/.ssh/config`'te
  `RemoteCommand` olan host komut satırındaki ikinci komutla hiç açılmaz;
  kabuksuz hesaplar (Windows OpenSSH, ağ cihazları, `ForceCommand`/`command=`)
  `exec sh -c`'yi çalıştıramaz → `ssh -G` okuması (`RemoteCommand`,
  `RequestTTY`, `SessionType`, `ControlMaster`) phase-1'e; `RemoteCommand`
  ya da tty istemeyen config'te sarma yok. Kabuksuz uç için Karar 7'ye ürün
  sorusu eklendi ("ilk bağlantıda öğren"). Bedel: `ssh -G` config'teki
  `Match exec`'i koşturur — bilinen sınır.
- Bu yüzden `bateri ssh-argv` alt komutu phase-1'de **kalıyor** (sadeliğin
  ertele önerisi reddedildi, aşağıda): `ssh -G`, canlı ayar ve önekin tek
  üreticisi Rust'ta; `wrap`/`unwrap` tek modülde (`bt-shell-common::ssh_wrap`)
  ve gidiş-dönüş sınamasıyla (`unwrap(wrap(x)) == x`, kullanıcının kendi
  `-t`'si dahil). `unwrap` önek koklamasıyla değil **konumla**, `ssh_target`'ın
  yürüyüşünden önce.
- Kapatma düğmesi sunucuya yazan özellikten sonra geliyordu → genel
  `[remote] integration` ve host başına kapama **phase-1**'e; okunamayan
  `settings.toml`'da kapalı (`osc52` emsali: uzağa yazmak, yanlış tahminin
  güvensiz yönü). Menü ve ayar penceresi phase-3'te kalır.
- Phase-1'e bütün `bateri.zsh` (40 KB, çoğu dock aynası) gereksiz ve
  unutulan bir dal yerele sızar (`8133;b`, `8133;w` → yerel dock uzak kabuğa
  `CSI 8133 ~` yollar) → phase-1 kabuk başına **küçük** uzak betik (zsh:
  `ZDOTDIR` dansı + OSC 7; bash: `--rcfile`, giriş dosyası sırası kitty'nin
  yöntemiyle; fish: kendi OSC 7'si varsa hiçbir şey — doğrulanacak, yoksa
  `vendor_conf.d` + `XDG_DATA_DIRS`'in geri yazılması). `ZDOTDIR` dansı yerel
  ve uzak betiğin paylaştığı ayrı bir dosyaya çıkar. Karar 5(a) reddedildi.
- Sızıntı savunması betiğin dikkatine bırakılmaz → `bt-core`'da: uzak oturum
  etkinken OSC 8133 ve `bt_block=` yok sayılır. "Phase-1 `bt-core`'a
  dokunmuyor" cümlesi düştü; savunma phase-1'in parçası.
- Uzak defterin "uzak oturum bitince sıfırla" + düz sayaç kuralı eski
  oturumun satırlarını yeni oturumun çıkış koduyla boyar (`BlockLog::start`
  aralıktaki kimliği yeniden açar, `shell.rs:1079`) → uzak kimlik yerel ssh
  komutunun kendi blok kimliğiyle önekli (`bt_remote=<P>.<n>`,
  `bateri://rblock/<P>.<n>`; `P` = `$__bateri_block`). Defter `context.remote`'a
  bağlanmaz (uzak `A` yoklamadan önce gelebilir); `parent` değişince temizlenir
  — eski satır "yanlış çizilmez", "çizilmez".
- Uzak kol `ShellLog::apply`'de `identified` hesabından **önce** ayrılır;
  yoksa `outcome.prompt` ⌘T'nin `initial_input`'unu uzak kabuğa yazar
  (`session.rs:1584`). Yerel faz + saat + halka üçlüsü bir tipe çıkar
  (`BlockTrack`), yerelde ve uzakta iki örnek. Uzak kipte `ssh` fonksiyonu
  tanımlanmaz (iç içe ssh — bilinen sınır).
- Uzak dizin: sürüm dizini gereksiz (her bağlantıda geçici ad + `mv`, kabuk
  dosyayı açılışta okur) → sabit `~/.local/share/bateri/shell/`; birikme yok.
- POSIX dışı araçlar: `base64 -d` yedek zinciri (`base64 -D`, `b64decode`,
  `openssl base64 -d`), `mktemp`'siz geçici ad; `exec "$SHELL" -l` ve `$0`
  farkı adıyla yazılır.
- Sessiz düşüşün nedeni pane'in etiketinde (bugünkü etiket yuvası; bariz
  beklenti, soru değil).
- Paket: `Makefile`'ın kopya ve `cmp` listeleri ile `bundle_assets` yeni
  betikleri kapsar. Sınama: sshd'nin yaptığı `"$SHELL" -c '<komut>'` yerelde
  `jobs`'un gerçek PTY'siyle, giriş kabuğu zsh/bash/fish/dash/busybox, geçici
  ev dizini, `make linux` imajında; `RemoteCommand`, motd ve `ForceCommand`
  için aynı imajda localhost `sshd`.

**Reddedilenler:**
- `ssh-argv`'yi phase-3'e ertelemek, zsh `getopts` ile sarmak (sadelik) —
  `RemoteCommand`/kabuksuz uç kontrolü `ssh -G` ister ve önek ile ayıklamanın
  aynı ikiliden çıkması gerekiyor; kapama düğmesi de phase-1'de.
- Uzakta `bateri.zsh`'i "uzak kip"le yeniden kullanmak (codebase-fit) —
  phase-1'de fazla yük ve sızıntı yüzeyi; ayrışma korkusunun konusu olan
  `ZDOTDIR` dansı paylaşılan dosyayla karşılanıyor. Uzak dock setinde yeniden
  açılır.
- Sürümlü uzak dizin + süpürme (işletme) — sabit yol aynı korumayı birikmesiz
  veriyor.

## Karar (2026-10-02, kullanıcı onayı)

Kullanıcı önerilerin tamamını onayladı ("önerilerle devam").

**Ürün kararları (kullanıcı):**
- **Varsayılan açık**, `production` işaretli host'ta kapalı (Karar 7). Genel
  `[remote] integration = false` ve host başına `integration = false` kapatır;
  host başına `integration = true` prod işaretini ezer.
- **Kabuksuz uçlar: ilk bağlantıda öğren** (soru 2-a). Bir host'a ilk
  bağlantı düz açılır; bateri o host'ta POSIX kabuğu olduğunu öğrendikten
  sonra sonraki bağlantılar sarılır. Router, Windows ya da `git@github.com`
  hiç öğrenilmez, yani hiç sarılmaz ve kırık bağlantı görülmez. Bedeli: her
  host'ta ilk bağlantı entegrasyonsuz.
- **motd'u önyükleme basar**, `Last login:` satırının kaybı kabul (soru 3-b).
- **Uzak blok şeritleri ssh bitince geçmişte kalır**, yerel bloklar gibi
  (soru 4).
- **"Remove bateri files" düğmesi yok** (soru 5); bateri dokunduğu host'ların
  yerel listesini tutar, düğme sonra.

**Teknik kararlar (ajan):**
- **Öğrenmenin kaynağı yardımcı oturumun selamı.** Uzak pane'in yardımcı
  oturumu (`remote_helper`, 045 Karar 10) sunucuda `sh` ile koşuyor ve
  `parse_greeting` `BT-HOME` satırını aldığında sunucuda çalışan bir POSIX
  `sh` olduğu kanıtlanmış olur; yük göstergesi (046) bu oturumu uzak oturum
  başlar başlamaz açtığı için ilk bağlantının içinde, kullanıcı bir şey
  yapmadan gerçekleşir. Yeni bir yoklama doğmaz. `BT-NOPROC` öğrenmeyi
  engellemez (macOS/BSD sunucuda da `sh` var). Yardımcı açılamıyorsa
  (parolalı host, 047'den önce) öğrenme yok ve host sarılmaz — yanlışın yönü
  güvenli; 047 inince Keychain'li host'lar da öğrenilir.
- **Öğrenilen kalıcı**, bellekte değil: bellekte tutulsaydı bateri her
  açıldığında her host'un ilk bağlantısı yeniden entegrasyonsuz olurdu.
  Kayıt yeri bateri'nin kendi durum dosyası (`settings.toml` değil — o
  kullanıcının dosyası ve menüden yazılan anahtarlardan ibaret): macOS'ta
  `~/Library/Application Support/bateri/remote-hosts`, Linux'ta
  `$XDG_STATE_HOME/bateri/remote-hosts`. Satır biçimi sekmeyle ayrık
  (`posix`/`touched`, host anahtarı, Unix zamanı); yazım geçici ad + `rename`,
  eşzamanlı iki `ssh` için `flock`. Aynı dosya "dokunulan host'lar" listesini
  de taşır (sarılmış bağlantı başına `touched`). Bozuk satır atlanır, dosya
  okunamazsa hiçbir host öğrenilmemiş sayılır (sarılmaz).
- **Host anahtarı `ssh -G`'nin kanonik `(user, hostname, port)` üçlüsü**,
  yazıldığı hâli değil: `ssh web` ile `ssh deploy@10.0.0.5` aynı makineyse
  bir kez öğrenilir. Sarma kararında `ssh-argv` `ssh -G`'yi zaten koşuyor;
  öğrenme tarafı da aynı fonksiyonla (`ssh_wrap::host_key`) ve yalnız host
  henüz öğrenilmemişken koşar. Prod işareti ise bugünkü gibi yazılan host'a
  eşlenir (`settings::host_mark`), ikisi ayrı sorular.
- **Sızıntı savunması phase-1'de yalnız OSC 8133 için**: uzak oturum
  etkinken 8133 yok sayılır (`ShellLog::apply_dock`'un kapısı). `bt_block=`'in
  uzaktan taklidine karşı bir oturum anahtarı **eklenmedi**: uzak betiklerimiz
  `bt_block=` basmıyor (yalnız `bt_remote=`, `plan.md` → phase-3), yabancı betikler onu
  bilmiyor; anahtar yerel işaretin biçimini değiştirirdi. Bilinen sınır,
  phase-3'ün Uygulama Notları'nda yeniden bakılır.

### Seçilen ve reddedilen

- **Seçilen:** yerel zsh'te `ssh` fonksiyonu (1-A), karar ve argv `bateri
  ssh-argv` alt komutunda `ssh -G` okumasıyla (2-B), satır içi önyükleme
  (3-A), ilk teslimde kabuk başına küçük uzak betik + `bt-core`'da sızıntı
  savunması + genel ve host başına kapama, uzak blok kimliği yerel ssh
  bloğuyla önekli ayrı izde (4), sabit `~/.local/share/bateri/shell/` ve
  etiketli düşüş (5), konumla `unwrap` (6), paylaşılan bağlantı son phase (9).
- **Reddedilen:** satır yeniden yazma (1-B), ayrı komut (1-C), zsh'te ikinci
  ayrıştırıcı (2-A), tty aktarımı (3-B), uzakta bütün `bateri.zsh` (5-a,
  Muhakeme), sürümlü uzak dizin, kullanıcının rc dosyasına satır eklemek
  (proje kuralı).

Phase bölmesi `plan.md`'de.

"İlk bağlantıda öğren" kararı 049'da geri alındı → `.tasks/049-uzak-entegrasyon-ilk-baglanti/`.
