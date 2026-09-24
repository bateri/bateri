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

- [x] PTY payı ile çizilen bandın ayrılması, dipten yerleşim
- [x] Bandın `Slide`'ı, `settled()`, snap kolları, yön biti
- [x] `set_origin`'de birleştirme; caret hedefi
- [x] `Drawn` dock geometrisi; `window_point_dock`
- [x] `Cursor`'un giriş satırı sayısı ve `frame()`'in bütçe argümanı (hep 1)
- [x] Test: `n = 3` bileşim bekçisi
- [x] Doğrulama geçti (`make hepsi`, `make duman`, `make test-yaris` — kare yolu değişti)
- [x] Riskli phase: `/code-review` koştu, iki bulgu giderildi

## Uygulama Notları

- **Test-first sırası tutmadı:** bileşim bekçisi kodla aynı turda yazıldı;
  yerine mutasyonla doğrulandı — `Frame::origin_px`'ten bandın fazlası
  çıkarılınca bekçi ilk animasyon karesinde düşüyor, makas kaldırılınca
  offscreen bekçi (`a_growing_band_reveals_its_rows_from_the_bottom`) düşüyor.
- **İki viewport + makas:** hücreler push anında piştiği için dibe yaslılık
  encode anında iki orijinle kuruldu — zemin/saç çizgileri bandın o anki
  boyundan (`yükseklik − bant`), hücreler/caret/efektler yerleşimden
  (`yükseklik − yerleşim`). Büyüyen bantta yerleşim bandın tepesini aşıyor ve
  taşan satırı `setScissorRect` kesiyor; makas yalnız o karelerde, çünkü
  dinlenen bantta efektlerin saç çizgisini aşan payını keserdi.
- **Bandın fazlası tek yuvarlama:** `Frame::set_dock_band` fazlayı bir kez
  piksele yuvarlıyor, bandın boyu (`PTY payı + fazla`) ve ızgaranın orijini
  (`… − fazla`) aynı sayıyı okuyor; iki ayrı yuvarlama bir piksel
  ayrışabilirdi. Birleştirme `link::compose`'ta (iki kare yolunun ortak
  noktası, `LinkDelegate`'siz sınanabilsin diye serbest fonksiyon).
- **Yön biti çağırandan gelmiyor:** "bandın hedefi bu karede değişti"yi
  `Motion::sync` kendisi hesaplıyor (bandın geçmişini bilen tek yer), `sync`
  yalnız bandın hedefini (`u16`) ek argüman olarak alıyor.
- **Bant snap'leri:** geometri ve öteleme kipinin snap'i (`snap`, Hareketi
  Azalt); tekerlek bandı snap'lemiyor — kaydırma dock'un satırını
  değiştirmiyor.
- **`Frame`'in satır sayısı toplam satır** (giriş + bağlam,
  `set_dock_rows`, hücrelerden önce); bağlam satırı "iki ve fazla satırda
  son satır". Tek satırlık sınama dock'ları (`renderer.rs`) bu yüzden
  geometrisi değişmeden kaldı: `open_dock(1, …)` → `set_dock_rows(1)` +
  `open_dock(…)`. `dock_height` artık tek satır arası boşluk veriyor; iki
  satırda eski formülle aynı sayı.
- **`bt-core`'un bağlam satırı `input_rows`. satır** (`render_with`'e
  `input_rows`), sabit `1` değil; `Session::dock` sayıyı argüman alıyor.
- **Bütçe bu phase'de `rows: 1`** (`link.rs`): tavan oranı (`DOCK_MAX_SHARE`)
  sarmayla gelsin diye phase-3'ün checklist'ine yazıldı.
- **`/code-review` bulguları:** (1) `compose` bandı yalnız bu karenin dock
  yüzeyi açıkken yazıyor — vim'den çıkışta payı dönmüş ama yüzeysiz bir
  karede hareket karesi caret'i çizilmeyen dock yuvasına atabilirdi;
  (2) büyüyen bantın makası dock caret'ini kesmiyor (yarım blok yerine bir
  kare boyunca bandın üstünde tam blok).
- **Duman:** ilk koşular HEAD'de de aynı "animasyon yerleşmedi" ile kırmızıydı
  (ortam — pencere görünmüyordu); gerçek pencere açıkken yeşil. Gözle kontrol
  (geçici paket, açık tema): tek satır, `for` döngüsü ve `PS2` satırları
  032 öncesiyle aynı görünüyor.
- `Frame::set_dock_top` yalnız sınamalarda kaldı (`#[cfg(test)]`); üretimde
  tepe bandın boyuyla aynı çağrıda yazılıyor.
