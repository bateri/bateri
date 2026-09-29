# Phase 2 — Küçültme

## Özet

Kapının iki kolundan da dönen aday, sınırın içindeyse küçük puntolu
kopyasıyla kabul edilir ve çizilir. Sınır phase-1'in dağılımından seçilir.

_Requirements: R2.1, R3, R3.1, R3.2, R3.3, R3.4_

## Değişiklikler

- **`crates/bt-atlas/src/font.rs`**
  - Sınır için adlı bir sabit tanımlanır. Doc'u türetmeyi taşır: phase-1'in
    dağılımı, emojinin üst ucu, `.LastResort`'un oranı.
  - `accept`'e üçüncü kol eklenir. İki kapı da reddettiyse kutu hücre olur
    (tek sütunluda) ya da iki hücre (geniş karakterde). Oran sınırın
    içindeyse adayın küçük puntolu kopyası kurulur
    (`CTFontCreateCopyWithAttributes`) ve `ink_fits_box` ile **yeniden**
    sınanır. Yuvarlama payı için geçmezse kutu. Sıra doc'ta gerekçelidir:
    küçültme en son kol, yani bugün geçen aday bit bit aynı (R3.3).
  - `.LastResort` bu kolda kabul edilmez (R3.2). Ölçüt phase-1'in bulgusuna
    göre seçilir. Oran sınırın üstündeyse geometri zaten yetiyor ve ek
    ölçüt yazılmaz. Altındaysa ölçüt ve gerekçesi Uygulama Notları'na
    yazılır; `CLAUDE.md`'deki "sihirli dizge yok" cümlesi de aynı commit'te
    güncellenir.
  - Dikey yerleşim, ~0.6'lık emoji için gözle seçilir (discussion.md →
    Karar 5). Ortalamaya karar verilirse formül `centre_shift` gibi tek
    yerde durur ve kapı ile çizim onu paylaşır.
- **Renk düzlemi**: küçük kopya aynı fonttan geldiği için trait biti
  (`has_color_glyphs`) değişmez. `draw_color_glyph` yolu dokunulmadan
  kalmalı; kalmıyorsa sebebi yazılır.
- **Bekçi**: `EXPECTED_TOFU` boşalır. Sınırın dışında kalan karakter varsa
  listede kalır ve nedeni yanına yazılır.
- **Sınamalar**: küçültülen glyph'in rasteri hücrenin içinde kalır (sol ve
  sağ sütunda mürekkep yok ya da sınırda). Sınırın hemen üstündeki aday
  kutu kalır. Bugün kabul edilen bir adayın (ör. `⏺`) rasteri değişmez.
- **`CLAUDE.md`**: "kutu ya da tam glyph" paragrafı "kutu, tam glyph ya da
  sığacak kadar küçültülmüş glyph" olur. Tek sütunlu emoji için yazılmış
  "kapsam dışı" cümlesi kalkar. Sınırın adı ve işaretçi eklenir.
- **`docs/YOL-HARITASI.md`**: 041 satırı.

## Kabul

- `⧉` ızgarada, dock'ta ve doldurma bandında hücreye sığan, kırpılmamış bir
  glyph olarak çizilir.
- Tek sütunlu renkli emoji (ör. `☺`) küçük ve renkli çizilir.
- `make tarama`'da `Rejected` grubunda yalnız sınırın üstündekiler kalır.
- Bekçi yeşil, `EXPECTED_TOFU` boş ya da gerekçeli.

## Checklist

- [ ] Sınır sabiti, türetmesi doc'ta
- [ ] `accept`'in üçüncü kolu + yeniden sınama
- [ ] `.LastResort` ölçütü (gerekirse)
- [ ] Dikey yerleşim kararı, gözle
- [ ] `EXPECTED_TOFU` boşaldı
- [ ] Test: küçültülen raster hücrede, sınır üstü kutu, bugünkü aday bit bit aynı
- [ ] `CLAUDE.md` ve `docs/YOL-HARITASI.md` güncellendi
- [ ] Küçültme katsayısı `fit`'ten (phase-1 → Uygulama Notları): `ratio` ile küçültülen `⧉` (1.11) sola yapışma yüzünden yeniden sınamada yine dönüyor, `fit` 1.22
- [ ] `.LastResort` (`fit` 1.660) emojinin (1.661–1.681) altında: geometri ayıramıyor, R3.2'nin ölçütü gerekli
- [ ] @1x'te Apple Color Emoji `fit` 2.124 ve iki hücreye de sığmıyor: sınırın @1x'i kapsayıp kapsamadığı kararı
- [ ] `CLAUDE.md` → Komutlar'a `make tarama` satırı
- [ ] Doğrulama geçti (`make hepsi`)
