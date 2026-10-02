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

- [x] `objc2-security` bağımlılığı (tek `Cargo.lock` satırı)
- [x] Parola deposu trait'i + reddedildi bayrağı + `Never`'ın açan kolu
- [x] `keychain.rs` gerçek gövde
- [x] Remember kutusu (varsayılan işaretli), bayat parola kolu
- [x] Forget Password menüsü
- [x] Sign In… düğmesi ve arka planın yeniden denemesi
- [x] Lisans dosyası, `CLAUDE.md`, gerekiyorsa `docs/AYARLAR.md`
- [x] Test: yukarıdaki kabul sınamaları
- [x] Doğrulama geçti (`make check`, `make linux`, `make bundle`)

## Uygulama Notları

- **Parolasız arka plan işi bugünkü argv'de kalıyor; Sign In… sinyali reddin
  kendisinden.** `Ask::Never` yalnız kayıtlı ve reddedilmemiş parolayla master
  açıyor (planın "yoksa susar"ı); parola yoksa rota `Direct` ve sunucu girişi
  reddedince helper'ın açılış hatası — yalnız masters'lı arka plan dial'ında
  (`Dial::sign_in`) — `ssh_route::SIGN_IN_NEEDED`'e çevriliyor
  (`ssh_route::login_refused`, "Permission denied"). Reddedilmiş hesap ve
  kayıtlı parolanın reddi doğrudan `Denied::SignIn`. Pane metni iki yerde
  tanıyor (yük göstergesinin `Unreachable`'ı, bağlantı doğrulamanın hatası:
  `TerminalPane::background_failed`); ulaşılamayan host bu metni üretmiyor.
  Kayıtlı parolayla denenen arka plan açılışı başka bir sebeple (ağ) düşerse
  rota `Direct`'e dönüyor ki hata bugünkü sözlerle gelsin.
- **Reddedildi bayrağı** `Masters`'ta hesap başına (`Account`: `ssh -G`'nin
  host/user/port'u); kullanıcının başarılı girişi ve Forget kaldırıyor. Bayrak
  kullanıcının kendi işini durdurmuyor; kullanıcı bayat parolanın sayfasını
  iptal ederse de kuruluyor (parola reddedildi, bu bilgi).
- **İşaretsiz kutu + reddedilmiş kayıt → kayıt siliniyor**: bilinen yanlış
  parolanın arka planda bir kez daha denenmesini önlüyor. Kutu işaretliyse
  yeni parola üstüne yazılıyor (`SecItemAdd` → `errSecDuplicateItem` →
  `SecItemUpdate`). Yazma yalnız master açıldıktan sonra.
- **Forget Password master'ı da `-O stop`'luyor ve pane'in helper'ını
  kapatıyor.** Yalnız kaydı silmek kabul sahnesini (Forget → Sign In…)
  `ControlPersist` (10 dk) boyunca imkânsız kılardı: arka plan canlı master'a
  binmeye devam ederdi. `stop` yeni işi kabul etmiyor ama üstündeki aktarımı
  kesmiyor (`exit`'in tersine); kullanıcının master'ına (`Direct` rota,
  `-S`/`ControlPath` yazan hedef) hiç dokunmuyor. 048 aynı soketi paylaşınca
  bu kural yeniden tartılmalı.
- **Forget'ın etkinliği** bir işin çözdüğü hesaptan (`Masters::has_saved`,
  argv başına önbellek): ana thread `ssh -G` başlatmıyor, yani uygulama
  açıldıktan sonra ilk uzak işe kadar gri. Keychain'e yalnız öznitelik sorusu
  (onay penceresi yok). Başlık `menuWillOpen:`'da key pencerenin host'undan.
- **Sign In… düğmesinin yeri yük göstergesinin yeri** (`stats_layout`'un ilk
  kolu): parola yokken örnek de yok; düğme yoldan önce geliyor, yol soldan
  kısalıyor; sığmazsa düşüyor. Çizim yükleme düğmeleriyle aynı (`DockButton`,
  etiket ön plan renginde, dolgu işaretin renginde, hover'da koyulaşıyor, el
  imleci); tıklama ve el imleci `Session::sign_in_span`'den. Hover yükleme
  hover'ının hunisinden (`upload_hover`) geçiyor.
