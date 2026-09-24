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

- [x] `render_with`/`hit`/seçim `layout`'tan; `window_skip` → dikey pencere
- [x] `Dock` sınır tipi (satır, sütun) ve koşular
- [x] `frame()` satır sayısını bütçeyle kırpıyor
- [x] Fare yolu satır alıyor
- [x] Çok satırda efektler `Reset`
- [x] Test: sarma, 2B isabet, satırlar arası seçim, tavan
- [x] phase-1'den devir: imleç bir `\n`'in arkasındayken ızgara başlangıcı ve caret kuralı (phase-1 → Uygulama Notları) — bu phase'in dock tarafına etkisini uygula ya da gerekçesiyle kapat
- [x] phase-2'den devir: `link.rs`'in `DockBudget { rows: 1, … }`'ı → `DOCK_MAX_SHARE` tavanı; `dock_caret_at`'in satır argümanı bugün sabit `0` (caret'in dock satırı); `window_point_dock` bloğun satırını zaten döndürüyor, çağıranlar (`dock_select`/`dock_extend`) onu henüz yoksayıyor
- [x] Doğrulama geçti (`make hepsi`, `make duman`, `make test-yaris` — `frame()` ve `dock_window` değişti)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi (üçü waive, aşağıda)

## Uygulama Notları

- **Tek yürüyüş `layout_with`:** `layout` karakter başına bir `place`
  çağrısı veren genel hâle (`Placed { row, col, … }`) çevrildi ve `columns`
  silindi; dock parametrizasyonu `dock_layout` (ilk ve devam satırları
  `TEXT_COL`, genişlik ızgaranınki, sütun doğrudan ekran sütunu). Çizim,
  isabet, `needed_rows` ve hayaletler onu okuyor. `render_with` yürüyüşü iki
  kez koşuyor (önce caret'in satırı → dikey pencerenin tepesi, sonra hücreler);
  ikisi aynı fonksiyon.
- **Tavan oran olarak geçiyor, satır olarak değil:** `DockBudget { share,
  cols }`, `fit(needed, grid_rows)`. `bt-gpu` ızgaranın satır sayısını
  `frame()`'den önce bilmiyor (`Layout`'un doc'u ikinci kopyayı yasaklıyor);
  `frame()` oranı `Term` kilidinin altında okuduğu satır sayısına uyguluyor.
  `DOCK_MAX_SHARE = 0.5` `bt-gpu/frame.rs`'te.
- **Satır sayısı yaprak kilit turunda:** `dock::needed_rows` aynanın `Live`
  hâlinden (bastırma olsun olmasın — dock `Live` metni her durumda çiziyor),
  kopya yok. **Öneri (`POSTDISPLAY`) bandı büyütmüyor**: autosuggestions'ın
  önerisi her tuşta toptan değişiyor ve sarma sınırında bant her tuşta
  büyüyüp küçülür, ızgara yazarken nefes alırdı; önerinin metnin satırlarına
  sığmayanı kırpılıyor (030'da sağ kenarda kesildiği gibi). Satırlar yine
  çizimin yürüyüşünden sayılıyor (öneri caret'in satırını değiştirebiliyor).
- **Seçim koşuları `Dock`'ta değil:** `Dock` `Copy` kalsın diye koşular
  çağıranın tamponuna akıyor (`Session::dock`'un `runs`'ı, `LinkIvars`'ta
  `dock_selection`); `Frame::push_dock_selection(&[SelectionRun])`.
  `Dock::caret` → `Option<DockCaret { col, row }>`.
- **İşaret dikey pencerede gizleniyor:** `Dock::sigil` `Option`; ilk satır
  ekranda değilse `None` (işaret bir devam satırının yanında komutun orada
  başladığını söylerdi).
- **Çok satırda `Reset` `dock.rs`'te, `glyph_fx.rs`'te değil:** hangi
  değişimin canlanacağı kararı `bt-core`'un (`DockEdit`'in doc'u); `bt-gpu`
  değişmedi. `window_skip` ve `Change`'in `old_skip`'i kalktı, `shift` hep
  `0`, `DockEdit::Shift` artık üretilmiyor (alanlar phase-6 için sınırda).
  phase-6'nın `glyph_fx` maddesi buna göre okunmalı: kaldırılacak `Reset`
  `render_with`'in `single` kapısı.
- **Dock seçim API'si `SelectionPoint` alıyor:** `dock_select/extend/drag(…,
  point)` — yan yana `row`/`col` iki `u16` olurdu.
- **phase-1 devri kapandı (dock tarafı):** dock parametrizasyonu ilk satırın
  başını gözlemiyor (`first = rest = TEXT_COL`), yani `\n`-arkası imleç
  varsayımı dock'u etkilemiyor; caret kuralı aynı (`layout`) ve tam dolan
  satırın ardındaki caret dock'u bir satır büyütüyor. Izgara tarafı phase-4'ün
  checklist'inde.
- **Test-first sırası tutmadı** (API değişimi sınamaları derletmeden
  yazdırmıyordu); yerine mutasyonla doğrulandı: dikey pencere kapatılınca üç
  bekçi, isabetin `top`'u atılınca pencere bekçisi, koşular satır başına
  bölünmeyince satırlar arası seçim bekçisi kırmızı.
- **Emekli bekçiler çevrildi:** `a_long_line_scrolls…` →
  `a_long_line_wraps_under_the_text_column`; `a_wide_char_is_never_split_at_the_window_edge`
  → `…_at_the_row_end`; `the_window_reserves_the_whole_char_under_the_caret`
  → `the_caret_stays_on_the_whole_wide_char_under_it`;
  `a_window_narrower_than…` → `a_row_narrower…`; `the_walk_stops_at_a_wide_char…`
  → `a_wide_char_that_does_not_fit_wraps_and_the_next_follows_it`; taşan satır
  efekt bekçileri → `a_wrapped_line_resets_its_effects_until_they_learn_two_axes`
  ve `typing_on_one_row_animates_without_a_shift`.
- **`/code-review` bulguları:** (1) "tek satır mı" kapısı önerinin satırlarını
  da sayıyordu → bandın ölçüsüyle aynı `measure`; (2) alternatif ekranda /
  dock'suz pencerede `Live` kalmış ayna `input_rows > 1` verebiliyordu →
  `frame()` orada 1; (3) isabet izi çizilen satır sayısını da taşıyor
  (`DockWindow::shown`), pencerenin dışına düşen nokta reddediliyor;
  (4) `stream` `display()`'den; (5) CLAUDE.md paragrafı kısaltıldı.
  **Waive:** `frame()` ile `Session::dock`'un aynaları iki kilit turu
  arasında ayrışabiliyor (bir karelik bilinen sınır, `line-finish`'inkinin
  kardeşi; `render_with`'in doc'u); dikey pencerenin üstündeki satırlara fare
  ulaşamıyor (pencere yalnız caret'i izliyor — klavye ya da ⌘A/üçlü tık);
  aynı bayt boylu farklı `BUFFER`'da isabet eski izle yürüyor (031'den kalan
  sınıf, sarmayla satır da kayabiliyor); ölü `Shift`/`shift` phase-6'ya.
- **Gözle kontrol** (geçici paket, açık ve koyu tema): uzun satır sarılıp bant
  büyüyor, ızgara yukarı çıkıyor; tavanı aşınca pencere caret'i izliyor ve
  işaret kayboluyor, ⌃A'da geri geliyor; iki satıra sürükleme seçiyor ve
  örtüşen koşular tek parça; ⌃U'da bant tek satıra dönüyor. Görülen tek sapma
  punto büyütmenin (resize) ardından ızgarada kalan eski satır — zsh'in
  SIGWINCH yeniden çiziminin artığı, yeni satırda tekrar etmedi; HEAD'de
  sınanmadı. Autosuggestions makinede kurulu değil: öneri kararı gözle
  sınanmadı, bekçiyle (`a_wrapping_suggestion_does_not_stop_the_effects`,
  `the_needed_rows_come_from_the_dock_layout`).
