# `bt-shell` ayrımı — Tartışma

Ne yapılacağı yol haritasında ve 040'ın kullanıcı kararlarında sabit (ortak
crate + macOS crate'i, katman kuralı, `make linux`'un büyümesi, İngilizce
yorum). Bu dosya **nasıl** sorularını karara bağlıyor. Kullanıcının gördüğü
hiçbir şey değişmediği için hiçbiri ürün kararı değil.

## Karar 1: Linux kolları bu sette mi? → ✅ evet, üçü; `watch` tek bildirim sözleşmesiyle ve kendi phase'inde

`make linux` `bt-shell-common`'ı koşacak. macOS gövdeleri `cfg(target_os =
"macos")` arkasında Linux'ta hiç derlenmediği için **taslak gerekmiyor**;
soru, Linux gövdelerinin bu sette mi yoksa MVP'de mi yazılacağı.

- **(a) Yalnız taşıma, Linux gövdeleri MVP'de.** Ortak crate yapısı gereği
  temiz kalır, yeni kod yok. Bedeli: gövdelerin tüketicisi (winit kabuğu)
  gelince yazılır ve `make linux` bu sette ortak crate'in Linux'ta yalnız
  *derlendiğini* kanıtlar.
- **(b) Arayüz + Linux gövdesi bu sette.** Yol haritası satırı okları bu
  set için sayıyor (`jobs`: `proc_*` → `/proc`, `child`: `NSLocale` → ortam,
  `login -flp` → `$SHELL -l`, `watch`: vnode → inotify) ve hizmetler satırı
  yalnız "MVP'de kalmayan kolları" sonraya bırakıyor. 040 Karar 7'nin
  ölçütüyle üçü de başsız Docker'da sınanabilir.

**Seçilen (b).** Sadelik jürisinin itirazı (tüketicisiz kod, `watch`'ın
şekli olay döngüsüne göre yeniden yazılır) `watch`'ta haklıydı ve cevabı
sözleşmede: bildirim **iki platformda da** modülün kendi arka plan
kuyruğundan/thread'inden geliyor, ana döngüye taşımak çağıranın. macOS'ta
çağıran bir satır (`DispatchQueue::main().exec_async`), winit'te bir
`EventLoopProxy` olayı — gövde değişmiyor. Modül başına:

- **`jobs`** — `ProcessTable`'ın ikinci gövdesi `Procfs` (`cfg(linux)`):
  çocuklar, kabuğun ön plan grubu (`stat`'ın `tpgid`'i), grubun üyeleri, ad
  (`comm`), argv (`cmdline`). Saf karar (`foreground`, `remote`) değişmez;
  gerçek PTY sınaması iki platformda koşar. Çağıranın adı platformdan
  bağımsız bir takma addan (`pane.rs:64` bugün `Libproc`'u adıyla alıyor).
- **`child`** — kabuk komutu ve ebeveyn **aynı dönüşten**:
  `shell_command() -> (Option<komut>, ShellParent)` (macOS `Login`, Linux
  `Direct`); bugün ebeveyni çağıran sabitliyor (`pane.rs:1251`) ve Linux
  kabuğu onu kopyalasa `jobs::foreground` kabuğun ilk çocuğunu kabuk sanardı.
  Linux komutu `$SHELL -l` — **alacritty paritesi değil** (0.26.0 macOS
  dışında kabuğu argümansız doğuruyor): macOS'taki login oturumuyla aynı
  başlangıç dosyası zinciri ve yol haritasının yazdığı. Yerel: `locale_env`
  sistem çiftini argüman alıyor, `NSLocale` okuması `bt-shell-macos`'ta
  kalıyor (Karar 2); Linux'ta çift yok, oturum ortamı `LANG`'ı taşıyor, yoksa
  `en_US.UTF-8` düşüşü aynı (`locale_installed`'in `/usr/share/locale`'i
  Linux'ta yerel dizini değil — o kol Linux'ta koşmuyor, doc'ta adıyla).
  Paketin betik dizini (`Contents/Resources/shell`) `cfg(macos)`; Linux
  release yolu paketleme setinin (**bilinen sınır:** Linux release
  derlemesinde entegrasyon kurulmaz), debug'daki depo kolu iki platformda.
- **`watch`** — `install(paths, notify)`, kuyruksuz. macOS gövdesi modülün
  özel seri kuyruğunda (bugünkü sınamaların `bateri.watch.test`'i gibi),
  Linux gövdesi inotify (`libc`) + izleme başına bir thread; sınamaların
  bariyeri `#[cfg(test)]` bir `flush`. Olay maskeleri bugünkü yedi sınamanın
  ölçütünü karşılar (yerinde ekleme, boşaltma, üstüne taşıma, bağın hedefi,
  okuma bildirmez); `Drop` beklemeden durdurur.
