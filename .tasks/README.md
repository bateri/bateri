# İşler

| # | İş | Durum | Not |
|---|---|---|---|
| 001 | [workspace-iskeleti](001-workspace-iskeleti/) | 🟢 | workspace, Makefile, shader zinciri, ilk pencere; `main`'de |
| 002 | [vt-motoru](002-vt-motoru/) | 🔨 | `main`'de — pencerede gerçek shell, klavye PTY'ye akıyor, boşta sıfır kare. Teslim bekliyor: `/measure` (iki ölçüm); Apache-2.0 attribution bundle setine devredildi |
| 003 | [glyph-atlas](003-glyph-atlas/) | 🔨 | `main`'de — 4 phase tamam (sRGB → `bt-atlas` CoreText raster → metrik geçişi → glyph): pencerede okunabilir metin var, `make duman` `glif=` jetonu kazandı. Teslim bekliyor: `/measure` (beş ölçüm) + göz kontrolü (`colorspace` `nil`, glyph yerleşimi) |

Durum işaretleri: **📐 planlama** · **🔨 devam** · **🟢 bitti** · **🗄️ arşiv**.
Düzen `.claude/is-akisi/duzen.md`'de.
