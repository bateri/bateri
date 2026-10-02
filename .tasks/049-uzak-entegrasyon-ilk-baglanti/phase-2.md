# Phase 2 — Her zaman sar, sessiz düşme, 048'in öğrenmesini sök

## Özet

Bilinmeyen host'u da sar, kabuksuz uçta zsh fonksiyonunu düz yeniden
bağlantıya düşür, yardımcının selamından öğrenmeyi kaldır; uçtan uca sına ve
belgeleri güncelle. (Set kapısı phase-3'e taşındı: orkestratör 2026-10-03'te
`LC_TERMINAL` değişkenleri için bir phase-3 ekledi; bu phase'in kapısı riskli
phase `/code-review`'u.)

_Requirements: R1, R3, R3.1, R4, R5, R5.2, R5.3_

## Değişiklikler

- **`crates/bt-shell-common/src/ssh_wrap.rs`** — `decide`: `posix` şartı
  kalkar, `plain` kaydı olan host sarılmaz; `ssh -G`, uzak komut, tty ve
  ayar kuralları aynen. `learn` ya silinir ya da yalnız pane'in `up`
  kaydının gövdesi olarak kalır (adı buna göre).
- **`assets/shell/zsh/bateri.zsh`** — `__bateri_ssh`: sarılmış `command ssh`
  sonrası `rc=$?`; 255 değilse `ssh-fell-back --rc $rc …`; cevap NUL ile
  biten boş olmayan argv ise `command ssh` ile yeniden koş; fonksiyonun
  dönüş kodu en son koşan ssh'ınki. Sarılmamış yol değişmez.
- **`crates/bt-shell-common/src/remote_helper.rs`, `crates/bt-shell-macos/src/pane.rs`**
  — `RemoteHelper::with_greeted`/`Greeted` ve `remote_helper_for(learning)`
  sökülür; yardımcının kendi işi (047) değişmez.
- **`crates/bt-shell-macos`** (Shell menüsü, 048 phase-4) — host için
  entegrasyonu açmak o host'un `plain` kaydını siler.
- **Sınamalar** — Docker sshd (`#[ignore]`, 047/048 emsali, iş bitince
  silinir): (a) oh-my-zsh'li parolalı kullanıcı, temiz durum dosyasıyla
  **ilk** bağlantı → `up`, `posix`, uzak OSC 7 ve blok işareti; (b) giriş
  kabuğu komut kabul etmeyen bir kullanıcı (ör. `ForceCommand` ile ya da
  `-c`'yi reddeden bir giriş "kabuğu") → oturum `up`'sız biter, `plain`
  yazılır, fonksiyon düz yeniden koşturur; ikinci parolanın sorulup
  sorulmadığı ölçülür ve Uygulama Notları'na yazılır. 255 kolu: yanlış port
  → hiçbir kayıt yok.
- **`CLAUDE.md`** — 048'in "yalnız öğrenilmiş host sarılır / ilk bağlantıda
  öğren" cümleleri → "her zaman sar, kabuksuz uçta sessiz düşme, `posix`
  `up`'tan, `plain` kaydı"; işaretçi `.tasks/049-…/discussion.md` → Karar.
  `docs/AYARLAR.md`'nin `integration` satırı gerekiyorsa.

## Kabul

- Uçtan uca (a): temiz durumla ilk bağlantıda dock'ta uzak dizin ve komut
  blokları; Sign In gerekmiyor.
- Uçtan uca (b): kabuksuz uçta bağlantı kendiliğinden düz açılıyor, ikinci
  bağlantı doğrudan düz; ikinci parola ölçüldü ve yazıldı.
- 048'in mevcut sınamaları yeşil (sarma, `RemoteCommand`, `ControlMaster`,
  host başına kapama).
- Gözle kontrol (set sonu, kullanıcıda): durum dosyasında host yokken
  `ssh` → ilk prompt'ta dizin ve bloklar; `ps`'te düşme yolu yok.

## Uygulama Notları

- **Orkestratörün (b) kararı — `exit`'ten sonra yeniden bağlantı yapısal
  olarak kapalı.** Üç parça:
  - **Kanıt `ssh -G`'den önce, dosya olarak**: pane `up`'ı görür görmez
    (`check_remote_up`'ın ilk yarısı, `TerminalPane::mark_up`, kendi
    thread'inde) `remote-hosts.up/{nonce}` diye boş bir dosya yaratıyor
    (`ssh_wrap::mark_up`, `up_dir`); kilit yok, `ssh -G` yok, probun
    eşleşmesi de beklenmiyor — sahte bir `up` bilmediği bir nonce'u
    yazabilir, en kötü sonucu olmayan bir düşme (güvenli yön). Durum
    dosyasında satır yerine dizin seçildi: yaratma atomik, kilit beklemiyor,
    tüketim tek `remove_file` (`take_up`) ve satır tablosu kısa ömürlü
    kayıtla kirlenmiyor. Tüketilmeyen dosya (255 ile biten ssh, öldürülen
    kabuk) sonraki `mark_up`'ta `UP_KEEP`'ten (7 gün, tasarım sabiti,
    ölçülmedi) eskiyse süpürülüyor; ondan uzun süren oturumun sunucusu o
    zamana `posix`, yani kalkan iki kat.
  - **`ssh-fell-back` önce kendi nonce'unu soruyor**: alt komut artık
    **sarılmış** argv'yi alıyor (`-- "${wrapped[@]}"`, checklist'teki
    `-- "$@"` yerine): `unwrap` kullanıcının argv'sini, `nonce` denemenin
    nonce'unu aynı ayrıştırıcıyla veriyor, kabuk komutumuzu parçalamıyor.
    Sıra: biçim bozuk / argv bizim değil / nonce yok → hiçbir şey; 255 →
    hiçbir şey; nonce görüldü → hiçbir şey; sonra `fell_back`'in tablosu
    (`posix` → hiçbir şey); kalan bekleme (`FELL_BACK_PATIENCE`) nonce
    dosyası **ve** durum dosyası için, ikisinden biri gelirse hiçbir şey.
  - **Yerel çoklayıcıda sarma yok**: `$TMUX` ya da `$STY` doluysa zsh
    fonksiyonu doğrudan `command ssh "$@"` — ne `ssh-argv` ne düşme. Kapı
    `decide`'da değil fonksiyonda, çünkü ortam onun; `decide` çağıranın
    `--tty` bitiyle de aynı şekilde çalışıyor.
- **phase-1 `/code-review` bulgusu 1 kapandı**: tmux/screen içi yukarıdaki
  kapıyla, yavaş `ssh -G` (`Match exec`) kanıtın `ssh -G`'den önce
  yazılmasıyla. Kalan pencere yalnız pane'in ana kuyruk turu + dosya
  yaratma, 500 ms'nin içinde.
- **`decide`**: `posix` şartı kalktı, yalnız `plain` kayıtlı host sarılmıyor.
  Okunamayan durum dosyası artık "her şeyi sar" diyor ama `touched` yazılamadığı
  için `ssh-argv` yine bir şey basmıyor (düz `ssh`) — doc'a yazıldı.
- **zsh fonksiyonu**: sarılmış koşudan sonra `rc=$?`; 255 ise doğrudan döner;
  değilse `ssh-fell-back --rc $rc [--instance I] -- <sarılmış argv>`; NUL'la
  biten cevap düz `command ssh`'la yeniden koşuluyor ve dönüş kodu onunki,
  yoksa ilk ssh'ınki. Bekçi `child::tests::the_wrappers_ssh_function_asks_the_binary`
  (gerçek zsh: düşme sorusunun argv'si, düz yeniden koşu ve `rc=7`, 255'te
  soru yok, `TMUX=t`'de sarma yok).
- **Söküldü**: `RemoteHelper::with_greeted`/`Greeted`, worker'ın kancası ve
  sınaması, pane'in `remote_helper_for`'u. `ssh_wrap::learn` →
  `record_posix` (`posix`'in tek yazarı, pane'in kanıtı); `record`'un gövdesi
  `rewrite`'a çıktı, `HostState::forget` ve `ssh_wrap::forget_plain` geldi.
- **Menü (R4)**: Shell ▸ Shell Integration on “{host}” `plain` satırını
  odaktaki pane'in uzak argv'siyle (`ssh -G`, kendi thread'inde,
  `AppDelegate::forget_plain`) siliyor. Menünün işareti ayarın, `plain`'i
  göstermiyor (validate'te `ssh -G` koşturulamaz); bu yüzden **işaretli**
  öğeye tık önce soruyor: bir `plain` satırı silindiyse tıklamanın bütün
  etkisi o ve ayar açık kalıyor (sonraki `ssh` sarılır), satır yoksa ana
  kuyrukta entegrasyon kapatılıyor. İşaretsiz öğe hemen açıyor ve siliyor.
  Sorulacak bir şey yoksa (süreli koşu, uzak hedef yok, thread doğmadı)
  kapatma hemen.
- **Riskli phase `/code-review`** (medium, çalışma ağacı) iki bulgu, ikisi
  giderildi: (1) sinyalle biten ssh (`rc > 128`; ör. parola sorusunda
  Ctrl-C = 130) düşmeye gidip kabuklu host'u `plain` yapar ve iptal edilen
  bağlantıyı açardı → `ssh_wrap::says_nothing` (255 ya da `> 128`) hem
  `fell_back`'te hem alt komutun erken dönüşünde; ölçüldü: etkileşimli zsh
  çocuğun SIGINT'inde fonksiyonu zaten kesiyor, kural öteki sinyallerin (dışarıdan
  `kill`) kapısı. (2) yanlış `plain`'li host'ta işaretli öğeye ilk tık
  entegrasyonu kapatıyordu → yukarıdaki "önce sor". Raporun daha az kesin
  notu, **bilinen sınır**: girişte parola değiştirmeye zorlayan sunucu ne
  komutumuzu ne kabuğu koşturup 1 ile çıkıyor → bir kez düz yeniden
  bağlantı ve `plain` (Shell menüsüyle geri alınır).
- **Uçtan uca ölçüm** (`child::tests::e2e_first_connection_and_the_silent_fallback`,
  `#[ignore]`): gerçek sarmalayıcı, gerçek `target/debug/bateri`, Docker'da
  parolalı sshd (`bateri-sshd-omz` imajından ayrı bir container,
  `127.0.0.1:2249`; `kapi` kullanıcısının giriş kabuğu `-c`'de
  `'exec' is not recognized…` basıp 1 ile çıkan bir betik, `router`
  kullanıcısı `Match User router` + `ForceCommand` etkileşimli bir CLI;
  iş bitince silindi). Pane'in yarısını sınama thread'i oynuyor
  (`remote_up` → `mark_up` + `record_posix`); uygulamanın wake → ana kuyruk
  gecikmesi kapsamda değil, gözle kontrolün konusu. `HOME` geçici, her `ssh`
  `-F /dev/null`, ajan/anahtar yok, `known_hosts` geçici. Sonuçlar:
  - (a) oh-my-zsh, temiz durum dosyası: **ilk** bağlantıda uzak dizin
    (`/home/deneme`, OSC 7), uzak blok şeridi (`false` → hata rengi) ve
    `posix` satırı. `exit` → yeniden bağlantı **yok**, `plain` yok.
  - (a') `TMUX` dolu: sarılmadı, durum dosyası değişmedi, yeniden bağlantı yok.
  - (b) kabuksuz uç: hata satırı, ardından düz yeniden koşu **547 ms** sonra;
    **ikinci parola sorulmadı** (yeniden koşu sarılmış oturumun `u-<key>`
    master'ına bindi, `SESSION_PERSIST` 2 sn'nin içinde). `plain` yazıldı;
    ikinci bağlantı baştan düz (hata satırı yok).
  - (255) yanlış port: `plain` yok, yeniden koşu yok.
  - (d) yeni sunucunun parola sorusunda Ctrl-C (`rc=130`): yeniden koşu
    yok, `plain` yok.
  - (c) **bilinen sınır — `ForceCommand`'lı etkileşimli CLI**: zorlanan
    komut bizimkini yok sayıp çalışıyor, kullanıcı çalışıyor ve `exit`
    diyor; `up` hiç gelmediği için düşme koşuyor ve kullanıcı **bir kez**
    (parolasız, master'dan) yeniden bağlanıyor, host `plain` oluyor,
    sonrası düz. Ölçüldü (`reconnected after exit: true; plain row: true`).
    Yaygın kabuksuz uçlar (Windows cmd/PowerShell, komutu CLI komutu olarak
    koşup kapanan router'lar) bu kola girmiyor. Kapatmak yeni bir sinyal
    ister (giriş kenarından sonra kullanıcının yazması) — ürün kararı,
    orkestratöre devredildi.

## Checklist

- [x] `decide` bilinmeyen host'u sarıyor, `plain`'i sarmıyor
- [x] zsh fonksiyonunun düşme yolu (`ssh-fell-back --rc $rc [--instance I] -- <sarılmış argv>` — Uygulama Notları)
- [x] Selamdan öğrenme söküldü
- [x] `posix`'i pane'in görmediği sarılmış oturum düşmede `plain` olmamalı — çözüldü: tmux/screen'de sarma yok, nonce kanıtı `ssh -G`'den önce (Uygulama Notları)
- [x] Menünün "aç"ı `plain`'i siliyor (işaretli öğede önce sorarak)
- [x] Test: Docker sshd (a) ilk bağlantıda entegrasyon, (b) kabuksuzda düz yeniden bağlanma, 255 kolu
- [x] CLAUDE.md ve gerekiyorsa docs/AYARLAR.md
- [x] Doğrulama geçti (`make check`, `make linux`, `make bundle`, `make test-race`, `make smoke`)
- [ ] ~~Set kapısı~~ → phase-3'e taşındı; bu phase'de riskli phase `/code-review`
