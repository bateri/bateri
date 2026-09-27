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

- [x] `split.rs`: yön, boyut, eşitle, büyüt + sınamalar
- [x] Ayırıcı sürüklemesi ve imleci
- [x] Büyütme: gizleme, `set_visible`, çıkış kuralları
- [x] Soluk örtü (AppKit, kare yolu dışında)
- [x] Menü öğeleri ve `validateMenuItem:`
- [x] `CLAUDE.md`
- [x] Doğrulama geçti (`make hepsi`, `make duman`)

## Uygulama Notları

- Test-first: yeni `split` sınamaları (yöndeki komşu L'de, sıra döngüsü,
  boyutlama en yakın ata + sınır + alan, iç içe tarafın sınırı, sürüklemenin
  yalnız o ayırıcıyı taşıması, eşitleme, büyütme) önce derlenmeyerek
  kırmızı. İlk yeşil koşuda iki sınama kendi hatasıyla düştü (alan
  toplamına ayırıcıların alanı girmemişti; iç içe sınamanın sürüklemesi 4'ü
  zaten sınırın altına itiyordu) — beklenti düzeldi, kod değil.
- **Eşitlemenin okuması**: planın iki cümlesi ("yaprak sayısına göre" ve
  "aynı eksendeki bütün pane'ler eşit") L düzeninde ayrışıyor; ikincisi
  seçildi — ağırlık o eksende yan yana duran pane sayısı (aynı eksende
  zincirlenen bölmeler çocuklarını sayıyor, öteki eksendeki alt ağaç 1).
  L'de sol pane yarı genişlik, üçte bir değil.
- **Sınır yaprak başına** (`Fn(u64) -> Size`, `TerminalPane::min_size`):
  punto farkı pane başına, hücre de. İç bölmelerin oranı sabit sayılıyor,
  yani bir tarafın sınırı toplam değil payı küçük olanın payından — her
  yaprak sınırda kalıyor. `grid_fits` artık `min_size`'ın tersinden (tek
  formül, `split_into_grid`'in tersi).
- **Boyutlamanın adımı** odaktaki pane'in bir hücresi (yatayda genişlik,
  dikeyde yükseklik): planda "adım kadar" yazıyordu, sayı fonttan.
  Yön Ghostty'nin: ok ayırıcının gideceği yön.
- **Boyutlama ve eşitleme de büyütmeyi bırakıyor** (planda bölme, gezinme,
  kapanış): gizli düzeni sessizce değiştirmek görünmez bir etki olurdu.
  `focus_pane` büyütülmüş başka bir pane'e klavye verirken önce büyütmeyi
  bırakıyor (`bateri://tab/`, kapanışın komşusu). Zoom Split öğesi
  büyütülmüşken onaylı.
- **Gizli pane'in link'i**: `windowDidChangeOcclusionState:` artık
  `SplitView::apply_visibility` — görünür **ve** gizli değil; büyütmenin
  kurulup bırakılması da aynı yoldan.
- **Tutamaklar** (`DividerHandle`): ayırıcı sayısı değişince yeniden kurulup
  en üste ekleniyor (yeni pane onların üstüne ekleniyordu), sürükleme
  boyunca aynı view. İmleç `resizeLeftRight`/`resizeUpDown` —
  kullanımdan kalkmış, yerini alan `columnResizeCursorInDirections:` macOS
  15'te; `#[allow(deprecated)]` gerekçesiyle. İsabet payı `HANDLE_PT = 3`
  (tasarım sabiti).
- **Soluk örtü** `NSBox` alt sınıfı (`DimOverlay`, `hitTest:` → `nil`),
  pane'in en üstteki çocuğu — arama paneli `view`'ın hemen üstüne girdiği
  için örtünün altında. Rengi temanın zemini (`background_srgb`),
  `DIM_ALPHA = 0.3` (Ghostty'nin `unfocused-split-opacity = 0.7`'si).
  Tazelenmesi `refresh_dim`: `focus_pane`, `pane_focused`'ın ertelenen
  turu (tıklamayla odak), kapanış ve büyütme.