- **`keys`** olduğu gibi; platformsuz tuş sözlüğü winit setinin.

## Karar 2: Platform gövdeleri nerede? → ✅ ortak crate'te, `cfg`'li ve adlı dosyalarda; `objc2` yalnız `watch`'ın macOS gövdesinde

- **(a) Gövdeler platform crate'lerinde.** Linux gövdeleri `bt-shell-linux`'u
  bekler — o crate MVP'nin; Karar 1(b) ile çelişir ya da bugünden boş bir
  crate açar.
- **(b) Gövdeler ortak crate'te, hedefe göre** — `bt-atlas`'ın deseni
  (`coretext.rs`/`freetype.rs`, `cfg`'li bağımlılıklar). Sistem hizmeti UI
  araç takımı değil.

**Seçilen (b)**, Codebase-fit jürisinin daraltmasıyla: `NSLocale` okuması
(üç satır) `bt-shell-macos`'ta kalıyor, yani ortak crate'in macOS hedefine
tek platform bağımlılığı `dispatch2` (`watch`'ın gövdesi). `make denetim`:
`cargo tree -p bt-shell-common -e normal --depth 1`'de
`objc2-app-kit|objc2-quartz-core|objc2-foundation|objc2-user-notifications|block2|bt-shell-macos`
yok; kaynakta `objc2|dispatch2|block2` yalnız `watch`'ın macOS dosyasında
(`bt-gpu`/`bt-atlas`'ın kaynak kuralı emsali). "`bt-gpu` yukarı bağlanmıyor"
satırının `bt-shell` deseni iki yeni adı da yakalıyor.

## Karar 3: Çevirinin kapsamı → ✅ ortak crate'e taşınan modüller; yerinde kalan macOS kodu değil

Kısıt "yazılan/taşınan kodun yorumları İngilizce"; `CLAUDE.md` → Dil
istisnası `bt-shell-common`/`-linux`'u adıyla sayıyor, gerekçesi "Linux'a
açılan kod Türkçe bilmeyen katkıcıya da okunmalı". 042'nin işletme ölçütü:
yeni modüle taşınan kod taşımadan önce yerinde, ayrı bir yorum commit'inde
çevrildi; yerinde kalan çevrilmedi.

- **(a) Her şey** — `bt-shell-macos`'un ~17 bin satırlık AppKit kodu da
  (dizin adlandığı için). Linux okurunun göreceği satır yok, commit'ler
  gözden geçirilemeyecek boyda.
- **(b) Ortak crate'e giden on bir modül** (~2,3 bin yorum satırı); macOS
  kodu kendi crate'inde, aynı modül ağacında kalıyor (paket adı değişiyor),
  yalnız yeni yazılan ve dokunulan yorumlar İngilizce — `bt-shell-macos`'un
  yeniden yazılan `lib.rs` başlığı dahil.

**Seçilen (b).** Dizin adı değişimi taşıma değil yeniden adlandırma.
`.tasks/` işaretçileri Türkçe kalır (042 phase-1 notu).

## Karar 4: `app.rs`/`lib.rs`'in AppKit'siz yarısı → ✅ bu sette değil

`Run`/`Workload`, `watchdog`, duman raporu ve hükmü (`Counters`, `Measured`,
`Report`, `verdict`), `decide_inputs`, `resolve_*`, `shell_integration_env`,
`split_into_grid` AppKit'e değmiyor (Codebase-fit doğruladı), ama yol
haritası satırında yoklar ve ikinci tüketicileri (Wayland'deki duman) yok.

- **(a) Dışarıda** — MVP setinin ilk taşıması; API'nin şeklini gerçek
  tüketicisi belirler.
- **(b) İçeride, kuralla** — `pub(crate)` → `pub` bir API bugünün
  bağlamıyla kurulur; `shell_integration_env` `child`'a bağlı olduğu için
  phase sırasını da kilitler.

**Seçilen (a)** (Sadelik jürisi). Kapsam Dışı'nda MVP'nin ilk işi diye
adıyla.

## Karar 5: Phase bölümü ve davranış tanığı → ✅ beş phase

1. **Yerinde çeviri** — on bir modülün yorumları, doc'ları, `assert!`
   gerekçeleri ve tanı metinleri; kod aynı. Kanıt 042'ninki: yorumsuz fark
   yalnız dizgi değişimi.
2. **Ortak crate + taşıma** — `bt-shell-common` doğar, on bir modül taşınır;
   macOS gövdeleri `cfg(macos)`, `NSLocale` okuması `bt-shell`'e, sınama
   yardımcıları (`settings::TempRoot`, `child::SilentWake`/`wait_until`)
   `test-support` özelliğinin arkasına (`bt-atlas`'ın `fixture` emsali;
   `cfg(test)` öğesi başka crate'in sınamasında görünmez). Yeni Linux kodu
   yok. Taşınan öğelere bakan belge işaretçileri aynı commit'te.
3. **`jobs` + `child` Linux kolları + `make linux`** — `Procfs`,
   `shell_command`, `make linux` += `bt-shell-common`; macOS kolunu sabitleyen
   `cfg(macos)` sınama.
4. **`watch`: tek sözleşme + inotify** — kuyruksuz `install`, macOS'ta özel
   kuyruk ve çağıranda ana kuyruğa sarma; Linux gövdesi. En yeni ve thread'li
   kod kendi commit'inde.
5. **`bt-shell` → `bt-shell-macos`** — dizin ve paket adı, `bateri`, `make
   denetim`, sözleşme belgeleri; set kapısı.

**Davranış tanığı:** (i) macOS'ta `cargo test --workspace -- --list`'in sınama
adları (crate başlıkları ve modül önekleri soyularak) her phase'te öncekiyle
aynı — phase-3/4 yalnız ekler, `watch` sınamalarının **gövde** farkı phase-4
kabulünde ayrıca gözden geçirilir; (ii) `login_command`/betik dizininin
macOS'ta `Some` verdiğini sabitleyen sınama (duman bu yollara uğramıyor);
(iii) set sonunda `make kur` + `make duman` ve paketli uygulamada gözle
kontrol (ayar kaydı canlı uygulanıyor, ⌘W koşan işi soruyor, dock var,
`Last login` yok). `Cargo.lock` phase-2 ve phase-5'te yalnız yol üyeleri ve
kenarlarla değişiyor — `source =` satırı yok, dış crate yok; `make denetim`'in
uyarısı beklenen hâl.

## Muhakeme (2026-09-30)

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU — Linux gövdelerinin tüketicisi yok (`watch`'ın şekli olay döngüsüne bağlı); Karar 4 yol haritasını büyütüyor, API'yi tahminle kuruyor; satır sayısı yanlıştı |
| Codebase-fit | SORUNLU — kuyruksuz `install` macOS'ta yedi `watch` sınamasını kırar ve bildirim sözleşmesi platforma göre ayrışır; `cfg(test)` yardımcıları taşımada görünmez olur ve phase-2 kırmızı kalır; `ShellParent`'ı çağıran sabitliyor |
| İşletme | SORUNLU — sınama adı tanığı `watch`/`child`/`jobs`'u macOS'ta görmüyor, duman da uğramıyor; belge işaretçileri taşıma commit'inde bayatlıyor; Docker'da `child`'ın `-l -i` zsh sınamaları Debian'ın `/etc/zsh`'inden geçiyor |

**Kabul edilen itirazlar → plan değişikliği:**
- Sadelik → `watch`'ın yeniden yazılma riski → iki platformda tek bildirim
  sözleşmesi (arka plandan gelir, çağıran ana döngüye taşır), `watch` kendi
  phase'inde (Karar 1, 5).
- Sadelik → Karar 4 dışarıda, MVP'nin ilk işi (Karar 4).
- Sadelik, Codebase-fit → `context.md`'nin satır sayısı düzeltildi (~27 bin).
- Codebase-fit → `watch` sınamaları kuyruğu kaybetmiyor: macOS gövdesi özel
  kuyrukta, `#[cfg(test)]` `flush` bariyeri (Karar 1).
- Codebase-fit → `TempRoot`/`SilentWake`/`wait_until` `test-support`
  özelliğinde; `jobs`, `child`, `watch`, `settings` aynı phase'te taşınıyor
  (Karar 5).
- Codebase-fit → `ShellParent` komutla aynı dönüşten (`shell_command`).
- Codebase-fit → `NSLocale` okuması macOS crate'inde; denetim kaynak kuralı
  `bt-gpu` emsaliyle (Karar 2).
- İşletme → tanık: `cfg(macos)` sabitleyici sınama + set sonunda `make kur`,
  duman ve paketli gözle kontrol; `--list` normalizasyonu ve yalnız macOS
  listesi (Karar 5).
- İşletme → belge işaretçileri (`docs/AYARLAR.md`, `docs/OLCUMLER.md`,
  `.claude/is-akisi/olcum.md`, `CLAUDE.md`) taşıyan commit'te düzeliyor.
- İşletme → Docker'daki `-l -i` zsh sınamaları phase-3'te ilk koşuda
  gözleniyor; Debian `/etc/zsh` etkileşimi bilinen risk olarak phase'te.
- İşletme → `Cargo.lock` değişen ve son olmayan phase (2) riskli kutusu
  taşıyor; `watch`'ın thread'li gövdesi (4) paylaşılan durum olarak da.
- İşletme → `pane.rs`'in `Libproc` adı platform takma adına (phase-3).

**Reddedilenler:**
- Sadelik → Linux gövdelerinin hepsini MVP'ye bırakmak — yol haritası
  satırı okları bu sete yazıyor; `watch`'ın gerekçesi tek sözleşmeyle
  karşılandı, `jobs` ve `child` olay döngüsüne bağlı değil.
- Sadelik → adlandırmayı `bt-shell-linux` doğana kadar ertelemek — hedef
  katman kuralı `bt-shell-macos`'u adıyla sayıyor ve denetim kuralı ona
  yazılıyor; ertelemek kuralı iki kez yazdırır.
- İşletme → dizini değil yalnız paket adını değiştirmek — `crates/bt-shell/`
  altında `bt-shell-macos` paketi, `bt-shell-linux` geldiğinde yanıltıcı;
  işaretçilerin güncellemesi tek commit'lik bir iş.
- İşletme → çeviriyi iki commit'e bölmek — Karar 4 dışarıda kalınca yük on bir
  modüle indi (~2,3 bin satır), 042 phase-1'in ölçütüyle tek commit.

## Karar (2026-09-30, otonom akış)

- **Seçilen:** Karar 1–5'teki ✅'ler — ortak crate on bir modülü
  (`settings`, `split`, `zoom`, `notices`, `gesture`, `quote`, `keys`,
  `upload`, `jobs`, `child`, `watch`) taşıyor, platform gövdeleri orada
  `cfg`'li; `jobs`/`child`/`watch`'ın Linux gövdeleri bu sette, `watch` tek
  bildirim sözleşmesiyle; çeviri yalnız taşınan modüllerde;
  `app.rs`/`lib.rs`'in AppKit'siz yarısı MVP'ye; beş phase. Panelden geçmiş
  öneri (üç mercek SORUNLU, itirazlar işlendi, KIRMIZI yok).
- **Reddedilen:** her kararın (a)/(b) reddi ve Muhakeme → Reddedilenler.
