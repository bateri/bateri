# İşler

| # | İş | Durum | Not |
|---|---|---|---|
| 001 | [workspace-iskeleti](001-workspace-iskeleti/) | 🟢 | workspace, Makefile, shader zinciri, ilk pencere; `main`'de |
| 002 | [vt-motoru](002-vt-motoru/) | 🔨 | `main`'de — pencerede gerçek shell, klavye PTY'ye akıyor, boşta sıfır kare. Teslim bekliyor: `/measure` (iki ölçüm — 2026-09-10'da koştu, **ölçüm aracı yok**: kanca seti bekliyor); Apache-2.0 attribution bundle setine devredildi |
| 003 | [glyph-atlas](003-glyph-atlas/) | 🔨 | `main`'de — 4 phase tamam (sRGB → `bt-atlas` CoreText raster → metrik geçişi → glyph): pencerede okunabilir metin var, `make duman` `glif=` jetonu kazandı. Teslim bekliyor: göz kontrolü (`colorspace` `nil`, glyph yerleşimi) + `/measure` (beş ölçüm — 2026-09-10'da koştu, **ölçüm aracı yok**: kanca seti bekliyor) |

Durum işaretleri: **📐 planlama** · **🔨 devam** · **🟢 bitti** · **🗄️ arşiv**.
Düzen `.claude/is-akisi/duzen.md`'de.
