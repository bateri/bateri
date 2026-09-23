# Phase 1 — Izgara jestleri: kelime, satır, Shift+tıklama, Select All

## Özet

Izgarada çift/üçlü tıklama, tip koruyan sürükleme, Shift+tıklama ve ⌘A;
görünüş bugünkü ters videoda kalıyor.

_Requirements: R1.1, R1.2, R1.3, R1.4, R1.5_

## Değişiklikler

- **`crates/bt-core/src/session.rs`** — `pub enum SelectKind { Simple, Word,
  Line }` ve `set_selection`'ın ona göre alacritty'nin `Simple`/`Semantic`/
  `Lines`'ını kurması; `pub` API'de alacritty tipi görünmez. Var olan seçimin
  ucunu taşıyan uzatma yolu (Shift+tıklama) tipi korur; seçim yoksa tıklanan
  noktadan `Simple` başlar. `update_selection` bugünkü gibi — tip seçimin
  içinde yaşıyor, sürükleme kelime/satır adımını alacritty'den alıyor. Bütün
  geçmişi seçen yol (`select_all`; `Lines`, en üst satırdan en alta). Kare
  kapısı (`visible_range` karşılaştırması) üç yolda da aynı.
- **`crates/bt-core/src/session.rs` → `term_config`** — `semantic_escape_chars`
  kelime sabitinden; `term_config_keeps_every_other_field` bu alanı artık
  varsayılana değil sabite çiviler.
- **`crates/bt-core/src/lib.rs`** (ya da seçimin modülü) — kelime ayırıcı
  sabiti (`WORD_SEPARATORS`), doc'unda Karar 5'in gerekçesi ve davranışın
  sahibinin alacritty'nin `Semantic`'i olduğu (parantez eşleme, ayırıcı üstünde
  çift tıklama — `alacritty_terminal` `selection.rs` `range_semantic`,
  `term/search.rs`). Dock phase-4'te aynı sabiti okuyacak.
- **`crates/bt-core/src/input.rs`** — `button_route`'un `Select` kolu Shift'i
  bugünkü gibi taşır; Shift+tıklamanın "uzat" anlamı fare kipinin dışında da
  geçerli. Karar tablo biçiminde kalır.
- **`crates/bt-shell/src/view.rs`** — jest defteri (`dragging`,
  `sent_buttons`, `motion_cell`'in basış/bırakma mantığı, tıklama sayısı, Shift)
  `NSEvent` görmeyen bir struct'a (`Gesture`) çıkar; `BateriView` olayı sayıya
  ve bayrağa çevirip ona sorar. `clickCount` 1/2/3 → `SelectKind`; 3'ten büyük
  sayı `Line`'da kalır. Yol haritasının "Farenin jest durumu sınanamıyor"
  kaleminin dört geçişi (basış, sürükleme, bırakma, kayıp bırakma) sınanır.
  `selectAll:` eylemi (`select_all`).
- **`crates/bt-shell/src/menu.rs`** — Edit ▸ Select All (⌘A). `keyDown:`'ın
  izin listesi değişmez (menü öğesi `performKeyEquivalent:`'la önce yakalanır).
- **`docs/YOL-HARITASI.md`** — "Farenin jest durumu sınanamıyor" kalemi
  kapanır (işaretçi: bu set).
- **`CLAUDE.md`** — `bt-shell` paragrafının menü listesi (Edit'te Select
  All), fare arbitrajı cümlesine çift/üçlü tıklama ve Shift+tıklama.

## Kabul

- Sınama: `a b.c/d:e f=g` satırında çift tıklama `b.c/d:e`'yi, `=`'nin
  sağında `g`'yi seçer; üçlü tıklama sarılmış satırın iki fiziksel satırını
  birden seçer; çift tıklayıp sürüklemek kelime sınırında durur; Shift+tıklama
  `Word` seçimini kelime adımıyla uzatır; seçim yokken Shift+tıklama (fare
  kipinde de) yeni seçim başlatır.
- Sınama: `Gesture`'ün dört geçişi, fare kipinde Shift+tıklama (rapor değil
  seçim) ve kayıp bırakmadan sonra bayat `dragging`'in inmesi.
- `term_config_keeps_every_other_field` yeni sabitle yeşil.
- `make hepsi` yeşil; `make duman` jetonları değişmez.

## Checklist

- [x] `SelectKind` + `set_selection`/uzatma/`select_all`
- [x] `WORD_SEPARATORS` + `term_config`
- [x] `Gesture` struct'ı ve `view.rs`'in ona geçişi; `clickCount`
- [x] Edit ▸ Select All
- [x] Test: kelime/satır/uzatma sınamaları; `Gesture` geçişleri
- [x] `CLAUDE.md` ve yol haritası cümleleri
- [x] Doğrulama geçti (`make hepsi`, `make duman`)

## Uygulama Notları

- `Gesture` `view.rs`'te değil kendi modülünde (`crates/bt-shell/src/gesture.rs`):
  saf struct + sınamaları, `view.rs` zaten 2300 satır. `moved_to_new_cell` ile
  `button_bit` oraya taşındı; ivar tek `Cell<Gesture>` (`Copy`, al-değiştir-koy).
- Davranış değişikliği (tek): yeni sol basış bayat `dragging`'i de indiriyor
  (`Gesture::begin_press`) — önceden yalnız raporlanan bit iniyordu ve
  raporlanan basışın yanında bayat seçim sürüklemesi kalabiliyordu.
- Satır seçimi (`Line`) satır sonunu da kopyalıyor (`"…\n"`): alacritty'nin
  `Lines`'ı, Terminal.app'in üçlü tıklamasıyla aynı; sarılma yerinde `\n` yok.
- Shift+tıklama tıklama sayısından önce gelir (Shift+çift tık da `Extend`).
- `input.rs`'te kod değişmedi, yalnız `ButtonRoute::Select`'in doc'u.
- Gözle (geçici paket, açık ve koyu tema): çift tık `b.c/d:e` ve `KEY=`'den
  sonra `value`; Shift+tık kelime adımıyla uzattı; üçlü tık sarılmış satırın
  iki fiziksel satırını; çift tık+sürükleme kelime adımıyla; ⌘A bütün ızgara.
  Pano metinleri doğru. Kelime arası boşluklar vurgusuz — bugünkü hücre
  kuralı, phase-2'nin koşusu kapatıyor.