- **`DockContext::clone_from` alanı elle kopyalıyor**: yeni `sign_in` alanı
  oraya da eklendi; eksikken kare yolu ve fare düğmeyi görmüyordu (Session
  sınaması yakaladı).
- **Başarılı her kullanıcı dial'ı bir giriş**: `TerminalPane::dial` kullanıcı
  işinin argv kapanışını sarıyor ve `Ok`'ta pane'e `signed_in` gönderiyor
  (düğme gider, helper tuttuğu hatayı unutur — `RemoteHelper::retry`, yük
  göstergesi aynı nesilde yeniden başlar — `Schedule::retry`). `ssh_route`'a
  kanca eklenmedi.
- **Keychain'in hata kodları modülde adlı** (`errSecSuccess`,
  `errSecDuplicateItem`): `SecBase` başlığını üç sabit için açmamak için;
  `objc2-core-foundation`'a `bt-shell-macos`'ta `CFData`/`CFNumber`
  bayrakları ve bir kenar. `Cargo.lock`'a giren tek paket `objc2-security`.
- **Anahtar parolası ve 2FA sayfasında kutu yok**; Karar 7'nin "kutu yerine
  ipucu satırı" önerisi yapılmadı (ssh'ın istem metni zaten sayfada).
- `preview_failed`'ın gövdesi `failure_sheet(title, text)` oldu; Sign In'in
  hatası ("Can't sign in to {host}") onu kullanıyor.
- **Set kapısı `/code-review` (10 bulgu, 9'u giderildi):** reddedilmiş
  hesabın kayıtlı parolası kullanıcı işinde de artık gönderilmiyor (sayfa ilk
  soruda "The saved password didn't work" diyor — bilinen yanlış parolayla
  ikinci başarısız giriş yok); arka plan denemesi kayıtlı paroladan sonra
  ikinci faktörde/anahtar parolasında takılırsa da hesap işaretleniyor (tek
  yarım giriş); Sign In… dönüşümü yalnız sunucu `password`/`keyboard-interactive`
  listeliyorsa (`ssh_route::password_refused`; anahtar-yalnız ret ssh'ın
  sebebini koruyor); düğmenin tek sahibi oturum (`Session::sign_in`, pane
  kopya tutmuyor — geç gelen başka nesil cevabı ve host değişimi
  ayrışamıyor); düğme **arka planın başarısıyla** kalkıyor (örnek ya da
  bağlantı doğrulama — başka pane'in girişi, `ssh-add`, kullanıcının
  master'ı), kullanıcı işinin başarısı ve `Direct` dönen Sign In yalnız
  yeniden denetiyor; uçuşa katılan arka plan işi Keychain'i okumuyor;
  menünün sorusu `Masters`'ın "kayıtlı mı" önbelleğinden (ana thread
  Keychain'i beklemiyor; ilk soru bir kez öznitelikle); `sign_in_span`
  düğme yokken kopyalamıyor; `-O check`/`-O stop` tek `control_argv`.
  **Kalan sınır:** başka pane'de görünen düğme, o pane'in arka planı yeniden
  denenene kadar (⌘-hover, yük göstergesinin bir sonraki nesli) duruyor; ona
  tık sayfa açmadan canlı master'ı bulup kalkıyor. Pane'ler arası yayın
  `AppDelegate`'e uzanmayı isterdi (pane modülünün sınırı).
- **Gözle kontrol sahnesi (set sonu, kullanıcıda)**: parolalı gerçek bir sshd'ye
  `ssh` ile bağlan, pencereye bir dosya bırak → parola sayfası, Remember
  işaretli; yükleme bitiyor; ⌘-tık ve yük göstergesi sorusuz; bateri'yi yeniden
  aç → sorusuz (Keychain); Shell ▸ Forget Password for “host” → durum
  çubuğunda Sign In…; düğme → sayfa → giriş → gösterge geri geliyor. Ad-hoc
  imzada macOS'un "confidential information" sorusu her derlemede (Karar 6).

