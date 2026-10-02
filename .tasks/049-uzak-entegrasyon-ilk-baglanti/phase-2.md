# Phase 2 — Her zaman sar, sessiz düşme, 048'in öğrenmesini sök

## Özet

Bilinmeyen host'u da sar, kabuksuz uçta zsh fonksiyonunu düz yeniden
bağlantıya düşür, yardımcının selamından öğrenmeyi kaldır; uçtan uca sına ve
belgeleri güncelle. Son phase: set kapısı burada koşar.

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

## Checklist

- [ ] `decide` bilinmeyen host'u sarıyor, `plain`'i sarmıyor
- [ ] zsh fonksiyonunun düşme yolu
- [ ] Selamdan öğrenme söküldü
- [ ] Menünün "aç"ı `plain`'i siliyor
- [ ] Test: Docker sshd (a) ilk bağlantıda entegrasyon, (b) kabuksuzda düz yeniden bağlanma, 255 kolu
- [ ] CLAUDE.md ve gerekiyorsa docs/AYARLAR.md
- [ ] Doğrulama geçti (`make check`, `make linux`, `make bundle`, `make test-race`, `make smoke`)
- [ ] Set kapısı: `/code-review` (set aralığı) + `/audit`
