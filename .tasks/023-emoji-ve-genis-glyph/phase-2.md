# Phase 2 — Renkli emoji

## Özet

Emoji `Atlas`'ın içinde ikinci bir `RGBA8Unorm_sRGB` düzlemde yaşar ve
`cell_vertex`'i aynen paylaşan kardeş bir fragment'le çizilir.

_Requirements: R5, R5.1, R5.2, R5.3, R5.4, R6, R6.1, R6.2, R7.2, R8_

## Değişiklikler

- **`crates/bt-atlas/src/lib.rs`** — `Atlas`'ın **içinde** ikinci düzlem.
  İkinci bir `Atlas` **açılmıyor**: o beş CoreText türetmesini (dört yüz +
  `small`) ve aynı anahtardan ikinci bir `Metrics`'i doğururdu ve `sync_atlas`
  tam bunu önlemek için var. Yuvalar yine **hücre boyunda**, yani phase-1'in
  `Half` mekanizması geniş emojinin geometrisini de çözüyor — ikinci bir
  geometri kavramı yok. Düzlem kendi **monoton** `next`'ini tutuyor: uv
  `prepare` anında pişiyor ve dördüncü geçiş koşarken ilk üçün tamponları
  zaten encode edilmiş, yani kare ortasında anlamı değişen paylaşımlı atlas
  durumu yasak. `slot_bytes` **format-duyarlı** olmak zorunda — doc'u "yuva
  geometrisinin tek sahibi burası" diyor ve RGBA yuva `4·w·h`.
- **`crates/bt-atlas/src/raster.rs`** — ikinci bir CG reçetesi: ön çarpımlı
  RGBA + renk uzayı. Bugünkü bağlam **alfa-only** (`space: None`,
  `CGImageAlphaInfo::Only`, satır adımı `w`) ve renk üretemiyor; maske
  reçetesi olduğu gibi kalıyor.
- **`crates/bt-atlas/src/font.rs`** — kapı, adayın renkli olup olmadığını
  **sormuyor**: ölçüt hâlâ geometrik. Renkli aday da phase-1'in sırasından
  geçiyor, ayrıştığı tek yer hangi düzleme rasterize edileceği.
- **`crates/bt-gpu/shaders/cell.metal`** — kardeş fragment. `cell_vertex`
  **aynen** paylaşılıyor (015'in `caret_fragment`'i `cell_bg_vertex`'i böyle
  paylaşıyor); `GlyphInstance` stride **32** ve `cell_px`/`uv_size`
  uniform'ları değişmiyor. `cell_fragment`'in "tek kanal kapsama, renk
  instance'tan" cümlesi hayatta kalıyor. Emojide caret'in `mix`'i **anlamsız**
  (rgb dokudan geliyor) ve emoji caret'ten sonra çizildiği için opak
  mürekkebinin altındaki caret örtülüyor, caret mürekkebin çevresinde bir
  halka olarak görünüyor — kabul edilen davranış, adıyla yazılı.
- **`crates/bt-gpu/src/renderer.rs`** — ikinci düzlemin dokusu
  `RGBA8Unorm_sRGB`; düz `RGBA8Unorm` paleti **sessizce** açar (hedef
  `BGRA8Unorm_sRGB` ve donanım fragment çıktısını lineer sayıyor).
  `upload_slot` `bytesPerRow`-duyarlı olmak zorunda: yorumu "ikisi ayrışırsa
  Metal kısa tamponun ötesini okur ve belirti sessizdir" diyor. Blend'de
  değişen **tek** çarpan RGB kaynağı (`One`, ön çarpımlı emoji için); alfa
  tarafı zaten doğru ve gerekçesi yazılı. Bu, `pipeline()`'ın 008 phase-5'te
  attığı `enum` parametresini geri alıyor — yazılı bir kararın geri alınması,
  gerekçesi bir cümleyle burada. **Reddedilen alternatif:** yükleme sırasında
  ön çarpımı geri almak — düşük alfada hassasiyet kaybı ve yuva başına bir CPU
  turu. Çizim sırası: arka planlar → caret → **emoji** → glyph + kural; ekleme
  **üç yüzeyde** ayrı ayrı (ızgara, doldurma, dock). `atlas.borrow_mut()`
  geçiş başına **tek** kalmak zorunda.
- **`crates/bt-shell/src/app.rs`** — duman jetonuna `yuva2=U/T`, `yuva=`'yi
  aynalayarak. "Jeton silinmez, eklenir"; göremediği bir düzlem 021'in
  Braille şekli olurdu.

## Kabul

- `echo 🎉📁🚀` renkli çiziliyor, iki hücre genişliğinde, ortalanmış.
- **Sentetik ara tonlu piksel tanığı**: bilinen ara tonlu bir RGBA yuvası
  yüklenip offscreen okunuyor. Gerçek emojiye bakan bir bekçi **yanlış güven
  verir** — bitmap CoreGraphics'ten geliyor ve macOS sürümleri arasında bit bit
  sabit değil. Ara ton şart, çünkü `0.0` ve `1.0` sRGB transfer fonksiyonunun
  sabit noktaları (`renderer::tests::MIDTONE` emsali).
- Ön çarpım tanığı: yarı saydam bir kenar pikselinde koyu halka yok.
- `hucre=`/`glif=`/`kural=` sayaçları oynamıyor, `yuva2=` basılıyor.
- `make shader` yeşil, `make hepsi` yeşil, `make duman` yeşil.

## Checklist

- [ ] `Atlas` içinde ikinci düzlem, kendi monoton `next`'i, format-duyarlı
      `slot_bytes`
- [ ] İkinci CG reçetesi (ön çarpımlı RGBA + renk uzayı)
- [ ] Kardeş fragment, `cell_vertex` paylaşımı, stride 32 korunuyor
- [ ] Doku `RGBA8Unorm_sRGB`, `upload_slot` `bytesPerRow`-duyarlı
- [ ] Blend RGB kaynağı `One`; `pipeline()`'ın parametresi ve 008 phase-5'in
      geri alınma gerekçesi yazıldı
- [ ] Çizim sırası **üç yüzeyde** kazanıldı (ızgara, doldurma, dock)
- [ ] `yuva2=U/T` jetonu
- [ ] Test: sentetik ara tonlu RGBA yuvası offscreen okunuyor (sRGB tanığı)
- [ ] Test: ön çarpım — yarı saydam kenarda koyu halka yok
- [ ] Doğrulama geçti (`make shader`, `make hepsi`, `make duman`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
