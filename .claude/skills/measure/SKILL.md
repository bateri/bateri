---
name: measure
description: Projenin performans ölçümlerini (kare süresi, gecikme, bellek, açılış, bench — hangileri olduğu projenin ölçüm profilinde) koşturur, tabanla karşılaştırır ve sonucu ölçüm defterine işler. Kullanıcı "ölç", "kaç fps", "gecikme ne oldu", "bu hızlandı mı", "bench çalıştır", "ölçümleri güncelle" dediğinde kullanılır. Ölçüm bir kapı değildir — yalnız kullanıcı istediğinde koşar.
allowed-tools: Read, Edit, Write, Glob, Grep, Bash(git:*), Bash(ls:*), Bash(diff:*), Bash(sort:*), Bash(wc:*)
---

Kullanıcı bir ölçüm istiyor. Ölçüm **kapı değildir** (`duzen.md` → Kalite
kapısı → Ölçüm): gerçek ortam, sessiz makine ve dakikalar ister; phase'i
bloke etmemesi için akıştan çıkarılmıştır. Bu skill onu kullanıcı istediğinde
koşar.

Projenin hangi türleri, hangi kancayla ölçtüğü ve koşu şartları
`.claude/is-akisi/olcum.md`'de; ölçüm defterinin yolu `proje.md` →
Belgeler'de. **Yöntem kuralları burada tekrarlanmaz** — ölçmeye başlamadan
önce defterin iki bölümünü oku:

- **`## Yöntem`** — neyin nasıl ölçüleceği, gürültü eşiği, tuzaklar
- **`## Nasıl yeniden ölçülür`** — komutlar ve ortam

Bu iki bölüm tek sahiptir. Çelişki görürsen onlar kazanır; skill yalnız akışı
yürütür. Defter **yoksa** ilk ölçüm onu kurar: önce `## Yöntem` ve `## Nasıl
yeniden ölçülür` yazılır, sonra sayı girer — yöntemsiz sayı sonraki ölçümle
karşılaştırılamaz. Profil kodda emanet duran yöntem notlarını gösteriyorsa o
türün ilk ölçümü onları `## Yöntem`'e taşır.

## 1. Neyi, neye karşı

`$ARGUMENTS` ne ölçüleceğini söylüyorsa onu al; söylemiyorsa sor. Türü
profilin tablosunda bul: nasıl koşulduğu, kancası ve defterdeki bölümü orada.

Kanca sözleşmedir: kancayı taşıyan kod henüz yoksa ölçüm "yok" değil "ölçüm
aracı yok"tur — bunu söyle, sayı uydurma ve kancayı ekleyen bir iş seti öner.

**Taban olmadan ölçüm yorumlanamaz.** Karşılaştırılacak değeri defterden oku;
yoksa önce mevcut hâli ölç (`git stash` ya da değişiklikten önceki commit) ve
tabanı kaydet. "Ölçtüm, şu çıktı" tek başına bir sonuç değildir.

## 2. Ölç

Ölçüm koşarken makinede başka ağır iş olmasın; profilin koşu şartları
(güç, ekran, pencere, derleme profili) kayda yazılır. Her ölçümü **en az iki
kez** koştur ve sayılar oynuyorsa defterin gürültü kuralını uygula.

Ölçtüğün yolun gerçekten koştuğunu **doğrula**: boşta duran bir yolu ölçmek,
önbelleği ısıtmadan ölçmek, uygulanmamış bir değişikliği ölçmek — hepsi
sessiz yanlış sayı üretir. Sayıyı almadan önce yolun ateşlendiğini göster
(sayaç, log, kasıtlı bozma ile değişen sonuç). Yetersiz örnek bildiren bir
koşuyu **yorumlama**, yeniden koş.

**Optimize edilmiş derlemeyi ölç.** Hata ayıklama derlemesinin sayısı bir
taban değildir.

## 3. Zaman ölçümünde dağılım, ortalama değil

Ortalama tek başına yeterli değildir — bir takılma ortalamayı oynatmaz ama
kullanıcı onu görür. Hangi türde hangi yüzdeliklerin yazıldığı profildedir
(`olcum.md` → Koşu şartları); karşılaştırmada iki dağılımı yan yana koy:
kazancın yanında kaybı da göster.

## 4. İşle

Sonuç ölçüm defterine yazılır ve **tek sahibi orasıdır**. Başka belgeye sayı
kopyalama; o belgeler niteliksel anlatıp buraya bağlanır.

- İlgili bölümdeki eski değeri **güncelle**, yanına ikinci bir sayı ekleme.
- Ölçümün tarihini, commit'ini, makineyi ve neyin değiştiğini yaz.
- Tarihli referans kayıtlarına (`proje.md` → Belgeler) dokunma: bilerek
  eskirler.

## 5. Bekleyen iddia

Ölçülen şeyin defterin `## Bekleyen iddialar` bölümünde bir maddesi varsa o
maddeyi düş; tek liste orası. Setlerin dosyalarına dokunulmaz — set çoktan
🟢'dir, ölçüm setin durumu değildir (`duzen.md` → İndeks).

## 6. Rapor

Kullanıcıya: ne ölçüldü, taban neydi, şimdi ne, fark anlamlı mı (gürültü
eşiğinin üstünde mi), defterde hangi bölüm güncellendi; dağılımın kuyruğu
ve zincirli bir ölçümde hangi halkanın büyüdüğü.
