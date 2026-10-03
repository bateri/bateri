# Phase 1 — Tel, sunucu ve istemci (`bt-shell-common`)

## Özet

Odak sorgusunun platformsuz yarısı: tel biçimi, sınırlı bir unix soketi
sunucusu, örnekleri bulan istemci ve süpürmenin yeni sokete uyumu — AppKit
yok, sahte bir cevaplayıcıyla birim sınanıyor.

_Requirements: R1.1, R1.2, R3, R4, R5, R6_

## Değişiklikler

- **`crates/bt-shell-common/src/focus.rs`** (yeni) — modül başlığı neden'i ve
  `.tasks/050-odak-sorgusu/` işaretçisini taşır.
  - **Tel:** isteğin ayrıştırılması (`focus 1 <UUID>`; sürüm, UUID
    `bt_core::TabId` ile, satır uzunluğu sınırlı) ve cevabın jeton satırı
    (`Answer`: `Live { focused, idle_secs }` / `None` / `Unknown`). Tanınmayan
    sürüm ve bozuk satır cevapsız kapanır; istemci onu `unknown` okur.
  - **Saat:** uykuyu sayan monoton damga (macOS `CLOCK_MONOTONIC`, Linux
    `CLOCK_BOOTTIME`; `libc::clock_gettime`) ve `idle`'ın tam saniyeye aşağı
    yuvarlanması — tek fonksiyon, sınanan.
  - **Sunucu:** `serve(dir, answerer)` — `<dir>/focus`'u bağlar (önce bayat
    dosyayı kaldırır; yol yalnız `SUN_PATH`'e göre sınanır, `fits()`'in
    boşluk/`%` reddi uygulanmaz), accept döngüsü bağlantı başına okuma
    sınırıyla; cevaplayıcı `Fn(&TabId) -> Option<Answer>` ve kendi
    sınırını kendisi uygular (macOS'unki ana kuyruğu bekler). Bir istemci
    döngüyü kilitleyemez: bağlantı başına sınır ya da bağlantı başına kısa
    ömürlü thread — seçim uygulamada, gerekçesi Uygulama Notları'na.
  - **İstemci:** `ask(roots, pid: Option<u32>, tab) -> Answer` — örnekleri
    bulur, örnek başına connect/read sınırı ve toplam son tarihle sorar;
    `--pid`'de tek örnek, yoksa sırayla ve ilk `Live` kazanır; hiçbiri
    tanımıyorsa `None`, cevap alınamadıysa `Unknown`. Sınır sayıları adlı
    tasarım sabitleri (yüzlerce ms; askpass'in `ANSWER_WAIT`'i emsal değil).
  - **Alt komutun gövdesi:** `focus_main(args, roots, out) -> i32` —
    `[--pid P] <url>` ayrıştırma (URL `TabId::from_url`), jeton satırını
    basar, çıkış kodları (cevap 0, `unknown` ayrı kod, kullanım hatası ayrı
    kod; adlı sabitler).
- **`crates/bt-shell-common/src/ssh_route.rs`**
  - `live_instances(roots)` (ya da eşdeğeri): kökler altındaki örnek
    dizinlerini sahibiyle verir; `sweep` aynı yürüyüşü ölüler için kullanır,
    `focus::ask` canlılar için — tek kopya. Aynı pid'in iki kökteki dizini
    bir örnek.
  - Soket adı (`FOCUS_SOCKET`) `remove_instance`'ın "bizim adlarımız"
    koşuluna açık sabit olarak girer; `our_socket_name`'e değil (`sweep_flat`
    ve `our_sockets` onu `ssh -O exit` için de kullanıyor). ⌘Q yolunun
    (`close_all`) dizini soketle birlikte kaldırdığı doğrulanır, değilse
    aynı küme oraya da.
- **`crates/bt-shell-common/src/lib.rs`** — modülün kaydı.

## Kabul

- Birim sınamalar: tel round-trip ve bozuk/uzun/tanınmayan sürüm satırı;
  `idle` yuvarlaması; sahte cevaplayıcıyla gerçek soket üstünden `Live`/
  `None`; dinleyip cevap vermeyen sokete karşı istemcinin sınır içinde
  `Unknown` dönmesi; bayat soket dosyası (`ECONNREFUSED`) atlanır; `--pid`
  yalnız o örneğe sorar; yazıp göndermeyen bir istemci ikinci istemcinin
  cevabını engellemez; `remove_instance` `focus` soketli dizini kaldırır
  (bugünkü süpürme sınamalarının yanında).
- `make check` ve `make linux` yeşil.

## Uygulama Notları

- **`ask` `Reply` dönüyor, `Answer` değil:** Karar 8'in "cevabı örnek üretir,
  CLI olduğu gibi aktarır"ı için `pane=live`'da örneğin kendi satırı
  (`Reply::line`) aynen basılıyor; istemci tanımadığı jetonu atlıyor, yalnız
  `pane=`/`focused=`/`idle=`'yi ve satırın basılabilir ASCII olmasını şart
  koşuyor. `none` ve `unknown` kanonik satırla.
- **Sunucu bağlantı başına kısa ömürlü thread** (en çok `MAX_IN_FLIGHT` = 8,
  fazlası kabul edilip kapanıyor): tek accept thread'inde sıralı sınır,
  yazmayan bir istemcinin ardındakini `SERVER_READ_LIMIT` kadar bekletirdi;
  sınama ikinci istemcinin bu sınırın altında cevaplandığını ölçüyor.
  `serve` soketi döndürmeden bağlıyor, accept döngüsünü kendi thread'inde
  başlatıyor (phase-2 ve sınamalar dinlemeyi hazır buluyor).
- **Canlı ama soketsiz örnek `unknown`:** eski sürüm, açılışta henüz
  kurulmamış dinleyici ya da bayat soket (`ECONNREFUSED`) o pane'i yine de
  tutabilir — "bilinmiyor ≠ yok". Toplama: ilk `live` kazanır, yoksa bir
  `unknown` varsa `unknown`, yoksa `none`; canlı örnek yoksa `none`.
- **connect'in ayrı sınırı yok:** std'de `UnixStream::connect` zaman aşımı
  almıyor ve yerel connect yalnız dolu backlog'da (Linux) bloklanıyor;
  sunucu accept'i hiçbir şeyi beklemeden döndüğü için o durum ölü dinleyici
  demek. Sınırı okuma/yazma zaman aşımı ve toplam son tarih taşıyor.
- **Sınır sayıları:** sunucu okuma 200 ms, cevaplayıcı bekleme
  (`ANSWER_WAIT`, phase-2'nin ana kuyruğu) 200 ms, istemci örnek başına
  500 ms, toplam 900 ms; çıkış kodları `0` cevap, `2` kullanım, `3` unknown.
- **Süpürme:** `sweep` ve `live_instances` tek yürüyüşü (`instance_entries`)
  paylaşıyor; `live_instances` yarım doğmuş dizinleri almıyor. `FOCUS_SOCKET`
  `focus.rs`'de, `remove_instance` onu yalnız adıyla siliyor (⌘Q'da
  dinleyici hâlâ canlı); `close_all` aynı yoldan dizini kaldırıyor, sınaması
  var.

## Checklist

- [x] `focus.rs`: tel, saat, sunucu, istemci, `focus_main`
- [x] `ssh_route.rs`: paylaşılan canlı örnek sayımı, `FOCUS_SOCKET` süpürmede
- [x] Test: yukarıdaki Kabul senaryoları
- [x] Doğrulama geçti (`make check` + `make linux`)
