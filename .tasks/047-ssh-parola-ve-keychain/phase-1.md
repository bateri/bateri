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

- [ ] `ssh_route` modülü: rota, soket yolu + yedek, master argv'si, ortam,
      istem sınıflaması, askpass istemcisi
- [ ] `upload::ssh_argv_for`
- [ ] Test: yukarıdaki kabul sınamaları
- [ ] Doğrulama geçti (`make check`, `make linux`)
