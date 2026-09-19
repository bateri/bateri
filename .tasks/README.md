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
| 009 | [shell-entegrasyonu](009-shell-entegrasyonu/) | 🔨 | `main`'de — zsh sarmalayıcısı ve OSC 133 komut durumu; teslim bekliyor: `/measure` (tarayıcının akış maliyeti) — `teslim.md` |
| 010 | [komut-bloklari](010-komut-bloklari/) | 🟢 | `main`'de — OSC 133'ün ilk ürün yüzeyi: komut işareti, çıkış kodu rengi, satıra çıpalanma. Gözle kontrol tasarımı değiştirdi: işaret bölge değil komut satırı, ve `>` şekli 011'e bırakıldı |
| 011 | [tabana-yapisik-icerik](011-tabana-yapisik-icerik/) | 🟢 | `main`'de — içerik tabana yaslanıyor ve **tek yönlü** kayıyor: büyüyen içerik süzülür, daralan anında oturur (yön kuralı gözle kontrolden çıktı). `/measure` bandı yeniden gözledi ve `QUIET_FLOOR`'u 870 → 868 indirdi; bir bilinçli `[~]` `teslim.md`'de (kaymanın yerleşme süresi — kanca yok) |
| 012 | [input-dock](012-input-dock/) | 🔨 | `main`'de — giriş satırı terminalin: dock ZLE'yi aynalıyor, prompt devredildi (`PS1` sıfır genişlik, `>` bizim chevron'umuz), çıpa `preexec`'e taşındı. 9 phase tamam; teslim bekliyor — gözle kontrol ve `make duman` kullanıcıda, üç ölçüm iddiası kanca borcuna takılı — `teslim.md` |
| 013 | [komut-suresi](013-komut-suresi/) | 🔨 | `main`'de — bir saniyeyi geçen komutların süresi komut satırının sağ ucunda, koşarken canlı. Kare talebinin **üçüncü** sebebi doğdu (**saat**) ve `bt-gpu::link`'in sözleşmesi üçe tamamlandı: koşan komutu olan pencere artık "boşta" sayılmıyor. 2 phase tamam; teslim bekliyor — gözle kontrol ve `make duman` kullanıcıda, bir ölçüm iddiası yük borcuna takılı — `teslim.md` |
| 014 | [imlec-stilleri](014-imlec-stilleri/) | 🔨 | `main`'de — imleç uygulamanın istediği şekli alıyor (blok/alt çizgi/çubuk), istenirse sönüyor ve rengini kendi tema rolünden alıyor. Saat ikinci bir tat kazandı: hasar dikmeyen uyandırma, saniyede iki kare. 3 phase tamam, kapı koştu (13 + 1 bulgu); teslim bekliyor — gözle kontrol kullanıcıda, bir tema kararı açık — `teslim.md` |
| 015 | [imlec-cilasi](015-imlec-cilasi/) | 📐 | Caret'in yüzeyi ve devri: köşe yarıçapı, yumuşak gölge, odak kaybında içi boş imleç — üçü de caret'e kendi çizim pipeline'ını vermenin sonucu. Yanına ölçülmüş bir kusur: hızlı komutta caret yukarı çıkıp geri iniyor (`ls` koşarken safha 44 ms sürüyor, animasyon 230 ms'de yerleşiyor) |

Durum işaretleri: **📐 planlama** · **🔨 devam** · **🟢 bitti** · **🗄️ arşiv**.
Düzen `.claude/is-akisi/duzen.md`'de.
