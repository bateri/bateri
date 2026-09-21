# Ölçümler

Depodaki ölçülmüş sayıların **tek sahibi** bu dosyadır. Başka belge sayı
kopyalamaz; niteliksel anlatır ve buraya bağlanır. Kuralın istisnaları kuralın
sahibinde yazılı (`/audit` → Ölçüm sahipliği).

Ölçüm bir **kapı değildir** (`.claude/is-akisi/proje.md` → Doğrulama): gerçek
pencere, sessiz makine ve dakikalar ister. Kullanıcı ister, `/measure` koşturur.

Dosya 006 phase-5'te kuruldu ve bugün **dört** ölçüm taşıyor: boşta kare,
atlas yuva ayak izi, kare süresi ve açılış. Sondaki ikisi 2026-09-21'de
girdi. Kare süresinin **CPU sütunları** ile açılış taban; **GPU sütunu
değil** — aynı kaynak ve bayt bayt aynı shader ikilisiyle 2,7 kat dolaştı ve
sınanan dört hipotezin hiçbiri onu ayıramadı (`## Kare süresi` → GPU
sütununun gezintisi). Bellek, giriş gecikmesi ve bench bölümlerinde sayı
yok; hangisinin kancası olduğu `/measure` skill'inin tablosunda.

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

`make duman` yükünde (`Workload::Smoke`) pencere ilk çizimden sonra boşta
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
gün `crates/bt-shell/src/app.rs`'teki `Measured`'ın doc'undan **buraya
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

### Atlas yuva ayak izi

Prob **depoda durmuyor**: ölçüm bir kapı değil ve `tests/` altında kalan bir
dosya `make hepsi`'nin her koşusunda derlenirdi. İki çalışma ağacı açılır
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
  make duman 2>&1 | tee "target/duman-$ARM-debug-$i.out" | grep -E 'kare=|bozuldu'
done

make kur
for i in $(seq 1 $N); do  # release paket — LaunchServices yolu
  env -u BT_SCROLL_TEST -u BT_FRAME_STATS open -W -n --env BT_RUN_SECONDS=3 \
    --stdout "$PWD/target/duman-$ARM-release-$i.out" --stderr "$PWD/target/duman-$ARM-release-$i.err" \
    "$PWD/target/release/bateri.app"
