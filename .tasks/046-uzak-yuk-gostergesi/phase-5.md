# Phase 5 — Ayrıntı popover'ı, tık ve el imleci

## Özet

Göstergeye tık canlı tazelenen bir `NSPopover` açar; göstergenin üstünde el
imleci; Esc ve dışarı tık kapatır.

_Requirements: R6.1, R6.2_

## Değişiklikler

- **`crates/bt-shell-macos/src/stats_popover.rs`** (yeni) — `uploader`'ın
  liste popover'ının emsali (`discussion.md` → Karar 7): `transient`, çıpa
  `bt_core::stats_span` → `BateriView::context_span_rect`, içerik bir
  `NSViewController`'ın görünümünde etiketler ve çubuklar (UI dizgileri
  İngilizce: `{host} · {OS}`, `CPU · N cores`, `Load 1/5/15`, `Memory`, `Swap`,
  `Disk /`, `Uptime`, süreç satırları); çubuk rengi `bt_core`'un eşik
  sınıfından temanın `info`/`warning`/`error`'ı. Esc yerel olay izleyicisiyle
  (pane'in penceresi, kabuğa gitmez); açıkken sürücünün `detail` bayrağı açık
  ve her `Detail` içeriği **yerinde** tazeler (yapı değişmedikçe görünüm
  yeniden kurulmaz — `uploader`'ın `live` emsali). OS ve süreçler henüz
  gelmediyse satırları boş değil "—".
- **`crates/bt-shell-macos/src/pane.rs`** — popover ivar'ı;
  `popoverWillClose:`/`popoverDidClose:` bildirimin nesnesini iki popover'la
  karşılaştırır, kapanış zamanı iki ayrı yuvada (bugünkü `list_closed_at`
  ikiye ayrılır: aynı göstergeye ikinci basış popover'ı yeniden açmasın).
  Gösterge kaybolunca (`set_remote_stats(None)`, aktarım satırı geldi, uzak
  oturum bitti, `off`) popover kapanır.
- **`crates/bt-shell-macos/src/stats.rs`** — popover açık/kapalı `Schedule`'a
  olay; `Detail` dinleyicisi popover'ı tazeler; popover açılırken bir sonraki
  tik beklenmeden `detail`'li bir istek (uçuşta istek varsa onun ardından).
- **`crates/bt-shell-macos/src/view.rs`** — bağlam satırına tık önce
  `upload_click`, tutmazsa `stats_click` (aktarım varken gösterge çizilmediği
  için çakışma yok); `hand_rects` göstergenin dikdörtgenini ekler
  (`context_span_rect`), gösterge doğunca/kaybolunca ya da genişliği
  değişince `sync_cursor_rects` (aktarımın `show_transfer` emsali).

## Kabul

- `make check` ve `make smoke` yeşil.
- Elle (gerçek Linux sunucu): göstergeye tık popover'ı açar, ikinci tık
  kapatır, dışarı tık ve Esc kapatır ve Esc uzak kabuğa `^[` yazmaz; açıkken
  CPU ve süreçler her örnekte değişiyor; göstergenin üstünde el imleci, yolun
  üstünde ok; bir dosya bırakıp yükleme başlatınca gösterge kaybolur ve
  popover kapanır, yükleme bitince gösterge geri gelir; "Show transfers"
  popover'ı bugünkü gibi çalışıyor.
- Set sonu gözle kontrolün üç yüzeyi (`proje.md` → Set kapısı ekleri):
  **dock** — ssh'ta bağlam satırının sağında sparkline döşüyor, küçük metinle
  aynı taban çizgisinde, yükte sayı sarı/kırmızı ve `▲`, daraltınca merdiven;
  **ızgara** ve **doldurma bandı** — değişiklik yok, çünkü bağlam satırı
  yalnız dock'un ve küçük sınıf yalnız orada çiziliyor (büyük sınıfın
  yordamsal raster'ı bit bit aynı, phase-1).

## Checklist

- [ ] `stats_popover.rs`: içerik, canlı tazeleme, Esc
- [ ] Delegate'in iki popover'ı ayırması, iki kapanış yuvası
- [ ] Sürücüyle `detail` bağlantısı
- [ ] Tık ve el imleci
- [ ] Gösterge kaybolunca kapanış
- [ ] Doğrulama geçti (`make check` + `make smoke`)
