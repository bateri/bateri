# Ölçümler

Depodaki ölçülmüş sayıların **tek sahibi** bu dosyadır. Başka belge sayı
kopyalamaz; niteliksel anlatır ve buraya bağlanır. Kuralın istisnaları kuralın
sahibinde yazılı (`/audit` → Ölçüm sahipliği).

Ölçüm bir **kapı değildir** (`.claude/is-akisi/proje.md` → Doğrulama): gerçek
pencere, sessiz makine ve dakikalar ister. Kullanıcı ister, `/measure` koşturur.

Dosya 006 phase-5'te kuruldu ve bugün **yalnız boşta kare** ölçümünü taşıyor.
Kare süresi, açılış, bellek, giriş gecikmesi ve bench bölümlerinde sayı yok;
hangisinin kancası olduğu `/measure` skill'inin tablosunda.

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

Sayı yok. Kancanın (`BT_FRAME_STATS`) dürüst sınırları — her biri **kapsam**
ya da **açık kalem** diye etiketli — bugün hâlâ `crates/bt-shell/src/app.rs`'te
`Measured`'ın doc'unda emaneten duruyor; bu türün ilk ölçümü onları buraya
taşır. Eksik bir kopya sessizce ayrışacağı için şimdiden kopyalanmadı.

Listenin **dışından** bir kapsam kalemi 008'de doğdu ve o türü ölçmeye
gerek olmadan biliniyor, o yüzden burada: **`ornek=` ile `gpu_ornek=` aynı
kare popülasyonunu saymıyor.** Hareket karesi CPU örneği yazmıyor (o karede
`session.frame` hiç koşmuyor, sahte örnek p95'i aşağı çekerdi) ama bir komut
tamponu commit ediyor, yani GPU'nun tamamlanma bloğu onu **görüyor**. Ayrılık
yapısal: aynı blok `FailureStreak`'i de besliyor ve hareket karesini ondan
muaf tutmak çizim hatasını görünmez kılardı. Sonucu, imleç kayan bir koşuda
iki sütunun p95'i **doğrudan karşılaştırılamaz**; gerekçesi
`bt-gpu/src/link.rs`'te hareket karesinin gövdesinde.

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

## Kare süresi

Ölçülmedi. Kanca var (`BT_FRAME_STATS=1 BT_SCROLL_TEST=1 BT_RUN_SECONDS=N`);
yöntemi için yukarıda "Kare süresi ve açılış".

## Açılış

Ölçülmedi. Kanca var (`acilis=` jetonu), tarifi dar; bkz. `Measured`'ın doc'u.

## Bellek

Ölçülmedi. Araç dışarıdan (`footprint`, `vmmap`); sekme yok.

## Giriş gecikmesi

Ölçülmedi. Kanca **yok**.

## Bench

Ölçülmedi. Kanca **yok** (`criterion` ayrı bir bağımlılık kararı).
