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

- [ ] `flagsChanged:`, ⌘'li hover, temizleme kancaları
- [ ] Arka plan doğrulama + `link_hover_lost` ile yeniden bulma
- [ ] Ön-rota bağlama, bırakmada açma
- [ ] Tek cursor-rect listesi
- [ ] Açma, onay sayfası, UTType sınıfı, makine adı
- [ ] `links::Content`'in cevabı paket dizinini de ayırıyor (`Package` →
  `Reveal`; `.app`'i `openURL` ile açmak onu çalıştırır — phase-3 Uygulama
  Notları)
- [ ] `CLAUDE.md` güncellendi
- [ ] Doğrulama geçti (`make check` + `make smoke`)
