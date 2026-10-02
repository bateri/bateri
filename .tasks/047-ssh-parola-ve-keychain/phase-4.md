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

- [x] bt-core: giriş işareti (termios + yan işaretler), uzak nesle bağlı
- [x] ssh_route: örnek başına soket dizini, güvenli süpürme, kayıt + `close`/`close_all`
- [x] `Ask::Never` çağıranları girişi bekliyor; kullanıcı işleri beklemiyor
- [x] Oturum sonunda ve ⌘Q'da master kapanıyor
- [x] Test: parola sorusu ekrandayken master soketi yok (yerel sshd, `#[ignore]`)
- [x] CLAUDE.md kuralı
- [x] Doğrulama geçti (`make check`, `make linux`, `make test-race`, `make smoke`, `make bundle` gerekirse)
- [x] Set kapısı yeniden: `/code-review` (phase-4'ün aralığı) + `/audit`

## Uygulama Notları

- **Master'ın kopyası yoktu; `tcgetattr` bir kat yukarıda.** Planın
  "kopya zaten `Session::spawn`'da alınıyor" cümlesi yanlıştı — `try_clone`
  yalnız borç olarak doc'taydı. `bt-core`'a `libc` kenarı `Cargo.lock`'u
  değiştirirdi (eskalasyon şartı), o yüzden kopya std ile alınıyor
  (`pty.file().try_clone()`, başarısızsa `None` — işaret çıktıdan gelir),
  `Session::with_pty_fd` onu `BorrowedFd` olarak veriyor ve sistem çağrısı
  `bt-shell-common`'da (`jobs::tty_modes`). Karar `bt-core`'da saf:
  `TtyModes::logged_in` ve `Session::remote_login(modes)` — modlar yalnız
  çıktı henüz söylemediyse okunuyor, evet uzak nesil için önbellekte
  (`ShellLog::login`). Linux'ta master'ın `tcgetattr`'ı slave'inkini
  veriyor: `jobs::tests::the_master_reads_the_programs_terminal_modes`
  `make linux`'ta yeşil. Kopya `begin_shutdown`'da `Pty`'nin `Drop`'undan
  önce kapanıyor (`Session::with_pty_fd`, yaprak kilit), yani slave'in
  hangup'ı eskisi gibi geliyor (`make smoke` → `teardown=clean`).
  Önbellekteki evet tek kilit turu (başlık ancak önbellek boşsa okunuyor).
- **`?2004h` tarayıcının ikinci CSI dizisi** (`CsiScan::private`, yalnız
  baştaki `?`) ve sayaç değil **akış sırasında bir olay**
  (`ScanEvent::PasteOn`): aynı okumada prompt'un `?2004h`'si ile `C`
  birlikte gelebiliyor (⌘T'nin ilk girdisi, typeahead). Ölçüt `C`'den beri
  değil **uzak durum kurulduktan beri** (`ShellLog::paste_since_remote`;
  `C` ve yeni hedef siliyor): `ssh $(fzf)`'in fzf'i `C`'den sonra
  `?2004h` basar. Başlık işareti de aynı ölçüte bağlı: `set_remote`
  `title_epoch`'u `Term` kilidi altında okuyup saklıyor (`title_at_remote`),
  yani `C` ile aynı okumada gelen yerel başlık ayrıştırılmış sayılıyor.
  Uzak durumdan önce gelen gerçek uzak prompt'u modlar yakalıyor. Mevcut mod
  değil kenar okunuyor, çünkü yanlış pozitif kullanıcının bildirdiği hatanın
  ta kendisi olurdu. Uzak OSC 7 `remote_cwd`. `CLAUDE.md`'nin "bir CSI
  dizisi" cümlesi "iki" oldu. (Üç yarış `/code-review`'dan.)
- **Tetik çıktı kenarı, saat yok.** Pane'e ikinci bir `RemoteProbe`
  (`ShellWake::login_probe`): uzak kenarda giriş yoksa kuruluyor, her
  çıktı tek iş atıyor (`TerminalPane::login_check`), giriş görülünce
  `sync_stats_generation` göstergeyi başlatıyor. Bağlanma kapısı iki
  çağıranın başında: yük göstergesi girişten önce nesli hiç almıyor,
  ⌘-hover/sağ tık `Check::Gone` (eksik önbelleğine yazılmıyor — girişten
  sonraki ilk hover soruyor). `TerminalPane::dial(target, None)`'a
  konmadı: oradan dönen hata helper'ın `RETRY_AFTER` tutmasına ve hover
  etiketine düşerdi. **Bilinen sınır:** girişten sonra hiç bayt basmayan
  bir uzak taraf, ilk baytına kadar göstergesiz kalır.
- **Örnek dizini** `<kök>/<8 hex>`; sahibi dizindeki `pid` dosyası. Dizin
  gizli bir adla doğuyor, `pid` yazılıyor, sonra yerine taşınıyor
  (`prepare_instance`) — başka örneğin süpürmesi sahipsiz yarım dizin
  görmesin. Süpürme yalnız sahibi ölmüş (`kill(pid, 0)` başarısız) dizine
  dokunuyor: içindeki canlı master'ı `-O exit`'liyor, bizim adlarımızı
  siliyor, dizini kaldırıyor; sahipsiz dizin (pid dosyası yok) hiç
  silinmiyor (yanlışın yönü güvenli). Kökteki eski düz soketler (phase-2/3
  düzeni) bugünkü kuralla. Yol bütçesi 9 bayt daraldı: macOS'ta ev
  dizininin `/Users/` + 29 karakterine kadar önbellek dizini, ötesi
  `/tmp/bateri-$UID`. **Bilinen sınır:** sahibinin pid'i yeniden kullanılmış
  ölü bir örneğin dizini kalır (sızıntı; içindeki master `ControlPersist`
  ile biter).
- **`-O exit` kullanıcının config'ini okumuyor** (`exit_argv`: `-F
  /dev/null`, `BatchMode=yes`, yer tutucu host) — kontrol komutu soketten
  gidiyor; gerçek sshd'de ölçüldü (anahtarlı sınama). Kayıt pane kimliğiyle
  (`Masters::session_started`/`session_ended`; bitiş kenarında
  `remote_target` zaten silinmiş): kapatma yalnız başka bir pane'in oturumu
  aynı sokete (ya da aynı argv'ye) çözülmüyorsa, kendi thread'inde.
  Oturum başlarken soketi bilinmiyorsa kendi thread'inde `ssh -G` ile
  öğreniliyor; hâlâ bilinmeyen başka bir oturum (bir takma ad olabilir)
  master'ı **tutuyor** — yanlışın yönü canlı oturumun master'ını kesmek
  olurdu, bedeli `ControlPersist`'e kalan bir master. Pane kapanışı da
  (`begin_close`) bir oturum bitişi. ⌘Q: `begin_quit` pane kapanışlarının
  kendi `exit`'ini susturuyor, `close_all` pane beklemesiyle paralel ve
  aynı son tarihte; o sırada açılmakta olan master açılır açılmaz kendini
  bitiriyor (`open_flight`) ve dizin sahip dosyasıyla kalıyor; yetişmeyen
  master'ın dizini de öyle — sonraki açılışın süpürmesi bitirsin.
- **Kapı kopyalanmadı mı?** (`/code-review` yükseklik bulgusu, waive):
  "girişten önce bağlanma" kapısı `TerminalPane::dial(target, None)`'da
  değil iki arka plan çağıranında. Dial'dan dönen hata helper'ın
  `RETRY_AFTER` tutmasına ve hover etiketine düşerdi; arka planın bugünkü
  iki girişi (yük göstergesinin nesli, bağlantının `check`'i) kapıda ve
  Sign In…'in yeniden denemesi de onlardan geçiyor.
- **Kabul sınaması iki parça:** anahtarlı yerel sshd sınaması
  (`a_real_master_opens_carries_a_stream_and_is_reused`) artık
  `session_ended` ve `close_all`'ı gerçek master'da sınıyor; parola sorusu
  için Docker'daki parolalı sshd (`127.0.0.1:2222`, `deneme`) —
  `password_sshd_background_waits_for_the_users_login`, `#[ignore]`, sunucu
  yoksa SKIPPED: PTY'de gerçek ssh, parola sorusu ekrandayken (modlar 1/0)
  `remote_login` hayır ve soket yok; parola yazılınca evet ve arka plan
  kayıtlı parolayla bizim master'ı açıyor; `exit` + `session_ended` → master
  yok. İkisi de bu makinede yeşil.
- **Set kapısı:** `/code-review` 10 bulgu — 9'u giderildi (üç sıralama
  yarışı, takma adlı pane'in master'ı, ⌘Q'da açılmakta olan master, kopyanın
  kapanış anı, önbellekten önce başlık okuması, ⌘Q'da çift `exit`, yardımcı
  tekrarları), biri waive (yukarıda); `/audit` temiz. `make linux`'ta bir
  koşuda `session::tests::zero_size_is_ignored` kırmızı düştü, yeniden
  koşuda yeşil (değişmeyen bir sınama, zamanlamaya bağlı).
- **Gözle kontrol sahnesi (set sonu, kullanıcıda):** bateri'yi kapat-aç,
  parolalı sunucuya `ssh` yaz → parola girilmeden gösterge **yok**, girince
  geliyor; `exit` → `ps`'te bateri'nin master'ı yok; yeniden bağlan, bir
  dosya bırak, ⌘Q → yok.
