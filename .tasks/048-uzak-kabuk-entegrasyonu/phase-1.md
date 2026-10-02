# Phase 1 — Sarma kararı, ayar ve sızıntı savunması

## Özet

Sarmanın bütün kararını ve argv'nin gidiş-dönüşünü saf ve sınanır biçimde
kur; ayar anahtarını ve uzak oturumdaki OSC 8133 savunmasını indir. Bu phase
sonunda hiçbir bağlantı henüz sarılmıyor (fonksiyon phase-2'de) ve
kullanıcının gördüğü tek fark yok.

_Requirements: R1, R2, R4_

## Değişiklikler

- **`crates/bt-shell-common/src/ssh_wrap.rs`** (yeni) — tek sahip:
  - `wrap(argv, boot) -> argv` ve `unwrap(argv) -> argv`; önek ve eklenen
    `-t`'nin yeri **konumla** tanınır, önek koklamayla değil. Önekin metni
    burada üretilir, betikte değil.
  - Sarma kuralı: `jobs`'un yürüyüşünü (`SshSession::options`) yeniden
    kullanarak uzak komut/etkileşimsiz bayrak yok + çağıranın verdiği "tty
    mi" biti (R1.1).
  - `ssh -G` çıktısının ayrıştırılması ve kuralı: `remotecommand`,
    `requesttty no`, `sessiontype` ≠ `default` → sarma yok (R1.2); kanonik
    `(user, hostname, port)` = `host_key`. `ssh -G`'yi koşturan kısım süreç
    doğuran ince bir kabuk, ayrıştırma saf.
  - Host durum dosyası: satır biçimi (`posix`/`touched` + anahtar + Unix
    zamanı) okuma/yazma saf, dosya G/Ç'si geçici ad + `rename` + `flock`;
    bozuk satır atlanır, okunamayan dosya "hiçbiri öğrenilmedi". Yol
    çağıranın argümanı (platform kabuğu verir).
  - `decide(argv, tty, settings, state) -> Option<argv>`: R1'in bütün
    koşulları tek fonksiyonda; sarılırsa `touched` kaydı çağıranın işi.
- **`crates/bt-shell-common/src/jobs.rs`** — `ssh_target` yürüyüşten
  **önce** `ssh_wrap::unwrap` uygular; `Target.argv`, `RemoteTarget::line` ve
  `upload::ssh_argv` kullanıcının yazdığını görür (R2).
- **`crates/bt-core/src/settings.rs`** — `[remote] integration` (bool,
  varsayılan `true`) ve `HostRule`'a `integration: Option<bool>`; çözüm
  fonksiyonu "ilk eşleşen kuralın `integration`'ı → kuralın `mark`'ı
  `production` ise `false` → genel anahtar"; `Settings::for_unusable_file`
  bu anahtarı `false`'a düşürür (`osc52` emsali). Bilinmeyen anahtarı koruyan
  round-trip sınaması.
- **`crates/bt-core/src/shell.rs`** — `ShellLog::apply_dock` (ve 8133'ün
  öbür kolları: `8133;b`, `8133;w`) uzak oturum etkinken hiçbir şey yazmaz
  (R4). Yerel `D` uzak oturumu bitirdikten sonra gelen yerel 8133 bugünkü
  gibi işlenir.
- **`crates/bt-shell-macos/src/lib.rs` + `crates/bateri/src/main.rs`** —
  `bateri ssh-argv -- <argv…>` alt komutu `has_aqua_session()`'dan **önce**
  ayrılır; gövde `bt_shell_macos` üzerinden (bin'e yeni crate kenarı yok):
  ayarı ve durum dosyasını okur, `decide`'ı çağırır, sarıldıysa `touched`
  yazar, argv'yi NUL ayrık basar ya da boş çıkar. Hata her kolda "boş çıkış"
  (çağıran düz ssh'a düşer). Önyükleme yükü bu phase'de boş bir yer tutucu;
  `decide` gerçek betik yokken `None` döner (phase-2 açar).
- **`docs/AYARLAR.md`** — `[remote] integration` ve host başına
  `integration`, prod varsayılanı, okunamayan dosyada kapalı.

## Kabul

- `ssh_wrap` sınamaları: `unwrap(wrap(x)) == x` (kullanıcının kendi `-t`'si,
  `--`, değer alan bayraklar, `ssh://` biçimi dahil); R1.1'in her kolu;
  `ssh -G` örnek çıktılarıyla R1.2; durum dosyası bozuk satır / yok / kilitli.
- `jobs` sınaması: sarılmış argv'li süreçten çıkan `RemoteTarget::line`
  kullanıcının yazdığı satır.
- `bt-core` sınaması: uzak oturum etkinken 8133 aynası, dalı ve `w`
  değişmiyor; uzak oturum bitince ilk yerel 8133 işleniyor.
- `settings` sınaması: çözüm sırası ve okunamayan dosyada `false`.
- `make check` yeşil; `make linux` yeşil.

## Uygulama Notları

- **Sarılmış biçim `--`'süz** (akış çizimindeki `-- <önek>` değil):
  `["-t", <kullanıcının argümanları>, "exec sh -c '<boot>' bateri-boot"]`.
  Kullanıcı seçenekleri hedeften önce `--` ile bitirmişse OpenSSH hedeften
  sonra yeniden ayrıştırmıyor ve eklenen ikinci `--` uzak komuta girerdi;
  komut harfle başlayan **tek** argüman olduğu için iki durumda da uzak
  komut. `unwrap` konumla: ilk `-t`, son argüman `remote_command`'ın tam
  biçimi (`'` içermeyen gövde, `bateri-boot` `$0`'ı) ve aradaki çağrı uzak
  komutsuz bir etkileşimli ssh (`jobs::ssh_call`); yoksa argv aynen.
- **Etkileşim kuralı `jobs`'un yürüyüşü**: `ssh_target`'ın gövdesi
  `jobs::ssh_call`'a çıktı (host, yeniden koşu argv'si, `command` biti);
  `ssh_target` önce `unwrap`'i uygular, `decide` aynı yürüyüşü sorar ve
  `-t`'li uzak komutu da (`ssh -t host tmux`) sarmaz.
- **`decide` saf değil, dikişli**: `ssh -G`'yi 047'nin `SshRunner`'ı
  üstünden **en son** soruyor (önce önyükleme, tty biti, yürüyüş ve ayar) —
  `ssh -G` bir süreç ve config'in `Match exec`'ini koşturur, sarılmayacak
  çağrıya ödetilmemeli. Sınamalar tablo cevaplı; gerçek `ssh -G` koşulmuyor.
  Dönüş `Wrapped { args, key }`: `touched`'ı çağıran yazıyor.
- **`host_key` 047'nin hesabı (`ssh_route::Account`) ile aynı üçlü**,
  `kullanıcı@hostname:port` olarak okunur — soket adının özütü (dörtlü,
  `proxyjump` dahil) değil: dosya ileride "dokunulan host'lar" listesi
  olarak okunacak ve "aynı sunucu mu" sorusunda atlama host'u fark etmez.
  Girdisi `ssh -G`'nin metni (`host_key(out)`); phase-2'nin öğrenme tarafı
  aynı fonksiyonu çağırmalı.
- **`ssh -G`'nin yazımı**: `RequestTTY no` `requesttty false` diye basılıyor
  (ölçüldü, OpenSSH 10.2); iki yazım da okunuyor. `remotecommand` yalnız
  doluyken basılıyor.
- **Kabul edilmeyen `[remote] integration` ve bozuk `[remote]` bölümü de
  kapatır** (plan yalnız okunamayan dosyayı söylüyordu): `osc52` emsalinin
  tamamı, çünkü yanlış tahminin yönü aynı — `integration = "no"` yazım
  hatası sunucuya yazmayı açardı. `CLAUDE.md`'nin "Tek istisna `osc52`"
  cümlesi "iki istisna" oldu; `docs/AYARLAR.md` → Hata olursa da.
- **Host girdisi yalnız `integration` taşıyabilir** (`{ host = "router*",
  integration = false }`) ve **iki anahtar ayrı çözülüyor** (plan "ilk
  eşleşen kuralın `integration`'ı → o kuralın `mark`'ı" diyordu):
  `HostRule::mark` `Option` oldu; işaret `mark` taşıyan, entegrasyon
  `integration` taşıyan ilk eşleşen girdiden, prod kuralı host'un çözülmüş
  işaretinden. Tek girdi kuralında `integration`'ı kapatılan host rengini
  kaybediyordu (`/code-review` bulgusu). Menünün işareti yerinde değiştiren
  kolu böyle bir girdiye `mark` ekliyor; silen ve başa taşıyan kolları,
  host'un entegrasyonunu belirleyen girdi siliniyorsa değeri başa yazılan
  girdiye taşıyor (`MarkPlan::integration`) — phase-4'e devredilecekti,
  bulgu üzerine burada.
- **Reddedilen `[remote] hosts` listesi de entegrasyonu kapatır**: liste
  reddedilince önceki liste kalıyor ve açılışta (alt komut dosyayı her
  seferinde açılış kuralıyla okuyor) o boş — prod işaretleri ve
  `integration = false`'lar gider, entegrasyon prod'a açık düşerdi
  (`/code-review` bulgusu).
- **Alt komutun teli** (phase-2 zsh tarafını buna göre yazar):
  `bateri ssh-argv [--tty] -- <ssh argümanları…>` — program adı yok;
  `--tty` "stdin ve stdout terminal" demek (`$(…)` altında bizim stdout'umuz
  boru, soran çağıran); çıktı her argümanın ardından bir NUL, ya da hiçbir
  şey; çıkış kodu her zaman 0. `touched` yazılamazsa (kilit
  `LOCK_PATIENCE` = 500 ms'de alınamadı, disk) **hiçbir şey basılmıyor**:
  liste bateri'nin nereye yazdığının kaydı, eksik kalması güvensiz yön.
- **Durum dosyasının yolu phase-1'e geldi** (phase-2'nin maddesiydi): alt
  komut onsuz koşamıyor — `bt-shell-macos::remote_hosts_path`
  (`~/Library/Application Support/bateri/remote-hosts`). Kilit kardeş
  dosyada (`remote-hosts.lock`): yazım her seferinde yeni bir inode'u
  `rename` ettiği için dosyanın kendisi kilidi taşıyamaz; okuyan kilit
  almıyor. Linux yolu (`$XDG_STATE_HOME`) Linux kabuğuyla.
- **`make test-race` koştu**: 8133 kapısı okuyucu thread'in yolunda
  (`apply_scan_answering`); yeni paylaşılan durum yok, kapı var olan
  `context.remote`'u okuyor.

## Checklist

- [x] `ssh_wrap`: wrap/unwrap, kural, `ssh -G` ayrıştırması, `host_key`, durum dosyası
- [x] `jobs::ssh_target` unwrap'i
- [x] `settings`: anahtar, host alanı, çözüm, okunamayan dosya
- [x] `bt-core`: uzak oturumda 8133 savunması
- [x] `bateri ssh-argv` alt komutu (Aqua'dan önce)
- [x] `docs/AYARLAR.md`
- [x] Test: gidiş-dönüş, R1 kolları, `ssh -G`, durum dosyası, `jobs` satırı, 8133 savunması, ayar round-trip
- [x] Doğrulama geçti (`make check` + `make linux`)
