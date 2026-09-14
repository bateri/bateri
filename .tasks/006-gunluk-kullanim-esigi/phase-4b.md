# Phase 4b — Dock açılışı: ev dizini ve yerel

## Özet

Kabuk her açılışta kullanıcının ev dizininde başlar; süreç ortamında yerel
(`LC_ALL`/`LC_CTYPE`/`LANG`) yoksa çocuğa macOS'un dil/bölge ayarından bir
UTF-8 yereli verilir.

_Requirements: R4.4_

---

## Neden bu phase var

phase-4'ün `/code-review`'u buldu, `phase-4.md` → `## Uygulama Notları` →
"Dock açılışında kabuğun başladığı yer": LaunchServices süreci `cwd=/` ile
başlatıyor (probe'la doğrulandı) ve Dock launchd'nin ortamını veriyor —
orada `LANG` olup olmadığı doğrulanmadı; yoksa UTF-8 girişi bozulur.
`cargo run` ikisini de göstermez (çağıranın dizinini ve ortamını miras alır).

Kullanıcı 2026-09-14'te üç karar verdi (`discussion.md` → Karar 6 eki):
**006'ya girer** (phase-3b'den sonra, phase-5 ölçümünden önce);
**her zaman ev dizini** (istisnasız, `cargo run` dahil);
**yerel yoksa macOS dil/bölge ayarından**.

