# İş seti düzeni

Bu dosya `/rfc` · `/plan-review` · `/implement` · `/ship` zincirinin ortak
sözleşmesidir: bir işin nerede durduğunu, nasıl adlandığını, hangi dosyalardan
oluştuğunu, durumunun nereden okunduğunu, kapıların ne zaman koştuğunu ve
teslimin kurallarını tanımlar. **Tek sahip burasıdır** — skill'ler bu
kuralları tekrar etmez, buraya bağlanır. Aynı kural iki yerde dursaydı biri
düzeltildiğinde öteki sessizce eskirdi.

**Projeden bağımsızdır**, skill'ler gibi. Komut, yol, dosya sınıfı ve proje
belgesi `proje.md`'dedir; burada "kapı komutu", "kilit dosyası", "sıra
belgesi" diye adlarıyla geçer.

## İçindekiler

- Konum ve ad
- Dosyalar
- Durum
- İndeks
- Kalite kapısı
- Teslim
- Arşiv

## Konum ve ad

Her iş `.tasks/{NNN}-{slug}/` altında yaşar.

- `NNN` — üç haneli artan sıra numarası. İlk iş `001`.
- `slug` — kebab-case, konuyu söyleyen kısa ad (`ornek-is`, `giris-ekrani`).
- Numara ile slug arasındaki tek tire zorunludur; slug'ın içindeki tireler serbesttir.
- **Numara asla yeniden kullanılmaz.** İş iptal edilse bile numarası ölür:
  sıra işlerin kronolojisidir ve boşluk da bilgidir.

Sıradaki numarayı **tahmin etme**, listeden oku (arşiv dahil, çünkü arşivlenmiş
bir numaranın tekrar dağıtılması iki işi aynı ada bağlar):

```sh
find .tasks .tasks/arsiv -maxdepth 1 -type d -name '[0-9][0-9][0-9]-*' 2>/dev/null \
  | sed 's|.*/||' | cut -d- -f1 | sort -n | tail -1
```

(`ls` ile glob, bazı kabuklarda hiç eşleşme yokken komutu hiç koşturmaz ve "no
matches found" der; `find` boş dizinde sessizce boş döner.)

Çıkan sayının bir fazlası, üç haneye tamamlanır (`printf '%03d'`). Hiç iş yoksa
`001`. O numarada klasör zaten varsa (paralel açılmış iş) bir sonrakine geç —
numara çakışması iki işi aynı ada bağlar.

## Dosyalar

| dosya | ne taşır | ne zaman yazılır | kim yazar |
|---|---|---|---|
| `context.md` | mevcut durum, motivasyon, kanıt | ilk | `/rfc` |
| `discussion.md` | seçenekler/karar noktaları + **karar kaydı** + muhakeme | seçenek varsa | `/rfc`, `/plan-review` |
| `plan.md` | onaylanan yaklaşım + gereksinimler + **durum tablosu** | onaydan sonra | `/rfc`, `/implement` |
| `phase-{N}.md` | kodun kılavuzu, checklist | plan onaylanınca | `/rfc`, `/implement` |

`discussion.md` opsiyoneldir; tek bariz yaklaşım varsa atlanır. Set defteri
(`teslim.md`) **yoktur** — gerekçesi Teslim bölümünde. Eski setlerin dosyaları tarihçe olarak duruyor, yenisi yazılmaz.

Şablonlar `.claude/is-akisi/sablonlar/` altındadır ve dosya biçiminin tek
sahibi orasıdır; skill'ler şablonu gövdelerine kopyalamaz, oradan okur.

**Yer tutucu sözleşmesi:** şablondaki `{...}` doldurulacak alandır;
`<!-- ... -->` yorumları ise yazana talimattır ve **üretilen dosyada
bırakılmaz**. Boş bırakılan bölüm de silinir — doldurulmamış başlık, okuyucuya
"burada bir şey yok" değil "burası unutulmuş" der.

### Tek phase'li set

Set tek phase'se `plan.md` yalnız **Hedef**, **Gereksinimler** (numarasız
madde listesi), **Kapsam Dışı** ve **Durum** taşır; **Yaklaşım** ve **Akış**
yazılmaz, çünkü `phase-1.md`'nin "Değişiklikler"i onların ta kendisi ve iki
yerde durunca biri eskir. `R1.1` numaralandırması ve phase'in
`_Requirements:_` satırı da yazılmaz: tek phase her gereksinimi kapsar ve
`/implement`'in kapsama kontrolü orada hiçbir şey bulamaz.

