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

- [x] Aralıklar ve sabitler, doc'lar, şablon
- [x] Ayar penceresi
- [x] `docs/AYARLAR.md`, `CLAUDE.md`
- [x] `CLAUDE.md`'ın çizim sırası cümleleri phase-2'nin sözleşmesine (phase-2'den devir): "Encode sırası ızgara → doldurma → dock" artık ızgara zemini → vurgu → caret → bant zemini → bant araması → ızgara glyph'leri → bant glyph'leri → dock; glyph/emoji dörtgeni yuva boyunda ve `slot_offset` kadar geride, glyph viewport'u taşma payı kadar kaldırılmış (dock'ta bandın tepesine kadar); kural sprite'ları hücre genişliğinde (`rules::rule_metrics`)
- [x] Test: okuma sınamaları
- [~] Doğrulama geçti: `make check`, `make linux`, `make smoke` — smoke ortamda `frames=0`, üst commit'te de aynı (Uygulama Notları)

## Uygulama Notları

- **Alt uç tek sabit**: `settings::MIN_SPACING` (iki aralık paylaşıyor),
  `bt_core`'dan dışa açık. Ayar penceresi zaten aralık sabitlerini
  okuyordu; kodu değişmedi, yalnız sınaması.
- `docs/AYARLAR.md`'nin `[font]` örneği `line_height = 0.9` gösteriyor:
  `1.0` şablonla aynı olduğu için yeni aralığı anlatmıyordu.
- `make smoke` bu ortamda `frames=0` (üst commit c090a1a'da da aynı):
  pencere drawable almıyor, ortam.
- **Set kapısının `/code-review` bulgusu (giderildi, `bt-atlas`):** ortalama
  kesirli paydan yapılıyordu, GPU ise dörtgeni tam piksel `slot_offset.x`'ten
  çiziyor. Küçük sınıfta pay kendi fontundan geldiği için bağlam satırının
  harfleri sparkline'dan ~0.9 px sola kayıyordu (13pt@2x, `0.5`), büyük
  sınıfta ortalanan yedek glyph'ler ≤ 1 px kayıyordu. `GlyphBox`'a `left`
  (= `slot_offset.x`, iki sınıfta da) girdi: ortalama ondan, kapının sınırı
  yine kesirli paydan; ilk kapı çizilen yeri ölçüyor. Taban fontun harfi
  `max(0)` ile yine `x = 0`'da, yani hücrenin kesirli merkezinin < 1 px
  sağında — tam piksel ızgaranın payı. `raster_digest` (HEAD c090a1a ile
  ağaç, 57 948 satır): fark **boş**.
