# Phase 2 — Değişken yükseklikli bant (görünmez)

## Özet

Bandın çizilen satır sayısını PTY payından ayır, dock'u dipten yerleştir,
bandın ek yüksekliğini kendi animatöründe süz ve ızgaranın orijinine çizim
anında katılaştır; `bt-core` bu phase'de hep bir giriş satırı bildirir, yani
ekran bit bit aynı.

_Requirements: R2.1, R2.2, R2.3, R2.4_

## Değişiklikler

- **`crates/bt-core/src/lib.rs` / `session.rs`** — `Cursor`'a çizilecek giriş
  satırı sayısı (sınır kaydı, grid hücresi değil); `frame()` giriş bütçesini
  (tavan) ve sarma genişliğini **argüman** olarak alır — yerleşim kararı
  çizenin (`context_cols` emsali). Bu phase'de değer 1. `Session::dock` onu
  argüman olarak alır, ikinci kez türetmez (`caret_in_dock`'un emsali).
- **`crates/bt-gpu/src/frame.rs`** — `DOCK_ROWS` PTY payı olarak kalır; çizilen
  bandın yüksekliği `n` giriş satırından ayrı bir fonksiyonla (adı PTY
  payıyla karışmayacak biçimde): giriş satırları arasında boşluk yok, boşluk
  ve ikinci saç çizgisi yalnız giriş bloğu ile bağlam satırı arasında; `n = 1`
  bugünkü `dock_px(2)` ile aynı piksel. Dock hücreleri bandın **dibinden**
  sayılır (bağlam satırı dipten ilk), `DOCK_CONTEXT_ROW`'un "1. satır bağlam"
  varsayımı (`column_px`, `SizeClass` eşiği, `dock_row_divider_y`,
  `dock_ground`) buna göre. Bandın o anki yüksekliği `Frame`'de bir değer ve
  **encode anında** okunur (`fill_origin_px`'in emsali); `set_dock_top` ve
  caret'in yuva seçimi aynı değerden.
- **`crates/bt-gpu/src/motion.rs`** — bandın ek satırı için dördüncü `Slide`
  (yeni tip yok): iki yönde süzülür; `set_style`, `set_reduce`, `finish`,
  geometri snap'i ve `settled()`'e bağlanır (dışında kalsaydı link kayma
  ortasında uyurdu). `sync`'in hedefi `u16` kalır; `filled`'in kardeşi tek
  bit ("bandın hedefi bu karede değişti") o karede yükselen içerik hedefini de
  süzer.
- **`crates/bt-gpu/src/link.rs`** — `set_origin` (iki kare yolunun ortak
  noktası) çizilen orijini `motion.origin() − band` olarak kurar ve
  `set_grid_top`'a aynı değeri verir; ızgara caret'inin hedefi
  `cursor.row + origin_target − ek hedef`; `dock_caret_at` bir giriş satırı
  alır; hareket karesi de bandın yüksekliğini yazar. `Drawn` bandın çizilen
  yüksekliği ile giriş satırı sayısını `px`/`fill_rows`'la **aynı yazımda**
  taşır.
- **`crates/bt-gpu/src/renderer.rs`** — `encode_dock` viewport'u bandın o
  anki yüksekliğinden; kırpma bugünkü gibi.
- **`crates/bt-shell/src/view.rs`** — `window_point_dock` ve
  `dock_input_top_px` `Drawn`'ın dock geometrisinden, `rows: 1` yerine giriş
  satırı sayısıyla; `split_into_grid` ve PTY yolu (`app.rs`, `window.rs`)
  değişmez.

## Kabul

- `n = 1`'de bütün mevcut offscreen ve dock sınamaları değişmeden yeşil.
- Sınama kancasıyla `n = 3`: bandın yüksekliği, dipten yerleşim, bağlam satırı
  dipte, tek saç çizgisi giriş bloğunun altında; **bileşim bekçisi** —
  ızgaranın alt kenarı, doldurma bandı ve dock bandının üst kenarı aynı
  karede, animasyonun ortasında da çakışıyor.
- Motion: bandın `Slide`'ı yerleşmeden `settled()` yanlış; Hareketi Azalt ve
  `snap`'te tek karede; bant değişen karede yükselen içerik hedefi süzülüyor.
- Dolu ızgarada negatif orijinde `point_to_cell` doğru satır.
- `make hepsi` ve `make duman` (jetonlar oynamıyor, `/bin/sh` bandı hiç
  büyütmüyor) yeşil.

## Checklist

- [ ] PTY payı ile çizilen bandın ayrılması, dipten yerleşim
- [ ] Bandın `Slide`'ı, `settled()`, snap kolları, yön biti
- [ ] `set_origin`'de birleştirme; caret hedefi
- [ ] `Drawn` dock geometrisi; `window_point_dock`
- [ ] `Cursor`'un giriş satırı sayısı ve `frame()`'in bütçe argümanı (hep 1)
- [ ] Test: `n = 3` bileşim bekçisi
- [ ] Doğrulama geçti (`make hepsi`, `make duman`)
