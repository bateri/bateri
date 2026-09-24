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

- [x] Sütun yürüyüşü iteratörü; `render`/`diff` ona geçer
- [x] `DockSelection`, çizilen pencerenin izi, `Dock::selection`
- [x] Kelime fonksiyonu + alacritty ile karşılaştırma sınaması
- [x] `view.rs` isabet testi, tek sahip, ⌘C/⌘A
- [x] `bt-gpu` dock seçim listesi
- [x] Test: eşleme, kelime paritesi, temizleme kuralları
- [x] `CLAUDE.md`
- [x] Doğrulama geçti (`make hepsi`, `make test-yaris`, `make duman`)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi

## Uygulama Notları

- Yürüyüş `dock::columns`: `render`'ın hücreleri, silmenin hayaletleri ve
  isabet testi (`dock::hit`) onu okuyor. `diff` yürüyüşe **geçmedi** — sütun
  yürümüyor, yalnız `window_skip`'i paylaşıyor.
- `render` → `render_with` (seçim parametresi, dönüşte `skip`); eski
  imza `#[cfg(test)]` sarmalayıcı, sınamalar dokunulmadan kaldı.
- İz yaprak bir yuvada ama `shell`'de değil, ayrı kilitte
  (`Session::dock_window`: kayma, sütun, `BUFFER` **bayt** boyu): kayma
  pencerelemeden sonra doğuyor ve `shell`'e ikinci tur yerine ayrı kilit.
  Canlı `BUFFER` iz ile aynı boyda değilse tık seçim kurmuyor.
- Uçlar `DockPoint { index, half }` (alacritty'nin `Anchor`'ı): `Simple`
  yarıdan sınır çiziyor, yani sürüklemesiz tık boş seçim (phase-5'in
  tıkla-caret'i buna dayanacak). Aralık mutasyonda bir kez çözülüp seçimin
  yanında saklanıyor; kare yolu kelime aramıyor.
- API planın adlarından sapıyor: `dock_select(kind, col, half)`,
  `dock_extend` (Shift+tık, seçim yoksa başlatır), `dock_drag` (sürükleme,
  seçimsiz sessiz); `dock_selection_text` / `dock_select_all` özel —
  `selection_text` ve `select_all` sahibe soruyor, view'ın `copy:`/`selectAll:`
  gövdesi değişmedi. ⌘A'nın sahibi `frame()`'in yayınladığı
  `caret_in_dock` (atomik, `alt_screen` emsali).
- ⌘A boş dock satırında ızgaraya düşüyor (seçilecek bir şey yok; Terminal.app
  normu). Üçlü tık kopyası `\n` **taşımıyor** (ızgaranın `Lines`'ı taşıyor):
  kabuğa geri yapıştırılan satır çalışmasın.
- Seçili dock hücresi ızgaranın kuralıyla: zemin düşük, `standout` çözülmüş
  (zsh'in yapıştırma vurgusu varsayılan `standout`).
- Orkestratör/kullanıcı eki: seçimin yarıçapı kendi sabitine çıktı —
  `bt_gpu::frame::SELECTION_RADIUS = 0.22` (hücre yüksekliği oranı;
  caret'in 0.10'u; 16pt@2x'te ≈8.6 px, tek hücrede kısa kenarın yarısına
  kırpılı). Karar 10'un "caret'in varsayılanı" yarısı buna göre değişti;
  `cursor_radius` hâlâ dokunmuyor. İki renderer bekçisinin sayıları güncellendi.
- Gözle (geçici paket, 16pt, koyu ve açık tema): dock'ta çift tık yol
  (`~/src/a-b.rs`), paranteze çift tık `(x y)`, ayırıcıya çift tık iki yandaki
  kelime (alacritty kuralı), üçlü tık bütün satır, sürükleme; pano metinleri
  doğru. Izgarada sürükleme dock seçimini kaldırıyor, ⌘A dock'u seçiyor,
  yazmak seçimi kaldırıyor. Yeni yarıçapla çok satırlı şekil (içbükey köşe
  dahil) temiz, tek karakter hap biçiminde ama bozulmadan.
- `/code-review` (medium) iki bulgu, ikisi de düzeltildi: (1) yürüyüş
  `filter_map`'le sağ yakada **durmuyordu** — sığmayan geniş karakterden
  sonraki dar karakter onun sütununa kayıyordu (`render`'ın eski `break`'i);
  `map_while` + bekçi `the_walk_stops_at_a_wide_char_that_does_not_fit`.
  (2) boşluğa/öneriye çift tık son kelimeyi seçiyordu; boşluk artık
  `len + uzaklık`'a iniyor, yani ızgaradaki gibi yalnız bitişik sütun son
  kelimeyi alıyor.
- `make hepsi` iki koşuda bir kez `bt-shell` lib sınamalarında bilinen
  SIGSEGV ile düştü (021/026/030 notları), yeniden koşu yeşil.
