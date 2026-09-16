# Phase 2 — Akışı dinleyen PTY sarmalayıcısı

## Özet

Tarayıcı gerçek baytları görmeye başlıyor: `Pty`'yi saran bir tip
`EventLoop`'a veriliyor ve okuma yolundan geçen her bayt geçerken taranıyor.

_Requirements: R1.1, R1.2_

## Değişiklikler

- **`crates/bt-core/src/session.rs`** — `Pty`'yi saran **özel** bir tip
  (`pub` API'de görünmez; katman kuralı: dışarıya alacritty tipi sızmaz).
  Sözleşmenin tamamı beş metot ve bir ilişkili tip:
  - `EventedReadWrite`: `type Reader = Self` — sarmalayıcının **kendisi**
    okuyucudur. Bu, kararın çekirdeği: `Pty::reader()` `&mut File` döndürüyor,
    yani delege etmek için ödünç yeterli ve **ikinci bir fd açılmıyor**.
    `pty.file().try_clone()` yolu bilerek seçilmedi; gerekçesi ve reddi
    `discussion.md` → Muhakeme'de.
  - `io::Read`: içerideki `Pty`'den okur, **baytları değiştirmeden** döndürür,
    dönmeden önce dilimi tarayıcıya verir. Tarayıcı durumu yaprak kilide yazar.
  - `writer()`, `register`/`reregister`/`deregister`, `EventedPty`'nin
    `next_child_event()`'i ve `OnResize` içerideki `Pty`'ye delege edilir —
    fd kaydı onun, yani hazır olma sinyali değişmez.
- **`Session::spawn`** — `tty::new`'un sonucu doğrudan `EventLoop`'a değil,
  sarmalayıcıdan geçerek gider. Başka çağrı yeri yok (bugün `Pty`'ye dokunan
  tek iki satır bunlar).

**Korunacak sınır:** kapanış yolu. `EventLoop`'un kanalı, `join`'i, `SIGHUP`
sırası ve `SHUTDOWN_GRACE` ölçümü **değişmiyor**; sarmalayıcı `Pty`'yi
sahipleniyor, yani `Pty::drop`'un penceresi de aynı kalıyor. `kapanis=`
jetonunun dağılımı bu phase'de oynamamalı.

## Kabul

- Duman koşusu değişmeden geçiyor: `make duman` yeşil ve jeton satırının
  sabit sayaçları (`hucre=8 glif=6 kural=15 yuva=13/2048`) aynı; `kapanis=clean`.
- Akış bozulmuyor: sarmalayıcının `read()`'i baytları **aynen** geçiriyor
  (dilimi kopyalamayan, değiştirmeyen bir sınama).
- Tarayıcı artık besleniyor: kabuğa elle OSC 133 bastıran bir sınama
  (`printf` ile) `Session::shell_state()`'i oynatıyor.
- Sarmalayıcı hiçbir kare istemiyor — durum değişimi kendi başına kare
  doğurmuyor; işaretin karesi zaten alacritty'nin `Wakeup`'ından geliyor
  (`discussion.md` → Muhakeme).

## Yayın Etkisi

shader yok · terminfo yok · ayar şeması yok · tema yok · app bundle yok ·
yeni bağımlılık yok.

**Ölçüm bekliyor:** tarayıcının akış maliyeti — baytlar artık iki kez
geziliyor (tarayıcı + ayrıştırıcı). Kanca hazır (`BT_SCROLL_TEST` yükü +
`BT_FRAME_STATS`); sayı `/measure` ile `docs/OLCUMLER.md`'ye girer. Bu phase
hiçbir maliyet iddiası **yazmaz**.

`CLAUDE.md`'nin `bt-core` satırı ("PTY ve okuyucu thread") sarmalayıcıyı
anacak kadar güncellenir.

**Yeni bağımlılık:** `polling` `bt-core`'a doğrudan bağımlılık oldu (aşağıda,
Uygulama Notları). Kullanıcı onayı alındı; `Cargo.lock`'ta yalnız bir kenar.

## Uygulama Notları

