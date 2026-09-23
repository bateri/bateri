# Phase 1 — `bt-core`: kesirli kaydırma konumu

## Özet

`Session` kaydırmanın kesrini tutar ve delta çağrısıyla oynatır; tam satır
`scroll_locked`'ta kalır, `frame()` kesri ve tepe satırını sınırdan verir.
Kimse henüz kesirli delta göndermiyor, yani ekranda değişen bir şey yok.

_Requirements: R1.1, R1.2, R1.3, R1.4, R1.5_

## Değişiklikler

- **`crates/bt-core/src/session.rs`**
  - `Session`'a kesir (`[0, 1)`, atomik; kilit rejimi `fill_shown` emsali ya
    da `Term` kilidi altında — hangisi seçilirse doc'unda gerekçesi),
    kaydırma nesli ve süzülme isteğinin birikimi (işaretli satır, kare yolu
    alıp sıfırlar).
  - Delta gövdesi (adı uygulamada, `Term` kilidi altında tek yer): kesre
    ekler, taşan tam kısmı `scroll_locked`'a indirir (bant eşlemesi
    dokunulmadan), uçta kesri sıfırlar. **İki girişi var ve uyandırma
    yalnız birinde:** olay yolu (tekerleğin doğrudan kolu ve yerleşme) ofset
    ya da kesir değiştiyse kare ister (`wake_if_moved`'ın genişlemesi),
    değişmediyse istemez; kare yolunun süzülme payı ise **`frame()`'in
    argümanı** olarak gelir, aynı kilit turunda uygulanır ve **uyandırmaz** —
    animasyonun kare talebi `Waker::wake`'ten geçemez (`CLAUDE.md` → Boşta
    sıfır kare). Kesir sıfırken argüman `0` ve kare bugünküyle aynı.
  - Yerleşme niyeti (`bt-shell` yalnız niyeti gönderir, kesri bilmez): payı
    `round(kesir) − kesir` kilit altında hesaplanıp süzülme isteği olarak
    birikir; momentum başı niyeti birikmiş isteği düşürür ve nesli artırır —
    uçuştaki yerleşme de biter (kalan pay düşer; göreli model, sıçrama yok).
  - `scroll_wheel` imzası kesirli ve tam satır miktarı ile niyeti alır (tip
    `bt-core`'da, AppKit'siz). Rota bugünkü gibi önce; `Arrows`/`Report` tam
    satırla, `Scroll` niyete göre: tam satır (`off`), doğrudan delta ya da
    süzülme isteği biriktir + kare iste. Dönen `Wheel`'ın anlamı ve
    `bt-shell`'in artık sıfırlama kuralı kollara göre doc'ta.
  - `send_input`'un dibe dönüşü ve `scroll_page` kesri sıfırlar, nesli
    artırır ve birikmiş süzülme isteğini düşürür. Geçmişteyken gelen çıktı
    ve resize kesre dokunmaz.
  - `Cursor`'a kesir ve nesil. `frame()`: kesir `> 0` ise ızgaranın üstündeki
    satır (`offset == 0` iken bandın üstü) doldurma kanalından, bandın en
    üst (fill-yerel `0.`) satırı olarak; varlığı `Cursor::fill`'den **ayrı**
    bir alanda, yani `fill` bugünkü anlamında kalır ve çizen taraf kanalın
    boyunu ikisinin toplamı diye okur. Kapısı yalnız satırın
    defterde olması; `fill_rows`/`slide_fill_rows`'un dock, Ctrl-L ve
    `display_offset` kapıları uygulanmaz. `fill_shown`'a yazılmaz; bant
    döngüsünün "ofset terimi yok" doc'u yeni satır için güncellenir.
- **`crates/bt-shell/src/view.rs`** — yalnız yeni imzaya uyum: bugünkü tam
  satır miktarı ve `off` niyeti; davranış aynı.
