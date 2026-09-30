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

- [x] Yazılan/taşınan kodun yorumları ve tanı metinleri İngilizce
- [x] `cell.wgsl`: glyph, kural, `emoji_fragment`; vertex paylaşımı
- [x] Atlas dokuları (maske + tembel renk), `write_texture` yüklemesi, tek `prepare`
- [x] Encode sırası ve üç viewport
- [x] Kâhin listesine grubun sahneleri
- [x] Kâhin listesine **blok şeridi** sahnesi (phase-2'den devir: şerit bugün `RuleCell` / chevron sprite, `cell` pipeline'ından çiziliyor; `cell_bg` + caret denemesinde çizilemedi)
- [x] Grubun bekçi ikizleri
- [x] Paylaşılan wgpu device'ında error scope yığını thread başına değil device başına: paralel sınamada bir doğrulama hatası başka sınamanın `pop`'una düşebilir — scope'lu çizimleri sıraya sok ya da paylaşımı bırak (phase-2'den not)
- [x] Doğrulama geçti (`make hepsi` + `make shader`)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi

## Uygulama Notları

- **Paylaşılan kod `slots` modülünde** (`crates/bt-gpu/src/slots.rs`): `fan`,
  `slot_uv`, `Part`, `SlotAsk` `renderer.rs`'ten taşındı (yorumları İngilizce)
  ve `prepare`'in liste doldurma gövdesi `glyph_lists` oldu. Arka uca özgü tek
  şey yükleme hedefi: `SlotUpload` trait'i, iki gerçeklemesi `MetalUpload`
  (`replaceRegion` + `ColorPlane`) ve `WgpuUpload` (`write_texture` + tembel
  renk dokusu, yükleme anında). Satır adımı/uzunluk tek kopya
  (`slots::slot_layout`). `prepare_fx` de `slots::fan`'dan geçiyor.
- **wgpu renderer ikiye bölündü — sapma.** `bt_atlas::Atlas` CoreText fontu
  tuttuğu için `Send`/`Sync` değil ve paylaşılan `static` onu taşıyamadı.
  `Gpu` (device, queue, dört pipeline, bind group layout, sampler) süreç
  başına bir ve paylaşılıyor; `WgpuRenderer` (atlas + iki instance tamponu,
  `RefCell`) test başına kuruluyor — Metal'in `Renderer`'ı gibi. Kazanç: atlas
  anahtarı renderer başına, yani phase-4'te 55 bekçi kendi ölçeğiyle
  koşabilir; ürün şekli de bu (bir device, pane başına renderer). Tampon
  kilidine gerek kalmadı: tamponlar renderer'ın, kuyruğun her `submit`'i
  bekleyen yazımları yalnız **erken** indirebilir.
- `CellMetrics::from_atlas`: `Renderer::cell_metrics`'in pay aritmetiği tek
  kopyaya çekildi (yorumları İngilizce), wgpu ikizi de onu çağırıyor. wgpu
  renderer bugün yalnız varsayılan fontu açıyor (`set_font` yok).
- `cell.wgsl`'in `Immediates`'i 64 bayt (≤ 128): `CursorBlock` aynen gömülü,
  sonra viewport/cell/uv_size, sonda açık `pad`. Glyph pipeline'larının tek
  bind group'u (doku + nearest/clamp sampler), düzlem başına doku ile birlikte
  kuruluyor. `GlyphInstance` düzeni `frame.rs`'teki
  `GLYPH_INSTANCE_OFFSETS`'ten.
- **Error scope maddesi olguyla kapandı:** wgpu 30'da `std` özelliğiyle scope
  yığını **thread-local** (`Device::push_error_scope`'un doc'u) ve hata çağıran
  thread'de doğuyor; sınamalar ayrı thread'lerde. Kod değişmedi, gerekçe
  `render_offscreen`'in doc'unda.
- Kâhin sahneleri `SCALE = 2.0`'da (13pt@1x'te bayrak kümesi taban karaktere
  düşüyor): dört yüz, alt çizgi aileleri + üstü çizili + SGR 58, blok şeridi,
  yordamsal karakterler, geniş CJK (iki yarı + tek hücreye sığan `☕`), renkli
  emoji + iki küme, ters video + caret altındaki glyph, doldurma bandı
  (negatif orijin), dock (chevron, giriş glyph'leri, caret, küçük bağlam
  satırı), büyüyen bant + glyph'ler (makas caret'ten sonra geri kuruluyor).
  Sahnelerin tamamı tam eşit/≤ 1/255 geçti. Duyarlılık bir kez sınandı:
  `emoji_fragment` instance rengini döndürünce emoji sahnesi 255/255 farkla
  düştü.
- İkizler: renk dokudan (sentetik ara ton yuva, instance rengi kırmızı) ve
  yarı saydam kenarın lineer kompoziti (0xBC); `一` ile iki yarının dikişsiz
  birleşmesi (sınır sütunları satır satır ±2); yordamsal `█` üstünde kırmızı
  alt çizgi (kural glyph'ten sonra).
- `/code-review` (medium): doğruluk bulgusu yok (düzenler, vertex ofsetleri,
  encode sırası, tembel renk dokusu, Metal refactor'ının davranışı teyitli).
  İki not: wgpu `Plan`'ı kare başına kuruluyordu, yani `glyph_lists`'in
  "kapasite kareden kareye kalır" sözü wgpu'da tutmuyordu — düzeltildi, plan
  renderer'ın `State`'inde yaşıyor ve `clear`'la yeniden kullanılıyor.
  `render_offscreen` planlama hatasında scope açıkken `expect`'le düşüyor —
  yalnız sınama kodu ve thread zaten ölüyor; bırakıldı.
- Ürün grafı wgpu'suz (`cargo tree -p bateri -e normal` → 0 satır);
  `Cargo.lock` değişmedi.
