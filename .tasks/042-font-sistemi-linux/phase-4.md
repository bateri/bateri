# Phase 4 — Linux arka ucu: maske yolu

## Özet

FreeType + fontconfig arka ucunun zinciri, yüzleri, metriği, mürekkebi,
cascade'i ve maske çizimi; imaj ve `make linux` `bt-atlas`'a büyür
(`discussion.md` → Karar 5, 6, 8).

**Ön koşul:** Karar'daki eskalasyon (crate/sürüm, dinamik bağlanma, imaj
paketleri) kullanıcı tarafından onaylandı. Onay yoksa phase başlamaz.
✅ **Kullanıcı onayı 2026-09-30:** `freetype-rs` 0.38, `fontconfig` 0.11,
`harfrust` 0.13; geçişli `freetype-sys`, `libz-sys`, `cc`, `pkg-config`,
`yeslogic-fontconfig-sys`, `dlib`, `once_cell`, `read-fonts`, `font-types`,
`bytemuck`, `smallvec` (hepsi yalnız linux hedefinde); pkg-config ile dinamik
bağlanma (`bundled`/`dlopen` kapalı); imaj paketleri `pkg-config`,
`libfreetype-dev`, `libfontconfig-dev`, `zlib1g-dev`, `fonts-dejavu-core`,
`fonts-noto-color-emoji`, `mesa-vulkan-drivers`, `libvulkan1`. **İkinci onay
(aynı gün, kullanıcı onayı 2026-09-30):** kilitte çıkan derleme zamanı
geçişlileri `vcpkg` 0.2.15 (libz-sys), `shlex` 2.0.1 ve `find-msvc-tools`
0.1.14 (`cc` 1.5.1) ile `bt-atlas` → `yeslogic-fontconfig-sys` doğrudan kenarı
(sarmalayıcı charset okumayı açmıyor). `Cargo.lock` değişimi bu kararın
kaydı.

_Requirements: R6, R8_

## Değişiklikler

- **`Cargo.toml` (workspace) ve `crates/bt-atlas/Cargo.toml`** — `freetype-rs`
  ve `fontconfig`, `[target.'cfg(target_os = "linux")'.dependencies]`,
  `bundled`/`dlopen` kapalı; gerekçe yorumu kararın kaydına bağlı.
  `Cargo.lock` değişimi bu karar.
