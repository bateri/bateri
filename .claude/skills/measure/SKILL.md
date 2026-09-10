---
name: measure
description: Kare süresi (GPU/CPU), giriş gecikmesi, sekme başına bellek, açılış süresi ve ayrıştırıcı bench ölçümlerini koşturur, tabanla karşılaştırır ve sonucu docs/OLCUMLER.md'ye işler. Kullanıcı "ölç", "kaç fps", "gecikme ne oldu", "bu hızlandı mı", "bench çalıştır", "ölçümleri güncelle" dediğinde kullanılır. Ölçüm gerçek pencere ve sessiz makine ister, bir kapı değildir — yalnız kullanıcı istediğinde koşar.
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

## 1. Neyi, neye karşı

`$ARGUMENTS` ne ölçüleceğini söylüyorsa onu al; söylemiyorsa sor. Ölçüm
türleri ve `docs/OLCUMLER.md`'deki karşılıkları:

| tür | nasıl | bölüm |
|---|---|---|
| kare süresi | `BT_SCROLL_TEST=1 BT_RUN_SECONDS=30` ile dolu scrollback kaydırma; GPU/CPU ms ve düşen kare `BT_FRAME_LOG` çıktısından; çapraz kontrol `xcrun xctrace record --template 'Metal System Trace'` | `## Kare süresi` |
| giriş gecikmesi | `BT_INPUT_LATENCY_SAMPLES=200` — tuş → PTY → echo → parse → commit → presented zinciri, medyan ve p95 | `## Giriş gecikmesi` |
| bellek | `footprint -p {pid}` ya da `vmmap --summary`; 1 sekme boş, 1 sekme 10 000 satır dolu, 8 sekme | `## Bellek` |
| açılış | `BT_STARTUP_TRACE=1` — process başlangıcından ilk presented frame'e | `## Açılış` |
| ayrıştırıcı / atlas bench | `cargo bench -p bt-core --bench parse`, `cargo bench -p bt-atlas` | `## Bench` |

Ortam değişkenleri **sözleşmedir**: kancayı taşıyan kod henüz yoksa ölçüm
"yok" değil "ölçüm aracı yok"tur — bunu söyle, sayı uydurma ve kancayı ekleyen
bir iş seti öner. Metalterm'in aynı iş için kullandığı kancalar
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
kasıtlı bozma ile değişen sonuç).

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
