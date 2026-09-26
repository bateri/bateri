# Phase 2 — Uzakta sıfır giriş satırı

## Özet

Uzak oturumda dock'un giriş satırı kalkıyor ve bant yalnız bağlam satırına
süzülerek iniyor; ızgara aşağı çiziliyor, tepesini doldurma bandı kapatıyor.
Üretimde tetik hâlâ yok (phase-3), davranış sınamayla doğrulanıyor.

_Requirements: R5.1, R5.2, R5.3, R5.4_

## Değişiklikler

- **`crates/bt-core/src/session.rs`**
  - `frame()`: `input_rows` uzak oturumda `0` (dock'lu pencere, alternatif
    ekran değil); karar bastırmanın okunduğu yaprak kilit turunda. `Cursor::
    input_rows`'un doc'u "her zaman `≥ 1`" demekten çıkıp sıfırın anlamını
    söylüyor (Karar 8).
  - `Session::dock`'taki `.max(1)` ve bağlı pencere aritmetiği sıfırı
    taşıyor: giriş satırı yok, dikey pencere yok, bağlam satırı 0. satırda.
  - **Uzak oturum `caret_in_dock`'un dördüncü ön koşulu** (pencerenin dock'u,
    alternatif ekran ve tazeliğin yanında), tutmadan **önce** uygulanıyor:
    `C`'den sonra Dock→Grid yönü `HANDOVER_HOLD` kadar tutuluyor ve
    `set_remote` o pencereye düşerse `input_rows = 0` ile `caret_in_dock =
    true` aynı karede doğar — caret bağlam satırına otururdu. Uzakta devralan
    bir giriş satırı yok, yani tutmanın gerekçesi (geri dönen caret) konusuz.
  - Dock'a tık (`dock_click`, `Release::Dock` yolu) ve dock seçimi giriş
    satırı yokken no-op (R5.3); ⌘A'nın dock kolu da (caret zaten ızgarada,
    yani `caret_in_dock` onu kapatıyor olmalı — sınamayla doğrula).
- **`crates/bt-core/src/dock.rs`** — `render_with` sıfır giriş satırında
  yalnız bağlam satırını basıyor ve erken dönüşlerin "gösterilen satır"
  cevabı sıfır olabiliyor; `.max(1)` bekçileri ve `audit:` gerekçeleri
  sıfıra göre yeniden yazılıyor. Prompt işareti (`sigil`) giriş satırı yokken
  `None`.
- **`crates/bt-gpu/src/link.rs`** — `band_target`: bandın fazlası
  `(band_px(input_rows) − dock_px(DOCK_ROWS)) / cell_h`, **kesirli ve
  işaretli**, tek formül (context.md → Kanıt: sıfırda bir hücre artı satır
  arası boşluk). `dock_caret_at` ve `glyph_fx.apply`'ın `input_rows`
  tüketimi sıfırda anlamlı (caret dock'ta olamıyor; efektler `Reset`).
- **`crates/bt-gpu/src/frame.rs`** — `set_dock_rows(input_rows + 1)` → tek
  satır; `dock_ground`'ın ikinci ayracı zaten sıfır yükseklikli
  (`rows < 2`), üst ayraç bandın tepesinde. `set_dock_band`'in piksel
  yuvarlaması negatif fazlada da aygıt ızgarasında.
- **`crates/bt-gpu/src/motion.rs`** — bant `Slide`'ı negatif hedefi
  taşıyor (tip zaten `f32`; yön kuralı yok). Snap kolları (geometri,
  `snap`, Hareketi Azalt) aynen.
- **Doldurma bandı** — `set_grid_top(grid_top.max(0.0).ceil())` pozitif
  tepeyi zaten uzatıyor; ek kod gerekiyorsa `Session::slide_fill_rows`'ta.
  Kasten temizlenmiş ekranda (`2J`, uzakta `clear`) şerit bugünkü kuralla
  boş kalıyor — kusur değil, 017'nin kapısı.

## Kabul

- `frame()` sınaması: uzak durum set edilmiş dock'lu oturumda
  `input_rows == 0`; alternatif ekranda ve dock'suz pencerede bugünkü değer.
- `bt-gpu` bileşim sınaması (`compose`/`band_target`, `LinkDelegate`'siz):
  sıfır giriş satırında bandın çizilen boyu `band_px(0)`, ızgaranın orijini
  o farkı kadar aşağıda, üst ayraç bandın tepesinde, ikinci ayraç sıfır
  yükseklikte; `input_rows == 1`'de kare bugünküyle bit bit aynı.
- `Motion`: 1 → 0 → 1 giriş satırında bant iki yönde süzülüyor ve
  `settled()` yerleşiyor; `snap`'te anında.
- Uzak set edilmiş karede `caret_in_dock == false` — `C`'den hemen sonra,
  tutmanın içinde de.
- Dock'a tık sıfır giriş satırında hiçbir komut göndermiyor.
- `make duman` yeşil (tetik yok, jetonlar bugünkü).

## Checklist

- [ ] `frame()`: uzakta `input_rows = 0`; doc
- [ ] `Session::dock` ve `dock::render_with`: sıfır satır, `sigil` yok
- [ ] `band_target`: kesirli işaretli fazla, tek formül
- [ ] `Frame`/`Motion`: negatif fazla, piksel yuvarlama, iki yönlü süzülme
- [ ] `caret_in_dock`: uzak oturum dördüncü ön koşul, tutmadan önce
- [ ] Fare: giriş satırı yokken dock tık/seçim no-op
- [ ] Test: yukarıdaki Kabul maddeleri
- [ ] Doğrulama geçti (`make hepsi` + `make duman`)
