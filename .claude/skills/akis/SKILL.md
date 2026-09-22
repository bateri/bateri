---
name: akis
description: Bir konuyu baştan sona yürütür — planlama setini ürettirir, gerekirse paneliyle vetletir, kararı kaydettirir, seti commit'ler ve phase'leri --auto ile kodlatır. Kullanıcı bir işi komple devrettiğinde ("şunu yap", "baştan sona hallet", "sen kur ve uygula") kullanılır. Zincirin sürücüsüdür, aşamaların işini kendi yapmaz; teslim/push etmez.
allowed-tools: Read, Write, Edit, Glob, Grep, Bash(git:*), Bash(ls:*), TodoWrite, Agent, Skill
---

Kullanıcı bir işi **baştan sona** devretti: konu verildi; set üretilecek,
vetlenecek, kararı kaydedilecek ve kodlanacak.

Bu skill **sürücüdür**: sırayı, ön koşulları ve eskalasyonu yönetir. Aşamaların
kuralları kendi skill'lerindedir ve burada tekrarlanmaz — `/rfc` seti üretir,
`/plan-review` sınar, `/implement --auto` kodlar, `/ship` kullanıcıda kalır.

Sürücü de hafızasızdır; kuralın kendisi ve gerekçesi
[implement/references/otonom-serit.md](../implement/references/otonom-serit.md)
§1'dedir. Okudukları: aşama özetleri, `.tasks/README.md` indeksi ve
`plan.md → ## Durum`. Aşama dosyalarının gövdesini okumaz.

## Girdi

`$ARGUMENTS` iki şeyden biridir:

- **konu** (serbest metin) — yeni akış. `.tasks/README.md`'yi tara: aynı konuda
  📐 ya da 🔨 bir set varsa **dur ve söyle**; iki set aynı işi böler ve numara
  geri alınamaz.
- **`{NNN-slug}`** — yarıda kalan akışı sürdür. Nerede kalındığı diskten
  okunur, sinyaller bu sırayla: `discussion.md` → `## Muhakeme` → `## Karar` →
  `plan.md` → `phase-*.md` → setin commit'i → `## Durum`. İlk eksik olandan
  devam et. (Konu biçimindeki mükerrer koruması burada geçerli değildir;
  ikisini karıştırmak yarıda kalan akışı kendi kapısında durdurur.)

## 1. Set

`/rfc`'yi **bütün olarak** bir subagent'a koştur (adım 1–9). Dilimleme yok:
panel turlarının yönetimi adım 6'nın işidir ve orada kalmalıdır — sürücü
`discussion.md` gövdesini okumadığı için "seçenekleri yeniden kur" işini
yapamaz, yani turu ondan koparmak döngüyü sahipsiz bırakır.

Otonom modun tek farkı adım 7'dedir ve `/rfc` bunu kendi gövdesinde yazar:
onay alınmaz, karar panelden geçmiş öneridir. Kaydın kendisi atlanmaz, damga
biçimi şablondadır.

Dönen özet sabit alanlıdır:

```
SET:      .tasks/{NNN-slug}/
ÖN KOŞUL: {set} teslim edilmeden başlamaz | yok
PANEL:    {mercek}: {verdict} · {n}. tur | koşmadı ({neden})
KARAR:    yazıldı — {seçilen}
PHASE:    {n} dosya
```

`ÖN KOŞUL` boş bırakılamaz, çünkü keşif onu `context.md` gövdesine yazar ve
sürücü o gövdeyi okumaz: 014 ile 015 "bu set 013 teslim edilmeden başlamaz"
diyor, alan olmasaydı akış henüz yazılmamış bir yüzeye kod yazardı.

`PANEL` **hücre içindeki verdict'i de** taşır: 016'da merceğin kendisi SORUNLU
ama içindeki bir kararın seçeneği KIRMIZI. Yalnız mercek satırına bakan sürücü
onu göremez.

**Dur:** `ÖN KOŞUL` dolu ve beklenen set 🟢 değilse; `PANEL`'de herhangi bir
yerde KIRMIZI geçiyorsa; `KARAR` yazılmamışsa.

## 2. Seti commit'le

Kod yazılmadan önce: `.tasks/{NNN-slug}/` **ve** indeksin bu sete ait satırı
birlikte. İleti kuralı `proje.md` → Teslim.

Gerekçesi burasıdır ve iki yönlüdür. `--auto`'ya kirli çalışma ağacıyla
girilmez — rapor sözleşmesinin `git status → temiz` satırı her phase'de ya
yalan söyler ya da implementer indeks satırını kod commit'ine süpürür. Ve
izlenmeyen dosya geri gelmez, oysa set kendisini yürütecek kılavuzdur.

`git diff --stat .tasks/README.md` bu setin satırından fazlasını gösteriyorsa
**dur**: indekse başka bir oturum yazıyor demektir ve o satır bu commit'e
girerse iki iş tek commit'e karışır.

## 3. Uygulama

`/implement {NNN-slug} --auto`. O noktadan sonra döngünün sahibi otonom
şerittir; sürücü yalnız eskalasyonları karşılar ve kapanış özetini alır.

Başlatılan her subagent'ın (`/rfc` ajanı dahil) istemine otonom şeridin
**Ajan kuralları** yazılır ve önbellek ön koşulu orada okunur.

**Bir oturum, bir akış.** Aynı oturumda ikinci bir `/akis` başlatma; set
bitince devir `/clear` önerir (gerekçe otonom şerit §1).

## Eskalasyon — şunlarda DUR ve sor

Uygulama tarafının listesi otonom şerittedir. Sürücünün kendi listesi:

- `ÖN KOŞUL` karşılanmamış: set başka bir setin teslimini bekliyor.
- Panelde KIRMIZI — mercek verdict'i ya da hücre içi.
- Panel ikinci turdan sonra da yaklaşımı ayakta bırakmıyor.
- **Yeni bağımlılık ya da mimari karar** — `CLAUDE.md` gereği kendiliğinden
  yapılmaz.
- Konu birden çok okumaya açık ve ölçüm hangisi olduğunu ayırmıyor; ya da iki
  probe aynı soruya farklı cevap veriyor. (Ölçüm ayırıyorsa sorma: kanıtı sete
  yaz ve devam et.)
- İndeks satırı beklenenden fazla değişmiş.
- Kapsam verilen konudan taşıyor. (Konunun içinde kalan büyüme sapma değildir,
  phase'e yazılır.)

Eskalasyonda durumu özetle (hangi aşama, ne bulundu, seçenekler + önerin) ve
bekle.

## Devir

Kapanışta: set yolu, phase commit'leri (sonuncusu kapıyı ve 🟢'yi taşır),
kayda değer sapmalar, waive'ler ve gözle kontrol satırı. Net yönlendirme: **"Teslim için `/ship {NNN-slug}`
çalıştır; sonraki iş için `/clear`"** — akış push etmez.
