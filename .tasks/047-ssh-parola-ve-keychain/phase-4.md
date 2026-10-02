# Phase 4 — bateri'nin bağlantısı kullanıcının ssh oturumuna bağlı

## Özet

Arka plan işlerini kullanıcının ssh girişini bekletmek, master'ı kullanıcının
o host'taki son oturumuyla ve uygulamanın kapanışıyla kapatmak, soket dizinini
örnek başına ayırmak (gerekçe ve kanıt `discussion.md` → "Karar — ek").

_Requirements: R9, R9.1, R9.2, R9.3_

## Değişiklikler

- **`crates/bt-core`** — kullanıcının uzak oturumunun **giriş yapıp
  yapmadığı** sınırdan okunabilir olmalı. Birincil işaret PTY'nin termios'u:
  ön planda ssh varken `ICANON` ve `ECHO` ikisi de kapalı = giriş yapıldı
  (macOS'ta master fd'nin `tcgetattr`'ı slave'in bayraklarını veriyor —
  ölçüldü: host anahtarı sorusu 1/1, parola sorusu 1/0, giriş 0/0). Master
  fd'nin kopyası zaten `Session::spawn`'da alınıyor; okumayı `Term` kilidinin
  dışında, ucuz ve yalnız sorulduğunda yap (kare yolunda değil). Yan
  işaretler (anında giriş): uzak oturumda `kullanıcı@host` biçimli başlık
  (`shell::title_directory`'nin şekli), uzak OSC 7, `CSI ? 2004 h`. Hepsi uzak
  oturum nesline bağlı ve `set_remote`/`clear_remote` ile sıfırlanır. Linux'ta
  da aynı (`make linux`).
- **`crates/bt-shell-common/src/ssh_route.rs`** — soket tabanının altında
  açılışta rastgele, örnek başına bir alt dizin (yol bütçesi yeniden
  hesaplanır, sınaması güncellenir); açılış süpürmesi yalnız sahibi ölmüş
  dizinleri siler (sahiplik: dizinde pid dosyası ya da dizin adı + `kill(pid,
  0)`). Kayıt, bu örneğin açtığı master'ları host anahtarıyla tutar ve
  `close(host)` / `close_all(deadline)` verir (`-O exit`).
- **`Ask::Never` çağıranları** (`remote_helper`, yük göstergesi —
  `bt-shell-macos/src/stats.rs`, `hyperlink.rs`) — rota kapısından önce
  "kullanıcı giriş yaptı mı" sorusu; yapmadıysa bağlanmaz, işaret gelince
  (yeni bir `Wake` ya da mevcut uzak oturum yoklamasının yolu) yeniden dener.
  Kullanıcının başlattığı işler (yükleme, indirme, önizleme, Finder'a
  sürükleme) beklemez.
- **`crates/bt-shell-macos/src/pane.rs`, `app.rs`** — pane'in uzak oturumu
  bitince (`clear_remote`'un kenarı) bu örnekte o host'ta başka uzak pane
  yoksa `close(host)`; ⌘Q'da `close_all` pane kapanışlarıyla paralel ve aynı
  `SHUTDOWN_GRACE` son tarihinde. Süreli koşuda kayıt boş, iş yok.
- **`CLAUDE.md`** — ssh dosya işlerinin parola cümlesine kural: bateri'nin
  bağlantısı kullanıcının oturumundan uzun yaşamaz; arka plan girişi bekler
  (işaretçi: `.tasks/047-ssh-parola-ve-keychain/discussion.md` → Karar — ek).

## Kabul

- Yerel `sshd` `#[ignore]` sınaması (phase-2/3'ün emsali): parola kayıtlıyken
  PTY'de `ssh` başlat; parola sorusu ekrandayken **master soketi yok**, giriş
  yapılınca arka plan işi bağlanıyor; ssh `exit` edince master kapanıyor.
- Saf sınamalar: termios + yan işaretlerin "giriş yapıldı" kararı (host
  anahtarı sorusu ve parola sorusu "hayır"), örnek başına dizin ve süpürmenin
  yaşayan örneğe dokunmaması, `close(host)`'un başka uzak pane varken
  koşmaması.
- Gözle kontrol (set sonu): bateri'yi kapat-aç, `ssh` yaz → gösterge parola
  girilmeden **gelmiyor**, girince geliyor; `exit` → `ps`'te bateri'nin
  master'ı yok; ⌘Q → yok.

## Checklist

- [ ] bt-core: giriş işareti (termios + yan işaretler), uzak nesle bağlı
- [ ] ssh_route: örnek başına soket dizini, güvenli süpürme, kayıt + `close`/`close_all`
- [ ] `Ask::Never` çağıranları girişi bekliyor; kullanıcı işleri beklemiyor
- [ ] Oturum sonunda ve ⌘Q'da master kapanıyor
- [ ] Test: parola sorusu ekrandayken master soketi yok (yerel sshd, `#[ignore]`)
- [ ] CLAUDE.md kuralı
- [ ] Doğrulama geçti (`make check`, `make linux`, `make test-race`, `make smoke`, `make bundle` gerekirse)
- [ ] Set kapısı yeniden: `/code-review` (phase-4'ün aralığı) + `/audit`
