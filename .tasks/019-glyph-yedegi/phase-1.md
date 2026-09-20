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

İkinci satırın ilk üçü uygulamada yeniden ölçüldü ve **yedek yoluna hiç
girmiyor**: üçü de Menlo'da var, satırın gerçek yedeği yalnız U+F8FF
(bkz. `## Uygulama Notları`).
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

## Uygulama Notları

- **Kapının sınırı `u16` değil, kesirli.** İmza `cell_px: u16` olarak
  planlanmıştı; `cell_advance: CGFloat` oldu. Sebep R5'i yanlışlamasıydı:
  yuvarlanmış hücre 8 px, taban fontun ilerlemesi 7.82666 px, yani
  `(8 - 7.82666) / 2 = 0.0866` px — "taban fontta kaydırma sıfır" iddiası
  yuvarlanmışla **yanlış** ve her glyph'in kenar yumuşatması sessizce
  değişirdi. Phase'in kendi 0.644 px'i de ancak kesirli hücreyle çıkıyor
  (`(7.827 - 6.539) / 2`). Aynı sayı hem kapıyı hem ortalamayı besliyor, yani
  iki iş tek ölçüye bağlı.
- **Kol `match result`'ın içinde değil, çizim adımının içinde** (`Sprite::Char`
  kolunda, guard `face == Face::Regular`). İki mecburi sebep: `match`'in bir
  kolu negatif önbellek koluna **düşemiyor** (yedek reddedilirse oraya inmek
  gerekiyor) ve `ch` yalnız orada kapsamda. Semantik sıra değişmedi — yüz
  merdiveni düz yüzle özyinelemeye giriyor, yedek ancak orada koşuyor, ret
  negatif önbelleğe düşüyor.
- **`context_cell_w` artık `context_advance`'ten türüyor**
  (`font::round_up(space_advance(&small))`), eskiden `font::metrics(&small,
  line_height).cell_px.0` idi. İkisi bit bit aynı sayı; tek kaynağa indirmek
  zorunluydu, yoksa kapı bir hücreye ortalama başka bir hücreye bakabilirdi.
  Yan etki: yanıltıcı "`line_height` küçük yüze de uygulanıyor" yorumu düştü —
  satır aralığı genişliğe hiç dokunmuyor.
- **`pub(crate)` olanlar:** `font::space_advance`, yeni `font::glyph_advance`,
  `font::round_up`.
- **Ölçümün düzelttiği bir satır:** tablodaki `✓`/`⚠`/`▶` yedek yoluna **hiç
  girmiyor** — üçü de Menlo'da var (`base_has = true`, ölçüldü). O satırın
  gerçek yedeği yalnız U+F8FF (Monaco, 0.9968×). Tablonun kalan altı satırı
  aynen doğrulandı: `⏵` 0.8355× (küçük sınıfta da aynı oran), `𝔸` 1.0747×,
  `漢` 1.6610×, U+E0B0 ve U+10FFFD 1.8273×, `🎉` 2.1721×. Hücre 7.82666 px,
  küçük sınıf 6.26133 px.
- **`⏵` dört kenarda da sığıyor** (fontun sınır dikdörtgeninden, iki sınıfta):
  normal'de mürekkep sütun 1–6 / satır 4–11, küçükte sütun 1–5 / satır 7–12.
  Kırpma yok.
- **144pt havuzu daraltılmadı:** 396 CJK karakteri **889 µs** (aynı süreçte
  PingFang SC açıkken). Oran ölçekten bağımsız olduğu için 144pt'de de
  reddediliyor (hücre 86.70 px, aday 144 px).
- **Soğuk açılış ölçüldü** (taze süreç): `⏵` için ilk çağrı **6.26 ms**,
  ardından gelen başka aileler 0.02–0.85 ms, önbellek isabeti < 1.2 µs. Atlas
  kurulumunun kendisi 11.7 s ve bu **bu setten önce de öyle** — değişiklik
  öncesi tek bir sınama 12.67 s sürüyordu, yani CoreText'in süreç başına ilk
  font taraması, yedeğin değil.
- **Beş mutasyon koşturuldu, beşini de adlı bir bekçi düşürdü:** kapı
  kaldırıldı (5 sınama), ortalama yuvarlanmış hücreye bağlandı
  (`every_base_glyph_advance_is_the_cell_advance`), kol kapatıldı
  (`fallback_glyph_is_drawn_in_both_size_classes`), yedeğin tabanı büyük
  sınıfa çivilendi (aynı sınama), yalnız sınırı büyük sınıfa çivilendi
  (`wide_fallback_candidates_are_rejected`). İlk turda son ikisi **geçiyordu**;
  "küçük sınıfın izi daha dar" ölçütü onun için eklendi.
- **"Ortalı" iddiası daraltıldı.** Ortalanan şey glyph'in **ilerleme kutusu**,
  mürekkebi değil, ve `⏵`'nin yan yatakları asimetrik (sol 1.04, sağ 0.13):
  "mürekkebin merkezi hücrenin merkezine yaklaştı" ölçütü doğru uygulamayı
  kırmızıya düşürürdü. Sınanan iki şey — kaydırma yedek yolunda uygulanıyor
  (bitmap kaydırmasız hâlinden farklı) ve taban yolunda tam olarak sıfır.

## Checklist

- [x] `font::fallback_font` (for_string + glyph_index + advance kapısı, logsuz)
- [x] `Atlas::slot`'un `NoGlyph` yaprağına kol; merdivenin altına, negatif
      önbelleğin üstüne; taban `SizeClass`'tan
- [x] `raster::draw` ortalama (evrensel, taban fontta no-op)
- [x] `lib.rs` başlık yorumu + `CLAUDE.md` aynı commit'te
- [x] Test: `⏵` `TOFU` değil, iki boy sınıfında da
      (`fallback_glyph_is_drawn_in_both_size_classes`)
- [x] Test: `⏵` bitmap hücreye sığıyor ve ortalı (bitmap düzeyinde — `slot !=
      TOFU` kırpmayı göremez) — `fallback_glyph_fits_the_cell`; fit fontun
      sınır dikdörtgeninden (bitmap de kırpmayı göremez, CG sessizce kesiyor)
- [x] Test: emoji → `TOFU`, `.LastResort` kod noktası → `TOFU`
      (`wide_fallback_candidates_are_rejected`, dördü birden)
- [x] Test: taban fontun çıktısı bit bit aynı (ortalamanın no-op olduğu)
      (`every_base_glyph_advance_is_the_cell_advance`, iki yarımlı)
- [x] Yeniden kurulan dört sınama: `tofu_box_is_drawn_and_resident`,
      `unknown_char_is_cached`, `negative_cache_is_capped_and_evicted`,
      `non_bmp_char_path_works` — `UNKNOWN_CHAR` **`'漢'` kalıyor**
- [x] `negative_cache_is_capped_and_evicted`'ın reddedilen karakter havuzu:
      CJK zaten reddediliyor, havuz aynen çalışıyor mu — koşup **ölç** (889 µs)
- [x] Ek bekçi: `the_cell_is_the_rounded_advance` (kesirli/yuvarlanmış iki
      temsil ayrışmasın)
- [x] Doğrulama geçti (`make hepsi`)
- [x] `make duman` (çizim yolu değişti): `kare=30 hucre=8 glif=6 kural=15
      icerik=3 hareket=27 sessiz=1754.22ms kapanis=clean` — beklenen jetonlar
      **aynen**, yani ortalamanın no-op olduğu gerçek pencerede de tutuyor
- [x] Yayın etkisi yazıldı
