# Phase 1 — Başarı işareti, `plain` olgusu ve düşme kararı (davranış değişmez)

## Özet

Sarmanın başarısını kanıtlayan nonce'lu `up` işaretini uçtan uca kur, durum
dosyasına `plain` olgusunu ve düşme kararını veren alt komutu ekle; sarma
kuralı (`decide`) ve zsh fonksiyonu henüz değişmez.

_Requirements: R2, R2.1, R2.2, R2.3, R3.2, R3.3, R5.1_

## Değişiklikler

- **`crates/bt-shell-common/src/ssh_wrap.rs`** — `wrap` deneme başına rastgele
  bir nonce üretip önyükleme komutuna argüman olarak koyar; nonce'u sarılmış
  argv'den okuyan saf bir fonksiyon (`unwrap`'ın kardeşi; `unwrap`'ın
  imzası ve konumla çalışma kuralı değişmez, gidiş-dönüş sınaması nonce'la
  genişler). `Fact::Plain` (`plain` satırı; eski dosyalar aynen okunur).
  Saf `fell_back(args, rc, state, …) -> Option<Vec<String>>`: rc 255 → `None`;
  host `posix` → `None`; host `touched` ve `posix` değil → `Some(düz argv)`
  ve çağıran `plain` yazar. Düz argv sarmasız ama `decide`'ın verdiği aynı
  `u-<key>` paylaşım seçenekleriyle (tek üretici: `Control`).
