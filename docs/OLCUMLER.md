# Ölçümler

Depodaki ölçülmüş sayıların **tek sahibi** bu dosyadır. Başka belge sayı
kopyalamaz; niteliksel anlatır ve buraya bağlanır. Kuralın istisnaları kuralın
sahibinde yazılı (`/audit` → Ölçüm sahipliği).

Ölçüm bir **kapı değildir** (`.claude/is-akisi/proje.md` → Doğrulama): gerçek
pencere, sessiz makine ve dakikalar ister. Kullanıcı ister, `/measure` koşturur.

Dosya 006 phase-5'te kuruldu ve bugün **beş** ölçüm taşıyor: boşta kare,
atlas yuva ayak izi, kare süresi, açılış ve wgpu denemesinin iki arka uçlu
offscreen karşılaştırması. Kare süresi ile açılış 2026-09-21'de girdi,
wgpu denemesi 2026-09-28'de (040 phase-2). Kare süresinin **CPU sütunları** ile açılış taban; **GPU sütunu
değil** — aynı kaynak ve bayt bayt aynı shader ikilisiyle 2,7 kat dolaştı ve
sınanan dört hipotezin hiçbiri onu ayıramadı (`## Kare süresi` → GPU
sütununun gezintisi). Bellek, giriş gecikmesi ve bench bölümlerinde sayı
yok; hangisinin kancası olduğu `/measure` skill'inin tablosunda.

**Jeton anahtarları 2026-10-01'de İngilizceye çevrildi** (değerler aynı,
sözleşme aynı: silinmez, eklenir). Bu dosyadaki o tarihten önceki satırlar
eski anahtarlarla kayıtlı ve öyle kalıyor; karşılıkları:

| eski | yeni | eski | yeni |
|---|---|---|---|
| `kare` | `frames` | `sessiz` | `quiet` |
| `hucre` | `cells` | `kapanis` | `teardown` |
| `glif` | `glyphs` | `profil` | `profile` |
| `kural` | `rules` | `ornek` | `samples` |
| `yuva` / `yuva2` | `slots` / `slots2` | `dusen` | `dropped` |
| `yuk` | `load` | `gpu_ornek` | `gpu_samples` |
| `istek` | `requests` | `gpu_elenen` | `gpu_discarded` |
| `icerik` | `content` | `taban` | `floor` |
| `hareket` | `motion` | `acilis` | `startup` |
| `kayma` | `slide` | `cpu_kare_*` | `cpu_frame_*` |
| `arka_uc` | `backend` | `ATLANDI` | `SKIPPED` |

Make hedefleri de aynı gün çevrildi (`duman` → `smoke`, `hepsi` → `check`,
`kur` → `bundle`, …); tam liste `CLAUDE.md` → Komutlar'da.

## Yöntem

Önce yöntem, sonra sayı: yöntemsiz bir sayı sonraki ölçümle karşılaştırılamaz.

### Her ölçümde

- **Ortam kayda girer:** makine, işletim sistemi, güç kaynağı (prizde mi,
  düşük güç kipi), ekranın tazeleme hızı, pencere boyutu, ölçülen commit ve
  profil (`profil=` jetonu satırın içinde söylüyor). 120 Hz ile 60 Hz'in kare
  bütçesi farklıdır, iki farklı pencere boyutu aynı sayı değildir.
- **Sessiz makine:** tarayıcı ve IDE indeksleyici kapalı, kullanıcı makineye
  dokunmuyor. Duman penceresi öne çıkıyor; koşu sırasında basılan bir tuş o
  pencereye düşebilir — phase-4c'nin `glif=` sapmasının kanıtsız hipotezi
  bu.
- **Yolun ateşlendiğini göster.** Boşta duran pencerede kare süresi ölçmek
  sıfır verir, uygulanmamış bir değişikliği ölçmek eski sayıyı verir. Sayıdan
  önce yolun koştuğunu gösteren bir sinyal alınır: sayaç, kasıtlı bozma ile
  değişen sonuç, pencerenin ekranda olduğunu gösteren bir yoklama.
- **Dağılım yazılır, ortalama değil.** Her değerin kaç kez görüldüğü ve uçlar.
  Bir takılma ortalamayı oynatmaz ama kullanıcı onu görür.
- **Tam jeton satırı saklanır.** Koşular arasında sabit kalan jetonlar bir kez,
  oynayanlar koşu başına yazılır; hiçbir jeton düşürülmez.
- **Zaman ölçümü release ister.** Debug derlemesinin süresi taban değildir.
  Sayım ölçümlerinde (aşağıda boşta kare) iki profil **ayrı** tutulur ve
  hangisinin kapıya bağlandığı gerekçesiyle yazılır.

### Gürültü kuralı

- Bir türün koşu sayısı **profil başına en az on**dur; iki koşu yalnız
  "yol çalışıyor mu" sorusunu cevaplar.
- Duman reçetesinin sabit sayaçları (`hucre=8 glif=6 kural=15 yuva=13/2048`)
  beklenenden sapan bir koşu, **ayıklanmadan önce** sapmanın nedeni yazılarak
  kayda girer. Nedeni bulunamayan koşu ayıklanmaz, dağılımda kalır.
- Önceki ölçümle karşılaştırırken tek bir koşu değil iki dağılım yan yana
  konur; bir uçtaki tek gözlem, öbür dağılımın ortasına düşüyorsa fark sayılmaz.

### Boşta kare (`IDLE_FRAME_LIMIT` ve `QUIET_FLOOR`)

`make smoke` yükünde (`Workload::Smoke`) pencere ilk çizimden sonra boşta
durur ve kapı **iki** ölçülmüş sayıya bakar: çizilen içerik karesi bir üst
sınırın altında (`icerik ≤ IDLE_FRAME_LIMIT`), son kareyle deadline
arasındaki sessizlik bir alt sınırın üstünde (`sessiz ≥ QUIET_FLOOR`,
`sessiz=none` de kırmızı) olmalı. İkisi de aynı iki dağılımdan türer:

- **Sağlıklı:** değiştirilmemiş HEAD. Profil başına en az on koşu, `BT_RUN_SECONDS=3`
  (kapının koştuğu süre), kıyas için birkaç `5` saniyelik koşu.
- **Bozuk:** boşta sıfır kareyi kasıtlı bozan geçici bir mutasyon. Profil
  başına en az üç koşu. Mutasyon **commit'e girmez**; geri alındığı `git diff`
  boşluğuyla gösterilir ve iki derleme de geri alındıktan sonra yenilenir
  (yoksa `target/` altında bozuk bir paket kalır). Üç mutasyonun üçü de ayrı
  bir sızıntı sınıfını temsil ediyor ve gövdeleri "Nasıl yeniden ölçülür"de:
  **hızlı sızıntı** (her karede hasar), **yavaş sızıntı** (yarım saniyede bir
  kare talebi) ve **durma koşulu** (`settled()` hep `false`).

**Üst sınırın kuralı:** en yüksek sağlıklı gözlemin en az iki katı ve en düşük
bozuk gözlemin altında. İki koşul çelişirse sınır oynatılmaz, iş durur:
sağlıklı dağılım bozuk dağılıma yaklaştıysa kusur sayıda değil koddadır. Sınır
bu iki koşulun **zorladığı** kadar oynar, fazlası değil: bozuk dağılımın
altında kalan her büyütme kuralı sağlar ama kapının algılama tabanını
yükseltir (yavaş bir sızıntı yeşil geçer; bkz. `IDLE_FRAME_LIMIT`'in doc'u).

**Alt sınırın kuralı ters yönde işler** ve bu jetonun bütün farkı orada:
sağlıklı koşuda `sessiz` **büyük**, bozuk koşuda küçük. Taban "en düşük
sağlıklı gözlemin en çok yarısı ve en yüksek bozuk gözlemin üstünde" ve
aralığın **en büyük** ucundan seçilir — üst sınırda büyütmek kapıyı
körleştirirken burada duyarlılığı **artırıyor**: yakalanan en yavaş sızıntının
periyodu ≈ tabanın kendisi, yani ortadan seçilen bir sayı kapıyı boşuna
kısıtlar.

**Dört sayı tek bloktan okunur ve birlikte oynar:** `BT_RUN_SECONDS`'ın 3'ü,
`bt_core::smoke_shell`'in 1 saniyelik uykusu, aynı reçetenin **imleç sıçrama
mesafesi** (bugün bir sütun, `\033[2G`) ve `QUIET_FLOOR`. Kuyruk yapısal olarak
`koşu süresi − (uyku + yerleşme)`; yerleşme ~0,25 sn olduğu için 3 saniyelik
koşuda ~1,75 sn. Süreyi 2'ye indiren, uykuyu uzatan **ya da mesafeyi büyüten**
biri tabanı da yeniden türetmek zorunda, yoksa kapı kod doğruyken düşer.
Dördüncüsü 011 phase-0'da keşfedildi ve en sinsisi: yay uzak sıçramayı daha
uzun uçuruyor, yerleşme uzuyor ve kuyruktan yiyor — üç sütunla ölçülen koşuda
kapı **hâlâ yeşildi**, yani ihlal jetonun arkasında saklanıyordu. Dördü dört
dosyaya dağılırsa biri oynadığında kapı sessizce kırılganlaşır.

Kapı debug'a bağlıdır (gerekçesi `IDLE_FRAME_LIMIT`'in doc'unda), ama release
paketi de aynı sınırlara tabi olduğu için onun dağılımı da ölçülür: sayılar
iki profili birden taşımalıdır.

### Kare süresi ve açılış

Yöntem 2026-09-21'de kuruldu; kancanın (`BT_FRAME_STATS`) dürüst sınırları o
gün `crates/bt-shell-macos/src/app.rs`'teki `Measured`'ın doc'undan **buraya
taşındı** ve sahibi artık burası. Her kalem **kapsam** ya da **açık kalem**
diye etiketli, çünkü okuyanın yapacağı şey farklı: kapsam bilinip geçilir,
açık kalem eylem bekler.

**Koşunun şekli:** release binary doğrudan çağrılır (`.app`'in `open`'ı
değil — o yol boşta kare ölçümünün LaunchServices sorusuna ait),
`BT_FRAME_STATS=1 BT_SCROLL_TEST=1`, **10 saniye** ve **10 koşu**. İki sayının
gerekçesi ayrı: süre örnek sayısı içindir ve 10 saniye ~1200 örnek veriyor,
yani p95'in tabanının (`MIN_SAMPLES` = 20) altmış katı — 5 saniye de (592
örnek) yetiyordu, 30 saniye tören olurdu. Koşu sayısı **turlar arası
sapma** içindir ve oradaki kural bu dosyanın kendi gürültü kuralı (profil
başına en az on).

- **Kapsam — `acilis=` iki ucundan da kısa.** Başı `main()`'in ilk satırı,
  süreç başlangıcı değil; sonu ilk **tamamlanan** kare
  (`addCompletedHandler`), sunulan kare değil. İkisi de
  `bt_gpu::Stats::startup`'ta yazılı. Ölçüm halkalarının ayrılması bu
  aralığın **içinde** kalıyor.
- **Kapsam — düşen kare ölçülmüyor** (005 R3, kapsam dışı). `dusen=` halkaya
  sığmayan **örnek**, atlanan kare değil; kuralı `bt_gpu::Samples`'ın
  doc'unda.
- **Kapsam — `ornek=` ile `gpu_ornek=` aynı kare popülasyonunu saymıyor.**
  Hareket karesi CPU örneği yazmıyor (o karede `session.frame` hiç koşmuyor,
  sahte örnek p95'i aşağı çekerdi) ama bir komut tamponu commit ediyor, yani
  GPU'nun tamamlanma bloğu onu **görüyor**. Ayrılık yapısal: aynı blok
  `FailureStreak`'i de besliyor ve hareket karesini ondan muaf tutmak çizim
  hatasını görünmez kılardı. Sonucu, imleç kayan bir koşuda iki sütunun p95'i
  **doğrudan karşılaştırılamaz**; gerekçesi `bt-gpu/src/link.rs`'te hareket
  karesinin gövdesinde.
- **Kapsam — wgpu pencere yolunda (040 phase-5'ten beri) drawable alımı CPU
  aralıklarının dışında, `present` içinde.** Tik dokuyu wgpu yüzeyinden
  `session.frame`'den **önce** alıyor (bekleme `cpu_kare`'ye girmiyor); Metal'in
  aralıkları da `nextDrawable`'ı görmüyordu (display link drawable'ı callback'ten
  önce veriyordu). `cpu_encode` plan + submit + `queue.present` — Metal'inki
  `presentDrawable`'ı taşıyan tamponun `commit`'ine kadardı. Uyuyan tik drawable
  **hiç** almıyor; bu, `CAMetalDisplayLink`'in her tikte drawable almasının
  tersi ve aralıklarda görünmüyor.
- **Kapsam — wgpu'nun GPU sütunu başka bir saat.** `gpu_*` 040 phase-5'ten beri
  `TIMESTAMP_QUERY`'nin pass başı/sonu damgasından (desteklenmiyorsa
  `unsupported`); Metal'in `GPUStartTime`/`GPUEndTime`'ı komut tamponunun
  tamamını kapsıyordu. İki dönemin `gpu_p95`'i birebir karşılaştırılmaz.
- **Açık kalem (jeton boşluğu) — CPU'nun elenen örneği sayılıyor ama
  basılmıyor.** `Stats::record_cpu` sıfır uzunluklu bir aralığı eliyor ve
  `bt_gpu::Samples::rejected`'a yazıyor; rapor bu sayacı yalnız GPU sütunu
  için (`gpu_elenen=`) okuyor. Yani elenen bir CPU örneği `ornek=`'i sessizce
  düşürüyor ve satırda sebebini söyleyen jeton **yok** (005 R5.2). Bugün
  zararsız: eleme yalnız sıfır uzunluklu aralıkta oluyor ve 2026-09-21
  koşularının hiçbirinde görülmedi. Kapatmanın bedeli **makine sözleşmesini
  genişletmek** (`cpu_elenen=`), yani geri alınamaz bir adım — ölçülmüş bir
  ihtiyaç beklemeden atılmadı.
- **Açık kalem (kayıtlı kusur) — `kapanis=abandoned`.** Örneklere etkisi
  **yok**, çünkü `shutdown()` beklemeye girmeden **önce** `link.stop()`
  çağırıyor: bekleme boyunca yeni kare istenmiyor. Bedeli yalnız koşunun
  duvar saatinde (`SHUTDOWN_GRACE` kadar). Kapanış tasarımının borcu; çaresi
  adı konmuş (`Session::spawn`'da master'ın bir kopyası), ayrıntısı
  `CLAUDE.md`'nin kapanış maddesinde. **Sıklığı 2026-09-21'de oynadı:** 2026-09-12
  ölçümlerinde on yedi koşuda dört (~%25), bugün on koşuda **altı**. Sebebi
  aranmadı ve sayı yorumlanmadı — kalem zaten açık.
