# Phase 3 — Sözleşmeyi ve defterleri gerçeğe uydur

## Özet

Dock'un sütun saydığı yazılı hâle geliyor, `CURSOR` borcu kapanıyor ve
bağımlılık kararı kayda geçiyor.

_Requirements: R6_

## Değişiklikler

- **`CLAUDE.md`** — dock paragrafı bugün giriş satırının **ne** gösterdiğini
  anlatıyor, **hangi birimle** gösterdiğini anlatmıyor. İki cümle giriyor:
  dock'un sütunu `unicode-width`'ten birikiyor (ızgaranın kullandığı crate,
  yani iki yüzey tanım gereği aynı cevabı veriyor) ve `region_highlight`'ın
  aralıkları **karakter** indeksinde kalıyor çünkü ZLE'nin birimi o.
  `bt-core` satırına bağımlılık ekleniyor; "Bağımlılık mimari karardır"
  maddesinin taban listesine `unicode-width` **gerekçesiyle** giriyor —
  `polling`'in emsali ve aynı cümle şekli ("yeni bir crate değil, grafta
  zaten vardı").
- **`docs/YOL-HARITASI.md`** — `CURSOR` kalemi **kapanıyor**. Kapanış
  cümlesi kökü söylemeli: kalem "caret kayıyor" diye yazılmıştı, çaresi
  "dock sütun saysın" çıktı ve aynı çare emojinin dock'ta kutu çıkmasını da
  kapattı. 023'ün satırına da bir cümle: setin bıraktığı dock kusuru 024'te
  kapandı.
- **`.tasks/README.md`** — 024'ün notu sonuç diliyle.

## Kabul

- `CLAUDE.md`'de kodla çelişen cümle yok; bağımlılık gerekçesiyle yazılı.
- `CURSOR` kalemi kapandı ve kapanış kökü söylüyor.
- `make hepsi` yeşil (`make denetim`'in belge ve bağımlılık taramaları dahil).

## Checklist

- [ ] `CLAUDE.md`: dock'un birimi, `region_highlight`'ın birimi, bağımlılık
- [ ] `docs/YOL-HARITASI.md`: `CURSOR` kapandı, 023'ün satırına not
- [ ] `.tasks/README.md`: 024'ün notu
- [ ] Doğrulama geçti (`make hepsi`)
