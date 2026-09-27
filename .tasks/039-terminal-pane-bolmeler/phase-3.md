# Phase 3 — Bölme: ağaç, kapsayıcı, ⌘D / ⇧⌘D, kapanış ve sekme düzeyi

## Özet

Saf bölme ağacını ve onu çerçevelere çeviren kapsayıcıyı ilk tüketicisiyle
(⌘D / ⇧⌘D) indir; pane kapanışı, odak, kimlik ve sekme düzeyindeki
toplamaları çok pane'e aç.

_Requirements: R3.1, R3.2, R3.3, R3.4, R3.5, R5_

## Değişiklikler

- **`crates/bt-shell/src/split.rs`** (yeni, saf) — İkili ağaç: yaprak pane
  kimliği, düğüm eksen + oran (Karar 6). İşlemler: bir yaprağı bir eksende
  ikiye bölmek (oran yarı), yaprağı kaldırıp kardeşi yukarı çekmek,
  sınırlarından çerçeveleri hesaplamak (ayırıcı kalınlığı düşülerek, piksel
  ızgarasına oturtulmuş), sıra (derinlik öncelikli) ve kapanışta odağın
  geçeceği komşu. AppKit görmüyor; kendi sınamaları. Phase-4'ün işlemleri
  (yön, boyut, eşitle, büyüt) buraya eklenecek — şimdi yazılmıyor,
  `dead_code` kapıyı kırardı.
- **`crates/bt-shell/src/split_view.rs`** (yeni) — `contentView` olan düz
  kapsayıcı `NSView`: ağacın çerçevelerini pane'lere uyguluyor
  (`setFrame`; pane'in kendi çerçeve bildirimi geometriyi zaten tazeliyor)
  ve ayırıcıları bir piksel, temanın `separator` tonunda çiziyor (R3.5,
  Karar 7). Tek pane'de ayırıcı yok ve pane kapsayıcıyı dolduruyor —
  bugünkü düzenin aynısı.
- **`crates/bt-shell/src/window.rs`** — `TerminalWindow` pane listesini ve
  ağacı tutuyor; odaktaki pane = pencerenin first responder'ının pane'i.
  Bölme: odaktaki pane'in `PaneLaunch`'ı `Opening::Split` ile (Karar 9)
  ve bölünme sınırı (Karar 14) — sınırın altına düşecekse no-op.
  Kapanış (R3.2): ⌘W odaktaki pane'i sorarak kapatır, son pane'de sekmenin
  bugünkü yolu; kabuk çıkınca (`PaneHost`'un kabuk-çıktı olayı) yalnız o
  pane; odak ağaçtaki komşuya. ⇧⌘W ve kırmızı düğme bütün pane'ler, soru
  `foregrounds_to_ask`'e pane listesiyle (tek pane'de metin bayt bayt aynı;
  çok pane'de "pane" sayar). Başlık, `⇄`, yükleme yüzdesi ve sekme noktası
  odaktaki pane'den ve odak değişince tazeleniyor (R3.3). Örtülme, ölçek ve
  key biti bütün pane'lere.
- **`crates/bt-shell/src/app.rs`** — `Opening::Split(axis)` kolu;
  `newWindow:`/`newTab:` "etkin pane"den okuyor (bugünkü `key_window` →
  odaktaki pane). `bateri://tab/<id>` pane'i buluyor, penceresini öne
  getirip pane'i first responder yapıyor (R3.4, Karar 10); her pane doğumda
  kendi `TabId`'si. ⌘Q sorusu pane'lerden. `key_remote_mark` odaktaki pane.
- **`crates/bt-shell/src/menu.rs`** — Shell ▸ Split Right (⌘D), Split Down
  (⇧⌘D); ⌘W'nin başlığı çok pane'de "Close" (Karar 8; `validateMenuItem:`
  başlığı güncelliyor, tek pane'de "Close Tab" kalıyor).
- **`crates/bt-shell/src/lib.rs`** — başlıkta "Bölme … sonraki setlerde"
  cümlesi düşüyor.
- **`CLAUDE.md`** — bölmelerin sözleşmesi (pane başına oturum/link/renderer,
  odaktaki pane'in kuralları, ⌘W'nin anlamı, pane başına `TERM_SESSION_ID`,
  URL'nin pane'i odaklaması) ve "bir pencere = bir oturum" cümlesinin yeni
  hâli.
- **`docs/YOL-HARITASI.md`** — "bölme" satırı 039'a bağlı (set açılırken
  yazıldı); bedelin keşifte `bt-shell`'e indiği cümlesi.

## Kabul

- `make hepsi` yeşil: `split.rs` sınamaları (böl → iki yaprak eşit;
  kaldır → kardeş yukarı; çerçeveler ayırıcı dahil sınırı tam kaplıyor ve
  örtüşmüyor; kapanışta komşu), kapatma metinlerinin tek pane'de değişmediği
  ve çok pane'de pane saydığı.
- `make duman` jeton değerleri aynı (süreli koşu bölmüyor, Karar 12).
- Gözle: ⌘D ve ⇧⌘D; her pane'de kendi dock'u, doldurma bandı ve blok
  işaretleri; yeni pane odaktakinin dizininde (ssh pane'inde aynı host'a);
  odakta olmayan pane'de içi boş caret; `exit` yalnız o pane'i kapatıyor;
  koşan işli pane'de ⌘W soruyor; son pane'de ⌘W sekmeyi kapatıyor;
  başlık odakla değişiyor; `echo $TERM_SESSION_ID` iki pane'de farklı ve
  `open bateri://tab/<id>` o pane'i odaklıyor.

## Checklist

- [ ] `split.rs` saf ağaç + sınamalar
- [ ] `split_view.rs` kapsayıcı + ayırıcı
- [ ] ⌘D / ⇧⌘D, devralma, bölünme sınırı
- [ ] Pane kapanışı (⌘W, kabuk çıkışı), odak komşuya, son pane → sekme
- [ ] Sekme düzeyi toplamalar (başlık, soru, örtülme, ölçek, odak)
- [ ] Pane başına kimlik ve `bateri://` pane'i odaklıyor
- [ ] Menü öğeleri
- [ ] `CLAUDE.md`, `lib.rs`, `docs/YOL-HARITASI.md`
- [ ] Doğrulama geçti (`make hepsi`, `make duman`)
