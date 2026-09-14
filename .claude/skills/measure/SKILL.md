---
name: measure
description: Kare süresi (GPU/CPU), giriş gecikmesi, sekme başına bellek, açılış süresi, boşta kare kapısının sınırı (IDLE_FRAME_LIMIT) ve ayrıştırıcı bench ölçümlerini koşturur, tabanla karşılaştırır ve sonucu docs/OLCUMLER.md'ye işler. Kullanıcı "ölç", "kaç fps", "gecikme ne oldu", "bu hızlandı mı", "bench çalıştır", "ölçümleri güncelle" dediğinde kullanılır. Ölçüm gerçek pencere ve sessiz makine ister, bir kapı değildir — yalnız kullanıcı istediğinde koşar.
allowed-tools: Read, Edit, Write, Glob, Grep, Bash(make:*), Bash(cargo:*), Bash(git:*), Bash(ls:*), Bash(diff:*), Bash(sort:*), Bash(xcrun:*), Bash(footprint:*), Bash(vmmap:*), Bash(wc:*)
---

Kullanıcı bir ölçüm istiyor. Ölçüm bu depoda **kapı değildir** (`proje.md` →
Doğrulama): gerçek pencere, sessiz makine ve dakikalar ister; phase'i bloke
etmemesi için akıştan çıkarılmıştır. Bu skill onu kullanıcı istediğinde koşar.

**Yöntem kuralları burada tekrarlanmaz.** Ölçmeye başlamadan önce oku:

- `docs/OLCUMLER.md` → **`## Yöntem`** — neyin nasıl ölçüleceği, gürültü
  eşiği, tuzaklar
- `docs/OLCUMLER.md` → **`## Nasıl yeniden ölçülür`** — komutlar ve ortam
  (makine, ekran Hz, güç kaynağı, pencere boyutu)

Bu iki bölüm tek sahiptir. Çelişki görürsen onlar kazanır; skill yalnız akışı
yürütür. Dosya **yoksa** ilk ölçüm onu kurar: önce `## Yöntem` ve `## Nasıl
yeniden ölçülür` yazılır, sonra sayı girer — yöntemsiz sayı sonraki ölçümle
karşılaştırılamaz.

Hangi türün sayısı olduğunu dosyanın başı söylüyor. Kare süresi ve açılışın
yöntemini sıfırdan yazmak gerekmiyor: o kancanın dürüst sınırları **kodda**
emaneten duruyor ve o türün ilk ölçümü onları `## Yöntem`'e taşır —

- `crates/bt-shell/src/app.rs` → `Measured`'ın doc'u, "**Ölçümün dürüst
  sınırları**" başlığı — listenin tamamı orada ve her kalem **kapsam** ya da
  **açık kalem** diye etiketli; buraya kopyalanmıyor, çünkü eksik kopyalanan
  bir liste sessizce ayrışır.
- Aynı dosyada `IDLE_FRAME_LIMIT` ve `Report::token_line`: kapının gerekçesi
  ve jeton sözleşmesinin dil kuralı.
- `crates/bt-gpu/src/stats.rs`: `MIN_SAMPLES`'ın türetimi (p95'in tabanı),
  halka kapasitesi, hangi karenin **elendiği**.

## 1. Neyi, neye karşı

`$ARGUMENTS` ne ölçüleceğini söylüyorsa onu al; söylemiyorsa sor. Ölçüm
türleri ve `docs/OLCUMLER.md`'deki karşılıkları:

