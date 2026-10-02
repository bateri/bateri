# Uzak entegrasyon ilk bağlantıdan itibaren — Tartışma

Karar-listesi biçimi. **Durum (2026-10-03):** aşağıdaki Karar 1–10 ilk
taslağın ("girişten sonra yazma", B) kaydıdır ve **reddedildi** — panelin
bulguları `## Muhakeme`'de, seçilen yol (A) `## Karar`'da. Karar 1–10 tarihçe
olarak duruyor; uygulanan kural yalnız `## Karar`'dadır.

Tek ilke: **iki yol, tek akış, kırık bağlantı yok, kullanılmayan arayüz
yok.** Kabuklu olduğu bilinen sunucuda 048'in sarması (görünmez, rc'den önce);
bilinmeyen sunucuda girişten sonra yazma (asla kırmaz). Yazma başarılı olunca
sunucu öğrenilir, sonraki bağlantı sarma.

## Karar 1 (taslak B — reddedildi, bkz. Karar): hangi bağlantı hangi yoldan

- **Öğrenilmiş (`posix`) host** → bugünkü sarma (`ssh_wrap::decide`,
  değişmez).
- **Öğrenilmemiş host** → sarma yok, argv'ye dokunulmaz; girişten sonra yazma
  (Karar 2–6).
- **`ssh -G`'nin kuralları** (`RemoteCommand`, `RequestTTY no`, `SessionType`)
  iki yola da uygulanır: o oturumda kabuk değil kullanıcının komutu koşuyor.
- **Kabuksuz servis listesi (github, `git@` …) yazılmıyor.** Sarma yalnız
  öğrenilmiş host'a gittiği için github hiç sarılmaz; yazma da orada
  tetiklenmez (`?2004h` gelmez, oturum hemen kapanır). Liste korumadığı bir
  şeyi korurdu (YAGNI).
