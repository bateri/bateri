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

- [x] `split.rs` saf ağaç + sınamalar
- [x] `split_view.rs` kapsayıcı + ayırıcı
- [x] ⌘D / ⇧⌘D, devralma, bölünme sınırı
- [x] Pane kapanışı (⌘W, kabuk çıkışı), odak komşuya, son pane → sekme
- [x] Sekme düzeyi toplamalar (başlık, soru, örtülme, ölçek, odak)
- [x] Pane başına kimlik ve `bateri://` pane'i odaklıyor
- [x] Menü öğeleri
- [x] `CLAUDE.md`, `lib.rs`, `docs/YOL-HARITASI.md`
- [x] Doğrulama geçti (`make hepsi`, `make duman`)

## Uygulama Notları

- **Sapma — ağaç ve pane listesi kapsayıcıda** (`SplitView`), pencerede
  değil: kapsayıcının boyu pencereden bağımsız değişiyor (sekme çubuğu) ve
  o bildirimi alan `resizeSubviewsWithOldSize:` onun; ağaç pencerede
  dursaydı view her boy değişiminde pencereye geri uzanırdı. Pencere
  kapsayıcıdan okuyor (`panes()`, `halves`, `insert`, `remove_leaf` +
  `detach`). Kapsayıcı `isFlipped` (ağaç üstten aşağı), `resizeSubviews`'ı
  kendisi karşıladığı için dolgunun çerçevesini de elle kuruyor. Tek pane'de
  oturtma yok: pane sınırın ta kendisi (bölmeden önceki düzen).
