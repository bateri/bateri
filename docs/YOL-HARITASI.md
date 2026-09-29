# Yol haritası

Bu dosya **sırayı ve gerekçesini** tutar: hangi iş neden o sırada, neyin neye
dayandığı. **Durumu tutmaz** — o `.tasks/README.md`'nin işidir (`duzen.md` →
İndeks) ve iki yerde durum tutmak drift üretir. Buradaki bir satır açılmış bir
sete dönüşünce, ayrıntısı o setin `context.md`'sine taşınır ve burada tek
satıra iner.

**Tahmin değil sıra.** Takvim, süre ve efor tahmini bilerek yok: ölçülmemiş
sayı yazılmaz kuralı buraya da geçer (`CLAUDE.md` → Dil ve ölçüm). Sıra
değişebilir; değişince **gerekçesiyle** değişir.

**Açılmamış iş numara almaz** (2026-09-23). Numara `.tasks/`'ın sırasıdır ve
set açılınca `duzen.md`'nin listesinden gelir; burada önceden verilen numara
araya giren her setle kayıyordu ve on beş "kayma" notu doğurdu. Tablonun `#`
sütunu açılmamış satırda `—`; aşağıdaki kayma notları tarihli kayıttır,
yenisi yazılmaz.

Referans ürünün envanteri `docs/ARASTIRMA.md`'dedir. Orası Metalterm'in **ne
yaptığını** söyler, burası **bizim hangi sırayla yapacağımızı**.

## Günlük kullanım eşiği

006'dan önce proje bir pencereydi: içinde gerçek shell koşuyor ve metin
kalın/eğik/altı çizili/üstü çizili doğru çiziliyordu. Ama **kopyalanamıyor,
yapıştırılamıyor, seçilemiyor, kaydırılamıyordu.** Yani sınanabilirdi ama
kullanılamazdı.

Eşik şu: **bateri'yi kendi terminalim olarak açabildiğim gün.** Bu tarihin
kendisi bir kilometre taşıdır, çünkü ondan sonra hatalar sınamadan değil
**kullanımdan** gelmeye başlar — ve kullanımın bulduğu hatalar başka türlü
bulunamaz.

| # | İş | Neden burada |
|---|---|---|
| 005 | ölçüm kancaları | 002, 003 ve 004'ün bekleyen on iki iddiası tek bir kanca setine bağlı. Taban, **bir sonraki büyük render değişikliğinden önce** alınırsa "hangi set yavaşlattı" sorusu cevaplanabilir olur; sonra alınırsa o soru kalıcı olarak cevapsız kalır. `docs/OLCUMLER.md` bilerek **kapsam dışı** bırakıldı (onu ilk `/measure` kurar) ve bench (`criterion`) de öyle; **bench'in dışarıda kalması** on ikinin ikisini bu setten sonra da açık bırakıyor — ikisi de saf `cargo bench` iddiası |
| 006 | pano + seçim + kaydırma + bundle | **Eşiği tek hamlede geçmek için bilerek şişirilmiş set.** Cila feda edilir: yapıştır, kopyala, fareyle seçim, tekerlek, `.app` bundle. Bundle burada çünkü bundle'sız süreç öne çıkamıyor, Dock ikonu almıyor ve varsayılan terminal olamıyor. 002'nin ertelenmiş Apache-2.0 attribution'ı da burada kapanır. **Bundle'ın bir yan ödevi var:** görünür pencere meşru kare sayısını değiştirir, yani `IDLE_FRAME_LIMIT` (boşta sıfır kare kapısı) bu sette **yeniden ölçülmeli** — bugünkü değeri görünmeyen bir pencerede ölçüldü *(sonradan: 006 phase-5 yoklamasında bundle'sız `make duman` penceresi de ekranda ve öndeydi; 005'in penceresi ise yoklanmamıştı, yani iki gerekçe de ölçülmüş değildi — sonuç `docs/OLCUMLER.md` → `## Boşta kare`)* |

> **006'nın kapsamı — karar verildi (2026-09-12).** Tek hamlede eşik **(b)**
> tutuldu: panel üç koldan bundle'ın ayrı sete çıkmasını önerdi, kullanıcı
> reddetti — Dock ikonu ve öne çıkma olmadan günlük kullanıma geçilemez, yani
> "kullanımın bulduğu hatalar" argümanı bundle'sız işlemiyor; OSC 52 köprüsü
> ise panelin önerisiyle 007'ye ertelendi. Set sonra yine kullanıcı
> kararlarıyla büyüdü ve beş phase'lik plan dokuza çıktı: tam ekran uygulamada
> tekerlek (3b, Karar 4 eki), Dock açılışında ev dizini ve yerel (4b, Karar 6
> eki), yerel yedeği (4c, aynı ekin son maddesi), seçim ve klavye rötuşu (4d,
> Kapsam eki) — dördü de kalite kapısının (`/code-review`) bulgularından doğdu.
> Kayıt `.tasks/006-gunluk-kullanim-esigi/discussion.md`'de, durumu
> `.tasks/README.md`'de.

## Eşikten sonra

