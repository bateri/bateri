# Phase 4 — Arama paneli, menü ve gezinme

## Özet

⌘F sağ üstte temaya uyan paneli açar; yazım vurguyu sürer, ⏎/⌘G eşleşmeler
arasında pencereyi süzerek gezer, Esc kapatıp eşleşmeyi seçim bırakır. Sayım
bu phase'de görünür eşleşmelerle sınırlı.

_Requirements: R4, R5, R6, R7_

## Değişiklikler

- **`crates/bt-shell/src/search_bar.rs`** (yeni) — panel: `NSSearchField`,
  `Aa` ve `.*` anahtar düğmeleri, sayım etiketi (monospace rakam), yukarı/aşağı
  ok, kapatma. Zemin temanın zemininden türetilmiş bir yüzey, ince kenar,
  yuvarlak köşe, hafif gölge; alan sistem kontrolü, görünümü pencereden.
  Sağ üstte iç payla, kapsayıcının çocuğu, `BateriView`'ın üstünde. Açılış ve
  kapanış `NSAnimationContext` ile belirme + kısa kayma; süre bir tasarım
  sabiti, 240 ms tabanından başlar ve gerçek pencerede ayarlanır; Hareketi
  Azalt'ta animasyonsuz. Alanın delegesi: metin değişimi → `set_search`;
  `control:textView:doCommandBySelector:` → `insertNewline:` (⇧ ile ters
  yön) ve `cancelOperation:` (kapat; `NSSearchField`'ın "metni sil"
  varsayılanı yerine). Durum sekme başına: sorgu ve iki anahtar kapanınca
  unutulmaz. Etiket: "N matches" görünür sayım; "No matches"; "Invalid
  pattern" (UI dizgileri İngilizce).
- **`crates/bt-shell/src/menu.rs`** — Edit ▸ Find alt menüsü: Find… (⌘F),
  Find Next (⌘G), Find Previous (⇧⌘G), Use Selection for Find (⌘E). Seçiciler
  kendi adlarımız (`performFindPanelAction:` değil — alan düzenleyicisi onu
  yutar).