### Phase numaraları

Phase'ler tamsayı olmak zorunda değil: `phase-0`, `phase-1`, `phase-1b`,
`phase-2a` geçerlidir (araya iş sıkıştırmak, sırayı bozmadan mümkün olmalı).
Bu yüzden sıralama **doğaldır, leksik değil**: `phase-2` < `phase-10` ve
`phase-2` < `phase-2a` < `phase-2b`. "phase-1'den N'e say" varsayma, glob'la.

**Ek phase eşiği.** Set yürürken çıkan iş, ancak kendi kılavuzunu hak
ediyorsa phase olur: birden çok dosyaya yayılır, kendi doğrulaması vardır.
Tek commit'lik düzeltme (tek dosyada otuz satır)
phase açmaz — doğrudan commit'lenir, `plan.md → ## Durum`'un altına tek satır
not düşer.

## Durum

Bir işin nerede kaldığının **birincil sinyali** `plan.md` içindeki `## Durum`
tablosudur; `/implement` resume noktasını oradan okur.

| işaret | anlamı |
|---|---|
| `✅` | phase bitti, commit atıldı |
| `⏳` | phase devam ediyor |
| `[~] {gerekçe}` | yalnız `kapı` satırında: set kapısı bilerek atlandı |
| (boş) | başlanmadı |

Phase'in ✅'ü **kendi kod commit'inin içinde** girer; hash tabloya yazılmaz.
Hash commit'ten önce bilinmediği için onu dosyaya yazmak her phase'e ikinci bir
"defter" commit'i doğuruyordu (ölçülen bir dönemde commit'lerin yarısı). Phase'in
commit'i gövdedeki `{NNN-slug} phase-{N}` satırından bulunur (Teslim). Tablonun son satırı `kapı`dır: set sonundaki kalite kapısının izi.

Tablo diskte yaşadığı için oturum geçmişine ihtiyaç yoktur: `/clear` sonrası
`/implement {iş}` kaldığı yerden devam eder.

### Set aralığı

Set sonundaki `/code-review` setin bütün commit'lerine **ve** henüz
commit'lenmemiş son phase'e bakar (kapı o commit'ten önce koşar, Kalite
kapısı). Aralığın başı `.tasks/{NNN-slug}/`'a dokunan **ilk**
commit'tir (set koddan önce commit'lendiyse o, değilse ilk phase'in
commit'i); sonu çalışma ağacı:

```sh
first=$(git log --reverse --format=%h -- .tasks/{NNN-slug} | head -1)
git diff --stat "$first^"
```

Set henüz hiç commit'lenmediyse (tek phase, set dosyaları da çalışma
ağacında) aralık `git diff HEAD` + izlenmeyen dosyalardır.

## İndeks

`.tasks/README.md` bütün işleri tek tabloda tutar. Klasör adı zaten sırayı
verdiği için indeks yalnız **durum** ve **tek cümlelik not** taşır — commit
listesi `git log --grep`'tedir, tarihçe phase'lerin Uygulama Notları'nda.
İndeksi her akış okur; paragraf büyüyen not her okumada bağlama biner:

```markdown
# İşler

| # | İş | Durum | Not |
|---|---|---|---|
| 001 | [ilk-is](001-ilk-is/) | 🟢 | tek cümlelik not |
```