Referans: alacritty macOS'ta `main`'de koşulsuz `env::set_current_dir(home)`
yapıyor ve `macos/locale.rs` → `set_locale_environment()` ortam yereli
geçersizse `NSLocale`'in `languageCode` + `countryCode`'undan
`{dil}_{ülke}.UTF-8` kuruyor, geçersizse `LC_CTYPE=UTF-8`'e düşüyor
(orkestratör `master`'dan okudu). **İkisini de kendi sürecinde `set_var` /
`set_current_dir` ile yapıyor — biz yapmıyoruz** (`CLAUDE.md` →
"`tty::setup_env()` çağrılmaz"): ikisi de yalnız çocuğa verilir.

---

## 1. `bt-core`: çocuğun dizini ve ek ortamı

`crates/bt-core/src/session.rs` → `SessionOptions` ve `Session::spawn`.

- `SessionOptions`'a çocuğun çalışma dizini (`tty::Options.working_directory`)
  ve **ek ortam** girer. Ad ve tip senin kararın (`Option<PathBuf>`,
  `Vec<(String, String)>` gibi); pub API'de alacritty tipi görünmez.
- `None` → bugünkü davranış (miras). Sınamalar ve duman bugünkü gibi
  `None` verir; sonuçları dizine bağlı olmamalı.
- Ek ortam `TERM`/`COLORTERM`'ü **ezemez**; öncelik sırasını doc'a yaz ve
  sınamaya bağla.
- **Politika `bt-core`'da değil:** "ev dizini" ve "yerel" kararları
  uygulamanın (`bt-shell`) kararıdır; `bt-core` yalnız verileni çocuğa
  geçirir. `bt-core` `/usr/share/locale` gibi macOS yolu da bilmez.

## 2. `bt-shell`: ev dizini

`crates/bt-shell/src/app.rs` → kullanıcı oturumunun `SessionOptions`'ı.

- Çalışma dizini `$HOME`. `HOME` yoksa ya da boşsa `None` (miras) — ve bunu
  notlara yaz. `bt-core`'da `libc` doğrudan bağımlılık değil ve
  `getpwuid` için bağımlılık **eklenmez**; launchd GUI süreçlerine `HOME`
  veriyor, bu yol yalnız kenar durum.
- Duman/süreli koşunun sabit komutu da aynı kuralı alabilir ya da `None`
  kalabilir; seçimini gerekçesiyle yaz (duman sonucunun dizine bağlı
  olmadığını göster).

## 3. `bt-shell`: yerel

- **Karar saf bir fonksiyonda** ve sınanabilir: girdi olarak ortam okuyucu
  (`LC_ALL`, `LC_CTYPE`, `LANG`), sistemin dil/ülke kodu ve "bu yerel var mı"
  sorusu; çıktı çocuğa eklenecek tek bir ortam çifti ya da hiçbir şey.
  - Üçünden biri **boş olmayan** bir değerle tanımlıysa → hiçbir şey eklenmez
    (kullanıcının ortamına dokunulmaz; `cargo run` bu yoldan geçer).
  - Değilse ve `{dil}_{ülke}.UTF-8` sistemde varsa → `LANG={dil}_{ülke}.UTF-8`.
    `LC_ALL` değil `LANG`: en zayıf değişken, Terminal.app'in yaptığı; kabuğun
    rc dosyası kendi `LC_*`'ını üstüne yazabilsin (alacritty `LC_ALL` yazıyor —
    farkı doc'a yaz).
  - Yoksa (ör. İngilizce dil + Türkiye bölgesi → `en_TR.UTF-8` **yok**, bu
    makinede `ls /usr/share/locale` ile doğrulandı) → `LC_CTYPE=UTF-8`
    (alacritty'nin düşüşü; `/usr/share/locale/UTF-8` var). Mesajlar İngilizce
    kalır ama UTF-8 girişi çalışır.
- **"Var mı" sorusu:** `/usr/share/locale/{ad}` dizininin varlığı. `setlocale`
  ile sınama **yapılmaz** — kendi sürecimizin global yerelini değiştirir.
- **Sistem kodu:** `NSLocale::currentLocale()` → `languageCode` ve
  `countryCode`. `objc2-foundation`'a `NSLocale` feature'ı eklenecek: **yeni
  crate değil**, var olan bağımlılığın bayrağı. `Cargo.lock` **oynamamalı** —
  oynarsa dur, eskalasyon. `Cargo.toml` değişikliğini notlara ve commit
  gövdesine yaz (karar kaydı: `discussion.md` → Karar 6 eki).

## 4. Belge

`CLAUDE.md` → "`tty::setup_env()` çağrılmaz" maddesi çocuğun ortamını
sayıyor (`TERM`, `COLORTERM`); `LANG`/`LC_CTYPE` kuralı ve ev dizini **aynı
commit'te** oraya girer. `bt-shell` katman satırındaki platform kütüphanesi
listesi `NSLocale`'i kapsamıyorsa düzelt.

---

## Uygulama Notları

- **`bt-core` (`SessionOptions`).** İki alan:
  - `working_directory: Option<PathBuf>` → `tty::Options.working_directory`.
    Gidilemeyen dizin sessizce yoksayılıyor, çocuk miras alıyor: alacritty
    `pre_exec`'te `chdir`'in sonucuna bakmıyor. Doc'ta.
  - `env: HashMap<String, String>` → `tty::Options.env`. İlk taslak
    `Vec<(String, String)>`'di; `/simplify` gösterdi ki tek tüketici onu
    bir satır sonra zaten haritaya topluyor ve "aynı anahtar iki kez"
    kenar durumu doc'a yük bindiriyordu. `std` tipi, alacritty tipi değil.
  - Öncelik doc'ta ve kodda aynı sırada: ek ortamın haritasına `TERM` ve
    `COLORTERM` **sonra** giriyor. Ek ortam alacritty'nin koşulsuz
    `USER`/`HOME`/`WINDOWID`'sini ezebiliyor, çünkü alacritty `config.env`'i
    onlardan sonra yazıyor; bugün bunu yapan çağıran yok.
  - Sınamalar: `working_directory_sets_child_cwd`,
    `child_inherits_cwd_without_working_directory`,
    `extra_env_reaches_child_without_overriding_term`. Çocuk `pwd -P` ya da
    değişkenleri basıyor ve kıyas `assert_eq!` ile. Beklenen yol
    `canonicalize` ediliyor, çünkü macOS'ta `/var` → `/private/var`.
  - **Test-first:** alanlar önce eklendi ama `tty::Options`'a bağlanmadı.
    Dizin ve ortam sınamaları kırmızı düştü (`cwd=…/crates/bt-core;`,
    `env=|xterm-256color|truecolor;`). Miras sınaması baştan yeşildi, çünkü
    bugünkü davranışın kendisi.
  - **Mutasyon:** `TERM`/`COLORTERM` ek ortamdan **önce** yazılınca ortam
    sınaması `env=reached|dumb|none;` ile kızardı.
  - Test yardımcıları: `test_options` (dizin ve ortam boş) ve `sh(script)`.
    `sh` üç kopya `("/bin/sh", ["-c", …])` demetini topladı; biri eski
    `dump_session`'daydı.
- **`bt-shell` → `child` (yeni modül).**
  - **Dizin — sapma:** kılavuz "`HOME` yoksa miras, `getpwuid` için
    bağımlılık eklenmez" diyordu. `/simplify`'ın altitude merceği gösterdi
    ki `std::env::home_dir` (1.88'de kullanımdan kalkmış değil) `HOME`'u,
    yoksa passwd kaydını (`getpwuid_r`) okuyor. Yeni bağımlılık istemiyor.
    alacritty çocuğa `HOME`'u aynı sırayla yazıyor (`ShellUser::from_env`),
    yani kabuğun `pwd`'si ile `$HOME`'u tek kaynaktan geliyor. Eski hâlde
    `HOME`'suz bir açılışta çocuk `HOME=<passwd>` alıp `/`'da başlardı.
    **Mutlak olmayan** `HOME` → `None` → miras: boş (`std` `Some("")`
    veriyor) ve göreli (`chdir` onu bizim dizinimize, Dock'ta `/`'a göre
    çözerdi) tek koşulda. Göreli kol `/code-review`'dan geldi. Sınama
    `home_directory_is_home_unless_missing_or_empty`; mutasyon (`is_absolute`
    → boş değil) göreli satırda kızardı. `pwd` ile `$HOME`'un ayrışabildiği
    tek kenar UTF-8 olmayan `HOME` (`std` alıyor, alacritty passwd'ye
    düşüyor); doc'ta, düzeltilmedi.
  - **Yerel:** saf `decide_locale(env, system, installed)` + ince
    sarmalayıcı `locale_env()`. Kolları:
    - üç değişkenden biri boş olmayan değerle tanımlı → hiçbir şey
      (`locale_in_env_is_left_alone`, değer `C` bile olsa);
    - boş değer tanımsız (`empty_locale_var_counts_as_unset`);
    - kurulu `{dil}_{bölge}.UTF-8` → `LANG` (`installed_system_locale_becomes_lang`);
    - kurulu değil → `LC_CTYPE=UTF-8` (`missing_system_locale_falls_back_to_utf8_ctype`);
    - **kılavuzda olmayan kol:** bölge `None` (bölgesiz yerel, ör. `en`) →
      `LC_CTYPE=UTF-8` (`locale_without_region_falls_back_to_utf8_ctype`).
  - **Sapma — dil `preferredLanguages`'tan, `currentLocale().languageCode`
    değil (`/code-review` bulgusu, paketle doğrulandı).** Paketin içinde
    `currentLocale` dili kullanıcının değil paketin yerelleştirmesinden
    seçiyor: `bateri.app` `.lproj` taşımıyor, `CFBundleDevelopmentRegion`
    `en`. İlk taslakla kurulmuş pakete `--args -AppleLanguages (tr-TR)
    -AppleLocale tr_TR` verilince çocuk `LC_CTYPE=UTF-8` aldı
    (`tr_TR.UTF-8` değil); `(fr-CA)` + `fr_CA` ile `LANG=en_CA.UTF-8` aldı
    (`fr_CA.UTF-8` değil). `cargo run` paketsiz olduğu için doğru dili
    veriyordu, birim sınamaları da dili elle geçiriyordu — ikisi de
    göremezdi. Düzeltme: dil `NSLocale::preferredLanguages()`'ın ilk
    etiketinden (`primary_language`: `tr-TR` → `tr`; sınama
    `primary_language_is_the_first_subtag`, test-first kırmızı; boş alt
    etiket denetimi silinince kızardı), bölge `currentLocale().regionCode`
    (paket onu etkilemiyor). Düzeltmeden sonra aynı iki koşu:
    `LANG=tr_TR.UTF-8` ve `LANG=fr_CA.UTF-8`. alacritty `currentLocale`
    kullanıyor; kılavuzun referansı bu noktada izlenmedi.
  - **Sapma — `countryCode` değil `regionCode`:** objc2-foundation 0.3.2'de
    `countryCode` `#[deprecated]` (SDK: `API_DEPRECATED_WITH_REPLACEMENT
    ("regionCode", …)`) ve `-D warnings` altında kırmızı olurdu.
    `regionCode` SDK'da `macosx(14.0)`, taban da 14. Farkı `@rg=` alt
    etiketi: kullanıcı bölge biçimini ayrıca seçtiyse o bölge gelir. Bu
    makinede JXA ile okundu (paketsiz): `localeIdentifier=en_TR`,
    `languageCode=en`, `regionCode=TR`.
  - "Kurulu mu" sorusu `/usr/share/locale/{ad}` dizini (`setlocale` yok).
    Ad `/` taşıyorsa reddediliyor (`locale_name_with_slash_is_not_installed`).
    **O sınamanın ilk hâli dişsizdi:** `../../usr` `/usr/usr`'e çözülüyor
    ve denetim silinince de yeşil kaldı. `../../../usr` (`/usr`) ile
    mutasyon kızardı.
  - **Test-first:** fonksiyonlar `None` döndüren taslakla yazıldı → 6
    sınamanın 5'i kırmızı. `locale_in_env_is_left_alone` taslağa karşı
    yeşildi (beklenen de `None`); dişi aşağıdaki mutasyonla gösterildi.
  - **Mutasyonlar (dördü de kızardı):**
    - `is_some_and(!empty)` → `is_some()`: `empty_locale_var_counts_as_unset`;
    - değişken listesi `["LANG"]`'a indi: `locale_in_env_is_left_alone`;
    - `installed` denetimi silindi: `missing_system_locale_falls_back_to_utf8_ctype`;
    - `/` denetimi silindi: `locale_name_with_slash_is_not_installed`.
- **Süreli koşu da aynı kuralı alıyor (dal yok).** Karar "istisnasız" ve iki
  sabit betik dizine ve yerele bağlı değil (`smoke_shell`: `printf` +
  `sleep`; `load_shell`: `date` + ASCII `printf`). Duman satırı değişmedi:
  `kare=1 hucre=8 glif=6 kural=15 yuva=13/2048 yuk=smoke istek=2
  kapanis=clean profil=debug ornek=off pipeline=ok`.
- **`Cargo.toml`:** `bt-shell`'in `objc2-foundation` feature listesine
  `NSLocale` eklendi. Varsayılan sette zaten açıktı (kod bayraksız da
  derlendi), satır `NSRunLoop`/`NSDate` gibi kırpmaya karşı koruma.
  `Cargo.lock` değişmedi. Koruma **eksik** ve yorum bunu söylüyor
  (`/code-review`): `regionCode`/`preferredLanguages` ayrıca `NSString` +
  `NSArray` ister. İkisi eklenmedi — implementer yetkisi yalnız `NSLocale`
  (karar kaydı); ikisi varsayılan sette ve crate onları başka yerlerde de
  listesiz kullanıyor.
- **`CLAUDE.md`:**
  - `tty::setup_env()` maddesine şunlar girdi: ek ortamın `TERM`/`COLORTERM`'ü
    ezemediği, dizin ve yerel kuralı, `LANG`/`LC_ALL` farkı, politikanın yeri.
  - Katman tablosu: `bt-core` satırına girdi kodlaması (phase-3b devri),
    `bt-shell` satırına `child` ve `objc2-foundation (NSLocale dahil)`.
  - `bt-shell` `lib.rs` başlık yorumu da `child`'ı sayıyor.
- **Paketli açılış kanıtı (ölçüm değil).** Yoklayıcı `probe.sh` scratchpad'de.
  Kullanıcının kabuğunun yerine geçiyor: `SHELL` ile verildi, alacritty
  onu `login -flp` altında koşuyor. `pwd -P`, `LANG`, `LC_ALL`, `LC_CTYPE`
  ve `env`'i dosyaya yazıp çıkıyor, uygulama `child_exit` ile kapanıyor.
  Yani ölçülen yol duman yolu değil, **kullanıcı oturumu yolu**. Komut:
  `env -i HOME USER LOGNAME PATH TMPDIR open -W -n --env SHELL=probe.sh
  target/release/bateri.app`, `cwd=/tmp`'den.
  - **Önce** (değişiklikten önce kurulmuş paket): `pwd=/`, `LANG`,
    `LC_ALL`, `LC_CTYPE` üçü de tanımsız. Bulgu doğrulandı.
  - **Sonra** (kapı sonrası son `make kur`, aynı komut):
    `pwd=/Users/kalaomer`, `LANG` tanımsız, `LC_CTYPE=UTF-8` (bu makine
    `en` + `TR`, `en_TR.UTF-8` yok).
  - **Başka kullanıcı ayarları** (aynı komut + `--args -AppleLanguages … -AppleLocale …`):
    `(tr-TR)`/`tr_TR` → `LANG=tr_TR.UTF-8`; `(fr-CA)`/`fr_CA` →
    `LANG=fr_CA.UTF-8`; ikisinde de `pwd=/Users/kalaomer`, `LC_CTYPE`
    tanımsız. (İlk taslakta `LC_CTYPE=UTF-8` ve `LANG=en_CA.UTF-8` idi,
    yukarıda.) `-Apple*` argümanları kullanıcının tercihlerini **bu süreç
    için** ezer; sistem ayarının kendisi değişmedi.
  - **Ortamda yerel varken** (`LANG=tr_TR.UTF-8`, hem çağıranın ortamında
    hem `--env` ile): `pwd=/Users/kalaomer`, `LANG=tr_TR.UTF-8`,
    `LC_CTYPE` tanımsız — dokunulmadı.
  - **`cargo run`** (terminalden, `LANG=en_US.UTF-8`):
    `pwd=/Users/kalaomer`, `LANG=en_US.UTF-8`, `LC_CTYPE` tanımsız.
  - **`HOME`'suz** (paketin binary'si doğrudan, `env -i` ile `HOME` yok,
    `cwd=/tmp`): `pwd=/Users/kalaomer`, çocukta `HOME=/Users/kalaomer`
    (alacritty passwd'den), `LC_CTYPE=UTF-8`. passwd düşüşü iki tarafta da
    aynı dizini verdi.
  - **Gösterdiği:** LaunchServices yolunda (`__CFBundleIdentifier`,
    `XPC_SERVICE_NAME`, launchd'nin `SSH_AUTH_SOCK`'u çocukta görünüyor)
    çocuğun dizini ve yereli kurala uyuyor ve `login -flp` ikisini de
    koruyor.
  - **Göstermediği:** gerçek Dock/Finder açılışı. `open` çağıranın ortamını
    geçiriyor, `env -i` launchd ortamının bir **yaklaşığı**, kendisi değil.
    Dock'ta `LANG` olmadığının dayanağı `launchctl getenv LANG`'ın boş
    dönmesi. `ğüşıöç İ` girişinin ekranda doğru görünmesi de gösterilmedi.
    İkisi `[elle]` maddesinde kalıyor.
  - Düşüşün kabuk tarafındaki anlamı ayrıca denendi (paketsiz):
    `env -i LC_CTYPE=UTF-8 /bin/zsh -fc` → `locale charmap` `UTF-8` ve
    7 harflik `ğüşıöçİ` 7 karakter sayılıyor; yerelsiz `US-ASCII` ve 14
    (bayt). Yani `LC_CTYPE=UTF-8` zsh'ı çok baytlı kipe alıyor; ekranda
    doğru görünmesi yine `[elle]`.
  - Not: çocukta `SHELL=/bin/zsh` görünüyor, yoklayıcının yolu değil.
    Yoklayıcı yine de koştu; değişken alacritty'nin `login -flp … /bin/zsh
    -fc "exec -a …"` zincirinde yeniden yazılıyor (hangi halkada olduğu
    ayrıştırılmadı).
- **`make test-yaris`:** kilit yolu ve paylaşılan durum değişmedi (yalnız
  açılış ayarları). Yine de `/simplify` sonrası koşuldu → 0; `/code-review`
  düzeltmeleri `bt-core`'da yalnız doc ve bir sınama ekledi, yeniden
  koşulmadı.
- **`/simplify` (4 mercek).**
  - Yeniden kullanım ve verimlilik: temiz.
  - Sadeleştirme, uygulandı: `SessionOptions.env` `Vec` → `HashMap`;
    sınamanın `env_of`'u doğrusal arama yerine harita.
  - Seviye (altitude), uygulandı: ev dizini `std::env::home_dir`'den
    (yukarıda, sapma).
  - Kendi okumamdan, uygulandı: `sh(script)` yardımcısı, `child_output`
    doc'undaki olmayan `script` parametresi, gereksiz `as Arc<dyn Wake>`.
  - Atlanan: `NSLocale`'i ortamda yerel varken hiç sormamak (tembel
    `system`). Açılışta bir kez birkaç mesaj; imzayı karmaşıklaştırırdı,
    mercek de "isteğe bağlı" dedi.
- **`/code-review` (12 bulgu).** Skill olarak koştu (arka plan).
  - **Giderilen:**
    - (1) paketin içinde yanlış dil → `preferredLanguages` (yukarıda, sapma);
    - (3) `CLAUDE.md` ve doc'un paketli davranışla çelişmesi → (1) ile
      kapandı, `CLAUDE.md` dilin kaynağını da söylüyor;
    - (4) `Cargo.toml` yorumunun eksik korumayı saklaması → yorum düzeldi;
    - (5) düşüş kolunda "rc üstüne yazabilir" sözünün daralması → doc;
    - (6) göreli `HOME` → `is_absolute` süzgeci; UTF-8 olmayan `HOME`'da
      `pwd`/`$HOME` ayrışması → doc (düzeltilmedi: `HOME`'u ek ortama da
      yazmak kapsam dışı ve UTF-8 olmayan değer `String` haritaya girmez);
    - (7) gidilemeyen dizinin yoksayılması sınamasız → yeni
      `unreachable_working_directory_is_inherited` (bugünkü davranışı
      bağlıyor, ilk koşuda yeşil — alacritty değişirse kızarır);
    - (8) `chdir` exec'ten önce, `command`'ın göreli yolları yeni dizinde
      çözülüyor → `working_directory` doc'u;
    - (9) alacritty'nin en sonda sildiği `XDG_ACTIVATION_TOKEN` /
      `DESKTOP_STARTUP_ID` → `env` doc'undaki öncelik sırası.
  - **WAIVE önerisi (2):** düşüş kolunun `LC_CTYPE=UTF-8`'i Linux'ta yerel
    adı değil ve macOS `ssh_config`'i `LC_*`'ı uzak makineye taşıyor
    (`SendEnv LANG LC_*`); oradaki araçlar `setlocale` uyarısı basıp `C`'ye
    düşer. Düşüşün kendisi kullanıcı kararı (Karar 6 eki); (1)'in düzeltmesi
    kolu daralttı (Türkçe/Almanca/Fransızca dil + kendi bölgesi artık
    `LANG` alıyor), kalan kitle dil + bölge çiftinin kurulu olmadığı
    kullanıcılar (bu makine: `en` + `TR`). Doc'ta "bilinen bedel" diye
    duruyor. Alternatif (ör. kurulu bir `{dil}_XX.UTF-8`'e düşmek) ürün
    kararı, orkestratöre/kullanıcıya.
  - **WAIVE önerisi (10):** boş olmayan her `LC_ALL`/`LC_CTYPE`/`LANG`
    (`C`, kurulu olmayan bir ad) "tanımlı" sayılıyor, UTF-8 eklenmiyor.
    Kılavuz bunu açıkça istiyor ("boş olmayan bir değerle tanımlıysa →
    hiçbir şey") ve geçerlilik `setlocale`'siz sorulamaz; alacritty `C`'yi
    tanımsız sayıyor. Değişmedi, doc'ta.
  - **Atlanan (11):** `currentLocale` süreç boyunca önbellekli; bugün
    açılış başına tek oturum var, sekme/bölme seti (009) `start_session`'ı
    tekrar çağırınca `autoupdatingCurrentLocale` düşünülür.
  - **Atlanan (12):** süreli koşuların da ev dizini ve yerel alması
    "hermetik değil". Karar "istisnasız", iki betik dizine bağlı değil ve
    çıktıları ASCII; `make duman` satırı değişmedi. Ölçüm yükünde yerelin
    `printf` hızına etkisi **ölçülmedi** — phase-5 kare sayısını ölçüyor,
    yük hızını değil.
- **`/audit`: bulgu yok.** Yargı mercekleri iki tane (7, 10), ikisi de inline.
  - Temiz:
    - 1 katman: `bt-core`'da `objc2`/`NSLocale`/`/usr/share` yok (`cargo
      tree` + grep); politika ve macOS yolu `bt-shell`'de.
    - 2 bağımlılık: yalnız `bt-shell/Cargo.toml`'da feature, `Cargo.lock`
      oynamadı, karar kaydı var.
    - 3 panik yolu: eklenen `unwrap`'ların hepsi `#[cfg(test)]`'te.
    - 6 ölçüm sahipliği: notlardaki duman satırı ve `zsh` sayımı açılış
      kanıtı, performans ölçümü değil; ölçülmemiş iddia yok.
    - 7 thread: `child::*` bir kez, `didFinishLaunching` → `start_session`
      içinde, ana thread'de koşuyor. Render yoluna G/Ç girmedi;
      `Session::spawn`'ın kilit sırası değişmedi.
    - 10 belge/üslup: tanımlayıcılar İngilizce, yorumlar ve `assert!`
      iletileri Türkçe, `#[allow]` yok. Yeni modülün başında sözleşme
      yorumu var; `CLAUDE.md` ve `lib.rs` başlığı kodla uyumlu.
  - İlgisiz (diff o alana değmiyor): 4 ayar şeması, 5 shell üçlüsü, 8 boşta
    kare (animasyon/zamanlayıcı yok), 9 hücre/shader.
- **Doğrulama (kapı sonrası son hâl).** `make hepsi` → 0; `make duman` → 0
  (`kare=1 hucre=8 glif=6 kural=15 yuva=13/2048 yuk=smoke istek=2
  kapanis=clean profil=debug ornek=off pipeline=ok`); `make kur` → 0,
  ardından yukarıdaki paketli yoklamalar. `make test-yaris` → 0
  (`/simplify` sonrası).

## Yayın Etkisi

- **Davranış değişikliği — dizin:** kabuk her açılışta ev dizininde
  başlıyor: Dock/Finder'dan (önceden `/`), `cargo run` ve `make duman` dahil
  (önceden çağıranın dizini). Terminalde `cd proje && cargo run` artık o
  dizinde açmıyor; kullanıcı kararı, "istisnasız".
- **Davranış değişikliği — yerel:** süreç ortamında `LC_ALL`/`LC_CTYPE`/
  `LANG` yoksa ya da boşsa çocuğa `LANG={dil}_{bölge}.UTF-8` (kuruluysa)
  ya da `LC_CTYPE=UTF-8` gidiyor. Dil kullanıcının tercih ettiği ilk dil
  (`preferredLanguages`), bölge sistemin bölgesi. Bilinen bedel
  (kullanıcıya söylendi): Türkçe sistemde Türkçe mesajlar ve eski
  betiklerde Türkçe `I` dönüşümü. Düşüş kolunda `LC_CTYPE=UTF-8` SSH ile
  Linux'a taşınınca `setlocale` uyarısı (WAIVE önerisi, notlarda).
  Ortamında yerel olan açılışta (terminalden) değişiklik yok.
- **Kendi sürecimiz değişmedi:** `set_current_dir`, `set_var`, `setlocale`
  yok; ikisi yalnız çocuğa.
- **`TERM`/terminfo değişmedi.** Ek ortam `TERM`/`COLORTERM`'ü ezemiyor
  (sınamayla bağlı).
- **Bağımlılık:** yeni crate yok; `bt-shell/Cargo.toml`'da
  `objc2-foundation` feature listesine `NSLocale` (varsayılan sette zaten
  açıktı). `Cargo.lock` değişmedi. Karar kaydı `discussion.md` → Karar 6 eki.
- **app bundle:** `Info.plist`, entitlements, imza: el değmedi. Paketli
  açılışın davranışı değişti (yukarıda); `make kur` koştu.
- Ayar şeması, tema, shell entegrasyonu (`assets/shell/`), `.metal`: el
  değmedi. Kullanıcının rc dosyasına dokunulmuyor — `LANG` en zayıf
  değişken, rc'deki `LC_*` üstüne yazabiliyor.
- **Ölçüm bekleyen iddia yok.**
- **Belge:** `CLAUDE.md` (çocuk ortamı maddesi, katman tablosunun `bt-core`
  ve `bt-shell` satırları), `bt-shell` `lib.rs` başlığı. `docs/MIMARI.md`
  yok.
- `[elle]` göz kontrolü bekliyor (kullanıcı): gerçek Dock açılışı ve
  `ğüşıöç İ` girişi.

---

## Checklist

- [x] `SessionOptions`: çalışma dizini + ek ortam; `None` bugünkü davranış; ek ortam `TERM`/`COLORTERM`'ü ezemiyor
- [x] `bt-shell` kullanıcı oturumu `$HOME`'da başlıyor (`HOME` yoksa miras, notta) — *sapma: `HOME` yoksa passwd kaydı (`std::env::home_dir`), mutlak değilse miras; notlarda*
- [x] Yerel kararı saf fonksiyonda; ortamda yerel varsa dokunmuyor; yoksa `LANG={dil}_{ülke}.UTF-8`, o yerel yoksa `LC_CTYPE=UTF-8`
- [x] `objc2-foundation` `NSLocale` feature'ı; `Cargo.lock` değişmedi
- [x] `CLAUDE.md` çocuk ortamı maddesi güncel
- [x] **(phase-3b'den devir)** `CLAUDE.md` katman tablosunun `bt-core` satırı girdi kodlamasını (`input.rs`: DECCKM'e uyan oklar, tekerlek raporu) saymıyor — aynı commit'te ekle; gövdesi `phase-3b.md` → Uygulama Notları
- [x] Test: çalışma dizini verilince çocuğun `pwd`'si o dizin; verilmeyince miras
- [x] Test: ek ortam çocuğa ulaşıyor, `TERM`'ü ezemiyor
- [x] Test: yerel kararı — `LANG`/`LC_ALL`/`LC_CTYPE` tanımlıysa hiçbir şey; boş değer tanımsız sayılıyor; geçerli sistem yereli → `LANG`; geçersiz → `LC_CTYPE=UTF-8`
- [ ] `[elle]` göz kontrolü: `make kur`, `bateri.app`'i Dock'tan aç → `pwd` ev dizini, `echo $LANG $LC_CTYPE` beklenen değer, `ğüşıöç İ` yazınca doğru görünüyor; `cargo run` ile terminalden açınca da ev dizininde başlıyor ve kendi `LANG`'ını koruyor
- [x] Doğrulama geçti (`proje.md` → Doğrulama; `make duman` **zorunlu**; `make kur` koşar)
- [x] `/simplify` çalıştırıldı, bulgular uygulandı
- [x] `/code-review` çalıştırıldı, bulgular giderildi (12 bulgu; 8 giderildi, 2 WAIVE önerisi, 2 atlandı — notlarda)
- [x] `/audit` çalıştırıldı, bulgular giderildi (bulgu yok)
- [x] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
