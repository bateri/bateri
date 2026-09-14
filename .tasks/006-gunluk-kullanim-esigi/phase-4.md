# Phase 4 — Bundle: `.app`, `make kur`, attribution

## Özet

bateri Dock'tan açılan, öne çıkabilen bir uygulama olur; lisans borcu kapanır.

_Requirements: R4, R4.1, R4.2, R4.3, R6.1, R6.2_

---

## 1. En küçük çalışan bundle

`crates/bateri/` + `Makefile` + `assets/` — `Info.plist`, ikon, gerçek
`make kur` (bugün `Makefile:76` "henüz yok"). `BT_RUN_SECONDS` yolu
bundle'lı açılışta da aynen çalışır; duman reçetesi değişmez.

Girmeyenler (Karar 6): imza, notarization, Sparkle. İmzasız `.app` ilk
açılışta Gatekeeper'a takılır — **tek seferlik** onay, her açılışta sağ-tık
değil. Bu gerçek eşik tanımına yazılır.

## 2. Attribution içerik denetimiyle

002'nin borcu: `alacritty_terminal` Apache-2.0, "lisans metni ve attribution
paneli **bundle**" (`.tasks/002-vt-motoru/teslim.md:54-56`). Eksik kalırsa
hiçbir kapı kızarmaz — sessiz lisans ihlali (işletme 3). O yüzden bundle
fazına **içerik denetimi** konur: Info.plist + lisans dosyası varlığı.

---

## Uygulama Notları

