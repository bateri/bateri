# Harf aralığı — Tartışma

Tek yaklaşım var: hücre genişliğini atlasın tek sayısında çarpmak
(`context.md`). Bu dosya yaklaşımın kendisini tartışmıyor. Jürinin ondan
çıkardığı kararları kaydediyor.

## Muhakeme (2026-10-03)

| Mercek | Verdict |
|---|---|
| Sadelik | TEMİZ |
| Codebase-fit | SORUNLU |
| İşletme | SORUNLU |

**Kabul edilen itirazlar → plan değişikliği:**

- **İki sütunlu karakter çarpanla tek hücreye düşüyor** (codebase-fit).
  `rules::accept` iki sütunlu adayda önce tek hücre kapısını deniyor
  (`rules.rs:635–651`). Mürekkebi yaklaşık 1.5–1.66 doğal hücre olan aday
  (CJK, `😀`), `ls ≳ 1.6`'da aralıklı tek hücreye sığıyor ve `Half::Whole`
  dönüyor. Sonuçta glyph iki sütunun ortasında değil sol sütunun ortasında
  çiziliyor.
  → İlk kapının **kol kararı** doğal ilerlemeye bakıyor. Çizim kutusu,
  ortalama ve ikinci kol aralıklı ilerlemeye bakıyor. `ls = 1`'de iki sayı
  aynı olduğu için davranış bit bit bugünkü. Bekçisi şu: `ls = 2`'de `中` ile
  `😀` hâlâ iki yuvaya bölünüyor.
- **En büyük köşede kapasite aileyi taşımıyor** (işletme; hesap, ölçüm
  değil). 144 etkin puntoda `lh = ls = 2` iken hücre yaklaşık 174×362,
  4096'lık kenarda bu 253 yuva ediyor. Ailenin istediği yaklaşık 429.
  → Kapasite sınaması köşeyi kapsıyor. Kırmızıysa `MAX_EDGE` 8192'ye çıkıyor
  (bkz. Karar). Sınama daraltılmıyor, çarpan tavanı da düşürülmüyor.
- **Konumsal beşinci `f64` sessizce yer değiştirebilir** (sadelik).
  `line_height` ile `letter_spacing` aynı tipte ve aynı aralıkta, testlerin
  hepsi `1.0, 1.0` geçiyor.
  → `bt_atlas::Spacing { line, letter }` geliyor. `Key` dört alanlı kalıyor
  ve `PartialEq` türetilmiş oluyor.
- **`zoom.rs` yalnız struct literal yüzünden değişiyor** (sadelik).
  → `FontOptions { size, ..font.clone() }`. Sonraki `[font]` anahtarları
  zoom'a hiç dokunmayacak.
- **`1.0`'da "bit bit aynı" sınaması totolojik** (sadelik).
  → Ayrı sınama yazılmıyor. Tanık `tests/raster_digest.rs`: üst commit ile
  ağaç arasındaki fark boş olmalı. Ağaç tarafında yalnız çağrı satırı
  uyarlanıyor.
- **Değişiklik listesi eksikti** (codebase-fit, işletme). Eksikler:
  `census.rs`, `raster_digest.rs`, `bt-core` sınamalarındaki `FontOptions`
  literal'leri, ayar penceresinin `Key::ALL` dizisi ile tanı sınaması,
  şablonun anahtar listesi.
  → `phase-1.md`'ye işlendi.
- **"Taban glyph'in kaydırması sıfır" cümleleri yalnız `ls = 1`'de doğru**
  (codebase-fit, işletme). Etkilenen yerler: `CLAUDE.md`, `raster.rs:72–79`,
  `lib.rs:484–488` ve `928–936`, `ensure`'un "four values"'u.
  → Bu cümleler yeni sözleşmeye çevriliyor: kaydırma `ls = 1`'de sıfır,
  açılınca glyph ortada.

**Reddedilenler:**

- "Font isteği" değerinin aile ve punto da taşıması (işletme) — kapsamı
  genişletiyor. `Spacing` yer değiştirme riskini zaten kapatıyor.

## Karar (2026-10-03, teknik karar)

- **Seçilen:** `Spacing` aralıklı genişliği taşıyor. Doğal ilerleme yalnız
  yedek kapının kol kararında kalıyor. Gerekçe: kullanıcı açtığı aralıkta CJK
  ve emojinin yerinden kaymasını beklemez.
- **Seçilen:** kapasite köşesi kırmızıysa `MAX_EDGE = 8192`.
  - Kenar slot hedefinden türediği için yalnız o köşede büyüyor, varsayılan
    punto bugünkü dokuda kalıyor.
  - Cihaz adaptörün doku tavanını istiyor (`renderer.rs:763`). Metal'de bu
    16384.
  - Bedel o köşede bellek: maske 64 MB, renk dokusu ancak ilk emojiyle
    doğuyor.
  - Gerekçe: özelliği kısmak (çarpan tavanını düşürmek) kullanıcının
    göreceği bir şey, kenarı büyütmek görmeyeceği bir şey. Boşlukta kodu
    açmak seçiliyor (`CLAUDE.md` → İş akışı).
- **Reddedilen:** sınamayı köşeden uzak tutmak. Sözleşmeyi delerdi ve
  kullanıcı Cmd+ ile o köşeye ulaşabiliyor (`zoom::MAX_SIZE`).