- **`CLAUDE.md`** — `frame()` sınırının ve doldurmanın cümlelerine tepe
  satırı ve kesir (aynı commit'te, sözleşme kuralı).

## Kabul

- `every_notch_moves_the_screen_by_one_row_at_most` değişmeden yeşil.
- Yeni sınamalar: kesirli deltaların toplamı tam satırları bantlı ve
  bantsız pencerede sürekli üretir (ekran her adımda en çok bir satır
  kayar); dipte negatif ve tepede pozitif kesir kalmaz ve kare istenmez;
  geçmişteyken gelen çıktı kesri ve ofseti geri almaz; girdi ve Shift+PgUp
  kesri sıfırlayıp nesli artırır; tepe satırı `Line(-offset-1)` (bandlı dipte
  bandın üstü), Ctrl-L bayrağı kuruluyken de; kesir sıfırken tepe satırı
  yok ve kare bugünküyle aynı.
- `make hepsi` ve `make test-yaris` yeşil.

## Checklist

- [x] Kesir, nesil, süzülme isteği ve delta çağrısı
- [x] `scroll_wheel`'ın yeni imzası; `view.rs` uyumu
- [x] Dibe dönüş ve Shift+PgUp kesri sıfırlıyor, nesil artıyor
- [x] `Cursor` alanları ve `frame()`'in tepe satırı (ayrı sayı, ayrı kapı)
- [x] Test: süreklilik, uçlar, dış yazıcılar, tepe satırı, kesirsiz kare aynı
- [x] `CLAUDE.md` cümleleri
- [x] Doğrulama geçti (`make hepsi`, `make test-yaris`; ayrıca `make duman`)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi

## Uygulama Notları

- **Nesil payın da içinde** (`ScrollGlide { rows, generation }`): `frame()`'in
  argümanı çıplak bir sayı değil, `take_scroll_glide` isteği aldığı neslle
  veriyor ve `frame()` güncel nesilden farklı payı düşürüyor (nesil `Term`
  kilidi altında okunuyor, artıranlar da kilidi tutuyor). Yalnız
  `Cursor::scroll_generation` yetmezdi: dibe dönüşten sonraki ilk karenin payı
  hâlâ eski nesle ait ve uygulanırsa pencere dipten bir kesir kadar yukarıda
  kalıyordu. İstek ile nesil **tek `AtomicU64`**'te (`fetch_update`): iki ayrı
  atomikte "isteği düşür + nesli artır" ile "isteği al + nesli oku" arasına
  düşen bir alım yeni nesle ait bir çentiği kaybederdi.
- **`ScrollIntent` `session.rs`'te** (`Wheel`/`Click`'in yanında) ve beş kollu:
  `Lines` bugünkü yol, dördü kesirli. Kesirli kolların hepsi olayın kendi
  `rows`'unu da uyguluyor (yerleşme ve jest başı olayı delta taşıyabilir).
  Momentum başı kolu **`GestureBegan`** oldu: parmağın yeniden değmesi de
  uçuştaki yerleşmeyi bitirmeli, yoksa önceki jestin kalan payı ekranı
  parmaktan uzaklaştırırdı (`/code-review`). `Lines` konumu dışarıdan
  sıfırlayan bir yol sayılıyor: kesir düşüyor **ve nesil artıyor**
  (`/code-review`: `off`'a geçerken uçuştaki pay kesri geri getiriyordu).
- **Uçta çentik isteği birikmiyor** (plan "biriktir + kare iste" diyordu):
  dipte aşağı, tepede yukarı dönen çentik hiçbir şeyi değiştirmeyen bir
  süzülme için kare isterdi — R1.2.
- **Kare yolu kesri normalleştiriyor**: tekerlek artık kaydırma koluna
  gitmiyorsa (alternatif ekran, **fare kipine geçen birincil ekran
  uygulaması** — `/code-review`) ya da tepenin üstünde defterde satır yoksa
  (`CSI 3 J`) `frame()` kesri sıfırlayıp nesli artırıyor; planın bilinen
  sınırı yalnız "sıfır sayılır" diyordu, kalıcı sıfırlamasaydı alternatif
  ekrandan dönen pencere yarım satırda kalırdı.
- **Geçerlilik ile çizim iki ayrı ölçü** (`/code-review`): kesrin geçerliliği
  olay yolunun ölçüsüyle (`visual_top(offset, fill_shown)`), tepe satırının
  **çizilmesi** kanalın bu karedeki boyuyla (`offset + fill`, kayma uzantısı
  dahil). Tek ölçü uzantının defteri doldurduğu karede geçerli bir kesri
  siliyordu.
- **Tam satıra yakın toplam tam satır** (`1e-5` satır, `/code-review`): pay
  `f32`'de geliyor ve yerleşmenin `−0.30000001`'i `floor`'da pencereyi bir
  satır aşağı, `0.99999998` kesirle bırakıyordu.
- **İstek `f64`'te toplanıp `i32` aralığına kırpılıyor** (`/code-review`):
  sonlu dev bir delta `f32`'de sonsuza taşıp isteği NaN'a çevirirdi.
- **Waive — çentik süzülürken gelen yerleşme**: yerleşme payını bugünkü
  kesirden hesaplıyor, uçuştaki çentiğin kalanını bilmiyor; çentik bitince
  pencere satırın dışında kalabilir. İkisi farklı aygıtlardan (klasik
  tekerlek + trackpad) aynı anda gelmeli; kalanı yalnız animatör biliyor ve
  kapatmanın yeri phase-2'nin `Motion`'ı değil bu çağrı — yönü güvenli
  (bir sonraki jest ya da yerleşme düzeltiyor).
- **Alan adları**: `Cursor::top_row` (`0`/`1`), `scroll_frac` (`f32`, `1`'in
  altına kırpılı), `scroll_generation`. Kesir içeride `f64` bitleri.
- **Testler imzayla birlikte yazıldı**, önce kırmızı değil: yeni API
  derlenmeden sınama koşamıyordu. Isırdıklarını mutasyonla gösterdim (uç
  kuralı, dibe dönüşün sıfırlaması ve nesil kapısı kapatılınca iki sınama
  düşüyor).
- `frame()`'in dördüncü argümanı ~100 sınama çağrısını `rustfmt`'le çok
  satıra açtı; değişiklik mekanik.
