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

- [x] `stats_popover.rs`: içerik, canlı tazeleme, Esc
- [x] Delegate'in iki popover'ı ayırması, iki kapanış yuvası
- [x] Sürücüyle `detail` bağlantısı
- [x] Tık ve el imleci
- [x] Gösterge kaybolunca kapanış
- [~] phase-4'ten devralınan elle kabul (ajan kabuğunda gerçek Linux sunucu ve göz yok — set kapısının gözle kontrolüne kaldı): gerçek Linux sunucuda gösterge ~1 s'de CPU'suz, sonra CPU'lu; sekme değişince örnekleme durur, dönünce hemen örnek; `stats = "off"` göstergeyi kaldırır; `exit` sonrası 120 s'de yardımcı ssh kapanır (`ps`); ayar penceresinin 780 pt yüksekliği Remote Files'a sığıyor mu (tahmin, ölçülmedi)
- [x] Doğrulama geçti (`make check` + `make smoke`; `bt-core` değiştiği için `make linux` da)

## Uygulama Notları

- **Tık, çıpa ve el imleci için `Session::stats_span(budget)`** (`bt-core`,
  yalnız yaprak kilit): `DockContext`'i dışarı veren bir yol yoktu; tık
  "sütun aralıkta mı" diye bu tek aralığa bakıyor (`dock::stats_at` ile aynı
  yerleşim). `bt-core` değiştiği için `make linux` tetiklendi.
- **Popover'ın satırları sabit**, yani `uploader`'ın `shape`/yeniden kurma
  kolu yok: görünümler bir kez kuruluyor, her örnek yalnız metni, çubuğun
  genişliğini ve rengini yazıyor. Çubuklar `NSProgressIndicator` değil
  `NSBox` (iz + dolgu): temanın rengini ancak öyle alıyor. Swap'ın eşiği yok,
  çubuğu hep `info`. Bilinmeyen değer "—"; süreç yoksa tek "—".
- **Son `Detail` sürücüde** (`StatsDriver::detail`, `last` ile aynı yerlerde
  siliniyor): popover açılır açılmaz son örnekle doluyor, ayrıntılı cevap
  gelince OS/çekirdek/süreçler beliriyor. Düz örnek önceki OS'u ve çekirdek
  sayısını koruyor (nesil içinde değişmezler); süreçler korunmuyor.
- **Tek kanca `stats_gauge_changed`**: gösterge çizilmiyorsa popover kapanır,
  çiziliyorsa `setPositioningRect` ile aralığı izler; el imlecinin
  dikdörtgenleri tazelenir. Çağıranlar: her örnek (değişmese de — pencere
  boyu değişince dikdörtgen kayar ve `upload_hover` yüklemesiz pencerede hiç
  koşmuyor), `Hide`, uzak kenar, ayar değişimi ve `show_transfer` (aktarım
  satırı göstergenin yerini alınca popover kapanıyor).
- **`uploader::label`** `pub(crate)` oldu (kopyalanmadı).
- **Bilinen sınır**: popover açıkken fare popover'ın penceresinde, yani
  `mouseMoved:` terminale gelmiyor; iki dakika (`STATS_IDLE`) hiç etkileşim
  olmazsa örnekleme duruyor ve popover son değerde donuyor. Bir tık ya da
  tuş geri başlatıyor. Açık popover'ı etkileşim saymak unutulmuş bir
  popover'la sunucuyu bütün gece örneklemek olurdu (Karar 6'nın gerekçesi).
- **Doğrulama:** `make check`, `make smoke` (`content=2`, `quiet=1758` ms —
  süreli koşuda uzak oturum yok), `make linux` yeşil. `make test-race`
  gerekmedi: yeni paylaşılan durum yok (popover ve sürücü yalnız ana thread).
- **Elle kabul** (gerçek Linux sunucu: popover aç/kapa, Esc'in uzak kabuğa
  gitmemesi, canlı tazeleme, el imleci, yükleme başlayınca kapanış; phase-4'ün
  sekme değişimi, `off`, `exit` sonrası 120 s'de yardımcı ssh'ın kapanması)
  ve ayar penceresinin 780 pt yüksekliği bu ajan kabuğunda yapılmadı; set
  kapısının gözle kontrolüne kaldı.
- **Set kapısı — `/code-review` (medium, setin bütün diff'i, `8ba8d9c`
  hariç)** üç bulgu: (1) pane kapanınca açık yük popover'ı ve Esc
  izleyicisi kalıyordu → `begin_close` `close_stats_popover` çağırıyor;
  (3) `STATS_IDLE` geçip tik henüz fark etmeden gelen etkileşim yeniden
  başlatma sayılıp sparkline'ı siliyordu → `Schedule::interaction` tik
  kuruluyken ya da istek uçuştayken yalnız damga vuruyor (sınaması
  `an_interaction_before_the_tick_found_the_pause_keeps_the_history`);
  ayrıca `view.rs`'te `pane()`'in doc yorumu yerine döndü.
  **Waive (2):** `iowait`'in geri gitmesi `idle` farkını negatif yapınca o
  örnek CPU'suz kalıyor ve CPU grubu bir an düşüyor. Bilinçli (phase-3'ün
  `a_counter_that_goes_back_gives_no_cpu_once` sınaması): doymalı çıkarma
  boş farkı %100 meşgul okur ve sahte bir kırmızı `▲` üretirdi; CPU'suz
  örnek `FIRST_FOLLOW` (1 s) sonra yeniden örnekleniyor, yani kayıp bir
  saniyelik. Rapor dışı bırakılan: kendi kapattığımız popover'ın kapanış
  bildirimi yükleme listesinin koluna düşüyor — liste yoksa no-op,
  görünür etkisi yok.
- **`/audit`:** `make audit` temiz; ayar şeması, ölçüm sahipliği, thread,
  boşta sıfır kare ve dil mercekleri temiz; bağımlılık ve hücre/shader
  ilgisiz (değişmedi).
- Düzeltmelerden sonra `make check`, `make smoke` yeşil; `make linux`
  ilk koşuda dokunulmamış `jobs::tests::the_process_table_reads_a_real_argv`
  ile bir kez kırmızı (phase-1'deki bilinen yarış), ikinci koşu yeşil.
