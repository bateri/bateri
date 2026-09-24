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
- [ ] Doğrulama geçti (`make hepsi`)