İlk iş eklenirken tablodaki `henüz iş açılmadı` yer tutucu satırı silinir.

Durum işaretleri: **📐 planlama** (plan var, kod yok) · **🔨 devam** (phase'ler
işleniyor) · **🟢 bitti** (kod ana dalda commit'li, kapı komutu yeşil, `## Durum`'un
`kapı` satırı ✅ ya da gerekçeli `[~]`) · **🗄️ arşiv** (yerini başka iş aldı
ya da iptal edildi). 🟢 **bekleyen ölçüm ya da manuel adımla ertelenmez**:
ölçüm iddiası ölçüm defterinin, ürün kararı sıra belgesinin konusudur
(`proje.md` → Belgeler), setin durumu değil.

İndeks bakımı komutlara gömülüdür: `/rfc` işi 📐 olarak ekler, `/implement`
ilk phase'in commit'inde 🔨, son phase'in commit'inde (set kapısıyla birlikte)
🟢 yapar. 🟢'nin anlamındaki "ana dalda" **yerel** daldır; push `/ship`'in
kararı ve durum değil. `/ship` 🟢'yi yalnız eksikse tamamlar.
Atlanırsa indeks drift'e düşer.

## Kalite kapısı

İki katman: ucuz olanı her phase'de, pahalı olanı sette bir kez. Komutlar ve
tetikleyiciler `proje.md`'de.

**Her phase — doğrulama.** `proje.md` → Doğrulama: kapı komutu ve dosyası
değiştiyse koşullu komutlar. Yeşil olmadan phase bitmiş sayılmaz. Projenin
kurallarının mekanik yarısı kapı komutunun içinde ajansız koşar.

**Riskli phase — ayrıca `/code-review`.** Phase `proje.md` → Riskli phase
tetikleyicilerinden birini tetiklediyse kendi diff'i phase sonunda incelenir:
o sınıflarda hata sessizdir ve sonraki phase'ler onun üstüne kurulur. Geri
kalan her şey set sonunu bekler. **Son phase hariç** — orada set kapısı onu
kapsıyor, aynı diff iki kez incelenmez.

**Set sonunda — bir kez**, son phase'in kodu doğrulandıktan sonra ve **o
phase'in commit'inden önce**; düzeltmeler, `kapı` ✅ ve indeksin 🟢'si son
phase'in commit'ine girer. Ayrı kapı ya da damga commit'i yok: öyleyken tek
bir kod commit'inin etrafında altı defter commit'i birikti.

1. `/code-review` — setin commit aralığı + çalışma ağacı (Set aralığı).
2. `/audit` — projede varsa (`proje.md` → Set kapısı ekleri), yalnız ilgili
   dosya değiştiyse.
3. Bulgu düzeltildiyse doğrulama yeniden.
4. **Gözle kontrol** — kullanıcının gördüğü davranış değiştiyse kapanış
   mesajı kullanıcıya **neye bakacağını** tek satırla söyler (sahne +
   beklenen görüntü). Kapı koda bakar, kullanıcı ekrana: kapıların hepsi
   koşup teslimden sonra kusurları kullanıcının gözünün bulduğu ölçüldü. Bu
   bir defter satırı değil, devir mesajının bir cümlesidir; 🟢'yi bekletmez.
   Projenin yüzeyleri varsa sahne **hepsini** sayar (`proje.md` → Set kapısı
   ekleri).

`/simplify` kapının parçası değildir; kullanıcı isterse koşar.

Tek istisna kesilmiş akıştır: bütün phase'ler ✅ ama `kapı` satırı boşsa kapı
koşar ve sonucu tek `{NNN-slug} kapı` commit'iyle kapanır. Reddedilen bir
waive'in sonradan düzeltmesi de aynı biçimi alır.

### Kapıyı kim koşturur

