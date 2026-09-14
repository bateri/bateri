# Phase 4c — Yerel yedeği `LANG=en_US.UTF-8`

## Özet

Sistem dil + bölge çiftinin yereli kurulu değilse kabuğa `LC_CTYPE=UTF-8`
yerine `LANG=en_US.UTF-8` verilir.

_Requirements: R4.4_

---

## Neden bu phase var

phase-4b'nin `/code-review`'u WAIVE önerisi (2) olarak buldu
(`phase-4b.md` → Uygulama Notları): düşüş kolunun `LC_CTYPE=UTF-8`'i Linux'ta
yerel adı değil ve macOS `ssh_config`'i `SendEnv LANG LC_*` ile onu uzak
makineye taşıyor → orada `setlocale` uyarısı. Bu makine tam o kolda (`en` +
`TR`, `en_TR.UTF-8` yok) ve kullanıcının bugünkü terminali çocuğa
`LANG=en_US.UTF-8` veriyor. Kullanıcı 2026-09-15'te yedeği `LANG=en_US.UTF-8`
seçti (`discussion.md` → Karar 6 eki, son madde).

Sıra: kullanıcının phase-3/3b/4/4b göz kontrollerinden **sonra**, phase-5
ölçümünden **önce** (R5.1: ölçüm son koda alınır). Göz kontrolleri bu phase
yüzünden tekrarlanmaz: değişen yalnız düşüş kolunun değişkeni, UTF-8 girişi
iki hâlde de çalışır.

---

## 1. Düşüş kolu

`crates/bt-shell/src/child.rs` → yerel kararı (`locale_env` ve çevresi).

Karar sırası:

1. Ortamda boş olmayan `LC_ALL`/`LC_CTYPE`/`LANG` varsa → hiçbir şey (değişmez).
2. `{dil}_{bölge}.UTF-8` kuruluysa → `LANG={dil}_{bölge}.UTF-8` (değişmez).
3. Değilse → **`LANG=en_US.UTF-8`** (yeni). macOS'ta her zaman kurulu;
   Linux sunucuların çoğunda da var. `en_US.UTF-8` de kurulu değilse
   (beklenmez) → `LC_CTYPE=UTF-8` son çare olarak kalabilir; tutup
   tutmadığını gerekçesiyle yaz.

Neden `LANG` ve neden `en_US`: kullanıcının bugünkü terminaliyle aynı değer;
`LANG` en zayıf değişken, rc dosyası üstüne yazabilir. Doc'taki "bilinen
bedel" (SSH uyarısı) cümlesi bu yeni hâle göre düzelir; alacritty/iTerm2'nin
`LC_CTYPE=UTF-8` düşüşünden bilerek ayrıldığımızı ve nedenini yaz.

`CLAUDE.md`'deki çocuk ortamı maddesi düşüş değerini sayıyorsa aynı commit'te
düzelir.

---

## Uygulama Notları

- **Düşüş kolu.** `decide_locale`'in üçüncü kolu `LANG=en_US.UTF-8`.
  İki kol da `LANG` yazdığı için `match` bir `map → filter(installed) →
  unwrap_or_else` zincirine indi. İlk iki kol değişmedi: ortamda yerel
  varsa `None`, kurulu `{dil}_{bölge}.UTF-8` → `LANG` o ad.
