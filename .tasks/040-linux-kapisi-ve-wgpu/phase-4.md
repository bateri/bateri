# Phase 4 — `glyph_fx` + `selection`, tamamlanma modeli; bekçiler wgpu'ya döner

## Özet

Son iki pipeline grubunu, tamamlanma modelini ve GPU damgasını wgpu
renderer'ına taşımak, ardından `render_offscreen`'i wgpu'ya çevirmek. Phase
sonunda `renderer.rs`'in bütün bekçileri wgpu'da koşuyor ve Metal yalnız
kâhin sahne listesinde kalıyor. wgpu hâlâ `cfg(test)`.

_Requirements: R3.2, R3.3, R3.4_

_Kısıt: yazılan/taşınan kodun yorumları, doc-comment'leri ve tanı metinleri İngilizce (plan.md → Yaklaşım, dil kısıtı)._

## Değişiklikler

### Kalan shader'lar

- **`crates/bt-gpu/shaders/glyph_fx.wgsl` (yeni)** — `glyph_fx.metal`'in
  karşılığı.
  - Kendi vertex'i ve 48 baytlık instance'ı var.
  - Ters dönüşüm glyph uzayına çeviriyor. İki doku da bağlı, düzlem
    instance'tan geliyor.
  - Ölçekleyen dallar doğrusal örnekliyor ama nokta **texel merkezine
    kırpılıyor**, yani süzgeç komşu yuvaya değmiyor (`CLAUDE.md` → 030
    paragrafı; bekçisi dolu/boş komşulu iki atlasın aynı kareyi vermesi).
  - `heat`'in kızgın rengi kare başına tek değer.
  - `t = 1`'de statik glyph'le piksel piksel aynı olma kapısı korunuyor.
- **`selection` pipeline'ı** (`cell_bg.wgsl`'e ya da ayrı dosyaya).
  - `Instance`'ı **aynen** okuyan kendi vertex'i; `rgba` yuvası köşe maskesi.
  - Renk ve yarıçap tek değer.
  - Arama vurgusu aynı pipeline'dan, rol başına bir encode. Sıra: zemin →
    eşleşme → geçerli eşleşme → seçim → caret → glyph.

### Tamamlanma modeli (`discussion.md` → Karar 6)

- **Gönderim indeksi + tik başında engellemeyen `poll`.** Kare başına
  closure kurulmuyor.
- **`kare=`** yalnız hatasız biten indeksleri sayıyor. Hata error scope'tan ve
  uncaptured/device-lost geri çağrısından geliyor.
- **Hata** `Retry::draw_failed`'in politikasına gidiyor: senkron ve asenkron
  tek yol.
- **`acilis=`** (`Stats::mark_startup`) ilk karenin **bittiği** anı ölçüyor.
- **Uykudan önceki bitmemiş kare** tek bir engellemeyen poll'u gecikmeli
  uyandırmaya kuruyor. Durma koşulu: kuyruk boş. Bağlantısı phase-5'te
  `Pacer` gelince kuruluyor; bu phase yalnız renderer tarafını ve sınamasını
  veriyor.
- **GPU damgası.** `TIMESTAMP_QUERY` varsa pass başı/sonu damgası çözülüp
  eşzamansız okunuyor ve `Stats::record_gpu`'ya veriliyor. Yoksa değer
  `unsupported`, jeton anahtarı kalıyor. Jetonun basıldığı yer
  (`bt-shell` → `Report::token_line`) bu phase'te değişmiyorsa değer eşlemesi
  phase-5'e not.

### `render_offscreen` wgpu'ya

- `renderer.rs`'in sınama modülündeki ortak gövde wgpu'yu çağırıyor.
- phase-2/3/4 ikizleri asıllara katılıyor, ikizlik kalkıyor.
- Doğrudan Metal'e inen sınamaların wgpu karşılığı yazılıyor (tamamlanma,
  atlas dokusu, elle kurulan pass). Metal hâlleri kâhin modülüne ya da
  silinmeye.
- **Sınamaların iddiaları değişmiyor.** Bir bekçi wgpu'da yalnız eşiği ya da
  beklenen değeri değişerek geçiyorsa bu bir bulgu: Uygulama Notları'na ve
  kâhin sahnesine yazılıyor, sessizce gevşetilmiyor.

### Kâhin sahne listesi

Eklenen sahneler: yazım efektlerinden birer geliş ve hayalet (`t` ortası ve
`t = 1`), seçim köşeleri (dışbükey/içbükey/basamak), arama eşleşmesi +
geçerli eşleşme, odaksız soluk seçim, doldurma bandı + dock ile üç viewport'lu
tam bir kare.

