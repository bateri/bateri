# Workspace iskeleti — Tartışma

## Karar 1: Olay döngüsü ve pencere — `objc2` doğrudan mı, `winit` mi?

**(a) `objc2` ile doğrudan AppKit.** `NSApplication::sharedApplication` +
`define_class!` ile `NSApplicationDelegate`, `NSWindow`, kendi `NSView`
alt sınıfımız. Örnek: `objc2/examples/app/delegate.rs`.

- Artı: sekme (`NSWindow` tab grubu), bölme, IME (`NSTextInputClient`),
  servis menüsü, Dock menüsü, pencere geri yükleme — hepsi AppKit'in
  yüzeyleri; Metalterm bunların tamamını kullanıyor (`docs/ARASTIRMA.md`).
- Artı: `CLAUDE.md` bunu söylüyor, `winit` yeni bağımlılık.
- Eksi: daha fazla `unsafe` ve boilerplate; klavye olaylarını kendimiz çeviririz.

**(b) `winit`.** Platformlar arası pencere; Alacritty ve WezTerm'in yolu.

- Artı: pencere/klavye/fare hazır; Linux kapısı bedavaya yakın.
- Eksi: yerel sekme, IME inceliği, servisler için `winit`'in altına inip yine
  AppKit çağırmak gerekir; iki katman aynı pencereyi yönetir. Vulkan kapısı
  zaten `bt-core`'un platformsuzluğuyla korunuyor, pencere katmanıyla değil.

## Karar 2: Çizim yüzeyi — `MTKView` mü, `CAMetalLayer` + `CAMetalDisplayLink` mi?

**(a) `MTKView`** (`objc2-metal-kit`, ek crate). Örneklerin yolu; drawable ve
zamanlamayı gizler; `isPaused` + `enableSetNeedsDisplay` ile talep üzerine çizim.

**(b) `NSView` + `CAMetalLayer` + `CAMetalDisplayLink`** (macOS 14+).
`view.setWantsLayer(true); view.setLayer(metal_layer)`; display link
`initWithMetalLayer`, delegate `metalDisplayLink:needsUpdate:` her karede
hazır drawable'ı verir; `setPaused(true)` ile boşta durur;
`preferredFrameRateRange` ile ProMotion.

**(c) `CAMetalLayer` + `CVDisplayLink`.** Eski yol; macOS 15'te deprecated.

