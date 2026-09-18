# Phase 2 — Saat: kare talebinin üçüncü sebebi

## Özet

Koşan komutun sayacı, PTY'den bayt gelmese bile ilerlesin. Kare talebinin
**üçüncü** sebebi doğuyor — hasar ve hareketin yanına **saat** — ve sözleşme
aynı commit'te üçe tamamlanıyor.

_Requirements: R2, R5_

## Bağlam

`sleep 5` boyunca PTY'den tek bayt gelmiyor: hasar yok, hareket yok, kare yok.
Sayaç phase-1'de çiziliyor ama kimse onu yeniden çizmiyor.

İki mevcut yol da uymuyor:

- **hasar** grid'in değişmesine bağlı; koşan komut grid'i değiştirmiyor.
- **hareket** link'i ekran hızında (60–120 Hz) uyanık tutuyor. Saniyede bir
  değişen bir sayı için 30 saniyelik komutta ~2000 kare üretirdi; gereken 30.

`bt-gpu::link`'in modül başlığındaki kural bu durumu **yasaklamıyor,
kapsamıyor**: yazıldığında zamana bağlı tek kare kaynağı animasyonlardı.
Yasağın gerekçesi de bunu söylüyor — `Waker`'dan istenen bir **hareket**
karesinin "kendini içerik diye saydırması". Sayaç tiki ise gerçekten içerik:
ızgaranın çizilen çıktısı değişiyor.

## Kararlar

- **Bir sonraki tiki `bt-core` söyler.** `Cursor` yeni bir alan alıyor:
  `next_tick: Option<Duration>` — bu karenin çizdiği sayaç ne kadar sonra
  başka bir şey gösterecek. "Ne zaman değişecek" biçimin doğrudan sonucu ve
  **kademe başına ayrı**: saatin altında bir sonraki tam saniye, saatin
  üstünde bir sonraki tam dakika (`1h 07m` dakikada bir değişiyor). Biçim
  `bt-core`'un kararı olduğu için çözünürlük de orada; `bt-gpu` hesaplamıyor,
  bekliyor. `None` = koşan sayaç yok = **durma koşulu**.
- **Saatin yeri `bt-gpu`**, link'in yanı. Kare talebi link'in işi, `dispatch2`
  orada zaten var (metallib, ana kuyruk) ve sözleşmenin yazılı olduğu modül
  orası. `bt-shell` bu sete hiç girmiyor.
- **Tik birikmiyor.** Saat yalnız link uyumaya giderken kuruluyor, yani aynı
  anda en çok bir bekleyen tik var. `DispatchQueue::after` **iptal
  edilemediği** için araya bir hasar karesi girip saati yeniden kurduğunda
  eski tik yine ateşleniyor; kuşak numarasını doğrulayıp susuyor.
- **Saat armed değilken hiç okunmuyor.** `next_tick` `None` ise tek bir saat
  okuması bile yok — boşta sıfır kare kapısının operandı büyümüyor.
- **Sözleşme aynı commit'te güncelleniyor:** `bt-gpu::link`'in modül başlığı
  ve `CLAUDE.md`'nin "boşta sıfır kare" maddesi üç sebebi de sayıyor ve
  saatin **adlandırılmış durma koşulu** taşıması şartını yazıyor. Koşan
  komutun olduğu pencere "boşta" değildir; bu cümle açıkça giriyor.

## Değişiklikler

- **`crates/bt-core/src/session.rs`** — `Cursor.next_tick: Option<Duration>`;
  `frame()` onu koşan bloğun yaşından ve biçimin kademesinden türetiyor.
- **`crates/bt-gpu/src/link.rs`** — saat kaynağı (`LinkDelegate::arm_clock`):
  link uyumaya giderken `next_tick` doluysa o süre sonrası için bir uyandırma
  dikiliyor. Modül başlığı üç sebebi sayıyor ve ayıran üç şartı yazıyor.
- **`CLAUDE.md`** — "boşta sıfır kare" maddesi üçe tamamlanıyor.

## Kabul

- `sleep 5` koşarken sayaç saniyede bir ilerliyor ve komut bitince son
  değerinde donuyor.
- Komut bittikten sonra pencere boşta: yeni kare istenmiyor.
- Bir saniyenin altındaki komut hiç tik doğurmuyor.
- Boş bir pencerede (komut koşmuyorken) kare sayısı bugünküyle aynı.
- `integration = "off"` oturumunda saat hiç kurulmuyor.

## Yayın Etkisi

- **`make duman` etkilenmiyor**, ama sebebi yapısal ve yazılması şart: reçete
  `/bin/sh` koşuyor, OSC 133 basmıyor, blok doğurmuyor — yani saat hiç
  **armed** olmuyor.
