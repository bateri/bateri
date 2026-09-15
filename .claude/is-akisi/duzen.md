# İş seti düzeni

Bu dosya `/rfc` · `/plan-review` · `/implement` · `/ship` zincirinin ortak
sözleşmesidir: bir işin nerede durduğunu, nasıl adlandığını, hangi dosyalardan
oluştuğunu ve durumunun nereden okunduğunu tanımlar. **Tek sahip burasıdır** —
skill'ler bu kuralları tekrar etmez, buraya bağlanır. Aynı kural iki yerde
dursaydı biri düzeltildiğinde öteki sessizce eskirdi.

## İçindekiler

- Konum ve ad
- Dosyalar
- Durum
- İndeks
- Arşiv

## Konum ve ad

Her iş `.tasks/{NNN}-{slug}/` altında yaşar.

- `NNN` — üç haneli artan sıra numarası. İlk iş `001`.
- `slug` — kebab-case, konuyu söyleyen kısa ad (`glyph-atlas`, `imlec-hareketi`).
- Numara ile slug arasındaki tek tire zorunludur; slug'ın içindeki tireler serbesttir.
- **Numara asla yeniden kullanılmaz.** İş iptal edilse bile numarası ölür:
  sıra işlerin kronolojisidir ve boşluk da bilgidir.

Sıradaki numarayı **tahmin etme**, listeden oku (arşiv dahil, çünkü arşivlenmiş
bir numaranın tekrar dağıtılması iki işi aynı ada bağlar):

```sh
find .tasks .tasks/arsiv -maxdepth 1 -type d -name '[0-9][0-9][0-9]-*' 2>/dev/null \
  | sed 's|.*/||' | cut -d- -f1 | sort -n | tail -1
```

(`ls` ile glob, zsh'de hiç eşleşme yokken komutu hiç koşturmaz ve "no
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
| `phase-{N}.md` | kodun birebir kılavuzu, checklist, yayın etkisi | plan onaylanınca | `/rfc`, `/implement` |
| `teslim.md` | doğrulama + yayın adımları, geri alma | son phase'den sonra | `/implement` |

`discussion.md` opsiyoneldir; tek bariz yaklaşım varsa atlanır. `teslim.md`
sıfırdan keşfedilerek değil, phase'lerin `## Yayın Etkisi` bloklarından derlenir.

Şablonlar `.claude/is-akisi/sablonlar/` altındadır ve dosya biçiminin tek
sahibi orasıdır; skill'ler şablonu gövdelerine kopyalamaz, oradan okur.

**Yer tutucu sözleşmesi:** şablondaki `{...}` doldurulacak alandır;
`<!-- ... -->` yorumları ise yazana talimattır ve **üretilen dosyada
bırakılmaz**. Boş bırakılan bölüm de silinir — doldurulmamış başlık, okuyucuya
"burada bir şey yok" değil "burası unutulmuş" der.

### Phase numaraları

Phase'ler tamsayı olmak zorunda değil: `phase-0`, `phase-1`, `phase-1b`,
`phase-2a` geçerlidir (araya iş sıkıştırmak, sırayı bozmadan mümkün olmalı).
Bu yüzden sıralama **doğaldır, leksik değil**: `phase-2` < `phase-10` ve
`phase-2` < `phase-2a` < `phase-2b`. "phase-1'den N'e say" varsayma, glob'la.

**Ek phase eşiği.** Set yürürken çıkan iş, ancak kendi kılavuzunu hak
ediyorsa phase olur: birden çok dosyaya yayılır, kendi doğrulaması ve yayın
etkisi vardır. Tek commit'lik düzeltme (006 phase-4c: tek dosyada otuz satır)
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
"defter" commit'i doğuruyordu (001–006'da 121 commit'in 57'si). Phase'in
commit'i gövdedeki `{NNN-slug} phase-{N}` satırından bulunur (`proje.md` →
Teslim). Tablonun son satırı `kapı`dır: set sonundaki kalite kapısının izi.

Tablo diskte yaşadığı için oturum geçmişine ihtiyaç yoktur: `/clear` sonrası
`/implement {iş}` kaldığı yerden devam eder.

### Set aralığı

Set sonundaki `/code-review` setin bütün commit'lerine bakar. Aralığın başı
`.tasks/{NNN-slug}/`'a dokunan **ilk** commit'tir (set koddan önce
commit'lendiyse o, değilse ilk phase'in commit'i):

```sh
first=$(git log --reverse --format=%h -- .tasks/{NNN-slug} | head -1)
git diff --stat "$first^" HEAD
```

## İndeks

`.tasks/README.md` bütün işleri tek tabloda tutar. Klasör adı zaten sırayı
verdiği için indeks yalnız **durum** ve **tek cümlelik not** taşır — commit
listesi, açık kalem dökümü ve tarihçe `teslim.md`'dedir. İndeksi her akış
okur; paragraf büyüyen not her okumada bağlama biner:

```markdown
# İşler

| # | İş | Durum | Not |
|---|---|---|---|
| 001 | [workspace-iskeleti](001-workspace-iskeleti/) | 🟢 | cargo workspace, Metal üstünde ilk pencere |
```

İlk iş eklenirken tablodaki `henüz iş açılmadı` yer tutucu satırı silinir.

Durum işaretleri: **📐 planlama** (plan var, kod yok) · **🔨 devam** (phase'ler
işleniyor) · **🟢 bitti** (kod `main`'de, teslim checklist'i kapalı) ·
**🗄️ arşiv** (yerini başka iş aldı ya da iptal edildi).

İndeks bakımı komutlara gömülüdür: `/rfc` işi 📐 olarak ekler, `/implement`
🔨 yapar, `/ship` 🟢 yapar. Atlanırsa indeks drift'e düşer.

## Arşiv

Bir iş iptal edilir ya da yerine yenisi geçerse klasörü `.tasks/arsiv/` altına
**numarasıyla birlikte** taşınır, indeks satırı 🗄️ olur ve nota işaretçi
yazılır (`→ [012-yeni-is](../012-yeni-is/)`). Silme yok: iptal edilmiş bir
planın gerekçesi, sonradan aynı fikre dönüldüğünde en değerli kayıttır.
