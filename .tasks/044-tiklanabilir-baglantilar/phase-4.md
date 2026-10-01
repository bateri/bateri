# Phase 4 — Izgara ve bantta uçtan uca (`bt-shell-macos`)

## Özet

⌘-hover alt çizgi + el imleci ve ⌘-tıkla açma ızgarada ve doldurma bandında
çalışıyor; sözleşme `CLAUDE.md`'de.

_Requirements: R3, R5.1, R6, R7_

## Değişiklikler

- **`crates/bt-shell-macos/src/view.rs`** — `flagsChanged:` (⌘ değişince
  farenin hücresinde hover'ı yeniden değerlendir ya da temizle); `mouseMoved:`
  hücre değişince ve ⌘ `modifierFlags`'ta basılıyken `link_at`
  (`LinkPoint::Screen`; bant için imzalı satır, `cover_of`'un aritmetiği —
  bant yalnız `link_at`'in hit testi için açılır, `mouse_motion`'ın raporu
  bugünkü `Reject`'i korur); URL/OSC 8 doğrudan
  `set_link_hover`, yol adayı arka plan kuyruğuna. Basışta ⌘ + doğrulanmış
  hover'ın aralığı → `Gesture::pressed_link`, `mouse_button` çağrılmaz;
  bırakmada `Release::Link` kilitli aralığın üstündeyse ve `clickCount == 1`
  ise açma. El imleci: bağlantı dikdörtgenleri yükleme düğmeleriyle **tek**
  `resetCursorRects` / `sync_cursor_rects` listesinde.
- **`crates/bt-shell-macos/src/pane.rs`** — yol doğrulamasının arka plan
  kuyruğu ve ana kuyruğa dönüş (`RemoteProbe` emsali, pane kimlikle
  `PaneLookup`'tan); dönüşte "fare hâlâ aynı adayın üstünde mi" sorusu;
  `Wake::link_hover_lost` → ⌘ basılıysa yeniden `link_at`. Pencerenin
  key'liği gidince ve uygulama deaktive olunca hover temizlenir.
- **Açma** (`app.rs`'in `NSWorkspace` yolu yeniden kullanılır) —
  `LinkAction`: URL/dosya/dizin `openURL`, `Reveal`
  `activateFileViewerSelectingURLs`, `Confirm` pencereye sayfa (hedefin
  tamamı; "Open" / "Cancel", varsayılan ve Esc Cancel), `Swallow` hiçbir şey.
  "Bilinen içerik tipi" UTType uyumundan (metin/kaynak kodu, görsel, PDF,
  ses/görüntü).
- **Makine adı** — pane doğarken `links`'in fonksiyonundan
  `SessionOptions::hostname`'e; OSC 7'nin adlı yerel yetkisi de böylece yerel.
- **`CLAUDE.md`** — bağlantı paragrafı (algılama, ⌘ arbitrajı, beyaz liste,
  `bateri://` yutma — 038 Karar 7'nin `CLAUDE.md`'ye istediği kural cümlesi,
  OSC 7'nin adlı yerel yetkisi).

## Kabul

- `make smoke` yeşil (boşta kare sınırı değişmedi).
- Gözle kontrol (devir mesajına): `echo https://example.com` → ⌘ basılıyken
  üstüne gel, alt çizgi + el, ⌘ bırak, kalkar; ⌘-tık tarayıcıda açar.
  `ls` → çıplak dosya adı ⌘-tıkla varsayılan uygulamada; bir betik Finder'da
  gösterilir. vim'de `:set mouse=a` içinde ⌘-tık URL'yi açar, vim'e tık
  gitmez. Bant: tamamlama listesiyle geçmişe itilen satırdaki URL.

## Checklist

- [x] `flagsChanged:`, ⌘'li hover, temizleme kancaları
- [x] Arka plan doğrulama + `link_hover_lost` ile yeniden bulma
- [x] Ön-rota bağlama, bırakmada açma
- [x] Tek cursor-rect listesi
- [x] Açma, onay sayfası, UTType sınıfı, makine adı
- [x] `links::Content`'in cevabı paket dizinini de ayırıyor (`Package` →
  `Reveal`; `.app`'i `openURL` ile açmak onu çalıştırır — phase-3 Uygulama
  Notları)
- [x] `CLAUDE.md` güncellendi
- [x] Doğrulama geçti (`make check` + `make smoke`)

## Uygulama Notları

- **Yer.** AppKit yarısı yeni bir modülde, `bt-shell-macos::hyperlink`
  (`impl BateriView`, `uploader`'ın emsali); `view.rs` yalnız kancaları
  taşıyor (`flagsChanged:`, `mouseMoved:`'a bir satır, basışta ön-rota,
  `Release::Link`, tek cursor-rect listesi `hand_rects`). Durum tek ivar
  (`LinkState`: son sorulan hücre, gösterilen doğrulanmış hover, uçuştaki
  aday, bulunamayan son aday, basışta kilitlenen hover, seri kuyruk).
- **Hücre.** `link_cell_at` saf: `cover_of`'un aritmetiği, tavan yerine taban
  (`-1` bandın alt satırı); sol pay, son sütunun ötesi ve ızgaranın altı
  (dock) `None`, bandın ekranda olup olmadığını `bt-core` `drawn_lines` ile
  soruyor. El imlecinin dikdörtgenleri bunun tersi (`span_rects_px`), bekçisi
  köşelerin aynı hücreye düştüğünü sınıyor.
- **Akan çıktıda yeniden doğrulama yok.** `link_hover_lost` → ⌘ hâlâ basılı ve
  pencere key ise aynı nokta yeniden soruluyor; bağlantı aynıysa (aralık,
  hedef, tür — damga hariç) önceki doğrulama yeniden kullanılıyor, yani akan
  çıktıda tur başına `stat` yok. phase-2'nin adıyla yazdığı bedel duruyor:
  hover kuruluyken akan çıktı başına iki kare, çıktıyla sınırlı; ⌘ bırakılmışsa
  ya da pencere key değilse döngü `clear_link`'le kesiliyor. Bulunamayan yol
  adayı da hatırlanıyor: içinde gezinmek `stat`'ı tekrarlamıyor.
- **Temizleme kancaları.** `flagsChanged:` (⌘ bırakıldı), her hareket olayında
  ⌘'nin `modifierFlags`'tan yeniden okunması ve `windowDidResignKey:`. Ayrı bir
  `applicationDidResignActive:` kancası **eklenmedi**: uygulama deaktive
  olunca key pencere de key'liği bırakıyor, yani tek kanca ikisini kapsıyor.
- **İçerik sınıfı (UTType) çalışma zamanından.** `objc2-uniform-type-identifiers`
  grafta yok ve yeni crate olurdu → `AnyClass::get(c"UTType")` + `msg_send!`
  (`updater`'ın emsali); `Cargo.lock` değişmedi. Belge kümesi
  `public.plain-text`, `public.source-code`, `public.json`, `public.image`,
  `com.adobe.pdf`, `public.audiovisual-content`; **önce** `public.script` ve
  `public.executable` soruluyor, çünkü `public.shell-script` kaynak kodu sayılıyor
  ve `.command`/`.py`'yi "açmak" onu koşturur (bekçi
  `documents_open_and_scripts_and_executables_never_do`). Sınıf uzantıdan;
  uzantısız dosya (`Makefile`) `Other` → Finder'da gösteriliyor (güvenli yön).
- **Paket dizini** (phase-3 devri): `content_of` önce
  `NSWorkspace::isFilePackageAtPath` soruyor → `Content::Package` → `Reveal`.
  Tık anında ana thread'de (diski okuyor ama tık başına bir kez, yolu arka
  plan `stat`'ı zaten bulmuş).
- **Onay sayfası.** "Open this link?" + hedefin tamamı; "Cancel" ilk düğme
  (Return), "Open" ikinci; Esc `uploader`'ın yerel olay izleyicisiyle
  (`add_key_monitor`/`remove_monitor`/`ESCAPE` `pub(crate)` oldu). Pencerede
  zaten bir sayfa varsa istek düşüyor.
- **Makine adı** pane doğarken `links::hostname()` → `SessionOptions::hostname`;
  süreli koşu dahil (jetonları oynatmıyor).
- **Adıyla bilinen sınırlar.** (1) `mouseMoved:`/`flagsChanged:` first
  responder'a gidiyor (`window.rs`'in notu), yani bölmelerde ⌘-hover odaktaki
  pane'de. (2) Doğrulama dönmeden gelen ⌘-basış bugünkü yolundan (Muhakeme).
  (3) Yolu UTF-8 olmayan dosyanın `NSURL`'ü `to_string_lossy`'den — açılmaz ya
  da yanlış yolu gösterir; tarayıcı zaten UTF-8 metinden aday çıkarıyor.
  (4) Bulunamayan aday ⌘ bırakılana kadar hatırlanıyor: ⌘ basılıyken
  yaratılan dosya ⌘ bırakılıp yeniden basılınca bulunur. (5) Pencere ⌘ zaten
  basılıyken key olursa vurgu ilk `mouseMoved:`'u bekliyor. (6) UTType
  bekçisi makinenin Launch Services veritabanını okuyor.
- **Gözle kontrol devirde.** Kabul'ün maddeleri (URL, `ls` dosyası, betik,
  vim `mouse=a`, bant) gerçek pencerede ⌘ + fare ister; set kapısının
  gözle kontrolüne kaldı. `make smoke` yeşil, boşta kare sınırı değişmedi
  (`content=2`).
- `make test-race` gerekmedi: `bt-core`'a dokunulmadı; arka plan kuyruğu
  paylaşılan durum yazmıyor (cevabı ana kuyruğa değerle taşıyor).
