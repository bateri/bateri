# ssh dosya işleri parolalı sunucuda — Tartışma

Karar-listesi biçimi. Her kararın başında kimin kararı olduğu yazılı:
**[ürün]** kullanıcının ekranda ayırt edeceği seçim, **[teknik]** ajanın.

## Karar 1 [teknik]: Parolayı ssh'a nasıl veriyoruz?

- **A — askpass.** ssh'a `SSH_ASKPASS={bateri'nin kendi binary'si}` +
  `SSH_ASKPASS_REQUIRE=force` veriliyor; ssh her soruda yardımcıyı istem
  metniyle çağırıyor, yardımcının stdout'u cevap. OpenSSH'ın resmi yolu; istem
  metni (`tdgunes@host's password:`, `Enter passphrase for key …`,
  `Verification code:`, host anahtarı sorusu) olduğu gibi geliyor.
- **B — ssh'ı kendi PTY'mizde koşturup istemi okumak (expect).** Yardımcı
  binary yok, ama istem metnini ekrandan ayıklamak kırılgan (banner, 2FA,
  yerelleştirilmiş PAM metinleri) ve parolayı bir terminale "yazmak" yankı ve
  zamanlama sorunu doğuruyor.

**Öneri: A.** Yardımcı ayrı bir binary değil, **aynı** `bateri` binary'si:
`main`'in ilk satırında `BATERI_ASKPASS` ortam değişkeni varsa askpass kipine
geçip Aqua kontrolüne ve AppKit'e hiç uğramadan dönüyor (paket içinde de,
`cargo run`'da da aynı yol; `current_exe`). Kipin gövdesi platformsuz
(`bt-shell-common`, `std::os::unix::net`).

## Karar 2 [teknik]: Yardımcı soruyu uygulamaya nasıl taşıyor?

Yardımcı kendi penceresini açsaydı (ikinci bir `NSApplication`) soru hangi
sekmeden geldiğini bilmez, Dock'ta ikinci bir simge belirir ve Keychain'e iki
farklı süreç erişirdi. **Öneri:** uygulama açılışta kullanıcıya özel bir
dizinde (`0700`) unix soket dinliyor; her master açılışı **tek kullanımlık
rastgele bir jeton** üretip ssh'ın ortamına koyuyor (`BATERI_ASKPASS=jeton`,
soket yolu ayrı değişken). Yardımcı istemi ve jetonu sokete yazıp cevabı
bekliyor. Jeton isteği **o** denemeye ve onu başlatan pane'e bağlıyor
(sayfa hangi pencereye açılacak, Keychain'de hangi host); bilinmeyen jeton
cevapsız kapanıyor ve yardımcı başarısızlıkla çıkıyor (ssh o denemeyi
bırakır). kitty de veriyi tek kullanımlık parolayla istiyor; aynı ilke. Soket
yolu unix soket sınırına (104 bayt) sığmalı: `~/Library/Caches/bateri/` altı
sığıyor, `$TMPDIR` (`/var/folders/…`) sınırda.

## Karar 3 [teknik]: Kimin master bağlantısı, kaç tane, ne kadar yaşar?

- **Host başına bir master, bateri'nin kendi soketiyle.** Açılış: ssh
  `-M -N -f -o ControlPath={dizin}/%C -o ControlPersist={süre}` +
  `ssh_argv`'nin bağlantı seçenekleri, `BatchMode=no`, askpass ortamı, stdio
  boş. `%C` ssh'ın kendi bağlantı özeti (yerel host, uzak host, port,
  kullanıcı, jump) — hangi argv'nin aynı bağlantı olduğuna ssh karar veriyor,
  biz ikinci bir eşleme yazmıyoruz. `-f` kimlik doğrulama bittikten sonra
  arka plana geçiyor, yani açılış süreci tam olarak "giriş başarılı mı"nın
  cevabı ve borusunu tutan kimse yok — `ControlMaster=no`'nun gerekçesi
  (boru ucu) burada geçerli değil, çünkü master akışın süreci değil.
