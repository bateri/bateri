# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Proje

`bateri`, macOS için GPU'nun (Metal 3) çizdiği bir terminal emülatörüdür. Rust
ile yazılır; AppKit ve Metal'e `objc2` ailesi üzerinden **doğrudan** bağlanır,
Swift katmanı yoktur. Referans ürün Metalterm'dir (metalterm.dev, kapalı
kaynak): komut blokları, sekiz rollü tema modeli, grain/sheen ile materyal
yüzeyler, fizik tabanlı imleç hareketi ve boşta sıfır kare. Referansın binary
incelemesinden çıkan mimari, özellik ve ayar envanteri `docs/ARASTIRMA.md`'dedir;
bir işe başlamadan önce ilgili bölümüne bakılır, sıfırdan keşfedilmez.

İskelet 001 ile kuruldu (workspace, Makefile, shader zinciri, ilk pencere),
VT motoru 002 ile: `bt-core` shell'i çalıştırır, `bt-gpu` hücre arka planlarını
çizer, `bt-shell` klavyeyi PTY'ye akıtır ve kapanışta shell'i düzgün bitirir.
`bt-atlas` 003 ile doldu: CoreText glyph'leri rasterize ediyor ve sabit yuva
ızgarasında adresliyor. Hücre ölçüsü oradan geliyor — `bt-gpu`
`Renderer::cell_metrics(scale)` ile yeniden yayınlıyor, `bt-shell`'in `CELL_PX`
yer tutucusu öldü ve grid gerçek font metriğinden türüyor. Glyph de artık
çiziliyor: `frame()` sınırı karakteri ve ön plan rengini geçiriyor, `bt-gpu`
atlası bir `R8Unorm` dokuya bağlayıp `cell` pipeline'ıyla harfleri arka
planların üstüne alfa karıştırarak koyuyor. 004 sınıra **biçimi** ekledi:
`bt-atlas` dört font yüzü ve altı kural sprite'ı (beş alt çizgi + üstü çizili)
tanıyor, `frame()` beş alan daha geçiriyor (`bold`, `italic`, `underline`,
`underline_color`, `strikeout`) ve `bt-gpu` `(bold, italic)`'i font yüzüne
çevirip kural çizgilerini glyph'lerden **sonra**, aynı pipeline'da çiziyor.
Ekranda kalın, eğik, altı çizili ve üstü çizili metin var; emoji, geniş glyph
ve kutu çizim ayrı setlerde. Aşağıdaki sözleşme kod geldikçe kodla birlikte
güncellenir — buradaki bir cümle kodla çelişirse ikisinden biri aynı commit'te
düzelir.

## Komutlar

```sh
make hepsi        # rustc sürümü + fmt --check + clippy -D warnings + test (definition of done)
make fmt          # cargo fmt --all -- --check
make clippy       # cargo clippy --workspace --all-targets -- -D warnings
make test         # cargo test --workspace
make shader       # kanarya: touch shaders/*.metal + cargo build -p bt-gpu (derleme reçetesi yalnız build.rs'te)
make duman        # uygulamayı BT_RUN_SECONDS=3 ile açar; kare, arka plan hücresi, glyph, kural çizgisi ve atlas yuvası sayar:
                  # kare=N hucre=K glif=G kural=R yuva=U/T yuk=smoke istek=I kapanis=clean profil=debug ornek=off pipeline=ok
                  # ilk dördünden biri 0 ise kırmızı; `kare` ayrıca ÜST SINIRLI (boşta sıfır karenin bekçisi, `IDLE_FRAME_LIMIT` = 8, ölçülmüş — türetmesi sabitin doc'unda, koşuları `docs/OLCUMLER.md`'de).
                  # `yuva`/`yuk`/`istek`/`profil` kapı değil sayaç ve etiket; `kapanis` kısmen kapı (panik kolları kırmızı düşürür, kayıtlı borç olan iki kol düşürmez — değerleri `teardown_token`'da).
                  # `ornek=off` = ölçüm kapısı kapalıydı ve o koşuda ölçüm jetonları hiç basılmaz; neden sıfır olmadığı `Report::token_line`'da.
make terminfo     # assets/terminfo'yu tic -x ile geçici dizine derler
make test-yaris   # yarış stresi: race_* (--ignored) + tek thread karşılaştırma koşusu
make kur          # release derler, target/release/bateri.app'i kurar ve içeriğini denetler (Info.plist, ikon, lisans); imza yok
```

