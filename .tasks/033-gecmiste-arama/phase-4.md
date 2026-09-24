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

- [ ] Panel görünüşü, anahtarlar, etiket, animasyon + Hareketi Azalt
- [ ] Menü öğeleri, `TerminalWindow` eylemleri ve `validateMenuItem:`
- [ ] Alan delegesi: ⏎/⇧⏎/Esc
- [ ] Odak iki bit
- [ ] `search_next` + reveal + süzülme; Esc→seçim; ⌘E + find panosu
- [ ] Gezinmenin (`search_next`) hedefi vurgunun kümesinden (phase-1'den devir): bastırılan satıra değen ya da mürekkepsiz eşleşme atlanır; desen `SearchSlot`'tan ödünç ya da ayrı bir kopya (yuva bugün tek desen tutuyor ve kare onu ödünç alıyor); ⌘E'nin kaçırması `bt_core::escape_search`
- [ ] Test: yukarıdaki hermetik senaryolar
- [ ] Doğrulama geçti (`make hepsi` + `make duman`)
