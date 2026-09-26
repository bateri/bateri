# ssh modunun ikinci turu — Bağlam

## Mevcut Durum

036 ssh/mosh'u tanıyor ve uzak oturumu gösteriyor; bugünkü hâli
`CLAUDE.md`'de (dock'un "ssh'ta dock bir durum çubuğuna iniyor" paragrafı,
`bt-shell` satırındaki "uzak oturumun algılanması") ve gerekçeleri
`.tasks/036-ssh-uzak-oturum/discussion.md` → Karar 1–8'de. Bu setin
dokunduğu dört yerin bugünkü hâli:

- **Renk tek.** Uzak oturumun bütün göstergesi `info` rolünde:
  `dock::render_remote_context`'te `⇄ host`, `dock.rs`'te üst saç çizgisi
  (`context.remote.is_some()` → `theme.info_linear()`). Host'u ayıran bir
  bilgi yok; `prod` ile laptop'taki bir VM aynı renkte. Sekmede renk yok
  (036 Karar 5: `NSWindowTab`'ın başlığını ikinci kez yazmamak için).
- **Yoklama argv'yi okuyor ama saklamıyor.** `jobs::remote`
  (`bt-shell/src/jobs.rs`) ön plan grubunun en üstteki ssh/mosh sürecinin
  argv'sinden yalnız host'u çıkarıyor (`Probe::Remote(String)`); argv atılıyor.
  `Session::set_remote(nesil, host)` `bt-core`'da yalnız host'u
  (`DockContext::remote`) tutuyor ve `C`/`D`/`A`'da siliyor.
- **Yeni sekme yalnız yerel dizini miras alıyor.** `AppDelegate::open_window`
  (`bt-shell/src/app.rs`) pencere doğuran tek yol (⌘N, ⌘T, sekme çubuğunun
  `+`'sı): kabuk etkin sekmenin OSC 7 dizininde doğuyor (026 Karar 4). ssh
  sekmesinde ⌘T yerel bir kabuk açıyor.
- **Finder damlası her zaman yerel yolu yapıştırıyor.** `BateriView`'ın
  `performDragOperation:`'ı (`view.rs`) yolları `quote::shell_quote`'tan
  geçirip `Session::paste`'e veriyor; `draggingEntered:` koşulsuz `Copy`
  diyor (eleme kayıtta). ssh ön plandayken yapıştırılan şey uzak kabuğun
  satırına düşen **yerel** bir yol — uzak makinede anlamsız.
- **Kopan bağlantı sessiz.** ssh bitince `D` gelip uzak durum siliniyor, dock
  bir sonraki prompt'ta yerel hâline dönüyor. Çıkış kodu `D`'de zaten var
  (`Mark::CommandEnd { exit, .. }`, `ShellState::last_exit`) ama kimse
  ssh'ın kodunu ayrıca okumuyor.

## Motivasyon

**Kullanıcı isteği (2026-09-26, 036'nın sohbeti):** dört madde tek sette —
host'a göre renk, ssh'ta Finder damlası, ⌘T'nin aynı host'a açılması ve
bağlantı kopunca haber. İstek ve kullanıcının orada verdiği kararlar
`docs/YOL-HARITASI.md`'nin 037 satırında (desen listesi `settings.toml`'da,
sırayla ve ilk eşleşen kazanır; menüde anlam seçilir, renk temadan; addan
tahmin reddedildi). Tam uzak entegrasyon (uzakta blok, dock aynası) bu setin
dışında ve yol haritasında ayrı satır.

En keskin kusur Finder damlası: ssh sekmesinde bugünkü davranış sessizce
yanlış bir yol yazıyor. Geri kalan üçü eksik özellik — ama üçü de "uzakta
olduğunu hisset" isteğinin devamı: prod'da olduğunu renkten bilmek, aynı
makineye ikinci bir kabuk açmak, düşen bağlantıyı tek tuşla geri almak.

### Kanıt

**ssh'ın çıkış kodu 255'te kopmayı başarısız bağlantıdan ayırmıyor.**
Ölçüm (2026-09-26, bu makine, OpenSSH_10.2p1 / LibreSSL 3.3.6; kopma
senaryoları için `127.0.0.1:22222`'de kullanıcı kipinde koşan geçici bir
`sshd`, anahtarları ve yapılandırması scratchpad'de, kullanıcının `~/.ssh`'ına
dokunulmadı; `~.` `expect` ile gerçek bir tty üstünden):

| senaryo | çıkış |
|---|---|
| host çözülemedi (`nonexistent-host.invalid`) | 255 |
| bağlantı reddedildi (port 1, localhost:22) | 255 |
| bağlanma zaman aşımı (`10.255.255.1`) | 255 |
| kimlik reddi (`Permission denied (publickey)`, github.com ve yerel sshd) | 255 |
| oturum sürerken sunucu tarafı öldürüldü (`closed by remote host`) | 255 |
| keepalive zaman aşımı (`ServerAliveCountMax` doldu) | 255 |
| kullanıcı `~.` ile kopardı | 255 |
| uzak komut `exit 255` | 255 |
| uzak komut `exit 7` | 7 |
| olağan kapanış | 0 |

Yani 255 "ssh'ın kendisi başarısız oldu" demek — bağlanamadı, giremedi ya da
bağlantı düştü — ve üçünü ayıran tek şey ızgarada hemen üstte duran ssh'ın
kendi hata satırı. Uzak kabuğun 255 ile çıkması da aynı kodu veriyor (seyrek:
etkileşimli kabuk son komutunun koduyla çıkıyor). Karar 4 bunun üstüne kurulu.

**mosh ölçülmedi**: bu makinede kurulu değil (`which mosh mosh-client` boş).
mosh kopan bağlantıda çıkmıyor, kendi "Last contact" şeridiyle bekleyip yeniden
bağlanıyor — tasarımının kendisi bu; Karar 4 mosh'u bu yüzden ayrı tutuyor.

**Hızlı başarısızlık yoklamadan önce bitebiliyor.** 036'nın yoklaması `C`
kenarında, kararsızsa sonraki çıktı kenarında koşuyor (036 Karar 2). Host
çözülemediğinde ssh milisaniyeler içinde hata basıp çıkıyor; çıktının
tetiklediği yoklama ana kuyruğa vardığında ön planda ssh kalmamış olabilir ve
uzak durum hiç kurulmuyor. Uzak duruma bağlı her şey (Karar 4'ün teklifi)
hızlı başarısızlıklarda zamanlamaya bağlı — bilinen sınır olarak Karar 4'te.

**Sekmede renk AppKit'te verilebiliyor.** `objc2-app-kit` 0.3.2'de
`NSWindowTab` iki yol açıyor: `setAttributedTitle` (pencerenin başlığını
**ezer** — 036 Karar 5'in reddettiği ikinci başlık yazarı) ve
`setAccessoryView` (başlığın yanında ayrı bir view). Bayrak (`NSWindowTab`)
`bt-shell`'in özellik listesinde yok, yalnız `NSWindowTabGroup` var. Gerçek
pencerede görünüşü ölçülmedi.