- **Son çare (`en_US.UTF-8` de yoksa `LC_CTYPE=UTF-8`) tutulmadı.**
  Kılavuz bunu serbest bırakmıştı. Gerekçe:
  - `bt-shell` yalnız macOS'ta derleniyor (AppKit).
  - `/usr/share/locale/en_US.UTF-8` sistemle geliyor ve `/` salt okunur
    bir birimde (`diskutil info /` → `Volume Read-Only: Yes`).
  - Dal hiç koşmayacaktı; bir `installed` sorgusu, bir sınama ve bir doc
    paragrafı getirirdi.
  - Ek olgu (`/simplify`'ın seviye merceği, elle doğrulandı):
    `en_US.UTF-8/LC_CTYPE` → `../C.UTF-8/LC_CTYPE` bağı, o da
    `UTF-8/LC_CTYPE` ile aynı inode. İki adın karakter sınıfı aynı dosya.
  - `en_US.UTF-8`'in kurulu olup olmadığı sorulmuyor. `installed` yalnız
    sistem çifti için çağrılıyor.
- **Doc'lar (`child.rs`):**
  - 3. kol yeni değeri söylüyor.
  - "Düşüş kolunda bu söz daralıyor" paragrafı kalktı: iki yazan kol da
    `LANG`, yani "rc üstüne yazabilir" sözü artık tam tutuyor.
  - "Bilinen bedel" paragrafının yerine ayrılma gerekçesi geldi: alacritty
    ve iTerm2'nin `LC_CTYPE=UTF-8` düşüşünden neden ayrıldığımız.
    `SendEnv LANG LC_*` bu makinenin `/etc/ssh/ssh_config`'inde (satır 55)
    okundu.
  - `locale_env` doc'unun geçmiş anlatısındaki `LC_CTYPE=UTF-8` "düşüş
    yereli" oldu.
- **`CLAUDE.md`:** çocuk ortamı maddesi yeni düşüşü ve SSH gerekçesini
  tek cümleyle söylüyor, ayrıntı için `child::decide_locale` doc'una
  işaret ediyor (`/simplify`: aynı gerekçe iki yerde kayardı).
- **Test-first.**
  - `missing_system_locale_falls_back_to_utf8_ctype` ve
    `locale_without_region_falls_back_to_utf8_ctype` adları `…_en_us_lang`
    oldu, beklentileri `LANG=en_US.UTF-8`.
  - Eski koda karşı ikisi de kırmızı düştü:
    `left: Some(("LC_CTYPE", "UTF-8")) right: Some(("LANG", "en_US.UTF-8"))`.
  - Diğer altı sınama dokunulmadan yeşil kaldı (ilk iki kol).
- **Mutasyonlar (üçü de kızardı, dosya geri yüklendi):**
  - yedek `en_GB.UTF-8`: iki düşüş sınaması;
  - `.filter(installed)` silindi: `missing_system_locale_falls_back_to_en_us_lang`
    (sonuç `en_TR.UTF-8` olurdu);
  - eski `match` (`LC_CTYPE=UTF-8` kolu) geri kondu: iki düşüş sınaması.
- **`make duman` gürültüsü.**
  - İlk üç koşu `glif=7/9/8` verdi (`kare=3/4/3`, `yuva=14/16/15`).
  - Bu ortamda `LANG=en_US.UTF-8` tanımlı, yani karar 1. koldan `None`
    dönüyor ve değişen kol hiç koşmuyor. Değişiklikten koda giden yol yok.
  - Taban: değişiklik `git stash`'lenip HEAD'de koşuldu → `glif=6`. Geri
    alınınca iki koşu daha → ikisi de `glif=6`.
  - Neden ayrıştırılmadı. Olası açıklama: koşu sırasında öne çıkan pencereye
    düşen tuş vuruşları (sayılar koşudan koşuya değişiyor). Kanıtlanmadı.
- **Paketli açılış kanıtı (ölçüm değil).**
  - Yöntem phase-4b'ninki. Yoklayıcı `scratchpad/4c/probe.sh` kabuğun
    yerine geçiyor (`--env SHELL=…`). `pwd -P`, `LANG`, `LC_ALL`,
    `LC_CTYPE`, `locale charmap` ve `env`'i dosyaya yazıp çıkıyor.
  - Komut, `make kur`'dan sonra ve `cwd=/tmp`'den: `env -i HOME USER LOGNAME
    PATH TMPDIR open -W -n --env SHELL=probe.sh target/release/bateri.app`.
    Öncesinde `pgrep -lf bateri` boştu, sonrasında da boş.
  - **Bu makine** (`AppleLanguages ("en-TR")`, `AppleLocale en_TR`):
    `pwd=/Users/kalaomer`, `LANG=en_US.UTF-8`, `LC_ALL` ve `LC_CTYPE`
    tanımsız, `charmap=UTF-8`. phase-4b'de aynı komut `LC_CTYPE=UTF-8`
    vermişti.
  - **2. kol** (`--args -AppleLanguages (tr-TR) -AppleLocale tr_TR`):
    `LANG=tr_TR.UTF-8`, `LC_CTYPE` tanımsız, `pwd=/Users/kalaomer`.
  - **1. kol** (ortamda ve `--env` ile `LANG=tr_TR.UTF-8`):
    `LANG=tr_TR.UTF-8`'e dokunulmadı, `pwd=/Users/kalaomer`.
  - **Komut kazası.** İlk 2. ve 1. kol koşuları `pwd=/` verdi. Ortam zsh'ta
    tırnaksız `$E` ile verilmişti, zsh sözcüklere bölmüyor. Uygulama `HOME`
    olarak `"/Users/kalaomer USER=… TMPDIR=…"` aldı: mutlak ama var olmayan
    bir dizin. `chdir` düştü, çocuk miras aldı. Bu, phase-4b'nin belgelediği
    "gidilemeyen dizin yoksayılır" davranışı. Çocuğun `HOME`'u yine de
    doğru göründü, çünkü `login -flp` onu passwd'den yeniden yazıyor (ayrı
    bir yoklamayla doğrulanmadı). Koşular tırnaklı değişkenlerle
    tekrarlandı (yukarıdaki sonuçlar). Bozuk dosyalar `scratchpad/4c/bad/`'da.
  - **Gösterdiği:** LaunchServices yolunda (`__CFBundleIdentifier`,
    `XPC_SERVICE_NAME` çocukta var) bu makinenin düşüş kolu
    `LANG=en_US.UTF-8` veriyor, `login -flp` onu koruyor ve kabuk UTF-8
    karakter kümesinde (`locale charmap`).
  - **Göstermediği:**
    - gerçek Dock/Finder açılışı: `env -i` launchd ortamının yaklaşığı;
    - SSH ile Linux'a gidişte uyarının kalktığı: Linux makine yok, yalnız
      `ssh_config` satırı okundu;
    - `ğüşıöç İ` girişinin ekranda doğru görünmesi.
  - `[elle]` gerekmiyor: phase-4b'nin göz kontrolü kapsıyor.
