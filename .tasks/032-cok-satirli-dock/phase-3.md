# Phase 3 — Dock sarar ve büyür

## Özet

Dock yatay pencerelemeyi bırakıp uzun satırı sarar; bant `layout`'un satır
sayısıyla (tavana kadar) büyür, isabet testi, seçim koşuları ve caret iki
boyutlu olur. Satır sonlu görüntü hâlâ `Multiline`.

_Requirements: R1.2, R1.3, R4.1, R4.2 (tek mantıksal satırda)_

## Değişiklikler

- **`crates/bt-core/src/dock.rs`** — `render_with`, `hit`, hayalet hücre ve
  seçim koşuları `layout`'un dock parametrizasyonundan (ilk ve devam satırları
  `TEXT_COL`'dan); `columns()` yürüyüşü `layout`'a katılır ya da onun satır
  başına kullanıcısı olur — tek kopya kuralı korunur. `window_skip` emekli;
  yerine tavanı aşan girişte caret'in satırını görünür tutan durumsuz dikey
  pencere. `Dock` sınır tipi: caret (satır, sütun), seçim görsel satır başına
  koşu listesi. `DockPoint`'e ulaşan isabet (satır, sütun, yarı).
  `selection_range`'in `Line`'ı **mantıksal satır** (`\n`'ler arası) — tek
  mantıksal satırda bugünkü "bütün `BUFFER`" ile aynı sonuç.
- **`crates/bt-core/src/session.rs`** — `frame()` `layout`'u **iki kez**
  koşar: ızgara parametrizasyonu bastırmanın `to`/`floor`'u için, dock
  parametrizasyonu (asma girinti, kapasite `cols − TEXT_COL`) çizilecek satır
  sayısı için — sarılan satırda ikisi ayrışır, biri ötekinin yerine
  kullanılmaz. Dock'un sayısını bütçeyle kırpıp `Cursor`'a yazar; `DockWindow` dikey pencereyi ve
  satır sayısını taşır.
- **`crates/bt-gpu`** — dock seçim koşuları birden çok satırda (031'in şekli
  zaten çok satırı biliyor); caret'in dock satırı.
- **`crates/bt-shell/src/view.rs`** — `dock_select`/`dock_extend`/sürükleme
  satır alır.
- **`crates/bt-gpu/src/glyph_fx.rs`** — `layout` birden çok satır verdiğinde
  efektler geçici olarak `Reset` (anında); phase-6'da iki eksene geçer.

## Kabul

- Uzun tek satırlık komut dock'ta sarılıyor, bant büyüyor, caret sarılan
  satırda doğru sütunda; geniş karakter satır sonunda yarılanmıyor.
- İkinci görsel satıra tıklama caret'i doğru karaktere taşıyor; satırlar
  arası sürükleme seçiyor, vurgu iki koşu.
- Tavanı aşan girişte caret görünür kalıyor.
- Emekli `window_skip` bekçileri silinmiyor, sarmanın karşılıklarına
  çevriliyor (`a_wide_char_is_never_split_at_the_window_edge` →
  satır sonu, vb.).
- `make hepsi`, `make duman`; gözle kontrol: uzun komutu yaz, dock büyür,
  ızgara yukarı süzülür, Enter'da geri döner.

## Checklist

- [ ] `render_with`/`hit`/seçim `layout`'tan; `window_skip` → dikey pencere
- [ ] `Dock` sınır tipi (satır, sütun) ve koşular
- [ ] `frame()` satır sayısını bütçeyle kırpıyor
- [ ] Fare yolu satır alıyor
- [ ] Çok satırda efektler `Reset`
- [ ] Test: sarma, 2B isabet, satırlar arası seçim, tavan
- [ ] phase-1'den devir: imleç bir `\n`'in arkasındayken ızgara başlangıcı ve caret kuralı (phase-1 → Uygulama Notları) — bu phase'in dock tarafına etkisini uygula ya da gerekçesiyle kapat
- [ ] Doğrulama geçti (`make hepsi`, `make duman`)
