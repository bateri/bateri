# TerminalPane ayrımı ve bölmeler

## Hedef

Sekmenin içeriğini `NSWindow`'dan bağımsız bir `TerminalPane`'e (tek
`NSView`, olay geri bildirimi, eylem API'si) ayırmak — tek pane'li pencere
bugünküyle bit bit aynı — ve onun üstüne ⌘D / ⇧⌘D bölmeleri, klavyeyle
gezinme, boyutlama, eşitleme ve büyütmeyi kurmak.

## Gereksinimler

- **R1** — Pane ayrımı, davranış değişmez
  - **R1.1** — `TerminalPane` (`NSView` alt sınıfı, bugünkü içerik
    kapsayıcısı) oturumun, link'in, renderer'ın, yüzeyin, `BateriView`'ın,
    `ShellWake`'in, `shell_parent`'ın, dock payının, punto farkının ve
    kimliğin sahibi; pencere tam bir pane tutuyor.
  - **R1.2** — Geometri (çerçeve bildirimi, ekran ölçeği), örtülme ve odak
    pane'e varıyor; çerçeve gözlemcisi pane kapanınca sökülüyor.
  - **R1.3** — Okuyucu thread'den ana kuyruğa dönüşler pane'i kimlikle
    buluyor (`app.pane(id)`).
  - **R1.4** — Süreli koşu pencerenin tek pane'ini okuyor; `make duman`
    jeton satırının değerleri öncekiyle aynı.
- **R2** — Sınır
  - **R2.1** — Arama paneli, yükleme kuyruğu/sayfaları/popover'ı ve pane
    düzeyindeki menü seçicileri pane'de; `BateriView` sahibini `superview()`
    ile buluyor (view.rs'in iki arama yolu kalkıyor).
  - **R2.2** — Pane girdilerini doğumda bir pakette (`PaneLaunch`) alıyor,
    olayları `PaneHost` trait'inden veriyor (OSC 52 kopyası dahil); pane
    modülünde `app::delegate` ve `app.settings()` yok.
  - **R2.3** — Menünün karşıladığı her pane işi pane'de adlı bir yöntem,
    seçici onu çağıran sarmalayıcı.
- **R3** — Bölme
  - **R3.1** — ⌘D sağa, ⇧⌘D aşağı böler; yeni pane dizini, punto farkını,
    temayı ve uzak satırı odaktaki pane'den devralır (Karar 9). Düzen saf,
    sınamalı bir ağaç modülünden.
  - **R3.2** — ⌘W ve kabuğun çıkışı yalnız o pane'i kapatır, odak komşuya
    geçer; son pane sekmeyi kapatır. ⇧⌘W ve kırmızı düğme bütün pane'leri.
  - **R3.3** — Başlık, `⇄`, yükleme yüzdesi ve host noktası odaktaki
    pane'den; kapatma soruları koşan işi pane'lerden toplar (tek pane'de
    metin bayt bayt aynı); Dock simgesi toplamı; örtülme ve ölçek bütün
    pane'lere.
  - **R3.4** — Her pane kendi `TERM_SESSION_ID`'sini taşır; `bateri://tab/<id>`
    pane'i bulup öne getirir ve odaklar.
  - **R3.5** — Odakta olmayan pane'in caret'i içi boş; ayırıcı bir piksel,
    temanın `separator` tonunda.
- **R4** — Gezinme ve düzen
  - **R4.1** — ⌘[ / ⌘] sırayla, ⌥⌘←↑→↓ yöne göre odak değiştirir.
  - **R4.2** — ⌃⌘←↑→↓ ve ayırıcı sürüklemesi boyutlar; en küçük pane
    sınırında durur, sınırın altına düşecek bölme yapılmaz (Karar 14).
  - **R4.3** — ⌃⌘= eşitler; ⇧⌘↩ odaktaki pane'i büyütür/geri alır; bölme,
    gezinme ve kapanış büyütmeyi bırakır.
  - **R4.4** — Odakta olmayan pane'ler AppKit örtüsüyle soluklaşır (kare
    yolu dışında); tek pane'de örtü yok.
- **R5** — Sözleşme belgeleri kodla aynı commit'te güncel (`CLAUDE.md`,
  `bt-shell/src/lib.rs` başlığı, `docs/YOL-HARITASI.md`).

## Yaklaşım

1. Pane nesnesini kur ve oturumun çekirdeğini pencereden ona taşı; pencere
   tek pane'e yönlendirir, `AppDelegate`'in arama ve dağıtım yolları pane'e
   varır (phase-1).
2. Arama, yükleme ve pane düzeyindeki eylemleri pane'e taşı; sahip arayüzünü
   (`PaneLaunch`, `PaneHost`) kapat, pane'in `AppDelegate`'e uzanan son
   yollarını kes (phase-2).
3. Saf bölme ağacını ve kapsayıcıyı ilk tüketicisiyle (⌘D/⇧⌘D) indir;
   kapanış, odak, kimlik ve sekme düzeyi toplamaları (phase-3).
4. Klavye gezinmesi, boyutlama, sürükleme, eşitleme, büyütme ve soluk örtü
   (phase-4).

Gerekçeler `discussion.md` → Karar 1–14.

## Kapsam Dışı

- Pane'i başka bir uygulamaya gömmek: dış API, C ABI, ayrı crate yayını
  (Karar 4 sınırı hazırlıyor, yayını değil).
- Zil (`Event::Bell`) — `bt-core`'un `Wake`'ine yeni kol ister.
- Bölme düzeninin kaydı ve pencere geri yükleme
  (`docs/METALTERM-KARSILASTIRMA.md` madde 9).
- Pane'i sekmeler/pencereler arasında sürükleyip taşımak, pane'i sekmeye
  çevirmek.
- Paylaşımlı renderer (Karar 5; gerekirse sonradan açılır).
- Soluk örtünün ve bölme kısayollarının ayar anahtarı.

## Akış

```
NSWindow ── delegate ── TerminalWindow   (krom, başlık, sekme, kapatma, odak/örtülme dağıtımı)
   └── contentView = SplitContainer       (phase-3; phase-1/2'de doğrudan tek pane)
          ├── TerminalPane ── Session · DisplayLink · Renderer · Surface · ShellWake
          │      ├── BateriView  (first responder → responder zinciri pane'den geçer)
          │      ├── SearchBar   (yüzen, Metal katmanının kardeşi)
          │      └── DimOverlay  (phase-4, hitTest nil)
          ├── divider
          └── TerminalPane …

okuyucu thread ─ ShellWake(pane id) ─▶ ana kuyruk ─▶ app.pane(id) ─▶ PaneHost olayı ─▶ TerminalWindow
menü (hedefsiz) ─▶ BateriView ─▶ TerminalPane (pane işleri) ─▶ NSWindow ─▶ TerminalWindow (sekme işleri) ─▶ AppDelegate
AppDelegate.reload_settings ─▶ her pencere ─▶ her pane.set_*()
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | |
| phase-2 | |
| phase-3 | |
| phase-4 | |
| kapı | |
