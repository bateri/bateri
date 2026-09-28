# Linux kapısı ve wgpu renderer'ı — Bağlam

## Mevcut Durum

bateri yalnız macOS'ta koşuyor ve platforma dört katmanda bağlı (katman
tablosu `CLAUDE.md` → Katman düzeni):

- **`bt-core`** zaten platformsuz ve bu artık varsayım değil ölçü (aşağıda,
  Kanıt). Ama `CLAUDE.md`'nin "kapı Linux hedefiyle derlemedir" cümlesi hiçbir
  komuta bağlı değil: makinede yalnız `aarch64-apple-darwin` std'si var
  (Homebrew rustc; başka hedefin std'si eklenemiyor), `Makefile`'da Linux
  hedefi yok, uzak CI yok. Platformsuzluğu bugün yalnız `make denetim`'in
  `objc2`/`core_text` grep'i tutuyor — yani "macOS kütüphanesi yok" deniyor,
  "Linux'ta derleniyor" denmiyor.
- **`bt-gpu`** doğrudan Metal: `renderer.rs` (5428 satır, 121 `MTL`/`objc2`
  referansı, 60 sınama; piksel bekçileri `getBytes` ile dokudan okuyor),
  `shaders/*.metal` (958 satır: `cell_bg` 205, `cell` 165, `glyph_fx` 588),
  `build.rs` (`xcrun metal` → gömülü `default.metallib`), küçük değerler
  `setVertexBytes`/`setFragmentBytes` ile, GPU damgası
  `GPUStartTime`/`GPUEndTime` ile, kare sayacı `addCompletedHandler` ile.
  Ritim `link.rs`'te (2707 satır): `CAMetalDisplayLink` drawable'ı callback'ten
  **önce** alıp `update.drawable()` ile veriyor, `setPaused` her thread'den
  `MainThreadBound` üzerinden, saatin gecikmeli uyandırması
  `dispatch2::after`. `frame.rs`, `motion.rs`, `glyph_fx.rs`, `blink.rs`,
  `stats.rs` Metal'e hiç dokunmuyor (0 referans).
- **`bt-atlas`** CoreText'e bağlı (`font.rs` 65 CT referansı; `raster.rs`'in
  çizim yarısı) — bu setin konusu değil, sıra `docs/YOL-HARITASI.md`'de.
- **`bt-shell`** AppKit; `bt-gpu`'dan Metal tipi görmüyor, yalnız
  `Surface::ca_layer()`'ı view'a takıyor (`pane.rs`) ve boyutu
  `Surface::set_size` ile veriyor. Ayrımı da bu setin konusu değil.

## Motivasyon

Kullanıcı kararı (2026-09-28): bateri macOS'ta **aynen** kalırken Linux'ta da
koşsun. Varılan mimari ve kesin kararlar `discussion.md`'nin başında
(kullanıcı kararları 1–4); bu set o yolun **ilk** adımı: Linux'u sınayan bir
kapı ve renderer'ın taşınabilir bir GPU katmanına (wgpu: macOS'ta Metal arka
ucu, Linux'ta Vulkan) geçişi. Geçiş ölçümlü bir denemeyle başlıyor ve deneme
kötüyse set orada durup kullanıcıya dönüyor.

Korunması zorunlu olanlar kullanıcının cümlesiyle: macOS'ta kullanıcı **hiçbir
fark görmemeli**; dock, caret/kayma/blink animasyonları, boşta sıfır kare ve
piksel bekçileri korunmalı. Sözleşmenin ilgili bölümleri `CLAUDE.md` → "Renk
uzayı sınırı geçer" (sRGB çizim hedefi, `MIDTONE` bekçisi), "Boşta sıfır kare"
(kare istemenin üç yolu, `bt-gpu::link` modül başlığı) ve Komutlar →
`make duman` (jeton sözleşmesi: silinmez, eklenir).

### Kanıt — `bt-core` Linux'ta

2026-09-28, bu makinede Docker (OrbStack, `linux aarch64`),
`rust:1.88-bookworm` imajı (yerel rustc ile aynı sürüm), `apt install zsh`:

- `cargo test -p bt-core`: **599 geçti, 3 düştü**. Üçü de gerçek zsh koşan
  ayna sınaması (`the_mirror_follows_a_real_zle_session`,
  `the_widget_deletes_moves_and_types_over_a_selection_in_zle`,
  `the_widget_is_bound_in_viins_and_in_a_keymap_linked_to_main`) ve belirti
  aynı: ASCII olmayan harf aynada `??` — imajda yerel yok (`LANG` boş, C
  yereli), yani zsh çok baytlı karakteri tanımıyor. Kod kusuru değil ortam.
- Aynı koşu `LANG=C.UTF-8` ile: **602 geçti, 0 düştü** (13 `ignored`).
- `cargo clippy -p bt-core --all-targets -- -D warnings`: temiz.

Yani `bt-core`'un Linux'u bugün yeşil; eksik olan onu **her seferinde**
söyleyen komut.

### Kanıt — wgpu'nun sürümü ve Metal bağı

`wgpu` 30.0.1 (`rust-version` 1.87; yerel 1.88). `wgpu-hal`'in Metal arka ucu
`objc2` 0.6 / `objc2-metal` 0.3.2 / `objc2-quartz-core` 0.3.2 / `block2` 0.6
kullanıyor — workspace'in zaten pinlediği nesil. Yani drawable'ı hal üzerinden
sarmak (`create_texture_from_hal`) `Cargo.lock`'a **ikinci bir Metal bağlama
yığını** sokmuyor; servo `core-text`'i reddettiren itiraz (`Cargo.toml`'daki
`objc2-core-text` yorumu) burada konusuz. Aynı sürümde küçük değerlerin
özelliği `Features::IMMEDIATES` (eski adı push constant; Metal ve Vulkan
destekli), GPU damgası `Features::TIMESTAMP_QUERY` (Metal destekli, çalışma
zamanında var olup olmadığı adaptöre bağlı).
