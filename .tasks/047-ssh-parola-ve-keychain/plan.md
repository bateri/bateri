# ssh dosya işleri parolalı sunucuda — Plan

## Hedef

Parolayla giriş yapılan sunucuda uzak dosya işleri (yükleme, indirme,
önizleme, Finder'a sürükleme, bağlantı varlık kontrolü, yük göstergesi)
çalışsın: bateri parolayı kendi sayfasında bir kez sorar, host başına kendi
ssh master bağlantısını açar ve parolayı istenirse Keychain'de hatırlar.
Anahtarla ya da kullanıcının kendi ControlMaster'ıyla bugün çalışan hiçbir yol
gerilemez.

## Gereksinimler

- **R1 — Tek rota kapısı.** Her uzak iş başlamadan önce tek bir fonksiyondan
  geçer ve bağlantının yolunu oradan alır (`ssh_route::ensure`).
  - **R1.1** — Sıra: bizim master'ımız canlı → onun soketi; kullanıcının kendi
    master'ı canlı (`ssh -G`'nin `ControlPath`'i) → bugünkü argv; değilse
    bizim master'ı aç.
  - **R1.2** — `-o ControlPath` yalnız bizim rotada eklenir; kullanıcının
    config'indeki `ControlPath` hiçbir rotada ezilmez.
  - **R1.3** — Soket yolu kısa bir taban + kanonik (kullanıcı, host, port,
    jump) dörtlüsünün 16 haneli özeti; uzunluk çalışma anında unix soket
    sınırına karşı denetlenir ve en kötü uzunluğu bağlayan bir sınama var.
  - **R1.4** — Host başına tek açılış (single-flight); `-O check` "refused"
    diyen bayat soket silinir, açılışta dizin süpürülür.
- **R2 — Askpass.** Parola ssh'a `SSH_ASKPASS` + `SSH_ASKPASS_REQUIRE=force`
  ile ve aynı `bateri` binary'sinden verilir.
  - **R2.1** — Askpass kipi `main`'in ilk işi, `has_aqua_session()`'dan önce;
    AppKit'e uğramaz; bin'e yeni crate kenarı yok (`bt_shell_macos` üzerinden).
  - **R2.2** — İstem uygulamaya deneme başına açılan geçici bir unix soketle
    taşınır (`0700` dizin, rastgele ad, master açılışı bitince silinir);
    global dinleyici ve jeton tablosu yok.
  - **R2.3** — Askpass değişkenleri yalnız master'ın `Command`'ına verilir;
    kabuğun ortamına ve akış süreçlerine sızmaz.
  - **R2.4** — İstem sınıflaması saf: "parola" ve "öteki" (anahtar parolası,
    2FA). Cevapsız kalan soru (iptal, pane kapandı, ⌘Q) yardımcıyı sıfırdan
    farklı kodla çıkarır, boş cevap gönderilmez.
- **R3 — Master bağlantısı.** `-M -N -f`, `BatchMode=no`,
  `StrictHostKeyChecking=yes`, `ControlPersist` boşta 10 dk (tasarım sabiti);
  bateri kapanırken master'lara dokunulmaz. Akış süreçleri `BatchMode=yes` ve
  `ControlMaster=no` ile kalır, hiçbir zaman soru sormaz.
- **R4 — Parola sayfası.** İşi başlatan pane'in penceresinde sayfa: başlık
  host, alt metin işin adı, ssh'ın istem metni altta; Remember in Keychain
  kutusu yalnız parola isteminde ve varsayılan işaretli. Yanlış parolada sayfa
  "Wrong password — try again" ile yeniden açılır; Cancel işi "Cancelled" ile
  bitirir. Sayfa pane'in sayfa hakemliğinden geçer (açık bir onay sayfasının
  üstüne ikinci sayfa açılmaz).
- **R5 — Altı tüketici rotada.** Yükleme (yoklama + akış), indirme,
  önizleme, Finder'a sürükleme, `remote_helper` ve yük göstergesi argv'yi
  rotadan alır. Bilinmeyen host anahtarı hata satırı: "connect once in the
  terminal".
- **R6 — Keychain.** `kSecClassInternetPassword` (server, account, port,
  protocol SSH), etiket `bateri — user@host:port`; erişim yalnız uygulama
  sürecinde ve bir trait arkasında; data-protection keychain kullanılmaz.
  - **R6.1** — Kayıtlı parola sorusuz verilir; reddedilirse aynı denemenin
    ikinci sorusu Keychain'e gitmez, sayfa "The saved password didn't work"
    ile açılır ve başarılı yeni parola kaydın üstüne yazılır.
  - **R6.2** — Shell ▸ Forget Password for “{host}” (uzak sekmede etkin,
    kayıt yoksa gri) kaydı siler.
