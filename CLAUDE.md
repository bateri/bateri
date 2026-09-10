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
planların üstüne alfa karıştırarak koyuyor. Ekranda okunabilir metin var;
kalanı 004'ün işi (`BOLD`/`ITALIC`/`UNDERLINE`/`STRIKEOUT`, emoji, kutu çizim).
Aşağıdaki sözleşme kod
geldikçe kodla birlikte güncellenir — buradaki bir cümle kodla çelişirse
ikisinden biri aynı commit'te düzelir.

## Komutlar

```sh
make hepsi        # rustc sürümü + fmt --check + clippy -D warnings + test (definition of done)
make fmt          # cargo fmt --all -- --check
make clippy       # cargo clippy --workspace --all-targets -- -D warnings
make test         # cargo test --workspace
make shader       # kanarya: touch shaders/*.metal + cargo build -p bt-gpu (derleme reçetesi yalnız build.rs'te)
make duman        # uygulamayı BT_RUN_SECONDS=3 ile açar; kare, arka plan hücresi ve glyph sayar: kare=N hucre=K glif=G pipeline=ok, biri 0 → kırmızı
make terminfo     # assets/terminfo'yu tic -x ile geçici dizine derler
make test-yaris   # yarış stresi: yaris_* (--ignored) + tek thread karşılaştırma koşusu
make kur          # release derler ve bateri.app paketini target/ altına kurar
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
| `bt-core` | VT durum makinesi, grid ve scrollback, PTY ve okuyucu thread, OSC (7/8/9/52), komut blokları, seçim, ayar modeli, shell bağlamı. OSC 133 alacritty'de **yok**: komut blokları `frame()` sınırına kanca isteyecek (00X) | macOS'a özgü **hiçbiri** — `objc2*`, `core-text`, `metal` yok. Unix PTY (`libc`, `rustix`) serbest; kapı Linux hedefiyle derlemedir |
| `bt-atlas` | glyph rasterizasyonu, atlas paketleme, kutu çizim karakterleri, font seti | `objc2-core-text`, `objc2-core-graphics` ve ortak tabanları `objc2-core-foundation`. `objc2` çekirdeğini bile **görmez**: kullanılan her şey C API'si, ObjC runtime'ı değil |
| `bt-gpu` | Metal renderer, shader'lar (`.metal`), display link ve `Waker` (kareyi süren ritim), hareket (motion), overlay'ler (palet, arama), durum çubuğu | `objc2`, `objc2-foundation`, `objc2-metal`, `objc2-quartz-core`, `dispatch2` (metallib yükleme, ana kuyruk), `block2` (tamamlanma bloğu) |
| `bt-shell` | AppKit kabuğu: pencere, sekme, bölme, menü, klavye, servisler, ayar penceresi; kapanış sırasının ve duman bekçisinin sahibi | `objc2`, `objc2-foundation`, `objc2-app-kit`, `objc2-quartz-core` (yalnız `CALayer` takma), `dispatch2` (ana kuyruk: `child_exit` → `terminate:`), `libc` (yalnız bekçinin `write` + `_exit`'i) |
| `bateri` | `main`, app bundle, Sparkle | — |

`bt-core`'un platformsuzluğu bir zevk değil kapıdır: Metalterm'in yol haritasında
"1.0'dan sonra Vulkan" var ve o kapı bu ayrımın üstüne kurulur.

## Bilinmesi gerekenler

- **Taban macOS 14, tek kaynağı `.cargo/config.toml`'daki
  `MACOSX_DEPLOYMENT_TARGET`.** rustc binary'nin minos'unu, `bt-gpu/build.rs`
  shader'ların `-mmacos-version-min`'ini oradan alır; ileride `Info.plist`'in
  `LSMinimumSystemVersion`'ı da oradan türetilir. Metalterm'in tabanıyla aynı.
- **Bağımlılık mimari karardır**, kendiliğinden eklenmez. Taban:
  `alacritty_terminal` (VT ayrıştırma, grid, PTY ve okuyucu thread; kendi
  ayrıştırıcımızı yazmıyoruz — `bt-core` onu **kapsüller**, `pub` API'de
  alacritty tipi görünmez), `objc2` ailesi (CoreText ve CoreGraphics dahil:
  servo ailesi `core-text` **reddedildi**, ikinci bir CF sarmalayıcı yığını
  olurdu — 003 kararı), `toml` + `serde`, `tracing`. `Cargo.lock` depodadır.
- **Hücre sabit boyuttadır** ve `const` assert ile bağlanır; emoji, grapheme
  kümeleri ve alt çizgi rengi gibi seyrek veriler yan tablolarda yaşar
  (alacritty'de `CellExtra`). Bugünkü sabit **24 bayt**: alacritty `Cell`'i
  (Metalterm 20'de tuttu). Assert `bt-core/src/lib.rs`'tedir; kendi hücremize
  geçiş `Session::frame()` sınırının arkasında yapılır ve renderer'ı değiştirmez.
- **Renk uzayı sınırı geçer.** Çizim hedefi `BGRA8Unorm_sRGB`: donanım
  fragment çıktısını **lineer** sayar ve yazarken sRGB'ye kodlar. Bu yüzden
  `bt-core` sınırdan lineer float verir (`color::lineer_rgba`) ve `MTLClearColor`
  da aynı kaynaktan beslenir — pencere zemini ile hücreler tek yerden düzelir.
  İkisi **birlikte** değişir; biri lineerleşmeden ötekine geçilirse palet
  `0x1a1c21`'den `0x5a5d65` griye açılır ve belirti sessizdir. Gören tek bekçi
  `cell_bg_pikseli_gpu_tarafinda_boyar` ve ancak **ara ton** bir renkle görür:
  `0.0` ve `1.0` sRGB transfer fonksiyonunun sabit noktalarıdır.
- **Boşta sıfır kare.** Kirli satır yoksa frame gönderilmez. Her animasyon bir
  durma koşulu taşır; `reduce_motion` ve sistemin Reduce Motion ayarı her
  animasyonu 90 ms'lik solmaya indirir.
- **Kapanış bloklar ve bunun bir sınırı var.** `Session::shutdown()`
  `SIGHUP`'tan sonra çocuğu bekler; sinyali yutan bir çocuk (`trap '' HUP`)
  ana thread'i süresiz bekletir. Duman koşusunda bekçi thread bunu keser
  (`_exit(70)`), etkileşimli kullanımda **kesen yok** — bilinen borç; kalıcı
  çözüm `bt-core`'da sınırlı bekleme (`SIGHUP` → süre → `SIGKILL`).
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
  `COLORTERM=truecolor`. Buna karşılık alacritty `ALACRITTY_WINDOW_ID` ve
  `WINDOWID`'yi koşulsuz yazar ve kapatılamaz — shell'de görünürler.
- **Tema = sekiz rol:** arka plan, ön plan, dim, accent ve dört durum. Materyal
  yüzey (grain, sheen) bunun üstüne ayrı bir katmandır ve `substrate` shader'ı
  çizer. Palet dosyaları `~/.config/bateri/themes/*.toml`.
- **Ayarlar** `~/.config/bateri/settings.toml`; bilinmeyen anahtar korunur,
  anahtar silinmez, ayar penceresi dosyayı yeniden yazar ama tanımadığını bırakır.
- **Shell entegrasyonu** üç kabuk içindir (zsh `ZDOTDIR`, bash `--rcfile`
  sarmalayıcısı, fish `vendor_conf.d`) ve kullanıcının rc dosyasına **asla**
  dokunmaz. Komut blokları OSC 133 işaretlerinden okunur.
- **Ölçülmemiş sayı yazılmaz.** Tek sahip `docs/OLCUMLER.md`; ölçüm bir kapı
  değildir, `/measure` ile kullanıcı ister. Kuralın ikinci yarısı **henüz
  borç**: ölçüm kancaları (`BT_FRAME_LOG`, `BT_SCROLL_TEST`,
  `BT_STARTUP_TRACE`) ve bench hedefleri yok, `docs/OLCUMLER.md` de yok —
  `/measure` bugün sayı değil **ölçüm aracı yok** döndürüyor. Bekleyen yedi
  iddia 002 ve 003'ün `teslim.md`'lerinde duruyor; kancalar gelince bu cümle
  kalkar ve `cargo bench` satırı yukarıdaki bloğa geri gelir.
- **Dil:** yorumlar, commit iletileri, belgeler Türkçe ve "neden"i anlatır;
  UI dizgileri, ayar anahtarları, tema ve materyal adları İngilizce. Kod
  tanımlayıcılarında dışa bakan ad (pub tip, fonksiyon, varyant) İngilizce;
  yerel yardımcı, `build.rs` ve `Makefile` hedefi Türkçe olabilir. Süreç ve
  tanı çıktısı (stderr iletileri, `make duman` satırları) UI dizgisi değildir,
  Türkçe kalır; `kare=`/`hucre=`/`glif=`/`pipeline=ok`/`ATLANDI` gibi anahtar-değer
  jetonları makine sözleşmesidir: **silinmez, eklenir** — okuyan taraf
  tanımadığı jetonu atlayabilir, kaybolanı arayamaz.

## İş akışı

Çok oturumlu ve tasarım kararı içeren işler `.claude/` altındaki zincirle
yürür: `/rfc → /plan-review → /implement → /ship`, sürücüsü `/akis`.
Kurallar `.claude/README.md` ve `.claude/is-akisi/`'de; iş setleri `.tasks/`
altında. Tek dosyalık düzeltme için set açılmaz.
