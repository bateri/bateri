# Phase 2 — Menü öğeleri ve sözleşme

## Özet

⌘K/⌥⌘K'yi, dört kaydırma kısayolunu ve ⌃⌘V Paste Escaped Text'i menüye
bağla, alternatif ekranda grile; `CLAUDE.md`'yi bugünkü sözleşmeye getir.

_Requirements: R2, R2.1, R2.2, R2.3, R3_

## Değişiklikler

- **`crates/bt-shell/src/menu.rs`** — Edit: Paste'in altına Paste Escaped
  Text (⌃⌘V: `v` + Control|Command), Select All'ın altına ayırıcı + Clear to
  Start (`k`) ve Clear Scrollback (`k` + Option|Command). View: punto
  öğelerinden sonra ayırıcı + Scroll to Top (⌘ + `NSHomeFunctionKey`
  U+F729), Scroll to Bottom (U+F72B), Page Up (U+F72C), Page Down (U+F72D).
  Seçiciler kendi adlarımız (`clearToStart:`, `clearScrollback:`,
  `scrollToTop:`, `scrollToBottom:`, `scrollPageUp:`, `scrollPageDown:`,
  `pasteEscaped:`). Modül başlığındaki menü envanteri ve "hedefi yok"
  paragrafı yeni öğeleri sayar. Fonksiyon tuşu kısayolunun menüde yakalandığı
  depoda emsalsiz (en yakını gizli ⌃⇥ öğeleri) — yakalanmazsa `keyDown:`'da
  yutulur ve ürün bir şey kaybetmez; gözle kontrolde doğrulanır.
- **`crates/bt-shell/src/window.rs`** — `TerminalWindow`'a altı eylem:
  temizlemenin iki kipi (phase-1 yöntemi), `scroll_page(±i32::MAX)` ve
  `scroll_page(±1)` (`bt-core`'a yeni kaydırma API'si yok — `scroll_page`
  süzülme neslini artırıyor ve `scroll_locked` bant kuralıyla kırpıyor).
  `validateMenuItem:`'a tek kol: altısı `!session.alt_screen()`; oturum
  yoksa `false`. Temizleme ve kaydırma pencere başına tek oturum
  (sekme = pencere, 026).
- **`crates/bt-shell/src/view.rs`** — `BateriView`'a `pasteEscaped:` (paste:
  emsali, arama alanı odaktayken zincirde değil): panodan düz metin; satır
  sonu yoksa `quote::shell_quote`, varsa bütünüyle tek tırnak (`'` → `'\''`);
  sonra `Session::paste`. `validateMenuItem:`'da panoda metin yoksa gri.
  Tek tırnak kuralı saf bir fonksiyonda (`quote.rs`'te, `shell_quote`'un
  yanında) ve bilinen sınır paragrafı pano kolunu anar.
- **`crates/bt-shell/src/quote.rs`** — yukarıdaki saf fonksiyon + sınaması.
- **`CLAUDE.md`** — Proje → bugünkü hâl: ana menü listesine yeni öğeler
  (Edit: Paste Escaped Text, Clear to Start/Clear Scrollback; View: dört
  kaydırma) ve tek paragraf kural: ⌘K terminal tarafında, kabuğa bayt
  gitmez, korunan ilk satırın kuralı, alternatif ekranda gri — gerekçe tek
  cümle, işaretçi `.tasks/034-ekrani-temizle/discussion.md` → Karar.
  Katman düzeni `bt-core` satırının "Dördüncü kol"u: `screen_clears`'ın
  ikinci yazarı (temizleme, kilit altında, uygulamadan sonra); "`3J` ve RIS
  için kol yok" cümlesi doğru kalır ama yanına terminal tarafı silmenin
  varlığı. "Dock ve komutlar arası atlama henüz yok" cümlesi yol haritasının
  satırına işaret eder.
- **`docs/YOL-HARITASI.md`** — 034 satırı zaten yazıldı (`/rfc`); set
  kapanırken değişmesi gereken bir şey yoksa dokunulmaz.

## Kabul

- `quote.rs` sınaması: tek satır metin `shell_quote` ile aynı; satır sonlu
  metin tek tırnakla, içindeki `'` kaçmış; `sh -c 'printf %s …'` gidiş
  dönüşü gerekmez — beklenen dizge sabit.
- `make hepsi` yeşil; `make duman` yeşil (menü kurulumu açılış yolunda).
- Gözle kontrol (sabit paket `bateri-dev`, devir mesajının cümlesi):
  - **Izgara:** `seq 1 300` → ⌘K: ekran boşalır, prompt/dock dipte kalır,
    tekerlek yukarı hiçbir şey göstermez, kabuğa hiçbir şey yazılmaz;
    ⌥⌘K'de ekran yerinde, yukarısı boş.
  - **Dock:** yarım yazılmış (ve çok satırlı) giriş ⌘K'den sonra dock'ta
    olduğu gibi durur, caret yerinde, Enter komutu çalıştırır.
  - **Doldurma bandı:** ⌘K'den sonra Tab ile tamamlama listesi açıp kapatınca
    silinen çıktı bant olarak geri gelmez.
  - `tail -f` koşarken ⌘K: çıktı yukarıdan temizlenir, `tail` sürer;
    `vim`'de Edit ▸ Clear… ve View ▸ Scroll… gri.
  - ⌘Home/⌘End/⌘PgUp/⌘PgDn geçmişte gezer; ⌃⌘V boşluklu bir yolu kaçırarak
    yapıştırır, çok satırlı metni tek tırnakla.

## Checklist

- [ ] Menü öğeleri ve başlık belgesi (`menu.rs`)
- [ ] `TerminalWindow` eylemleri + `validateMenuItem:` kolu
- [ ] `pasteEscaped:` + tek tırnak kuralı (`view.rs`, `quote.rs`)
- [ ] Test: `quote.rs` pano kuralı
- [ ] `CLAUDE.md` kural + gerekçe + işaretçi
- [ ] Doğrulama geçti (kapı komutu + `make duman`)
