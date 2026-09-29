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

_Kısıt: yazılan/taşınan kodun yorumları, doc-comment'leri ve tanı metinleri İngilizce (plan.md → Yaklaşım, dil kısıtı)._

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

- [ ] Yazılan/taşınan kodun yorumları ve tanı metinleri İngilizce
- [ ] `cell.wgsl`: glyph, kural, `emoji_fragment`; vertex paylaşımı
- [ ] Atlas dokuları (maske + tembel renk), `write_texture` yüklemesi, tek `prepare`
- [ ] Encode sırası ve üç viewport
- [ ] Kâhin listesine grubun sahneleri
- [ ] Kâhin listesine **blok şeridi** sahnesi (phase-2'den devir: şerit bugün `RuleCell` / chevron sprite, `cell` pipeline'ından çiziliyor; `cell_bg` + caret denemesinde çizilemedi)
- [ ] Grubun bekçi ikizleri
- [ ] Paylaşılan wgpu device'ında error scope yığını thread başına değil device başına: paralel sınamada bir doğrulama hatası başka sınamanın `pop`'una düşebilir — scope'lu çizimleri sıraya sok ya da paylaşımı bırak (phase-2'den not)
- [ ] Doğrulama geçti (`make hepsi` + `make shader`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
