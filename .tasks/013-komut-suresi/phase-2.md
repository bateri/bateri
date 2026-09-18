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
  başka bir şey gösterecek. "Ne zaman değişecek" biçimin bir sonucu (ilk 10
  saniyede onda bir, sonra saniye) ve biçim `bt-core`'un kararı; `bt-gpu`
  hesaplamıyor, bekliyor. `None` = koşan sayaç yok = **durma koşulu**.
- **Saatin yeri `bt-gpu`**, link'in yanı. Kare talebi link'in işi, `dispatch2`
  orada zaten var (metallib, ana kuyruk) ve sözleşmenin yazılı olduğu modül
  orası. `bt-shell` bu sete hiç girmiyor.
- **Saat yeniden dikilir, biriktirilmez.** Her kare bir sonraki tiki yeniden
  söylüyor ve bekleyen eski talep iptal ediliyor; iki tik üst üste binmiyor.
- **Saat armed değilken hiç okunmuyor.** `next_tick` `None` ise tek bir saat
  okuması bile yok — boşta sıfır kare kapısının operandı büyümüyor.
- **Sözleşme aynı commit'te güncelleniyor:** `bt-gpu::link`'in modül başlığı
  ve `CLAUDE.md`'nin "boşta sıfır kare" maddesi üç sebebi de sayıyor ve
  saatin **adlandırılmış durma koşulu** taşıması şartını yazıyor. Koşan
  komutun olduğu pencere "boşta" değildir; bu cümle açıkça giriyor.

## Değişiklikler

- **`crates/bt-core/src/session.rs`** — `Cursor.next_tick: Option<Duration>`;
  `frame()` onu koşan bloğun yaşından ve biçimin kademesinden türetiyor.
- **`crates/bt-gpu/src/link.rs`** — saat kaynağı: `next_tick` dolu ise o süre
  sonrası için bir uyandırma dikiliyor, boşsa bekleyen iptal ediliyor. Modül
  başlığı üç sebebi sayıyor.
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

## Checklist

- [ ] `Cursor.next_tick` + `frame()` onu türetiyor
- [ ] Saat kaynağı `link.rs`'te; dikme/iptal tek yerde
- [ ] Test: `next_tick` eşiğin altında `None`, üstünde dolu
- [ ] Test: komut bitince `next_tick` `None`
- [ ] `link.rs` modül başlığı üç sebebi sayıyor
- [ ] `CLAUDE.md` "boşta sıfır kare" maddesi güncel
- [ ] Gerçek pencerede gözle: `sleep 5`, sonra boşta kare yok
- [ ] Doğrulama geçti (`make hepsi`)
- [ ] Yayın etkisi yazıldı
