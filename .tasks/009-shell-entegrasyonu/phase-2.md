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

## Checklist

- [ ] Sarmalayıcı: `Reader = Self`, `io::Read`, kalan metotların delegesi
- [ ] `Session::spawn` sarmalayıcıdan geçiyor
- [ ] Test: baytlar aynen geçiyor; OSC 133 basan bir oturumda durum oynuyor;
      kapanış `clean` kalıyor
- [ ] Doğrulama geçti (`make hepsi` + `make duman` + `make test-yaris`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
- [ ] Yayın etkisi yazıldı
