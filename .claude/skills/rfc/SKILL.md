---
name: rfc
description: Yeni bir özellik veya iyileştirme için planlama seti (.tasks/NNN-slug/) üretir — bağlam, tartışma, karar kaydı, plan ve phase dosyaları. Kullanıcı bir işi planlamak, RFC/tasarım dokümanı yazmak ya da "önce planlayalım" dediğinde kullanılır. Kod yazmaz.
allowed-tools: Read, Write, Edit, Glob, Grep, Bash(ls:*), Bash(mkdir:*), Bash(cp:*), Bash(mv:*), Bash(printf:*), Bash(sed:*), Bash(cut:*), Bash(sort:*), Bash(tail:*), Skill
---

Kullanıcı yeni bir iş için planlama seti üretmek istiyor. Bu skill **kodu
yazmaz**, kılavuzu üretir.

**Zincir:** `/rfc` kılavuzu üretir → `/plan-review` tasarımı sınar →
`/implement` kılavuzu yürütür → `/ship` teslim eder.

Başlamadan iki dosyayı oku — set düzeninin ve projeye özgü kuralların tek
sahibi onlardır, burada tekrarlanmaz:

- `.claude/is-akisi/duzen.md` — klasör adı, numaralandırma, dosya rolleri, indeks
- `.claude/is-akisi/proje.md` — doğrulama komutları, yayın etkisi başlıkları

Belge biçimleri `.claude/is-akisi/sablonlar/` altındadır: yeni dosyayı ilgili
şablondan **kopyalayarak** başlat, gövdeni oradan doldur.

## 1. Konu

`$ARGUMENTS` verilmişse konu odur. Verilmemişse sor: ne yapılacak (kısa
açıklama) ve slug önerisi (kebab-case).

## 2. Araştır

Konuyla ilgili mevcut kodu oku: hangi paketler, hangi katman, mevcut akış ne.
Sorunun kök nedenini tespit et. Bu adım atlanırsa plan varsayımlar üstüne
kurulur.

## 3. Seti kur

Sıradaki numarayı `duzen.md`'deki komutla **listeden oku** (tahmin etme),
`.tasks/{NNN}-{slug}/` klasörünü aç ve `.tasks/README.md` indeksine **📐
planlama** satırı ekle (indeks dosyası yoksa `duzen.md`'deki biçimle oluştur). Yeni set mevcut bir setin yerine geçiyorsa eskisini
arşivle (yine `duzen.md`).

## 4. context.md

Şablondan üret. Mevcut durum, motivasyon, varsa kanıt. Sorun çözümü için de
geliştirme için de aynı biçim kullanılır.

## 5. discussion.md (opsiyonel)

Birden fazla geçerli yaklaşım **veya** birden fazla açık karar noktası varsa
yaz. Tek bariz yaklaşım varsa atla, doğrudan plan.md'ye geç. Biçim seçimi
(seçenek / karar-listesi) şablonun başındaki notta anlatılır.

## 6. Muhakeme paneli — kullanıcıya SUNMADAN önce

Kullanıcı onayını vetlenmemiş materyalle isteme.

- discussion.md yazıldıysa → **varsayılan olarak** `/plan-review {NNN-slug}`
  koş. Tasarım hatası en ucuz burada yakalanır.
- Tek bariz yaklaşımlı küçük işte → atla (kullanıcı isterse koşulur).
- Panel `KIRMIZI` verirse (yaklaşım değişmeli) → kullanıcıya **gitmeden**
  seçenekleri yeniden kur, gerekirse paneli tekrarla. Kullanıcı ikinci tura
  çağrılmaz; o yalnız vetlenmiş sonucu görür.

## 7. Kullanıcıyla tartış (tek onay noktası)

> `/akis` ile koşuluyorsa adım 7'nin **onayı alınmaz**, adım 9'un tetiği
> ("phase'e geç") yerine de karar kaydının yazılmış olması geçer: karar
> panelden geçmiş öneridir. Kaydın kendisi atlanmaz; damga biçimi şablondadır.

discussion.md varsa: seçenekleri özetle, panel koştuysa mercek verdiktlerini
(`TEMİZ`/`SORUNLU`/`KIRMIZI`) ve öneriye işlenen revizyonları birlikte sun,
kendi önerini belirt, kararı bekle. Kullanıcı panelde hiç değerlendirilmemiş
bambaşka bir yön seçerse paneli o yön için tekrarlamayı öner.

discussion.md yoksa: context.md'yi özetle, önerilen yaklaşımı belirt, onay bekle.

**Karar netleşince discussion.md `## Karar` bölümüne işle** (tarih + gerekçe +
reddedilenler). Chat'te verilip dosyaya düşmeyen karar kaybolur.

## 8. plan.md

Onaydan sonra yaz. Yalnız onaylanan yaklaşımı içerir; gerekçe tartışmasını
tekrarlamaz. plan.md setin **omurgasıdır** ve çift rol taşır: onaylı tasarım
(statik) + ilerleme defteri (`## Durum`, `/implement` günceller).

## 9. Phase dosyaları

Kullanıcı planı onaylayıp "phase'e geç" dediğinde `phase-1.md` üret, gerekirse
`phase-2.md`... Her phase: değişecek dosyalar, kabul ölçütü, `## Yayın
Etkisi`, checklist.

> Phase dosyası kodun **kılavuzudur, kopyası değil**: kod örneği yazılmaz,
> hedef 3–5 KB (şablonun başındaki not). 001–006'nın phase dosyaları kod
> örnekleriyle 15–39 KB'a çıktı; `.tasks/` toplamı üretilen kodu geçti ve
> her implementer o metni bağlamında taşıdı. Bir imza ya da alan sırası
> sözleşmeyse adıyla tek satır yeter, gövdesini implementer yazar.
> Checklist'i ve kapıyı **kodlama adımı** uygular; bu skill yalnız yazar.

Phase bölmenin ölçüsü: her phase **tek başına doğrulanabilir** olmalı
(`make hepsi` yeşil bırakmalı) ve tek commit'e sığmalı. Doğrulanamayan bir
ara durum bırakan bölme yanlıştır — ya birleştir ya da sınırı kaydır. Tersine,
tek commit'lik düzeltme phase olmaz (`duzen.md` → Ek phase eşiği).

Riskli phase kutusunu (`proje.md` → Kalite kapısı) yalnız koşulu tetikleyecek
phase'e koy; set sonundaki kapı `plan.md → ## Durum`'un `kapı` satırıdır,
phase checklist'lerine yazılmaz.

Her phase'in `_Requirements:_` satırı plan.md'deki gereksinimlere bağlanır;
`/implement` ön uçuşta kapsanmayan gereksinim / öksüz phase arar.

## 10. teslim.md

Bu skill teslim.md'yi **yazmaz**. Sorumluluğu her phase'e doğru bir
`## Yayın Etkisi` bloğu koymaktır; derlemeyi `/implement` kapanışta yapar.