- (b) lehine: drawable'ı display link'in **vermesi** "boşta sıfır kare"
  ilkesini doğrudan modelliyor (kirli yoksa `paused`); Metalterm'in seçtiği
  yol (`metalDisplayLink:needsUpdate:` seçicisi binary'de); ek crate yok
  (`objc2-quartz-core` zaten `NSView.layer` için gerekli).
- (a) lehine: daha az kod. Aleyhine: `MetalKit` bağımlılığı, drawable
  boyutu/ölçek yönetimini görünmez kılar; ileride `CAMetalLayer`'a inmek
  gerekince iki yol birden yaşar.
- macOS 14 tabanı Metalterm'inkiyle aynı (`LSMinimumSystemVersion 14.0`).

## Karar 3: Shader derlemesi — `build.rs` + `xcrun metal` mi, çalışma zamanında kaynak mı?

**(a) `build.rs`:** `crates/bt-gpu/shaders/*.metal` → `xcrun -sdk macosx metal -c`
→ `xcrun metallib` → `$OUT_DIR/default.metallib`; `cargo:rerun-if-changed`
her `.metal` için. Hata **derleme zamanında** ve satır numarasıyla.

**(b) Çalışma zamanı:** `newLibraryWithSource_options_error(include_str!(...))`.
Örneklerin yolu; `xcrun` gerekmez.

- (a) lehine: `proje.md`'nin `make shader` sözleşmesi; Metalterm aynı yolu
  taşıyor (`Resources/default.metallib`, `build/mt-gpu-*/out/default.metallib`);
  shader hatası `cargo build`'i kırar, uygulama açılışını değil; açılışta
  derleme yok (süresi **ölçülmedi**, iddia edilmiyor — gerekçe hatanın yeri).
- (b) lehine: `xcrun` olmayan makinede derlenir. Karşı: bu bir macOS
  uygulaması ve Xcode zaten ön koşul; `build.rs` `xcrun` yoksa **anlaşılır
  hata** verir (metallib'siz devam etmez).

## Karar 4: metallib'i yükleme — gömülü bayt mı, bundle kaynağı mı?

**(a) `include_bytes!(concat!(env!("OUT_DIR"), "/default.metallib"))`** →
`DispatchData` → `newLibraryWithData_error`. Tek dosya; çıplak binary
(`make duman`, `cargo run`) bundle olmadan çalışır.

**(b) `bateri.app/Contents/Resources/default.metallib`** →
`newLibraryWithURL_error` ya da `newDefaultLibrary` (main bundle ister).

- (a) lehine: iki çalışma kipi (çıplak binary ve `.app`) aynı yolu kullanır;
  `dispatch2` zaten `objc2-metal`'in feature bağımlılığı, yeni crate değil.
- (b) lehine: binary küçülür (157 KB'lık metallib dışarıda). Önemsiz.
- Metalterm (b)'yi kullanıyor (`Resources/default.metallib`); biz (a) ile
  başlıyoruz, `make kur` ileride isterse (b)'ye geçebilir — `bt-gpu`
  `&[u8]` alır, kaynağını bilmez.

## Karar 5: Crate derinliği — beş crate şimdi mi, ihtiyaç olunca mı?

**(a) Beşi de şimdi.** `bt-core` ve `bt-atlas` yalnız `lib.rs` başlığında
sözleşmesini anlatan yorum + boş modül + bir tane "derleniyor" sınaması.

**(b) Yalnız `bt-gpu`, `bt-shell`, `bateri`.** Diğerleri 002'de.

- (a) lehine: `/audit` merceği 1 (`cargo tree -p bt-core | grep objc2` boş
  dönmeli) ilk günden anlamlı; `Cargo.toml`'daki bağımlılık **yönü** kodla
  değil workspace'le kurulur, sonradan bölmek "kim kimi görür"ü yeniden
  müzakere ettirir. Boş crate'in maliyeti bir dizin.
- **Bu set `alacritty_terminal` eklemez.** VT motoru kararı 002'nindir
  (`alacritty_terminal` mi, `vte` + kendi grid'imiz mi); `CLAUDE.md`'nin
  taban listesi bir niyettir, seçim o sette kaydedilir.

## Karar 6: Görev koşucusu — `Makefile` mi, `just`/`cargo-make` mi?

`Makefile`. `proje.md` hedef adlarını `make` olarak yazdı; `make` sistemde var,
ötekiler kurulum ister. Hedefler `cargo`'ya ince sarmalayıcıdır; `make shader`
ve `make terminfo` `build.rs`'in yaptığını tek başına koşturmak içindir
(doğrulamada "shader derlendi mi"yi `cargo test`'ten ayırmak için).

## Karar Noktaları

Yukarıdaki altı karar için önerilen yön: **1a, 2b, 3a, 4a, 5a, 6 Makefile.**
Panel bunları sınar; kullanıcı onayı adım 7'de.

Ayrıca iki küçük sözleşme, itiraz yoksa plana girer:

- Edition **2024**, `rust-version = "1.88"` workspace'te; `rust-toolchain.toml`
  yok (Homebrew rustc kullanılıyor, `rustup` zorunlu kılınmaz).
- Panik politikası şimdiden: `bt-gpu`'da `MTLCreateSystemDefaultDevice`
  başarısızlığı `Result` ile yukarı çıkar, `bateri` main'de anlaşılır iletiyle
  çıkar; `unwrap` yalnız "olamaz" durumlarında ve `// audit:` yorumuyla.

## Muhakeme (9 Eylül 2026)

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU — set, bugün hiçbir şeyi doğrulamayan üç mekanizma kuruyor: koşamayacak make hedefleri, süreceği içerik olmayan display link, çıktının kullanmadığı shader zinciri |
| Codebase-fit | SORUNLU — API'lerin hepsi var (K4a KIRMIZI değil) ama `bt-shell`'e `objc2-quartz-core` sokan sahiplik `CLAUDE.md` tablosunu kırıyor; `dispatch2`'nin "yeni crate değil" gerekçesi yanlış; `make shader` `build.rs`'i ikinci kez yazıyor |
| İşletme | SORUNLU — `make duman` yalnız çıkış koduna bakıyor (siyah pencere yeşil geçer); "`Cell` 16 bayt, 001 ölçer" cümlesi boş `bt-core` ile çelişiyor; `git init` planın hiçbir yerinde değil ve kök commit geri alınamaz |

**Kabul edilen itirazlar → plan değişikliği:**

- **K2 daraltıldı: `CAMetalDisplayLink` bu sette yok.** (Sadelik) Sürecek içerik
  yok; `paused=false` ilk commit'te "boşta sıfır kare"yi ihlal eder,
  `paused=true` ölü kod bırakır. 001'de düz `NSView` + `CAMetalLayer` +
  `nextDrawable` ile **tek kare** çizilir; yeniden çizim yalnız pencere
  boyutu değişince (aynı delegate sınıfında `NSWindowDelegate::windowDidResize`).
  Display link, kirli satır kavramıyla birlikte 002'ye. `bt-gpu`'nun çizim
  imzası drawable alır, kaynağını bilmez — 002'de link eklemek yerel değişiklik.
- **Çizim yüzeyinin sahibi `bt-gpu`.** (Codebase-fit) `CAMetalLayer`'ı
  `bt-gpu::Surface` kurar, `bt-shell` yalnız `&CALayer` alıp view'a takar.
  `bt-shell`'in `objc2-quartz-core`'u **yalnız `CALayer` feature'ıyla** görmesi
  gerekir; `CLAUDE.md` tablosu phase-1 commit'inde düzeltilir.
- **`setLayer` önce, `setWantsLayer(true)` sonra.** (Codebase-fit) Ters sıra
  AppKit'e kendi layer'ını kurdurur; plana yanlış yazılmıştı.
- **Çıktı shader'ın çizdiği tam ekran quad.** (Sadelik) "Tek renk temizleme"
  `loadAction = clear` ile shader'sız olur ve K3+K4 hiç çalışmaz. Vertex + tek
  renk döndüren fragment; K3a/K4a böylece hak edilir.
- **`dispatch2` açık bağımlılık ve mimari karar.** (Codebase-fit)
  `objc2-metal`, `dispatch2`'yi `block2` feature'ı olmadan çeker;
  `DispatchData::from_static_bytes` o feature'ın arkasında. `bt-gpu`'nun
  `Cargo.toml`'una `dispatch2 = { features = ["block2"] }` girer; K4'ün
  gerekçesi düzeltildi ve karar kullanıcıya **bağımlılık kararı** olarak sunulur.
  `include_bytes!` `&'static [u8]` verdiği için `from_static_bytes` (kopyasız).
- **`make shader` = `touch crates/bt-gpu/shaders/*.metal && cargo build -p bt-gpu`.**
  (Codebase-fit, İşletme) Derleme reçetesi tek yerde (`build.rs`) kalır;
  `proje.md`'deki "cargo test onu görmez" gerekçesi K3a altında yanlıştı,
  satır "cargo'nun bayatlık takibini atlayan kanarya" olarak yeniden yazılır.
- **`build.rs` sertleştirmeleri.** (İşletme) `crates/bt-gpu/build.rs`'te
  (kökte değil — `bt-core` GPU'suz test edilebilir kalsın); `xcrun -f metal`
  önce kontrol, yoksa Türkçe anlaşılır hata ("Xcode gerekiyor; Command Line
  Tools metal derleyicisini taşımaz"); `-mmacos-version-min=14.0` (macOS 26
  SDK'sıyla üretilen metallib macOS 14'te çalışma zamanında reddedilir);
  `cargo:rerun-if-changed=shaders` **dizin** olarak (yeni eklenen `.metal`
  izlensin); `CoreGraphics`'e `#[link]` stanza'sı (`MTLCreateSystemDefaultDevice`
  ister, crate bağımlılığı değildir).
- **`make duman` bir kare doğrular.** (İşletme) `BT_RUN_SECONDS` bir tavandır:
  süre dolunca kare sayısı > 0 ise `kare=N pipeline=ok` basıp `exit 0`, değilse
  `exit 1`. Fonksiyon adı `newFunctionWithName` → `None` ise `Result` hatası,
  sessiz atlama yok. Başsız ortam (`launchctl managername` ≠ `Aqua`) `exit 78`
  + "ATLANDI: Aqua oturumu yok" — atlama, geçme değil.
- **Üç make hedefi ertelendi: `terminfo`, `test-yaris`, `kur`.** (Sadelik)
  Girdileri yok (`assets/terminfo`, thread, bundle). `Makefile`'da yer alır ama
  "henüz yok — hangi set" deyip `exit 78` ile çıkarlar; `proje.md` tablosuna
  aynı commit'te not düşer. `make hepsi` başında `rustc --version` basar
  (Homebrew kayması kırmızısını kod kırmızısından ayırmak için).
- **"`Cell` 16 bayt, 001 ölçer" cümleleri 002'ye taşınır.** (İşletme,
  Codebase-fit) `bt-core` bu sette boş; `CLAUDE.md` ve `proje.md` phase-1
  commit'inde düzeltilir.
- **Boş crate'lere sahte "derleniyor" sınaması konmaz.** (Sadelik)
  `cargo build` zaten kanıtlıyor. `cargo tree` merceğinin bağımlılık
  düzeyinde bir vekil olduğu, gerçek kapının (`--target x86_64-unknown-linux-gnu`)
  `rustup` gelene kadar kapalı olduğu kayda girer.
- **`git init` ayrı bir phase-0 commit'idir: "Depoyu başlat".** (İşletme)
  Mevcut belgeler + `.gitignore` (`target/`, `*.metallib`, `*.air`, `*.dSYM`,
  `*.dmg`, `*.app`, `.DS_Store`; `Cargo.lock` girer), kod yok. Phase
  commit'leri bunun üstüne oturur ve tek tek geri alınabilir.
- **Üç phase, her biri tam bir dış bağımlılık ekler.** (İşletme) phase-1
  workspace (Xcode'suz makinede de yeşil) → phase-2 `bt-gpu` + `xcrun`
  (device, library, pipeline; fonksiyon adı sınaması) → phase-3 `bt-shell` +
  WindowServer (pencere, kare, `make duman`).

**Reddedilenler:**

- "K3'ü de ertele, `clear` ile yetin" (Sadelik'in (b) seçeneği) — `build.rs`
  + `xcrun` zinciri iskeletin en az geri dönülebilir riski; bugün ölçülmesi
  doğru. Sadelik'in kendi önerisi de (a).
- "`make test-yaris`'i tamamen kaldır" — hedef adı `proje.md` sözleşmesinde;
  var olup "koşamadı" demesi, hiç olmamasından iyi (`[~]`'ın oturacağı yer).
- "Beş crate yerine üç" — üç jüri de K5a'yı kabul etti; itiraz yalnız
  gerekçenin abartısınaydı, o düzeltildi.

## Karar (10 Eylül 2026, kullanıcı onayı)

- **Seçilen:** K1a `objc2` ile doğrudan AppKit · K2 düz `NSView` + `CAMetalLayer`,
  tek kare, **display link yok** (002'ye) · K3a `crates/bt-gpu/build.rs` +
  `xcrun metal`, `-mmacos-version-min=14.0`, `make shader` yalnız sarmalayıcı ·
  K4a `include_bytes!` + `DispatchData::from_static_bytes`; **`dispatch2`
  (`block2` feature'ıyla) `bt-gpu`'nun açık bağımlılığı — kullanıcı onaylı
  bağımlılık kararı** · K5a beş crate, `bt-core`/`bt-atlas` boş, sahte sınama
  yok · K6 `Makefile`; `terminfo`/`test-yaris`/`kur` "henüz yok" deyip `exit 78`.
  Çıktı shader'ın çizdiği tam ekran quad; `make duman` kare sayısına bakar;
  phase-0 `git init`. Gerekçeler `## Muhakeme`'de.
- **Reddedilen:** K1b `winit` — ikinci pencere sahibi, sekme/IME/servisler için
  AppKit'e yine inilir · K2a `MTKView` — `objc2-metal-kit` yeni crate, ileride
  atılır · K2 `CAMetalDisplayLink` 001'de — sürecek içerik yok · K3b çalışma
  zamanı derleme — 002+'daki dokuz pipeline'da zaten terk edilir · K4b bundle
  kaynağı — çıplak binary çalışmaz · K5b üç crate — bağımlılık yönü sonradan
  müzakere ettirir · `just`/`cargo-make` — kurulum ister.
