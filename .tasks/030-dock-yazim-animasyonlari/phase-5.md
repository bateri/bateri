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

- [x] Yedi shader dalı (parçalı ikisi dahil)
- [x] `EraseFx` kolları, tohum, şişme payı
- [x] `NAMES`, şablon, popup başlıkları, `docs/AYARLAR.md`
- [x] Test: R5 döngüsü sekiz efektte, `t = 0` eşitliği, tohum kararlılığı, ayrıştırma
- [x] Ölçeklenen karelerde pürüz (phase-4'ün offscreen karelerinde görüldü, `recede` dahil): `glyph_fx` ölçekli örneklemede **doğrusal** filtreye geçsin, atlasın komşu yuvasına sızmadan (uv yuva sınırına kırpılır; komşu yuva bekçisi yeşil kalır). Büyütmeyen efektler bit bit aynı.
- [x] `CLAUDE.md`'nin emoji blend cümlesi (R10): metin RGB kaynak çarpanı `One` diyor, kod `SourceAlpha` — kodla hizala (hangisi doğruysa; ön çarpımlı bayt iddiasını da doğrula).
- [x] Görünürlük devri (orkestratör, kullanıcı bildirimi: "animasyonlar hiç belli olmuyor"): süreler 240/300 ms, yumuşak eğri, genlikler, efekt caret'in üstünde, kırpılmayan viewport
- [x] Doğrulama geçti (`make hepsi` + `make shader` + `make duman`)

## Uygulama Notları

- **Kimlikler:** `recede` 16'da kaldı, yedi yeni ad Karar 6'nın sırasıyla
  17–23 (`glyph_fx::Effect`); shader'da hayaletin `echo`'su `FX_GHOST_ECHO`.
- **Tohum sütundan değil sıradan** (phase-2'nin kararı, `Fx::seed`'in
  doc'u); kararlılığın iki tanığı: `a_seed_is_fixed_for_the_life_of_an_entry`
  (ilerleme ve kayma tohuma dokunmuyor) ve
  `a_shattered_glyph_breaks_the_same_way_every_frame` (aynı tohum aynı kare,
  başka tohum başka kırılma — shader'ın `fx.z`'yi okuduğunun tek tanığı).
- **Şişme payı `frame.rs`'te değil shader'da** (phase-4'teki gibi): `FX_PAD`
  1 → 1.5, en uzak noktalar `echo`'nun geniş glyph'teki kopyası ve
  `shatter`'ın düşen parçası.
- **`shatter` k×k değil 2×2** (geniş glyph'te 3×2): hücre 1:2 ve tek sütun
  sayısı ortadaki parçayı dikişin üstüne koyuyor, geniş glyph tek kutu gibi
  kırılıyor. Hareket balistik (dağılma `t`, düşüş `t²`), parçalar arası
  çatlak bilinçli; üst üste binen parçalarda "ilk isabet" değil en koyusu.
- **Doğrusal örnekleme** yalnız ölçekleyen/döndüren dallarda
  (`paint`'in `smooth`'u); kayan dallar ve uçlar `nearest` — `t = 0`
  hayaleti için ayrı bir özdeşlik kolu gerekti (`center + (g-center)/1`
  bit bit `g` değil). Renk düzleminde donanım süzgeci değil elle ön çarpımlı
  dört texel (baytlar düz alfa). `echo`'da yalnız kopya doğrusal.
- **Emoji blend cümlesi** c0b9571'de zaten hizalanmıştı ("düz alfa …
  `SourceAlpha`"); `raster::unpremultiply` ve pipeline kodu yeniden
  doğrulandı, düzenleme gerekmedi.
- **Görünürlük devri** (orkestratörün eklediği iş, ölçülmüş teşhis:
  kübik eğri değişimi başa yığıyordu, efekt caret bloğunun içinde
  oynuyordu, genlikler küçüktü): `KEYPRESS_DURATION` 0.12 → 0.24,
  `ERASE_DURATION` 0.16 → 0.30; `ease_out` kübikten **karesele**;
  `ECHO_ALPHA` 0.7 (kopya kalanın karesiyle sönüyor, yoksa ilk kareler kalın
  bir harf gibi okunuyordu), `ECHO_SCALE` 2.2, `RECEDE_SCALE` 0.3,
  `POP_START` 0.3, `RISE` 0.4, `DROP` 0.5, `SQUEEZE` 0.4×1.45.
  **Efekt caret'in üstünde, kendi renginde**: `glyph_fx` `CursorBlock`
  okumuyor (R4'ten sapma; encode sırası zaten caret'ten sonraydı, eksik olan
  ters çevirmenin kalkmasıydı). **Kendi viewport'u** (pencere uzayı):
  offscreen karelerde `drop`'un ilk kareleri ve `sublime` dock bandının
  tepesinde kesiliyordu — phase-4'ün `drop`'unda da vardı, döküm tek satırlık
  dock'la bakıldığı için görülmemişti. Bedeli: bu iki efekt saç çizgisini
  kısa bir an aşıyor (`docs/AYARLAR.md` söylüyor).
- **Komşu yuva bekçisi yeniden yazıldı**: eski ölçüt ("`.`'nın hücresinin
  dışı sızıntıdır") genliği `.`'nın hücre içindeki boşluğuna bağlıyordu ve
  büyüyen genlikler onu aştı. Yenisi genlikten bağımsız: aynı efekt dolu
  komşulu ve tek başına iki atlasta aynı kareyi vermeli (tolerans 2/255).
  Mutasyonla doğrulandı: sınır testi ve kırpma kaldırılınca 121 ayrışıyor.
- **Offscreen karelerde görülen ve ayarlanan** (16pt@2x, t = 0…1, iki
  satırlık dock; geçici döküm depoya girmedi): `bleed`'in 3×3 ızgarası
  büyük yarıçapta dokuz kopya gösteriyordu → 16 örnekli Vogel diski
  (`spread`, `sublime` da kullanıyor); `iris` kutunun köşesinden
  başlayınca sürenin üçte biri boş kapanıyordu → çap mürekkebin kabaca
  sınırından (`IRIS_REACH`); `undertow` görünürken yol almıyordu →
  `smoothstep` çekilme, geç sönme, hafif küçülme; `shatter` dağılıp duruyordu
  → balistik. `unravel`, `echo`, `recede`, `sublime` ilk hâlleriyle kaldı;
  hiçbirinde `t = 1`'de kalıntı, dikişte yarılma ya da kırpılma yok.
- **Gerçek pencerede** (geçici paket, açık tema, `pop` + `shatter`): silinen
  harfin parçaları caret bloğunun üstünde kendi renginde kırılıp düşüyor;
  hızlı yazımda üç geliş aynı anda uçuşta, satır titremiyor, son hâl temiz.

## Set kapısı

- **`/code-review`** (set aralığı + çalışma ağacı): üç doğruluk bulgusu
  giderildi — geliş gizlediği statik glyph'in **şimdiki** hücresiyle
  çiziliyor (vurgu uçuşta değişince renk sıçramıyordu; tanık
  `an_arrival_wears_the_static_glyphs_current_color`), uçuştaki bir gelişi
  silmek hayalet doğurmuyor (yarı belirmiş harf tam opağa sıçrıyordu; tanık
  `erasing_an_arrival_in_flight_leaves_no_ghost`), geometri değişimi
  uçuştakileri bitiriyor (`DisplayLink::resize`). `CLAUDE.md`'deki anlatı
  kısaltıldı, `cursor` rolünün tüketicilerine `heat` eklendi.
- **Waive:** taşan pencerenin sağ kenarında dar + geniş iki karakterin ileri
  silinmesinde ikinci hayaletin düşmesi (`dock.rs`, nadir ve yönü güvenli:
  animasyon eksik, harf kaybı yok); dört temizlik önerisi (`Ghosts` ile
  `EditCells`'in ortak sınırlı listesi, `Change`'in `old_skip`'i, `advance`'in
  iki çağrısı, hareket karesinde `dock_shown`'un yeniden kurulması) — davranış
  değiştirmiyor, ayrı bir sadeleştirme işi.
- **`/audit`:** mekanik yarı temiz; ölçüm merceğinde tek bulgu (shader
  yorumunda hayaletin süresi 240 ms diye geçiyordu, 300 ms) düzeltildi;
  ayar, thread, boşta kare, düzen ve dil mercekleri temiz, bağımlılık
  merceği ilgisiz.
