# İşler

| # | İş | Durum | Not |
|---|---|---|---|
| 001 | [workspace-iskeleti](001-workspace-iskeleti/) | 🟢 | workspace, Makefile, shader zinciri, ilk pencere; `main`'de |
| 002 | [vt-motoru](002-vt-motoru/) | 🔨 | `main`'de — pencerede gerçek shell; teslim bekliyor: `/measure` (iki iddia; #2'nin yarısı bench ister) — `teslim.md` |
| 003 | [glyph-atlas](003-glyph-atlas/) | 🔨 | `main`'de — CoreText glyph atlası ve metin çizimi; teslim bekliyor: `/measure` (#4, #5 kancayla ölçülebilir; #1, #2 bench ister) — `teslim.md` |
| 004 | [yazi-bicimleri](004-yazi-bicimleri/) | 🔨 | `main`'de — kalın/eğik yüzler, beş alt çizgi, üstü çizili; teslim bekliyor: `/measure` (dört iddia kancayla ölçülebilir, #4'ün yarısı bench ister) — `teslim.md` |
| 005 | [olcum-kancalari](005-olcum-kancalari/) | 🔨 | `main`'de — ölçüm kancaları (`BT_FRAME_STATS`, `BT_SCROLL_TEST`), sınırlı kapanış, `IDLE_FRAME_LIMIT` = 8; teslim bekliyor: kare süresi ve açılış ailesinin `/measure`'ı — `teslim.md` |
| 006 | [gunluk-kullanim-esigi](006-gunluk-kullanim-esigi/) | 🟢 | `main`'de — seçim, pano, geçmişte kaydırma, `bateri.app`, ev dizini ve UTF-8 yereli; iki bilinçli `[~]` `teslim.md`'de |
| 007 | [ayarlar-ve-tema](007-ayarlar-ve-tema/) | 🟢 | `main`'de — canlı ayar dosyası, açık/koyu tema, font, ana menü, OSC 52 kopyası; iki bilinçli `[~]` `teslim.md`'de |
| 008 | [hareket-ve-imlec](008-hareket-ve-imlec/) | 🟢 | `main`'de — imleç kayıyor: hareket altyapısı, üç stil, Hareketi Azalt ve ölçülmüş sessizlik kapısı (`QUIET_FLOOR`) |
| 009 | [shell-entegrasyonu](009-shell-entegrasyonu/) | 🔨 | zsh entegrasyonu + OSC 133 komut durumu; 5 phase tamam, teslim bekliyor |

Durum işaretleri: **📐 planlama** · **🔨 devam** · **🟢 bitti** · **🗄️ arşiv**.
Düzen `.claude/is-akisi/duzen.md`'de.
