# Geçmişte arama (⌘F) — Bağlam

## Mevcut Durum

Geçmişte metin bulmanın bugün tek yolu gözle kaydırmak. Ana menüde Find yok
(`crates/bt-shell/src/menu.rs`: Edit'te yalnız Cut/Copy/Paste/Select All),
⌘F `keyDown:`'ın Cmd izin listesinin dışında olduğu için yutuluyor
(`CLAUDE.md` → `bt-shell` paragrafı), `bt-core`'da arama durumu yok.
`bt-gpu`'nun sorumluluk satırında "overlay'ler (palet, arama)" yazıyor ama
kod yok; yol haritasında "Sonrası (sırasız): palet ve arama overlay'leri".

Aramanın dayanacağı parçaların çoğu yerinde:

- **Eşleştirme alacritty'de hazır.** `alacritty_terminal 0.26`'nın
  `term::search` modülü `RegexSearch` (tembel DFA, ileri/geri dört otomat),
  `Term::search_next`, `Term::regex_search_left/right` ve `RegexIter`'i
  (`Match = RangeInclusive<Point>`) dışa açıyor. Arama ızgara hücrelerinde
  yürüyor: sarılan satırı (`WRAPLINE`) kesintisiz, sert satır sonunu sınır
  sayıyor, geniş karakterin spacer'ını atlıyor, defterin tamamını (`topmost_line`
  → dip) kapsıyor ve karmaşıklık sınırını aşan desen panik değil `None`
  (`warn!`). Altındaki `regex-automata 0.4.18` + `regex-syntax 0.8.11`
  **`Cargo.lock`'ta zaten var** (alacritty'nin bağımlılığı) — regex yeni bir
  bağımlılık değil.
- **Akıllı büyük/küçük harf de orada:** `RegexSearch::new` desende büyük harf
  yoksa `case_insensitive(true)` kuruyor. Satır içi bayrak (`(?-i)`) bu
  varsayılanı desen içinden eziyor; yani "her zaman duyarlı" düğmesi aynı
  API'den çıkıyor.
- **Vurgu şekli ve pipeline'ı 031'den:** `selection` pipeline'ı rengi
  **çağrı başına uniform** alıyor, instance'ın `rgba` yuvası köşe maskesi
  (`bt-gpu/src/renderer.rs` → `encode_selection`, `frame.rs` →
  `selection_corners`, `SELECTION_RADIUS`). İkinci ve üçüncü bir renk yeni bir
  shader değil, aynı pipeline'a ikinci bir encode. Seçili metnin rengi
  kuralı 031 `discussion.md` → Karar 3.
- **Pencereyi bir satıra götürmek 027'den:** `scroll_locked`, süzülme isteği
  (`Session::take_scroll_glide`, nesil), `ScrollIntent`, doldurma bandının
  `1..=fill` atlaması (`CLAUDE.md` → "Bant bir sanal kaydırmadır").
- **Kaydırmanın sayısı 011/017'den:** `row_identity` (satırın hücre
  tamponunun adresi) ve onunla bulunan "kaç satır geçmişe kaydı"
  (`Session::scrolled_rows`, `Cursor::scrolled`) — defter doysa da sayan tek
  ölçü.
- **Girdi pencereyi dibe döndürüyor** (`Session::write_owned`'ın doc'u):
  geçmişte bırakılan bir pencere ilk tuşta istemine dönüyor.
- **Pencere düzeni:** içerik view'ı doğrudan `BateriView` ve **layer-hosting**
  (`window.rs` → `TerminalWindow::new`: `setLayer` + `setWantsLayer`); başlık
  çubuğu saydam, pencere zemini temanın (`apply_chrome`). İçerik view'ının
  çerçevesi değişince `refresh_geometry` koşuyor ve PTY yeniden
  boyutlanıyor (`viewFrameDidChange:`).
- **AppKit metin alanı bedava:** `objc2-app-kit 0.3.2`'nin `NSSearchField`,
  `NSSearchFieldCell`, `NSSegmentedControl`, `NSVisualEffectView`,
  `NSTextFieldCell`, `NSActionCell`, `NSAnimationContext` özellikleri
  `bt-shell/Cargo.toml`'a eklenince **`Cargo.lock` değişmiyor** (ölçüldü,
  2026-09-25: depo `git archive`'la geçici dizine açıldı, özellikler eklendi,
  `cargo metadata --offline` koştu, kilit dosyası `diff` ile aynı; olmayan
  bir özellik adı aynı komutta hata veriyor, yani adlar gerçek).

## Motivasyon

**Kullanıcı isteği (2026-09-25):** "arama için bir akış oluştur; kullanıcı
deneyimi güzel olsun, UI ve UX temiz ve güzel olsun."

Referans davranış `docs/ARASTIRMA.md` → Ürün özellikleri ("scrollback'te
regex arama (GPU vurgulama)") ve envanterin `mt-gpu` satırı (`search`,
`overlay/find`); sitenin cümlesi: "⌘F Find in scrollback — literal or regex,
across your whole scrollback, highlighted on the GPU as you type. No lag on
the first keystroke, none on the last."

Referans çubuğu Metal'de çiziyor (`overlay/find`, `ui_text` pipeline'ı); bu
set o yolu seçmiyor — gerekçe `discussion.md` → Karar 1.

Bugünkü eksik günlük kullanımda hissediliyor: uzun bir derleme çıktısında
hata satırını, bir `git log`'da hash'i, Claude Code'un araç çıktısında bir
yolu bulmak, tekerlekle binlerce satır taramak demek.

**Maliyet sorusu açık ve ölçülmedi.** 10 000 satırlık bir defterin `Term`
kilidi altında tek seferde taranması PTY okuyucusunu ve kare yolunu (ikisi
de aynı `FairMutex`'i alıyor) ne kadar durdurur, bilinmiyor — sayı yok,
tahmin de yazılmıyor. Tasarım bu yüzden sayıdan bağımsız güvenli olmak
zorunda (sınırlı parça, iptal) ve parçanın boyu ilk phase'in ölçümünden
türüyor.
