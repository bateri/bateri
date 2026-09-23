# Phase 5 — Erase'in kalan yedi efekti

## Özet

`iris`, `undertow`, `echo`, `bleed`, `unravel`, `sublime`, `shatter` —
`discussion.md` → Karar 6'daki tanımlarla — shader'da ve ayarda.

_Requirements: R9, R5_

## Değişiklikler

- **`crates/bt-gpu/shaders/glyph_fx.metal`** — yedi hayalet dalı. `iris`:
  glyph merkezli dairesel maske kapanır; `undertow`: aşağı ve sola (caret'e)
  kayma + sönme; `echo`: büyüyüp sönen halka; `bleed`: kapsama eşiği düşerken
  alfa ve renk zemine; `sublime`: yukarı süzülme + hafif açılma; `unravel`:
  yatay şeritler, her şerit kendi gecikmesiyle yana kayar; `shatter`: k×k
  karo, her karo tohumundan yön/dönme alır, düşer ve söner — geometri yok,
  fragment karoları gezip ilk isabeti alıyor, karo sınırında kırpılıyor.
  Parçalı ikisinin uzağa gitmesi süreyi değil eğriyi değiştiriyor (Karar 6).
  Her dal `t = 1`'de tam saydam.
- **`crates/bt-gpu/src/glyph_fx.rs`** — `EraseFx`'e yedi kol; tohum girdi
  başına (sütundan türeyen, kare boyunca sabit — hareket karesinde parçalar
  titremesin).
- **`crates/bt-gpu/src/frame.rs`** — şişme payı `shatter`'ın ve `undertow`'un
  en uzak konumunu karşılar.
- **`crates/bt-core/src/settings.rs`** — `Erase::NAMES`'e yedi ad; `TEMPLATE`.
- **`crates/bt-shell/src/settings_window.rs`** — `Choice` başlıkları.
- **`docs/AYARLAR.md`** — değer listesi ve her efektin tek cümlesi.

## Kabul

- R5 değişmezleri sekiz hayalet efektinin hepsinde: `t = 1`'de düz zemin,
  komşu yuva örneklenmiyor, geniş glyph tek kutu; ek olarak `t = 0`'da
  hayalet statik glyph'le piksel piksel aynı (silinme anında sıçrama yok).
- `shatter`'ın tohumu kararlı: aynı girdi iki karede aynı parçaları veriyor.
- Ayrıştırma sınaması yedi yeni adı tanıyor.
- Gözle: her efekt Backspace'te tanımıyla uyuşuyor; basılı Backspace'te
  hayaletler sola doğru sıralanıyor; satır ortasında silinen harfin hayaleti
  kayan metnin altında kalıyor; emoji ikiye bölünmüyor.

## Checklist

- [ ] Yedi shader dalı (parçalı ikisi dahil)
- [ ] `EraseFx` kolları, tohum, şişme payı
- [ ] `NAMES`, şablon, popup başlıkları, `docs/AYARLAR.md`
- [ ] Test: R5 döngüsü sekiz efektte, `t = 0` eşitliği, tohum kararlılığı, ayrıştırma
- [ ] Ölçeklenen karelerde pürüz (phase-4'ün offscreen karelerinde görüldü, `recede` dahil): `glyph_fx` ölçekli örneklemede **doğrusal** filtreye geçsin, atlasın komşu yuvasına sızmadan (uv yuva sınırına kırpılır; komşu yuva bekçisi yeşil kalır). Büyütmeyen efektler bit bit aynı.
- [ ] `CLAUDE.md`'nin emoji blend cümlesi (R10): metin RGB kaynak çarpanı `One` diyor, kod `SourceAlpha` — kodla hizala (hangisi doğruysa; ön çarpımlı bayt iddiasını da doğrula).
- [ ] Doğrulama geçti (`make hepsi` + `make shader`)
