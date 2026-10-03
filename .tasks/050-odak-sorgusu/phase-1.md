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

## Checklist

- [ ] `focus.rs`: tel, saat, sunucu, istemci, `focus_main`
- [ ] `ssh_route.rs`: paylaşılan canlı örnek sayımı, `FOCUS_SOCKET` süpürmede
- [ ] Test: yukarıdaki Kabul senaryoları
- [ ] Doğrulama geçti (`make check` + `make linux`)
