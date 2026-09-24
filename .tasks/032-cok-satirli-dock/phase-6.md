# Phase 6 — Yazım efektleri iki eksende

## Özet

030'un Keypress/Erase efektleri çok satırlı dock'ta da koşsun: anahtar sütun
değil (satır, sütun) konumu, sarma yüzünden satır değiştiren kayma iki
eksende.

_Requirements: R6_

## Değişiklikler

- **`crates/bt-core/src/dock.rs`** — `diff` düz metin üstünde kalır (tek
  bitişik ekleme/silme, `EDIT_MAX`); `DockEdit` konumu `layout` üzerinden
  (satır, sütun) olarak verir, kayma iki eksenli. Satır sayısı atlayan ya da
  eşlenemeyen hâl `Reset`.
- **`crates/bt-gpu/src/glyph_fx.rs`** — girdiler (satır, sütun) anahtarlı;
  `shift` ve `retain` iki boyutlu pencereyle; phase-3'ün çok satırlı
  `Reset`'i kalkar — o kapı `bt-core`'da, `dock::render_with`'in `single`
  koşulu (phase-3 → Uygulama Notları); `DockEdit::Shift` phase-3'ten beri
  üretilmiyor.

## Kabul

- Satır sonunda yazılan harf sarılıp yeni satıra geçerken efektiyle geliyor;
  ikinci satırda Backspace Erase ile gidiyor; kayan harfler yeni konumunda
  animasyonsuz.
- Tek satırda mevcut 030 bekçileri değişmeden yeşil.
- `make hepsi`; gözle kontrol: çok satırlı `BUFFER`'da yazma/silme.

## Checklist

- [ ] `DockEdit` (satır, sütun)
- [ ] `glyph_fx` iki eksen
- [ ] Test: sarma sınırında ekleme, ikinci satırda silme
- [ ] phase-5'ten devir (gözle görülen, 032'nin değil): çok satırlı komutun ilk satırı ekranın üstüne kayınca blok işareti (chevron) devam satırına, ızgaranın tepesine oturup orada kalıyor — çıpa `preexec`'e kadar açık, `blocks.anchors` görünen ilk çıpalı satırı komut satırı sayıyor (teşhis `phase-5.md` → Uygulama Notları). Çare: çıpa değişiminde üstteki satır (geçmiş dahil) aynı kimliği taşıyorsa o satıra işaret ve sayaç çizme; doldurma bandının devamında da aynı soru. Kapsam dışı sayılırsa `docs/YOL-HARITASI.md`'ye adıyla
- [ ] Doğrulama geçti (`make hepsi`)
