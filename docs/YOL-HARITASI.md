# Yol haritası

Bu dosya **sırayı ve gerekçesini** tutar: hangi iş neden o sırada, neyin neye
dayandığı. **Durumu tutmaz** — o `.tasks/README.md`'nin işidir (`duzen.md` →
İndeks) ve iki yerde durum tutmak drift üretir. Buradaki bir satır açılmış bir
sete dönüşünce, ayrıntısı o setin `context.md`'sine taşınır ve burada tek
satıra iner.

**Tahmin değil sıra.** Takvim, süre ve efor tahmini bilerek yok: ölçülmemiş
sayı yazılmaz kuralı buraya da geçer (`CLAUDE.md` → Dil ve ölçüm). Sıra
değişebilir; değişince **gerekçesiyle** değişir.

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
| 016 | klavye: macOS metin kısayolları | **Kullanıcı isteği (2026-09-19):** Option+oklar kelime atlamıyor, Option+Delete kelime silmiyor, Cmd+Delete satırı silmiyor — "bu kısayollar yok diye pratiklik çok azalıyor". Borcun kendisi yeni değil, aşağıdaki **Klavye kalanları** maddesinde 006'dan beri yazılı; sete bağlanması yeni. **İçinde iki ayrı problem var ve tek set olmalarının sebebi bu:** Option'ın terminal karşılığı **var** (Meta: `\eb`, `\e\x7f` — zsh onları zaten biliyor, biz göndermiyoruz), Cmd'nin **yok** (hiçbir kaçış dizisi Cmd'yi kodlamıyor, üstelik `view::reaches_terminal` Command'lı her tuşu yapısal olarak kesiyor). İkisi aynı dosyada (`bt-shell/keys.rs` + `view.rs`) ve aynı sınama yüzeyinde buluşuyor. **Tek gerçek karar:** Option'ı Meta yapmak Option+harf bileşimini (`∫ ç é`) bitirir; referansın çaresi üç kipli bir ayar ve sol/sağ Option'ın ayrılması (`docs/ARASTIRMA.md`: `keyboard.left_option` / `right_option`, "Option tuşu modları (auto/macOS/Esc+)"). Home/End bedavaya geliyor. **Ölçüm borcu baştan yazılı:** Cmd+Delete'in karşılığı (`\x15`?) gerçek zsh'te doğrulanmadan yazılmaz — zsh'in `kill-whole-line`'ı bash'in `unix-line-discard`'ı değil |
| 017 | materyal yüzey | `substrate` shader'ı, grain/sheen, birkaç materyal; 007'nin tema rollerine bağlanır. Metalterm'in görünüşü kapalı kaynak — adlarını biliyoruz (`docs/ARASTIRMA.md` → Görünüm), matematiğini bilmiyoruz; `/rfc`'nin ilk işi referans görüntü/video toplamak ve tasarım denemesi. Efekt GPU bütçesi yer: kare süresi tabanı (`/measure`, bugün sayısı yok) bu setten **önce** alınmış olmalı, yoksa "materyal ne kadar yavaşlattı" cevapsız kalır. *(2026-09-16'da ertelendi; gerekçesi değişmedi, yalnız sırası — ve ertelenmesi ölçüm baskısını da erteledi.)* |
| 018 | emoji + geniş glyph + kutu çizim | Üçü tek iş: 003 `teslim.md` B.3 "geniş karakter tek yuvaya kırpılıyor" diyor, 004 `plan.md` ikisini aynı sete bağlıyor. İçinde gerçek bir mimari çatal var: atlas `R8Unorm`, yani tek kanallı **kapsama maskesi**; emoji ise renkli bitmap. İkisi aynı dokuda yaşayamaz → ikinci atlas mı, RGBA mı, sprite başına format bayrağı mı? `/rfc` şart. TUI'ler (htop, tmux, lazygit) bu setten sonra düzgün görünür. **Bedel:** 2026-09-16'daki ikinci kaymayla TUI çerçeveleri **yedi set** boyunca bozuk görünür — bilerek |
| 019 | sekme + bölme | 2026-09-16'da ertelendi. **Bedeli kayıtlı:** komut blokları ve Input Dock "bir pencere = bir oturum" varsayımıyla inecek, bu set onları retrofit eder |

Sonrası (sırasız): palet ve arama overlay'leri, durum çubuğu (+ sayaç
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

## Sete bağlanmamış borçlar

Bunlar kendi setlerini hak etmiyor; yukarıdaki setlerden birine yamanırlar.
Yamandıkları yer belli olunca buradan silinip o setin dosyasına geçerler.

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
  ölçüm olmadan değiştirilmez.
- **Hareket karesi ucuz değil.** 008 Karar 4 hareket karesinde grid'i yeniden
  taramayı önlüyor (`Frame::move_cursor` listeleri koruyor) ama encode yolu
  korunan listeyi yine de **baştan kuruyor**: `AtlasTexture::prepare` her
  glyph ve kural için yuvayı yeniden çözüyor, `instance_buffer` her karede
  yeni bir `MTLBuffer` ayırıyor. Bedel hücre sayısıyla büyüyor ve kayma
  boyunca her karede yeniden ödeniyor — kaç kare olduğunu jetonun `hareket=`
  sayacı söylüyor (`/code-review`, 008 kapısı). Çare biçimi belli — listeler değişmedikçe örnek tamponunu
  saklamak, kirli bayrağı `Frame::clear`'da dikmek — ama **ölçüm bekliyor**:
  kare süresi hiç ölçülmedi (`docs/OLCUMLER.md` → `## Kare süresi`) ve
  ölçülmemiş bir kazanç için yeni bir önbellek eklenmiyor. Aynı ölçümün
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
- **Ayar ayrıştırmasının beş kopyası.** `osc52`, `cursor_motion` ve
  `reduce_motion` aynı "şu üç dizgeden biri, değilse tanı bırak ve
  varsayılana düş" örüntüsünü elle tekrarlıyor; her enum'un `name()`'i de
  ayrıştırıcının kollarıyla **elle** eşleşiyor (`/code-review`, 008 kapısı).
  *(2026-09-20, 016: yardımcı **doğdu** — `named_enum`, `cursor_unfocused` ile
  birlikte. Beş kopya taşınmadı, çünkü her birinin tanı cümlesi kendi
  sözcükleriyle yazılı ve taşımak mesajları tek turda değiştirirdi. Ondalık
  tarafın ikizi `ranged_float` da aynı sette doğdu ve `line_height` ona
  **taşındı**; `font_size` tek uçlu olduğu için kaldı.)*
  Dördüncü anahtar altıncı kopyayı doğurur. Çare bir yardımcı
  (`(anahtar, &[(dizge, değer)], varsayılan)`), yeri bir sonraki ayar seti —
  yeni anahtar eklemeden yapılırsa hiçbir davranış değişmez.
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
- **Fare raporlamasının geri kalanı.** 006 yalnız tekerlek kolunu getirdi
  (SGR, UTF-8, düz kodlama). Eksik: tıklama/sürükleme/hareket raporu,
  değiştirici bitleri (bugün `0`), yatay tekerlek, SGR-pixel. Görünen bedeli:
  `mouse=a` açık vim'de tıklama imleci taşımıyor, seçim yapıyor. Tıklama
  raporu seçimle çakışır, yani 006'nın seçim modelini (Karar 1) yeniden açar.
  Kaynak: 006 `phase-3b.md` → Kapsam dışı; 006 `discussion.md` → Karar 4 eki.
- **Tamamlama listesi ızgarayı kaydırıyor.** ZLE'nin `BUFFER` olmayan çıktısı
  (tamamlama listesi, `menu-select`, `bck-i-search`, `zle -M`) aynada yok ve
  ızgaraya düşüyor — 012'nin kayıtlı bedeli. Görünür sonucu **ölçüldü**
  (2026-09-19, PTY koşumu): zsh listeyi satır ilerletmeleriyle
  basıyor, alternatif ekran kullanmıyor, yani ekranı gerçekten kaydırıyor ve
  üstteki çıktı scrollback'e düşüyor; Ctrl-C ile iptalde yalnız `ESC[J`
  gönderiyor ve **hiçbir satırı geri basmıyor**. Yani kaybı hiçbir terminal geri
  getiremez — iTerm de getirmiyor. Farkımız şu: normal terminalde prompt
  hayatta kalan satırın hemen altına dönüyor ve boşluk onun **altında**
  kalıyor; bizde giriş satırı dock'ta çivili olduğu için boşluk **arada**
  kalıyor ve delik gibi duruyor. Kullanıcı bunu kusur olarak bildirdi
  (2026-09-19, dört ekran görüntüsü). **Gerçek çare listeyi ızgaraya hiç
  düşürmemek**: aynanın altıncı kanalı ya da bir overlay. `content_rows`'u
  oynatmak çare değil — denendi ve deliği yalnız yer değiştirdi
  (commit geri alındı).
- **Klavye kalanları.** Home/End bilerek yutuluyor (terminfo `khome`/`kend`,
  oklar gibi DECCKM'e bağlı; yutmayı bağlayan sınama duruyor). Değiştiricili
  oklar (`\e[1;5A` vb.) yok. Ctrl+Shift+Tab `0x19` (readline/zle `yank`)
  gönderiyor, Ctrl+numpad Enter aynı yapıdan `0x03` — ikisi de gerçek
  klavyede doğrulanmadı; aday çare `charactersIgnoringModifiers`'ı
  `encode_key`'e geçirmek. Kaynak: 006 `phase-4d.md` → Uygulama Notları ve
  orkestratör kararı (WAIVE (1)); 006 `phase-3b.md` → Kapsam dışı.
  **2026-09-19'da sete bağlandı → 016** (yukarıdaki tablo). Madde burada
  kalıyor çünkü set henüz açılmadı; açılınca ayrıntısı `context.md`'sine taşınır
  ve burası tek satıra iner. 016 bunun üstüne kullanıcının asıl isteğini
  ekliyor: Option'ın Meta olması ve Cmd'nin adı konmuş bir izin listesiyle
  terminale ulaşması.
- **Yerel ara kolu.** Kabuğun dili `preferredLanguages`'ın yalnız dil alt
  etiketinden alınıyor, etiketin kendi bölgesi atılıyor: `en-GB` dili + `TR`
  bölgesi → `en_TR` kurulu değil → `LANG=en_US.UTF-8`, oysa `en_GB.UTF-8`
  kurulu. `{dil}_{etiket bölgesi}` ara kolu bir ürün kararı; düşüş `en_US`
  olduğu için etkisi görünür (ABD tarih biçimi). Kaynak: 006 `phase-4c.md` →
  `/code-review` WAIVE (5) ve orkestratör kararı.
- **Küçük hijyen.** `make kur` boş hedef dizinini denetlemiyor — bugün
  zararsız. Pano sınamaları oluşturdukları geçici panoları bırakmıyor
  (`releaseGlobally` yok) — kullanıcıya görünmez. İkisi de doğrulanmadı;
  kullanıcı 006'ya almadı. Kaynak: 006 `phase-4c.md` → `/code-review` (8),
  (9); 006 `discussion.md` → Kapsam eki.
- **Paket dumanı.** Paketten açılan duman/ölçüm koşusu bugün elle yazılan bir
  `open` komutu; bir `Makefile` hedefi `docs/OLCUMLER.md` → `## Nasıl yeniden
  ölçülür`'deki dört tuzağın üçünü tasarımla kapatır. Aynı işe bir cümle
  daha: `report_and_exit`'in doc'u kapı düşünce jeton satırının basılmadığını
  ve tanının stderr'e gittiğini söylemiyor. Kaynak: 006 `phase-5.md` →
  `/simplify` takip önerisi ve orkestratör kararı.
