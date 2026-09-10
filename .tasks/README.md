# İşler

| # | İş | Durum | Not |
|---|---|---|---|
| 001 | [workspace-iskeleti](001-workspace-iskeleti/) | 🟢 | workspace, Makefile, shader zinciri, ilk pencere; `main`'de |
| 002 | [vt-motoru](002-vt-motoru/) | 🔨 | `main`'de — pencerede gerçek shell, klavye PTY'ye akıyor, boşta sıfır kare. Teslim bekliyor: `/measure` (iki ölçüm — 2026-09-10'da koştu, **ölçüm aracı yok**: kanca seti bekliyor); Apache-2.0 attribution bundle setine devredildi |
| 003 | [glyph-atlas](003-glyph-atlas/) | 🔨 | `main`'de — 4 phase tamam (sRGB → `bt-atlas` CoreText raster → metrik geçişi → glyph): pencerede okunabilir metin var, `make duman` `glif=` jetonu kazandı. Göz kontrolü 2026-09-10'da yapıldı ✅. Tek açık kalem `/measure` (beş ölçüm) ve o **eyleme geçilebilir değil**: ölçüm kancaları yok, kanca seti açılınca 002 ile birlikte kapanır |
| 004 | [yazi-bicimleri](004-yazi-bicimleri/) | 📐 | `BOLD`/`ITALIC`/`UNDERLINE`/`STRIKEOUT`: ikinci font yüzü ve kural çizgileri. 003 glyph'i çizdi ama biçim bayrakları `frame()` sınırından hiç geçmiyor |

Durum işaretleri: **📐 planlama** · **🔨 devam** · **🟢 bitti** · **🗄️ arşiv**.
Düzen `.claude/is-akisi/duzen.md`'de.