- Riskli phase tetikleyicisi yok (PTY okuyucu, render thread, `.metal`,
  kilit dosyası dokunulmadı).
- Duman öncesi/sonrası aynı: `kare=29 hucre=8 glif=6 kural=15 yuva=13/1984
  yuva2=0/1984 yuk=smoke istek=4 icerik=2 hareket=27 kayma=0 kapanis=clean
  pipeline=ok`.

### Set kapısı

- `/code-review` (medium, `d9cb8ad..` + çalışma ağacı) tek bulgu (orta):
  başka pane'in arama alanına tıklamak odağı taşıyordu ama örtü, başlık ve
  sekme noktası eski pane'de kalıyordu — olay yalnız `BateriView`'ın
  `becomeFirstResponder`'ındandı (phase-3'ün "bilinen sınır"ı). Giderildi:
  `TerminalWindow` pencerenin `firstResponder`'ını KVO ile izliyor
  (`observe_focus`, sökümü `windowWillClose:`), odak değişiminin her yolu
  `pane_focused`'a varıyor; `focused_pane` aynı yardımcıdan
  (`responder_pane`). Doğrulama yeniden yeşil, duman jetonları aynı.
- `/audit`: `make denetim` temiz; mercek 3, 4, 5, 7 temiz; 1, 2, 6 ilgisiz
  (bağımlılık, ayar/tema şeması, hücre/shader değişmedi).

### Gözle kontrol sahnesi (setin tamamı; bu otonom koşuda yapılmadı — kullanıcıda)

Tek pencerede, zsh + dock'lu oturumla:
1. **Pane ayrımı (phase-1/2):** ⌘T yeni sekme etkin sekmenin dizininde; sekme
   aç/kapa; pencereyi başka ekrana taşı (metin keskin); arka sekme boşta
   kare çizmiyor; odaksız pencerede caret içi boş. ⌘F alanında ⌘G/⇧⌘G/Esc,
   ⌘E, ⌘K/⌥⌘K, ⌘Home/⌘End, Cmd +/−/0; Edit menüsünün gri öğeleri. ssh
   sekmesinde Finder damlası → onay sayfası, `Show files` popover'ı, ⌘.
   sorusu, başlıkta `↑ N%`, Dock simgesinin çubuğu.
2. **Bölme (phase-3):** ⌘D ve ⇧⌘D; her pane'de dock, doldurma bandı ve blok
   işaretleri; yeni pane aynı dizinde (ssh pane'inde aynı host); odaksız
   pane'in caret'i içi boş; soluk pane'e tıklayınca odak geçiyor; başka
   pane'in arama alanına tıklayınca örtü ve başlık o pane'e geçiyor; `exit`
   yalnız o pane; koşan işli pane'de ⌘W "Close this pane?"; son pane'de ⌘W
   sekmeyi kapatıyor; iki pane'de `echo $TERM_SESSION_ID` farklı ve
   `open bateri://tab/<id>` o pane'i odaklıyor; ayırıcının tonu ve
   Retina'da keskinliği; en küçük pane (20×5) sınırında bölme gri.
3. **Gezinme ve düzen (phase-4):** üç pane'de ⌘[ / ⌘] döngüsü, ⌥⌘ oklarla
   yön; ⌃⌘ oklarla ve ayırıcıyı sürükleyerek boyutlama (imleç ↔/↕, vim
   açıkken satır/sütun güncelleniyor, sınırda duruyor); ⌃⌘= eşitliyor;
   ⇧⌘↩ büyütüp geri alıyor, büyütülmüşken ⌘D büyütmeyi bırakıyor; odakta
   olmayan pane'ler hafif soluk (örtünün oranı `DIM_ALPHA`), tek pane'de
   örtü yok; boşta pencerede kare sayacı artmıyor.
