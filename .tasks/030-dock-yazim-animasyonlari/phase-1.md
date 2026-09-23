# Phase 1 — Farkı `bt-core`'da bul, sınırdan ikinci sink'le geçir

## Özet

`Session::dock` son çizilen aynaya karşı eklenen/silinen glyph'i bulur ve
`dock::render` onu ekran sütunlu bir `DockEdit` olarak ikinci sink'e basar;
`bt-gpu` sink'i bağlar ama düzenlemeleri henüz tüketmez — görünür değişim yok.

_Requirements: R1, R1.1, R1.2, R1.3, R1.4, R2_

## Değişiklikler

- **`crates/bt-core/src/dock.rs`** — `DockEdit` (`Arrive { col, cells }`,
  `Erase { col, ghosts }`, `Reset`; hücreler çözülmüş `Cell`, sabit kapasiteli
  kap — kare başına ayırma yok, kapasite en çok girdi sayısı kadar glyph'i
  taşımalı ya da taşmada `Reset`) ve saf `diff(old: &DockState, new:
  &DockState) -> Option<RawEdit>`: yalnız `buffer` üzerinde ortak önek/sonek,
  yön `cursor`'dan, glyph sayısı ≤ `new.answers - old.answers`; ikisi `Live`
  değilse ya da kural tutmazsa `Reset` (`discussion.md` → Karar 1, 2). Hayalet
  karakterleri ve vurgu stilleri `old`'dan (`style_at` eski duruma). `render`
  ham düzenlemeyi alır, ekran sütununu **kendi pencereleme hesabından**
  çözer (geliş: caret'in solundaki `width` sütun; silme: caret'in sütunu),
  hayaletleri `cell()` ile temaya çözer ve ikinci sink'e basar. Eski
  tamponun `skip`'i aynı saf hesapla bulunur; farklıysa `Reset`. Pencerenin
  dışına düşen geliş/hayalet basılmaz.
- **`crates/bt-core/src/session.rs`** — `Session::dock` ikinci sink'i alır
  (`impl FnMut(DockEdit)`); yaprak kilit turunda **`clone_from`'dan önce**
  `into.answers == shell.dock.answers` kapısı (damga ilerlemediyse girdi yok,
  fark yok), değilse `dock::diff`. Taban kuralı: yeni taraf `Live`, eski
  taraf `Live` ya da `Idle` (boş satır sayılır); başka her durum `Reset`
  (`discussion.md` → Karar 1).
- **`crates/bt-core/src/shell.rs`** — `apply_dock`'un `End` kolu `reset()`'ten
  sonra o anki nesli (`answers`) yazar; yoksa `Idle` taban sıfır damgalı kalır
  ve prompt'taki ilk yapıştırma girdi sınırını boşa düşürür.
  `the_mirror_carries_the_generation_it_answers`'ın `End` iddiası güncel
  damgaya göre düzeltilir. Doc'a yeni sözleşme: fark çağıranın tamponuna
  karşı, yani tampon **son çizilen** ayna olmalı (tek çağıran `bt-gpu`'nun
  içerik karesi). Mevcut sınama çağıranları no-op sink'le güncellenir.
- **`crates/bt-core/src/lib.rs`** — `DockEdit`'in ihracı.
- **`crates/bt-gpu/src/link.rs`** — ikinci sink bağlanır; düzenlemeler bu
  phase'de yoksayılır (sink boş gövdeli), yorum phase-2'yi işaret eder.

## Kabul

- `dock::diff` sınamaları `discussion.md` → Karar 2 tablosunun her satırı için:
  tek harf, iki tuşun tek karede birleşmesi, tek/çoklu Backspace, forward
  delete, yapıştırma (bir girdi çok glyph → `Reset`), geçmiş değiştirmesi,
  Ctrl-U, Tab tamamlama, tek karakterlik tamamlama, ölü tuş (iki girdi bir
  glyph), girdisiz ayna (`None`), öneri değişen ama `buffer` aynı (`None`),
  `Live` → `Idle` (`Reset`), geniş karakter (iki sütun, tek glyph),
  **prompt'tan sonraki ilk harf** (`Idle` taban → `Arrive`), **prompt'taki ilk
  eylem yapıştırma** (`Idle` taban damgalı → `Reset`), `Unavailable` →
  `Live` (`Reset`).
