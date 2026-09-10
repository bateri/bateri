# Otonom şerit (`--auto`) ve model katmanlaması

`/implement`'in bu dosyaya iki yerden ihtiyacı olur: kullanıcı `--auto`
verdiğinde (otonom şerit) ve kalite kapısında subagent fan-out'u koşarken
(model katmanlaması).

## Otonom şerit

Kullanıcı süreci komple devretti: plan onaylandı, phase'ler ardışık ve
kendiliğinden yürütülür. Döngünün adımları aynı kalır; değişenler şunlardır.

### 0. Ön uçuş orkestratöre girmez

Adım 0 bütün phase dosyalarını okutur (yükleme, sıralama, traceability).
Otonom şeritte bu okuma **orkestratöre girmez**: 0.1–0.3 bir subagent'a
devredilir ve dönen şey ihlal listesidir — kapsanmayan gereksinim,
`_Requirements:_` satırı olmayan phase, doğal sıralamadaki sürpriz. Liste
boşsa orkestratör dosyaların adlarından fazlasını bilmez.

Resume noktası (0.4) orkestratörün kendi okumasıdır ve tek satırdır:
`plan.md → ## Durum`.

### 1. Orkestratör hafızasızdır

Ana döngü **orkestratördür, kodu yazmaz** — ve kendi bağlamını da
biriktirmez. Otonom şeridin bütün kazancı buna bağlıdır: taze bağlam
`/clear` ile değil, **işin subagent'ta yaşamasıyla** sağlanır. Subagent'ın
okudukları, koşturduğu komutlar ve ara adımları orkestratöre dönmez; dönen
tek şey final rapordur. Korunması gereken şey bu yüzden subagent'ın işi
değil, **orkestratörün kendi okumalarıdır**.

| orkestratör okur | okumaz |
|---|---|
| `plan.md → ## Durum` (resume noktası) | phase dosyalarının gövdesi |
| implementer raporları (§3) | `git diff`'in gövdesi (§4 yalnız `--stat`) |
| sapma anında `plan.md` → Yaklaşım/Gereksinimler | doğrulama komutlarının tam çıktısı |
| | kapı bulgularının gövdesi |

Kural tek cümledir: **orkestratörün bağlamı phase sayısıyla değil sapma
sayısıyla büyür.** 011'in dört phase dosyası 1.478 satırdır; orkestratör
onları okusaydı, kararı hiç gerekmeyen ayrıntıyı setin sonuna kadar taşırdı.

Hafızasızlığın bedelsiz olması `duzen.md` → Durum'a dayanır: hatırlanması
gereken her şey zaten diskte yaşar. Buradan tek bir ek yükümlülük doğar ve
otonom şeridin en kolay unutulan kuralı budur: **raporun taşıdığı bir bilgi,
karar verildikten sonra bir dosyaya düşmediyse kaybolmuştur.** Orkestratör
onu bir sonraki turda hatırlamayacaktır.

### 2. Yürütme: phase başına taze implementer

Her phase için bir implementer subagent başlat. Ona **dosya yollarını ver,
içeriğini değil** (`plan.md` + o phase dosyası; ikisini kendisi okur).

Kalite kapısını da **implementer koşturur**. Bu, insan-döngüdeki adım 5'in
("kapıyı sen koşturursun") otonom şeritteki karşılığıdır ve sebebi yine
bağlamdır: kapının fan-out'ları zaten taze ve bağımsız agent'lardır, ama
bulguların **gövdesi** orkestratörde koşsaydı orkestratörün bağlamına
girerdi. Kapının iniş sırası `proje.md` → "Kapıyı kim koşturur".

Devredilmeyen tek şey **bulgu kararıdır**: implementer gideremediği ya da
kapsam dışı bulduğu bulguyu raporun `WAIVE` alanına yazar, kabul kararını
orkestratör verir (model katmanlaması tablosunun son satırı).

**Doğrulama kapıdan sonra yeniden koşar.** Kapı kodu değiştirir, dolayısıyla
kapıdan önceki yeşil ölçüm geçersizdir. Kaynak projede (odunluk, 012 phase-2) `/code-review`'un
düzeltmesi türetilmiş bir dosyayı kırmış ve kusuru `/ship` yakalamıştı — **otonom şeritte `/ship` koşmaz**, o yüzden
raporun `DOĞRULAMA` alanı kapı sonrası koşuyu bildirir.

**Phase'ler arası devir iki yönlüdür ve hedef dosyaya yazılır.** İmplementer
bir işi başka phase'e devrediyorsa satırı **hedef phase dosyasının
checklist'ine** o yazar; devraldığı iş varsa raporun `DEVRALINAN` alanı
taşır. Yazılmazsa devir yalnız raporda kalır ve §1 gereği buharlaşır: 011'de
`Stem` yüzeyi phase-4'ten phase-3'e alınmıştı, hedefe yazılmasaydı phase-4'ün
implementer'ı işi yapılmış bulur, sadakat kontrolü de onu eksik sanardı.

