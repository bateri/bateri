# Phase 6 — `bt-gpu` lavapipe'ta ve sözleşme

## Özet

`bt-gpu` Linux'ta derlenir ve offscreen piksel sınamaları lavapipe üstünde
koşar; sözleşme belgeleri setin sonucuna göre güncellenir (`discussion.md` →
Karar 7, 8).

_Requirements: R8, R9, R1.2_

## Değişiklikler

- **`crates/bt-gpu/src/renderer.rs`** — `Backends::METAL` macOS'ta, Linux'ta
  `Backends::VULKAN` (`cfg`); doc'undaki "font setinde açılır" cümlesi.
- **`crates/bt-gpu/src/surface.rs`** — `Surface::from_layer`
  `cfg(target_os = "macos")`.
- **`crates/bt-gpu/src/renderer/tests.rs`, `renderer/wgpu_tests.rs`** —
  Karar 7'nin `bt-gpu` listesi: aile adlı üç assert, `☕` sınaması, renk/emoji
  sınamaları, `scene_wide`, `SCALE`'in gerekçesi; örnekler `bt-atlas`'ın
  fikstürüyle aynı kaynaktan. Linux'ta düşen sınama önce font/arka uç diye
  ayrılır; arka uç farkı için karar (arka uca özgü beklenti ya da tolerans)
  Uygulama Notları'na.
- **`tools/linux/Dockerfile`** — `mesa-vulkan-drivers`, `libvulkan1`.
- **`Makefile`** — `LINUX_CRATES += -p bt-gpu`, `linux` yorumu.
- **`CLAUDE.md`** — katman tablosunun `bt-atlas` satırı (iki arka uç,
  platform kütüphaneleri) ve bağımlılık paragrafı (`freetype-rs`,
  `fontconfig`, `harfrust`, pkg-config, hedefe koşullu, Linux bildirim
  borcu); `bt-gpu` satırının "Linux'ta derlendiği font setinde" ifadesi.
- **`.claude/is-akisi/proje.md`** — Doğrulama'nın `make linux` satırı
  (kapsam üç crate, tetik).

## Kabul

- `make linux` yeşil: `bt-core`, `bt-atlas`, `bt-gpu`; `wgsl_pipelines_build`
  ve piksel sınamaları lavapipe'ta.
- `make hepsi` ve `make shader` yeşil; macOS'ta `bt-gpu` sınamaları
  değişmeden geçiyor.
- Belgeler kodla çelişmiyor (katman tablosu, Doğrulama satırı, Dockerfile
  ve `Makefile` yorumları aynı kapsamı söylüyor).

## Checklist

- [ ] `cfg` düzenlemeleri (backend, `from_layer`)
- [ ] `bt-gpu` sınamaları fikstüre
- [ ] İmaj + `LINUX_CRATES`
- [ ] `CLAUDE.md`, `proje.md`, yorumlar
- [ ] Test: lavapipe'ta piksel sınamaları; macOS'ta değişmeden
- [ ] Doğrulama geçti (`make hepsi` + `make shader` + `make linux`)
