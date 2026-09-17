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
kaydırma, ana menü (About, Settings…, Quit; Edit'te Copy/Paste; View'da
Theme ▸ ve Cmd +/−/0 geçici punto) ve kapanış sırası ondadır; uygulamanın
OSC 52 kopyasını (`Wake::copy_to_clipboard`) genel panoya o yazar;
`settings.toml`'u okur (bugün `scrollback`, tema seçimi, font, `osc52`,
`cursor_motion`, `reduce_motion`, `shell.integration` ve `shell.prompt`),
Theme ▸'nin seçimini oraya
yazar ve temayı `themes/{ad}.toml`'dan ya da gömülü
`bateri`/`bateri-light`'tan çözer. Ayar ve etkin tema dosyası **kayıt
anında** uygulanır (`watch`: vnode kaynakları; `Session::set_theme`,
`Session::set_terminal_options`, `Renderer::set_font`,
`DisplayLink::set_cursor_motion`, `DisplayLink::set_reduce_motion`);
varsayılan tema sistemin açık/koyu görünümünü, `reduce_motion = "system"` de
sistemin Hareketi Azalt ayarını canlı izler; tek istisna `[shell]` bölümü
(`integration` ve `prompt`), kabuk çoktan doğduğu için **sonraki oturumda**
geçerlidir. Kabuk zsh ise
`bt-shell` sarmalayıcıyı `ZDOTDIR` ile kurar (betik `.app`'in
`Contents/Resources/shell`'inden, debug'da depodan) ve kabuğun bastığı OSC 133
işaretleri `Session::shell_state()`'te birikir. Aynı betik her satır çiziminde
ZLE'nin görüntüsünü (`PREDISPLAY`, `BUFFER`, `POSTDISPLAY`, `region_highlight`,
`CURSOR`) OSC 8133 ile aynalıyor; `Session::dock()` onu **çözülmüş** dock
hücrelerine çevirip sınırdan veriyor ve `bt-gpu` pencerenin altındaki **ikinci
bir `setViewport`**'la çiziyor — kendi listeleri, kendi caret'i, opak zemini ve
ızgaradan ayıran saç çizgisiyle. Payı `DOCK_ROWS * cell_h` **artı iki nefes
payı** (`bt_gpu::dock_px`; formülün tek kopyası orada, `split_into_grid` onu
tüketiyor): iki satır saç çizgisine yapışınca dock bakılamaz duruyordu. Payın
kaynağı sol payın ta kendisi (`CellMetrics::gutter_px`) — ikinci bir tasarım
sabiti yok, aynı içi girinti iki eksende ve punto büyüyünce pay da büyüyor.
Saç çizgisi payın **üstünde**, viewport'un tepesinde. Dock ötelemeden
**yapısal olarak** muaf:
listeleri dock-yerel doğuyor, ekrana taşıyan şey o ikinci viewport. Üst
satırında `>` işareti (safha rengiyle), metin, sönük öneri,
`region_highlight` renkleri ve caret var; alt satırında **bağlam** —
`{tam yol} | {dal}`, sol altta ve sönük. Bağlamın iki ucu iki ayrı yerden:
dizin **OSC 7**'den (tarayıcının üçüncü kolu; `file://` yetkisi boş ya da
`localhost` olmalı, adlı host yabancı sayılır), dal aynanın kanalından
(`8133;b`, `precmd`'de bir `git rev-parse` fork'u). İkisi de aynanın
**yanında** yaşıyor (`DockContext`), içinde değil: ayna tuş başına gelip
`line-finish`'te sıfırlanıyor, bağlam prompt başına gelip komut koşarken de
duruyor. Taşmada yol **soldan** kısalır (`…` önekiyle), dal asla kısalmaz;
karar `bt-core`'da, çizen taraf yalnız hücreleri alır. Dock payı ızgaranın satırlarından
düşülüyor ve **yalnız entegrasyonlu zsh oturumunda** ayrılıyor — ayrım oturum
doğarken kararlaşıyor, yani `/bin/sh` koşan duman reçetesi dock almıyor.
**Alternatif ekranda dock kalkıyor** (vim, htop, `less`): `frame()` bayrağı
`Term` kilidi altındayken yayınlıyor (`Session::alt_screen`), kare yolu onu her
karede karşılaştırıyor ve değişince `bt-shell`'e enjekte edilmiş haberciyi
çağırıyor; resize **çizilen karenin içinde değil**, `dispatch2` ana kuyruğunun
bir sonraki turunda koşuyor. Bedel komut başına değil **geçiş başına**: `git
log` gibi alternatif ekrana girmeyen komutlar hiç resize görmüyor. Dock'u
olmayan pencerede haberci **hiç kurulmuyor**, yani yol yapısal olarak kapalı ve
alternatif ekrandan çıkış orada dock doğurmuyor.
Giriş satırı ızgarada **çizilmiyor**: kabuk `Input` safhasındayken ve ayna
canlıyken (`ShellLog::suppressed_input`; karar `Term` kilidinden **önce**
okunuyor, `Theme` örüntüsü) yazılmakta olan bloğun çıpa satırından imlecin
satırına kadar hücreler sink'e uğramıyor — caret dock'ta.
Kapı çıpa taramasından **sonra**, yoksa blok şeridi de ölürdü. Ayna
gösteremiyorsa (`Unavailable`), ZLE satırı bırakmışsa (`Idle`) ya da ayna
**bayatsa** bastırma **yok**: gösteremediğimiz satır ızgarada kalmak zorunda.
Bayatlık kip sezerek değil iki kesin veriyi karşılaştırarak anlaşılıyor —
ızgaranın son mürekkebi ile aynanınki (`DockState::last_ink`); yanlış alarmın
yönü güvenli, satırı iki yerde gösterir ama sessizce kaybetmez.
**Devrin tek yüklemi var** (`shell::caret_home` + üç ön koşul) ve **üç
tüketicisi**: hangi hücrelerin atlanacağı, imlecin çizilip çizilmeyeceği ve
**doluluk sayısı**. Üçü ayrı sorulduğunda ayrışıyorlardı ve belirti ölçüldü:
boş prompt'ta hiçbir hücre çıpayı taşımadığı için satır çizilmiyor ama
doluluğa **giriyordu**, ilk tuşta çıpa doğunca doluluk bir satır düşüyor ve
ızgaranın tamamı oynuyordu — satır gizliydi ama yer kaplıyordu. Tek yüklem
`display: none` veriyor. Caret'in sahibi satırın nerede çizildiğine uyuyor:
komut koşarken (`Running`), ayna gösterilemiyorken (`Unavailable`) ve ZLE
satırı bırakmışken (`Input` + `Idle`; `CORRECT`'in `[nyae]`'i, R3.3) ızgaranın;
**kalan her hâlde dock'un** — kabuğun henüz hiç konuşmadığı açılış, prompt
çizilirken ve iki komut arası (`Finished`, içinde bir `git` fork'u) dahil,
çünkü sıçrayan caret tam da o pencerelerde görülüyordu. Üç ön koşul:
pencerenin dock'u olacak (`SessionOptions::dock`; yoksa devralacak kimse yok
ve satır da imleç de ızgarada kalır), alternatif ekranda olmayacak (dock zaten
kalkıyor) ve bastırılan bir satır varsa tazelik kapısı geçilecek. Metinsiz dock
satırı caret'siz değil: `Live` olmayan aynada caret satırın başında duruyor.
Bastırmanın kendi gerekçesi
ölçülmüş: `bracketed-paste-magic` yapıştırmayı `zle -U` ile kuyruğa geri
basıyor, ZLE typeahead varken redisplay'i atlıyor ve ayna bir sonraki tuşa
kadar güncellenmiyor. Aynı ölçüm yapıştırmaya **dar bir istisna** getirdi:
dock satırın sahibiyken, **ZLE ekleme keymap'indeyken**, tek satırlık ve
kontrol karakteri taşımayan yük bracketed sarmadan akıtılıyor
(`Session::can_be_typed`) — satır sonu yoksa hiçbir şey kendiliğinden
çalışmaz, kontrol karakteri yoksa hiçbir bağlama tetiklenmez, yani sarmanın
koruduğu iki şey de koşulun dışında. Keymap koşulu şart: `vicmd`'de aynı
baytlar metin değil **komut** olurdu (panodaki `dd` satırı siler) ve keymap
aynanın altıncı gövdesiyle geliyor; bilinmeyen ya da hiç gelmemiş keymap
istisnayı **kapatıyor**. Prompt artık
**terminalin**: `PS1` ile `RPS1` sıfır görünür genişliğe iniyor ve prompt'un
yerini dock'un `>` işareti alıyor. Dayatma **iki yerden** ve ikisi de zorunlu —
`precmd` ilk basımı doğru yapıyor, aynanın ZLE kancası temanın geri yazdığını
`zle reset-prompt` ile geri alıyor (p10k/starship `PS1`'i `precmd`'den **sonra**,
kendi ZLE kancalarından kuruyor; ölçüldü: kancadan atanan `PS1` `reset-prompt`
olmadan ekranı hiç etkilemiyor, çünkü prompt `line-init` koşmadan basılıyor ve
zsh genişlettiği hâli tutuyor). Nöbet prompt başına **tek** sıfırlama bırakıyor,
yoksa `reset-prompt` kendi kancasını besler. `[shell] prompt = "shell"`
prompt'u kullanıcıya geri veriyor ve **dock'u kapatmıyor** — dock kabuğun
prompt'unu değil ZLE'nin tamponunu çiziyor; `shell.integration` ile aynı sınıf,
yani **sonraki oturumda** geçerli. Çıpanın kapanışı `PS1`'in sonunda değil
**`preexec`'te**: sıfır genişlikli prompt hiçbir hücre yazmadığı için kapanış
orada kalsaydı çıpayı taşıyan hücre hiç doğmaz, blok şeridi **ve** bastırma
birlikte sessizce ölürdü. Bağlantı `Input` boyunca açık, komutun çıktısında
kapalı. Her prompt bir blok kimliği
basar, `frame()` o kimlikleri prompt'un OSC 8 çıpasından okuyup blokları
**komutun satırı ve rengi** olarak sınırdan verir; `bt-gpu` o işareti
ızgaranın solunda ayrılan paya `cell_bg` pipeline'ıyla çizer —
**animasyonsuz**, işaret anında belirir. İşaret komutun kendi satırında,
çıktısında **değil**: hangi satırın hangi bloğa ait olduğu ancak çıpası
görünen satırlar için biliniyor ve bölge boyamak onu tahmine çevirirdi.
**İçerik pencerenin tabanına yaslanır**: `frame()` kaç satırın dolu olduğunu
sınırdan verir (`Cursor::content_rows`; alternatif ekranda ızgaranın tamamı),
`DisplayLink` onu `rows - content_rows` ile ötelemeye çevirir ve `encode_pass`
tek bir `setViewport` ile iki pipeline'ı birden kaydırır — dört liste ve imleç
aynı yerden. Öteleme **yumuşak kayar**: `bt-gpu::motion`'ın ikinci animatörü
(`Slide`) onu imleçle aynı stil ve aynı `settled()` kapısı altında sürer, imlecin
hedefi de **ekran satırıdır** (`row + origin`), yani Enter'da imleç dipteki
satırında durur ve geçmiş arkasından yukarı akar. Kayma **tek yönlüdür**:
içerik büyüyünce (hedef düşünce) süzülür, daralınca (vim'den çıkış, dolu
ekranda `clear`) **snap**'ler — yukarı akış içeriğin gelmesi, aşağı iniş
düşmesi gibi okunuyor; ölçüt mesafe değil işaret, çünkü eşik ölçülmemiş bir
sayı olurdu. Tekerlek ve geometri (pencere/font/punto) ayrıca snap'ler.
Piksel aygıt ızgarasına
yuvarlanır (`Frame::set_origin_rows`): kaymanın durduğu kare ekranda kalıcı ve
kesirli bir piksel bütün metni bulanıklaştırırdı. Ötelemenin tek sahibi
kare yolu; fare eşlemesi onu `bt_gpu::Origin` ile **encode edilen** değerden
okur.
Dock ve komutlar arası atlama henüz yok. `make kur` `bateri.app` paketini
üretir.
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
                  # kare=N hucre=K glif=G kural=R yuva=U/T yuk=smoke istek=I icerik=C hareket=M kayma=S sessiz=Sms kapanis=clean profil=debug ornek=off pipeline=ok
                  # ilk dördünden ya da hareket'ten biri 0 ise, icerik > IDLE_FRAME_LIMIT ise, sessiz < QUIET_FLOOR ya da sessiz=none ise
                  # ya da deadline'da animasyon yerleşmemişse kırmızı. iki sınır da ölçülmüş; değerleri ve türetmeleri sabitlerin doc'unda.
                  # üst sınır kare'de değil icerik'te: icerik çizilmeye karar verilen kare, kare GPU'nun bitirdiği — animasyon ikincisini meşru olarak şişirir.
                  # sessiz'in kuralı ters (sağlıklıda büyük) ve kapının en duyarlı katı: icerik sınırının göremediği yavaş sızıntıyı o görüyor.
                  # yuva/yuk/istek/kayma/profil sayaç ve etiket; kapanis kısmen kapı (değerler teardown_token'da); ornek=off'ta ölçüm jetonu basılmaz.
                  # hareket ile kayma iki ayrı animatörün tanığı (imleç / içeriğin ötelemesi): aynı karede ikisi birden artabilir, toplamları kare değildir.