- **Akışlar değişmiyor:** `BatchMode=yes` ve `ControlMaster=no` kalıyor,
  önlerine yalnız `-o ControlPath={bizim soket}` ekleniyor. Akış süreci hiçbir
  zaman soru sormuyor ve asılı kalmıyor; soru yalnız master açılışında.
- **Sıra:** (1) bizim master'ımız canlıysa (`ssh -O check`) onun üstünden;
  (2) değilse bugünkü argv (anahtar, agent ya da kullanıcının kendi master'ı
  bugünkü gibi çalışmayı sürdürüyor); (3) o kimlik doğrulamadan düşerse
  (255 + `Permission denied`, `upload::probe_failure`'ın bugün metne çevirdiği kol) bizim
  master'ı aç, başarılıysa işi bir kez yeniden dene.
- **Ömür:** `ControlPersist` bir **tasarım sabiti** (öneri 10 dk boşta), çünkü
  bateri çökerse master yetim kalır ve sınırsız yaşamamalı; bateri kapanırken
  açtığı master'lara `ssh -O exit` (yerel soket, anında; kapanışın son tarihi
  içinde). Keychain'de parola varsa boşta kapanan master bir sonraki işte
  sorusuz yeniden açılıyor, yani kısa ömrün kullanıcıya bedeli yok.
- **mosh:** ssh argv'si yok, yalnız host (bugünkü kol); aynı master yolu.

## Karar 4 [ürün]: Parola penceresi neye benziyor?

**Öneri:** isteği başlatan pane'in penceresine bağlı bir sayfa (sheet):

> **Sign in to tdgunes@192.168.0.218**
> bateri needs your password to upload `backup.tar.gz`.
> [ parola alanı ]
> ☑ Remember in Keychain
> [Cancel] [Sign In]

- Başlık host'u, alt metin **neden** sorulduğunu söylüyor (hangi iş).
- ssh'ın istem metni olduğu gibi küçük puntoyla altta (PAM metinleri ve 2FA
  için tek doğru kaynak).
- **Remember in Keychain varsayılan işaretli mi?** → kullanıcı sorusu
  (aşağıda). Öneri: işaretli; kullanıcı Keychain'i istedi ve işaretsiz
  varsayılanda her boşta kapanmada yeniden sorulur.
- Yanlış parolada ssh yeniden soruyor (sunucunun deneme sayısı kadar); sayfa
  "Wrong password — try again" ile yeniden açılıyor, Keychain'e yazılmıyor.
