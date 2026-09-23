# Phase 4 — Dock seçimi (terminal tarafı)

## Özet

Dock'un giriş satırı fareyle seçiliyor (sürükleme, çift, üçlü tıklama),
ızgarayla aynı görünüşte; pencere başına tek seçim, ⌘C ve ⌘A sahibe gidiyor.
Kabuğa hiçbir şey gönderilmiyor.

_Requirements: R3.1, R3.2, R3.3, R3.4, R3.5_

## Değişiklikler

- **`crates/bt-core/src/dock.rs`** — `render`'ın döngüsündeki sütun yürüyüşü
  (sıfır genişlik atlama, sol/sağ yaka, geniş karakter) bir iteratöre çıkar
  ve `render`, `diff` ve isabet testi onu tüketir — üçüncü bir kopya yazılmaz
  (024'ün "tek tablo" gerekçesi). `Dock` yüzeyine `selection: Option<(u16,
  u16)>` (ekran sütunu, pencerelenmiş; `Dock::caret` emsali). Kelime sınırı
  arayan saf fonksiyon phase-1'in `WORD_SEPARATORS`'ını okur ve alacritty'nin
  `Semantic` davranışını kopyalar (parantez eşleme, ayırıcı üstünde çift
  tıklama); sınama iki uygulamayı aynı dizgilerde karşılaştırır.
- **`crates/bt-core/src/shell.rs`** — `DockSelection { anchor, head, kind }`
  (`BUFFER`'ın karakter indeksleri) aynanın **yanında**, `DockState`'in içinde
  değil — `dock::change`/`diff` onu karşılaştırsaydı her sürükleme adımı
  030'un efektlerini `Reset`'lerdi. `BUFFER` değişen bir ayna seçimi siler.
  Son **çizilen** pencerenin izi (kayma + `BUFFER` uzunluğu) yaprak bir
  yuvada: isabet testi canlı aynaya değil ekrandakine bakar.
- **`crates/bt-core/src/session.rs`** — `Session::dock_select(col, kind)`,
  `dock_extend(col)`, `dock_selection_text()`, `dock_select_all()`; dock
  seçimi başlamak ızgara seçimini temizler ve tersi (tek sahip).
  `send_input` iki seçimi de temizler (tek huni). `Session::dock` çizdiği
  pencerenin izini yazar ve `Dock::selection`'ı doldurur. `PREDISPLAY` ve
  öneri (`POSTDISPLAY`) seçilmez; sürükleme satırın içine kırpılır.
- **`crates/bt-shell/src/view.rs`** — isabet testi: işaretçi dock bandının
  giriş satırındaysa (bant `bt_gpu::dock_px`'ten, satır/sütun `CellMetrics` ve
  `gutter_px`'ten) `Gesture` hedefi dock; tıklama sayısı ve sürükleme ızgarayla
  aynı defterden. Bağlam satırı ve bandın payı hiçbir şey yapmaz. `copy:` ve
  `selectAll:` sahibe sorar (dock caret'in sahibiyken ⌘A dock'u seçer). Fare
  kipi dock'a hiç uygulanmaz (bant uygulamanın ekranı değil).
- **`crates/bt-gpu/src/frame.rs` / `link.rs`** — dock geçişinde seçim listesi,
  ızgara seçim listesinin pipeline'ı ve köşe kararıyla (tek satır: dört köşe
  yuvarlak; phase-3 geri alınırsa düz dörtgene düşer); sıra zemin → seçim →
  caret → glyph.
- **`CLAUDE.md`** — dock paragrafına seçim (yanında yaşar, tek sahip, isabet
  testi çizilen aynaya), "Dock ve komutlar arası atlama henüz yok" cümlesinin
  bağlamı.

## Kabul

- Sınama: kaydırılmış (pencerelenmiş) bir `BUFFER`'da sütun→indeks eşlemesi
  geniş karakter ve sol/sağ yaka dahil `render`'ın çizdiğiyle aynı; öneriye
  düşen sütun `BUFFER`'ın sonuna.
- Sınama: dock'ta çift tıklama ile ızgarada çift tıklama aynı dizgide aynı
  aralığı verir (parantez ve ayırıcı vakaları dahil).
- Sınama: ayna `BUFFER`'ı değişince seçim kalkar; `send_input` iki seçimi de
  temizler; birinde başlamak ötekini temizler.
- `make hepsi`, `make test-yaris` yeşil; `make duman` jetonları değişmez.

## Checklist

- [ ] Sütun yürüyüşü iteratörü; `render`/`diff` ona geçer
- [ ] `DockSelection`, çizilen pencerenin izi, `Dock::selection`
- [ ] Kelime fonksiyonu + alacritty ile karşılaştırma sınaması
- [ ] `view.rs` isabet testi, tek sahip, ⌘C/⌘A
- [ ] `bt-gpu` dock seçim listesi
- [ ] Test: eşleme, kelime paritesi, temizleme kuralları
- [ ] `CLAUDE.md`
- [ ] Doğrulama geçti (`make hepsi`, `make test-yaris`, `make duman`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
