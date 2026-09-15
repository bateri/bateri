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

### Boşta kare (`IDLE_FRAME_LIMIT`)

`make duman` yükünde (`Workload::Smoke`) pencere ilk çizimden sonra boşta
durur ve kapı `kare` jetonunu bir üst sınırla karşılaştırır. Sınır iki
dağılımdan türer ve ikisi de ölçülür:

- **Sağlıklı:** değiştirilmemiş HEAD. Profil başına en az on koşu, `BT_RUN_SECONDS=3`
  (kapının koştuğu süre), kıyas için birkaç `5` saniyelik koşu.
- **Bozuk:** boşta sıfır kareyi kasıtlı bozan geçici bir mutasyon —
  `crates/bt-gpu/src/link.rs`'te `needs_update`'in sonuna koşulsuz
  `iv.waker.wake();`. Profil başına en az üç koşu. Mutasyon **commit'e girmez**;
  geri alındığı `git diff` boşluğuyla gösterilir ve iki derleme de geri
  alındıktan sonra yenilenir (yoksa `target/` altında bozuk bir paket kalır).

Sınırın kuralı: **en yüksek sağlıklı gözlemin en az iki katı ve en düşük bozuk
gözlemin altında.** İki koşul çelişirse sınır oynatılmaz, iş durur: sağlıklı
dağılım bozuk dağılıma yaklaştıysa kusur sayıda değil koddadır. Sınır bu iki
koşulun **zorladığı** kadar oynar, fazlası değil: bozuk dağılımın altında
kalan her büyütme kuralı sağlar ama kapının algılama tabanını yükseltir
(yavaş bir sızıntı yeşil geçer; bkz. `IDLE_FRAME_LIMIT`'in doc'u).

Kapı debug'a bağlıdır (gerekçesi `IDLE_FRAME_LIMIT`'in doc'unda), ama release
paketi de aynı sınıra tabi olduğu için onun dağılımı da ölçülür: sınır iki
profili birden taşımalıdır.

### Kare süresi ve açılış

Sayı yok. Kancanın (`BT_FRAME_STATS`) dürüst sınırları — her biri **kapsam**
ya da **açık kalem** diye etiketli — bugün hâlâ `crates/bt-shell/src/app.rs`'te
`Measured`'ın doc'unda emaneten duruyor; bu türün ilk ölçümü onları buraya
taşır. Eksik bir kopya sessizce ayrışacağı için şimdiden kopyalanmadı.

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

Bozuk kol için önce yukarıdaki mutasyon uygulanır; ardından
`git checkout -- crates/bt-gpu/src/link.rs` ve `git diff` boş. Sağlıklı kolun
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

**Sınır: `8`** (`crates/bt-shell/src/app.rs` → `IDLE_FRAME_LIMIT`).

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
