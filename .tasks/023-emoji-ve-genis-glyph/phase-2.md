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

## Uygulama Notları

- **Düzlem kararı fontun trait bitinden** (`kCTFontTraitColorGlyphs`), aile
  adından değil. Plan "renkli bitmap" diyordu ama ölçütü söylemiyordu; aile
  adını aramak kullanıcının kurduğu başka bir renkli fontu sessizce maske
  düzlemine düşürürdü.
- **Renk dokusu `slot_uv`'nin içinde, yükleme anında kuruluyor.** İlk yazım
  `prepare`'in başında bir ön kontrol denedi ("bu karede renkli glyph var
  mı") ve o kontrol cascade'i **ikinci kez** yürümeyi gerektiriyordu. Bir
  kare sonra kurmak da olmuyor: yuva yazılmadan önbelleğe girer ve emoji
  **kalıcı olarak** görünmez kalır. Çare `ColourPlane` tipi — doku yuvası +
  device + kenar, tek argümanda (`slot_uv` yine clippy'nin sınırında).
- **Kapasite kapısı iki kez soruluyor.** Yukarıdaki `need` kapısı maskenin
  sayacına bakıyor, çünkü düzlem ancak çizim sırasında biliniyor; tahsisten
  hemen önce düzlemin kendi sayacı **yeniden** soruluyor. İkisi yalnız bir
  hâlde ayrışıyor (maskede yer var, renk düzlemi dolu) ve o hâlde bedel
  önbelleğe girmeyen renkli karakter başına kare başına bir rasterizasyon —
  ölçülmedi, kabul edildi ve koda yazıldı. Alternatifleri iki düzlemi tek
  havuza bağlamak (emoji-ağır oturum harfleri tofu'ya düşürürdü) ya
  `fallback_font`'u ikiye bölmekti.
- **`pipeline()` parametresini geri aldı** (008 phase-5 onu "tek değere
  düşünce" atmıştı) ve fonksiyonun doc'undaki "blend parametre değil" cümlesi
  daraldı: blend'in **üç** çarpanı parametre değil, dördüncüsü (RGB kaynağı)
  fragment'in ön çarpım sözleşmesinden türüyor.
- **Maske listesi boş olabiliyor.** Yalnız emoji taşıyan bir kare mümkün
  (`glyphs` dolu, hepsi renk düzlemine gitti) ve `encode_glyphs`'in baştaki
  kapısı `GlyphCell`'leri sayıyor, düzleme ayrılmış instance'ları değil —
  sıfır uzunluklu bir `newBufferWithBytes` doğuyordu. İkinci bir kapı
  eklendi.
- **Ön çarpım tanığının eşiği gevşek.** Aranan şey kararmanın **yokluğu**,
  kesin bir bayt değil: lineer uzayda iki kez çarpım dörtte bire iner ve
  eşik onu rahatça yakalıyor. Kesin bayt istemek sınamayı sürücünün sRGB
  yuvarlamasına rehin ederdi.
- **Kapı beş bulgu verdi, dördü gerçek kusurdu ve düzeltildi.** (1) `sync_atlas`
  atlası yeniden kurarken maske dokusunu düşürüyordu, **renk dokusunu
  düşürmüyordu**: Cmd+ ile punto büyüyünce doku kenarı değişiyor ve eski
  kenarda kalan renk dokusuna yeni ızgaranın köşeleriyle yazmak
  `replaceRegion`'ı dokunun dışına taşırıyordu (bekçisi
  `rebuilding_the_atlas_drops_both_textures`). (2) Negatif önbellek tahliyesi
  yuva **numarasına** bakıyordu ve renk sayacı 0'dan başladığı için ilk
  emojinin pozitif kaydını da atıyordu — ölçüt kaydın tamamı oldu.
  (3) Çizimden önceki kapasite kapısı yalnız maskenin sayacına bakıyordu ve bu
  `colour_next`'in yazılı sözünü ("CJK-ağır oturum emojiyi tofu'ya düşürmez")
  **çürütüyordu**; ölçüt iki düzlemin boşta olanı (`min`) oldu, yani kapı
  ancak ikisi de doluyken kapanıyor. (4) `a_colour_glyph_goes_to_the_colour_list`'in
  kaçış dalı ("renkli font kurulu değil") tam da `has_color_glyphs`'in
  regresyon hâliyle aynı görünüyordu — dal artık maske düzleminin yalnız
  tofu tuttuğunu sınıyor. Beşincisi belge sapmasıydı ve `CLAUDE.md`'nin kendi
  kuralı ("bir cümle kodla çelişirse ikisinden biri **aynı commit'te**
  düzelir") gereği phase-3'ten **bu commit'e alındı**: pipeline sayısı,
  iki düzlemin dokuları, emoji paragrafının tamamı ve jeton sözleşmesi
  (`CLAUDE.md` + `Makefile`). Phase-3'e kalan `docs/OLCUMLER.md` envanteri ve
  yol haritasının borç kalemleri.
- **Release profilinde `bt-shell`'in beş sınaması düşüyor ve bu `main`'de de
  böyle** (`git stash` ile doğrulandı): sarmalayıcı betiği depo kolunda
  `target/debug` üzerinden aranıyor. Kapının profili debug, yani bu setin
  konusu değil — ama yol haritasına yazılacak bir kalem.

## Checklist

- [x] `Atlas` içinde ikinci düzlem, kendi monoton `next`'i, format-duyarlı
      `slot_bytes`
- [x] İkinci CG reçetesi (ön çarpımlı RGBA + renk uzayı)
- [x] Kardeş fragment, `cell_vertex` paylaşımı, stride 32 korunuyor
- [x] Doku `RGBA8Unorm_sRGB`, `upload_slot` `bytesPerRow`-duyarlı
- [x] Blend RGB kaynağı `One`; `pipeline()`'ın parametresi ve 008 phase-5'in
      geri alınma gerekçesi yazıldı
- [x] Çizim sırası **üç yüzeyde** kazanıldı (tek gövde: `prepare` kare başına dört kez koşuyor) (ızgara, doldurma, dock)
- [x] `yuva2=U/T` jetonu (`CLAUDE.md` + `Makefile` sözleşmesiyle birlikte)
- [x] Test: sentetik ara tonlu RGBA yuvası offscreen okunuyor → `a_midtone_colour_slot_survives_the_round_trip`, `the_colour_plane_is_an_srgb_texture`
- [x] Test: ön çarpım — `a_premultiplied_edge_does_not_darken`; ayrıca `a_colour_glyph_goes_to_the_colour_list` ve `rebuilding_the_atlas_drops_both_textures`
- [x] Doğrulama geçti (`make shader` + `make hepsi` yeşil; `make duman` `yuva=13/1984 yuva2=0/1984 hareket=27 icerik=3 sessiz=1754.12ms kapanis=clean`)
- [x] Riskli phase: `/code-review` koştu, beş bulgunun beşi giderildi (bkz. Uygulama Notları)
