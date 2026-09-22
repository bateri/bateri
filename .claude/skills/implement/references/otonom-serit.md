# Otonom şerit (`--auto`) ve model katmanlaması

`/implement`'in bu dosyaya iki yerden ihtiyacı olur: kullanıcı `--auto`
verdiğinde (otonom şerit) ve bir subagent başlatırken (ajan kuralları, model
katmanlaması).

## Ajan kuralları — her subagent isteminde

Bu kurallar implementer'ın ve kapanış ajanının isteminde
**her seferinde** yazılır; subagent bu dosyayı okumaz, istemi okur.

- **Boşta bekleme yok.** `sleep`, `until [ -f … ]`, `while …; sleep` ve
  "hâlâ koşuyor mu" diye tekrar tekrar `echo`/`date` koşmak yasak. Her bekleme
  turu ajanın bütün bağlamını yeniden okutur: 004 phase-3'ün implementer'ı 929
  `echo tick` turu attı ve harcamasının büyük kısmı bekleme oldu.
- **Kapıyı ve alt ajanı ön planda koştur.** `Skill` çağrısının senkron olduğu
  **varsayılmaz**: 017/018'in kapı ajanında `/code-review` fork olarak koştu ve
  ajan sonucu beklemeden ilerledi (2026-09-20). Bir kapıyı `Skill` ile
  koşturan ajan sonucun geldiğini **görmeden** rapor yazmaz; gelmediyse
  bunu `ARTIK:` alanına yazar. Uzun komut (`make hepsi`, `make kur`) ön planda
  ve `timeout`'la koşar. Arka plana alınmış iş bitince harness ajanı zaten
  uyandırır; yoklamaya gerek yoktur.
- **Yalnız kendi başlattığın süreci kapat.** `pkill`/`killall bateri` yasak:
  kullanıcı aynı anda kendi `bateri` örneğini açık tutuyor olabilir.
- **Arkada kalanı bildir.** Raporun `ARTIK:` alanı açık kalan süreç, shell ya
  da geçici dosyayı yazar; boş değilse orkestratör kapanıştan önce temizler ya
  da kullanıcıya söyler.

**Önbellek ön koşulu.** Uzun koşan ajan (implementer, kapı) prompt
önbelleğinin çalıştığı yoldan koşmalı. 006 phase-1 ve phase-2'nin
implementer'ları proxy'li bir oturumdan açıldı (`toolUseId` `toolu_` değil
`call_` ile başlıyor), kayıtta `cache_read` sıfır ve her tur bütün bağlamı tam
fiyattan okudu. Belirti oturum dökümündedir
(`~/.claude/projects/…/subagents/*.jsonl` → `usage.cache_read_input_tokens`);
şüphede otonom şeride girmeden önce sor.

## Otonom şerit

Kullanıcı süreci komple devretti: plan onaylandı, phase'ler ardışık ve
kendiliğinden yürütülür. Döngünün adımları aynı kalır; değişenler şunlardır.

### 0. Ön uçuş

Traceability (0.3) phase dosyası açtırmaz: gereksinim listesi ve phase'lerin
`_Requirements:_` satırları grep'le okunur
(`grep -n "_Requirements:" .tasks/{set}/phase-*.md`). Resume noktası (0.4)
`plan.md → ## Durum`'dur.

### 1. Orkestratör hafızasızdır

Ana döngü **orkestratördür, kodu yazmaz** ve kendi bağlamını biriktirmez.
Taze bağlam işin subagent'ta yaşamasıyla sağlanır: subagent'ın okudukları ve
ara adımları orkestratöre dönmez, dönen tek şey raporudur.

| orkestratör okur | okumaz |
|---|---|
| `plan.md → ## Durum` (resume noktası) | phase dosyalarının gövdesi |
| ajan raporları (§3) | `git diff`'in gövdesi (yalnız `--stat`) |
| sapma anında `plan.md` → Yaklaşım/Gereksinimler | doğrulama çıktısı, kapı bulgularının gövdesi |

Kural: **orkestratörün bağlamı phase sayısıyla değil sapma sayısıyla büyür.**
Bunun bedeli tek yükümlülüktür: raporun taşıdığı bir karar bir dosyaya
düşmediyse kaybolmuştur, orkestratör onu sonraki turda hatırlamaz.

**Bir oturum, bir set.** Hafızasızlık oturumu da kapsar: set bitince devir
mesajı kullanıcıya `/clear` önerir ve aynı oturumda ikinci bir sete
başlanmaz. 001–006 tek oturumda koştu; orkestratörün bağlamı ortalama 330K'da
dolaştı ve 22 kez compact'landı — dosyalardaki kanonik kayıtla yarışan kayıplı
özetler.

### 2. Yürütme: phase başına taze implementer

Her phase için bir implementer başlat. Ona **dosya yollarını ver, içeriğini
değil** (`plan.md` + o phase dosyası) ve istemine ajan kurallarını yaz.

İmplementer kodu yazar, doğrulamayı koşar (`proje.md` → Doğrulama) ve phase'i
**tek commit**'le kapatır: kod + checklist + `## Durum` ✅. Phase riskliyse
(`proje.md` → Kalite kapısı; son phase hariç, orada set kapısı koşar — §5) `/code-review`'u da o koşturur ve doğrulamayı
**kapıdan sonra yeniden** koşar — kapı kodu değiştirir, önceki yeşil geçersizdir.

Devredilmeyen tek şey **bulgu kararıdır**: implementer gideremediği bulguyu
`WAIVE` alanına yazar, kabulü orkestratör verir.