- **Yerleşim.** Girdiler `assets/bundle/`: `Info.plist.in` (şablon),
  `bateri.png` (1024 px ikon kaynağı), `Credits.html`,
  `THIRD-PARTY-LICENSES.txt`. Ürün `target/release/bateri.app` (hedef dizini
  `cargo metadata`'dan; `build.target` üçlüsü kapsam dışı). `.icns` ve
  `.iconset` `make kur`'da
  türüyor ve depoya girmiyor (`.gitignore`). `crates/bateri/Cargo.toml`
  **değişmedi** — paket metadatası gerekmedi.
- **Tek kaynak.** Şablonda iki yer tutucu: `@VERSION@` ← `cargo pkgid -p
  bateri`, `@MACOS_MIN@` ← binary'nin `minos`'u (`vtool -show-build`), o da
  `.cargo/config.toml`'un `[env]`'inden. İlk taslak TOML'u sed'le okuyordu;
  `/code-review` gösterdi ki `[env]` `force`suz ve kabukta ihraç edilmiş bir
  `MACOSX_DEPLOYMENT_TARGET` rustc için onu eziyor — plist ile binary
  ayrışırdı. Binary'den okuyunca ayrışamıyor, TOML ayrıştırması da kalktı.
  Şablondaki açıklayıcı XML yorumu da kalktı: yer tutucu adlarını andığı için
  `sed` onu da doldurup ürün plist'ine sızdırıyordu. İlk taslak make'te
  `unterminated call` ile düştü: `#` satırın **içinde** de yorum başı; sed
  ayırıcıyı adıyla değil karakter sınıfının tersiyle siliyor.
- **İçerik denetimi iki katman (sapma: kılavuz tek `Test:` diyor).**
  1. `crates/bateri/src/bundle_assets.rs` (bin'in `#[cfg(test)]` modülü) —
     `make hepsi`'de, **girdiler**: şablon geçerli plist,
     `CFBundleExecutable` bin hedefinin adıyla aynı (`CARGO_BIN_NAME`),
     kimlik/ikon/`NSHighResolutionCapable` yerinde,
     sürüm ve taban **yer tutucu** (düz sayı kırmızı), lisans metni ve atıf
     `alacritty_terminal` + Apache 2.0 taşıyor. Test-first: dosyalar yokken
     3/3 kırmızı, eklenince yeşil. Mutasyon: atıf silinince, şablona düz
     `14.0` yazılınca, `CFBundleExecutable` `bateri2` olunca — üçü de kırmızı.
  2. `make kur` — **ürün**, hazırlık dizininde (`bateri.app.partial`) ve
     yalnız geçerse `bateri.app`'in yerine taşınarak: plist geçerli ve yer
     tutucusuz, plist'in adını verdiği çalıştırılabilir **dosya** pakette,
     `LSMinimumSystemVersion` o binary'nin `minos`'uyla aynı, ikon pakette,
     `Credits.html` ve lisans metni girdiyle bayt bayt aynı (`cmp`; içeriğin
     sahibi 1). Derlemeden önce iki koruma: hedef yolunda boşluk yok (tarif
     yolları bölerdi), sürüm `CFBundleVersion` biçiminde (en çok üç sayı).
     Mutasyon: lisans kopyalanmayınca (eski paket yerinde kaldı), sürüm yer
     tutucusu doldurulmayınca, plist tabanı `13.0` yazılınca, ikon
     üretilmeyince, `CFBundleExecutable` boşken (önceki `test -x` dizine
     düşüp yeşil geçiyordu), sürüm `0.2.0-alpha.1` ve `1.2.3.4` iken — hepsi
     `kur: …` ile kırmızı.
  Neden iki: girdi yerindeyken kopya satırı silinirse (1) yeşil kalır; (2)
  onu görür ama yalnız `make kur`'da koşar. Denetimin listesi kopya
  satırlarından **ayrı** yazılı — aynı değişkenden okusaydı kopyadan silinen
  dosya denetimden de silinirdi. `proje.md` Doğrulama tablosuna `make kur`
  satırı eklendi (tetik: `assets/bundle/*`, `crates/bateri`, `kur` hedefi).
  `/simplify` iki katmanı korudu ama örtüşmeyi kaldırdı: sınama önce
  `tests/` altındaydı, cargo her `make hepsi`'de uygulama binary'sini ayrıca
  bağlıyordu → bin'in birim test demetine taşındı.
- **"Panel" = AppKit'in standart About paneli (sapma).** Panel
  `Credits.html`'i `Resources`'tan kendiliğinden okur; paneli açan menü
  öğesi yok (menü 00X, Karar 2 (a)) → panel bugün **erişilemez** ve gözle
  hiç görülmedi. Menü eklemek bu setin kapsamı dışı. Lisans yükümlülüğü
  paketteki metinle karşılanıyor (Apache-2.0 §4(a): lisans kopyası;
  `alacritty_terminal`'da `NOTICE` yok, §4(d) doğmuyor). Borç
  `docs/YOL-HARITASI.md` → sete bağlanmamış borçlar (→ menü).
- **Lisans metni birebir.** `THIRD-PARTY-LICENSES.txt` = İngilizce başlık
  (ad, depo, "Copyright 2020 The Alacritty Project") + crate'in
  `LICENSE-APACHE`'i (`diff` ile birebir). Crate sürümü yazılmadı: `Cargo.lock`
  güncellenince sessizce eskirdi, Apache 2.0 metni sürümle değişmiyor.
- **Attribution kapsamı yalnız `alacritty_terminal`** (002'nin kaydı).
  `cargo metadata --filter-platform aarch64-apple-darwin` diğer dış
  paketlerin çoğunun MIT ya da MIT seçeneği taşıdığını gösterdi (`objc2`,
  `objc2-foundation`, `objc2-encode`, `block2` yalnız MIT); MIT de bildirimin
  kopyalarla gitmesini istiyor. Dağıtım işi → `YOL-HARITASI` borcu.
- **İkon yer tutucu.** Koyu yuvarlatılmış kare + `>` + blok imleç; tasarım
  kararı değil. Scratchpad'de tek seferlik bir stdlib Python betiğiyle
  (zlib + struct) çizildi; betik depoya girmedi, kaynak PNG'nin kendisi.
  `sips -s format icns` 1024'lük PNG'yi reddetti (çıkış 13) → `sips -z` ile
  on boyutlu `.iconset` + `iconutil`. Reçetede Python yok.
- **`CFBundleIdentifier` = `io.github.bateri.bateri` — plan'da yok, karar.**
  Uzak depodan (`github.com/bateri/bateri`) türetildi. Tercihler ve kayıtlı
  pencere durumu bu kimliğe bağlanır: dağıtımdan önce değiştirmek ucuz, sonra
  değil. **TCC izinleri bağlanmaz** (ilk taslağın iddiası yanlıştı,
  `/code-review`): paket imzasız, linker imzasının kimliği `bateri-<özet>` ve
  imzasız kodda TCC kaydı kod özetine düşer — yani her `make kur` sonrası
  verilen izinler (Tam Disk Erişimi, Belgeler) yeniden sorulabilir. Ekranda
  **doğrulanmadı**; kalıcı çaresi imza, dağıtım setinin işi.
- **`NSHighResolutionCapable = true`** eklendi (kılavuzda yok): GPU'nun
  çizdiği terminal Retina'da bulanık açılmasın.
- **On `NS*UsageDescription` anahtarı eklendi (kılavuzda yok,
  `/code-review`).** Dock'tan açılınca kabuğun çocuklarının "sorumlu
  uygulaması" artık bateri; amaç dizgisi olmayan uygulamada TCC isteği
  (`osascript` → AppleEvents, `ffmpeg` → kamera/mikrofon) sormadan
  reddedilir. Emsal alacritty'nin paketindeki aynı küme. Etkisi **gözle
  doğrulanmadı**; dizgiler ("A program running in bateri …") ürün metni,
  değişebilir.
- **İmza yok ve açılış için gerekmedi.** Paket linker'ın ad-hoc imzasıyla
  (`flags=adhoc,linker-signed`, `Info.plist=not bound`) hem doğrudan hem
  `open` ile açıldı. `codesign -vv`: "code has no resources but signature
  indicates they must be present" — imzasız paketin beklenen hâli.
  `codesign -s -` reçeteye girmedi, R4.3'e dokunulmadı.
- **Gatekeeper beklentisi yerel kopyada yanlış (sapma, `[elle]` maddesi
  düzeltildi).** `xattr -l target/release/bateri.app` → yalnız
  `com.apple.provenance`; `com.apple.quarantine` **yok** → Gatekeeper yerel
  `make kur` kopyasında hiç sormaz. Dağıtılan (quarantine'li) kopya için:
  `spctl -a -vv -t exec` imzasız pakette "code has no resources…" (geçersiz
  imza) diyor; scratchpad'de **ad-hoc imzalanan bir kopyada** `codesign -vv`
  "valid on disk", `spctl` "rejected" (geçerli ama notarize değil). Bu
  ayrımın ekrandaki karşılığı **doğrulanmadı**: quarantine'li kopyayı açmak
  kullanıcının ekranına modal diyalog düşürür, otonom koşuda yapılmadı. Yani
  R4.3'ün "tek seferlik onay" cümlesi bugünkü paket için **kanıtlı değil**;
  `plan.md → Hedef`'e bu hâliyle yazıldı, gerçek davranışı dağıtım seti
  görmeli (`YOL-HARITASI` borcu).
- **Dock açılışında kabuğun başladığı yer (kapsam dışı bulgu).**
  LaunchServices süreci `cwd=/` ile başlatıyor — scratchpad'de `open` ile
  açılan bir probe paket `cwd=/` bastı. `bt-core` `tty::Options`'a
  `working_directory` vermiyor ve alacritty macOS'ta `login -flp` kullanıyor
  (`-l` ev dizinine geçmez) → Dock'tan açılan bateri'nin kabuğu büyük
  olasılıkla `/`'da açılıyor; `cargo run` bunu göstermez (çağıranın dizinini
  miras alır). Ortam da farklı: `open` çağıranın ortamını geçiriyor (probe
  kabuğun değişkenlerini gördü), Dock launchd'ninkini verir — orada `LANG`
  olup olmadığı **doğrulanmadı**; yoksa UTF-8 girişi bozulur. İkisinin yeri
  `bt-core` (`Session::spawn`), bu phase'in kapsamı dışında; `[elle]`
  maddesine gözlem olarak eklendi, karar orkestratörde. **WAIVE önerisi
  (`/code-review` bulgusu):** düzeltme `Session::spawn`'da iki satır
  (`working_directory` yoksa `$HOME`, `LANG` yoksa bir UTF-8 yereli) ama
  iki ürün kararı taşıyor — `cargo run`'ın çağıran dizinini miras alması
  korunacak mı, hangi yerel — ve phase dosyası `bt-core`'a dokunmuyor.
  Öneri: phase-5'ten önce küçük bir izleme (phase-4b) ya da 007 (ayarlar);
  `[elle]` gözlemi sonucu netleştirir.
- **Paketli açılış kanıtı (ölçüm değil; kapı sonrası son hâlde).**
  `BT_RUN_SECONDS=3 target/release/bateri.app/Contents/MacOS/bateri` → exit
  0 ve `kare=2 hucre=8 glif=6 kural=15 yuva=13/2048 yuk=smoke istek=3
  kapanis=clean profil=release ornek=off pipeline=ok`; `open -W -n --env
  BT_RUN_SECONDS=3 --stdout … target/release/bateri.app` → aynı satır
  `kare=3 … istek=4` ile; ikisinden sonra `pgrep -fl bateri` boş. `open`'ın
  çıkış kodu uygulamanınki değil (hep 0). Duman
  reçetesi değişmedi. `IDLE_FRAME_LIMIT` ölçümü phase-5'in; nasıl açılacağı
  ve tuzakları phase-5'in checklist'ine devredildi.
- **`/simplify` (4 mercek).** Uygulanan: `ICONSET` değişkeni ve `key()`
  yardımcısı (tekrar), taban denetiminin binary `minos`'una bağlanması ve
  kopya denetiminin `cmp`'ye inmesi (seviye), sınamanın bin demetine
  taşınması (verimlilik), belge tekrarlarının kısalması — `proje.md` satırı
  `Makefile` yorumuna işaret ediyor, `plan.md → Hedef` tek cümle. Atlanan:
  `VERSION`/`MACOS_MIN`'in `$(eval)` ile ezberlenmesi (tekrarlı `cargo pkgid`
  release derlemesinin yanında gürültü; hile okunaklılığı düşürürdü), ikonun
  dosya hedefiyle önbelleklenmesi (`kur` sıcak yol değil), `plutil`
  çağrılarının tek `PlistBuddy`'ye toplanması (gürültü; ezberleme sonra
  `/code-review`'da hedef dizini `cargo metadata`'ya taşınınca gerekli oldu,
  bkz. aşağısı). Yeniden kullanım
  merceği temiz döndü. `plan.md`'nin R4.3 satırı **değiştirilmedi**
  (Gereksinimler'e dokunmak orkestratörün kararı); Hedef cümlesi onun
  doğrulanan hâlini taşıyor.
- **`/code-review` (14 bulgu).** Giderilen: hazırlık dizini (düşen koşu
  yarım paket bırakıyordu), hedef dizini `cargo metadata`'dan (yalnız
  `CARGO_TARGET_DIR`'a bakmak `build.target-dir` altında bayat binary
  paketlerdi) + `$(eval)` ezberlemesi (`$(APP)` tarifte yirmiden fazla
  açılıyor; tek sefer ve tembel olduğu scratchpad'de make 3.81 ile sınandı),
  `test -x`'in dizine düşmesi, boşluklu yol koruması, `CFBundleVersion`
  biçimi, tabanın binary'den türemesi (çoklu `minos` satırında ilki), şablon
  yorumunun ürüne sızması, ikon adının tek kaynağa (`CFBundleIconFile`)
  inmesi, `NS*UsageDescription`, TCC notunun düzeltilmesi, CLAUDE.md'de
  MIT boşluğunun görünür kılınması (`THIRD-PARTY-LICENSES.txt` başlığı
  "among others"), checklist tutarlılığı (attribution kutusu `[~]`, 002'nin
  B.1'i `[x]`). WAIVE önerisi: `cwd=/` + `LANG` (yukarıda). Değiştirilmedi:
  `.cargo/config.toml`'a `force = true` — taban binary'den okununca paket
  için gereksiz kaldı; env'in `[env]`'i ezmesi cargo'nun belgelenmiş
  davranışı ve `build.rs`'e dokunan bir karar. `.tasks/README.md`'nin 006
  satırı (bayat "henüz yok") kapanışın (adım 10) işi.
- **`/audit`: bulgu yok.** Temiz: 1 katman (`cargo tree` + grep), 2 yeni
  bağımlılık (`Cargo.toml`/`Cargo.lock` oynamadı), 6 ölçüm sahipliği
  (notlardaki jeton satırları "ölçüm değil" etiketli açılış kanıtı;
  `IDLE_FRAME_LIMIT` için veri sayılmaz), 10 belge/üslup (tanımlayıcılar
  İngilizce; yorum, `assert!` iletisi ve `kur:` tanı metni Türkçe; plist
  değerleri ve paket metinleri İngilizce; `#[allow]` yok). İlgisiz (diff o
  alana değmiyor): 3 panik yolu, 4 ayar şeması, 5 shell üçlüsü, 7 thread,
  8 boşta kare, 9 hücre/shader. Tek yargı merceği (10) inline koştu.

## Yayın Etkisi

- **app bundle** (`proje.md` → Yayın etkisi): `make kur` gerçek →
  `target/release/bateri.app`. `Info.plist` (şablon + iki yer tutucu),
  ikon (yer tutucu), `Credits.html`, `THIRD-PARTY-LICENSES.txt`. Yeni kimlik
  `io.github.bateri.bateri` (tercih anahtarı; TCC değil — notlar). On
  `NS*UsageDescription` anahtarı (çocuk süreçlerin TCC istekleri için).
  Entitlements yok; imza/notarization yok (linker ad-hoc imzası,
  `codesign -vv` paketi reddeder). Kurulum hazırlık dizininden taşınarak.
- `Makefile`: `kur` gerçek oldu, "henüz yok" bloğundan çıktı; `proje.md`
  başındaki listeden çıktı, Doğrulama tablosuna `make kur` satırı ve
  "Depoya girmeyenler"e `*.icns`/`*.iconset/` eklendi; `.gitignore` aynı.
- `CLAUDE.md`: taban cümlesi ("ileride" → `make kur`), bağımlılık maddesine
  Apache-2.0 atıf sözleşmesi, `make kur` komut satırı. `.cargo/config.toml`
  yorumu. `docs/YOL-HARITASI.md`: iki borç (About paneli erişimi; kalan
  üçüncü taraf bildirimleri + Gatekeeper'ın gerçek davranışı).
  `.tasks/002-vt-motoru/teslim.md` B.1 ve `.tasks/README.md`'nin 002 satırı:
  attribution kapandı notu.
- Yeni bağımlılık yok, `Cargo.lock` oynamadı. Ölçüm bekleyen iddia yok
  (`IDLE_FRAME_LIMIT` phase-5'te).

---

## Checklist

- [x] `Info.plist` + ikon + gerçek `make kur` (ikon yer tutucu; sürüm ve taban tek kaynaklarından)
- [~] Attribution: lisans metni + panel; içerik denetimi (dosya varlığı) — lisans metni, atıf ve iki katmanlı denetim **tamam**; panel yalnız içerik olarak pakette (`Credits.html`), onu açan menü öğesi menü setine (00X) devredildi ve panel hiç görülmedi (notlar, `YOL-HARITASI` borcu)
- [x] `BT_RUN_SECONDS` yolu bundle'lı açılışta çalışıyor (doğrudan: exit 0; `open`: jeton satırı — notlar)
- [x] Test: içerik denetimi (Info.plist + lisans varlığı) — `bundle_assets` (girdi, `make hepsi`) + `make kur` (ürün); ikisi de mutasyonla kırmızı
- [ ] `[elle]` göz kontrolü: `make kur`, sonra `target/release/bateri.app`'i Finder'dan aç ve Dock'a sabitleyip oradan yeniden aç → Dock ikonu görünüyor, pencere öne çıkıyor ve klavye ona gidiyor. Gözlem (kapsam dışı): kabuk hangi dizinde açılıyor (`pwd`), `locale` UTF-8 mi. **Gatekeeper onayı bu kopyada beklenmez** — yerel derlemede quarantine yok (kanıt: notlar)
- [x] Doğrulama geçti (`proje.md` → Doğrulama; `make duman` **zorunlu**) — kapı sonrası son hâlde: `make hepsi` exit 0 · `make duman` exit 0 (`kare=2 hucre=8 glif=6 kural=15 yuva=13/2048 yuk=smoke istek=3 kapanis=clean profil=debug ornek=off pipeline=ok`) · `make kur` exit 0 · paket doğrudan exit 0 ve `open` ile jeton satırı (notlar) · `make test-yaris` gerekmedi (PTY/render/paylaşılan duruma dokunulmadı) · `make shader` gerekmedi
- [x] `/simplify` çalıştırıldı, bulgular uygulandı (4 mercek; atlananlar notlarda)
- [x] `/code-review` çalıştırıldı, bulgular giderildi (14 bulgu; `cwd=/` + `LANG` WAIVE önerisi notlarda)
- [x] `/audit` çalıştırıldı, bulgular giderildi (bulgu yok; ilgisiz mercekler notlarda)
- [x] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [x] Commit: dd46e85