- **R7 — Arka plan işleri.** `remote_helper` ve yük göstergesi hiç sayfa
  açmaz; Keychain'de parola varsa `NumberOfPasswordPrompts=1` ile tek
  denemede açar, yoksa ya da parola reddedildiyse susar.
  - **R7.1** — Host başına bellekte "kayıtlı parola reddedildi" bayrağı; nesil
    ve `RETRY_AFTER` onu kaldırmaz, yalnız kullanıcının başlattığı başarılı
    giriş kaldırır.
  - **R7.2** — Parola olmadığı için susan arka plan işi ssh durum çubuğunda
    tıklanabilir **Sign In…** düğmesi gösterir; düğme parola sayfasını açar ve
    başarılı girişten sonra arka plan işleri yeni nesil beklemeden yeniden
    dener.
- **R8 — Belge ve lisans.** `objc2-security` bağımlılığı `CLAUDE.md`'nin
  bağımlılık ve katman satırlarında; `THIRD-PARTY-LICENSES.txt` yeniden
  üretilir; yeni ayar anahtarı doğarsa `docs/AYARLAR.md`.

- **R9 — bateri'nin bağlantısı kullanıcının oturumundan uzun yaşamaz**
  (kullanıcı bildirdi, 2026-10-02). Arka plan işleri kullanıcının ssh'ı
  giriş yapmadan bağlanmaz; master kullanıcının o host'taki son oturumu
  bitince ve uygulama kapanınca kapanır; başka bir bateri örneğinin
  bağlantısına dokunulmaz.
  - **R9.1** Giriş işareti: ön plandaki ssh'ın PTY'si kanonik ve yankısız
    değil (`ICANON` ve `ECHO` ikisi de kapalı; ölçüldü — host anahtarı
    sorusu 1/1, parola sorusu 1/0, giriş 0/0) ya da uzak başlık
    `kullanıcı@host` biçiminde, uzak OSC 7 ya da `?2004h` geldi. Yalnız
    `Ask::Never` çağıranlar (yük göstergesi, ⌘-hover) bekler; kullanıcının
    başlattığı işler beklemez.
  - **R9.2** Soket dizini örnek başına (açılışta rastgele alt dizin);
    açılış süpürmesi yaşayan örneğin dizinine dokunmaz.
  - **R9.3** Pane'in uzak oturumu bitince ve bu örnekte o host'ta başka uzak
    pane yoksa master `-O exit`; ⌘Q'da bu örneğin bütün master'ları, kapanış
    son tarihinin içinde.

## Yaklaşım

1. **Saf parçalar (`bt-shell-common`).** `ssh_route` modülü: rota tipi
   (`Ours(soket)` / `Direct`), soket yolu ve uzunluk denetimi, `ssh -G`
   okuması, `-O check` sonucu, rotaya göre argv (`upload::ssh_argv`'nin
   yanına), master açılış argv'si ve ortamı, istem sınıflaması, askpass
   istemcisinin gövdesi (sokete bağlan, istemi yaz, cevabı stdout'a). Gerçek
   süreç çağrıları bir dikişin arkasında; sahte `ssh` betiğiyle sınanır.
   Hiçbir tüketici henüz bağlanmaz.
2. **Master + askpass + sayfa (Keychain'siz).** `main`'in askpass dalı;
   `ensure`'ün master açan kolu (single-flight kaydı `PaneLaunch` ile geçer;
   bayat soket temizliği); deneme başına soketin uygulama tarafı (accept →
   pane'in ana kuyruğunda sayfa → kanal ile cevap); altı tüketici rotaya
   bağlanır. Arka plan işleri bu phase'de `Ask::Never` ile yalnız canlı
   master'ı kullanır, kendileri açmaz.
3. **Keychain + arka plan.** `objc2-security` (trait arkasında), Remember
   kutusu, bayat parola kolu, Forget Password menüsü, arka planın tek
   denemeli sessiz açılışı ve reddedildi bayrağı, Sign In… düğmesi, lisans
   dosyası ve belgeler.

## Kapsam Dışı

- Bilinmeyen host anahtarı için "Trust" sayfası (ret, Karar 7).
- Ayar penceresinde kayıtlı host listesi (yalnız menü, Karar 6).
- Anahtar parolasını ve 2FA kodunu kaydetmek.
- Kullanıcının terminal oturumunu master yapmak — 048'in son phase'i; soket
  düzeni bu setten.
- mosh'un kendi bağlantısı (mosh hedefinde dosya işleri yine ssh ile, aynı
  rota).

## Akış

```
iş (damla, ⌘-tık, indirme, sürükle, hover, gösterge)
  └─ ssh_route::ensure(target, ask)
       ├─ bizim soket canlı (-O check)        → Ours(soket)
       ├─ kullanıcının master'ı canlı (ssh -G) → Direct
       └─ aç: ssh -M -N -f  [single-flight, StrictHostKeyChecking=yes]
            SSH_ASKPASS=bateri  BATERI_ASKPASS=<geçici soket>
              └─ bateri (askpass kipi) ──soket──▶ uygulama
                   ask = Sheet  → Keychain? → yoksa pane sayfası → cevap
                   ask = Never  → Keychain (tek deneme, reddedildi bayrağı) → yoksa çık≠0
       → Ours(soket)
  └─ akış süreci: ssh -T BatchMode=yes ControlMaster=no [-o ControlPath=<soket> yalnız Ours]
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | ✅ |
| phase-3 | ✅ |
| phase-4 | |
| kapı | |
