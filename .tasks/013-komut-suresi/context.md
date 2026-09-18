# Komut süresi sayacı — Bağlam

## Mevcut Durum

Komut blokları 010'da indi: her prompt bir blok kimliği basıyor, `frame()` o
kimlikleri prompt'un OSC 8 çıpasından okuyup blokları **komutun satırı ve
rengi** olarak sınırdan veriyor, `bt-gpu` de işareti ızgaranın 0. sütununa
çiziyor. Renk safhayı anlatıyor: `accent` koşuyor, `success`/`error` bitti.

Defterin kendisi `ShellLog::blocks` — `BlockLog`, bir `VecDeque<Outcome>` +
`first: u32` + `capacity`. `Outcome` iki değerli: `Pending` ve
`Finished(Option<i32>)`. Kayıt OSC 133'ten geliyor: `C` (`Mark::CommandStart`)
safhayı `Running` yapıyor, `D` (`Mark::CommandEnd { exit, id }`) hem safhayı
`Finished`'a çeviriyor hem `blocks.finish(id, exit)` diyor.

Yani bloğun **ne zaman** başladığı ve **ne kadar** sürdüğü hiçbir yerde
tutulmuyor: iki işaret de görülüyor ama arasındaki zaman ölçülmüyor.

## Motivasyon

Kullanıcı ızgarada, komut satırının **en sağında**, komutun ne kadar sürdüğünü
görmek istiyor — bitmiş komutlarda son süre, koşan komutta canlı sayan bir
sayaç, ve **yalnız bir saniyeyi geçenlerde**.

Eşiğin kendisi talebin özü: her `ls`'in yanında `0.01s` yazması gürültüdür.
Bir saniyeyi geçen komut ise iki soruyu doğuruyor — koşarken "asıldı mı?",
bittikten sonra "ne kadar sürdü?" — ve ikisinin cevabı aynı sayı.

Referans ürün bunu yapıyor ve ayarını da açıyor: `docs/ARASTIRMA.md` → satır 98
`command_duration_threshold`, satır 108 "Komut blokları (süre, kırmızı
gutter)". Yani hem özellik hem eşiğin ayarlanabilirliği envanterde.

## Kanıt

**Canlı sayaç yeni bir kare kaynağıdır.** Bugün kare istemenin iki yolu var ve
ikincisi kimseyi uyandırmıyor:

1. **hasar** — kirli satır; PTY'den bayt gelince doğuyor.
2. **hareket** — `bt-gpu::motion`'ın yerleşmemiş animatörü; link'i ekran
   hızında uyanık tutuyor ve durma koşulu taşıyor.

Koşan bir komutun sayacı ikisine de uymuyor:

- **hasar değil**, çünkü `sleep 5` boyunca PTY'den tek bayt gelmiyor — ekran
  değişmeli ama grid değişmiyor.
- **hareket olmamalı**, çünkü hareket saati link'i ekran hızında (60–120 Hz)
  uyanık tutuyor. Saniyede bir değişen bir sayı için 30 saniyelik bir komutta
  ~2000 kare üretirdi; oysa gereken 30.

`bt-gpu::link`'in modül başlığındaki kural ("zamana bağlı kare talebinin tek
yolu hareket saatidir") bu durumu **yasaklamıyor, kapsamıyor**: yazıldığında
zamana bağlı tek kare kaynağı animasyonlardı. Kuralın gerekçesi de bunu
söylüyor — yasak, `Waker`'dan istenen bir hareket karesinin "kendini içerik
diye saydırması" yüzünden. Sayaç tiki ise **gerçekten içeriktir**: ızgaranın
çizilen çıktısı değişiyor, yani `icerik` karesi sayması doğru.

Yani sözleşme ihlal edilmiyor, **eksik tanımlı**. Bu set onu üçe tamamlıyor.

## Mevcut Mimari

| yer | bugün ne var | bu setin dokunacağı |
|---|---|---|
| `bt-core::shell` | `BlockLog`, `Outcome`, `ShellLog`; OSC 133 kayıtları | süre `Outcome`'a girer, başlangıç `ShellLog`'a |
| `bt-core::session` | `frame()`; blok çıpalarını `Term` kilidi altında toplar, rengi kilitten sonra çözer | sayaç hücreleri aynı sink'ten çıkar |
| `bt-gpu::link` | kare kararı: hasar + hareket | üçüncü sebep (**saat**) ve durma koşulu |
| `bt-gpu::frame` | `push_block` işareti 0. sütuna çiziyor | dokunulmuyor — sayaç sıradan hücre |

Ölçü birimi kararı burada da geçerli: **karar `bt-core`'da, boyama `bt-gpu`'da.**
Sayacın metnini, rengini ve nereye sığdığını `bt-core` söylüyor; `bt-gpu`
gelen hücreleri ötekilerden ayırt etmiyor bile.
