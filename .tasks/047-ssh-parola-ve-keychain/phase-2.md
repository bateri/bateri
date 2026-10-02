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

- [ ] `main`'in askpass dalı
- [ ] `ensure`'ün açan kolu: single-flight, geçici soket, bayat soket
- [ ] Parola sayfası + sayfa hakemliği + kanal + kapanışta düşen gönderici
- [ ] Altı tüketici rotada; arka plan `Never`
- [ ] Master kaydı `app.rs`'te, `PaneLaunch` ile; süreli koşuda yok
- [ ] Test: yukarıdaki kabul sınamaları
- [ ] Doğrulama geçti (`make check`, `make linux`, `make bundle`, `make smoke`)