- **Adlandırılmış sınır:** `QUIET_FLOOR = 868 ms` ile 1000 ms'lik tik periyodu
  **ilkesel olarak uyumsuz** — kapı "son içerik karesinden sonraki sessizlik"
  ölçüyor ve saniyede bir kare isteyen bir yük onu 868 ms'nin altına
  düşürebilir. Bugün çarpışmıyorlar çünkü reçete entegrasyonsuz. İleride bir
  yük gerçekten kabuk entegrasyonu koşarsa kapının `saat=` jetonuyla
  saatin meşru karelerini ayırt etmesi gerekir. **Bu sette düzeltilmiyor,
  adlandırılıyor.**
- **Ölçüm bekliyor:** "saat armed'ken kare maliyeti" iddiası — koşan komut
  boyunca saniyede bir kare. Kanca var (`BT_FRAME_STATS`), yükü yok:
  entegrasyonlu bir ölçüm yükü bugün tanımlı değil.
- ayar şeması, tema, terminfo, kabuk betiği, shader, yeni bağımlılık: yok.

## Uygulama Notları

- **Biçim kararı düzeltildi (plan Karar 5).** phase-1 onda biri koşan sayaçta
  da gösteriyordu ve maliyeti hesaplanmamıştı: her değişim bir kare istediği
  için ilk on saniye **saniyede on kare** ederdi — setin kendi "saniyede bir"
  gerekçesiyle çelişiyordu. `Precision::{Whole, Tenths}` ayrımı girdi; koşan
  sayaç tam saniye, bitmiş değer onda bir. Bedelsiz, çünkü bitmiş değer donmuş.
- **Saat uyku noktasında kuruluyor**, içerik karesinde değil. Uyanıkken
  kurulsaydı her içerik karesi bir tik daha dikerdi; link ancak yapacak işi
  kalmayınca uyuduğu için tik de tam o an gerekiyor. Sonuç: aynı anda en çok
  bir tik.
- **Kuşak sayacı** (`clock_generation`) gerekli çıktı: `DispatchQueue::after`
  iptal edilemiyor, yani araya giren bir hasar karesi saati yeniden kurduğunda
  eski tik yine ateşleniyor. Kuşak eşleşmeyince susuyor — yoksa her yeniden
  kurulum fazladan bir içerik karesi doğururdu.
- **Alternatif ekranda saat yok:** `resolve_blocks` dalı hiç koşmuyor, yani
  `next_tick` `None` kalıyor. Ayrı bir kol yazılmadı, yapısal olarak kapalı.
- **`after`'ın `Result`'ı** yutuluyor ve gerekçesi kodda: düşen bir tik yalnız
  sayacı durdurur, bir sonraki hasar karesi saati yeniden kurar.
- **Kilit kapsamı bir tık genişledi** (denetim, 4. mercek — bulgu değil kayıt):
  `resolve_blocks` artık `sink`'i `shell` yaprak kilidi altında çağırıyor ve
  `sink` `bt-gpu`'da bir `Vec::push` (ayırma yapabilir). Öncesinde kilit yalnız
  ucuz çözüm döngüsünü kapsıyordu. Ölçek mikrosaniye ve kare başına en çok bir
  avuç hücre; okuyucu thread aynı kilide olay başına giriyor, yani bedel
  görünür değil ama **kayıtlı** olmalı. `Term` kilidi zaten düşmüş durumda,
  yani kilit sırası kuralı etkilenmiyor.
- **Ölçülen şey komutun kendisi değil, `C`–`D` arası.** Kancalarımız
  `add-zsh-hook` ile sona ekleniyor (gerekçeleri betikte), yani kullanıcının
  kendi kancaları ikisinden de önce koşuyor: `C` geç basılıyor (süre kısalır),
  `D` kullanıcının `precmd`'lerinden sonra basılıyor (uzar). Kendi işimiz payın
  **dışında** — `D` `precmd`'in ilk işi, dalın `git` fork'undan önce.
  Düzeltilmedi: kancayı ikiye bölmek betiği değiştirir (`make kur` zorunlu
  olur) ve pay gösterilen 0,1 saniyelik kademenin altında kalıyor. Sınır
  `Outcome::elapsed_ms`'in doc'unda ve `teslim.md`'de.

## Checklist

- [x] `Cursor.next_tick` + `frame()` onu türetiyor
- [x] Saat kaynağı `link.rs`'te; dikme/iptal tek yerde
- [x] Test: `next_tick` eşiğin altında `None`, üstünde dolu
- [x] Test: komut bitince `next_tick` `None`
- [x] Test: koşan sayaç tam saniye, bitmiş olan onda bir
- [x] `link.rs` modül başlığı üç sebebi sayıyor
- [x] `CLAUDE.md` "boşta sıfır kare" maddesi güncel
- [~] Gerçek pencerede gözle: `sleep 5`, sonra boşta kare yok — **kullanıcıda**, `teslim.md` B.2-B.3 (`make duman` ajanın kabuğunda yanlış tanıyla düşüyor)
- [x] Doğrulama geçti (`make hepsi`)
- [x] Yayın etkisi yazıldı
