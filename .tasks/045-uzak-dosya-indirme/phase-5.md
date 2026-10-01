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

- [ ] Jest eşiği ve `Drag::Link`
- [ ] Sürükleme kaynağı + file promise delegesi + NSProgress
- [ ] Test: Kabul listesi
- [ ] Doğrulama geçti (`make check`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi (yalnız `Cargo.lock` değiştiyse kalır)