## Kabul

- `make hepsi` ve `make shader` yeşil. `renderer.rs`'in bütün bekçileri
  (bugün 60) wgpu'da geçiyor.
- Kâhin sahne listesi bütün pipeline'ları kapsıyor ve toleransta.
- Tamamlanma sınamaları geçiyor:
  - hatasız kare sayılıyor, hatalı kare sayılmıyor;
  - `acilis` bitiş anından;
  - uykudan önceki kare tek bir poll'la sayılıyor ve kuyruk boşalınca poll
    kurulmuyor.
- Ürün grafı hâlâ wgpu'suz.

## Checklist

- [x] Yazılan/taşınan kodun yorumları ve tanı metinleri İngilizce
- [x] `glyph_fx.wgsl` (texel merkezine kırpma, `t = 1` eşitliği)
- [x] `selection` pipeline'ı + arama vurgusu, encode sırası
- [x] Tamamlanma modeli: indeks + poll, `kare=` hatasız, `Retry`, `acilis=`, uykudan önceki kare
- [x] GPU damgası `TIMESTAMP_QUERY` / `unsupported`
- [x] `render_offscreen` wgpu'ya; ikizler birleşti; Metal'e doğrudan inen sınamaların karşılığı
- [x] Kâhin sahne listesi tamam
- [x] `WgpuRenderer`'ın Metal `Renderer`'dan eksikleri (phase-3'ten devir): `set_font`/`font_notice`, `atlas_occupancy`/`color_atlas_occupancy`, `last_*_count` sayaçları — bugün yalnız varsayılan font; `atlas_occupancy_is_republished` ve `rebuilding_the_atlas_drops_both_textures` wgpu'da koşunca gerekiyor
- [x] `AtlasTexture`'ı elle kuran sınamalar (`a_wide_cell_becomes_two_quads`, `a_color_glyph_goes_to_the_color_list`, küme sınaması) `slots::glyph_lists`'e bir sınama `SlotUpload`'ıyla inmeli — Metal sökülünce onları taşıyan doku kalmıyor (phase-3'ten devir)
- [x] Doğrulama geçti (`make hepsi` + `make shader`)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi

## Uygulama Notları

