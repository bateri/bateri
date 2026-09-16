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
| 011 | Input Dock (+ yazma animasyonları) | **Zincirin en ucu, kısayolu yok.** Pencere altında sabit ayrı satır editörü; zsh ZLE kancalarına, OSC 133'e ve komut bloklarına birden oturuyor. Ayrıca yazmayı devralan uygulamaları (Claude, Codex, REPL) davranıştan tespit edip alanı geri vermesi gerekiyor — bu, blokların çalışıyor olmasını varsayar. Tuş vuruşu ve silme animasyonları (`keypress`, `delete_mode`) burada: "bu harfi kullanıcı mı yazdı?" sorusunun kesin cevabı dock'ta, ızgarada yalnız tahmin |
| 012 | materyal yüzey | `substrate` shader'ı, grain/sheen, birkaç materyal; 007'nin tema rollerine bağlanır. Metalterm'in görünüşü kapalı kaynak — adlarını biliyoruz (`docs/ARASTIRMA.md` → Görünüm), matematiğini bilmiyoruz; `/rfc`'nin ilk işi referans görüntü/video toplamak ve tasarım denemesi. Efekt GPU bütçesi yer: kare süresi tabanı (`/measure`, bugün sayısı yok) bu setten **önce** alınmış olmalı, yoksa "materyal ne kadar yavaşlattı" cevapsız kalır. *(2026-09-16'da ertelendi; gerekçesi değişmedi, yalnız sırası — ve ertelenmesi ölçüm baskısını da erteledi.)* |
| 013 | emoji + geniş glyph + kutu çizim | Üçü tek iş: 003 `teslim.md` B.3 "geniş karakter tek yuvaya kırpılıyor" diyor, 004 `plan.md` ikisini aynı sete bağlıyor. İçinde gerçek bir mimari çatal var: atlas `R8Unorm`, yani tek kanallı **kapsama maskesi**; emoji ise renkli bitmap. İkisi aynı dokuda yaşayamaz → ikinci atlas mı, RGBA mı, sprite başına format bayrağı mı? `/rfc` şart. TUI'ler (htop, tmux, lazygit) bu setten sonra düzgün görünür. **Bedel:** 2026-09-16'daki ikinci kaymayla TUI çerçeveleri **beş set** boyunca bozuk görünür — bilerek |
| 014 | sekme + bölme | 2026-09-16'da ertelendi. **Bedeli kayıtlı:** komut blokları ve Input Dock "bir pencere = bir oturum" varsayımıyla inecek, bu set onları retrofit eder |

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

## Sete bağlanmamış borçlar

Bunlar kendi setlerini hak etmiyor; yukarıdaki setlerden birine yamanırlar.
Yamandıkları yer belli olunca buradan silinip o setin dosyasına geçerler.

- **Çıpa kaybının iki kolu — bash/fish setiyle birlikte.** İkisi de 010'un
  kapısında (`/code-review`) çıktı ve ikisi de sarmalayıcının sözleşmesine
  dokunuyor, o yüzden çaresi betiklerin doğduğu sete ait.
  **(a) `exec zsh` sonrası pay kalıcı vurgu rengi:** `preexec` `C` basıyor,
  yeniden doğan kabuk kullanıcının `ZDOTDIR`'ını miras alıp sarmalayıcıyı
  yüklemiyor, `D` hiç gelmiyor ve `Session::frame`'in çıpasız kolu "komut
  koşuyor" diye bütün payı boyuyor. Çare sarmalayıcının exec'i takip etmesi
  (`BATERI_ZDOTDIR`'ı koruyup yeniden dikmek). Kayıt `resolve_blocks`'un
  "bilinen sınır" bloğunda.
  **(b) `psvar[9]` geç kayıt olan bir `precmd` hook'uyla silinebilir:**
  `add-zsh-hook` sırası yalnız `.zshrc` koşarken kayıt olanlar için garanti;
  zsh-defer ya da p10k'nın instant-prompt sonu gibi ilk prompt'ta kayıt olan
  bir hook `psvar=(…)` yazarsa `%9v` boşa genişler ve bütün çıpalar sessizce
  düşer (blok yok, şerit yok — yanlış çizim değil). Çare prompt'taki genişlemeyi
  `psvar`'dan çıkarmak, yani R1.1'in `psvar` kararını yeniden açmak.
- **Blok animasyonları.** 010 Karar 5 şeridin belirmesini setten **çıkardı**:
  şerit bugün anında beliriyor. Gerekçe animasyonun zorluğu değil, bedeli —
  `bt-gpu::motion` ikinci bir tüketici kazanır, `Mode::Fade`'in "indirgemenin
  tek yeri" kuralı ikinci bir yer bulur ve boşta sıfır kare kapısı şeridin de
  durma koşulunu sormak zorunda kalır. Şerit animasyonsuz olduğu sürece o
  kapının koruduğu şey bu yoldan tehdit altında değil. Yamandığı yer belli
  değil: hareketin ikinci tüketicisi (yumuşak kaydırma, 008'in artığı) geldiğinde
  aynı sete girmesi doğal olur.
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
- **Ayar ayrıştırmasının beş kopyası.** `osc52`, `cursor_motion` ve
  `reduce_motion` aynı "şu üç dizgeden biri, değilse tanı bırak ve
  varsayılana düş" örüntüsünü elle tekrarlıyor; her enum'un `name()`'i de
  ayrıştırıcının kollarıyla **elle** eşleşiyor (`/code-review`, 008 kapısı).
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
- **Klavye kalanları.** Home/End bilerek yutuluyor (terminfo `khome`/`kend`,
  oklar gibi DECCKM'e bağlı; yutmayı bağlayan sınama duruyor). Değiştiricili
  oklar (`\e[1;5A` vb.) yok. Ctrl+Shift+Tab `0x19` (readline/zle `yank`)
  gönderiyor, Ctrl+numpad Enter aynı yapıdan `0x03` — ikisi de gerçek
  klavyede doğrulanmadı; aday çare `charactersIgnoringModifiers`'ı
  `encode_key`'e geçirmek. Kaynak: 006 `phase-4d.md` → Uygulama Notları ve
  orkestratör kararı (WAIVE (1)); 006 `phase-3b.md` → Kapsam dışı.
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