make terminfo     # assets/terminfo'yu tic -x ile geçici dizine derler
make test-yaris   # yarış stresi: race_* (--ignored) + tek thread karşılaştırma koşusu
make kur          # release derler, target/release/bateri.app'i kurar ve içeriğini denetler (Info.plist, ikon, lisans, shell betiği); imza yok
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
| `bt-core` | VT durum makinesi, grid ve scrollback, PTY ve okuyucu thread (PTY okuma yolu **taranıyor**: araya giren sarmalayıcı baytları aynen geçirir, geçerken **üç** OSC numarasını çeker), OSC (7/8/9/52; 7 çalışma dizinini dock'un bağlam satırına verir, 52'nin yazma yönü `Wake` ile kabuğa çıkar, panoyu görmez), komut blokları, seçim, girdi kodlaması (DECCKM'e uyan oklar, tekerlek raporu), ayar modeli, shell bağlamı. Tarayıcının üç kolu var ve üçü de alacritty'de **yok** (`vte` üçünü de `unhandled`'a düşürüyor): OSC 133 oturumun safhasını ve blok kimliklerini `ShellState`'e yazar (`Session::shell_state()`), OSC 8133 ZLE'nin görüntü aynasını — `PREDISPLAY`, `BUFFER`, `POSTDISPLAY`, `region_highlight`, `CURSOR`, base64 gövdelerle — çözüp `DockState`'e (`Session::dock_state()`) ve dalı `DockContext`'e, OSC 7 de çalışma dizinini yine `DockContext`'e (yüzde çözme ve yabancı host elenmesi orada; bozuk URI panik değil yoksayma). Aynanın kendi yük sınırı var ve aşımı **görünür** (`DockStatus::Unavailable`), sessizce düşmez. Komut blokları `frame()` sınırından **çözülmüş** geçer (komutun satırı + renk, çıkış kodu değil; bölge değil işaret): kimlik prompt'un OSC 8 çıpasından `Term` kilidi altında toplanır, renk kilit bırakıldıktan sonra kabuk defterinden çözülür. Giriş satırının **bastırılması** da burada: safha ile aynanın durumu tek yüklemde birleşiyor (`ShellLog::suppressed_input`) ve kopya `Term` kilidinden **önce** alınıyor — yaprak kilit `Term`'ün altına girmez | macOS'a özgü **hiçbiri** — `objc2*`, `core-text`, `metal` yok. Unix PTY (`libc`, `rustix`, `polling`) serbest; kapı Linux hedefiyle derlemedir |
| `bt-atlas` | glyph rasterizasyonu, atlas paketleme, kutu çizim karakterleri, font seti | `objc2-core-text`, `objc2-core-graphics` ve ortak tabanları `objc2-core-foundation`. `objc2` çekirdeğini bile **görmez**: kullanılan her şey C API'si, ObjC runtime'ı değil |
| `bt-gpu` | Metal renderer, shader'lar (`.metal`), display link ve `Waker` (kareyi süren ritim), kare yolunun **ölçüm defteri** (`Stats`: iki CPU aralığı, GPU deltası, açılış damgası, p95'in tabanı — biriktirir, **basmaz**), hareket (motion), **dock yüzeyi** (ikinci `setViewport`, kendi listeleri ve caret'i; kaç satır olduğu `DOCK_ROWS`), overlay'ler (palet, arama), durum çubuğu | `objc2`, `objc2-foundation`, `objc2-metal`, `objc2-quartz-core`, `dispatch2` (metallib yükleme, ana kuyruk), `block2` (tamamlanma bloğu) |
| `bt-shell` | AppKit kabuğu: pencere, sekme, bölme, menü, klavye, servisler, ayar penceresi; kapanış sırasının ve duman bekçisinin sahibi; kabuğun başlangıç dizini, yereli, hangi kabuğun koşacağı ve sarmalayıcı betiğinin yeri (`child`), entegrasyonun kurulup kurulmayacağı ve `ZDOTDIR`/`BATERI_ZDOTDIR` çifti (`app::shell_integration_env`) | `objc2`, `objc2-foundation` (`NSLocale` dahil: kabuğun yereli), `objc2-app-kit`, `objc2-quartz-core` (yalnız `CALayer` takma), `dispatch2` (ana kuyruk: `child_exit` → `terminate:`, OSC 52'nin pano işi; vnode kaynakları: ayar izleme), `libc` (bekçinin `write` + `_exit`'i, izlemenin `O_EVTONLY`'si, kabuğun passwd kaydı için `getpwuid_r`) |
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
  Karar), `tracing`. `polling` **yeni bir crate değil**, `libc` gibi: grafta
  zaten vardı (alacritty PTY'yi onunla yokluyor) ve `bt-core`'un listesine
  yalnız bir kenar ekliyor — `EventedReadWrite`'ı uygulamak imzadaki
  `Poller`/`Event`/`PollMode`'u adlandırmayı gerektiriyor ve alacritty onları
  yeniden ihraç etmiyor (`.tasks/009-shell-entegrasyonu/phase-2.md` → Uygulama
  Notları). `Cargo.lock` depodadır.
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
- **Boşta sıfır kare.** Kirli satır **ve** yerleşmemiş animasyon yoksa frame
  gönderilmez; kare istemenin iki yolu var ve ikincisi kimseyi uyandırmıyor
  (`bt-gpu::link` modül başlığı: zamana bağlı kare talebinin tek yolu hareket
  saatidir). Kapı bu yüzden `kare`'ye değil **içerik** karesine bakıyor —
  200 ms'lik bir imleç kayması `kare`'yi meşru olarak şişirir. Her animasyon bir
  durma koşulu taşır; `reduce_motion` ve sistemin Reduce Motion ayarı
  **imleci** 90 ms'lik bir **belirmeye** indirir — imleç kaymaz, yeni yerinde
  belirir — ve içeriğin ötelemesini **snap**'ler, çünkü her yeni satırda bütün
  ekranın belirmesi indirgemeye çalıştığı hareketten beter olurdu (kip iki,
  yer bir: `Motion::origin_mode`). Belirme **duraksamadan sonraki** harekete
  ait: belirme süresinden
  sık gelen hareketlerde (akan çıktı) imleç opak kalır, yoksa alfa sıfıra
  çakılır ve imleç büsbütün kaybolurdu.
  İndirgemenin tek yeri `bt-gpu::motion` (`Mode::Fade`); üç değerli
  ayar ile sistemin cevabı `bt-shell`'de tek `bool`'a iniyor, `bt-gpu` AppKit
  görmüyor. `cursor_motion = "snap"` bunun üstündedir: hareketi zaten kapatmış
  olana erişilebilirlik ayarı animasyon *eklemez*.
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
- **Kabuğu doğuran komutu da biz kuruyoruz** (`child::login_command`) ve
  alacritty'nin macOS yolundan **tek** farkı var: `login(1)` her zaman `-q`
  alıyor, yani `Last login: …` banner'ı ızgaraya hiç düşmüyor (ölçüldü:
  `-flp` basıyor, `-qflp` basmıyor). alacritty `-q`'yu yalnız `~/.hushlogin`
  varsa ekliyor; koşulu kaldırdık çünkü alternatifi kullanıcının ev dizinine
  dosya yazmaktı ve o yasak. Geri kalan her şey parite: `-flp`, argv[0]'ı
  `-zsh` yapan `exec -a` ve onu koşturan `/bin/zsh`. Kullanıcı ya da kabuk
  çözülemezse komut `None`'a düşer ve alacritty'nin kendi yolu geri gelir —
  banner döner, pencere çalışır. Süreli koşu (`BT_RUN_SECONDS`) bu yola
  **uğramaz**: kendi sabit betiğini verir.
- **Tema = sekiz rol:** arka plan, ön plan, dim, accent ve dört durum. Bugün
  altısı tüketiliyor — `background`, `foreground`, `dim` (SGR 2'li varsayılan ön
  plan), `accent` (imleç **ve** koşan komut bloğunun şeridi), `success` ve
  `error` (biten bloğun şeridi) — ve yanlarında `[ansi]`'nin 16 rengi; kalan iki
  durum rolü (uyarı, bilgi) 013 ile gelir. Çizilmeyen rol eklenmiyor.
  `bt_core::Theme` paletin **tek kaynağı**: zemin atlaması,
  clear, imleç, blok şeridi ve renk sorusunun yanıtı aynı değerden. `Adapter`'da **yaprak
  kilit** altında durur; `frame()` kopyayı `Term` kilidinden önce alır,
  `set_theme` tek başına yazar ve kare ister (aynı temada no-op). Sönük
  (SGR 2) adlı renk temanın zeminine doğru üçte bir karışır
  (`color::dim_toward`); `dim` rolü yalnız varsayılan ön planın.
  Materyal yüzey (grain, sheen) bunun üstüne ayrı bir katmandır ve `substrate`
  shader'ı çizer. Palet dosyaları `~/.config/bateri/themes/*.toml`, her
  anahtar opsiyonel ve eksiği gömülü `bateri`'den; biçim `docs/AYARLAR.md` →
  Temalar.
- **Ayarlar** `~/.config/bateri/settings.toml`; bilinmeyen anahtar korunur,
  anahtar silinmez. Dosyaya yazan iki yol var: Settings… yalnız dosya
  **yokken** şablonu yaratır (`settings::create_if_missing`), View ▸ Theme ▸
  yalnız `[appearance] theme`'i yazar, biçimi koruyarak
  (`Settings::with_theme`), yerinde (sembolik bağın hedefine);
  ayrıştırılamayan dosyaya yazmaz. Menü yalnız yazar, uygulayan dosyayı
  okuyan yol.
  Anahtarlar, varsayılanlar ve hata davranışı (pencere alt başlığı)
  `docs/AYARLAR.md`'de; ayrıştırma ve fark (`Settings::changes`)
  `bt-core::settings`'te saf, okuma ve izleme `bt-shell`'de. İzleme kaynağı
  okumadan **önce** kurulur ve her olayda yeniden kurulur; kayıt anında
  kullanılamayan dosya hiçbir şeyi, kabul edilmeyen değer kendi anahtarını
  değiştirmez (`Settings::parse_keeping`). **Tek istisna `osc52`:**
  kabul edilmeyen değeri ve açılışta kullanılamayan dosya (ya da
  çözülemeyen ev dizini) panoyu **kapalıya** düşürür
  (`Settings::for_unusable_file`) — yanlış tahmini görünmeyen tek anahtar. Süreli koşu
  (`BT_RUN_SECONDS`) dosyayı **hiç okumaz ve izlemez**: dalın tek yeri
  `bt-shell`'in `app::Inputs`'u.
- **Shell entegrasyonu bugün yalnız zsh'tir** (`ZDOTDIR`); bash (`--rcfile`) ve
  fish (`vendor_conf.d`) sonraki settedir. Kullanıcının rc dosyasına **asla**
  yazılmaz — kapısı `make denetim` ve listesi zsh'in beş dosyasını da kapsar.
  Betik `assets/shell/` altında **kaynaktır**, üretilmez: `make kur` onu
  pakete kopyalar ve kopyayı `cmp` ile denetler, `make hepsi` de girdi
  dizininin envanterini (`bundle_assets`). Sarmalayıcı hiçbir kolda ölümcül
  değildir ve kullanıcının özgün `ZDOTDIR`'ını geri koyar; gerekçeler
  `assets/shell/zsh/bateri.zsh`'in başlığında. Komut durumu OSC 133
  işaretlerinden okunur ve `Session::shell_state()`'te durur; satıra
  çıpalanması prompt'un OSC 8 bağlantısıyla, yani blok kimliği hücrelerde
  taşınır. bash ve fish betikleri doğduğunda çıpa satırı onlara da yazılır —
  yoksa o kabuklarda blok yok, işaret de yok.
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