- **FreeType arka ucu (yeni modül, `cfg(target_os = "linux")`)** — Karar 6:
  bayt tamponu font başına bir kez + `new_memory_face`; `open`/`open_default`
  (`monospace`); `derive` (fontconfig stil eşleşmesi + edinilen stilin
  sınaması); `is_monospaced` (`FT_IS_FIXED_WIDTH`), `families`
  (`FcFontList`, `spacing = mono`); tasarım biriminden kesirli ham metrik
  (OS/2 x-yüksekliği, yoksa `x`'in sınırı); `advance`; `ink`
  (`FT_Outline_Get_BBox`); `cascade` (`sort_fonts` atlas başına bir kez,
  charset yürüyüşü); `at_size`/`size`; `is_last_resort` → `false`;
  `draw_mask` (`FT_LOAD_NO_HINTING`, 26.6 öteleme, gri render, yuvaya
  kırpılarak blit; `baseline`'ı üstten satıra çevirir). `draw_color` ve
  `shape` **adlı taslak**: renkli aday maskeye değil kutuya, dizi taban
  karaktere düşer.
- **Sınama fikstürü (Linux kolu)** — DejaVu'ya göre örnek karakterler ve
  aile adı; renk/küme değişmezleri Linux'ta `cfg`'li bekliyor.
- **`tools/linux/Dockerfile`** — `pkg-config`, `libfreetype-dev`,
  `libfontconfig-dev`, `zlib1g-dev`, `fonts-dejavu-core`, `fc-cache`; başlık
  yorumu. `pkg-config --modversion freetype2` ≥ 24.3.18 doğrulanır.
- **`Makefile`** — `LINUX_CRATES = -p bt-core -p bt-atlas`, `linux` yorumu.

## Kabul

- `make linux` yeşil: `bt-atlas`'ın platformsuz değişmezleri (renk/küme
  hariç) Linux'ta geçiyor.
- `make hepsi` yeşil ve tanık ebeveynle aynı (macOS'a dokunulmadı).
- `cargo tree` (macOS hedefi) yeni crate göstermiyor; `THIRD-PARTY-LICENSES`
  değişmiyor.

## Checklist

- [x] Eskalasyon onayı alındı
- [x] Bağımlılıklar hedefe koşullu, kararlı yorumla
- [x] FreeType arka ucu (maske yolu) + adlı taslaklar
- [x] İmaj ve `LINUX_CRATES`
- [x] Linux `fixture` modülü (`system::fixture`'ın `cfg`'i Linux'u da kapsar): `coretext::fixture`'ın adları — `DEFAULT_FAMILY`, `PROPORTIONAL_FAMILY`, `SECOND_FAMILY`, `UNKNOWN_CHAR`, `WIDE_CHAR`, `FALLBACK_CHAR`, `INK_CHAR`, `GATE_PROBES`, `BASE_SYMBOLS`, `CLUSTERS`, `CLUSTER_BASE`, `CLUSTER_SCALE`, `family_name(&Font)` (phase-3'ten devir)
- [x] Test: Linux'ta değişmezler; macOS'ta tanık aynı
- [x] Doğrulama geçti (`make hepsi` + `make linux`)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi

## Uygulama Notları

- Kilide giren crate'ler: cc, dlib, find-msvc-tools, fontconfig, freetype-rs,
  freetype-sys, libz-sys, pkg-config, shlex, vcpkg, yeslogic-fontconfig-sys
  (bitflags, libc, once_cell zaten grafta). macOS ürün grafı değişmedi
  (`cargo tree --target aarch64-apple-darwin -p bateri`), yani
  THIRD-PARTY-LICENSES de. `harfrust` ve geçişlileri phase-5'e kaldı.
- `derive`'ın stil kapısı fontconfig'in eşleşme desenine değil dosyanın
  kendi `style_flags`'ine bakıyor: `90-synthetic.conf` roman dosyanın
  eşleşmesine `slant=oblique` yazıyor ve biz sentezlemiyoruz. Aile de
  karşılaştırılıyor (başka ailenin kalın yüzü ikame sayılır).
- Görüntüde eğik yüz yok (fonts-dejavu-core): Italic/BoldItalic düz yüze
  çöküyor. `bold_face_gets_own_slot` bu yüzden yalnız ailenin **gerçekten
  sahip olduğu** yüzleri (`Faces::effective(face) == face`) dolaşıyor ve
  kalın yüzü şart koşuyor; macOS'ta dördü de var, sınama aynı.
- `a_single_cell_rejection_does_not_answer_the_wide_request`
  `cfg(target_os = "macos")` kalibrasyonu: tek hücrelik ret `.LastResort`'tan
  geliyor. Son çare fontu olmayan arka uçta iki hücreye sığan her glyph
  yarı boyutta tek hücreye de sığıyor (fit ≤ 2 < `SHRINK_LIMIT`), yani öncül
  Linux'ta kurulamıyor.
- `SECOND_FAMILY` Linux fikstüründe **var** (uygulama öncesi bulgunun
  aksine): tek tüketicisi `ensure_rebuilds_when_family_changes` platformsuz
  ve yalnız yeniden kurulmayı soruyor. Görüntüde ikinci eşaralıklı aile
  olmadığı için değeri eşaralıklı olmayan `DejaVu Serif`.
- **Linux'un gördüğü platformsuz kusur, düzeltildi:** dolu atlasta tek boş
  yuva kalınca önbelleklenmemiş `Half::Right` isteği bir yuva sayıyordu ve
  (`.LastResort`'suz arka uçta) sol yarı tofu iken sağ hücreye küçültülmüş
  bütün glyph çiziyordu. `lib.rs`'in kapasite kapısında `Right` artık iki
  sayıyor — mevcut yorumun "sol yarı da aynı sayıdan tofu" sözünün koşulu.
  macOS'ta tanık aynı.
- Fikstür (DejaVu, 13pt@1x ölçüldü): `WIDE_CHAR` `⁂` (mürekkep 0.05..1.61
  hücre), `FALLBACK_CHAR` `⁊` (ilerleme 0.83), `INK_CHAR` `∖` (ilerleme
  1.058, mürekkep 0.32..0.88), kutu tarafı `⟹` (fit 2.30). `BASE_SYMBOLS`
  macOS'unkiyle aynı: hepsi DejaVu Sans Mono'da tam hücre ilerliyor.
  Küme sınamaları Linux'ta `ignore = "phase-5: …"`
  (`a_cluster_takes_two_colour_slots`,
  `the_same_cluster_is_interned_and_cached_once`,
  `an_unshaped_cluster_answers_with_its_base_char`).
- İlerleme `linearHoriAdvance` (tasarım birimi × ölçek, 16.16), metrikler
  tasarım biriminden, mürekkep `FT_Outline_Get_BBox`; boyut 26.6'ya
  nicemlenip `size()` o değeri veriyor. Font dosyası thread başına bir kez
  okunuyor (FreeType kütüphanesi thread-local; `Atlas` zaten `Send` değil).
  Fontu bulunamayan makinede font "boş" (tofu), panik yok.
- Sözleşme satırları aynı commit'te: `CLAUDE.md` (Komutlar'ın `make linux`
  satırı, katman tablosunun `bt-atlas` hücresi) ve `proje.md` → Doğrulama.
  Tam güncelleme phase-6'da.
- Tanık: ebeveyn (`e7c8f04`) çıktısı düzenlemeden önce temiz ağaçta alındı;
  38 676 satır, fark boş.
- `/code-review` (medium, --fix) üç bulguyu giderdi: bayt önbelleği `Weak`
  tutuyor (ayar penceresinin listesi her eşaralıklı aileyi açıp belleği
  ömür boyu tutuyordu), `families()` `spacing = dual`'ı da listeliyor (CJK
  eşaralıklı aileler), Dockerfile yorumu 24.3.18 = FreeType 2.12.1. Kapıdan
  sonra `make hepsi` ve `make linux` yeniden yeşil.
