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

- [x] Sekiz shader dalı
- [x] `KeypressFx` kolları, `heat` rengi, şişme payı
- [x] `NAMES`, şablon, popup başlıkları, `docs/AYARLAR.md`
- [x] Test: R5 döngüsü dokuz efektte, ayrıştırma
- [x] Doğrulama geçti (`make hepsi` + `make shader`; `make duman` yeşil)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi (bulgu yok; not: 1x ekranda ince fontta `ink` 1 px çizgileri sürenin ilk üçte birinde gizli tutuyor — gözle yargılanacak tasarım seçimi)

## Uygulama Notları

- **`KeypressFx` yok** (phase-3 tek enum'a geçti): kollar `bt-core`'un
  `Keypress`'inde, kimlikler `glyph_fx::Effect`'te (1..9, Karar 6'nın
  sırası), shader'da `FX_*`.
- **`heat`'in rengi `GlyphFx`'ten değil `Frame`'den:** kare başına tek bir
  uniform (`Frame::dock_fx_heat`, `glyph_fx.metal` → `buffer(3)`); yazarı
  listelerle aynı `set_dock_fx(fx, heat)`, iki kare yolu temayı zaten tutuyor.
  `CursorBlock.rgba` imlecin rengi değil altındaki metnin rengi, o yüzden
  kullanılamadı. Bloğun altında kızgın renk imlecin metin rengine dönüyor
  (blokla aynı renkte harf görünmezdi).
- **Şişme payı shader'da kaldı** (`FX_PAD = 1`, `frame.rs`'te değil): en
  büyük taşma `echo`'nun geniş glyph'teki kopyası (0,8 hücre); doc'u söylüyor.
- **Renk düzlemi (emoji):** `heat` boyamıyor, `ink` eşiklemiyor — ikisi de
  düz belirmeye düşüyor (renk dokudan, `emoji_fragment`'in kuralı).
- **Offscreen karelerde görülen ve ayarlanan** (16pt@2x, t = 0…1, geçici
  `#[ignore]` döküm sınaması; depoya girmedi): `heat` Karar 6'nın eğrisiyle
  ilk çeyrekte soğuyordu → `smoothstep`; `ink`'in cephesi ilk karede
  bitiyordu → zamanda doğrusal; `extrude`'un ilk karesi `nearest`
  örneklemede kesik bir çizgiydi → belirme eklendi; `drop` 0,35 hücreden
  zor seçiliyordu → 0,45. `pop` (≈1,08 tepe), `rise`, `echo`, `squeeze`
  ilk hâlleriyle kaldı; büyüyen karelerde `nearest`'in pürüzü görülüyor
  (`recede` emsali, 120 ms). Hiçbirinde kenar kırpılması ya da `t = 1`'de
  sıçrama yok; `漢` ve `🎉` tek parça.
- **Gerçek pencerede** (geçici paket, `pop` ve `echo`): yazım ve son hâl
  temiz, komşu harfler bozulmuyor; 120 ms'lik ara kareler ekran
  görüntüsüyle yakalanamıyor.
- **Komşu yuva sınaması aynı kaldı**, doc'u `.`'nın seçimini açıklıyor:
  kayan/büyüyen efektler hücreyi meşru olarak taşırıyor, `.`'nın mürekkebi
  bugünkü genliklerde içeride kalıyor. Geniş glyph sınaması gelişlere de
  genişledi (`a_wide_glyph_transforms_as_one_box`); `heat`'in uniform'unu
  `heat_starts_in_the_cursor_color` bekliyor.
- Bilinmeyen anahtar tanığı `keypress = "pop"` → `"bounce"`.