**Phase'ler arası devir hedef dosyaya yazılır.** İmplementer bir işi başka
phase'e devrediyorsa satırı hedef phase'in checklist'ine o yazar; yazılmazsa
devir yalnız raporda kalır ve §1 gereği buharlaşır.

### 3. Rapor sözleşmesi

Rapor **sabit biçimlidir ve on iki satırı geçmez**. Anlatma, bildir.

```
DURUM:      tamam | eskalasyon
COMMIT:     {hash}
DOĞRULAMA:  make hepsi → exit {kod} · {koşullu komut} → exit {kod} | gerekmedi ({neden})
            git status → temiz | Cargo.lock değişti ({karar kaydında} | KUSUR)
KAPI:       /code-review → koştu (riskli: {neden}) | gerekmedi | [~] {gerekçe}
WAIVE:      {giderilemeyen bulgu, tek satır; gövdesi phase dosyasında} | yok
SAPMA:      {plan varsayımından sapan her şey} | yok
DEVRALINAN: {iş} ← phase-M | yok
DEVREDİLEN: {iş} → phase-N (hedef checklist'e yazıldı) | yok
ARTIK:      {arkada kalan süreç/shell/dosya} | yok
```

"gerekmedi" bir cevaptır, boş bırakmak değildir: orkestratör diff okumadığı
için koşullu komutun tetiklenip tetiklenmediğini başka yerden göremez.
Doğrulama düştüğünde `DURUM: eskalasyon` olur ve fail satırları rapora girer.

### 4. Koşullu kapı

Adım 7'nin insan onayı yerine, şunlar **birlikte** sağlanınca sonraki phase'e
otomatik geç:

1. `DOĞRULAMA` yeşil (riskli phase'de kapı sonrası koşu, koşullu komutlar dahil)
2. `KAPI` koştu ya da gerekmedi; `WAIVE` varsa onaylandı
3. `git show --stat {COMMIT}` listesinde `plan.md` var — ✅ aynı commit'e girmiş

Kontrol raporun alanlarından ve tek bir `--stat`'tan okunur. Ayrı bir sadakat
kontrolü (commit'in dosya listesini checklist'le karşılaştırmak) 001–006'da
17 phase'de 17 kez "makas yok" döndü ve kendi kuralı gereği kaldırıldı.

### 5. Set kapısı (son phase'in içinde, bir kez)

Ayrı kapı ajanı yok. **Son phase'in implementer'ı** kodu doğruladıktan sonra,
commit'ten önce `proje.md` → Kalite kapısı → "Set sonunda" adımlarını koşar
(aralık `duzen.md` → Set aralığı + çalışma ağacı), bulguları giderir,
doğrulamayı yeniden koşar ve phase'i **tek commit**'le kapatır: kod + kapı
düzeltmeleri + `## Durum`'da phase ✅ ve `kapı` ✅ + indeks 🟢. Raporunun
`SAPMA` satırına gözle kontrol sahnesini yazar; `KAPI:` satırı
`/code-review · /audit → koştu`. Bu phase'in riskli phase kapısı ayrıca
koşmaz — set kapısının `/code-review`'u onu kapsıyor. Bulgu gövdesi
orkestratöre girmez; `WAIVE` kararı orkestratörde kalır — reddedilirse
düzeltme ayrı bir commit olur (tek istisna).

### Eskalasyon — şunlarda DUR ve sor, tahmin etme

- Planı geçersiz kılan sapma. (Küçük sapma → `## Uygulama Notları`, devam.
  `plan.md`'nin Yaklaşım/Gereksinimler'ini değiştiren sapma → dur; kararı
  vermek için `plan.md` **o an** okunur.)
- Üç denemede geçmeyen test.
- Giderilemeyen ve waive de edilemeyen `/code-review` bulgusu; reddedilen `WAIVE`.
- Yayın etkili sürpriz: beklenmeyen `Cargo.lock` değişimi, yeni bağımlılık
  ihtiyacı, ayar şeması / `TERM` / shell entegrasyonu etkisi.
- Beklenmeyen ölçüm gerilemesi ya da boşta kare üreten bir yol.
- Phase dosyası dışına taşan kapsam ihtiyacı. (Tek commit'lik ek iş phase
  açmaz, `duzen.md` → Ek phase eşiği.)

Eskalasyonda durumu özetle (hangi phase, ne bulundu, seçenekler + önerin) ve
bekle. Yarıda kalan `--auto` koşusu aynı komutla kaldığı yerden devam eder.

**Bitiş:** Kapanış adımları aynen koşar; teslim/push otonom şeritte de
kullanıcıda kalır. Devir mesajı `/ship` ve `/clear` önerir.

## Model katmanlaması

İlke: *cevabı bulmak zorsa ucuz model çok yer gezsin; cevabı görmek zorsa güçlü
model dar bağlamda düşünsün.* Tek-oy karar noktaları güçlü kalır. Agent
çağrılarında `model:` açıkça geç.

| Katman | Model | Neden |
|---|---|---|
| Okuma/haritalama/grep fan-out'ları | `sonnet` | çıkarım az, aktarım çok |
| `/code-review` FIND — mekanik açılar | `sonnet` | fan-out genişliği telafi eder |
| `/code-review` FIND — semantik açılar, VERIFY, SWEEP | `opus` | tek oy karar verir |
| `/plan-review` jürileri | `opus` | mercek başına tek ses |
| `/audit` yargı mercekleri (fan-out olursa) | `opus` | dosyalar arası akıl yürütme |
| `/akis` — `/rfc` ajanı | `opus` | tasarım kararı üretir |
| Otonom şerit — implementer (son phase'de set kapısı dahil) | `opus` | kodlar, kapıyı koşturur |
| Sentez, bulgu değerlendirme, "hangi sapma kabul" | ana döngü | devredilmez |