### 3. Rapor sözleşmesi

Dönen rapor **sabit biçimlidir ve on beş satırı geçmez**. Serbest anlatı iki
şeyi birden bozar: orkestratörün bağlamını taşır ve koşullu kapının (§5)
şartlarını raporun içinde görünmez kılar. Anlatma, bildir.

```
DURUM:      tamam | eskalasyon
COMMIT:     {kod hash} [+ {durum hash}]
DOĞRULAMA:  make hepsi → exit {kod} · {koşullu komut} → exit {kod} | gerekmedi ({neden})
            git status → temiz | Cargo.lock değişti ({karar kaydında} | KUSUR)
KAPI:       /simplify · /code-review · /audit → koştu | [~] {gerekçe}
WAIVE:      {giderilemeyen bulgu, tek satır; gövdesi phase dosyasında} | yok
SAPMA:      {plan varsayımından sapan her şey} | yok
DEVRALINAN: {iş} ← phase-M | yok
DEVREDİLEN: {iş} → phase-N (hedef checklist'e yazıldı) | yok
```

`DOĞRULAMA`'nın koşullu komutları `proje.md`'de tanımlıdır (`make shader`,
`make terminfo`, `make test-yaris`, `make duman`); "gerekmedi" bir cevaptır, boş bırakmak
değildir — orkestratör diff okumadığı için koşulun tetiklenip
tetiklenmediğini başka yerden göremez.

`WAIVE` ile `KAPI`'nın `[~]`'si karıştırılmaz: ilki bir *bulgunun*
waive'idir, ikincisi *kapının* atlanmasıdır (`proje.md` → İz).

Doğrulama düştüğünde `DURUM: eskalasyon` olur ve **fail satırları rapora
girer**; geçtiğinde satır bir iddiadır ve kontrol edilebilirliği §4'e
dayanır. "Testler geçti" cümlesi hiçbir durumda alanın yerini tutmaz.

### 4. Sadakat: ucuz kontrol, ölçülmüş karar

Rapor kendini denetleyemez: checklist'i işaretleyen ile işi yapan aynı
subagent'tır. Koşan kapıların hiçbiri bu boşluğu kapatmaz — `make hepsi`
"kod çalışıyor mu" der, `/simplify` ve `/code-review` phase dosyasını **hiç
görmez**, `/audit` projeye özgü kurallara bakar, kapanıştaki eksik-checklist
taraması (adım 8) kutunun *işaretini* sayar, doğruluğunu değil.

Kontrol orkestratördedir ve ucuzdur: `git show --stat {commit}` çıktısını
phase'in checklist'iyle karşılaştır. Dosya listesi bir avuç satırdır ve
diff'in gövdesi okunmaz, yani §1 delinmez. Aynı çıktı koşullu kapının
dördüncü şartını da kanıtlar: `plan.md` listede yoksa `## Durum`
yazılmamıştır (ayrı commit'e düştüyse `COMMIT` iki hash taşır).

Makas çıkarsa **aynı implementer'a geri dön** — bağlamı hâlâ ayakta, phase'i
yeniden anlatmak gerekmez. Geri sarma refleksi yanlıştır ve tuzağı somuttur:
`/rfc` seti commit'lemez, phase dosyaları çoğu sette ilk kez o phase'in kod
commit'iyle depoya girer (011'in dördü birden `ead9d66` ile), yani
`reset --hard {commit}~1` kılavuzun kendisini çalışma ağacından siler.
(`/akis` ile koşulan işte set koddan önce commit'lenir ve tuzak orada
kapanır; gerekçesi o skill'in §2'sindedir.)

**Ayrı bir denetçi subagent kurulmadı ve bu bilinçlidir.** Gerekçesi
ölçülmemişti; bu depoda ölçülmemiş mekanizma kurulmaz, üstelik devir doğru
yazılmadığında ilk işi yanlış pozitif üretmek olurdu. Ölçüm bu adımın
kendisidir ve kontrol **koştuğunda her hâlükârda iz bırakır**: phase'in
`## Uygulama Notları`'na tek satır düşer — `sadakat: makas yok` ya da
`sadakat: {ne eksikti}`. Satırın yokluğu sıfır makas değil, koşmamış kontrol
demektir; ikisi ayrışmasaydı iki set sonraki sayım boş kümeyle temiz kümeyi
aynı görürdü. O sayım sıfır çıkarsa kontrol de kalkar; dolu çıkarsa ayrı bir
denetçinin gerekçesi kanıtlanmış olur ve dar soruyla kurulur.