- **Ayırıcı bir boşluk**: pane'ler opak, aralarında bir aygıt pikseli açık
  ve oradan pane'lerin arkasındaki tek `NSBox`'ın dolgusu görünüyor
  (`drawRect:` ve `CGColor` yok; tek pane'de kutu gizli). **Sapma —
  `bt-core`'a dokunuldu**: `NSColor` sRGB istiyor ve yalnız
  `separator_linear` vardı; `Theme::separator_srgb` eklendi, iki çıkış tek
  zincirden (`separator_rgb`), bekçisi `color::tests::separator_has_one_source`.
- **Odak**: `focused_pane` pencerenin first responder'ından üst view zinciriyle
  pane'e çıkıyor (arama alanının alan düzenleyicisi de pane'in torunu);
  first responder bir pane'de değilse son odaklanan (`focused` ivar'ı), o da
  yoksa ilk pane. Başlığın tazelenmesi için `PaneHost`'a `focused(pane)`
  eklendi (klavyenin **gelişi**, `keyboard_moved(true)`; gidişi değil, arama
  alanına geçen klavye aynı pane'de). `becomeFirstResponder` sırasında
  `firstResponder` henüz güncel değil, o yüzden olay kimlikle geliyor ve
  başlığın tazelenmesi bir ana kuyruk turu erteleniyor (`focused_pane`
  first responder'ı ivar'dan önce soruyor; o an okusaydı eski pane'in
  başlığını yazabilirdi).
  Pencere kurucusunda pencere listede olmadığı için olay düşüyor; ivar
  kurucuda tohumlanıyor. **Bilinen sınır**: başka bir pane'in arama alanına
  doğrudan tıklamak başlığı tazelemiyor (olay yalnız `BateriView`'dan);
  menü eylemleri ve bölme doğru pane'i first responder'dan buluyor.
- **Sapma — `Opening::Split` yüksüz**: `initial_line` ekseni kullanmıyor,
  eksen `AppDelegate::open_split`'in argümanı. Doğum paketi `pane_launch`'a
  ayrıldı (pencere doğuran yol ve bölme tek kaynak); miras etkin
  pencerenin **odaktaki pane'inden**. `open_window` kromu artık
  `set_theme` ile boyuyor (pane'ler oturumsuz, no-op; ayırıcının rengi de
  aynı çağrıdan).
- **Bölünme sınırı** `TerminalPane::grid_fits` (odaktakinin hücresi ve dock
  payı, `split_into_grid`) ve iki yarı çerçeve hesabının aynı aritmetiğinden
  (`split::split_halves`); sabitler `MIN_PANE_COLS = 20`,
  `MIN_PANE_ROWS = 5` (dock hariç), tasarım sabiti, gözle kontrolde
  ayarlanacak. Menü öğesi sınırda gri, eylem no-op.
- **Kapanış**: ⌘W çok pane'de `close_pane_asking` (`CloseScope::Pane`,
  "Close this pane?"), onayda `CloseTarget::Pane`; sekme soruları
  `CloseTarget::Tabs`. Soru pane'lerden (`foregrounds_to_ask` artık pane
  listesi alıyor) ve birim `unit_for(pane, sekme)`: tek pane'li sekmelerde
  metin bayt bayt aynı (eski sınamalar `Unit::Tab` ile değişmeden geçiyor).
  `close_pane` odaktaki pane kapanıyorsa odağı **sökümden önce** komşuya
  veriyor. `begin_close` pencerede pane başına sonuç (`Vec<Option<Closing>>`),
  `shutdown` düzleştiriyor (ilk pane'in sonucu raporda). **Bilinen sınır**:
  pane sorusu açıkken o pane'in kabuğu çıkarsa sayfa açık kalıyor, onayı
  no-op (pane zaten yok).
- ⌘W'nin başlığı `TerminalWindow`'un yeni `validateMenuItem:`'ında
  (`close_title`); terminal olmayan pencere key iken bölmeli sekmenin
  bıraktığı "Close" kalmasın diye `AppDelegate`'e de `validateMenuItem:`
  eklendi (yalnız başlığı sıfırlıyor, cevabı hep `true`).
- `bateri://tab/<id>`: `AppDelegate::pane_by_tab` (pencere + pane) →
  `TerminalWindow::bring_to_front(pane)` klavyeyi o pane'e veriyor.
  `TabId` zaten pane başınaydı (phase-1).
- Test-first kısmen: `split` sınamaları uygulamayla aynı turda yazıldı; ilk
  koşuda biri kırmızıydı (1×'te kesirli nokta sınırı piksele iniyor —
  beklenti oturtulmuş alana düzeltildi, kod değil). Kapatma metni
  sınamaları (`one_pane_per_tab_keeps_the_tab_wording`,
  `many_panes_are_counted_as_panes`, `one_pane_asks_about_the_pane`) imza
  değişimiyle birlikte.
- Riskli phase tetikleyicisi yok (PTY okuyucu, render thread, `.metal`,
  kilit dosyası dokunulmadı): `/code-review` set kapısında.
- Duman öncesi/sonrası aynı: `kare=29 hucre=8 glif=6 kural=15 yuva=13/1984
  yuva2=0/1984 yuk=smoke istek=4 icerik=2 hareket=27 kayma=0 kapanis=clean
  pipeline=ok`.
- Gözle kontrol bu otonom koşuda yapılmadı — set kapısında: ⌘D/⇧⌘D, her
  pane'de dock/doldurma bandı/blok işaretleri, yeni pane'in dizini (ssh
  pane'inde aynı host), odaksız pane'de içi boş caret, **tıkla odak
  değişiyor mu** (`BateriView` `mouseDown:`'ı override ediyor; first
  responder'ı AppKit'in `sendEvent:`'i veriyor olmalı), `exit` yalnız o
  pane, koşan işli pane'de ⌘W sorusu, son pane'de ⌘W sekmeyi kapatıyor,
  başlık odakla, iki pane'de `echo $TERM_SESSION_ID` farklı ve
  `open bateri://tab/<id>` o pane'i odaklıyor, ayırıcının tonu ve
  Retina'da keskinliği, en küçük pane sınırının sayıları.
