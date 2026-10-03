# Phase 3 — Aralığı aç

## Özet

İki ayarı `0.5..=2.0`'a açıyor; ayar penceresi, şablon ve belgeler yeni
sözleşmeyi anlatıyor.

_Requirements: R4_

## Değişiklikler

- **`crates/bt-core/src/settings.rs`**
  - `LINE_HEIGHT_RANGE` ve `LETTER_SPACING_RANGE` `0.5..=MAX`. Alt uç için
    adlı bir sabit (`MIN_SPACING` ya da iki ayrı ad).
  - `FontOptions`'ın iki doc'u: alt uç artık koruma değil taşma, gerekçe
    `.tasks/052-tasan-glyph/`.
  - Şablon metni ("Below 1 is refused…" iki satır) yeni aralığı söylüyor.
  - Okuma sınamaları: `0.5` kabul, `0.49` red.
- **`crates/bt-shell-macos/src/settings_window.rs`** — İki `Number`
  denetiminin aralığı sabitlerden. `parse_decimal("0.9", …)` artık geçerli,
  sınama `0.4`'e taşınıyor.
- **`docs/AYARLAR.md`** — `[font]` örneği, tablo (`0.5`–`2`) ve maddeler:
  - `1`'in altında satırlar ya da harfler sıkışır, harf kesilmez, komşu
    hücreye taşar.
  - Aksanlar ve kuyruklar komşu satıra değebilir.
  - Dock'ta taşma bandın içinde kalır.
  - Pencere kenarında kesilir.
  - Bilinen sınır: çok dar `letter_spacing`'te (yaklaşık `0.7` altı) iki
    sütunlu karakter (CJK, emoji) taşmak yerine küçültülerek çizilir.
  - `1.0` tam olarak fontun aralığı.
- **`CLAUDE.md`** — Sözleşme cümleleri yeni hâle çevriliyor:
  - "yuva = hücre", "dörtgen tek hücre" ve "`cell_px` uniform'u el değmiyor"
    (023 paragrafı);
  - "kutu ya da tam glyph" → "kutu, yuvaya tam sığan glyph ya da küçültülmüş
    glyph";
  - çizim sırası cümlesi;
  - `line_height`'ın alt ucu.
  - Her biri kural + tek cümle + işaretçi.

## Kabul

- `bt-core`: `0.5` ve `0.75` okunuyor, `0.49` ile `0` reddediliyor ve
  anahtar adıyla tanı bırakıyor.
- Gözle (sahne her yüzeyde):
  - `line_height = 0.7`, `letter_spacing = 0.8` kaydedilince ızgarada,
    doldurma bandında ve dock'ta satırlar ve harfler sıkışıyor.
  - `İÖÜ gjy` kesilmiyor, komşu satıra taşıyor.
  - `tree` çizgileri hücre ızgarasında bitişik.
  - `echo 中文 😀` kesilmiyor: `0.8`'de tam boy, `0.5`'te küçültülmüş.
  - Caret ve seçim hücreyi kaplıyor.
  - `1.0`'a dönünce görüntü phase-0'dan sonraki hâliyle aynı.

## Checklist

- [ ] Aralıklar ve sabitler, doc'lar, şablon
- [ ] Ayar penceresi
- [ ] `docs/AYARLAR.md`, `CLAUDE.md`
- [ ] Test: okuma sınamaları
- [ ] Doğrulama geçti: `make check`, `make linux`, `make smoke`
