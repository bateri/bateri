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

**Bugünkü hâl** (hangi setin neyi getirdiği `.tasks/README.md`'de): `bt-core`
shell'i çalıştırır ve kareyi `frame()` sınırından verir — karakter, ön plan
rengi ve biçim (`bold`, `italic`, `underline`, `underline_color`,
`strikeout`). `bt-atlas` CoreText ile dört font yüzünün glyph'lerini ve altı
kural sprite'ını (beş alt çizgi + üstü çizili) sabit yuva ızgarasında
rasterize eder; hücre ölçüsü oradan gelir ve `bt-gpu`
`Renderer::cell_metrics(scale)` ile yeniden yayınlar. `bt-gpu` atlası
`R8Unorm` dokuya bağlar, `(bold, italic)`'i font yüzüne çevirir ve `cell`
pipeline'ında arka planın üstüne önce glyph'leri, **sonra** kural çizgilerini
çizer. `bt-shell` klavyeyi PTY'ye akıtır; fareyle seçim, pano, geçmişte
kaydırma ve kapanış sırası ondadır; açılışta `settings.toml`'u okur (bugün
`scrollback` ve tema adı) ve temayı `themes/{ad}.toml`'dan ya da gömülü
`bateri`'den çözer. `make kur` `bateri.app` paketini üretir.
Emoji, geniş glyph ve kutu çizim henüz yok. Aşağıdaki sözleşme kod geldikçe
kodla birlikte güncellenir — buradaki bir cümle kodla çelişirse ikisinden biri
aynı commit'te düzelir.

## Komutlar

