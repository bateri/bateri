# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Proje

`bateri`, macOS için GPU'nun (Metal 3) çizdiği bir terminal emülatörüdür. Rust
ile yazılır; AppKit ve Metal'e `objc2` ailesi üzerinden **doğrudan** bağlanır,
Swift katmanı yoktur. Referans ürün Metalterm'dir (metalterm.dev, kapalı
kaynak): komut blokları, dokuz rollü tema modeli, grain/sheen ile materyal
yüzeyler, fizik tabanlı imleç hareketi ve boşta sıfır kare. Referansın binary
incelemesinden çıkan mimari, özellik ve ayar envanteri `docs/ARASTIRMA.md`'dedir;
bir işe başlamadan önce ilgili bölümüne bakılır, sıfırdan keşfedilmez.

**Bugünkü hâl** (hangi setin neyi getirdiği `.tasks/README.md`'de): `bt-core`
shell'i çalıştırır ve kareyi `frame()` sınırından verir — karakter, ön plan
rengi ve biçim (`bold`, `italic`, `underline`, `underline_color`,
`strikeout`). `bt-atlas` CoreText ile dört font yüzünün glyph'lerini ve yedi
yordamsal sprite'ı (beş alt çizgi, üstü çizili ve prompt chevron'u) sabit yuva
ızgarasında rasterize eder; hücre ölçüsü oradan gelir ve `bt-gpu`
`Renderer::cell_metrics(scale)` ile yeniden yayınlar. **Seçili fontta olmayan
karakter sistemin cascade'inden geliyor** (`font::fallback_font`, 019): yüz
merdiveni tükendikten sonra, negatif önbellekten önce, yani anahtar başına
atlasın ömründe bir kez ve kabul edilen aday sıradan bir yuvaya düşüyor —
yeni önbellek, yeni tavan, yeni tahliye yok. **Kapı geometrik**: adayın
**boyayacağı piksel** hücrenin dışına taşıyorsa kutu kalıyor, ve emoji,
`.LastResort`, CJK ile geniş matematik harfi o **tek** kapıdan eleniyor — aile
adı karşılaştırması, trait biti ve sihirli dizge yok (oranlar ölçüldü, sayıları
`.tasks/019-glyph-yedegi/phase-1.md` → Uygulama Notları). Ölçülen şey
**mürekkep**, ilerleme değil, ve ölçüt sonradan değişti: 019 kapıyı
ilerlemeye kurmuştu ve belirtisi kullanıcıda görüldü — Claude Code'un araç işareti `⏺`
(U+23FA) kutu çıkıyordu, çünkü STIX Two Math'ten gelen aday hücreden %4.6
geniş **ilerliyor** ama %8.6 dar **boyuyor**. 019'un kalibrasyon örneklerinde
(2.17× / 1.83× / 1.66×) 1.0'ın yakınında hiçbir aday yoktu, yani kapı sembol
fontlarına karşı hiç sınanmamıştı. Ölçüt ters yöndeki boşluğu da kapatıyor:
dar ilerleyip geniş boyayan aday eskiden geçip sağdan kırpılıyordu, artık
kutu — yani "kutu ya da tam glyph" ilk kez bir dilek değil sözleşme.
Alternatifi yarım çizilmiş bir glyph'ti: kutu görünür bir eksiklik, kırpılmış
glyph sessiz bir bozulma. Ölçüt **yatay ve yalnız yatay**; dikeyi de sınamak
bugün hiçbir adayı elemiyor (ölçüldü: dikeyde taşan tek küme emoji ve o zaten
yatayda dönüyor), o yüzden dikey taşma kutuya değil kırpmaya düşüyor ve sınır
adıyla yazılı (`font::ink_fits_cell`). Kaydırmanın formülü **tek yerde**
(`font::centre_shift`, iki tüketici): kapı adayın **çizileceği** yerdeki
mürekkebini ölçmek zorunda, yoksa çizilmeyen bir yerleşimi sınardı. Boy sınıfının ikisi
de **ayrı ayrı** değerlendiriliyor; yedeğin tabanı ile sınırı o sınıfın kendi
fontu, yoksa küçük satıra büyük punto glyph düşerdi. Glyph hücrede
**ortalanıyor** ve kural evrensel, yedeğe koşullu değil: eşaralıklı taban
fontta her glyph'in ilerlemesi hücrenin ilerlemesinin ta kendisi (ölçüldü, beş
yüzün beşinde de — dört yüz artı küçük sınıf), yani kaydırma tam olarak sıfır
ve raster bit bit aynı. Ortalamanın girdisi hücrenin **kesirli** ilerlemesi,
yuvarlanmış genişliği değil: ızgaranın adımı yuvarlanmış olan, ama ortalamayı
**yuvarlanmışa** bağlamak taban fontun kendi glyph'ini bile hücreden dar
gösterir (7.827 < 8) ve her harfi yarım pikselin altında kaydırırdı.
`bt-gpu` atlası
atlasın **iki düzlemini** iki dokuya bağlar — maske `R8Unorm`, renk
`RGBA8Unorm_sRGB` —, `(bold, italic)`'i font yüzüne çevirir ve `cell`
pipeline'ında arka planın üstüne önce glyph'leri, **sonra** kural çizgilerini
çizer. Pipeline **dört**: arka planlar/dörtgenler (`cell_bg`), glyph'ler ve
kurallar (`cell`), caret (`caret_fragment`) ve renkli emoji
(`emoji_fragment`). Son ikisi paylaşımla doğdu: caret `cell_bg`'nin,
emoji `cell`'in **vertex'ini aynen** paylaşıyor ve ayrılan yalnız fragment.
Caret'te sebep bir SDF (yuvarlak köşe, kenar, hale) ve o hesabı kare başına
binlerce arka plan dörtgenine ödetmenin anlamı yok; emojide sebep **iki
ayrım** — rengi dokudan alıyor (instance'tan değil) ve baytları **ön
çarpımlı**, yani blend'in RGB kaynak çarpanı `One`. İkinci ayrım pipeline
durumunun kendisi, yani tek bir fragment dalına birleşemiyor. `bt-shell` klavyeyi PTY'ye akıtır ve **metin yolu AppKit'in
yığınından geçer**: `keyDown:` tek kapı değil dört kollu bir arbitraj —
Cmd'li olay **kapalı bir izin listesinin üç tuşu dışında** yutulur (⌘⌫ →
`\x15` `kill-whole-line`, ⌘← → `\x01` `beginning-of-line`, ⌘→ → `\x05`
`end-of-line`; üçü de macOS'un satır jesti ve üçünün de baytı zsh'te
**gerçekten** bağlı — 018'in ölçtüğü karşılıksız diziler Home/End'in
şekliydi, bu baytlar değil. Liste kapalı kalmak zorunda, yoksa bir gün
Cmd-T kabuğa `t` yazar) ve geçen tuş da yığına girmez,
Shift+PgUp/PgDn terminalin kaydırmasıdır, **Control'lü
olay yığına hiç girmez** (numpad Enter'ın U+0003'ü Ctrl-C ile, Ctrl-Y'nin
U+0019'u Shift+Tab ile paylaşımlı; kolu AppKit'e bırakmak her komutu
kesebilirdi) ve kalanı `interpretKeyEvents:` ile metin yığınına verilir.
Ölü tuş bileşimini (`Option+ü` + boşluk → `~`) o yığın tamamlıyor, düzen
verisini biz okumuyoruz; `BateriView` bunun için `NSTextInputClient`'ın **11
zorunlu** metodunu uyguluyor — kısmi uyum yok, `define_class!` eksiğinde
panikler. Yığın olayı aldıysa bir bayrak (`consumed`; `insertText:` **ve**
`setMarkedText:` kurar, değişmez "yığın aldı", "metin geldi" değil) onu
söyler; kurulmadıysa olay `keys::encode_key`'e düşer ve fonksiyon tuşları,
Enter/Tab/Esc/Backspace ile Control'lü harfler baytını bugünkü yerden alır.
Option'lı **gezinme ve silme** de oradan geçiyor ve Meta kodlanıyor
(Option+←/→/⌫ → `\eb`/`\ef`/`\e\x7f`, ayarsız); Option'lı **harf**
değişmiyor (`Option+7` Türkçe Q'da `{`), çünkü kabuğun bütün metakarakterleri
o düzende Option'da ve topluca Meta olsaydı kabuk yazılamaz hâle gelirdi.
`doCommandBySelector:` **sessiz** bir no-op: gövdesiz kalsaydı
`NSResponder`'ın varsayılanı bip çalardı. Bileşimin durumu asgari
(`marked_text`), **çizim yok** — altı çizili preedit `bt-gpu`'nun borcu.
Basılı tutulan harfin aksan popover'ı kapalı (`ApplePressAndHoldEnabled`,
uygulamanın **kendi** `registerDefaults`'ında; kullanıcının plist'ine
dokunulmuyor), çünkü terminalde basılı tuş yineleme demek. **View aynı
zamanda bir sürükleme hedefi**: Finder'dan bırakılan dosyanın yolu ters
bölüyle kaçırılıp (`quote::shell_quote`; kaçacak küme bir kara liste değil
**beyaz listenin tümleyeni** — ASCII harf/rakam ile `/ . _ -` ve ASCII
olmayan her karakter geçer, kalanı kaçar, yani yanlışın yönü güvenli)
`Session::paste`'ten giriş satırına düşer, bracketed sarma ve dock istisnası
oradan bedavaya gelir. Kayıt tek tiple (`NSPasteboardTypeFileURL`): düz metin
damlası kaçış kuralını tipe koşullu yapar ve Finder tek damlada iki tip
koyduğu için kolların sırasını da bir karara çevirirdi. **Fare isteyen
uygulama fareyi alıyor**: kip (1000/1002/1003) açıkken sol/orta/sağ tuşun
basışı ve bırakması uygulamaya rapor olarak gidiyor, kodlaması (1006 SGR /
1005 UTF-8 / X10) uygulamanın seçtiği. Arbitraj tek cümle ve **Shift tek
kaçış yolu**: Shift basılıyken rapor gitmez, seçim başlar — onsuz vim'in ya
da Claude Code'un içinde fareyle metin seçilemezdi. Kural tekerleğinkiyle
**asimetrik** (orada Shift fare kipini geçersiz kılmıyor) ve asimetri
bilerek: tekerlekte Shift'in üstüne binecek ikinci bir tüketici yok, düğmede
iki gerçek tüketici yarışıyor. Karar `bt-core`'da (`input::button_route`,
`wheel_route`'un yanında), cevabı üç varyantlı (`Click`) ve rapor
`send`'den geçiyor — seçim durur, pencere dibe dönmez. Rota **basışta
kilitleniyor** (`ViewIvars::sent_buttons`), yoksa sürüklemenin ortasında
Shift'i bırakmak jesti değiştirirdi; bırakma o yüzden Shift'i sormuyor ama
**kipi soruyor** — uygulama bu arada çıkmışsa rapor kabuğun komut satırına
düşerdi. Bırakmanın koordinatı reddedilmiyor **kırpılıyor**: düşürmek
uygulamada takılı kalmış bir düğme bırakırdı. Doldurma bandının üstündeki
basış ne rapor ne seçim üretiyor (bandın satırları geçmişte, uygulamanın
ekranında yoklar). **Hareket de raporlanıyor**: 1003 her hareketi ister, 1002
yalnız basılı olanı, 1000 hiçbirini (`input::motion_route`) — düğme yolunda
tek cevap veren üç bit burada ayrışıyor. Pencere hareket olaylarını
**koşulsuz** dinliyor (`setAcceptsMouseMovedEvents`), çünkü kipe göre açmak
kipi `bt-shell`'e yayınlamayı isterdi; bedeli düşüren şey kısmanın `bt-core`
çağrısından **önce** koşması — rapor hücre başına bir kez gidiyor
(`ViewIvars::motion_cell`, ölçü görünür pencere hücresi) ve aynı hücrede
kalan hareket `Term` kilidine hiç uğramıyor. Çentiği basış ve bırakma da
tazeliyor. Fareyle seçim, pano, geçmişte
kaydırma, ana menü (About, Settings…, Quit; Edit'te Copy/Paste; View'da
Theme ▸ ve Cmd +/−/0 geçici punto) ve kapanış sırası ondadır; uygulamanın
OSC 52 kopyasını (`Wake::copy_to_clipboard`) genel panoya o yazar;
`settings.toml`'u okur (bugün `scrollback`, tema seçimi, font ailesi/puntosu/satır aralığı, `osc52`,
`cursor`, `cursor_blink`, `cursor_radius`, `cursor_glow`, `cursor_unfocused`,
`cursor_blink_interval`, `cursor_motion`, `reduce_motion` ve
`shell.integration`),
Theme ▸'nin seçimini oraya
yazar ve temayı `themes/{ad}.toml`'dan ya da gömülü
`bateri`/`bateri-light`'tan çözer. Ayar ve etkin tema dosyası **kayıt
anında** uygulanır (`watch`: vnode kaynakları; `Session::set_theme`,
`Session::set_terminal_options`, `Renderer::set_font`,
`DisplayLink::set_cursor_motion`, `DisplayLink::set_reduce_motion`);
varsayılan tema sistemin açık/koyu görünümünü, `reduce_motion = "system"` de
sistemin Hareketi Azalt ayarını canlı izler; tek istisna `[shell] integration`,
kabuk çoktan doğduğu için **sonraki oturumda** geçerlidir. Kabuk zsh ise
`bt-shell` sarmalayıcıyı `ZDOTDIR` ile kurar (betik `.app`'in
`Contents/Resources/shell`'inden, debug'da depodan) ve kabuğun bastığı OSC 133
işaretleri `Session::shell_state()`'te birikir. Aynı betik her satır çiziminde
ZLE'nin görüntüsünü (`PREDISPLAY`, `BUFFER`, `POSTDISPLAY`, `region_highlight`,
`CURSOR`) OSC 8133 ile aynalıyor; `Session::dock()` onu **çözülmüş** dock
hücrelerine çevirip sınırdan veriyor ve `bt-gpu` pencerenin altındaki **ikinci
bir `setViewport`**'la çiziyor — kendi listeleri, kendi caret'i, opak zemini ve
ızgaradan ayıran saç çizgisiyle. **Caret tek**: ızgaranın imleci ile dock'un
caret'i aynı animatörün (`bt-gpu::motion`) iki hedefi, yani dock'ta yazarken de
süzülüyor ve devir bir ışınlanma değil bir kayma. **Şekli de tek** ve sınırdan
geliyor (`Cursor::shape`): DECSCUSR'ın üç biçimi — blok, alt çizgi, dikey
çubuk — uygulamanın isteğiyle, o susunca `[terminal] cursor` ile. İnce
şekillerde **boyanan dörtlü ile ters çevirme dikdörtgeni birlikte** daralıyor
(`bt_gpu::frame::caret_rect`, tek yer iki tüketici), kalınlık fontun kendi alt
çizgi metriğinden (`CellMetrics::rule_px`) ve daraltma yuva seçiminden
**sonra**, yoksa alt çizgi caret'i dock bandına değmez ve zeminin altında
kalırdı. **Yüzeyi kendi fragment'inin**: köşesi yuvarlak ve çevresinde hafif
bir hale var, ikisi de yuvarlak dikdörtgenin imzalı mesafesinden
(`shaders/cell_bg.metal` → `caret_fragment`). Sayılar uydurulmuyor —
yarıçap hücre **yüksekliğinin** oranı (üç şeklin ortak tek boyutu o), hale
payı sol paydan türüyor (`CellMetrics::gutter_px`'in beşte ikisi, aynı içi
girintinin üçüncü kullanımı; oran iki tur gözle indi) ve kenar kalınlığı yine `rule_px`; punto büyüyünce üçü
birden büyüyor. Hale caret'in **kendi alfasıyla** çarpılıyor, yani blink
sönerken hale de sönüyor ve ikinci bir yol yok. Dörtlü hale payı kadar
**şişiyor** ama **yuva seçimi şişmemiş dikdörtgene bakıyor**: hale ayak izini
büyütüp caret'i dock yuvasına kaydırsaydı caret ızgaranın glyph'lerinden sonra
çizilir ve altındaki harfi boyardı. `caret_rect` bu yüzden **iki** dikdörtgen
veriyor — boyanan ve ters çevirmenin opak içi; dolu caret'te eşitler. "Yarıçap 0, hale 0" kolu **desteklenen ve sınanan** bir hâl:
çıktısı düz dörtgenle bit bit aynı ve geri alma yolu o.
**Odakta olmayan pencerede caret'in içi boşalıyor** (`[terminal]
cursor_unfocused`; `"solid"` bunu kapatıyor ve blink'in durmasına
**dokunmuyor** — iki ayrı sinyal) ve blink duruyor: kenar
kalınlığı yine `rule_px`, ters çevirme ise **kalkıyor** — boyanmayan bir
pikselin altındaki harf kendi rengiyle kalmak zorunda, yoksa çerçevenin
ortasındaki metin zemin renginde çizilir ve görünmez olurdu. `caret_rect`'in
iki dikdörtgeni tam burada ayrılıyor: boyanan duruyor, opak iç boşalıyor.
İçi boşalma **yalnız bloğa**; alt çizgi ve dikey çubuk zaten `rule_px`
kalınlığında birer şerit ve çıkarma onları büsbütün yutardı, o şekillerde
sinyal blink'in durması. Odak `bt-core`'a **hiç girmiyor**
(`DisplayLink::set_focused`; `CaretShape`'e de eklenmedi — o enum ayar
dosyasının sözlüğü, odak ona dik bir eksen) ve hermetik koşuda **hiç
okunmuyor**: kapı çağrı yerinde, `bt-shell`'in pencere delegate'inde. **Alternatif ekrandan çıkışta imlecin
stili kullanıcının tabanına dönüyor** (`Term::set_cursor_style(None)`,
`Session::frame`'de `alt_screen` bayrağının **düşen kenarında**): ölçüt
"uygulama bitti", yani bıraktığı şekil de sönme de bitiyor. Gerekçe ölçüldü
(kullanıcı bildirdi): `cursor_blink = "auto"` vim'den bir kez geçtikten sonra
kalıcı olarak sönmeyi bırakıyordu ve suçlu DECSCUSR değil terminfo —
`xterm-256color`'ın `cnorm`'u (`\e[?12l\e[?25h`) blink'i kapatan özel mod 12'yi
**içinde** taşıyor, yani `cnorm` gönderen her program (vim, less, man, htop)
onu öldürüyor ve geri açan kimse yok. alacritty bunu kendiliğinden yapmıyor ve
bu onun tercihi: `cursor_style` `Term` seviyesinde tek bir alan ve `swap_alt`
ona hiç dokunmuyor. Bedeli bir prompt'luk — zsh vi-kipinin stilini
`zle-line-init`'te yeniden gönderiyor. Hedef **ekran hücresi**
cinsinden ve dock'unki kesirli — band nefes payı kadar aşağıdan başlıyor ve
artık şeridin altında duruyor; yuvarlansaydı caret bir hücre yukarıda dururdu.
Çizim **yuvası** konuma göre seçiliyor (`Frame::push_caret`): blok, üstünde
duracağı yüzeyin zemininden sonra ama glyph'lerinden önce çizilmek zorunda,
yani ızgarada kalsaydı dock'un opak zemini onu örter, dock'ta kalsaydı
ızgaranın harfini boyardı. Ölçüt örtüşme — banda değen caret dock yuvasına
geçiyor ve orada en üstte kalıyor. Ters çevirme dikdörtgeni **tek** ve pencere
uzayında, iki glyph encode'una da aynısı gidiyor. Payı `DOCK_ROWS * cell_h` **artı iki nefes
payı** (`bt_gpu::dock_px`; formülün tek kopyası orada, `split_into_grid` onu
tüketiyor): iki satır saç çizgisine yapışınca dock bakılamaz duruyordu. Payın
kaynağı sol payın ta kendisi (`CellMetrics::gutter_px`) — ikinci bir tasarım
sabiti yok, aynı içi girinti iki eksende ve punto büyüyünce pay da büyüyor.
Saç çizgisi payın **üstünde**, viewport'un tepesinde; **ikincisi** iki satırın
arasındaki boşluğun ortasında, aynı renk ve aynı kalınlıkta — boşluk ayrımı
önerir, çizgi söyler. Kenara değil ortaya konuyor, yoksa bir satıra yapışır ve
ona ait görünürdü. Satır arası boşluk bu yüzden dış payın **iki katı**: çizgi
her satırı kendi bandı yaptığı için bandın içi simetrik olmalı ve çizginin iki
yanına birer pay düşünce dock'un dört boşluğu da eşitleniyor (kalan ±1 px
çizgilerin kendi kalınlığından). phase-9'un `pad / 2`'si "dış boşluk içtekinden
büyük" kuralındandı; o kural **gruplar** için doğru, araya çizgi girince grup
kalmıyor. Dock ötelemeden
**yapısal olarak** muaf:
listeleri dock-yerel doğuyor, ekrana taşıyan şey o ikinci viewport. Üst
satırında prompt işareti (safha rengiyle), metin, sönük öneri,
`region_highlight` renkleri ve caret var; alt satırında **bağlam** —
`{tam yol} | {dal}`, sol altta ve sönük; **içinde iki kademe** — yolun son
bileşeni (aktif klasör) ile dal öne çıkıyor, üst dizinler ve ayraç geri
çekiliyor, çünkü aranan bilgi "hangi klasördeyim". Soluk ton yeni bir renk
değil (`Theme::quiet_linear`), aynı kuralın (`dim_toward`) ikinci uygulaması.
Zincir **üç kademe**: `dim` → `quiet` (üst dizinler) → `separator` (saç
çizgileri). Çizgiler bir adım daha ötede, çünkü **mürekkep değiller**: yan
yanaki en sessiz metinle aynı ağırlıkta olsalardı göz onları da okunacak bir
şey sanardı. Kökte ve eğik
çizgisiz yolda ayrım yok, tamamı öne çıkıyor — yanlışın yönü güvenli.
Satır **dock'un sol kenarından**, giriş
metninin hizasından değil: metinle hizalanınca sebepsiz girintili görünüyordu
ve bağlam giriş satırının devamı değil, dock'un altbilgisi. **Puntosu da ayrı**
— gösterim fontunun %80'i (`bt_atlas::CONTEXT_SCALE`), çünkü hiyerarşiyi
yalnız renge yüklemek yetmiyordu; oran ölçülmüş bir sayı değil bir tasarım
sabiti (`GUTTER_PT` emsali) ve **oran**, mutlak punto değil, yani Cmd +/− ile
iki satır birlikte büyüyor. Küçük harfler atlasta **aynı yuvaya, aynı taban
çizgisine** rasterize oluyor (`SizeClass`), yani doku, ızgara ve band
aritmetiği (`dock_px`) hiç değişmiyor: ayrışan tek şey bir harfin kaç piksel
ilerlettiği (`CellMetrics::context_cell_px`) ve bağlam satırının sütun
bütçesi (`DockCols`, oranı `bt_gpu::context_cols` veriyor — `bt-core` piksel
görmüyor). Dörtlü büyük kalıyor, küçük harf sol kenarında duruyor ve komşu
dörtlüler örtüşüyor; örtüşen piksel saydam, blend `SourceAlpha`, yani ikinci
bir draw call doğmuyor. Küçük sınıfta yalnız **düz yüz** var: bağlam satırı
terminalin kendi altbilgisi, kabuğun biçimlendirmesi oraya girmiyor. İki satırın arasında
da boşluk var (dış payın iki katı; aynı dosyanın yukarısı zaten öyle diyor). Bağlamın iki ucu iki ayrı yerden:
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
**Dock sütun sayıyor** (024): giriş satırının pencerelemesi, caret'in yeri ve
geniş karakterin iki hücresi karakter indeksinden değil **genişlikten**
birikiyor; pencere caret'in altındaki karakterin tamamını ayırıyor ve iki
kenarda da geniş glyph yarılanmıyor — sığmayan karakter hiç çizilmiyor.
`region_highlight`'ın aralıkları **karakter** indeksinde kalıyor, çünkü
ZLE'nin birimi o; yayılan şey boyanan **zemin** ve onu baş hücrenin `wide`'ı
ile spacer sütununa düşen glyph'siz bir hücre taşıyor. **Bağlam satırı
karakter biriminde** ve gerekçesi küçük boy sınıfı (021'in emsali), yani
CJK'lı bir yol orada hâlâ sütun kaydırıyor — bilinen sınır, bekçili.
Bastırmanın tazelik kapısı da aynı birime geçti: ayna tarafı **sıfır
genişlikli** kod noktalarını atlıyor, çünkü onlar ızgara hücresine hiç
girmiyor (`CellExtra`) ve saymak kapıyı kalıcı olarak "bayat" yapardı —
`❤️` yazan satır her tuşta ızgaraya fırlıyordu.

Giriş satırı ızgarada **çizilmiyor**: kabuk `Input` safhasındayken ve ayna
canlıyken (`ShellLog::suppressed_input`; karar `Term` kilidinden **önce**
okunuyor, `Theme` örüntüsü) yazılmakta olan bloğun çıpa satırından imlecin
satırına kadar hücreler sink'e uğramıyor — caret dock'ta.
Kapı çıpa taramasından **sonra**, yoksa blok şeridi de ölürdü. Ayna
gösteremiyorsa (`Unavailable`), görüntü **satır sonu taşıyorsa**
(`Multiline`), ZLE satırı bırakmışsa (`Idle`) ya da ayna **bayatsa** bastırma
**yok**: gösteremediğimiz satır ızgarada kalmak zorunda. `Multiline`
`Unavailable`'ın kardeşi, kolu değil — orada kanal bozuk ya da yük sınırı
aşmış, burada veri sağlam ve **yüzey dar**: dock'un giriş satırı bir tane, çok
satırlı bir `BUFFER` tek satıra yassılırdı (`\n` glyph üretmiyor ama sütun
tüketiyor) ve caret düz karakter indeksinden geldiği için hiçbir harfin
üstünde durmazdı. Belirti kullanıcıda görüldü: çok satırlı yapıştırmada metin
ızgarada kalıyor, caret dock'a iniyordu. Ölçüt `BUFFER` değil **görüntünün
tamamı** ve dönüş kuralı kendiliğinden — durum her ayna yükünde yeniden
hesaplanıyor, yani satır sonu silinince bir sonraki aynada `Live`. Dock'u çok
satırlı girişe göre **büyütmek** ayrı bir iş ve bilerek yapılmadı: bandın boyu
ızgaranın satırlarından düşüldüğü için her yeni satır bir PTY resize'ı, yani
kullanıcı yazarken nefes alan bir ekran demekti.
Bayatlık kip sezerek anlaşılmıyor ve **önce zaman soruluyor** (025):
kullanıcı girdisinin tek hunisi (`Session::send_input`) her gönderimde bir
nesil artırıyor, okuyucu ayna çözüldüğü anda o nesli aynanın **yanına**
damga olarak koyuyor (`DockState::answers`) ve damga güncel nesle eşitse
ayna son girdinin cevabıdır — taze. Damga içerikle aynı yaprak kilit
turunda okunuyor, yani bayat bir okuma bayat damga getirir ve karar ikinci
soruya kalır. Gerekçe kullanıcıda görüldü: zsh bazı kod noktalarını
**kendisi** `<hex>` diye yazıyor (`🥰` → ters videolu `<0001f970>`), ayna
ise ham emojiyi taşıyor; içerik karşılaştırması onları hiç eşleştiremiyor
ve caret yazarken ızgaraya sıçrıyordu. **Cevap gelmediyse** (yapıştırmanın
`bracketed-paste-magic` kolu) kapı iki kesin veriyi karşılaştırıyor —
ızgaranın son mürekkebi ile aynanınki (`DockState::last_ink`); yanlış alarmın
yönü güvenli, satırı iki yerde gösterir ama sessizce kaybetmez. Zamansal
sorunun **üç bilinen sınırı** var ve üçü de adıyla yazılı
(`.tasks/025-tazelik-zamansal/discussion.md` → Karar 2): damga aynanın ne
zaman geldiğini söylüyor, hangi girdiye cevap olduğunu değil (bir tuşun
aynası yoldayken giden yapıştırma bir tuş boyunca "cevaplanmış" görünür), ve
kabuğun dışından gelen yazım (arka plan işinin satıra bastığı çıktı) nesli
oynatmıyor, düzenleme boyunca bastırılan aralıkta gizli kalıyor; zsh'in
redisplay'siz tuttuğu tuş (`^X` öneki, vi'de çıplak `Esc`) ise nesli
ilerletip ayna doğurmuyor ve o süre kapı içeriğe düşüyor. **Dock'un
çizmediği kontrol karakteri** (sekme hariç) satırı `DockStatus::Control`'e
indiriyor — `Multiline`'ın kardeşi, aynı kural: gösteremediğimiz satır
ızgarada, okunur `^A` ile. Bu kol gelmeden önce karar kapının tesadüfüne
kalıyordu ve `^A` satırın ortasındaysa satır dock'a gidip kayboluyordu. **Aynanın hiç
karakteri yoksa o karşılaştırma vakuma düşüyor** (iki taraf da `None`) ve
ayıran ikinci veri çıpanın satırı (`session::anchor_row_at_or_above`):
karakteri olmayan bir ayna imleci prompt'un satırından aşağı itemez, yani
imleç çıpanın satırında olmak zorunda. Boş prompt'ta öyle — `PS1`'in iki
boşluğu çıpayı taşıyor — yapıştırmadan sonra değil. Gerekçe ölçüldü: zsh
bracketed yapıştırmanın **son satır sonunu tamponda tutuyor**, yani ızgaranın
imleci boş bir satıra düşüyor ve `bracketed-paste-magic` (oh-my-zsh onu
kuruyor) aynayı bir tuş boyunca boş bıraktığı için iki boşluk birbirine
uyuyordu. Çıpa hiç bulunamazsa kapı **susuyor**: hücresiz bir prompt'ta
söyleyecek bir şey yok.
**Devrin tek yüklemi var** (`shell::caret_home` + üç ön koşul) ve **dört
tüketicisi**: hangi hücrelerin atlanacağı, imlecin çizilip çizilmeyeceği,
**doluluk sayısı** ve **dock'un caret'i**. Ayrı sorulduklarında ayrışıyorlardı ve belirti ölçüldü:
boş prompt'ta hiçbir hücre çıpayı taşımadığı için satır çizilmiyor ama
doluluğa **giriyordu**, ilk tuşta çıpa doğunca doluluk bir satır düşüyor ve
ızgaranın tamamı oynuyordu — satır gizliydi ama yer kaplıyordu. Tek yüklem
`display: none` veriyor. Caret'in sahibi satırın nerede çizildiğine uyuyor:
komut koşarken (`Running`), ayna gösterilemiyorken (`Unavailable`), görüntü
tek satıra sığmıyorken (`Multiline`) ve ZLE satırı bırakmışken (`Input` +
`Idle`; `CORRECT`'in `[nyae]`'i, R3.3) ızgaranın;
**kalan her hâlde dock'un** — kabuğun henüz hiç konuşmadığı açılış, prompt
çizilirken ve iki komut arası (`Finished`, içinde bir `git` fork'u) dahil,
çünkü sıçrayan caret tam da o pencerelerde görülüyordu. Üç ön koşul:
pencerenin dock'u olacak (`SessionOptions::dock`; yoksa devralacak kimse yok
ve satır da imleç de ızgarada kalır), alternatif ekranda olmayacak (dock zaten
kalkıyor) ve bastırılan bir satır varsa tazelik kapısı geçilecek.
**Dock→Grid yönü tutuluyor** (`shell::HANDOVER_HOLD`, histerezis): yüklem
`Grid` dese de cevap kısa bir süre `Dock` kalıyor ve o süre içinde geri
dönerse devir **hiç olmamış** sayılıyor. Sebebi ölçülmüş: `ls` gibi hızlı bir
komutun safhası imleç animasyonunun yerleşmesinden **çok daha kısa sürüyor**
(ikisi de ölçüldü, sayıları `.tasks/015-imlec-cilasi/context.md` → Kanıt),
yani caret dock'tan çıkıp yarı yolda geri dönüyordu ve göz bunu bir zıplama
olarak okuyordu. Tutmanın kalanı saate **`min`'lenerek** giriyor (`shell::sooner`),
yazarak değil: süre sayacının tiki de aynı yuvayı kullanıyor ve ezilseydi
koşan komutun sayacı donardı. Ters yön (Grid→Dock) **tutulmuyor** — komut
bitince caret ızgarada asılı kalsaydı kullanıcı yazmaya başladığında dock'ta
caret'siz bir satır görürdü, yani yanlışın yönü güvenli değil. Aynanın
**arızası** da tutmanın dışında (`Unavailable`, `Multiline`): gösteremediğimiz
satır ızgarada duruyor, caret'i de orada durmalı, ve arıza zaten bir sıçrama
üretmiyor; tutma çok satırlıyı kapsasaydı yapıştırmadan sonra caret 150 ms
dock'ta kalırdı. Cevap
**hesaplandığı yerden geçiyor**, ikinci kez türetilmiyor: `frame()` onu
`Cursor::caret_in_dock` ile veriyor, `Session::dock` argüman olarak alıyor.
Dock kendi başına sorduğunda üç ön koşulu bilmiyordu ve bayat aynada **iki
caret** doğuyordu — ızgara imlecini gösterirken dock da sahipleniyor, çizen
taraf dock'u seçiyor ve kullanıcının yazdığı taze satır caret'siz kalıyordu
(set kapısı, 012). Aynı tekleştirme iki ayrı kilit turundan türetme yarışını da
kapattı. Metinsiz dock
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
**terminalin**: `RPS1` sıfır görünür genişliğe iniyor, `PS1` ise **iki sütuna**
— iki sıfır genişlikli işaret artı iki **gerçek** boşluk. O iki sütun ızgaranın
blok işaretinin yeri: chevron 0. sütuna oturuyor, komut 2.'den başlıyor ve
dock'un prompt işareti de 0. sütunda, metni de 2.'de. **Hiza hesaplanmıyor**,
iki işaret de `Frame::pos`'tan geçiyor. Boşluklar bir çizim hilesi değil gerçek
genişlik ve alternatifi komut satırını çizerken kaydırmaktı: o yol fare
eşlemesini o satırda kaydırır, tam genişlikteki komutu taşırır ve zsh'in satır
sarmasını yanlışlardı. Sayı iki yerde yaşıyor (betikteki boşluklar ve
`dock::TEXT_COL`) ve bir sınama ikisini bağlıyor. Yan kazanç: boşluklar çıpayı
taşıdığı için boş promptta da çıpalı bir hücre var. Dayatma **iki yerden** ve ikisi de zorunlu —
`precmd` ilk basımı doğru yapıyor, aynanın ZLE kancası temanın geri yazdığını
`zle reset-prompt` ile geri alıyor (p10k/starship `PS1`'i `precmd`'den **sonra**,
kendi ZLE kancalarından kuruyor; ölçüldü: kancadan atanan `PS1` `reset-prompt`
olmadan ekranı hiç etkilemiyor, çünkü prompt `line-init` koşmadan basılıyor ve
zsh genişlettiği hâli tutuyor). Nöbet prompt başına **tek** sıfırlama bırakıyor,
yoksa `reset-prompt` kendi kancasını besler. Prompt'unu geri isteyen
kullanıcının anahtarı `[shell] integration = "blocks"`: sarmalayıcı yine
kuruluyor, yani **bloklar ve işaretler kalıyor**, ama dock hiç açılmıyor ve
satır da prompt da ızgarada duruyor. Prompt ile dock'un **tek karar** olması
ölçüldü: bir dönem ayrı bir `prompt` anahtarı vardı ve ekranda iki prompt
üretiyordu (kullanıcınınki ızgarada, dock'unki altta), caret de ikisi arasında
sıçrıyordu — "prompt benim olsun" demek zaten "satır ızgarada" demek. Anahtar
**emekli**: dosyada korunuyor, okunmuyor, görülünce tanı bırakıyor. Kademenin
dock istemediği kararı terminalin tarafında (`ShellIntegration::wants_dock`),
kabuğa hiç sorulmuyor; `blocks` üstelik bash ve fish betikleri doğduğunda o
kabukların **zaten** olacağı hâl — dock ZLE'nin aynasına bağlı. Çıpanın kapanışı `PS1`'in sonunda değil
**`preexec`'te**: sıfır genişlikli prompt hiçbir hücre yazmadığı için kapanış
orada kalsaydı çıpayı taşıyan hücre hiç doğmaz, blok şeridi **ve** bastırma
birlikte sessizce ölürdü. Bağlantı `Input` boyunca açık, komutun çıktısında
kapalı. Her prompt bir blok kimliği
basar, `frame()` o kimlikleri prompt'un OSC 8 çıpasından okuyup blokları
**komutun satırı ve rengi** olarak sınırdan verir; `bt-gpu` o işareti
ızgaranın solunda ayrılan paya çizer — **animasyonsuz**, işaret anında belirir.
Aynı komutun **süresi** de oradan geliyor: `C` ile `D` arası ölçülüp deftere
yazılıyor ve bir saniyeyi geçenler komut satırının **sağ ucunda**, sönük, sıradan
hücreler olarak sınırdan çıkıyor — yeni bir sınır tipi yok, `bt-gpu` onu
ızgaranın herhangi bir harfinden ayırt etmiyor. Koşan komutta sayaç tam saniye
(`3s`, `45s`), bitince **dakikaya kadar** onda bire oturuyor (`3.4s`, `45.3s`;
dakikadan sonra `1m 05s`): koşan sayacın her değişimi bir kare
istiyor, donmuş değer istemiyor. Komutun mürekkebiyle arasında bir boş hücre
kalmıyorsa sayaç **çizilmiyor** — kullanıcının yazdığı örtülmez. Sayacı
ilerleten şey kare talebinin üçüncü sebebi, **saat** (`Cursor::next_tick`).
Şekli dock'un prompt işaretinin **ta kendisi** (`bt_atlas::RuleKind::Chevron`):
ikisi de safha renginde bir prompt işareti ve ayrı şekillerle çizilmeleri bir
kalıntıydı. İşaret fonttan **alınmıyor**, yordamsal çiziliyor — kullanıcının
fontu değişince prompt'un şekli değişmemeli; dikey merkezi üstü çizili
metriğinden, yani x-height'ın ortasından, kalınlığı da alt çizginin
kalınlığından geliyor (ikinci bir sayı uydurulmadı). `bt-core` sınırdan yalnız
**rengini** veriyor (`Dock::sigil`, `Block::stripe`); şekil boyamanın kararı. İşaret komutun kendi satırında,
çıktısında **değil**: hangi satırın hangi bloğa ait olduğu ancak çıpası
görünen satırlar için biliniyor ve bölge boyamak onu tahmine çevirirdi.
**İçerik pencerenin tabanına yaslanır**: `frame()` kaç satırın dolu olduğunu
sınırdan verir (`Cursor::content_rows`; alternatif ekranda ızgaranın tamamı),
`DisplayLink` onu `rows - content_rows` ile ötelemeye çevirir ve `encode_pass`
tek bir `setViewport` ile ızgaranın bütün pipeline'larını birden kaydırır —
dört liste ve imleç aynı yerden. **Kaydırılmış pencerede de aynı kural**:
doluluk görünür satırlardan doğuyor, yani geçmişe bakarken de içerik tabana
yaslı kalıyor. 017 bir dönem burada `display_offset != 0 => rows` denedi ve
kullanıcı gördü: ızgaranın **boş** alt satırları doluluğa giriyor, öteleme
kapanıyor ve bütün içerik pencerenin tepesine sıçrıyordu — terminal aşağıdan
yukarı akar. Öteleme **yumuşak kayar**: `bt-gpu::motion`'ın ikinci animatörü
(`Slide`) onu imleçle aynı stil ve aynı `settled()` kapısı altında sürer, imlecin
hedefi de **ekran satırıdır** (`row + origin`), yani Enter'da imleç dipteki
satırında durur ve geçmiş arkasından yukarı akar. Kayma **boşluk boş kalıyorsa
tek yönlüdür**: içerik büyüyünce (hedef düşünce) süzülür, daralınca (dock'u
olmayan pencere, kasten temizlenmiş ekran, geçmişe kaydırılmış pencere)
**snap**'ler — yukarı akış içeriğin gelmesi,
aşağı iniş düşmesi gibi okunuyor; ölçüt mesafe değil işaret, çünkü eşik
ölçülmemiş bir sayı olurdu. **Alternatif ekrandan çıkış bu listede değil**
(017 kapı, ölçüldü): `vim`'in giriş `2J`'si bayrağı kurmadığı için çıkış
karesinde `fill > 0` ve öteleme süzülmeye **başlıyor**; snap'i getiren şey
dock'u geri getiren resize'ın bir sonraki ana kuyruk turunda dikeceği
`geometry` bayrağı, yani kayma bir kare sürüyor. Yönü savunulur — boşluğa
gerçekten geçmiş giriyor — ama tasarlanmış değil ve adıyla yazılı
(`Motion::sync_origin`'in doc'u). **Doldurma o yönü açıyor** (`Motion::sync`'in
`filled` biti, `link.rs`'te `cursor.fill > 0`): boşluk defterin satırlarıyla
doluyorsa aşağı inen şey boşluk değil, üstten **gelen geçmiştir**, yani
011'in gerekçesi o kolda konusuz kalıyor ve yükselen hedef de süzülüyor —
kural kalkmadı, **daraldı**. Terim guard'da `!snap`'in **içinde**: dışına
yazılsaydı Rust'ın önceliği onu `animated`'ın da üstüne çıkarır, doldurmalı
pencerede tekerlek ve geometri animasyona başlardı ve `cursor_motion =
"snap"` ile Hareketi Azalt delinirdi. Tekerlek ve geometri
(pencere/font/punto) ayrıca snap'ler.
Piksel aygıt ızgarasına
yuvarlanır (`Frame::set_origin_rows`): kaymanın durduğu kare ekranda kalıcı ve
kesirli bir piksel bütün metni bulanıklaştırırdı. Ötelemenin tek sahibi
kare yolu; fare eşlemesi onu `bt_gpu::Origin` ile **encode edilen** değerden
okur. **Üstte kalan boşluk artık boş değil**: `frame()` oraya defterin en yeni
satırlarını veriyor (`Cursor::fill = min(gap, temizlemeden beri gelen satır)`),
ama **ayrı bir sink'ten** ve satırları **fill-yerel** (`0..fill`) — doluluğa girmiyor, yani
öteleme aritmetiği dokunulmadan duruyor. Kapısı tek yerde
(`Session::fill_rows`) ve **dört** koşullu: pencerenin dock'u olacak,
alternatif ekranda olmayacak, kasten temizleme bayrağı temiz olacak
(`CSI 2 J`; `Session::observe_screen_clear`) ve pencere dibe yaslı olacak
(`display_offset == 0`). **Bant bir sanal kaydırmadır**, kaydırmanın rakibi
değil: dibe yaslı pencerede ekranın tepesi `Line(-fill)` ve orası
`display_offset == fill` olan bir pencerenin de tepesi, yani bant ile grid
tam olarak o ofsetin gösterdiği şeyi gösteriyor. Süreklilik bu yüzden
kaydırmanın işi — `scroll_locked` çentiği `0`'dan değil **banttan** devam
ettiriyor (`Session::fill_shown`, son dibe yaslı karenin bıraktığı sayı) ve
`1..=fill` aralığı ekranda hiç görülmüyor: yukarı çıkarken üstüne atlanıyor,
aşağı inerken o aralığa **değen** hedef doğrudan dibe düşüyor — `fill`'in
kendisi dahil, çünkü defter tam bandın boyu kadarsa yukarı çıkan pencere
`clamp` yüzünden orada duruyor ve orası görsel olarak dibin ta kendisi.
Kuralın tek muafiyeti resize'ın bırakabileceği **iç** ofsetler (`1..fill`):
pencereyi büyütmek geçmişten satır çekip ofseti düşürüyor ve oradan yukarı
çıkan çentik dibe inmemeli. Ölçüt "her çentikte ekran ya aynı kalır ya tam
bir satır kayar" ve bekçisi o (`every_notch_moves_the_screen_by_one_row_at_most`). Alternatifi
**ölçüldü ve kullanıcının bildirdiği kusurdu** (2026-09-20, gözle kontrol):
doldurma kaydırılmış pencerede de koşunca `fill = rows - content_rows` her
çentikte bir azalıyor, `fill + offset` sabit kalıyor ve bandın okuma noktası
`-(fill + offset)` hiç kıpırdamıyordu — kaydırma boşluk kadar çentik boyunca
ölü görünüyordu. Kapı `fill`'i sıfırlayınca doluluk yine görünür satırlardan
doğuyor ve tepeden yeni satır giriyor. Sıfır dönerse
ikinci sink hiç çağrılmıyor ve kare doldurmasız hâliyle bit bit aynı. Çizen
taraf **üçüncü bir `setViewport`**: bandın orijini `origin_px - fill_px`
(`Frame::fill_origin_px`) ve o sayı **encode anında** türüyor, yani bant
ızgarayla **birlikte** kayıyor — push anında pişmiş bir konum hareket
karesinde (listeler korunur, yalnız öteleme değişir) bandı yerinde
dondururdu. Orijin kaymanın ortasında **negatife** iniyor ve bırakılıyor:
bandın pencereye sığmayan en eski satırlarını Metal tepeden kırpıyor
(ölçüldü, 017 phase-0). Listeleri dock örüntüsünde **ayrı** ve sayaçlardan
muaf (`hucre=`/`glif=`/`kural=` oynamıyor); **blok işareti de o listelerden**
(`Blocks::fill_slice`, `Frame::push_fill_block` → `fill_rules`): bant ikinci
bir yüzey ve ızgaradan türeyen her şeyi ayrıca kazanmak zorunda — hücreleri
017 phase-2'de almıştı, işareti almamıştı ve kullanıcı bunu gördü (tamamlama
listesi komut satırını geçmişe itiyor, bant satırı geri getiriyor ama
**işaretsiz**, kaydırınca aynı satır ızgaradan geçtiği için işaret geri
geliyor). Çıpa yeni bir kaynak değil, hücrenin kendi OSC 8 bağlantısı;
eksik olan **okuyan** döngüydü. Satırlar fill-yerel, yani işaret bandın kendi
`setViewport`'unda. **Süre sayacı hâlâ bantta yok** ve bu bilinçli daraltma:
sayaç hücre üretiyor (`Counter`) ve çakışma ölçütünü (`last_col`) ikinci kez
kurmayı isterdi; işaret ise bir `RuleCell`. Encode sırası **ızgara →
doldurma → dock**, çünkü ızgaranın listeleri bandın içine hiç girmiyor ama
ötelemeden muaf olan caret girebiliyor, ve dock'un opak zemini en altta
kalmak zorunda.
**Seçim içeriği vurgular, içerik yaratmaz**: vurgu yalnız seçim olmasaydı da
çizilecek hücrelere uygulanıyor, yani boş ekranda fareyi sürüklemek hiçbir şey
boyamıyor ve gözün gördüğü ile panonun verdiği ayrışmıyor. Ölçüt "mürekkep"
değil **çizilirlik** — ters videolu bir boşluk (vim'in durum satırı, tmux
çubuğu) mürekkepsizdir ama görünürdür ve seçilince vurgulanır; varsayılan
zeminli boş hücre görünmezdir ve seçim onu görünür kılmaz. Doldurma bandı bu
kuralın tek istisnası değil **tersi**: satırları görünür ama **seçilemez**,
çünkü hepsi geçmişte, yani sınırın satır numaralarıyla temsil edilemiyorlar.
Fare bu yüzden orijinin üstünü **reddediyor** (`point_to_cell`, `fill > 0`);
kırpma orayı 0. satıra yapıştırır ve vurguyu gözün gördüğü yerden başka bir
yerde başlatırdı — "yanlış seçilir" ile "seçilemez" arasında dürüst olan
ikincisi. Kırpma **kalkmıyor**, yanına geçiyor: doldurma yokken orası
gerçekten boş ve yukarıdan başlayan sürükleme ilk satırı seçime katmalı.
Tekerleğin işaretçisi reddin dışında, çünkü o bir seçim ucu değil rapora
giden koordinat — reddedilseydi band ekrandayken kaydırma büsbütün ölürdü.
Dock ve komutlar arası atlama henüz yok. `make kur` `bateri.app` paketini
üretir.
**Geniş karakter ve renkli emoji çiziliyor** (023) ve ikisi tek
mekanizmadan: mürekkebi bir hücreye sığmayan **iki sütunlu** karakter iki
hücre boyunda bir kutuya ortalanıp **iki yuvaya** rasterize ediliyor
(`bt_atlas::Half`; sağ yarı tam sayı piksel ofsetiyle, yani AA fazı ikisinde
birebir aynı ve bölünmüş bir tampon gerekmiyor). Yuvalar yine tam bir hücre,
yani doku düzeni, `slot_bytes` ve ızgara aritmetiği **değişmiyor**; dörtlü de
tek hücre kalıyor ve `GlyphInstance`'ın 32 baytlık stride'ı ile `cell_px`
uniform'u el değmiyor — 012'nin `>` işaretini durduran sınır bu setle
**aşılmadı, etrafından dolaşıldı**. Kapının **sırası** karar ve ölçülmüş:
geniş hücrede önce tek hücrelik mürekkep kapısı, geçerse bugünkü tek yuvalı
yol (raster bit bit aynı), geçmezse iki hücrelik kapı, o da geçmezse kutu.
Sıra ters olsaydı geniş **ilan edilmiş ama dar boyayan** 65 karakter
(21'i Menlo'nun `☕ ⚡ ♈`'si, 44'ü cascade'den gelen `丨 、 》 ！`) iki
hücrelik kutuya göre ortalanır ve bugünkü yerlerinden kayardı; yan kazanç
kapasite — o 65 ikinci bir yuva da harcamıyor. Sütun sayısının **tek yetkilisi
`unicode-width`'in tablosu** ve iki yüzey de onu okuyor: ızgara alacritty
üzerinden (`Flags::WIDE_CHAR`), dock ise doğrudan (`dock::column_width`,
024). Dock'un ızgaraya **sorması mümkün değil** — çizdiği şey ZLE'nin
`BUFFER`'ı, ızgaranın hücreleri değil — ve tabloyu paylaşmaları tam bu
yüzden zorunlu: ikinci bir tablo ayrıştığı gün dock bir sütun kayar ve
belirti sessizdir. `bt-atlas` onu hiç görmüyor; kutu genişliğini argüman
olarak alıyor ve `Cell::wide` sınırdan geçiyor. Yelpazeleme
`AtlasTexture::prepare`'de, `Frame::push`'ta **değil**: "bir yuva mı iki mi"
kararı mürekkep kapısında doğuyor ve sink atlası ödünç alamıyor — yan kazanç
üç yüzeyin (ızgara, doldurma bandı, dock) tek yerden kazanılması. Çift
**atomik** ayrılıyor, yoksa kapasite sınırı iki yarının arasına düşer ve
ekranda yarım glyph + yarım kutu belirirdi. **Renk ikinci bir düzlem**
(`bt_atlas::Plane`), ikinci bir `Atlas` değil — o beş CoreText türetmesini ve
ikinci bir `Metrics`'i doğururdu; düzlemin **kendi monoton sayacı** var (uv
`prepare` anında pişiyor, kare ortasında anlamı değişen paylaşımlı bir sayaç
önceki geçişlerin uv'lerini geçersizleştirirdi) ve dokusu **tembel**, ilk
renkli yuvayla doğuyor. Düzlem kararı fontun **trait bitinden**
(`kCTFontTraitColorGlyphs`), aile adından değil. Kapsam dışı ve adıyla
yazılı: grapheme dizileri (ZWJ, ten rengi, VS16 — anahtar `char` değil `&str`
olmak zorunda) ve **tek sütunlu emojinin 78'i** (rengi var, iki sütunu yok,
mürekkebi 1.66 hücre — çaresi küçültme ve o ayrı bir karar). **Blok elemanları, Braille,
çizgi çizim ve terminalin grafik kümesi kapının konusu değil, artık
yordamsal çiziliyor** (021,
`raster::is_procedural`): U+2580–U+259F, U+2800–U+28FF, U+2500–U+257F ile
U+23B8–U+23BF fonta hiç sorulmadan hücre ölçüsünden hesaplanıyor ve kapı `Atlas::slot`'ta,
`Sprite::Rule` kolunun ikizi olarak, **yedekten önce** duruyor. Sıra zorunlu
ve "fontta yoksa yordamsal çiz" yanlış kol olurdu: `█` Menlo'da *var* ama
hücreyi doldurmuyor (8×18 hücrenin yalnız 3–16 satırları), yani yedeğe hiç
gitmeden bozuk geliyor; `⠋` ise Menlo'da yok ve yedek koşarsa Apple
Braille'den gelirdi. **Yordamsal çizim fontu
koşulsuz yeniyor** — kullanıcı bu karakterleri taşıyan bir font seçse de
kazanıyor, çünkü fontun em kutusunun hücre kutusu olacağını garanti edecek
hiçbir ölçüt yok; 012 phase-9'un prompt işareti kararının aynısı. Yüz
`Face::Regular`'a **normalize** ediliyor (ince/kalın ayrımı Unicode'da
karakterin kendisinde: `─` U+2500 ince, `━` U+2501 kalın), yani dört yüz tek
yuva paylaşıyor. Kapı **küçük sınıfta kapalı** ve gerekçe döşeme değil ölçü
ayrışması: sprite büyük hücre genişliğinde çizilir, dock'un bağlam satırının
sütun adımı ise küçük yüzün ilerlemesi — komşu hücreler örtüşürdü. Gölgeler
(`░▒▓`) dama deseniyle değil **düz kapsamayla** çiziliyor: dama ancak adım
hücrenin iki ölçüsünü de bölerse döşer ve bölmüyor (ölçüldü: 13pt@2x hücresi
16×33, yüksekliği tek). **Çizginin dört kolu var** (yukarı/aşağı/sol/sağ) ve
her kolun stili {yok, ince, kalın, çift}; tablo **gerçek tablo**, formül
değil — aynı ailenin iki yarısı (`251C..2523` ile `252C..2533`) kalın
maskesinin iki ayrı permütasyonu. İnce kalınlık alt çizginin kalınlığı, kalın
onun iki katı (`HEAVY_FACTOR`, tasarım sabiti), çift ise iki ince ray ve
aralarında bir kalınlık boşluk (`RuleKind::Double`'ın aralığı). Raylar piksel
ızgarasına **oturtuluyor** (`raster::rail`): 13pt@1x'te dikey eksen x = 4.0 ve
yuvarlanmamış bant iki sütuna %50'şer düşerdi, yani dikey çizgiler gri
yataylar net olurdu. Çift çizgi bir çizgi değil **iki duvarlı bir kanal** ve
kavşaktaki bütün kararlar tek cümleden çıkıyor — kanal kapanmaz: `╠`'te dış
duvar kesintisiz geçiyor, iç duvar kırılıp yatay raylara dirsek yapıyor;
`╬`'in ortasından boş bir satır ve boş bir sütun geçiyor; `╪`'nin tek dikey
çizgisi ise kanalı **geçiyor**, çünkü karşı kolu var — `╤`'nin sapının yok ve
o yüzden alt rayda duruyor. Yuvarlak köşeler (`╭╮╯╰`) chevron'un mesafe
alanının ikizi, `curl`'ün değil: yarıçap dört köşede de aynı ve oturtulmuş
eksenlerin hücre kenarlarına uzaklıklarının en küçüğü. Kesikli aile
`dividing_period`'u **koruyor**, yani periyot hücreyi tam bölüyor ve bedeli
görünür bir bilgi kaybı: sekiz genişlikte `┄` ile `╌` aynı sprite'a çöküyor.
Döşeme bu setin varlık sebebi; onu üç yoğunluğun ayrışması için feda etmek işi
kendi amacına çevirirdi. **Köşegenler (`╱╲╳`) kapsamın dışında** ve bu
aralığın içinde bilerek bırakılmış bir delik — üçü de nadir, mesafe alanı
onları da çizebilirdi. Deliğin ikinci bir işi var: Menlo Regular'da olup
Bold'da olmayan **tek** blok o aralık, yani yüz merdiveninin tek bekçisi
(`face_fallback_is_cached_under_the_requested_face`) onların içinde yaşıyor.
Doğrulama sprite başına el yazması sınamayla değil **değişmezle**: kenar
profili yalnız kolun stiline bağlı (dikiş), ayrık kol kümelerinin piksel-max'i
birleşim kümesini veriyor (`┌ ∪ ┘ == ┼`) ve kol tablosunun ikinci kopyası
sınamaya **Unicode adlarından** yazıldı — birleşim yasası aynalanmış bir
tabloyu göremez, adlar görüyor.
**Terminalin grafik kümesi (U+23B8–U+23BF) aynı ailenin akrabası ama ekseni
kenarda** (`raster::technical`): iki dikey kenar çizgisi (`⎸⎹`) hücrenin sol
ve sağ sütununda, dört tarama satırı (`⎺⎻⎼⎽`) hücreyi dokuza bölen bantların
1., 3., 7. ve 9.'sunda, iki köşe (`⎾⎿`) sol kenarda tam boy dikey artı üst
ya da alt kenarda tam boy yatay. U+2500 ailesinin `LINES` tablosuna satır
eklenmiyor, çünkü orada eksen `w/2` / `h/2` olarak yazılı ve indeks
`cp - 0x2500`. Formül ikinci bir sabit uydurmuyor: beşinci bant tam olarak
hücrenin ortası, yani Unicode'un `─` ile birleştirdiği tarama satırı
`arm`'in yatay kolunun yerine düşüyor. Üçünün de fonttaki hâli **ayrı ayrı
kusurluydu** (ölçüldü, Menlo 16pt@2x, hücre 20×39): `⎾⎿` cascade'den
1.66× ilerlemeli bir Hiragino glyph'iyle geliyor ve mürekkebi kutunun sağ
yarısında olduğu için kapıdan dönüyordu — belirti kullanıcıda görüldü, Claude
Code araç sonuçlarını `⎿` ile başlatıyor; `⎸⎹` Apple Symbols'tan 0.42×
ilerlemeyle gelip **ortaya** kayıyordu (sol kenar çizgisi solda durmuyordu);
`⎺⎻⎼⎽` Monaco'dan geliyor ve 20 px hücrede 0.03–19.19 boyadığı için yan yana
**döşemiyordu**. `⎷` (U+23B7) bilerek dışarıda: kök işaretinin kuyruğu bir
ray değil ve cascade'den gelen hâli kapıyı geçiyor — kapsama almak kusuru
değil çalışan bir glyph'i değiştirirdi. Doğrulama yine değişmez: kenar
profili piksel piksel (`⎿` = sol sütun ∪ alt satır), `⎾` ile `⎿` ve `⎸` ile
`⎹` birbirinin **tam aynası**, beş bant yukarıdan aşağıya sıralı, hücreyi
boydan boya geçiyor ve eşit aralıklı.
`bt-gpu` ile `bt-core` bundan **habersiz** — `Sprite::Char(ch)` çağrısı aynı,
yuva aritmetiği aynı. Aşağıdaki sözleşme kod geldikçe
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
                  # kare=N hucre=K glif=G kural=R yuva=U/T yuva2=U/T yuk=smoke istek=I icerik=C hareket=M kayma=S sessiz=Sms kapanis=clean profil=debug ornek=off pipeline=ok
                  # ilk dördünden ya da hareket'ten biri 0 ise, icerik > IDLE_FRAME_LIMIT ise, sessiz < QUIET_FLOOR ya da sessiz=none ise
                  # ya da deadline'da animasyon yerleşmemişse kırmızı. iki sınır da ölçülmüş; değerleri ve türetmeleri sabitlerin doc'unda.
                  # üst sınır kare'de değil icerik'te: icerik çizilmeye karar verilen kare, kare GPU'nun bitirdiği — animasyon ikincisini meşru olarak şişirir.
                  # sessiz'in kuralı ters (sağlıklıda büyük) ve kapının en duyarlı katı: icerik sınırının göremediği yavaş sızıntıyı o görüyor.
                  # yuva/yuva2/yuk/istek/kayma/profil sayaç ve etiket; kapanis kısmen kapı (değerler teardown_token'da); ornek=off'ta ölçüm jetonu basılmaz.
                  # yuva atlasın maske düzlemi, yuva2 renk düzlemi (023): ikisi aynı yuva ızgarasını paylaşıyor, ayrı sayaçları var ve toplamları aynı.
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
| `bt-core` | VT durum makinesi, grid ve scrollback, PTY ve okuyucu thread (PTY okuma yolu **taranıyor**: araya giren sarmalayıcı baytları aynen geçirir, geçerken **üç** OSC numarasını ve **bir** CSI dizisini çeker), OSC (7/8/9/52; 7 çalışma dizinini dock'un bağlam satırına verir, 52'nin yazma yönü `Wake` ile kabuğa çıkar, panoyu görmez), komut blokları, seçim, girdi kodlaması (DECCKM'e uyan oklar, farenin düğme/hareket/tekerlek raporu; kipten karar veren tablolar `input::button_route`/`motion_route`/`wheel_route`), ayar modeli, shell bağlamı. Tarayıcının üç kolu var ve üçü de alacritty'de **yok** (`vte` üçünü de `unhandled`'a düşürüyor): OSC 133 oturumun safhasını ve blok kimliklerini `ShellState`'e yazar (`Session::shell_state()`), OSC 8133 ZLE'nin görüntü aynasını — `PREDISPLAY`, `BUFFER`, `POSTDISPLAY`, `region_highlight`, `CURSOR`, base64 gövdelerle — çözüp `DockState`'e (`Session::dock_state()`) ve dalı `DockContext`'e, OSC 7 de çalışma dizinini yine `DockContext`'e (yüzde çözme ve yabancı host elenmesi orada; bozuk URI panik değil yoksayma). Aynanın kendi yük sınırı var ve aşımı **görünür** (`DockStatus::Unavailable`), sessizce düşmez; satır sonu taşıyan görüntü de görünür bir durum (`DockStatus::Multiline`) — dock tek satır, satırı ızgaraya bırakıyor. **Dördüncü kol OSC değil CSI** ve yükü yok: `CSI 2 J`'yi tanıyıp "ekran kasten temizlendi" bayrağını kurar (`Session::observe_screen_clear`; `3J` ve RIS için kol **yok**, ikisi de geçmişi siliyor). **Alternatif ekranda kurmaz** — orada `ClearMode::All` `reset_region(..)` çağırıyor, geçmiş büyümüyor ve birincil ekranın durumuna dokunulmuyor, yani geri getirilmeyecek bir şey yok; nesil yine de **tüketilir**, yoksa `vim`'den çıkışta birikmiş sayaç bayrağı kurar ve doldurma ilk `vim`'den sonra kalıcı olarak kapanırdı. Bayrak **defter temizlemeden sonra büyüyünce** düşer: geçmişe temizlemeden sonra satır düşmüş demektir ve doldurma o kadarını güvenle geri verebilir. Ölçüt bir damga ve tek karşılaştırma (`Session::screen_clear_history`); damga bayrak kurulduktan **sonraki** ilk karede alınıyor, çünkü kuran kare ızgarayı henüz temizlenmemiş görebiliyor ve temizlemenin kendisi satırları geçmişe itiyor — bayat damga anında aşılırdı. Üstünde iki koşul var — alternatif ekranda değil ve `display_offset == 0`; ikincisi olmasa geçmişe kaydırılan pencere dolu **görünür** ve tek bir tekerlek jesti Ctrl-L'i geri alırdı. (Bu koşul **bayrağın ömrüne** ait; doldurmanın kendi `display_offset` kapısı ayrı bir şey ve ayrı gerekçeli.) **Bayrak bir kapı, damga bir ölçü:** kapı "hiç" der, aynı damga doldurmada ikinci kez okunup `fill`'i temizlemeden beri gelen satır sayısına **kırpar** — yoksa tek satırlık bir büyüme bayrağı düşürür ve doldurma boşluğun tamamını, yani kullanıcının sildiği ekranı geri getirirdi (ölçüldü). `content_rows == rows` kolu yok: dock'lu pencerede doluluk giriş satırını saymadığı için erişilemez. **Bilinen sınır**, defter `scrollback`'te doyunca damganın üstüne çıkacak sayı kalmıyor ve o oturumda bir Ctrl-L'den sonra doldurma koşmuyor; yönü güvenli. Yarışı kapatan şey bir **nesil sayacı**: tarayıcı baytları uygulamadan **önce** sayıyor, kare yolu sayacı `Term` kilidinin **altında** doluluk sayısıyla aynı okumada tüketiyor, ve henüz hesaba katılmamış bir nesil aynı karede doldurma kuralını ezer. Bayrağın tek tüketicisi doldurmanın kapısı (`Session::fill_rows`) ve sıra zorunlu: ömür **önce** işliyor. Komut blokları `frame()` sınırından **çözülmüş** geçer (komutun satırı + renk, çıkış kodu değil; bölge değil işaret): kimlik prompt'un OSC 8 çıpasından `Term` kilidi altında toplanır, renk kilit bırakıldıktan sonra kabuk defterinden çözülür. Giriş satırının **bastırılması** da burada: safha ile aynanın durumu tek yüklemde birleşiyor (`ShellLog::suppressed_input`) ve kopya `Term` kilidinden **önce** alınıyor — yaprak kilit `Term`'ün altına girmez | macOS'a özgü **hiçbiri** — `objc2*`, `core-text`, `metal` yok. Unix PTY (`libc`, `rustix`, `polling`) serbest; kapı Linux hedefiyle derlemedir |
| `bt-atlas` | glyph rasterizasyonu, atlas paketleme, **iki düzlem** (maske `R8`, renk `RGBA8`; ayrı sayaç, ortak yuva ızgarası), **geniş glyph'in iki yarısı** (`Half`; kutu iki hücre, yuva yine bir hücre), **sistemin cascade'inden yedek glyph** (kapı geometrik ve **sıralı**: önce tek hücre, sonra iki; ikisine de sığmayan aday kutu kalır), **yordamsal karakterler** (blok elemanları, Braille ve çizgi çizim — köşegenler hariç; fonta sorulmadan, yüzden bağımsız, yalnız büyük sınıfta), font seti. **Doku kenarı sabit değil**: hedeflenen **yuva sayısından** türüyor (`SLOT_TARGET` = 1024 yuva; kenarın kendisi `MIN_EDGE` = 1024 px ile `MAX_EDGE` = 4096 px arasında, iki 1024 tesadüfen aynı sayı), çünkü hücre büyüdükçe kapasite düşüyor ve bir yerde yordamsal ailenin altına iniyordu — ölçülen kırılma Retina'da 29pt'ti (406 yuva, ailenin istediği 429: 421 karakter + tofu + kural payı). Varsayılan punto tabanda kalıyor, yani ızgara ve raster bit bit aynı. Tahliye **yok**: dolan atlas hâlâ tofu'ya düşüyor ve kalan senaryo (tek karede hedeften fazla farklı glyph) ölçülmedi | `objc2-core-text`, `objc2-core-graphics` ve ortak tabanları `objc2-core-foundation`. `objc2` çekirdeğini bile **görmez**: kullanılan her şey C API'si, ObjC runtime'ı değil |
| `bt-gpu` | Metal renderer, shader'lar (`.metal`), **geniş glyph'in yelpazelenmesi** (`prepare`; karar `Atlas::slot`'ta doğduğu için sink'te değil), display link ve `Waker` (kareyi süren ritim), kare yolunun **ölçüm defteri** (`Stats`: iki CPU aralığı, GPU deltası, açılış damgası, p95'in tabanı — biriktirir, **basmaz**), hareket (motion), **dock yüzeyi** (ikinci `setViewport`, kendi listeleri ve caret'i; kaç satır olduğu `DOCK_ROWS`), **doldurma bandı** (üçüncü `setViewport`, kendi listeleri; orijini ötelemeden türüyor, kaç satır olduğu `Cursor::fill`), overlay'ler (palet, arama), durum çubuğu | `objc2`, `objc2-foundation`, `objc2-metal`, `objc2-quartz-core`, `dispatch2` (metallib yükleme, ana kuyruk), `block2` (tamamlanma bloğu) |
| `bt-shell` | AppKit kabuğu: pencere, sekme, bölme, menü, klavye (metin yolu AppKit'in yığınından: `BateriView` `NSTextInputClient`, ölü tuş bileşimi orada tamamlanır), **Finder damlası** (`NSDraggingDestination`, yalnız dosya URL'si; yol `quote::shell_quote`'tan geçip `Session::paste`'e gider), servisler, ayar penceresi; kapanış sırasının ve duman bekçisinin sahibi; kabuğun başlangıç dizini, yereli, hangi kabuğun koşacağı ve sarmalayıcı betiğinin yeri (`child`), entegrasyonun kurulup kurulmayacağı ve `ZDOTDIR`/`BATERI_ZDOTDIR` çifti (`app::shell_integration_env`) | `objc2`, `objc2-foundation` (`NSLocale` dahil: kabuğun yereli), `objc2-app-kit`, `objc2-quartz-core` (yalnız `CALayer` takma), `dispatch2` (ana kuyruk: `child_exit` → `terminate:`, OSC 52'nin pano işi; vnode kaynakları: ayar izleme), `libc` (bekçinin `write` + `_exit`'i, izlemenin `O_EVTONLY`'si, kabuğun passwd kaydı için `getpwuid_r`) |
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
  **reddedildi**), `unicode-width` (dock'un sütun aritmetiği ve bastırmanın tazelik kapısı,
  yalnız `bt-core`'da) **yeni bir crate değil**, `polling` gibi: grafta zaten
  vardı ve **ızgaranın kullandığının ta kendisi** — alacritty
  `Flags::WIDE_CHAR`'ı onunla kuruyor ama yeniden ihraç etmiyor (ölçüldü),
  yani `bt-core`'un listesine yalnız bir kenar ekliyor ve hiçbir sürüm
  oynamıyor. Aynı tabloyu paylaşmak kararın **özü**, yan etkisi değil
  (`.tasks/024-dock-sutun-aritmetigi/discussion.md` → Karar 1).
  `toml_edit` (ayar ve tema dosyası, yalnız `bt-core`'da;
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
  ötekine geçilirse palet açılır (`0x1a1c21` ara tonu `0x5a5d65` griye) ve belirti
  sessizdir. Gören tek bekçi `cell_bg_paints_pixels_on_the_gpu` ve ancak **ara
  ton** bir renkle görür: `0.0` ve `1.0` sRGB transfer fonksiyonunun sabit
  noktalarıdır. **Bekçinin ara tonu bu yüzden temadan gelmiyor**
  (`renderer::tests::MIDTONE`): `bateri`'nin zemini artık **saf siyah**, yani
  sabit noktanın ta kendisi — tanığı temanın zeminine bağlı bırakmak onu bir
  zevk kararıyla körleştirirdi ve tam da öyle olacaktı. Clear yolunun tanığı
  ayrı ve hâlâ temadan (`accent`); ikisinin farklı renk olması şart, yoksa
  hücre yolu ile clear yolu birbirinden ayırt edilemez.
- **Boşta sıfır kare.** Kirli satır, yerleşmemiş animasyon **ve** ilerleyen
  bir süre sayacı yoksa frame gönderilmez; kare istemenin **üç** yolu var
  (`bt-gpu::link` modül başlığı): **hasar** (`Waker::wake`), **hareket** (uyanık
  callback'in kendi kararı, kimseyi uyandırmaz) ve **saat** (link uyumaya
  giderken kurulan tek gecikmeli uyandırma). Saatin **iki tadı** var ve tadını
  bekleyen işin cinsi belirliyor: *içerik tadı* hasar diker (koşan komutun süre
  sayacı; süresi ve durma koşulu `bt-core`'dan, `Cursor::next_tick`), *hareket
  tadı* dikmez (`Waker::resume`; imlecin yanıp sönmesi, fazın sahibi
  `bt-gpu::blink`). Kurulan uyandırma yine **tek**: iki son tarihten yakın
  olanı seçiliyor, çünkü `after` iptal edilemiyor. Animasyonun zamana bağlı kare talebi
  hareket saatinden geçer, **`Waker::wake`'ten değil** — oradan istenen bir
  hareket karesi kendini "içerik" diye saydırırdı. Yasağın öznesi o **kapı**,
  `Waker` tipi değil: `Waker::resume` hasar dikmediği için aynı yasağın altına
  girmiyor ve saatin hareket tadını o taşıyor. Saatin **içerik** tadı da
  istisna değil başka bir şey: animasyon aynı içeriği farklı çizer, o tat
  **içeriğin kendisini** değiştirir (koşan komutun süre sayacı), yani `icerik=`
  sayması doğrudur.
  Ölçütü üç şart — içerik gerçekten değişecek, periyodu ekran hızından çok
  düşük olacak, adlandırılmış bir durma koşulu taşıyacak. **Koşan komutu ya da sönen bir
  imleci olan pencere boşta değildir** — ve **odakta olmayan pencerede blink
  hiç koşmuyor**, yani orada saat de kurulmuyor (015 R7.4). İkisi de adlandırılmış bir durma
  koşulu taşıyor: komut biter, blink ise varsayılan **kapalıdır**
  (`[terminal] cursor_blink`), periyodu ayardan gelir
  (`cursor_blink_interval`; kısaltmanın bedeli doğrusal ve **kapı onu
  göremiyor** — süreli koşu ayar okumuyor, tek koruma kabul aralığı) ve
  açıkken bile klavye sessizliğinden sonra durur — durduğunda fazı **açık** bırakır, yoksa imleç bir sonraki hasara
  kadar kaybolurdu. Hareketi Azalt açıkken blink hiç başlamaz: erişilebilirlik
  ayarı animasyon *eklemez*. Kapı bu yüzden `kare`'ye değil **içerik** karesine bakıyor —
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
- **Tema = dokuz rol:** arka plan, ön plan, dim, accent, cursor ve dört durum.
  Bugün yedisi tüketiliyor — `background`, `foreground`, `dim` (SGR 2'li
  varsayılan ön plan), `accent` (koşan komut bloğunun şeridi), `cursor` (imleç
  bloğu **ve** ANSI 258'in cevabı), `success` ve `error` (biten bloğun şeridi)
  — ve yanlarında `[ansi]`'nin 16 rengi; kalan iki durum rolü (uyarı, bilgi)
  sonraki setlerde gelir. Çizilmeyen rol eklenmiyor. `cursor` **014'te
  ayrıldı**: ikisi tek değerden beslenirken "imleci altın yap" isteği koşan
  komutun şeridini de altın yapıyordu, ve 258 yuvası zaten `accent`'e takma
  addı. Tema dosyasının kuralı **istisnasız**: her anahtar opsiyonel ve eksik
  olan gömülü tabandan geliyor, `cursor` da. Bir dönem eksikte `accent`'i
  izliyordu ("rolden önce yazılmış tema dosyaları değişmesin"); uygulama
  yayınlanmadığı için koruduğu kimse yoktu ve yirmiden fazla anahtar içinde
  **tek** istisnaydı.
  `bt_core::Theme` paletin **tek kaynağı**: zemin atlaması,
  clear, imleç, blok şeridi ve renk sorusunun yanıtı aynı değerden. `Adapter`'da **yaprak
  kilit** altında durur; `frame()` kopyayı `Term` kilidinden önce alır,
  `set_theme` tek başına yazar ve kare ister (aynı temada no-op). Sönük
  (SGR 2) adlı renk temanın zeminine doğru üçte bir karışır
  (`color::dim_toward`); `dim` rolü varsayılan ön planın **ve** blok
  üstverisinin — komut süresi sayacı (013) ile dock'un bağlam satırı aynı
  rolden besleniyor, çünkü üçü de "okunacak metnin bir adım gerisi".
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
  yoksa o kabuklarda blok yok, işaret de yok. Sarmalayıcı bir de **`LISTMAX=0`**
  dayatıyor (yalnız dock'lu kademede, kullanıcının başlangıç dosyalarından
  **önce**, yani kendi değerini yazan kullanıcı kazanıyor; `LISTMAX` zsh'te
  varsayılan olarak set olduğu için "kullanıcı mı ayarlamış" sınanamıyor ve
  bedeli kapatan şey sıra): zsh'in varsayılan ölçütü seçenek **sayısı** (100)
  ve ekranı satırca aşan küçük bir liste sormadan basılıyor — basıldıktan
  sonra da zsh onu **temizlemiyor** (ölçüldü: taşan listede ne imleç geri
  alınıyor ne `ED` geliyor, yani 017'nin geri dönüşü tetiksiz kalıyor, çünkü
  zsh normal bir terminalde geçmişe kayan satırları geri getiremez). `0`
  ölçütü sayıdan yere çeviriyor: sığmayan liste **önce soruyor** ve `n` ekranı
  olduğu gibi bırakıyor. **Bilinen sınır**: `y` dendiğinde liste basılıyor ve
  yine kalıcı — ölçüt "bozulmadan önce sor", "geri getir" değil.
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
  skill'inin tablosunda, bekleyen iddialar `docs/OLCUMLER.md` → `## Bekleyen
  iddialar`'da — bir setin durumu ölçüm beklemez.
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
ve `/audit` set sonunda bir kez koşar; set defteri (`teslim.md`) ve "ölçüm
bekliyor" kalemi yoktur, panel yalnız pahalı kararda açılır
(`.claude/README.md` → Sadeleştirme).

**Boşlukta kullanıcı tarafı seçilir.** Bir kapı ya da jüri bulgusu kod
hakkında **olgu** verir ("şu aritmetik şunu yapamaz"); ondan çıkan "öyleyse
şunu yapmayalım" cümlesi **olgu değil seçimdir**. Çıkarımı karşılamanın iki
yolu vardır — özelliği kısmak ya da kodu açmak — ve ikisi kullanıcının
gördüğünde ayrışıyorsa karar ürün kararıdır, kullanıcıya sorulur. Belirsizse
varsayılan **kodu açmaktır**. Aynı kural talebin kendisine de uygulanır:
kapsamdaki boşluk kullanıcının lehine okunur. Yöntemi `.claude/skills/rfc`
→ Bulguyu işleme yolu; bedeli ölçüldü (023: dock'ta geniş bayrağın hiç
kurulmaması bir kod kısıtından *karar* diye türetildi ve kullanıcının yazdığı
emoji kutu çıktı). Bir kısıtı "yapısal olarak zorunda" diye yazmak o seçimi
görünmez kılar; ölçüt "bugünkü aritmetikle mi, gerçekten mi".

Hangi işin **neden o sırada** olduğu `docs/YOL-HARITASI.md`'dedir; henüz
açılmamış setlerin sırası ve bağımlılıkları oraya yazılır. **Durumu** o dosya
tutmaz — tek sahibi `.tasks/README.md` indeksidir, iki yerde durum tutmak
drift üretir.
