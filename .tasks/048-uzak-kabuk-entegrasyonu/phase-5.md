# Phase 5 — Paylaşılan bağlantı

## Özet

Sarılmış etkileşimli oturum 047'nin soketinde master olsun; kullanıcı
parolayı terminalde bir kez yazar, dosya işleri pencere açmadan onun
üstünden geçer.

_Requirements: R7_

**Ön koşul:** 047 bitmiş olmalı — `ssh_route`, soket dizini ve `host_key`'e
dayalı soket adı (047 `discussion.md` → Muhakeme). Bu phase o düzeni
kullanır, ikinci bir düzen kurmaz.

## Değişiklikler

- **`crates/bt-shell-common/src/ssh_wrap.rs`** — `wrap` `-o
  ControlMaster=auto -o ControlPath=<047'nin yolu> -o ControlPersist=<047'nin
  süresi>` ekler; `ssh -G` kullanıcının kendi `controlmaster`/`controlpath`'ini
  gösteriyorsa eklemez. `unwrap` bunları da geri alır (gidiş-dönüş).
- **`crates/bt-shell-common/src/ssh_route.rs`** (047'nin) — kullanıcının
  oturumunun master'ı `Route::Ours` olarak tanınır; ömrü oturuma bağlı,
  oturum kapanınca 047'nin kendi master'ını açma yolu devralır.

## Kabul

- `wrap`/`unwrap` gidiş-dönüşü yeni seçeneklerle; kullanıcının
  `ControlMaster`'ı varken ek yok.
- Yerel `sshd` (`#[ignore]`, 047'nin altyapısı) ile: sarılmış oturum açıkken
  bir yükleme yeni bağlantı kurmadan geçiyor; oturum kapanınca sonraki iş
  047'nin yoluna düşüyor.
- `make check`, `make linux` yeşil.
- Gözle kontrol: parolalı sunucuya `ssh` (parola terminalde) → Finder'dan
  bırakılan dosya parola sayfası açmadan yükleniyor.

## Checklist

- [x] `wrap`'e ControlMaster seçenekleri, kullanıcı config'inde atlama
- [x] `ssh_route`'ta kullanıcının oturumunun tanınması
- [x] Test: gidiş-dönüş, yerel sshd senaryosu
- [x] Doğrulama geçti (`make check` + `make linux`)

## Uygulama Notları

- **047 phase-4 ile uzlaştırma: aynı soket değil, kardeş soket.** Plan
  kullanıcının oturumunu 047'nin soketinde (`<anahtar>`) ve 047'nin
  `ControlPersist`'iyle (600 sn) master yapıyordu; 047 phase-4 (Karar — ek)
  ondan sonra "bağlantı kullanıcının oturumundan uzun yaşamaz, oturum
  sonunda ve ⌘Q'da `-O exit`" dedi. İkisi birlikte uygulanamıyor: aynı
  sokette bateri'nin `-O exit`'i kullanıcının terminalini keser
  (`sync_ssh_session` uzak hedef her değiştiğinde `session_ended` çağırıyor —
  askıya alınan ssh'ın ardından gelen yerel prompt da), `-O exit` olmadan da
  master 600 sn yaşar. Seçilen: oturum aynı örnek dizininde **ayrı adlı**
  bir sokette master (`u-<anahtar>`, `ssh_route::session_socket`), adı bizim
  adlarımızdan biri değil — `close_all`, `session_ended`, süpürme ona
  `-O exit` göndermiyor; dizini temizlerken canlıysa dizin ve sahip dosyası
  kalıyor, ölüyse siliniyor (`remove_instance`). Ömür kısa bir
  `ControlPersist`'ten (`SESSION_PERSIST` = 2 sn, tasarım sabiti): master
  ayrılıyor, kullanıcının `exit`'i dönüyor, master üstündeki işler bitince
  2 sn'de kendiliğinden gidiyor. Ürün sonucu 047'ninki: bağlantı kullanıcının
  oturumu (ve onun üstündeki işler) bitince kapanıyor, ⌘Q'da pane'ler
  kapanınca da öyle.
- **Ölçüldü (Docker'da parolalı sshd):** `ControlPersist=no`'lu master'da
  kullanıcının `exit`'i üstüne binen 10 sn'lik akış bitene kadar 10 sn
  bekledi; `ControlPersist=2`'de 0 sn'de döndü, akış ayrılmış master'da
  bitti, soket ardından gitti. Persist'siz yol yük göstergesinin uzun ömürlü
  yardımcı oturumuyla kilitlenirdi (ssh çıkmaz → `D` gelmez → uzak durum
  bitmez → yardımcı kapanmaz).
- **İki fark, adıyla:** (a) kullanıcının sarılmış `ssh`'ı bateri'nin kendi
  `-M -N -f` master'ına **katılmıyor** (plan `auto` ile aynı sokete
  koyduğu için katılırdı) — katılsaydı onun `-O exit`'i kullanıcının
  oturumunu keserdi; bedeli: bateri'nin master'ı açıkken bile terminalde
  parola soruluyor. (b) Aynı bateri'de aynı host'a ikinci `ssh` ilkinin
  oturumuna katılıyor (`ControlMaster=auto` — planda vardı): parola sorulmuyor,
  ilk pane `exit` edince ikincisi sürüyor (master ayrılmış). Kullanıcının
  oturumuna binen bir aktarım oturum bitince **kesilmiyor**, bitiyor —
  bateri'nin master'ına binen ise 047'deki gibi kesiliyor.
- **Rota sırası:** bizim master → kullanıcının terminal oturumu
  (`Route::Ours(u-…)`) → kullanıcının kendi master'ı → bizimkini aç
  (`ssh_route::decide`, beş kol). Oturum soketi yalnız dosya varken
  sınanıyor; cevap vermeyen soket ssh'a kalıyor (`auto` onu siliyor).
- **`ssh-argv` başka bir süreç**: örnek dizinini bilmesi için uygulama
  `BATERI_SSH_INSTANCE`'ı `BATERI_BIN`'le birlikte veriyor
  (`app::with_bateri_bin`), sarmalayıcı onu ortamdan alıp `--instance` ile
  geçiriyor; alt komut dizini **yaratmıyor**, yalnız sahip dosyalı, özel bir
  dizin varsa kullanıyor (`ssh_route::instance_dirs`, ad 8 hex — yol
  enjeksiyonu yok). Dizin artık açılışta süpürme thread'inde kuruluyor
  (`Masters::sweep`), ilk dosya işinde değil. Anahtar 047'nin FNV'si
  (`ssh_route::host_key`), durum dosyasının üçlüsü değil.
- **Atlama:** `ssh -G`'de `controlmaster` `false`/`no` dışında ya da
  `controlpath` doluysa (`SshConfig::shares_connections`) seçenek yok; tek
  `-G` okuması `decide`'ınki. `unwrap` altı kelimeyi `-t`'nin hemen
  arkasında **konumla** alıyor; `jobs::ssh_target` yoluyla hedefin argv'sinde
  `ControlPath` kalmadığı sınanıyor (kalsaydı her iş `Route::Direct`'e
  düşer, R7 sessizce ölürdü).
- **Kabul sınaması:** `password_sshd_jobs_ride_the_users_session`
  (`#[ignore]`, 047'nin parolalı sshd'si `127.0.0.1:2222`): PTY'de sarılmış
  `ssh`, parola terminalde; kayıtlı parola yokken arka plan işi
  `BatchMode=yes` ile oturumun üstünden geçiyor (yeni giriş imkânsız —
  yani yeni bağlantı yok); `session_ended` + `close_all` oturuma
  dokunmuyor; binen 6 sn'lik akış varken `exit` 3 sn'den kısa sürede dönüyor,
  akış bitiyor, soket gidiyor. 047'nin iki sshd sınaması da yeşil.
- **Set kapısı:** `/code-review` (aralık `8e19ccc^`..çalışma ağacı) üç
  bulgu, üçü giderildi: (1) boşluklu ya da `%`'li ev dizini `-o
  ControlPath=`'i bozup **kullanıcının** sarılmış `ssh`'ını açılmaz
  kılardı → `ssh_route::fits` böyle bir tabanı atlıyor (`/tmp/bateri-$UID`
  devralıyor; bizim master'ın yolu da aynı kapıdan); (2) komut satırında
  açıkça seçilen paylaşım — `-o ControlMaster=no`, `-S none` dahil — `ssh
  -G`'de varsayılanla aynı görünüyordu ve bizim öndeki seçeneklerimiz onu
  ezerdi → argv'de `-S`/`ControlMaster`/`ControlPath`/`ControlPersist`
  varsa ekleme yok (`ssh_wrap::names_sharing`); (3) açılış süpürmesi
  canlı soket başına ssh koşarken örnek dizini henüz yoktu → dizin
  süpürmeden **önce** kuruluyor. **Waive:** `~/.ssh/config`'te açıkça
  yazılmış `ControlMaster no` / `ControlPath none` `ssh -G`'de varsayılanla
  bit bit aynı; ayırmak kullanıcının config'ini kendimiz ayrıştırmayı
  (`Include`, `Match`) ister — bilinen sınır, bedeli o host'ta oturumun
  bateri'nin örnek dizininde master olması (oturumla biter).
- **Gözle kontrol sahnesi (bütün set, kullanıcıda):** parolalı sunucuya
  `ssh` (parola terminalde) → dizin ve komut blokları uzakta da görünüyor,
  yük göstergesi geliyor; Finder'dan bırakılan dosya parola sayfası açmadan
  yükleniyor; `exit` hemen dönüyor ve `ps`'te `u-` soketli ssh birkaç saniye
  içinde yok; Settings ▸ Remote Files'ta onay kutusu ve Shell menüsünde
  host başına aç/kapa çalışıyor.
