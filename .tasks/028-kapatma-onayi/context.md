# Kapatma onayı — Bağlam

## Mevcut Durum

Kapanışın hiçbir yolunda soru yok. Yollar 026'nın kurduğu hâliyle
(`CLAUDE.md` → Sekmeler, `.tasks/026-sekmeler/discussion.md` → Karar 5):

- **⌘W / Close Tab** menüden `performClose:` gönderiyor; **kırmızı düğme**
  de `performClose:`. İkisi de `windowWillClose:`'a
  (`crates/bt-shell/src/window.rs`) ve oradan `TerminalWindow::begin_close`'a
  varıyor: ritim durur, `SIGHUP` gider, beklenmez. `TerminalWindow`
  `windowShouldClose:`'u **uygulamıyor**, yani `performClose:` her zaman
  geçiyor.
- **⇧⌘W / Close Window** (`closeWindow:`) sekme grubunun her penceresine ayrı
  ayrı `performClose:` gönderiyor.
- **⌘Q** (`terminate:`) `applicationWillTerminate:` → `AppDelegate::shutdown`:
  bütün oturumlar paralel, tek son tarih. `applicationShouldTerminate:`
  uygulanmıyor. Doc'u bugün de "Cmd-Q açık programı sormadan kapatır: kapatma
  onayı yok" diyor (`app.rs` → `will_terminate`) ve 007 bunu kapsam dışı
  yazmıştı (`.tasks/007-ayarlar-ve-tema/discussion.md` → Kapsam dışı).
- **Kabuğun çıkışı** `ShellWake::child_exit` → `TerminalWindow::close` →
  `NSWindow::close`. `close` delegate'e **sormuyor** (`performClose:`'un
  aksine), yani `exit` yolu yapısal olarak sorusuz. Süreli koşuda aynı haber
  doğrudan `terminate:`'e gidiyor.

Ayar tarafında `confirm_close` diye bir anahtar yok. Referans ürünün
envanterinde var: `docs/ARASTIRMA.md` → ayar anahtarlarının tamamı
(`confirm_close`) ve Ürün özellikleri ("kapatma onayı
(never/running/always)").

**Süreç tarafı.** `bt-core` PTY'nin çocuğunu alacritty'den alıyor
(`tty::new` → `Pty`, `Pty::child()` bir `std::process::Child`) ama pid'i
dışarı vermiyor. Çocuk **kabuk değil**: süresiz oturumda komut
`child::login_command` (`/usr/bin/login -qflp …`), o çözülemezse
alacritty'nin macOS yolu — o da `login`. `login` oturum lideri olarak kalıyor
ve kabuğu **çatallıyor**; kabuk kendi süreç grubunu kuruyor ve iş denetimiyle
ön plan grubunu ona göre değiştiriyor. Ölçüldü (2026-09-23, bu makinede
`ps -axo pid,ppid,pgid,tpgid,comm`):

| tty | süreçler | TPGID |
|---|---|---|
| boşta | `login` 52582 (pgid 52582) → `-/bin/zsh` 52584 (pgid 52584) | 52584 = kabuk |
| Claude Code | `login` 39528 → `zsh` 39529 → `claude` 39944 (pgid 39944) | 39944 |
| sarmalayıcıyla | `login` 18920 → `zsh` 18921 → `bash` 19326 (pgid 19326) → `Orca` → `claude` 19380 (pgid 19326) | 19326 = `bash` |

Üç sonuç: (1) PTY'nin ön plan grubu **kabuğun kendi grubu değilse** kabuğun
dışında bir iş ön planda — sinyal budur; (2) karşılaştırılacak şey çocuğun
(login) pid'i değil kabuğun grubu, yani "kabuk hangisi" sorusu tasarımın
parçası; (3) ön plan grubunun **lideri** kullanıcının başlattığı program
olmayabilir (üçüncü satırda lider bir `bash` sarmalayıcısı, program onun
torunu `claude`). Aynı tabloda Metalterm'in (pid 97905) kabuğu `login`'siz
doğrudan `-zsh` olarak doğurduğu da görülüyor.

macOS'un süreç tablosu bu soruların hepsine `libc`'nin Apple yarısından
cevap veriyor (`proc_pidinfo` + `PROC_PIDTBSDINFO` → `proc_bsdinfo`'nun
`pbi_ppid`, `pbi_pgid`, `e_tpgid`, `pbi_name`; `proc_listchildpids`;
`proc_listpgrppids`) — `libc` `bt-shell`'de zaten bağımlılık. `e_tpgid`,
`ps`'in TPGID sütununun ta kendisi: ön plan grubu için master fd'ye
(`tcgetpgrp`) gerek yok. `alacritty_terminal` 0.26'da ön plan süreci
yardımcısı yok (kaynakta `foreground`/`tcgetpgrp` geçmiyor).

**Pencere tarafı.** `NSAlert`'in sayfa (sheet) kurucusu
`beginSheetModalForWindow:completionHandler:` `objc2-app-kit`'te `block2`
bayrağının arkasında; o bayrak ve `NSAlert` bayrağı bugün kapalı, `bt-shell`'in
`block2` kenarı da yok (`block2` grafta `bt-gpu` üzerinden var).
`runModal` bayraksız.

## Motivasyon

Kullanıcı isteği (2026-09-23): "terminali kapatmadaki kapama durumlarını
yapmamız lazım. mesela claude code açıkken çat diye kapanıyor. bu konudaki UX
senaryolarına uygun şekilde uygulayalım."

⌘W bir sekmeyi, ⌘Q bütün oturumları hiçbir şey sormadan kapatıyor; içinde
koşan Claude Code oturumu, `vim`'de kaydedilmemiş dosya, `ssh` bağlantısı ya
da uzun bir derleme geri gelmeyecek şekilde gidiyor. macOS terminallerinin üçü
de (Terminal.app, iTerm2, Ghostty) bu yolda soruyor ve kabuk boştayken
sormuyor; bilinen bedel 026'da adıyla kaydedilmişti ("⌘W'nin koşan komutu
sormadan kapatması bilinen bedel", Karar 5).
