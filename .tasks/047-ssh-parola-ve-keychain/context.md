# ssh dosya işleri parolalı sunucuda — Bağlam

## Mevcut Durum

Uzak oturumda bateri'nin kendi başlattığı **bütün** ssh süreçleri aynı
argv'den doğuyor: `upload::ssh_argv` (`crates/bt-shell-common/src/upload.rs`),
`ssh -T -o BatchMode=yes -o ControlMaster=no` + kullanıcının hedef argv'sinden
süzülen bağlantı seçenekleri. Tüketicileri:

| iş | modül | kim başlatıyor |
|---|---|---|
| Finder damlasının yüklemesi (yoklama + akış) | `upload::probe`/`transfer`, `bt-shell-macos::uploader` | kullanıcı (damla + onay) |
| sağ tık indirme | `download`, `uploader` | kullanıcı |
| ⌘-tık önizleme | `bt-shell-macos::preview` | kullanıcı |
| ⌘-sürükle Finder'a | `bt-shell-macos::promise` | kullanıcı |
| bağlantı varlık kontrolü (⌘-hover, sağ tık menüsü) | `remote_helper` (uzun ömürlü `ssh … sh`), `hyperlink` | **arka plan**, hover'dan tembel |
| uzak yük göstergesi | `remote_stats` + `remote_helper`, `bt-shell-macos::stats` | **arka plan**, kendiliğinden |

`BatchMode=yes` parolayı, anahtar parolasını ve host anahtarı sorusunu
**kapatıyor**: GUI'den doğan ssh'ın kontrol terminali yok, sorabilseydi
asılı kalırdı. Bu bilinçli bir sınırdı (037 → Seçenek B "Parola girişi
çalışmıyor"; 045 Karar 10; 046 → açılış hatası örneklemeyi durdurur).
`ControlMaster=no`: kullanıcının **açık** master bağlantısı varsa üstünden
geçiliyor, ama bizim ssh master olmuyor — olsaydı arka plana geçen master
akışın borusunun ucunu tutardı (`ssh_argv`'nin doc'u).

Sonuç olarak dosya işleri yalnız iki durumda çalışıyor: anahtar/ssh-agent ile
giriş ya da kullanıcının `~/.ssh/config`'inde `ControlMaster auto` +
`ControlPath` tanımlı ve etkileşimli oturumu master olmuş. macOS'un varsayılan
ssh ayarında ControlMaster yok.

Ortam: yerel ssh `OpenSSH_10.2p1` (macOS 26) — `SSH_ASKPASS_REQUIRE=force`
8.4'ten beri var, yani `DISPLAY`'siz ve tty'siz askpass çalışıyor.
`crates/bateri`'nin `main`'i bugün tek kip (uygulama); Keychain'e erişen hiçbir
crate grafta yok (`Cargo.lock`'ta `security` geçmiyor). objc2 ailesinin
`objc2-security` 0.3.2'si var, `SecItem` özelliği yalnız
`objc2-core-foundation`'ı (grafta) çekiyor.

## Motivasyon

Kullanıcıdan (2026-10-02): bir arkadaşın Fedora sunucusunda uzak oturumda
etiket çıktı; bu sohbette ssh dosya katmanı incelenince parolayla giren
herkeste yükleme, indirme, önizleme, ⌘-tık ve yük göstergesinin **hiç**
çalışmadığı görüldü. Hata metni ("Uploads need key-based login (ssh-agent) or
an open ControlMaster connection.") kullanıcıya ssh yapılandırmasını
öğretiyor; kullanıcının kararı: bunun yerine bateri parolayı kendisi sorsun,
host başına kendi master bağlantısını açsın ve parolayı Keychain'de
hatırlasın.

Rakip emsali: kitty'nin ssh kitten'ı kendi askpass'ini (`askpass native`) ve
paylaşılan bağlantıyı (`share_connections`, ControlMaster, kitty çıkınca
temizlenir) kuruyor (https://sw.kovidgoyal.net/kitty/kittens/ssh/). Aynı
sohbette iTerm2'nin CVE-2025-22275'i (ssh entegrasyonunun uzakta
`/tmp/framer.txt`'e girdi/çıktı yazması) kayıt dışı hiçbir iz bırakmama
kuralının emsali olarak not edildi.

İlişkili set: 048 (uzak kabuk entegrasyonu) kullanıcının **kendi** ssh'ını
sarınca onun oturumunu master yapabilir; bu set onun kapsamadığı her durumun
(betikten ssh, mosh, entegrasyonu kapalı host) yolu. Kesişim
`discussion.md` → Karar 9.
