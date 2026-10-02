# Phase 3 — Keychain ve arka plan işleri

## Özet

Parolayı Keychain'de hatırla (Remember kutusu, bayat parola, Forget Password),
arka plan işlerini kayıtlı parolayla tek denemede sessizce bağla ve parola
yokken ssh durum çubuğuna Sign In… düğmesini koy.

_Requirements: R6, R6.1, R6.2, R7, R7.1, R7.2, R8_

## Değişiklikler

- **`Cargo.toml` (workspace) + `crates/bt-shell-macos/Cargo.toml`** —
  `objc2-security = { version = "0.3.2", default-features = false, features =
  ["std", "SecItem"] }`; `objc2-core-foundation`'a gereken `CFData`/`CFString`
  bayrakları. `Cargo.lock`'a tek satır girmeli (karar: discussion → Karar 8,
  kullanıcı onayı 2026-10-02); fazlası çıkarsa dur ve sor.
- **`crates/bt-shell-common/src/ssh_route.rs`** — parola deposu trait'i
  (oku / yaz / sil; anahtar host, kullanıcı, port) ve "kayıtlı parola
  reddedildi" bayrağı (host başına, bellekte, master kaydında): bayrak yalnız
  `Sheet` ile başarılı girişte kalkar, nesil ve süre kaldırmaz. `Never` kipi
  artık master açar: bayrak kalkıksa ve depoda parola varsa
  `NumberOfPasswordPrompts=1` ile; ikinci soru gelirse cevap "iptal" ve bayrak
  kurulur. `Sheet`'te ilk soru depodan, aynı denemenin ikinci sorusu sayfaya
  ("The saved password didn't work").
- **`crates/bt-shell-macos/src/keychain.rs`** (yeni) — trait'in gerçek gövdesi:
  `kSecClassInternetPassword` (server, account, port, protocol SSH), etiket
  `bateri — user@host:port`; `kSecUseDataProtectionKeychain` yok. Erişim
  yalnız uygulama sürecinde; askpass süreci Keychain'e hiç dokunmaz.
- **Parola sayfası** — Remember in Keychain kutusu, yalnız `Prompt::Password`'da
  ve varsayılan işaretli; işaretliyse başarılı girişten **sonra** yazılır
  (yanlış parola kaydedilmez).
- **`crates/bt-shell-macos/src/menu.rs`** — Shell ▸ Forget Password for
  “{host}”, Mark “{host}” as ▸'nin yanında; uzak sekmede etkin, kayıt yoksa
  gri (`validateMenuItem:`). Seçici pane'de adlı bir yöntem.
- **ssh durum çubuğu (Sign In…)** — `remote_helper`/gösterge parola yokluğu
  ya da reddedildi bayrağı yüzünden sustuğunda pane bunu bilir; bağlam
  satırında 037 phase-6'nın düğme mekanizmasıyla (`bt_core::dock`'un
  `transfer_button_at` emsali, tek yerleşim çizim ve fare için) fiil etiketli
  **Sign In…** düğmesi. Düğme `Sheet` ile `ensure`'ü koşar; başarıda arka plan
  işleri yeni nesil beklemeden yeniden denenir. Aktarım satırı varken o
  kazanır. `bt-core`'a giren yalnız düğmenin durumu ve yerleşimi; Keychain ya
  da ssh görmez.
- **`assets/bundle/THIRD-PARTY-LICENSES.txt`** — `tools/third_party_notices.py`
  ile yeniden üret.
- **`CLAUDE.md`** — bağımlılık paragrafına `objc2-security` (kural + tek cümle
  gerekçe + işaretçi), `bt-shell-macos` satırının platform kütüphanesi
  sütununa; ssh paragrafına parola/master/Keychain'in tek cümlesi ve
  `BatchMode=yes` → "parola sorulamaz" cümlelerinin güncellenmesi.
- **`docs/AYARLAR.md`** — yalnız yeni anahtar doğduysa (planlanan yok).

## Kabul

- `make check`, `make linux`, `make bundle` (lisans dosyası pakete giriyor)
  yeşil; `make audit`'in `Cargo.lock` uyarısı karar kaydına bağlı.
- Sınamalar (sahte depo + sahte ssh): kayıtlı parola sorusuz veriliyor;
  reddedilince ikinci soru sayfaya gidiyor ve depo yalnız başarıda
  yazılıyor; arka planda reddedilen parola bayrağı kuruyor ve nesil/süre
  geçince yeniden denenmiyor; `Sheet` başarısı bayrağı kaldırıyor; Remember
  işaretsizken depo yazılmıyor; Forget kaydı siliyor. Gerçek Keychain
  sınaması `make check`'e girmez (giriş Keychain'ine yazar ve izin sorar).
- Düğmenin yerleşimi: çizim ve fare aynı yerleşimden (`bt-core` sınaması).
- Set sonu gözle kontrol sahnesi (devir mesajı): parolalı gerçek bir sshd'ye
  `ssh` ile bağlan, pencereye bir dosya bırak → parola sayfası, Remember
  işaretli; yükleme bitiyor; ⌘-tık ve yük göstergesi sorusuz çalışıyor;
  bateri'yi yeniden aç → sorusuz; Forget Password → durum çubuğunda Sign In….

## Checklist

- [ ] `objc2-security` bağımlılığı (tek `Cargo.lock` satırı)
- [ ] Parola deposu trait'i + reddedildi bayrağı + `Never`'ın açan kolu
- [ ] `keychain.rs` gerçek gövde
- [ ] Remember kutusu (varsayılan işaretli), bayat parola kolu
- [ ] Forget Password menüsü
- [ ] Sign In… düğmesi ve arka planın yeniden denemesi
- [ ] Lisans dosyası, `CLAUDE.md`, gerekiyorsa `docs/AYARLAR.md`
- [ ] Test: yukarıdaki kabul sınamaları
- [ ] Doğrulama geçti (`make check`, `make linux`, `make bundle`)
