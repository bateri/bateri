# Phase 1 — Küçük sınıfta yordamsal karakterler

## Özet

Yordamsal kapı `SizeClass::Small`'da küçük yüzün kendi ölçüsüyle açılır;
sparkline'ın blokları bağlam satırında döşer, büyük sınıf bit bit aynı kalır.

_Requirements: R1.1, R1.2, R1.3, R7_

## Değişiklikler

- **`crates/bt-atlas/src/lib.rs`** —
  - `Atlas::new` küçük yüzden ikinci bir `Metrics` türetir (`rules::metrics`
    aynı kural ve aynı `line_height`; genişliği `context_cell_w`). Alan
    `small_metrics`; `context_advance`/`context_cell_w` ile aynı yerde doğar,
    ikinci bir genişlik kaynağı açılmaz (`two representations of the small
    class` bekçisinin kapsamı genişler).
  - `Atlas::slot`: `(Sprite::Char(ch), SizeClass::Small) if
    raster::is_procedural(ch)` kolu yüzü `Regular`'a normalize eder, sınıfı
    `Small` bırakır (anahtar büyük sınıfınkiyle çakışmaz). Çizim kolunda
    küçük sınıf yordamsal karakter `small_metrics`'le **ayrı bir tampona**
    çizilir (`draw_procedural`'ın `slot_bytes` assert'i o ölçüye göre) ve
    büyük yuvaya kopyalanır: sol kenar 0, dikey kaydırma `metrics.baseline_px
    − small_metrics.baseline_px` (küçük hücrenin taban satırı büyük hücreninkine
    oturur; alt kenar büyük hücreyi aşarsa kırpılır — `saturating`, panik
    yok). Yuvanın geri kalanı sıfır. Tampon `buffer` gibi atlasın sahipliğinde,
    kare başına ayırma yok.
  - Yorumlar: kapının "küçük sınıfta kapalı" gerekçesi (`slot`'un uzun yorumu,
    `Half::Whole` normalizasyonunun yorumu) yeni kurala göre yeniden yazılır —
    ölçü ayrışması kalktı, kapı açık; `Half::Whole` kuralı aynen kalır
    (aile tanımı gereği tek hücre).
  - Sınamalar: `the_small_class_still_asks_the_font` tersine döner — küçük
    sınıfın `█`'i yordamsal ve **yalnız** `context_cell_w × küçük yükseklik`
    kutusunu dolduruyor (kutunun içi 255, sağındaki sütunlar ve kutunun
    dışı 0), yuvası büyük sınıfınkinden ayrı. Yeni: `▁…█` küçük sınıfta
    taban çizgisi hizalı (alt kenarları aynı satırda, yükseklikler sekizde
    bir adımla artıyor); büyük sınıfın yordamsal raster'ı değişmedi (mevcut
    piksel bekçileri dokunulmadan yeşil). `▲`, `●` için sözlük bekçisi:
    `bt-core`'un phase-2'de doğacak `STATS_GLYPHS` listesinin elle kopyası,
    Menlo'nun küçük sınıfında kutu değil (`the_upload_row_is_the_one_the_atlas_checks`
    emsali; listeyi bağlayan karşı bekçi phase-2'de `bt-core` tarafında).
  - `face_fallback_is_cached_under_the_requested_face` büyük sınıfın köşegen
    deliğinde yaşıyor, dokunulmaz; `census` yalnız büyük sınıfı sayıyor,
    değişmez.
- **`crates/bt-atlas/src/raster.rs`** — `draw_procedural` imzası değişmez;
  gerekiyorsa yalnız doc'u ("the caller's `Metrics`" artık iki ölçüden biri).
- **`CLAUDE.md`** — "Kapı **küçük sınıfta kapalı** ve gerekçe döşeme değil ölçü
  ayrışması…" cümlesi yeni kurala döner: küçük sınıfta küçük yüzün ölçüsüyle,
  taban çizgisi hizalı (gerekçe ve işaretçi `.tasks/046-uzak-yuk-gostergesi/discussion.md`
  → Karar 3); `bt-atlas` satırındaki "yalnız büyük sınıfta" ibaresi kalkar.

## Kabul

- `cargo test -p bt-atlas` yeşil; ters dönen ve yeni bekçiler geçiyor,
  büyük sınıfın piksel bekçileri değişmeden geçiyor.
- `make check` ve `make linux` yeşil (FreeType arka ucunda da aynı kapı).

## Checklist

- [ ] `small_metrics` ve küçük sınıf yordamsal kolu
- [ ] Ayrı tampona çizim + taban çizgisi hizalı kopya
- [ ] Test: küçük `█` yalnız küçük kutuyu dolduruyor, yuva ayrı
- [ ] Test: `▁…█` küçük sınıfta hizalı ve sekizde bir adımlı
- [ ] Test: `▲`, `●` küçük sınıfta kutu değil
- [ ] `CLAUDE.md` cümlesi
- [ ] Doğrulama geçti (`make check` + `make linux`)