- `render` sınamaları: gelişin ve hayaletin sütunu pencerelenmiş satırda
  doğru; `skip` kayınca `Reset`; hayaletin rengi eski vurgudan.
- Kapı kapalı karede (`answers` ve `buffer` eşit) `diff` çağrılmıyor (sayaçla
  ya da saf kapı fonksiyonunun sınamasıyla).
- Ekranda değişiklik yok: mevcut dock sınamaları ve `make hepsi` yeşil.

## Checklist

- [x] `DockEdit` + `dock::diff` + `render`'ın sütun çözümü
- [x] `Session::dock` kapısı ve ikinci sink; doc
- [x] `bt-gpu` ikinci sink'i bağlar (no-op)
- [x] Test: Kabul'deki diff ve render sınamaları
- [x] Doğrulama geçti (`make hepsi`)
- [~] `make test-yaris` — tetiklenmedi: paylaşılan duruma tek değişiklik
  `End` kolunun damgası, mevcut yaprak kilit altında; panel phase-1 için
  bunu açıkça reddetti (`discussion.md` → Muhakeme → Reddedilenler). Riskli
  phase değil, `/code-review` set sonunda.
- [~] `make duman` — gerekmedi: görünür değişim yok ve süreli koşu dock
  almıyor (`/bin/sh`), yani yeni yol dumanda hiç koşmuyor.

## Uygulama Notları

- **Kapı durumu da soruyor:** `dock::change` damga **ya da** durum
  değiştiyse `diff`'i koşturuyor. Damgasız bir geçiş (`Unavailable` sıfır
  damgalı; girdisiz bir `End`) uçuştakileri bitirmeli; yalnız damgaya
  bakan kapı onları asılı bırakırdı.
- **`Change::Same`:** `BUFFER` aynı ama ayna ilerlediyse `diff` `None`
  değil `Same { old_skip }` döndürüyor, çünkü caret'in kaydırdığı pencere
  (Karar 3) metin değişmeden de kayabiliyor; `render` kaymada `Reset`,
  yoksa hiçbir şey basıyor. Kabul'ün "`None`"ı gözlemlenebilir hâliyle
  sınanıyor (düzenleme sink'i çağrılmıyor). Sonucu: taşan satırda her tuş
  pencereyi kaydırdığı için orada yazım animasyonsuz — seçilmiş sınır.
- **Eski pencerenin kayması `diff`'te** (`render`'da değil): `render`
  koşarken eski ayna ezilmiş oluyor. Formül tek (`window_skip`), iki
  tüketici; `diff` bu yüzden `cols` alıyor.
- **Glyph = genişliği sıfırdan büyük karakter:** `❤️` iki kod noktası,
  tek girdi, tek glyph (birleştirici `render`'da da hücre almıyor). Koşu
  yalnız birleştiriciyse `Reset`.
- **`PREDISPLAY` değiştiyse `Reset`:** metin kaymıştır; `Idle` tabanda
  sorulmuyor (ekranda metin yok).
- **Caret ızgaradaysa (`owned == false`) canlanma yok, `Reset`:** satır
  ızgarada (bayat ayna) ve efektin konusu dock'ta yazmak.
- **Hücreler yalnız glyph'liler;** boşluk yazımı boş hücreli bir `Arrive`
  basıyor, çünkü sütunu uçuştaki gelişlerin bitme kuralına giriyor.
- `EDIT_MAX = 8` (tasarım sabiti); aşan düzenleme `Reset`.
- `render` dokuz parametre aldı: `#[allow(clippy::too_many_arguments)]`,
  gerekçesi doc'unda.
- `make hepsi` iki kez `bt-shell`'in pano sınamalarında düştü (bir
  SIGSEGV, bir `pending_copy_delivers_to_the_given_board` — paylaşılan
  panoya başka bir yazar); bu phase `bt-shell`'e dokunmuyor, tek başına ve
  üçüncü tam koşuda yeşil.