- **Açık kalem (cevaplanmamış soru) — `kare` ile `istek` iki yükte apayrı
  davranıyor** ve mekanizması **ölçülmedi** (kapı mı yutuyor, ana thread mi
  doyuyor, sistem mi link'i kısıyor): duman yükünde `istek ≈ icerik + 1..2`
  (2026-09-16), ölçüm yükünde ikisi **mertebelerce** ayrışıyor — 2026-09-21'de
  `kare ≈ 1185`'e karşı `istek ≈ 260 000`. Üstüne, ölçüm yükünün kendisi
  **aynı komut ve aynı derlemeyle** iki farklı rejim vermişti: `kare` bir
  koşuda onlarda, başka bir koşuda yüzlerde. **2026-09-21 koşusu o rejim
  çatalını görmedi:** `kare` jetonu saklanan **on bir** koşunun (on ölçüm
  koşusu artı deneme koşusu) on birinde de `kare / süre ≈ 118`, yani
  tazeleme hızı — pencere görünür ve tam hızda. Kalan beş koşunun satırı
  yalnız `gpu_*` için süzüldüğü için `kare` saklanmadı. Rejimi satırdan okumanın
  yolu bu oran; ölçümü yorumlayan taraf onu **koşu başına** yazmalı.
- **Açık kalem — GPU sütunu bu koşumda taban olacak kadar kararlı değil ve
  sebebi bulunamadı.** Aynı kaynak, aynı makine, aynı yük ve **bayt bayt aynı
  metallib** ile `gpu_p95` 2026-09-21'de **0,25 – 0,68 ms** arasında
  dolaştı, yani 2,7 kat. Dizinin tamamı `## Kare süresi` → "GPU sütununun
  gezintisi"nde. İmzası iki parçalı: bir blok **içinde** çarpıcı biçimde
  kararlı (on üç koşu tam `0,68`, on koşu tam `0,63`) ama bloklar arasında
  sıçrıyor. **Dört hipotez sınandı, dördü de ayırmadı:**
  *koşu süresi* (5 sn de 10 sn de aynı değeri verdi), *derlemeden sonraki
  ilk koşu* (yeniden derleme değeri bir kez indirdi, bir kez çıkardı),
  *güç durumu* (`0,63` hem pil %25'te hem priz %82'de; `0,25` hem pil
  %51'de hem priz %86'da — yani **ayırıcı değil**) ve *metallib kimliği*
  (`shasum` yeniden derlemeden önce ve sonra aynı: `9f0a7969…`, sayı yine
  sıçradı — yani shader ikilisi **aklandı**). Geriye bizim binary'mizin
  dışındaki bir şey kalıyor (GPU saat durumu, başka bir GPU tüketicisi,
  compositor) ve oraya bu ölçümün araçları yetmiyor.
  **Sonucu iki tane.** Bir: GPU sütununun tabanı **alınmadı**, çünkü
  gürültüsü ölçülecek çoğu etkiden büyük. İki, ve daha önemlisi: **022
  materyal yüzeyin ölçümü tam da bu sıçramanın üstüne oturuyor** —
  "shader'lı hâl shader'sız hâlden yavaş mı" sorusu doğası gereği bir
  yeniden derleme sınırının iki yanını karşılaştırmak demek ve sıçrama tam
  orada. 022'nin `/rfc`'si ölçme yöntemini **önce** çözmek zorunda: aynı
  binary içinde çalışma zamanı anahtarıyla A/B, ya da çok sayıda yeniden
  derleme üzerinden ortalama. Bunu çözmeden alınacak bir "materyal %X
  yavaşlattı" cümlesi ölçüm değil gürültü olur.

### wgpu denemesi (offscreen, iki arka uç)

040'ın durak kuralının ölçümü (`.tasks/040-linux-kapisi-ve-wgpu/discussion.md`
→ Karar 3): aynı `Frame`, aynı sayıda, bugünkü Metal renderer'ı ve `cfg(test)`
wgpu renderer'ı ile offscreen çiziliyor. Kanca `#[ignore]`'lu bir sınama
(`bt-gpu` → `renderer::wgpu_tests::offscreen_frame_loop`; 040 phase-7'de Metal
renderer'ı söküldü, kanca bugün yalnız `arka_uc=wgpu` satırını basıyor ve
aşağıdaki Metal satırları tarihli kayıttır);
arka uç başına **bir satır** basıyor (`arka_uc=… profil=… kare=… ornek=…
cpu_kare_p95/max cpu_encode_p95/max gpu_p95/max`, değerler mikrosaniye).

- **Aralıklar `Stats`'ın aynısı.** `cpu_kare` karenin kurulması (`Frame::clear`
  + push'lar; iki arka uçta **aynı kod**, yani koşunun gürültüsünün tanığı),
  `cpu_encode` encode + gönderim — Metal'de komut tamponunun kurulmasından
  `commit`'e, wgpu'da `write_buffer`'dan `submit`'e. **Karar veren sütun
  `cpu_encode`.** GPU'nun bitirmesi beklenir ama aralığın **dışında**:
  kareler birbirinin arkasında kuyruğa girmesin.
- **Kare:** 1024×1024 doku, 8×16 hücre, her hücrede zemin (~7 700 dörtlü),
  caret ve iki satırlık dock — o phase'in iki pipeline'ının (`cell_bg` +
  caret) çizebildiği en ağır kare. 1000 kare, önünde sayılmayan 50 ısınma.
- **Arka uçlar kare kare dönüşümlü.** Sıralı koşuda (önce 1000 Metal, sonra
  1000 wgpu) aynı kodun `cpu_kare_p95`'i iki yarıda 63 / 85 µs çıktı, yani
  sıra bir arka uca yazılan bir fark üretiyordu; dönüşümlüde 71,5 / 72,8 µs.
  Dönüşümlü koşu bu yüzden yöntem.
- **GPU sütunu karar vermiyor** (Karar 3): Metal'de `GPUStartTime`/`EndTime`,
  wgpu'da `unsupported` (damga `TIMESTAMP_QUERY` ister, 040 phase-4).
- **Durak:** profil başına on koşu; release `cpu_encode_p95` kümeleri
  **örtüşmüyor ve wgpu'nunki daha kötüyse** set durur ve eskale edilir. Debug
  kaydedilir, karar vermez.
- **Kapsam — iki yarının tampon stratejisi farklı.** Metal yarısı üretimin
  yolu (liste başına kare başına `newBufferWithBytes`), wgpu yarısı kalıcı tek
  tampon + `write_buffer` ve planın kare başına bir ara kopyası. Fark bu
  yüzden "arka uç" ile "tampon stratejisi"nin toplamı; ayrıştırma aşağıdaki
  gözlemde (040 phase-2 `/code-review`).
- **Kapsam — offscreen pencere yolu değil.** Drawable, display link ve
  compositor yok; bu karşılaştırma Metal'in pencere yolu tabanıyla (`## Kare
  süresi`) **karşılaştırılmaz**, o taban 040 phase-5'in geçiş ölçümünün
  karşılığıdır.

### Atlas yuva ayak izi

**Bu tür bir sayımdır, bir süre değil** ve üstteki kuralların üçü ona
uygulanmaz: gürültü eşiği yok (aynı commit aynı sayıyı veriyor), sessiz makine
gerekmiyor, profil ayrımı anlamsız — sayı fontun metriğinden ve ızgara
aritmetiğinden türüyor, zamandan değil. Yine de **iki profilde de koşulur ve
eşitliği yazılır**: eşit olmadıkları gün ortada bir kusur var demektir.

- **Ölçülen şey:** `Atlas::occupancy()`'nin ilk bileşeni (harcanan yuva) ile
  ikincisi (kapasite). Kapasite `floor(kenar / hücre_genişliği) * floor(kenar
  / hücre_yüksekliği)` ve yalnız hücre ölçüsünden türüyor, yani aileyi hiç
  istemeden de okunabiliyor. **Kenar 2026-09-22'ye kadar sabit 1024'tü**;
  aşağıdaki 021 ölçümü o hâlin kaydı ve tarihî olarak doğru. 022'den sonra
  kenar hedeflenen yuva sayısından türüyor (`bt_atlas::SLOT_TARGET`), yani
  aynı hücre ölçüsü daha büyük bir kapasite verebiliyor.
- **Ayak izi bir tavandır, bir maliyet değil:** yuvalar **istendikçe**
  harcanıyor. Bir oturum yalnız çizdiği karakterin yuvasını öder; 421 sayısı
  "bütün aileyi kullanan içerik" hâlidir.
- **Doyma ölçülürken sıra önemlidir.** Prob aileyi çizgi → köşegen → blok →
  Braille → teknik sırasıyla istiyor, yani atlas dolduğunda kırpılan **son
  istenen** küme oluyor (aşağıdaki tabloda Braille). Ölçütün kendisi sıradan
  bağımsız: kapasite ile ailenin boyu karşılaştırılıyor, kırpılma yalnız
  atlasın gerçekten dolduğunun tanığı.
- **Yolun ateşlendiğinin tanığı iki bilinen sayı** (13pt@2x → 1984,
  32pt@2x → 338; ikisi de 021'in planından) ve **taban koşusunun kendisi**:
  021 öncesi commit'te Braille sıfır yuva harcıyor, sonrasında 256 — kapı
  koşmasaydı iki koşu aynı sayıyı verirdi.
- **Taban 021 öncesinin son commit'i** (`b17fa78`). Arada 021'in iki phase'i
  **ve** yedek glyph kapısının mürekkebe dönmesi (`bc4d451`) var; teknik
  kümenin 6 → 8 ile kök kuyruğunun 0 → 1 hareketi o ikinci değişikliği de
  taşıyor, blok/çizgi/Braille ise yalnız 021'i.

**İkinci tür: kutu envanteri** (023, 2026-09-22). Aynı bölümün altında,
çünkü ikisi de bir **sayım** ve ikisi de fontun metriğinden türüyor — ama
soru farklı: "hangi karakterler kutu çıkıyor ve **neden**".

- **Ölçülen şey, karakter başına dört değer:** taban fontta glyph var mı;
  yoksa cascade hangi adayı veriyor (`CTFont::for_string`); adayın
  ilerlemesi ve **mürekkebi** hücrenin kaç katı; ve bugünkü kapı
  (`font::fallback_font`) onu kabul ediyor mu. Sütun sayısı Unicode'un
  `East_Asian_Width`'inden (`W`/`F` → 2), yani ızgaranın `unicode-width`
  üzerinden kullandığı ölçütle **aynı kaynak** — ayrı bir tablo ikinci bir
  genişlik yetkilisi olurdu.
- **Kapsam — oran ölçekten bağımsız, mutlak sayı değil.** "1.66×" bu
  makinenin Menlo'su için tek bir değer; başka bir ailede ya da başka bir
  sistem sürümünde cascade başka bir aday verir. Karara giren şey oranın
  **2.0'ın altında olması**, sayının kendisi değil.
- **Prob depoda durmuyor**, yukarıdaki kuralın aynısı: `bt-atlas`'ın
  içine geçici bir `#[ignore]` sınaması olarak yazılıp koşuldu ve
  `git checkout` ile alındı. `pub(crate)` iç yollara (`font::fallback_font`,
  `glyph_ink`) erişmesi gerektiği için crate'in **dışından** koşamıyor.

## Nasıl yeniden ölçülür

### Ortam

```sh
system_profiler SPDisplaysDataType SPHardwareDataType   # makine, ekran
sw_vers; rustc --version
pmset -g batt; pmset -g | grep lowpowermode             # güç kaynağı, düşük güç kipi
```

Tazeleme hızı `system_profiler`'da görünmüyor (ProMotion ekranda değişken);
en çok değeri `NSScreen.main.maximumFramesPerSecond` verir, koşudaki gerçek
hızın dolaylı kanıtı ise bozuk koşunun saniye başına karesidir.

### Kare süresi ve açılış

Kanca ortamdan açılıyor; ikisi de **sıfırdan büyük** bir `BT_RUN_SECONDS`
ister, yoksa süreç çıkış 1 verir. `.app` değil **binary** koşuyor: `open`
yolu boşta kare ölçümünün LaunchServices sorusuna ait ve çıkış kodunu
yutuyor.

```sh
cargo build --release -p bateri
for i in $(seq 1 10); do
  BT_FRAME_STATS=1 BT_SCROLL_TEST=1 BT_RUN_SECONDS=10 \
    ./target/release/bateri > "target/olcum-kare/run-$i.out" 2>&1
  grep -h '^kare=' "target/olcum-kare/run-$i.out" | tail -1
done
```

Koşmadan önce `pmset -g batt` ile güç kaynağı **prizde** olmalı ve kayda
girmeli. Güç durumunun CPU sütunlarını ve açılışı **etkilemediği** 2026-09-21'de
ölçüldü (iki blok yan yana, `## Kare süresi`), ama kural kalıyor: ölçülmüş
olan bu yük ve bu makine, bütün yükler değil.

**GPU sütununu yorumlamadan önce bir yoklama.** Sayı bloklar arasında
sıçrıyor ve dört hipotez onu ayıramadı (`## Yöntem`, yedinci kalem). Yeniden
derleme sınırının iki yanını karşılaştıran her ölçüm önce shader ikilisinin
gerçekten değişip değişmediğini sormalı:

```sh
shasum -a 256 target/release/build/bt-gpu-*/out/default.metallib
```

2026-09-21'de bu hash yeniden derlemeden **önce ve sonra aynıydı** ve
`gpu_p95` yine sıçradı — yani sıçramanın kaynağı shader değil. Aynı hash
üstünde iki farklı sayı görüyorsan ölçtüğün şey kod değil ortamdır.

Her koşunun **tam jeton satırı** saklanır. Yorumlamadan önce üç yoklama:
`ornek` ile `gpu_ornek` tabanın (`taban=`, bugün 20) üstünde mi,
`insufficient` var mı, ve `kare / süre` tazeleme hızına yakın mı — sonuncusu
rejim tanığı (`## Yöntem`, altıncı kalem).

### wgpu denemesi

Ölçüm pencere açmıyor; sınama binary'si koşuyor. Koşu başına bir satır
(`arka_uc=wgpu`; Metal yarısı 040 phase-7'de söküldü, iki arka uçlu yoklamalar
yalnız tarihli kayıtlar için):

```sh
mkdir -p target/olcum-wgpu
for i in $(seq 1 10); do
  cargo test --release -p bt-gpu offscreen_frame_loop -- --ignored --nocapture \
    | grep '^arka_uc' >> target/olcum-wgpu/release.txt
done
# debug için aynı döngü `--release` olmadan (device kurulumu bu makinede
# koşu başına ~30 sn sürüyor — ortam, kod değil)
```

Yorumlamadan önce iki yoklama: `ornek=1000` (her iki arka uçta) ve
`cpu_kare`'nin iki arka uçta örtüşmesi — örtüşmüyorsa koşu koşulları arka
uçlar arasında ayrışmıştır ve `cpu_encode` farkı yorumlanamaz.

### Atlas yuva ayak izi

Prob **depoda durmuyor**: ölçüm bir kapı değil ve `tests/` altında kalan bir
dosya `make check`'nin her koşusunda derlenirdi. İki çalışma ağacı açılır
(ölçülen commit ve taban), aynı prob ikisine kopyalanır, koşulur ve silinir:

```sh
git worktree add /tmp/wt-head <ölçülen-commit>
git worktree add /tmp/wt-base <taban-commit>
# prob: crates/bt-atlas/tests/atlas_probe.rs — `Atlas::new(None, punto, ölçek, 1.0)`,
# her aralığı `Atlas::slot(Sprite::Char(ch), Face::Regular, SizeClass::Normal)` ile
# isteyip `occupancy()` farkını basar. Gövdesi bu bölümün altındaki tabloyu üretir.
for wt in /tmp/wt-head /tmp/wt-base; do
  (cd $wt && cargo test --release -p bt-atlas --test atlas_probe -- --ignored --nocapture)
done
git worktree remove /tmp/wt-head; git worktree remove /tmp/wt-base
```

Çalışma ağacı şart değil ama **kirli ağaçta ölçüm yapılmaz**: 2026-09-21
koşusunda depoda başka bir oturumun commit'lenmemiş değişikliği vardı ve prob
ayrı ağaçlarda koştuğu için ona hiç değmedi.

### Boşta kare

Sıra: **önce bozuk kol**, sonra geri alma, sonra sağlıklı kol. Böylece geri
almadan sonraki derleme sağlıklı kolun derlemesi olur (fazladan derleme yok)
ve sağlıklı dağılım geri almanın tuttuğunu `git diff`'in boşluğuna ek olarak
gösterir. (2026-09-15 koşusu sağlıklı kolun yarısını mutasyondan önce koştu;
iki yarı aynı dağılımı verdi.)

İki kol aynı iki döngüyü koşar, yalnız `ARM` ve `N` değişir — dosya adı kolu
taşıdığı için sağlıklı kol bozuk kolun kanıtını ezmez (bozuk paket koşusunun
tek kaydı `.err` dosyası):

```sh
ARM=broken N=3     # bozuk kol; sağlıklı kolda: ARM=healthy N=10

cargo build -p bateri   # derleme süresi yoklamanın bekleme payına girmesin
for i in $(seq 1 $N); do  # debug — kapının kendi tarifi
  make smoke 2>&1 | tee "target/duman-$ARM-debug-$i.out" | grep -E 'kare=|bozuldu'
done

make bundle
for i in $(seq 1 $N); do  # release paket — LaunchServices yolu
  env -u BT_SCROLL_TEST -u BT_FRAME_STATS open -W -n --env BT_RUN_SECONDS=3 \
    --stdout "$PWD/target/duman-$ARM-release-$i.out" --stderr "$PWD/target/duman-$ARM-release-$i.err" \
    "$PWD/target/release/bateri.app"
done
```

