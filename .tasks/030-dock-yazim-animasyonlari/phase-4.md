# Phase 4 — Keypress'in kalan sekiz efekti

## Özet

`rise`, `pop`, `extrude`, `heat`, `echo`, `drop`, `ink`, `squeeze` —
`discussion.md` → Karar 6'daki tanımlarla — shader'da ve ayarda.

_Requirements: R8, R5_

## Değişiklikler

- **`crates/bt-gpu/shaders/glyph_fx.metal`** — sekiz geliş dalı. Geometri
  vertex'te ya da ters dönüşümde (kayma: `rise`, `drop`; ölçek: `pop`,
  `squeeze`; sol kenara sabit yatay ölçek: `extrude`), renk/kapsama
  fragment'te (`heat`: temanın `cursor` rengi uniform ya da instance'tan,
  kendi rengine karışım; `ink`: kapsama eşiği çekirdekten kenara). `echo`
  aynı dörtlüde ikinci bir örnekleme: glyph + büyüyen soluk kopya, şişme payı
  kopyanın tavanını taşır. Genlikler hücre oranında, taşmalar kapalı formdan;
  her dal `t = 1`'de statik yola iner.
- **`crates/bt-gpu/src/glyph_fx.rs`** — `KeypressFx`'e sekiz kol; `heat`'in
  rengi için temanın `cursor`'ı fx'e girer (`LinkIvars::theme`'den).
- **`crates/bt-gpu/src/frame.rs`** — şişme payı en büyük efektin taşmasını
  karşılar (sabit, doc'unda hangi efektten geldiği).
- **`crates/bt-core/src/settings.rs`** — `Keypress::NAMES`'e sekiz ad;
  `TEMPLATE` yorumu.
- **`crates/bt-shell/src/settings_window.rs`** — `Choice` başlıkları.
- **`docs/AYARLAR.md`** — değer listesi ve her efektin tek cümlesi.

## Kabul

- R5 değişmezleri dokuz geliş efektinin hepsinde (döngü yeni kolları
  kendiliğinden kapsıyor): `t = 1`'de statik glyph'le piksel piksel aynı,
  komşu yuva örneklenmiyor, geniş glyph tek kutu.
- Ayrıştırma sınaması sekiz yeni adı tanıyor.
- Gözle: her efekt dock'ta yazarken tanımıyla uyuşuyor, emoji ve CJK'de
  ikiye bölünmüyor, hızlı yazımda yan yana harfler birbirini bozmuyor.

## Checklist

- [ ] Sekiz shader dalı
- [ ] `KeypressFx` kolları, `heat` rengi, şişme payı
- [ ] `NAMES`, şablon, popup başlıkları, `docs/AYARLAR.md`
- [ ] Test: R5 döngüsü dokuz efektte, ayrıştırma
- [ ] Doğrulama geçti (`make hepsi` + `make shader`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
