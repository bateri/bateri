# Phase 5 — Dock yüzeyi

## Özet

Dock'un giriş satırına yazılmış URL ya da yol aynı kurallarla ⌘-hover'da
vurgulanıyor ve ⌘-tıkla açılıyor.

_Requirements: R8_

## Değişiklikler

- **`crates/bt-core/src/session.rs`** — `LinkPoint::Dock` kolu: metin
  `dock::selectable`'dan, isabet son çizilen pencerenin çözümleyicisinden
  (`DockWindow`, `dock_select`'in yolu) ve dock'un tek düzen yürüyüşünden
  (`dock_layout`); `link::scan` aynı. Damga aynanın nesli / `BUFFER`'ı; dock
  hover'ı `Session::dock`'taki ezme yardımcısından çizilir (phase-2).
  Bağlam satırı bağlantı değil; uzak oturumda giriş satırı yok.
- **`crates/bt-shell-macos/src/view.rs`** — dock'un `window_point_dock`'u
  hover ve ⌘-tık yolunda; dock basışında da ⌘ + doğrulanmış bağlantı
  `pressed_dock`'tan önce.

## Kabul

- Sınama: `BUFFER` = `open https://a.dev/x` → dock noktasında `LinkHit`;
  sarılmış satırda bölünen URL tek bağlantı; `BUFFER` değişince damga
  tutmuyor.
- Gözle kontrol: dock'a `open https://example.com` yaz, ⌘-hover alt çizgi,
  ⌘-tık açar, caret yerinden oynamaz.
- `make check` + `make linux` + `make smoke` yeşil.

## Checklist

- [ ] `LinkPoint::Dock` hit testi ve damgası
- [ ] (phase-2'den devralındı) `Session::dock`'un sink'ine ezme yardımcısını bağla: `hover_style`/`underline_link` (`session.rs`) dock hover'ıyla; `LinkHover`'ın yüzeyi (ekran mı dock mu) ve dock damgası (aynanın nesli / `BUFFER`) burada doğuyor, `frame()`'in damga denetimi dock hover'ını bayat saymamalı
- [ ] View'da dock bağlama
- [ ] Test: dock hit, sarma, bayatlık
- [ ] Doğrulama geçti (`make check` + `make linux` + `make smoke`)
