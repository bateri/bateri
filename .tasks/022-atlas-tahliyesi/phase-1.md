# Phase 1 — Kenarı yuva hedefinden türet

## Özet

Atlas dokusunun kenarı sabit 1024 olmaktan çıkıp hedeflenen yuva sayısından
türüyor; varsayılan punto bugünkü hâlinde kalıyor.

_Requirements: R1, R1.1, R1.2, R1.3, R2, R3, R3.1, R4, R5, R6, R6.1_

## Değişiklikler

- **`crates/bt-atlas/src/lib.rs`**
  - `TEXTURE_EDGE` üçe ayrılıyor: `SLOT_TARGET`, `MIN_EDGE`, `MAX_EDGE`.
    `SLOT_TARGET`'in doc'u **türetmeyi** yazar: yordamsal aile + tofu = 422
    yuva (ölçüm `docs/OLCUMLER.md` → Atlas yuva ayak izi), kural "ailenin
    payı atlasın yarısını geçmesin" → 2 × 422 = 844, yukarı yuvarlanmış
    **1024**. Bu bir **tasarım sabiti**, ölçülmüş sayı değil — emsali
    `GUTTER_PT` ve `CONTEXT_SCALE`. Sayının düşük riskli olduğu da yazılır:
    `(450, 1624]` aralığındaki her değer 13/28/29pt'de aynı davranışı verir.
  - `MIN_EDGE = 1024`'ün doc'u: taban bugünkü davranışı **koruma sözü**;
    düşürülürse varsayılan puntonun rasteri değişir.
  - `MAX_EDGE = 4096`'nın doc'u: tavan olmadan `MAX_POINT_SIZE` ×
    `MAX_LINE_HEIGHT` köşesinde aritmetik sınırsız büyür.
  - `Atlas::new` (`grid`'in kurulduğu satır): kenar tabandan başlar, kapasite
    hedefin altında kaldıkça **ve** tavana varmadıkça ikiye katlanır; `grid`
    o kenardan türer. Kapasite hesabı `u32`'de yapılır (kenar/hücre
    bölümlerinin çarpımı `u16`'yı aşabilir), `grid` `u16` kalır.
  - **`u16` kırpması yolun dışında ve gerekçesi yazılır** (R4): büyüme yalnız
    `kapasite < SLOT_TARGET` iken tetikleniyor, katlama kapasiteyi dörtle
    çarpıyor, yani büyümenin ürettiği kapasite her zaman `4 × SLOT_TARGET`in
    altında — `capacity()`'nin `u16::MAX` kırpmasına (`lib.rs`) giden yol yok.
  - `negative_cache_cap`'in doc'u: "kapasitenin iki katı" artık **türetilmiş**
    kapasitenin iki katı; hangi kapasite olduğu açıkça yazılır.
  - Dört `00X` yorumu (`TEXTURE_EDGE`'in doc'u, `slot()`'un `next >= cap`
    kolu, negatif önbellek yorumu, dolu-atlas sınaması) bu kararla hizalanır:
    "LRU 00X'in işi" yerine kapasitenin hücre ölçüsünden türediği ve
    kare-başına geri dönüşümün **ölçülmüş bir ihtiyaç beklediği**.
  - Üç sınama iddiası (`tw <= TEXTURE_EDGE` kalıbı, iki ayrı sınamada)
    türetilmiş kenara göre yeniden yazılır.

- **`CLAUDE.md`** — `bt-atlas` satırına kapasitenin hücre ölçüsünden türediği
  cümlesi.

- **`docs/YOL-HARITASI.md`** — "Atlas dolunca geri dönüşü yok" borcu
  **daralır**: kapasite artık türetiliyor, kalan senaryo "tek karede hedeften
  fazla farklı glyph" ve **ölçülmedi**; çaresi de LRU değil, `encode_pass`
  sınırında geri dönüşüm.

## Kabul

- 13pt@2x'te `occupancy().1 == 1984` ve `texture_px()` bugünkü değerinde —
  **varsayılan yol bit bit aynı** (R2).
- 29pt@2x'te kapasite ailenin (422) üstünde; bugünkü 406 kalkmış (R1).
- Kabul edilen punto × ölçek × `line_height` kombinasyonlarında kapasite
  ≥ 422 (R3). Köşe (`MAX_POINT_SIZE` × `MAX_LINE_HEIGHT`) **gerçekten
  hesaplanır**; değişmez orada sağlanmıyorsa tavan yükselir ya da değişmez
  yazılı bir istisna alır — sayı doğrulanmadan kabul edilmez (R3.1).
- `crates/bt-gpu` diff'i **boş** (R5).
- `make hepsi` yeşil.

## Yayın Etkisi

- **shader** — yok (`.metal` değişmiyor).
- **ayar şeması** — yok (yeni anahtar yok; `font_size` ve `line_height`'ın
  kabul aralıkları değişmiyor).
- **`CLAUDE.md` / crate başlığı** — `bt-atlas` satırı güncellenir (R6).
- **app bundle / terminfo / tema / shell** — yok.
- **ölçüm bekliyor: sekme başına bellek.** Doku büyük puntoda 1 MB'dan
  4 MB'a çıkıyor. `docs/OLCUMLER.md` → `## Bellek` bugün **boş** ve kancası
  yok (`footprint`/`vmmap` dışarıdan), yani bu bir blokaj değil kayıtlı bir
  kalem. Varsayılan puntoda fark **sıfır bayt**.

## Checklist

- [ ] `SLOT_TARGET` / `MIN_EDGE` / `MAX_EDGE` ayrımı ve türetme doc'ları
- [ ] `Atlas::new`'de kenarın türetilmesi
- [ ] `u16` gerekçesi ve `negative_cache_cap` doc'u
- [ ] Dört `00X` yorumu + `CLAUDE.md` + yol haritası borcu hizalandı
- [ ] Test: varsayılan yol değişmedi (13pt@2x → 1984, `texture_px` sabit)
- [ ] Test: değişmez — kabul edilen her punto × ölçek × `line_height` için
      kapasite ≥ 422, köşe dahil ve köşenin sayısı **hesaplanmış**
- [ ] Test: üç eski `TEXTURE_EDGE` iddiası türetilmiş kenara göre yeniden
      yazıldı
- [ ] `crates/bt-gpu` diff'i boş
- [ ] Doğrulama geçti (`make hepsi`)
- [ ] Yayın etkisi yazıldı