- **`crates/bt-shell-macos/src/lib.rs`, `crates/bateri/src/main.rs`** —
  `bateri ssh-fell-back --rc N [--instance I] -- <args…>` alt komutu,
  `ssh-argv` ile aynı dalda (Aqua denetiminden önce, `bt_shell_macos`
  üzerinden; bin'e yeni kenar yok); tel biçimi `ssh-argv`'nin ki (NUL ayrımlı
  ya da boş, çıkış hep 0).
- **`assets/shell/remote/boot.sh`** — ilk eylem `printf '\033]8133;i;up;%s\007'`
  (nonce `$1`'in yanında ikinci argüman), motd'dan ve her `bt_fault`'tan önce.
  Nonce yalnız `[0-9a-f]` olmalı; değilse işaret basılmaz (048'in tırnak
  kuralı).
- **`crates/bt-core`** — tarayıcı `8133;i;up;{nonce}`'u uzak 8133
  savunmasının dar istisnası olarak tanır (`8133;f` → `RemoteSetupFault`
  emsali; biçim dışı nonce reddedilir); son görülen nonce uzak nesle bağlı
  `ShellLog`'da, `C`/`set_remote` temizler; okuyucu `Wake` ile yüksüz haber
  verir (başlık/OSC 7 emsali). Yerel oturumda `i;up` yok sayılır.
- **`crates/bt-shell-macos/src/pane.rs`** — uzak oturum hedefi çözülünce ve
  `up` haberi gelince: hedefin argv'sindeki nonce görülenle eşleşirse
  `ssh_wrap::record(Fact::Posix, key)` kendi thread'inde (anahtar `ssh -G`'den,
  `learn`'in gövdesi). Eşleşmeyen ya da sarılmamış oturumda hiçbir şey.

## Kabul

- Saf sınamalar (R5.1): nonce gidiş-dönüşü (kullanıcının kendi `-t`'si
  dahil), `fell_back`'in tablosu, `Fact::Plain`'in okunup yazılması,
  tarayıcının `i;up`'u uzak oturumda kabul edip yerelde ve biçim dışında
  reddetmesi.
- PTY simülasyonu: sarılmış önyükleme (048'in mevcut kabuk simülasyonları)
  ilk çıktısı olarak `up;{nonce}` basıyor.
- Kullanıcının gördüğü değişmiyor: `decide` hâlâ yalnız `posix` host'u
  sarıyor, fonksiyon `ssh-fell-back`'i henüz çağırmıyor.

## Uygulama Notları

- **Nonce `$2`, yoksa ebeveyn `-`**: uzak komut artık
  `exec sh -c '<tek satır>' bateri-boot <P|-> <nonce>`. Ebeveyn yokken (bozuk
  ya da eksik `--block`) yer tutucu `-` (`NO_PARENT`) koyuluyor; olmasaydı
  nonce `$1`'e kayar ve yalnız rakamdan oluşan bir nonce `boot.sh`'te
  `BATERI_RBLOCK` diye okunurdu. Nonce `/dev/urandom`'dan 16 küçük hex
  (`ssh_wrap::new_nonce`, `NONCE_LEN`; yeni crate yok); okunamazsa
  `ssh-argv` hiçbir şey basmıyor (düz `ssh`) — nonce'suz sarma `posix`'i hiç
  kazanamaz ve phase-2'nin düşmesi host'u yanlışlıkla `plain` yapardı.
  `unwrap` ad'dan sonrasını tek yerde okuyor (`boot_tail`): boş, `<P>` (048'in
  biçimleri, nonce'suz) ya da `<P|-> <tam 16 hex>`; başka her kuyruk sarılmış
  sayılmıyor. `ssh_wrap::nonce` yalnız `unwrap` argv'yi bizim saydığında cevap
  veriyor.
- **Nonce `unwrap`'tan önce alınıyor** (`jobs::Target::nonce`): `ssh_target`
  argv'yi açtığı için `Target.argv`'de (ve `bt_core::RemoteTarget`'ta) nonce
  yok; alan yalnız `jobs::Target`'ta, `RemoteTarget`'a girmedi (20 yapım yeri +
  elle yazılmış `clone_from`; çekirdeğin onu bilmesine gerek yok).
- **`bt-core`'un kapısı "uzak durum" değil "komut koşuyor"**
  (`ScanEvent::RemoteUp`, `ShellLog::remote_up`): `up` yükün ilk baytı ve
  anahtarlı girişte yoklamadan önce geliyor; `remote.is_some()`'a bağlansaydı
  düşer ve phase-2'de kabuklu host kalıcı olarak `plain` olurdu. Kayıt
  `(komut nesli, nonce)` — `login`'in emsali, `C` kendiliğinden geçersizleştiriyor;
  `set_remote` ve `D` **silmiyor** (pane'in ana kuyruk kontrolü `D`'den sonra
  koşabilir). Yerel prompt'ta (koşan komut yok) yok sayılıyor. Biçim
  `parse_dock`'un yeni `i` kolunda: `up;{1..=64 küçük hex}` (`NONCE_LIMIT`),
  başka her `i` sessiz — eski hâlde bilinmeyen `i` `Malformed` olup yerelde
  dock'u `Unavailable`'a düşürürdü. Haber `Wake::remote_up` (yeni, yüksüz,
  kenar); dört uygulayıcı güncellendi.
- **Pane iki uçtan bakıyor** (`WrapProof`, `TerminalPane::check_remote_up`):
  `up` ile yoklamanın bulduğu sarılmış ssh her sırayla gelebiliyor; ikinciyi
  gören uç karşılaştırıyor — aynı nesil, aynı nonce → `ssh_wrap::learn`
  (gövdesi `ssh -G` + `record(Posix)`) kendi thread'inde, nesil başına bir kez.
  Süreli koşuda hiçbir şey. 048'in selamdan öğrenmesi bu phase'de **duruyor**
  (phase-2 söküyor); iki yol da aynı satırı yazar, `learn` zaten biliyorsa
  yazmıyor.
- **Çözücü hatası da `up` basıyor** (planın "boot.sh'in ilk eylemi"nin
  genişletmesi): base64 çözücüsü olmayan sunucuda yük hiç koşmuyor, tek satır
  `8133;f;decode` basıp düz giriş kabuğunu açıyor — kullanıcının çalışan bir
  oturumu var. `up` yalnız `boot.sh`'te olsaydı phase-2 o host'u `exit`'ten
  sonra `plain` sayıp kullanıcıyı **yeniden bağlardı**. Tek satırın hata kolu
  `up`'u `awk` ile basıyor (`ESC` zaten oradan), nonce `awk`'ın regex'iyle
  sınanıyor (`[!…]` tek satırda yasak, `is_inline`). Sonuç: `sh -c`'mizin
  koştuğu her sunucu `posix` — csh/ash giriş kabuklu host'lar da (048'in
  `f;shell` + düz kabuk davranışı aynen, her bağlantıda).
- **`fell_back` saf, `ssh -G` çağıranın**: imza `fell_back(args, rc, config,
  state, sockets) -> Option<FellBack { args, key }>`; `config` `ssh -G`'nin
  çıktısı. Böylece alt komut `ssh -G`'yi bir kez koşturup durum dosyasını
  bekleme boyunca yeniden okuyabiliyor. Düz argv'nin paylaşım seçenekleri
  `decide`'ınkiyle aynı yoldan (`control` + `names_sharing`); sınama sarılmış
  çağrının `-o` altılısıyla bayt bayt aynı olduğunu bağlıyor.
- **Yarış (orkestratör notu): sınırlı yeniden okuma, alt komutta.** `posix`'i
  pane `up` görünce yazıyor: ana kuyruk turu + `ssh -G` + kilit. Bu zincirden
  kısa süren bir oturum (giriş dosyası hemen `exit` eden) satırı bulamaz ve
  kabuklu bir host'u kalıcı olarak `plain` yapardı. `ssh_fell_back_main`
  yalnız "touched, posix değil" cevabında durum dosyasını `FELL_BACK_PATIENCE`
  (500 ms, tasarım sabiti, ölçülmedi) boyunca 25 ms'de bir yeniden okuyor;
  `posix` gelirse hiçbir şey basmıyor. 255 ve `posix` cevapları beklemiyor.
  Bedeli: kabuksuz uç düz yeniden bağlanmadan önce bir kez 500 ms bekliyor
  (sonraki bağlantısı baştan düz); `SESSION_PERSIST` (2 sn) içinde kaldığı
  için master'a binme şansı değişmiyor. Bekçisi
  `the_fallback_subcommand_records_plain_and_prints_the_rerun` (100 ms sonra
  gelen `posix` satırı görülüyor).
- **`plain` yazılamazsa da düz argv basılıyor** (`ssh-argv`'nin `touched`
  kuralının tersi): orada liste bateri'nin nereye yazdığının kaydı, burada
  satır yalnız bir sonraki bağlantının dolambacını kısaltıyor; bağlantı
  kullanıcının.
- **PTY simülasyonu yerine ham akış**: `remote_shells`'in PTY harness'i
  ekranı ve defteri görüyor, koşan komutu olmayan oturumda `up` tutulmuyor.
  `the_bootstrap_says_up_before_anything_else` aynı `"$SHELL" -c '<komut>'`'u
  borulu stdout'la koşturup ilk baytların `up;{nonce}` olduğunu dört kolda
  (zsh entegrasyonu, düz `sh` → `f;shell`, yazma hatası, çözücü hatası)
  ve hex olmayan nonce'un basılmadığını sınıyor. macOS ve `make linux`'ta yeşil.
- **`CLAUDE.md`**: 048 paragrafındaki sarılmış çağrı biçimi ve öğrenme
  cümlesi bugünkü koda göre düzeltildi (işaret + nonce, `ssh-fell-back`'in
  henüz çağıranı yok); "her zaman sar" yeniden yazımı phase-2'nin.

- **`/code-review` (medium) iki düşük bulgu:** (1) `posix` satırı yalnız
  pane sarılmış ssh'ı kendi kabuğunun ön plan işi olarak görürse yazılıyor;
  bir bateri pane'indeki tmux/screen içinde sarılan ssh'ta (yoklama tmux'u
  görür) ya da `FELL_BACK_PATIENCE`'ı aşan yavaş bir `ssh -G` `Match exec`'inde
  phase-2'nin düşmesi kabuklu host'u `plain` yapıp kullanıcıyı `exit`'ten
  sonra yeniden bağlar — phase-1'de erişilemez (`decide` hâlâ `posix` istiyor,
  fonksiyon `ssh-fell-back`'i çağırmıyor); **phase-2'ye devredildi**
  (checklist). (2) `recorded` thread koşmadan kuruluyordu, başarısız yazım
  hiç yeniden denenmiyordu → giderildi: thread `learn`'i hata (kilit, disk)
  hâlinde `POSIX_ATTEMPTS` (3) kez deniyor, thread doğmazsa `recorded`
  sıfırlanıyor.

## Checklist

- [x] `ssh_wrap`: nonce, `Fact::Plain`, `fell_back`
- [x] `bateri ssh-fell-back` alt komutu
- [x] `boot.sh`: ilk eylem `up;{nonce}`
- [x] `bt-core`: `i;up` kabulü, uzak nesle bağlı nonce, `Wake`
- [x] pane: nonce eşleşince `posix` kaydı
- [x] Test: nonce gidiş-dönüşü, `fell_back` tablosu, tarayıcı kabul/ret
- [x] Doğrulama geçti (`make check`, `make linux`, `make bundle` — `assets/shell/*`, `make test-race` — okuyucu yolu)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi (okuyucu tarayıcısı)