Girdisi henüz olmayan hedefler "henüz yok" deyip kırmızı düşer; listesi
`.claude/is-akisi/proje.md` başındadır.

Tek crate / tek sınama:

```sh
cargo test -p bt-core -- osc::tests
```

## Katman düzeni

Katmanlar tek yönlüdür; **hiçbir bağımlılık yukarı doğru gitmez**:

```
bateri (bin) → bt-shell → bt-gpu → {bt-atlas, bt-core}
                   └──────────────────────→ bt-core
```

| crate | sorumluluk | görebildiği platform kütüphanesi |
|---|---|---|
| `bt-core` | VT durum makinesi, grid ve scrollback, PTY ve okuyucu thread, OSC (7/8/9/52), komut blokları, seçim, girdi kodlaması (DECCKM'e uyan oklar, tekerlek raporu), ayar modeli, shell bağlamı. OSC 133 alacritty'de **yok**: komut blokları `frame()` sınırına kanca isteyecek (00X) | macOS'a özgü **hiçbiri** — `objc2*`, `core-text`, `metal` yok. Unix PTY (`libc`, `rustix`) serbest; kapı Linux hedefiyle derlemedir |
| `bt-atlas` | glyph rasterizasyonu, atlas paketleme, kutu çizim karakterleri, font seti | `objc2-core-text`, `objc2-core-graphics` ve ortak tabanları `objc2-core-foundation`. `objc2` çekirdeğini bile **görmez**: kullanılan her şey C API'si, ObjC runtime'ı değil |
| `bt-gpu` | Metal renderer, shader'lar (`.metal`), display link ve `Waker` (kareyi süren ritim), kare yolunun **ölçüm defteri** (`Stats`: iki CPU aralığı, GPU deltası, açılış damgası, p95'in tabanı — biriktirir, **basmaz**), hareket (motion), overlay'ler (palet, arama), durum çubuğu | `objc2`, `objc2-foundation`, `objc2-metal`, `objc2-quartz-core`, `dispatch2` (metallib yükleme, ana kuyruk), `block2` (tamamlanma bloğu) |
| `bt-shell` | AppKit kabuğu: pencere, sekme, bölme, menü, klavye, servisler, ayar penceresi; kapanış sırasının ve duman bekçisinin sahibi; kabuğun başlangıç dizini ve yereli (politika, `child`) | `objc2`, `objc2-foundation` (`NSLocale` dahil: kabuğun yereli), `objc2-app-kit`, `objc2-quartz-core` (yalnız `CALayer` takma), `dispatch2` (ana kuyruk: `child_exit` → `terminate:`), `libc` (yalnız bekçinin `write` + `_exit`'i) |
| `bateri` | `main`, app bundle, Sparkle | — |

`bt-core`'un platformsuzluğu bir zevk değil kapıdır: Metalterm'in yol haritasında
"1.0'dan sonra Vulkan" var ve o kapı bu ayrımın üstüne kurulur.

## Bilinmesi gerekenler

- **Taban macOS 14, tek kaynağı `.cargo/config.toml`'daki
  `MACOSX_DEPLOYMENT_TARGET`.** rustc binary'nin minos'unu, `bt-gpu/build.rs`
  shader'ların `-mmacos-version-min`'ini oradan alır; `make kur` `Info.plist`'in
  `LSMinimumSystemVersion`'ını binary'nin `minos`'undan, yani dolaylı olarak
  yine oradan doldurur. Metalterm'in tabanıyla aynı.
- **Bağımlılık mimari karardır**, kendiliğinden eklenmez. Taban:
  `alacritty_terminal` (VT ayrıştırma, grid, PTY ve okuyucu thread; kendi
  ayrıştırıcımızı yazmıyoruz — `bt-core` onu **kapsüller**, `pub` API'de
  alacritty tipi görünmez), `objc2` ailesi (CoreText ve CoreGraphics dahil:
  servo ailesi `core-text` **reddedildi**, ikinci bir CF sarmalayıcı yığını
  olurdu — 003 kararı), `toml` + `serde`, `tracing`. `Cargo.lock` depodadır.
  `alacritty_terminal` **Apache-2.0**: lisans metni pakete
  `assets/bundle/THIRD-PARTY-LICENSES.txt` ile girer, atfı About panelinin
  `Credits.html`'inde durur. Atıf isteyen yeni bir bağımlılık da o iki
  dosyaya yazılır — bu bir teamül, denetim yalnız `alacritty_terminal`'ı arıyor.
  Liste **eksik**: ağaçtaki MIT paketlerinin bildirimleri henüz yok, borç
  `docs/YOL-HARITASI.md` → sete bağlanmamış borçlar.
- **Hücre sabit boyuttadır** ve `const` assert ile bağlanır; emoji, grapheme
  kümeleri ve alt çizgi rengi gibi seyrek veriler yan tablolarda yaşar
  (alacritty'de `CellExtra`). Bugünkü sabit **24 bayt**: alacritty `Cell`'i
  (Metalterm 20'de tuttu). Assert `bt-core/src/lib.rs`'tedir; kendi hücremize
  geçiş `Session::frame()` sınırının arkasında yapılır ve renderer'ı değiştirmez.
  **Bu 24 bayt bir *grid* hücresidir; `frame()` sınırının `bt_core::Cell`'i
  ayrı bir kare kaydıdır ve aynı bütçeye tabi değil** — grid hücresi 10 000
  satırlık scrollback'te sekme başına megabaytlarca yaşar, sınır hücresi kare
  başına ve yalnız **çizilen** hücreler için doğar (004'te beş alan daha
  kazandı ve assert oynamadı). Sınır hücresine alan eklenirken ölçüt bu assert
  değil, kare başına maliyettir.
- **Renk uzayı sınırı geçer.** Çizim hedefi `BGRA8Unorm_sRGB`: donanım
  fragment çıktısını **lineer** sayar ve yazarken sRGB'ye kodlar. Bu yüzden
  `bt-core` sınırdan lineer float verir (`color::linear_rgba`) ve `MTLClearColor`
  da aynı kaynaktan beslenir — pencere zemini ile hücreler tek yerden düzelir.
  İkisi **birlikte** değişir; biri lineerleşmeden ötekine geçilirse palet
  `0x1a1c21`'den `0x5a5d65` griye açılır ve belirti sessizdir. Gören tek bekçi
  `cell_bg_paints_pixels_on_the_gpu` ve ancak **ara ton** bir renkle görür:
  `0.0` ve `1.0` sRGB transfer fonksiyonunun sabit noktalarıdır.
- **Boşta sıfır kare.** Kirli satır yoksa frame gönderilmez. Her animasyon bir
  durma koşulu taşır; `reduce_motion` ve sistemin Reduce Motion ayarı her
  animasyonu 90 ms'lik solmaya indirir.
- **Kapanış sınırlı bekler, çocuk yine de ölmeyebilir.**
  `Session::shutdown()` `SIGHUP`'tan sonra `join`'i ve `Pty`'nin düşmesini
  ayrı bir thread'e alır ve en çok `SHUTDOWN_GRACE` (yarım saniye) bekler;
  ne sinyali yutan bir çocuk (`trap '' HUP`) ne de PTY'ye yazarken `SIGHUP`
  alıp **çıkışın içinde takılan** çocuk (`ps` durumu `?Es`) artık kapanışı
  asamaz; tek istisna kapanış thread'inin kurulamaması (OS thread sınırı),
  o dalda sınır yoktur. Kalan borç çocuğun kendisi: süre dolunca arkada
  bırakılır ve ancak süreç çıkışı master fd'yi kapatınca gider. Kalıcı çare
  "süre → `SIGKILL`" **değil** — ölçüm onu çürüttü, o çocuk `SIGKILL`
  almıyor; çare `wait` bloklarken master'ı boşaltmak, yolu da
  `Session::spawn`'da `pty.file().try_clone()` (yeni bağımlılık istemiyor) —
  `EventLoop` `Pty`'yi `join`'den sonra vermediği için kopya baştan alınmak
  zorunda. Duman koşusundaki bekçi (`_exit(70)`) duruyor ama artık kapanış
  yolunun **başka** asılmalarına karşı. Borç aynı borç, ama artık
  **görünür**: `shutdown()` sonucu döndürüyor (`Teardown`) ve süreli koşu onu
  `kapanis=` jetonuyla basıyor (değerler `teardown_token`'da) — sınırın
  dolduğu koşu eskiden yeşil bir satırla geçip yalnız stderr'de iz
  bırakıyordu.
- **Render yolu bloklanmaz.** PTY okuma ve ayrıştırma kendi thread'inde; AppKit
  çağrıları `MainThreadMarker` ile ana thread'de; renderer `CAMetalDisplayLink`
  ile sürülür.
- **PTY ve ayrıştırma yolunda panik yok.** Bilinmeyen dizi yoksayılır, loglanır.
  Kuralın ikinci yarısı **henüz borç**: `tracing` bağlanmadı, logger yok —
  yoksayılan olaylar (başlık, zil, pano) ve alacritty'nin `log` satırları
  sessizce düşüyor; logger gelince bu cümle kalkar.
- **`tty::setup_env()` çağrılmaz.** O, *kendi* sürecimizin ortamını `set_var`
  ile değiştirir ve makinede alacritty kuruluysa `TERM=alacritty` yazar.
  Çocuğun ortamı `tty::Options.env` ile verilir: `TERM=xterm-256color`,
  `COLORTERM=truecolor` — `SessionOptions.env`'in ek ortamı ikisini
  **ezemez**. Buna karşılık alacritty `ALACRITTY_WINDOW_ID` ve
  `WINDOWID`'yi koşulsuz yazar ve kapatılamaz — shell'de görünürler.
  **Dizin ve yerel de yalnız çocuğa gider** (006 phase-4b): kabuk her
  açılışta ev dizininde başlar (`HOME`, yoksa passwd kaydı; mutlak değilse miras) ve ortamda
  `LC_ALL`/`LC_CTYPE`/`LANG`'dan hiçbiri boş olmayan bir değer taşımıyorsa
  macOS'un dil/bölge ayarından `LANG={dil}_{bölge}.UTF-8` alır; o yerel
  `/usr/share/locale`'de yoksa `LANG=en_US.UTF-8` — alacritty'nin
  `LC_CTYPE=UTF-8`'i değil, SSH onu Linux'a taşıyınca `setlocale` uyarısı
  verir (006 phase-4c; gerekçe `child::decide_locale`'in doc'unda). Dil
  `NSLocale.preferredLanguages`'tan okunur: paketin içinde
  `currentLocale().languageCode` kullanıcının değil **paketin** dilini
  (`en`) verir ve `cargo run` bunu göstermez. Kendi sürecimizde
  `set_current_dir`, `set_var` ve `setlocale` **yok** — alacritty ikisini de
  kendi sürecinde yapıyor ve `LC_ALL` yazıyor; biz `LANG` yazıyoruz ki
  kabuğun rc dosyası `LC_*`'ı üstüne yazabilsin. Politika `bt-shell`'de
  (`child`), `bt-core` yalnız geçirir (`SessionOptions.working_directory`,
  `.env`). Sebep Dock'tan açılış: LaunchServices süreci `cwd=/` ile
  başlatıyor ve launchd'nin ortamında `LANG` yok.
- **Tema = sekiz rol:** arka plan, ön plan, dim, accent ve dört durum. Materyal
  yüzey (grain, sheen) bunun üstüne ayrı bir katmandır ve `substrate` shader'ı
  çizer. Palet dosyaları `~/.config/bateri/themes/*.toml`.
- **Ayarlar** `~/.config/bateri/settings.toml`; bilinmeyen anahtar korunur,
  anahtar silinmez, ayar penceresi dosyayı yeniden yazar ama tanımadığını bırakır.
- **Shell entegrasyonu** üç kabuk içindir (zsh `ZDOTDIR`, bash `--rcfile`
  sarmalayıcısı, fish `vendor_conf.d`) ve kullanıcının rc dosyasına **asla**
  dokunmaz. Komut blokları OSC 133 işaretlerinden okunur.
- **Ölçülmemiş sayı yazılmaz.** Tek sahip `docs/OLCUMLER.md`; ölçüm bir kapı
  değildir, `/measure` ile kullanıcı ister. **Zaman kancaları 005 ile geldi**
  ve ikisi de env: `BT_SCROLL_TEST` yükü seçer (akan çıktı; boşta bir pencere
  kare süresi vermez), `BT_FRAME_STATS` ölçümü açar. İkisi de
  `BT_RUN_SECONDS`'ı **sıfırdan büyük** ister, yoksa süreç çıkış 1 verir:
  rapor yalnız deadline yolunda basılıyor, süresiz bir ölçüm sessizce örnek
  biriktirip atardı. Kapı kapalıyken tek bir saat okuması bile yok. Açık
  ölçüm koşusu jetonlarla dönüyor — CPU'nun iki aralığı, GPU deltası, açılış,
  örnek sayısı ve tabanı — ama **sayının kendisi buraya yazılmaz**:
  hangi türün sayısı olduğunu `docs/OLCUMLER.md`'nin başı söylüyor. Kare
  süresi ve açılışın yöntemi sıfırdan yazılmayacak: kancanın dürüst sınırları
  — her biri **kapsam** ya da **açık kalem** diye etiketli — `bt-shell`'de
  `Measured`'ın doc'unda emaneten duruyor ve o türün ilk `/measure`'ı onları
  dosyanın `## Yöntem`'ine taşır. **Bench seti hâlâ borç:** `criterion` yeni bir bağımlılık, yani
  ayrı bir mimari karar; 005 bir borcu bilerek başka bir borçla takas etti ve
  `cargo bench` satırı yukarıdaki komut bloğuna **bench seti gelince** döner,
  kancalarla değil. Aynı şey giriş gecikmesi zinciri
  (`BT_INPUT_LATENCY_SAMPLES`) ve düşen kare sayımı için de geçerli. Hangi
  iddianın hangi araca baktığı `/measure` skill'inin tablosunda, set başına
  durum `.tasks/README.md`'de, gövdeler 002, 003 ve 004'ün
  `teslim.md`'lerinde.
- **Dil:** yorumlar, commit iletileri ve belgeler Türkçe ve "neden"i anlatır.
  **Kod tanımlayıcılarının tamamı İngilizce** — dışa bakan ad (pub tip,
  fonksiyon, varyant) da, yerel yardımcı, alan, değişken ve sınama adı da;
  `build.rs` dahil, istisnasız. UI dizgileri, ayar anahtarları, tema ve
  materyal adları İngilizce. **Üç öbek Türkçe kalır ve üçü de kod değildir:**
  tanı metni (stderr iletileri, `assert!` gerekçeleri, `make duman`'ın düşen
  koşuda bastığı açıklama satırı) UI dizgisi olmadığı için; `Makefile`
  hedefleri (`hepsi`, `duman`, `shader`, `test-yaris`, `kur`, `terminfo`)
  projenin komut yüzeyi olduğu için; süreli koşunun jeton satırındaki
  **anahtarlar** (`kare=`, `hucre=`, …) sözleşme donmuş olduğu için. Bu
  sonuncusu tanı metni değil bir **makine sözleşmesidir** ve içinde üçe
  ayrılır — anahtar Türkçe ve donmuş, **değer İngilizce**, tanı metni satırın
  dışında; ayrımın gerekçesi `Report::token_line`'ın doc'unda. Sözleşmenin
  kuralı burada kalıyor çünkü depo geneli: **jeton silinmez, eklenir** —
  okuyan taraf tanımadığı jetonu atlayabilir, kaybolanı arayamaz. `ATLANDI`
  da aynı sözleşmenin parçası.

## İş akışı

Çok oturumlu ve tasarım kararı içeren işler `.claude/` altındaki zincirle
yürür: `/rfc → /plan-review → /implement → /ship`, sürücüsü `/akis`.
Kurallar `.claude/README.md` ve `.claude/is-akisi/`'de; iş setleri `.tasks/`
altında. Tek dosyalık düzeltme için set açılmaz.

Hangi işin **neden o sırada** olduğu `docs/YOL-HARITASI.md`'dedir; henüz
açılmamış setlerin sırası ve bağımlılıkları oraya yazılır. **Durumu** o dosya
tutmaz — tek sahibi `.tasks/README.md` indeksidir, iki yerde durum tutmak
drift üretir.
