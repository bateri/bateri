# Phase 2 — Master, askpass ve parola sayfası

## Özet

bateri'nin kendi master bağlantısını aç, askpass kipini `main`'e bağla,
istemi işi başlatan pane'in sayfasına taşı ve altı tüketiciyi rotaya geçir;
Keychain yok, parolalı sunucuda kullanıcının başlattığı işler artık çalışıyor.

_Requirements: R1.4, R2, R2.1, R2.2, R2.3, R3, R4, R5_

## Değişiklikler

- **`crates/bateri/src/main.rs`** — ilk iş: `BATERI_ASKPASS` varsa
  `bt_shell_macos` üzerinden yeniden ihraç edilen askpass girişine dön; stamp,
  `has_aqua_session()` ve AppKit hiç koşmaz. `crates/bateri/Cargo.toml`'a yeni
  kenar yok.
- **`crates/bt-shell-common/src/ssh_route.rs`** — `ensure`'ün açan kolu:
  - Host başına tek açılış: soket yoluyla anahtarlanan bir kayıt (mutex +
    bekleyen kuyruğu), sahibi uygulama, pane'e `PaneLaunch` ile geçer
    (`static` değil). İkinci istek birincinin sonucunu bekler.
  - Deneme başına geçici soket: `0700` dizinde rastgele ad, `bind` → master'ı
    `Command` ortamıyla başlat (yalnız o sürece) → `-f` dönene kadar `accept`
    döngüsü → istemi `ask` geri çağrısına ver → soketi sil. Cevapsız soru
    (geri çağrı `None`) yardımcıya "iptal" gider.
  - Bayat soket: `-O check` "refused" → dosya silinir; uygulama açılışında
    soket dizini süpürülür (yalnız bizim adlarımız).
  - `Ask` kipi: `Sheet(neden)` ve `Never`. Bu phase'de `Never` master açmaz,
    yalnız canlı master'ı kullanır (arka plan; phase-3 genişletir).
- **`crates/bt-shell-macos/src/uploader.rs`, `preview.rs`, `promise.rs`,
  `hyperlink.rs`, `stats.rs`** — argv'yi `ensure` + `ssh_argv_for`'dan al.
  `ensure` iş thread'inde koşar (ana thread'de bloklanan çağrı yok); kullanıcı
  işleri `Sheet`, `hyperlink` ve `stats` `Never`. Bilinmeyen host anahtarı ve
  açılamayan master kendi hata metniyle (`connect once in the terminal`;
  `upload::probe_failure`'ın bugünkü metni anahtarlı yol için kalır).
- **`crates/bt-shell-common/src/remote_helper.rs`, `remote_stats.rs`** — argv
  rotayı argüman alır; `RETRY_AFTER` ve nesil kuralı değişmez.
- **`crates/bt-shell-macos/src/pane.rs`** (+ gerekiyorsa yeni
  `password_sheet.rs`) — parola sayfası: `NSSecureTextField`, başlık host,
  alt metin işin adı, ssh'ın istem metni küçük puntoyla; yanlış parolada
  "Wrong password — try again". Sayfa `uploads().set_asking` hakemliğinden
  geçer; iş thread'i cevabı bir kanaldan bekler, pane kapanınca (`begin_close`)
  ve ⌘Q'da gönderici düşer. Remember kutusu bu phase'de **yok** (phase-3).
- **`crates/bt-shell-macos/src/app.rs`** — master kaydını kurar, `PaneLaunch`'a
  koyar; süreli koşuda (`BT_RUN_SECONDS`) kayıt ve askpass hiç kurulmaz.
  Kapanışta master'lara bir şey gönderilmez.
- **`objc2-app-kit`** özellik bayrağı `NSSecureTextField` (yeni crate değil;
  `Cargo.lock` değişmemeli — değişirse dur ve sor).

## Kabul

- `make check`, `make linux`, `make bundle` (bin değişti), `make smoke`
  (`main` değişti) yeşil.
- Sınamalar: sahte `ssh` betiği `SSH_ASKPASS`'i çağırıp master gibi davranır
  (soket yaratır, `-f` gibi döner) — askpass protokolü uçtan uca, iptalin
  sıfırdan farklı çıkışı, iki eşzamanlı `ensure`'ün tek açılışa inmesi, bayat
  soketin silinmesi, askpass değişkenlerinin akış argv'sinin ortamında
  olmaması. Gerçek master yaşam döngüsü: kullanıcı ayrıcalıklı, anahtarla
  giriş yapılan, yüksek portta koşan yerel `sshd` ile `#[ignore]` sınama
  (parola yolu gerçek sshd'siz sınanamaz — gözle kontrol, set sonu).
- Anahtarla giriş yapılan host'ta altı iş bugünkü gibi çalışıyor (rota
  `Ours` olsa da soru gelmiyor).

## Checklist

- [x] `main`'in askpass dalı
- [x] `ensure`'ün açan kolu: single-flight, geçici soket, bayat soket
- [x] Parola sayfası + sayfa hakemliği + kanal + kapanışta düşen gönderici
- [x] Altı tüketici rotada; arka plan `Never`
- [x] Master kaydı `app.rs`'te, `PaneLaunch` ile; süreli koşuda yok
- [x] Test: yukarıdaki kabul sınamaları
- [x] Doğrulama geçti (`make check`, `make linux`, `make bundle`, `make smoke`)

## Uygulama Notları

- **Rota helper'ın worker'ında çözülüyor, argv cevaba geri dönüyor.**
  İndirme, önizleme, Finder'a sürükleme, bağlantı doğrulama ve yük göstergesi
  aynı `remote_helper` oturumundan geçiyor ve ilk üçü akışın argv'sini o
  isteğin argv'sinden alıyordu. `Request.ssh` yerine `Dial` (kapıyı worker
  thread'inde, yalnız oturum açılacağı zaman koşturan bir kapanış + `user`
  biti) geldi ve `Reply` oturumun açıldığı argv'yi ikinci argüman olarak
  alıyor: soru ile akış aynı rotada. Yükleme kendi thread'inde kapıyı
  doğrudan çağırıyor. `remote_stats.rs` argv görmüyordu, dokunulmadı.
