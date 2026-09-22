# Phase 1 — Dock sütun biriktirir

## Özet

`dock::render` karakter indeksini sütun sanmayı bırakıyor: genişliği
biriktiriyor, geniş karakterin baş hücresine `wide` koyuyor ve spacer
sütununa zemin veriyor.

_Requirements: R1, R1.1, R1.2, R2, R2.1, R2.2, R2.3, R2.4, R3, R3.1, R3.2, R5_

## Değişiklikler

- **`Cargo.toml` (workspace) + `crates/bt-core/Cargo.toml`** —
  `unicode-width` bağımlılığı. Sürüm **kilitteki** olmalı (alacritty'nin
  kullandığı), yani `Cargo.lock` yalnız `bt-core`'un kenarını kazanıyor ve
  hiçbir sürüm oynamıyor. Oynuyorsa dur: kararın kaydı `discussion.md` →
  K1'de ve o "aynı crate, aynı sürüm" diyor.
- **`crates/bt-core/src/dock.rs`** — çizim döngüsünün birimi değişiyor.
  Bugün `col = TEXT_COL + (index - skip)`; artık sütun **birikiyor** ve
  karakter başına genişlik `unicode-width`'ten geliyor. Dört tüketici ayrı
  ayrı ele alınıyor:
  - **Hücrenin sütunu** biriken toplamdan.
  - **`style_at(state, index)` değişmiyor** ve bu şart: `region_highlight`'ın
    aralıkları ZLE'nin kendi birimi, yani karakter indeksi. Sütuna çevirmek
    aralıkları yanlışlardı.
  - **`skip`** sütun penceresine çevriliyor. Bugün `(cursor + 1) - available`
    ve karakter sayıyor; caret'in sütunu artık indeksinden büyük olabildiği
    için pencere de sütun cinsinden hesaplanmak zorunda.
  - **Caret'in sütunu** imleçten önceki karakterlerin genişlik toplamı.
  Kenar kuralı: pencerenin iki yakasında da sığmayan geniş karakter **hiç
  çizilmiyor** ve o sütun boş kalıyor — yarım glyph sessiz bir bozulma,
  boşluk görünür bir eksiklik (`discussion.md` → K2).
  Baş hücre `wide: true` taşıyor ve **spacer sütununa bir zemin hücresi**
  düşüyor: glyph'i yok, yalnız arka planı ve kuralları var. Izgaranın
  `WIDE_CHAR_SPACER` kolunun aynısı ve gerekçesi `frame()`'de yazılı
  ("hücreyi tümden elemek onun sağ yarısını renksiz bırakırdı"). Bu olmadan
  `region_highlight`'ın sarı zemini emojinin sağ yarısında biterdi.
  **Bağlam satırı (`DOCK_CONTEXT_ROW`) bu yolun dışında**: küçük sınıf,
  sütun adımı küçük yüzün ilerlemesi ve geniş yol orada kapalı kalıyor
  (021'in emsali, gerekçe ölçü ayrışması).

## Kabul

- Dock'un giriş satırında `🎉` **iki hücre** ve `Cell::wide` taşıyor; spacer
  sütununda glyph yok ama zemin var.
- Caret CJK'lı bir satırda doğru sütunda: `漢字` yazıp sola gitmek caret'i
  glyph'in başına koyuyor, bir sütun kaymıyor.
- `region_highlight` geniş karakteri **iki hücre** boyuyor.
- Pencere kenarında yarım glyph yok: `skip` bir geniş karakterin ortasına
  düşerse o karakter çizilmiyor.
- 023'ün `the_dock_never_marks_a_cell_wide` bekçisi **tersine çevrildi**:
  kutu silinmedi, iddiası değişti ve dock artık geniş hücreyi işaretliyor.
- `make hepsi` yeşil, `make duman` yeşil.
- `Cargo.lock` yalnız `bt-core`'un `unicode-width` kenarını kazandı.

## Checklist

- [ ] `unicode-width` workspace + `bt-core` bağımlılığı; `Cargo.lock` farkı
      tek kenar
- [ ] Sütun biriktirme; `style_at` indeksle kalıyor
- [ ] `skip` sütun penceresi
- [ ] Caret'in sütunu genişlik toplamından
- [ ] Kenarda yarılanma yok (iki yaka)
- [ ] Baş hücre `wide`, spacer sütununa zemin hücresi
- [ ] Bağlam satırı geniş yolun dışında
- [ ] Test: `🎉` iki hücre + spacer zemini
- [ ] Test: CJK'lı satırda caret'in sütunu
- [ ] Test: vurgu iki hücreye yayılıyor
- [ ] Test: kenarda sığmayan geniş karakter çizilmiyor
- [ ] 023'ün dock bekçisi tersine çevrildi
- [ ] Doğrulama geçti (`make hepsi`, `make duman`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
