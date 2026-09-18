# Phase 1 — Süre defterde, sayaç hücrede

## Özet

Blok defteri geçen süreyi de tutsun ve `frame()` onu komut satırının sağ
ucunda hücre olarak versin. Bu phase **canlı tik getirmiyor**: biten komutun
süresi görünür oluyor, çünkü komut bitince zaten bir kare var. Yeni kare
kaynağı phase-2'nin işi.

_Requirements: R1, R3, R4, R6_

## Bağlam

Defter bugün `Outcome::{Pending, Finished(Option<i32>)}`. `C` işareti safhayı
`Running` yapıyor, `D` işareti `blocks.finish(id, exit)` diyor — arasındaki
zaman hiç ölçülmüyor.

Çizim tarafında `Blocks` iki fazlı: faz 1 `Term` kilidi altında çıpaları
(`(kimlik, satır)`) topluyor, faz 2 kilit düştükten sonra defterden renk
çözüp `Block` listesini üretiyor. Sayacın ihtiyacı tam olarak bu ikisinin
kesişimi — satır faz 1'den, süre faz 2'den — ve `sink` faz 2 çağrılırken
hâlâ kapsamda, yani sayaç hücreleri için ikinci bir yola gerek yok.

## Kararlar

- **Süre `Outcome::Finished`'ın içinde**, yanında bir tabloda değil: bloğun
  akıbetiyle aynı ömre sahip ve halka tahliyesi ikisini birlikte atıyor. İki
  yapı olsaydı tahliye iki yerden yürütülürdü.
- **Başlangıç anı `ShellLog`'da tek alan** (`running_since: Option<Instant>`),
  girdi başına değil: aynı anda tek komut koşuyor. 10 000 girdinin her birine
  `Instant` koymak sekme başına ödenen ölü bir bedel olurdu.
- **Sayaç yeni bir sınır tipi değil.** `Block`'a alan eklemek yerine
  `resolve_blocks` metni üretip mevcut `sink`'e hücre basıyor. `bt-gpu` onu
  ızgaranın herhangi bir harfinden ayırt etmiyor, yani çizen tarafta tek satır
  kod değişmiyor.
- **Son dolu sütun faz 1'de toplanıyor.** `Blocks::anchors` üçüncü bir
  alan alıyor: `(kimlik, satır, son dolu sütun)`. Tip **opak**, yani
  `pub` API değişmiyor. İkinci bir tarama yapılsaydı `Term` kilidi yeniden
  alınırdı.
- **Çakışmada sayaç kaybeder.** Sayaç ile satırın son **dolu** hücresi arasında
  en az bir boş hücre kalmıyorsa sayaç o satırda **hiç** çizilmiyor.
  Kullanıcının yazdığını örtmek yerine sayacı gizlemek; yön güvenli ve ölçütü
  kesin. *(Kapıda daraltıldı: ölçüt "mürekkep" değil "dolu" — seçim vurgusu
  ve geniş glyph'in ikinci yarısı da sütunu işgal ediyor.)*
- **Biçim okuma sorusuna göre:** 10 saniyenin altında onda birli (`1.4s`),
  üstünde tam saniye (`12s`), dakikadan sonra `1m 05s`, saatten sonra
  `1h 02m`. Altındaki soru "ne kadar sürdü", üstündeki "asıldı mı" — ikincide
  ondalık gürültü. Metin yığın tamponunda üretiliyor, kare başına `String`
  yok. *(phase-2'de daraltıldı: onda bir **yalnız bitmiş** komutta kalıyor,
  koşan sayaç her zaman tam saniye — gerekçesi orada.)*
- **Renk `dim`.** Sayaç bloğun üstverisi, komutun parçası değil — dock'un
  bağlam satırıyla aynı sınıf ve aynı rolden.
- **Eşik `1s` tasarım sabiti**, ölçüm değil: `docs/OLCUMLER.md`'ye girmiyor.

## Değişiklikler

- **`crates/bt-core/src/shell.rs`**
  - `Outcome::Finished` → `{ exit: Option<i32>, elapsed_ms: u32 }`.
  - `BlockLog`'un doc'undaki bütçe: kayıt başına 8 → **12** bayt, 10 000
    satırda 80 → 120 KB. `const` assert ile bağlı — elle toplandığında 16
    çıkıyor ve ilk yazımda öyle yazılmıştı; Rust `Option<i32>`'nin
    etiketindeki niche'i `Outcome`'ın ayrımı için kullanıyor.
  - `BlockLog::finish(id, exit, elapsed_ms)`.
  - `ShellLog.running_since: Option<Instant>`; `CommandStart` dikiyor,
    `CommandEnd` tüketiyor.
  - `ShellLog::duration(id, running) -> Option<Duration>` — biten blokta
    kayıtlı süre, koşan blokta `running_since`'in yaşı.
  - `Counter` — sayacın metnini **yığında** üreten tip (`fmt::Write`, kare
    başına ayırma yok); yanında `COUNTER_FLOOR` ve `COUNTER_TENTHS_UNTIL`.
- **`crates/bt-core/src/session.rs`**
  - `Blocks::anchors`: `Vec<(u32, u16, u16)>`; faz 1 satırın son **dolu**
    sütununu da kaydediyor (mürekkep, zemin ya da kural çizgisi).
  - `resolve_blocks` `&mut sink` alıyor ve eşiği geçen blokların sayacını
    sağa yaslayarak basıyor.

## Kabul

- Bir saniyeyi geçen komut bittiğinde süresi komut satırının sağ ucunda,
  sönük renkte görünüyor.
- Bir saniyenin altındaki komutta hiçbir sayaç hücresi doğmuyor.
- Sayaç komutun metnine değmiyor; uzun komutta sayaç kayboluyor, metin değil.
- Süre scrollback'te yukarı kayarken bloğuyla birlikte taşınıyor.
- `integration = "off"` ve `/bin/sh` oturumunda hiç sayaç yok.

## Yayın Etkisi

- **`make kur` gerekmiyor:** kabuk betiği, ayar şeması, tema ve terminfo
  değişmiyor.
- **`make duman` etkilenmiyor:** reçete `/bin/sh` koşuyor, OSC 133 yok, blok
  yok, sayaç yok. Jeton satırına dokunulmuyor.
- **Bellek:** blok defteri 80 KB → 120 KB (varsayılan 10 000 scrollback,
  sekme başına). `BlockLog`'un doc'unda yazılı.
- **Göç yok:** set indiğinde açık olan pencerelerin daha önce koşmuş
  komutları süresiz kalır — defterde yok, uydurulmuyor.
- shader, yeni bağımlılık: yok.

## Checklist

- [x] `Outcome::Finished` süreyi taşıyor; defterin bayt bütçesi doc'ta güncel
- [x] `running_since` tek alan; `C` dikiyor, `D` tüketiyor
- [x] Biçim fonksiyonu + `COUNTER_FLOOR`
- [x] `anchors` son **dolu** sütunu taşıyor (mürekkep, zemin, kural)
- [x] `resolve_blocks` sayacı sağa yaslayarak basıyor
- [x] Test: bir saniyenin altındaki komut sayaç doğurmuyor
- [x] Test: eşiği geçen komutun süresi doğru sütunda ve `dim` renginde
- [x] Test: uzun komut metninde sayaç düşüyor, metin bozulmuyor
- [x] Test: biçimin dört kademesi (`1.4s`, `12s`, `1m 05s`, `1h 02m`)
- [x] Doğrulama geçti (`make hepsi`)
- [x] Yayın etkisi yazıldı