| tür | nasıl | kanca | bölüm |
|---|---|---|---|
| kare süresi | `BT_FRAME_STATS=1 BT_SCROLL_TEST=1 BT_RUN_SECONDS=30` — jeton satırı üç sütun basar: `cpu_kare_*` (`session.frame`: kilit + ayrıştırma + grid + sink), `cpu_encode_*` (encode + commit), `gpu_*` (Metal'in kendi saatinden). Çapraz kontrol `xcrun xctrace record --template 'Metal System Trace'` | **var** | `## Kare süresi` |
| düşen kare | — | **yok** (005 R3, bilerek kapsam dışı). `dusen=` halkaya sığmayan **örnek**, atlanan kare **değil**; ikisini karıştırma | `## Kare süresi` |
| giriş gecikmesi | `BT_INPUT_LATENCY_SAMPLES=200` — tuş → PTY → echo → parse → commit → presented zinciri, medyan ve p95 | **yok** (005 kapsam dışı: zincirin orta halkaları `alacritty_terminal`'de) | `## Giriş gecikmesi` |
| bellek | `footprint -p {pid}` ya da `vmmap --summary`; 1 sekme boş, 1 sekme 10 000 satır dolu, 8 sekme | araç dışarıdan, sekme yok | `## Bellek` |
| açılış | aynı koşunun `acilis=` jetonu — `main()`'in **ilk satırından** ilk **tamamlanan** kareye | **var**, ama tarifi dar: süreç başlangıcı ve *presented* değil (bkz. yukarıdaki dürüst sınırlar) | `## Açılış` |
| boşta kare (`IDLE_FRAME_LIMIT`) | sağlıklı `make duman` (debug) + paketten `open` (release) koşuları, üstüne kasıtlı bozulmuş bir koşu; tarif ve paket yolunun tuzakları dosyada | **var** (`kare=`/`istek=` jetonları); kapının kendisi değil, sınırını doğuran ölçüm | `## Boşta kare` |
| ayrıştırıcı / atlas bench | `cargo bench -p bt-core --bench parse`, `cargo bench -p bt-atlas` | **yok** — `criterion` ayrı bir bağımlılık kararı; `cargo bench --workspace -- --list` → `0 benchmarks` | `## Bench` |

Ölçüm koşusunun iki şartı: `BT_FRAME_STATS` ile `BT_SCROLL_TEST` **sıfırdan
büyük** bir `BT_RUN_SECONDS` ister (yoksa süreç çıkış 1 verir — rapor yalnız
deadline yolunda basılıyor), ve ölçüm yükü olmadan kare akmaz. Satır profilini
kendi söylüyor (`profil=debug|release`), yani release şartını (§2) sonradan
da doğrulayabilirsin. `ornek=` ile `taban=` birlikte okunur: taban altında p95
**ve** en kötü değer basılmaz, ikisi de `insufficient` çıkar — o koşu bir sayı
değil, bir **arıza** raporudur (örnekleme durmuş).

Sütun sayaçları ayrı ve karıştırılmaz — `ornek=` CPU'nun, `gpu_ornek=`
GPU'nun, `gpu_elenen=` ölçülemeyip atılan kare, `dusen=` halkaya sığmayan
örnek. İkisinin neden eşit olmayabildiği ve `dusen=`'in nasıl türediği
alanların kendi doc'unda (`Measured`, `bt_gpu::Samples`); buraya
kopyalanmıyor.

Ortam değişkenleri **sözleşmedir**: kancayı taşıyan kod henüz yoksa ölçüm
"yok" değil "ölçüm aracı yok"tur — bunu söyle, sayı uydurma ve kancayı ekleyen
bir iş seti öner. Yukarıdaki tabloda **yok** yazan üç satır bugün tam olarak
bu durumda. Metalterm'in aynı iş için kullandığı kancalar
`docs/ARASTIRMA.md` → "Shell entegrasyonu" altındadır.

**Taban olmadan ölçüm yorumlanamaz.** Karşılaştırılacak değeri
`docs/OLCUMLER.md`'den oku; yoksa önce mevcut hâli ölç (`git stash` ya da
değişiklikten önceki commit) ve tabanı kaydet. "Ölçtüm, şu çıktı" tek başına
bir sonuç değildir.

## 2. Ölç

Ölçüm koşarken makinede başka ağır iş olmasın; tarayıcı ve IDE indeksleyici
kapalı, güç kaynağı **prizde**, ekran Hz'i ve pencere boyutu (hücre sayısı)
kayda yazılır — 120 Hz ile 60 Hz'in frame bütçesi farklıdır, `118×34` ile
`200×60` aynı sayı değildir. Her ölçümü **en az iki kez** koştur ve sayılar
oynuyorsa `docs/OLCUMLER.md#yöntem`'deki gürültü kuralını uygula.

Ölçtüğün yolun gerçekten koştuğunu **doğrula**: boşta duran pencerede kare
süresi ölçmek (renderer frame göndermez, sıfır çıkar), atlası ısıtmadan glyph
yükleme ölçmek, uygulanmamış bir değişikliği ölçmek — hepsi sessiz yanlış sayı
üretir. Sayıyı almadan önce yolun ateşlendiğini göster (frame sayacı, log,
kasıtlı bozma ile değişen sonuç). Kare süresinde bunun bir kısmı satırın
kendisinde: `kare=`, `ornek=`, `gpu_ornek=` ve `gpu_elenen=` beklediğinden
küçükse ölçüm durmuştur — `insufficient` gördüğün bir koşuyu **yorumlama**,
yeniden koş.

**Release derlemesi ölç.** Debug derlemesinin sayısı bir taban değildir;
`cargo build --release` ve `target/release` altındaki binary.

## 3. Gecikme ve karede: dağılım, ortalama değil

Ortalama tek başına yeterli değildir — bir takılma (hitch) ortalamayı
oynatmaz ama kullanıcı onu görür. Kare süresinde **p95 ve en kötü kare**,
gecikmede **medyan ve p95** yazılır; düşen kare sayısı ayrı sütundur.
Karşılaştırmada iki dağılımı yan yana koy: kazancın yanında kaybı da göster.

## 4. İşle

Sonuç `docs/OLCUMLER.md`'ye yazılır ve **tek sahibi orasıdır**. Başka belgeye
sayı kopyalama; o belgeler niteliksel anlatıp buraya bağlanır.

- İlgili bölümdeki eski değeri **güncelle**, yanına ikinci bir sayı ekleme.
- Ölçümün tarihini, commit'ini, makineyi ve neyin değiştiğini yaz.
- `docs/ARASTIRMA.md`'ye dokunma: o Metalterm'in kendi sayılarını aktaran
  tarihli bir kayıttır, bilerek eskir.

## 5. İş setini kapat

Ölçüm bir `.tasks/{set}/` işinden geldiyse: o phase'in `## Yayın Etkisi`
bloğundaki **"ölçüm bekliyor: {ne}"** maddesini kapat, checklist kutusunu
`[x]` yap ve yeni değeri `## Uygulama Notları`'na tek satır olarak düş
(sayının kendisi değil, nereye yazıldığı — sayı `docs/OLCUMLER.md`'de).

## 6. Rapor

Kullanıcıya: ne ölçüldü, taban neydi, şimdi ne, fark anlamlı mı (gürültü
eşiğinin üstünde mi), `docs/OLCUMLER.md`'de hangi bölüm güncellendi. Kare
ölçtüysen düşen kare sayısı ve en kötü kare; gecikme ölçtüysen zincirin hangi
halkasının büyüdüğü.