- **`acilis=` bitiş anını değil poll'un gözlediği anı ölçüyor — sapma.**
  Native wgpu tamamlanmayı yalnız `device.poll`'un içinde, poll eden
  thread'de bildiriyor; Karar 6 ayrı thread'i yasaklıyor. `mark_startup`
  (phase-5'te çağıranın işi) `poll`'un ilk `Ok`'unda kapanıyor; gecikme en
  çok bir tik ya da bir gecikmeli poll. Sınama iddiası buna göre: submit'te
  `None`, poll'dan sonra `Some`.
- **Engellemeyen poll = sıfır süreli `Wait`.** Düz `PollType::Poll` yalnız
  "kuyruk boş mu" diyor ve cihaz paylaşımlı; indeks başına
  `Wait { submission_index, timeout: Some(ZERO) }` (wgpu-hal Metal'in
  `wait`'i sıfırda condvar'da uyumadan dönüyor, okundu). `Err(Timeout)` →
  bitmedi.
- **Hatanın iki kolu:** senkron — `draw` encode+submit'i Validation +
  OutOfMemory scope'larıyla sarıyor, hata `Err(GpuError::Wgpu)` ve kare hiç
  izlenmiyor (sayılamaz). Asenkron — uncaptured handler ve device-lost
  callback'i paylaşılan `Fault` nesline yazıyor; submit anındaki nesilden
  yeni bir arıza o karenin `poll` sonucunu `Err` yapıyor (tüketmeden: pane'ler
  aynı device'ı paylaşacak). `GpuError::Wgpu(String)` yeni varyant (ürün
  enum'u, metin İngilizce). Senkron kolun sınaması için `#[cfg(test)]`
  `poison_next_frame` (hedef dışı makas); asenkron kolun sınaması kendi
  device'ında (sızmasın diye `Box::leak`'li ikinci `Gpu`).
- **GPU damgası** `TIMESTAMP_QUERY` varsa istenen özellik; pass başı/sonu
  damgası resolve + readback, `map_async` bitmiş karede `poll` içinde. Map
  yalnız callback'le bildirildiği için damgalı kare başına bir closure var —
  yol yalnız ölçüm kapısı açıkken (`set_gpu_timing`). Bu makinede destekli;
  ölçüm kancası artık wgpu'nun `gpu_p95`'ini de basıyor (sayı yazılmadı,
  ölçüm kapı değil). Jetonun `unsupported` eşlemesi phase-5'te
  (`Report::token_line` bu phase'te değişmedi).
- **Seçim `cell_bg.wgsl`'de ve caret'in immediates bloğunu paylaşıyor**:
  `core` = renk, `shape.x` = yarıçap. İkinci bir pipeline layout'u
  kurmamak için; anlam `Immediates`'in doc'unda.
- **`glyph_fx.wgsl`:** her örnekleme `textureSampleLevel(.., 0)` (WGSL
  türevli örneklemeyi yalnız tekdüze akışta kabul ediyor, `paint` veriye
  bağlı dallardan çağrılıyor; atlasın tek mip'i var, sonuç aynı).
  `select((t < 1.0), (t > 0.0), ..)` parantezli — parantezsiz hâli şablon
  listesi diye ayrıştırılıyor. Efekt bind group'u (iki düzlem + nearest +
  linear sampler) atlasta, "renk dokusu var mı" anahtarıyla önbellekte ve
  planlamadan **sonra** çözülüyor (aynı karenin sonraki listesi renk dokusunu
  kurmuş olabilir).
- **Efekt instance'ı `slots::fx_list`'e taşındı** (`prepare_fx`'in gövdesi;
  Metal de onu çağırıyor) ve `fx_is_color` paketin tek okuyucusu.
- **Sınama göçü:** `renderer.rs`'in bekçileri `renderer()` /
  `render_offscreen(&WgpuRenderer, ..)` üstünden wgpu'da koşuyor; Metal
  gövdesi `metal_offscreen` adıyla yalnız kâhin sahnelerinde ve ölçüm
  kancasında. Hiçbir eşik ve beklenen değer değişmedi — `t = 1` bit
  eşitliği, 2/255 komşu yuva ve seçim/arama bekçileri wgpu'da ilk koşuda
  geçti. Metal'e doğrudan inenler: iki tamamlanma sınaması wgpu tamamlanma
  sınamalarına (dört yeni sınama) döndü; `AtlasTexture`'ı elle kuran dördü
  (`a_wide_cell_*` ikisi, küme, renk listesi) kaydeden bir `SlotUpload` ile
  `slots::glyph_lists`'e indi ("renk dokusu kuruldu" iddiası "renk düzlemine
  yükleme yapıldı" oldu); renk dokusu formatı `plane_format` ile wgpu'dan;
  Metal'in iki emoji round-trip'i silindi, wgpu ikizleri asıl adlarını
  aldı; atlassız kare `try_submit_offscreen` ile. phase-2'nin altı ikizi
  asıllarına katılıp silindi; asılsız iki bekçi (geniş glyph dikişi, `█`
  üstünde kural) kaldı. `metallib_is_embedded_and_valid` Metal'in (phase-7).
- **Kâhin sahneleri:** seçim köşeleri (dışbükey/içbükey/basamak), odaksız
  soluk seçim, arama (eşleşme, geçerli, sarılan, bant, üstünde seçim), üç
  viewport'lu tam kare (ızgara + bant + dock seçimi, caret, uçuşta geliş) ve
  bütün gelişler/hayaletler `t = 0.5`'te + birer tanesi `t = 1`'de; hepsi tam
  eşit/≤ 1/255. Duyarlılık sınandı: WGSL'de `SHATTER_SPIN` 0.8→0.9
  (`ghost 23` 231/255) ve içbükey yarıçap ×0.9 (köşe sahnesi 33/255) düştü.
- Ürün grafı wgpu'suz (`cargo tree -p bateri -e normal` → 0), `Cargo.lock`
  değişmedi.
- `/code-review` (medium) iki bulgu, ikisi de giderildi: (1) arıza kolunda
  `Timing` readback'i `map_async` ortasındayken havuza dönüyordu ve sonraki
  her damgalı kare doğrulamada düşerdi — hatalı karenin `Timing`'i artık
  atılıyor; (2) Metal hâlâ ürün renderer'ı olduğu için iki tamamlanma
  bloğu sınaması ve `encode_pass`'in hata yolunda `endEncoding` bekçisi
  (`metal_renderer_without_atlas_refuses_glyphs_and_still_ends_encoding`)
  geri kondu; Metal'le birlikte phase-7'de gidecekler.

