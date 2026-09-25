# Phase 3 — İçerik view'ı kapsayıcıya döner

## Özet

Pencerenin içerik view'ı düz, layer-backed bir kapsayıcı olur ve
`BateriView` onun tek çocuğu; kullanıcının gördüğü hiçbir şey değişmez.

_Requirements: R4.1_

## Değişiklikler

- **`crates/bt-shell/src/window.rs`** — `TerminalWindow::new`: kapsayıcı
  (`setWantsLayer(true)`; yoksa sonraki phase'in paneli Metal katmanının altında
  kalabilir), `BateriView` onu doldurur (autoresizing), first responder yine
  `BateriView`. Geometrinin kaynağı `window.contentView()` değil `BateriView`
  (`sync_geometry`); çerçeve gözlemcisi zaten view'a bağlı, öyle kalır.
- **`crates/bt-shell/src/view.rs`** — sürükleme hedefi kaydı ve fare eşlemesi
  view-yerel kalır (`convertPoint_fromView`); değişiklik gerekmiyorsa
  dokunulmaz.

## Kabul

- `make hepsi` ve `make duman` yeşil, jetonlar bugünkü aralıkta.
- Gözle kontrol (kapanış mesajında): yazma, fareyle seçim, Finder damlası,
  pencere boyutlandırma, ikinci sekme açma/kapama (içerik uzayıp kısalıyor),
  punto değişimi bugünkü gibi.

## Checklist

- [x] Kapsayıcı + `BateriView` çocuğu, first responder
- [x] Geometri `BateriView`'dan
- [x] Doğrulama geçti (`make hepsi` + `make duman`)
- [~] Gerçek pencerede gözle kontrol — computer-use başka bir oturumca
  kullanılıyordu; sahne kapanış mesajında kullanıcıya bırakıldı

## Uygulama Notları

- `sync_geometry` artık `Option` dönmüyor: kaynak `WindowIvars.view`
  (`contentView()` değil), yani `None` kolu ve `start`'taki `expect`
  konusuz kaldı ve silindi.
- `view.rs`'e dokunulmadı: sürükleme kaydı ve `convertPoint_fromView(_, None)`
  zaten view-yereldi.