```sh
make hepsi        # rustc sürümü + fmt --check + denetim + clippy -D warnings + test (definition of done)
make fmt          # cargo fmt --all -- --check
make denetim      # kuralların mekanik yarısı: katman yönü, bt-core'da gerekçesiz panik, rc dosyasına yazma; Cargo.lock değiştiyse uyarır
make clippy       # cargo clippy --workspace --all-targets -- -D warnings
make test         # cargo test --workspace
make shader       # kanarya: touch shaders/*.metal + cargo build -p bt-gpu (derleme reçetesi yalnız build.rs'te)
make duman        # uygulamayı BT_RUN_SECONDS=3 ile açar ve jeton satırı basar:
                  # kare=N hucre=K glif=G kural=R yuva=U/T yuk=smoke istek=I kapanis=clean profil=debug ornek=off pipeline=ok
                  # ilk dördünden biri 0 ise ya da kare > IDLE_FRAME_LIMIT (= 8, ölçülmüş; türetmesi sabitin doc'unda) ise kırmızı.
                  # yuva/yuk/istek/profil sayaç ve etiket; kapanis kısmen kapı (değerler teardown_token'da); ornek=off'ta ölçüm jetonu basılmaz.
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
| `bt-core` | VT durum makinesi, grid ve scrollback, PTY ve okuyucu thread, OSC (7/8/9/52), komut blokları, seçim, girdi kodlaması (DECCKM'e uyan oklar, tekerlek raporu), ayar modeli, shell bağlamı. OSC 133 alacritty'de **yok**: komut blokları `frame()` sınırına kanca isteyecek | macOS'a özgü **hiçbiri** — `objc2*`, `core-text`, `metal` yok. Unix PTY (`libc`, `rustix`) serbest; kapı Linux hedefiyle derlemedir |
| `bt-atlas` | glyph rasterizasyonu, atlas paketleme, kutu çizim karakterleri, font seti | `objc2-core-text`, `objc2-core-graphics` ve ortak tabanları `objc2-core-foundation`. `objc2` çekirdeğini bile **görmez**: kullanılan her şey C API'si, ObjC runtime'ı değil |
| `bt-gpu` | Metal renderer, shader'lar (`.metal`), display link ve `Waker` (kareyi süren ritim), kare yolunun **ölçüm defteri** (`Stats`: iki CPU aralığı, GPU deltası, açılış damgası, p95'in tabanı — biriktirir, **basmaz**), hareket (motion), overlay'ler (palet, arama), durum çubuğu | `objc2`, `objc2-foundation`, `objc2-metal`, `objc2-quartz-core`, `dispatch2` (metallib yükleme, ana kuyruk), `block2` (tamamlanma bloğu) |
| `bt-shell` | AppKit kabuğu: pencere, sekme, bölme, menü, klavye, servisler, ayar penceresi; kapanış sırasının ve duman bekçisinin sahibi; kabuğun başlangıç dizini ve yereli (politika, `child`) | `objc2`, `objc2-foundation` (`NSLocale` dahil: kabuğun yereli), `objc2-app-kit`, `objc2-quartz-core` (yalnız `CALayer` takma), `dispatch2` (ana kuyruk: `child_exit` → `terminate:`), `libc` (yalnız bekçinin `write` + `_exit`'i) |
| `bateri` | `main`, app bundle, Sparkle | — |

`bt-core`'un platformsuzluğu bir zevk değil kapıdır: Metalterm'in yol haritasında
"1.0'dan sonra Vulkan" var ve o kapı bu ayrımın üstüne kurulur.

## Bilinmesi gerekenler

- **Taban macOS 14, tek kaynağı `.cargo/config.toml`'daki
  `MACOSX_DEPLOYMENT_TARGET`.** rustc binary'nin minos'unu, `bt-gpu/build.rs`
  shader'ların `-mmacos-version-min`'ini oradan alır; `make kur`
  `LSMinimumSystemVersion`'ı binary'nin `minos`'undan, yani dolaylı olarak yine
  oradan doldurur. Metalterm'in tabanıyla aynı.
- **Bağımlılık mimari karardır**, kendiliğinden eklenmez. Taban:
  `alacritty_terminal` (VT ayrıştırma, grid, PTY ve okuyucu thread; kendi
  ayrıştırıcımızı yazmıyoruz — `bt-core` onu **kapsüller**, `pub` API'de
  alacritty tipi görünmez), `objc2` ailesi (CoreText ve CoreGraphics dahil;
  servo ailesi `core-text` ikinci bir CF sarmalayıcı yığını olacağı için
  **reddedildi**), `toml_edit` (ayar ve tema dosyası, yalnız `bt-core`'da;
  `toml` + `serde` yerine, çünkü menüden yazılan dosyada yorum ve bilinmeyen
  anahtar yerinde kalmalı — `.tasks/007-ayarlar-ve-tema/discussion.md` →
  Karar), `tracing`. `Cargo.lock` depodadır.
  `alacritty_terminal` **Apache-2.0**: lisans metni
  `assets/bundle/THIRD-PARTY-LICENSES.txt` ile pakete girer, atfı
  `Credits.html`'de durur; atıf isteyen yeni bağımlılık da o iki dosyaya
  yazılır (denetim yalnız `alacritty_terminal`'ı arıyor). Liste **eksik**: MIT
  paketlerinin bildirimleri borçtur (`docs/YOL-HARITASI.md`).
- **Hücre sabit boyuttadır** ve `const` assert ile bağlanır
  (`bt-core/src/lib.rs`, bugün **24 bayt**: alacritty `Cell`'i; Metalterm 20'de
  tuttu). Emoji, grapheme kümeleri ve alt çizgi rengi gibi seyrek veriler yan
  tablolarda yaşar (alacritty'de `CellExtra`); kendi hücremize geçiş
  `Session::frame()` sınırının arkasında yapılır ve renderer'ı değiştirmez.
  **Bu bir *grid* hücresidir; `frame()` sınırının `bt_core::Cell`'i ayrı bir
  kare kaydıdır ve aynı bütçeye tabi değil** — grid hücresi 10 000 satırlık
  scrollback'te sekme başına megabaytlarca yaşar, sınır hücresi yalnız çizilen
  hücreler için kare başına doğar. Sınır hücresine alan eklerken ölçüt kare
  başına maliyettir.
- **Renk uzayı sınırı geçer.** Çizim hedefi `BGRA8Unorm_sRGB`: donanım
  fragment çıktısını **lineer** sayar ve yazarken sRGB'ye kodlar. Bu yüzden
  `bt-core` sınırdan lineer float verir (`color::linear_rgba`) ve `MTLClearColor`
  da aynı temadan (`Theme::background_linear`) beslenir. İkisi **birlikte** değişir; biri lineerleşmeden
  ötekine geçilirse palet `0x1a1c21`'den `0x5a5d65` griye açılır ve belirti
  sessizdir. Gören tek bekçi `cell_bg_paints_pixels_on_the_gpu` ve ancak **ara
  ton** bir renkle görür: `0.0` ve `1.0` sRGB transfer fonksiyonunun sabit
  noktalarıdır.
- **Boşta sıfır kare.** Kirli satır yoksa frame gönderilmez. Her animasyon bir
  durma koşulu taşır; `reduce_motion` ve sistemin Reduce Motion ayarı her
  animasyonu 90 ms'lik solmaya indirir.
- **Kapanış sınırlı bekler, çocuk yine de ölmeyebilir.**
  `Session::shutdown()` `SIGHUP`'tan sonra `join`'i ve `Pty`'nin düşmesini ayrı
  bir thread'e alır ve en çok `SHUTDOWN_GRACE` (yarım saniye) bekler; sinyali yutan ya da
  çıkışın içinde takılan çocuk (`ps` durumu `?Es`) kapanışı asamaz. Tek
  istisna kapanış thread'inin kurulamamasıdır, o dalda sınır yoktur. Süre
  dolunca çocuk arkada bırakılır ve süreç çıkışı master fd'yi kapatınca gider.
  Kalıcı çare "süre → `SIGKILL`" **değil** (ölçüm çürüttü, o çocuk `SIGKILL`
  almıyor); çare `wait` bloklarken master'ı boşaltmak, yolu
  `Session::spawn`'da `pty.file().try_clone()` — `EventLoop` `Pty`'yi
  `join`'den sonra vermediği için kopya baştan alınmak zorunda. Sonuç
  `Teardown` olarak döner ve süreli koşu onu `kapanis=` jetonuyla basar
  (değerler `teardown_token`'da). Duman bekçisi (`_exit(70)`) kapanış yolunun
  başka asılmalarına karşı durur.
- **Render yolu bloklanmaz.** PTY okuma ve ayrıştırma kendi thread'inde; AppKit
  çağrıları `MainThreadMarker` ile ana thread'de; renderer `CAMetalDisplayLink`
  ile sürülür.
- **PTY ve ayrıştırma yolunda panik yok.** Bilinmeyen dizi yoksayılır, loglanır
  (`make denetim` `bt-core`'da gerekçesiz `unwrap`/`expect`/`panic!` arar).
  Loglama yarısı **henüz borç**: `tracing` bağlanmadı, yoksayılan olaylar ve
  alacritty'nin `log` satırları sessizce düşüyor; logger gelince bu cümle kalkar.
- **`tty::setup_env()` çağrılmaz**: *kendi* sürecimizin ortamını değiştirir ve
  makinede alacritty kuruluysa `TERM=alacritty` yazar. Çocuğun ortamı
  `tty::Options.env` ile verilir: `TERM=xterm-256color`, `COLORTERM=truecolor`
  — `SessionOptions.env`'in ek ortamı ikisini **ezemez**. Alacritty
  `ALACRITTY_WINDOW_ID` ve `WINDOWID`'yi koşulsuz yazar; shell'de görünürler.
  **Dizin ve yerel de yalnız çocuğa gider:** kabuk ev dizininde başlar (`HOME`,
  yoksa passwd kaydı; mutlak değilse miras). Ortamda `LC_ALL`/`LC_CTYPE`/`LANG`'dan
  hiçbiri boş olmayan bir değer taşımıyorsa macOS'un dil/bölge ayarından
  `LANG={dil}_{bölge}.UTF-8`, o yerel `/usr/share/locale`'de yoksa
  `LANG=en_US.UTF-8` alır — alacritty'nin `LC_CTYPE=UTF-8`'i değil (gerekçe
  `child::decide_locale`'in doc'unda). Dil `NSLocale.preferredLanguages`'tan
  okunur: paketin içinde `currentLocale().languageCode` **paketin** dilini
  (`en`) verir ve `cargo run` bunu göstermez. Kendi sürecimizde
  `set_current_dir`, `set_var` ve `setlocale` **yok**; `LC_ALL` değil `LANG`
  yazılır ki kabuğun rc dosyası `LC_*`'ı üstüne yazabilsin. Politika
  `bt-shell`'de (`child`), `bt-core` yalnız geçirir
  (`SessionOptions.working_directory`, `.env`). Sebep Dock'tan açılış:
  LaunchServices süreci `cwd=/` ile başlatıyor ve launchd'nin ortamında `LANG` yok.
- **Tema = sekiz rol:** arka plan, ön plan, dim, accent ve dört durum. Bugün
  dördü tüketiliyor — `background`, `foreground`, `dim` (SGR 2'li varsayılan ön
  plan), `accent` (imleç) — ve yanlarında `[ansi]`'nin 16 rengi; durum rolleri
  013 ile gelir. `bt_core::Theme` paletin **tek kaynağı**: zemin atlaması,
  clear, imleç ve renk sorusunun yanıtı aynı değerden. `Adapter`'da **yaprak
  kilit** altında durur; `frame()` kopyayı `Term` kilidinden önce alır.
  Materyal yüzey (grain, sheen) bunun üstüne ayrı bir katmandır ve `substrate`
  shader'ı çizer. Palet dosyaları `~/.config/bateri/themes/*.toml`, her
  anahtar opsiyonel ve eksiği gömülü `bateri`'den; biçim `docs/AYARLAR.md` →
  Temalar.
- **Ayarlar** `~/.config/bateri/settings.toml`; bilinmeyen anahtar korunur,
  anahtar silinmez, ayar penceresi dosyayı yeniden yazar ama tanımadığını bırakır.
  Anahtarlar, varsayılanlar ve hata davranışı (pencere alt başlığı)
  `docs/AYARLAR.md`'de; ayrıştırma `bt-core::settings`'te saf, okuma
  `bt-shell`'de. Süreli koşu (`BT_RUN_SECONDS`) dosyayı **hiç okumaz**: dalın
  tek yeri `bt-shell`'in `app::Inputs`'u.
- **Shell entegrasyonu** üç kabuk içindir (zsh `ZDOTDIR`, bash `--rcfile`
  sarmalayıcısı, fish `vendor_conf.d`) ve kullanıcının rc dosyasına **asla**
  dokunmaz. Komut blokları OSC 133 işaretlerinden okunur.
- **Ölçülmemiş sayı yazılmaz.** Tek sahip `docs/OLCUMLER.md` (dosyanın başı
  hangi türün sayısı olduğunu söyler); ölçüm bir kapı değildir, `/measure` ile
  kullanıcı ister. Zaman kancaları env'dir: `BT_SCROLL_TEST` yükü seçer (boşta
  bir pencere kare süresi vermez), `BT_FRAME_STATS` ölçümü açar; ikisi de
  `BT_RUN_SECONDS`'ı **sıfırdan büyük** ister, yoksa süreç çıkış 1 verir —
  rapor yalnız deadline yolunda basılır, süresiz ölçüm örnekleri sessizce
  atardı. Kapı kapalıyken tek bir saat okuması bile yok. Kancanın dürüst
  sınırları (**kapsam** / **açık kalem** etiketli) `bt-shell`'de `Measured`'ın
  doc'unda emanettir; o türün ilk `/measure`'ı onları `docs/OLCUMLER.md`'nin
  `## Yöntem`'ine taşır. **Bench seti borçtur:** `criterion` ayrı bir
  bağımlılık kararı; `cargo bench` satırı yukarıdaki komut bloğuna bench seti
  gelince döner. Giriş gecikmesi zinciri (`BT_INPUT_LATENCY_SAMPLES`) ve düşen
  kare sayımı da aynı durumda. Hangi iddianın hangi araca baktığı `/measure`
  skill'inin tablosunda, bekleyen iddialar setlerin `teslim.md`'lerinde.
- **Dil:** yorumlar, commit iletileri ve belgeler Türkçe ve "neden"i anlatır.
  **Kod tanımlayıcılarının tamamı İngilizce** — pub adlar da, yerel yardımcı,
  alan, değişken ve sınama adı da; `build.rs` dahil, istisnasız. UI dizgileri,
  ayar anahtarları, tema ve materyal adları İngilizce. **Üç öbek Türkçe kalır ve
  üçü de kod değildir:** tanı metni (stderr, `assert!` gerekçeleri, `make
  duman`'ın düşen koşuda bastığı açıklama); `Makefile` hedefleri (projenin
  komut yüzeyi); süreli koşunun jeton satırındaki **anahtarlar** (`kare=`,
  `hucre=`, …). Pencerede görünen tanı (alt başlıktaki ayar hatası) tanı
  metni değil **UI dizgisidir**, İngilizce; stderr'e aynı metin kopyalanır.
  Jeton satırı bir **makine sözleşmesidir**: anahtar Türkçe ve
  donmuş, **değer İngilizce**, tanı metni satırın dışında (gerekçe
  `Report::token_line`'ın doc'unda). Depo geneli kural: **jeton silinmez,
  eklenir** — okuyan taraf tanımadığı jetonu atlayabilir, kaybolanı arayamaz.
  `ATLANDI` da aynı sözleşmenin parçası.

## İş akışı

Çok oturumlu ve tasarım kararı içeren işler `.claude/` altındaki zincirle
yürür: `/rfc → /plan-review → /implement → /ship`, sürücüsü `/akis`. Kurallar
`.claude/README.md` ve `.claude/is-akisi/`'de; iş setleri `.tasks/` altında.
Tek dosyalık düzeltme için set açılmaz; set yürürken çıkan tek commit'lik
düzeltme de phase açmaz. Her phase'in kapısı `make hepsi`'dir, `/code-review`
ve `/audit` set sonunda bir kez koşar.

Hangi işin **neden o sırada** olduğu `docs/YOL-HARITASI.md`'dedir; henüz
açılmamış setlerin sırası ve bağımlılıkları oraya yazılır. **Durumu** o dosya
tutmaz — tek sahibi `.tasks/README.md` indeksidir, iki yerde durum tutmak
drift üretir.
