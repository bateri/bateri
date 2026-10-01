# Phase 5 — ⌘-sürükle ile Finder'a indirme

## Özet

Uzak bağlantıya ⌘-basıp eşiği aşan sürükleme file promise sürüklemesi
başlatır; Finder'ın verdiği hedefe sıra beklemeden indirir.

_Requirements: R7_

## Değişiklikler

- **`crates/bt-shell-common/src/gesture.rs`** — `Gesture` basış konumunu
  tutar; bağlantı basışında eşiği (tasarım sabiti) aşan ilk hareket
  `Drag::Link`'i (yeni kol) bir kez döner, sonrası `Ignore`; eşik altı
  bırakma bugünkü `Release::Link`. Yerel bağlantıda kol yok (Karar 14).
- **`crates/bt-shell-macos/src/view.rs`** — `NSDraggingSource` (yalnız Copy),
  `drag_event`'in `Drag::Link` kolu `beginDraggingSessionWithItems` ile
  `NSFilePromiseProvider` (dosya tipi uzak adın uzantısından UTType, klasörde
  `public.folder`) başlatır; sürükleme görüntüsü sistem dosya simgesi + ad.
- **`crates/bt-shell-macos/src/promise.rs`** (yeni) —
  `NSFilePromiseProviderDelegate`: `fileNameForType` (uzak ad),
  `writePromiseToURL:completionHandler:` Finder şeridine iş verir ve
  tamamlanınca handler'ı çağırır; ilerleme hedef URL'ye `NSProgress`
  (`publish`) olarak; iptal/hata handler'a hata döner. Çakışma: Finder'ın
  verdiği ad yazılır, çakışma kuralı uygulanmaz (Finder hedefi kendisi
  seçti).
- **`crates/bt-shell-macos/Cargo.toml`** — `objc2-app-kit` bayrakları
  (`NSFilePromiseProvider`, `NSDraggingSession`, `NSDraggingItem`,
  `NSPasteboardItem`); `Cargo.lock` değişirse dur (Karar 15).

## Kabul

- Sınamalar: `gesture` eşiği (altında açma, üstünde bir kez `Drag::Link`,
  yerel bağlantıda hiç).