**Ajan koşturur, `Skill` aracıyla ve ön planda.** Kapıyı arka planda başlatıp
yoklamak yasaktır (`implement/references/otonom-serit.md` → Ajan kuralları).
Skill çağrısı gerçekten hata verirse bir inceleme subagent'ı; o da olmazsa
**dur ve kullanıcıdan iste**. Waive *bulgu* içindir: kapı hiç koşmadıysa bu
waive değil, atlanmış kapıdır.

"Yok" demeden önce aramanın o şeyi bulabilecek türden olduğunu göster:
yerleşik skill'ler `.claude/skills/` altında durmaz.

### İz

| işaret | anlamı |
|---|---|
| `- [x]` | yapıldı |
| `- [~]` | **waive / atlandı** — yanına gerekçe |
| `- [ ]` | yapılmadı |

Phase'in izi checklist'indedir (doğrulama, riskli ise `/code-review`). Set
kapısının izi `plan.md → ## Durum` tablosunun `kapı` satırıdır. Kutu
silinmez: koşmayan kapının kutusu `[~]` ve gerekçesiyle durur, silinen kutu
atlandığını hiçbir yerde göstermez.

### Ölçüm

**Ölçüm bir kapı değildir** ve ölçülmemiş sayı yazılmaz. Performans iddiası
taşıyan phase o iddiayı **hiç yazmaz**; ölçmek isteyen kullanıcı `/measure`
çağırır ve sonuç ölçüm defterine girer (`proje.md` → Belgeler). "Ölçüm
bekliyor" diye bir kalem yoktur: vardı ve bir phase'in iddiasını setin
durumuna çevirip setleri süresiz 🔨'da tuttu.

## Teslim

Projenin dalı, push komutu ve commit dili `proje.md` → Teslim'de.

- **Commit iletisi tek satırlık özet**; set commit'inde gövdenin ilk satırı
  `{NNN-slug} phase-{N}`. Hash dosyaya yazılmaz, phase'in commit'i `git log
  --grep` ile bu satırdan bulunur.
- **Phase = tek commit:** kod, phase checklist'i, `plan.md ## Durum` ✅ ve
  (ilk phase'de) indeksin 🔨'ü birlikte girer; **son phase'de** set kapısının
  düzeltmeleri, `kapı` ✅ ve indeksin 🟢'si de. Defter için ayrı commit
  atılmaz.
- **Her bilgi tek yerde.** Set dosyalarının rolleri ayrık: `discussion.md`
  kararı ve gerekçesini, `plan.md` hedefi ve kapsamı, `phase-{N}.md` hangi
  dosyada ne değişeceğini, kod yorumu yerel "neden"i taşır. Aynı paragraf
  ikinci bir dosyaya kopyalanmaz, işaretçiyle bağlanır. Proje sözleşmesi
  (`proje.md` → Belgeler) **bugünkü sözleşmedir** ve her oturumun başında
  okunur: yeni bir kural oraya kural + tek cümle gerekçe + işaretçi olarak
  girer; tarihçe, ölçüm anlatısı, reddedilen seçenekler ve bilinen sınır
  listeleri `.tasks/`'ta kalır. Aynı sınır listesi beş dosyaya yazıldığında
  setin belgesi kodundan büyük çıktı.
- **Set defteri yok.** Bir dönem `teslim.md` vardı (doğrulama + yayın
  checklist'i + geri alma); her sette aynı "revert et"i söyledi ve
  commit'lerin üçte birini defter yaptı. Kapanışın izi `plan.md → ## Durum`
  ve indeks satırıdır.
- **Push `/ship`'in kararıdır**, `/implement` push etmez. Kapı yeşil olmadan
  push yok.

## Arşiv

Bir iş iptal edilir ya da yerine yenisi geçerse klasörü `.tasks/arsiv/` altına
**numarasıyla birlikte** taşınır, indeks satırı 🗄️ olur ve nota işaretçi
yazılır (`→ [012-yeni-is](../012-yeni-is/)`). Silme yok: iptal edilmiş bir
planın gerekçesi, sonradan aynı fikre dönüldüğünde en değerli kayıttır.