- **`polling` doğrudan bağımlılık oldu — plan da Muhakeme de bunu görmemişti.**
  `EventedReadWrite`'ın üç kayıt metodu imzasında `Poller`, `Event` ve
  `PollMode` geçiriyor; alacritty `polling`'i **yeniden ihraç etmiyor** (yalnız
  `vte`'yi ediyor), yani bir impl yazabilmek için tipleri adlandırmanın başka
  yolu yok. Kapı işletildi ve kullanıcı onayladı. Emsali `libc` ve `block2`:
  yeni bir crate değil, grafta zaten vardı (alacritty PTY'yi onunla yokluyor),
  sürüm sınırımız (3.8) onunkiyle aynı 3.11'e çözülüyor ve `Cargo.lock`'ta
  tek satır değişti — `bt-core`'un bağımlılık listesine bir kenar.
- **Yuva `spawn`'ın başına taşındı.** Eskiden dönüş yapısının içinde doğuyordu;
  sarmalayıcı `EventLoop::new`'dan önce kurulduğu için `Arc` de artık ondan
  önce doğuyor ve bir ucu oraya, biri `Session`'a gidiyor.
- **Trait'lerin ada gelmesi asimetrik ve bu bilinçli.** `EventedReadWrite`
  `use` ile alınıyor, `EventedPty` ve `io::Read` alınmıyor: Rust bir trait'i
  **kendi impl bloğunun içinde** metot çözümüne zaten sokuyor, `Pty::reader()`
  çağrısı ise `io::Read` gövdesinden — yani başka bir bloktan — geliyor.
  Üçünü birden almak `-D warnings`'i kırmızıya düşürüyordu; sebep yorumla
  çivilendi, yoksa ilk okuyan eksik sanıp geri ekler.
- **Test-first bu kez sırayla koştu ve kırmızının yeri kendisi bir kanıt
  oldu.** Phase-1'in `shell_state_stays_empty_without_a_feeder`'ı ikiye
  ayrıldı: biri işaretsiz akışta `None`'ı çiviliyor (`None` = "entegrasyon
  yok", "besleyen yok" değil), öteki dört işaretin duruma düşüşünü. İkincisi
  bağlamadan önce koşturuldu ve **yalnız durum satırında** düştü — `abcd`
  satırı daha o zaman geçiyordu, yani baytların aynen geçmesi zaten
  doğruydu ve sınama yeni davranışı ölçüyor.
- **"Baytlar aynen geçiyor"un kanıtı runtime kontrolü değil ızgaranın
  kendisi.** Tarayıcı `&[u8]` alıyor, dilimin sahibi çağıran — kopyalayamaz da
  değiştiremez de. Izgaradaki `abcd` bunun üstüne ikinci bir şeyi kanıtlıyor:
  çerçeveleme `vte`'ninkiyle hizalı. Ayrı hizalasaydı dizinin bir parçası harf
  olarak basılırdı.
- **Taranan dilim `&buf[..read]`, tamponun tamamı değil.** `EventLoop` tamponu
  biriktirerek okuyor (`reader().read(&mut buf[unprocessed..])`); baştan
  taramak aynı baytı iki kez işarete çevirirdi.
- **`/code-review` phase-1'in tarayıcısında dördüncü bir parite kuralı
  buldu** ve bu bir kusurdu: `advance_esc` C0'ların 0x18/0x1A dışındakilerinde
  ve 0x7F'ten büyük baytlarda `Escape`'te **kalıyor**, yani `ESC \r ] 133;A BEL`
  ızgarada geçerli bir işaret. Tarayıcı orada Ground'a düşüyor, işareti
  sessizce kaybediyordu — tam da modülün "iki taraf aynı hikâyeyi okusun"
  gerekçesinin yasakladığı sapma. Kural, modül başlığı ve iki yönlü sınaması
  (kalan üç bayt sınıfı + gerçekten Ground'a götüren 0x18/0x1A) eklendi.
- **İkinci bulgu bir belge çelişkisiydi:** `TappedPty`'nin ilk doc'u
  `pty.file().try_clone()`'u "kapanış dengesini oynatırdı" diye reddediyordu,
  oysa `Session::shutdown`'ın doc'u ve `CLAUDE.md` aynı çağrıyı takılan çocuk
  için **kalıcı çare** olarak borca yazmış. Doğru cümle panelin kendi cümlesi:
  taramak için ikinci fd'ye *gerek yok*, açmamak da bu phase'in ölçülmüş
  dengeye dokunmaması demek. Borçtaki dup ayrı bir iş (master'ı `wait`
  bloklarken boşaltmak) ve sarmalayıcı onu **engellemiyor** — doc bunu artık
  açıkça söylüyor, yoksa sonraki okuyucu borcu kapanmış sanardı.
- **Üçüncü bulgu:** `polling` `CLAUDE.md`'nin bağımlılık listesine de yazıldı;
  `Cargo.toml`'daki gerekçe tek başına yeterli değildi, çünkü `/audit`
  merceğinin okuduğu liste orası.
- **Dördüncü bulgu bilerek ertelendi (ölçüm işi).** Tarayıcının boşta hızlı
  yolu skaler `position(|b| b == 0x1b)`, `vte` aynı baytları SIMD `memchr` ile
  yeniden geziyor; ayrıca `Skip` durumunun hiç hızlı yolu yok (büyük bir OSC 52
  yükü bayt bayt `step()`'ten geçiyor). İkisi de **doğruluk** sorunu değil ve
  ikisinin de çaresi ölçülmemiş bir iddia üstüne kod eklemek olurdu; zaten bu
  phase'in `## Yayın Etkisi`'nde duran "ölçüm bekliyor: tarayıcının akış
  maliyeti" kalemi tam olarak bunu soruyor. `memchr`'ı doğrudan bağımlılık
  yapmak ayrıca **ikinci** bir bağımlılık kapısı demek. `/measure` sayıyı
  koyduktan sonra karar verilir.

## Checklist

- [x] Sarmalayıcı: `Reader = Self`, `io::Read`, kalan metotların delegesi
- [x] `Session::spawn` sarmalayıcıdan geçiyor
- [x] Test: baytlar aynen geçiyor; OSC 133 basan bir oturumda durum oynuyor;
      kapanış `clean` kalıyor — ilk ikisi tek gerçek PTY turunda
      (`marks_from_the_stream_walk_the_shell_state`; `abcd` satırı geçişin
      kanıtı), işaretsiz akışın `None`'ı ayrı sınamada. **Kapanış için yeni
      sınama yazılmadı:** `shutdown_ends_the_reader` zaten `Teardown::Clean`
      iddia ediyor ve `make duman` `kapanis=clean` basıyor, ikisi de bu
      değişiklikten sonra yeşil
- [x] Doğrulama geçti (`make hepsi` + `make duman` + `make test-yaris`;
      duman'ın sabitleri Kabul'deki gibi: `hucre=8 glif=6 kural=15
      yuva=13/2048`, `kapanis=clean`)
- [x] Riskli phase: `/code-review` koştu, dört bulgunun üçü giderildi;
      dördüncüsü (tarayıcının bayt başına maliyeti) ölçüme bırakıldı,
      gerekçesi Uygulama Notları'nda
- [x] Yayın etkisi yazıldı
