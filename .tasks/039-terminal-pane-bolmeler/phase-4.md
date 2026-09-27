# Phase 4 — Gezinme, boyutlama, eşitleme, büyütme, soluk örtü

## Özet

Bölmeler arası klavye gezinmesini, klavye ve fareyle boyutlamayı,
eşitlemeyi, büyütmeyi ve odakta olmayan pane'lerin soluk örtüsünü ekle.

_Requirements: R4.1, R4.2, R4.3, R4.4, R5_

## Değişiklikler

- **`crates/bt-shell/src/split.rs`** — Yeni işlemler: yöndeki komşu
  (odaktaki pane'in çerçevesinin kenarından, dikey eksende en çok örtüşen;
  eşitlikte ağaç sırası), bir yönde boyutlama (o eksendeki en yakın atadaki
  oranı adım kadar oynatır, iki tarafın en küçük pane sınırında kırpılır —
  Karar 14), eşitleme (her düğümde oran, alt ağaçların yaprak sayısına göre,
  yani aynı eksendeki bütün pane'ler eşit), sürüklemenin oranı ve büyütülmüş
  yaprak (çerçeve hesabı büyütülmüşse yalnız onu tüm alana yayar).
- **`crates/bt-shell/src/split_view.rs`** — Ayırıcı sürüklemesi (çizilen
  çizgiden geniş isabet alanı, `resizeLeftRight`/`resizeUpDown` imleci,
  sürükleme boyunca oran ağaca yazılıyor ve pane'ler yeniden oturuyor;
  pane'in kendi geometri yolu PTY'yi boyutluyor). Büyütülmüşken ayırıcı yok
  ve öteki pane'ler gizli (`setHidden`; görünmeyen pane'in link'i örtülme
  gibi sıfır kare çizmeli — `set_visible(false)`).
- **`crates/bt-shell/src/pane.rs`** — Soluk örtü (Karar 7): pane'in içinde
  Metal katmanının kardeşi, `hitTest` → `nil` bir view; rengi temanın
  zemini, saydamlığı bir tasarım sabiti (doc'unda gerekçe). Görünürlüğü
  sahip belirler ("odakta değil ve pencerede birden çok pane"); kare yoluna
  ve `bt-gpu`'ya dokunmuyor. Tema değişince rengi tazelenir.
- **`crates/bt-shell/src/window.rs`** — Eylemler: `selectPreviousSplit:` /
  `selectNextSplit:`, yöne göre seçim, yöne göre boyutlama, `equalizeSplits:`,
  `toggleSplitZoom:`; bölme, gezinme ve kapanış büyütmeyi bırakır (Karar 8).
  Odak değişimi örtüleri tazeler.
- **`crates/bt-shell/src/menu.rs`** — Window menüsü: Select Previous/Next
  Split (⌘[ / ⌘]), Select Split ▸ (⌥⌘←↑→↓), Resize Split ▸ (⌃⌘←↑→↓),
  Equalize Splits (⌃⌘=), Zoom Split (⇧⌘↩); tek pane'de gri
  (`validateMenuItem:`).
- **`CLAUDE.md`** — gezinme/boyutlama/büyütme kısayolları, örtünün kare yolu
  dışında olduğu, en küçük pane kuralı; `keyDown:` Cmd izin listesinin
  değişmediği cümlesi.

## Kabul

- `make hepsi` yeşil: `split.rs` sınamaları (yöndeki komşu L şeklinde
  düzende doğru pane; boyutlama sınırda duruyor ve toplam alan korunuyor;
  eşitleme aynı eksendeki pane'leri eşitliyor; büyütülmüş yaprak tüm alanı
  alıyor, geri alınınca eski çerçeveler).
- `make duman` jeton değerleri aynı.
- Gözle: üç pane'de ⌘[ / ⌘] döngüsü ve ⌥⌘ oklarla yön; ⌃⌘ oklarla ve
  ayırıcıyı sürükleyerek boyutlama (vim açıkken satır/sütun sayısı
  güncelleniyor); ⌃⌘= eşitliyor; ⇧⌘↩ büyütüp geri alıyor, büyütülmüşken ⌘D
  büyütmeyi bırakıyor; odakta olmayan pane'ler hafif soluk, tek pane'de
  örtü yok; boşta pencerede kare sayacı artmıyor (örtü kare istemiyor).

## Checklist

- [ ] `split.rs`: yön, boyut, eşitle, büyüt + sınamalar
- [ ] Ayırıcı sürüklemesi ve imleci
- [ ] Büyütme: gizleme, `set_visible`, çıkış kuralları
- [ ] Soluk örtü (AppKit, kare yolu dışında)
- [ ] Menü öğeleri ve `validateMenuItem:`
- [ ] `CLAUDE.md`
- [ ] Doğrulama geçti (`make hepsi`, `make duman`)
