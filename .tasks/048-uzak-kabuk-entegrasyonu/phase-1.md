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

## Checklist

- [ ] `ssh_wrap`: wrap/unwrap, kural, `ssh -G` ayrıştırması, `host_key`, durum dosyası
- [ ] `jobs::ssh_target` unwrap'i
- [ ] `settings`: anahtar, host alanı, çözüm, okunamayan dosya
- [ ] `bt-core`: uzak oturumda 8133 savunması
- [ ] `bateri ssh-argv` alt komutu (Aqua'dan önce)
- [ ] `docs/AYARLAR.md`
- [ ] Test: gidiş-dönüş, R1 kolları, `ssh -G`, durum dosyası, `jobs` satırı, 8133 savunması, ayar round-trip
- [ ] Doğrulama geçti (`make check` + `make linux`)
