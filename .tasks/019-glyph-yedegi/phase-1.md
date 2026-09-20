# Phase 1 — Yedek arama, genişlik kapısı ve sınamalar

## Özet

Eksik glyph sistemin cascade'inden aranıyor, hücreye sığmayan aday
reddediliyor, glyph hücrede ortalanıyor ve varsayımı kırılan dört sınama
yeniden kuruluyor.

_Requirements: R1, R2, R2.1, R3, R4, R5, R6_

## Neden tek phase

"Arama var, süzgeç yok" ara durumu `make hepsi`'yi **yeşil bırakamaz**:
kapı olmadan `TOFU` hiç üretilemiyor (`CTFontCreateForString`'in başarısızlık
kipi yok, `.LastResort` her koda cevap veriyor), yani
`tofu_box_is_drawn_and_resident`, `unknown_char_is_cached` ve
`negative_cache_is_capped_and_evicted` kırılmakla kalmaz — **adını taşıdıkları
şeyi sınayamaz** hâle gelir. Üçü tek commit.

## Değişiklikler

- **`crates/bt-atlas/src/font.rs`** — yeni `fallback_font(base, ch,
  cell_px) -> Option<CFRetained<CTFont>>`. `CTFont::for_string` (objc2-core-text
  0.3.2'de var, `Cargo.lock` oynamıyor), `glyph_index`, sonra `advance <=
  cell_px` kapısı. `None` = "kabul edilmedi". **Log yok** — fonksiyon
  `slot()`'un çizim yolunda ve glyph başına çıktı `Faces::derive`'ın yazılı
  kuralını deler.
- **`crates/bt-atlas/src/lib.rs`** — `Atlas::slot`'un `NoGlyph` yaprağına tek
  kol, yüz merdiveninin **altına** ve negatif önbellek kolunun **üstüne**.
  Taban font `SizeClass`'tan seçiliyor (`Normal → faces.get(Regular)`,
  `Small → &self.small`), sınır o sınıfın hücre genişliği. Kabul edilen aday
  bugünkü `Drawn` kolunun aynısından geçiyor: **aynı anahtar**, aynı `Upload`.
  Başlık yorumundaki "yedek font listesi … hâlâ kapsam dışı" cümlesi aynı
  commit'te düşüyor.
- **`crates/bt-atlas/src/raster.rs`** — glyph `x = (cell_px - advance) / 2`'ye
  çiziliyor. Kural **evrensel**, yedeğe koşullu değil: taban monospace fontta
  `advance == cell_px` olduğu için kaydırma sıfır ve çıktı bit bit aynı.
- **`CLAUDE.md`** — `bt-atlas` satırı ve "Emoji, geniş glyph ve kutu çizim
  henüz yok" cümlesi; ikincisi artık tek-hücre yedeği anmak zorunda.

## Ölçülmüş sayılar (uydurulmayacak)

Menlo 13pt, bu makine (macOS 26.4.1); hücre **7.827 px**, küçük sınıf
(CONTEXT_SCALE 0.8 → 10.4pt) **6.261 px**.

| Karakter | Yedek | advance | oran | Karar |
|---|---|---|---|---|
| `⏵` U+23F5 | STIX Two Math | 6.539 | 0.84× | KABUL |
| `✓`/`⚠`/`▶`, U+F8FF | Menlo / Monaco | ≤ 7.83 | ≤ 1.00× | KABUL |
| `𝔸` U+1D538 | STIX Two Math | 8.41 | 1.07× | RED |
| `漢` U+6F22 | PingFang SC | 13.00 | 1.66× | RED |
| U+E0B0, U+10FFFD | `.LastResort` | 14.30 | 1.83× | RED |
| `🎉` U+1F389 | Apple Color Emoji | 17.00 | 2.17× | RED |

**Oran ölçekten bağımsız:** `⏵` küçük sınıfta da 0.84× (5.231 / 6.261), yani
iki sınıfta da kabul. `⏵`'nin mürekkep kutusu `origin.x = 1.04 > 0`, yani
**negatif sol yatak yok** ve ortalama kaydırması (0.644 px) kırpma üretmiyor.

## Kabul

- `⏵` `TOFU` **değil** ve bitmap hücreye sığıyor, mürekkebi ortalı.
- `漢`, emoji ve `.LastResort` kod noktası `TOFU`; `occupancy().0 == 1`
  (yuva harcanmıyor).
- Taban fontun çıktısı **değişmiyor**: mevcut piksel sınamaları dokunulmadan
  geçiyor.
- `make hepsi` yeşil.

## Yayın Etkisi

- **`CLAUDE.md`:** `bt-atlas` satırı + "Emoji, geniş glyph ve kutu çizim henüz
  yok" cümlesi (aynı commit, R6).
- **Ölçüm bekliyor:** yedek fontun **soğuk ilk açılışı** ölçüldü ve dipnot
  değil — taze süreçte `漢` için PingFang SC'yi açmak **5.9–7.6 ms** (üç koşu,
  ctypes ek yüküyle). 60 fps'de kare bütçesi 16.7 ms, yani bir CJK dosyasının
  ilk karesinde düşen bir kare **beklenen** davranış. Ana thread'de, font
  ailesi başına bir kez. Gerçek pencerede kare süresine etkisi `/measure`
  ile gösterilmeli.

## Checklist

- [ ] `font::fallback_font` (for_string + glyph_index + advance kapısı, logsuz)
- [ ] `Atlas::slot`'un `NoGlyph` yaprağına kol; merdivenin altına, negatif
      önbelleğin üstüne; taban `SizeClass`'tan
- [ ] `raster::draw` ortalama (evrensel, taban fontta no-op)
- [ ] `lib.rs` başlık yorumu + `CLAUDE.md` aynı commit'te
- [ ] Test: `⏵` `TOFU` değil, iki boy sınıfında da
- [ ] Test: `⏵` bitmap hücreye sığıyor ve ortalı (bitmap düzeyinde — `slot !=
      TOFU` kırpmayı göremez)
- [ ] Test: emoji → `TOFU`, `.LastResort` kod noktası → `TOFU`
- [ ] Test: taban fontun çıktısı bit bit aynı (ortalamanın no-op olduğu)
- [ ] Yeniden kurulan dört sınama: `tofu_box_is_drawn_and_resident`,
      `unknown_char_is_cached`, `negative_cache_is_capped_and_evicted`,
      `non_bmp_char_path_works` — `UNKNOWN_CHAR` **`'漢'` kalıyor**
- [ ] `negative_cache_is_capped_and_evicted`'ın reddedilen karakter havuzu:
      CJK zaten reddediliyor, havuz aynen çalışıyor mu — koşup **ölç**
- [ ] Doğrulama geçti (`make hepsi`)
- [ ] Yayın etkisi yazıldı