done
```

Bozuk kolun **üç** mutasyonu var; üçü de `git checkout` ile geri alınır ve
`git diff` boş kalır. Hangisinin hangi kapıyı ateşlediği ölçümün kendi
kaydında, gövdeleri burada:

1. **Hızlı sızıntı** — `crates/bt-gpu/src/link.rs`'te `needs_update`'in
   sonuna, `match drawn { … }`'den sonra koşulsuz `iv.waker.wake();`. Her
   çizilen kare hasar diker, yani sıradaki callback'te de hasar bulunur:
   tazeleme hızında **içerik** karesi. **Uyarı — 008'de düşürdüğü kol
   değişti:** kalıcı hasar "hasar yok" dalını hiç çalıştırmıyor, yani
   `hareket=0` ve kapı `ExcessFrames`'ten **önce** `MissingCounter` diyor.
   Satırdaki `içerik karesi` sayısı yine de okunuyor (tanı onu basıyor).
2. **Yavaş sızıntı** — aynı dosyada, "hasar yok + yerleşti" dalında
   `link.setPaused(true)`'dan **önce**:

   ```rust
   let w = iv.waker.clone();
   let _ = DispatchQueue::main().after(
       DispatchTime::try_from(Duration::from_millis(500)).unwrap(),
       move || w.wake(),
   );
   ```

   (`use dispatch2::DispatchTime` gerekir.) Yarım saniyede bir içerik karesi:
   üç saniyede `icerik=8`, yani **üst sınırı aşmıyor** — bu kolu yalnız
   `sessiz` görüyor ve `QUIET_FLOOR` inmeden önce yeşil geçiyordu.
3. **Durma koşulu** — `crates/bt-gpu/src/motion.rs`'te `Motion::settled`'ın
   gövdesine `&& false`. Animasyon hiç yerleşmez: `Verdict::MotionUnsettled`.

Her mutasyondan sonra `git checkout -- {dosya}` ve `git diff` boş. Sağlıklı kolun
5 saniyelik kıyası aynı döngülerde `BT_RUN_SECONDS=5` ile koşar (debug'da
`make duman` yerine Makefile tarifinin aynısı:
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
`(make duman … & ./probe; wait)`, pakette `(env … open -W … & ./probe; wait)`.
Sahibi `bateri` olan, ekran genişliğinde ve 33 pt yüksekliğinde ekran dışı
pencereler de listede çıkıyor; ana pencere onlar değil (ne oldukları
doğrulanmadı), boyutundan tanınır.

## Boşta kare

**Üst sınır: `icerik ≤ 8`** (`crates/bt-shell/src/app.rs` → `IDLE_FRAME_LIMIT`).
**Alt sınır: `sessiz ≥ 868 ms`** (aynı dosya → `QUIET_FLOOR`; 2026-09-17'de
870'ten indirildi, gerekçe aşağıda).

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
| debug · `make duman` · 3 sn | 10 | `2` ×10 | `29` ×10 | 1737,12 – 1751,58 (ort. 1745,02) |
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
| debug · `make duman` · 3 sn | 10 | `3` ×9, `2` ×1 | `30` ×9, `29` ×1 | `27` ×10 | 1745,95 – 1755,25 |
| release · paket (`open`) · 3 sn | 10 | `2` ×7, `3` ×3 | `29` ×7, `30` ×2, `27` ×1 | `27` ×8, `26` ×1, `25` ×1 | 1746,88 – 1757,29 |
| debug · `cargo run` · 5 sn | 5 | `3` ×5 | `30` ×5 | `27` ×5 | 3742,95 – 3755,83 |
| release · paket (`open`) · 5 sn | 5 | `3` ×5 | `30` ×3, `29` ×2 | `27` ×3, `26` ×2 | 3746,16 – 3754,35 |
| **doğrulama koşuları** · 3 sn (kapı indikten sonra; `bt-gpu` değişmemiş) | 7 | `3` ×6, `2` ×1 | `30` ×6, `29` ×1 | `27` ×7 | 1742,29 – 1754,42 |

Son satır dağılımın **parçasıdır**, ayrı bir kol değil: gürültü kuralı hiçbir
sağlıklı koşuyu düşürmüyor ve bu yedi koşu türetmenin bağlayıcı ucunu
1745,95'ten **1742,29**'a indiriyor (dördü debug `make duman`, üçü release
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

**Sağlıklı koşular** (`kare/istek`, koşu sırasıyla; `make duman` ve paket
koşularının hepsi yeşil):

| profil · yol · süre | n | `kare` dağılımı | `istek` dağılımı | koşular |
|---|---|---|---|---|
| debug · `make duman` · 3 sn | 21 | `1` ×20, `2` ×1 | `2` ×9, `3` ×12 | 1/2 1/2 1/3 1/2 1/3 1/3 2/3 1/3 1/3 1/2 · 1/3 (yoklamalı) · 1/3 1/2 1/3 1/2 1/3 1/2 1/3 1/2 1/3 1/2 |
| release · paket (`open`) · 3 sn | 20 | `2` ×18, `1` ×2 | `3` ×20 | 2/3 (yoklamalı) 2/3 2/3 2/3 2/3 1/3 2/3 2/3 2/3 2/3 · 1/3 2/3 2/3 2/3 2/3 2/3 2/3 2/3 2/3 2/3 |
| debug · `cargo run` · 5 sn | 5 | `1` ×5 | `3` ×5 | 1/3 1/3 1/3 1/3 1/3 |
| release · paket (`open`) · 5 sn | 5 | `2` ×3, `1` ×2 | `3` ×5 | 1/3 2/3 2/3 1/3 2/3 |

**Bozuk koşular** (3 sn; kapı düştü, jeton satırı yok, stderr satırından):

| profil · yol | n | `kare` | kare talebi |
|---|---|---|---|
| debug · `make duman` (çıkış 2) | 3 | 353, 354, 353 | 356, 356, 356 |
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

## Bellek

Ölçülmedi. Araç dışarıdan (`footprint`, `vmmap`); sekme yok.

## Giriş gecikmesi

Ölçülmedi. Kanca **yok**.

## Bench

Ölçülmedi. Kanca **yok** (`criterion` ayrı bir bağımlılık kararı).