- **Kullanıcı işi `RETRY_AFTER` tutmasını ne okuyor ne yazıyor.** Okusaydı
  parolalı host'ta başarısız bir hover ⌘-tık önizlemeyi on saniye
  sorusuz reddederdi; yazsaydı iptal edilen sayfanın "Cancelled"ı hover
  etiketine düşerdi.
- **İptal sessiz bitiyor.** Planın "Cancel işi Cancelled ile bitirir"i
  yükleme/indirme/önizlemede hata sayfası açmadan bitmek olarak uygulandı
  (`ssh_route::CANCELLED`'ı tüketiciler tanıyor): yeni kapanan sayfanın
  üstüne "Cancelled" diyen ikinci bir sayfa gürültü. Finder'a sürüklemede
  söz Finder'a "Cancelled" hatasıyla bitiyor.
- **Cevapsız soruda önce ssh durduruluyor (SIGTERM), sonra yardımcıya
  `CANCEL` gidiyor.** ssh başarısız askpass'i boş parolaya çevirip sunucuya
  deniyor (`readpass.c`); sıra tersi olsaydı iptal sunucuda bir başarısız
  giriş daha sayardı. Bekçisi sahte ssh'ın `empty` kaydı (sıra bozulunca
  kırmızı düştüğü elle denendi).
- **ssh'ın stderr'i boruya değil özel dizindeki bir dosyaya**
  (`q-<rastgele>.err`, askpass soketinin yanında): `-f` ile arkaya geçen
  master tanımlayıcıyı miras alıyor ve borunun sonu hiç gelmeyebilirdi.
  `accept` döngüsü yoklamasız: ssh'ın ön yarısını bekleyen thread çıkınca
  askpass soketine bir kez bağlanıp döngüyü uyandırıyor.
- **Master'a `ConnectTimeout=15` (`ssh_route::CONNECT_TIMEOUT`, tasarım
  sabiti, helper'ın `OPEN_TIMEOUT`'u)** — planın R3 listesinde yoktu
  (`/code-review`): ulaşılamayan host işi ve arkasındaki helper worker'ını TCP
  zaman aşımı kadar tutuyordu. Bilinen sınır: sayfa açılmadan önce (bağlanırken)
  pane'i kapatmak ssh'ı durdurmuyor, bu süre kadar sürüyor; takılan bir
  `ProxyCommand`'ı süre bağlamıyor.
- **Uçuşa katılma kuralı** (`/code-review`): açılış sürerken gelen arka plan işi
  de bekliyor (beklemeseydi bugünkü argv'nin hatası `RETRY_AFTER` boyunca master
  açıldıktan sonra da hover'ı reddederdi); sahibi **iptal** edilen uçuşta
  kullanıcı işi kendi sayfasında soruyor (başka pane'in sayfasının iptali onun
  cevabı değil), arka plan işi bugünkü argv'ye düşüyor.
- **Sayfa hakemliği**: pencerede bağlı bir sayfa varsa (başka pane'inki,
  kapatma sorusu dahil) parola sayfası açılmıyor; geçidi zaten tutan iş
  (yüklemenin yoklaması, indirmenin sayımı) içinde açıyor, tutmayan
  (önizleme, sürükleme) boşsa sayfa ömrünce alıyor. Açılamayan sayfa bip +
  "cevapsız" → iş iptal. Gönderici pane'in `password` yuvasında;
  `begin_close` onu helper'ın `close`'undan **önce** düşürüyor (worker
  sayfayı bekliyorken `Close`'u işleyemez).
- **Sayfa metni**: başlık host, metin işin cümlesi ("Log in to upload the
  dropped items." …), yanlış parolada üstünde "Wrong password — try again.";
  ssh'ın istemi küçük puntoyla alanın üstünde; düğmeler "Log In" / "Cancel"
  (Esc).
- **Askpass'in uçtan uca sınaması test binary'sinin kendisiyle.** Sahte
  `ssh` (POSIX sh) `$SSH_ASKPASS`'i çağırıyor; o bir sarmalayıcı ve test
  binary'sini `ssh_route::tests::askpass_child --exact --ignored` ile
  koşturuyor, yani teldeki istemci gerçek `run_askpass`. Cevap dosyaya
  yazılıyor, çünkü harness'ın kendi başlığı stdout'ta.
- **Gerçek master yaşam döngüsü** `a_real_master_opens_carries_a_stream_and_is_reused`
  (`#[ignore]`): geçici dizinde kendi host anahtarı, istemci anahtarı ve
  `known_hosts`'u olan, yüksek portta kullanıcı ayrıcalıklı `/usr/sbin/sshd`;
  `-F /dev/null`, `~/.ssh` okunmuyor. Bu makinede yeşil koştu
  (`cargo test -p bt-shell-common real_master -- --ignored`).
- **`CLAUDE.md`'nin "BatchMode=yes: parola sorulamaz" cümlesi bu phase'de
  düzeltildi** (kodla çelişmesin diye tek cümle); Keychain ve bağımlılık
  satırları phase-3'te.