- **`/simplify` (4 mercek).**
  - Verimlilik: temiz. `installed` sorgusu yine en çok bir kez.
  - Uygulandı:
    - `CLAUDE.md` gerekçeyi ikinci kez yazmıyordu artık: tek cümle +
      `child::decide_locale` doc'una işaretçi (reuse + sadeleştirme).
    - Doc'taki "üç kolda da" → "yazan iki kolda da" (1. kol hiçbir şey
      yazmıyor).
    - Son çare paragrafına `LC_CTYPE` dosyasının ortaklığı eklendi
      (seviye). Merceğin "koşsa da kurtarmazdı" çıkarımı **alınmadı**:
      yalnız `en_US.UTF-8` dizini kalksa `UTF-8` yaşardı.
  - Atlandı: `decide_locale` → `Option<String>`, `"LANG"` `locale_env`'de.
    Anahtar kararın parçası (`LC_ALL` değil `LANG`) ve sınanan saf
    fonksiyonda kalmalı. Taşınsaydı sınanmayan sarmalayıcıya düşerdi ve
    yukarıdaki üçüncü mutasyonu hiçbir sınama yakalamazdı.
- **`/code-review` (10 bulgu).** Skill olarak koştu (arka plan). Aralığı
  `@{upstream}...HEAD` + çalışma ağacı, yani 006'nın yayımlanmamış 25
  commit'i. İkisi 4c diff'ine değiyor:
  - Kısmen giderildi: (4) `en_US.UTF-8` yalnız karakter sınıfını değil
    bütün kategorileri ABD'ye çekiyor, ve `en_US.UTF-8`'i olmayan sunucuda
    SSH uyarısı yine çıkıyor. Doc bu iki bedeli artık yazıyor (sıralama;
    "kurulu olmayanda uyarı yine çıkar").
  - **WAIVE önerisi (4):** reviewer'ın önerdiği `C.UTF-8`'i `en_US`'den
    önce denemek. Yedeğin değeri kullanıcı kararı (Karar 6 eki, son madde:
    `LANG=en_US.UTF-8`); değiştirmek orkestratör/kullanıcı işi.
    `/usr/share/locale/C.UTF-8` bu makinede var. macOS 14'te varlığı
    doğrulanmadı.
  - **WAIVE önerisi (5):** dil `preferredLanguages`'ın yalnız dil alt
    etiketinden alınıyor, etiketin kendi bölgesi atılıyor. Örnek: `en-GB` +
    bölge `TR` → `en_TR` yok → `en_US.UTF-8`, oysa `en_GB.UTF-8` kurulu.
    Bu 2. kolun mantığı (phase-4b), 4c'nin kapsamı (yalnız düşüş kolu)
    dışında. Bu makinede sonucu değiştirmez (`en-TR` → `en_TR` yok).
    `{dil}_{etiket bölgesi}` ara kolu ürün kararı. Düşüş artık `en_US`
    olduğu için etkisi eskisinden görünür: ABD tarih biçimi.
  - **Kapsam dışı, 4c diff'ine değmiyor, doğrulanmadı** (önceki phase
    commit'leri; karar orkestratörde):
    - (1) `bt-core` `session.rs:864`: seçim `INVERSE` ile OR'lanıyor, ters
      videolu hücrede seçim görünmüyor.
    - (2) `session.rs:1319`: girdi seçimi temizlemiyor, eski seçim kayarak
      kopyalanıyor.
    - (3) `bt-shell` `keys.rs:86`: Shift+Tab ham `0x19`, fn+Backspace
      yutuluyor.
    - (6) `session.rs:1376`: 2004 kapalıyken `\r\n` dönüşümü yok (phase-2'de
      waive'li).
    - (7) `session.rs:1319`: her tuş `Term` kilidini alıyor.
    - (8) `clipboard.rs:79`: sınama pasteboard'ları `releaseGlobally`'siz.
    - (9) `Makefile:94`: boş `TARGET_DIR` denetimi yok.
    - (10) `.tasks/README.md:10`: 006 satırı tablodan kopuk ve notu bayat.
- **`/audit`: bulgu yok.**
  - Temiz: 1 katman, 2 bağımlılık (`Cargo.toml`/`Cargo.lock` diff'i boş),
    3 panik yolu (eklenen satırda `unwrap`/`set_var`/`setlocale` yok),
    6 ölçüm sahipliği, 7 thread (inline: çağrı yeri ve G/Ç sayısı aynı),
    10 üslup (inline: sınama adları İngilizce, yorumlar Türkçe, `#[allow]`
    yok; depoda kalan `LC_CTYPE=UTF-8` anmalarının hepsi "bu değil"
    cümlesi).
  - İlgisiz: 4 ayar şeması, 5 shell üçlüsü, 8 boşta kare, 9 hücre/shader.
- **Doğrulama (kapı sonrası son hâl).**
  - `make hepsi` → 0.
  - `make duman` → 0. İlk koşu `kare=3 hucre=8 glif=7 kural=15 yuva=14/2048
    yuk=smoke istek=5 kapanis=clean profil=debug ornek=off pipeline=ok`
    verdi, yani yukarıdaki gürültü yine görüldü.
  - Ardından A/B koşuldu: çalışma ağacında üç koşu, `git stash` ile HEAD'de
    üç koşu. Altısı da aynı satırı verdi: `kare=1 hucre=8 glif=6 kural=15
    yuva=13/2048 yuk=smoke istek=3 kapanis=clean profil=debug ornek=off
    pipeline=ok`.
  - `make kur` → 0, ardından paketli yoklama bir kez daha: `pwd=/Users/kalaomer`,
    `LANG=en_US.UTF-8`, `LC_CTYPE` tanımsız, `charmap=UTF-8`.
  - `make test-yaris` gerekmedi: PTY/render/paylaşılan duruma dokunulmadı,
    yalnız açılışta bir kez koşan saf karar. `make shader` gerekmedi.

## Yayın Etkisi

- **Davranış değişikliği — yalnız düşüş kolu.** Ortamda yerel yokken ve
  sistemin `{dil}_{bölge}.UTF-8`'i kurulu değilken (bölgesiz yerel dahil)
  kabuk `LC_CTYPE=UTF-8` yerine `LANG=en_US.UTF-8` alıyor. Etkilenen
  açılışlar:
  - Dock/Finder: bu makine (`en` + `TR`) tam bu kolda, paketli yoklamayla
    görüldü;
  - `cargo run`, `make duman`: yalnız ortamda yerel **yoksa**.
- **Değişmeyenler.** Ortamda yerel varsa ya da sistem çifti kuruluysa
  davranış aynı (sınamalar ve paketli yoklama). Terminalden açılış
  (ortamda `LANG`) etkilenmiyor.
- **Kullanıcıya görünen fark (düşüş kolunda):**
  - Tarih, sayı ve sıralama `en_US`'ninki; önceden `C`/POSIX'ti.
  - Mesajlar İngilizce (önceden de).
  - rc'de yalnız `LANG` değiştiren kullanıcının karakter sınıfı artık
    kilitli değil.
- **SSH:** `SendEnv LANG LC_*` artık Linux'a geçerli bir yerel adı
  taşıyor. `en_US.UTF-8`'i olmayan sunucuda uyarı yine çıkar. Linux'ta
  sınanmadı.
- **Kendi sürecimiz değişmedi:** `set_var`, `set_current_dir`, `setlocale`
  yok.
- **`TERM`/terminfo değişmedi.**
- **Bağımlılık:** yok. `Cargo.toml`/`Cargo.lock` değişmedi.
- **app bundle:** `Info.plist`, entitlements, imza: el değmedi. Paketli
  açılışın yereli değişti (yukarıda); `make kur` koştu.
- Ayar şeması, tema, shell entegrasyonu (`assets/shell/`), `.metal`: el
  değmedi. Kullanıcının rc dosyasına dokunulmuyor.
- **Ölçüm bekleyen iddia yok.** phase-5'in ölçümü son koda alınacak (R5.1).
  Bu phase kare yoluna dokunmuyor.
- **Belge:** `CLAUDE.md` (çocuk ortamı maddesi), `child.rs` doc'ları.
  `bt-shell` `lib.rs` başlığı düşüş değerini saymıyor, değişmedi.
  `docs/MIMARI.md` yok.
- `[elle]` yok: phase-4b'nin göz kontrolü kapsıyor.

---

## Checklist

- [x] Düşüş kolu `LANG=en_US.UTF-8`; ilk iki kol değişmedi — son çare (`LC_CTYPE=UTF-8`) tutulmadı, gerekçe Uygulama Notları'nda
- [x] Doc'lar ve `CLAUDE.md` yeni düşüşü söylüyor (SSH gerekçesi dahil) — gerekçenin gövdesi `decide_locale` doc'unda, `CLAUDE.md` tek cümle + işaretçi
- [x] Test: kurulu olmayan dil + bölge → `LANG=en_US.UTF-8` (önce eski beklentiyle kırmızı gör) — `missing_system_locale_falls_back_to_en_us_lang`, `locale_without_region_falls_back_to_en_us_lang`; eski koda karşı kırmızı, üç mutasyon kızardı
- [x] Test: ortamda yerel varsa ve kurulu çift varsa davranış değişmedi (mevcut sınamalar yeşil) — dokunulmadan yeşil
- [x] Paketli açılış yoklaması: bu makinede (`en` + `TR`) çocuk `LANG=en_US.UTF-8` görüyor — kapı sonrası son `make kur` ile; 2. ve 1. kol da yoklandı (notlar)
- [x] `[elle]` yok — phase-4b'nin göz kontrolü kapsıyor (UTF-8 girişi iki hâlde de çalışır)
- [x] Doğrulama geçti (`proje.md` → Doğrulama; `make duman` **zorunlu**; `make kur` koşar) — kapı sonrası: `make hepsi` 0 · `make duman` 0 (`kare=1 hucre=8 glif=6 kural=15 yuva=13/2048 yuk=smoke istek=3 kapanis=clean profil=debug ornek=off pipeline=ok`; bir koşuda `glif=7` gürültüsü, A/B notlarda) · `make kur` 0 · `make test-yaris`/`make shader` gerekmedi
- [x] `/simplify` çalıştırıldı, bulgular uygulandı — 3 uygulandı, 1 atlandı (gerekçe notlarda)
- [x] `/code-review` çalıştırıldı, bulgular giderildi — 4c'ye değen (4) doc'ta kısmen giderildi + WAIVE önerisi, (5) WAIVE önerisi; kalan 8'i önceki phase'lere ait, kapsam dışı (notlar)
- [x] `/audit` çalıştırıldı, bulgular giderildi — bulgu yok
- [x] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [x] Commit: aa6b3a0
