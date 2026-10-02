# Phase 1 — Rota ve askpass'in saf parçaları

## Özet

`bt-shell-common`'a `ssh_route` modülünü ekle: rota kararı, soket yolu, argv,
istem sınıflaması ve askpass istemcisinin gövdesi; hiçbir tüketici henüz
bağlanmaz, davranış bugünküyle aynı kalır.

_Requirements: R1, R1.1, R1.2, R1.3, R2.4_

## Değişiklikler

- **`crates/bt-shell-common/src/ssh_route.rs`** (yeni) —
  - `Route` (`Ours(PathBuf)` / `Direct`) ve `ensure`'ün **karar** yarısı:
    girdisi iki yoklamanın sonucu (bizim soket `-O check`, kullanıcının
    `ControlPath`'i `-O check`), çıktısı rota ya da "master aç". Süreç
    çağrıları bir dikişin (trait ya da fonksiyon argümanı) arkasında; gerçek
    gövdesi `ssh -G` + `ssh -O check` koşturur.
  - Soket yolu: kısa taban (`~/Library/Caches/bateri/s/` ya da daha kısası;
    `0700`, sahibi denetlenir) + `ssh -G`'nin `user`, `hostname`, `port`,
    `proxyjump` dörtlüsünün 16 haneli özeti. Yol ssh'ın geçici son eki dahil
    `sun_path` sınırını aşıyorsa `/tmp/bateri-$UID/` yedeği (`0700`, sahip
    denetimi); o da aşarsa rota `Direct`'e düşer (bugünkü davranış).
    Özetin algoritması tek yerde; 048 aynı fonksiyonu kullanacak (discussion
    → Karar 9).
  - Master açılış argv'si: `-M -N -f -o ControlPath=… -o ControlPersist=600
    -o BatchMode=no -o StrictHostKeyChecking=yes` + `upload::ssh_argv`'nin
    bağlantı seçenekleri (aynı süzgeç, ikinci bir ayrıştırıcı yok);
    `ControlPersist` süresi adlı bir tasarım sabiti. Arka plan kipinde ek
    `NumberOfPasswordPrompts=1`.
  - Askpass ortamı: `SSH_ASKPASS`, `SSH_ASKPASS_REQUIRE=force`,
    `BATERI_ASKPASS=<soket yolu>`; ortam bir değer olarak döner, kimsenin
    `set_var`'ı yok.
  - İstem sınıflaması: `Prompt::Password` (`…'s password:`, `Password:`) /
    `Prompt::Other` (anahtar parolası, `Verification code:`, kalan her şey).
  - Askpass istemcisi gövdesi: `BATERI_ASKPASS` soketine bağlan, istemi yaz,
    cevabı oku, stdout'a yaz; bağlantı kopar ya da cevap "iptal" ise sıfırdan
    farklı çıkış. Kablo biçimi tek satırlık başlık + uzunluklu gövde; tek
    sahibi bu modül.
- **`crates/bt-shell-common/src/upload.rs`** — `ssh_argv` rotayı argüman
  olarak alan bir kardeş kazanır (`ssh_argv_for(target, &Route)`):
  `Ours`'ta bizim seçeneklerin arkasına `-o ControlPath=…` eklenir, `Direct`'te
  bugünkü argv bayt bayt aynı. Bugünkü `ssh_argv` imzası tüketiciler phase-2'de
  geçene kadar kalır.
- **`crates/bt-shell-common/src/lib.rs`** — modül kaydı ve başlık yorumu.

## Kabul

- `make check` yeşil; bugünkü tüketicilerin argv'si değişmedi (mevcut
  `upload` sınamaları dokunulmadan geçiyor).
- Sınamalar: rota kararının dört kolu; `Direct` argv'sinin bugünkünün aynısı
  olması; `Ours`'ta `ControlPath`'in **arkada** olması ve kullanıcının
  `-o ControlPath`'ini ezmemesi; en uzun kullanıcı adıyla yol bütçesi ve
  yedeğe düşüş; istem sınıflamasının örnek istemleri (Türkçe PAM metni dahil
  `Other`'a düşmeyen parola istemi); askpass istemcisinin sahte bir soket
  sunucusuna karşı gidiş-dönüşü ve kopan bağlantıda sıfırdan farklı çıkışı.
- `make linux` yeşil (`bt-shell-common` Linux'ta derleniyor).

## Checklist

- [x] `ssh_route` modülü: rota, soket yolu + yedek, master argv'si, ortam,
      istem sınıflaması, askpass istemcisi
- [x] `upload::ssh_argv_for`
- [x] Test: yukarıdaki kabul sınamaları
- [x] Doğrulama geçti (`make check`, `make linux`)

## Uygulama Notları

- **Kullanıcının elle yazdığı soket rotayı `Direct`'e kilitliyor.** Hedefin
  argv'si `-S` ya da `-o ControlPath=` taşıyorsa `ssh_route::plan` hiçbir
  yoklama koşmadan `Route::Direct` dönüyor. Sebep sıra: akışın
  `ControlPath`'i kullanıcının seçeneklerinin **arkasında** (ssh ilk değeri
  alır, kullanıcınınki ezilmesin diye) ve o hedefte bizim soketimiz hiç
  kullanılamazdı — master açılır, akış yine kullanıcının ölü soketine
  giderdi. Bedel: o hedef bugünkü gibi davranıyor. Config dosyasındaki
  `ControlPath` bu kolun dışında (komut satırı onu yener, bizim rotada bizimki
  kullanılır).
- **Master argv'sinde bizim seçenekler önde, akışta `ControlPath` arkada.**
  Master bizim ve başka bir şeye dönüşmemeli (`-M`, `ControlPath`,
  `ControlPersist`, `BatchMode=no`, `StrictHostKeyChecking=yes` ilk değer
  olarak kazanıyor); akışın ise kullanıcının yazdığını ezmemesi gerekiyordu.
- **Hedefin argv ayrıştırıcısı tek**: `upload::connection` (program,
  korunan seçenekler, hedef) — `ssh_argv`, `ssh_argv_for`, `ssh -G`,
  `-O check` ve master argv'si hepsi ondan; `ssh_argv` artık
  `ssh_argv_for(target, &Route::Direct)`.
- **Bayat soketin silinmesi bu phase'de**, `plan`'ın içinde ("refused" →
  `remove_file`); açılıştaki dizin süpürmesi phase-2'de kalıyor.
- **Soket tabanı Linux'ta `~/.cache/bateri/s`** (macOS'ta
  `~/Library/Caches/bateri/s`), yedeği ikisinde de `/tmp/bateri-$UID`.
  Sınır `sun_path`'in boyu `libc::sockaddr_un`'dan (`size_of − offset_of`,
  macOS 104 / Linux 108), ssh'ın geçici son eki (`.` + 16) dahil. Özet
  FNV-1a 64 (sürümden bağımsız, sınamada sabitlenmiş değer); macOS'ta ev
  dizini 45 bayta kadar önbellek tabanına sığıyor, ötesi yedeğe düşüyor.
- **Bizim master canlıyken kullanıcının `-O check`'i koşmuyor** (cevap rotayı
  değiştirmez); `ssh -G`'nin `ControlPath`'i yalnız "var mı" diye okunuyor,
  canlılığını ssh'ın kendisi (`-O check` hedefin argv'siyle) söylüyor.
