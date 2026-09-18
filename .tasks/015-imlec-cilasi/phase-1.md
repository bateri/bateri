# Phase 1 — Devrin histerezisi

## Özet

Hızlı komutta caret'in dock → ızgara → dock gidiş-dönüşü görünmez olsun:
Dock→Grid geçişi bir süre tutulsun, o sürede geri dönerse hiç olmamış sayılsın.

_Requirements: R1, R1.1, R1.2, R1.3, R1.4, R10 (bayat doc yarısı)_

## Değişiklikler

- **`crates/bt-core/src/shell.rs`** — histerezisin tamamı burada.
  - `ShellLog`'a devrin **damgası**: son `caret_home` cevabı ve ne zaman
    değiştiği. Geçiş `apply_scan`'de gözleniyor — tek giriş noktası ve yaprak
    kilidin altında, yani `Term`'e hiç dokunmadan.
  - `caret_home` üçüncü bir argüman alıyor (tutma süresi). **Yalnız Dock→Grid
    yönü tutuluyor:** ters yön (Grid→Dock) geciktirilirse komut bitince caret
    ızgarada asılı kalır ve kullanıcı yazmaya başladığında dock'ta caret'siz
    bir satır görür — yanlışın yönü güvenli değil.
  - **Devir `line-finish`'te başlıyor, `CommandStart`'ta değil** (ölçüldü,
    `context.md` → Kanıt). `running_since`'e bağlanan bir eşik ilk geçişi
    görmez ve üçüncü bir devir doğurur; bu phase ona **hiç dokunmuyor**.
- **`crates/bt-core/src/session.rs`** — eşiğin kalan süresi `Cursor::next_tick`
  ile isteniyor.
  - **Aynı kilit turunda** hesaplanıyor (`caret_home`'un okunduğu tur), yoksa
    iki ayrı ana ait iki cevap doğar.
  - **`min`'leniyor**, ezilmiyor: bugün `resolve_blocks` `next_tick`'i
    doğrudan yazıyor ve o yol koşan bloğun çıpasının görünür olmasına bağlı —
    devir buna bağlanamaz. Emsal, iki son tarihi tek saatte birleştiren
    `arm_clock` (014 phase-2).
  - Saatin üç şartı bu phase'de yazılı olacak: içerik gerçekten değişiyor
    (caret yer değiştiriyor, doluluk sayısı oynuyor), tek atımlık,
    adlandırılmış durma koşulu (tutma süresi doldu ya da yüklem geri döndü).
- **`crates/bt-core/src/shell.rs` (doc)** — `caret_home`'un serbest fonksiyon
  gerekçesi bayat: "`dock::render` defteri değil kopyalarını taşıyor" diyor ama
  o çağrı kalkmış; üretimde tek çağıran `ShellLog::caret_home`.

## Kabul

- `ls` gibi hızlı bir komutta caret **hiç kıpırdamıyor** — ne yukarı çıkıyor ne
  içerik zıplıyor. İkisi tek yüklemden beslendiği için tek değişiklik ikisini
  de kapatıyor.
- `sleep 2` gibi yavaş bir komutta devir **oluyor** ve gecikmesi tutma süresi
  kadar: caret ızgaraya geçiyor, komut bitince dock'a dönüyor.
- Komut **girdi isteyince** (`cat` beklerken, `ssh` parolası) caret ızgarada;
  tutma süresi o davranışı geciktiriyor ama değiştirmiyor.
- Tutma süresi dolduğunda kare **isteniyor**: çıktısı olmayan bir komutta
  (`sleep 5`) devir kendiliğinden gerçekleşiyor, bir sonraki hasarı beklemiyor.
- Komut bitince saat **sönüyor** — bekleyen bir tutma yokken `next_tick` bu
  yoldan `Some` dönmüyor.

## Uygulama Notları

<!-- Kodlanırken doldurulacak. -->

## Yayın Etkisi

- **ölçüm bekliyor: "sıçrama azaldı".** İddia **azaltma**, kaldırma değil:
  animasyon ~230 ms'de yerleşiyor ve tutma süresi onun altındaysa belirti
  küçülür ama bitmez. Doğrulaması önce/sonra göz kontrolü; kancası yok ve bu
  set kanca doğurmuyor.
- **seçilmiş sayı:** tutma süresi. `const` doc'unda "seçilmiş, ölçülmemiş" +
  gerekçe (013'ün "bir saniyeyi geçmeyen komutun sayacı gösterilmez" emsali).
  `docs/OLCUMLER.md`'nin konusu **değil**.
- shader / ayar şeması / tema / terminfo / app bundle / yeni bağımlılık: yok.
  `make kur` gerekmiyor.
- `CLAUDE.md`: devrin tarifi ("kalan her hâlde dock'un") tutma süresini
  anmalı — bugünkü cümle onsuz yanlış olur.

## Checklist

- [ ] `bt-core`: `ShellLog`'da devrin damgası, `apply_scan`'de gözlem
- [ ] `bt-core`: `caret_home` tutma süresini alıyor, **yalnız Dock→Grid** yönü
- [ ] `bt-core`: `next_tick` aynı kilit turunda, `resolve_blocks`'unkiyle
      `min`'leniyor (ezilmiyor)
- [ ] Test: hızlı komut devir **doğurmuyor** (yüklem düzeyinde)
- [ ] Test: yavaş komut devri **doğuruyor**, gecikmesi tutma süresi kadar
- [ ] Test: tutma dolunca `next_tick` kare istiyor; komut bitince sönüyor
- [ ] Test: `resolve_blocks`'un tiki ezilmiyor (`min` bekçisi)
- [ ] `caret_home`'un bayat doc'u düzeltildi
- [ ] `CLAUDE.md` devrin tarifi
- [ ] Doğrulama geçti (`make hepsi`)
- [ ] `make test-yaris` (paylaşılan durum: `ShellLog`'a yeni alan)
- [ ] `make duman` (kullanıcıda)
- [ ] Yayın etkisi yazıldı
