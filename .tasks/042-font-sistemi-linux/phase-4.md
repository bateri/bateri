# Phase 4 — Linux arka ucu: maske yolu

## Özet

FreeType + fontconfig arka ucunun zinciri, yüzleri, metriği, mürekkebi,
cascade'i ve maske çizimi; imaj ve `make linux` `bt-atlas`'a büyür
(`discussion.md` → Karar 5, 6, 8).

**Ön koşul:** Karar'daki eskalasyon (crate/sürüm, dinamik bağlanma, imaj
paketleri) kullanıcı tarafından onaylandı. Onay yoksa phase başlamaz.

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

- [ ] Eskalasyon onayı alındı
- [ ] Bağımlılıklar hedefe koşullu, kararlı yorumla
- [ ] FreeType arka ucu (maske yolu) + adlı taslaklar
- [ ] İmaj ve `LINUX_CRATES`
- [ ] Linux `fixture` modülü (`system::fixture`'ın `cfg`'i Linux'u da kapsar): `coretext::fixture`'ın adları — `DEFAULT_FAMILY`, `PROPORTIONAL_FAMILY`, `SECOND_FAMILY`, `UNKNOWN_CHAR`, `WIDE_CHAR`, `FALLBACK_CHAR`, `INK_CHAR`, `GATE_PROBES`, `BASE_SYMBOLS`, `CLUSTERS`, `CLUSTER_BASE`, `CLUSTER_SCALE`, `family_name(&Font)` (phase-3'ten devir)
- [ ] Test: Linux'ta değişmezler; macOS'ta tanık aynı
- [ ] Doğrulama geçti (`make hepsi` + `make linux`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