- **Cancel** akışı "Cancelled" sonuç satırıyla bitiriyor (037'nin dili).

## Karar 5 [ürün]: Arka plan işleri parola sorabilir mi?

Yük göstergesi kendiliğinden, ⌘-hover varlık kontrolü fareyle tetikleniyor;
ikisi de kullanıcı bir şey istemeden sayfa açsaydı kullanıcı ssh'a bağlanır
bağlanmaz bir parola penceresiyle karşılaşırdı (ve 046'nın "fail2ban" uyarısı
geçerli: arka plan tekrar denememeli).

**Öneri:** arka plan işleri **hiç sayfa açmıyor**. Keychain'de parola varsa
master'ı sorusuz açıyorlar (yardımcı cevabı Keychain'den veriyor, pencere
yok); yoksa bugünkü gibi o nesilde susuyorlar ve pane'in etiketi ne
yapılacağını söylüyor: `Sign in to use remote files — ⌘-click a file or drop
one`. Kullanıcı bir kez bir dosya işiyle giriş yapınca master açılıyor ve
gösterge ile ⌘-hover o andan itibaren onun üstünden çalışıyor (yeni nesil
beklemeden yeniden denemeleri gerekiyor — teknik ayrıntı, phase'te).
Alternatif: etiketin kendisi tıklanabilir bir "Sign In…" düğmesi — 037'nin
"düğme simge değil fiil" kuralıyla uyumlu; kullanıcı sorusu.

## Karar 6 [ürün + teknik]: Keychain'de ne, nasıl duruyor?

- **Tür [teknik]:** `kSecClassInternetPassword`, `server` = host, `account` =
  kullanıcı, `port`, `protocol` = SSH. Bu tür Keychain Access'te host adıyla
  ve "internet password" olarak görünüyor; ssh'ın kendi `UseKeychain`'i de
  anahtar parolası için aynı Keychain'i kullanıyor.
- **Görünüş [ürün]:** öğenin etiketi `bateri — tdgunes@192.168.0.218:2222`.
  Kullanıcı Keychain Access'te arayınca "bateri" ile bulmalı.
- **Unutma [ürün]:** Shell menüsünde uzak sekmedeyken `Forget Password for
  “{host}”` (Mark “{host}” as ▸'nin yanında) ve ayar penceresinin Remote Files
  sayfasında kayıtlı host listesi + Remove? Öneri: yalnız menü öğesi
  (küçük kapsam); liste bir sonraki tur.
- **Bayat parola [teknik]:** Keychain'den verilen parola reddedilirse ssh
  ikinci kez soruyor; aynı denemedeki ikinci soru Keychain'e **gitmiyor**,
  sayfa "The saved password didn't work" diyerek açılıyor ve başarılı yeni
  parola öğenin üstüne yazılıyor. Arka plan işinde (Karar 5) ikinci soru
  boş cevapla bitiyor.
- **Bilinen sınır:** ad-hoc imzalı geliştirme derlemesinde imza her derlemede
  değiştiği için macOS her seferinde "bateri wants to use your confidential
  information" soruyor; Developer ID ile imzalı sürümde yalnız ilk seferde.

## Karar 7 [ürün]: Anahtar parolası, 2FA ve host anahtarı sorusu

- **Anahtar parolası** (`Enter passphrase for key '…'`): aynı sayfa, ama
  Remember kutusu yok — Apple'ın ssh'ı bunu `UseKeychain yes` ile zaten
  kendisi saklıyor; ikinci bir kopya tutmuyoruz. Öneri: kutu yerine tek satır
  ipucu.
- **2FA / keyboard-interactive** (`Verification code:`): aynı sayfa, istem
  metniyle, **kaydetme yok** (tek kullanımlık).
- **Host anahtarı sorusu** (`Are you sure you want to continue connecting`):
  kullanıcı o host'a terminalde zaten bağlandığı için `known_hosts`'ta
  olması beklenir; değilse (mosh, farklı takma ad) — **öneri: güvenli yön**,
  sayfa açılmıyor, cevap `no`, hata satırı "bateri doesn't know this server's
  key yet — connect once in the terminal". Parmak izini gösterip Trust
  düğmesi sunmak ayrı bir güvenlik kararı; bu sette yok. Kullanıcı sorusu.
- İstem sınıflaması saf bir fonksiyon (istem metni → parola / anahtar parolası
  / onay / diğer), sahte istemlerle sınanır.

## Karar 8 [teknik]: Yeni bağımlılık — Keychain erişimi

- **A — `objc2-security` 0.3.2**, `default-features = false`, yalnız `std` +
  `SecItem`. objc2 ailesinin aynı nesli (0.3.2); tek yeni kenarı grafta zaten
  olan `objc2-core-foundation`. Lisans `Zlib OR Apache-2.0 OR MIT` (MIT
  seçeneği, `THIRD-PARTY-LICENSES` yeniden üretilir). Yalnız `bt-shell-macos`.
- **B — `security-framework`** (kornelski): olgun ama ikinci bir CF sarmalayıcı
  yığını; `CLAUDE.md` → "servo ailesi `core-text` … reddedildi" ile aynı
  gerekçeyle reddedilir.
- **C — elle `extern "C"` + `#[link(name = "Security")]`**: yeni crate yok ama
  bağlayıcı imzaları elle yazmak, `objc2-*`'nin üretilmiş ve sınanmış
  imzalarına karşı bir geri adım.
- **D — `/usr/bin/security` CLI**: parola argv'de (`-w`) `ps`'te görünür;
  reddedilir.

**Öneri: A.** Yeni crate bağımlılığı mimari karardır → kullanıcı onayı.

## Karar 9 [teknik]: 048 ile kesişim

048 kullanıcının `ssh`'ını sardığında onun etkileşimli oturumunu
`ControlMaster auto` + bizim `ControlPath`'imizle başlatabilir; o zaman
kullanıcı parolayı terminalde bir kez yazar, master **o** oturum olur ve bu
setin sayfası hiç açılmaz. Bu yüzden:

- Soket dizini ve `%C` adlandırması bu sette tek yerde tanımlanıyor
  (`bt-shell-common`), 048 onu kullanıyor — iki set aynı host için iki ayrı
  master açmasın.
- Karar 3'ün sırası (1)'de "bizim soketimizde canlı master" kimin açtığına
  bakmıyor; 048'in master'ı da bunu sağlar.
- 048'in master'ı kullanıcının oturumu olduğu için oturum kapanınca
  (`ControlPersist` yoksa) gider; o zaman bu setin yolu devralıyor. Bu
  davranışın ayrıntısı 048'in kararı.

## Muhakeme (2026-10-02)

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU |
| Codebase-fit | SORUNLU |
| İşletme | SORUNLU |

Yön (askpass, aynı binary, `%C`-benzeri host başına master, `objc2-security`)
üç mercekte de doğru bulundu; itirazlar yolun iç düzeni üstüne. Bağımlılık
doğrulandı: `objc2-security 0.3.2`'nin zorunlu tek bağımlılığı grafta zaten
olan `objc2-core-foundation`.

**Kabul edilen itirazlar → plan değişikliği:**
- Hatadan sonra "master aç, bir kez yeniden dene" altı tüketiciye dağılıyor
  (`uploader.rs:147,308`, `preview.rs:285`, `promise.rs:462`,
  `hyperlink.rs:670`, `stats.rs:297`) → karar iş başlamadan **önce**, tek
  kapıda: `bt-shell-common::ssh_route::ensure(target, ask) -> Route`
  (`Ours(soket)` / `Direct`). Sıra: bizim master canlı (`-O check`) → `Ours`;
  kullanıcının kendi master'ı canlı (`ssh -G`'nin `ControlPath`'i + `-O
  check`) → `Direct`; yoksa bizim master'ı aç → `Ours`. Hata metni ayıklama ve
  yeniden deneme yok. Anahtar/agent kullanan da bizim master'ımızı alır (ekranda
  görünmez, sonraki işler bağlantı kurmadan geçer; sunucuda boşta en çok 10 dk
  bir oturum) — teknik karar.
- `ControlPath` koşulsuz eklenirse kullanıcının config'indekini ezer → yalnız
  `Route::Ours`'ta eklenir.
- Global dinleyici + jeton tablosu yeni bir app-düzeyi servis ve çökmede bayat
  soket → **deneme başına geçici soket**: master açılırken `0700` dizinde
  rastgele adlı soket, yolu `BATERI_ASKPASS` ile yalnız o master'ın
  `Command`'ına; `-f` dönünce silinir. Bağlam istemi başlatan pane'in
  thread'inde zaten belli; cevap `on_pane` ile açılan sayfadan kanal ile döner.
  Pane kapanırsa ya da ⌘Q'da gönderici düşer, yardımcı sıfırdan farklı çıkar.
- Host anahtarı sorusu için ayrı sınıf gereksiz → master'a
  `StrictHostKeyChecking=yes`; ssh soruyu hiç sormaz. Sınıflayıcı iki kol:
  "parola (Remember kutulu)" ve "öteki her şey (anahtar parolası, 2FA;
  kutusuz)".
- Bayat Keychain parolası arka planda sunucuyu kilitletebilir (fail2ban;
  `remote_helper`'ın `RETRY_AFTER` ve nesil başına yeniden araması) →
  arka plan açılışında `NumberOfPasswordPrompts=1`; ikinci soruda boş cevap
  değil sıfırdan farklı çıkış; host başına bellekte "kayıtlı parola
  reddedildi" bayrağı — nesil ve süre onu kaldırmaz, yalnız kullanıcının
  başlattığı başarılı giriş kaldırır. Arka planın sessiz açılışı bu bayrak
  olmadan inmez.
- Bayat soket ve eşzamanlı açılış → `-O check` "refused" ise soket silinir,
  açılışta dizin süpürülür; host başına tek açılış (single-flight), sahibi
  `PaneLaunch` ile geçen bir kayıt (`static` değil).
- Soket yolu 104 bayt sınırına yakın (`%C` 40 + ssh'ın geçici son eki) →
  `ControlPath` açık bir yol: kısa taban + `ssh -G`'nin kanonik
  (kullanıcı, host, port, jump) dörtlüsünün 16 haneli özeti; yol uzunluğu
  çalışma anında denetlenir, en kötü uzunluğu bağlayan bir sınama.
- Kapanışta `-O exit` iki bateri örneğinin ve 048'in kullanıcı oturumunun
  master'ını öldürebilir → kapanışta hiçbir şey gönderilmez, `ControlPersist`
  (10 dk) bitirir; kapanışın son tarihine iş eklenmez.
- Keychain yalnız uygulama sürecinde; askpass yalnız boru. Eski tip giriş
  Keychain'i (internet password), `kSecUseDataProtectionKeychain` **yok**
  (entitlement ve profil isterdi). Askpass değişkenleri kabuğun ortamına
  sızmaz. `main`'de askpass dalı `has_aqua_session()`'dan önce ve
  `bt_shell_macos` üzerinden (bin'e yeni kenar yok). Süreli koşuda askpass
  hiç kurulmaz. Parola sayfası pane'in sayfa hakemliğinden geçer
  (`set_asking`).
- Phase sırası: (i) saf parçalar (rota, istem sınıflaması, yol, argv);
  (ii) master + askpass + sayfa, Keychain'siz; (iii) Keychain + arka plan
  (reddedildi bayrağıyla). Her phase sonunda davranış "bugünkü ya da daha
  iyi". Sınama: sahte `ssh` betiğiyle (`local_ssh` emsali) askpass protokolü
  `make check`'te; master yaşam döngüsü kullanıcı ayrıcalıklı yerel `sshd` ile
  `#[ignore]`; Keychain bir trait arkasında.

**Reddedilenler:**
- `-O check` yerine `Path::exists` (codebase-fit) — bayat soket `-M`'i
  multiplexing'siz bir arka plan bağlantısına çevirir (işletme); kontrol yerel
  bir soket çağrısı, ucuz.

## Karar (2026-10-02, kullanıcı onayı)

- **Seçilen:** askpass (aynı `bateri` binary'si, `BATERI_ASKPASS` kipi,
  deneme başına geçici soket) + iş başlamadan tek kapıda rota
  (`ssh_route::ensure`: bizim master / kullanıcının master'ı / bizim master'ı
  aç) + host başına bateri'nin kendi `-M -N -f` master'ı (kısa özetli
  `ControlPath`, `StrictHostKeyChecking=yes`, boşta 10 dk, kapanışta
  dokunulmaz) + `ControlPath` yalnız bizim rotada + Keychain'de internet
  password (`objc2-security`, yalnız uygulama süreci). Arka plan işleri sayfa
  açmaz; Keychain'den tek denemeyle açar ya da susar, reddedilen parola
  kullanıcı yeniden girene kadar denenmez.
- **Reddedilen:** PTY'de expect (kırılgan istem ayıklama); yardımcının kendi
  penceresi (bağlam yok, ikinci Dock simgesi); `security-framework` (ikinci
  CF yığını); elle FFI (sınanmamış imzalar); `security` CLI (parola `ps`'te);
  akış süreçlerinin `BatchMode`'u kaldırması (soru akışın içinde asılı
  kalabilirdi); hatadan sonra yeniden deneme (altı tüketiciye dağılır);
  uygulama ömürlü dinleyici + jeton tablosu (bayat soket, yeni global
  servis); kapanışta `-O exit` (öteki örneğin ve 048'in master'ını öldürür).
- **Ürün kararları (kullanıcı, 2026-10-02 — "önerilerle devam"):**
  - Parola sayfasındaki **Remember in Keychain** varsayılan **işaretli**
    (Karar 4).
  - Arka plan işleri sayfa açmıyor; Keychain'de parola yoksa pane'in
    etiketinde tıklanabilir bir **Sign In…** düğmesi (fiil etiketli, dolgulu;
    Karar 5). Düğme parola sayfasını açar ve giriş başarılı olunca arka plan
    işleri yeni nesil beklemeden yeniden dener.
  - Bilinmeyen host anahtarında **ret**, sayfa yok: master
    `StrictHostKeyChecking=yes` ile açılır, hata satırı "connect once in the
    terminal" der (Karar 7). Trust sayfası bu sette yok.
  - Kayıtlı parolayı silmek: Shell menüsünde uzak sekmedeyken **Forget
    Password for “{host}”**; ayar penceresindeki liste sonraki tur (Karar 6).
  - Yeni bağımlılık **`objc2-security` 0.3.2** onaylı (`default-features =
    false`, `std` + `SecItem`; yalnız `bt-shell-macos`; Karar 8).

## Karar — ek (2026-10-02, kullanıcı onayı): bağlantı kullanıcının oturumuna bağlı

**Kanıt (kullanıcı, gözle kontrol adım 4):** bateri-dev kapatılıp yeniden
açıldı, `ssh` yazıldı ve parola girilmeden yük göstergesi geldi. Süreç
tablosu: adım 1'de 18:03:38'de açılan master (`ControlPersist=600`)
uygulama 18:07:03'te yeniden açıldığında hâlâ canlıydı — Muhakeme'nin
"kapanışta hiçbir şey gönderilmez" kararının sonucu. Bedeli: bağlantı
yaşadıkça aynı kullanıcının her süreci soket üzerinden parolasız komut
koşturabiliyor (Keychain'in "izin ver?" sorusu atlanıyor) ve sunucuda
kullanıcı girmeden bir oturum açılıyor.

**Kullanıcı kararı:** gösterge kullanıcının ssh girişinden **sonra** gelir.

- **Seçilen:** (1) arka plan işleri (`Ask::Never`) kullanıcının girişini
  bekler — işaret PTY'nin termios'u (`ICANON` + `ECHO` kapalı; master fd'den
  okunabildiği macOS'ta ölçüldü), hızlı yan işaretler uzak başlık, uzak OSC 7,
  `?2004h`; (2) bağlantı kullanıcının o host'taki son oturumu bitince ve
  ⌘Q'da kapanır (`-O exit`); (3) soket dizini örnek başına, yani iki bateri
  bağlantı paylaşmaz ve birinin kapanışı ötekini öldürmez. Muhakeme'nin
  "kapanışta dokunulmaz" maddesi bununla **geri alındı**.
- **Reddedilen:** "ssh başladıktan sonraki ilk Enter" işareti — ilk Enter
  çoğu zaman host anahtarı sorusunun `yes`'i, bateri yine paroladan önce
  bağlanırdı; yalnız açanın kaydıyla `-O exit` — paylaşılan soket yolunda
  ikinci örneğin aktarımını öldürür. Bedel: iki örnek aynı host'a iki giriş.