### 5. İnsan kapısı → koşullu kapı

Adım 7'nin insan onayı yerine, şunlar **birlikte** sağlanınca sonraki
phase'e otomatik geç:

1. Doğrulama yeşil — `DOĞRULAMA` (kapı sonrası koşu, koşullu komutlar dahil)
2. Kapı koştu; bulgu giderildi ya da waive'i onaylandı — `KAPI` + `WAIVE`
3. Sadakat kontrolü makassız (§4)
4. Commit atıldı ve `## Durum` güncellendi — `COMMIT` + §4'ün `--stat`'ı

Şartlar raporun alanlarından ve tek bir `--stat` çıktısından okunur: kapı,
orkestratöre phase dosyası ya da diff gövdesi açtırmaz. Açtırsaydı §1 her
phase'de bir kez delinirdi.

İzlenebilirlik disiplini aynen sürer: kullanıcı `git log` + Durum
tablosundan her an denetleyebilir.

### Eskalasyon — şunlarda DUR ve sor, tahmin etme

- Planı geçersiz kılan sapma. (Küçük sapma → `## Uygulama Notları`'na yaz,
  devam. `plan.md`'nin Yaklaşım/Gereksinimler'ini değiştiren sapma → dur.)
  Ayrımı raporun `SAPMA` alanı taşır; kararı vermek için `plan.md` **o an**
  okunur — §1'in tablosundaki tek koşullu okuma budur.
- Üç denemede geçmeyen test.
- `/code-review`'un giderilemeyen, waive de edilemeyen bulgusu.
- Reddedilen `WAIVE` önerisi: bulgu ne giderildi ne kabul edildi, karar
  kullanıcınındır.
- Aynı phase'de ikinci sadakat makası.
- Yayın etkili sürpriz: beklenmeyen `Cargo.lock` değişimi, yeni bağımlılık
  ihtiyacı, ayar şeması / `TERM` / shell entegrasyonu etkisi.
- Beklenmeyen ölçüm gerilemesi ya da boşta frame üreten bir yol. (Bir
  efekti bilinçli sadeleştiren düzeltme gerileme olmayabilir — ama bu kararı
  ana döngü vermez.)
- Phase dosyası dışına taşan kapsam ihtiyacı.

Eskalasyonda durumu özetle (hangi phase, ne bulundu, seçenekler + önerin) ve
bekle. Yarıda kalan `--auto` koşusu aynı komutla kaldığı yerden devam eder.

**Bitiş:** Kapanış adımları aynen koşar; teslim/push otonom şeritte de
kullanıcıda kalır.

## Model katmanlaması

İlke: *cevabı bulmak zorsa ucuz model çok yer gezsin; cevabı görmek zorsa güçlü
model dar bağlamda düşünsün.* Redundant katmanlar (çok açı + sweep birbirini
telafi eder) ucuzlayabilir; **tek-oy karar noktaları güçlü kalır**. Agent
çağrılarında `model:` (gerekirse `effort:`) açıkça geç.

| Katman | Model | Neden |
|---|---|---|
| Okuma/haritalama/grep fan-out'ları | `sonnet` | çıkarım az, aktarım çok |
| `/simplify` — reuse + simplification mercekleri | `sonnet` | mekanik karşılaştırma |
| `/simplify` — efficiency + altitude mercekleri | `opus` | yargı içerir |
| `/code-review` FIND — mekanik açılar (satır tarama, konvansiyon, cross-file trace) | `sonnet` | fan-out genişliği telafi eder |
| `/code-review` FIND — semantik açılar (eşzamanlılık, hata politikası, dil tuzağı) | `opus` | derin muhakeme |
| `/code-review` VERIFY | `opus` (+ `effort: 'high'`) | **tek oy karar verir**; yanlış REFUTED gerçek bug'ı öldürür |
| `/code-review` SWEEP | `opus` | boşluk bulmak yargı ister |
| `/plan-review` jürileri | `opus` | mercek başına tek ses |
| `/audit` — mekanik mercekler (katman, `Cargo.lock`, panik yolu, ayar şeması, shell üçlüsü, ölçüm sahipliği) | ajansız | grep, `cargo tree` ve dosya varlığı |
| `/audit` — yargı mercekleri (thread/blokaj, boşta sıfır kare, hücre boyutu ve shader düzeni, üslup) | `opus` | dosyalar arası akıl yürütme |
| `/akis` — keşif ve tasarım aşamaları | `opus` | tasarım kararı üretir |
| Otonom şerit — ön uçuş devri (§0) | `sonnet` | okuma ve eşleştirme, yargı az |
| Otonom şerit — implementer subagent | `opus` | phase'i kodlar, kapıyı da o koşturur |
| Sentez, bulgu değerlendirme, "hangi sapma kabul" | ana döngü | devredilmez |
