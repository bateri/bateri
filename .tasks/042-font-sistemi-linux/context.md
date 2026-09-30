# Font sistemi soyutlaması ve Linux font yığını — Bağlam

## Mevcut Durum

Linux yolunun ikinci adımı (kullanıcı kararları ve altı adımlık mimari
`.tasks/040-linux-kapisi-ve-wgpu/discussion.md` → baştaki "Kullanıcı
kararları" ve Karar 1; satırın kendisi `docs/YOL-HARITASI.md`'de). 040 bitti:
`bt-gpu` wgpu üstünde ve platform kütüphanesi görmüyor, `make linux` Docker'da
yalnız `bt-core`'u koşuyor (`Makefile` → `LINUX_CRATES = -p bt-core`,
`tools/linux/Dockerfile`: `rust:1.88-bookworm` + `zsh`).

`bt-atlas` bugün CoreText'e bağlı ve bağ iki dosyada toplanmış:

- **`font.rs`** (1101 satır, 106 CT/CG/`unsafe` referansı) — iki tür kodu
  **karışık** taşıyor:
  - *Platform yüzeyi:* ailenin açılması ve dönen adın sınanması (`open`,
    `open_chain`, `open_default`, `same_family`), yüz türetme (`derive_face`,
    trait maskesi), `is_monospaced`, `monospaced_families`, `glyph_index`,
    `glyph_advance`, `glyph_ink`, fontun ham metriği (ascent/descent/leading,
    alt çizgi, x-yüksekliği), cascade adayı (`cascade_candidate`,
    `CTFontCreateForString`), punto kopyası (`copy_with_attributes`),
    `is_last_resort` (PostScript adı), `has_color_glyphs` (trait biti),
    dizinin şekillendirilmesi (`shape_cluster`, `CTLine`).
  - *Platformsuz kural:* `centre_shift`, `ink_fits_placed`, `fit_ratio`,
    `accept`'in üç kollu sırası, `shrink`'in ikiye bölmesi, `Accepted::rise`,
    `metrics()`'in hücre formülü, `rule_envelope`, `round_up`,
    `Faces::effective`'in merdiveni, `SHRINK_LIMIT`. Bunlar bugün `CGFloat` /
    `CGRect` / `&CTFont` imzası taşıdığı için CoreText'e bağlı görünüyor ama
    hesapları fontsuz.
- **`raster.rs`** — iki çizim fonksiyonu (`draw_glyph`: alfa-only
  `CGBitmapContext`; `draw_color_glyph`: sRGB `PremultipliedLast` bağlam +
  `unpremultiply`) platforma bağlı; yordamsal çizim (`draw_rule`,
  `draw_procedural`, 1100+ satır) zaten saf.
- **`lib.rs`** — `Atlas` `Faces`, `small: CFRetained<CTFont>` ve `CGFloat`
  alanları tutuyor ama font kararını kendisi vermiyor; `slot()` yalnız
  `font::` ve `raster::` fonksiyonlarını çağırıyor. 76 sınamanın 59 satırı
  `&CTFont`/`CGFloat`/`font::`/`raster::`'a dokunuyor ve 25 satır macOS'un font
  adlarını (Menlo, SF Mono, Monaco, Helvetica, Apple Color Emoji, STIX) anıyor.
- **`census.rs`** (`make tarama`, 041) sınamaya özel ve CoreText'in cascade'ine
  bakıyor.

`bt-atlas`'ın dış yüzü **fontsuz**: `Atlas`, `Metrics`, `FontIssue`, `Face`,
`SizeClass`, `Sprite`, `Half`, `Plane`, `RuleKind`, `monospaced_families`,
`family_issue` — hiçbirinde CoreText tipi yok. `bt-gpu` onları kullanıyor
(`metrics.rs`, `frame.rs`, `slots.rs`, `renderer.rs`), `bt-shell` yalnız
`bt_gpu::monospaced_families`'i.

`bt-gpu` 040'tan beri platformsuz **ama Linux'ta hiç derlenmedi**, çünkü
`bt-atlas` derlenmiyor. Derlendiğinde görülecek üç macOS'a özgü nokta bugünden
okunuyor:

