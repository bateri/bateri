# Ekranı temizle (⌘K) ve eksik standart davranışlar — Bağlam

## Mevcut Durum

**⌘K hiçbir yere bağlı değil.** Menüde öğe yok (`crates/bt-shell/src/menu.rs`
→ `install`: Edit'te Cut/Copy/Paste/Select All/Find ▸, View'da Theme ▸ ve
punto, Shell'de pencere/sekme — "Clear" yok) ve `keyDown:`'ın Cmd izin
listesi kapalı üç tuş (⌘⌫/⌘←/⌘→; `keys::encode_key`), yani ⌘K bugün
**sessizce yutuluyor**: ne ekran temizleniyor ne kabuğa bir şey gidiyor.

Temizlemeye değen mevcut makine `bt-core`'da ve **kabuğun `CSI 2 J`'sine**
kurulu (`CLAUDE.md` → Katman düzeni, `bt-core` satırının "Dördüncü kol"u):

- Tarayıcı `2J`'yi sayıyor, kare yolu nesli `Term` kilidi altında tüketip
  "kasten temizlendi" bayrağını kuruyor (`Session::observe_screen_clear`,
  `session.rs` ~4115; çağrısı `frame()` içinde ~3636 ve `frame()` kilidi
  `let mut term` ile tutuyor). Bayrak doldurma bandını kapatıyor
  (`fill_rows`), kayma sayısını sıfırlıyor (`scrolled`) ve son temizlemenin
  geçmişe ittiği satırı `clear_boundary` olarak işaretliyor.
- `3J` ve RIS için kol **yok**; geçmişi silen tek yol kabuğun `clear(1)`'i
  (`\e[H\e[2J\e[3J`, tek `write`) — doc'un "Bilinen sınır 3"ü.
- alacritty'de birincil ekranda `ClearMode::All` → `Grid::clear_viewport`
  görünen satırları **geçmişe itiyor** (alacritty_terminal 0.26
  `grid/mod.rs:309`); `ClearMode::Saved` → `grid.clear_history()` yalnız
  **etkin** ızgaranın geçmişini siliyor (`term/mod.rs:1804`).
- Alternatif ekranda birincil ızgara `Term::inactive_grid`'de ve alan
  **özel**, erişimcisi yok (`term/mod.rs:287`) — yani vim açıkken birincil
  geçmiş kitaplığı çatallamadan silinemiyor.

Geçmişe dokunan diğer durumlar ve her birinin temizlemede ne görmesi
gerektiği:

| durum | yeri | temizlemeye karşı |
|---|---|---|
| doldurma bandı | `Session::fill_rows`, `fill_shown` | silinen satır geri gelmemeli |
| kayma sayısı | `scrolled_rows`, `row_identity` | halkanın dönmesi "patlama" diye süzülmemeli |
| son `2J` kalıntısı | `Session::clear_boundary` (tampon adresi) | silinen tampon yeniden verilir → sıfırlanmalı |
| kaydırma kesri/süzülme | `reset_scroll`, `scroll_glide` nesli | pencere dibe, uçuştaki pay düşer |
| seçim | alacritty `Term.selection` | `ClearMode::Saved` geçmişteki seçimi süzüyor |
| arama sayımı | `SearchIndex`, `LedgerMark.epoch` = PTY çıktı nesli (`session.rs` ~5100) | terminal tarafı temizlik çıktı üretmez → sayım bayat kalır |

## Motivasyon

Kullanıcı isteği (2026-09-25): "⌘K ile ekranın temizlenmesi olmuyor.
Developer'ın beklediği senaryoyu gerçekleştir … başka koymadığımız (macOS
terminal kullanıcısının beklediği) davranışlar var mı bak."

Beklenen senaryo (kararlaşmış, soru değil): Terminal.app'in **Clear to
Start**'ı / iTerm2'nin **Clear Buffer**'ı — ekran ve geçmiş silinir, o anki
prompt (ve yazılmakta olan satır) en üstte kalır, **hiçbir komut çalışmaz**,
yukarı kaydırınca eski çıktı yoktur. Komut koşarken de (`tail -f`) çalışır
ve koşan programın girdisine bayt yazmaz. bateri'de içerik tabana yaslı
olduğu için "en üstte" görüntüde "dipte, üstü boş" demek — bugünkü Ctrl-L'in
görüntüsü (`CLAUDE.md` → "İçerik pencerenin tabanına yaslanır").

### Envanter — macOS terminal kullanıcısının beklediği, bateri'de olmayan

Kanıt dosyası parantezde; "yok" menüde öğesi olmayan ve `keys.rs`'te yutulan
demek (Cmd izin listesi üç tuş, `FUNCTION_KEYS` yutuluyor).

| davranış | referans | bateri | maliyet | yeri |
|---|---|---|---|---|
| Clear to Start ⌘K | Terminal.app Edit, iTerm2 ⌘K | yok (`menu.rs`) | orta: `bt-core`'da temizleme + menü | **bu set** |
| Clear Scrollback ⌥⌘K | Terminal.app Edit | yok | aynı mekanizmanın yarısı | **bu set** |
| Scroll to Top/Bottom ⌘Home/⌘End | Terminal.app View | yok (Home/End `FUNCTION_KEYS`'te yutuluyor) | menü öğesi + `Scroll::Top/Bottom` | **bu set** |
| Page Up/Down ⌘PgUp/⌘PgDn | Terminal.app View | yalnız ⇧PgUp/PgDn (`keys::page_scroll`) | menü öğesi, `scroll_page` hazır | **bu set** |
| Paste Escaped Text ⌃⌘V | Terminal.app Edit | yok; kaçırma Finder damlasında var (`quote::shell_quote`) | menü öğesi, iki hazır parça | **bu set** |
| komutlar arası atlama ⌘↑/⌘↓, Select Between Marks, Clear to Previous Mark ⌘L, son çıktıyı kopyala | Terminal.app Edit ▸ Navigate Marks, iTerm2 shell integration | yok (`CLAUDE.md`: "Dock ve komutlar arası atlama henüz yok"); bloklar yalnız **görünür** çıpalardan çözülüyor | yeni tasarım: geçmişte çıpa taraması, blok **bölgesi** | yol haritası |
| ⌘-tık ile URL/dosya yolu açma, OSC 8 bağlantısı | Terminal.app, iTerm2, Ghostty; Metalterm `path_link` (`docs/ARASTIRMA.md`) | yok (OSC 8 yalnız blok çıpası) | algılama + vurgu + tıklama rotası | yol haritası |
| zil (BEL): görsel/sesli, arka sekmede işaret | Terminal.app, iTerm2 | yutuluyor (`session.rs` `Event::Bell` kolu) | ayar + görsel efekt + sekme işareti | yol haritası |
| Reset / Hard Reset ⌥⌘R / ⌃⌥⌘R | Terminal.app Shell | yok | RIS'in dock/bayrak/blok etkileşimi ayrı tasarım | yol haritası |
| Home/End, Option-as-Meta | — | zaten yol haritasında (`docs/YOL-HARITASI.md` → "Klavye kalanları") | — | değişmez |

Bu sete girenlerin ortak paydası: hepsi **tek menü öğesi** ve ya ⌘K'nin
mekanizmasını ya da depoda hazır bir parçayı (`scroll_page`, `Scroll::Top`,
`shell_quote`, `Session::paste`) kullanıyor. Yol haritasına gidenler kendi
tasarımını istiyor.
