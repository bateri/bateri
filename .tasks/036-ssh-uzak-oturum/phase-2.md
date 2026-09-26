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

- [x] `frame()`: uzakta `input_rows = 0`; doc
- [x] `Session::dock` ve `dock::render_with`: sıfır satır, `sigil` yok
- [x] `band_target`: kesirli işaretli fazla, tek formül
- [x] `Frame`/`Motion`: negatif fazla, piksel yuvarlama, iki yönlü süzülme
- [x] `caret_in_dock`: uzak oturum dördüncü ön koşul, tutmadan önce
- [x] Fare: giriş satırı yokken dock tık/seçim no-op
- [x] Test: yukarıdaki Kabul maddeleri
- [x] Doğrulama geçti (`make hepsi` + `make duman`; kare yolu değiştiği için `make test-yaris` da)
- [x] Riskli phase: `/code-review` koştu (render thread'in kare yolu değişti → `make test-yaris` tetiklendi); bulgular aşağıda

## Uygulama Notları

- **Dördüncü ön koşul `ShellLog::caret`'in içinde**, `caret_in_dock`'un
  ifadesinde değil: orada kalsaydı `hold_left` dolu kalır ve saat 150 ms
  sonra aynı kareyi çizdirirdi. Uzakta `caret` tutmadan önce `Grid` +
  `None` dönüyor; ham cevabın damgası (`observe_caret`) değişmiyor.
- **`Frame` bağlam satırını artık açık bir bayraktan biliyor**
  (`dock_context`): tek satırlık yerleşimin iki anlamı var — uzak oturumun
  yalnız-bağlam bandı ve sınamaların bağlamsız tek giriş satırı. Üretim
  `set_dock_input_rows(input_rows)`'a geçti; `set_dock_rows` yalnız
  sınamada (`#[cfg(test)]`) ve eski "iki ve fazlası → bağlam" kuralıyla.
  Satır arası boşluk yalnız üstünde giriş satırı olan bağlam satırına.
- **`dock_hit` sıfırda `Some((top, 0))`**, `None` değil: `None` fare
  tarafında tek satırlık geri düşüşe gidip hayalet bir giriş bloğu doğururdu;
  `point_to_cell` sıfır satırı zaten reddediyor. `DockWindow.shown` da sıfır
  (isabet testi her noktayı reddediyor), `render_with`'in izi `(0, 0)` —
  `dock_scroll` kaydırmıyor.
- **Yazım efektleri sıfırda koşulsuz bitiyor** (`glyph_fx.finish()`),
  yalnız `Reset`'te değil; `GlyphFx::shift`'in `.max(1)`'i kalktı.
- `band_target` `Cursor` yerine `input_rows` alıyor (sınanabilirlik);
  `Motion::sync`'in bant argümanı `f32`.
- Doldurma bandı için ek kod gerekmedi: negatif bant `grid_top`'u büyütüyor
  ve `slide_fill_rows` şeridi kapatıyor (bileşim sınaması doğruluyor).
- `/code-review` (high) sekiz bulgu. **Düzeltilen:**
  - *Kaydırılmış pencerede tepedeki şerit boş kalıyordu* (ve ilk çentik bir
    satırdan fazla kayıyordu): `slide_fill_rows` kaydırılmış pencerede 0
    dönüyordu. `set_grid_top(rows, lowered)` bandın kısalığını ayrı taşıyor
    (`Session::grid_lowered`) ve kaydırılmış pencerede yalnız o pay, ofsetin
    ötesindeki defterle kapatılıyor — çentik başına sabit, yani 017'nin
    reddettiği çentikle değişen boşluk doldurulmuyor.
  - *Kesirli bant fazlasının `f32` hatası* son satırdaki ızgara caret'ini
    dock yuvasına itebiliyordu: `push_caret`'in örtüşme ölçütü yarım piksel
    toleranslı.
  - `cols.grid == 0` erken dönüşü sıfır giriş satırında `rows = 1`
    bırakıyordu (tekerlek kaydırırdı); `Motion::sync`'in artık yeniden
    bağlaması.
  **Waive:**
  - *`frame()` ile `Session::dock` uzak durumu ayrı kilit turlarında okuyor*:
    ssh'ın iki kenarında bir kare giriş satırı sayısı ile bağlam satırının
    biçimi ayrışabilir. `render_with`'in belgelediği "bilinen sınır, bir
    kare"nin kardeşi; kapatmak uzak durumu `Cursor` üstünden taşımak demek —
    bir karelik görsel fark için sınır tipine alan.
  - *`dock_context` alanı sınamalar için var*: `set_dock_rows(1)` kullanan
    renderer/efekt sınamaları büyük yüzlü bağlamsız giriş satırı istiyor;
    onları `set_dock_input_rows`'a taşımak satırı küçük yüze çevirip
    sınadıkları şeyi değiştirirdi.
  - *`CLAUDE.md` güncellenmedi*: sözleşme phase-3'ün (R7); çelişen cümleler
    phase-3 checklist'ine yazıldı.
