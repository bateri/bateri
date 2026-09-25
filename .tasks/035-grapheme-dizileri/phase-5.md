# Phase 5 — Kümeleme açık; sözleşme güncel

## Özet

Kümeleme varsayılan açılır ve `CLAUDE.md` ile yol haritası bugünkü
sözleşmeye çevrilir.

_Requirements: R5_

## Değişiklikler

- **`crates/bt-core/src/session.rs` / `crates/bt-shell/src/app.rs`** —
  oturum seçeneğinin varsayılanı açık; süreli koşu dahil bütün pencereler.
  Bayrak kod yolunda kalır (geri alma tek satır), ayar anahtarı **değil** —
  kullanıcıya açılmıyor (Karar 2).
- **`CLAUDE.md`** — 023 paragrafının "grapheme dizileri kapsam dışı"
  cümlesi, 024'ün "sıfır genişlikli kod noktasını atlıyor" cümlesi, sınır
  `Cell` ve katman tablosu (`bt-core`: okuyucu döngünün sahibi; `bt-atlas`:
  küme şekillendirme) kural + tek cümle gerekçe + işaretçi olarak; iki
  ürün bedeli (`discussion.md` → Karar) adıyla.
- **`docs/YOL-HARITASI.md`** — 024 kapanışındaki "grapheme dizileri taban
  karakteriyle çiziliyor (ayrı set)" cümlesi kapanır, kapsam dışı kalanlar
  (çıplak 78, tek RI, emoji dışı kümeler, aramanın kümeyi görmemesi)
  sete bağlanmamış borç olarak.

## Kabul

- `make duman` yeşil.
- Gözle kontrol (devir mesajı): Claude Code'da `🇹🇷 "YouTube için…"`
  satırı — bayrak tek renkli glyph, tırnak iki sütun sonra; ızgarada
  `echo '👍🏽 ❤️ 👨‍👩‍👧 🌡️ x'` — dördü de tek glyph, `x` hizalı; doldurma
  bandında aynı satır (Tab listesi kalkınca); dock'ta aynı dizileri
  **yazarak** — satır dock'ta kalıyor, caret doğru sütunda, ⌫ (düzenleme
  kapısı açıkken, yani ekleme keymap'inde) bütün kümeyi siliyor.

## Checklist

- [ ] Varsayılan açık
- [ ] `CLAUDE.md` ve yol haritası
- [ ] Doğrulama geçti (`make hepsi` + `make duman`)