- **Ne görünür** (ürün sonucu, Karar 7'deki soruyla birlikte): arkadaşın ilk
  `ssh`'ta parolayı terminalde girer, ilk prompt'tan hemen sonra dizin ve
  bloklar gelir; router/Windows'a bağlanan biri bugünkü düz ssh'ı görür,
  hiçbir şey kırılmaz, hiçbir ek düğme yok.

## Karar 2 (taslak B — reddedildi, bkz. Karar): yazmanın tetiği — kapı

Uzak çıktı yalnız **zamanı** belirler; kapının bütün koşulları **aynı anda**
(teknik karar, güvenlik ilkesi context → Motivasyon):

1. Uzak oturum var ve bu uzak nesilde henüz yazılmadı (nesil başına **bir
   kez**).
2. Kullanıcının girişi görüldü (termios kenarı, `remote_login`) — parola
   sorusunda (`ICANON=1 ECHO=0`) ve host anahtarı sorusunda asla.
3. `CSI ? 2004 h` uzak durum kurulduktan **sonra** geldi
   (`paste_since_remote`; zsh ≥ 5.1, bash ≥ 5.1, fish 3+ basar). Gelmezse
   (eski bash, csh, kabuksuz uç) hiçbir şey yazılmaz — yanlışın yönü güvenli.
4. **O an** PTY kanonik değil (`ICANON=0`: satır düzenleyici okuyor) ve
   alternatif ekranda değil. `read` bekleyen bir rc sorusu (`ICANON=1`) ya da
   rc'nin başlattığı vim (alternatif ekran) yazmayı almaz.
5. Kullanıcı uzak durum kurulduğundan beri **hiç tuş göndermedi**
   (`send_input`'un nesli). Yazmaya başladıysa vazgeçilir; o oturum düz kalır,
   host öğrenilmez, sonraki bağlantı yeniden dener.
6. `ssh -G` kuralları (Karar 1) ve ayar (Karar 8) izin veriyor.

**Gönderilen bayt sabit ve yereldir** (Karar 3); uzak çıktının içeriği ona hiç
girmez.

## Karar 3 (taslak B — reddedildi, bkz. Karar): ne yazılıyor — kısa satır + tty'den yük

048 Karar 3-B (tty aktarımı) sarma yolu için reddedilmişti (orada yük satır
içi sığıyordu). Burada satır içi 30 KB **kullanıcının prompt'una yazılmış
bir komut** olurdu: ekranda 400 satır yankı, geçmişte dev bir satır, bazı
satır düzenleyicilerde uzunluk sınırı. Bu yüzden iki adım:

- **Kısa satır** (sabit, ~150 bayt, zsh/bash/fish'in ortak alt kümesinde:
  `;`, tek tırnak, `source`, `$()` yok), başında bir boşluk:
  `stty -echo` → `sh -c '<küçük okuyucu>'` → `source
  ~/.local/share/bateri/shell/inject`.
- **Okuyucu** önce bir hazır işareti basar (`8133;i;ready`); bateri işareti
  görünce yükü gönderir: `boot.sh`'in dosyaları, base64, 76'lık satırlar
  (kanonik kipte `MAX_CANON` 1024'ün altında), bitiş satırı. Okuyucu çözer,
  dosyaları 048'in yerine aynı kuralla yazar (`bt_put`, geçici ad + `mv`) ve
  giriş kabuğuna göre `inject` dosyasını kurar. Hazır işareti de yalnız
  **zamanı** belirler; işaret gelmezse (`sh` yok, `stty` yok) bateri yükü hiç
  göndermez ve zaman aşımında (Karar 5) vazgeçer.
- `stty -echo` yükün ekrana dökülmesini önler (komut koşarken kabuk tty'yi
  kanonik ve yankılı açar); okuyucu çıkarken `stty echo`.

## Karar 4 (taslak B — reddedildi, bkz. Karar): çalışan kabuğa kurulum — `source`, `exec` değil

- **`exec` ile yeniden başlatmak** (048'in `ZDOTDIR` dansı) giriş dosyalarını
  **ikinci kez** koşturur (`.zprofile`, `.bash_profile`: ssh-agent, banner,
  tmux auto-attach iki kez) ve motd'u ikinci kez basar. Reddedildi.
- **`source` ile çalışan kabuğa kanca eklemek**: OSC 7 ve OSC 133 yalnız
  kanca istiyor (zsh `add-zsh-hook`, bash `PROMPT_COMMAND`/`PS0`, fish
  `--on-event`). 048'in uzak betikleri (`assets/shell/remote/{zsh,bash,fish}`)
  bugün kancaları açılışta kuruyor; `__bateri_hooks` gövdesi (zsh) ve
  karşılıkları ayrılıp hem açılış yolundan hem `inject`'ten çağrılır — tek
  kanca kodu, iki giriş. Blok kimliği aynı: `bt_remote=<P>.<S>.<n>`; `P`
  (yerel ssh bloğu) yük başlığıyla gider — bateri'nin bildiği yerel bir sayı.
- Öneri: **`source`**. Bilinen sınır: kullanıcı rc'si `PROMPT_COMMAND`'ı
  sonradan ezen bir tema kurduysa (bash) kanca bir prompt geç kalabilir;
  `inject` kendini en sona ekler.

## Karar 5 (taslak B — reddedildi, bkz. Karar): yazma sırasında kullanıcının tuşları ve zaman aşımı

- Yazma başladığı an `held_input` emsaliyle kullanıcının tuşları
  **bekletilir** ve okuyucunun bitiş işaretinden (`8133;i;done`) sonra kabuğa
  gider — ⌘T'nin ilk girdisinin yolu (`session.rs` okuyucu döngüsü).
- Zaman aşımı (tasarım sabiti, yük boyutu ve tipik RTT'den; ölçülmüş bir sayı
  değil): işaret gelmezse bekletilen tuşlar gönderilir, `stty echo` için
  okuyucunun kendi çıkışı yeterli; bateri yalnız vazgeçer, host öğrenilmez.
- Kapı 5 gereği yazma kullanıcı tuşa basmadan başlar; bekletme yalnız yazma
  sürerken basılan tuşlar için.

## Karar 6 (taslak B — reddedildi, bkz. Karar): ekranda kalan iz [ürün]

Satır düzenleyici yazılan satırı kendisi çiziyor (`ECHO` termios'ta zaten
kapalı), yani kısa satır prompt'ta görünür. Yollar:

- **(a) Silmek:** okuyucu bitince satırın kapladığı satırları geri siler;
  satır sayısını bateri yük başlığında verir (yazdığı yerin sütunu ve
  genişlik bateri'de belli — yerel veri). Sonuç: ilk prompt'un altında **boş
  bir prompt** daha, yani kullanıcı Enter'a bir kez basmış gibi.
- **(b) Bırakmak:** ` stty -echo; sh -c …; source …` satırı ekranda kalır.
- Öneri: **(a)**. İlk bağlantıda ne görülür: parola → motd → prompt, bir an
  (yük süresince) prompt'un ardında kısa bir komut, sonra boş bir prompt
  satırı ve yeni prompt; dock'ta dizin. İkinci bağlantıdan itibaren (sarma)
  hiçbir iz yok.

## Karar 7 (taslak B — reddedildi, bkz. Karar): kabuk geçmişi [ürün]

- Satır bir boşlukla başlar: oh-my-zsh (`hist_ignore_space`) ve Debian/Ubuntu
  bash (`ignoreboth`) onu geçmişe yazmaz. Bunun dışında kalan kabuk (düz zsh,
  Fedora bash) satırı geçmişe alır; yükün kendisi asla (tty'den okunuyor).
- Geçmiş dosyasından silmek kullanıcının bir dosyasına yazmak olurdu
  (`history -d` + `history -w` / `fc`): proje kuralının ruhuna aykırı ve
  kabuk başına kırılgan. Öneri: **silmemek**, satırın kendisini açıklayıcı ve
  kısa tutmak (`# bateri` son eki). İlk bağlantıda görülen: geçmişte en çok
  bir kısa satır, yalnız o sunucunun ilk bağlantısında ve yalnız bu iki
  kabukta.

## Karar 8 (taslak B — reddedildi, bkz. Karar): ayar

`[remote] integration` ve host başına kapama (048) **iki yolu birlikte**
kapatır; ayrı bir "sonradan yazma" anahtarı yok (öneri, teknik — kullanıcıya
iki anahtar tek kavram için anlamsız). `production` işaretli host'ta varsayılan
kapalı kuralı aynen.

## Karar 9 (taslak B — reddedildi, bkz. Karar): `ssh` fonksiyonunun görmediği oturumlar

Kapı (Karar 2) yerel `ssh` fonksiyonuna değil **uzak oturum algılamasına**
(`jobs::remote`, `RemoteProbe`) bağlı; yani betikten, `exec ssh`'tan,
kullanıcının alias'ından ya da bash/fish yerel kabuğundan açılan ssh da
yazma yolunu alır (Ghostty #9708'in sınıfı kapanıyor). `ssh -G` için argv
`jobs`'un yürüyüşünden. **mosh kapsam dışı**: istemcinin tahmini yankısı
yazılan satırı ve yükü ekrana basar, ayrı karar ister.

## Karar 10 (taslak B — reddedildi, bkz. Karar): öğrenme ve 048'den kalkanlar

- **`posix` yalnız başarıdan yazılır:** sarma yolunda `boot.sh`'in yeni
  "başladım" işareti (`8133;i;up`, kancalar kurulunca, ilk prompt'tan önce),
  yazma yolunda okuyucunun `done`'ı. İşaret yerel kabuğun okuyucu döngüsünde
  kenarda yakalanır; `ssh_wrap::record(Fact::Posix)` arka planda.
- **Yardımcının selamından öğrenme kalkar** (`remote_helper_for(learning)`,
  `ssh_wrap::learn`): Sign In bağımlılığı onunla gidiyor. Yardımcının kendi
  işi (047) değişmez.
- `touched` kalır (sarma ve yazma ikisi de yazar).
- Belgeler: `CLAUDE.md`'nin 048 cümleleri (ilk bağlantıda öğren → iki yol),
  `docs/AYARLAR.md`'nin `integration` satırı, 048 `discussion.md`'de
  işaretçi (yazıldı).
- **Sarılmış oturumda kırılma** (öğrenilmiş host sonradan kabuksuz olursa)
  için ayrı arayüz yazılmıyor: `boot.sh` her hatada düz giriş kabuğuna düşüyor
  (048 R3), yani bağlantı kırılmıyor; `8133;f` etiketi bugünkü yerinde.

## Kaba phase bölmesi (taslak B — reddedildi; geçerli bölme `plan.md`'de)

1. **Saf parçalar:** kapının durum makinesi (`bt-core`, sınamalı: parola
   sorusu, `ICANON=1`, alternatif ekran, kullanıcı tuşu, nesil başına bir
   kez), kısa satır ve yük protokolü (`bt-shell-common::ssh_wrap`), okuyucu ve
   `inject` betikleri, kanca gövdelerinin ayrılması; `boot.sh`'in `up`
   işareti. Davranış değişmez.
2. **Uçtan uca:** pane'de kapı → yazma → bekletme → silme → öğrenme;
   yardımcının öğrenmesinin kaldırılması; `jobs`/PTY simülasyonu (zsh, bash,
   fish) ve Docker sshd (parolalı + oh-my-zsh) ile "ilk bağlantıda dizin"
   sınaması.
3. **Belgeler ve set kapısı:** `CLAUDE.md`, `docs/AYARLAR.md`, `make bundle`.

## Muhakeme (2026-10-03)

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU |
| Codebase-fit | SORUNLU |
| İşletme | SORUNLU |

Üç mercek bağımsız olarak "girişten sonra yazma" yolunun (B) bugünkü kodla
güvenilir kurulamadığını gösterdi; sadelik merceği arayüzsüz daha basit bir
yol (A) önerdi ve kullanıcı onu seçti (`## Karar`).

**Kabul edilen bulgular → B reddedildi:**
- Kapının "satır düzenleyici okuyor" koşulu (`ICANON=0`) yerelden
  gözlenemez: girişten sonra master'ın `tcgetattr`'ı bütün oturum boyunca
  ssh'ın raw kipini veriyor (`TtyModes`'un doc'u, `bt-core/src/shell.rs`).
  Uzak rc'nin `read` sorusu görünmez; koşul hep doğru, koruma sahte.
- "Kullanıcı henüz tuşa basmadı" koşulu parolalı sunucuda hep yanlış: uzak
  durum `C` kenarında, girişten önce kuruluyor ve parolanın tuşları
  `send_input` üzerinden `key_gen`'i artırıyor — motivasyondaki senaryonun ta
  kendisi.
- `?2004h` yalnız kabuğun prompt'u değil: readline 8.1+ kullanan psql, mysql,
  python REPL, rc'nin `vared`/`read -e`'si ve `sudo -i` zincirinin root
  kabuğu da basıyor; kısa satır yanlış programa gidebilirdi.
- `stty -echo`'nun kurtarma yolu oturumu bozuk bırakabiliyordu (okuyucu
  yalnız bitiş satırıyla çıkar; vazgeçişte bırakılan tuşlar yankısız
  okuyucuya düşer, sonraki parola körlemesine yazılır).
- Uzak 8133 savunması (048) gevşerdi: sahte `ready` 540 satırlık base64'ü
  satır düzenleyiciye komut olarak dökerdi, sahte `done` host'u yanlış
  öğretirdi (iTerm2 "cat readme.txt" dersi).
- Host başına **tek** bağlantıda koşan ikinci bir kurulum yolu testlerden
  kaçar; bozulduğunu yalnız yeni sunucu ekleyen kullanıcı görür.

**A'ya taşınan bulgular:**
- Başarı işareti (`up`) **nonce'lu** ve yalnız sarılmış oturumda — argv'si
  `ssh_wrap::unwrap`'tan geçen ve nonce'u o argv'de taşıyan — kabul edilir;
  sahte `up` host'u yanlış öğretemez.
- Argv'de uzak komut varsa sarma yok (`decide`'ın bugünkü kuralı, aynen).
- ssh'ın kendi hata kodu **255** (bağlantı ya da kimlik hatası) host'u
  kabuksuz diye işaretlemez; yalnız bağlanıp `up` göndermeyen oturum işaretler.
- Çalışan kabuk `$SHELL`'den farklı olabilir: sarma yolu giriş kabuğunu
  zaten kendisi `exec` ediyor, yani B'nin "hangi kabuğa source" sorunu A'da
  doğmuyor (not).

**Reddedilenler:**
- B'nin kendisi (yukarıdaki bulgular).
- "Önce sar + kırılınca *reconnect without integration* arayüzü" — arayüz
  gereksiz: düşme sessiz ve kendiliğinden (`## Karar`).
- Bağlanmadan önce kabuğu yoklamak — tek yolu yardımcı oturum, parolalı
  sunucuda yine Sign In demek (048'in geri alınan hatası).

## Karar (2026-10-03, kullanıcı onayı)

Kullanıcı: "A ile devam et, --auto ile uygula".

- **Seçilen (A): her zaman sar, kırılırsa sessizce düz ssh ile yeniden
  bağlan; arayüz yok.**
  - Bilinmeyen host da sarılır; yalnız `plain` (kabuksuz) olarak kayıtlı
    host ve `ssh -G` kuralları (`RemoteCommand`, `RequestTTY no`,
    `SessionType`), uzak komut ve ayar sarmayı engeller.
  - `boot.sh` ilk iş olarak nonce'lu `up` basar; bateri onu görünce host'u
    `posix` kaydeder.
  - Sarılmış oturum `up` görmeden biter ve ssh'ın çıkış kodu 255 değilse
    host `plain` kaydedilir ve zsh `ssh` fonksiyonu aynı argv'yi sarmasız
    ama aynı `u-<key>` ControlPath'iyle yeniden koşturur.
- **Kullanıcı ilk bağlantıda ne görür:**
  - *Kabuklu sunucu (neredeyse herkes):* parola terminalde, motd, prompt;
    dock'ta dizin, komut blokları ve süre — ilk prompt'tan itibaren. İz yok,
    geçmişe satır yok, Sign In yok.
  - *Kabuksuz uç (router, Windows), yalnız ilk kez:* bir hata satırı
    (sunucunun kendi mesajı, ör. `'exec' is not recognized…`) ve bağlantı
    kendiliğinden düz yeniden açılır. İkinci parola **beklenmiyor**:
    sarılmış oturumun `u-<key>` master'ı `ControlPersist=2` ile 2 sn yaşıyor
    ve yeniden bağlanma aynı ControlPath'le ona biniyor — **doğrulanacak**
    (phase-2); tutmazsa ikinci parola sorulur, bilinen sınır olarak yazılır.
  - *Sonraki bağlantılar:* kabukluda sarma, kabuksuzda doğrudan düz.
- **048'in "ilk bağlantıda öğren" kararı geri alındı:** yardımcının selamından
  `posix` öğrenme ve onunla gelen Sign In bağımlılığı kalkar. Durum
  dosyasında `posix` yalnız `up`'tan yazılır; yeni `plain` olgusu; `touched`
  kalır (ileride "Remove bateri files" düğmesi için).
- **Kapsam dışı:** girişten sonra yazma (B); betik, `exec ssh`, alias ya da
  yerel bash/fish'ten açılan ssh'lar (ayrı ve daha sağlam bir tasarım ister);
  mosh; uzak dock.
- **Reddedilen:** B (Muhakeme); kırılma arayüzü; bağlanmadan önce yoklama.