- **`crates/bt-shell/src/window.rs`** — eylemler `TerminalWindow`'da
  (alan odaktayken responder zinciri `BateriView`'dan geçmiyor); yeni
  `validateMenuItem:` (sorgu yokken Find Next/Previous kapalı; bilinmeyen öğe
  `true`). ⌘F: panel açık değilse açar, alanı odaklar, metni seçer; sorgu
  yoksa find panosundan. ⌘E: seçim (ızgara ya da dock, `Session::selection_text`)
  sorgu olur, regex kipinde kaçırılarak, find panosuna da yazılır. Esc ve
  kapatma: panel gider, klavye `BateriView`'a, geçerli eşleşme ızgara seçimi
  olur (`Session::set_selection`), pencere yerinde kalır.
- **Odak iki bit** — `crates/bt-gpu/src/link.rs`'e "klavye terminalde"
  girdisi: caret odaksızdır = pencere key değil **ya da** klavye terminalde
  değil; vurgu ve seçim solması yalnız pencere key değilken. `apply_focus`'un
  key değişimi ile alanın first responder olması iki ayrı çağrı; birleştirme
  `bt-shell`'de değil `bt-gpu`'da (tek yer).
- **`crates/bt-core/src/session.rs`** — `search_next(yön)`: geçerli
  eşleşmeden `Term::search_next` ile (yukarı = daha eski), sarar; görünürlük
  (panelin kapladığı satırlar görünür sayılmaz, bant satırları sayılır) ve
  hedef ofset `Term` kilidi altında; bir ekran içinde süzülme isteği (027'nin
  `add_glide`'ı ve nesil kuralı), uzakta `scroll_locked` ile hedefin bir ekran
  yakınına konup kalan süzülme; `smooth_scroll` kapalıyken doğrudan. Geçerli
  eşleşme yazarken "başlanan pencerenin altından yukarı ilk eşleşme".
  Panelin kapladığı satır sayısını çağıran verir (`bt-core` piksel görmüyor).
- **`crates/bt-shell/Cargo.toml`** — `objc2-app-kit` özellikleri
  (`NSSearchField`, `NSSearchFieldCell`, `NSTextFieldCell`, `NSActionCell`,
  `NSAnimationContext`, gerekirse `NSSegmentedControl`); `Cargo.lock`
  değişmemeli — değişirse dur ve eskale et.

## Kabul

- Hermetik: `search_next` yönü ve sarması, görünür eşleşmede ofset
  değişmiyor, panel altındaki eşleşmede değişiyor, bant eşleşmesi görünür;
  uzak hedefte konma + süzülme payı; `snap` kolunda doğrudan; ⌘E'nin regex
  kipinde kaçırması.
- `make hepsi` ve `make duman` yeşil; `Cargo.lock` değişmedi.
- Gözle kontrol (kapanış mesajında; `make kur` ile gerçek pencere):
  ⌘F/yazım/⏎/⇧⏎/⌘G/⇧⌘G/Esc/⌘E; ölü tuş ve ⌘V alanda; alan odaktayken caret
  odaksız, vurgu tam renkli; açık ve koyu temada panelin görünüşü;
  animasyonun hissi; bant satırında ve alternatif ekranda (less) vurgu; dock'ta
  vurgu yok; panel açık ve boştayken kare yok (`BT_FRAME_STATS` gerekmez,
  `icerik=` değil gözle: imleç ve CPU); uzun defterde yazarken tuş gecikmesi.

## Checklist

- [x] Panel görünüşü, anahtarlar, etiket, animasyon + Hareketi Azalt
- [x] Menü öğeleri, `TerminalWindow` eylemleri ve `validateMenuItem:`
- [x] Alan delegesi: ⏎/⇧⏎/Esc
- [x] Odak iki bit
- [x] `search_next` + reveal + süzülme; Esc→seçim; ⌘E + find panosu
- [x] Gezinmenin (`search_next`) hedefi vurgunun kümesinden (phase-1'den devir): bastırılan satıra değen ya da mürekkepsiz eşleşme atlanır; desen `SearchSlot`'tan ödünç ya da ayrı bir kopya (yuva bugün tek desen tutuyor ve kare onu ödünç alıyor); ⌘E'nin kaçırması `bt_core::escape_search`
- [~] Arama vurgusunun renkleri gerçek pencerede, iki gömülü temada (phase-2'den devir) — computer-use başka bir oturumca meşguldü ve süreç ekran kaydı izni taşımıyor (`screencapture -l` reddedildi); panelin kontrolleri offscreen dökümle iki temada görüldü, yüzeyin zemini ve vurgu renkleri görülemedi → phase-5'in gözle kontrolüne devredildi. Vurgu rengi artık key bitinden ("Odak iki bit")
- [x] Test: yukarıdaki hermetik senaryolar
- [x] Doğrulama geçti (`make hepsi` + `make duman` + `make test-yaris`)
- [x] Riskli phase (`make test-yaris`: yuvaya ikinci `Term` kilidi sahibi): `/code-review` koştu, iki bulgu düzeltildi (aşağıda)

## Uygulama Notları

- **Test-first sırası:** sınamalar uygulamadan sonra yazıldı; ısırdıkları
  mutasyonla gösterildi (kümenin dışını atlamamak, ⏎'nin yönünü çevirmek,
  görünürlük kapısını kaldırmak ve uzak hedefte konmayı kaldırmak ilgili
  sınamaları kırdı).
- **Geçerli eşleşme yuvada durum** (`SearchSlot::current`, mutlak `Match`):
  `set_search` onu seçiyor (pencerede çizilen en alttaki, yoksa aramanın
  başladığı pencerenin dibinden yukarı ilk), gezinme taşıyor, kare onu
  `same_place` ile işaretliyor (açgözlü desende iki yönün farklı uca
  durabilmesi yüzünden uçlardan biri yetiyor). phase-1'in "görünürdeki son"
  kuralı kalktı. **Bilinen sınır:** mutlak satır çıktıyla kayıyor; içeriğe
  yapışma phase-5'in işi.
- **Bastırılan satırlar yuvaya karenin cevabı olarak iniyor**
  (`SearchSlot::hidden`, mutlak `Line`): ikinci kez türetilmiyor. Arama
  kapalıyken de yazılıyor (içerik karesi başına yarışmasız bir yaprak kilit),
  yoksa ilk sorgunun kararı bastırılan satırı göremezdi.
- **Esc→seçim `Session::set_selection`'dan geçmiyor** (plan onu adlandırmıştı):
  `SelectionPoint` görünür pencerenin `u16` satırı ve eşleşme bantta
  (negatif satır) olabiliyor; yeni `Session::select_search_match` seçimi
  mutlak uçlardan kuruyor.
- **Panelin örttüğü alan** `SearchCover { first_row, from_col }` — ilk tam
  görünür satır ızgaranın 0. satırına göre (negatif = bandın satırları) ve
  örtülen satırlarda panelin sol kenarının sütunu; panelin solundaki eşleşme
  görünür sayılıyor. Varsayılanı "hiçbir şey örtülmüyor" (`i32::MIN`), `0`
  bandı örterdi. Çeviri `view::cover_of` (saf, `point_to_cell`'in
  aritmetiği), ölçü panelin **durduğu** çerçeveden (animasyon yolda olabilir).
- **Etiketin sayısı varılacak pencerede** (`SearchReport::visible`): kareden
  kabuğa kanal kurulmadı; sayı gezinme ve yazım anında hedef ofsetin
  penceresinde sayılıyor. Çıktı akarken ya da tekerlekle kaydırınca bayatlıyor
  — phase-5'in bütün defter sayımı ve `Wake` haberi onu değiştiriyor.
- **Klavye biti tek kaynaktan:** `BateriView`'ın `becomeFirstResponder`/
  `resignFirstResponder` kancaları → `TerminalWindow::keyboard_moved` →
  `DisplayLink::set_keyboard_in_terminal`; ⌘F, Esc ve terminale tık aynı
  yoldan.
- **⌘G panel kapalıyken** paneli odağı çalmadan açıp arıyor; sekmede sorgu
  yoksa find panosunun metni (Karar 6). Find Next/Previous'ın kapısı "sekmenin
  sorgusu ya da find panosunda metin".
- **⌘E seçimin ilk satırını** alıyor: arama sert satır sonunu aşmıyor, sonraki
  satırlar hiçbir şeyle eşleşemezdi. Panoya düz metin, alana (regex kipinde)
  kaçırılmış hâl.
- **Görünüş sayıları:** alan 190 pt, anahtar/ok 24 pt, sayım 84 pt (etiket
  değişince panel eni zıplamıyor), köşe 11 pt, pay 7×6 pt, sağ üstten 10 pt;
  yüzey temanın zemininin ön plana %11 (koyu) / %4.5 (açık) karışımı, kenar
  ön planın %16/%14'ü, hafif gölge. Açılış/kapanış 180 ms (240 tabanından
  kısaltıldı: panel küçük ve hareketi 6 pt) — **gerçek pencerede
  ayarlanamadı**, gözle kontrolde bakılacak.
- **Bayraklar:** `objc2-app-kit`'e `NSActionCell`, `NSAnimation`,
  `NSAnimationContext`, `NSButtonCell`, `NSSearchField`, `NSSearchFieldCell`,
  `NSShadow`, `NSTextFieldCell`; `objc2-quartz-core`'a
  `CAMediaTimingFunction`. `Cargo.lock` değişmedi. Yüzey `NSBox` (katmanın
  `CGColor`'ı `objc2-core-graphics` kenarı isterdi).
- **`/code-review` bulguları, ikisi de düzeltildi:** (1) görünür eşleşmede
  pencere oynamıyordu ama önceki gezinmenin uçuştaki süzülmesi de durmuyordu
  (⏎'den hemen sonra ⇧⏎ eşleşmeyi ekrandan taşırdı) — görünür kolda da nesil
  artıyor (`a_visible_match_stops_the_glide_toward_the_previous_one`); (2)
  Esc'ten sonra ⌘G/⇧⌘G paneli açarken sorguyu yeniden verip en yakın
  eşleşmeyi seçiyor, üstüne bir adım daha atıyordu (⇧⌘G en eskiye sarıyordu)
  — yeniden verilen sorguda adım o seçimin kendisi.