| # | İş | Neden bu sırada |
|---|---|---|
| 007 | ayarlar + sekiz rollü tema + font seçimi | Görünüşün temeli: 008 ayar dosyasına, 009 tema rollerine yaslanır. Set açıldı → `.tasks/007-ayarlar-ve-tema/context.md` |
| 008 | hareket altyapısı + imleç animasyonu | Metalterm'i ekranda tanıtan üç şeyden biri (renk, imleç, yüzey) ve shell entegrasyonuna **bağlı değil**. İlk tüketici imleç; boşta kare kapısının yavaş animasyon borcu da bu setin içinde kapanıyor. Set açıldı → `.tasks/008-hareket-ve-imlec/context.md`. *(Sonradan: borç phase-6'da **ölçüyle** kapandı — `sessiz ≥ QUIET_FLOOR` kapısı yavaş sızıntıyı görüyor, dağılımlar `docs/OLCUMLER.md` → `## Boşta kare`. **Kapanmayan yarısı:** kapı sızıntıyı ancak periyodu tabandan kısaysa görüyor, yani hareket saatini atlayıp saniyede bir kare isteyen bir kodu hiçbir sayı tutmuyor — onu yapısal kural (`bt-gpu::link` modül başlığı: zamana bağlı kare talebinin tek yolu hareket saatidir) ve `/audit` tutuyor.)* Yumuşak kaydırma ve çıktı gelince tamponun kayması aynı altyapının ikinci tüketicisi; sete sığmazsa hemen ardından |
| 009 | shell entegrasyonu (zsh) + OSC 133 komut durumu | Kullanıcının rc dosyasına **asla** dokunulmaz: zsh `ZDOTDIR` sarmalayıcısı; bash `--rcfile` ve fish `vendor_conf.d` sonraki sette. OSC 133 alacritty'de **yok** — `vte` onu `Handler`'a hiç vermeden düşürüyor, yani `bt-core` baytı `Pty`'yi saran bir dinleyiciden görüyor. **Sıra 2026-09-16'da öne alındı** (kullanıcı kararı: Input Dock'a hızlı varmak). Set açıldı → `.tasks/009-shell-entegrasyonu/context.md` |
| 010 | komut blokları | OSC 133 işaretlerinden okunur; `frame()` sınırına kanca ister; blok şeridi 008'in altyapısıyla gelir. **Kapandı** → `.tasks/010-komut-bloklari/`. *(Sonradan: "(+ blok animasyonları)" bu setten **çıktı** — Karar 5, aşağıdaki borç listesinde.)* |
| 011 | **tabana yapışık içerik** + yumuşak kayma | **Kullanıcı kararı (2026-09-17):** içerik tabandan tavana doğru büyümeli — ekran dolmadan önce de tabana yapışık dursun, yukarıda birikmesin. Bugün dolmamış ve dolmuş ekran **iki ayrı his**; bu set ikisini tek hisse indiriyor ve yeni satır geldiğinde kayma **animasyonlu** oluyor. **Saf yerleşim işi:** kabuk betiğine, OSC 133'e ve 010'a hiç dokunmuyor. **Bedeli üç kalem:** (a) alternatif ekran dışarıda kalmalı (vim/htop ızgaranın tamamını sahipleniyor); (b) dikey ofsetin **tek sahibi** olmalı — çizim orijini, fare eşlemesi ve imleç aynı değeri okur, üç kopya ayrışır; (c) kayma `bt-gpu::motion`'ın **ikinci tüketicisi**, yani durma koşulu ve Hareketi Azalt indirgemesi onun da sorusu. Set açıldı → `.tasks/011-tabana-yapisik-icerik/context.md`. *(Kapsam iki turda daraldı: Input Dock ve prompt'un devri **012**'ye ayrıldı — gerekçe o setin kaydında.)* |
| 012 | Input Dock + prompt'un devri | **Zincirin en ucu, kısayolu yok.** Pencere altında sabit ayrı satır editörü; zsh ZLE kancalarına, OSC 133'e ve komut bloklarına birden oturuyor. Tek tutarlı iş, çünkü dördü aynı cümlenin parçası: prompt'u kabuk değil terminal çizer (PS1/RPS1 sıfır görünür genişlik), `>` **o zaman** çizilebilir (bugünkü `Frame`'de temsil edilemiyor — pay glyph almıyor, `GlyphInstance`'ın boyu kare başına tek uniform), blok çıpası komut metnine taşınır (`anchor_close` → `preexec`; alacritty kaynağında doğrulandı, kimlik zaten `bt_block=` ile akışta) ve dock **ayna** olur — tuşlar yine PTY'ye gider, ZLE `BUFFER`'ı geri bildirir, yani Tab/geçmiş/Ctrl-R ZLE'de kalır. *(2026-09-17, 012 açılırken: tuş vuruşu ve silme animasyonları — `keypress`, `delete_mode` — setten **çıkarıldı**. "Bu harfi kullanıcı mı yazdı" sorusunun cevabı hâlâ dock'ta, ama aynanın gerekçesi onlara **bağlı değil**: ayrı bir yüzeyde giriş satırını çizmek zaten içeriğini bilmeyi gerektiriyor, yani ayna dock'un ön şartı. Animasyonlar aynanın üstüne kurulur ve kendi setine gider.)* **Ödenmemiş bedeli kayıtlı:** aynanın görsel dikişi — ZLE'nin `BUFFER` olmayan çıktısı (tamamlama listesi, `menu-select`, `bck-i-search`, `zle -M`) aynada yok, ızgaraya düşüyor; ve kullanıcının p10k/starship prompt'u **çizilmez**, geri dönüş bugün hep-ya-hiç (`shell.integration = "off"`). Hazırlığı 011'in ikinci turunda yapıldı → `.tasks/011-tabana-yapisik-icerik/discussion.md` (Karar 8, 10, 12 ve `## Muhakeme — 2. tur`). |
| 013 | komut süresi sayacı | 010'un blok defterinin üstüne ince bir katman: bir saniyeyi geçen komutların süresi komut satırının sağ ucunda, koşarken canlı. Kendi seti olmasının sebebi kapsamı değil **bedeli** — canlı sayaç kare talebinin üçüncü sebebini (**saat**) doğuruyor ve `bt-gpu::link`'in modül başlığındaki yazılı sözleşmeyi üçe tamamlıyor; ayrıca blok defterinin girdi başına bayt bütçesini 8'den 12 bayta çıkarıyor (`const` assert ile bağlı). Referansta karşılığı var (`docs/ARASTIRMA.md`: satır 98 `command_duration_threshold`, satır 108 "Komut blokları (süre, kırmızı gutter)"); eşiğin ayara bağlanması bu sette **yok**. Set açıldı → `.tasks/013-komut-suresi/context.md` |
| 014 | imleç stilleri (DECSCUSR) + blink | DECSCUSR'ın üç şekli ile blink tek sette, çünkü protokol onları tek dizide birleştiriyor (altı değer = 3 şekil × {sabit, yanıp sönen}). 008'de adıyla ertelenmişti ve gerekçesi de yazılıydı: blink **ilk süresiz animasyon** olurdu. Kendi setini hak etmesinin sebebi o: blink içerik değil **hareket** karesi ve tetiği 013'ün saati — yani `Waker`'a hasar dikmeyen ikinci bir kol, saatin "süre" yerine "son tarih" tutması ve blink'e adlandırılmış bir durma koşulu. Şekiller bedavaya geliyor (`Term::cursor_style()` ikisini birden veriyor) ve kare altyapısına hiç dokunmuyor. Set açıldı → `.tasks/014-imlec-stilleri/plan.md` |
| 015 | imleç cilası | 014'ün üstüne ince bir katman ve **üç isteği tek sete** topluyor: caret'in yüzeyi (köşe yarıçapı + hale), odak kaybında içi boş imleç, ve ölçülmüş bir kusur — hızlı komutta caret dock'tan ızgaraya çıkıp geri iniyor, çünkü komutun safhası animasyonun yerleşmesinden çok daha kısa sürüyor (sayılar setin `context.md`'sinde). Kendi setini hak etmesinin sebebi yüzey yarısı: caret `cell_bg`'nin düz dörtgeninden çıkıp **kardeş bir fragment**'e taşınıyor, yani `bt-gpu` üçüncü pipeline'ını kazanıyor. Set açıldı → `.tasks/015-imlec-cilasi/plan.md` |
| 016 | imleç ayarları | 015'in **bilinçli** borcundan doğdu (`.tasks/016-imlec-ayarlari/context.md` → Mevcut durum, 015 R8: "ayar anahtarı yok"): caret'in köşe yarıçapı, halesi, odaksız hâli ve blink periyodu koda gömülü dört sayıydı ve oradan `[terminal]` anahtarlarına çıktı. Kendi setini hak etmesinin sebebi kapsam değil **yüzey**: dördü de kayıt anında uygulanıyor, yani ayar şemasının ve canlı izlemenin yolu. Set açıldı → `.tasks/016-imlec-ayarlari/plan.md` |
| 017 | ekranın geri dönüşü | **Sete bağlanmamış borçtan doğdu** (aşağıdaki "tamamlama listesi ızgarayı kaydırıyor"): Tab listesi kalkınca geriye bir delik kalıyor ve ekran Tab öncesine dönmüyor. Kökü 011'in kayıtlı bedeline ve 012'nin Karar 3a'sına bağlı, yani `bt-core`'un bastırma aralığı + `origin`'in tek yönlü kayması birlikte açılıyor. Set açıldı → `.tasks/017-ekranin-geri-donusu/context.md` |
| 018 | klavye + dosya sürükleme | **Üç kullanıcı isteği bir arada** (2026-09-19 ve -20): macOS metin kısayolları ("bu kısayollar yok diye pratiklik çok azalıyor"), **ölü tuşlar** — Türkçe Q'da `~` ve `` ` `` yazılamıyor, ölçüldü — ve Finder'dan dosya sürükleme. Tek set olmalarının sebebi kapsam değil dosya: üçü de `bt-shell/view.rs` + `keys.rs`'te, `keyDown:`'ın aynı yönlendirmesinde buluşuyor. Set açıldı → `.tasks/018-klavye-ve-surukleme/context.md` (düzen taraması ve iki ölçüm orada; kapsamı panel daralttı — `discussion.md` → Muhakeme) |
| 019 | glyph yedeği | **Sete bağlanmamış borçtan doğdu** (aşağıdaki "font fallback yok"): seçili fontta olmayan karakter kutu çiziliyor. Ölçüldü — `⏵` (U+23F5) Menlo'da yok, aynı satırdaki `→`/`↻`/`░` var; `CTFontGetGlyphsForCharacters` cascade list'e düşmüyor. **Aile** düzeyinde yedek zaten var (`PREFERRED` → `FALLBACK`), eksik olan **karakter** düzeyi. Emoji setinden ayrı ve çok daha ucuz: yedekten gelen glyph de tek kanallı maske, `R8Unorm` atlas duruyor; değişen tek şey yuva anahtarına gerçek fontun kimliğinin girmesi. Set açıldı → `.tasks/019-glyph-yedegi/context.md` |
| 020 | fare raporlama | **Kullanıcı isteğinden doğdu** (2026-09-21): Claude Code'un giriş kutusunda tıklanan yere imleç gelmiyor. Sebep fare raporunun yokluğu — adı TUI desteği değil mouse tracking. Ölçüldü (CLI bir pty'ye koşturuldu): Claude Code `?1000/1002/1003/1006` istiyor, etkin kip 1003. Raporun yarısı hazır: `wheel_report` adı tekerlek ama gövdesi genel X10/SGR raporu ve kip takibi alacritty'den bedava geliyor; eksik olan düğme/hareket kodlaması ve `mouseDown:`'ın kipi hiç sormaması. İçindeki ürün kararı Shift arbitrajı — fare kipinde Shift terminali geri alır, yoksa uygulama içinde metin seçme yeteneği ölür. Set açıldı → `.tasks/020-fare-raporlama/` |
| 021 | kutu çizim | **Sete bağlanmamış borçtan doğdu** (aşağıdaki "blok, çizgi ve Braille fonttan geliyor"): kutu/blok çizim ve Braille fonttan geliyor ve **döşemiyor** — Menlo'nun `█`'i hücreyi doldurmuyor, Braille de genişlik kapısından dönüyor. Çare yordamsal çizim ve örüntü depoda hazır (`RuleKind`'ın yedi sprite'ı); çıktı yine tek kanallı kapsama maskesi, yani emoji setinin "ikinci atlas mı, RGBA mı" çatalı **hiç açılmıyor** — 019'un 020/021'den ayrıldığı gerekçenin aynısı. Kullanıcı görünürlüğü yüksek ve sürekli: Claude Code'un maskotu, spinner'ı ve her TUI çerçevesi. Set açıldı → `.tasks/021-kutu-cizim/` |
| 022 | atlas tahliyesi | **Sete bağlanmamış borçtan doğdu** (aşağıdaki "Atlas dolunca geri dönüşü yok") ve borcun kendi yazdığı sıra geldi: *önce ölçüm, sonra LRU* — ölçüm 021'de koştu. Kusur kullanıcıya iki sıradan kapıdan çarpıyor: 16 kez Cmd + (varsayılan 13pt, adım 1pt, tavan 72pt) Retina'da doyma eşiğine çıkarıyor, ve Braille bloğunu tarayan bir TUI (`btop`, Claude Code spinner'ı) tek başına 256 yuva isteyebiliyor. Dolduktan sonra o oturumda ilk kez görülen her karakter kalıcı kutu. Set açıldı → `.tasks/022-atlas-tahliyesi/` |
| 023 | emoji + geniş glyph | **Kutu çizim yarısı 2026-09-21'de ayrıldı → 021** (kullanıcı kararı); kalan ikisi tek iş: 003 `teslim.md` B.3 "geniş karakter tek yuvaya kırpılıyor" diyor, 004 `plan.md` ikisini aynı sete bağlıyor. İçinde gerçek bir mimari çatal var: atlas `R8Unorm`, yani tek kanallı **kapsama maskesi**; emoji ise renkli bitmap. İkisi aynı dokuda yaşayamaz → ikinci atlas mı, RGBA mı, sprite başına format bayrağı mı? `/rfc` şart. TUI'ler (htop, tmux, lazygit) bu setten sonra düzgün görünür. **Bedel:** 2026-09-16'daki ikinci kaymayla TUI çerçeveleri **yedi set** boyunca bozuk görünür — bilerek **Sıra 2026-09-22'de öne alındı** (kullanıcı kararı: materyal pas geçildi) ve envanter `/rfc`'nin araştırmasında ölçüldü: adayı olan 1346 geniş karakterin ilerlemesi de mürekkebi de hücrenin **1.66** katı, yani kapıyı sütunla çarpmak ailenin tamamını kabul ediyor; emojinin **78'i tek sütunlu** ve geometri kolu onlara yardım etmiyor. Set açıldı → `.tasks/023-emoji-ve-genis-glyph/`. **Setin bıraktığı dock kusuru 024'te
kapandı:** yazılan emoji dock'ta kutu çıkıyordu ve bazıları giriş satırını
ızgaraya fırlatıyordu; kökü bu setin kendi değişmeziydi (dock'ta geniş bayrak
hiç kurulmasın) ve o değişmez bir kod kısıtından *karar* diye türetilmişti |
| 024 | dock sütun saysın | **Kullanıcı bildirdi (2026-09-22)**: yazılan emoji dock'ta kutu çıkıyor ve bazıları giriş satırını ızgaraya fırlatıyor. Kök tek — `dock::render` karakter indeksini sütun sanıyor — ve üç belirtiyi birden doğuruyor: dock `Cell::wide`'ı kuramıyor (023'ün kendi değişmezi), tazelik kapısının iki tarafı farklı birim okuyor (ayna `char`, ızgara hücrenin `c`'si — birleştirici `CellExtra`'da) ve CJK'lı satırda caret kayıyor (aşağıdaki `CURSOR` borcu). Ölçüldü: `🎉` dock'a düşüyor ama kutu, `❤️` satırı fırlatıyor. Sete girmesinin sebebi kapsam değil **sıra**: 023 emojiyi çizilebilir yaptı, yani artık yazılıyor ve kusur her yazışta görünüyor. Set açıldı → `.tasks/024-dock-sutun-aritmetigi/` |
| 025 | tazelik kapısı zamansal | **Kullanıcı bildirdi (2026-09-22)**: `🥰` yazınca caret dock'tan ızgaraya sıçrıyor. zsh bazı kod noktalarını kendisi `<hex>` diye yazıyor ve tazelik kapısı **içerik** karşılaştırdığı için aynayı bayat sanıyor. Aşağıdaki borç kalemi ("ölçütü içerik, oysa zaman olmalı") sete bağlandı. Set açıldı → `.tasks/025-tazelik-zamansal/` |
| — | materyal yüzey | `substrate` shader'ı, grain/sheen, birkaç materyal; 007'nin tema rollerine bağlanır. Metalterm'in görünüşü kapalı kaynak — adlarını biliyoruz (`docs/ARASTIRMA.md` → Görünüm), matematiğini bilmiyoruz; `/rfc`'nin ilk işi referans görüntü/video toplamak ve tasarım denemesi. Efekt GPU bütçesi yer: kare süresi tabanı bu setten **önce** alınmış olmalı, yoksa "materyal ne kadar yavaşlattı" cevapsız kalır. *(2026-09-21: taban **yarım alındı** ve kalan yarısı sanıldığından zor çıktı. CPU sütunları ile açılış prizde ölçüldü, ikisi de taban. **GPU sütunu alınamadı:** aynı kaynak ve bayt bayt aynı metallib ile 0,25–0,68 ms arasında dolaşıyor ve sınanan dört hipotez (koşu süresi, derleme sonrası ilk koşu, güç durumu, metallib kimliği) onu ayıramadı. Bu setin bedeli bir **shader** bedeli, yani ihtiyacı olan sütun tam da gezinen sütun — üstelik sıçrama yeniden derleme sınırında oluyor ve "shader'lı hâl shader'sız hâlden yavaş mı" sorusu doğası gereği o sınırın iki yanını karşılaştırmak demek. Yani ön koşul **açık** ve artık ondan fazlası: bu setin `/rfc`'si ölçme yöntemini de çözmek zorunda (aynı binary içinde çalışma zamanı anahtarıyla A/B, ya da çok sayıda yeniden derleme üzerinden ortalama). `docs/OLCUMLER.md` → `## Kare süresi`.)* *(2026-09-16'da ertelendi; gerekçesi değişmedi, yalnız sırası — ve ertelenmesi ölçüm baskısını da erteledi.)* |
| 026 | sekme | **Kullanıcı isteği (2026-09-23)**: macOS'un kendi sekmeleri — her sekme bir pencere ve kendi oturumu, standart kısayollar, temaya boyanmış saydam başlık çubuğu; kayıtlı bedel ("bir pencere = bir oturum") sekme gerçekten bir pencere olduğu için ödenmedi → `.tasks/026-sekmeler/` |
| 027 | yumuşak kaydırma | **Kullanıcı isteği (2026-09-23)**: ızgarada smooth scroll — 008 satırının "hareketin ikinci tüketicisi"nin kalan yarısı (öteki yarısı, çıktı gelince kayma, 011'de geldi). Trackpad pikseli izliyor, çentik süzülüyor, kesirli konumda tepede yarım satır → `.tasks/027-yumusak-kaydirma/` |
| 028 | kapatma onayı | **Kullanıcı isteği (2026-09-23)**: "claude code açıkken çat diye kapanıyor" — ⌘W ve ⌘Q koşan süreci sormadan kapatıyordu; 007 ve 026'nın adıyla kaydettiği bedel. Referansın `confirm_close`'u (never/running/always) → `.tasks/028-kapatma-onayi/` |
| 029 | ayarlar penceresi | **Kullanıcı isteği (2026-09-23)**: "güzel olmalı ve anlaşılır, abartmadan temiz" — bugün her ayar TOML'u elle düzenlemek demek. Yerel pencere, kenar çubuğunda dört kategori; tek kaynak yine `settings.toml`, yazma `with_theme`'in genellemesi ve geçerli değerler ayrıştırıcının tablolarından (aşağıdaki "beş kopya" borcunun `name()` yarısı burada kapanıyor) → `.tasks/029-ayarlar-penceresi/` |
| 030 | dock'ta yazma ve silme animasyonları | **Kullanıcı isteği (2026-09-23)**: "metalterm için bunlar animasyonlu ayarlanmış, oradaki seçenekler gibi istiyorum" — 012'nin adıyla ertelediği `keypress`/`delete_mode` (yukarıdaki 012 satırı). Ön şartları yerinde: ayna (012), dock'un sütun aritmetiği (024), tazelik (025) ve 029'un Motion bölmesi. Bedeli beşinci pipeline (`glyph_fx`) ve `Motion`'ın yanında ayrı bir animatör (`GlyphFx`, uyku testinde adlı terim) → `.tasks/030-dock-yazim-animasyonlari/` |
| 031 | fareyle seçim: ızgara ve dock | **Kullanıcı isteği (2026-09-24)**: "çift tıklamada hiçbir aksiyon yok. özellikle dock kısmında metin seçme yok" — ızgarada kelime/satır seçimi ve Shift+tıklama, dock'ta tıkla-caret, seçip silme ve yerine yazma, temaya `selection` rolü ve yuvarlak köşeli vurgu. Aşağıdaki "Farenin jest durumu sınanamıyor" kalemi burada kapanıyor (kalemin kendi dediği ev). Bedeli altıncı pipeline ve sarmalayıcıda bir widget → `.tasks/031-fare-ile-secim/` |
| 032 | çok satırlı dock | **Kullanıcı isteği (2026-09-24)**: çok satırlı giriş (yapıştırma, `for`, heredoc, `\`-devam) ızgaraya gidiyor ve 030/031'in dock davranışı orada kayboluyor — aşağıdaki "Dock çok satırlı girişi göstermiyor" borcu. Dock yukarı doğru büyüyor ama **yalnız çizimde**: PTY sabit, ızgara ötelenir, yani borcun "nefes alan ekran" gerekçesi konusuz. Bedeli betikte yedinci ayna gövdesi (`PREBUFFER`); kullanıcının `PS2`'sine dokunulmuyor → `.tasks/032-cok-satirli-dock/` |
| 033 | geçmişte arama | **Kullanıcı isteği (2026-09-25)**: "arama için bir akış oluştur; kullanıcı deneyimi güzel olsun, UI ve UX temiz ve güzel olsun" — aşağıdaki "Sonrası" listesinin arama overlay'i. Eşleştirme alacritty'de hazır (`RegexSearch`; regex yeni bağımlılık değil), vurgu 031'in `selection` pipeline'ı, gezinme 027'nin süzülmesi. Çubuk Metal overlay değil AppKit paneli (metin girişinin doğruluğu bedava, IME borcuna dokunmuyor) ve PTY'yi yeniden boyutlandırmıyor → `.tasks/033-gecmiste-arama/` |
| 034 | ekranı temizle + standart menü kalanları | **Kullanıcı isteği (2026-09-25)**: "⌘K ile ekranın temizlenmesi olmuyor … başka koymadığımız davranışlar var mı bak". Terminal.app'in Clear to Start / Clear Scrollback'i terminal tarafında (kabuğa bayt gitmez), yanına aynı turda tek menü öğesiyle kapanan kalanlar: ⌘Home/⌘End/⌘PgUp/⌘PgDn kaydırma ve ⌃⌘V Paste Escaped Text. Envanterin kendi tasarımını isteyen yarısı aşağıdaki iki satırda → `.tasks/034-ekrani-temizle/` |
| 035 | grapheme dizileri | **Kullanıcı gördü (2026-09-25)**: Claude Code'un `🇹🇷 "…"` satırlarındaki bayraklar yan yana iki kutu çıkıyor. 024'ün adıyla bıraktığı "grapheme dizileri taban karakteriyle" sınırı. Emoji dizileri (bayrak, ZWJ, ten rengi, VS16) ızgarada tek geniş hücrede kümeleniyor (okuyucu döngü `bt-core`'a geçti), atlas diziyi `CTLine` ile tek glyph'e şekillendiriyor, dock düzenlemesi kümeyi bölmüyor. **Kapandı** → `.tasks/035-grapheme-dizileri/`. Kapsam dışı kalanlar aşağıdaki borç listesinde. |
| 036 | ssh'ta uzak oturum hissi | **Kullanıcı isteği (2026-09-26)**: "ssh'a bağlanıldığında kullanıcı uzak makinede olduğunu hissetsin" — bugün bağlam satırı ssh sürerken **yerel** yolu ve dalı gösteriyor, yani yanlış bilgi. Uzak tarafa hiçbir şey kurulmuyor: ssh/mosh 028'in süreç tablosundan (argümanlar `KERN_PROCARGS2`, yeni bağımlılık yok) `C` kenarında algılanıyor, dock tek satırlık bir durum çubuğuna iniyor (`⇄ host  /uzak/yol`, yeni `info` rengi, renkli üst çizgi), başlık ve sekme `⇄` taşıyor, uzak OSC 7 artık okunuyor → `.tasks/036-ssh-uzak-oturum/` |
| 037 | ssh modunun ikinci turu | **Kullanıcı isteği (2026-09-26, 036'nın sohbeti)**: dört madde tek sette. (1) **Host'a göre renk**: prod/dev ayrımını terminal kendiliğinden bilemez, kullanıcı söyler. Tek kaynak `settings.toml`'daki bir desen listesi (`[remote] hosts`, host adıyla sırayla eşleşir, ilk eşleşen kazanır, eşleşme yoksa bugünkü `info`). Kolay yol menü: ssh sekmesinde "Mark `{host}` as ▸ Production / Staging / Development / None", dosyaya satırı kendisi yazar (Theme ▸ emsali). Menüde **anlam** seçilir, renk temadan gelir (önerilen eşleme: Production → `error`, Staging → henüz çizilmeyen `warning` rolü, Development → `success`). Renk `⇄`'e, dock'un üst çizgisine ve sekmeye gider. Adın içinden tahmin **reddedildi**: `product-db` eşleşir, adında "prod" olmayan prod sunucusu uyarısız kalır. (2) **Finder damlası ssh'ta**: bugün uzak makinede anlamsız olan **yerel** yol yapıştırılıyor, yani kusur. Seçenekler: damlayı reddetmek ya da uzak dizine yüklemeyi (`scp`) önermek. Belirsizse kullanıcı tarafı. (3) **⌘T aynı host'a**: ssh sekmesinde yeni sekme aynı hedefe bağlanır (yeni sekmenin etkin sekmenin dizininde doğmasının uzak karşılığı). (4) **Bağlantı kopunca haber**: ssh 255 ile bitince dock'ta "Connection lost · ⏎ reconnect". Bugün sessizce yerel prompt'a dönülüyor. Set kararları: sıralı `{ host, mark }` dizisi, `*`/`?` glob, doğrudan `#rrggbb` de kabul; `warning` temanın ANSI sarısı; sekmede `accessoryView` noktası (yalnız işaretli host); ⌘T yerel kabukta aynı ssh komutunu koşturur, ⌥⌘T New Local Tab, ⌘N yerel; teklif yalnız ssh 255'te ve ilk tuşta kalkar (255 kopmayı başarısız bağlantıdan ayırmıyor — ölçüldü). Finder damlası **uzak dizine yükleniyor** (kullanıcı kararı, genişletilmiş): onay sayfası (Replace/Merge, `df`), ara arşivsiz `tar c | ssh … tar x`, sıralı kuyruk ve `▴ list`, dock'un üst çizgisinde ve Dock simgesinde ilerleme, ⌘./✕ iptal → `.tasks/037-ssh-ikinci-tur/` |
| — | komut işaretleri üstünde gezinme | 034'ün envanterinden (2026-09-25): ⌘↑/⌘↓ komutlar arası atlama, Select Between Marks, ⌘L Clear to Previous Mark, "son komutun çıktısını kopyala" — Terminal.app'in Edit ▸ Navigate Marks'ı, iTerm2'nin shell integration'ı. `CLAUDE.md`'nin "komutlar arası atlama henüz yok" borcu bu satır. **Neden ayrı set:** bloklar bugün yalnız **görünür** çıpalardan ve yalnız **komutun satırı** olarak çözülüyor (işaret, bölge değil — 010 Karar); dördü de geçmişte çıpa taraması ve bir blok **bölgesi** (komut satırı → sonraki prompt) istiyor, yani `frame()` sınırında değil defter üstünde yeni bir dizin. Arama sayımının parça parça yürüyüşü (033, `SearchIndex`) o dizinin emsali. Sırası: 034'ten sonra, ön koşulu yok |
| — | tıklanabilir bağlantılar | 034'ün envanterinden (2026-09-25): ⌘-tık ile URL ve dosya yolu açma, OSC 8 bağlantıları (Terminal.app, iTerm2, Ghostty; referansta `path_link`, `docs/ARASTIRMA.md`). Bugün OSC 8 yalnız blok çıpası olarak okunuyor. `bateri://` bağlantıları `NSWorkspace`'e verilmeden yutulmalı — kendimize yollamanın anlamı yok ve yutulmasa bile `block/` yolunu `TabId::from_url` sessizce eliyor (038 Karar 7). **Neden ayrı set:** algılama (regex + OSC 8), üstüne gelince vurgu (031'in `selection` pipeline'ının üçüncü kullanıcısı) ve fare rotasının dördüncü kolu (`input::button_route`; fare kipinde ⌘'nin anlamı bir ürün kararı) birlikte geliyor |
| 039 | bölme | 2026-09-23'te sekmeden ayrıldı (026 yalnız sekme). **Bedeli kayıtlı:** komut blokları, Input Dock ve doldurma bandı "bir yüzey = bir oturum" varsayımıyla indi; bölme tek pencerede N yüzey demek. 026 sekmeyi pencereye koydu (`TerminalWindow`: pencere başına oturum, link ve renderer), yani bölmenin bedeli **ödenmedi, ertelendi** — bölme o nesneyi pencere başına bir yerine yüzey başına bir yapmak demek. **Keşif (2026-09-27):** varsayım yalnız `bt-shell`'in sahipliğinde, `bt-core`/`bt-gpu`'da değil; önce `TerminalPane` ayrımı, sonra bölmeler (kullanıcı kararı). Bedel keşifte `bt-shell`'e indi: bölme pane başına oturum/link/renderer demek ve `bt-core`/`bt-gpu` değişmeden taşındı (tek istisna `Theme::separator_srgb`'nin sRGB kopyası); gezinme, boyutlama, büyütme ve odaksız pane'in soluk örtüsü de kare yolunun dışında, AppKit'te → `.tasks/039-terminal-pane-bolmeler/` |
| — | tam uzak entegrasyon | 036'nın kapsam dışı bıraktığı yarı (2026-09-26): uzakta blok, işaret, süre sayacı ve dock aynası — kitty'nin `kitten ssh`'ı, Ghostty'nin ssh-integration'ı gibi sarmalayıcı betiği (ve istenirse terminfo'yu) ssh üstünden taşımak. 036 yalnız **yerel** tarafı biliyor: host, uzak OSC 7 ve başlık. **Bedeli kayıtlı:** betik taşımak üç kabuk ve uzak makinenin rc'sine dokunmama kuralı demek; terminfo taşımak `TERM`'in ssh'ta ncurses'ı kırdığı yere (Metalterm #23, `docs/ARASTIRMA.md`) yeniden girmek. Ön koşulu 036 |
| — | uzun yerel komutta giriş satırını gizleme | **Kullanıcı reddetti (2026-09-26, 036'nın sohbeti)** — kayıt yeniden önerilmesin diye burada. 036 dock'un giriş satırını **yalnız ssh/mosh** boyunca kaldırıyor; uzun süren yerel komutta aynısı reddedildi: süre eşiği kullanıcı okurken ekranı oynatır ve komut bitince ters bir sıçrama doğurur. Açılacaksa gerekçesi bu iki bedeli karşılamalı |
| 040 | Linux kapısı + wgpu renderer'ı | **Kullanıcı kararı (2026-09-28)**: bateri macOS'ta aynen kalırken Linux'ta da koşsun; pencere katmanı winit, Wayland birincil (X11'de ritim best-effort), MVP'de ayar penceresi de var, bağımlılıklar (`wgpu`, `winit`, `freetype`, `fontconfig`, `harfrust`/`rustybuzz`, gerekirse `raw-window-handle`/`zbus`) onaylı. Yol altı adım ve **ilki bu set**: Docker'lı `make linux` kapısı ve renderer'ın ölçümlü denemeyle başlayan wgpu geçişi. Önde, çünkü yolun geri kalanını **durdurabilecek tek** adım o (deneme kötüyse eskale). **040 sürerken `bt-gpu`'ya dokunan başka set açılmaz**: taşıma boyunca iki renderer var ve her shader düzeltmesi iki arka uca yazılmak zorunda kalırdı. Set açıldı → `.tasks/040-linux-kapisi-ve-wgpu/` |
| 041 | yedek glyph'in küçültülmesi | **Kullanıcı gördü (2026-09-29)**: Claude Code'un artifact bağlantısı `⧉` kutu çıkıyor — mürekkebi hücreden %11 geniş ve 019'un kapısı "kutu ya da tam glyph" diyordu. Tek kural bütün aileyi kapsıyor (tek karakterlik yordamsal çizim reddedildi): sınırın içindeki aday küçük puntolu kopyasıyla çiziliyor, tek sütunlu emoji dahil (kullanıcı küçük emojiyi kutuya tercih etti). Önde, çünkü `bt-atlas`'ta ve 040'ın `bt-gpu` yasağına dokunmuyor; kusurlu karakterin kullanıcıdan önce bilinmesi için önce bir tarama (`make tarama`) ve araç karakterlerinin bekçisi → `.tasks/041-yedek-glyph-kucultme/` |
| — | font sistemi soyutlaması + Linux font yığını | 040'tan sonra — trait'in kendisi 040'a bağlı değil, ama kapının büyümesi (`bt-gpu`'nun Linux'ta piksel sınaması) wgpu'yu istiyor: `bt-atlas`'ın CoreText yarısı (`font.rs`'in açma/türetme/metrik/mürekkep/yedek/şekillendirme yüzeyi ve `raster.rs`'in iki çizim fonksiyonu) bir `FontSystem` trait'inin arkasına; macOS'ta CoreText kalır, Linux'ta FreeType + fontconfig (cascade = `FcFontSort`) + HarfBuzz ailesi (`harfrust`/`rustybuzz`, emoji kümeleri). Kapı mantığı (`ink_fits_box`, `centre_shift`, sıra), atlas ve yordamsal çizim dokunulmaz; macOS kalibrasyon sınamaları `cfg(target_os = "macos")`, değişmez bekçileri platformsuz. **Kapı büyür:** `make linux` `bt-atlas`'ı ve lavapipe (Mesa'nın yazılım Vulkan'ı) üstünde `bt-gpu`'nun offscreen piksel sınamalarını da koşar — bu setten sonra renderer Linux'ta da piksel bekçili |
| — | `bt-shell` ayrımı | Font setinden sonra, winit'ten önce: `bt-shell` → `bt-shell-common` (AppKit'siz modüller: ayar okuma, bölme ağacı, punto, tanılar, jest defteri, kaçış; küçük uyarlamayla yükleme kuralı, `jobs` (`proc_*` → `/proc`), `child` (`NSLocale` → ortam, `login -flp` → `$SHELL -l`), `watch` (kqueue vnode → inotify), `keys`) + `bt-shell-macos` (bugünkü AppKit kodu; 040'ın macOS `Pacer`'ı buraya taşınır). Davranış değişmez; katman kuralı `bateri → bt-shell-{macos,linux} → bt-shell-common → bt-gpu → {bt-atlas, bt-core}` olur ve `make denetim`'in katman yönü kontrolü ona göre yazılır. `make linux` `bt-shell-common`'ı da koşar |
| — | `bt-shell-linux` (winit) MVP | Ayrımdan sonra. Pencere, klavye + IME (winit IME), fare, pano (primary selection dahil), sekmeler, bölmeler, arama çubuğu ve **ayar penceresi** — üçü GPU'da kendimiz çiziyoruz (kullanıcı kararı); Ctrl+Shift kısayolları, Option → Alt/Meta. Linux `Pacer`'ı (Wayland frame callback'i; X11 best-effort, adıyla yazılı bilinen sınır) ve yüzey `raw-window-handle`'dan. Kalite kapıları (boşta sıfır kare, animasyon ritmi) Wayland'de; kapı başsız bir Wayland bileşimcisiyle duman koşusuna büyür. Dock zsh üstünde aynen çalışmalı. Sete sığmazsa GPU'da çizilen kromun (sekme çubuğu, arama, ayar penceresi) ikinci bir sete ayrılması o setin `/rfc`'sinin kararı |
| — | Linux platform hizmetleri | MVP'den sonra: bildirim (D-Bus), açık/koyu ve Hareketi Azalt (xdg-desktop-portal), `bateri://` için `.desktop` `x-scheme-handler`, `/proc` ve inotify'ın MVP'de kalmayan kolları, yüklemenin bildirimi ve ilerlemesi |
| — | Linux paketleme | Son: `.deb` ve AppImage. Flatpak **sonra**: sandbox kullanıcının host kabuğunu koşturmayı engelliyor ve terminalin varlık sebebi o |

Sonrası (sırasız): palet overlay'i (arama → 033), durum çubuğu (+ sayaç
animasyonu), Sparkle ile güncelleme.

> **Sıra değişti (2026-09-15, kullanıcı kararı).** Kullanıcı Metalterm'in
> asıl değerini "animasyonlar, görünüş" diye koydu; eski sıra hareketi ve
> materyali Input Dock'tan sonraya, sırasız bir listeye bırakıyordu. Renk,
> imleç ve yüzey Metalterm'i ekranda tanıtan üç şey ve üçü de shell
> entegrasyonuna bağlı değil — o yüzden 007–009. Hareket toplu bir set
> değil, parça parça: altyapı + imleç 008'de, blok animasyonları 013'te,
> yazma animasyonları 014'te. Orkestratörün bir önceki önerisindeki
> "imleç, emoji setinden sonra (genişlik kesinleşsin)" gerekçesi zayıftı —
> geniş hücre bilgisi grid'de — ve düştü. Bedeli: emoji ve kutu çizimi iki
> set gecikir.
>
> **Numara kayması.** Bu tarihten önceki belgelerde 008 = emoji/geniş/kutu,
> 009 = sekme/bölme, 010 = shell entegrasyonu, 011 = komut blokları, 012 =
> Input Dock; bugün sırasıyla 010, 011, 012, 013, 014. Kapanmış setlerin
> dosyaları (ör. 006'da "sekme/bölme (009)") tarihli kayıttır, düzeltilmedi.
>
> **İkinci kayma (2026-09-16).** Sıra kullanıcı kararıyla Input Dock'a doğru
> kısaltıldı: shell entegrasyonu 012'den **009**'a çekildi, komut blokları
> 010, Input Dock 011 oldu; materyal yüzey, emoji/geniş/kutu ve sekme/bölme
> sırasıyla 012, 013, 014'e ertelendi. Gerekçeleri değişmedi — yalnız sıraları.
> Bu tarihten önceki belgelerde (ör. 008'in dosyaları) eski numaralar geçer ve
> düzeltilmedi; tarihli kayıt böyle okunur.
>
> **Üçüncü kayma (2026-09-17).** 011 açıldıktan sonra **kapsamı** daraldı, sırası
> değil: Input Dock ve prompt'un devri 011'den çıkıp kendi setine (**012**)
> ayrıldı, materyal yüzey / emoji-geniş-kutu / sekme-bölme sırasıyla **013, 014,
> 015**'e kaydı. Gerekçe iki turluk panelden çıktı ve setin kaydında duruyor
> (`.tasks/011-tabana-yapisik-icerik/discussion.md` → Karar 8 ve Karar 2 eki):
> `>` bugünkü `Frame`'de temsil edilemiyor, yani prompt devralınıp yerine bir
> şey konamıyor; ve dock'un gerçek karşılığı (ayna) üç katmanda yeni tesisat
> istiyor. Prompt'un devri, `>`, çıpanın taşınması ve dock **birbirine kilitli**
> olduğu için dördü tek sete gitti. Bedeli açık: Input Dock bir set daha
> gecikiyor. Karşılığı, 011'in kabuk betiğine ve 010'a hiç dokunmadan inmesi.
> **Slug da değişti** (`011-input-dock` → `011-tabana-yapisik-icerik`); numara
> yeniden kullanılmıyor, `input-dock` adı 012'ye kalıyor.
>
> **Dördüncü kayma (2026-09-18).** Araya **013 komut süresi sayacı** girdi;
> materyal yüzey / emoji-geniş-kutu / sekme-bölme sırasıyla **014, 015, 016**'ya
> kaydı. Gerekçeleri değişmedi, yalnız sıraları. İstek kullanıcıdan geldi ve
> kendi setini hak etmesinin sebebi kapsamı değil bedeli: canlı sayaç kare
> talebinin **üçüncü** sebebini doğuruyor (hasar ve hareketin yanına **saat**)
> ve `bt-gpu::link`'in modül başlığındaki yazılı sözleşmeyi değiştiriyor —
> yamanacak bir düzeltme değil. Bu tarihten önceki belgelerde eski numaralar
> geçer ve düzeltilmedi; tarihli kayıt böyle okunur.
>
> **Beşinci kayma (2026-09-18).** Araya **014 imleç stilleri + blink** girdi;
> materyal yüzey / emoji-geniş-kutu / sekme-bölme sırasıyla **015, 016, 017**'ye
> kaydı. Gerekçeleri değişmedi, yalnız sıraları. İstek kullanıcıdan geldi
> ("metalterm'deki gibi blink") ve kendi setini hak etmesinin sebebi yine
> kapsamı değil bedeli: blink içerik karesi **olamaz** — `bt-gpu::link`'in
> modül başlığı onu 013'te adıyla `Waker`'dan men etti — ama hareket karesi
> olarak da ekran hızına bağlanamaz. Kalan yol saatin ikinci bir tadı ve o,
> 013'ün taze kodunu (`arm_clock`) yeniden yazmayı gerektiriyor.
> **Materyalin bedeli ikinci kez ödeniyor:** yazılı ön koşulu (kare süresi
> tabanı `/measure` ile bu setten önce alınmış olmalı) bir kez daha erteleniyor
> — dördüncü kaymanın "ertelenmesi ölçüm baskısını da erteledi" cümlesi
> bugün ikinci kez geçerli. Bu tarihten önceki belgelerde eski numaralar geçer
> ve düzeltilmedi; tarihli kayıt böyle okunur.
>
> **Altıncı kayma (2026-09-19).** Araya **iki** set girdi: **015 imleç cilası**
> ve **016 klavye**; materyal yüzey / emoji-geniş-kutu / sekme-bölme sırasıyla
> **017, 018, 019**'a kaydı. Gerekçeleri değişmedi, yalnız sıraları. İkisi de
> kullanıcıdan geldi ve ikisi de **günlük kullanımda hissedilen** kusurlar:
> 015'in çekirdeği ölçülmüş bir sıçrama (hızlı komutta caret çıkıp iniyor),
> 016'nınki "kısayollar yok diye pratiklik çok azalıyor". Kendi setlerini hak
> etmelerinin sebebi yine kapsamları değil bedelleri — 015 `bt-gpu`'ya üçüncü
> bir pipeline ekliyor, 016 `view::reaches_terminal`'ın Command kapısını ve
> `encode_key`'in imzasını açıyor, yani ikisi de yamanacak düzeltme değil.
> **Materyalin bedeli üçüncü kez ödeniyor:** yazılı ön koşulu (kare süresi
> tabanı `/measure` ile bu setten önce alınmış olmalı) bir kez daha
> erteleniyor. Bu tarihten önceki belgelerde eski numaralar geçer ve
> düzeltilmedi; tarihli kayıt böyle okunur.
>
> **Yedinci kayma (2026-09-20).** Araya **016 imleç ayarları** girdi ve
> kapandı; klavye **017**, materyal yüzey / emoji-geniş-kutu / sekme-bölme
> sırasıyla **018, 019, 020** oldu. Kayma bu satır yazılana kadar
> **kayıtsızdı**: `.tasks/016` imleç ayarlarına gitmiş, tablo hâlâ 016 =
> klavye diyordu — iki yerde numara tutmanın ürettiği drift, altıncı
> kaymanın aynısı ve bu kez gerekçesi yok, yalnız düzeltmesi var. 016'nın
> kendi seti olmasının sebebi 015'in **bilinçli** kararıydı
> (`.tasks/016-imlec-ayarlari/context.md` → Mevcut durum, 015 R8: "ayar
> anahtarı yok"): caret'in dört sayısı koda gömülüydü ve oradan ayara çıktı.
> **Materyalin bedeli dördüncü kez ödeniyor:** yazılı ön koşulu (kare süresi
> tabanı) bir kez daha erteleniyor.
>
> **Sekizinci kayma (2026-09-20).** Araya **017 ekranın geri dönüşü** girdi
> (sete bağlanmamış bir borç sete dönüştü); klavye **018**, materyal yüzey /
> emoji-geniş-kutu / sekme-bölme sırasıyla **019, 020, 021** oldu. 017'nin
> satırı bu düzeltmeyle **eklendi**: set `.tasks/`'ta açıkken tabloda yoktu,
> yani yedinci kaymanın düzelttiği driftin aynısı ikinci kez oluşmuştu.
> Klavye setine **dosya sürükleme** de girdi (kullanıcı isteği, aynı gün) —
> gerekçesi kapsam değil dosya: ikisi de `view.rs`'in kancalarında buluşuyor.
> **Materyalin bedeli beşinci kez ödeniyor.**
>
> **Dokuzuncu kayma (2026-09-20, kullanıcı kararı).** Araya **019 glyph
> yedeği** girdi — yine sete bağlanmamış bir borç sete dönüştü ve yine aynı
> gün bildirildi (Claude Code'un `⏵⏵` göstergesi kutu çıkıyor). Kullanıcı onu
> materyalden **önce** istedi; materyal yüzey / emoji-geniş-kutu / sekme-bölme
> sırasıyla **020, 021, 022** oldu. Gerekçe ucuzluk ve görünürlük: yedekten
> gelen glyph de tek kanallı maske, yani emoji setinin mimari çatalı
> açılmıyor, ama belirti ok/sembol/kutu karakterlerinin geçtiği her yerde.
> **Materyalin bedeli altıncı kez ödeniyor** — ve bu, ölçüm baskısının da
> altıncı kez ertelenmesi demek.
>
> **Onuncu kayma (2026-09-21, kullanıcı kararı).** Araya **020 fare
> raporlama** girdi — yine bir kullanıcı isteği, yine aynı gün bildirildi
> (Claude Code'un giriş kutusunda tıklanan yere imleç gelmiyor). Materyal
> yüzey / emoji-geniş-kutu / sekme-bölme sırasıyla **021, 022, 023** oldu.
> Gerekçe yine ucuzluk ve görünürlük: raporun yarısı (`wheel_report`'un genel
> gövdesi, alacritty'nin kip takibi) zaten depoda, eksik olan düğme kodlaması
> ve bir kapı; belirti ise fare isteyen her TUI'de (vim, htop, lazygit, tmux)
> görünüyor. Kullanıcı kaymayı görüp devam dedi.
> **Materyalin bedeli yedinci kez ödeniyor** — ve ölçüm baskısı yedinci kez
> erteleniyor.
>
> **On birinci kayma (2026-09-21, kullanıcı kararı).** Araya **021 kutu
> çizim** girdi — 022'nin (emoji + geniş glyph + kutu çizim) kutu yarısı
> ayrıldı ve kendi seti oldu; materyal yüzey / emoji-geniş / sekme-bölme
> sırasıyla **022, 023, 024** oldu. Ayrılmanın gerekçesi 019'unkiyle
> **kelimesi kelimesine** aynı ve bu üçüncü tekrar: yordamsal çizimin
> çıktısı tek kanallı kapsama maskesi, yani emoji setini pahalı yapan şey
> (renkli bitmap ve "ikinci atlas mı, RGBA mı" çatalı) bu sette hiç
> açılmıyor. Kullanıcı sırayı sordu ("kutu çizimi daha önemli sanırım") ve
> ucuzluk + görünürlük kıyası onu doğruladı: maskot, spinner ve her TUI
> çerçevesi her gün görünüyor, oysa emoji'nin çatalı bir `/rfc` istiyor.
> **Materyalin bedeli sekizinci kez ödeniyor** — yazılı ön koşulu (kare
> süresi tabanı `/measure` ile bu setten önce alınmış olmalı) bir kez daha
> erteleniyor ve o cümle artık sekiz kayma boyunca tekrarlanıyor.

>
> **On ikinci kayma (2026-09-22, kullanıcı kararı).** Araya **022 atlas
> tahliyesi** girdi — yine sete bağlanmamış bir borç sete dönüştü; materyal
> yüzey / emoji-geniş / sekme-bölme sırasıyla **023, 024, 025** oldu.
> Gerekçe bu kez "ucuzluk ve görünürlük" değil **sıra**: borcun kendi maddesi
> "önce ölçüm, sonra LRU" diyordu ve ölçüm 021'de geldi (aile 421 yuva, doyma
> eşiği Retina'da 29pt). 021 eşiği hem yaklaştırdı hem somutladı — Braille'in
> 256'sı bu setten önce sıfır yuva harcıyordu.
> **Materyalin bedeli dokuzuncu kez ödeniyor**, ama bu kez ödenen şey
> ölçüm baskısı **değil**: kare süresi ölçümü 2026-09-21'de koştu. CPU
> sütunları ile açılış taban oldu; GPU sütununun tabanı **alınamadı** ve
> sebebi ölçüldü (aynı kaynak ve bayt bayt aynı metallib ile 0,25–0,68 ms
> arası geziniyor, dört hipotez onu ayıramadı). Yani materyalin ön koşulu
> artık "ölçüm yapılmadı" değil, **"ölçme yöntemi çözülmedi"** — ve o, o
> setin `/rfc`'sinin ilk işi. Ayrıntısı materyal yüzeyin satırında ve
> `docs/OLCUMLER.md` → `## Kare süresi`.

> **On üçüncü kayma (2026-09-22, kullanıcı kararı).** **Emoji + geniş glyph
> materyal yüzeyin önüne geçti**: emoji **023**, materyal yüzey **024**, sekme
> + bölme **025** oldu. Gerekçe ilk kez "ucuzluk ve görünürlük" **değil** —
> emoji setin en pahalısı, projenin ilk gerçek mimari çatalı. Gerekçe
> materyalin kendi ön koşulu: on ikinci kaymanın yazdığı gibi o artık "ölçüm
> yapılmadı" değil **"ölçme yöntemi çözülmedi"**, yani sırası gelen set
> `/rfc`'sinde bir tasarım işinin yanında bir **ölçüm yöntemi** icat etmek
> zorunda ve o ikinci iş kuyruğun geri kalanını bekletiyor. Kullanıcı
> materyali pas geçti; ön koşulu kalkmadı, **sırası kalktı** — ölçme yöntemi
> çözülünce yerine döner.
> **Materyalin bedeli onuncu kez ödeniyor** ve bu kez ödenen şey ne ölçüm
> baskısı ne sıra: ödenen şey **tasarımın kendisi**, çünkü referans görüntü
> toplamak da o setin `/rfc`'sine ait ve o iş de kullanıcıyı bekliyor.
> Numara kayması yine tarihli kayıt: bu tarihten önceki belgelerde emoji 024,
> materyal 023 geçer ve düzeltilmedi. **Tek düzeltme** on ikinci kaymanın
> materyale bakan işaretçisiydi: "023'ün satırında" artık emojiyi gösterdiği
> için sete **adıyla** bağlandı — numarayla değil, çünkü aynı cümle bir
> sonraki kaymada yine eskirdi.


> **On dördüncü kayma (2026-09-22, kullanıcı kararı).** Araya **024 dock sütun
> saysın** girdi; materyal yüzey **025**, sekme + bölme **026** oldu. Gerekçe
> bu kez ne ucuzluk ne ölçüm baskısı: **023'ün kendi bıraktığı kusur**.
> Kullanıcı emojiyi yazınca dock'ta kutu gördü ve bazı emojilerde giriş satırı
> ızgaraya fırladı; ikisinin de kökü `dock::render`'ın karakter indeksini
> sütun sanması. Sıraya girmesinin sebebi şu: 023 emojiyi **çizilebilir**
> yaptı, yani artık yazılıyor — kusur her yazışta görünüyor ve materyalin ön
> koşulu (ölçme yöntemi) hâlâ çözülmedi.
> **Setin kendisi bir süreç dersinin faturası.** 023'te `/plan-review`'un
> Codebase-fit jürisi doğru bir kod kısıtı buldu (dock'un sütunu karakter
> indeksinden geliyor) ve ondan "dock'ta geniş bayrağı hiç kurulmasın"
> **sonucunu** çıkardı; sentez onu karar sanıp plana gereksinim, koda değişmez
> diye yazdı. Kısıtın ikinci çözümü — dock sütun saysın — ilk turda
> konuşulsaydı bu set hiç açılmayacaktı. Ders iki yere yazıldı
> (`.claude/skills/rfc` → Bulguyu işleme yolu, `proje.md` → Set kapısı ekleri)
> ve özü tek cümle: jüri bulgusunun **gözlemi ve çıkarımı** onun yetkisi,
> **çözüm önerisi** değil; boşlukta kullanıcı tarafı seçilir.
> **Materyalin bedeli on birinci kez ödeniyor.**

> **On beşinci kayma (2026-09-23).** Araya **025 tazelik kapısı zamansal**
> girdi; materyal yüzey **026**, sekme + bölme **027** oldu. Gerekçe 024'ünkünün
> aynısı: önceki setin görünür kıldığı kusur. 024 emojiyi dock'ta çizilebilir
> yaptı, kullanıcı `🥰` yazdı ve caret yazarken ızgaraya sıçradı. Kusur 024'ün
> değil 012'nin kapısında ve borç olarak zaten kayıtlıydı; sıraya girmesinin
> sebebi her yazışta görünmesi.



## Sete bağlanmamış borçlar

Bunlar kendi setlerini hak etmiyor; yukarıdaki setlerden birine yamanırlar.
Yamandıkları yer belli olunca buradan silinip o setin dosyasına geçerler.

- **035'in kapsam dışı bıraktığı kümeler.** Emoji dizileri tek glyph, ama:
  eşlenmemiş tek RI kutu kalıyor; emoji dışı UAX #29 kümeleri (Arapça
  lam-elif, Hangul jamo, Hint SpacingMark) ve tek sütunlu birleştirici
  (aksan, hareke, `⌚︎`) taban karakteriyle çiziliyor; ⌘F kümenin ikinci kod
  noktasını görmüyor (`🇹🇷` taban karakteriyle bulunur — `RegexIter`
  alacritty'nin); dock'un bağlam satırı karakter biriminde; dock'ta küme
  `BUFFER`/öneri sınırını aşabiliyor (`👍` yazılı, öneri `🏽` ile başlıyor —
  caret iki sütun solda; `phase-5.md` → Uygulama Notları). Çıplak 78 tek
  sütunlu emoji aşağıdaki küçültme kalemine bağlı
  (`.tasks/035-grapheme-dizileri/plan.md` → Kapsam Dışı).

- **Zil (BEL) yutuluyor.** `Event::Bell` `session.rs`'in olay kolunda boş
  düşüyor: ne ses, ne görsel zil, ne arka sekmede işaret (Terminal.app ve
  iTerm2'nin üçü de var). 034'ün envanterinde çıktı (2026-09-25). Ayar
  anahtarı (`bell`), görsel efekt (hareket saatinden, durma koşullu) ve
  sekme/Dock rozeti ister; yamanacağı set belli değil.
- **Reset / Hard Reset yok.** Terminal.app Shell ▸ Reset (⌥⌘R) ve Hard Reset
  (⌃⌥⌘R). 034'ün envanterinde çıktı (2026-09-25). RIS geçmişi siliyor ve
  `CSI 2 J` bayrağının, dock aynasının ve blok defterinin RIS'e karşı ne
  yapacağı tasarlanmadı (`CLAUDE.md`: "`3J` ve RIS için kol yok"); 034'ün
  terminal tarafı temizliği o sorunun yarısını cevaplıyor, yani doğal evi
  034'ten sonraki ilk kabuk/terminal durumu seti.

- **Duman kapısı saatin meşru karelerini ayırt edemiyor.** 013 kare talebinin
  üçüncü sebebini (**saat**) getirdi ve periyodu 1000 ms; kapının en duyarlı
  katı ise `sessiz ≥ QUIET_FLOOR` (868 ms), yani "son içerik karesinden
  sonraki sessizlik". İkisi **ilkesel olarak uyumsuz**: saniyede bir kare
  isteyen bir yük sessizliği tabanın altına düşürür. Bugün çarpışmıyorlar ve
  sebebi yapısal — duman reçetesi `/bin/sh` koşuyor, OSC 133 basmıyor, blok
  doğurmuyor, yani saat hiç armed olmuyor. Borcun doğacağı an: **entegrasyonlu
  bir ölçüm yükü**. O gün kapının jeton satırına `saat=` eklenip sessizlik
  ölçütünün saatin karelerini dışlaması gerekecek (jeton silinmez, eklenir).
  Yamanacağı yer belli değil, o yüzden burada.
- **`psvar[9]` geç kayıt olan bir `precmd` hook'uyla silinebilir —
  bash/fish setiyle birlikte.** 010'un kapısında (`/code-review`) çıktı ve
  sarmalayıcının sözleşmesine dokunuyor, o yüzden çaresi betiklerin doğduğu
  sete ait. *(Aynı kalemde duran `exec zsh` kusuru — payın kalıcı boyanması —
  010'un sonunda **kapandı**: işaret bölge değil komut satırı olunca boyanacak
  bölge kalmadı.)*
  `add-zsh-hook` sırası yalnız `.zshrc` koşarken kayıt olanlar için garanti;
  zsh-defer ya da p10k'nın instant-prompt sonu gibi ilk prompt'ta kayıt olan
  bir hook `psvar=(…)` yazarsa `%9v` boşa genişler ve bütün çıpalar sessizce
  düşer (blok yok, şerit yok — yanlış çizim değil). Çare prompt'taki genişlemeyi
  `psvar`'dan çıkarmak, yani R1.1'in `psvar` kararını yeniden açmak.
  **012 phase-4 bu borcu büyüttü:** giriş satırının bastırılması da aynı
  çıpadan türüyor (`suppressed_input` kimliği verir, satırı çıpa bulur), yani
  `%9v` boşa genişlerse bedel artık yalnız kayıp şerit değil — bastırma hiç
  koşmaz ve kullanıcı yazdığını **iki yerde** görür (ızgarada + dock'ta).
  Belirti hâlâ sessiz ve hâlâ yanlış çizim değil, ama artık görünür.
- **Git dalı prompt başına bir fork.** 012 phase-6 dalı `precmd`'de
  `git rev-parse --abbrev-ref HEAD` ile okuyor (detached HEAD'de ikinci bir
  çağrı, kısa SHA için). Terminal kendi `git` sürecini doğurmuyor ve bu
  bilinçli — dal kabuğun bildiği bir şey — ama bedel **her prompt'ta** ödeniyor
  ve büyük depoda hissediliyor; p10k'nın `gitstatusd` daemon'ı tam da bunun
  için var. Çaresi bir önbellek (dizin + `.git/HEAD` damgası) ya da bir daemon
  ve ikisi de kendi tasarımını ister. **Ölçüm de borç:** maliyet ölçülmedi ve
  ölçecek kanca yok (`BT_INPUT_LATENCY_SAMPLES`), yani `/measure` bugün
  kapatamaz; kullanıcının göreceği tek yüzey "büyük depoda prompt gecikmesi".
  Yamandığı yer belli değil; dock'un bağlam satırını elden geçiren ilk set
  doğal ev.
- **Bastırmanın tazelik kapısı prompt hücrelerini de sayıyor.** 012 set
  kapısının (`/code-review`) iki bulgusu tek köke bağlı: kapı ızgara satırının
  **tamamının** son mürekkebini aynanınkiyle karşılaştırıyor, oysa ayna
  prompt'u hiç taşımıyor. `prompt = "terminal"`'de `PS1` sıfır genişlikli
  olduğu için sorun görünmüyor; `prompt = "shell"`'de ve kullanıcının
  `precmd`'si prompt satırına bir şey bastığında kapı boş prompt'ta "bayat",
  ilk tuştan sonra "taze" diyor — yani satır her prompt'ta bir kez yanıp
  sönüyor. Üstelik bastırma **satır geneli** (`from..=to` bütün sütunları
  atlıyor) ve çıpa kullanıcının `PS1`'inin önüne ekleniyor, yani
  `prompt = "shell"` seçen kullanıcı prompt'unu ilk tuşta kaybediyor —
  `docs/AYARLAR.md`'nin vaadinin tersi. Çaresi bir tasarım kararı (kapı
  yalnız çıpa sütunundan sonrasını mı saysın, mod'a mı baksın) ve kip'e
  bağlı olduğu için tek satırlık değil. Yamandığı yer: prompt devrini elden
  geçiren ilk set.
- **Bastırılan satır, altında bir şey çizilince boş şerit bırakıyor.** Aynı
  kapının küçük bulgusu: `suppress_to` yalnız tamponun kendi satırlarını
  kapsıyor, ZLE'nin **altına** bastığı tamamlama listesi ya da `zle -M`
  mesajı çiziliyor ve doluluk sayısı onları sayınca bastırılan satır arada
  görünür bir boşluk oluyor. `display: none` yalnız giriş satırı son çizilen
  satırken tam. Aynanın görsel dikişi borcunun (012 satırı) görünür belirtisi.
- **Dock çok satırlı girişi göstermiyor, ızgaraya bırakıyor.** **Kapandı
  (032 phase-4):** satır sonlu görüntü ve `PREBUFFER` dock'ta, bastırma
  bütün satırlarda, `Multiline` kalktı → `.tasks/032-cok-satirli-dock/`.
- **Aynanın `CURSOR`'u karakter indeksi, sütun değil.** **Kapandı (024)** ve
  kapanışın şekli kalemin kendi yazdığından daha geniş: kalem "caret kayıyor"
  diyordu, çaresi "dock sütun saysın" çıktı ve aynı çare üç belirtiyi birden
  kapattı — caret'in kayması, yazılan emojinin dock'ta kutu çıkması ve
  birleştirici taşıyan satırın ızgaraya fırlaması. Kalan **iki bilinen
  sınır** adıyla yazılı ve bekçili: bağlam satırı karakter biriminde
  (küçük boy sınıfı, 021'in emsali — CJK'lı bir yol orada hâlâ sütun
  kaydırıyor) ve grapheme dizileri taban karakteriyle çiziliyor — **ikincisi
  kapandı (035)**: emoji dizileri üç yüzeyde tek glyph ve iki sütun
  (`.tasks/035-grapheme-dizileri/`). Ayrıntısı
  `.tasks/024-dock-sutun-aritmetigi/`'de. Aşağıdaki gerekçe tarih olarak
  duruyor: 023'ün panelinde
  çıktı (2026-09-22) ve o setin kapsamı dışında bırakıldı: `dock::render`
  sütunu karakter sayısından türetiyor (`TEXT_COL + offset`, `offset = index -
  skip`), spacer yok, genişlik farkındalığı yok — caret de öyle. Yani dock'un
  giriş satırında bir CJK karakteri varsa caret **yanlış hücrede** durur ve
  ondan sonraki her harf bir sütun kayar. Kusur 012'den beri var; 023 onu
  **kötüleştirmiyor** ve gerekçesi yapısal — o set geniş bayrağını dock
  kayıtlarında hiç kurmuyor (adıyla yazılmış değişmez), çünkü iki hücrelik bir
  glyph komşu karakterin üstüne boyardı. Çare iki yönden birinde: ya ZLE
  aynası sütun gönderir (sarmalayıcının sözleşmesi değişir), ya `bt-core`
  `BUFFER` üstünde gösterim genişliği hesaplar (`unicode-width` `bt-core`'un
  grafında zaten var, `alacritty_terminal` üzerinden). İkincisi ucuz görünüyor
  ama `region_highlight`'ın indeksleri de aynı uzayda, yani dönüşüm **tek
  yerde** olmak zorunda. Dock'un giriş satırını elden geçiren ilk set doğal ev;
  021'in "küçük sınıfta kapalı" emsali burada **yetmez**, giriş satırı
  `SizeClass::Normal`.
- **Dock kontrol karakterini çizmiyor.** zsh `Ctrl-V Ctrl-A`'yı ızgarada
  okunur bir `^A` diye basıyor; dock ise o sütunu boş bırakırdı, o yüzden
  025'ten beri böyle bir satır `DockStatus::Control` ile ızgarada kalıyor ve
  caret'i de orada. Kusur değil daraltma: dock'ta yazmak isteyen kullanıcı
  o satırda dock'u kaybediyor. Çaresi yer tutucu (`^X`, zsh'in biçimi) ve
  024'ten beri mümkün — aritmetik sütun sayıyor. Kendi kararları var: TAB
  kontrol ama `^I` değil boşluğa açılıyor, DEL `^?`, ESC `^[`. Yer tutucu
  geldiği gün `Control` kolu silinir. *(Tazelik kapısının ölçütü borcu 025'te
  kapandı: kapı önce "son girdinin aynası geldi mi" diye soruyor.)*
- **Blok animasyonları.** 010 Karar 5 şeridin belirmesini setten **çıkardı**:
  şerit bugün anında beliriyor. Gerekçe animasyonun zorluğu değil, bedeli —
  `bt-gpu::motion` ikinci bir tüketici kazanır, `Mode::Fade`'in "indirgemenin
  tek yeri" kuralı ikinci bir yer bulur ve boşta sıfır kare kapısı şeridin de
  durma koşulunu sormak zorunda kalır. Şerit animasyonsuz olduğu sürece o
  kapının koruduğu şey bu yoldan tehdit altında değil. Yamandığı yer belli
  değil: hareketin ikinci tüketicisi (yumuşak kaydırma, 008'in artığı) geldiğinde
  aynı sete girmesi doğal olur.
  *(2026-09-17: o ikinci tüketici **011** oldu — içeriğin origin'i `Motion`'ın
  içine giriyor. Şeridin belirmesi 011'e **alınmadı**: set bilerek saf yerleşim
  işi tutuldu ve şerit 012'de prompt işaretiyle birlikte zaten elden geçecek.
  Yani bu borcun doğal evi artık 012.)*
- **Kaymanın yerleşme süresinin kancası yok.** 011 ikinci animatörü
  (`Slide`) getirdi ve `kayma=` jetonu onu sayıyor, ama **kare** sayıyor, süre
  değil — üstelik ölçüm harness'ının iki yükü de onu tetiklemiyor: duman
  reçetesi ötelemeyi hiç oynatmıyor, yük yükü ise PTY'yi 256'lık öbeklerle
  doyurduğu için ekran ilk içerik karesinde zaten dolu (ölçüldü 2026-09-17,
  `kayma=0`). Yani ötelemenin animasyon yolunun **gerçek pencerede koşan
  tanığı yok**; yapısal sınırı kanıtlı (son içerik karesinden sonra en çok
  `TIME_CEILING` = 0,7 sn) ama ampirik süresi bilinmiyor. İki yol var ve
  hangisinin ucuz olduğu açılınca tartışılır: ötelemenin başlangıç→yerleşme
  damgasını tutan bir kanca, ya da satırları **aralıklı** ekleyen üçüncü bir
  yük (`Workload`). İkincisi duman reçetesinin ölçülmüş sözleşmesine
  dokunmadan yeni bir tanık doğurur.
- **Ötelemenin yön kuralının ölçülmemiş yarısı.** 011 kapı sonrası gözle
  kontrolle kayma tek yöne indirildi: içerik büyürken süzülüyor, daralırken
  snap'liyor. Kabul edilmiş bedeli art arda satır yazıp silen bir program
  (spinner, çok satırlı ilerleme çubuğu) — büyürken kayıp daralırken zıplıyor,
  yani simetrik bir salınım yerine testere. Gerçek bir örnekte rahatsız
  ediyorsa çaresi yön değil **mesafe** eşiği olur ve o ölçülmemiş bir sayı;
  ölçüm olmadan değiştirilmez. **017 phase-4 kalemi daralttı:** doldurmanın
  açık olduğu oturumda (dock'lu pencere, birincil ekran, temizlenmemiş, dibe
  yaslı) daralan yön de kayıyor, yani testere yalnız doldurmanın kapalı olduğu
  kollarda kalıyor. Kalan yarı hâlâ ölçülmemiş.
- **Hareket karesi ucuz değil.** 008 Karar 4 hareket karesinde grid'i yeniden
  taramayı önlüyor (`Frame::move_cursor` listeleri koruyor) ama encode yolu
  korunan listeyi yine de **baştan kuruyor**: `AtlasTexture::prepare` her
  glyph ve kural için yuvayı yeniden çözüyor, `instance_buffer` her karede
  yeni bir `MTLBuffer` ayırıyor. Bedel hücre sayısıyla büyüyor ve kayma
  boyunca her karede yeniden ödeniyor — kaç kare olduğunu jetonun `hareket=`
  sayacı söylüyor (`/code-review`, 008 kapısı). Çare biçimi belli — listeler değişmedikçe örnek tamponunu
  saklamak, kirli bayrağı `Frame::clear`'da dikmek — ama **ölçüm bekliyor**:
  ölçülmemiş bir kazanç için yeni bir önbellek eklenmiyor.
  *(2026-09-21: kare süresine ilk sayılar girdi — `docs/OLCUMLER.md` →
  `## Kare süresi` — ve kapıyı **açmıyorlar, kapalı tutuyorlar**: encode
  sütunu kare bütçesinin %3'ünde, yani saklanacak kazanç bugün ölçülebilir
  değil. Önbellek hâlâ inmiyor.)* Aynı ölçümün
  ikinci sorusu: `cell_bg` geçişi 90 ms'lik belirme uğruna **tamamen**
  harmanlamaya açıldı (`Blend::Opaque` kalktı), yani opak arka plan dörtgenleri
  de harmanlama biriminden geçiyor; alternatif imleci ayrı bir çağrıda
  çizmek.
  *(2026-09-17: 011'in kayma animasyonu bu borcu **büyütüyor** — ekran dolana
  kadar her yeni satır bir kayma kuyruğu doğuruyor ve kuyruğun her karesi bu
  bedeli yeniden ödüyor. Sayısı 011'in yeni `kayma=` jetonunda görünecek.
  Borç yine de 011'e alınmadı: çaresi bir önbellek ve **ölçülmemiş bir kazanç
  için önbellek eklenmiyor** — önce `/measure`.)*
- **Duman kapısı hiç görünmemiş pencerede yanlış tanı veriyor.** Pencere ön
  plana gelmeden açılan bir koşuda (ajanın kabuğu, `cargo run` arka planda)
  display link callback vermiyor: `advance` bir daha koşmuyor, hareket 1
  karede kalıyor ve deadline `Verdict::MotionUnsettled` deyip **"bir durma
  koşulu bozuk"** diye kodu suçluyor. Ölçüldü (jeton satırı
  `.tasks/009-shell-entegrasyonu/phase-4.md`'de): aynı commit kullanıcının
  kendi terminalinde yeşil, ajanın kabuğunda üç koşu da kırmızı; `2af9d87`'de
  de aynı. Örtülme
  kolu (`set_visible(false)` → `Motion::finish`) deliği kapatmıyor, çünkü o
  kol örtülme **bildirimine** bağlı ve hiç görünmemiş pencerede bildirim yok
  — `Motion::finish`'in doc'u sonucu zaten tarif ediyor. Çarenin yönü belli
  (deadline'da pencere hiç görünmediyse hareket kapısı muaf, `Workload`
  muafiyetinin emsali) ama kapıyı gevşetiyor: gerçek bir durma koşulu
  kusurunu da örtmemeli. `make duman`'ın belgelenmiş tek kaçış hâli başsız
  ortamdaki `ATLANDI`; bu üçüncü hâl hiçbir yerde yazılı değil.
- **Tema kare başına iki kez okunuyor.** `Session::frame` kopyayı `Term`
  kilidinden önce kendi içinde alıyor, `link.rs` aynı karede `session.theme()`
  ile ikinci kez alıyor; araya düşen bir `set_theme` o kareyi hücreler eski,
  clear ile imleç yeni paletteyken çizer. Tek kare sürer ve kendini düzeltir
  (tema değişimi zaten kare istiyor), ama "palet tek kaynak" cümlesini
  harfiyen ihlal ediyor (`/audit`, 008 kapısı; 008 öncesinden beri var).
  Çaresi ucuz: `frame()` kullandığı temayı döndürsün.
- **Doc yorumunun altına kod sokmak sessizce doc çalıyor.** 015 ve 016'da
  **dört kez** oldu (`set_reduce_motion`, `apply_reduce_motion`,
  `cursor_blink`, `set_focused`): yeni bir fonksiyon var olanın doc bloğu ile
  gövdesi arasına girince doc yeni fonksiyona geçiyor, eskisi **doc'suz**
  kalıyor ve yeni fonksiyonun doc'u bambaşka bir şeyi anlatıyor. `rustdoc`
  şikâyet etmiyor, `clippy` yalnız araya **boş satır** girerse görüyor
  (`empty_line_after_doc_comments`). Çaresi bir kapı: `make denetim`'e "doc
  bloğu ile `fn`/`pub` arasında başka bir öğe yok" taraması ya da her eklemede
  `git diff`'te doc sınırını gözle doğrulamak. Şimdilik ikincisi, yani
  **kural yazılı ama mekanik değil**.
- **`docs/AYARLAR.md`'nin bölüm örnekleri hiçbir sınamayla bağlı değil.**
  `### Şablon` bloğu `TEMPLATE` ile bayt bayt çivili
  (`documented_template_is_the_template`) ama `## Anahtarlar` altındaki bölüm
  örnekleri (`### [terminal]`'ın küçük `toml` bloğu gibi) serbest: 016'da dört
  anahtar eklendi ve o blok üç anahtarda kaldı, `/audit` görene kadar sessiz
  bayatladı. Çare ya aynı türden bir sınama (bölüm örneği şablonun o bölümünün
  alt kümesi olmalı) ya da örnekleri büsbütün kaldırıp tabloya güvenmek.
- **Var olan ayar dosyası yeni anahtarları hiç görmüyor.** Şablon kullanıcının
  diskine **yalnız dosya yokken** yazılıyor (`settings::create_if_missing`);
  var olan dosyaya yazan tek yol View ▸ Theme ▸ ve o da yalnız
  `[appearance] theme` satırını değiştiriyor. Sonuç: her yeni anahtarla
  kullanıcının dosyası bir adım geriye düşüyor — **yeni anahtarlar yok** ve
  **eski yorumlar bayat** kalıyor (016'da iki kez elle tazelendi: kullanıcının
  `cursor_blink` açıklaması hâlâ 014 öncesi metni taşıyordu). Belirti sessiz:
  anahtarı bilmeyen onu arayamaz. Çare eksik anahtarları **yorumlarıyla**
  dolduran bir yol; `toml_edit` zaten bunun için seçildi (yorumlar ve bilinmeyen
  anahtarlar yerinde kalsın diye), yani altyapı hazır. **Var olan dosyaya
  yazmak yeni bir yazma yolu**, yani mimari karar: kimin tetiklediği (açılış mı,
  Settings… mi), yedek alınıp alınmayacağı ve kullanıcının sildiği bir
  anahtarın geri gelip gelmeyeceği kararlaşmadan inmez. Kaynak: 016
  `phase-1.md`/`phase-3.md` → Uygulama Notları.
  029'dan beri ayar penceresi de var olan dosyaya yazıyor, ama yalnız
  değiştirilen anahtarı ve **yorumsuz** ekliyor; doldurma borcu yerinde.
- **Ayar ayrıştırmasının beş kopyası.** `osc52`, `cursor_motion` ve
  `reduce_motion` aynı "şu üç dizgeden biri, değilse tanı bırak ve
  varsayılana düş" örüntüsünü elle tekrarlıyor; her enum'un `name()`'i de
  ayrıştırıcının kollarıyla **elle** eşleşiyor (`/code-review`, 008 kapısı).
  *(2026-09-20, 016: yardımcı **doğdu** — `named_enum`, `cursor_unfocused` ile
  birlikte. Beş kopya taşınmadı, çünkü her birinin tanı cümlesi kendi
  sözcükleriyle yazılı ve taşımak mesajları tek turda değiştirirdi. Ondalık
  tarafın ikizi `ranged_float` da aynı sette doğdu ve `line_height` ona
  **taşındı**; `font_size` tek uçlu olduğu için kaldı.)*
  **Madde 029 phase-1'de kapandı:** her dizge enum'u tek bir `NAMES`
  tablosu taşıyor, `name()` ve ayrıştırıcı (`named_enum`, `osc52` dahil)
  ondan okuyor, elle yazılmış kol kalmadı. Tanı metinleri bayt bayt aynı —
  yardımcının kurduğu cümle beş kopyanınkiyle zaten özdeşti (bekçisi mevcut
  tanı sınamaları). Tablolar ayar penceresinin seçeneklerini de veriyor.
- **Logger yok.** `tracing` bağlanmadı; yoksayılan olaylar (başlık, zil, pano)
  ve alacritty'nin `log` satırları **sessizce** düşüyor. Hata ayıklamayı
  körleştiriyor, o yüzden erken yamanmalı. 005 doğal adayıydı — ölçüm
  kancaları aynı enstrümantasyon damarından geçiyor — ama **almadı**: ayrı
  bir bağımlılık kararı (`tracing`) ve seti "ölçüm + gözlemlenebilirlik" diye
  şişirirdi (`005/plan.md` → Kapsam Dışı). Hâlâ sete bağlanmadı.
- **Kapanışta arkada kalan çocuk.** Sınırsız bekleme **kapandı** (005
  phase-2b) ve bu maddenin eski "→ 006" bağı da onunla düştü: 006'ya
  bağlanma gerekçesi "günlük kullanımda donan bir terminal kabul edilemez"
  idi, artık donmuyor. Kalan borç daha dar ve eşiği bloke etmiyor: süre
  dolunca çocuk arkada bırakılıyor, onu süreç çıkışı topluyor. Bu yüzden
  **sete bağlanmadı** — `bt-core`'un kapanış tasarımına meşru biçimde dokunan
  ilk set toplar. Ayrıntısı ve çürütülmüş çaresi `CLAUDE.md`'nin kapanış
  maddesinde; artık **ölçülebilir** de (`kapanis=abandoned`).
- **Üçüncü taraf bildirimlerinin geri kalanı.** 006 yalnız `alacritty_terminal`'ın
  (Apache-2.0) borcunu kapattı. macOS ağacındaki diğer dış paketlerin çoğu
  MIT ya da MIT seçeneği taşıyor (`objc2` ailesinin dördü yalnız MIT) ve MIT
  de bildirimin kopyalarla gitmesini istiyor. 007 phase-1 `toml_edit` ile
  yedi paket ekledi, yedisi de MIT seçilebilir: `toml_edit`, `toml_parser`,
  `toml_datetime`, `winnow`, `indexmap`, `hashbrown`, `equivalent`; phase-7
  sekizinciyi (`toml_writer`, MIT seçilebilir). Paket **dağıtılmadan önce**
  toplanmalı → imza/notarization/Sparkle ile gelecek dağıtım seti. Aynı set
  indirilen kopyada Gatekeeper'ın gerçek davranışını da görmeli (kanıt ve
  açık soru: 006 `phase-4.md` → Uygulama Notları).
- **004'ün altı kalemlik bulgu borcu.** Listesi ve gerekçeleri
  `.tasks/004-yazi-bicimleri/teslim.md` ile phase-3'ün `## Uygulama
  Notları`'nda. Sete bağlanmadı; ilgili dosyaya meşru biçimde dokunan ilk set
  toplar.
- **Pencereye duyarlı hasar yok.** Geçmişe kaydırılmış pencerede akan çıktı,
  ekranda hiçbir şey değişmediği hâlde her `Wakeup`'ta kare istiyor: `dirty`
  bayrağı pencereyi bilmiyor. **Boşta değil, çıktı akarken** — yukarıdaki
  boşta kare borcundan ayrı bir iş. Kaydırma (006) görünür kıldı, getirmedi;
  maliyeti ölçülmedi. Kaynak: 006 `phase-3.md` → `/audit` mercek 8 ve
  orkestratör kararı; `bt-core`'da `AdapterInner::dirty`'nin doc'u.
- **Fare raporlamasının geri kalanı.** **2026-09-21'de sete bağlandı → 020
  ve madde büyük ölçüde kapandı:** tıklama, sürükleme, hareket ve değiştirici
  bitleri (Meta 8 / Control 16) geldi, seçimle çakışmayı Shift arbitrajı
  çözdü (006'nın Karar 1'i yeniden açıldı ve yerine "Shift terminali geri
  alır" kuralı kondu). **Kalan iki kalem:** yatay tekerlek raporu (66/67) ve
  SGR-pixel fare (1016); ikisi de 020'de adıyla kapsam dışı. Kimse
  istemedi, kendi setlerini hak etmiyorlar. Kaynak: 006 `phase-3b.md` →
  Kapsam dışı; `.tasks/020-fare-raporlama/`.
- **Farenin jest durumu sınanamıyor.** 020 set kapısının (`/code-review`)
  waive edilen bulgusu: rotayı kilitleyen defter (`sent_buttons`,
  `dragging`, `Click`'in üç kolu, kayıp bırakmanın telafisi)
  `BateriView`'ın `impl` gövdesinde ve `define_class!` komşusu kod
  sınanamıyor — saf fonksiyona çıkarılanlar yalnız `moved_to_new_cell` ile
  `button_bit`. Bugün dört geçişin dördü de yalnız gözle doğrulandı. Çaresi
  jest durumunu `NSEvent` görmeyen bir struct'a taşımak; bedeli orta ve
  ancak `view.rs`'in fare yolunu zaten elden geçiren bir set içinde ucuz.
  Yamandığı yer belli değil — hareketin ikinci tüketicisi ya da çift/üçlü
  tıkla seçim doğal ev. **031 phase-1'de kapandı:** defter `bt-shell`'in
  `gesture::Gesture`'ına taşındı ve dört geçiş (basış, sürükleme, bırakma,
  kayıp bırakma) orada sınanıyor → `.tasks/031-fare-ile-secim/`.
- **Tamamlama listesi ızgarayı kaydırıyor.** ZLE'nin `BUFFER` olmayan çıktısı
  (tamamlama listesi, `menu-select`, `bck-i-search`, `zle -M`) aynada yok ve
  ızgaraya düşüyor — 012'nin kayıtlı bedeli; liste kalkınca dock ile içerik
  arasında delik kalıyor (kullanıcı 2026-09-19'da dört ekran görüntüsüyle
  bildirdi). **2026-09-20'de sete bağlandı → 017**, ayrıntısı ve ölçümleri
  `.tasks/017-ekranin-geri-donusu/context.md`'de.
  **Bu maddenin bir cümlesi 2026-09-20'de çürüdü ve düzeltildi:** "kaybı
  hiçbir terminal geri getiremez" **yanlıştı** — zsh satırları geri basmıyor,
  ama itilen satırların tamamı scrollback'te duruyor ve gösterilebiliyor
  (`2fdca50`'nin kendi iletisi de bunu söylüyordu). 017 bu yüzden overlay'siz:
  üstte kalan boşluk geçmişin en yeni satırlarıyla doluyor. Kalan borç
  **küçülüyor ama kapanmıyor**: liste *ekrandayken* üstteki çıktı yine
  görünmez (iTerm de göstermiyor), ve gerçek çare — listeyi ızgaraya hiç
  düşürmemek (aynanın altıncı kanalı ya da overlay).
  **Madde 2026-09-20'de kapandı** (gözle kontrol, kullanıcı): bildirilen
  şikâyet "liste kalkınca ekran geri gelmiyor"du ve o çözüldü. "Liste
  *ekrandayken* üstteki çıktı da görünsün" **ayrı ve çok daha büyük** bir
  istek — tamamlama listesini kendi yüzeyinde çizmeyi gerektirir, iTerm de
  yapmıyor, ve kimse istemedi. İstenirse kendi seti açılır; bu maddeyi yarım
  tutmak onu bitmemiş bir iş gibi gösterirdi. `content_rows`'u oynatmak çare
  değil: denendi, deliği yalnız yer değiştirdi (`2fdca50` → `27a0b98`).
  **Kapanışın bir kolu 2026-09-21'de ölçülüp yamandı:** dönüşün tetiği zsh'in
  kendi `\e[J`'si ve zsh onu yalnız liste ekrana **sığdığında** gönderiyor;
  taşan listede ne imleci geri alıyor ne temizliyor (ölçüldü, saf PTY —
  sayılar `assets/shell/zsh/bateri.zsh`'in `LISTMAX` yorumunda), yani 017'nin
  doldurma bandı tetiksiz kalıyor ve liste ekranda **kalıyordu**. Sarmalayıcı
  artık dock'lu kademede `LISTMAX=0` dayatıyor: ölçüt seçenek sayısından
  kapladığı **yere** iniyor ve sığmayan liste basılmadan **önce** soruyor.
  Kalan borç `y` kolu — o listede ekran yine bozuluyor — ve çaresi yeni bir
  madde değil, yukarıdaki "listeyi kendi yüzeyinde çiz" isteğinin ta kendisi.
- **Font fallback yok: seçili fontta olmayan karakter kutu (tofu) çiziliyor.**
  **2026-09-20'de sete bağlandı → 019 ve madde kapandı** (ölçüldü); ayrıntısı
  `.tasks/019-glyph-yedegi/`'de. Eksik glyph artık `CTFontCreateForString`
  ile sistemin cascade'inden geliyor ve kapı **geometrik**: adayın boyayacağı
  piksel hücrenin dışına taşıyorsa kutu kalıyor. (Ölçüt 2026-09-21'de
  ilerlemeden mürekkebe çevrildi — `⏺` U+23FA ilerlemesiyle eleniyor,
  mürekkebiyle sığıyordu; ayrıntısı 019 phase-1 → Ölçülmüş sayılar.)
  Kapının elediği her şey — emoji, CJK,
  geniş ok, **ve ölçüm sırasında çıkan Braille** — aşağıdaki "blok, çizgi ve
  Braille fonttan geliyor" maddesine ve **023**'e devredildi (020 ile 021 araya
  girince numara kaydı; aynı commit aşağıdaki kardeş referansı düzeltmişti,
  bu satır atlanmıştı). Aşağıdaki gerekçe
  tarih olarak duruyor:
  Ölçüldü (2026-09-20, kullanıcı ekran görüntüsüyle bildirdi): Claude Code'un
  `⏵⏵ auto mode on` göstergesi iki boş kutu olarak çıkıyor. Sebep `⏵`
  (U+23F5) — CoreText'e doğrudan soruldu, **Menlo'da yok**; aynı satırdaki
  `→` (U+2192), `↻` (U+21BB) ve `░` (U+2591) Menlo'da **var** ve düzgün
  çiziliyorlar, yani belirti tek karakterde ve font kaynaklı.
  `bt-atlas` glyph'i `CTFontGetGlyphsForCharacters` ile arıyor ve o
  **cascade list'e düşmüyor** (`lib.rs`'in `UNKNOWN_CHAR` sabiti bunu zaten
  adıyla söylüyor); macOS'un fallback yolu `CTFontCreateForString`. iTerm2,
  Terminal.app ve ghostty fallback yapıyor, yani bu bir parite açığı.
  **Emoji setinden (020) ayrı ve çok daha ucuz:** fallback'in getirdiği
  glyph de tek kanallı bir kapsama maskesi, yani `R8Unorm` atlas olduğu gibi
  kalıyor ve oradaki "ikinci atlas mı, RGBA mı" çatalı hiç açılmıyor.
  Değişen şey glyph **cache anahtarı**: bugün yuva `(karakter, yüz, boy
  sınıfı)` ile aranıyor, fallback gelince gerçek fontun kimliği de anahtara
  girmek zorunda. Kapsamı 020'ye eklenebilir ya da kendi küçük setini alır;
  kullanıcı görünürlüğü yüksek, çünkü ok/sembol/kutu karakterleri TUI'lerde
  ve prompt'larda her yerde.
- **`⎿` (U+23BF) kutu çiziliyor ve mürekkep kapısı onu kurtarmıyor.**
  **Kapandı (2026-09-21, tek commit)**: U+23B8–U+23BF artık yordamsal
  (`raster::technical`) ve kapsam maddenin önerdiğinden **bir karakter dar**
  — `⎷` (U+23B7) dışarıda kaldı, çünkü kök işaretinin kuyruğu bir ray değil
  ve cascade'den gelen hâli kapıyı zaten geçiyor (Apple Symbols, mürekkebi
  hücrenin 0.88'i). Kümenin öteki altısı kutu **değildi** ama ölçünce ikisi
  de kusurluydu: `⎸⎹` Apple Symbols'tan 0.42× ilerlemeyle gelip hücrenin
  **ortasına** kayıyordu, `⎺⎻⎼⎽` Monaco'dan gelip 20 px hücrede 0.03–19.19
  boyadığı için **döşemiyordu**. Yani yordamsal çizim burada üç ayrı kusuru
  birden kapattı. İki görünür fark ölçüldü ve ikisi de kabul edildi — ölçüt
  **referans**, çünkü bateri'de `⎿` hiç çizilmiyordu: (1) ayağı hücrenin
  **tam dibinde**, cascade'in adayında (ve onu çizen referans terminalde)
  satır kutusunun ~4 px üstünde duruyor; kenar ailesinin `▁` ve 9. tarama
  satırıyla hizalı olması tercih edildi. (2) dikey çizgi hücrenin **0.
  sütununda**, adayın küçük bir sol boşluğu var — soldaki hücrede mürekkep
  varsa (`│⎿`, `▌⎿`) çizgiler birleşir. Claude Code'un yerleşimi bunu hiç
  uyarmıyor (`  ⎿  `), ama isteniyorsa iki köşeye — `⎸`'ye **değil**, onun
  işi kenarın kendisi — bir piksel girinti verilebilir; bedeli aynalama
  bekçisinin değişmesi. Aşağıdaki gerekçe tarih olarak duruyor:
  Ölçüldü (2026-09-21, Menlo 16pt,
  hücre 9.633): cascade Hiragino Sans W3 veriyor, ilerleme 16.0 (1.66×) ve
  mürekkep **7.68'den 16.0'a** — yarım genişlikli bir glyph ilerleme
  kutusunun **sağ yarısına** yaslanmış, yani ortalama kaydırması
  (`font::centre_shift`, ilerleme hücreyi aştığı için sıfır) onu hücreye
  sokamıyor. Mürekkebe göre ortalamak çare gibi görünüyor ama değil: ortalama
  kuralı **evrensel** ve yedeğe koşullu değil (taban fontun rasteri bit bit
  aynı kalmak zorunda, `every_base_glyph_advance_is_the_cell_advance`).
  Doğru yol **yordamsal çizim**: `⎿` bir köşe parçası, yani 021'in U+2500
  ailesinin akrabası ve aynı mesafe alanı onu da çizer. Kapsam küçük —
  U+23B7–U+23BF'in terminal grafik kümesi — ve 021'in `Family::Line`
  tablosuna bir kol ekliyor. Kendi seti gerekmez, 021'in yanına ya da tek
  commit'lik bir ek olarak girer. (Uygulamada tabloya **satır eklenmedi**:
  `LINES`'ın indeksi `cp - 0x2500` ve kolların ekseni hücrenin ortası olarak
  yazılı, bu kümenin çizgileri ise tanımı gereği kenarda — ayrı bir aile ve
  ayrı bir çizici oldu.)
- **Kutu kalan karakterlerin envanteri ölçüldü (2026-09-21) ve üç ayrı
  sınıf çıktı; üçünün çaresi de ayrı.** Tarama `⎿` düzeltilirken yapıldı
  (taban Menlo 16pt@2x, bu makine, hücre 20×39): U+2190–21FF, U+2200–22FF,
  U+2300–23FF, U+25A0–25FF, U+2600–26FF, U+2700–27BF, U+2B00–2BFF,
  U+1FB00–1FBFF ve U+E000–E0FF. Kutuya düşen her karakter **adayının
  fontuna** göre ayrıldı, çünkü sınıfı belirleyen şey o:
  - **676'sının adayı yok** (cascade `.LastResort` veriyor, yani makinede o
    karakteri taşıyan font kurulu değil). İçinde U+1FB00–1FBFF'in **tamamı**
    var: 256 "Symbols for Legacy Computing" — altılı/sekizli bloklar, kama
    ve köşe şekilleri. Onlar 021'in doğal devamı ve aynı gerekçeyle
    yordamsal çizilmeli (ızgara grafiği, döşemesi hücre ölçüsünden çıkıyor;
    kitty ile ghostty de fonttan almıyor). Powerline/Nerd özel kullanım
    alanı (U+E0B0…) ayrı bir karar: orada kutu **fontun yokluğu**, Nerd Font
    kurulu makinede gerçek glyph geliyor ve çizilmesi doğru.
  - **279'unun glyph'i var ama mürekkep kapısından dönüyor** (Apple Symbols
    128, STIX Two Math 104, Zapf Dingbats 30, kalanı dağınık; `≪ ≫ ⊢ ⊤ ⊥
    ❶ ➀ ⏱ ⏳ ⎯ ⧉`). **89'unun mürekkebi hücreye sığıyor** ve yalnız
    *yerleşimi* dışarı düşüyor: aday ilerlemesine göre ortalanıyor
    (`font::centre_shift`), ilerlemesi hücreyi aşınca kaydırma sıfır kalıyor
    ve hücrenin sağ yarısına yaslanmış mürekkep dışarıda kalıyor — `⎿`'nin
    tam olarak bu hâliydi. Yani bu 89 için çare kapıyı gevşetmek değil
    **yerleşimi mürekkebe bağlamak**, ve bedeli bir mimari karar: ortalamanın
    kuralı bugün **evrensel** ve yedeğe koşullu değil (taban fontun rasteri
    bit bit aynı kalmak zorunda, `every_base_glyph_advance_is_the_cell_advance`).
    Kalan 190'ının mürekkebi gerçekten hücreden geniş; onlar için tek yol
    **küçültme** (kitty yedek glyph'i hücreye sığacak kadar ölçekliyor) ve
    o da ayrı bir karar — eşaralıklı bir ızgarada ölçeklenmiş bir glyph
    komşularından farklı ağırlıkta görünür.
    **Kalem 023'te büyüdü ve daraldı.** Büyüdü: tek sütunlu **78 emoji**
    (`🌡 🎙 🏋 🏔`) aynı karara katıldı — rengi var, iki sütunu yok, mürekkebi
    1.66 hücre, yani 023'ün geometri kolu onlara yardım etmiyor ve kutu
    kaldılar. Daraldı: 190'ın **iki sütunlu** olanları 023'te çizildi
    (ölçüm: adayı olan 1346 geniş karakterin tamamı iki hücreye sığıyor),
    yani kalan küme gerçekten "tek sütun, mürekkebi taşıyor" — ve orada
    küçültmenin alternatifi yok.
  - **41'inin adayı Apple Color Emoji** → **kapandı (023)**: çatal "ikinci
    atlas mı, RGBA mı" diye açılmıştı ve cevabı **ikisi de değil** oldu —
    aynı `Atlas`'ın içinde ikinci bir düzlem (`Plane`), kendi monoton sayacı
    ve `RGBA8Unorm_sRGB` dokusuyla. Bu taramanın 41'i 023'ün kendi
    envanterinde **1000**'e çıktı (tarama emoji bloklarını da kapsadı) ve
    922'si çizildi; kalan 78'i tek sütunlu ve aşağıdaki küçültme kalemine
    devredildi.
  Sıra önerisi: legacy computing (021'in devamı, ucuz ve yordamsal),
  sonra yerleşim kararı (89 karakter, tek dosyalık ama mimari cümleyi
  inceltiyor), sonra emoji.
- **Terminal→uygulama bildirim kanalları eksik: `?1004` odak, `?2031` tema.**
  020'nin ölçümünde çıktı (2026-09-21, Claude Code bir pty'ye koşturuldu) ve
  o setin kapsamı dışında bırakıldı — konusu fare değil. İkisi de aynı
  şekle sahip: **sinyal elimizde, söyleyecek kanal yok.**
  `?1004` (odak): `bt-shell` odağı biliyor (`TerminalWindow::apply_focus` →
  `DisplayLink::set_focused`) ama uygulamaya `\e[I`/`\e[O` demiyor.
  Bedeli "~20 satır" değil: `CLAUDE.md`'de **adıyla yazılı** bir mimari
  kararın ("Odak `bt-core`'a hiç girmiyor … kapı çağrı yerinde") inceltilmesi
  ve süreli koşunun (`apply_focus`'un `run.is_some()` erken dönüşü) yeni bir
  mini kararı. Durumsuz bir iletim (`Session::report_focus`, kipi kilit
  altında sorar, hiçbir şey saklamaz) cümleyi "odak `bt-core`'da yaşamıyor"
  diye inceltir.
  `?2031` (tema): sistemin açık/koyu geçişini `Session::set_theme` canlı
  izliyor; uygulama bunu öğrenemediği için Claude Code kendi paletini bizim
  temamıza uyduramıyor. `bt-shell`'in görünüm izleyicisine ve `Theme` yoluna
  dokunuyor.
  İkisi tek sette birleşebilir: ortak yanları fare değil **bildirim yönü**.
- **`line_height = 1.0` bir no-op değil: hücreye bir piksel ekliyor.**
  019'un kapısında ölçüldü (2026-09-20). `font::metrics` fazlalığı
  `round_up(natural * (line_height - 1.0))` ile hesaplıyor ve `round_up`'ın
  **tabanı 1** — taban ölçüler (genişlik, yükseklik) sıfır olmasın diye
  konulmuş, ama fazlalık için sıfır meşru bir değer. Sonuç: varsayılan ayarda
  her hücre fontun istediğinden bir piksel uzun (Menlo 13pt: font 17 istiyor,
  hücre 18 oluyor). Fazlalık alta düşüyor (`above = extra / 2 = 0`), yani
  taban çizgisi ve kurallar oynamıyor; belirti yalnız bir piksel fazla satır
  aralığı — kusurlu değil ama kodun **söylediği şey değil**
  (`font.rs`: "`1.0`'da fazlalık sıfır, yani bu yol varsayılanda bir
  no-op"). Bekçisi de yoktu: `line_height_grows_the_cell_…`'in dördüncü
  iddiası `1.0`'ı yine `1.0` ile karşılaştırıyordu, yani totolojiydi (yorumu
  019'un kapısında düzeltildi).
  **Düzeltmesi bir ürün kararı**, o yüzden 019'da yapılmadı: `extra`'yı
  tabansız yuvarlamaya çevirmek her kullanıcının ızgarasını bir piksel
  sıkıştırır ve ekrana bir satır daha sığdırır. İsteniyorsa tek satırlık bir
  düzeltme; istenmiyorsa `font.rs`'in yorumu gerçeğe uydurulur.
- **Blok, çizgi ve Braille fonttan geliyor ve döşemiyor.** **Kapandı
  (2026-09-21) → `.tasks/021-kutu-cizim/`**: üçü de artık yordamsal
  çiziliyor, fonta hiç sorulmadan. Geriye bilinçli tek bir delik kaldı —
  **köşegenler** (`╱╲╳`, U+2571–U+2573) fonttan gelmeye devam ediyor (Karar
  3B: üçü de nadir ve kapsamı kapalı tutmak setin ölçüsünü tuttu) ve o delik
  ayrıca 019'un yüz merdiveni kolunun bu makinedeki tek bekçisini ayakta
  tutuyor. Aşağıdaki gerekçe tarih olarak duruyor:
  2026-09-20'de
  kullanıcı ekran görüntüsüyle bildirdi: 019'dan **sonra** da Claude Code'un
  maskotu bozuk. Ölçüldü ve sebep yedek **değil** — maskotun karakterleri
  (blok elemanları U+2580–U+259F) ile çizgi çizim karakterleri (U+2500–U+254B)
  **zaten Menlo'da var**, yedek yoluna hiç girmiyorlar. Sorun fontun
  glyph'inin hücreyi doldurmaması: Menlo'nun `█`'i (U+2588) hücrenin üstünde
  ve altında birkaç satır boş bırakıyor, yani iki `█` alt alta gelince arada
  siyah bir şerit kalıyor. Yatayda dolduruyor, o yüzden belirti yalnız
  satırlar arasında. **Braille aynı olgunun öteki yüzü:** Menlo'da yok, Apple
  Braille'den geliyor ve ilerlemesi hücreninkini aşıyor, yani 019'un genişlik
  kapısından dönüyor — Claude Code'un spinner'ı (`⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`) bu yüzden
  kutu. Sayılar ve nasıl ölçüldükleri
  `.tasks/019-glyph-yedegi/phase-2.md` → Uygulama Notları'nda.
  **Çaresi üçü için de tek ve yedek font değil: yordamsal çizim**, `RuleKind`
  sprite'ları gibi. Blok, çizgi ve Braille birer **ızgara grafiği** (Braille
  2×4 nokta, 256 kombinasyon tek formülden); fonttan gelmeleri zaten yanlış,
  çünkü fontun em kutusu hücre kutusu değil ve döşeme orada kırılıyor. kitty,
  WezTerm, iTerm2 ve Alacritty bunları bilerek fonttan almıyor.
  **Emoji setinden (023) ayrılabilir ve ayrılmalı** — glyph yedeğinin (019)
  o setten ayrıldığı aynı gerekçeyle: emoji setinin pahalı olmasının sebebi renkli bitmap'i ve
  "ikinci atlas mı, RGBA mı" çatalı; yordamsal çizim o çatalın hiçbirine
  dokunmuyor (çıktı yine tek kanallı kapsama maskesi, `R8Unorm` atlas
  duruyor) ve kendi başına ucuz. Kullanıcı görünürlüğü yüksek: maskot,
  spinner ve her TUI çerçevesi.
- **Atlas dolunca geri dönüşü yok.** **022'de daraldı, kapanmadı.** Kapasite
  artık sabit bir dokudan değil **hedeflenen yuva sayısından** türüyor
  (`bt_atlas::SLOT_TARGET`), yani ölçülen kırılma — Retina'da 29pt, 406 yuva,
  ailenin istediği 429 — kalktı ve değişmez bir bekçiye bağlandı
  (`capacity_clears_the_family_at_every_accepted_size`: kabul edilen her
  punto × ölçek × `line_height` için kapasite ≥ aile). Varsayılan punto
  bugünkü dokusunda kaldı.
  **Borç 023'te büyüdü.** Kapıdan dönen CJK o güne kadar **sıfır** yuva
  harcıyordu (negatif önbellek `TOFU`'ya bağlıyor); artık kabul edilen her
  geniş karakter **iki** yuva harcıyor ve tavan hangi karakterlerin
  göründüğüne bağlı — karakterlere açık yuva `capacity() - RULE_RESERVE - 1`
  ve mürekkebi iki hücre isteyen her karakter ikisini alıyor, tek hücreye
  sığan geniş ilan edilmişler (ölçülen 65) birini. 13pt@2x'te en kötü hâl
  **988 ayırt edici geniş karakter** ve 80×24'te bir ekran 960 geniş hücre
  tutuyor, yani 022'nin "tek karede hedeften fazla farklı glyph" dediği
  senaryo varsayılan puntoda **erişilebilir**. 022'nin bekçisi bunu
  göremiyordu (yordamsal aile baştan sona tek sütunlu, değişmez yeşil
  kalıyor); 023 kendi bekçisini getirdi
  (`a_wide_char_is_rejected_whole_when_only_one_slot_is_left`: tam bir yuva
  boşken istenen çift **tümden** reddediliyor, yani yarım glyph + yarım kutu
  yapısal olarak doğmuyor). Renk düzleminin **ayrı** sayacı var, yani emoji
  maskelerin havuzuna binmiyor.
  **Kalan borç ve neden LRU değil:** atlas hâlâ dolabilir ve dolunca hâlâ
  tofu'ya düşüyor; kalan senaryo "tek karede hedeften fazla farklı glyph" ve
  o **ölçülmedi**. Ölçülürse çaresi LRU **değil**, `encode_pass` sınırında
  geri dönüşüm — gerekçesi 022'nin panelinden çıktı ve kaynaktan doğrulandı:
  `slot_uv` uv'yi çözüm anında pişiriyor, `prepare` kare başına **dört kez**
  koşuyor, yani kare **ortasında** yapılan her yuva yeniden kullanımı önceki
  geçişlerin uv'lerini geçersizleştirir ve ızgaradaki harf dock'un
  bitmap'iyle çizilir — sessizce, hiçbir sayaç kıpırdamadan. Ayrıntısı
  `.tasks/022-atlas-tahliyesi/discussion.md` → Karar. Aşağıdaki gerekçe
  tarih olarak duruyor. Yuva tahliyesi
  (LRU) yok: `Atlas::slot` `next >= cap` olduğunda **her** yeni anahtara
  `TOFU` veriyor ve karar önbelleğe girmiyor, yani o andan sonra yazılan her
  yeni karakter kutu çıkıyor — pencere yeniden boyutlanana ya da font/punto
  değişip `ensure()` atlası yeniden kurana kadar. Doluluk **bugün de**
  mümkündü (büyük puntoda birkaç yüz farklı glyph); 021 onu erişilebilir
  kıldı ve sayı artık somut (021 set kapısı, `/code-review`): kapasite
  `floor(1024/w) * floor(1024/h)`, yordamsal aile **421** yuva (32 blok +
  125 çizgi + 256 Braille + 8 teknik; sonuncusu 2026-09-21'de eklendi ve
  altısı zaten fonttan yuva harcıyordu, yani tavana gerçek katkısı 2) ve Braille'in 256'sı bu setten önce **sıfır**
  harcıyordu (genişlik kapısından dönüp negatif önbellekte `TOFU`'ya
  bağlanıyordu, o önbellek ise tavanlı ve toptan boşaltılıyor). Retina'da
  ~27pt civarında `capacity() - RULE_RESERVE` ailenin kendi boyuna iniyor;
  Braille bloğunu tarayan bir TUI (`btop`'un grafikleri) atlası tek başına
  tüketebilir. Kapsam kararı bilinçliydi — `RULE_RESERVE`'ün kutu ailesi için
  karşılığı yok, 416 karaktere pay ayırmak kapasitenin beşte birini bağlardı
  (`.tasks/021-kutu-cizim/discussion.md` → Kapsam dışı) — ama borcun sahibi
  belli: **tahliye**. Önce ölçüm (`/measure`: yuva ayak izi ve puntoya göre
  doyma eşiği), sonra LRU.
- **Doldurma bandının satırları seçilemiyor.** 017'nin bandı geçmiş
  satırlarını gösteriyor ama fare orayı **reddediyor** (`point_to_cell`,
  `fill > 0`): satırlar `frame()` sınırının satır numaralarıyla temsil
  edilemiyor ve kırpma, vurguyu gözün gördüğü yerden başka bir yerde
  başlatırdı — "yanlış seçilir" ile "seçilemez" arasında dürüst olan ikincisi
  (2026-09-20 kararı, kullanıcı bana bıraktı). **Bugün bir çıkış yolu var ve
  ücretsiz:** bant, `display_offset == fill` olan pencereyle **aynı satırları**
  gösteriyor, yani bir çentik yukarı kaydıran kullanıcı aynı görüntüde ama
  seçilebilir bir pencere buluyor. Gerçek çare o eşdeğerliği tıklamaya da
  öğretmek — bandın üstüne tıklayınca pencereyi sessizce o ofsete taşımak;
  görüntü değişmediği için sıçrama olmaz. `Cell.row`'u negatife açmak
  **gerekmiyor**, ki o yol sınır tipini ve bütün tüketicilerini değiştirirdi.
  Sırası: ölçülmüş bir ihtiyaç beklemeye değer — kullanıcı bandı kopyalamak
  isterse bugün de yapabiliyor.
- **Klavye kalanları: Home/End ve iki Control borcu.** Home/End bilerek
  yutuluyor (yutmayı bağlayan sınama duruyor): tüketicisi less/vim, yani tam
  ekran uygulama borcu — varsayılan zsh'te `^[[H`/`^[[F`/`^[OH`/`^[OF` için
  **sıfır** bağlama ölçüldü, yani kabukta görünür kazancı yok. Borç **yalnız o
  yarısı**: kabuğun satır başı/sonu jesti 2026-09-21'de ⌘←/⌘→ ile kapandı
  (`\x01`/`\x05`; izin listesi `view::reaches_terminal`, gerekçesi
  `encode_key`'in kolunda). **Şekli
  bağlandı:** `bt_core::Arrow` **genişletilmez** — doc'u "klavyenin dördü,
  tekerleğin ikisi" diyor ve tekerlek onu gerçekten kuruyor (`session.rs`),
  genişletmek o değişmezi bozar ve tekerleğe "Home" ifade etme yetkisi
  verirdi. Yerine `bt-core::input`'ta iki varyantlı yeni bir `pub enum`, baytı
  `arrow()`'un **yanında** ve DECCKM'in `intro` baytını onunla **paylaşarak**
  (ikinci bir sayı uydurulmaz), çıkışı `write_arrow` gibi `send_input`'tan
  geçen yeni bir `Session` metodu, `keys.rs::KeyInput`'a üçüncü varyant ve
  `arrows_are_keys_not_bytes`'ın aynadaki eşi. Baytlar ölçülü (`infocmp
  xterm-256color`): `khome=\EOH`, `kend=\EOF`, `smkx=\E[?1h\E=`. İki Control
  borcu: Ctrl+Shift+Tab `0x19` (readline/zle `yank`) gönderiyor, Ctrl+numpad
  Enter aynı yapıdan `0x03` — ikisi de gerçek klavyede doğrulanmadı, aday çare
  `charactersIgnoringModifiers`'ı `encode_key`'e geçirmek. Dışarıda kalan iki
  şey daha: Option'ın **topluca** Meta olması (Türkçe Q'da kabuğun bütün
  metakarakterleri Option'da ve o kol varsayılanda hiç koşmazdı, yani
  `[keyboard]` bölümü ilk "Meta istiyorum" isteğinde açılır) ve değiştiricili
  oklar (`\e[1;5A`; Option+ok'un `\eb`'si onların yerine geçmiyor — o bir Meta
  dizisi, xterm'in değiştirici kodlaması değil). Kaynak: 006 `phase-4d.md` →
  Uygulama Notları ve orkestratör kararı (WAIVE (1)); 006 `phase-3b.md` →
  Kapsam dışı; 018 `discussion.md` → Karar 2 ve `plan.md` → Kapsam Dışı
  (R1.2'nin Control kolu iki Control borcunu da bugünkü davranışta tutuyor).
- **Tam IME: altı çizili preedit çizilmiyor.** 018 ölü tuşları ve emoji
  paletinin **girişini** `insertText:` ile kapatıyor; **CJK** ve press-and-hold gibi
  *bekleyen metni gösteren* girdi kipleri kapanmıyor, çünkü `setMarkedText:`in
  karşılığı bir çizim yüzeyi: altı çizili, imlecin bulunduğu yerde duran ve
  ızgaranın hücrelerine ait olmayan geçici metin. Yani borç `keys.rs`'te değil
  `bt-gpu`/`Frame` sınırında — dock'un ikinci `setViewport`'u emsal, overlay'ler
  (palet, arama) de aynı yüzeyi isteyecek. 018'in **içine alınmamasının**
  sebebi bu: o set `view.rs` + `keys.rs`'te kalıyor. Sırası overlay'lerle
  birlikte; bugünkü bedeli Türkçe/İngilizce klavyede **görünmüyor**.
- **Odak raporu (DEC 1004) yok.** Odak `bt-shell`'den `DisplayLink`'e giriyor
  (caret'in içi boşalıyor, blink duruyor) ama uygulamaya **bildirilmiyor**:
  `\e[?1004h` isteyen program (vim'in `FocusGained`, tmux) pencere öne çıkınca
  haber almıyor. Kipi alacritty tutuyor (`TermMode::FOCUS_IN_OUT`), eksik olan
  `\e[I`/`\e[O`'yu yazan kol — ve odağın `bt-core`'a **hiç girmemesi** 015'in
  bilinçli kararı, yani bu borç o kararı yeniden açıyor. Fare raporlamasının
  kalanıyla aynı cinsten ve aynı sete yamanır.
- **Yerel ara kolu.** Kabuğun dili `preferredLanguages`'ın yalnız dil alt
  etiketinden alınıyor, etiketin kendi bölgesi atılıyor: `en-GB` dili + `TR`
  bölgesi → `en_TR` kurulu değil → `LANG=en_US.UTF-8`, oysa `en_GB.UTF-8`
  kurulu. `{dil}_{etiket bölgesi}` ara kolu bir ürün kararı; düşüş `en_US`
  olduğu için etkisi görünür (ABD tarih biçimi). Kaynak: 006 `phase-4c.md` →
  `/code-review` WAIVE (5) ve orkestratör kararı.
- **`bt-shell`'in beş sınaması release profilinde düşüyor.** 023'ün
  uygulamasında görüldü (2026-09-22) ve `git stash` ile taban commit'te de
  doğrulandı, yani **bu setin kusuru değil**: `child.rs`'in sarmalayıcı
  arayışı depo kolunda `target/debug` üzerinden gidiyor ve `cargo test
  --release` o yolu bulamıyor (`sarmalayıcı bulunamadı`). Kapının profili
  debug (`make hepsi`), yani bugün hiçbir şeyi bloke etmiyor — ama
  `cargo test --release` koşturan biri beş kırmızı görüp yanlış yere bakar.
  Çaresi yolu profile duyarlı yapmak ya da `CARGO_MANIFEST_DIR`'dan
  türetmek; ikisi de tek satırlık. `bt-shell`'in `child` modülüne meşru
  biçimde dokunan ilk set toplar.
- **Küçük hijyen.** `make kur` boş hedef dizinini denetlemiyor — bugün
  zararsız. Pano sınamaları oluşturdukları geçici panoları bırakmıyor
  (`releaseGlobally` yok) — kullanıcıya görünmez. İkisi de doğrulanmadı;
  kullanıcı 006'ya almadı. **Panonun yarısı 2026-09-22'de görünür oldu:**
  023'ün set kapısında `pending_copy_delivers_to_the_given_board` tam koşuda
  bir kez düştü ve izole koşuda iki kez geçti — sınamalar genel
  `NSPasteboard`'u paylaşıyor, yani bırakılan geçici panolar **flaky bir
  kapı** üretiyor. Kullanıcıya hâlâ görünmez ama artık kapıya görünüyor. Kaynak: 006 `phase-4c.md` → `/code-review` (8),
  (9); 006 `discussion.md` → Kapsam eki.
- **Paket dumanı.** Paketten açılan duman/ölçüm koşusu bugün elle yazılan bir
  `open` komutu; bir `Makefile` hedefi `docs/OLCUMLER.md` → `## Nasıl yeniden
  ölçülür`'deki dört tuzağın üçünü tasarımla kapatır. Aynı işe bir cümle
  daha: `report_and_exit`'in doc'u kapı düşünce jeton satırının basılmadığını
  ve tanının stderr'e gittiğini söylemiyor. Kaynak: 006 `phase-5.md` →
  `/simplify` takip önerisi ve orkestratör kararı.