**040'tan beri debug kolunun şartı: pencere ön planda olmalı.** wgpu
örtülü pencereye drawable vermiyor (040 phase-5 → Uygulama Notları), yani
`cargo run`'ın açtığı pencere başka bir uygulamanın penceresinin arkasında
kalırsa koşu `kare=0` ile kırmızı düşer — kod doğruyken. `cargo run`
etkinleşme almıyor (çağıran kabuk ön planda değilse macOS etkinleşmeyi
vermiyor); `open` veriyor. Ön plandaki uygulama bir başkasıysa debug kolu
aynı `open` döngüsüyle, debug binary'si taşıyan geçici bir paketten koşar
(kimliği `dev.bateri.duman`, URL şeması **silinmiş** — kullanıcının
`bateri://`'si ona gitmesin; binary kopyalandıktan sonra `codesign -f -s -`).
Yoklamanın `front` sütunu hangisinin gerektiğini söylüyor.

Bozuk kolun **üç** mutasyonu var; üçü de `git checkout` ile geri alınır ve
`git diff` boş kalır. Hangisinin hangi kapıyı ateşlediği ölçümün kendi
kaydında, gövdeleri burada:

1. **Hızlı sızıntı** — `crates/bt-gpu/src/link.rs`'te `Core::tick`'in
   içerik kolunun sonuna, `self.notice_alt_screen();`'dan sonra koşulsuz
   `self.waker.wake();` (040'tan önce: `needs_update`'in sonu, `match drawn
   { … }`'den sonra). Her çizilen kare hasar diker, yani sıradaki tik'te de
   hasar bulunur: tazeleme hızında **içerik** karesi. **Uyarı — 008'de
   düşürdüğü kol değişti:** kalıcı hasar "hasar yok" dalını hiç
   çalıştırmıyor, yani `hareket=0` ve kapı `ExcessFrames`'ten **önce**
   `MissingCounter` diyor. Satırdaki `içerik karesi` sayısı yine de okunuyor
   (tanı onu basıyor).
2. **Yavaş sızıntı** — aynı dosyada, `motion_tick`'in `at_rest` dalında
   `self.waker.pacer().set_running(false)`'dan **önce** (040'tan önce: "hasar
   yok + yerleşti" dalında `link.setPaused(true)`'dan önce,
   `DispatchQueue::main().after` ile):

   ```rust
   let w = self.waker.clone();
   self.waker.pacer().after(std::time::Duration::from_millis(500), Box::new(move || w.wake()));
   ```

   Yarım saniyede bir içerik karesi: üç saniyede `icerik=7`–`8`, yani **üst
   sınırı aşmıyor** — bu kolu yalnız `sessiz` görüyor ve `QUIET_FLOOR`
   inmeden önce yeşil geçiyordu.
3. **Durma koşulu** — `crates/bt-gpu/src/motion.rs`'te `Motion::settled`'ın
   gövdesine `&& false`. Animasyon hiç yerleşmez: `Verdict::MotionUnsettled`.

Her mutasyondan sonra `git checkout -- {dosya}` ve `git diff` boş. Sağlıklı kolun
5 saniyelik kıyası aynı döngülerde `BT_RUN_SECONDS=5` ile koşar (debug'da
`make smoke` yerine Makefile tarifinin aynısı:
`env -u BT_SCROLL_TEST -u BT_FRAME_STATS BT_RUN_SECONDS=5 cargo run -q -p bateri`).

Paket yolunun dört tuzağı (profil ayrımı yukarıda, `## Yöntem`'de):

1. `open`'ın çıkış kodu uygulamanınki **değil**, hep 0. Karar jeton
   satırından okunur.
2. `open` çağıranın ortamını geçiriyor; `env -u` hermetikliği burada da şart.
3. `--stdout`/`--stderr` yolu **mutlak** olmalı: LaunchServices süreci
   `cwd=/` ile başlatıyor.
4. `--stderr` **şart**: kapının düştüğü koşu jeton satırı basmıyor, tanı
   satırı ("boşta sıfır kare bozuldu — …") stderr'e gidiyor. Yalnız
   `--stdout` verilirse bozuk bir paket koşusu boş bir dosya bırakır ve
   hiçbir yerde görünmez.

Pencere görünürlüğü yoklaması — koşu sürerken ayrı bir süreçte, pencere
listesinden sahibi `bateri` olan katman-0 penceresinin `kCGWindowIsOnscreen`
değeri ve öndeki uygulama okunur:

```swift
import AppKit
Thread.sleep(forTimeInterval: 1.5)   // pencerenin açılmasına zaman tanı
let front = NSWorkspace.shared.frontmostApplication?.localizedName ?? "?"
let all = CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as? [[String: Any]] ?? []
for w in all where (w[kCGWindowOwnerName as String] as? String) == "bateri"
                && (w[kCGWindowLayer as String] as? Int) == 0 {
  print(w[kCGWindowIsOnscreen as String] ?? false, w[kCGWindowBounds as String] ?? "", front)
}
print(NSScreen.main?.maximumFramesPerSecond ?? -1)
```

`swiftc -o probe probe.swift`. Ayrı bir koşu açmaz: iki döngünün **ilk**
turunda komutun yanında koşar ve o koşu `n`'e sayılır — debug'da
`(make smoke … & ./probe; wait)`, pakette `(env … open -W … & ./probe; wait)`.
Sahibi `bateri` olan, ekran genişliğinde ve 33 pt yüksekliğinde ekran dışı
pencereler de listede çıkıyor; ana pencere onlar değil (ne oldukları
doğrulanmadı), boyutundan tanınır.

## Boşta kare

**Üst sınır: `icerik ≤ 8`** (`crates/bt-shell-macos/src/app.rs` → `IDLE_FRAME_LIMIT`).
**Alt sınır: `sessiz ≥ 868 ms`** (aynı dosya → `QUIET_FLOOR`; 2026-09-17'de
870'ten indirildi, gerekçe aşağıda).

### 2026-09-30 — wgpu pencere yolu, iki sınır yeniden gözlendi (040 phase-6)

Neden: 040 phase-5 kare yolunu değiştirdi — sessizliğin saati artık
`Pacer`'ın damgası, kare sayımı gönderim indeksi + `poll`, drawable yalnız
çizen tik'te. İki sınırın doc'u "kare yolunu değiştiren set gelince yeniden
ölç" diyor. **İki sınır da değişmedi**: sağlıklı dağılım ikisinin de doğru
tarafında, üç bozuk kolun üçü de kırmızı.

**Ortam**

| | |
|---|---|
| commit | `b234946` (çalışma ağacı temiz; mutasyonlar yalnız bozuk kolda, her birinden sonra `git diff` boş) |
| makine | Apple M1 Pro, macOS 26.4.1 (25E253); rustc 1.88.0 |
| ekran | 120 Hz (`maximumFramesPerSecond`); pencere 900×632 pt, `kCGWindowIsOnscreen = 1`, ön planda koşunun kendisi |
| kullanıcı | makine kullanımdaydı (Safari ön plandaydı, `/Applications/bateri.app` açık) |

**Yol sapması — debug kolu `make smoke` değil, `open` ile bir paket.** İlk
`make smoke` iki kez `kare=0` verdi: `cargo run`'ın penceresi ekrandaydı
(`onscreen=1`) ama ön planda Safari vardı ve pencerenin tamamını örtüyordu;
wgpu örtülü pencereye drawable vermiyor. Ortamın kusuru, kodun değil — aynı
debug binary'si `open` ile açılınca ön plana geçti ve yeşil düştü. Debug kolu
bu yüzden debug binary'si taşıyan geçici bir paketten (`dev.bateri.duman`)
release koluyla aynı döngüde koştu (`## Nasıl yeniden ölçülür` → Boşta kare).
Paketsiz süreçle farkı: süreli koşu ayar okumuyor ve `/bin/sh` koşuyor, yani
ölçülen yol aynı.

**Sağlıklı koşular** (hepsi yeşil):

| profil · yol · süre | n | `icerik` | `kare` | `hareket` | `sessiz` (ms) |
|---|---|---|---|---|---|
| debug · paket (`open`) · 3 sn | 10 | `2` ×10 | `29` ×10 | `27` ×10 | 1746,88 – 1759,09 (ort. 1752,42) |
| release · paket (`open`) · 3 sn | 10 | `2` ×10 | `29` ×8, `28`, `27` | `27` ×8, `26`, `25` | 1749,16 – 1758,93 (ort. 1753,80) |
| debug · paket · 5 sn | 3 | `2` ×3 | `29` ×2, `27` | `27` ×2, `25` | 3741,21 – 3750,72 |
| release · paket · 5 sn | 3 | `2` ×3 | `29` ×3 | `27` ×3 | 3748,44 – 3753,41 |

Sabit jetonlar, yirmi altı koşunun hepsinde: `hucre=8 glif=6 kural=15
yuva=13/1984 yuva2=0/1984 yuk=smoke istek=4 kayma=0 kapanis=clean ornek=off
pipeline=ok`.

**Bozuk koşular** (hepsi kırmızı, n = 3 × profil):

| mutasyon | profil | düşen kapı | `icerik` | `sessiz` (ms) |
|---|---|---|---|---|
| hızlı sızıntı | debug | `MissingCounter` (`hareket=0`) | 349 · 354 · 356 | 0,00 ×3 |
| hızlı sızıntı | release | `MissingCounter` (`hareket=0`) | 342 · 355 · 347 | 0,00 ×3 |
| yavaş sızıntı | debug | `QuietTooShort` | 7 ×3 | 150,01 · 142,08 · 124,61 |
| yavaş sızıntı | release | `QuietTooShort` | 7 ×3 | 155,21 · 141,24 · 130,75 |
| durma koşulu | debug | `MotionUnsettled` | — | 0,00 · 0,00 · 2,30 |
| durma koşulu | release | `MotionUnsettled` | — | 0,00 · 4,32 · 0,00 |

**Kuralların sınaması**

- **`IDLE_FRAME_LIMIT = 8`:** en yüksek sağlıklı `icerik` 2, iki katı 4 ≤ 8;
  en düşük bozuk (hızlı sızıntı) 342 > 8. Yavaş sızıntı `icerik=7` ile
  sınırın altında — bilinen boşluk, onu `sessiz` görüyor (aşağıda).
- **`QUIET_FLOOR = 868 ms`:** en düşük sağlıklı gözlem 1746,88, yarısı
  **873,44** ≥ 868; en yüksek bozuk gözlem 155,21 < 868. Taban kuralın
  içinde; bu turun bandı 2026-09-17'ninkinden (alt uç 1737,12) ~10 ms yukarıda,
  yani 868'in payı genişledi. Sayı "aralığın en büyük ucu" kuralıyla 873'e
  **çıkarılmadı**: kural tabanın tavanını bağlıyor, her turda yeniden
  seçilmesini değil — tavanın içinde kalan bir sayıyı oynatmak bir sonraki
  turun dar bandında (09-17 gibi) yeniden indirmeyi doğururdu.

**Sayaç kayıtları, sınırların çok altında:** release `icerik` 2026-09-17'nin
`3` ×10'undan **`2` ×10**'a indi (debug zaten 2'ydi) — wgpu yolunda iki profil
aynı. `kare` release'te iki koşuda 28 ve 27, `hareket` 26 ve 25; nedeni
ölçülmedi, yön kısalma, yani sızıntı imzası değil. Yavaş
sızıntının bozuk `sessiz`'i 2026-09-16'nın 129,25'inden 155,21'e çıktı — hâlâ
tabanın beşte birinin altında. Durma koşulu kolunda `hareket` yine 25–27; kapı yerleşme
sorusundan düşüyor, kare sayısından değil.

### 2026-09-17 — duman reçetesi değişti, band yeniden gözlendi (011)

Neden: 011 phase-0 reçetenin imleç sıçramasını dikeyden yataya çevirdi
(`\033[H` → `\033[2G`) ve mesafe `QUIET_FLOOR`'un **dördüncü bağlı
girdisi** (bkz. `## Yöntem` → Boşta kare). Phase-0 tek koşuyla "band yerinde"
demişti; bu ölçüm bandın kendisini yirmi koşuyla yeniden gözlüyor. **Üst sınır
değişmedi; alt sınır aşağıdaki bulgu üzerine 870'ten 868 ms'ye yeniden
türetildi.**

**Ortam**

| | |
|---|---|
| commit | `4c291ee` (çalışma ağacı temiz) |
| makine | Apple M1 Pro, 32 GB |
| sistem | macOS 26.4.1 (25E253); rustc 1.88.0 (Homebrew) |
| güç | prizde (`AC Power`), pil %100 dolu, `lowpowermode 0` |
| ekran | 2026-09-16 koşusuyla aynı makine; Hz ve pencere boyutu bu turda **yoklanmadı** |
| kullanıcı | makineye dokunulmadı; tarayıcı ve IDE indeksleyici kapalı |

**Sabit jetonlar.** Yirmi sağlıklı koşunun **hepsinde**:

```
hucre=8 glif=6 kural=15 yuva=13/2048 yuk=smoke istek=4 hareket=27 kayma=0 kapanis=clean ornek=off pipeline=ok
```

**Sağlıklı koşular** (hepsi yeşil):

| profil · yol · süre | n | `icerik` | `kare` | `sessiz` (ms) |
|---|---|---|---|---|
| debug · `make smoke` · 3 sn | 10 | `2` ×10 | `29` ×10 | 1737,12 – 1751,58 (ort. 1745,02) |
| release · paket (`open`) · 3 sn | 10 | `3` ×10 | `30` ×10 | 1739,36 – 1756,28 (ort. 1748,61) |

Bozuk kol **koşulmadı**: sınırları doğuran dağılım 2026-09-16'da ölçüldü ve bu
tur onu yeniden türetmiyor, yalnız sağlıklı bandın yerini soruyor.

**`hareket=27` oynamadı** — reçetenin yatay hedefi dikeyle aynı sayıda hareket
karesi üretiyor, yani phase-0'ın tek koşuyla verdiği karar yirmi koşuda da
ayakta.

**İki sayaç kaydı, ikisi de sınırın çok altında:** `icerik` debug'da 2026-09-16'nın
`3` ×9'undan **`2` ×10**'a, release'te `2` ×7'den **`3` ×10**'a geçti. İkisi de
`IDLE_FRAME_LIMIT = 8`'in çok altında ve yön profiller arasında ters, yani
bir sızıntı imzası değil; ayrımın sebebi aranmadı.

**Bulgu: alt sınır kendi türetme kuralının dışına düşmüştü → 870'ten 868'e.**
Kural "en düşük sağlıklı gözlemin **en çok yarısı**"; en düşük gözlem
**1737,12 ms** ve yarısı **868,56 ms**, oysa `QUIET_FLOOR` 870 ms idi. Aşım
**1,44 ms** (‰1,7). Kapı bu koşularda yeşildi — `sessiz` hiç 1737'nin altına
inmedi — yani kusur yine jetonun arkasındaydı: kuralın istediği ×2 güvenlik
payı ×1,997'ye inmişti. Yeni değer dosyanın kendi seçim kuralından geliyor
(*"aralığın en büyük ucundan"*): 868,56'nın altındaki en büyük tam milisaniye
**868**. Bozuk kolun en yüksek gözlemi (yavaş sızıntı, 129,25 ms) hâlâ çok
altta, yani duyarlılık kaybı yok.

**Karşıt okuma kayda geçiyor:** ‰1,7'lik bir sapma için donmuş bir sabiti
oynatmak fazla titiz görülebilir ve payın ×2'den ×1,997'ye inmesinin pratik
sonucu yok. Yine de indirildi, çünkü "en çok yarısı" bir yaklaşıklık değil
**bağ**; bir kez "yaklaşık" okunursa bir daha hiçbir şeyi bağlamaz — phase-0
aynı türden bir aşımı (o gün 17 ms) kusur sayıp reçeteyi değiştirmişti.

Taban 2026-09-16'da **1742,29 ms**'lik bağlayıcı uçtan türetilmişti (yarısı
871,15 → 870 seçildi). Bu turun üç gözlemi (1737,12 · 1738,94 · 1739,36) o
ucun **altında** ve gürültü kuralını geçiyor: üçü de önceki dağılımın ortasına
değil, **tamamının altına** düşüyor. Bandın ~5 ms aşağı kayması reçete
değişiminden mi yoksa daha derin bir kuyruk örneklemesinden mi, bu veriyle
ayırt edilemez — ikisi de aynı düzeltmeyi gerektiriyor.

**`kayma=` hiçbir koşuda ateşlenmedi** ve yük yükü de onu tetikleyemedi
(`yuk=load`, n=2: `kare=355`/`351`, `icerik=355`/`351`, **`kayma=0`**).
Sebebi yapısal: yük PTY'yi 256'lık öbeklerle doyuruyor, yani ekran **ilk
içerik karesinde** zaten dolu; öteleme hedefi 0'da doğuyor ve hiç oynamıyor.
Aynı koşuda `hareket=0` — imlecin ekran satırı da dipte sabit. (O kolun
`kapanis=abandoned`'ı **beklenen**, kusur değil: yük `sleep` değil deadline'a
kadar yazıyor, yani çocuk `SHUTDOWN_GRACE` içinde ölmüyor ve arkada
bırakılıyor.) Sonuç bir
**kapsam kalemi**: ötelemenin animasyon yolunun gerçek pencerede koşan tanığı
yok, kanıtı birim sınamaları ve göz kontrolü (011 phase-2, `/measure`
2026-09-17). Yerleşme **süresi** ayrıca ölçülemedi: `kayma=` kare sayıyor,
süre değil — kanca yok.

### 2026-09-16 — imleç hareketi, iki sınır, debug + release paket (008 phase-6)

Neden: 008 imlece animasyon getirdi ve kapının operandı `kare`'den `icerik`'e
geçti (phase-1), yani aşağıdaki iki ölçümün sayıları **başka bir sayacın**
sayıları. Aynı sette hareket kareleri `kare`'yi meşru olarak şişirdiği için
kapının üçüncü bir kata ihtiyacı doğdu: altyapıyı atlayıp yavaşça kare isteyen
kodu ne `icerik` sınırı ne de yerleşme sorusu görüyor.

**Ortam**

| | |
|---|---|
| commit | `c3fb4d4` + phase-6'nın tanı düzeltmesi (çalışma ağacında; ölçülen kod `bt-gpu` tarafında **değişmemiş**) |
| makine | Apple M1 Pro, 32 GB |
| sistem | macOS 26.4.1 (25E253); rustc 1.88.0 (Homebrew) |
| güç | prizde (`AC Power`), pil %13 → şarj oluyor, `lowpowermode 0` |
| ekran | dahili, ölçek 2 (1512×982 pt); `maximumFramesPerSecond=120` |
| pencere | içerik 900×600 pt; yoklama çerçeveyi 900×632 pt ölçtü. Hücre sayısı **ölçülmedi** |
| görünürlük | yoklama debug ve paket kollarının birer koşusunda: `kCGWindowIsOnscreen=true`, 900×632. Öndeki uygulama **pakette `bateri`, debug'da değil** (`cargo run` yolu bu oturumda öne çıkmadı; 006'da çıkmıştı) |
| kullanıcı | makineye dokunulmuyor; **tarayıcı açıktı** (debug kolunun yoklamasında öndeki uygulama oydu) — yöntemin "tarayıcı kapalı" şartından sapma, kayda geçiyor |

**Sabit jetonlar.** Otuz sağlıklı koşunun **hepsinde**:

```
hucre=8 glif=6 kural=15 yuva=13/2048 yuk=smoke istek=4 kapanis=clean profil={debug|release} ornek=off pipeline=ok
```

**Sağlıklı koşular** (hepsi yeşil):

| profil · yol · süre | n | `icerik` | `kare` | `hareket` | `sessiz` (ms) |
|---|---|---|---|---|---|
| debug · `make smoke` · 3 sn | 10 | `3` ×9, `2` ×1 | `30` ×9, `29` ×1 | `27` ×10 | 1745,95 – 1755,25 |
| release · paket (`open`) · 3 sn | 10 | `2` ×7, `3` ×3 | `29` ×7, `30` ×2, `27` ×1 | `27` ×8, `26` ×1, `25` ×1 | 1746,88 – 1757,29 |
| debug · `cargo run` · 5 sn | 5 | `3` ×5 | `30` ×5 | `27` ×5 | 3742,95 – 3755,83 |
| release · paket (`open`) · 5 sn | 5 | `3` ×5 | `30` ×3, `29` ×2 | `27` ×3, `26` ×2 | 3746,16 – 3754,35 |
| **doğrulama koşuları** · 3 sn (kapı indikten sonra; `bt-gpu` değişmemiş) | 7 | `3` ×6, `2` ×1 | `30` ×6, `29` ×1 | `27` ×7 | 1742,29 – 1754,42 |

Son satır dağılımın **parçasıdır**, ayrı bir kol değil: gürültü kuralı hiçbir
sağlıklı koşuyu düşürmüyor ve bu yedi koşu türetmenin bağlayıcı ucunu
1745,95'ten **1742,29**'a indiriyor (dördü debug `make smoke`, üçü release
paket).

**Bozuk koşular** (3 sn, debug; hızlı sızıntı ayrıca release pakette):

| mutasyon | n | düşen kol | `icerik` | `kare` | `sessiz` |
|---|---|---|---|---|---|
| hızlı sızıntı · debug | 3 | `MissingCounter` (`hareket=0`) | 357, 358, 358 | 357, 358, 358 | 0,00 ms |
| hızlı sızıntı · release paket | 3 | `MissingCounter` (`hareket=0`) | ölçülmedi (tanı o gün `icerik` basmıyordu) | 355, 353, 356 | 0,00 ms |
| yavaş sızıntı · debug | 3 | **hiçbiri — koşu yeşildi** | 8, 8, 8 | 34 ×3 | 105,96 · 122,17 · 129,25 ms |
| durma koşulu · debug | 3 | `MotionUnsettled` | ölçülmedi | 352, 353, 354 (hareket karesi) | 0,00 ms |

**Türetme — `IDLE_FRAME_LIMIT` oynamadı.** En yüksek sağlıklı `icerik` `3`, en
düşük bozuk `357`. Kural (`≥ 2×3 = 6` ve `< 357`) bugünkü `8` ile sağlanıyor,
yani sınırı **zorlayan bir gözlem yok**. Yavaş sızıntının `icerik=8`'i sınırın
tam üstünde duruyor ve kuralı okuyunca sınırı `7`'ye indirmek gerekirdi; o
gözlem **bilerek** dışarıda bırakıldı, çünkü mutasyonun periyodu (500 ms)
sınırın kendisine bakılarak seçildi — sınırın türetmesine sokmak dairesel
olurdu (her indirimden sonra biraz daha yavaş bir sızıntı yine altta kalır).
O sızıntı sınıfının doğru cevabı sınırı kısmak değil, aşağıdaki alt sınır.

**Türetme — `QUIET_FLOOR = 870 ms` doğdu.** Otuz yedi sağlıklı koşunun en
düşüğü `1742,29 ms` → tavan `871,14 ms`; en yüksek bozuk gözlem `129,25 ms` →
taban onun üstünde olmalı. Kuralın izin verdiği aralık `(129,25 · 871,14]` ve
jeton ters çalıştığı için **en büyük** uç seçildi: on milisaniyelik adımlarla
`870 ms`. Sağlıklı dağılıma payı tam iki kat, yakaladığı en yavaş sızıntı
~1,15 Hz — `IDLE_FRAME_LIMIT`'in ~3 Hz'lik tabanından **2,6 kat** duyarlı.

**Taban tavanın 1 ms altında ve bu bilerek:** jetonun kuralı duyarlılığı
büyütmeyi ödüllendiriyor, yani aralığın ortasından seçilen bir sayı kapıyı
boşuna kısıtlardı. Bedeli, **yeniden türetme tetiğinin dar** olması: 3
saniyelik sağlıklı bir koşu `1740 ms`'nin altına inerse kuralın kendisi
bozulur (kapı değil — kapının payı hâlâ iki kat) ve `QUIET_FLOOR` yeniden
türetilmelidir. Bugüne kadarki otuz yedi koşunun bandı `1742,29 – 1757,29 ms`,
yani 15 ms.

**Kapının ateşlediği gösterildi.** `QUIET_FLOOR` indikten sonra yavaş sızıntı
mutasyonu tekrar koşuldu: üç koşunun ikisi `QuietTooShort` (120,95 ve
116,95 ms), biri `ExcessFrames` (`icerik=9`) ile kırmızı düştü. Aynı derlemede
sağlıklı koşu yeşil (`sessiz=1742,29 ms`).

**Gözlemler — yorum değil, kayıt:**

- **Sağlıklı kuyruk şaşırtıcı derecede dar:** otuz yedi koşunun tamamı
  1742,29 – 1757,29 ms, yani 15 ms'lik bir bant. Yapısal olarak beklenen de
  bu: `3 sn − (1 sn uyku + ~0,25 sn yerleşme)`. Kapının payı bu yüzden
  gürültüden değil **tasarımdan** geliyor.
- **Profil ayrışması döndü.** 006'da `kare` debug'da çoğunlukla `1`, release
  pakette `2` idi; burada `icerik` debug'da çoğunlukla `3`, release pakette
  `2`. `istek` üç ölçümde de sabit (bugün `4`), yani fark yine **talepten**
  gelmiyor. Mekanizması yine **ölçülmedi**.
- **Hızlı sızıntı artık başka bir kolu düşürüyor.** 006'da `ExcessFrames`
  veriyordu; 008'de kalıcı hasar "hasar yok" dalını hiç çalıştırmadığı için
  `hareket=0` ve kapı daha temel arızayı (`MissingCounter`) yazıyor. Reçete
  bu yüzden güncellendi — tarifin "hangi kolu ateşler" cümlesi bir
  **gözlemdi**, sözleşme değil.
- **Yavaş sızıntı kolu, kapının bu sette neden büyüdüğünün kanıtı:** üç
  koşunun üçü de, `icerik` sınırı ve yerleşme sorusu yerinde dururken
  **yeşil** geçti.
- **Bozuk koşu yine tam tazeleme hızında:** 3 saniyede 353–358 kare ≈ saniyede
  118–119; 006'daki gibi kısılma görülmedi.
- **`istek=` düşündüğüm kadar sabit değil.** Otuz ölçüm koşusunun ve yedi
  doğrulama koşusunun tamamı `4` verdi, ama kapı indikten sonraki bir koşu
  `3` bastı ve peşinden gelen altı koşu yine `4`. Ayıklanmadı (gürültü
  kuralı): nedeni **ölçülmedi**, akla yatkın mekanizma `Waker`'ın
  birleştirmesi — `pending` bayrağı zaten diklken gelen ikinci uyandırma
  sayaca giriyor ama yeni bir dispatch doğurmuyor, yani yavaş bir açılışta
  iki talep tek kareye düşebilir. Jeton bir kapı değil, bu yüzden koşu yeşil.
- **Beş saniyelik koşular sınırları zorlamadı:** `icerik` yine `3`, `sessiz`
  ~3,75 sn. Kuyruk süreyle doğrusal büyüyor, yani taban 3 saniyelik reçeteye
  bağlı ve orada en dar hâlinde.

### 2026-09-15 — görünür pencere, debug + release paket (006 phase-5)

Neden: 006 `.app` paketi getirdi. Önceki ölçüm (005 phase-3) bundle'sız süreçte
yapılmıştı ve pencerenin görünür olduğu **yoklanmamıştı**; yol haritası onu
"görünmeyen pencerede ölçüldü" diye kaydetti ve görünür pencerenin meşru kare
sayısını değiştirip değiştirmediği bilinmiyordu.

**Ortam**

| | |
|---|---|
| commit | `571a0e8` (çalışma ağacı temiz; ölçülen kaynak `3907585`'in kodu) |
| makine | Apple M1 Pro, 32 GB |
| sistem | macOS 26.4.1 (25E253); rustc 1.88.0 (Homebrew) |
| güç | prizde, pil %100 dolu, `lowpowermode 0` |
| ekran | dahili Liquid Retina XDR, 3024×1964, ölçek 2 (1512×982 pt); ProMotion, `maximumFramesPerSecond=120` |
| pencere | içerik 900×600 pt (`app.rs`'in pencere dikdörtgeni); yoklama çerçeveyi başlık çubuğuyla 900×632 pt ölçtü. Hücre sayısı jetonda yok, **ölçülmedi** |
| görünürlük | yoklama biri debug biri paket iki koşuda, t≈1,5 sn'de: `kCGWindowIsOnscreen=true`, öndeki uygulama `bateri`. Diğer koşular yoklanmadı |
| kullanıcı | açık `bateri` penceresi yok, makineye dokunulmuyor |

**Sabit jetonlar.** Aşağıdaki elli bir sağlıklı koşunun **hepsinde** satırın
geri kalanı aynıydı; oynayan yalnız `kare` ve `istek`:

```
hucre=8 glif=6 kural=15 yuva=13/2048 yuk=smoke kapanis=clean profil={debug|release} ornek=off pipeline=ok
```

**Sağlıklı koşular** (`kare/istek`, koşu sırasıyla; `make smoke` ve paket
koşularının hepsi yeşil):

| profil · yol · süre | n | `kare` dağılımı | `istek` dağılımı | koşular |
|---|---|---|---|---|
| debug · `make smoke` · 3 sn | 21 | `1` ×20, `2` ×1 | `2` ×9, `3` ×12 | 1/2 1/2 1/3 1/2 1/3 1/3 2/3 1/3 1/3 1/2 · 1/3 (yoklamalı) · 1/3 1/2 1/3 1/2 1/3 1/2 1/3 1/2 1/3 1/2 |
| release · paket (`open`) · 3 sn | 20 | `2` ×18, `1` ×2 | `3` ×20 | 2/3 (yoklamalı) 2/3 2/3 2/3 2/3 1/3 2/3 2/3 2/3 2/3 · 1/3 2/3 2/3 2/3 2/3 2/3 2/3 2/3 2/3 2/3 |
| debug · `cargo run` · 5 sn | 5 | `1` ×5 | `3` ×5 | 1/3 1/3 1/3 1/3 1/3 |
| release · paket (`open`) · 5 sn | 5 | `2` ×3, `1` ×2 | `3` ×5 | 1/3 2/3 2/3 1/3 2/3 |

**Bozuk koşular** (3 sn; kapı düştü, jeton satırı yok, stderr satırından):

| profil · yol | n | `kare` | kare talebi |
|---|---|---|---|
| debug · `make smoke` (çıkış 2) | 3 | 353, 354, 353 | 356, 356, 356 |
| release · paket (`open`, stdout boş) | 3 | 356, 357, 357 | 359, 360, 360 |

**Türetme.** Sağlıklı en yüksek gözlem `2` (iki profilde de), bozuk en düşük
`353`. `8` bu ölçümde en yüksek sağlıklı gözlemin dört katı, en düşük bozuk
gözlemin kırk dörtte biri; kural (iki kat / bozuğun altında) iki profil için
de sağlanıyor. Sınır **değişmedi**: yükseltecek bir sağlıklı gözlem yok,
düşürmek ise 005'in sağlıklı `4`'ünü (5 sn, debug) ölçmeden geçersiz saymak
olurdu.

**Gözlemler — yorum değil, kayıt:**

- **Görünür pencere meşru kare sayısını artırmadı.** 005'in görünürlüğü
  yoklanmamış koşularında sağlıklı `kare` 1–2 (bir kez 4) idi; burada 1–2.
  Bugün bundle'sız `cargo run` yolu da ekranda ve önde (yoklama) — 005'teki
  pencerenin gerçekten görünmez olup olmadığı geriye dönük bilinemiyor.
- **Bozuk koşu tam tazeleme hızında.** 3 sn'de 353–357 kare ≈ saniyede 118–119;
  yani koşu sırasında link ~120 Hz'de sürdü. 005'te bozuk duman dağınıktı —
  3 sn'de 82–354 (üst ucu bu koşuyla aynı hız), 5 sn'de 49–63 (kısılmış);
  burada altı koşu da dar bir aralıkta ve kısılma görülmedi. Kısılmanın ne
  zaman devreye girdiği ölçülmedi.
- **Profil ayrışıyor, talep ayrışmıyor.** Debug koşuların çoğu `kare=1`,
  release paket koşularının çoğu `kare=2`; `istek` ise iki profilde de 2–3.
  Mekanizması **ölçülmedi**; `IDLE_FRAME_LIMIT`'in doc'undaki hipotez (açılış
  karesi shell'in ilk baytlarından önce çizilirse ikinci bir kare gerekir)
  profil farkıyla sınanabilir (`acilis=` jetonu iki profilde), ama bu koşuda
  açılış **ölçülmedi**.
- **phase-4c'nin `glif=` gürültüsü tekrarlanmadı.** Elli bir sağlıklı koşunun
  hepsi `glif=6 yuva=13`; ayıklanan koşu yok. Kullanıcı tuş vuruşu
  hipoteziyle uyumlu, onu kanıtlamıyor.

### 2026-09-12 — bundle'sız süreç, debug (005 phase-3)

`2` → `8`. Bu dosya yokken ölçüldü (005 R7.3 dosyayı o sete yasaklamıştı);
kaynağı `.tasks/005-olcum-kancalari/phase-3.md` → `## Uygulama Notları` →
"Ölçülenler", buraya taşındı çünkü `8`'i bağlayan iki kutup (`4`, `49`) bu
ölçümden geliyor. Pencere görünürlüğü **yoklanmadı**; ortam kaydı yok
(makine aynı). Sağlıklı koşuların ikisi `BT_FRAME_STATS=1` ile.

| koşu | n | `kare` |
|---|---|---|
| sağlıklı duman, 3 sn | 18 | `1` ×10, `2` ×8 |
| sağlıklı duman, 5 sn | 11 | `1` ×9, `2` ×1, `4` ×1 |
| sağlıklı duman, 10 sn | 2 | `2` ×2 |
| sağlıklı duman, 5 sn, kare akışının serbest olduğu rejim | 5 | `1` ×5 |
| bozuk duman, 3 sn | 7 | 82–354 |
| bozuk duman, 5 sn | 2 | 49–63 |

O günün iki ek gözlemi: oynama değişiklikten gelmiyordu (on beş koşu
değiştirilmemiş `854f027`'de aynı dağılımı verdi) ve eski `2` doğru bir
build'i kırmızıya düşürdü (5 sn'lik koşudaki `4`). İki rejim aynı günün
ölçüm yükünde görüldü (aynı komutla 5 sn'de bir kez `kare=21`, bir kez `597`).

## Atlas yuva ayak izi

### 2026-09-21 — yordamsal aile ve doyma eşiği (021)

Ölçülen commit `51e1459`, taban `b17fa78` (021 öncesi). MacBook Pro M1 Pro,
32 GB, macOS 26.4.1 (25E253), rustc 1.88.0, prizde, Retina 3024×1964. Font
**Menlo** (varsayılan), `line_height = 1.0`. Debug ile release **birebir aynı**
sayıyı verdi (425); aşağıdaki tablo ikisinin ortak değeri.

**Ayak izi** — aynı Unicode aralıkları, iki commit (yuva):

| küme | taban (`b17fa78`) | bugün (`51e1459`) |
|---|---|---|
| çizgi çizim (U+2500–257F, köşegensiz) | 125 (fonttan) | 125 (yordamsal) |
| köşegen (`╱╲╳`) | 3 (fonttan) | 3 (fonttan — kapsam dışı) |
| blok elemanları (U+2580–259F) | 32 (fonttan) | 32 (yordamsal) |
| Braille (U+2800–28FF) | **0** (kapıdan dönüp tofu'ya) | **256** (yordamsal) |
| teknik (U+23B8–23BF) | 6 (fonttan, ikisi tofu) | 8 (yordamsal) |
| kök kuyruğu (`⎷` U+23B7) | 0 (tofu) | 1 (fonttan — bilerek dışarıda) |
| **yordamsal ailenin kendisi** | **160** | **421** |
| **aralıkların toplamı** | 166 | 425 |

421 sayısı planın beklediğinin **birebir aynısı**. Braille'in 0 → 256 sıçraması
setin asıl bedeli; teknik kümenin 6 → 8'i ile kök kuyruğunun 0 → 1'i araya
giren mürekkep kapısını da taşıyor (bkz. Yöntem).

**Kutu envanteri** (2026-09-22, `0d383bc`) — Menlo 16pt@2x, hücre ilerlemesi
**19.266 px**. Taranan aralıklar 019/021'in envanterindekiler artı CJK ve
emoji blokları (U+3000–30FF, U+4E00–4E7F, U+1F300–1F5FF, U+1F600–1F64F,
U+1F900–1F9FF, U+FF00–FF60): **3521 kod noktası, 2559'u kutu.**

| aday fontu | 1 sütun | 2 sütun | toplam |
|---|---:|---:|---:|
| Apple Color Emoji | 78 | **922** | 1000 |
| `.LastResort` (makinede font yok) | 825 | 0 | 825 |
| Hiragino Sans | 31 | 204 | 235 |
| PingFang SC | 0 | 220 | 220 |
| Apple Symbols | 116 | 12 | 128 |
| STIX Two Math | 104 | 0 | 104 |
| Zapf Dingbats | 30 | 0 | 30 |
| Arial Unicode MS, Hiragino Sans GB, diğer | 7 | 10 | 17 |

Üç sayı 023'ün kapsamını belirledi:

1. **Adayı olan her 2 sütunlu karakterin ilerlemesi tam olarak hücrenin 1.66
   katı** — emoji, PingFang ve Hiragino'da aynı, dağılım değil **tek değer**
   (1.66..1.66). Yani mürekkebi iki hücreye sığıyor: **1346/1346**. Kapının
   argümanını sütunla çarpmak ailenin tamamını kabul ediyor; ikinci bir eşik
   ya da "emoji mi" sorusu gerekmedi.
2. **Emojinin 78'i tek sütunlu** (`🌡 🎙 🏋 🏔`) ve mürekkebi 1.66 hücre, yani
   tek hücreye sığmıyor. Rengi olan ama iki sütunu **olmayan** karakterler:
   geometri kolu onlara yardım etmiyor ve 023'te kutu kaldılar.
3. **65 karakter 2 sütunlu ve ölçümden önce de çiziliyordu** — 21'i Menlo'nun
   kendi glyph'i (`◽ ◾ ☔ ☕ ♈…♓ ♿ ⚓ ⚡`), 44'ü cascade'den narin mürekkeple
   geçenler (`丨 、 。 》 」 ！ １ Ｉ ｜`). 023'ün kapı **sırası** (önce tek
   hücre) bu sayıdan çıktı: ölçüm olmadan "geniş ilan edilmiş" ile "geniş
   boyuyor" ayrımı görünmezdi ve bayrağa bağlanan bir tasarım 65 çalışan
   çizimi yerinden oynatırdı.

**Doyma eşiği** — ailenin kendisi kapasiteyi hangi puntoda aşıyor
(`floor(1024/w) * floor(1024/h)`, aile + tofu = 422 yuva ister):

| punto @2x | kapasite | aile sığıyor mu |
|---|---|---|
| 13 | 1984 | evet (ailenin payı %21) |
| 16 | 1326 | evet (%32) |
| 20 | 800 | evet (%53) |
| 26 | 512 | evet (%82) |
| 28 | 450 | evet — **24 yuva artıyor** |
| **29** | **406** | **hayır** (Braille 256 yerine 238 aldı, teknik küme hiç) |
| 31–32 | 338 | hayır (Braille 170) |
| 56 | 105 | hayır (çizgi ailesi bile kırpılıyor: 97) |

Ölçek 1'de eşik **58pt** (kapasite 406; 57pt'de 435 ile sığıyor) — aynı kapasite
sayısı, çünkü kırılma noktası hücrenin piksel boyu.

**Okunuşu (2026-09-21, ölçüldüğü gün):** Retina'da 29pt ve üstünde yordamsal
ailenin kendisi atlasa sığmıyor ve **tahliye olmadığı için** sığmayan her
karakter o oturumun kalanında kalıcı olarak kutu çıkıyor. 28pt'de teknik
olarak sığıyor ama geriye 24 yuva kalıyor, yani ASCII ile kullanıcının metni
için yer yok — pratik eşik 29 değil, ona yaklaşan **her** punto. Ayak izi bir
**tavan**: yuvalar istendikçe harcanıyor, yani bütün aileyi kullanmayan
içerik bu sayıyı ödemiyor.

> **Bu okunuş 2026-09-22'de geçersizleşti (022).** Doku kenarı artık
> hedeflenen yuva sayısından türüyor: 29pt@2x'te kenar 2048'e çıkıyor ve
> kapasite 406 değil **1624** oluyor, yani aile sığıyor. Bu sayı yeni bir
> ölçüm **değil**, yukarıdaki tablonun kendi hücre ölçüsünden aritmetik
> (`floor(2048/35) * floor(2048/73)`) ve `Atlas::occupancy` üstünden
> sınamayla doğrulanıyor — bu bölüm yalnız **ölçülmüş** sayının sahibi, bu
> bir türetme. Yukarıdaki tablo sabit 1024 kenarın kaydı olarak duruyor
> — ölçüm o gün doğruydu ve tarihî kayıt silinmez — ama "29pt ve üstünde
> kalıcı kutu" cümlesi bugünün kodu için **yanlıştır**. Bugünkü sözleşme bir
> tablo değil bir değişmez:
> `bt_atlas::tests::capacity_clears_the_family_at_every_accepted_size`
> (kabul edilen her aile × punto × ölçek × `line_height` için kapasite ≥
> ailenin istediği 429). Atlas hâlâ dolabilir; kalan senaryo "tek karede
> hedeften fazla farklı glyph" ve **ölçülmedi** (`docs/YOL-HARITASI.md` →
> "Atlas dolunca geri dönüşü yok").

## Kare süresi

### 2026-09-30 — wgpu pencere yolu, (b) Pacer (040 phase-5): durak tetiklendi

Metal renderer ürün yolundan çıktı; pencere yolu wgpu (`Surface` +
`NSView.displayLink` zamanlayıcı olarak). Soru Karar 3/7'nin durağı: pencere
yolunun CPU dağılımı Metal tabanıyla örtüşüyor mu?

Reçete aşağıdaki bloklarla aynı: release, binary doğrudan, `BT_FRAME_STATS=1
BT_SCROLL_TEST=1`, 10 saniye, 10 koşu. Ağaç `d004de3` + phase-5 (commit'lenmemiş
hâli). MacBook Pro M1 Pro, macOS 26.4.1 (25E253), rustc 1.88.0, **prizde**
(%35, şarjda). Ortam kullanıcının "teste hazır" dediği hâl: tarayıcı ve ağır
uygulamalar kapalı; buna rağmen yük ortalaması **4,44** (1/5/15 dk: 4,44 /
3,52 / 3,40 — tabanın gününde 1,66), `top`'ta WindowServer %42, syspolicyd
%40 (yeni binary'nin imza denetimi), bir kullanıcı uygulaması %29.

| # | cpu_kare p95 / max | cpu_encode p95 / max | gpu p95 / max | acilis | kare | istek | kapanis |
|---|---|---|---|---|---|---|---|
| 1 | 0,08 / 0,42 | 0,42 / 3,12 | 0,35 / 0,80 | 660,35 | 1193 | 319427 | abandoned |
| 2 | 0,07 / 0,18 | 0,41 / 2,65 | 0,24 / 0,84 | 255,93 | 1193 | 307444 | abandoned |
| 3 | 0,10 / 0,76 | 0,49 / 2,52 | 0,55 / 1,08 | 247,29 | 1192 | 296412 | abandoned |
| 4 | 0,08 / 0,16 | 0,43 / 2,66 | 0,51 / 0,95 | 279,86 | 1193 | 283835 | clean |
| 5 | 0,08 / 0,42 | 0,41 / 2,47 | 0,34 / 1,05 | 242,88 | 1194 | 306546 | clean |
| 6 | 0,07 / 0,63 | 0,40 / 2,32 | 0,24 / 1,07 | 237,93 | 1194 | 307899 | clean |
| 7 | 0,07 / 0,19 | 0,40 / 2,60 | 0,23 / 1,18 | 239,15 | 1194 | 311957 | abandoned |
| 8 | 0,07 / 0,16 | 0,40 / 2,44 | 0,23 / 1,06 | 232,80 | 1194 | 311867 | abandoned |
| 9 | 0,07 / 0,37 | 0,39 / 2,63 | 0,35 / 0,83 | 252,09 | 1192 | 334183 | abandoned |
| 10 | 0,07 / 0,16 | 0,39 / 2,55 | 0,35 / 1,06 | 231,60 | 1194 | 328431 | clean |

Sabit jetonlar: `hucre=0 glif=1785 kural=0 yuva=37/1984 yuva2=0/1984 yuk=load
icerik=kare hareket=0 kayma=0 sessiz=0.00ms profil=release dusen=0
gpu_elenen=0 taban=20 pipeline=ok`; `ornek`/`gpu_ornek` = `kare` (±1). Tam
satırlar `target/olcum-kare/wgpu-pencere/run-*.out`.

**Sonuç — durak kuralı tetiklendi.** `cpu_encode_p95` **0,39–0,49** (medyan
0,405) — Metal tabanı (`3c7a46e` bloğu, aşağıda) **0,26–0,29**; dağılımlar
örtüşmüyor ve wgpu kötü (medyanda +~0,13 ms, ~1,5×). Tanık sütun `cpu_kare_p95`
**0,07–0,10** (taban 0,07–0,08; aynı kod, yük farkına rağmen aynı bant — fark
encode'da). Offscreen denemede (phase-2, `## wgpu denemesi`) aynı sütunun farkı
~0,08 ms idi; pencere yolunda fazlası `present`'in ve yüzey dokusunun kare başı
view'ının aralığa girmesiyle tutarlı, ayrıştırılmadı. `gpu_p95` 0,23–0,55
(başka saat, karar vermiyor); `acilis` 231–280 (1. koşu soğuk, 660), taban
172–216. `kare` 1192–1194, taban 1197–1198.

### 2026-09-30 — patlama ile akışı süre ayırıyor: encode payı korunuyor

Aşağıdaki bloğun düzeltmesinin takibi. Gözle kontrolde (gerçek pencere)
parçalı gelen kısa patlama (`ls -la`, `seq 1 200`) bazen süzülüyor bazen
süzülmüyordu: ayrım kare sayısına dayanıyordu ve ikinci ekran boyu kare akış
sanılıyordu. Yeni ölçüt süre: ekran boyu karelerin kesintisiz koşusu
`BURST_WINDOW`'dan (`EASE_DURATION`, 0,18 sn) gençse aynı patlama (konum
tavana kırpılır, kayma sürer), yaşlıysa akış (kayma bitirilir, koşu bitene
kadar yeniden kurulmaz). `Motion::origin_pouring` → `Motion::screenful_run`.
Soru: akışın ilk 0,18 sn'si artık tavanda süzüldüğüne göre encode payı geri
geliyor mu?

Reçete aşağıdakinin aynısı: release, binary doğrudan, `BT_FRAME_STATS=1
BT_SCROLL_TEST=1`, 10 saniye, 10 koşu. Ağaç `123ae5d` + bu düzeltme
(commit'lenmemiş hâli). MacBook Pro M1 Pro, macOS 26.4.1 (25E253), rustc
1.88.0, **prizde** (%100, dolu). Yük ortalaması 1,66 (5 dk'lık 2,71); açık
bateri yok.

| # | cpu_kare p95 / max | cpu_encode p95 / max | gpu p95 / max | acilis | kare | istek | kapanis |
|---|---|---|---|---|---|---|---|
| 1 | 0,07 / 0,40 | 0,27 / 1,44 | 0,25 / 0,36 | 200,78 | 1198 | 337847 | clean |
| 2 | 0,08 / 0,15 | 0,28 / 1,50 | 0,25 / 0,58 | 207,98 | 1197 | 330559 | abandoned |
| 3 | 0,07 / 0,24 | 0,28 / 1,48 | 0,25 / 0,58 | 205,08 | 1197 | 333091 | clean |
| 4 | 0,08 / 0,16 | 0,28 / 1,51 | 0,25 / 0,58 | 183,99 | 1197 | 332886 | abandoned |
| 5 | 0,08 / 0,18 | 0,27 / 1,48 | 0,25 / 0,72 | 186,75 | 1198 | 324550 | clean |
| 6 | 0,08 / 0,15 | 0,26 / 1,43 | 0,25 / 0,58 | 171,65 | 1198 | 332839 | clean |
| 7 | 0,08 / 0,28 | 0,27 / 1,42 | 0,25 / 0,58 | 172,60 | 1198 | 331044 | abandoned |
| 8 | 0,08 / 0,14 | 0,27 / 1,53 | 0,25 / 0,58 | 215,60 | 1197 | 336662 | abandoned |
| 9 | 0,07 / 0,15 | 0,28 / 1,44 | 0,25 / 0,58 | 195,76 | 1198 | 329895 | clean |
| 10 | 0,08 / 0,55 | 0,29 / 1,52 | 0,25 / 0,58 | 211,91 | **610** | 346992 | clean |

Sabit jetonlar bir kez: `hucre=0 glif=1785 kural=0 yuva=37/1984
yuva2=0/1984 yuk=load hareket=0 kayma=0 profil=release dusen=0
gpu_elenen=0 taban=20 pipeline=ok`; `icerik`/`ornek`/`gpu_ornek` = `kare`.
`sessiz=0.00ms` dokuz koşuda. Tam satırlar
`target/olcum-kare/burst-window/run-*.out`.

**Sapma ayıklanmadı:** 10. koşu `kare=610`, `sessiz=4878.47ms` — aşağıdaki
bloğun 6. koşusuyla aynı desen (link koşunun yarısını uyudu); p95'leri
öteki dokuzla aynı bantta.

**Sonuç.** `cpu_encode_p95` **0,26–0,29** (önce 0,25–0,28), `cpu_kare_p95`
**0,07–0,08** (aynı), `gpu_p95` **0,25** (aynı). Akışın ilk 0,18 sn'si
(~22 kare, 10 sn'lik koşunun ~%2'si) koşunun en pahalı kareleri, yani üst
%5'e (~60 kare) giriyor ve p95'i en çok o kadar itebilir; ölçülen +0,01
bununla tutarlı ya da gürültü, ayrılamıyor. Belirgin bir geri çıkış yok,
123ae5d'nin kazancı korundu.

### 2026-09-30 — birinci basamağın düzeltmesi: akış hatırlanıyor

Aşağıdaki bloğun `0777e13` mekanizması doğrulandı ve kapatıldı. Kod
okuması + sınama: `Motion::scroll_in`'in akış kolu kaymayı bitirip yerleşik
bırakıyor, yani sürekli akışta bir sonraki ekran boyu kare kaymayı "durgun"
buluyor ve patlamayı yeniden kuruyordu — N patlama, N+1 bitir, N+2 yine
patlama (`a_stream_of_screenful_scrolls_never_bursts_again` düzeltmeden önce
3. karede 30,0 ile kırmızıydı). Düzeltme: bir önceki içerik karesi de ekran
boyu kaydırdıysa patlama kurulmuyor (`Motion::origin_pouring`); durgun
ızgaradaki tek patlama (`seq 1 200`) aynen süzülüyor.

Reçete aşağıdakinin aynısı: release, binary doğrudan, `BT_FRAME_STATS=1
BT_SCROLL_TEST=1`, 10 saniye, 10 koşu. Ağaç `14c4dac` + bu düzeltme
(commit'lenmemiş hâli, yalnız `motion.rs`). MacBook Pro M1 Pro, macOS 26.4.1
(25E253), rustc 1.88.0, **prizde** (%100, dolu). Makine sessiz: yük
ortalaması 1,65 (5 dk'lık 2,04), `top`'ta %1'i geçen süreç yok; açık kalan
tek bateri kullanıcının `/Applications/bateri.app`'i.

| # | cpu_kare p95 / max | cpu_encode p95 / max | gpu p95 / max | acilis | kare | istek | kapanis |
|---|---|---|---|---|---|---|---|
| 1 | 0,07 / 0,15 | 0,26 / 1,43 | 0,25 / 0,58 | 206,37 | 1198 | 355893 | abandoned |
| 2 | 0,08 / 0,19 | 0,27 / 1,54 | 0,25 / 0,58 | 220,93 | 1198 | 335683 | abandoned |
| 3 | 0,07 / 0,15 | 0,26 / 1,39 | 0,25 / 0,58 | 206,24 | 1198 | 351216 | clean |
| 4 | 0,07 / 0,50 | 0,27 / 1,50 | 0,25 / 0,58 | 193,93 | 1197 | 333588 | abandoned |
| 5 | 0,07 / 0,15 | 0,27 / 1,50 | 0,25 / 0,58 | 205,48 | 1197 | 341079 | clean |
| 6 | 0,08 / 0,17 | 0,28 / 1,45 | 0,25 / 0,63 | 181,00 | **514** | 345725 | abandoned |
| 7 | 0,07 / 0,17 | 0,25 / 1,57 | 0,25 / 0,58 | 227,64 | 1193 | 361711 | clean |
| 8 | 0,07 / 0,18 | 0,25 / 1,39 | 0,25 / 0,72 | 223,66 | 1194 | 358844 | clean |
| 9 | 0,08 / 0,17 | 0,26 / 1,57 | 0,25 / 0,58 | 224,00 | 1194 | 354941 | abandoned |
| 10 | 0,08 / 0,16 | 0,25 / 1,47 | 0,25 / 0,58 | 228,60 | 1193 | 352490 | clean |

Sabit jetonlar bir kez: `hucre=0 glif=1785 kural=0 yuva=37/1984
yuva2=0/1984 yuk=load hareket=0 profil=release dusen=0 gpu_elenen=0
taban=20 pipeline=ok`; `icerik`/`ornek`/`gpu_ornek` = `kare` (8. ve 10.
koşuda biri bir eksik, 8.'de `kayma=1`). `sessiz=0.00ms` dokuz koşuda. Tam
satırlar `target/olcum-kare/pouring-fix/run-*.out`.

**Sapma ayıklanmadı:** 6. koşu `kare=514`, `sessiz=5696.41ms` — link
koşunun yarısından fazlasını uyuyarak geçirdi (pencere örtülmüş olabilir;
sebebi aranmadı). p95'leri öteki dokuzla aynı bantta.

**Sonuç.** `cpu_encode_p95` **0,25–0,28** (önce 0,36–0,39), `cpu_kare_p95`
**0,07–0,08** (önce 0,13–0,14), `gpu_p95` **0,25** (önce 0,43–0,49).
`cpu_kare` ile `gpu` `5f74180`'in sayısına (0,06–0,07 / 0,25) döndü, yani
birinci basamağın payı tamamen kalktı; encode'da kalan ~+0,03 ikinci
basamağın (`c0b9571`, glyph_fx, +0,04) ve üçüncü küçük kaymanın payı,
dokunulmadı.

### 2026-09-30 — 0,24 → 0,41 sıçraması: sessiz makinede iki commit ve bisect

2026-09-28 bloğunun ayıramadığı soru: artış koddan mı, makinenin yükünden
mi? Cevap **ikisi de, ama çoğu koddan**. Reçete 2026-09-28'inkinin aynısı:
release, binary doğrudan, `BT_FRAME_STATS=1 BT_SCROLL_TEST=1`, 10 saniye, 10
koşu. MacBook Pro M1 Pro, macOS 26.4.1 (25E253), rustc 1.88.0, **prizde**
(%100, dolu, düşük güç kipi kapalı), `kare / süre ≈ 119`. **Makine sessiz:**
tarayıcı ve ağır uygulamalar kapalı, yük ortalaması 1,54 (5 dk'lık 2,23),
`top`'ta %6'yı geçen süreç yok (kernel_task 7,3, WindowServer 5,6). Açık
kalan tek bateri kullanıcının kendi `/Applications/bateri.app`'i (%3,1).
Eski commit `git worktree` ile ayrı dizinde derlendi; ana ağaca dokunulmadı.

**HEAD `651c0a8`** (Metal yolu; wgpu yalnız dev-dependency)

| # | cpu_kare p95 / max | cpu_encode p95 / max | gpu p95 / max | acilis | kare | istek | kapanis |
|---|---|---|---|---|---|---|---|
| 1 | 0,13 / 0,32 | 0,37 / 2,00 | 0,43 / 0,72 | 288,26 | 1190 | 351243 | abandoned |
| 2 | 0,13 / **10,13** | 0,39 / 1,47 | 0,49 / 0,90 | 251,51 | 1190 | 328978 | abandoned |
| 3 | 0,14 / 0,21 | 0,36 / 1,53 | 0,49 / 0,91 | 225,13 | 1194 | 358515 | clean |
| 4 | 0,14 / 0,23 | 0,37 / 1,48 | 0,43 / 0,70 | 213,20 | 1194 | 351352 | clean |
| 5 | 0,14 / 0,20 | 0,37 / 1,54 | 0,43 / 0,58 | 212,44 | 1194 | 361343 | abandoned |
| 6 | 0,14 / 0,21 | 0,38 / 1,58 | 0,49 / 0,95 | 230,52 | 1193 | 354634 | abandoned |
| 7 | 0,14 / 0,21 | 0,37 / 1,41 | 0,49 / 0,58 | 222,76 | 1193 | 360815 | clean |
| 8 | 0,14 / 0,20 | 0,37 / 1,44 | 0,49 / 0,71 | 215,64 | 1194 | 357369 | abandoned |
| 9 | 0,14 / 0,19 | 0,39 / 1,41 | 0,49 / 0,58 | 218,41 | 1181 | 339439 | clean |
| 10 | 0,14 / 0,50 | 0,37 / 1,36 | 0,49 / 1,04 | 235,59 | 1193 | 362224 | abandoned |

**`5f74180`** (2026-09-21 tabanının commit'i, bugün aynı makinede)

| # | cpu_kare p95 / max | cpu_encode p95 / max | gpu p95 / max | acilis | kare | istek | kapanis |
|---|---|---|---|---|---|---|---|
| 1 | 0,06 / 0,15 | 0,23 / 1,69 | 0,25 / 0,56 | 217,50 | 1192 | 345133 | abandoned |
| 2 | 0,06 / 0,16 | 0,23 / 1,42 | 0,25 / 0,61 | 210,02 | 1191 | 341934 | abandoned |
| 3 | 0,07 / 0,31 | 0,23 / 1,52 | 0,25 / 0,58 | 220,65 | 1193 | 342609 | abandoned |
| 4 | 0,06 / 1,50 | 0,23 / 1,62 | 0,25 / 0,72 | 212,49 | 1193 | 341226 | clean |
| 5 | 0,06 / 0,15 | 0,23 / 1,56 | 0,25 / 0,58 | 217,98 | 1193 | 348629 | abandoned |
| 6 | 0,07 / 0,14 | 0,23 / 1,48 | 0,25 / 0,58 | 211,79 | 1192 | 344137 | abandoned |
| 7 | 0,06 / 0,16 | 0,23 / 1,42 | 0,25 / 0,58 | 225,26 | **1110** | **157789** | clean |
| 8 | 0,06 / 0,15 | 0,23 / 1,47 | 0,25 / 0,58 | 218,39 | 1191 | 349847 | clean |
| 9 | 0,06 / 0,18 | 0,23 / 1,48 | 0,25 / 1,07 | 219,16 | 1192 | 351566 | abandoned |
| 10 | 0,07 / **10,07** | 0,22 / 1,45 | 0,25 / 0,84 | 217,22 | 1192 | 348612 | clean |

Sabit jetonlar bir kez (iki commit'te de): `hucre=0 glif=1785 kural=0
yuva=37/1984 yuk=load sessiz=0.00ms profil=release dusen=0 gpu_elenen=0
taban=20 pipeline=ok`; HEAD'de ayrıca `yuva2=0/1984` (5f74180 o jetonu
henüz basmıyordu). `icerik`/`ornek`/`gpu_ornek` = `kare`; HEAD 2. koşuda
`hareket=2 kayma=2`, 10. koşuda `kayma=1`, 5f74180 3. ve 10. koşuda
`hareket=1` — o koşularda `ornek` bir ya da iki eksik (`## Yöntem`,
üçüncü kalem). Hiçbir koşuda `insufficient` yok. Tam satırlar
`target/olcum-kare/sicrama-{head,5f74180}/run-*.out`.

**Sapmalar ayıklanmadı:** 5f74180 7. koşu `kare=1110`, `istek` diğerlerinin
yarısı — rejim oranı 111, geri kalanların 119'u; sebebi aranmadı.
`cpu_kare_max` ~10 ms iki commit'te de bir kez (HEAD 2., 5f74180 10.) —
2026-09-21'in pildeki 10,08'i ile aynı imza, yani yeni değil.

**Ayrışma.** `cpu_encode_p95`: 5f74180 **0,22–0,23**, HEAD **0,36–0,39**
(2026-09-28, yüklü makine: 0,40–0,42). `cpu_kare_p95`: 0,06–0,07 → 0,13–0,14
(yüklü: 0,14–0,15). Dağılımlar örtüşmüyor. Yani 0,24 → 0,41'in ~0,14 ms'i
**kod**, kalan ~0,03 ms 2026-09-28'deki tarayıcı yükü. 5f74180 bugün
2026-09-21'in kendi sayısını (0,23–0,24) verdi, yani makine aynı yerde.
**040 phase-5'in karşılaştırma tabanı olarak bu HEAD tablosu 2026-09-28'inkinden
temiz** (aynı kod yolu, sessiz makine).

**Bisect** (aynı reçete, adım başına 3 koşu; kararın ölçütü üç `cpu_encode_p95`'in
medyanı). Birinci tur `5f74180..651c0a8`, eşik 0,30; ikinci tur ikinci bir
basamak için `82d07bc..88140d0`, eşik 0,345. Adım satırları taban değil:

| commit | cpu_encode_p95 (3 koşu) | cpu_kare_p95 | gpu_p95 | karar |
|---|---|---|---|---|
| `04d5ce4` | 0,23 0,23 0,24 | 0,07 0,07 0,07 | 0,25 | iyi (son iyi) |
| `0777e13` | 0,32 0,33 0,33 | 0,12 0,12 0,12 | 0,45 | **birinci basamak** |
| `3573c98` | 0,23 0,23 0,24 | 0,07 | 0,25 | iyi |
| `3cdc11a` | 0,23 0,23 0,23 | 0,06–0,07 | 0,25 | iyi |
| `e479537` | 0,32 0,33 0,32 | 0,12 | 0,27 0,45 0,44 | kötü |
| `82d07bc` | 0,32 0,33 0,32 | 0,12 | 0,45 0,45 0,27 | kötü |
| `9607214` | 0,32 0,33 0,32 | 0,12 | 0,45 | ikinci turda iyi |
| `07a1579` | 0,32 0,33 0,33 | 0,12 | 0,45–0,51 | ikinci turda iyi (son iyi) |
| `c0b9571` | 0,37 0,36 0,36 | 0,11–0,12 | 0,45–0,46 | **ikinci basamak** |
| `9b2318b` | 0,36 0,36 0,36 | 0,11–0,12 | 0,46–0,47 | kötü |
| `88140d0` | 0,38 0,37 0,36 | 0,12 | 0,45–0,46 | kötü |

Adım çıktıları commit'lenmedi (oturumun geçici dizininde kaldı).

**Birinci basamak — `0777e13` "Dolu ızgarada taşan çıktıyı da süzdür"**,
+0,10 ms encode. Ayırıcı: **üç sütun birlikte** sıçradı (`cpu_kare` 0,07 →
0,12, `gpu_p95` 0,25 → 0,45), yani `frame()` daha fazla hücre üretiyor ve
GPU daha fazla çiziyor — encode'un kendisi pahalılaşmadı. Mekanizma **kod
okumasından, ölçümle ayrılmadı** (kod değiştirmek bu ölçümün kapsamı
dışındaydı): `Motion::scroll_in`'in yeni patlama kolu durgun ızgarada
ötelemeyi `target + rows`'a koyuyor, sonraki içerik karesinde çizen taraf bu
ötelemeyi `set_grid_top`'la bildiriyor ve `Session::slide_fill_rows`
doldurma bandını bir ekran boyu uzatıyor — patlama karesi iki ekran çiziyor.
`BT_SCROLL_TEST` her karede ekrandan fazla satır kaydırıyor (45–467) ve
"akış" ayrımının hafızası yok: N'de patlama başlıyor, N+1'de kayma uçuşta
olduğu için bitiriliyor, N+2'de ızgara yine durgun ve patlama yeniden —
**karelerin yaklaşık yarısı** çift ekran çiziyor ve p95 o yarıya düşüyor.
Doldurma bandının listeleri sayaçlardan muaf olduğu için `glif=` bunu
göstermiyor, `kayma=` da göstermiyor (yalnız hasarsız hareket karesini
sayıyor). Doğrulamanın yolu: yalnız worktree'de patlama kolunu kapatıp aynı
reçeteyle ölçmek; 0,23'e dönüş mekanizmayı kanıtlar.

**İkinci basamak — `c0b9571` "Dock'ta yazılan harfi belirt… glyph_fx
pipeline'ı"** (030 phase-2), +0,04 ms encode. Ayırıcı: `cpu_kare` **oynamadı**
(0,12 → 0,11–0,12), `gpu_p95` de, yani ek maliyet encode + commit'in
içinde, `renderer.rs`'te. Ölçüm yükünde dock yok, yani efektlerin kendisi
koşmuyor; aday glyph başına `prepare` yolunun `fan` yardımcısına taşınması
(dizi dönen, her glyph'te çağrılan). Commit büyük (13 dosya) ve sebep
**ayrılmadı**.

**Üçüncü, küçük ve bisect'lenmemiş bir kayma:** `88140d0..651c0a8` (73
commit) arasında `cpu_kare_p95` 0,12 → 0,13–0,14; encode 0,36–0,38 →
0,36–0,39 (ayırt edilemez).

### 2026-09-28 — Metal pencere yolu tabanı, wgpu geçişinden önce (040 phase-2)

wgpu geçişinin (040 phase-5) karşılaştıracağı taban; Metal sökülünce bir
daha alınamaz. Ölçülen ağaç `27d295b` + 040 phase-2'nin commit'lenmemiş
değişikliği (yalnız `cfg(test)` kodu ve dev-dependency — ürün binary'sine
girmiyor), binary doğrudan çağrıldı. MacBook Pro M1 Pro, macOS 26.4.1
(25E253), rustc 1.88.0, `kare / süre ≈ 120`. Yük `BT_SCROLL_TEST=1`, 10
saniye, profil başına 10 koşu, **prizde** (%86, şarj oluyor, düşük güç kipi
kapalı). **Makine sessiz değildi:** bir tarayıcı süreci ~%77 CPU'daydı, yük
ortalaması ~4 — kullanıcının oturumu, kapatılamadı.

**release**

| # | cpu_kare p95 / max | cpu_encode p95 / max | gpu p95 / max | acilis | kare | istek | kapanis |
|---|---|---|---|---|---|---|---|
| 1 | 0,14 / 0,21 | 0,41 / 1,59 | 0,57 / 3,79 | 314,07 | 1197 | 275290 | abandoned |
| 2 | 0,14 / 0,44 | 0,42 / 1,76 | 0,54 / 2,71 | 200,51 | 1198 | 263791 | clean |
| 3 | 0,14 / 0,45 | 0,42 / 1,42 | 0,54 / 2,18 | 199,32 | 1197 | 273700 | clean |
| 4 | 0,14 / 0,34 | 0,41 / 1,47 | 0,46 / 3,90 | 216,61 | 1198 | 276873 | abandoned |
| 5 | 0,15 / 0,40 | 0,40 / 1,55 | 0,49 / 3,96 | 249,51 | 1197 | 274837 | clean |
| 6 | 0,14 / 0,24 | 0,40 / 1,49 | 0,37 / 2,06 | 192,53 | 1198 | 277851 | abandoned |
| 7 | 0,14 / 0,23 | 0,42 / 1,54 | 0,56 / 2,08 | 224,24 | 1198 | 267472 | abandoned |
| 8 | 0,14 / 0,22 | 0,41 / 1,34 | 0,39 / 2,06 | 218,53 | 1199 | 284002 | abandoned |
| 9 | 0,14 / 0,44 | 0,42 / 1,48 | 0,47 / 2,09 | 215,00 | 1198 | 267210 | clean |
| 10 | 0,14 / 0,63 | 0,41 / 1,55 | 0,48 / 2,07 | 218,43 | 1198 | 272021 | clean |

**debug**

| # | cpu_kare p95 / max | cpu_encode p95 / max | gpu p95 / max | acilis | kare | istek | kapanis |
|---|---|---|---|---|---|---|---|
| 1 | 3,05 / 4,00 | 2,38 / 3,03 | 1,55 / 1,90 | 303,76 | 1198 | 2646 | clean |
| 2 | 3,05 / 4,17 | 2,35 / 3,03 | 1,63 / 2,27 | 223,71 | 1198 | 2515 | clean |
| 3 | 3,04 / 3,88 | 2,37 / 2,94 | 1,61 / 2,23 | 215,53 | 1198 | 2614 | clean |
| 4 | 3,10 / 3,65 | 2,36 / 3,00 | 1,59 / 2,79 | 205,92 | 1198 | 2578 | clean |
| 5 | 3,12 / 4,29 | 2,36 / 2,94 | 1,54 / 2,19 | 223,04 | 1198 | 2572 | clean |
| 6 | 3,05 / 4,03 | 2,39 / 2,71 | 1,49 / 1,88 | 204,91 | 1197 | 2763 | clean |
| 7 | 3,10 / 11,71 | 2,39 / 2,81 | 1,52 / 3,02 | 221,84 | 1197 | 2594 | clean |
| 8 | 3,09 / 5,00 | 2,35 / 2,87 | 1,48 / 2,82 | 208,11 | 1198 | 2629 | clean |
| 9 | 3,07 / 3,98 | 2,36 / 3,02 | 1,60 / 2,42 | 224,41 | 1198 | 2513 | abandoned |
| 10 | 3,17 / 4,21 | 2,36 / 2,97 | 1,50 / 2,43 | 238,25 | 1197 | 2521 | clean |

Sabit jetonlar bir kez (iki profilde de): `hucre=0 glif=1785 kural=0
yuva=37/1984 yuva2=0/1984 yuk=load hareket=0 sessiz=0.00ms dusen=0
gpu_elenen=0 taban=20 pipeline=ok`; `ornek`/`gpu_ornek`/`icerik` = `kare`
(debug 1. ve 8. koşuda `ornek` ve `icerik` bir eksik, `kayma=1` — hareket
karesi CPU örneği yazmıyor, `## Yöntem` üçüncü kalem). Tam satırlar
`target/olcum-kare/040-{release,debug}-run-*.out`.

**release CPU tabanı:** `cpu_kare_p95` 0,14–0,15 ms, `cpu_encode_p95`
0,40–0,42 ms; uçlar `cpu_kare_max` 0,21–0,63, `cpu_encode_max` 1,34–1,76.
**debug:** 3,04–3,17 ve 2,35–2,39 ms. GPU sütunu raporlanıyor, taban değil
(0,37–0,57 ms; gezintisi aşağıda).

**2026-09-21 tabanından yüksek** (sebebi 2026-09-30'da bulundu, yukarıdaki
blok: çoğu kod, ~0,03 ms makinenin yükü)**:** o gün `cpu_kare_p95`
0,07–0,08, `cpu_encode_p95` 0,23–0,24 ms'ydi; iki dağılım örtüşmüyor. Arada
iki şey değişti ve ayrılmadı — kod (o günden bu yana gelen setler, kare yolu
dahil) ve makinenin yükü (yukarıda). Bu tablo 040'ın **karşılaştırma**
tabanı: phase-5'in wgpu pencere yolu aynı koşullarla (aynı gün, aynı yük)
ölçülmeli, 2026-09-21'le değil.

### 2026-09-21 — taban: CPU ve açılış (prizde); GPU alınamadı

Ölçülen commit `5f74180`, `profil=release`, binary doğrudan çağrıldı. MacBook
Pro M1 Pro, 32 GB, macOS 26.4.1 (25E253), rustc 1.88.0, Retina 3024×1964,
`kare / süre ≈ 118`. Yük `BT_SCROLL_TEST=1` (`load_shell`), 10 saniye,
10 koşu, **prizde** (%82 → %84, şarj oluyor, düşük güç kipi kapalı).

| # | cpu_kare p95 / max | cpu_encode p95 / max | gpu p95 / max | acilis | kare | kapanis |
|---|---|---|---|---|---|---|
| 1 | 0,08 / 0,79 | 0,23 / 2,06 | 0,63 / 0,92 | 318,84 | 1188 | clean |
| 2 | 0,08 / 0,17 | 0,24 / 1,51 | 0,63 / 0,76 | 233,57 | 1190 | abandoned |
| 3 | 0,08 / **2,76** | 0,24 / 1,69 | 0,63 / 1,27 | 329,58 | 1187 | abandoned |
| 4 | 0,07 / 0,19 | 0,24 / 1,47 | 0,63 / 1,45 | 242,98 | 1189 | clean |
| 5 | 0,07 / 0,67 | 0,24 / 1,52 | 0,63 / 0,75 | 264,85 | 1190 | clean |
| 6 | 0,07 / 0,11 | 0,24 / 1,54 | 0,63 / 1,28 | 257,52 | 1191 | clean |
| 7 | 0,08 / 0,15 | 0,24 / 1,50 | 0,63 / 1,24 | 268,98 | 1192 | abandoned |
| 8 | 0,08 / 0,16 | 0,24 / 1,64 | 0,63 / 1,23 | 250,61 | 1184 | abandoned |
| 9 | 0,08 / 0,13 | 0,23 / 1,67 | 0,63 / 1,78 | 266,36 | 1190 | clean |
| 10 | 0,07 / 0,16 | 0,24 / 1,51 | 0,63 / 1,28 | 246,10 | 1191 | clean |

Sabit jetonlar bir kez: `hucre=0 kural=0 yuva=37/1984 yuk=load hareket=0
kayma=0 sessiz=0.00ms profil=release dusen=0 gpu_elenen=0 taban=20
pipeline=ok`, `glif` onunda da 1785. Oynayanlar: `istek` 282 770 – 296 754,
`icerik` = `kare` (9. ve 10. koşuda +1, `Measured::read`'in yazdığı bir
örneklik kayma), `gpu_ornek` = `kare`, `kapanis` altı `clean` dört
`abandoned`.

**CPU tarafı taban.** `cpu_kare_p95` 0,07 (4., 5., 6., 10. koşu) ile 0,08
(kalan altısı) arasında; `cpu_encode_p95` sekiz koşuda 0,24, ikisinde 0,23.
120 Hz'in kare bütçesi 8,33 ms, yani `session.frame`'in tamamı bütçenin
**%1'i**, encode **%3'ü**. Uçlar: `cpu_kare_max` dokuz koşuda 0,11–0,79 ms,
**3. koşu 2,76 ms** — bütçenin altında ama dağılımın on beş katı, sebebi
aranmadı ve **ayıklanmadı**. `cpu_encode_max` 1,47–2,06 ms.

**Pildeki blokla karşılaştırma:** CPU sütunları iki blok arasında oynamadı
(pil 0,07–0,08 ve 0,24–0,27, priz 0,07–0,08 ve 0,23–0,24), yani bu iki sütun
güç durumundan bağımsız. Aşağıdaki pil tablosu **gözlem olarak** duruyor;
taban bu tablodur.

**GPU sütunu tabana girmiyor.** Bu bloğun onunda da `0,63` — blok içi
kararlılık çarpıcı, ama aynı gün aynı binary `0,25` ve `0,68` de verdi.
Gezintinin tamamı ve sınanan dört hipotez aşağıda.

### 2026-09-21 — GPU sütununun gezintisi (taban neden alınamadı)

Kronolojik, hepsi aynı kaynak ve aynı makine; `gpu_p95`:

| # | blok | güç | `gpu_p95` |
|---|---|---|---|
| 1 | `cargo build` sonrası ilk koşu (5 sn) | pil %51 | **0,25** |
| 2 | 10 koşu × 10 sn | pil %28→26 | **0,68** (onunda da) |
| 3 | 3 doğrulama koşusu (5 sn) | pil %25 | **0,68** (üçünde de) |
| 4 | shader `touch` + yeniden derleme, 2 koşu | pil %25 | **0,63** (ikisinde de) |
| 5 | 10 koşu × 10 sn (**yukarıdaki tablo**) | priz %82→84 | **0,63** (onunda da) |
| 6 | shader `touch` + yeniden derleme, 2 koşu | priz %84 | **0,25** (ikisinde de) |
| 7 | 6 koşu, yeniden derleme yok | priz %86 | 0,32 0,34 0,31 0,25 0,25 0,25 |
| 8 | shader `touch` + yeniden derleme, 3 koşu | priz %86 | 0,66 0,62 0,56 |

**Dört hipotez sınandı, dördü de ayırmadı:**

1. **Koşu süresi** — 5 sn de 10 sn de aynı değeri verdi (#2 ile #3).
2. **Derlemeden sonraki ilk koşu** — yeniden derleme değeri bir kez indirdi
   (#4, #6), bir kez **çıkardı** (#8). Yön tutarlı değil.
3. **Güç durumu** — `0,63` hem pil %25'te (#4) hem priz %82'de (#5); `0,25`
   hem pil %51'de (#1) hem priz %86'da (#6, #7). **Ayırıcı değil.** Bu
   hipotez 2026-09-21'de bir kez yazıldı ve **aynı gün prizdeki koşuyla
   çürütüldü**; kayıt olarak burada duruyor.
4. **Metallib kimliği** — #8'in yeniden derlemesinden önce ve sonra
   `shasum -a 256 target/release/build/bt-gpu-*/out/default.metallib`
   **aynı** (`9f0a7969…`), yani shader ikilisi bayt bayt değişmedi ama sayı
   yine sıçradı. Shader **aklandı**.

Geriye binary'mizin dışındaki bir şey kalıyor — GPU saat durumu, başka bir
GPU tüketicisi, compositor — ve bu ölçümün araçları oraya yetmiyor. `#7`
ayrıca gösteriyor ki değer her zaman iki ondalıkta donmuyor: 0,32/0,34/0,31
sonra 0,25. Yani "P-state gibi" bir benzetme değil, **tarif** yazıldı.

### 2026-09-21 — pildeki blok (gözlem, taban değil)

Ortam üsttekiyle aynı; tek fark makine **pilde** koştu (%28 → %26, düşük güç
kipi kapalı). Yöntem prizde olmayı şart koştuğu için bu blok **taban değil
gözlem** olarak duruyor ve taban üstteki priz bloğudur. Kayıtta kalmasının
sebebi kıyas: CPU sütunlarının ve açılışın güç durumundan **bağımsız**
olduğu bu iki bloğun yan yana durmasından okunuyor — gezinti yalnız GPU
sütununda.

| # | cpu_kare p95 / max | cpu_encode p95 / max | gpu p95 / max | acilis | kare | kapanis |
|---|---|---|---|---|---|---|
| 1 | 0,08 / 0,45 | 0,24 / 2,18 | 0,68 / 0,97 | 392,23 | 1178 | clean |
| 2 | 0,07 / 0,55 | 0,25 / 3,51 | 0,68 / 1,02 | 262,59 | 1187 | clean |
| 3 | 0,08 / 0,47 | 0,24 / 1,60 | 0,68 / 1,10 | 255,54 | 1191 | abandoned |
| 4 | 0,08 / 0,58 | 0,26 / 5,11 | 0,68 / 1,06 | 256,26 | 1183 | abandoned |
| 5 | 0,08 / 0,58 | 0,24 / 1,54 | 0,68 / 1,82 | 275,16 | 1184 | clean |
| 6 | 0,07 / 0,19 | 0,25 / 1,67 | 0,68 / 1,14 | 253,53 | 1186 | abandoned |
| 7 | 0,07 / **10,08** | 0,24 / 1,90 | 0,68 / 1,72 | 262,53 | 1187 | abandoned |
| 8 | 0,08 / 0,21 | 0,25 / 1,56 | 0,68 / 0,81 | 263,39 | 1191 | abandoned |
| 9 | 0,07 / 0,15 | 0,25 / 1,55 | 0,68 / 1,02 | 263,72 | 1192 | clean |
| 10 | 0,08 / 0,22 | 0,27 / 2,49 | 0,68 / 1,25 | 263,55 | 1178 | abandoned |

Bütün koşularda sabit kalan jetonlar bir kez: `hucre=0 kural=0 yuva=37/1984
yuk=load kayma=0 sessiz=0.00ms profil=release dusen=0 gpu_elenen=0 taban=20
pipeline=ok`. Oynayanlar: `glif` 1785 (biri 1836), `istek` 247 680 – 273 227,
`hareket` sekiz koşuda 0 (1. ve 3. koşuda 1, 10. koşuda 11), `icerik`
1167–1192, `gpu_ornek` **onunda da `kare`'ye eşit**, `ornek` ise `icerik`'e
eşit — yani `ornek = kare − hareket` (2. koşuda +1 sapıyor, `Measured::read`'in
yazdığı bir örneklik kayma).

**Dağılım.** `cpu_kare_p95` iki değere oturuyor (0,07 **dört** koşuda —
2., 6., 7., 9.; 0,08 kalan **altı**sında); `cpu_encode_p95` 0,24–0,27;
`gpu_p95` **onunda da 0,68**. 120 Hz'in
kare bütçesi 8,33 ms, yani üç sütunun p95'i bütçenin sırasıyla %1, %3 ve %8'i.

**Uçlar sayılıyor, ortalamaya gömülmüyor.** `cpu_kare_max` dokuz koşuda
0,15–0,58 ms; **7. koşu 10,08 ms** ile bütçeyi aşan tek takılma. Bir kez
görüldü, sebebi aranmadı ve **ayıklanmadı** — gürültü kuralı nedeni
bulunamayan koşuyu dağılımda tutuyor. `cpu_encode_max` 1,54–5,11 ms ve bu
sütunun uçları p95'inin yirmi katına çıkıyor; `gpu_max` 0,81–1,82 ms, yani
GPU tarafı en kötü karede bile bütçenin dörtte birinde.

**Okunuşu.** Bu yükte kare süresi bütçenin çok altında ve darboğaz CPU değil:
`cpu_kare_p95` 0,08 ms'de, yani `session.frame`'in tamamı (kilit + ayrıştırma
+ grid + sink) kare bütçesinin yüzde biri. Encode tarafı üç kat pahalı ama
hâlâ %3. Ölçülmemiş olan, yani 022'nin soracağı şey, GPU'nun prizdeki hâli.

**Jetonların okunuşu — sabit sıfırların sebebi reçetede.** `load_shell` düz
ASCII basıyor ve tek bir SGR dizisi içermiyor, yani varsayılan zemin dışında
arka plan yok (`hucre=0`) ve altı çizili/üstü çizili hücre yok (`kural=0`).
`yuva=37/1984` aynı sebepten dar: yük ASCII'nin ötesine çıkmıyor.
`istek` ile `kare` arasındaki üç mertebelik fark bu dosyanın kayıtlı açık
kalemidir (`## Yöntem`, altıncı kalem), bu koşuda da görüldü ve
**yorumlanmadı**.

### Yolun ateşlendiğinin tanığı

Her iki blokta da üç sütunun üçü de dolu ve `ornek` tabanın (`taban=20`)
elli katı; hiçbir koşuda `insufficient` görülmedi, yani örnekleme durmadı.
`kare / süre ≈ 118` — tazeleme hızı, yani pencere görünür ve tam hızda
çiziyor (rejim tanığı, `## Yöntem` altıncı kalem). Yolun gerçekten
koştuğunun ikinci tanığı yukarıdaki **gezinti tablosunun kendisi**: kapalı
bir ölçüm yolu on koşuda aynı sayıyı verirdi, bloklar arası sıçrama
üretmezdi.

## Açılış

### 2026-09-28 — Metal pencere yolu, wgpu geçişinden önce (040 phase-2)

`## Kare süresi` → 2026-09-28 bloğunun `acilis=` jetonu (ortam orada; makine
sessiz değildi). release **192,53 – 314,07 ms**, dokuz koşu 192–250 ve 1.
koşu (derlemeden sonraki ilk koşu) 314,07; debug **204,91 – 303,76 ms**, yine
1. koşu ayrışıyor. 2026-09-21'in priz tabanıyla (233–330) örtüşüyor.

### 2026-09-21 — taban (prizde)

Üstteki priz bloğunun `acilis=` jetonu; ortam aynı, sınırları `## Yöntem` →
Kare süresi ve açılış, birinci kalem (iki ucundan da kısa: `main()`'in ilk
satırından ilk **tamamlanan** kareye — süreç başlangıcı değil, sunulan kare
değil).

**233,57 – 329,58 ms**, ortası ~259 ms. Sekiz koşu 233–269 aralığında;
**1. koşu 318,84 ve 3. koşu 329,58** ile ayrışıyor. 1. koşunun derlemeden
sonraki ilk koşu olması bir açıklama **adayı**, ama 3. koşu için böyle bir
gerekçe yok ve ikisi de **aranmadı**: sebebi bulunamayan koşu dağılımda
kalıyor.

Pildeki blok (gözlem) 253,53 – 392,23 ms verdi ve oradaki 392,23 de
derlemeden sonraki ilk koşuydu. İki bloğun sıcak koşuları örtüşüyor
(priz 233–269, pil 253–275), yani **açılış güç durumundan etkilenmiyor** —
kare süresinin CPU sütunlarıyla aynı sonuç.

## wgpu denemesi

### 2026-09-28 — `cell_bg` + caret, offscreen, iki arka uç (040 phase-2)

Yöntem `## Yöntem` → wgpu denemesi. Ortam `## Kare süresi` → 2026-09-28
bloğunun aynısı (aynı makine, aynı oturum, prizde, makine sessiz değil).
wgpu 30.0.1, Metal arka ucu. Değerler mikrosaniye, her satır bir koşunun
p95'i (1000 kare).

| profil | sütun | Metal (10 koşu) | wgpu (10 koşu) |
|---|---|---|---|
| release | `cpu_kare_p95` (tanık) | 55,0 – 61,8 | 53,5 – 62,1 |
| release | **`cpu_encode_p95`** | **95,4 – 107,2** | **172,4 – 190,6** |
| release | `cpu_encode_max` | 158,8 – 293,9 | 246,8 – 751,0 |
| release | `gpu_p95` | 114,0 – 266,4 | unsupported |
| debug | `cpu_kare_p95` (tanık) | 679,4 – 702,5 | 684,2 – 705,4 |
| debug | `cpu_encode_p95` | 294,6 – 315,4 | 746,7 – 823,3 |
| debug | `cpu_encode_max` | 366,2 – 497,0 | 936,6 – 1516,9 |
| debug | `gpu_p95` | 414,7 – 557,5 | unsupported |

Koşu başına değerler (release `cpu_encode_p95`, sırayla): Metal 106,1 103,9
101,8 106,1 106,3 104,6 107,2 106,1 102,0 95,4; wgpu 190,6 178,4 182,1 181,2
177,1 181,7 179,8 181,7 172,4 178,3. Tam satırlar, koşu sırasıyla (her
koşu iki satır, önce Metal):

```text
# release
arka_uc=metal profil=release kare=1000 ornek=1000 cpu_kare_p95=61.8us cpu_kare_max=136.8us cpu_encode_p95=106.1us cpu_encode_max=211.2us gpu_p95=266.4us gpu_max=1816.1us
arka_uc=wgpu profil=release kare=1000 ornek=1000 cpu_kare_p95=62.1us cpu_kare_max=100.3us cpu_encode_p95=190.6us cpu_encode_max=520.9us gpu_p95=unsupported gpu_max=unsupported
arka_uc=metal profil=release kare=1000 ornek=1000 cpu_kare_p95=58.2us cpu_kare_max=142.4us cpu_encode_p95=103.9us cpu_encode_max=158.8us gpu_p95=124.1us gpu_max=1809.1us
arka_uc=wgpu profil=release kare=1000 ornek=1000 cpu_kare_p95=59.8us cpu_kare_max=144.3us cpu_encode_p95=178.4us cpu_encode_max=267.4us gpu_p95=unsupported gpu_max=unsupported
arka_uc=metal profil=release kare=1000 ornek=1000 cpu_kare_p95=61.4us cpu_kare_max=157.9us cpu_encode_p95=101.8us cpu_encode_max=214.7us gpu_p95=149.6us gpu_max=630.0us
arka_uc=wgpu profil=release kare=1000 ornek=1000 cpu_kare_p95=58.2us cpu_kare_max=119.6us cpu_encode_p95=182.1us cpu_encode_max=277.6us gpu_p95=unsupported gpu_max=unsupported
arka_uc=metal profil=release kare=1000 ornek=1000 cpu_kare_p95=57.2us cpu_kare_max=153.0us cpu_encode_p95=106.1us cpu_encode_max=231.8us gpu_p95=118.1us gpu_max=1436.1us
arka_uc=wgpu profil=release kare=1000 ornek=1000 cpu_kare_p95=59.1us cpu_kare_max=132.4us cpu_encode_p95=181.2us cpu_encode_max=751.0us gpu_p95=unsupported gpu_max=unsupported
arka_uc=metal profil=release kare=1000 ornek=1000 cpu_kare_p95=55.1us cpu_kare_max=129.2us cpu_encode_p95=106.3us cpu_encode_max=179.6us gpu_p95=146.2us gpu_max=657.0us
arka_uc=wgpu profil=release kare=1000 ornek=1000 cpu_kare_p95=58.4us cpu_kare_max=120.5us cpu_encode_p95=177.1us cpu_encode_max=246.8us gpu_p95=unsupported gpu_max=unsupported
arka_uc=metal profil=release kare=1000 ornek=1000 cpu_kare_p95=55.8us cpu_kare_max=132.2us cpu_encode_p95=104.6us cpu_encode_max=248.0us gpu_p95=127.7us gpu_max=671.7us
arka_uc=wgpu profil=release kare=1000 ornek=1000 cpu_kare_p95=59.0us cpu_kare_max=104.4us cpu_encode_p95=181.7us cpu_encode_max=302.0us gpu_p95=unsupported gpu_max=unsupported
arka_uc=metal profil=release kare=1000 ornek=1000 cpu_kare_p95=57.8us cpu_kare_max=144.5us cpu_encode_p95=107.2us cpu_encode_max=174.9us gpu_p95=166.2us gpu_max=659.0us
arka_uc=wgpu profil=release kare=1000 ornek=1000 cpu_kare_p95=59.3us cpu_kare_max=118.3us cpu_encode_p95=179.8us cpu_encode_max=308.0us gpu_p95=unsupported gpu_max=unsupported
arka_uc=metal profil=release kare=1000 ornek=1000 cpu_kare_p95=56.6us cpu_kare_max=124.5us cpu_encode_p95=106.1us cpu_encode_max=205.0us gpu_p95=161.5us gpu_max=627.1us
arka_uc=wgpu profil=release kare=1000 ornek=1000 cpu_kare_p95=58.7us cpu_kare_max=136.4us cpu_encode_p95=181.7us cpu_encode_max=347.1us gpu_p95=unsupported gpu_max=unsupported
arka_uc=metal profil=release kare=1000 ornek=1000 cpu_kare_p95=55.2us cpu_kare_max=120.8us cpu_encode_p95=102.0us cpu_encode_max=194.0us gpu_p95=114.0us gpu_max=670.8us
arka_uc=wgpu profil=release kare=1000 ornek=1000 cpu_kare_p95=53.5us cpu_kare_max=123.1us cpu_encode_p95=172.4us cpu_encode_max=263.6us gpu_p95=unsupported gpu_max=unsupported
arka_uc=metal profil=release kare=1000 ornek=1000 cpu_kare_p95=55.0us cpu_kare_max=134.7us cpu_encode_p95=95.4us cpu_encode_max=293.9us gpu_p95=114.4us gpu_max=600.3us
arka_uc=wgpu profil=release kare=1000 ornek=1000 cpu_kare_p95=58.8us cpu_kare_max=97.5us cpu_encode_p95=178.3us cpu_encode_max=286.2us gpu_p95=unsupported gpu_max=unsupported
# debug
arka_uc=metal profil=debug kare=1000 ornek=1000 cpu_kare_p95=683.3us cpu_kare_max=830.2us cpu_encode_p95=294.6us cpu_encode_max=366.2us gpu_p95=419.7us gpu_max=1849.0us
arka_uc=wgpu profil=debug kare=1000 ornek=1000 cpu_kare_p95=690.2us cpu_kare_max=917.8us cpu_encode_p95=746.7us cpu_encode_max=1039.5us gpu_p95=unsupported gpu_max=unsupported
arka_uc=metal profil=debug kare=1000 ornek=1000 cpu_kare_p95=689.1us cpu_kare_max=791.1us cpu_encode_p95=311.2us cpu_encode_max=385.8us gpu_p95=442.6us gpu_max=1806.3us
arka_uc=wgpu profil=debug kare=1000 ornek=1000 cpu_kare_p95=691.8us cpu_kare_max=810.9us cpu_encode_p95=770.8us cpu_encode_max=1000.0us gpu_p95=unsupported gpu_max=unsupported
arka_uc=metal profil=debug kare=1000 ornek=1000 cpu_kare_p95=694.5us cpu_kare_max=864.0us cpu_encode_p95=308.3us cpu_encode_max=387.7us gpu_p95=515.6us gpu_max=1850.9us
arka_uc=wgpu profil=debug kare=1000 ornek=1000 cpu_kare_p95=705.0us cpu_kare_max=1334.7us cpu_encode_p95=786.6us cpu_encode_max=1065.4us gpu_p95=unsupported gpu_max=unsupported
arka_uc=metal profil=debug kare=1000 ornek=1000 cpu_kare_p95=694.2us cpu_kare_max=766.6us cpu_encode_p95=306.8us cpu_encode_max=412.9us gpu_p95=473.2us gpu_max=1853.4us
arka_uc=wgpu profil=debug kare=1000 ornek=1000 cpu_kare_p95=705.4us cpu_kare_max=820.7us cpu_encode_p95=777.8us cpu_encode_max=936.6us gpu_p95=unsupported gpu_max=unsupported
arka_uc=metal profil=debug kare=1000 ornek=1000 cpu_kare_p95=694.2us cpu_kare_max=1227.6us cpu_encode_p95=312.3us cpu_encode_max=449.8us gpu_p95=557.5us gpu_max=1850.1us
arka_uc=wgpu profil=debug kare=1000 ornek=1000 cpu_kare_p95=700.9us cpu_kare_max=1252.3us cpu_encode_p95=823.3us cpu_encode_max=1516.9us gpu_p95=unsupported gpu_max=unsupported
arka_uc=metal profil=debug kare=1000 ornek=1000 cpu_kare_p95=702.5us cpu_kare_max=919.8us cpu_encode_p95=315.4us cpu_encode_max=497.0us gpu_p95=479.6us gpu_max=1783.4us
arka_uc=wgpu profil=debug kare=1000 ornek=1000 cpu_kare_p95=703.7us cpu_kare_max=1557.8us cpu_encode_p95=797.5us cpu_encode_max=1120.7us gpu_p95=unsupported gpu_max=unsupported
arka_uc=metal profil=debug kare=1000 ornek=1000 cpu_kare_p95=681.6us cpu_kare_max=767.7us cpu_encode_p95=301.5us cpu_encode_max=398.8us gpu_p95=414.7us gpu_max=1828.2us
arka_uc=wgpu profil=debug kare=1000 ornek=1000 cpu_kare_p95=686.8us cpu_kare_max=966.5us cpu_encode_p95=765.5us cpu_encode_max=951.9us gpu_p95=unsupported gpu_max=unsupported
arka_uc=metal profil=debug kare=1000 ornek=1000 cpu_kare_p95=687.5us cpu_kare_max=793.2us cpu_encode_p95=307.6us cpu_encode_max=419.3us gpu_p95=441.4us gpu_max=1811.3us
arka_uc=wgpu profil=debug kare=1000 ornek=1000 cpu_kare_p95=686.0us cpu_kare_max=823.6us cpu_encode_p95=771.9us cpu_encode_max=975.2us gpu_p95=unsupported gpu_max=unsupported
arka_uc=metal profil=debug kare=1000 ornek=1000 cpu_kare_p95=679.4us cpu_kare_max=781.7us cpu_encode_p95=296.8us cpu_encode_max=414.3us gpu_p95=422.3us gpu_max=1802.7us
arka_uc=wgpu profil=debug kare=1000 ornek=1000 cpu_kare_p95=684.2us cpu_kare_max=830.2us cpu_encode_p95=765.9us cpu_encode_max=971.1us gpu_p95=unsupported gpu_max=unsupported
arka_uc=metal profil=debug kare=1000 ornek=1000 cpu_kare_p95=688.5us cpu_kare_max=781.2us cpu_encode_p95=307.2us cpu_encode_max=422.8us gpu_p95=457.6us gpu_max=2285.7us
arka_uc=wgpu profil=debug kare=1000 ornek=1000 cpu_kare_p95=684.2us cpu_kare_max=803.2us cpu_encode_p95=778.3us cpu_encode_max=1036.2us gpu_p95=unsupported gpu_max=unsupported
```

**Tanık geçti:** aynı kodun `cpu_kare`'si iki arka uçta örtüşüyor (release
55–62 / 53–62), yani koşu koşulları arka uçlar arasında ayrışmadı.

**Durak kuralı tetiklendi** (kullanıcı kararı 2026-09-29: mutlak ölçek
kabul, set sürüyor): release `cpu_encode_p95` kümeleri örtüşmüyor
(Metal'in en kötüsü 107,2, wgpu'nun en iyisi 172,4) ve wgpu daha kötü —
p95 ortalarında ~1,7 kat. Debug'da ~2,5 kat. Mutlak ölçek: iki release
değeri de 120 Hz'in 8,33 ms'lik kare bütçesinin %1,3 ile %2,3'ü.

**Ayrıştırma — gözlem, taban değil** (tek koşular, release, sıralı ya da
dönüşümlü olduğu yanında):

- Planı boş bir kare (tampon yok, çizim yok; yalnız encoder, pass ve
  `submit`): wgpu `cpu_encode_p95` **57 µs** (sıralı) — wgpu-core'un kare
  başına sabit bedeli, Metal'in bütün karesinin (~105 µs) yarısı.
- Instance tamponu **kare başına** `create_buffer_init` ile: 177 µs (sıralı),
  228 µs (dönüşümlü). Kalıcı tampon + `write_buffer` (ölçülen tasarım): 149
  µs (sıralı), 177–187 µs (dönüşümlü). Tampon stratejisi farkın bir kısmı;
  sabit bedel yapısal.
- wgpu yarısını kare başına bir `autoreleasepool`'a sarmak (Metal yarısında
  var) sayıyı oynatmadı (196 / 185 µs, iki koşu).

## Bekleyen iddialar

Ölçüm bekleyen iddiaların **tek listesi**. 2026-09-22'ye kadar her set kendi
`teslim.md`'sinde taşıyordu ve sonuç sekiz setin süresiz **🔨**'da kalmasıydı:
defter, *alınmış bir kararı* (bench bağımlılığı reddedildi) *bekleyen bir iş*
gibi sayıyordu. Kodun tamamı `main`'de ve çalışıyordu.

Buraya taşınmalarının sebebi sahiplik: bu dosya zaten ölçülmüş sayının tek
sahibi, bekleyen iddianın da sahibi olması gerekiyordu. Setlerin
checklist'lerinde kutuları `[~]` ve gerekçesiyle duruyor — silinmedi, çünkü
silinen kutu atlandığını hiçbir yerde göstermez.

### Emekli — `criterion` alınmadı

Bench iddiaları **kapanmayacak** ve bu bir eksik değil bir karar: `criterion`
ayrı bir bağımlılık kararıdır, kimse bench sayısı istemedi ve `cargo bench
--workspace -- --list` bugün `0 benchmarks` diyor. İstenirse kendi setini
hak ediyor; o gün bu bölüm yeniden yazılır.

- 002 #2'nin yarısı — ayrıştırıcının saf maliyeti
- 003 #1 — `#[inline]` işaretlerinin renk yolundaki kazancı
- 003 #2 — `Atlas::slot`'un hücre başına maliyeti
- 004 #4'ün yarısı — kural instance'larının maliyeti

### GPU sütununun tabanına bağlı

Hepsi "bu değişikliğin kare süresine **etkisi**" biçiminde, yani önce/sonra
karşılaştırması istiyor — ve o karşılaştırma `## Kare süresi`'nde kayıtlı
kararsızlığa düşüyor (aynı kaynakla 0,25–0,68 ms). Sıra bu yüzden tersine
döndü: **önce ölçme yöntemi, sonra bu iddialar.** Aynı yöntem 023 materyal
yüzeyin de ön koşulu.

- 003 #5 — ikinci pipeline ile glyph instance tamponunun etkisi
- 004'ün beş iddiası — sınır `Cell`'i, kural instance'ları, alfa maliyeti
- 019 B.1 — yedek glyph'in etkisi. **İkinci bir sebebi var:** ölçüm yükü
  (`load_shell`) düz ASCII basıyor, yani yedek yoluna hiç girmiyor — bugünkü
  kancayla bu iddianın tanığı yok, ölçüm yolunun kendisi genişlemeli.

### Kancası ya da yükü olmayanlar

- 009 B.2 — tarayıcının akış maliyeti
- 012'nin üç iddiası — dock'un kare yolu (kanca borcu, setin kendi kaydında)
- 013 — "saat armed'ken kare maliyeti": kanca var, **yük yok** (entegrasyonlu
  bir ölçüm yükü tanımlı değil)
- 022 B.1 — sekme başına bellek: `## Bellek` boş, araç dışarıdan
  (`footprint`, `vmmap`). Doku en kötü köşede ~16 MB'a çıkıyor.
- 011 — kaymanın yerleşme süresi (kanca yok; setin kendi `[~]` kaydı)
- 033 — "sayım dizininin parça boyu (`search::CHUNK_LINES`, 500 satır)
  `Term` kilidini tuş gecikmesi hissettirmeyecek kadar kısa tutuyor": kilit
  altında tarama süresini ölçen kanca yok; güvenliği sayıdan değil parçanın
  sınırlı ve iptal edilebilir olmasından

## Bellek

Ölçülmedi. Araç dışarıdan (`footprint`, `vmmap`); sekme yok.

## Giriş gecikmesi

Ölçülmedi. Kanca **yok**.

## Bench

Ölçülmedi. Kanca **yok** (`criterion` ayrı bir bağımlılık kararı).