- `renderer.rs:728` — `wgpu::Backends::METAL` sabit ("Vulkan kolu font setinde
  açılır ve sınanır", aynı yerin doc'u).
- `surface.rs` — `Surface::from_layer` `wgpu::SurfaceTargetUnsafe::CoreAnimationLayer`'ı
  kullanıyor ve o varyant wgpu 30'da `#[cfg(metal)]` (`wgpu-30.0.1/src/api/surface.rs:435`).
- `renderer/tests.rs` — üç assert macOS aile adlarını bekliyor
  (`["SF Mono", "Menlo"]`, `"Helvetica"`).

## Motivasyon

Yol haritasının sıradaki satırı: `bt-shell` ayrımı ve winit MVP bu setten
sonra geliyor ve ikisi de Linux'ta **hücre çizebilen** bir `bt-atlas` istiyor.
Bugün Linux'ta bir harfin metriği bile alınamıyor. İkinci kazanç kapının
büyümesi: renderer 040'ta wgpu'ya geçti ama piksel bekçileri yalnız Metal
arka ucunda koşuyor; Vulkan kolu (Linux ürününün tek arka ucu) hiç
sınanmamış. Lavapipe üstünde koşunca renderer iki arka uçta da piksel
bekçili olur.

Kullanıcının kısıtı **macOS'ta hiçbir fark görünmemesi**: raster bit bit aynı,
yedek kapısının kararları aynı, hücre ölçüsü aynı. Linux font yığınının
davranışı (varsayılan aile, AA, emoji) yalnız Linux'ta görünüyor ve macOS'u
etkilemiyor. 040'ın dil kısıtı geçerli: yazılan ve **taşınan** kodun
yorumları, doc-comment'leri ve tanı metinleri İngilizce
(`.tasks/040-linux-kapisi-ve-wgpu/plan.md` → Dil kısıtı).

**Bit-bit eşitliğin bugün tanığı yok.** Mevcut sınamalar çizimin
özelliklerini (tabanda oturma, hücreye sığma, dörtlü döşeme) bekliyor, bir
yeniden düzenlemenin önce ve sonrasındaki baytları karşılaştırmıyor. Yedek
kapısının kararlarını ise `make tarama` (041) zaten envanter olarak döküyor —
önce/sonra karşılaştırmasının yarısı hazır, raster yarısı yok.

### Kanıt — bağımlılıkların bugünkü hâli (crates.io, 2026-09-30)

| crate | son sürüm | MSRV | lisans | taşıdığı |
|---|---|---|---|---|
| `freetype-rs` | 0.38.0 | — | MIT | `freetype-sys` 0.23, `bitflags`, `libc` |
| `freetype` (servo) | 0.8.0 | — | MIT/Apache-2.0 | `freetype-sys` 0.23 (opsiyonel), `libc` |
| `freetype-sys` | 0.23.0 | — | MIT | `libz-sys` (normal), `cc` + `pkg-config` (build); `bundled` özelliği kapalıyken `pkg-config freetype2 >= 24.3.18` ve bulamazsa **derleme düşer** (build.rs, sessiz bundled yok) |
| `fontconfig` | 0.11.0 | 1.77 | MIT | `yeslogic-fontconfig-sys` 6; `sort_fonts`, `CharSet` API'de var |
| `yeslogic-fontconfig-sys` | 6.0.1 | 1.77 | MIT | `dlib`, `once_cell`, `pkg-config` (build); `dlopen` özelliği opsiyonel |
| `harfrust` | 0.13.3 | 1.85 | MIT | `read-fonts` 0.43 (MIT/Apache), `bytemuck`, `bitflags`, `smallvec`, `once_cell` |
| `rustybuzz` | 0.20.1 | — | MIT | `ttf-parser` |

Yerel `rustc` 1.88.0 (Homebrew), workspace `rust-version = "1.88"`, imaj
`rust:1.88-bookworm`; üçünün MSRV'si de altında. Debian bookworm'un FreeType'ı
2.12.1 (libtool 24.3.18 — `freetype-sys`'in alt sınırı tam o; FreeType'ın
`VERSIONS.TXT` tablosundan, imajda `pkg-config --modversion freetype2` ile
doğrulanacak).
