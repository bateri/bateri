# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Proje

`bateri`, macOS için GPU'nun çizdiği bir terminal emülatörüdür. Rust ile
yazılır; AppKit'e `objc2` ailesi üzerinden **doğrudan** bağlanır, GPU'ya
`wgpu` üzerinden (macOS'ta Metal arka ucu) ulaşır, Swift katmanı yoktur. Referans ürün Metalterm'dir (metalterm.dev, kapalı
kaynak): komut blokları, dokuz rollü tema modeli, grain/sheen ile materyal
yüzeyler, fizik tabanlı imleç hareketi ve boşta sıfır kare. Referansın binary
incelemesinden çıkan mimari, özellik ve ayar envanteri `docs/ARASTIRMA.md`'dedir;
bir işe başlamadan önce ilgili bölümüne bakılır, sıfırdan keşfedilmez.

**Bugünkü hâl** (hangi setin neyi getirdiği `.tasks/README.md`'de): `bt-core`
shell'i çalıştırır ve kareyi `frame()` sınırından verir — karakter, ön plan
rengi ve biçim (`bold`, `italic`, `underline`, `underline_color`,
`strikeout`). `bt-atlas` platformun font sistemiyle (`FontSystem` trait'i:
macOS'ta CoreText, Linux'ta FreeType + fontconfig + `harfrust`; 042) dört
font yüzünün glyph'lerini ve yedi
yordamsal sprite'ı (beş alt çizgi, üstü çizili ve prompt chevron'u) sabit yuva
ızgarasında rasterize eder; hücre ölçüsü oradan gelir ve `bt-gpu`
`Renderer::cell_metrics(scale)` ile yeniden yayınlar. **Seçili fontta olmayan
karakter sistemin cascade'inden geliyor** (`rules::fallback_font`, 019): yüz
merdiveni tükendikten sonra, negatif önbellekten önce, yani anahtar başına
atlasın ömründe bir kez ve kabul edilen aday sıradan bir yuvaya düşüyor —
yeni önbellek, yeni tavan, yeni tahliye yok. **Kapı geometrik**: adayın
**boyayacağı piksel** hücrenin dışına taşıyorsa kutu kalıyor — aile adı ve
trait biti yok; **tek ad** küçültme kolunun `.LastResort`'u (041,
`FontSystem::is_last_resort`, PostScript adı), çünkü geometri onu emojiden
ayıramıyor ve küçültülse de bir kutu çizer (oranlar ölçüldü, sayıları
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
glyph sessiz bir bozulma. **041'den beri sözleşme "kutu, tam glyph ya da
sığacak kadar küçültülmüş glyph"**: iki kapıdan da dönen aday, bugünkü
yerleşimle sığması için gereken küçültme (`rules::fit_ratio`) sınırın
içindeyse (`rules::SHRINK_LIMIT`, taramanın dağılımından; tek sütunlu emojiyi
@1x'te de kapsıyor — kullanıcı küçük emojiyi kutuya tercih etti) küçük
puntolu kopyasıyla kabul ediliyor, kopya kapıdan **yeniden** geçiyor ve
mürekkebi hücrede dikey ortalanıyor (`rules::Accepted::rise`); iki kapıdan
geçen aday bit bit aynı. Gerekçeler `.tasks/041-yedek-glyph-kucultme/`.
Ölçüt **yatay ve yalnız yatay**; dikeyi de sınamak
bugün hiçbir adayı elemiyor (ölçüldü: dikeyde taşan tek küme emoji; tam
boyuyla yatayda dönüyor, küçültülünce hücrede ortalanıp içine giriyor), o yüzden dikey taşma kutuya değil kırpmaya düşüyor ve sınır
adıyla yazılı (`rules::ink_fits_box`). Kaydırmanın formülü **tek yerde**
(`rules::centre_shift`, iki tüketici): kapı adayın **çizileceği** yerdeki
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
çizer. Pipeline **altı**: arka planlar/dörtgenler (`cell_bg`), glyph'ler ve
kurallar (`cell`), caret (`caret_fragment`), renkli emoji
(`emoji_fragment`), dock'un yazım efektleri (`glyph_fx`) ve fareyle seçim
(`selection`). Caret ile emoji
paylaşımla doğdu: caret `cell_bg`'nin, emoji `cell`'in **vertex'ini aynen**
paylaşıyor ve ayrılan yalnız fragment. Caret'te sebep bir SDF (yuvarlak
köşe, kenar, hale) ve o hesabı kare başına binlerce arka plan dörtgenine
ödetmenin anlamı yok — aynı fragment'in ikinci tüketicisi yükleme satırının
düğmeleri (dolgu + çerçeve, çizim başına bir dörtlü, `Renderer::encode_rounded`); emojide sebep rengi dokudan alması (instance'tan
değil) — baytlar **düz alfa** (ön çarpım yüklemeden önce geri alınıyor,
`rules::unpremultiply`), yani blend altı pipeline'da aynı (`SourceAlpha`).
`glyph_fx` kendi vertex'ini ve 48 baytlık instance'ını taşıyor, çünkü
dörtlüsü efekt payı kadar şişiyor ve fragment noktayı efektin ters
dönüşümüyle glyph uzayına çeviriyor; iki dokuyu birden bağlıyor, düzlem
instance'tan. `selection` `Instance`'ı **aynen** okuyan kendi vertex'ini
taşıyor, çünkü fragment dörtgenini bilmek zorunda (caret onu tek dörtgen
olduğu için uniform'dan alıyor); `rgba` yuvası orada renk değil köşe
maskesi, renk ile yarıçap uniform. **Geçmişte aramanın vurgusu (033) aynı
pipeline'ı ve şekli paylaşıyor**: renk uniform olduğu için rol başına bir
encode (`search_match`, sonra `search_current`), sıra zemin → eşleşme →
geçerli eşleşme → seçim → caret → glyph, yani kullanıcının seçimi aramanın
üstünde ve metin kendi renginde; ızgarada ve doldurma bandının viewport'unda
ayrı listeler. Köşeler **eşleşme başına** (`Frame::push_search`,
`SearchRun::continues`): ardışık satırlardaki iki eşleşme iki şekil, sarılan
tek eşleşme tek şekil. `bt-shell-macos` klavyeyi PTY'ye akıtır ve **metin yolu AppKit'in
yığınından geçer**: `keyDown:` tek kapı değil beş kollu bir arbitraj —
Cmd'li olay **kapalı bir izin listesinin üç tuşu dışında** yutulur (⌘⌫ →
`\x15` `kill-whole-line`, ⌘← → `\x01` `beginning-of-line`, ⌘→ → `\x05`
`end-of-line`; üçü de macOS'un satır jesti ve üçünün de baytı zsh'te
**gerçekten** bağlı — 018'in ölçtüğü karşılıksız diziler Home/End'in
şekliydi, bu baytlar değil. Liste kapalı kalmak zorunda, yoksa bir gün
Cmd-T kabuğa `t` yazar) ve geçen tuş da yığına girmez,
Shift+PgUp/PgDn terminalin kaydırmasıdır, **Control'lü
olay yığına hiç girmez** (numpad Enter'ın U+0003'ü Ctrl-C ile, Ctrl-Y'nin
U+0019'u Shift+Tab ile paylaşımlı; kolu AppKit'e bırakmak her komutu
kesebilirdi), **dock seçiminin tuşları** — değiştiricisiz ⌫/⌦/←/→,
⇧←/⇧→ ve ⇧⏎ (`keys::dock_key`) — yığından önce `Session::dock_key`'e sorulur
(tüketmezse bugünkü yolundan devam) ve kalanı `interpretKeyEvents:` ile
metin yığınına verilir. ⇧⏎ dock'ta satırı çalıştırmadan satır sonu ekliyor
ve **yapıştırmanın yolundan** gidiyor (`paste(b"\n")`): bracketed sarma
her keymap'te harfi harfine ekliyor, `\e\r` ise `viins`'te satırı kabul
ederdi (bekçisi iki keymap'te canlı zsh ile).
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
oradan bedavaya gelir — **yerel oturumda**; uzak oturumda yerel yol uzak
kabuğa yazılmıyor, damla uzak dizine **yükleniyor** (aşağıda, ssh). Kayıt tek tiple (`NSPasteboardTypeFileURL`): düz metin
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
kilitleniyor** (`gesture::Gesture`; jest defteri `NSEvent` görmüyor ve
sınanıyor), yoksa sürüklemenin ortasında
Shift'i bırakmak jesti değiştirirdi; bırakma o yüzden Shift'i sormuyor ama
**kipi soruyor** — uygulama bu arada çıkmışsa rapor kabuğun komut satırına
düşerdi. Bırakmanın koordinatı reddedilmiyor **kırpılıyor**: düşürmek
uygulamada takılı kalmış bir düğme bırakırdı. Doldurma bandının üstündeki
basış ne rapor ne seçim üretiyor (bandın satırları geçmişte, uygulamanın
ekranında yoklar). **Hareket de raporlanıyor**: 1003 her hareketi ister, 1002
yalnız basılı olanı, 1000 hiçbirini (`input::motion_route`) — düğme yolunda
tek cevap veren üç bit burada ayrışıyor. Pencere hareket olaylarını
**koşulsuz** dinliyor (`setAcceptsMouseMovedEvents`), çünkü kipe göre açmak
kipi `bt-shell-macos`'a yayınlamayı isterdi; bedeli düşüren şey kısmanın `bt-core`
çağrısından **önce** koşması — rapor hücre başına bir kez gidiyor
(`Gesture::moved_to`, ölçü görünür pencere hücresi) ve aynı hücrede
kalan hareket `Term` kilidine hiç uğramıyor. Çentiği basış ve bırakma da
tazeliyor. **Seçimin adımı tıklama sayısından**: çift tıklama kelime, üçlü
tıklama sarılmış mantıksal satır (`SelectKind`, alacritty'nin
`Semantic`/`Lines`'ı) ve sürükleme o adımla büyür; kelimenin tek tanımı
`WORD_SEPARATORS` (yol, `user@host`, `host:port` tek parça; `=` ayırıcı —
031 Karar 5). **Shift+tıklama var olan seçimin ucunu taşır**, tipini
koruyarak (`Session::extend_selection`; seçim yoksa oradan başlar) ve bu iki
kipte de aynı kural — fare kipinde Shift zaten seçimin tek yolu.
**⌘ basılıyken bağlantı alt çizgi ve el imleci alıyor, ⌘-tık onu açıyor**
(044; ızgarada, doldurma bandında ve dock'un giriş satırında, alternatif
ekran dahil): URL
(`http`/`https`/`ftp`/`mailto`/`file`), **var olan** dosya ya da dizin
(`~`, göreli yol pane'in OSC 7 dizinine; `:satır:sütun` tanınır, atlanmaz;
çıplak kelime de aday — `ls`'in `src`'si, `Makefile` — çünkü kararı şekil
değil varlık veriyor, iTerm2'nin semantic history kuralı; **boşluklu ad da**
— `My Drive`, `4.04.2022 06.29.36.pklg` — çünkü yol tek belirteç değil bir
**aday sorgusu**: noktanın çevresi `\t ():",`'den parçalanıyor ve önce sağa,
sonra sola bir parça büyüyen birleşimler en kısadan denenip **ilk var olan**
kazanıyor, yani komşu `ls` sütununu yutmuyor — `link::path_candidates` saf ve
en çok 100 aday, `LinkHit::candidates`/`choose`, karar
`links::resolve_first`; gerekçe ve iTerm2'den sapmalar
`.tasks/044-tiklanabilir-baglantilar/phase-1.md` → Uygulama Notları; URL tek
belirteç kalıyor) ve OSC 8
bağlantısı (metin taramasını yener). Algılama ve hit test `bt-core`'da
(`link`, `Session::link_at`; uzak oturumda `file://` bağlantı değil, düz metin
yol **uzak işaretli** hit — `LinkHit::remote` — ve varlığını pane başına tembel
açılan **yardımcı ssh oturumu** söylüyor (`remote_helper`, 045 Karar 10:
`BatchMode`, cevap uzak nesil başına önbellekte, nesil değişince ya da boşta
kapanır — yük göstergesi örnekledikçe boşta değil, açık kalır; 046 Karar 1); göreli adın tabanı uzak OSC 7 dizini, o yoksa başlığın `kullanıcı@host: dizin`
biçimi (Debian/Ubuntu'nun hazır `.bashrc`'si; boşluksuz `kullanıcı@host:dizin` de — oh-my-zsh; `Session::remote_link_directory`),
ikisi de yoksa göreli ad bağlantı değil ve nedeni de
açılamayan oturumun nedeni de pane'in etiketinde; sağ tık menüsü (Open
Preview, Download to Downloads/To…, Copy Path, Copy as scp Path) indirir —
klasörde, `ask` altında çakışmada ya da yer yokken onay sayfası, tek dosya
sorusuz, 037'nin kuyruğunda; **⌘-tık uzak dosyayı önizler** — kopya
`{preview_dir}/{host}/{uzak yol}`'a iner, salt okunur açılır, betik ve `x`
bitli dosya düz metin uygulamasında, `preview_max_size` üstü önce sorar, aynı
boyut+mtime yeniden inmez, klasöre ⌘-tık no-op; **⌘-sürükle** (basışta eşiği
aşan hareket, `Gesture`) file promise'le Finder'ın verdiği yere sıra
beklemeden indirir (`promise`); önizleme klasörü açılışta saklama + boyut
sınırıyla, günde bir saklamayla ve Clear Now ile süpürülür, bateri'nin
yazdığından farklılaşmış kopya silinmez, indirme klasörüne taşınır
(`preview_cache`; okunan dosya kullanıcının altından silinmesin — 045 Karar
9); gerekçeler `.tasks/045-uzak-dosya-indirme/`),
vurgu hover yuvasından ve damgalı (`Session::set_link_hover`; damga tutmazsa
çizilmez, düşer ve `Wake::link_hover_lost` ile pencere key'se yeniden
bulunur — akan çıktıda çıktı başına iki kare, boşta sıfır; **dock'ta damga
seçilebilir metin** (`PREBUFFER ++ BUFFER`), denetleyeni `Session::dock` ve
çizimi seçimin karakter aralığı yolundan (`dock::render_with`, ezme yardımcısı
aynı) — `BUFFER` ya da çizilen pencere (tekerlek, caret takibi, genişlik)
değişince düşer, çıktı onu bayatlatmaz; isabet son çizilen
pencereden ve dock'un tek düzen yürüyüşünden, `dock_select`'inkinden ayrı:
pay, prompt işareti ve öneri bağlantı değil), yolun varlığı
**arka planda** pane başına seri bir kuyrukta (`hyperlink`; ağ diskinde
takılan `stat` ana thread'i ve kareyi kilitlemesin) ve doğrulanmamış yol
vurgulanmıyor. ⌘ her olayda yeniden okunuyor; `flagsChanged:` ve pencerenin
key'liğini kaybetmesi (uygulamanın deaktivasyonu dahil) vurguyu kaldırıyor.
**⌘ + doğrulanmış bağlantı her kipte raporu ve Shift'i yener** (jest
defterinin ön-rotası, `Gesture::pressed_link`): uygulamaya ne basış ne
bırakma gider, seçim başlamaz; açma bırakmada, basışta kilitlenen aralığın
üstündeyse ve çift tıkta bir kez. ⌘ fare kipinden kaçış değil — bağlantısız
hücrede ⌘'li basış bugünkü yolundan. Açma **beyaz listeli**
(`links::action`): URL ve bilinen içerik tipinde (UTType: metin/kaynak,
görsel, PDF, ses/görüntü; betik ve çalıştırılabilir hariç) `x` bitsiz dosya
varsayılan uygulamada, dizin Finder'da, kalan her şey — paket dizini dahil —
Finder'da **gösteriliyor**, OSC 8'in yaygın olmayan şeması onay sayfası
istiyor; **`bateri://` hiçbir yoldan `NSWorkspace`'e verilmiyor**, yutuluyor
(038 Karar 7: kendimize yollamanın anlamı yok). El imleci yükleme
düğmeleriyle tek cursor-rect listesinde. **⌘'siz yalnız OSC 8 bağlantısı
vurgulanıyor, kesikli** (`hyperlink::hover_style`; el imleci yok, tık seçim):
metni hedefini söylemiyor, düz metin bağlantı ise kendi hedefi ve ⌘'siz
vurgulansaydı her `ls` kelimesi yanardı. **⌘ ile OSC 8'in üstündeyken hedef
pane'in sol altındaki etikette** (`TerminalPane::set_link_target`;
`hitTest → nil`, kare yolunun dışında, uzunsa ortadan `…`), çünkü tıklamadan
önce görmek güvenlik. **Bağlantı üstünde sağ tık bir menü açıyor** (URL'de
Open Link / Copy Link, var olan yolda Open / Reveal in Finder / Copy Path;
ızgarada fare kipi kapalıyken ya da Shift'le, bantta ve dock'ta her zaman —
oralar uygulamanın ekranı değil): "Open" tıkın politikasından geçiyor, yol
yine arka planda doğrulanıyor ve bağlantısız yerde sağ tık bugünkü gibi
hiçbir şey. Gerekçeler
`.tasks/044-tiklanabilir-baglantilar/discussion.md` → Karar ve Muhakeme. Fareyle
seçim, pano, geçmişte
kaydırma, ana menü (About, Settings…, Quit; Shell'de New Window/Tab, New
Local Tab (⌥⌘T), Mark “{host}” as ▸, Cancel Upload (⌘.), Split
Right/Down (⌘D/⇧⌘D) ve Close Tab/Window — çok pane'de ⌘W'nin başlığı
"Close"; Edit'te Cut/Copy/Paste/Paste Escaped Text/Select All
ve Clear to Start/Clear Scrollback — Cut yalnız dock
seçimi varken ve düzenleme kapısı açıkken etkin (`validateMenuItem:`,
varsayılan cevabı `true`); ⌘A geçmişin tamamını
(dock caret'in sahibiyken dock'un satırını) seçer ve menüden yakalanır, Cmd
izin listesi değişmez; Edit ▸ Find ▸'de Find…/Find Next/Find Previous/Use
Selection for Find (⌘F/⌘G/⇧⌘G/⌘E), seçicileri kendi adlarımız ve karşılayanı
`TerminalPane` — `performFindPanelAction:` alan odaktayken alan
düzenleyicisine yutulurdu; View'da Theme ▸, Cmd +/−/0 geçici
punto ve Scroll to Top/Bottom, Page Up/Down (⌘Home/⌘End/⌘PgUp/⌘PgDn,
`scroll_page`'in yolu; menü kısayolu, tuş kodlaması değil); Window'da sekme
geçişi, Select Tab ▸ ve bölmeler — Select Previous/Next Split, Select
Split ▸, Resize Split ▸, Equalize Splits, Zoom Split) ve kapanış sırası ondadır.
**⌘K terminalin işi, kabuğun değil** (034): Clear to Start ekranı ve
geçmişi siler, ⌥⌘K Clear Scrollback yalnız geçmişi; ikisi de tek `Term`
kilidi turunda (`Session::clear_to_start`/`clear_scrollback`) ve kabuğa ya
da koşan programa **tek bayt gitmiyor** — komut koşarken de çalışıyor, `cat`'in
girdisine `^L` düşmüyor. Korunan ilk satır imlecin blok kimliğini taşıyan en
üstteki ekran satırı (sarılan ve çok satırlı giriş, `PREBUFFER`, çok satırlı
`PS1` bütünüyle kalır; kimlik yoksa imlecin satırı), üstü ekrandan atılıyor
ve temizlik `2J` neslinin ikinci yazarı, yani doldurma bandı silineni geri
getirmiyor. Alternatif ekranda temizleme ve dört kaydırma öğesi **gri** —
birincil geçmiş orada erişilemez (`Term::inactive_grid` özel). ⌃⌘V Paste
Escaped Text panoyu tek argüman yapıp yapıştırıyor: satır sonu yoksa
damlanın ters bölüsü, varsa bütünüyle tek tırnak (`quote::paste_quote`) —
`\` + satır sonu satır devamı olup parçaları birleştirirdi. Gerekçeler
`.tasks/034-ekrani-temizle/discussion.md` → Karar.
**Geçmişte arama paneli AppKit'in** (033, `search_bar`): içerik view'ı düz
bir kapsayıcı, `BateriView` onun çocuğu ve panel (`NSSearchField` + `Aa`/`.*`
+ sayım + oklar + kapatma) sağ üstte üstüne biniyor — PTY boyutu ⌘F'de
değişmiyor. Alanda ⏎ daha eski, ⇧⏎ daha yeni eşleşme, Esc paneli kapatıp
geçerli eşleşmeyi seçim bırakır ve pencereyi yerinde tutar; geçerli
eşleşme, gezinme ve pencerenin ona süzülmesi `bt-core`'da
(`Session::set_search`/`search_next`/`search_reveal`, hedef vurgunun
kümesinden) ve panelin örttüğü hücreleri `bt-shell-macos` satır/sütun olarak
veriyor (`SearchCover`). Etiketin sayımını ("3 of 17…") ana kuyrukta tur
başına **bir parça** süren `TerminalPane::kick_search` sürüyor, çünkü tuş
olayları turların arasına girmeli; tetiği yüksüz ve kenarda
`Wake::search_changed` (arka sekmede de). Gerekçeler
`.tasks/033-gecmiste-arama/`.
**Sekmeler macOS'un kendi sekmeleri** (026) ve **sekme bölünüyor** (039):
her sekme bir `NSWindow`, içinde bir ya da daha çok pane (`TerminalPane`) ve
her pane'in kendi `Session`/`DisplayLink`/`Renderer`'ı — kural artık "bir
pane = bir oturum" (renderer pane başına, çünkü atlasın anahtarı punto ve
punto farkı pane'in; 039 Karar 5). Düzen saf bir ikili ağaçta (`split`:
yaprak pane kimliği, düğüm eksen + oran; çerçeveler ayırıcı dahil aygıt
pikseline oturuyor) ve onu pane'lere uygulayan `contentView` kapsayıcısında
(`split_view::SplitView`; ayırıcı bir piksel, temanın `separator` tonu —
`Theme::separator_srgb`, dock'un çizgileriyle tek zincir). ⌘D sağa, ⇧⌘D
aşağı böler; yeni pane odaktakinin dizinini, punto farkını, temasını ve uzak
satırını devralır (⌘T'nin kuralı, `Opening::Split`) ve iki yarıdan biri en
küçük pane sınırının (`MIN_PANE_COLS`/`MIN_PANE_ROWS`, tasarım sabiti)
altına düşecekse bölme gri ve no-op. **Odaktaki pane** pencerenin first
responder'ının pane'i (`TerminalWindow::focused_pane`; değişimi pencerenin
`firstResponder`'ının KVO'sundan — arama alanına tık dahil — ve
`PaneHost::focused`'tan): başlık, `⇄`, yükleme yüzdesi ve sekme noktası ondan
ve odakla değişir; odakta olmayan pane'in caret'i içi boş (odağın ikinci
biti, `bt-gpu` değişmeden), pencerenin key biti, örtülme ve ölçek bütün
pane'lere. ⌘W odaktaki pane'i kapatır (koşan iş varsa yalnız onu sorar,
"Close this pane?"), son pane'de sekmeyi; kabuk çıkınca yalnız o pane
kapanır ve odak ağaçtaki komşuya geçer; ⇧⌘W, kırmızı düğme ve ⌘Q bütün
pane'leri kapatır ve soruyu pane'lerden toplar — tek pane'li sekmede metin
bölmelerden öncekinin aynısı, çok pane'de "pane" sayar (`window::unit_for`).
**Gezinme ve düzen de ağaç işlemi** (039 phase-4): ⌘[ / ⌘] ağaç sırasında
döngüsel, ⌥⌘ + ok yöndeki pane (kenardan, dik eksende en çok örtüşen;
`split::Layout::neighbour`), ⌃⌘ + ok o eksendeki en yakın ayırıcıyı odaktaki
pane'in bir hücresi kadar taşır, ayırıcı sürüklemesi de (isabet alanı
çizgiden geniş, pane'lerin üstündeki saydam tutamak; `split_view`), ⌃⌘=
aynı eksendeki pane'leri eşitler, ⇧⌘↩ odaktaki pane'i büyütür/geri alır
(ötekiler gizli ve link'leri örtülmüş pencere gibi sıfır kare çiziyor;
bölme, gezinme, boyutlama, eşitleme ve pane kapanışı büyütmeyi bırakır).
Boyutlama ve sürükleme **en küçük pane sınırında** durur ve sınır yaprak
başına, pane'in kendi hücresinden (`TerminalPane::min_size`; bölmenin
kapısı da o); pencere küçülünce pane'ler oranlarını korur. Hepsi Window
menüsünün öğesi, tek pane'de gri; menü `keyDown:`'dan önce eşlediği için
Cmd izin listesi üç tuşta kalıyor ve dock'un ⇧⏎'si değişmiyor. **Odakta
olmayan pane soluk** (Karar 7): pane'in en üstündeki `hitTest` → `nil` bir
AppKit örtüsü (temanın zemini, `DIM_ALPHA` saydamlığında, tasarım sabiti) —
kare yolunun ve `bt-gpu`'nun dışında, yani boşta sıfır kare korunuyor; tek
pane'de örtü yok. Gerekçeler `.tasks/039-terminal-pane-bolmeler/discussion.md` → Karar 6–14. **Pane ile sahibi arasındaki sınır üç parça** (039 Karar 1–3):
girdiler doğumda tek pakette (`PaneLaunch`: ayar anlık görüntüsü, tema,
`Run`, `Stats`, entegrasyon ortamı + dock payı, kimlik, dizin ve ilk girdi,
hareket bayrakları; canlı değişim pane'in `set_*` yöntemleriyle), olaylar
`PaneHost` trait'inden (başlık, kabuğun çıkışı, yükleme durumu, bildirim,
alt başlık tanısı, OSC 52 kopyası; bugünkü sahip `window::WindowHost`) ve
menünün karşıladığı her pane işi pane'de adlı bir yöntem, seçici onu
çağıran bir satır. Pane düzeyindeki seçiciler (punto, bul, temizle,
kaydır, `cancelUpload:`) ile arama paneli ve yükleme kuyruğu pane'de,
çünkü responder zinciri `BateriView` → pane → kapsayıcı → pencere →
delegate — hedefsiz öğe odaktaki pane'e varıyor, arama alanı odaktayken de;
sekme işleri (`closeTab:`, `closeWindow:`, `selectTab:`, bölme, gezinme ve
düzen eylemleri) pencerede. Pane modülü
`AppDelegate`'e uzanmıyor: ana kuyruk dönüşleri pane'i sahibin verdiği
yoldan (`PaneLookup`, düz `fn`) kimlikle buluyor, `BateriView` sahibini
`superview()`'dan. Arama paneli kapsayıcısını **tutmuyor** (pane → panel →
pane çemberi pane'i hiç düşürmezdi); yeni sekme etkin sekmenin OSC 7 dizininde ve punto farkıyla
doğar (uzak sekmede ⌘T ve `+` **aynı host'a** gidiyor: yerel dizinde yerel
bir kabuk, ilk girdisi hedefin satırı + `\r` — `SessionOptions::initial_input`,
sarmalayıcılı oturumda ilk kimlikli `A`'da; o ana kadar yazılan tuşlar
satırın **arkasına** tutuluyor. ⌥⌘T New Local Tab ve ⌘N her zaman yerel;
037 Karar 6), sekme kısayolları (⌃⇥ dahil) menü öğesidir ve `keyDown:`'ın Cmd
izin listesi üç tuşta kalır, arka sekme örtülme yolundan sıfır kare çizer, kabuk çıkınca
yalnız o pane (son pane'se sekme) kapanır ve son pencere kapanınca uygulama açık kalır —
gerekçeler `.tasks/026-sekmeler/discussion.md` → Karar. **`bateri://`
şemasının iki yolu var** (038): `block/N` prompt'un iç OSC 8 çıpası ve
dışarıya hiç verilmiyor; `tab/<id>` **pane'in** dış adı (`BATERI_TAB_URL`,
`TERM_SESSION_ID`; pane başına, 039 Karar 10) — `open` ile o pane'in
sekmesi öne gelir (küçültülmüşse geri açılır) ve klavye o pane'e, ölü kimlikte
yalnız uygulama, başka her biçimde hiçbir şey (`application:openURLs:`).
**URL yalnız odaklar**: kabuğa bayt göndermez, komut koşturmaz, pencere
açmaz — bir güvenlik değişmezi, çünkü şemayı her uygulama açabilir
(`.tasks/038-terminal-kimligi/discussion.md` → Karar 5–7). **Kapatmak koşan işi
sorar** (028, `[terminal] confirm_close`): kabuğun dışında ön planda bir
program varsa (süreç tablosundan, adıyla — `jobs`) ⌘W, kırmızı düğme, "Close
Other Tabs" ve ⇧⌘W jest başına **tek** sayfa, ⌘Q tek uyarı açar; boş kabuk,
`exit` ve süreli koşu hiç sormaz. Kırmızı düğme ve sekme menüsü grubun
sekmelerine birer `windowShouldClose:` yolladığı için karar bir tur sonra
jestin kapsamıyla veriliyor, ⌘W de `performClose:` değil `closeTab:` —
gerekçeler `.tasks/028-kapatma-onayi/phase-2.md` → Uygulama Notları. **Krom temanın**
(`TerminalWindow::apply_chrome`): başlık çubuğu saydam ve ayırıcısız,
pencerenin zemini temanın `background`'ı (sRGB), görünümü zeminin
açıklığından (`window::is_dark_background`), yani tek sekmede başlık ile
içerik tek yüzey. Görünümü kurulan pencere sistemden miras almayı bıraktığı
için açık/koyu değişimi view'dan değil `NSApp.effectiveAppearance`'ın
KVO'sundan geliyor, geometri de pencereden değil `BateriView`'ın çerçeve
bildiriminden — sekme çubuğu pencereyi değil içeriği boyutlandırıyor
(`.tasks/026-sekmeler/phase-4.md` → Uygulama Notları). İçerik view'ı
bölmelerin kapsayıcısı (`SplitView`, üstten aşağı koordinat, pane'leri
kendi `resizeSubviewsWithOldSize:`'ında oturtuyor; tek pane'de pane sınırın
ta kendisi), her pane (`TerminalPane`, `NSView` alt sınıfı; oturumun
çekirdeği onda — 039 Karar 1–2) layer-backed düz bir **kapsayıcı** ve
`BateriView` onu autoresizing'le dolduran çocuğu, çünkü yüzen arama paneli Metal katmanının kardeşi olmak
zorunda (033 → R4.1). Uygulamanın
OSC 52 kopyasını (`Wake::copy_to_clipboard` → `PaneHost::copy_to_clipboard`)
genel panoya o yazar;
`settings.toml`'u okur (bugün `scrollback`, tema seçimi, font ailesi/puntosu/satır aralığı, `osc52`, `[remote] hosts`,
`cursor`, `cursor_blink`, `cursor_radius`, `cursor_glow`, `cursor_unfocused`,
`cursor_blink_interval`, `confirm_close`, `cursor_motion`, `reduce_motion`,
`smooth_scroll`, `keypress`, `erase`, `shell.integration` ve `[remote]`'un
önizleme/indirme anahtarları — `preview_max_size`, `preview_read_only`,
`preview_dir`, `preview_keep`, `preview_limit`, `download_dir`,
`download_conflict`, `download_notify`; 045 — ve yük göstergesinin `stats`,
`stats_interval`'ı; 046, ayar penceresinde Remote Files),
Theme ▸'nin seçimini oraya
yazar ve temayı `themes/{ad}.toml`'dan ya da gömülü
`bateri`/`bateri-light`'tan çözer. Ayar ve etkin tema dosyası **kayıt
anında** uygulanır (`watch`: macOS'ta vnode kaynakları, Linux'ta inotify; bildirim arka plandan, çağıran ana kuyruğa taşır; `Session::set_theme`,
`Session::set_terminal_options`, `Renderer::set_font`,
`DisplayLink::set_cursor_motion`, `DisplayLink::set_reduce_motion`,
`DisplayLink::set_glyph_fx`);
varsayılan tema sistemin açık/koyu görünümünü, `reduce_motion = "system"` de
sistemin Hareketi Azalt ayarını canlı izler; tek istisna `[shell] integration`,
kabuk çoktan doğduğu için **sonraki oturumda** geçerlidir. Kabuk zsh ise
`bt-shell-macos` sarmalayıcıyı `ZDOTDIR` ile kurar (betik `.app`'in
`Contents/Resources/shell`'inden, debug'da depodan) ve kabuğun bastığı OSC 133
işaretleri `Session::shell_state()`'te birikir. Aynı betik her satır çiziminde
ZLE'nin görüntüsünü (`PREDISPLAY`, `BUFFER`, `POSTDISPLAY`, `region_highlight`,
`CURSOR`; yedinci ve isteğe bağlı gövde olarak çok satırlı komutun kabul
edilmiş satırları `PREBUFFER`, bugün çözülüyor ama çizilmiyor) OSC 8133 ile
aynalıyor; `Session::dock()` onu **çözülmüş** dock
hücrelerine çevirip sınırdan veriyor ve `bt-gpu` pencerenin altındaki **ikinci
bir `set_viewport`**'la çiziyor — kendi listeleri, kendi caret'i, opak zemini ve
ızgaradan ayıran saç çizgisiyle. Aynı çağrı **ikinci bir sink**'ten, son
çizilen aynaya karşı bulduğu en çok bir `DockEdit`'i (`Arrive`/`Erase`/
`Shift`/`Reset`, **(satır, sütun)** konumuyla) veriyor: yalnız girdi sayısını
aşmayan tek bitişik ekleme ya da silme canlanıyor, gerisi uçuştakileri
bitiriyor — yazım animasyonlarının girdisi, tüketicisi `bt-gpu` (030; kural
`.tasks/030-dock-yazim-animasyonlari/discussion.md` → Karar 1–3). Sarılan ve
çok satırlı girişte de (032 phase-6): hayaletler eski düzenin konumunda, satır
sonunu aşan silmede alt satıra iniyor; düzenlemenin arkasında sarmayla kayan
harf canlanmıyor (statik glyph'ini bulamayan geliş bitiyor) ve oturma kuralı
okuma sırasında — `(satır, sütun)`'u düzenlemeninkinden büyük ya da eşit
gelişler biter. Tavanı aşan girişte dikey pencerenin kayması `Reset` değil
**satır** farkı (`shift`, `Shift`; son **çizilen** tepeden, `dock::with_shift`):
uçuştakiler metinle birlikte kayıyor, pencereden taşan düşüyor. `PREBUFFER`
değişimi ve satır sonu silen düzenleme `Reset`. **Zaman ve çizim
`bt-gpu`'da** (`glyph_fx::GlyphFx`, `Motion`'ın yanında ayrı bir ivar, blink
emsali): içerik karesi düzenlemeyi işliyor, uçuştaki gelişin statik glyph'i
çizilecek listeden çıkıyor (`Frame::set_dock_fx`) ama `dock_glyphs`'ten
değil — hareket karesi dock'u yeniden basmıyor ve efekt bitince glyph geri
gelmeli. Uyku testinin **dördüncü** terimi efektler ve `advance`'ten
**önceki** hâle bakıyor: efektin bittiği kare çizilmeden uyunsaydı yarı
saydam bir harf asılı kalırdı. Hasar dikmiyor, `icerik=` saymıyor. Her
efekt `t = 1`'de statik glyph'le **piksel piksel aynı** (geliş) ya da düz
zemin (hayalet) ve bu bir kapı (`renderer::tests`): devir karesinde harf
sıçramıyor. Efektler `[motion] keypress`/`erase`'ten **ham** iniyor ve
adların sözlüğü `bt-core`'un (`Keypress`/`Erase`; `NAMES` yalnız çizilebilen
adları taşır), shader kimliği `bt-gpu`'nun (`glyph_fx::Effect`). Geometri
ters dönüşümle (kutunun merkezi ya da sol kenarı, yarının değil) ve her
örnekleme yuvanın içinde: ölçekleyen dallar **doğrusal** örnekliyor ama
nokta texel merkezine kırpılıyor, yani süzgeç komşu yuvaya değmiyor
(bekçisi genlikten bağımsız: dolu ve boş komşulu iki atlas aynı kareyi
vermeli). Genlik, süre ve eğri tasarım sabiti (`shaders/glyph_fx.wgsl`,
`glyph_fx.rs`). **Efekt caret'in üstünde, kendi renginde ve dock bandıyla
kırpılmadan** çiziliyor, çünkü Backspace'ten sonra caret tam hayaletin
üstüne geliyor (`Renderer::encode_fx`; `phase-5.md` → Uygulama Notları).
Parçalı efektlerin tohumu girdi başına sabit, parçalar titremiyor. `heat`'in kızgın rengi temanın `cursor`'ı ve
instance'ta yeri olmadığı için kare başına bir uniform (`Frame::dock_fx_heat`);
renk düzlemi (emoji) boyanmaz ve eşiklenmez, renk dokudan. İndirgeme
`Motion::glyph_fx`'te — `snap` ikisini kapatır, Hareketi Azalt gelişi
belirmeye indirip hayaleti kapatır; ayar penceresi ezilen satırı devre dışı
bırakıp nedenini söylüyor (`settings_window::motion_override`). **Caret tek**: ızgaranın imleci ile dock'un
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
(`shaders/cell_bg.wgsl` → `caret_fragment`). Sayılar uydurulmuyor —
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
sinyal blink'in durması. **Odak iki bit** (033): caret "pencere key **ve**
klavye terminalde" değilse odaksız (`DisplayLink::set_keyboard_in_terminal`,
kaynağı `BateriView`'ın first responder kancaları — arama alanına yazarken
caret içi boş), seçim ve arama vurgusu ise yalnız pencere key değilken
soluyor; birleştirme `bt-gpu`'da tek yerde. Odak `bt-core`'a **hiç girmiyor**
(`DisplayLink::set_focused`; `CaretShape`'e de eklenmedi — o enum ayar
dosyasının sözlüğü, odak ona dik bir eksen) ve hermetik koşuda **hiç
okunmuyor**: kapı çağrı yerinde, `bt-shell-macos`'un pencere delegate'inde. **Alternatif ekrandan çıkışta imlecin
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
geçiyor ve orada en üstte kalıyor; ölçülen konum kaydırma kesrinden
**önceki**, yani kesirle banda itilen ızgara caret'i ızgarada kalıp harfiyle
birlikte dock'un zemininin altına giriyor ve ters çevirmesi bandın tepesinde
kırpılıyor. Ters çevirme dikdörtgeni **tek** ve pencere
uzayında, iki glyph encode'una da aynısı gidiyor — yazım efektlerine
gitmiyor: onlar caret'ten sonra ve ters çevrilmeden çiziliyor (yukarıda,
030). PTY payı `DOCK_ROWS * cell_h` **artı iki nefes
payı** (`bt_gpu::dock_px`; formülün tek kopyası orada, `split_into_grid` onu
tüketiyor; çizilen bant aşağıda ayrı): iki satır saç çizgisine yapışınca dock
bakılamaz duruyordu. Payın
kaynağı sol payın ta kendisi (`CellMetrics::gutter_px`) — ikinci bir tasarım
sabiti yok, aynı içi girinti iki eksende ve punto büyüyünce pay da büyüyor.
Saç çizgisi bandın **üstünde**, viewport'un tepesinde; **ikincisi** giriş
bloğu ile bağlam satırı arasındaki boşluğun ortasında (giriş satırlarının
kendi arasında çizgi yok — tek editör), aynı renk ve aynı kalınlıkta — boşluk ayrımı
önerir, çizgi söyler. Kenara değil ortaya konuyor, yoksa bir satıra yapışır ve
ona ait görünürdü. Satır arası boşluk bu yüzden dış payın **iki katı**: çizgi
her satırı kendi bandı yaptığı için bandın içi simetrik olmalı ve çizginin iki
yanına birer pay düşünce dock'un dört boşluğu da eşitleniyor (kalan ±1 px
çizgilerin kendi kalınlığından). phase-9'un `pad / 2`'si "dış boşluk içtekinden
büyük" kuralındandı; o kural **gruplar** için doğru, araya çizgi girince grup
kalmıyor. **PTY'nin ayırdığı pay ile çizilen bant ayrı** (032): pay
`DOCK_ROWS`'la sabit ve hiç değişmiyor (kabuk SIGWINCH görmüyor), çizilen
bant `Cursor::input_rows` giriş satırı + bağlam satırı (uzak oturumda giriş
satırı **sıfır**, aşağıda; `bt_gpu::band_px`;
boşluk ve ikinci saç çizgisi yalnız giriş bloğu ile bağlam satırı arasında)
ve **dibe yaslı** — hücreler yerleşimin viewport'undan, zemin bandın o anki
boyundan, büyüyen bandın üstüne taşan satırı makas kesiyor. Fark ızgaranın
çizimde ötelenmesiyle kapanıyor ve **işaretli**, kesirli tek formülden
(`link::band_target`): bant paydan uzunsa yukarı, kısaysa (uzak oturum) aşağı
— açılan tepedeki şeridi doldurma bandı kaydırılmış pencerede de kapatıyor
(`Session::grid_lowered`). Çizilen orijin `origin − band`, tek
yerde (`link::compose`) ve aynı yuvarlanmış pikselle, yani ızgaranın alt
kenarı, doldurma bandı ve bandın tepesi aynı karede çakışıyor; dolu ızgaranın
tepesi geçici olarak kırpılıyor. Bandın fazlası `Motion`'ın dördüncü
`Slide`'ında **iki yönde** süzülüyor (panelin boyu içerik değil, 011'in yön
kuralı ona uymuyor), `settled()`'e giriyor, snap/Hareketi Azalt/geometride
oturuyor ve değiştiği karede yükselen içerik hedefi de süzülüyor ki iki eğri
birbirini götürsün. Fare dock'u çizilen kareden okuyor (`Origin::dock`,
orijinle aynı yazımda). **Uzun satır sarılıyor ve bant büyüyor**: devam
satırları metnin sütunundan, satır sayısı `frame()`'de bastırmayla aynı
kilit turunda dock'un düzeninden (`dock::needed_rows`; öneri sayılmıyor, her
tuşta boyu değişip bandı nefes aldırırdı) ve tavanı ızgaranın yarısı
(`bt_gpu::DOCK_MAX_SHARE`; `DockBudget` oran taşıyor, çünkü satır sayısının
tek okuması `frame()`'de). Aşan girişte caret'i izleyen bir dikey pencere
açılıyor; dock'un üstündeki tekerlek ve bloğun kenarını aşan sürükleme onu
kaydırıyor (`Session::dock_scroll`; tepe aynanın yanında,
`ShellLog::dock_scroll`), caret'in yeri değişince pencere yine caret'i
izliyor. **Çok satır da dock'ta** (032 phase-4): satır sonlu görüntü
(yapıştırma, `Esc-Enter`) satır kırıyor ve `PREBUFFER` (ZLE'nin kabul
ettiği `for`/heredoc/`\`-devam satırları) düzenlenebilir satırların
**üstünde**, aynı girintide çiziliyor — seçilebilir ve kopyalanabilir ama
salt okunur: seçimin uzayı `PREBUFFER ++ BUFFER` (`dock::selectable`), ona
değen aralıkta düzenleme tuşları komut göndermiyor, tık caret'i taşımıyor;
⌘A ekrandaki bütün komutu seçiyor. Gerekçeler
`.tasks/032-cok-satirli-dock/phase-3.md` ve `phase-4.md`.
Dock ötelemeden
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
dizin **OSC 7**'den (tarayıcının üçüncü kolu; `file://` yetkisi boş,
`localhost` ya da bu makinenin adı olmalı — ad `gethostname`'den
`SessionOptions::hostname` ile geliyor, karar bağlantının `file://`'siyle tek
fonksiyonda, `shell::is_local_authority`; başka adlı host yabancı sayılır), dal aynanın kanalından
(`8133;b`, `precmd`'de bir `git rev-parse` fork'u). İkisi de aynanın
**yanında** yaşıyor (`DockContext`), içinde değil: ayna tuş başına gelip
`line-finish`'te sıfırlanıyor, bağlam prompt başına gelip komut koşarken de
duruyor. Taşmada yol **soldan** kısalır (`…` önekiyle), dal asla kısalmaz;
karar `bt-core`'da, çizen taraf yalnız hücreleri alır. Dock payı ızgaranın satırlarından
düşülüyor ve **yalnız entegrasyonlu zsh oturumunda** ayrılıyor — ayrım oturum
doğarken kararlaşıyor, yani `/bin/sh` koşan duman reçetesi dock almıyor.
**Alternatif ekranda dock kalkıyor** (vim, htop, `less`) — **uzak oturumda
hariç**: orada tek satırlık durum çubuğu (`⇄ host`, aktarım satırı) kalıyor,
payı bir satır ve `band_px(0) == dock_px(1)` olduğu için uygulamanın ızgarası
ötelenmiyor (`dock_rows_for`; uzakta vim'in nerede koştuğu görünür kalsın).
`frame()` bayrağı
`Term` kilidi altındayken yayınlıyor (`Session::alt_screen`), kare yolu onu her
karede karşılaştırıyor ve değişince `bt-shell-macos`'a enjekte edilmiş haberciyi
çağırıyor; resize **çizilen karenin içinde değil**, `dispatch2` ana kuyruğunun
bir sonraki turunda koşuyor. Bedel komut başına değil **geçiş başına**: `git
log` gibi alternatif ekrana girmeyen komutlar hiç resize görmüyor. Dock'u
olmayan pencerede haberci **hiç kurulmuyor**, yani yol yapısal olarak kapalı ve
alternatif ekrandan çıkış orada dock doğurmuyor.
**ssh'ta dock bir durum çubuğuna iniyor** (036): ön planda etkileşimli bir
ssh ya da mosh varken giriş satırı yok (`Cursor::input_rows == 0` — yazılan
satır uzak kabuğun, ızgarada), bant yalnız bağlam satırı ve o satır
`⇄ host  /uzak/yol` — `⇄` (fonttan, sıradan hücre) ile host **işaretinin**
renginde, yol bugünkü iki kademede, dal yok; host kısalmaz, sığmazsa yalnız
`⇄` kalır. Dock'un üst saç çizgisi de işaretin renginde (`Dock::edge`;
yükleme sürerken çubuğun boş izi, aşağıda).
**İşaret** (037 Karar 2–5): `[remote] hosts` sıralı `{ host, mark }` dizisi
(`production`/`staging`/`development`/`none`/`#rrggbb`; glob `*`/`?`, harf
duyarsız, desende `@` yoksa `user@` atılıyor, ilk eşleşen kazanır), renk
temanın rolünden (`error`/`warning`/`success`, işaretsizde `info`) ve çözüm
kenarda (`set_remote`, `Session::set_host_marks`) — kare yolu desen
görmüyor. Shell ▸ Mark “{host}” as ▸ dosyaya tek girdi yazıyor
(`SettingsEdit::RemoteHostMark`); işaretli sekmenin başlığında işaretin
renginde bir nokta (`NSWindowTab.accessoryView`). **Kopan ssh** (037 Karar
8): bizim `D`'miz 255 taşıyorsa boş giriş satırında `⇄ host  Connection
lost · ⏎ reconnect` (`DockContext::reconnect`) ve ⏎ hedefin satırını
yeniden koşturuyor; mosh'ta ve dock'suz pencerede yok. **Finder damlası
uzak dizine yükleniyor** (037 Karar 7 → Kullanıcı kararı): önce uzakta tek
bir yoklama (`df`, `tar`'ın varlığı, aynı adlı öğe; `upload::probe`), sonra
onay sayfası (Upload/Replace/Merge, klasörde dosya sayısı ve boyut, yer
yetmiyorsa düğme kapalı), sonra **sıralı** bir kuyruk: ara arşivsiz `tar c |
ssh … tar x` akışı, baytlar bizden geçtiği için ilerleme kesin
(`upload::TarWatcher`). **Kuyruk iki yönlü** (045): `Transfers` yön
(`↑`/`↓`) ve şerit taşıyor — sıralı kuyruk (yükleme + sağ tık indirmesi),
önizleme ve Finder'a bırakma; son ikisi sıra beklemiyor, çünkü kullanıcı
onları o an bekliyor. İndirme `ssh … tar c | tar x`, yerelde gizli geçici ad
+ `rename` (iptal ve hata geçiciyi siler) ve karantina etiketi; metinler yönü
okur (`↑1 ↓2`, "Stop downloading?", biten indirmede Show in Finder). Durum satırı bağlam satırının yerini alıyor
(`DockContext::transfer`, `Session::set_transfer`; `⇄ host`'un öneki ve
rengi korunuyor; sağa yaslı, fiil etiketli düğmeler — tek öğede `Cancel
⌘.`, fazlasında `Show transfers (N)` ve `Cancel all ⌘.`, sığmazsa
önce ipucu sonra liste düşer; dolgu ve çerçeve işaretin renginde,
`caret_fragment`'ten, fare üstündeyken koyulaşıyor ve el imleci — kare
yalnız düğme değişince; tıklama alanı dolgunun tamamı ve çizimle fareye tek
yerleşimden, `bt_core::transfer_button_at`, 037 phase-6), üst saç çizgisi bütün
kuyruğun baytlarına göre dolan bir çubuk (`Dock::progress`; dolan kısım her
zaman `info`, boş iz işaretli host'ta işaretin renginde, işaretsizde ayracın —
`Dock::track`, çünkü prod'da kırmızı dolan çubuk hata gibi okunuyordu) ve
uygulamanın Dock simgesinde de bir çubuk. **Hiçbir yol kendiliğinden
yapıştırılmıyor** (037 phase-7): yükleme dakikalar sürebilir ve yol o an açık
olan vim'e ya da mysql'e yazılırdı; sonuç satırı nereye gittiğini söylüyor
(`✓ backup.tar.gz → /var/www/app`, `✓ 3 files → …`, hedefler farklıysa
`✓ 2 files uploaded` — `success`; `Cancelled — …, partial file removed` —
`dim`; `Failed — {sebep} · k of n uploaded`, sebep `error`'da;
`Transfer::tone`/`lead`). `Show transfers (N)` bir `NSPopover` açıyor (N biten
dahil bütün kalemler; `transient`, Esc'i bir yerel olay izleyicisi yutuyor,
düğmeye yeniden basış `popoverWillClose:`'un olay zamanıyla ayırt ediliyor):
kalem başına ad, çubuk ya da `Waiting`/`✓ Uploaded`, hedef ve `Cancel`/`Remove`
(kimlikle, sırayla değil); ilerleme yerinde tazeleniyor, yapı değişince
yeniden kuruluyor. Akan kalem `upload::STOP_ASK_AFTER`'dan (30 sn, tasarım
sabiti) uzun sürüyorsa ⌘., `Cancel`/`Cancel all` önce "Stop uploading?"
soruyor (`Keep uploading` varsayılan ve Esc; sayfa açıkken yükleme sürüyor ve
kalem biterse sayfa kapanıyor); `Remove` hiç sormuyor. İptal yazılmakta
olan dosyayı uzakta siliyor; disk dolunca ve ssh kapanınca da kuyruk bitiyor
ve sonuç satırı `upload::LINGER` kadar kalıyor — durma koşulu o. Akarken
pencere ve sekme başlığı `↑ N% · {başlık}` (`upload::titled`, yüzde başına
bir yazım) — alternatif ekranda dock yok ve ⌘. menü kısayolu olduğu için
orada da çalışıyor; bateri arkadayken biten, hata veren ya da bağlantısı
kopan kuyruk bir bildirim gönderiyor (`NSUserNotification`, kullanımdan
kalkmış ama yeni crate istemeyen tek yol; paketsiz süreçte çağrılmıyor). ssh `BatchMode=yes` ile koşuyor: parola sorulamaz,
anahtar/agent ya da açık bir ControlMaster gerekiyor ve yoksa sayfa bunu
açıkça söylüyor. Uzak komut `sh -c`'ye sarılı ve tırnağı ters bölüsüz
(giriş kabuğu fish olabilir); adında ters bölü ya da kontrol karakteri olan
öğe reddediliyor. Uzak yol **OSC
7'nin yetkisinden**: uzak oturum sürerken her OSC 7, değilken yabancı
yetkili olanı uzak yuvaya gidiyor ve yerel dizine yazmıyor. Uzak durum
`bt-core`'da (`Session::set_remote(nesil, host)`, `DockContext::remote`) ve
`C`/`D`/`A`'da kendiliğinden siliniyor; algılama `bt-shell-macos`'ta, aşağıda.
**Bağlam satırının sağında uzak makinenin yükü** (046): CPU (sparkline),
bellek ve dolunca disk, yardımcı oturuma pane başına jetonlu bir ana kuyruk
zamanlayıcısının `bt_load` isteğiyle (`stats`; karar saf
`remote_stats::Schedule`'da, yerleşim merdiveni `bt-core`'da) — örnekleme uzak
oturum, görünür pane ve son iki dakikada etkileşim ister, kare yalnız
gösterilen değer değişince (`Session::set_remote_stats`'ın nesil + eşitlik
kapısı); göstergeye tık yerinde tazelenen bir `NSPopover` açıyor (host + OS,
CPU, load, bellek, swap, disk, uptime, ilk üç süreç; `stats_popover`, yükleme
listesinin emsali — açıkken örnekler ayrıntılı, Esc kabuğa gitmez, gösterge
kaybolunca kapanır) ve üstünde el imleci; tık, çıpa ve el imleci çizimin
yerleşiminden (`Session::stats_span`); gerekçeler
`.tasks/046-uzak-yuk-gostergesi/discussion.md` → Karar 1–8.
Dock'a tık giriş satırı yokken no-op. Gerekçeler
`.tasks/036-ssh-uzak-oturum/discussion.md` → Karar 3–8 ve
`.tasks/037-ssh-ikinci-tur/discussion.md`.
**Dock sütun sayıyor** (024): giriş satırının sarması, caret'in yeri ve
geniş karakterin iki hücresi karakter indeksinden değil **genişlikten**
birikiyor ve satır sonunda geniş glyph yarılanmıyor — sığmayan karakter alt
satıra iniyor, arkasında boş bir sütun kalıyor (ızgaranın
`LEADING_WIDE_CHAR_SPACER` kuralı; tek yürüyüş `dock::layout_with`, dock
parametrizasyonu `dock::dock_layout`).
`region_highlight`'ın aralıkları **karakter** indeksinde kalıyor, çünkü
ZLE'nin birimi o; yayılan şey boyanan **zemin** ve onu baş hücrenin `wide`'ı
ile spacer sütununa düşen glyph'siz bir hücre taşıyor. **Bağlam satırı
karakter biriminde** ve gerekçesi küçük boy sınıfı (021'in emsali), yani
CJK'lı bir yol orada hâlâ sütun kaydırıyor — bilinen sınır, bekçili.
Bastırmanın tazelik kapısı da aynı birime geçti: ayna tarafı **kümeyle**
yürüyor ve son mürekkebi son kümenin taban karakteri (035; sıfır
genişlikli kod noktası ve kümenin kalanı ızgara hücresine girmiyor,
`CellExtra`), yoksa kapı kalıcı olarak "bayat" kalırdı — `❤️` yazan satır her tuşta ızgaraya
fırlıyordu.

Giriş satırı ızgarada **çizilmiyor**: kabuk `Input` safhasındayken ve ayna
canlıyken (`ShellLog::suppressed_input`; karar `Term` kilidinden **önce**
okunuyor, `Theme` örüntüsü) yazılmakta olan bloğun çıpa satırından imlecin
satırına kadar hücreler sink'e uğramıyor — caret dock'ta. Aralığın imlecin
üstünde ve altında kaç satır tuttuğu **tek düzen yürüyüşünden**
(`dock::layout`, ızgara parametrizasyonu `dock::grid_span`): satır sonu ve
geniş karakter ızgaradaki gibi yürünüyor, sütun bölmesiyle sayılmıyor, çünkü
dock'un çizimi de aynı fonksiyondan okuyacak ve iki aritmetik ayrıştığı gün
biri gizlenir öteki görünür (032).
Kapı çıpa taramasından **sonra**, yoksa blok şeridi de ölürdü. Ayna
gösteremiyorsa (`Unavailable`), ZLE satırı bırakmışsa (`Idle`) ya da ayna
**bayatsa** bastırma **yok**: gösteremediğimiz satır ızgarada kalmak zorunda.
Çok satırlı giriş bastırılıyor ve **bütün** satırları: imlecin altındakiler
de (`BUFFER`'ın imleçten sonraki satırları, sarmalarıyla), `PREBUFFER`
doluysa üst taban **çıpanın satırı** (`SuppressedInput::from_anchor`;
`PS2`'nin genişliği aynada yok, bağlantı `preexec`'e kadar açık). İmleç bir
`\n`'in arkasındayken ilk satırın başı gözlenemiyor ve `dock::TEXT_COL`
varsayılıyor (bastırma yalnız dayatılan `PS1`'le koşuyor). **`PS2`
satırları arasındaki `line-finish` tutuluyor** (`ShellLog::end_since`,
`HANDOVER_HOLD` kadar; `u`, bir 133 işareti ya da süre bitiriyor, süreyi
kare yolu çözüyor ve saate giriyor): zsh her kabulde `e` basıyor ve tutma
olmasaydı her ⏎'de bant bir kare küçülüp kabul edilen satır ızgarada
belirirdi. Bedeli `CORRECT`'in `[nyae]` sorusunun 150 ms geç görünmesi.
032'ye kadar satır sonlu görüntü `Multiline` ile ızgarada kalıyordu; dock
satır kırmayı ve büyümeyi öğrenince kol kalktı (PTY'nin boyu değişmiyor,
yani "nefes alan ekran" gerekçesi konusuz) — `.tasks/032-cok-satirli-dock/`.
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
ızgaranın son giriş satırının mürekkebi ile aynanın **son satırınınki**
(`DockState::last_ink`; sondaki satır sonunun açtığı boş satırda ikisi de
`None`); yanlış alarmın
yönü güvenli, satırı iki yerde gösterir ama sessizce kaybetmez. Zamansal
sorunun **üç bilinen sınırı** var ve üçü de adıyla yazılı
(`.tasks/025-tazelik-zamansal/discussion.md` → Karar 2): damga aynanın ne
zaman geldiğini söylüyor, hangi girdiye cevap olduğunu değil (bir tuşun
aynası yoldayken giden yapıştırma bir tuş boyunca "cevaplanmış" görünür), ve
kabuğun dışından gelen yazım (arka plan işinin satıra bastığı çıktı) nesli
oynatmıyor, düzenleme boyunca bastırılan aralıkta gizli kalıyor; zsh'in
redisplay'siz tuttuğu tuş (`^X` öneki, vi'de çıplak `Esc`) ise nesli
ilerletip ayna doğurmuyor ve o süre kapı içeriğe düşüyor. **Dock'un
çizmediği kontrol karakteri** (sekme ve satır sonu hariç, `PREBUFFER` dahil)
satırı `DockStatus::Control`'e indiriyor — gösteremediğimiz satır ızgarada,
okunur `^A` ile. Bu kol gelmeden önce karar kapının tesadüfüne
kalıyordu ve `^A` satırın ortasındaysa satır dock'a gidip kayboluyordu. **Aynanın hiç
karakteri yoksa** (`SuppressedInput::blank`: ne görüntüde ne `PREBUFFER`'da;
tek bir `\n` de imleci iter) **o karşılaştırma vakuma düşüyor** (iki taraf da `None`) ve
ayıran ikinci veri çıpanın satırı (`session::anchor_row_at_or_above`):
karakteri olmayan bir ayna imleci prompt'un satırından aşağı itemez, yani
imleç çıpanın satırında olmak zorunda. Boş prompt'ta öyle — `PS1`'in iki
boşluğu çıpayı taşıyor — yapıştırmadan sonra değil. Gerekçe ölçüldü: zsh
bracketed yapıştırmanın **son satır sonunu tamponda tutuyor**, yani ızgaranın
imleci boş bir satıra düşüyor ve `bracketed-paste-magic` (oh-my-zsh onu
kuruyor) aynayı bir tuş boyunca boş bıraktığı için iki boşluk birbirine
uyuyordu. Çıpa hiç bulunamazsa kapı **susuyor**: hücresiz bir prompt'ta
söyleyecek bir şey yok.
**Devrin tek yüklemi var** (`shell::caret_home` + dört ön koşul) ve **dört
tüketicisi**: hangi hücrelerin atlanacağı, imlecin çizilip çizilmeyeceği,
**doluluk sayısı** ve **dock'un caret'i**. Ayrı sorulduklarında ayrışıyorlardı ve belirti ölçüldü:
boş prompt'ta hiçbir hücre çıpayı taşımadığı için satır çizilmiyor ama
doluluğa **giriyordu**, ilk tuşta çıpa doğunca doluluk bir satır düşüyor ve
ızgaranın tamamı oynuyordu — satır gizliydi ama yer kaplıyordu. Tek yüklem
`display: none` veriyor. Caret'in sahibi satırın nerede çizildiğine uyuyor:
komut koşarken (`Running`), ayna gösterilemiyorken (`Unavailable`,
`Control`) ve ZLE satırı bırakmışken (`Input` +
`Idle`; `CORRECT`'in `[nyae]`'i, R3.3) ızgaranın;
**kalan her hâlde dock'un** — kabuğun henüz hiç konuşmadığı açılış, prompt
çizilirken ve iki komut arası (`Finished`, içinde bir `git` fork'u) dahil,
çünkü sıçrayan caret tam da o pencerelerde görülüyordu. Dört ön koşul:
pencerenin dock'u olacak (`SessionOptions::dock`; yoksa devralacak kimse yok
ve satır da imleç de ızgarada kalır), alternatif ekranda olmayacak (dock zaten
kalkıyor), bastırılan bir satır varsa tazelik kapısı geçilecek ve uzak
oturum olmayacak (036; `ShellLog::caret`'te, tutmadan **önce** — uzakta
devralan bir giriş satırı yok ve tutmanın içine düşen `set_remote` caret'i
bağlam satırına oturturdu).
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
**arızası** da tutmanın dışında (`Unavailable`, `Control`): gösteremediğimiz
satır ızgarada duruyor, caret'i de orada durmalı, ve arıza zaten bir sıçrama
üretmiyor. Cevap
**hesaplandığı yerden geçiyor**, ikinci kez türetilmiyor: `frame()` onu
`Cursor::caret_in_dock` ile veriyor, `Session::dock` argüman olarak alıyor.
Dock kendi başına sorduğunda ön koşulları bilmiyordu ve bayat aynada **iki
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
istisnayı **kapatıyor**. Satır sonlu yük sarılı kalıyor ama düzenleme kapısı
yapıştırmadan **önce** açıksa arkasına aynı yazımda `CSI 8133 ~ r BEL`
gidiyor (`Session::paste_refreshes`; widget `BUFFER`'a dokunmadan yalnız
aynalıyor): kuyruğun arkasındaki komut aynayı yapıştırmanın sonucuyla
bastırıyor ve satır bir tuş boyunca ızgarada kalmıyor (032 phase-5). Prompt artık
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
**Devam satırı da işaret almıyor** (032 phase-6): bağlantı `preexec`'e kadar
açık, yani çok satırlı ya da sarılan bir komutun bütün satırları çıpayı
taşıyor; üstteki satır — geçmiş dahil, ızgarada da bantta da — aynı kimliği
taşıyorsa satır komutun başı değil ve ne işaret ne sayaç oraya oturuyor
(`session::block_row_continues`). Komutun başı ekranda değilse işaret de yok.
Tek istisna son `CSI 2 J`'nin geçmişe ittiği satır (`Session::clear_boundary`):
Ctrl-L prompt'u aynı kimlikle yeniden basıyor ve o kalıntı komutun başı değil.
**İçerik pencerenin tabanına yaslanır**: `frame()` kaç satırın dolu olduğunu
sınırdan verir (`Cursor::content_rows`; alternatif ekranda ızgaranın tamamı),
`DisplayLink` onu `rows - content_rows` ile ötelemeye çevirir ve `encode_pass`
tek bir `set_viewport` ile ızgaranın bütün pipeline'larını birden kaydırır —
dört liste ve imleç aynı yerden. **Kaydırılmış pencerede de aynı kural**:
doluluk görünür satırlardan doğuyor, yani geçmişe bakarken de içerik tabana
yaslı kalıyor. 017 bir dönem burada `display_offset != 0 => rows` denedi ve
kullanıcı gördü: ızgaranın **boş** alt satırları doluluğa giriyor, öteleme
kapanıyor ve bütün içerik pencerenin tepesine sıçrıyordu — terminal aşağıdan
yukarı akar. Öteleme **yumuşak kayar**: `bt-gpu::motion`'ın ikinci animatörü
(`Slide`; üçüncüsü çentiğin süzülmesi) onu imleçle aynı stil ve aynı `settled()` kapısı altında sürer, imlecin
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
(pencere/font/punto) ötelemeyi ayrıca snap'ler — tekerleğin pürüzsüzlüğü
ötelemeden değil kaydırmanın kesrinden geliyor (aşağıda).
**Dolu ızgara da kayıyor** (`Motion::scroll_in`): ızgara dolunca doluluk
sabitleniyor, yeni satırlar içeriği hücrelerin içinde kaydırıyor ve ötelemenin
hedefi hiç oynamıyor — kayma ızgara dolana kadar vardı, sonra yoktu
(kullanıcı bildirdi). Kaç satırın geçmişe kaydığı sınırdan ayrı geçiyor
(`Cursor::scrolled`) ve ötelemenin **konumu** o kadar geri alınıp hedefine
yeniden süzülüyor; tavanı bir ekran. Tek karede bir ekran ya da fazlası
kaydıysa ayıran şey **süre**, kare sayısı değil (`Motion::screenful_run`,
`BURST_WINDOW` = kaymanın kendi süresi, `EASE_DURATION`): kısa bir patlama
(dolu ekranda `seq 1 200`, `ls -la`) PTY'den bir, iki ya da üç okumada gelip
rastgele parçalanıyor ve pencerenin içindeki ekran boyu kareler aynı
patlamanın devamı — konum tavana kırpılıyor, son ekran bir ekran aşağıdan
süzülüyor (dolu ekranda `seq 1 200` hiç kaymıyordu; kare sayısına bağlı
ölçütte de yalnız ilk `ls -la` süzülüyordu — kullanıcı iki kez bildirdi).
Ekran boyu kareler pencereyi aşarak sürerse çıktı **akıyor** ve kayma
bitiriliyor (ölçüldü: `BT_SCROLL_TEST`'te öteleme tavanda asılı kalıp en
yeni çıktıyı bir ekran geriden gösteriyordu). Akış **hatırlanıyor**,
kaymadan türetilmiyor: bitirme kaymayı yerleşik bırakıyor ve akışta her
ikinci kare patlamayı yeniden kurup bandı bir ekran uzatıyordu (ölçüldü,
`docs/OLCUMLER.md` 2026-09-30); koşuyu yalnız ekrandan az kaydıran bir içerik
karesi bitiriyor, yani sonraki patlama yine süzülüyor. Sayının ölçütü defterin boyu **değil**
ekran tepesindeki satırın kimliği (`session::row_identity`, hücre tamponunun
adresi): `history_size()` `scrollback`'te doyuyor ve ondan türeyen sayı on
bin satır sonra aynı kusuru geri getirirdi, satırın tamponu ise alacritty'nin
halkasında kaydırmayla yerinde kalıyor. Temizleme bayrağı kuruluyken sayı
sıfır — `CSI 2 J`'nin geçmişe ittiği ekran süzülerek gitmemeli. Tepede açılan
şeridi doldurma bandı kapatıyor: çizen taraf ızgaranın çizildiği yeri
bildiriyor (`Session::set_grid_top`) ve bant o kadar **uzuyor**
(`Session::slide_fill_rows`), dock'suz pencerede de, ama yalnız ızgara
doluyken; uzantı `fill_shown`'a yazılmıyor, yani tekerleğin sanal kaydırması
onu saymıyor.
Piksel aygıt ızgarasına
yuvarlanır (`Frame::set_origin_rows`): kaymanın durduğu kare ekranda kalıcı ve
kesirli bir piksel bütün metni bulanıklaştırırdı. Ötelemenin tek sahibi
kare yolu; fare eşlemesi onu `bt_gpu::Origin` ile **encode edilen** değerden
okur. **Üstte kalan boşluk artık boş değil**: `frame()` oraya defterin en yeni
satırlarını veriyor (`Cursor::fill = min(gap, temizlemeden beri gelen satır)`),
ama **ayrı bir sink'ten** ve satırları **fill-yerel** (`0..fill`; kesrin tepe
satırı varsa en üstte bir fazlası) — doluluğa girmiyor, yani
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
taraf **üçüncü bir `set_viewport`**: bandın orijini `origin_px - fill_px`
(`Frame::fill_origin_px`; `fill_px` kanalın boyu, kesrin tepe satırı dahil) ve o sayı **encode anında** türüyor, yani bant
ızgarayla **birlikte** kayıyor — push anında pişmiş bir konum hareket
karesinde (listeler korunur, yalnız öteleme değişir) bandı yerinde
dondururdu. Orijin kaymanın ortasında **negatife** iniyor ve bırakılıyor:
bandın pencereye sığmayan en eski satırlarını GPU tepeden kırpıyor
(ölçüldü, 017 phase-0). Listeleri dock örüntüsünde **ayrı** ve sayaçlardan
muaf (`hucre=`/`glif=`/`kural=` oynamıyor); **blok işareti de o listelerden**
(`Blocks::fill_slice`, `Frame::push_fill_block` → `fill_rules`): bant ikinci
bir yüzey ve ızgaradan türeyen her şeyi ayrıca kazanmak zorunda — hücreleri
017 phase-2'de almıştı, işareti almamıştı ve kullanıcı bunu gördü (tamamlama
listesi komut satırını geçmişe itiyor, bant satırı geri getiriyor ama
**işaretsiz**, kaydırınca aynı satır ızgaradan geçtiği için işaret geri
geliyor). Çıpa yeni bir kaynak değil, hücrenin kendi OSC 8 bağlantısı;
eksik olan **okuyan** döngüydü. Satırlar fill-yerel, yani işaret bandın kendi
`set_viewport`'unda. **Süre sayacı hâlâ bantta yok** ve bu bilinçli daraltma:
sayaç hücre üretiyor (`Counter`) ve çakışma ölçütünü (`last_col`) ikinci kez
kurmayı isterdi; işaret ise bir `RuleCell`. Encode sırası **ızgara →
doldurma → dock**, çünkü ızgaranın listeleri bandın içine hiç girmiyor ama
ötelemeden muaf olan caret girebiliyor, ve dock'un opak zemini en altta
kalmak zorunda.
**Kaydırma konumu göreli bir kesir taşıyor** (027): trackpad parmağı piksel
piksel izliyor, momentum AppKit'in olaylarıyla yavaşlıyor, jest bitince pencere
en yakın satıra süzülüyor ve klasik tekerleğin çentiği süzülüyor.
`Session`'ın tek yeni kaydırma durumu
`[0, 1)` satırlık bir kesir (`Cursor::scroll_frac`, ızgara o kadar
**aşağı**), tam satırın tek yetkilisi yine `scroll_locked` ve bant eşlemesi
dokunulmadan geçerli — mutlak bir konum `display_offset`'in dört dış
yazıcısını (dibe dönüş, Shift+PgUp, geçmişteyken gelen çıktı, resize) her
karede ezerdi. `scroll_wheel` olayı kesirli ve tam satır hâliyle birlikte
alıyor ve rota önce seçiliyor: kesir yalnız kaydırma kolunda, ok ve rapor tam
satırla; niyet (`ScrollIntent`: satır, doğrudan, çentik, yerleşme, jest başı)
çağıranın. `bt-shell-macos` onu olayın **jest fazından** sınıflıyor (`view::smooth_wheel`,
saf): fazlı olay (trackpad, Magic Mouse) doğrudan kesir, `Ended`/`Cancelled`
yerleşme, `Began`/`MayBegin` ve momentum başı jest başı; fazsız olay çentik ve
miktarı **tam satır** — bitişini söyleyen faz yok, kesirli hedef pencereyi
yarım satırda bırakırdı. `[motion] smooth_scroll`, Hareketi Azalt ve
`cursor_motion = "snap"` tek `bool`'a iniyor (`app::resolve_smooth_scroll`,
Hareketi Azalt'ın yolunda) ve `false` kolu bugünkü satır yolunun ta kendisi —
nicemleme kaynakta, `Motion`'da değil, çünkü geri alma yolu bayt bayt bugünkü
davranış olmalı. Çentiğin ve yerleşmenin payı bir **istek** olarak birikiyor
(`Session::take_scroll_glide`) ve `frame()` payı **argüman** olarak alıyor —
aynı kilit turu, uyandırma yok. Konumu dışarıdan sıfırlayan her yol (dibe
dönüş, sayfa, satır adımı, jest başı) bir **nesil** artırıyor ve eski neslin
payı düşüyor; istek ile nesil tek atomik kelimede, çünkü ikisi ayrılınca
aradaki alım bir çentiği kaybediyordu. Uçlarda kesir kalmıyor, tam satıra
yakın toplam tam satıra oturuyor (pay `f32`) ve hiçbir şeyi değiştirmeyen olay
kare istemiyor. Kesrin açtığı şeridi ekranın tepesinin **hemen üstündeki
satır** kapatıyor (`Cursor::top_row`, `Line(-(offset + fill) - 1)`): doldurma
kanalının en üst satırı (fill-yerel `0`, bant onun altında), ama sayısı
`fill`'e **karışmıyor** ve tek kapısı satırın defterde olması — bandın dock,
Ctrl-L ve ofset kapıları ona uygulanmıyor, yoksa kaydırılmış pencerede tepe
boş bir yarım satır olurdu. Tekerlek artık kaydırmıyorsa (alternatif ekran,
fare kipi) ya da tepenin üstünde satır kalmadıysa kare yolu kesri sıfırlıyor.
**Kesir orijine ekleniyor ama ayrı yuvarlanıyor** (`Frame::set_scroll_frac`):
ızgaranın viewport'u toplamı, ızgaradaki caret ise yalnız kesri alıyor — caret
ötelemenin kaymasından muaf, kesirden değil, ve kaysaydı bloğu harfinden
ayrılırdı; dock'taki caret kaydırmadan muaf. Doldurma **kanalının** boyu
`top_row + fill`, yani tepe satırı bandın viewport'unda onun üstünde çiziliyor
ve `Origin::fill_rows` onu da sayıyor — tepedeki yarım satır bant satırı gibi
seçilemiyor. **Çentiğin süzülmesi `bt-gpu::motion`'ın üçüncü animatörü**
(`Slide`'ın ikinci örneği, birimi teslim edilecek satır): istek kare başında
`advance`'ten **sonra** alınıyor (uykudan uyanan link'in kırpılmış `dt`'si
çentiğin yarısını tek karede götürmesin) ve kare başına pay `frame()`'in
argümanı; uçuştayken hasarsız kare de içerik karesi (`icerik=` sayıyor, talep
yine hareketin). Nesil değişimi süzülmeyi **düşürüyor** (dibe dönüş gidilmek
istenen yer), örtülme, `snap` ve Hareketi Azalt ise **teslim ederek**
bitiriyor — düşseydi pencere bir satırın ortasında dinlenirdi. Payı konumu
oynatmayan süzülme geçmişin ucuna çarpmıştır ve orada bitiyor
(`Motion::observe_scroll`), yoksa ulaşılamayan kalan boş kareler çizdirip ters
yöndeki çentiği yerdi.
Gerekçeler `.tasks/027-yumusak-kaydirma/discussion.md` → Muhakeme.
**Seçim içeriği vurgular, içerik yaratmaz**: vurgu temanın `selection`
rengiyle çizilen **satır koşusu** (`SelectionRuns`, `frame()`'in `&mut`
tamponu; `bt-gpu` onu zeminden sonra, caret ve glyph'lerden önce çiziyor) ve
koşular **yuvarlak köşeli tek parça** bir şekil: açıkta kalan köşe dışbükey,
bitişik satırın koşusunca örtülen köşe kare, basamakta içbükey dolgu — karar
saf bir fonksiyonda (`frame::selection_corners`) ve boş ara satır şekli
bölüyor. Yarıçap seçimin **kendi** oranı (`frame::SELECTION_RADIUS`, hücre
yüksekliğinin 0.22'si, tek hücrede kısa kenarın yarısına kırpılı) — caret'in
oranı metin bloğunu saran bir yüzeyde köşeyi görünmez kılıyordu; kullanıcının
`cursor_radius`'u ona dokunmuyor (031 Karar 10: anahtar imlecin). Koşu satırın **ilk çizilir seçili hücresinden
sonuncusuna** uzanıyor — aradaki boşluklar köprülü, çünkü pano onları zaten
kopyalıyor; boş kuyruk ve boş satır koşusuz, yani boş ekranda fareyi
sürüklemek hiçbir şey boyamıyor (031 Karar 4). Ölçüt "mürekkep" değil
**çizilirlik** — ters videolu bir boşluk (vim'in durum satırı, tmux çubuğu)
mürekkepsizdir ama görünürdür ve koşuyu uzatır; varsayılan zeminli boş hücre
görünmezdir ve uzatmaz. Seçili metin **kendi ön planıyla**, ters video
çözülmüş çiziliyor (Karar 3) ve seçim atlama kapısını, dolayısıyla doluluğu
oynatmıyor; odakta olmayan pencerede renk zemine doğru üçte bir soluyor —
iki renk sınırdan hazır, seçimi odağı bilen `bt-gpu` yapıyor (Karar 9). Doldurma bandı bu
kuralın tek istisnası değil **tersi**: satırları görünür ama **seçilemez**,
çünkü hepsi geçmişte, yani sınırın satır numaralarıyla temsil edilemiyorlar.
Fare bu yüzden orijinin üstünü **reddediyor** (`point_to_cell`, kanal
boyu `> 0`);
kırpma orayı 0. satıra yapıştırır ve vurguyu gözün gördüğü yerden başka bir
yerde başlatırdı — "yanlış seçilir" ile "seçilemez" arasında dürüst olan
ikincisi. Kırpma **kalkmıyor**, yanına geçiyor: doldurma yokken orası
gerçekten boş ve yukarıdan başlayan sürükleme ilk satırı seçime katmalı.
Tekerleğin işaretçisi reddin dışında, çünkü o bir seçim ucu değil rapora
giden koordinat — reddedilseydi band ekrandayken kaydırma büsbütün ölürdü.
Bağlantının hit testi de dışında (044): bandın satırı imzalı ekran satırı
olarak soruluyor (`LinkPoint::Screen`, negatif = bant) ve `bt-core`
`drawn_lines` ile kapılıyor — bağlantı seçilmiyor, açılıyor.
**Dock'un giriş satırı da seçiliyor** (031 phase-4) ve ızgaranın
görünüşüyle — aynı pipeline, renk ve yarıçap, görsel satır başına bir koşu
(`Session::dock`'un `runs` tamponu, dikey pencerenin satırı ve ekran sütunu;
032 sarmasıyla birden çok satır). Seçim `bt-core`'da,
aynanın **yanında** (`ShellLog::dock_selection`, `BUFFER`'ın karakter
indeksleri; içinde dursaydı `dock::diff` her sürükleme adımında yazım
efektlerini sıfırlardı) ve `BUFFER` değişince kalkıyor. Yalnız `BUFFER`
seçiliyor: `PREDISPLAY` başına, öneri sonuna iniyor. Kelime ızgaranın
kelimesi (`dock::selection_range`, alacritty'nin `Semantic`'inin tek boyutlu
kopyası; bekçisi iki yüzeyi aynı dizgide karşılaştırıyor), üçlü tık
**mantıksal satırı** (`\n`'ler arası, sarılmış görsel satırlarıyla; ızgaranın
ve macOS'un paragraf seçimi — tek satırda bütün `BUFFER`, ⌘A her zaman) ve
kopyası satır sonu **taşımıyor** — kabuğa geri yapıştırılan satır
çalışmasın. İsabet testi (satır, sütun) ve dock'un **tek düzen
yürüyüşünden** (`dock::dock_layout`; çizim, hayaletler, satır sayısı ve fare
aynı yürüyüşü okuyor) ve **son çizilen** pencereye karşı
(`Session::dock_window`: dikey pencerenin tepesi + `BUFFER` boyu; canlı ayna
o kareden beri başka bir `BUFFER`'a geçtiyse tık seçim kurmuyor). Fare kipi
dock'a hiç uygulanmıyor, hedef basışta kilitleniyor (`Gesture`, `Drag::SelectDock`)
ve sürükleme giriş bloğunun içine kırpılıyor. **Pencerede tek seçim**: birinde
başlamak ötekini temizliyor, girdi (`send_input`) ikisini de; ⌘C sahibin
metnini (`Session::selection_text`), ⌘A dock caret'in sahibiyken ve satırda
metin varken dock'u seçiyor (`frame()`'in yayınladığı `caret_in_dock`).
**Dock bir metin alanı gibi düzenleniyor** (031 phase-5, Karar 8): sürüklemesiz
tık caret'i tıklanan sınıra taşır (bırakmada, `Release::Dock` →
`Session::dock_click`; öneriye tık satırın sonuna), seçim varken ⌫/⌦ siler,
yazılan harf (`Session::type_text`) ve yapıştırma (`Session::paste`)
seçimin yerine geçer, ⌘X keser, ←/→ seçimi başına/sonuna daraltır, ⇧←/⇧→
seçimin hareketli ucunu oynatır ya da caret'ten başlatır (yalnız terminalde);
başka her tuş seçimi kaldırıp bugünkü yolundan gider. Shift+tık seçim yoksa
caret'ten başlıyor. Seçim ZLE'ye **hiç gitmiyor**: kabuğa eylem anında **tek
komut** gidiyor, `CSI 8133 ~ d;S;E;L BEL` (`[S,E)`'yi sil, caret `S`'e;
`S == E` yalnız taşır; `L` tutmazsa widget no-op) — dock'un kabuğa giden ilk
teli, biçimi betiğin tel başlığında. Metin tele girmiyor, silmeden sonra
olağan yoldan gidiyor, yani `self-insert`, `can_be_typed` ve yazım efektleri
aynen. Komut `send_input`'tan geçiyor (nesil ilerler, iki seçim kalkar) ve
yalnız **düzenleme kapısı** açıkken (`Session::can_edit_dock`, dört koşul:
dock satırın sahibi, ekleme keymap'i, ayna güncel nesle cevap, kabuk bu
prompt'ta `8133;w` dedi — `ShellLog::dock_editable`, `line-finish` ve `A`
siler); kapalıyken hiçbir komut gitmiyor (`vicmd`'de dizinin baytları komut
olurdu, bağlamasız kabukta BEL `send-break`), seçim yalnız kopyalanabilir.
**Geçmişte arama çekirdeği `bt-core`'da** (033): `Session::set_search`
sorguyu derler (düz metin meta
karakterleri kaçırılarak, `Aa` kapalıyken alacritty'nin akıllı kipi, açıkken
`(?-i)`; geçersiz desen panik değil `SearchStatus::Invalid`) ve arama
etkinken `frame()` yalnız **çizilen** satırların eşleşmelerini `SearchRuns`
olarak verir — ızgara ve fill-yerel kanal (bant + kesrin tepe satırı) ayrı
listelerde, eşleşme satır başına bir koşu (`continues`), tarama sarılmış
satırın mantıksal başına uzanıyor (`search::WRAP_REACH`). Bastırılan giriş
satırına değen ve mürekkepsiz (yalnız boşluk) eşleşme dışarıda: ilki
bastırmanın **tek yükleminden** (`suppressed_rows`, atlanan hücreler ve seçim
de ondan), ikincisi "vurgu içerik yaratmaz". Derlenmiş desen **ödünç**:
yaprak yuvadan (`SearchSlot`) `Term` kilidinden önce alınıp nesil
değişmediyse geri konur, `Term` altında kilit alınmaz
(`.tasks/033-gecmiste-arama/discussion.md` → Muhakeme). **Sayım çıpasız bir
dizin** (`search::SearchIndex`, `Session::search_step`): dipten yukarı
parça parça, sorgu ya da defter değişince baştan, eşleşmeyi yalnız son
satırı parçadayken sayarak — `row_identity` doymuş defterde çıpa olamıyor.
Defter haberi uçuştaki geçişi **kesmiyor**, bitince bir geçiş daha
başlatıyor (kesen kural `yes` akarken sayımı hiç bitirmezdi). Geçerli
eşleşme yalnız kesin kaynaklarla içeriğine yapışıyor, kalanında
**kayboluyor** ve son geçişte en yakına dönüyor (`search::ledger_shift`):
yanlış satırı geçerli göstermektense hiçbirini. Ayrıntı
`.tasks/033-gecmiste-arama/phase-5.md` → Uygulama Notları.
Dock ve komutlar arası atlama henüz yok (`docs/YOL-HARITASI.md` → komut işaretleri üstünde gezinme). `make bundle` `bateri.app` paketini
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
(`kCTFontTraitColorGlyphs`), aile adından değil. Tek sütunlu emoji (iki
sütunu yok, mürekkebi 1.66 hücre) küçültülerek çiziliyor (041).
**Emoji dizileri tek glyph ve tek geniş hücre** (035): bayrak (`🇹🇷`), ZWJ
(`👨‍👩‍👧`), ten rengi (`👍🏽`) ve VS16 (`❤️`) ızgarada **bir** geniş hücrede
kümeleniyor — taban karakter hücrenin kendisi, kalanı `zerowidth` — ve
sütun sayısı `UnicodeWidthStr::width`'ten. Kural tek saf fonksiyon
(`cluster`, dört emoji kolu; ızgara yalnız 1 → 2 genişletir, Arapça `لا`,
VS15 ve eşlenmemiş RI bugünkü gibi) ve **tek yürüyüş** (`cluster::Walk`):
ızgara, dock düzeni, bastırmanın ızgara yürüyüşü ve tazelik kapısı aynı
kümeyi görüyor, yoksa iki aritmetik ayrışırdı. Kümeleme `Term::input`'un
önünde, `Handler`'ı aktaran sarmalayıcıda (`handler::ClusterHandler`;
araya giren her başka çağrı kümeyi kapatıyor) — alacritty kümeyi hiç
kurmuyor, okuyucu döngünün `bt-core`'a geçmesinin sebebi bu. Sınır
`Cell`'i küme **indeksi** taşıyor (`Cell::cluster`, dizgiler karenin
`Clusters` tablosunda — hücre başına dizgi kare başına ayırma olurdu),
`bt-atlas` diziyi `CTLine` ile tek glyph'e şekillendiriyor
(`Sprite::Cluster`, atlasın kendi interner'ı; aynı mürekkep kapısı, aynı
renk düzlemi) ve tek glyph'e şekillenmeyen ya da kapıdan dönen dizi
**taban karakteriyle** çiziliyor. Üç yüzey ve yazım efektleri kümeyi
çiziyor; dock'un seçimi, `d;S;E`'si ve farkı küme sınırına hizalı.
**Basılı tuş kümeyi bölmüyor**: satırda bir emoji kümesi varken ⌫/⌦/←/→
widget komutuyla gidiyor ve ayna yoldayken düzenleme kapısı son komutun
**beklenen** sonucuna bakıyor (`shell::DockPrediction`) — tekrar aynadan
hızlı gelebiliyor, kapalı kapıdan ZLE'ye giden tuş `🇹🇷`'nin yalnız
`🇷`'sini silerdi; yanlış tahmin sonraki komutun `L`'sini tutturmuyor, yani
bedeli kayıp bir tekrar, bölünmüş bir küme değil. Seçenek
(`SessionOptions::cluster`) açık ve ayar anahtarı değil. **İki ürün bedeli**
adıyla: wcwidth sayan uygulamalar (vim, less, eski tmux) bu dizilerde
kayıyor — kayma modern terminallerin tarafına alındı; ⌘F kümenin ikinci
kod noktasını görmüyor (`🇹🇷` taban karakteriyle bulunur). Gerekçeler
`.tasks/035-grapheme-dizileri/discussion.md` → Karar. **Blok elemanları, Braille,
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
yuva paylaşıyor. Kapı **küçük sınıfta da açık**, küçük yüzün kendi
hücresiyle (`Atlas::small_metrics`, genişliği bağlam satırının sütun adımı):
sprite ayrı bir tampona o ölçüyle çiziliyor ve büyük yuvaya **taban çizgisi
hizalı** taşınıyor — büyük hücrenin genişliğinde çizilseydi bağlam satırında
komşu hücreler örtüşürdü, fonttan alınsaydı sparkline döşemezdi
(`.tasks/046-uzak-yuk-gostergesi/discussion.md` → Karar 3). Gölgeler
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
make check        # rustc sürümü + eski .o temizliği (prune) + fmt --check + audit + clippy -D warnings + test (definition of done)
make fmt          # cargo fmt --all -- --check
make audit        # kuralların mekanik yarısı: katman yönü, bt-core'da gerekçesiz panik, rc dosyasına yazma; Cargo.lock değiştiyse uyarır
make clippy       # cargo clippy --workspace --all-targets -- -D warnings
make test         # cargo test --workspace --all-targets (doc-test yok, rustdoc'un boş turu koşmaz)
make shader       # WGSL kanaryası: cargo test -p bt-gpu wgsl_pipelines_build (naga doğrulaması + bütün pipeline'ların kurulumu)
make smoke        # uygulamayı BT_RUN_SECONDS=3 ile açar ve jeton satırı basar:
                  # frames=N cells=K glyphs=G rules=R slots=U/T slots2=U/T load=smoke requests=I content=C motion=M slide=S quiet=Sms teardown=clean profile=debug samples=off pipeline=ok
                  # ilk dördünden ya da motion'dan biri 0 ise, content > IDLE_FRAME_LIMIT ise, quiet < QUIET_FLOOR ya da quiet=none ise
                  # ya da deadline'da animasyon yerleşmemişse kırmızı. iki sınır da ölçülmüş; değerleri ve türetmeleri sabitlerin doc'unda.
                  # süreli koşuda pencere kayan seviyede açılır: wgpu örtülü pencereye drawable vermez, kapı öndeki uygulamaya bağlı kalmasın (`TerminalWindow::float_for_timed_run`).
                  # üst sınır frames'te değil content'te: content çizilmeye karar verilen kare, frames GPU'nun bitirdiği — animasyon ikincisini meşru olarak şişirir.
                  # quiet'in kuralı ters (sağlıklıda büyük) ve kapının en duyarlı katı: content sınırının göremediği yavaş sızıntıyı o görüyor.
                  # slots/slots2/load/requests/slide/profile sayaç ve etiket; teardown kısmen kapı (değerler teardown_token'da); samples=off'ta ölçüm jetonu basılmaz.
                  # slots atlasın maske düzlemi, slots2 renk düzlemi (023): ikisi aynı yuva ızgarasını paylaşıyor, ayrı sayaçları var ve toplamları aynı.
                  # motion ile slide iki ayrı animatörün tanığı (imleç / içeriğin ötelemesi): aynı karede ikisi birden artabilir, toplamları kare değildir.
make terminfo     # assets/terminfo'yu tic -x ile geçici dizine derler
make test-race    # yarış stresi: race_* (--ignored) + tek thread karşılaştırma koşusu
make scan         # yedek glyph kapısının envanteri (041): sembol/emoji bloklarını 13/16pt × @1x/@2x kapıdan geçirir; make check'de yok (kurulu fontlara bağlı), BT_SCAN_FONT taban aileyi değiştirir
make linux        # bt-core'un, bt-atlas'ın, bt-gpu'nun ve bt-shell-common'ın Linux kapısı: Docker'da clippy -D warnings + test --locked (bt-gpu Vulkan'da, piksel sınamaları lavapipe'ta; bt-shell-common'da gerçek PTY ve zsh); yerel rustc ≠ imaj etiketi kırmızı, Docker yoksa SKIPPED
make bundle       # release derler, target/release/bateri.app'i kurar (Sparkle dahil; ilk koşuda sha256'lı indirir), anahtarlıktaki Developer ID ya da Apple Development kimliğiyle hardened runtime'la, yoksa ad-hoc imzalar (SIGN_ID ile ezilir) ve içeriğini denetler (Info.plist, URL şeması, ikon, lisans, shell betiği, Sparkle ve imzaları)
make package      # bundle + target/release/bateri-<sürüm>.zip (Sparkle'ın) ve bateri.dmg (ilk kurulumun, sürümsüz ad; dmgbuild, düzen assets/dmg/); Developer ID'de ikisi de notarize + staple (NOTARY_PROFILE=bateri-notary)
make release      # kapı (temiz ağaç, v<sürüm> etiketi yok, CHANGELOG.md'de ## [<sürüm>] — not oradan kesilir) + package + tek öğeli imzalı appcast → target/release/v<sürüm>/; derlenen commit'i kaydeder, hiçbir şey yayınlamaz
make publish      # o commit origin/main'deyse v<sürüm> diye etiketler, push eder ve GitHub release'ini (bateri.dmg, zip, appcast.xml, notlar) --latest açar
make ship         # main'de: release + git push origin main + publish — sürüm yayınlamanın tek komutu
make install      # bundle + bu Mac'e kurar: /Applications/bateri.app (INSTALL_DIR ile değişir); açık bateri varken durur
```

Girdisi henüz olmayan hedefler "not yet" deyip kırmızı düşer; hangileri
olduğu `.claude/is-akisi/proje.md` → Doğrulama'da.

Tek crate / tek sınama:

```sh
cargo test -p bt-core -- osc::tests
```

## Katman düzeni

Katmanlar tek yönlüdür; **hiçbir bağımlılık yukarı doğru gitmez**:

```
bateri (bin) → bt-shell-macos → bt-shell-common → bt-gpu → {bt-atlas, bt-core}
                   └────────────────┴──────────────────────→ bt-core
```

`bt-shell-linux` (winit) doğduğunda `bt-shell-macos`'un yanına, aynı kata
girer: iki platform kabuğu da `bt-shell-common`'a bağlanır, ortak crate
ikisine de bağlanmaz.

| crate | sorumluluk | görebildiği platform kütüphanesi |
|---|---|---|
| `bt-core` | VT durum makinesi, grid ve scrollback, PTY ve **okuyucu döngünün sahibi** (alacritty 0.26.0 döngüsünün kopyası, `reader`; `Term` `Handler`'ı aktaran sarmalayıcının arkasında, `handler` — emoji dizisini ızgarada orada kümeliyor, 035), PTY okuma yolu **taranıyor** (araya giren sarmalayıcı baytları aynen geçirir, geçerken **üç** OSC numarasını ve **bir** CSI dizisini çeker), OSC (0/2/7/8/9/52; 0/2 uygulamanın başlığını `Term` kilidi altındaki olaydan yaprak bir yuvaya indirir ve pencere başlığı ondan kurulur — öncelik OSC 0/2 → dizinin son bileşeni (ev `~`) → `bateri`, `Session::title`; uzak oturumda (036) `⇄ {OSC başlığı}`, yoksa `⇄ {host}`; başlık ya da **değişen** OSC 7 dizini `Wake::title_changed` ile yüksüz haber verir, 7 çalışma dizinini **yetkisiyle** verir (yerel yetki dock'un bağlam satırına, uzak oturumda ya da yabancı yetkide uzak yuvaya) (`Session::working_directory` onu okur), 52'nin yazma yönü `Wake` ile kabuğa çıkar, panoyu görmez), komut blokları, seçim, geçmişte arama (sorgunun derlenmesi, görünür satırların eşleşmeleri, bütün defterin parça parça sayımı; `search`), girdi kodlaması (DECCKM'e uyan oklar, farenin düğme/hareket/tekerlek raporu; kipten karar veren tablolar `input::button_route`/`motion_route`/`wheel_route`), ayar modeli, shell bağlamı. Tarayıcının üç kolu var ve üçü de alacritty'de **yok** (`vte` üçünü de `unhandled`'a düşürüyor): OSC 133 oturumun safhasını ve blok kimliklerini `ShellState`'e yazar (`Session::shell_state()`) ve `Running`'e her **geçişte** bir komut nesli artırıp `Wake::command_started` ile yüksüz haber verir (036, uzak oturum yoklamasının tetiği); kimliğimizi (`bt_block=`) bir kez görmüş bir oturumda **uzak oturumu yalnız bizim işaretimiz bitiriyor** — uzak oturum etkinken kimliksiz `A`/`B`/`C`/`D` yok sayılıyor, çünkü ssh'ın öbür ucundaki fish 4 ya da kitty/iTerm2 entegrasyonu aynı PTY'ye 133 basıyor ve uzak `A` göstergeyi silip uzak `C` yeni bir nesil açardı; yoklamadan önce gelen uzak `A` için komut bizim `D`'mize kadar açık sayılıyor (`ShellLog::command_open`). Kapı `Running`'e değil uzak oturuma bağlı, yoksa `exec fish` `Running`'i hiç bitirmez ve saat boşta kare isterdi (`.tasks/036-ssh-uzak-oturum/phase-3.md` → Uygulama Notları), OSC 8133 ZLE'nin görüntü aynasını — `PREDISPLAY`, `BUFFER`, `POSTDISPLAY`, `region_highlight`, `CURSOR`, base64 gövdelerle; `KEYMAP` ve `PREBUFFER` sondaki isteğe bağlı gövdeler, eski betik onlarsız da çözülüyor — çözüp `DockState`'e (`Session::dock_state()`), dalı `DockContext`'e ve düzenleme widget'ının yeteneğini (`8133;w`) `ShellLog::dock_editable`'a, OSC 7 de çalışma dizinini yine `DockContext`'e (yüzde çözme ve yabancı host elenmesi orada; bozuk URI panik değil yoksayma). Aynanın kendi yük sınırı var ve aşımı **görünür** (`DockStatus::Unavailable`), sessizce düşmez; dock'un çizmediği kontrol karakteri de görünür bir durum (`DockStatus::Control`) ve satırı ızgaraya bırakıyor. **Dördüncü kol OSC değil CSI** ve yükü yok: `CSI 2 J`'yi tanıyıp "ekran kasten temizlendi" bayrağını kurar (`Session::observe_screen_clear`; `3J` ve RIS için kol **yok**, ikisi de geçmişi siliyor — geçmişi silen tek yol terminal tarafı temizlik, ⌘K/⌥⌘K). **Sayacın iki yazarı var** (`screen_clears`): tarayıcı `2J`'yi baytlar uygulanmadan **önce** sayıyor, terminal tarafı temizlik (034) `Term` kilidi altında ve uygulandıktan **sonra** adlı tek yöntemden (`Session::note_screen_clear`) — ikisi de yalnız artırıyor ve tüketici tek. **Alternatif ekranda kurmaz** — orada `ClearMode::All` `reset_region(..)` çağırıyor, geçmiş büyümüyor ve birincil ekranın durumuna dokunulmuyor, yani geri getirilmeyecek bir şey yok; nesil yine de **tüketilir**, yoksa `vim`'den çıkışta birikmiş sayaç bayrağı kurar ve doldurma ilk `vim`'den sonra kalıcı olarak kapanırdı. Bayrak **defter temizlemeden sonra büyüyünce** düşer: geçmişe temizlemeden sonra satır düşmüş demektir ve doldurma o kadarını güvenle geri verebilir. Ölçüt bir damga ve tek karşılaştırma (`Session::screen_clear_history`); damga bayrak kurulduktan **sonraki** ilk karede alınıyor, çünkü kuran kare ızgarayı henüz temizlenmemiş görebiliyor ve temizlemenin kendisi satırları geçmişe itiyor — bayat damga anında aşılırdı. Üstünde iki koşul var — alternatif ekranda değil ve `display_offset == 0`; ikincisi olmasa geçmişe kaydırılan pencere dolu **görünür** ve tek bir tekerlek jesti Ctrl-L'i geri alırdı. (Bu koşul **bayrağın ömrüne** ait; doldurmanın kendi `display_offset` kapısı ayrı bir şey ve ayrı gerekçeli.) **Bayrak bir kapı, damga bir ölçü:** kapı "hiç" der, aynı damga doldurmada ikinci kez okunup `fill`'i temizlemeden beri gelen satır sayısına **kırpar** — yoksa tek satırlık bir büyüme bayrağı düşürür ve doldurma boşluğun tamamını, yani kullanıcının sildiği ekranı geri getirirdi (ölçüldü). `content_rows == rows` kolu yok: dock'lu pencerede doluluk giriş satırını saymadığı için erişilemez. **Bilinen sınır**, defter `scrollback`'te doyunca damganın üstüne çıkacak sayı kalmıyor ve o oturumda bir Ctrl-L'den sonra doldurma koşmuyor; yönü güvenli. Yarışı kapatan şey bir **nesil sayacı**: tarayıcı baytları uygulamadan **önce** sayıyor, kare yolu sayacı `Term` kilidinin **altında** doluluk sayısıyla aynı okumada tüketiyor, ve henüz hesaba katılmamış bir nesil aynı karede doldurma kuralını ezer. Bayrağın tek tüketicisi doldurmanın kapısı (`Session::fill_rows`) ve sıra zorunlu: ömür **önce** işliyor. Komut blokları `frame()` sınırından **çözülmüş** geçer (komutun satırı + renk, çıkış kodu değil; bölge değil işaret): kimlik prompt'un OSC 8 çıpasından `Term` kilidi altında toplanır, renk kilit bırakıldıktan sonra kabuk defterinden çözülür. Giriş satırının **bastırılması** da burada: safha ile aynanın durumu tek yüklemde birleşiyor (`ShellLog::suppressed_input`) ve kopya `Term` kilidinden **önce** alınıyor — yaprak kilit `Term`'ün altına girmez | macOS'a özgü **hiçbiri** — `objc2*`, `core-text`, `metal` yok. Unix PTY (`libc`, `rustix`, `polling`) serbest; kapı Linux hedefiyle derlemedir — `make linux` (Docker, `tools/linux/Dockerfile`), çünkü platformsuzluğu yalnız Linux'ta gerçekten derlemek kanıtlar |
| `bt-atlas` | glyph rasterizasyonu, atlas paketleme, **emoji dizisinin şekillendirilmesi** (`Sprite::Cluster`, tek glyph — macOS'ta `CTLine`, Linux'ta `harfrust`; atlasın interner'ı, şekillenmeyen dizi taban karakteriyle), **iki düzlem** (maske `R8`, renk `RGBA8`; ayrı sayaç, ortak yuva ızgarası), **geniş glyph'in iki yarısı** (`Half`; kutu iki hücre, yuva yine bir hücre), **sistemin cascade'inden yedek glyph** (kapı geometrik ve **sıralı**: önce tek hücre, sonra iki; ikisine de sığmayan aday kutu kalır), **yordamsal karakterler** (blok elemanları, Braille ve çizgi çizim — köşegenler hariç; fonta sorulmadan, yüzden bağımsız, iki boy sınıfında da kendi hücresiyle), font seti. **Doku kenarı sabit değil**: hedeflenen **yuva sayısından** türüyor (`SLOT_TARGET` = 1024 yuva; kenarın kendisi `MIN_EDGE` = 1024 px ile `MAX_EDGE` = 4096 px arasında, iki 1024 tesadüfen aynı sayı), çünkü hücre büyüdükçe kapasite düşüyor ve bir yerde yordamsal ailenin altına iniyordu — ölçülen kırılma Retina'da 29pt'ti (406 yuva, ailenin istediği 429: 421 karakter + tofu + kural payı). Varsayılan punto tabanda kalıyor, yani ızgara ve raster bit bit aynı. Tahliye **yok**: dolan atlas hâlâ tofu'ya düşüyor ve kalan senaryo (tek karede hedeften fazla farklı glyph) ölçülmedi | `objc2-core-text`, `objc2-core-graphics` ve ortak tabanları `objc2-core-foundation` — yalnız macOS hedefinde ve yalnız CoreText arka ucunda (`coretext.rs`; kural yarısı `FontSystem` trait'inin arkasında platformsuz, `make audit` bekçisi — 042). `objc2` çekirdeğini bile **görmez**: kullanılan her şey C API'si, ObjC runtime'ı değil. Linux hedefinde `freetype-rs` + `fontconfig` (+ ham `yeslogic-fontconfig-sys`), pkg-config ile dinamik ve yalnız FreeType arka ucunda (`freetype.rs`: maske, `CBDT`/`COLR` renk düzlemi, `harfrust` ile küme). Arka ucun ölçülmüş örnek karakterleri (`fixture`) `fixture` özelliğiyle dışarı açık — yalnız `bt-gpu`'nun sınamaları için, API değil |
| `bt-gpu` | wgpu renderer, shader'lar (`.wgsl`), **geniş glyph'in yelpazelenmesi** (`prepare`; karar `Atlas::slot`'ta doğduğu için sink'te değil), kare döngüsü ve `Waker` (`DisplayLink`: platformsuz `tick`; ritim dışarıdan, dört görevli `Pacer` dikişiyle — vsync tik'i, her thread'den `set_running`, tek gecikmeli uyandırma, zaman tabanı), kare yolunun **ölçüm defteri** (`Stats`: iki CPU aralığı, GPU deltası, açılış damgası, p95'in tabanı — biriktirir, **basmaz**), hareket (motion), **dock yüzeyi** (ikinci `set_viewport`, kendi listeleri ve caret'i; PTY payı `DOCK_ROWS`, çizilen bant `Cursor::input_rows` giriş satırı + bağlam satırı), **doldurma bandı** (üçüncü `set_viewport`, kendi listeleri; orijini ötelemeden türüyor, kaç satır olduğu `Cursor::fill`), overlay'ler (palet), durum çubuğu | `wgpu` (arka uç hedefe göre sabit: macOS'ta Metal, Linux'ta Vulkan — `make linux` piksel sınamalarını lavapipe'ta koşar; doğrudan bağımlılığında ve kaynağında platform kütüphanesi yok: pencerenin katmanı tek `unsafe` girişle — `Surface::from_layer`, yalnız macOS; Linux'unki pencere setiyle — ve ritim `Pacer` olarak `bt-shell-macos`'tan geliyor; `make audit` doğrudan bağımlılıkta ve kaynakta `objc2`/`dispatch2`/`block2`/`metal` arar, wgpu'nun dolaylı çektikleri konusu değil) |
| `bt-shell-macos` | AppKit kabuğu: pencere, sekme, bölme, menü, klavye (metin yolu AppKit'in yığınından: `BateriView` `NSTextInputClient`, ölü tuş bileşimi orada tamamlanır), **Finder damlası** (`NSDraggingDestination`, yalnız dosya URL'si; yol `quote::shell_quote`'tan geçip `Session::paste`'e gider), servisler, ayar penceresi, **terminal pane'i** (`pane::TerminalPane`, `NSView` alt sınıfı: oturumun çekirdeği, pane düzeyindeki menü seçicileri; sahiple sınırı `PaneLaunch` + `PaneHost`, 039), **bölmeler** (kapsayıcı `split_view`, saf ağacı `bt-shell-common`'ın `split`'i; 039), **arama paneli** ve sayım dizininin ana kuyruk sürücüsü (`search_bar`, `TerminalPane::kick_search`), **uzak oturumun algılanması** (036: `C` kenarında ön plan grubunun en üstteki ssh/mosh süreci ve argv'sinden hedefi, `jobs::remote`; kararsızsa sonraki çıktıda yeniden, ana kuyrukta en çok bir iş — `pane::RemoteProbe`; host yazıldığı gibi, etkileşimsiz ssh uzak sayılmıyor), **bağlantılar** (044: ⌘-hover, ⌘-tık, yol doğrulamasının arka plan kuyruğu, UTType sınıfı, açma ve onay sayfası — `hyperlink`; kural ve politika `bt-shell-common`'ın `links`'inde), **uzak aktarım** (037 yükleme, 045 indirme ve önizleme: kural, metin ve `ssh`/`tar` süreçleri `bt-shell-common`'ın `upload`/`download`/`remote_files`/`remote_helper`/`preview_cache`'inde; sayfa, kuyruk sürücüsü, popover, durdurma sorusu, başlık öneki, bildirim ve Dock simgesi `uploader`'da, önizlemenin açılması `preview`'da, Finder'a sürükleme `promise`'te; kuyruk pane'in, Dock simgesi pane'lerin toplamı); kapanış sırasının ve duman bekçisinin sahibi; sistemin dil/bölge çifti (`locale`, `NSLocale`; kararı `child::locale_env`), entegrasyonun kurulup kurulmayacağı ve `ZDOTDIR`/`BATERI_ZDOTDIR` çifti (`app::shell_integration_env`), **güncelleme** (`updater`: paketteki `Sparkle.framework`'ü çalışma zamanında `NSBundle`'dan yükler ve "Check for Updates…"ın hedefi olur; framework link'lenmiyor, yani paketsiz ve süreli koşu Sparkle'sız) | `objc2`, `objc2-foundation` (`NSLocale` dahil: kabuğun yereli; `NSUUID`: sekme kimliği), `objc2-app-kit`, `objc2-quartz-core` (pane'in `CAMetalLayer`'ı — wgpu yüzeyi ondan açılıyor, ölçeği pane'in — ve macOS `Pacer`'ı: `NSView.displayLink` yalnız zamanlayıcı olarak ve `CACurrentMediaTime`; `pacer`), `objc2-user-notifications` (yüklemenin bildirimi, `UNUserNotificationCenter`; paketsiz süreçte çağrılmıyor), `block2` (kapatma sorusu sayfasının tamamlanma bloğu), `dispatch2` (ana kuyruk: `Pacer`'ın `set_running`'i ve gecikmeli uyandırması; `child_exit` → o pane'in kapanışı, süreli koşuda `terminate:`; OSC 52'nin pano işi; arama sayımının parçaları; uzak oturum yoklaması; ayar izlemenin bildirimini ana kuyruğa taşıma — kaynaklar `bt-shell-common`'ın kendi seri kuyruğunda), `libc` (bekçinin `write` + `_exit`'i) |
| `bt-shell-common` | kabuk katmanının AppKit görmeyen yarısı (043): ayar dosyasının okunması ve tanısı (`settings`, `notices`), bölmelerin saf ağacı (`split`), geçici punto (`zoom`), fare jest defteri (`gesture`), kabuk kaçışı (`quote`), tuş kodlaması (`keys`), uzak yüklemenin kuralı, metni ve `ssh`/`tar` süreçleri (`upload`), süreç tablosu (`jobs`: kapatma sorusunun ön plan işi ve uzak oturumun hedefi), kabuğun doğuşu (`child`: başlangıç dizini, yerel kararı, hangi kabuk, sarmalayıcı betiğin yeri) ve dosya izleme (`watch`). Sistem hizmetinin gövdesi `cfg(target_os)` arkasında ve adlı (`jobs::Libproc`/`jobs::Procfs`, çağıranın adı `jobs::SystemTable`; `watch`); kabuk komutu ebeveyniyle tek dönüşten (`child::shell_command`: macOS `login -qflp` + `Login`, Linux `$SHELL -l` + `Direct`); Linux gövdelerinin kapısı `make linux`; sınama yardımcıları `test-support` özelliğinin arkasında | AppKit, Foundation, Quartz ve bildirim merkezi **yok**; `libc` (passwd kaydı için `getpwuid_r`, macOS'ta `proc_*` ve `sysctl(KERN_PROCARGS2)`, Linux'ta `/proc` — `jobs` —, yükleme iptalinin `kill`'i, izlemenin `O_EVTONLY`'si ve Linux'ta inotify/`eventfd`/`poll`'u) ve yalnız macOS hedefinde `dispatch2` (`watch`'ın vnode kaynakları); `make audit` doğrudan bağımlılıkta AppKit ailesini, `block2`'yi ve platform kabuklarını, kaynakta `objc2`/`dispatch2`/`block2`'yi `watch`'ın macOS gövdesinin (`watch/dispatch.rs`) dışında arar |
| `bateri` | `main`, app bundle | — |

`bt-core`'un platformsuzluğu bir zevk değil kapıdır: Metalterm'in yol haritasında
"1.0'dan sonra Vulkan" var ve o kapı bu ayrımın üstüne kurulur.

## Bilinmesi gerekenler

- **Taban macOS 14, tek kaynağı `.cargo/config.toml`'daki
  `MACOSX_DEPLOYMENT_TARGET`.** rustc binary'nin minos'unu oradan alır; `make bundle`
  `LSMinimumSystemVersion`'ı binary'nin `minos`'undan, yani dolaylı olarak yine
  oradan doldurur. Metalterm'in tabanıyla aynı. Xcode'un `metal` derleyicisi
  derleme şartı değil: shader'lar WGSL ve `include_str!` ile gömülü.
- **Bağımlılık mimari karardır**, kendiliğinden eklenmez. Taban:
  `alacritty_terminal` (VT ayrıştırma, grid ve PTY; okuyucu döngü 035'ten
  beri onun 0.26.0 döngüsünün `bt-core`'daki kopyası, `reader.rs` — sürüm
  `=` ile sabit; kendi ayrıştırıcımızı yazmıyoruz — `bt-core` onu **kapsüller**, `pub` API'de
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
  Notları). `cursor-icon` de aynı emsal: grafta 1.2.0 (vte'nin `ansi`'si),
  `Handler`'ı aktaran sarmalayıcı `set_mouse_cursor_icon`'un tipini
  adlandırmak zorunda — ne vte ne alacritty onu yeniden ihraç ediyor ve
  aktarımın `missing_trait_methods` bekçisi metodu atlamaya izin vermiyor
  (`.tasks/035-grapheme-dizileri/discussion.md` → Karar).
  `wgpu` (30, `std`/`wgsl`/`metal`/`vulkan`; renderer'ın Metal'den ölçümlü
  geçişi) **`bt-gpu`'nun tek GPU bağımlılığı**
  (`.tasks/040-linux-kapisi-ve-wgpu/discussion.md` → Karar 3, 7, 10).
  **Linux font yığını** yalnız `bt-atlas`'ta ve yalnız Linux hedefinde:
  `freetype-rs` 0.38, `fontconfig` 0.11 (+ altındaki ham
  `yeslogic-fontconfig-sys`, grafta zaten olan), `harfrust` 0.13 (`icu`
  kapalı); sistem kütüphanelerine pkg-config ile **dinamik** (`bundled` ve
  `dlopen` kapalı, sessiz C derlemesi yok). macOS ürün grafı değişmedi.
  Kullanıcı onayı 2026-09-30, geçişli crate'lerle birlikte
  (`.tasks/042-font-sistemi-linux/phase-4.md`, `discussion.md` → Karar 5).
  **Sparkle 2** bir crate değil, pakete gömülen bir framework (`make bundle`
  sürümü ve sha256'sı `Makefile`'da sabit indirir, depoya girmez) ve
  `bt-shell-macos` onu link'lemeden, çalışma zamanında yüklüyor
  (`bt-shell-macos::updater`). Besleme GitHub'ın
  `releases/latest/download/appcast.xml`'i — her zaman en yeni release'in
  tek öğeli beslemesi, yani sürüm yayınlamak güncellemeyi yayınlamak ve site
  sürüm tutmuyor (indirme düğmesi aynı yolun `bateri.dmg`'si); açık
  EdDSA anahtarı `Info.plist.in`'de, gizli anahtar kullanıcının
  anahtarlığında (`generate_keys`); paket kimliği `dev.bateri.bateri` ve
  güncellemeler ona bağlı, değişmez.
  `objc2-user-notifications` (yalnız `bt-shell-macos`, aynı 0.3 nesli, varsayılan
  set kırpık — `objc2-core-location` grafa girmiyor) yüklemenin macOS
  bildirimini taşıyor, çünkü `NSUserNotification` macOS 11'den beri
  kullanımdan kalkmış; izin ilk bildirimde isteniyor ve paketsiz süreçte
  merkez hiç çağrılmıyor (kullanıcı onayı 2026-09-27,
  `.tasks/037-ssh-ikinci-tur/phase-7.md` → Uygulama Notları). `Cargo.lock`
  depodadır.
  **bateri'nin kendi lisansı GPL-3.0-or-later** (`Cargo.toml`; metni kökteki
  `LICENSE`, gnu.org'un metni, `make bundle` onu pakete kopyalayıp `cmp`'liyor,
  bekçisi `bundle_assets::own_license_ships_with_notice`): ürün grafındaki
  her lisans (Apache-2.0, MIT, Zlib, Unlicense) v3'le uyumlu ve yeni
  bağımlılık da öyle olmak zorunda — **GPL-2.0-only bir crate giremez**.
  `alacritty_terminal` **Apache-2.0**: lisans metni
  `assets/bundle/THIRD-PARTY-LICENSES.txt` ile pakete girer, atfı
  `Credits.html`'de durur. Geri kalan crate'ler MIT seçeneği taşıyorsa MIT'le,
  taşımıyorsa kendi metinleriyle (izin listesi `OWN_LICENSES`: Apache-2.0 —
  varsa NOTICE'ıyla —, Zlib, ISC; wgpu'nun getirdiği `codespan-reporting`,
  `foldhash`, `libloading` kullanıcı kararıyla, çünkü üçü de GPL-3 uyumlu —
  `.tasks/040-linux-kapisi-ve-wgpu/phase-5.md`) ve Sparkle kendi LICENSE'ıyla
  aynı dosyada; dosya elle değil `tools/third_party_notices.py` ile ürün
  grafından (`cargo tree`) üretiliyor ve **ürün grafı değişince yeniden
  üretilir** — ne MIT seçeneği ne izin listesinde lisansı olan bir crate
  betiği durdurur. Betik macOS ürün grafında
  (`aarch64-apple-darwin`) koşuyor; Linux grafının (FreeType/fontconfig
  crate'leri) bildirim dosyası paketleme setinin borcu.
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
- **Renk uzayı sınırı geçer.** Çizim hedefi `Bgra8UnormSrgb` (`renderer::FORMAT`): donanım
  fragment çıktısını **lineer** sayar ve yazarken sRGB'ye kodlar. Bu yüzden
  `bt-core` sınırdan lineer float verir (`color::linear_rgba`) ve pass'in clear rengi
  de aynı temadan (`Theme::background_linear`) beslenir. İkisi **birlikte** değişir; biri lineerleşmeden
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
  callback'in kendi kararı, kimseyi uyandırmaz; çentiğin süzülmesi de bu yol,
  ama payı pencereyi `frame()`'in içinde kaydırdığı için karesi içerik karesi
  olarak çiziliyor) ve **saat** (link uyumaya
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
  ayar ile sistemin cevabı `bt-shell-macos`'ta tek `bool`'a iniyor, `bt-gpu` AppKit
  görmüyor. `cursor_motion = "snap"` bunun üstündedir: hareketi zaten kapatmış
  olana erişilebilirlik ayarı animasyon *eklemez*.
- **Kapanış sınırlı bekler, çocuk yine de ölmeyebilir.**
  `Session::shutdown()` `SIGHUP`'tan sonra `join`'i ve `Pty`'nin düşmesini ayrı
  bir thread'e alır ve en çok `SHUTDOWN_GRACE` (yarım saniye) bekler; sinyali yutan ya da
  çıkışın içinde takılan çocuk (`ps` durumu `?Es`) kapanışı asamaz. İki
  yarıdan kurulu: `begin_shutdown()` başlatır ve beklemez, dönen
  `ShutdownHandle::wait_until(son tarih)` bekler — tutamak beklenmeden düşse
  de kapanış biter, yalnız sonucu kimse okumaz. Tek
  istisna kapanış thread'inin kurulamamasıdır, o dalda sınır yoktur. Süre
  dolunca çocuk arkada bırakılır ve süreç çıkışı master fd'yi kapatınca gider.
  Kalıcı çare "süre → `SIGKILL`" **değil** (ölçüm çürüttü, o çocuk `SIGKILL`
  almıyor); çare `wait` bloklarken master'ı boşaltmak, yolu
  `Session::spawn`'da `pty.file().try_clone()` — `EventLoop` `Pty`'yi
  `join`'den sonra vermediği için kopya baştan alınmak zorunda. Sonuç
  `Teardown` olarak döner ve süreli koşu onu `kapanis=` jetonuyla basar
  (değerler `teardown_token`'da). Duman bekçisi (`_exit(70)`) kapanış yolunun
  başka asılmalarına karşı durur. **Bir sekmenin ya da pane'in kapanışı beklemez**
  (her pane'de `TerminalPane::begin_close`: yükleme kuyruğunu bırakır, ritmi keser,
  `Waker`'ı ana thread'de `ShellWake`'ten söker, kapanışı başlatır ve
  tutamağı düşürür); ⌘Q koşan
  iş varsa önce sorar (`applicationShouldTerminate:`), sonra bütün
  pane'lerin oturumlarını başlatıp **tek** son tarihe kadar paralel bekler; süreli koşuda
  kabuğun çıkışı doğrudan `terminate:` — rapor pencereyi listede bulmalı
  (026 → Karar 5, 9).
- **Render yolu bloklanmaz.** PTY okuma ve ayrıştırma kendi thread'inde; AppKit
  çağrıları `MainThreadMarker` ile ana thread'de; renderer platformun `Pacer`'ıyla
  sürülür (macOS: `NSView.displayLink` zamanlayıcı olarak, `bt-shell-macos::pacer`;
  drawable'ı wgpu yüzeyinden yalnız çizen tik alır).
- **PTY ve ayrıştırma yolunda panik yok.** Bilinmeyen dizi yoksayılır, loglanır
  (`make audit` `bt-core`'da gerekçesiz `unwrap`/`expect`/`panic!` arar).
  Loglama yarısı **henüz borç**: `tracing` bağlanmadı, yoksayılan olaylar ve
  alacritty'nin `log` satırları sessizce düşüyor; logger gelince bu cümle kalkar.
- **`tty::setup_env()` çağrılmaz**: *kendi* sürecimizin ortamını değiştirir ve
  makinede alacritty kuruluysa `TERM=alacritty` yazar. Çocuğun ortamı
  `tty::Options.env` ile verilir: `TERM=xterm-256color`, `COLORTERM=truecolor`
  — `SessionOptions.env`'in ek ortamı ikisini **ezemez**. Aynı katmanda
  kimlik (038, `bt_core::identity`): `TERM_PROGRAM=bateri`,
  `TERM_PROGRAM_VERSION` (workspace sürümü) ve sekme başına
  `TERM_SESSION_ID=<UUID>` + `BATERI_TAB_URL=bateri://tab/<UUID>`; kimliği
  pencere doğarken `NSUUID` üretir, biçimi `bt-core`'un (`TabId`). Gerekçe:
  miras kalan `TERM_PROGRAM=Apple_Terminal` `/etc/zshrc` üzerinden
  sarmalayıcının dizinine yazdırıyordu
  (`.tasks/038-terminal-kimligi/context.md` → Kanıt). Alacritty
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
  `bt-shell-common`'da (`child`; `NSLocale` okuması `bt-shell-macos`'ta), `bt-core` yalnız geçirir
  (`SessionOptions.working_directory`, `.env`). Sebep Dock'tan açılış:
  LaunchServices süreci `cwd=/` ile başlatıyor ve launchd'nin ortamında `LANG` yok.
- **Kabuğu doğuran komutu da biz kuruyoruz** (`child::shell_command`, macOS kolu `login_command`) ve
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
  Bugün dokuzu da tüketiliyor — `background`, `foreground`, `dim` (SGR 2'li
  varsayılan ön plan), `accent` (koşan komut bloğunun şeridi), `cursor` (imleç
  bloğu, ANSI 258'in cevabı **ve** dock'un `heat` efektinin kızgın rengi),
  `success` ve `error` (biten bloğun şeridi), `info` (ssh'ta dock'un `⇄ host`'u
  ve üst saç çizgisi, 036; `accent` değil, çünkü ssh de koşan bir komut ve
  aynı renk iki anlam taşırdı) ve `warning` (Staging işaretli host'un rengi,
  037 — `production` `error`'u, `development` `success`'i ödünç alıyor)
  — ve yanlarında `selection` (fareyle seçimin vurgusu, modelin dışında;
  031), `search_match`/`search_current` (geçmişte aramanın vurgusu, yine
  modelin dışında; ölçütü seçiminki — zeminde 3:1'i geçen metin vurguda da
  geçer, bekçisi `color::tests`; odaksız pencerede aynı kuralla soluyor, 033)
  ile `[ansi]`'nin 16 rengi. Çizilmeyen rol eklenmiyor. `cursor` **014'te
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
  anahtar silinmez. Dosyaya yazan dört yol var: ayar penceresinin Open
  settings.toml düğmesi yalnız dosya **yokken** şablonu yaratır
  (`settings::create_if_missing`), View ▸ Theme ▸ yalnız `[appearance]
  theme`'i, Shell ▸ Mark “{host}” as ▸ yalnız `[remote] hosts`'un o host'un
  girdisini (`SettingsEdit::RemoteHostMark`; kural `settings::host_mark_plan`,
  037 Karar 5), **ayar penceresi** (bateri ▸ Settings…, `settings_window`) yalnız
  değiştirilen anahtarı yazar — dördü de tek düzenlemeden, biçimi koruyarak
  (`Settings::with_edit`, tipli `SettingsEdit`), yerinde (sembolik bağın
  hedefine); ayrıştırılamayan dosyaya yazmaz. Menü ve pencere yalnız yazar,
  uygulayan dosyayı okuyan yol; pencere kendi durumunu tutmaz, her okumada
  dosyanın hâlinden (`settings::FileState`: kilit, satır tanısı; alt
  başlıkla aynı metin) ve yazma yuvasından tazelenir, çünkü tek kaynak dosya
  (`.tasks/029-ayarlar-penceresi/discussion.md` → Karar 7).
  Anahtarlar, varsayılanlar ve hata davranışı (pencere alt başlığı)
  `docs/AYARLAR.md`'de; ayrıştırma ve fark (`Settings::changes`)
  `bt-core::settings`'te saf, okuma ve izleme `bt-shell-common`'da. İzleme kaynağı
  okumadan **önce** kurulur ve her olayda yeniden kurulur; kayıt anında
  kullanılamayan dosya hiçbir şeyi, kabul edilmeyen değer kendi anahtarını
  değiştirmez (`Settings::parse_keeping`). **Tek istisna `osc52`:**
  kabul edilmeyen değeri ve açılışta kullanılamayan dosya (ya da
  çözülemeyen ev dizini) panoyu **kapalıya** düşürür
  (`Settings::for_unusable_file`) — yanlış tahmini görünmeyen tek anahtar. Süreli koşu
  (`BT_RUN_SECONDS`) dosyayı **hiç okumaz ve izlemez**: dalın tek yeri
  `bt-shell-macos`'un `app::Inputs`'u.
- **Shell entegrasyonu bugün yalnız zsh'tir** (`ZDOTDIR`); bash (`--rcfile`) ve
  fish (`vendor_conf.d`) sonraki settedir. Kullanıcının rc dosyasına **asla**
  yazılmaz — kapısı `make audit` ve listesi zsh'in beş dosyasını da kapsar.
  Betik `assets/shell/` altında **kaynaktır**, üretilmez: `make bundle` onu
  pakete kopyalar ve kopyayı `cmp` ile denetler, `make check` de girdi
  dizininin envanterini (`bundle_assets`). Sarmalayıcı hiçbir kolda ölümcül
  değildir ve kullanıcının özgün `ZDOTDIR`'ını geri koyar; gerekçeler
  `assets/shell/zsh/bateri.zsh`'in başlığında. Komut durumu OSC 133
  işaretlerinden okunur ve `Session::shell_state()`'te durur; satıra
  çıpalanması prompt'un OSC 8 bağlantısıyla, yani blok kimliği hücrelerde
  taşınır. bash ve fish betikleri doğduğunda çıpa satırı onlara da yazılır —
  yoksa o kabuklarda blok yok, işaret de yok. Dock'lu kademede sarmalayıcı
  bir **düzenleme widget'ı** kuruyor (`__bateri_dock_edit`, `CSI 8133 ~`'e
  bağlı) ve bağlamayı `main`/`emacs`/`viins`'e **her `line-init`'te**
  yeniliyor, ardından yeteneği bildiriyor (`8133;w`): `bindkey -v`, keymap'i
  sıfırlayan bir eklenti ya da `bindkey -A mymap main` bir sonraki prompt'ta
  onarılıyor; bağlanan tek şey hiçbir klavyenin üretmediği bir dizi, yani
  kullanıcının bağlamalarına dokunulmuyor (031 Karar 1). Widget her komuttan
  sonra aynayı açıkça basıyor — terminal her girdisine bir cevap bekliyor.
  bash/fish betikleri doğduğunda karşılığı orada da yazılır. Sarmalayıcı bir de **`LISTMAX=0`**
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
  sınırları (**kapsam** / **açık kalem** etiketli) `bt-shell-macos`'ta `Measured`'ın
  doc'unda emanettir; o türün ilk `/measure`'ı onları `docs/OLCUMLER.md`'nin
  `## Yöntem`'ine taşır. **Bench seti borçtur:** `criterion` ayrı bir
  bağımlılık kararı; `cargo bench` satırı yukarıdaki komut bloğuna bench seti
  gelince döner. Giriş gecikmesi zinciri (`BT_INPUT_LATENCY_SAMPLES`) ve düşen
  kare sayımı da aynı durumda. Hangi iddianın hangi araca baktığı `/measure`
  skill'inin tablosunda, bekleyen iddialar `docs/OLCUMLER.md` → `## Bekleyen
  iddialar`'da — bir setin durumu ölçüm beklemez.
- **Dil:** **kod düzeyindeki her şey İngilizce** (2026-10-01'den beri,
  istisnasız): tanımlayıcılar, yorumlar, doc-comment'ler, tanı metni (stderr,
  `assert!` gerekçeleri), `Makefile` hedefleri ve mesajları, `.wgsl`, zsh
  betiği, `tools/`. UI dizgileri, ayar anahtarları, tema ve materyal adları da
  İngilizce. Türkçe kalan yalnız **test verisi** (Türkçe karakterli girdi
  sınanıyorsa) ve Türkçe belgelere işaretçiler (`.tasks/…` yolları, `→ Karar
  N`, bölüm adları) — işaret ettikleri belgeler Türkçe. Belgeler (`CLAUDE.md`,
  `docs/`, `.tasks/`, `.claude/`) ve commit iletileri Türkçe ve "neden"i
  anlatır. Jeton satırı bir **makine sözleşmesidir**: anahtar ve değer
  İngilizce, tanı metni satırın dışında (gerekçe `Report::token_line`'ın
  doc'unda). Depo geneli kural: **jeton silinmez, eklenir** — okuyan taraf
  tanımadığı jetonu atlayabilir, kaybolanı arayamaz. Anahtarlar 2026-10-01'de
  bir kez Türkçeden İngilizceye çevrildi (eski → yeni tablosu
  `docs/OLCUMLER.md`'nin başında). `SKIPPED` da aynı sözleşmenin parçası.

## İş akışı

Çok oturumlu ve tasarım kararı içeren işler `.claude/` altındaki zincirle
yürür: `/rfc → /plan-review → /implement → /ship`, sürücüsü `/akis`. Kurallar
`.claude/README.md` ve `.claude/is-akisi/`'de; iş setleri `.tasks/` altında.
Tek dosyalık düzeltme için set açılmaz; set yürürken çıkan tek commit'lik
düzeltme de phase açmaz. Her phase'in kapısı `make check`'dir, `/code-review`
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

**Bu dosya bugünkü sözleşmedir, tarihçe değil.** Yeni bir kural buraya kural +
tek cümle gerekçe + işaretçi olarak girer; ölçüm anlatısı, reddedilen
seçenekler ve bilinen sınır listeleri setin dosyalarında kalır (`duzen.md` →
Teslim → Her bilgi tek yerde). Dosya her oturumun başında baştan sona
okunuyor ve her set ona bir paragraf ekliyordu.

Hangi işin **neden o sırada** olduğu `docs/YOL-HARITASI.md`'dedir; henüz
açılmamış setlerin sırası ve bağımlılıkları oraya yazılır. **Durumu** o dosya
tutmaz — tek sahibi `.tasks/README.md` indeksidir, iki yerde durum tutmak
drift üretir.
