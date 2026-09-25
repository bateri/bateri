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

- [x] Menü öğeleri ve başlık belgesi (`menu.rs`)
- [x] `TerminalWindow` eylemleri + `validateMenuItem:` kolu
- [x] `pasteEscaped:` + tek tırnak kuralı (`view.rs`, `quote.rs`)
- [x] Test: `quote.rs` pano kuralı
- [x] `CLAUDE.md` kural + gerekçe + işaretçi
- [x] Doğrulama geçti (kapı komutu + `make duman`)

## Uygulama Notları

- **Etkinliğin kolu bir yüklem fonksiyonu** (`window::is_scrollback_action`),
  sabit dizi değil: `sel!` `const` bağlamda değerlendirilemiyor.
- **Tek tırnak kuralı `quote::paste_quote`**; satır sonu ölçütü `\n` **ya
  da** `\r` (Windows'tan gelen pano `\r\n` taşıyabilir, ikisi de kabukta
  komut sınırı). Test önce yazıldı; çok satırlı kol `shell_quote`'a
  düşürülünce `paste_quote_wraps_multiline_text_in_single_quotes` kırmızı.
- **Gözle kontrol** (`bateri-dev` / `dev.bateri.agent-check`, geçici HOME,
  2026-09-25): `seq 1 300` → ⌘PgUp bir sayfa, ⌘Home geçmişin başı (`seq`'in
  komut satırı), ⌘End dip — fonksiyon tuşu kısayolları menüde
  yakalanıyor; yarım yazılmış `echo yarim` ile ⌘K → ekran boş, satır dock'ta,
  caret yerinde, tekerlek ve ⌘Home hiçbir şey göstermiyor, Enter komutu
  çalıştırdı; ardından Tab tamamlaması silinen çıktıyı bant olarak geri
  getirmedi; `vim`'de Edit ▸ Clear to Start/Clear Scrollback gri; koşan
  `ping`'de ⌥⌘K geçmişi sildi (tekerlek ekranın üstüne çıkmıyor), ⌘K ekranı
  temizledi ve `ping` sürdü. **Görülmeyen:** View ▸'nin alternatif ekrandaki
  grisi (menü açılmadı; kol Edit'inkiyle aynı yüklem) ve ⌃⌘V'nin kendisi —
  panoya yazmak kullanıcının panosunu ezerdi; kural birim sınamasında.
- **Set kapısı `/code-review`** — iki bulgu:
  - *Giderildi:* `paste_quote` `\r\n`'i olduğu gibi geçiriyordu; zsh'in
    bracketed okuyucusu her `\r`'yi `\n` yaptığı için Windows panosu satır
    başına iki satır sonu olurdu. Tırnağın içinde `\r\n` ve tek `\r` `\n`'e
    iniyor, bekçisi aynı sınamada.
  - *WAIVE (orkestratörün kabulüne):* `protected_top` "kimlik prompt başına
    tek" varsayıyor; zsh prompt'u `precmd` koşmadan yeniden bastığında
    (ekranı aşan tamamlama listesi, `setopt notify`'ın arka plan iş
    bildirimi) eski prompt satırı da aynı kimliği taşıyor ve ⌘K eski
    prompt + liste/bildirim + yeni prompt'u tutuyor. Ayırt edecek veri
    ızgarada yok: bağlantı `preexec`'e kadar açık, yani liste ve bildirim
    satırları da aynı kimliği taşıyor (mürekkep ya da bitişiklik ölçütü
    onları ayıramaz). Çare prompt çizimi başına bir işaret — betik
    değişikliği, bu setin kapsamı dışında (`assets/shell` değişmiyor).
    Yanlışın yönü güvenli: fazladan satır kalıyor, güncel blok asla
    silinmiyor. Tetik dar (LISTMAX=0 + ALWAYS_LAST_PROMPT'ta sığan liste
    prompt'u yeniden basmıyor).
