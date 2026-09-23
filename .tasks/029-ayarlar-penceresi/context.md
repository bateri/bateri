# Ayarlar penceresi — Bağlam

## Mevcut Durum

- **bateri ▸ Settings… (Cmd-,) bir pencere açmıyor, dosyayı açıyor**:
  `AppDelegate::edit_settings` dosya yoksa şablonla yaratıyor
  (`settings::create_if_missing`), yeniden okuyor ve `.toml`'u açan
  uygulamada (yoksa TextEdit) gösteriyor (`open_in_editor`). Davranışın
  tarifi `docs/AYARLAR.md` → Settings….
- **Dosyaya yazan tek var-olan-dosya yolu View ▸ Theme ▸**:
  `bt-core`'da saf `Settings::with_theme` (yalnız `[appearance] theme`'i
  değiştirir; yorum, sıra, bilinmeyen anahtar, satır sonu ve süs korunur;
  ayrıştırılamayan metin ve bölüm-olmayan `appearance`/bölüm-olan `theme`
  `Err`), `bt-shell`'de `settings::write_theme` (o anda okur, yoksa yaratır,
  **yerinde** yazar — sembolik bağın hedefine; waive kaydı
  `.tasks/007-ayarlar-ve-tema/phase-7.md`). Menü yalnız yazar, uygulayan
  izleme yolu (`AppDelegate::reload_settings`) — `save_theme` yazdıktan
  hemen sonra okumayı kendisi tetikliyor.
- **Uygulama yolu hazır ve tek**: kayıt → vnode olayı → `reload_settings` →
  `Settings::changes` → pencerelere dağıtım. Kabul edilmeyen değer kendi
  anahtarını değiştirmez (`Settings::parse_keeping`), ayrıştırılamayan dosya
  hiçbir şeyi (`Loaded::live` → `None`). Tanılar alt başlıkta, kaynak başına
  yuvada (`notices`); modal yok (`.tasks/007-ayarlar-ve-tema/discussion.md` →
  Karar 8).
- **Geçerli değerlerin bilgisi `bt-core`'da ama dışarı kapalı.** Dizge
  enum'larının yazılışları çoğunda özel `name()` + ayrıştırıcının elle
  yazılmış `match` kolları; yalnız `UnfocusedCaret` ve `ConfirmClose` tek
  tablodan (`NAMES`) okuyor. Aralıklar özel sabit (`CURSOR_BLINK_RANGE`,
  `CURSOR_RADIUS_RANGE`, `CURSOR_GLOW_RANGE`), `SCROLLBACK_MAX` `pub(crate)`,
  `MAX_LINE_HEIGHT` `pub`. `Osc52` `session.rs`'te. Bu, yol haritasının
  "Ayar ayrıştırmasının beş kopyası" borcunun ta kendisi ve çaresinin yerini
  o borç "bir sonraki ayar seti" diye yazıyor (`docs/YOL-HARITASI.md` →
  Sete bağlanmamış borçlar).
- **Eşaralıklılığın ölçütü tek yerde**: `bt-atlas::font::open_chain`'in
  `TraitMonoSpace` biti; sonucu `FontNotice` olarak `bt-gpu` üzerinden
  `bt-shell`'e çıkıyor. Makinedeki aileleri listeleyen bir yol **yok**.
- **Geçici punto aralığı** `bt-shell/src/zoom.rs`'te (`MIN_SIZE`/`MAX_SIZE`,
  4–72); dosyadaki `size`'ın kendi sınırı yalnız "0'dan büyük"
  (`docs/AYARLAR.md` → `[font]`).
- **AppKit yüzeyi**: `objc2-app-kit` 0.3.2, `bt-shell` bayrakları
  `Cargo.toml`'da gerekçeleriyle; pencere listesi (`AppDelegate::windows`)
  yalnız `TerminalWindow`'ları tutuyor ve `key_window` terminal olmayan key
  pencereyi (panel, ayar penceresi) zaten `None` sayıyor.
- **Süreli koşu** (`Inputs::Hermetic`) ayar dosyasını okumuyor, yazmıyor;
  `edit_settings` o dalda erken dönüyor.

## Motivasyon

Kullanıcı isteği (2026-09-23): *"ayarlar kısmı ile ilgili bir sayfa yapmamız
lazım. güzel olmalı ve anlaşılır. abartmadan temiz bir şeyler"*. Bugün her
ayar değişikliği bir TOML dosyasını elle düzenlemek demek: geçerli değerleri
bilmek için belgeyi ya da şablonun yorumunu okumak gerekiyor ve yanlış yazılan
değer yalnız alt başlıkta bir satırla görünüyor.

Düzen kullanıcıyla kararlaştırıldı (ürün kararı, bu setin girdisi):

- Yerel macOS ayar penceresi; **solda kenar çubuğu** (System Settings tarzı,
  SF Symbols ikonlu) dört kategori — General / Appearance / Cursor / Motion;
  sağda seçilen kategorinin satırları: solda sağa yaslı etiket, sağda kontrol,
  gerektiğinde altında küçük sönük açıklama.
- General: Confirm before closing, Clipboard access (OSC 52), Scrollback
  lines, Shell integration ("yeni sekmelerde geçerli" notuyla). Appearance:
  Theme, Light theme, Dark theme, Font, Size, Line height. Cursor: Shape,
  Blink, Blink speed, Corner radius, Glow, When unfocused. Motion: Cursor
  motion, Smooth scrolling, Reduce motion. Bir "Open settings.toml" düğmesi
  (bugünkü Cmd-, davranışı).
- Cmd-, bu pencereyi açar, tek örnek.
- **Tek kaynak `settings.toml`**: pencere yalnız dosyaya yazar, uygulayan
  mevcut izleme yolu; dosya dışarıdan değişince kontroller tazelenir.

Referans üründe ayar penceresi var ve etiketleri anahtarlarla birebir
(`docs/ARASTIRMA.md` → İmleç, "Ayar penceresinin etiketleri anahtarlarla
birebir"; Motion sekmesinin dökümü → Hareket). Biz sekme değil kenar çubuğu
kullanıyoruz (kullanıcı kararı).
