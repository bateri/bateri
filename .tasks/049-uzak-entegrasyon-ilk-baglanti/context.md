# Uzak entegrasyon ilk bağlantıdan itibaren — Bağlam

## Mevcut Durum

048 düz `ssh`'ı uzakta entegre ediyor, ama **yalnız daha önce öğrenilmiş
sunucuda**:

- Yerel `bateri.zsh`'in `ssh` fonksiyonu `bateri ssh-argv`'ye soruyor;
  `ssh_wrap::decide` sarmayı yalnız durum dosyasında `posix` satırı olan host
  için veriyor (`crates/bt-shell-common/src/ssh_wrap.rs` → `decide`,
  `state.knows(Fact::Posix, &key)`).
- `posix` satırını uzak yardımcının selamı yazıyor (`ssh_wrap::learn`,
  `bt-shell-macos/src/pane.rs` → `remote_helper_for(learning)`): yardımcı
  sunucuda `sh` koşturabildiyse sunucu "kabuklu" sayılıyor. Kural 048
  `discussion.md` → Karar ("ilk bağlantıda öğren").
- Sarılmış yol satır içi: `ssh -t … exec sh -c '<base64 yük>'`, uzakta
  `boot.sh` dosyaları yazıp giriş kabuğunu `exec` ediyor
  (`assets/shell/remote/boot.sh`). `boot.sh` yalnız **hata** işareti basıyor
  (`8133;f;{write|decode|shell}`); başarının bir işareti yok.
- Sarılmış oturum bugün yalnız öğrenilmiş host'a gidiyor; sarılmamış bir
  bağlantının kabuksuz uçta kırılıp kırılmayacağını bilen bir şey yok —
  049'un seçtiği yol (A) bunu sarmanın kendisinden öğreniyor (`discussion.md`
  → Karar).
- (Taslak B için, reddedildi:) Elimizde iki kesin sinyal zaten var: kullanıcının girişi (termios kenarı,
  `jobs::remote_login` / `TtyModes::logged_in`, 047 phase-4) ve uzak satır
  düzenleyicinin satır okumaya başlaması (`CSI ? 2004 h`,
  `ScanEvent::PasteOn`, `ShellLog::paste_since_remote`). Kullanıcının tuşlarını
  bir satırın **arkasında** bekletmenin emsali de var: ⌘T'nin ilk girdisi
  (`SessionOptions::initial_input` + `held_input`, `session.rs` okuyucu
  döngüsü).

## Motivasyon

**Kullanıcı gördü (2026-10-02, gözle kontrol, Docker'da zsh + oh-my-zsh
sunucusu, `127.0.0.1:2223`):** ilk `ssh` düz açıldı — dizin, blok ve süre
yok; öğrenme ancak 047'nin Sign In… düğmesine basınca oldu (parolalı
sunucuda yardımcı başka türlü bağlanamıyor), entegrasyon ikinci bağlantıda
geldi. Süreç tablosu bunu doğruladı: ilk sekmede düz `ssh -p 2223 …` →
uzakta `-zsh`, `~/.local/share/bateri` ilk kez ⌘T'nin açtığı sarılmış
bağlantıda yazıldı. Kullanıcının sözü: "oğlum ux katliamı yapmışsın".

Kararın hatası: router/Windows gibi kabuksuz uçlardaki kırılmayı (~%1)
önlemek için **her** sunucunun ilk bağlantısını bozdu ve parolalı sunucuya
bir Sign In adımı ekledi — motivasyondaki arkadaşın senaryosunun ta kendisi
("Remote folder unknown").

Emsaller (2026-10-02 taraması): kitty uzak komutla önyüklüyor ve kabuksuz uçta
kırılıyor (Windows kullanıcıları host başına `delegate` istiyor, kitty
#6609); Ghostty uzakta betik koşturmuyor (yalnız terminfo/ortam); Warp
girişi sezgiyle algılayıp ("Last login:" ya da prompt'a benzeyen satır)
kurulum betiğini **açık oturumun içine** yazıyor; iTerm2'nin oturum içi
"conductor"u iki açık doğurdu (CVE-2025-22275 `/tmp/framer.txt`; uzak
çıktının protokolü taklit edip kod koşturması — "cat readme.txt is not
safe"). Ders: uzak çıktı yazmanın **ne zaman** olacağını tetikleyebilir,
**ne** yazılacağına karışamaz.