- `make check` yeşil.
- Gözle kontrol (gerçek ssh + Finder): dosyayı ve klasörü Masaüstü'ne
  sürükle; kuyrukta başka bir aktarım varken de hemen başlar; simgede
  ilerleme (Karar 7 — görünmezse Uygulama Notları'na ve kullanıcıya); uzun
  indirmede Finder'ın tahammülü; iptalde Masaüstü'nde yarım öğe kalmaz.

## Checklist

- [x] Jest eşiği ve `Drag::Link`
- [x] Sürükleme kaynağı + file promise delegesi + NSProgress
- [x] Test: Kabul listesi
- [x] Doğrulama geçti (`make check`, `make linux`)
- [~] Riskli phase: `/code-review` koştu, bulgular giderildi — `Cargo.lock` değişmedi

## Uygulama Notları

- Jest: `Gesture` yalnız sürüklenebilir (uzak) bağlantı basışında basış noktasını tutuyor (`pressed_link(Some(nokta))`, yerelde `None`); `dragged` artık `&mut self` + pencere noktası alıyor ve eşiği (`LINK_DRAG_THRESHOLD` = 4 pt, tasarım sabiti, ölçülmedi) aşan ilk hareket `Drag::Link`'i döner, bağlantı bitini düşürür — sonrası `Ignore`, bırakma `Done`. `Gesture`'ın `Eq` türetmesi kalktı (nokta `f64`; hiçbir yer karşılaştırmıyordu). `link_press` artık `Option<bool>` (sürüklenebilir mi) döner; view `Drag::Link`'te kilitli hover'ı alıp (`link_drag`) sürüklemeyi başlatıyor, çünkü oturum başlayınca AppKit `mouseUp:`'ı yutuyor.
- Kaynak `BateriView` (`NSDraggingSource`, iç/dış yalnız Copy); delege ayrı sınıf `BateriFilePromise` (`promise.rs`), `MainThreadOnly` **değil** ve kendi arka plan `NSOperationQueue`'sunu veriyor: AppKit yazımı dosya koordinasyonuyla sarıp teslim eden thread'de handler'ı bekleyebilir; handler'ın arkasındaki iş ana thread'i istediği için ana kuyruk kilitlenme riskiydi. Yazım geri çağrısı handler'ı kopyalayıp hemen döner ve ana kuyruğa atlar.
- Handler **yapı gereği bir kez**: `Promised` sarmalayıcısı `finish` ya da — bitmeden düştüğü her yolda (pane kapandı, oturum bitti/değişti, sayım hatası, kuyruk reddetti, `begin_close`) — `Drop`'unda `NSUserCancelledError` ile çağırıyor. Delege sağlayıcıda zayıf; pane onu sürükleme oturumuyla birlikte tutuyor (`FinderDrops::promises`), Finder sorunca ya da sürükleme bırakmasız bitince (`draggingSession:endedAtPoint:operation:` → `None`) bırakıyor.
- Yazım: ana thread → yardımcıya taze `Query::Count` (klasörün dosya/bayt toplamı çubuğun ve `NSProgress`'in paydası; önizlemenin emsali) → ana thread `Job::download(…, Lane::Finder, Conflict::Replace)` → `upload_confirmed`. `enqueue` `asking`'e bakmadığı için bir sayfa açıkken ve kuyrukta başka aktarım akarken de hemen başlıyor; reddin tek hâli durdurulan kuyruğun bitmemiş öğesi (`ending`) ya da başka uzak oturumun kuyruğu — promise hata metniyle döner. Çakışma kuralı uygulanmıyor (Finder adı ve yeri seçti); `land`'in Replace kolu klasörü de değiştirebiliyor.
- `NSProgress` (kind file, downloading, `fileURL` = Finder'ın URL'si, iptal edilebilir) iş kuyruğa girerken `publish`, bitişte `unpublish`; tamamlanan bayt `upload_refresh`'te (`Shared::progress`, artık `pub`) güncelleniyor. Finder'ın iptali (`cancellationHandler`) ana kuyrukta öğeyi sormadan durduruyor (`apply_stop`, `pub(crate)` oldu). **Doğrulanmadı (Karar 7):** indirme hedefin yanındaki gizli geçici klasöre yazıp sonda `rename` ediyor, yani akış boyunca URL'de dosya yok — Finder'ın simgedeki ilerlemeyi var olmayan öğede gösterip göstermediği gözle kontrolün konusu. Göstermezse bile yer tutucu dosya **eklenmedi**: iptalde Masaüstü'nde yarım öğe bırakırdı (geçici klasör iptal/hata'da siliniyor, öğe yalnız tamamlanınca beliriyor).
- Sürükleme görüntüsü: tipin sistem simgesi (`NSWorkspace iconForContentType:`, `UTType` çalışma zamanından — `iconForFileType:` kullanımdan kalkmış) + adı, atılan bir view'ın PDF'inden; simge alınamazsa görüntüsüz ama çerçeveli öğe. Dosya tipi uzantının `UTType` kimliği, bilinmeyende `public.data`, klasörde `public.folder`. İnen öğe diğer indirmeler gibi karantinalı (`spawn_transfer`'ın `quarantine` kancası).
- Bayraklar `NSFilePromiseProvider`, `NSDraggingItem`, `NSDraggingSession` yetti; `NSPasteboardItem` gerekmedi (sağlayıcı `NSPasteboardWriting`, `NSDraggingItem::initWithPasteboardWriter` `NSPasteboard` bayrağıyla zaten açık) — eklenmedi. `Cargo.lock` değişmedi.
- Gözle kontrol (Kabul) gerçek ssh sunucusu + Finder istiyor, işaretlenmedi: dosyayı ve klasörü Masaüstü'ne ⌘-sürükle; kuyrukta başka aktarım varken de hemen başlar; simgede ilerleme görünür mü (Karar 7); uzun indirmede Finder'ın tahammülü (zaman aşımı/hata?); Finder'ın iptali ve popover'ın `Cancel`'ı sonrası Masaüstü'nde yarım öğe kalmaz; eşik altı ⌘-tık hâlâ önizler; yerel bağlantıda ⌘-sürükle hiçbir şey yapmaz.
