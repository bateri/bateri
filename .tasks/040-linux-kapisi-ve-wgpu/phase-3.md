# Phase 3 — `cell` + `emoji`: atlasın iki düzlemi wgpu'da

## Özet

`cell` pipeline'ını (glyph'ler ve kurallar) ve `emoji` fragment'ini wgpu
renderer'ına taşımak. Kapsam:

- atlasın iki düzleminin wgpu dokuları;
- yüklemeleri;
- geniş glyph'in iki yarısı;
- sahneleri kâhin listesine eklemek.

wgpu hâlâ `cfg(test)` / dev-dependency.

_Requirements: R3.1_

## Değişiklikler

- **`crates/bt-gpu/shaders/cell.wgsl` (yeni)** — `cell.metal`'in karşılığı:
  - `cell_vertex`, glyph ve kural fragment'i, `emoji_fragment`.
  - Emoji `cell`'in vertex'ini **aynen** paylaşıyor; ayrılan yalnız fragment
    (bugünkü sözleşme, `CLAUDE.md` → pipeline listesi).
  - Maske `R8Unorm`, renk `Rgba8UnormSrgb`. Renk düzlemi **düz alfa**, blend
    altı pipeline'da `SourceAlpha` (bugünkü `raster::unpremultiply`
    sözleşmesi değişmiyor).
- **wgpu renderer modülü (phase-2'nin iskeleti)**
  - Atlas dokuları ve `replaceRegion` → `Queue::write_texture` karşılığı.
  - Doku kenarı `bt-atlas`'tan geliyor (değişmiyor). Renk dokusu **tembel**:
    ilk renkli yuvayla doğuyor.
  - `AtlasTexture::prepare`'in yelpazelemesi ve uv pişirme **tek yerde**
    kalıyor. İki renderer aynı `prepare` çıktısını okuyor; ikinci bir kopya
    yazılmıyor. Paylaşım için gereken ayrım kodlayanın, ama
    düzlemin monoton sayacının anlamı değişmiyor (`CLAUDE.md` → "Renk ikinci
    bir düzlem").
  - Encode sırası bugünküyle aynı: zemin → glyph → **sonra** kurallar.
    Izgara, doldurma bandı ve dock için üç `set_viewport`.
- **Kâhin sahne listesi** — eklenen sahneler:
  - dört yüz (düz/kalın/eğik/kalın-eğik);
  - alt çizgi aileleri ve üstü çizili;
  - prompt chevron'u;
  - yordamsal bir blok/çizgi karakteri;
  - geniş CJK glyph'i (iki yarı);
  - renkli emoji ve bir emoji kümesi;
  - bağlam satırının küçük boy sınıfı;
  - ters video.
- **Grubun özgül bekçi ikizleri** — renkli glyph'in rengi dokudan gelir,
  instance'tan değil; iki yarının dikişsiz birleşmesi; kuralın glyph'ten
  sonra çizilmesi. Bugünkü karşılıkları `renderer.rs`'in sınamaları.
  İkizler phase-4'te asıllara katılıyor.

## Kabul

- `make hepsi` ve `make shader` yeşil.
- Kâhin sahne listesinin yeni sahneleri Karar 4'ün toleransında: düz alanlar
  tam, AA kenarı ≤ 1/255.
- Ürün grafı hâlâ wgpu'suz (`cargo tree -p bateri -e normal`).

## Checklist

- [ ] `cell.wgsl`: glyph, kural, `emoji_fragment`; vertex paylaşımı
- [ ] Atlas dokuları (maske + tembel renk), `write_texture` yüklemesi, tek `prepare`
- [ ] Encode sırası ve üç viewport
- [ ] Kâhin listesine grubun sahneleri
- [ ] Grubun bekçi ikizleri
- [ ] Doğrulama geçti (`make hepsi` + `make shader`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
