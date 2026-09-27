# TerminalPane ayrımı ve bölmeler — Bağlam

## Mevcut Durum

Sekme = `NSWindow` (026 → Karar 1, C). O pencerenin **her şeyi** tek bir
nesnede: `TerminalWindow` (`crates/bt-shell/src/window.rs`, ~2900 satır)
hem `NSWindowDelegate` (krom, başlık, sekme, kapatma sorusu, odak, örtülme)
hem de oturumun sahibi. `WindowIvars` (window.rs:617) pencere başına birer
tane tutuyor: `renderer`, `surface`, `view` (`BateriView`), `link`,
`session`, `shell_parent`, `wake` (`ShellWake`), `zoom`, `dock_rows` /
`dock_rows_at_birth`, arama paneli (`search`, `search_status`,
`search_driving`), yükleme kuyruğu ve sayfaları (`uploads`, `upload_alert`,
`upload_stop`, `upload_list`, `list_closed_at`) ve `tab_id`.
`uploader.rs` bütünüyle bir `impl TerminalWindow` (uploader.rs:81).

Bu nesneye bağlanan yollar (keşif, 2026-09-27):

- **`BateriView` pencereye iki farklı yoldan uzanıyor**: `terminal_window()`
  (view.rs:1803; `window()` → `AppDelegate::window_owning`, doğrusal arama)
  sürükle-bırak, yükleme düğmelerinin rect/hover/tık yolları için;
  `keyboard_moved()` (view.rs:1491) ise pencerenin delegate'ini downcast
  ederek odağın ikinci bitini veriyor. Oturum view'ın kendi ivar'ında
  (`attach`), link'e hiç dokunmuyor; kalan `window()` kullanımları
  (`backingScaleFactor`, `convertRectToScreen`, cursor rect'ler) pencereye
  özgü değil, her view için doğru.
- **Menü hedefsiz** (menu.rs:420): `copy:`/`cut:`/`paste:`/`pasteEscaped:`/
  `selectAll:` view'da; punto, bul, temizle, kaydır, `cancelUpload:`,
  `closeTab:`/`closeWindow:`, `selectTab:` `TerminalWindow`'da (window.rs:906–1135;
  arama alanı odaktayken zincir `BateriView`'dan geçmediği için — 033 Karar 10);
  pencere açma, tema, `markHost:` `AppDelegate`'te.
- **Okuyucu thread'den ana kuyruğa dönüşler** pencereyi **kimlikle** buluyor
  (`app.window(id)`; `ShellWake`'in `child_exit`/`title_changed`/
  `search_changed`, alternatif ekran habercisi, `uploader::on_window`) —
  referans çemberi açmayan, `Send` bir tamsayı.
- **`AppDelegate`** (app.rs) pencere listesini (`windows`, app.rs:691) tutuyor,
  ayar kaydını her pencereye dağıtıyor (`reload_settings` app.rs:1929:
  `zoom_after_reload`, `set_terminal_options`, `set_host_marks`,
  `set_cursor_motion`, `apply_caret`, `apply_reduce_motion`, `apply_font`,
  `set_theme`), yeni sekmeyi etkin pencerenin oturumundan doğuruyor
  (`open_window` app.rs:1732: dizin, tema, punto, uzak satır → `Launch`) ve
  ⌘Q sorusunu bütün pencerelerden topluyor.
- **Süreli koşu tek pencere varsayıyor** (026 Karar 9): `quiet_since`,
  `shutdown` ve `report_and_exit` `windows().first()`'ün `renderer()` ve
  `link()`'ini okuyor (app.rs:1790, 2426, 2469).
- **Pane'in içine sızan uygulama bağımlılıkları**: `start_session`
  `app.settings()`, `app.stats()`, `app.shell_integration()`,
  `app.reduce_motion()`, `app.smooth_scroll()` ve `app.post_notices`'e
  uzanıyor. Sarmalayıcının yeri ve ortamı zaten dışarıdan geliyor
  (`app::shell_integration_env`, `child`).

## Motivasyon

İki iş, tek set (kullanıcı kararı, 2026-09-27; `discussion.md` → Karar):

1. **Pane ayrımı.** Sekmenin içeriği `NSWindow`'dan bağımsız bir
   `TerminalPane`'e iniyor: dışarıya tek bir `NSView` olarak veriliyor,
   olayları (başlık, dizin, kabuk çıktı, yükleme, bildirim, pano) sahibine
   geri bildiriyor ve menünün karşıladığı işler üstünde bir **eylem API'si**
   olarak duruyor. Davranış bit bit aynı kalıyor. Uzun vadeli hedef — pane'i
   native bir workspace uygulamasına gömmek — bu setin kapsamı değil, ama
   sınır ona göre çiziliyor: ayar ve tema dışarıdan veriliyor, dosya okuma ve
   izleme sahibin işi kalıyor.
2. **Bölmeler.** Metalterm 0.1.6'nın ⌘D / ⇧⌘D ile iki eksende bölmesi,
   klavyeyle gezinme, boyutlama, eşitleme ve büyütme
   (`docs/METALTERM-KARSILASTIRMA.md` → madde 1; referans envanteri
   `docs/ARASTIRMA.md` → Ürün özellikleri). Yol haritasında "bölme" satırı
   olarak bekliyordu.

**Kanıt — yol haritasının yazılı bedeli ölçüldü.** `docs/YOL-HARITASI.md`'nin
"bölme" satırı bedeli "komut blokları, Input Dock ve doldurma bandı 'bir yüzey
= bir oturum' varsayımıyla indi" diye yazıyor. Keşif bunun `bt-core`/`bt-gpu`
kısıtı **olmadığını** gösterdi; varsayım yalnız `bt-shell`'in sahipliğinde
yaşıyor:

- `bt-gpu`'nun tek `static`'i salt okunur metallib baytları
  (renderer.rs:47); `Renderer::system_default` her çağrıda kendi device
  tutamağını, pipeline'larını, kuyruğunu ve atlasını kuruyor (renderer.rs:379)
  ve bugün zaten pencere başına çağrılıyor. `Surface` yalnız kendi
  `CAMetalLayer`'ı; ölçek argüman.
- `DisplayLink` katman başına bir `CAMetalDisplayLink` ve bütün durumu —
  bloklar, seçim, arama, dock, doldurma, öteleme, hareket, odak, blink —
  link'in kendi ivar'larında (link.rs:1841); `set_focused`,
  `set_keyboard_in_terminal`, `set_visible` link başına birer `Cell`.
  Aynı ekranda N link bugün N pencereyle zaten koşuyor.
- `bt-core`'da tek `OnceLock` oturumun kendi göndericisi (session.rs:1112);
  kimlik (`TERM_SESSION_ID`) `SessionOptions::tab_id`'den oturum başına.

Yani dock, doldurma bandı ve bloklar her pane'in **kendi** oturumunda ve
link'inde el değmeden çalışır; bölmenin bedeli `TerminalWindow`'un pencere
başına tuttuğunu pane başına tutmak ve pencereye bir "odaktaki pane"
yönlendiricisi eklemek.

**Zil** bugün hiç işlenmiyor (`Event::Bell` session.rs:1354'te yutuluyor):
pane'in olay listesine girmesi `bt-core`'un `Wake`'ine yeni bir kol demek ve
bu setin konusu değil.
