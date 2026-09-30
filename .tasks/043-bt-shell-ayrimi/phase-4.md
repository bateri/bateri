# Phase 4 — `watch`: tek bildirim sözleşmesi ve inotify gövdesi

## Özet

`Watch::install(paths, notify)` kuyruksuz olur; bildirim iki platformda da
modülün kendi arka plan kuyruğundan/thread'inden gelir, macOS çağıranı onu
ana kuyruğa taşır. Linux gövdesi inotify (`discussion.md` → Karar 1).

_Requirements: R3.3, R3.4_

## Değişiklikler

- **`crates/bt-shell-common/src/watch.rs`** (ya da `watch/` altında adlı
  iki gövde) — sözleşme doc'u: bildirim hangi thread'de gelir, "önce kur,
  sonra oku" ve "olmayan yol kaynak doğurmaz" aynen. macOS gövdesi modülün
  özel seri kuyruğunda (statik), kaynak ve iptal işleyicisi bugünkü gibi.
  Linux gövdesi `libc` inotify: dizin (giriş doğumu/silinmesi/üstüne taşıma,
  dizinin kendisi gidince) ve dosya (yazma, ekleme, öznitelik — boşaltma
  için —, silinme, taşınma; bağın hedefi) maskeleri; okuma bildirmez. İzleme
  başına bir thread, `Drop` onu beklemeden durdurur (kendi uyandırma
  tanıtıcısıyla; kapatılan fd'ye okuyan thread bırakılmaz).
  `#[cfg(test)]` (ya da `test-support`) `flush`: sınamanın bariyeri, iki
  gövdede de o ana kadarki olayların bildirildiği an.
- **Sınamalar** — yedi sınama bu `flush`'la; kendi kuyruklarını kurmuyorlar.
  Senaryoları ve ölçütleri değişmez.
- **`crates/bt-shell/src/app.rs`** — iki `Watch::install` çağrısı kuyruksuz;
  `watch_notify` bildirimi `DispatchQueue::main().exec_async` ile ana
  kuyruğa taşır (bugün kaynaklar ana kuyrukta koşuyor ve
  `notify_settings_changed` `MainThreadMarker` istiyor). `audit:` notu
  (`app.rs:560`) yeni sözleşmeye göre.

## Kabul

- `watch`'ın yedi sınaması macOS'ta ve `make linux`'ta geçiyor; gövdelerinin
  ebeveynle farkı yalnız düzenek (kuyruk → `flush`), senaryo ve assert aynı —
  diff'te gözden geçirildi.
- Ayar kaydının canlı uygulanması macOS'ta bugünkü gibi (kapının gözle
  kontrolünde, phase-5).
- `make hepsi` ve `make linux` yeşil.

## Checklist

- [x] Kuyruksuz `install`, macOS özel kuyruk, `app.rs` ana kuyruğa taşıyor
- [x] inotify gövdesi ve `Drop`
- [x] `flush` ve yedi sınama iki platformda
- [x] Test: sınama gövdelerinin farkı yalnız düzenek
- [x] Doğrulama geçti (`make hepsi` + `make linux`)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi (thread'li paylaşılan durum)

## Uygulama Notları

- **`flush` bir yöntem (`Watch::flush(&self)`), sınamaların `drain`'i
  `&Watch` alıyor:** Linux'ta bariyer izlemenin kendi thread'i. Eski
  izlemenin geç bildirimi ayrıca kapalı: thread durma bitini ve `notify`'ı
  `Drop`'un biti kurduğu mutex altında soruyor, yani `Drop`'tan sonra
  bildirim gelmiyor (`Drop` en çok uçuştaki bir bildirimi bekliyor, thread'i
  değil). Sözleşmeye kural olarak girdi: `notify` beklemez ve kendi
  `Watch`'ını düşürmez.
- Linux'ta `sources` yalnız sınamaların saydığı `wd` listesi (inotify
  tanıtıcısını kapatmak hepsini siliyor); `not(test)`'te `dead_code` izni
  gerekçeli. Thread kurulamazsa izleme boş döner (yok sayılan yol gibi).
- Ana kuyruğa taşıma `watch_notify`'da; `notify_settings_changed`'in `audit:`
  notu ve `expect` metni ona göre.
- Paylaşılan durum (thread) değiştiği için `make test-yaris` de koştu.
- **`/code-review` bulguları:** macOS'ta olay birleştirmesi kayboluyordu
  (kaynaklar ana kuyruktayken libdispatch meşgul ana kuyrukta olayları tek
  çağrıda topluyordu; artık her olay ana kuyruğa ayrı iş atardı) →
  `watch_notify`'da uçuşta en çok bir taşıma (`WATCH_PENDING`, ana kuyruk işi
  bayrağı yeniden okumadan önce indiriyor). `watch` modülü yalnız
  macOS/Linux'ta; eskiyen üç manifest/`CLAUDE.md` cümlesi güncel. İki bulgu
  tasarım gereği kaldı: izleme başına thread (phase metni, kurulum kayıt
  başına — saniyede değil) ve `notify`'ın kilit altında çağrılması (`Drop`'tan
  sonra bildirim yok; kural sözleşme doc'unda, macOS'ta çağıran
  `exec_async`).
